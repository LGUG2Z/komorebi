//! The channel that carries [`WindowManagerEvent`]s from the backend to the window manager.
//!
//! The backend raises events on its own threads, as the compositor reports them, and calls
//! [`send_notification`]. The window manager takes them, one at a time and in order, on the thread
//! [`listen_for_events`](crate::process_event::listen_for_events) starts, from the receiver it was
//! made with ([`event_rx`]). The channel is bounded: an event that does not fit is dropped and
//! logged, so a backend never waits for the window manager. At most one step of a drag
//! ([`WindowManagerEvent::ResizeStep`]) waits at a time, and the window manager says when it has
//! taken it ([`resize_step_taken`]).

use crate::window_manager_event::WindowManagerEvent;
use crossbeam_channel::Receiver;
use crossbeam_channel::Sender;
use std::sync::OnceLock;
use std::sync::atomic::AtomicBool;
use std::sync::atomic::Ordering;

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

/// Whether a `ResizeStep` is already waiting in the channel. A drag makes them as fast as the pointer
/// moves, and each says where the pointer is now, so one waiting is enough: the window manager asks
/// the window where its edges are when it gets to it, and that is the newest.
static RESIZE_STEP_WAITING: AtomicBool = AtomicBool::new(false);

/// Says that the window manager has taken the `ResizeStep` that was waiting, so that the next one
/// may be queued.
pub fn resize_step_taken() {
    RESIZE_STEP_WAITING.store(false, Ordering::SeqCst);
}

/// Queues an event for the window manager, or drops it with a warning if 20 are already waiting.
///
/// A [`WindowManagerEvent::ResizeStep`] is dropped without a word while another one is waiting.
pub fn send_notification(notification: WindowManagerEvent) {
    let is_step = matches!(notification, WindowManagerEvent::ResizeStep(..));

    if is_step && RESIZE_STEP_WAITING.swap(true, Ordering::SeqCst) {
        return;
    }

    if event_tx().try_send(notification).is_err() {
        if is_step {
            RESIZE_STEP_WAITING.store(false, Ordering::SeqCst);
        }
        tracing::warn!("channel is full; dropping notification")
    }
}
