#![deny(clippy::unwrap_used, clippy::expect_used)]

use crate::WindowManager;
use crossbeam_channel::Receiver;
use crossbeam_channel::Sender;
use parking_lot::Mutex;
use std::sync::Arc;
use std::sync::OnceLock;

#[derive(Copy, Clone)]
pub struct Notification {
    pub monitor_idx: usize,
    pub workspace_idx: usize,
}

static CHANNEL: OnceLock<(Sender<Notification>, Receiver<Notification>)> = OnceLock::new();

pub fn channel() -> &'static (Sender<Notification>, Receiver<Notification>) {
    CHANNEL.get_or_init(|| crossbeam_channel::bounded(1))
}

pub fn event_tx() -> Sender<Notification> {
    channel().0.clone()
}

pub fn event_rx() -> Receiver<Notification> {
    channel().1.clone()
}

pub fn listen_for_notifications(wm: Arc<Mutex<WindowManager>>) {
    std::thread::spawn(move || loop {
        match handle_notifications(wm.clone()) {
            Ok(()) => {
                tracing::warn!("restarting finished thread");
            }
            Err(error) => {
                tracing::warn!("restarting failed thread: {}", error);
            }
        }
    });
}
pub fn handle_notifications(wm: Arc<Mutex<WindowManager>>) -> color_eyre::Result<()> {
    tracing::info!("listening");

    let receiver = event_rx();

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
                monitor.focus_workspace(notification.workspace_idx)?;
                monitor.load_focused_workspace(mouse_follows_focus)?;
            }
        }
    }

    Ok(())
}
