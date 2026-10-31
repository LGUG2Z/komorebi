use crate::LOG_TARGET;
use crossbeam_channel::Receiver;
use crossbeam_channel::Sender;
use std::sync::atomic::AtomicBool;
use std::sync::atomic::Ordering;

/// A bounded channel for the events a backend raises on its own threads, which never makes a
/// sender wait
///
/// [`send`](EventChannel::send) queues the event, or drops it with a warning when the channel is
/// full, so a callback that must not block (the callbacks of the main run loop on macOS, the
/// compositor thread on KDE) never does.
///
/// A channel made with [`coalescing`](EventChannel::coalescing) lets at most one event for which
/// the predicate is true wait at a time (the steps of a drag: each says where the pointer is now,
/// so one waiting is enough). The consumer says that it has taken that event with
/// [`coalesced_taken`](EventChannel::coalesced_taken), at the start of handling it and not at the
/// end, so that what comes while it is handled waits for the next round and the last step of a
/// drag is never lost.
///
/// The channel remembers that an event was dropped because it was full
/// ([`take_overflow`](EventChannel::take_overflow)). Nothing reads that yet.
pub struct EventChannel<E> {
    tx: Sender<E>,
    rx: Receiver<E>,
    coalesce: Option<fn(&E) -> bool>,
    coalesced_waiting: AtomicBool,
    overflowed: AtomicBool,
}

impl<E> EventChannel<E> {
    /// A channel that holds `capacity` events and coalesces none
    #[must_use]
    pub fn bounded(capacity: usize) -> Self {
        let (tx, rx) = crossbeam_channel::bounded(capacity);
        Self {
            tx,
            rx,
            coalesce: None,
            coalesced_waiting: AtomicBool::new(false),
            overflowed: AtomicBool::new(false),
        }
    }

    /// A channel that holds `capacity` events, and lets one event for which `is_coalesced` is
    /// true wait at a time
    #[must_use]
    pub fn coalescing(capacity: usize, is_coalesced: fn(&E) -> bool) -> Self {
        Self {
            coalesce: Some(is_coalesced),
            ..Self::bounded(capacity)
        }
    }

    /// The receiving end, for the consumer (a handle on the same channel each time)
    #[must_use]
    pub fn receiver(&self) -> Receiver<E> {
        self.rx.clone()
    }

    /// Queues `event`, or drops it: without a word while another coalesced event is waiting, with
    /// a warning when the channel is full
    pub fn send(&self, event: E) {
        let is_coalesced = self
            .coalesce
            .is_some_and(|is_coalesced| is_coalesced(&event));

        if is_coalesced && self.coalesced_waiting.swap(true, Ordering::SeqCst) {
            return;
        }

        if self.tx.try_send(event).is_err() {
            // there is nothing in the channel for an event that was not sent to wait for
            if is_coalesced {
                self.coalesced_taken();
            }

            self.overflowed.store(true, Ordering::SeqCst);
            tracing::warn!(target: LOG_TARGET, "channel is full; dropping notification")
        }
    }

    /// Says that the coalesced event that was waiting is being taken, so that the next one is let
    /// through
    pub fn coalesced_taken(&self) {
        self.coalesced_waiting.store(false, Ordering::SeqCst);
    }

    /// Whether an event was dropped because the channel was full since this was last asked
    pub fn take_overflow(&self) -> bool {
        self.overflowed.swap(false, Ordering::SeqCst)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Debug, PartialEq)]
    enum Event {
        Step(i32),
        Other(i32),
    }

    fn is_step(event: &Event) -> bool {
        matches!(event, Event::Step(_))
    }

    #[test]
    fn a_step_is_let_through_when_none_is_waiting() {
        let channel = EventChannel::coalescing(20, is_step);

        channel.send(Event::Step(1));

        assert_eq!(channel.receiver().try_recv(), Ok(Event::Step(1)));
    }

    #[test]
    fn only_one_step_waits_however_many_come() {
        let channel = EventChannel::coalescing(20, is_step);

        channel.send(Event::Step(1));
        channel.send(Event::Step(2));
        channel.send(Event::Step(3));
        channel.send(Event::Other(4));

        let rx = channel.receiver();
        assert_eq!(rx.try_recv(), Ok(Event::Step(1)));
        assert_eq!(rx.try_recv(), Ok(Event::Other(4)));
        assert!(rx.try_recv().is_err());
    }

    #[test]
    fn a_step_that_comes_while_one_is_being_taken_is_let_through_for_after() {
        let channel = EventChannel::coalescing(20, is_step);
        channel.send(Event::Step(1));
        let rx = channel.receiver();
        assert_eq!(rx.try_recv(), Ok(Event::Step(1)));

        // the consumer says that it has begun, and what comes after is the next one
        channel.coalesced_taken();

        channel.send(Event::Step(2));
        channel.send(Event::Step(3));
        assert_eq!(rx.try_recv(), Ok(Event::Step(2)));
        assert!(rx.try_recv().is_err());
    }

    #[test]
    fn without_a_predicate_every_event_is_queued() {
        let channel = EventChannel::bounded(20);

        channel.send(Event::Step(1));
        channel.send(Event::Step(2));

        assert_eq!(channel.receiver().len(), 2);
    }

    #[test]
    fn an_event_that_does_not_fit_is_dropped_and_a_dropped_step_does_not_block_the_next() {
        let channel = EventChannel::coalescing(2, is_step);
        channel.send(Event::Other(1));
        channel.send(Event::Other(2));
        assert!(!channel.take_overflow());

        // full: the step is dropped, and must not count as waiting
        channel.send(Event::Step(3));
        assert!(channel.take_overflow());
        assert!(!channel.take_overflow());

        let rx = channel.receiver();
        assert_eq!(rx.try_recv(), Ok(Event::Other(1)));
        channel.send(Event::Step(4));
        assert_eq!(rx.try_recv(), Ok(Event::Other(2)));
        assert_eq!(rx.try_recv(), Ok(Event::Step(4)));
    }
}
