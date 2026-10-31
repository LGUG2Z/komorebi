//! Stopping apps from animating the moves and resizes we make
//!
//! When `AXEnhancedUserInterface` is on for an application, the app animates every move and resize
//! it is asked to make over AX, which turns each of our frames into a slow animation of its own. The
//! attribute belongs to the *application* element, not to a window, and it is shared by all of that
//! application's windows, so turning it off has to be reference counted per process: the last window
//! to finish puts it back.
//!
//! Almost no app has the attribute on, so the common case must cost nothing: once an app has been
//! seen not to have it, it is left alone for a while without asking again.

use crate::AccessibilityUiElement;
use crate::accessibility::private::get_enhanced_user_interface_if_present;
use crate::accessibility::private::set_enhanced_user_interface;
use parking_lot::Mutex;
use std::collections::HashMap;
use std::sync::Arc;
use std::sync::LazyLock;
use std::time::Duration;
use std::time::Instant;

/// How long an app that does not have the attribute on is left alone before it is asked again
const QUIET_PERIOD: Duration = Duration::from_secs(10);

/// The attribute, as seen by the state machine
pub trait Flag {
    /// The current value, or `None` if the app has no such attribute or did not answer
    fn read(&self) -> Option<bool>;

    /// Returns whether the write succeeded
    fn write(&self, value: bool) -> bool;
}

#[derive(Debug, Default)]
struct State {
    /// How many windows are currently relying on the attribute being off
    active: u32,
    /// The attribute may be off because of us, so it is owed back to the app
    ///
    /// Setters can change app state before they report an error, so this is set before the attribute
    /// is touched rather than after. It also stops us reading our own `false` back later and
    /// mistaking it for the app's own setting.
    restore_pending: bool,
    quiet_until: Option<Instant>,
}

impl State {
    /// Returns whether the caller now holds a reference that it has to [`State::release`]
    fn acquire(&mut self, flag: &impl Flag, now: Instant) -> bool {
        if self.active > 0 {
            self.active += 1;
            return true;
        }

        if !self.restore_pending {
            if self.quiet_until.is_some_and(|until| now < until) {
                return false;
            }

            if flag.read() != Some(true) {
                self.quiet_until = Some(now + QUIET_PERIOD);
                return false;
            }
        }

        self.restore_pending = true;
        flag.write(false);
        self.active = 1;

        true
    }

    fn release(&mut self, flag: &impl Flag) {
        self.active = self.active.saturating_sub(1);

        if self.active == 0 && self.restore_pending && flag.write(true) {
            self.restore_pending = false;
        }
    }
}

struct AxFlag(AccessibilityUiElement);

impl Flag for AxFlag {
    fn read(&self) -> Option<bool> {
        get_enhanced_user_interface_if_present(&self.0)
    }

    fn write(&self, value: bool) -> bool {
        set_enhanced_user_interface(&self.0, value).is_ok()
    }
}

static STATES: LazyLock<Mutex<HashMap<i32, Arc<Mutex<State>>>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));

/// Keeps an app's `AXEnhancedUserInterface` off for as long as it is held
pub struct EnhancedUiGuard {
    state: Arc<Mutex<State>>,
    flag: AxFlag,
}

impl Drop for EnhancedUiGuard {
    fn drop(&mut self) {
        let mut state = self.state.lock();
        state.release(&self.flag);

        if state.restore_pending {
            tracing::warn!("could not restore AXEnhancedUserInterface, will retry next time");
        }
    }
}

/// Turn `AXEnhancedUserInterface` off for the application `app` belongs to
///
/// Returns `None` when there is nothing to do, which is the case for nearly every app.
pub fn disable(pid: i32, app: &AccessibilityUiElement) -> Option<EnhancedUiGuard> {
    let state = STATES.lock().entry(pid).or_default().clone();
    let flag = AxFlag(app.clone());

    let acquired = state.lock().acquire(&flag, Instant::now());

    acquired.then_some(EnhancedUiGuard { state, flag })
}

