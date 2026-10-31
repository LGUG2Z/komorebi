//! How the window manager answers what the compositor reports: [`WindowManager::process_event`].
//!
//! Nothing changes while the window manager is paused (`TogglePause`). An event about a window
//! komorebi does not manage ([`Window::should_manage`]: a dialog, or a window an ignore rule
//! covers and no manage rule rescues), or about one the user took out of the layout with
//! `UnmanageFocusedWindow`, changes nothing either. Otherwise:
//!
//! - Whatever the event, the windows of the workspaces are looked at first for workspace rules
//!   ([`WindowManager::enforce_workspace_rules`]): a window a rule names is sent to its
//!   workspace.
//! - **Show**, and **Manage** (`ManageFocusedWindow`): the window gets a container of its own after
//!   the focused one, or joins the focused container when the window container behavior is
//!   `Append` ([`WindowManager::window_management_behaviour`]), and the workspace is tiled again
//!   and is on its tiling layer. The first window on an empty workspace is given the focus. With
//!   the float override on, the window floats instead. A window that another workspace has
//!   already is hidden and left to that workspace. A window a float rule matches goes into the
//!   workspace's floating windows instead, on a Show (a Manage tiles it whatever the rules say),
//!   and the workspace is on its floating layer.
//! - **Destroy**, and **Unmanage** (`UnmanageFocusedWindow`): the window is taken out of whichever
//!   workspace has it, and if that is the one in front it is tiled again. The window that was
//!   behind it in a stack comes to the front. A window that is
//!   unmanaged stays open, centered in the work area so that it is not left behind the tiles, and
//!   komorebi leaves it alone until it is managed again.
//! - **FocusChange**: the workspace is tiled again and the container of the window that has the
//!   focus becomes the focused container. In the scrolling layout the workspace is tiled after
//!   that instead, so that the strip scrolls to the window that has the focus. While the
//!   workspace has a monocle container, the focus goes back to its window instead, no matter
//!   which window was given it. The workspace is on its floating layer when the window is one of
//!   its floating windows, and on its tiling layer otherwise.
//! - **Minimize**: a window the user minimized leaves its workspace and the rest are tiled
//!   again; **Restore** brings it back in a container of its own. A minimize of a window komorebi
//!   hid ([`struct@HIDDEN_WINDOWS`]) is not the user's, and changes nothing.
//! - **FullscreenChange**: a window that is fullscreen stays in its container but is left where it
//!   is ([`Window::set_position`] does nothing for it), and the other windows are tiled as if it
//!   were not fullscreen. When it comes back from fullscreen the compositor gives it the frame it
//!   had before, which may not be the tile it has now, so the workspace is tiled again if it is
//!   the one in front.
//! - **MaximizeChange**: a window the compositor maximized, which komorebi did not ask for (the
//!   title bar, a double click, a shortcut), becomes the workspace's maximized window, as
//!   `ToggleMaximize` makes the focused window, and one the user restored goes back in a container
//!   of its own ([`WindowManager::adopt_maximize_change`]). The maximize and the restore that
//!   `ToggleMaximize` asks for come back as this event too, and change nothing. A window that
//!   opens maximized is taken the same way, after its Show.
//! - **TitleUpdate**: nothing. The rules decide when a window opens; a window an ignore rule
//!   matched stays unmanaged when its title changes, and a managed one stays managed, as in kfw.
//! - **MoveStart** and **MoveEnd**: the user dragged a window with the pointer. When they let go
//!   on another monitor, the window's container goes there and takes the place of the container
//!   whose tile the pointer is on ([`WindowManager::transfer_window`]). When they let go on the
//!   same monitor, the focused container swaps places with the container whose tile the pointer
//!   is on, or, when the window container behavior is `Append`, the window joins it, or, with the
//!   float override on, the window floats where it was let go; the workspace is tiled again, and
//!   a window let go where there is no tile goes back to its own. On the same monitor a drag
//!   counts as a move when the window's frame is as wide and as tall as its tile, and a floating
//!   window is not swapped.
//! - **ResizeStart** and **ResizeEnd**: the user dragged an edge or a corner of a window. When they
//!   let go, each edge of the frame they left it with is compared with the same edge of the frame
//!   it had when the drag began, the edges of its tile move by as much
//!   ([`WindowManager::resize_window`]), and the workspace is tiled again. **ResizeStep**, part
//!   way through, tiles the workspace as if the drag ended there, from the layout the drag began
//!   on, and leaves the dragged window where the user has it.
//! - **OutputChange**: a window is on another output than it was, because the user moved it (a
//!   shortcut that sends a window to the next screen) or KWin did (an output went away or came
//!   back). The containers and floating windows of every monitor follow their windows to the
//!   monitors they are on, and the monitors that changed are tiled again
//!   ([`WindowManager::follow_window_to_its_monitor`]).
//! - **WorkspaceChange**: a monitor shows another workspace, because something other than komorebi
//!   switched it (a pager, a shortcut, an app raising a window of another desktop). The window
//!   manager follows ([`WindowManager::adopt_workspace_change`]) and asks the compositor for
//!   nothing but the wallpaper of the workspace: it has already shown the windows and chosen which
//!   to focus. komorebi's own switch comes back as one too, and changes nothing.
//! - **WorkspaceMove**: a window was put on the desktop of another workspace by something other
//!   than komorebi. It moves to that workspace, and whichever of the two is in front is tiled
//!   again.
//!
//! A window that is shown or managed is put on the desktop of the workspace it was managed into
//! ([`Monitor::assign_windows_to_workspaces`]).
//!
//! The daemon hands events to the window manager on a thread of their own,
//! [`listen_for_events`], one at a time and in the order the backend raised them.
//!
//! Once an event about a managed window has been handled, every subscriber is sent a notification
//! of it ([`notify_subscribers`]), the backend is asked for the look again
//! ([`border_manager::send_notification`]), which it does nothing about unless a setting changed,
//! told the color of each window's border ([`border_manager::update_colours`]), and the windows are
//! made as see-through as the settings say ([`transparency_manager::apply`]).
//!
//! [`Window::should_manage`]: crate::window::Window::should_manage
//! [`Monitor::assign_windows_to_workspaces`]: crate::monitor::Monitor::assign_windows_to_workspaces
//! [`Window::set_position`]: crate::window::Window::set_position

