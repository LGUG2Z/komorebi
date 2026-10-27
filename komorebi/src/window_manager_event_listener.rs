//! The channel that carries [`WindowManagerEvent`]s from the backend to the window manager.
//!
//! The backend raises events on its own threads, as the compositor reports them, and calls
//! [`send_notification`]. The window manager takes them, one at a time and in order, on the thread
//! [`listen_for_events`](crate::process_event::listen_for_events) starts, from the receiver it was
//! made with ([`event_rx`]). The channel is bounded: an event that does not fit is dropped and
//! logged, so a backend never waits for the window manager.

use crate::window_manager_event::WindowManagerEvent;
use crossbeam_channel::Receiver;
use crossbeam_channel::Sender;
use std::sync::OnceLock;

static CHANNEL: OnceLock<(Sender<WindowManagerEvent>, Receiver<WindowManagerEvent>)> =
    OnceLock::new();

fn channel() -> &'static (Sender<WindowManagerEvent>, Receiver<WindowManagerEvent>) {
    CHANNEL.get_or_init(|| crossbeam_channel::bounded(20))
}

fn event_tx() -> Sender<WindowManagerEvent> {
    channel().0.clone()
}

/// Returns the receiving end of the channel, for the window manager.
pub fn event_rx() -> Receiver<WindowManagerEvent> {
    channel().1.clone()
}

/// Queues an event for the window manager, or drops it with a warning if 20 are already waiting.
pub fn send_notification(notification: WindowManagerEvent) {
    if event_tx().try_send(notification).is_err() {
        tracing::warn!("channel is full; dropping notification")
    }
}
