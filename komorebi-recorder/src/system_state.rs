//! Whether the user's session is locked or switched away.
//!
//! Locking the screen and fast user switching change what the capture API can see and can kill
//! its streams. We cannot subscribe to those transitions without a run loop, but the session
//! state is cheap to read (see [`crate::platform::session`]), so the recorder polls it and logs
//! each transition as an incident, next to whatever the video and the logs did at the same moment.
//!
//! | Item | Does |
//! |---|---|
//! | [`SessionState`] | what the session looks like at one moment: locked or not, at the console or not |
//! | [`read`] | reads it now, from the OS (see [`crate::platform::session`]) |
//! | [`SessionTracker`] | compares successive readings, and turns each change into an incident |
//!
//! The host decides how the state is read: from the CoreGraphics session dictionary on macOS, and
//! on Windows from whether the input desktop can be opened (the lock screen runs on a desktop that
//! an ordinary process may not open) and from which session is at the console. The tracker is the
//! same on both. On Windows a power watcher also reports a lock as it happens (see
//! [`crate::power::PowerKind::SessionLocked`]), which only makes the poll happen sooner: the
//! incident is still written here, once, when the poll sees the change.
//!
//! # Example
//!
//! ```rust
//! use komorebi_recorder::incidents::kind;
//! use komorebi_recorder::system_state::{SessionState, SessionTracker};
//!
//! let mut tracker = SessionTracker::default();
//! assert!(tracker.capturable());
//!
//! // the screen locks: one incident, and the session cannot be captured
//! let drafts = tracker.observe(SessionState { locked: true, on_console: true });
//! assert_eq!(drafts.len(), 1);
//! assert_eq!(drafts[0].kind, kind::SCREEN_LOCKED);
//! assert!(!tracker.capturable());
//!
//! // still locked: nothing new to say
//! assert!(tracker.observe(SessionState { locked: true, on_console: true }).is_empty());
//! ```

use crate::incidents::Draft;
use crate::incidents::kind;

/// The state of the user's session at one moment: whether it is locked, and whether it is the one
/// in front.
///
/// The default is the session as it is when nothing is wrong: unlocked and at the console. That
/// is also what [`read`] falls back to where the host gives no answer.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SessionState {
    /// Whether the screen is locked. On Windows this is also true while a prompt on the secure
    /// desktop is up (a UAC prompt reads the same way): the screen cannot be captured then either.
    pub locked: bool,
    /// `false` while another user's session is in front (fast user switching).
    pub on_console: bool,
}

impl Default for SessionState {
    fn default() -> Self {
        Self {
            locked: false,
            on_console: true,
        }
    }
}

/// Returns the current session state, or the default (unlocked, on console) if it cannot be read.
///
/// This asks the OS each time ([`crate::platform::session`]): it is cheap, needs no permission,
/// and never fails. It reflects the machine it is run on, so a test cannot assume either state;
/// the test `reading_the_real_session_works_in_any_state` only checks that the reading and a
/// [`SessionTracker`] agree.
#[must_use]
pub fn read() -> SessionState {
    crate::platform::session::read()
}

/// Turns successive [`SessionState`]s into incidents.
///
/// A state machine whose only state is the last [`SessionState`] it saw, which starts as the
/// default (unlocked, at the console). Each change from it is reported once.
///
/// # Example: both flags change at once
///
/// ```rust
/// use komorebi_recorder::incidents::kind;
/// use komorebi_recorder::system_state::{SessionState, SessionTracker};
///
/// let mut tracker = SessionTracker::default();
///
/// // fast user switching away: another user's session is in front
/// let drafts = tracker.observe(SessionState { locked: false, on_console: false });
/// assert_eq!(drafts[0].kind, kind::SESSION_INACTIVE);
/// assert!(!tracker.capturable());
///
/// // this user comes back, to a locked screen: both changes are reported, the lock first
/// let drafts = tracker.observe(SessionState { locked: true, on_console: true });
/// let kinds: Vec<_> = drafts.iter().map(|draft| draft.kind).collect();
/// assert_eq!(kinds, [kind::SCREEN_LOCKED, kind::SESSION_ACTIVE]);
/// ```
#[derive(Debug, Default)]
pub struct SessionTracker {
    last: SessionState,
}

