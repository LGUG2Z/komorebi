//! What a window turned out to be once it got where it was sent.
//!
//! An app is free to refuse a size, and many do: a window that will not get narrower than some
//! width stays that wide however it is asked. A layout that does not know about this places the
//! next column as if the window had the width that it was given, and so places it on top of the
//! window that did not.
//!
//! Whatever finds out that a window is wider than it was told to be, which is the animator when a
//! journey ends and the layout when it writes a size without animating, says so here. Nothing in
//! this module waits for anything or takes the lock of the window manager, so it can be called
//! from any thread. The layout picks up what has been said the next time that it runs.

use crate::core::Rect;
use parking_lot::Mutex;
use std::collections::HashMap;
use std::sync::LazyLock;

/// How much wider than it was told to be a window can be before it counts as refusing, since
/// windows are placed in whole pixels and some apps round.
pub const TOLERANCE: i32 = 2;

/// Where a window was sent, and what it turned out to be
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Arrival {
    pub target: Rect,
    pub actual: Rect,
}

impl Arrival {
    /// How much wider than it was told to be the window is, or zero if it is not by more than the
    /// tolerance
    pub fn extra_width(&self) -> i32 {
        let extra = self.actual.right - self.target.right;

        if extra > TOLERANCE { extra } else { 0 }
    }
}

/// The latest word on every window that was not what it was told to be
static ARRIVALS: LazyLock<Mutex<HashMap<u32, Arrival>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));

/// Say what a window turned out to be when it was sent to `target`.
///
/// Only a window that did not take the width that it was given is kept. The latest word is the one
/// that counts, so a window that has taken its size since is forgotten.
pub fn report(window_id: u32, target: Rect, actual: Rect) {
    let arrival = Arrival { target, actual };

    let mut arrivals = ARRIVALS.lock();
    if arrival.extra_width() > 0 {
        arrivals.insert(window_id, arrival);
    } else {
        arrivals.remove(&window_id);
    }
}

/// What a window was last said to have turned out to be, if that was wider than it was told to be,
/// and forget it.
pub fn take(window_id: u32) -> Option<Arrival> {
    ARRIVALS.lock().remove(&window_id)
}

/// How wide a column has to be for a window that turned out as `arrival` says, given what the
/// column takes up around the window on either side, and that no column is wider than the
/// viewport.
///
/// `None` is for a window that took the size it was given.
pub fn column_width_for(arrival: &Arrival, around_window: i32, viewport_width: i32) -> Option<i32> {
    (arrival.extra_width() > 0)
        .then(|| (arrival.actual.right + 2 * around_window).min(viewport_width))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rect(width: i32) -> Rect {
        Rect {
            left: 0,
            top: 0,
            right: width,
            bottom: 800,
        }
    }

    #[test]
    fn a_window_that_took_its_size_is_not_a_refusal() {
        assert_eq!(
            column_width_for(
                &Arrival {
                    target: rect(400),
                    actual: rect(400)
                },
                10,
                1200
            ),
            None
        );

        // nor is rounding
        assert_eq!(
            column_width_for(
                &Arrival {
                    target: rect(400),
                    actual: rect(402)
                },
                10,
                1200
            ),
            None
        );

        // nor is a window that is narrower than it was told to be
        assert_eq!(
            column_width_for(
                &Arrival {
                    target: rect(400),
                    actual: rect(300)
                },
                10,
                1200
            ),
            None
        );
    }

    #[test]
    fn a_window_that_is_wider_needs_a_column_wide_enough_for_it_and_what_is_around_it() {
        let arrival = Arrival {
            target: rect(400),
            actual: rect(700),
        };

        assert_eq!(column_width_for(&arrival, 0, 1200), Some(700));
        assert_eq!(column_width_for(&arrival, 12, 1200), Some(724));
    }

    #[test]
    fn no_column_is_wider_than_the_viewport() {
        let arrival = Arrival {
            target: rect(400),
            actual: rect(1300),
        };

        assert_eq!(column_width_for(&arrival, 12, 1200), Some(1200));
    }

    #[test]
    fn the_latest_word_on_a_window_is_the_one_that_counts() {
        // ids that no other test uses, since this is shared by every test that runs at once
        let id = 4_000_001;

        report(id, rect(400), rect(700));
        assert_eq!(take(id).map(|a| a.actual.right), Some(700));
        assert_eq!(take(id), None, "it is taken once");

        report(id, rect(400), rect(700));
        report(id, rect(400), rect(401));
        assert_eq!(take(id), None, "the window took its size afterwards");

        report(id, rect(400), rect(700));
        report(id, rect(400), rect(800));
        assert_eq!(take(id).map(|a| a.actual.right), Some(800));
    }
}
