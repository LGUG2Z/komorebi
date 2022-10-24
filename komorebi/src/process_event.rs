use std::fs::OpenOptions;
use std::sync::atomic::Ordering;
use std::sync::Arc;

use color_eyre::eyre::anyhow;
use color_eyre::Result;
use crossbeam_channel::select;
use parking_lot::Mutex;

use komorebi_core::OperationDirection;
use komorebi_core::Rect;
use komorebi_core::Sizing;
use komorebi_core::WindowContainerBehaviour;

use crate::border::Border;
use crate::current_virtual_desktop;
use crate::notify_subscribers;
use crate::window_manager::WindowManager;
use crate::window_manager_event::WindowManagerEvent;
use crate::windows_api::WindowsApi;
use crate::Notification;
use crate::NotificationEvent;
use crate::BORDER_COLOUR_CURRENT;
use crate::BORDER_COLOUR_SINGLE;
use crate::BORDER_COLOUR_STACK;
use crate::BORDER_ENABLED;
use crate::BORDER_HIDDEN;
use crate::BORDER_HWND;
use crate::DATA_DIR;
use crate::HIDDEN_HWNDS;
use crate::TRAY_AND_MULTI_WINDOW_IDENTIFIERS;

#[tracing::instrument]
pub fn listen_for_events(wm: Arc<Mutex<WindowManager>>) {
    let receiver = wm.lock().incoming_events.lock().clone();

    std::thread::spawn(move || {
        tracing::info!("listening");
        loop {
            select! {
                recv(receiver) -> mut maybe_event => {
                    if let Ok(event) = maybe_event.as_mut() {
                        match wm.lock().process_event(event) {
                            Ok(()) => {},
                            Err(error) => tracing::error!("{}", error)
                        }
                    }
                }
            }
        }
    });
}