impl SessionTracker {
    /// Compares `now` with the last state seen, makes `now` the last state, and returns an
    /// incident for each change.
    ///
    /// A change of `locked` gives [`screen-locked`](kind::SCREEN_LOCKED) or
    /// [`screen-unlocked`](kind::SCREEN_UNLOCKED), and a change of `on_console` gives
    /// [`session-active`](kind::SESSION_ACTIVE) or
    /// [`session-inactive`](kind::SESSION_INACTIVE); when both change, the lock comes first. So
    /// the result has at most two drafts, and none while the state stays the same. The drafts are
    /// undated: they are dated when they are recorded, which is the poll that saw the change.
    ///
    /// The first observation is compared with the default state. A recording that begins with the
    /// screen locked sees a `screen-locked` incident from it unless the caller makes a first
    /// observation to set the starting state and drops its drafts, as the recorder does.
    pub fn observe(&mut self, now: SessionState) -> Vec<Draft> {
        let mut drafts = Vec::new();

        if now.locked != self.last.locked {
            drafts.push(if now.locked {
                Draft::new(
                    kind::SCREEN_LOCKED,
                    "the screen was locked; video may pause or restart until it is unlocked",
                )
            } else {
                Draft::new(kind::SCREEN_UNLOCKED, "the screen was unlocked")
            });
        }

        if now.on_console != self.last.on_console {
            drafts.push(if now.on_console {
                Draft::new(
                    kind::SESSION_ACTIVE,
                    "this user's session is in front again",
                )
            } else {
                Draft::new(
                    kind::SESSION_INACTIVE,
                    "another user's session is in front; this one is in the background",
                )
            });
        }

        self.last = now;
        drafts
    }

    /// Returns true if and only if the session can currently be captured at all: the last state
    /// seen was unlocked and at the console.
    #[must_use]
    pub fn capturable(&self) -> bool {
        !self.last.locked && self.last.on_console
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn state(locked: bool, on_console: bool) -> SessionState {
        SessionState { locked, on_console }
    }

    #[test]
    fn transitions_are_reported_once_each() {
        let mut tracker = SessionTracker::default();
        assert!(tracker.observe(state(false, true)).is_empty());

        let drafts = tracker.observe(state(true, true));
        assert_eq!(drafts.len(), 1);
        assert_eq!(drafts[0].kind, kind::SCREEN_LOCKED);
        assert!(!tracker.capturable());

        assert!(
            tracker.observe(state(true, true)).is_empty(),
            "no repeat while it stays locked"
        );

        let drafts = tracker.observe(state(false, true));
        assert_eq!(drafts[0].kind, kind::SCREEN_UNLOCKED);
        assert!(tracker.capturable());
    }

    #[test]
    fn fast_user_switching_is_tracked_separately_from_locking() {
        let mut tracker = SessionTracker::default();
        let drafts = tracker.observe(state(false, false));
        assert_eq!(drafts.len(), 1);
        assert_eq!(drafts[0].kind, kind::SESSION_INACTIVE);
        assert!(!tracker.capturable());

        // both change at once: both are reported
        let drafts = tracker.observe(state(true, true));
        let kinds: Vec<_> = drafts.iter().map(|draft| draft.kind).collect();
        assert_eq!(kinds, vec![kind::SCREEN_LOCKED, kind::SESSION_ACTIVE]);
    }

    /// Against the real session: it must be readable, whatever state it is in. Whether the screen
    /// happens to be locked while the tests run (a developer who stepped away, a CI machine under
    /// a screensaver) is not something to assert on.
    #[test]
    fn reading_the_real_session_works_in_any_state() {
        let state = read();

        let mut tracker = SessionTracker::default();
        let drafts = tracker.observe(state);

        // the tracker agrees with what was read, and reports at most one change per flag
        assert_eq!(tracker.capturable(), !state.locked && state.on_console);
        assert!(drafts.len() <= 2);
    }
}