use crate::FLOATING_APPLICATIONS;
use crate::HIDDEN_WINDOWS;
use crate::Notification;
use crate::NotificationEvent;
use crate::REGEX_IDENTIFIERS;
use crate::UNMANAGED_WINDOW_IDS;
use crate::backend::backend;
use crate::border_manager;
use crate::core::DefaultLayout;
use crate::core::Layout;
use crate::core::OperationDirection;
use crate::core::Rect;
use crate::core::Sizing;
use crate::core::WindowContainerBehaviour;
use crate::notify_subscribers;
use crate::state::State;
use crate::transparency_manager;
use crate::window::RuleDebug;
use crate::window::Window;
use crate::window::live_resize_begins;
use crate::window::live_resize_ends;
use crate::window::should_act;
use crate::window_manager::LiveResize;
use crate::window_manager::WindowManager;
use crate::window_manager_event::WindowManagerEvent;
use crate::workspace::WorkspaceLayer;
use color_eyre::eyre;
use color_eyre::eyre::OptionExt;
use parking_lot::Mutex;
use std::sync::Arc;
use tracing::instrument;

/// Starts the thread that takes each event from the window manager's channel
/// ([`WindowManager::incoming_events`]) and handles it with [`WindowManager::process_event`].
///
/// An event that fails is logged and the next one is taken: one event the window manager could
/// not act on does not stop the ones after it.
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

/// After a managed window is destroyed KWin activates whatever it considers next. Only when there
/// is nothing to activate does komorebi step in and focus its own selection; otherwise the
/// `FocusChange` for the successor does the reconciling and the two do not fight.
fn should_refocus_after_destroy(foreground: Option<u32>) -> bool {
    foreground.is_none()
}

impl WindowManager {
    /// Handle an event from the compositor, and log how long it took
    ///
    /// The subscribers are sent a notification of an event that was handled
    /// ([`notify_subscribers`]), and a notification that cannot be sent is an error. An event
    /// that fails, or is about a window komorebi does not manage, is not notified.
    pub fn process_event(&mut self, event: WindowManagerEvent) -> eyre::Result<()> {
        let started = std::time::Instant::now();
        let result = self.handle_event(event);
        tracing::debug!("handled {event} in {:?}", started.elapsed());
        result
    }

