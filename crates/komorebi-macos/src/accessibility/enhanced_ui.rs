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
use komorebi_macos_core::enhanced_ui::Flag;
use komorebi_macos_core::enhanced_ui::State;
use parking_lot::Mutex;
use std::collections::HashMap;
use std::sync::Arc;
use std::sync::LazyLock;
use std::time::Instant;

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
