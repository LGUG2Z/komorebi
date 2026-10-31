use crate::accessibility::AccessibilityApi;
use crate::accessibility::error::AccessibilityError;
use crate::border_manager;
use crate::window::Window;
use crate::window_manager::WindowManager;
use crossbeam_channel::Receiver;
use crossbeam_channel::Sender;
use objc2_application_services::AXUIElement;
use objc2_core_foundation::CFBoolean;
use parking_lot::Mutex;
use std::sync::Arc;
use std::sync::OnceLock;

pub enum ReaperNotification {
    InvalidWindow(u32),
    MouseUpKeyUp,
}

static CHANNEL: OnceLock<(Sender<ReaperNotification>, Receiver<ReaperNotification>)> =
    OnceLock::new();

pub fn channel() -> &'static (Sender<ReaperNotification>, Receiver<ReaperNotification>) {
    CHANNEL.get_or_init(|| crossbeam_channel::bounded(50))
}

fn event_tx() -> Sender<ReaperNotification> {
    channel().0.clone()
}

fn event_rx() -> Receiver<ReaperNotification> {
    channel().1.clone()
}

pub fn send_notification(notification: ReaperNotification) {
    if event_tx().try_send(notification).is_err() {
        tracing::warn!("channel is full; dropping notification")
    }
}

pub fn listen_for_notifications(wm: Arc<Mutex<WindowManager>>) {
    std::thread::spawn(move || {
        loop {
            match handle_notifications(wm.clone()) {
                Ok(()) => {
                    tracing::warn!("restarting finished thread");
                }
                Err(error) => {
                    tracing::warn!("restarting failed thread: {}", error);
                }
            }
        }
    });
}

fn handle_notifications(wm: Arc<Mutex<WindowManager>>) -> color_eyre::Result<()> {
    tracing::info!("listening");

    let receiver = event_rx();

    'notification: for notification in receiver {
        let mut wm = wm.lock();
        if wm.is_paused {
            tracing::debug!("skipping reaper notification while wm is paused");
            continue 'notification;
        }

        handle_notification(&mut wm, notification, Window::is_valid)?;
    }

    Ok(())
}

pub(crate) fn handle_notification(
    wm: &mut WindowManager,
    notification: ReaperNotification,
    is_valid: impl Fn(&Window) -> bool,
) -> color_eyre::Result<()> {
    match notification {
        ReaperNotification::InvalidWindow(window_id) => reap_window(wm, window_id)?,
        ReaperNotification::MouseUpKeyUp => {
            // This must not move any window: it is handled as soon as the button goes up, ahead
            // of the events queued for the end of a drag, and a window put back on its tile
            // would be measured against where it was dragged from, which is backwards
            for window_id in invalid_window_ids(wm, is_valid)? {
                reap_window(wm, window_id)?;
            }
        }
    }

    Ok(())
}

/// The windows of the focused workspace that no longer exist
fn invalid_window_ids(
    wm: &WindowManager,
    is_valid: impl Fn(&Window) -> bool,
) -> color_eyre::Result<Vec<u32>> {
    let workspace = wm.focused_workspace()?;

    let windows = workspace
        .containers()
        .iter()
        .chain(workspace.monocle_container.iter())
        .flat_map(|container| container.windows().iter())
        .chain(workspace.floating_windows().iter());

    Ok(windows
        .filter(|window| !is_valid(window))
        .map(|window| window.id)
        .collect())
}

fn reap_window(wm: &mut WindowManager, window_id: u32) -> color_eyre::Result<()> {
    let mut should_update = false;
    for monitor in wm.monitors_mut() {
        for workspace in monitor.workspaces_mut() {
            for container in workspace.containers_mut() {
                container.windows_mut().retain(|w| {
                    if w.id == window_id {
                        should_update = true;
                        tracing::info!("reaping window: {window_id}");
                    }

                    w.id != window_id
                });
            }

            if let Some(container) = &mut workspace.monocle_container {
                container.windows_mut().retain(|w| {
                    if w.id == window_id {
                        should_update = true;
                        tracing::info!("reaping window: {window_id}");
                    }

                    w.id != window_id
                });

                if container.windows().is_empty() {
                    workspace.monocle_container = None;
                    workspace.monocle_container_restore_idx = None;
                }
            }

            workspace.floating_windows_mut().retain(|w| {
                if w.id == window_id {
                    should_update = true;
                    tracing::info!("reaping window: {window_id}");
                }

                w.id != window_id
            });
        }
    }

    // If an invalid window was cleaned up, we update the workspace
    if should_update {
        wm.update_focused_workspace(false, false)?;
        border_manager::send_notification(None, Some(window_id), true);
    }

    Ok(())
}

pub fn notify_on_error(
    window: &Window,
    result: Result<(), AccessibilityError>,
) -> Result<(), AccessibilityError> {
    if result.is_err() {
        notify_if_gone(window.id, &window.element);
    }

    result
}

/// Tell the reaper about a window if a failure to write to it was because it no longer exists
///
/// Returns whether the window is gone.
pub fn notify_if_gone(window_id: u32, element: &AXUIElement) -> bool {
    // calibre does some weird stuff when pressing on the menu options which cause layout updates to
    // fail, and tabbed applications hand out elements for tabs that then stop being valid while the
    // window carries on: a window whose element still answers is never reaped
    if AccessibilityApi::copy_attribute_names(element).is_some() {
        return false;
    }

    if let Some(is_fullscreen) =
        AccessibilityApi::copy_attribute_value::<CFBoolean>(element, "AXFullScreen")
        && is_fullscreen.as_bool()
    {
        tracing::debug!(
            "skipping reap for fullscreen window {window_id} during position update failure"
        );

        return false;
    }

    send_notification(ReaperNotification::InvalidWindow(window_id));

    true
}