    #[instrument(skip_all)]
    fn handle_event(&mut self, event: WindowManagerEvent) -> eyre::Result<()> {
        if self.is_paused {
            tracing::trace!("ignoring while paused");
            return Ok(());
        }

        // part way through a drag: only the neighbours move, there is no notification to make
        if let WindowManagerEvent::ResizeStep(_, window) = event {
            crate::window_manager_event_listener::resize_step_taken();
            return self.live_resize_step(window);
        }

        // a monitor changed which workspace it shows; nothing to do with any window
        if let WindowManagerEvent::WorkspaceChange(_, monitor_id, idx) = event {
            let result = self.adopt_workspace_change(monitor_id, idx);

            // the workspace may have a wallpaper, and so a theme, and other windows are in view
            border_manager::send_notification();
            border_manager::update_colours(self);
            transparency_manager::apply(self);
            return result;
        }

        let Some(window) = event.window() else {
            return Ok(());
        };
        let window_id = window.id;

        // A destroyed window can no longer be asked whether it should be managed, and rules must
        // not stop us dropping a window we do manage, so this is settled before the rules are
        let is_removal = matches!(
            event,
            WindowManagerEvent::Destroy(_, _) | WindowManagerEvent::Unmanage(_)
        );

        let mut rule_debug = RuleDebug::default();
        let should_manage = is_removal
            || (window.should_manage(Some(event), &mut rule_debug)?
                && !UNMANAGED_WINDOW_IDS.lock().contains(&window_id));

        if !should_manage {
            tracing::debug!("ignoring event as window should not be managed: {window}");
            return Ok(());
        }

        tracing::info!(
            "processing event: {event} for window {window_id} with notification {}",
            event.notification(),
        );

        // events are read against what is in the containers now, not against what was there when the
        // last one finished
        self.update_known_window_ids();

        #[allow(clippy::useless_asref)]
        // We don't have From implemented for &mut WindowManager
        let initial_state = State::from(self.as_ref());

        // a window can open on a monitor other than the focused one, and focus can follow
        if matches!(
            event,
            WindowManagerEvent::FocusChange(_, _) | WindowManagerEvent::Show(_, _)
        ) && let Some(monitor_idx) = self.monitor_idx_from_window(window)
            && self.focused_monitor_idx() != monitor_idx
        {
            self.focus_monitor(monitor_idx)?;
        }

        self.enforce_workspace_rules()?;

        match event {
            WindowManagerEvent::FocusChange(_, _) => {
                let is_known = self.known_window_ids.get(&window_id).cloned();
                let mut is_on_current_workspace = false;

                // don't want to trigger the full workspace updates when there are no managed
                // containers - this makes floating windows on empty workspaces go into very
                // annoying focus change loops which prevents users from interacting with them
                if !matches!(
                    self.focused_workspace()?.layout,
                    Layout::Default(DefaultLayout::Scrolling)
                ) && !self.focused_workspace()?.containers().is_empty()
                {
                    self.update_focused_workspace(self.mouse_follows_focus, false)?;
                }

                let workspace = self.focused_workspace_mut()?;
                if workspace.contains_window(window_id) {
                    is_on_current_workspace = true;
                }

                let floating_window_idx = workspace
                    .floating_windows()
                    .iter()
                    .position(|w| w.id == window_id);

                match floating_window_idx {
                    None => {
                        if let Some(monocle) = &workspace.monocle_container {
                            if let Some(window) = monocle.focused_window() {
                                window.focus(false)?;
                            }
                        } else if is_on_current_workspace {
                            workspace.focus_container_by_window(window_id)?;
                        }

                        workspace.layer = WorkspaceLayer::Tiling;

                        if matches!(
                            self.focused_workspace()?.layout,
                            Layout::Default(DefaultLayout::Scrolling)
                        ) && !self.focused_workspace()?.containers().is_empty()
                        {
                            self.update_focused_workspace(self.mouse_follows_focus, false)?;
                        }
                    }
                    Some(idx) => {
                        if let Some(_window) = workspace.floating_windows().get(idx) {
                            workspace.layer = WorkspaceLayer::Floating;
                        }
                    }
                }

                // A window of another workspace being activated (an app raising itself) makes
                // KWin switch that monitor to the window's desktop, and tells us in a
                // `WorkspaceChange`: there is nothing to do about the window until that arrives
                let _ = (is_known, is_on_current_workspace);
            }
            // handled before the window is looked up
            WindowManagerEvent::WorkspaceChange(..) => {}
            // handled before the window is looked up
            WindowManagerEvent::ResizeStep(..) => {}
            WindowManagerEvent::OutputChange(..) => {
                self.follow_window_to_its_monitor(window)?;
            }
            WindowManagerEvent::MaximizeChange(_, _, maximized) => {
                self.adopt_maximize_change(window_id, maximized)?;
            }
            WindowManagerEvent::FullscreenChange(_, _, fullscreen) => {
                // While it is fullscreen the window is left alone. Coming back, KWin gives it the
                // frame it had before, which may not be the tile it has now.
                if !fullscreen
                    && let Some((m_idx, w_idx)) = self.known_window_ids.get(&window_id).cloned()
                {
                    let offset = self.work_area_offset;
                    if let Some(monitor) = self.monitors_mut().get_mut(m_idx)
                        && monitor.focused_workspace_idx() == w_idx
                    {
                        monitor.update_focused_workspace(offset)?;
                    }
                }
            }
            WindowManagerEvent::WorkspaceMove(_, _, idx) => {
                // somebody put the window on another workspace's desktop: it belongs there now.
                // (komorebi's own moves come back here too, and find the window already there.)
                if let Some((m_idx, w_idx)) = self.known_window_ids.get(&window_id).cloned()
                    && w_idx != idx
                {
                    tracing::info!("window {window_id} was moved to workspace {idx}");
                    let offset = self.work_area_offset;

                    if let Some(monitor) = self.monitors_mut().get_mut(m_idx) {
                        monitor.ensure_workspace_count(idx + 1);

                        if let Some(from) = monitor.workspaces_mut().get_mut(w_idx) {
                            from.remove_window(window_id)?;
                        }

                        if let Some(to) = monitor.workspaces_mut().get_mut(idx) {
                            to.new_container_for_window(&window)?;
                        }

                        // whichever of the two is showing needs its tiles redone
                        monitor.update_focused_workspace(offset)?;
                    }
                }
            }
            WindowManagerEvent::Show(_, _) | WindowManagerEvent::Manage(_) => {
                let focused_monitor_idx = self.focused_monitor_idx();
                let focused_workspace_idx =
                    self.focused_workspace_idx_for_monitor_idx(focused_monitor_idx)?;

                let mut create = true;

                if self.focused_workspace()?.contains_window(window_id) {
                    tracing::debug!("ignoring show event for window already on workspace");

                    // ignore bogus show events
                    create = false;
                }

                if let Some((m_idx, w_idx)) = self.known_window_ids.get(&window_id)
                    && let Ok(focused_workspace_idx) = self.focused_workspace_idx()
                    && (*m_idx != self.focused_monitor_idx() || *w_idx != focused_workspace_idx)
                {
                    tracing::debug!(
                        "ignoring show event for window already associated with another workspace"
                    );

                    window.hide();
                    create = false;
                }

                if create {
                    let behaviour = self
                        .window_management_behaviour(focused_monitor_idx, focused_workspace_idx);
                    let workspace = self.focused_workspace_mut()?;
                    let workspace_contains_window = workspace.contains_window(window_id);
                    let monocle_container = workspace.monocle_container.clone();

                    let floating_applications = FLOATING_APPLICATIONS.lock();
                    let mut should_float = false;

                    if !floating_applications.is_empty() {
                        let regex_identifiers = REGEX_IDENTIFIERS.lock();

                        if let (Some(title), Some(exe_name), Some(class), Some(path)) =
                            (window.title(), window.exe(), window.class(), window.path())
                        {
                            should_float = should_act(
                                &title,
                                &exe_name,
                                &[&class],
                                &path.to_string_lossy(),
                                &floating_applications,
                                &regex_identifiers,
                            )
                            .is_some();
                        }
                    }
                    drop(floating_applications);

                    if behaviour.float_override
                        || behaviour.floating_layer_override
                        || (should_float && !matches!(event, WindowManagerEvent::Manage(_)))
                    {
                        let placement = if behaviour.floating_layer_override {
                            // Floating layer override placement
                            behaviour.floating_layer_placement
                        } else if behaviour.float_override {
                            // Float override placement
                            behaviour.float_override_placement
                        } else {
                            // Float rule placement
                            behaviour.float_rule_placement
                        };
                        // Center floating windows according to the proper placement if not
                        // on a floating workspace
                        let center_spawned_floats = placement.should_center() && workspace.tile;
                        workspace.floating_windows_mut().push_back(window);
                        workspace.layer = WorkspaceLayer::Floating;
                        if center_spawned_floats {
                            let mut floating_window = window;
                            floating_window
                                .center(&workspace.globals.work_area, placement.should_resize())?;
                        }

                        self.update_focused_workspace(false, false)?;
                    } else {
                        match behaviour.current_behaviour {
                            WindowContainerBehaviour::Create => {
                                workspace.new_container_for_window(&window)?;
                                workspace.layer = WorkspaceLayer::Tiling;
                                self.update_focused_workspace(false, false)?;
                            }
                            WindowContainerBehaviour::Append => {
                                workspace
                                    .focused_container_mut()
                                    .ok_or_eyre("there is no focused container")?
                                    .add_window(&window)?;
                                workspace.layer = WorkspaceLayer::Tiling;
                                self.update_focused_workspace(true, false)?;
                            }
                        }

                        if (self.focused_workspace()?.containers().len() == 1
                            && self.focused_workspace()?.floating_windows().is_empty())
                            || (self.focused_workspace()?.containers().is_empty()
                                && self.focused_workspace()?.floating_windows().len() == 1)
                        {
                            // If after adding this window the workspace only contains 1 window, it
                            // was previously empty, so give this window the focus
                            window.focus(self.mouse_follows_focus)?;
                        }
                    }

                    // an app that remembers it was maximized opens that way
                    if backend().window(window_id).is_some_and(|w| w.maximized) {
                        self.update_known_window_ids();
                        self.adopt_maximize_change(window_id, true)?;
                    }

                    if workspace_contains_window {
                        let mut monocle_window_event = false;
                        if let Some(ref monocle) = monocle_container
                            && let Some(monocle_window) = monocle.focused_window()
                            && monocle_window.id == window_id
                        {
                            monocle_window_event = true;
                        }

                        let workspace = self.focused_workspace()?;
                        if !(monocle_window_event || workspace.layer != WorkspaceLayer::Tiling)
                            && monocle_container.is_some()
                        {
                            window.hide();
                        }
                    }
                }
            }
            WindowManagerEvent::Destroy(_, _) | WindowManagerEvent::Unmanage(_) => {
                let destroyed = matches!(event, WindowManagerEvent::Destroy(_, _));

                // the window may be on any workspace, not only the one in front
                let location = self.known_window_ids.get(&window_id).cloned();

                if let Some((m_idx, w_idx)) = location {
                    let behaviour = self.window_management_behaviour(m_idx, w_idx);
                    let focused = m_idx == self.focused_monitor_idx()
                        && Some(w_idx) == self.focused_workspace_idx().ok();

                    if let Some(workspace) = self
                        .monitors_mut()
                        .get_mut(m_idx)
                        .and_then(|m| m.workspaces_mut().get_mut(w_idx))
                    {
                        let removed = workspace.remove_window(window_id);

                        if !destroyed
                            && focused
                            && let Ok(mut window) = removed
                        {
                            // we stopped managing a window that is still open, so it should not end up
                            // hidden behind the ones we do manage
                            let placement = if behaviour.floating_layer_override {
                                behaviour.floating_layer_placement
                            } else if behaviour.float_override {
                                behaviour.float_override_placement
                            } else {
                                behaviour.float_rule_placement
                            };

                            window
                                .center(&workspace.globals.work_area, placement.should_resize())?;
                        }
                    }

                    if focused {
                        let refocus = should_refocus_after_destroy(backend().foreground_window());
                        self.update_focused_workspace(refocus, refocus)?;
                    }
                }

                if destroyed {
                    self.minimized_windows.remove(&window_id);
                    self.already_moved_window_handles.lock().remove(&window_id);
                    HIDDEN_WINDOWS.lock().remove(&window_id);
                    UNMANAGED_WINDOW_IDS.lock().retain(|id| *id != window_id);
                } else {
                    // we only want to add the window ID after the window has been removed
                    UNMANAGED_WINDOW_IDS.lock().push(window_id);
                }
            }
            WindowManagerEvent::Minimize(_, _) => {
                // A window komorebi took out of sight is not one the user minimized, whatever the
                // compositor reports about it: it stays in its container, and only the user's own
                // minimizes are acted on
                if HIDDEN_WINDOWS.lock().contains(&window_id) {
                    tracing::trace!("ignoring minimize of window {window_id}, hidden by komorebi");
                } else {
                    self.extract_minimized_window(window_id)?;
                    self.update_focused_workspace(false, false)?;
                }
            }
            WindowManagerEvent::Restore(_, _) => match self.minimized_windows.remove(&window_id) {
                None => {}
                Some(window) => {
                    let workspace = self.focused_workspace_mut()?;
                    workspace.new_container_for_window(&window)?;
                    self.update_focused_workspace(false, false)?;
                }
            },
            WindowManagerEvent::TitleUpdate(_, _) => {
                // The rules decide when a window opens, by the title it has then. A later title
                // change does not make the daemon look at the window again: one an ignore rule
                // matched stays unmanaged, and one that is managed stays so. kfw does the same.
            }
            WindowManagerEvent::MoveStart(_, window) => {
                let window_id = window.id;
                if self.pending_move_op.is_none() && self.pending_resize_op.is_none() {
                    let monitor_idx = self.focused_monitor_idx();
                    let workspace_idx = self
                        .focused_monitor()
                        .ok_or_eyre("there is no monitor with this idx")?
                        .focused_workspace_idx();

                    let pending_move_op = Arc::make_mut(&mut self.pending_move_op);
                    *pending_move_op = Option::from((monitor_idx, workspace_idx, window_id));
                }
            }
            WindowManagerEvent::MoveEnd(_, window) => {
                let window_id = window.id;
                // We need this because if the event ends on a different monitor,
                // that monitor will already have been focused and updated in the state
                let pending = *self.pending_move_op;
                // Always consume the pending move op whenever this event is handled
                let pending_move_op = Arc::make_mut(&mut self.pending_move_op);
                *pending_move_op = None;

                if let Some((origin_monitor_idx, origin_workspace_idx, wid)) = pending {
                    // If the window handles don't match then something went wrong and the pending move
                    // is not related to this current move, if so abort this operation.
                    if wid != window_id {
                        eyre::bail!(
                            "window handles for move operation don't match: {} != {}",
                            wid,
                            window_id
                        );
                    }
                    let known_window_ids = self.known_window_ids.clone();

                    let target_monitor_idx = self
                        .monitor_idx_from_current_pos()
                        .ok_or_eyre("cannot get monitor idx from current position")?;

                    let focused_monitor_idx = self.focused_monitor_idx();
                    let focused_workspace_idx = self.focused_workspace_idx().unwrap_or_default();
                    let window_management_behaviour = self
                        .window_management_behaviour(focused_monitor_idx, focused_workspace_idx);

                    let workspace = self.focused_workspace_mut()?;
                    let focused_container_idx = workspace.focused_container_idx();

                    if let Some(container) = workspace.focused_container()
                        && let Some(window) = container.focused_window()
                    {
                        // TODO: not sure about this clone
                        let window = *window;
                        let new_position = window.rect()?;
                        let old_position = *workspace
                            .latest_layout
                            .get(focused_container_idx)
                            // If the move was to another monitor with an empty workspace, the
                            // workspace here will refer to that empty workspace, which won't
                            // have any latest layout set. We fall back to a Default for Rect
                            // which allows us to make a reasonable guess that the drag has taken
                            // place across a monitor boundary to an empty workspace
                            .unwrap_or(&Rect::default());

                        // This will be true if we have moved to another monitor
                        let mut moved_across_monitors = false;

                        if let Some((m_idx, _)) = known_window_ids.get(&window_id)
                            && *m_idx != target_monitor_idx
                        {
                            moved_across_monitors = true;
                        }

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
                                    .ok_or_eyre("cannot get monitor idx")?
                                    .workspaces()
                                    .get(origin_workspace_idx)
                                    .ok_or_eyre("cannot get workspace idx")?;

                                let managed_window = origin_workspace.contains_window(window_id);

                                if !managed_window {
                                    moved_across_monitors = false;
                                }
                            }
                        }

                        let workspace = self.focused_workspace_mut()?;
                        if (workspace.tile && workspace.contains_managed_window(window_id))
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
                                    if let Some((
                                        origin_monitor_idx,
                                        origin_workspace_idx,
                                        w_hwnd,
                                    )) = pending
                                    {
                                        let target_workspace_idx = self
                                            .monitors()
                                            .get(target_monitor_idx)
                                            .ok_or_eyre("there is no monitor at this idx")?
                                            .focused_workspace_idx();

                                        let target_container_idx = self
                                            .monitors()
                                            .get(target_monitor_idx)
                                            .ok_or_eyre("there is no monitor at this idx")?
                                            .focused_workspace()
                                            .ok_or_eyre(
                                                "there is no focused workspace for this monitor",
                                            )?
                                            .container_idx_from_current_point()
                                            // Default to 0 in the case of an empty workspace
                                            .unwrap_or(0);

                                        let origin =
                                            (origin_monitor_idx, origin_workspace_idx, w_hwnd);
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
                                            .ok_or_eyre("there is no monitor at this idx")?;
                                        origin_monitor.focus_workspace(origin_workspace_idx)?;
                                        self.update_focused_workspace(false, false)?;

                                        self.focus_monitor(target_monitor_idx)?;
                                        let target_monitor = self
                                            .monitors_mut()
                                            .get_mut(target_monitor_idx)
                                            .ok_or_eyre("there is no monitor at this idx")?;
                                        target_monitor.focus_workspace(target_workspace_idx)?;
                                        self.update_focused_workspace(false, false)?;

                                        // Make sure to give focus to the moved window again
                                        window.focus(self.mouse_follows_focus)?;
                                    }
                                } else if window_management_behaviour.float_override {
                                    // TODO: unsure of this clone
                                    workspace.floating_windows_mut().push_back(window);
                                    self.update_focused_workspace(false, false)?;
                                } else {
                                    match window_management_behaviour.current_behaviour {
                                        WindowContainerBehaviour::Create => {
                                            match workspace.container_idx_from_current_point() {
                                                Some(target_idx) => {
                                                    workspace.swap_containers(
                                                        focused_container_idx,
                                                        target_idx,
                                                    );
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
                                                    workspace
                                                        .move_window_to_container(target_idx)?;
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
                                    }
                                }
                            }
                        }
                    }
                }
            }
            WindowManagerEvent::ResizeStart(_, window) => {
                let window_id = window.id;

                // A drag is measured from the frame the window has when it begins, which is what
                // the user had under the pointer, and not from the tile the layout gave it: an app
                // that snaps its size to a grid (a terminal), or has a minimum, is never exactly on
                // its tile, and every drag would then look as if it had moved the other edges too.
                // A start reported again during the same drag changes nothing: the first one is
                // the one that counts.
                let already_started = matches!(
                    *self.pending_resize_op,
                    Some((started, Some(_))) if started == window_id
                );

                if !already_started {
                    let started_from = match crate::backend::backend().drag_origin(window_id) {
                        Some(origin) => origin,
                        None => window.rect()?,
                    };
                    let pending_resize_op = Arc::make_mut(&mut self.pending_resize_op);
                    *pending_resize_op = Option::from((window_id, Some(started_from)));

                    // what the layout is as the user takes hold, for the steps of the drag to be
                    // worked out from, and for it to go back to if they take the drag back
                    self.live_resize = self.focused_workspace().ok().and_then(|workspace| {
                        workspace
                            .container_idx_for_window(window_id)
                            .filter(|_| workspace.tile)
                            .map(|_| LiveResize {
                                window: window_id,
                                containers: workspace.containers().len(),
                                resize_dimensions: workspace.resize_dimensions.clone(),
                                latest_layout: workspace.latest_layout.clone(),
                                scrolling: workspace.scrolling.clone(),
                                stepped: false,
                            })
                    });

                    if self.live_resize.is_some() {
                        live_resize_begins(window_id);
                    }
                }
            }
            WindowManagerEvent::ResizeEnd(_, window) => {
                let window_id = window.id;
                let pending = *self.pending_resize_op;
                // Always consume the pending resize op whenever this event is handled
                let pending_resize_op = Arc::make_mut(&mut self.pending_resize_op);
                *pending_resize_op = None;

                // the drag is over: the window is komorebi's to put in its tile again, and the
                // layout is what it was when the drag began, to have the whole drag done on it
                live_resize_ends(window_id);
                let live = self
                    .live_resize
                    .take()
                    .filter(|live| live.window == window_id);
                let stepped = live.as_ref().is_some_and(|live| live.stepped);
                if let Some(live) = live
                    && let Ok(workspace) = self.focused_workspace_mut()
                    && live.stepped
                    && workspace.containers().len() == live.containers
                {
                    workspace.resize_dimensions = live.resize_dimensions;
                    workspace.latest_layout = live.latest_layout;
                    workspace.scrolling = live.scrolling;
                }

                if let Some((wid, Some(started_from))) = pending {
                    // If the window handles don't match then something went wrong and the pending resize
                    // is not related to this current resize, if so abort this operation.
                    if wid != window_id {
                        eyre::bail!(
                            "window handles for resize operation don't match: {} != {}",
                            wid,
                            window_id
                        );
                    }

                    // where the user took the edges, which is not the window's frame while the
                    // client is still drawing at the new size, and the frame events after the
                    // drag can have changed that by now: the end of a resize does not wait for
                    // them
                    let new_position = match crate::backend::backend().drag_end(window_id) {
                        Some(asked) => asked,
                        None => window.rect()?,
                    };
                    let resized = self.apply_mouse_resize(window, started_from, new_position)?;

                    // put every window where the layout now says: the dragged one too when
                    // nothing could be done with what it was given (an edge at the edge of the
                    // screen has no neighbor to take room from); and the others back, if they
                    // were moved along with the drag and the drag was taken back
                    if resized && (new_position != started_from || stepped) {
                        self.update_focused_workspace(false, false)?;
                    }
                }
            }
        }

        // a window that was just managed is on the desktop the compositor opened it on, which is the
        // one in front where it opened; make sure that is the workspace it was managed into
        if matches!(
            event,
            WindowManagerEvent::Show(_, _) | WindowManagerEvent::Manage(_)
        ) {
            for monitor in self.monitors() {
                monitor.assign_windows_to_workspaces();
            }
        }

        self.update_known_window_ids();

        notify_subscribers(
            Notification {
                event: NotificationEvent::WindowManager(event),
                state: self.as_ref().into(),
            },
            initial_state.has_been_modified(self.as_ref()),
        )?;

        border_manager::send_notification();
        border_manager::update_colours(self);
        transparency_manager::apply(self);

        Ok(())
    }
}