impl WindowManager {
    #[allow(clippy::too_many_lines, clippy::cognitive_complexity)]
    #[tracing::instrument(skip(self))]
    pub fn process_event(&mut self, event: &mut WindowManagerEvent) -> Result<()> {
        if self.is_paused {
            tracing::trace!("ignoring while paused");
            return Ok(());
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

        // Make sure we have the most recently focused monitor from any event
        match event {
            WindowManagerEvent::FocusChange(_, window)
            | WindowManagerEvent::Show(_, window)
            | WindowManagerEvent::DisplayChange(window)
            | WindowManagerEvent::MoveResizeEnd(_, window) => {
                self.reconcile_monitors()?;

                let monitor_idx = self.monitor_idx_from_window(*window)
                    .ok_or_else(|| anyhow!("there is no monitor associated with this window, it may have already been destroyed"))?;

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
                if window.class()? != "OleMainThreadWndClass" {
                    self.focus_monitor(monitor_idx)?;
                }
            }
            _ => {}
        }

        let invisible_borders = self.invisible_borders;
        let offset = self.work_area_offset;

        for (i, monitor) in self.monitors_mut().iter_mut().enumerate() {
            let work_area = *monitor.work_area_size();
            for (j, workspace) in monitor.workspaces_mut().iter_mut().enumerate() {
                let reaped_orphans = workspace.reap_orphans()?;
                if reaped_orphans.0 > 0 || reaped_orphans.1 > 0 {
                    workspace.update(&work_area, offset, &invisible_borders)?;
                    tracing::info!(
                        "reaped {} orphan window(s) and {} orphaned container(s) on monitor: {}, workspace: {}",
                        reaped_orphans.0,
                        reaped_orphans.1,
                        i,
                        j
                    );
                }
            }
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
                window.raise();
                self.has_pending_raise_op = false;
            }
            WindowManagerEvent::Destroy(_, window) | WindowManagerEvent::Unmanage(window) => {
                self.focused_workspace_mut()?.remove_window(window.hwnd)?;
                self.update_focused_workspace(false)?;
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
                    self.update_focused_workspace(false)?;
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

                    // We don't want to purge windows that have been deliberately hidden by us, eg. when
                    // they are not on the top of a container stack.
                    let programmatically_hidden_hwnds = HIDDEN_HWNDS.lock();

                    if ((!window.is_window()
                        || tray_and_multi_window_identifiers.contains(&window.exe()?))
                        || tray_and_multi_window_identifiers.contains(&window.class()?))
                        && !programmatically_hidden_hwnds.contains(&window.hwnd)
                    {
                        hide = true;
                    }
                }

                if hide {
                    self.focused_workspace_mut()?.remove_window(window.hwnd)?;
                    self.update_focused_workspace(false)?;
                }
            }
            WindowManagerEvent::FocusChange(_, window) => {
                let workspace = self.focused_workspace_mut()?;
                if !workspace
                    .floating_windows()
                    .iter()
                    .any(|w| w.hwnd == window.hwnd)
                {
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
                        self.focused_workspace_mut()?
                            .focus_container_by_window(window.hwnd)?;
                    }
                }
            }
            WindowManagerEvent::Show(_, window) | WindowManagerEvent::Manage(window) => {
                let mut switch_to = None;
                for (i, monitors) in self.monitors().iter().enumerate() {
                    for (j, workspace) in monitors.workspaces().iter().enumerate() {
                        if workspace.contains_window(window.hwnd) {
                            switch_to = Some((i, j));
                        }
                    }
                }

                if let Some((known_monitor_idx, known_workspace_idx)) = switch_to {
                    if self.focused_monitor_idx() != known_monitor_idx
                        || self
                            .focused_monitor()
                            .ok_or_else(|| anyhow!("there is no monitor"))?
                            .focused_workspace_idx()
                            != known_workspace_idx
                    {
                        self.focus_monitor(known_monitor_idx)?;
                        self.focus_workspace(known_workspace_idx)?;
                        return Ok(());
                    }
                }

                // There are some applications such as Firefox where, if they are focused when a
                // workspace switch takes place, it will fire an additional Show event, which will
                // result in them being associated with both the original workspace and the workspace
                // being switched to. This loop is to try to ensure that we don't end up with
                // duplicates across multiple workspaces, as it results in ghost layout tiles.
                for (i, monitor) in self.monitors().iter().enumerate() {
                    for (j, workspace) in monitor.workspaces().iter().enumerate() {
                        if workspace.container_for_window(window.hwnd).is_some()
                            && i != self.focused_monitor_idx()
                            && j != monitor.focused_workspace_idx()
                        {
                            tracing::debug!(
                                "ignoring show event for window already associated with another workspace"
                            );

                            window.hide();
                            return Ok(());
                        }
                    }
                }

                let behaviour = self.window_container_behaviour;
                let workspace = self.focused_workspace_mut()?;

                if !workspace.contains_window(window.hwnd) {
                    match behaviour {
                        WindowContainerBehaviour::Create => {
                            workspace.new_container_for_window(*window);
                            self.update_focused_workspace(false)?;
                        }
                        WindowContainerBehaviour::Append => {
                            workspace
                                .focused_container_mut()
                                .ok_or_else(|| anyhow!("there is no focused container"))?
                                .add_window(*window);
                            self.update_focused_workspace(true)?;
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
                let container_idx = self
                    .focused_monitor()
                    .ok_or_else(|| anyhow!("there is no monitor with this idx"))?
                    .focused_workspace()
                    .ok_or_else(|| anyhow!("there is no workspace with this idx"))?
                    .focused_container_idx();

                WindowsApi::bring_window_to_top(window.hwnd())?;

                self.pending_move_op = Option::from((monitor_idx, workspace_idx, container_idx));
            }
            WindowManagerEvent::MoveResizeEnd(_, window) => {
                // We need this because if the event ends on a different monitor,
                // that monitor will already have been focused and updated in the state
                let pending = self.pending_move_op;
                // Always consume the pending move op whenever this event is handled
                self.pending_move_op = None;

                let target_monitor_idx = self
                    .monitor_idx_from_current_pos()
                    .ok_or_else(|| anyhow!("cannot get monitor idx from current position"))?;

                let new_window_behaviour = self.window_container_behaviour;
                let invisible_borders = self.invisible_borders;

                let workspace = self.focused_workspace_mut()?;
                if !workspace
                    .floating_windows()
                    .iter()
                    .any(|w| w.hwnd == window.hwnd)
                {
                    let focused_container_idx = workspace.focused_container_idx();

                    let mut new_position = WindowsApi::window_rect(window.hwnd())?;

                    let old_position = *workspace
                        .latest_layout()
                        .get(focused_container_idx)
                        // If the move was to another monitor with an empty workspace, the
                        // workspace here will refer to that empty workspace, which won't
                        // have any latest layout set. We fall back to a Default for Rect
                        // which allows us to make a reasonable guess that the drag has taken
                        // place across a monitor boundary to an empty workspace
                        .unwrap_or(&Rect::default());

                    // This will be true if we have moved to an empty workspace on another monitor
                    let mut moved_across_monitors = old_position == Rect::default();

                    if let Some((origin_monitor_idx, _, _)) = pending {
                        // If we didn't move to another monitor with an empty workspace, it is
                        // still possible that we moved to another monitor with a populated workspace
                        if !moved_across_monitors {
                            // So we'll check if the origin monitor index and the target monitor index
                            // are different, if they are, we can set the override
                            moved_across_monitors = origin_monitor_idx != target_monitor_idx;
                        }
                    }

                    // Adjust for the invisible borders
                    new_position.left += invisible_borders.left;
                    new_position.top += invisible_borders.top;
                    new_position.right -= invisible_borders.right;
                    new_position.bottom -= invisible_borders.bottom;

                    let resize = Rect {
                        left: new_position.left - old_position.left,
                        top: new_position.top - old_position.top,
                        right: new_position.right - old_position.right,
                        bottom: new_position.bottom - old_position.bottom,
                    };

                    // If we have moved across the monitors, use that override, otherwise determine
                    // if a move has taken place by ruling out a resize
                    let is_move = moved_across_monitors
                        || resize.right == 0 && resize.bottom == 0
                        || resize.right.abs() == invisible_borders.right
                            && resize.bottom.abs() == invisible_borders.bottom;

                    if is_move {
                        tracing::info!("moving with mouse");

                        if moved_across_monitors {
                            if let Some((
                                origin_monitor_idx,
                                origin_workspace_idx,
                                origin_container_idx,
                            )) = pending
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

                                self.transfer_container(
                                    (
                                        origin_monitor_idx,
                                        origin_workspace_idx,
                                        origin_container_idx,
                                    ),
                                    (
                                        target_monitor_idx,
                                        target_workspace_idx,
                                        target_container_idx,
                                    ),
                                )?;

                                // We want to make sure both the origin and target monitors are updated,
                                // so that we don't have ghost tiles until we force an interaction on
                                // the origin monitor's focused workspace
                                self.focus_monitor(origin_monitor_idx)?;
                                self.focus_workspace(origin_workspace_idx)?;
                                self.update_focused_workspace(false)?;

                                self.focus_monitor(target_monitor_idx)?;
                                self.focus_workspace(target_workspace_idx)?;
                                self.update_focused_workspace(false)?;
                            }
                            // Here we handle a simple move on the same monitor which is treated as
                            // a container swap
                        } else {
                            match new_window_behaviour {
                                WindowContainerBehaviour::Create => {
                                    match workspace.container_idx_from_current_point() {
                                        Some(target_idx) => {
                                            workspace
                                                .swap_containers(focused_container_idx, target_idx);
                                            self.update_focused_workspace(false)?;
                                        }
                                        None => {
                                            self.update_focused_workspace(
                                                self.mouse_follows_focus,
                                            )?;
                                        }
                                    }
                                }
                                WindowContainerBehaviour::Append => {
                                    match workspace.container_idx_from_current_point() {
                                        Some(target_idx) => {
                                            workspace.move_window_to_container(target_idx)?;
                                            self.update_focused_workspace(false)?;
                                        }
                                        None => {
                                            self.update_focused_workspace(
                                                self.mouse_follows_focus,
                                            )?;
                                        }
                                    }
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

                        if resize.right != 0 && resize.left == 0 {
                            ops.push(resize_op!(resize.right, <, OperationDirection::Right));
                        }

                        if resize.bottom != 0 && resize.top == 0 {
                            ops.push(resize_op!(resize.bottom, <, OperationDirection::Down));
                        }

                        for (edge, sizing, delta) in ops {
                            self.resize_window(edge, sizing, delta, true)?;
                        }

                        self.update_focused_workspace(false)?;
                    }
                }
            }
            WindowManagerEvent::DisplayChange(..) | WindowManagerEvent::MouseCapture(..) => {}
        };

        if *self.focused_workspace()?.tile() && BORDER_ENABLED.load(Ordering::SeqCst) {
            match event {
                WindowManagerEvent::MoveResizeStart(_, _) => {
                    let border = Border::from(BORDER_HWND.load(Ordering::SeqCst));
                    border.hide()?;
                    BORDER_HIDDEN.store(true, Ordering::SeqCst);
                }
                WindowManagerEvent::MoveResizeEnd(_, window)
                | WindowManagerEvent::Show(_, window)
                | WindowManagerEvent::FocusChange(_, window)
                | WindowManagerEvent::Hide(_, window)
                | WindowManagerEvent::Minimize(_, window) => {
                    let border = Border::from(BORDER_HWND.load(Ordering::SeqCst));
                    let mut target_window = None;
                    if self
                        .focused_workspace()?
                        .floating_windows()
                        .iter()
                        .any(|w| w.hwnd == window.hwnd)
                    {
                        target_window = Option::from(*window);
                        WindowsApi::raise_window(border.hwnd())?;
                    };

                    if let Some(monocle_container) = self.focused_workspace()?.monocle_container() {
                        if let Some(window) = monocle_container.focused_window() {
                            target_window = Option::from(*window);
                        }
                    }

                    if target_window.is_none() {
                        match self.focused_container() {
                            // if there is no focused container, the desktop is empty
                            Err(..) => {
                                WindowsApi::hide_border_window(border.hwnd())?;
                            }
                            Ok(container) => {
                                if !(matches!(event, WindowManagerEvent::Minimize(_, _))
                                    && container.windows().len() == 1)
                                {
                                    let container_size = self.focused_container()?.windows().len();
                                    target_window = Option::from(*self.focused_window()?);

                                    if container_size > 1 {
                                        BORDER_COLOUR_CURRENT.store(
                                            BORDER_COLOUR_STACK.load(Ordering::SeqCst),
                                            Ordering::SeqCst,
                                        );
                                    } else {
                                        BORDER_COLOUR_CURRENT.store(
                                            BORDER_COLOUR_SINGLE.load(Ordering::SeqCst),
                                            Ordering::SeqCst,
                                        );
                                    }
                                }
                            }
                        }
                    }

                    if let Some(target_window) = target_window {
                        let window = target_window;
                        let mut rect = WindowsApi::window_rect(window.hwnd())?;
                        rect.top -= self.invisible_borders.bottom;
                        rect.bottom += self.invisible_borders.bottom;

                        let activate = BORDER_HIDDEN.load(Ordering::SeqCst);

                        WindowsApi::invalidate_border_rect()?;
                        border.set_position(target_window, &self.invisible_borders, activate)?;

                        if activate {
                            BORDER_HIDDEN.store(false, Ordering::SeqCst);
                        }
                    }
                }
                _ => {}
            }
        }

        // If we unmanaged a window, it shouldn't be immediately hidden behind managed windows
        if let WindowManagerEvent::Unmanage(window) = event {
            window.center(&self.focused_monitor_work_area()?, &invisible_borders)?;
        }

        // If there are no more windows on the workspace, we shouldn't show the border window
        if self.focused_workspace()?.containers().is_empty() {
            let border = Border::from(BORDER_HWND.load(Ordering::SeqCst));
            border.hide()?;
            BORDER_HIDDEN.store(true, Ordering::SeqCst);
        }

        tracing::trace!("updating list of known hwnds");
        let mut known_hwnds = vec![];
        for monitor in self.monitors() {
            for workspace in monitor.workspaces() {
                for container in workspace.containers() {
                    for window in container.windows() {
                        known_hwnds.push(window.hwnd);
                    }
                }
            }
        }

        let hwnd_json = DATA_DIR.join("komorebi.hwnd.json");
        let file = OpenOptions::new()
            .write(true)
            .truncate(true)
            .create(true)
            .open(hwnd_json)?;

        serde_json::to_writer_pretty(&file, &known_hwnds)?;
        notify_subscribers(&serde_json::to_string(&Notification {
            event: NotificationEvent::WindowManager(*event),
            state: self.as_ref().into(),
        })?)?;

        tracing::info!("processed: {}", event.window().to_string());
        Ok(())
    }
}
