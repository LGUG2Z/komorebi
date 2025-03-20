use std::sync::atomic::Ordering;
use std::sync::Arc;
use std::time::Duration;
use std::time::Instant;

use color_eyre::eyre::anyhow;
use color_eyre::Result;
use crossbeam_utils::atomic::AtomicConsume;
use parking_lot::Mutex;

use crate::core::OperationDirection;
use crate::core::Rect;
use crate::core::Sizing;
use crate::core::WindowContainerBehaviour;

use crate::border_manager;
use crate::border_manager::BORDER_OFFSET;
use crate::border_manager::BORDER_WIDTH;
use crate::current_virtual_desktop;
use crate::notify_subscribers;
use crate::stackbar_manager;
use crate::transparency_manager;
use crate::window::should_act;
use crate::window::RuleDebug;
use crate::window_manager::WindowManager;
use crate::window_manager_event::WindowManagerEvent;
use crate::windows_api::WindowsApi;
use crate::winevent::WinEvent;
use crate::workspace::WorkspaceLayer;
use crate::workspace_reconciliator;
use crate::workspace_reconciliator::ALT_TAB_HWND;
use crate::workspace_reconciliator::ALT_TAB_HWND_INSTANT;
use crate::Notification;
use crate::NotificationEvent;
use crate::State;
use crate::FLOATING_APPLICATIONS;
use crate::HIDDEN_HWNDS;
use crate::REGEX_IDENTIFIERS;
use crate::TRAY_AND_MULTI_WINDOW_IDENTIFIERS;

#[tracing::instrument]
pub fn listen_for_events(wm: Arc<Mutex<WindowManager>>) {
    let receiver = wm.lock().incoming_events.clone();

    std::thread::spawn(move || {
        tracing::info!("listening");
        loop {
            if let Ok(event) = receiver.recv() {
                let mut guard = wm.lock();
                match guard.process_event(event) {
                    Ok(()) => {}
                    Err(error) => {
                        if cfg!(debug_assertions) {
                            tracing::error!("{:?}", error)
                        } else {
                            tracing::error!("{}", error)
                        }
                    }
                }
            }
        }
    });
}

