//! How the window manager answers what the compositor reports: [`WindowManager::process_event`].
//!
//! An event about a window komorebi does not manage ([`Window::should_manage`]) changes nothing.
//! Otherwise:
//!
//! - **Show**: the window gets a container of its own after the focused one, and the workspace is
//!   tiled again. The first window on an empty workspace is given the focus.
//! - **Destroy**: the window is taken out of whichever workspace has it, and if that is the one in
//!   front it is tiled again.
//! - **FocusChange**: the workspace is tiled again and the container of the window that has the
//!   focus becomes the focused container.
//! - **Minimize**: a window the user minimized leaves its workspace and the rest are tiled
//!   again; **Restore** brings it back in a container of its own.
//!
//! The daemon hands events to the window manager on a thread of their own,
//! [`listen_for_events`], one at a time and in the order the backend raised them.
//!
//! Once an event about a managed window has been handled, every subscriber is sent a notification
//! of it ([`notify_subscribers`]).
//!
//! [`Window::should_manage`]: crate::window::Window::should_manage

use crate::Notification;
use crate::NotificationEvent;
use crate::backend::backend;
use crate::notify_subscribers;
use crate::state::State;
use crate::window_manager::WindowManager;
use crate::window_manager_event::WindowManagerEvent;
use color_eyre::eyre;
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
        let Some(window) = event.window() else {
            return Ok(());
        };
        let window_id = window.id;

        // A destroyed window can no longer be asked whether it should be managed, so a removal is
        // let through without asking
        let is_removal = matches!(event, WindowManagerEvent::Destroy(_, _));

        let should_manage = is_removal || window.should_manage();

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

        match event {
            WindowManagerEvent::FocusChange(_, _) => {
                let mut is_on_current_workspace = false;

                // don't want to trigger the full workspace updates when there are no managed
                // containers
                if !self.focused_workspace()?.containers().is_empty() {
                    self.update_focused_workspace(self.mouse_follows_focus, false)?;
                }

                let workspace = self.focused_workspace_mut()?;
                if workspace.contains_window(window_id) {
                    is_on_current_workspace = true;
                }

                if is_on_current_workspace {
                    workspace.focus_container_by_window(window_id)?;
                }
            }
            WindowManagerEvent::Show(_, _) => {
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

                    create = false;
                }

                if create {
                    let workspace = self.focused_workspace_mut()?;
                    workspace.new_container_for_window(&window)?;
                    self.update_focused_workspace(false, false)?;

                    if self.focused_workspace()?.containers().len() == 1 {
                        // If after adding this window the workspace only contains 1 window, it
                        // was previously empty, so give this window the focus
                        window.focus(self.mouse_follows_focus)?;
                    }
                }
            }
            WindowManagerEvent::Destroy(_, _) => {
                // the window may be on any workspace, not only the one in front
                let location = self.known_window_ids.get(&window_id).cloned();

                if let Some((m_idx, w_idx)) = location {
                    let focused = m_idx == self.focused_monitor_idx()
                        && Some(w_idx) == self.focused_workspace_idx().ok();

                    if let Some(workspace) = self
                        .monitors_mut()
                        .get_mut(m_idx)
                        .and_then(|m| m.workspaces_mut().get_mut(w_idx))
                    {
                        let _ = workspace.remove_window(window_id);
                    }

                    if focused {
                        let refocus = should_refocus_after_destroy(backend().foreground_window());
                        self.update_focused_workspace(refocus, refocus)?;
                    }
                }

                self.minimized_windows.remove(&window_id);
            }
            WindowManagerEvent::Minimize(_, _) => {
                self.extract_minimized_window(window_id)?;
                self.update_focused_workspace(false, false)?;
            }
            WindowManagerEvent::Restore(_, _) => match self.minimized_windows.remove(&window_id) {
                None => {}
                Some(window) => {
                    let workspace = self.focused_workspace_mut()?;
                    workspace.new_container_for_window(&window)?;
                    self.update_focused_workspace(false, false)?;
                }
            },
        }

        self.update_known_window_ids();

        notify_subscribers(
            Notification {
                event: NotificationEvent::WindowManager(event),
                state: self.as_ref().into(),
            },
            initial_state.has_been_modified(self.as_ref()),
        )?;

        Ok(())
    }
}
