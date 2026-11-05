//! Whether the session is locked, and whether it is the one at the console, from the CoreGraphics
//! session dictionary.
//!
//! Locking the screen and fast user switching change what the capture API can see and can kill its
//! streams. [`read`] is cheap enough to poll, which is how the recorder notices (see
//! [`crate::system_state`]). The same function exists on Windows, which reads the state from the
//! input desktop.

use crate::system_state::SessionState;
use objc2_core_foundation::CFBoolean;
use objc2_core_foundation::CFDictionary;
use objc2_core_foundation::CFString;
use objc2_core_graphics::CGSessionCopyCurrentDictionary;
use std::ptr::NonNull;

fn flag(dictionary: &CFDictionary, key: &'static str) -> Option<bool> {
    let key = CFString::from_static_str(key);
    // SAFETY: `key` is a valid CFString and the dictionary is live
    let value = unsafe { CFDictionary::value(dictionary, NonNull::from(&*key).as_ptr().cast()) };
    NonNull::new(value.cast_mut()).map(|value| {
        // SAFETY: the session dictionary stores these flags as CFBoolean
        unsafe { value.cast::<CFBoolean>().as_ref() }.as_bool()
    })
}

/// Returns the current session state, or the default (unlocked, on console) if it cannot be read.
///
/// `locked` is the session dictionary's `CGSSessionScreenIsLocked` and `on_console` its
/// `kCGSSessionOnConsoleKey`; a flag the dictionary does not carry reads as unlocked, or on the
/// console. Never fails.
///
/// This reads the machine it runs on, so it has no doctest. It is covered by
/// `reading_the_real_session_works_in_any_state` in the unit tests of [`crate::system_state`].
#[must_use]
pub fn read() -> SessionState {
    let Some(dictionary) = CGSessionCopyCurrentDictionary() else {
        return SessionState::default();
    };

    SessionState {
        locked: flag(&dictionary, "CGSSessionScreenIsLocked").unwrap_or(false),
        on_console: flag(&dictionary, "kCGSSessionOnConsoleKey").unwrap_or(true),
    }
}