impl WindowManager {
    /// Work out what the user did to the edges of a tile and ask the layout for it. The edges are
    /// compared from where the window was when the drag began; the layout moves a shared edge by half
    /// of what it is given. Returns whether there was a tile to resize.
    fn apply_mouse_resize(
        &mut self,
        window: Window,
        started_from: Rect,
        new_position: Rect,
    ) -> eyre::Result<bool> {
        let window_id = window.id;
        let workspace = self.focused_workspace_mut()?;

        // The container that was dragged, which is not necessarily the focused one: the press that
        // began the drag may not have been given focus yet
        let Some(container_idx) = workspace
            .container_idx_for_window(window_id)
            .filter(|_| workspace.tile)
        else {
            return Ok(false);
        };

        if container_idx != workspace.focused_container_idx() {
            workspace.focus_container(container_idx);
        }

        // How far each edge went. (A rect is left, top, width, height, so the right and bottom edges
        // have to be worked out: comparing the widths, which is what comparing the rects field by
        // field does, makes a drag of the left edge look like a drag of the right one too, and the
        // code that then had to tell them apart did so by guessing.)
        let edges = |rect: &Rect| {
            (
                rect.left,
                rect.top,
                rect.left + rect.right,
                rect.top + rect.bottom,
            )
        };
        let (start_left, start_top, start_right, start_bottom) = edges(&started_from);
        let (end_left, end_top, end_right, end_bottom) = edges(&new_position);

        let moved = [
            (OperationDirection::Left, end_left - start_left),
            (OperationDirection::Up, end_top - start_top),
            (OperationDirection::Right, end_right - start_right),
            (OperationDirection::Down, end_bottom - start_bottom),
        ];

        tracing::debug!(
            "resizing with mouse: {started_from:?} -> {new_position:?}, edges moved (left, up, right, down): {:?}",
            moved.map(|(_, by)| by)
        );

        for (edge, by) in moved {
            if by == 0 {
                continue;
            }

            // an edge that goes outward makes the tile bigger, inward smaller; the layout moves a
            // shared edge by half of what it is given
            let outward = match edge {
                OperationDirection::Left | OperationDirection::Up => by < 0,
                OperationDirection::Right | OperationDirection::Down => by > 0,
            };
            let sizing = if outward {
                Sizing::Increase
            } else {
                Sizing::Decrease
            };

            self.resize_window(edge, sizing, by.abs() * 2, false)?;
        }

        Ok(true)
    }

