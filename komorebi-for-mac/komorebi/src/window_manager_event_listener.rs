use crate::window_manager_event::WindowManagerEvent;
use crossbeam_channel::Receiver;
use komorebi_transport::EventChannel;
use std::sync::OnceLock;

static CHANNEL: OnceLock<EventChannel<WindowManagerEvent>> = OnceLock::new();

fn channel() -> &'static EventChannel<WindowManagerEvent> {
    // A step of a drag is only a signal that the window has moved, and what is done about it is to
    // look at where the window is, which is the newest of what any number of them would have said.
    // So one that waits is enough, and the channel is not filled with the ones that come faster
    // than they are handled.
    CHANNEL.get_or_init(|| {
        EventChannel::coalescing(20, |event| {
            matches!(event, WindowManagerEvent::ResizeStep(_, _, _))
        })
    })
}

pub fn event_rx() -> Receiver<WindowManagerEvent> {
    channel().receiver()
}

/// Say that a step of a drag is being taken, so the next one that comes is let through.
///
/// This is for the start of a step and not the end of it: one that comes while a step is being
/// taken waits for the next, and the last frame of a drag is never lost.
pub fn resize_step_taken() {
    channel().coalesced_taken();
}

pub fn send_notification(notification: WindowManagerEvent) {
    channel().send(notification);
}
