#![deny(clippy::unwrap_used, clippy::expect_used)]

use crossbeam_channel::Receiver;
use crossbeam_channel::Sender;
use parking_lot::Mutex;
use std::ops::Deref;
use std::sync::Arc;
use std::sync::OnceLock;

use crate::Window;
use crate::WindowManager;

pub struct Notification(isize);

impl Deref for Notification {
    type Target = isize;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

static CHANNEL: OnceLock<(Sender<Notification>, Receiver<Notification>)> = OnceLock::new();

pub fn channel() -> &'static (Sender<Notification>, Receiver<Notification>) {
    CHANNEL.get_or_init(|| crossbeam_channel::bounded(20))
}

fn event_tx() -> Sender<Notification> {
    channel().0.clone()
}

fn event_rx() -> Receiver<Notification> {
    channel().1.clone()
}

// Currently this should only be used for async focus updates, such as
// when an animation finishes and we need to focus to set the cursor
// position if the user has mouse follows focus enabled
pub fn send_notification(hwnd: isize) {
    if event_tx().try_send(Notification(hwnd)).is_err() {
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

pub fn handle_notifications(wm: Arc<Mutex<WindowManager>>) -> color_eyre::Result<()> {
    tracing::info!("listening");

    let receiver = event_rx();

    for notification in receiver {
        let mouse_follows_focus = wm.lock().mouse_follows_focus;
        let _ = Window::from(*notification).focus(mouse_follows_focus);
    }

    Ok(())
}
