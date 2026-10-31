//! A guard against floods of move and resize notifications
//!
//! An app that is moving its own windows, or is being moved faster than it can report, can send a
//! notification for every frame, and the window manager's event channel is small. The notifications
//! that matter while the mouse button is up say nothing a second one a moment later would not, so
//! only the first in any short interval is let through.

use super::tracks::WindowId;
use std::collections::HashMap;
use std::time::Duration;
use std::time::Instant;

/// The shortest interval between two of the same notification for the same window
pub const MIN_INTERVAL: Duration = Duration::from_millis(100);

/// Entries older than this are forgotten
const TTL: Duration = Duration::from_secs(1);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Kind {
    Moved,
    Resized,
}

#[derive(Debug, Default)]
pub struct Throttle {
    last: HashMap<(WindowId, Kind), Instant>,
    last_prune: Option<Instant>,
}

impl Throttle {
    /// Whether a notification may be let through, recording that it was if so
    pub fn admit(&mut self, window_id: WindowId, kind: Kind, now: Instant) -> bool {
        self.prune(now);

        match self.last.get(&(window_id, kind)) {
            Some(last) if now.saturating_duration_since(*last) < MIN_INTERVAL => false,
            _ => {
                self.last.insert((window_id, kind), now);
                true
            }
        }
    }

    fn prune(&mut self, now: Instant) {
        if self
            .last_prune
            .is_some_and(|pruned| now.saturating_duration_since(pruned) < TTL)
        {
            return;
        }

        self.last_prune = Some(now);
        self.last
            .retain(|_, last| now.saturating_duration_since(*last) < TTL);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_first_notification_is_let_through_and_a_burst_after_it_is_not() {
        let now = Instant::now();
        let mut throttle = Throttle::default();

        assert!(throttle.admit(1, Kind::Moved, now));
        assert!(!throttle.admit(1, Kind::Moved, now + Duration::from_millis(10)));
        assert!(!throttle.admit(1, Kind::Moved, now + Duration::from_millis(99)));
        assert!(throttle.admit(1, Kind::Moved, now + MIN_INTERVAL));
    }

    #[test]
    fn dropped_notifications_do_not_extend_the_interval() {
        let now = Instant::now();
        let mut throttle = Throttle::default();

        assert!(throttle.admit(1, Kind::Moved, now));

        // a steady stream still gets one through every interval
        let mut admitted = 0;
        for millis in 1..=200 {
            if throttle.admit(1, Kind::Moved, now + Duration::from_millis(millis)) {
                admitted += 1;
            }
        }

        assert_eq!(admitted, 2);
    }

    #[test]
    fn windows_and_kinds_are_throttled_separately() {
        let now = Instant::now();
        let mut throttle = Throttle::default();

        assert!(throttle.admit(1, Kind::Moved, now));
        assert!(throttle.admit(2, Kind::Moved, now));
        assert!(throttle.admit(1, Kind::Resized, now));
        assert!(!throttle.admit(1, Kind::Moved, now));
    }

    #[test]
    fn old_entries_are_forgotten() {
        let now = Instant::now();
        let mut throttle = Throttle::default();

        throttle.admit(1, Kind::Moved, now);
        throttle.admit(2, Kind::Moved, now + Duration::from_secs(5));

        assert_eq!(throttle.last.len(), 1);
    }
}
