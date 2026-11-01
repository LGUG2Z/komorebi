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
use komorebi_transport::EventChannel;
use std::sync::OnceLock;

static CHANNEL: OnceLock<EventChannel<WindowManagerEvent>> = OnceLock::new();

fn channel() -> &'static EventChannel<WindowManagerEvent> {
    // a drag makes `ResizeStep`s as fast as the pointer moves, and each says where the pointer is
    // now, so one waiting is enough: the window manager asks the window where its edges are when
    // it gets to it, and that is the newest
    CHANNEL.get_or_init(|| {
        EventChannel::coalescing(20, |event| {
            matches!(event, WindowManagerEvent::ResizeStep(..))
        })
    })
}

/// Returns the receiving end of the channel, for the window manager.
pub fn event_rx() -> Receiver<WindowManagerEvent> {
    channel().receiver()
}

/// Says that the window manager has taken the `ResizeStep` that was waiting, so that the next one
/// may be queued.
pub fn resize_step_taken() {
    channel().coalesced_taken();
}

/// Queues an event for the window manager, or drops it with a warning if 20 are already waiting.
///
/// A [`WindowManagerEvent::ResizeStep`] is dropped without a word while another one is waiting.
pub fn send_notification(notification: WindowManagerEvent) {
    channel().send(notification);
}
