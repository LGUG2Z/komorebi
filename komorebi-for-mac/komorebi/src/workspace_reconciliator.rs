#![deny(clippy::unwrap_used, clippy::expect_used)]

use crate::window_manager::WindowManager;
use crate::window_manager_event::WindowManagerEvent;
use crossbeam_channel::Receiver;
use crossbeam_channel::Sender;
use parking_lot::Mutex;
use std::sync::Arc;
use std::sync::OnceLock;
use std::sync::atomic::AtomicBool;
use std::sync::atomic::AtomicU64;
use std::sync::atomic::Ordering;
use std::time::SystemTime;
use std::time::UNIX_EPOCH;

#[derive(Copy, Clone, Debug)]
pub struct Notification {
    pub monitor_idx: usize,
    pub workspace_idx: usize,
    pub triggered_by: WindowManagerEvent,
}

static RECONCILIATION_IN_PROGRESS: AtomicBool = AtomicBool::new(false);
static LAST_RECONCILIATION: AtomicU64 = AtomicU64::new(0);
const COOLDOWN_MS: u64 = 1000; // 1 second cooldown

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

pub fn send_notification(
    monitor_idx: usize,
    workspace_idx: usize,
    triggered_by: WindowManagerEvent,
) {
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64;

    if now - LAST_RECONCILIATION.load(Ordering::SeqCst) < COOLDOWN_MS {
        tracing::debug!("within cooldown period, dropping notification");
        return;
    }

    if !RECONCILIATION_IN_PROGRESS.load(Ordering::Relaxed) {
        tracing::debug!("sending reconciliation request");
        if event_tx()
            .try_send(Notification {
                monitor_idx,
                workspace_idx,
                triggered_by,
            })
            .is_err()
        {
            tracing::warn!("channel is full; dropping notification")
        }
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
                    if cfg!(debug_assertions) {
                        tracing::error!("restarting failed thread: {:?}", error)
                    } else {
                        tracing::error!("restarting failed thread: {}", error)
                    }
                }
            }
        }
    });
}
pub fn handle_notifications(wm: Arc<Mutex<WindowManager>>) -> color_eyre::Result<()> {
    tracing::info!("listening");

    let receiver = event_rx();

    for notification in &receiver {
        RECONCILIATION_IN_PROGRESS.store(true, Ordering::Relaxed);
        tracing::info!("running reconciliation for notification {notification:?}");

        let mut wm = wm.lock();
        let focused_monitor_idx = wm.focused_monitor_idx();
        let focused_workspace_idx =
            wm.focused_workspace_idx_for_monitor_idx(focused_monitor_idx)?;

        let focused_pair = (focused_monitor_idx, focused_workspace_idx);
        let updated_pair = (notification.monitor_idx, notification.workspace_idx);

        if focused_pair != updated_pair {
            // don't switch workspaces if the current workspace is empty
            // this happens when the user just closed the last window on a workspace
            // and the last focused application, usually on another workspace, takes focus
            // and in doing so triggers the reconciliator
            if let Ok(workspace) = wm.focused_workspace()
                && workspace.containers().is_empty()
            {
                tracing::debug!(
                    "current workspace is empty (user closed last window), not reconciling to prevent unwanted workspace switch"
                );
                let now = SystemTime::now()
                    .duration_since(UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_millis() as u64;
                LAST_RECONCILIATION.store(now, Ordering::SeqCst);
                RECONCILIATION_IN_PROGRESS.store(false, Ordering::Relaxed);
                continue;
            }

            tracing::info!("reconciliating workspace");
            wm.focus_monitor(notification.monitor_idx)?;
            let mouse_follows_focus = wm.mouse_follows_focus;

            if let Some(monitor) = wm.focused_monitor_mut() {
                let previous_idx = monitor.focused_workspace_idx();
                monitor.last_focused_workspace = Option::from(previous_idx);
                monitor.focus_workspace(notification.workspace_idx)?;
            }

            // the window that came to the front is the one to focus, and not whichever one the
            // workspace had focused when it was last in view
            if let Some(window_id) = notification.triggered_by.window_id()
                && let Ok(workspace) = wm.focused_workspace_mut()
                && let Err(error) = workspace.focus_container_by_window(window_id)
            {
                tracing::debug!("not focusing window {window_id} on reconciliation: {error}");
            }

            if let Some(monitor) = wm.focused_monitor_mut() {
                monitor.load_focused_workspace(mouse_follows_focus)?;
            }
        }

        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as u64;

        LAST_RECONCILIATION.store(now, Ordering::SeqCst);
        RECONCILIATION_IN_PROGRESS.store(false, Ordering::Relaxed);
    }

    Ok(())
}