/// Forget what is known about a process, so that a new process with the same id starts afresh
pub fn forget(pid: i32) {
    STATES.lock().remove(&pid);
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::Cell;

    struct Fake {
        value: Cell<Option<bool>>,
        reads: Cell<u32>,
        writes: Cell<u32>,
        fail_writes: Cell<bool>,
    }

    impl Fake {
        fn new(value: Option<bool>) -> Self {
            Self {
                value: Cell::new(value),
                reads: Cell::new(0),
                writes: Cell::new(0),
                fail_writes: Cell::new(false),
            }
        }
    }

    impl Flag for Fake {
        fn read(&self) -> Option<bool> {
            self.reads.set(self.reads.get() + 1);
            self.value.get()
        }

        fn write(&self, value: bool) -> bool {
            self.writes.set(self.writes.get() + 1);

            if self.fail_writes.get() {
                return false;
            }

            self.value.set(Some(value));
            true
        }
    }

    #[test]
    fn an_app_with_the_attribute_on_has_it_turned_off_and_back_on() {
        let flag = Fake::new(Some(true));
        let mut state = State::default();
        let now = Instant::now();

        assert!(state.acquire(&flag, now));
        assert_eq!(flag.value.get(), Some(false));

        state.release(&flag);
        assert_eq!(flag.value.get(), Some(true));
        assert!(!state.restore_pending);
    }

    #[test]
    fn two_windows_share_one_disable_and_one_restore() {
        let flag = Fake::new(Some(true));
        let mut state = State::default();
        let now = Instant::now();

        assert!(state.acquire(&flag, now));
        assert!(state.acquire(&flag, now));
        assert_eq!(flag.writes.get(), 1);
        assert_eq!(flag.reads.get(), 1);

        state.release(&flag);
        assert_eq!(flag.value.get(), Some(false));

        state.release(&flag);
        assert_eq!(flag.value.get(), Some(true));
        assert_eq!(flag.writes.get(), 2);
    }

    #[test]
    fn an_app_without_the_attribute_costs_nothing_after_the_first_look() {
        let flag = Fake::new(None);
        let mut state = State::default();
        let now = Instant::now();

        assert!(!state.acquire(&flag, now));
        assert!(!state.acquire(&flag, now + Duration::from_secs(1)));
        assert!(!state.acquire(&flag, now + Duration::from_secs(9)));

        assert_eq!(flag.reads.get(), 1);
        assert_eq!(flag.writes.get(), 0);
    }

    #[test]
    fn an_app_with_the_attribute_off_is_left_alone() {
        let flag = Fake::new(Some(false));
        let mut state = State::default();

        assert!(!state.acquire(&flag, Instant::now()));
        assert_eq!(flag.writes.get(), 0);
    }

    #[test]
    fn an_app_is_asked_again_after_the_quiet_period() {
        let flag = Fake::new(Some(false));
        let mut state = State::default();
        let now = Instant::now();

        assert!(!state.acquire(&flag, now));

        // somebody turned on VoiceOver in the meantime
        flag.value.set(Some(true));
        assert!(!state.acquire(&flag, now + QUIET_PERIOD - Duration::from_secs(1)));
        assert!(state.acquire(&flag, now + QUIET_PERIOD + Duration::from_secs(1)));
        assert_eq!(flag.value.get(), Some(false));
    }

    #[test]
    fn a_failed_restore_is_owed_and_never_mistaken_for_the_apps_own_setting() {
        let flag = Fake::new(Some(true));
        let mut state = State::default();
        let now = Instant::now();

        assert!(state.acquire(&flag, now));

        flag.fail_writes.set(true);
        state.release(&flag);

        assert!(state.restore_pending);
        assert_eq!(flag.value.get(), Some(false));

        // the next window must not read the false we left behind and conclude the app never had it on
        flag.fail_writes.set(false);
        let reads = flag.reads.get();
        assert!(state.acquire(&flag, now + Duration::from_secs(1)));
        assert_eq!(flag.reads.get(), reads);

        state.release(&flag);
        assert_eq!(flag.value.get(), Some(true));
        assert!(!state.restore_pending);
    }

    #[test]
    fn a_failed_disable_is_still_owed_back() {
        let flag = Fake::new(Some(true));
        flag.fail_writes.set(true);
        let mut state = State::default();

        // the setter may have changed the app before it reported the error
        assert!(state.acquire(&flag, Instant::now()));
        assert!(state.restore_pending);

        flag.fail_writes.set(false);
        state.release(&flag);

        assert_eq!(flag.value.get(), Some(true));
        assert!(!state.restore_pending);
    }

    #[test]
    fn releasing_more_than_was_acquired_is_harmless() {
        let flag = Fake::new(Some(true));
        let mut state = State::default();

        state.release(&flag);
        assert_eq!(flag.writes.get(), 0);
        assert_eq!(state.active, 0);
    }
}
