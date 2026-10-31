use crate::window_manager_event::WindowManagerEvent;
use crossbeam_channel::Receiver;
use crossbeam_channel::Sender;
use std::sync::OnceLock;
use std::sync::atomic::AtomicBool;
use std::sync::atomic::Ordering;

static CHANNEL: OnceLock<(Sender<WindowManagerEvent>, Receiver<WindowManagerEvent>)> =
    OnceLock::new();

/// Whether a step of a drag is waiting in the channel for the window manager to take it
static RESIZE_STEP_WAITING: AtomicBool = AtomicBool::new(false);

fn channel() -> &'static (Sender<WindowManagerEvent>, Receiver<WindowManagerEvent>) {
    CHANNEL.get_or_init(|| crossbeam_channel::bounded(20))
}

fn event_tx() -> Sender<WindowManagerEvent> {
    channel().0.clone()
}

pub fn event_rx() -> Receiver<WindowManagerEvent> {
    channel().1.clone()
}

/// Whether a step of a drag may be sent, which it may if none is waiting. Says that one is.
///
/// A step is only a signal that the window has moved, and what is done about it is to look at where
/// the window is, which is the newest of what any number of them would have said. So one that waits
/// is enough, and the channel is not filled with the ones that come faster than they are handled.
fn claim_resize_step(waiting: &AtomicBool) -> bool {
    !waiting.swap(true, Ordering::AcqRel)
}

fn release_resize_step(waiting: &AtomicBool) {
    waiting.store(false, Ordering::Release);
}

/// Say that a step of a drag is being taken, so the next one that comes is let through.
///
/// This is for the start of a step and not the end of it: one that comes while a step is being
/// taken waits for the next, and the last frame of a drag is never lost.
pub fn resize_step_taken() {
    release_resize_step(&RESIZE_STEP_WAITING);
}

pub fn send_notification(notification: WindowManagerEvent) {
    let is_step = matches!(notification, WindowManagerEvent::ResizeStep(_, _, _));

    if is_step && !claim_resize_step(&RESIZE_STEP_WAITING) {
        return;
    }

    if event_tx().try_send(notification).is_err() {
        // there is nothing in the channel for a step that was not sent to wait for
        if is_step {
            resize_step_taken();
        }

        tracing::warn!("channel is full; dropping notification")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_step_is_let_through_when_none_is_waiting() {
        let waiting = AtomicBool::new(false);

        assert!(claim_resize_step(&waiting));
    }

    #[test]
    fn only_one_step_waits_however_many_come() {
        let waiting = AtomicBool::new(false);

        assert!(claim_resize_step(&waiting));
        assert!(!claim_resize_step(&waiting));
        assert!(!claim_resize_step(&waiting));
    }

    #[test]
    fn a_step_that_comes_while_one_is_being_taken_is_let_through_for_after() {
        let waiting = AtomicBool::new(false);
        assert!(claim_resize_step(&waiting));

        // the handler says that it has begun, and what comes after is the next one
        release_resize_step(&waiting);

        assert!(claim_resize_step(&waiting));
        assert!(!claim_resize_step(&waiting));
    }
}