    /// The user has the edge of a tile part way: the tiles next to it take the room, as they will
    /// when it is let go. The dragged window is theirs until then (`window::live_resize_begins`).
    fn live_resize_step(&mut self, window: Window) -> eyre::Result<()> {
        let Some((wid, Some(started_from))) = *self.pending_resize_op else {
            return Ok(());
        };
        let Some(live) = self.live_resize.clone().filter(|live| live.window == wid) else {
            return Ok(());
        };
        if wid != window.id {
            return Ok(());
        }

        let new_position = window.rect()?;
        let offset = self.work_area_offset;

        // the drag so far, on the layout as it was when the drag began
        {
            let workspace = self.focused_workspace_mut()?;

            if workspace.containers().len() != live.containers {
                // a window opened or closed: that layout is gone, and the drag goes on without
                // being shown (what is asked of the layout when it ends is what counts)
                self.live_resize = None;
                return Ok(());
            }

            workspace.resize_dimensions = live.resize_dimensions;
            workspace.latest_layout = live.latest_layout;
            workspace.scrolling = live.scrolling;
        }

        if !self.apply_mouse_resize(window, started_from, new_position)? {
            return Ok(());
        }

        if let Some(live) = self.live_resize.as_mut() {
            live.stepped = true;
        }

        self.focused_monitor_mut()
            .ok_or_eyre("there is no monitor")?
            .update_focused_workspace(offset)
    }
}
