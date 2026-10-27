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
//! - **TitleUpdate**: a window komorebi does not have, and that was not minimized, is announced
//!   again as a Show, so that a rule that matches on the title it has now decides.
//! - **WorkspaceChange**: a monitor shows another workspace, because something other than komorebi
//!   switched it (a pager, a shortcut, an app raising a window of another desktop). The window
//!   manager follows ([`WindowManager::adopt_workspace_change`]) and asks the compositor for
//!   nothing: it has already shown the windows and chosen which to focus. komorebi's own switch
//!   comes back as one too, and changes nothing.
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
//! and told the color of each window's border ([`border_manager::update_colours`]).
//!
//! [`Window::should_manage`]: crate::window::Window::should_manage
//! [`Monitor::assign_windows_to_workspaces`]: crate::monitor::Monitor::assign_windows_to_workspaces

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
use crate::core::WindowContainerBehaviour;
use crate::notify_subscribers;
use crate::state::State;
use crate::window::RuleDebug;
use crate::window::should_act;
use crate::window_manager::WindowManager;
use crate::window_manager_event::KWinNotification;
use crate::window_manager_event::SystemNotification;
use crate::window_manager_event::WindowManagerEvent;
use crate::window_manager_event_listener;
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

        // a monitor changed which workspace it shows; nothing to do with any window
        if let WindowManagerEvent::WorkspaceChange(_, monitor_id, idx) = event {
            let result = self.adopt_workspace_change(monitor_id, idx);

            // other windows are in view
            border_manager::send_notification();
            border_manager::update_colours(self);
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
                // Windows are routinely announced before their title is set. Now that it is, a rule
                // that matches on the title can finally decide, so a window that was not picked up
                // gets another chance to be.
                if !self.known_window_ids.contains_key(&window_id)
                    && !self.minimized_windows.contains_key(&window_id)
                {
                    window_manager_event_listener::send_notification(WindowManagerEvent::Show(
                        SystemNotification::KWin(KWinNotification::WindowAdded),
                        window,
                    ));
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

        Ok(())
    }
}
