#![deny(clippy::unwrap_used, clippy::expect_used)]

use crate::border_manager;
use crate::WindowManager;
use crossbeam_channel::Receiver;
use crossbeam_channel::Sender;
use crossbeam_utils::atomic::AtomicCell;
use lazy_static::lazy_static;
use parking_lot::Mutex;
use std::sync::Arc;
use std::sync::OnceLock;
use std::time::Duration;
use std::time::Instant;

#[derive(Copy, Clone)]
pub struct Notification {
    pub monitor_idx: usize,
    pub workspace_idx: usize,
}

pub static ALT_TAB_HWND: AtomicCell<Option<isize>> = AtomicCell::new(None);

lazy_static! {
    pub static ref ALT_TAB_HWND_INSTANT: Arc<Mutex<Instant>> = Arc::new(Mutex::new(Instant::now()));
}

static CHANNEL: OnceLock<(Sender<Notification>, Receiver<Notification>)> = OnceLock::new();

pub fn channel() -> &'static (Sender<Notification>, Receiver<Notification>) {
    CHANNEL.get_or_init(|| crossbeam_channel::bounded(1))
}

fn event_tx() -> Sender<Notification> {
    channel().0.clone()
}

fn event_rx() -> Receiver<Notification> {
    channel().1.clone()
}

pub fn send_notification(monitor_idx: usize, workspace_idx: usize) {
    if event_tx()
        .try_send(Notification {
            monitor_idx,
            workspace_idx,
        })
        .is_err()
    {
        tracing::warn!("channel is full; dropping notification")
    }
}

pub fn listen_for_notifications(wm: Arc<Mutex<WindowManager>>) {
    std::thread::spawn(move || loop {
        match handle_notifications(wm.clone()) {
            Ok(()) => {
                tracing::warn!("restarting finished thread");
            }
            Err(error) => {
                if cfg!(debug_assertions) {
                    tracing::error!("restarting failed thread: {:?}", error)
                } else {
                    tracing::error!("restarting failed thread: {}", error)
                }
            }
        }
    });
}
pub fn handle_notifications(wm: Arc<Mutex<WindowManager>>) -> color_eyre::Result<()> {
    tracing::info!("listening");

    let receiver = event_rx();
    let arc = wm.clone();

    for notification in receiver {
        tracing::info!("running reconciliation");

        let mut wm = wm.lock();
        let focused_monitor_idx = wm.focused_monitor_idx();
        let focused_workspace_idx =
            wm.focused_workspace_idx_for_monitor_idx(focused_monitor_idx)?;

        let focused_pair = (focused_monitor_idx, focused_workspace_idx);
        let updated_pair = (notification.monitor_idx, notification.workspace_idx);

        if focused_pair != updated_pair {
            wm.focus_monitor(notification.monitor_idx)?;
            let mouse_follows_focus = wm.mouse_follows_focus;

            if let Some(monitor) = wm.focused_monitor_mut() {
                let previous_idx = monitor.focused_workspace_idx();
                monitor.set_last_focused_workspace(Option::from(previous_idx));
                monitor.focus_workspace(notification.workspace_idx)?;
                monitor.load_focused_workspace(mouse_follows_focus)?;
            }

            // Drop our lock on the window manager state here to not slow down updates
            drop(wm);

            // Check if there was an alt-tab across workspaces in the last second
            if let Some(hwnd) = ALT_TAB_HWND.load() {
                if ALT_TAB_HWND_INSTANT
                    .lock()
                    .elapsed()
                    .lt(&Duration::from_secs(1))
                {
                    // Sleep for 100 millis to let other events pass
                    std::thread::sleep(Duration::from_millis(100));
                    tracing::info!("focusing alt-tabbed window");

                    // Take a new lock on the wm and try to focus the container with
                    // the recorded HWND from the alt-tab
                    let mut wm = arc.lock();
                    if let Ok(workspace) = wm.focused_workspace_mut() {
                        // Regardless of if this fails, we need to get past this part
                        // to unblock the border manager below
                        let _ = workspace.focus_container_by_window(hwnd);
                    }

                    // Unblock the border manager
                    ALT_TAB_HWND.store(None);
                    // Send a notification to the border manager to update the borders
                    border_manager::send_notification(None);
                }
            }
        }
    }

    Ok(())
}
