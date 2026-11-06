use std::collections::VecDeque;

/// How far back a gesture is looked at to tell how fast it is going, in seconds.
const HISTORY_SECONDS: f64 = 0.150;

/// Works out how fast a gesture is going, from the last 150 milliseconds of it.
///
/// A host gives it every movement of a finger or a wheel with when it happened, and asks how fast
/// the gesture is going when it lets go. It never looks at a clock, so what it says is the same
/// for the same movements whenever they are given.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct SwipeTracker {
    /// When each movement happened, in seconds, and how far it went.
    history: VecDeque<(f64, f64)>,
}

impl SwipeTracker {
    /// A tracker that has seen nothing.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Forget everything, for the start of a gesture.
    pub fn reset(&mut self) {
        self.history.clear();
    }

    /// Say that the gesture moved by `delta` at `time`, in seconds. The times only have to be in
    /// order and in the same unit as each other.
    pub fn push(&mut self, time: f64, delta: f64) {
        self.history.push_back((time, delta));

        while let Some((oldest, _)) = self.history.front() {
            if time - oldest > HISTORY_SECONDS {
                self.history.pop_front();
            } else {
                break;
            }
        }
    }

    /// How fast the gesture is going, in the unit of the deltas per second, which is the distance
    /// covered from the first movement that is looked at to the last, over the time that it took.
    ///
    /// Nothing is moving as far as can be told from fewer than two movements, or from movements
    /// that all happened at the same time.
    #[must_use]
    pub fn velocity(&self) -> f64 {
        let (Some((first, _)), Some((last, _))) = (self.history.front(), self.history.back())
        else {
            return 0.0;
        };

        let span = last - first;
        if span <= 0.0 {
            return 0.0;
        }

        // the first movement is where the span starts, and the distance is what happened after it
        let distance: f64 = self.history.iter().skip(1).map(|(_, delta)| delta).sum();

        distance / span
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_steady_gesture_goes_at_the_speed_that_it_goes() {
        let mut tracker = SwipeTracker::new();

        // 10 units every 10 milliseconds is 1000 a second
        for step in 0..10 {
            tracker.push(f64::from(step) * 0.010, 10.0);
        }

        assert!((tracker.velocity() - 1000.0).abs() < 1e-9);
    }

    #[test]
    fn what_is_too_old_is_not_looked_at() {
        let mut tracker = SwipeTracker::new();

        // fast, and then a pause, and then slow
        for step in 0..5 {
            tracker.push(f64::from(step) * 0.010, 100.0);
        }
        for step in 0..5 {
            tracker.push(1.0 + f64::from(step) * 0.010, 1.0);
        }

        assert!((tracker.velocity() - 100.0).abs() < 1e-9);
    }

    #[test]
    fn it_goes_the_other_way_when_the_gesture_does() {
        let mut tracker = SwipeTracker::new();
        for step in 0..4 {
            tracker.push(f64::from(step) * 0.010, -5.0);
        }

        assert!(tracker.velocity() < 0.0);
    }

    #[test]
    fn not_enough_to_tell_is_not_moving() {
        let mut tracker = SwipeTracker::new();
        assert_eq!(tracker.velocity(), 0.0);

        tracker.push(0.0, 50.0);
        assert_eq!(tracker.velocity(), 0.0);

        // several at once do not make a span
        tracker.push(0.0, 50.0);
        assert_eq!(tracker.velocity(), 0.0);
    }

    #[test]
    fn a_reset_forgets_the_gesture_before() {
        let mut tracker = SwipeTracker::new();
        for step in 0..4 {
            tracker.push(f64::from(step) * 0.010, 50.0);
        }

        tracker.reset();

        assert_eq!(tracker, SwipeTracker::new());
        assert_eq!(tracker.velocity(), 0.0);
    }
}