impl WindowManager {
    #[allow(clippy::too_many_lines, clippy::cognitive_complexity)]
    #[tracing::instrument(skip(self, event), fields(event = event.title(), winevent = event.winevent(), hwnd = event.hwnd()))]
    pub fn process_event(&mut self, event: WindowManagerEvent) -> Result<()> {
        if self.is_paused {
            tracing::trace!("ignoring while paused");
            return Ok(());
        }

        let mut rule_debug = RuleDebug::default();

        let should_manage = event.window().should_manage(Some(event), &mut rule_debug)?;

        // All event handlers below this point should only be processed if the event is
        // related to a window that should be managed by the WindowManager.
        if !should_manage {
            let mut transparency_override = false;

            if transparency_manager::TRANSPARENCY_ENABLED.load_consume() {
                for m in self.monitors() {
                    for w in m.workspaces() {
                        let event_hwnd = event.window().hwnd;

                        let visible_hwnds = w
                            .visible_windows()
                            .iter()
                            .flatten()
                            .map(|w| w.hwnd)
                            .collect::<Vec<_>>();

                        let contains_managed_window = w.contains_managed_window(event_hwnd);

                        // this is for an old stackbar clicking fix
                        if contains_managed_window && !visible_hwnds.contains(&event_hwnd) {
                            transparency_override = true;
                        }

                        // but we always want to handle a minimize event when transparency overrides
                        // are applied
                        if !transparency_override
                            && contains_managed_window
                            && matches!(event, WindowManagerEvent::Minimize(_, _))
                        {
                            transparency_override = true;
                        }
                    }
                }
            }

            if !transparency_override {
                if rule_debug.matches_ignore_identifier.is_some() {
                    border_manager::send_notification(Option::from(event.hwnd()));
                }

                return Ok(());
            }
        }

        if let Some(virtual_desktop_id) = &self.virtual_desktop_id {
            if let Some(id) = current_virtual_desktop() {
                if id != *virtual_desktop_id {
                    tracing::info!(
                        "ignoring events and commands while not on virtual desktop {:?}",
                        virtual_desktop_id
                    );
                    return Ok(());
                }
            }
        }

        #[allow(clippy::useless_asref)]
        // We don't have From implemented for &mut WindowManager
        let initial_state = State::from(self.as_ref());

        // Make sure we have the most recently focused monitor from any event
        match event {
            WindowManagerEvent::FocusChange(_, window)
            | WindowManagerEvent::Show(_, window)
            | WindowManagerEvent::MoveResizeEnd(_, window) => {
                if let Some(monitor_idx) = self.monitor_idx_from_window(window) {
                    // This is a hidden window apparently associated with COM support mechanisms (based
                    // on a post from http://www.databaseteam.org/1-ms-sql-server/a5bb344836fb889c.htm)
                    //
                    // The hidden window, OLEChannelWnd, associated with this class (spawned by
                    // explorer.exe), after some debugging, is observed to always be tied to the primary
                    // display monitor, or (usually) monitor 0 in the WindowManager state.
                    //
                    // Due to this, at least one user in the Discord has witnessed behaviour where, when
                    // a MonitorPoll event is triggered by OLEChannelWnd, the focused monitor index gets
                    // set repeatedly to 0, regardless of where the current foreground window is actually
                    // located.
                    //
                    // This check ensures that we only update the focused monitor when the window
                    // triggering monitor reconciliation is known to not be tied to a specific monitor.
                    if let Ok(class) = window.class() {
                        if class != "OleMainThreadWndClass"
                            && self.focused_monitor_idx() != monitor_idx
                        {
                            self.focus_monitor(monitor_idx)?;
                        }
                    }
                }
            }
            _ => {}
        }

        self.enforce_workspace_rules()?;

        if matches!(event, WindowManagerEvent::MouseCapture(..)) {
            tracing::trace!(
                "only reaping orphans and enforcing workspace rules for mouse capture event"
            );
            return Ok(());
        }

        match event {
            WindowManagerEvent::Raise(window) => {
                window.focus(false)?;
                self.has_pending_raise_op = false;
            }
            WindowManagerEvent::Destroy(_, window) | WindowManagerEvent::Unmanage(window) => {
                if self.focused_workspace()?.contains_window(window.hwnd) {
                    self.focused_workspace_mut()?.remove_window(window.hwnd)?;
                    self.update_focused_workspace(false, false)?;

                    let mut already_moved_window_handles = self.already_moved_window_handles.lock();

                    already_moved_window_handles.remove(&window.hwnd);
                }
            }
            WindowManagerEvent::Minimize(_, window) => {
                let mut hide = false;

                {
                    let programmatically_hidden_hwnds = HIDDEN_HWNDS.lock();
                    if !programmatically_hidden_hwnds.contains(&window.hwnd) {
                        hide = true;
                    }
                }

                if hide {
                    self.focused_workspace_mut()?.remove_window(window.hwnd)?;
                    self.update_focused_workspace(false, false)?;
                }
            }
            WindowManagerEvent::Hide(_, window) => {
                let mut hide = false;
                // Some major applications unfortunately send the HIDE signal when they are being
                // minimized or destroyed. Applications that close to the tray also do the same,
                // and will have is_window() return true, as the process is still running even if
                // the window is not visible.
                {
                    let tray_and_multi_window_identifiers =
                        TRAY_AND_MULTI_WINDOW_IDENTIFIERS.lock();
                    let regex_identifiers = REGEX_IDENTIFIERS.lock();

                    let title = &window.title()?;
                    let exe_name = &window.exe()?;
                    let class = &window.class()?;
                    let path = &window.path()?;

                    // We don't want to purge windows that have been deliberately hidden by us, eg. when
                    // they are not on the top of a container stack.
                    let programmatically_hidden_hwnds = HIDDEN_HWNDS.lock();
                    let should_act = should_act(
                        title,
                        exe_name,
                        class,
                        path,
                        &tray_and_multi_window_identifiers,
                        &regex_identifiers,
                    )
                    .is_some();

                    if !window.is_window()
                        || (should_act && !programmatically_hidden_hwnds.contains(&window.hwnd))
                    {
                        hide = true;
                    }
                }

                if hide {
                    self.focused_workspace_mut()?.remove_window(window.hwnd)?;
                    self.update_focused_workspace(false, false)?;
                }

                let mut already_moved_window_handles = self.already_moved_window_handles.lock();

                already_moved_window_handles.remove(&window.hwnd);
            }
            WindowManagerEvent::FocusChange(_, window) => {
                // don't want to trigger the full workspace updates when there are no managed
                // containers - this makes floating windows on empty workspaces go into very
                // annoying focus change loops which prevents users from interacting with them
                if !self.focused_workspace()?.containers().is_empty() {
                    self.update_focused_workspace(self.mouse_follows_focus, false)?;
                }

                let workspace = self.focused_workspace_mut()?;
                let floating_window_idx = workspace
                    .floating_windows()
                    .iter()
                    .position(|w| w.hwnd == window.hwnd);

                match floating_window_idx {
                    None => {
                        if let Some(w) = workspace.maximized_window() {
                            if w.hwnd == window.hwnd {
                                return Ok(());
                            }
                        }

                        if let Some(monocle) = workspace.monocle_container() {
                            if let Some(window) = monocle.focused_window() {
                                window.focus(false)?;
                            }
                        } else {
                            workspace.focus_container_by_window(window.hwnd)?;
                        }

                        workspace.set_layer(WorkspaceLayer::Tiling);
                    }
                    Some(idx) => {
                        if let Some(_window) = workspace.floating_windows().get(idx) {
                            workspace.set_layer(WorkspaceLayer::Floating);
                        }
                    }
                }
            }
            WindowManagerEvent::Show(_, window)
            | WindowManagerEvent::Manage(window)
            | WindowManagerEvent::Uncloak(_, window) => {
                if matches!(event, WindowManagerEvent::Uncloak(_, _))
                    && self.uncloack_to_ignore >= 1
                {
                    tracing::info!("ignoring uncloak after monocle move by mouse across monitors");
                    self.uncloack_to_ignore = self.uncloack_to_ignore.saturating_sub(1);
                } else {
                    let focused_monitor_idx = self.focused_monitor_idx();
                    let focused_workspace_idx =
                        self.focused_workspace_idx_for_monitor_idx(focused_monitor_idx)?;

                    let focused_pair = (focused_monitor_idx, focused_workspace_idx);

                    let mut needs_reconciliation = false;

                    if let Some((m_idx, w_idx)) = self.known_hwnds.get(&window.hwnd) {
                        if focused_pair != (*m_idx, *w_idx) {
                            // At this point we know we are going to send a notification to the workspace reconciliator
                            // So we get the topmost window returned by EnumWindows, which is almost always the window
                            // that has been selected by alt-tab
                            if let Ok(alt_tab_windows) = WindowsApi::alt_tab_windows() {
                                if let Some(first) =
                                    alt_tab_windows.iter().find(|w| w.title().is_ok())
                                {
                                    // If our record of this HWND hasn't been updated in over a minute
                                    let mut instant = ALT_TAB_HWND_INSTANT.lock();
                                    if instant.elapsed().gt(&Duration::from_secs(1)) {
                                        // Update our record with the HWND we just found
                                        ALT_TAB_HWND.store(Some(first.hwnd));
                                        // Update the timestamp of our record
                                        *instant = Instant::now();
                                    }
                                }
                            }

                            workspace_reconciliator::send_notification(*m_idx, *w_idx);
                            needs_reconciliation = true;
                        }
                    }

                    // There are some applications such as Firefox where, if they are focused when a
                    // workspace switch takes place, it will fire an additional Show event, which will
                    // result in them being associated with both the original workspace and the workspace
                    // being switched to. This loop is to try to ensure that we don't end up with
                    // duplicates across multiple workspaces, as it results in ghost layout tiles.
                    let mut proceed = true;

                    if let Some((m_idx, w_idx)) = self.known_hwnds.get(&window.hwnd) {
                        if let Some(focused_workspace_idx) = self
                            .monitors()
                            .get(*m_idx)
                            .map(|m| m.focused_workspace_idx())
                        {
                            if *m_idx != self.focused_monitor_idx()
                                && *w_idx != focused_workspace_idx
                            {
                                tracing::debug!(
                                    "ignoring show event for window already associated with another workspace"
                                );

                                window.hide();
                                proceed = false;
                            }
                        }
                    }

                    if proceed {
                        let mut behaviour = self.window_management_behaviour(
                            focused_monitor_idx,
                            focused_workspace_idx,
                        );
                        let workspace = self.focused_workspace_mut()?;
                        let workspace_contains_window = workspace.contains_window(window.hwnd);
                        let monocle_container = workspace.monocle_container().clone();

                        if !workspace_contains_window && !needs_reconciliation {
                            let floating_applications = FLOATING_APPLICATIONS.lock();
                            let mut should_float = false;

                            if !floating_applications.is_empty() {
                                let regex_identifiers = REGEX_IDENTIFIERS.lock();

                                if let (Ok(title), Ok(exe_name), Ok(class), Ok(path)) =
                                    (window.title(), window.exe(), window.class(), window.path())
                                {
                                    should_float = should_act(
                                        &title,
                                        &exe_name,
                                        &class,
                                        &path,
                                        &floating_applications,
                                        &regex_identifiers,
                                    )
                                    .is_some();
                                }
                            }

                            behaviour.float_override = behaviour.float_override
                                || (should_float
                                    && !matches!(event, WindowManagerEvent::Manage(_)));

                            if behaviour.float_override {
                                // Center floating windows if we are already on the `Floating`
                                // layer and the window doesn't match a `floating_windows` rule and
                                // the workspace is not a floating workspace
                                let center_spawned_floats =
                                    matches!(workspace.layer, WorkspaceLayer::Floating)
                                        && !should_float
                                        && workspace.tile;
                                workspace.floating_windows_mut().push(window);
                                workspace.set_layer(WorkspaceLayer::Floating);
                                if center_spawned_floats {
                                    let mut floating_window = window;
                                    floating_window.center(&workspace.globals().work_area)?;
                                }
                                self.update_focused_workspace(false, false)?;
                            } else {
                                match behaviour.current_behaviour {
                                    WindowContainerBehaviour::Create => {
                                        workspace.new_container_for_window(window);
                                        workspace.set_layer(WorkspaceLayer::Tiling);
                                        self.update_focused_workspace(false, false)?;
                                    }
                                    WindowContainerBehaviour::Append => {
                                        workspace
                                            .focused_container_mut()
                                            .ok_or_else(|| {
                                                anyhow!("there is no focused container")
                                            })?
                                            .add_window(window);
                                        workspace.set_layer(WorkspaceLayer::Tiling);
                                        self.update_focused_workspace(true, false)?;
                                        stackbar_manager::send_notification();
                                    }
                                }
                            }

                            if (self.focused_workspace()?.containers().len() == 1
                                && self.focused_workspace()?.floating_windows().is_empty())
                                || (self.focused_workspace()?.containers().is_empty()
                                    && self.focused_workspace()?.floating_windows().len() == 1)
                            {
                                // If after adding this window the workspace only contains 1 window, it
                                // means it was previously empty and we focused the desktop to unfocus
                                // any previous window from other workspace, so now we need to focus
                                // this window again. This is needed because sometimes some windows
                                // first send the `FocusChange` event and only the `Show` event after
                                // and we will be focusing the desktop on the `FocusChange` event since
                                // it is still empty.
                                window.focus(self.mouse_follows_focus)?;
                            }
                        }

                        if workspace_contains_window {
                            let mut monocle_window_event = false;
                            if let Some(ref monocle) = monocle_container {
                                if let Some(monocle_window) = monocle.focused_window() {
                                    if monocle_window.hwnd == window.hwnd {
                                        monocle_window_event = true;
                                    }
                                }
                            }

                            if !monocle_window_event && monocle_container.is_some() {
                                window.hide();
                            }
                        }
                    }
                }
            }
            WindowManagerEvent::MoveResizeStart(_, window) => {
                let monitor_idx = self.focused_monitor_idx();
                let workspace_idx = self
                    .focused_monitor()
                    .ok_or_else(|| anyhow!("there is no monitor with this idx"))?
                    .focused_workspace_idx();

                WindowsApi::bring_window_to_top(window.hwnd)?;

                let pending_move_op = Arc::make_mut(&mut self.pending_move_op);
                *pending_move_op = Option::from((monitor_idx, workspace_idx, window.hwnd));
            }
            WindowManagerEvent::MoveResizeEnd(_, window) => {
                // We need this because if the event ends on a different monitor,
                // that monitor will already have been focused and updated in the state
                let pending = *self.pending_move_op;
                // Always consume the pending move op whenever this event is handled
                let pending_move_op = Arc::make_mut(&mut self.pending_move_op);
                *pending_move_op = None;

                // If the window handles don't match then something went wrong and the pending move
                // is not related to this current move, if so abort this operation.
                if let Some((_, _, w_hwnd)) = pending {
                    if w_hwnd != window.hwnd {
                        color_eyre::eyre::bail!(
                            "window handles for move operation don't match: {} != {}",
                            w_hwnd,
                            window.hwnd
                        );
                    }
                }

                let target_monitor_idx = self
                    .monitor_idx_from_current_pos()
                    .ok_or_else(|| anyhow!("cannot get monitor idx from current position"))?;

                let focused_monitor_idx = self.focused_monitor_idx();
                let focused_workspace_idx = self.focused_workspace_idx().unwrap_or_default();
                let window_management_behaviour =
                    self.window_management_behaviour(focused_monitor_idx, focused_workspace_idx);

                let workspace = self.focused_workspace_mut()?;
                let focused_container_idx = workspace.focused_container_idx();
                let new_position = WindowsApi::window_rect(window.hwnd)?;
                let old_position = *workspace
                    .latest_layout()
                    .get(focused_container_idx)
                    // If the move was to another monitor with an empty workspace, the
                    // workspace here will refer to that empty workspace, which won't
                    // have any latest layout set. We fall back to a Default for Rect
                    // which allows us to make a reasonable guess that the drag has taken
                    // place across a monitor boundary to an empty workspace
                    .unwrap_or(&Rect::default());

                // This will be true if we have moved to another monitor
                let mut moved_across_monitors = false;

                if let Some((m_idx, _)) = self.known_hwnds.get(&window.hwnd) {
                    if *m_idx != target_monitor_idx {
                        moved_across_monitors = true;
                    }
                }

                if let Some((origin_monitor_idx, origin_workspace_idx, _)) = pending {
                    // If we didn't move to another monitor with an empty workspace, it is
                    // still possible that we moved to another monitor with a populated workspace
                    if !moved_across_monitors {
                        // So we'll check if the origin monitor index and the target monitor index
                        // are different, if they are, we can set the override
                        moved_across_monitors = origin_monitor_idx != target_monitor_idx;

                        if moved_across_monitors {
                            // Want to make sure that we exclude unmanaged windows from cross-monitor
                            // moves with a mouse, otherwise the currently focused idx container will
                            // be moved when we just want to drag an unmanaged window
                            let origin_workspace = self
                                .monitors()
                                .get(origin_monitor_idx)
                                .ok_or_else(|| anyhow!("cannot get monitor idx"))?
                                .workspaces()
                                .get(origin_workspace_idx)
                                .ok_or_else(|| anyhow!("cannot get workspace idx"))?;

                            let managed_window = origin_workspace.contains_window(window.hwnd);

                            if !managed_window {
                                moved_across_monitors = false;
                            }
                        }
                    }
                }

                let workspace = self.focused_workspace_mut()?;
                if (*workspace.tile() && workspace.contains_managed_window(window.hwnd))
                    || moved_across_monitors
                {
                    let resize = Rect {
                        left: new_position.left - old_position.left,
                        top: new_position.top - old_position.top,
                        right: new_position.right - old_position.right,
                        bottom: new_position.bottom - old_position.bottom,
                    };

                    // If we have moved across the monitors, use that override, otherwise determine
                    // if a move has taken place by ruling out a resize
                    let right_bottom_constant = 0;

                    let is_move = moved_across_monitors
                        || resize.right.abs() == right_bottom_constant
                            && resize.bottom.abs() == right_bottom_constant;

                    if is_move {
                        tracing::info!("moving with mouse");

                        if moved_across_monitors {
                            if let Some((origin_monitor_idx, origin_workspace_idx, w_hwnd)) =
                                pending
                            {
                                let target_workspace_idx = self
                                    .monitors()
                                    .get(target_monitor_idx)
                                    .ok_or_else(|| anyhow!("there is no monitor at this idx"))?
                                    .focused_workspace_idx();

                                let target_container_idx = self
                                    .monitors()
                                    .get(target_monitor_idx)
                                    .ok_or_else(|| anyhow!("there is no monitor at this idx"))?
                                    .focused_workspace()
                                    .ok_or_else(|| {
                                        anyhow!("there is no focused workspace for this monitor")
                                    })?
                                    .container_idx_from_current_point()
                                    // Default to 0 in the case of an empty workspace
                                    .unwrap_or(0);

                                let origin = (origin_monitor_idx, origin_workspace_idx, w_hwnd);
                                let target = (
                                    target_monitor_idx,
                                    target_workspace_idx,
                                    target_container_idx,
                                );
                                self.transfer_window(origin, target)?;

                                // We want to make sure both the origin and target monitors are updated,
                                // so that we don't have ghost tiles until we force an interaction on
                                // the origin monitor's focused workspace
                                self.focus_monitor(origin_monitor_idx)?;
                                let origin_monitor = self
                                    .monitors_mut()
                                    .get_mut(origin_monitor_idx)
                                    .ok_or_else(|| anyhow!("there is no monitor at this idx"))?;
                                origin_monitor.focus_workspace(origin_workspace_idx)?;
                                self.update_focused_workspace(false, false)?;

                                self.focus_monitor(target_monitor_idx)?;
                                let target_monitor = self
                                    .monitors_mut()
                                    .get_mut(target_monitor_idx)
                                    .ok_or_else(|| anyhow!("there is no monitor at this idx"))?;
                                target_monitor.focus_workspace(target_workspace_idx)?;
                                self.update_focused_workspace(false, false)?;

                                // Make sure to give focus to the moved window again
                                window.focus(self.mouse_follows_focus)?;
                            }
                        } else if window_management_behaviour.float_override {
                            workspace.floating_windows_mut().push(window);
                            self.update_focused_workspace(false, false)?;
                        } else {
                            match window_management_behaviour.current_behaviour {
                                WindowContainerBehaviour::Create => {
                                    match workspace.container_idx_from_current_point() {
                                        Some(target_idx) => {
                                            workspace
                                                .swap_containers(focused_container_idx, target_idx);
                                            self.update_focused_workspace(false, false)?;
                                        }
                                        None => {
                                            self.update_focused_workspace(
                                                self.mouse_follows_focus,
                                                false,
                                            )?;
                                        }
                                    }
                                }
                                WindowContainerBehaviour::Append => {
                                    match workspace.container_idx_from_current_point() {
                                        Some(target_idx) => {
                                            workspace.move_window_to_container(target_idx)?;
                                            self.update_focused_workspace(false, false)?;
                                        }
                                        None => {
                                            self.update_focused_workspace(
                                                self.mouse_follows_focus,
                                                false,
                                            )?;
                                        }
                                    }

                                    stackbar_manager::send_notification();
                                }
                            }
                        }
                    } else {
                        tracing::info!("resizing with mouse");
                        let mut ops = vec![];

                        macro_rules! resize_op {
                            ($coordinate:expr, $comparator:tt, $direction:expr) => {{
                                let adjusted = $coordinate * 2;
                                let sizing = if adjusted $comparator 0 {
                                    Sizing::Decrease
                                } else {
                                    Sizing::Increase
                                };

                                ($direction, sizing, adjusted.abs())
                            }};
                        }

                        if resize.left != 0 {
                            ops.push(resize_op!(resize.left, >, OperationDirection::Left));
                        }

                        if resize.top != 0 {
                            ops.push(resize_op!(resize.top, >, OperationDirection::Up));
                        }

                        // TODO: Determine if this is still needed
                        let top_left_constant = BORDER_WIDTH.load(Ordering::SeqCst)
                            + BORDER_OFFSET.load(Ordering::SeqCst);

                        if resize.right != 0
                            && (resize.left == top_left_constant || resize.left == 0)
                        {
                            ops.push(resize_op!(resize.right, <, OperationDirection::Right));
                        }

                        if resize.bottom != 0
                            && (resize.top == top_left_constant || resize.top == 0)
                        {
                            ops.push(resize_op!(resize.bottom, <, OperationDirection::Down));
                        }

                        for (edge, sizing, delta) in ops {
                            self.resize_window(edge, sizing, delta, true)?;
                        }

                        self.update_focused_workspace(false, false)?;
                    }
                }
            }
            WindowManagerEvent::MouseCapture(..)
            | WindowManagerEvent::Cloak(..)
            | WindowManagerEvent::TitleUpdate(..) => {}
        };

        // If we unmanaged a window, it shouldn't be immediately hidden behind managed windows
        if let WindowManagerEvent::Unmanage(mut window) = event {
            window.center(&self.focused_monitor_work_area()?)?;
        }

        // Update list of known_hwnds and their monitor/workspace index pair
        self.update_known_hwnds();

        notify_subscribers(
            Notification {
                event: NotificationEvent::WindowManager(event),
                state: self.as_ref().into(),
            },
            initial_state.has_been_modified(self.as_ref()),
        )?;

        border_manager::send_notification(Some(event.hwnd()));
        transparency_manager::send_notification();
        stackbar_manager::send_notification();

        // Too many spammy OBJECT_NAMECHANGE events from JetBrains IDEs
        if !matches!(
            event,
            WindowManagerEvent::Show(WinEvent::ObjectNameChange, _)
        ) {
            tracing::info!("processed: {}", event.window().to_string());
        } else {
            tracing::trace!("processed: {}", event.window().to_string());
        }

        Ok(())
    }
}
