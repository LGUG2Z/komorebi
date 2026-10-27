//! Whether the session is locked, and whether it is the one at the console.
//!
//! The lock screen runs on a desktop of its own that an ordinary process cannot open, so being
//! refused the input desktop is how a process outside it can tell. (A UAC prompt is the same kind
//! of desktop and reads the same; either way the screen cannot be captured then.) This is a poll,
//! not a notification: the power watcher ([`crate::platform::power`]) hears about lock and unlock
//! as they happen, and the recording loop reads [`read`] when it does. See
//! [`crate::system_state`] for what is done with it.
//!
//! The same function exists on macOS, which reads the CoreGraphics session dictionary.

use crate::system_state::SessionState;
use windows::Win32::System::RemoteDesktop::ProcessIdToSessionId;
use windows::Win32::System::RemoteDesktop::WTSGetActiveConsoleSessionId;
use windows::Win32::System::StationsAndDesktops::CloseDesktop;
use windows::Win32::System::StationsAndDesktops::DESKTOP_CONTROL_FLAGS;
use windows::Win32::System::StationsAndDesktops::DESKTOP_SWITCHDESKTOP;
use windows::Win32::System::StationsAndDesktops::OpenInputDesktop;
use windows::Win32::System::Threading::GetCurrentProcessId;

/// `WTSGetActiveConsoleSessionId` when no session is at the console.
const NO_SESSION: u32 = 0xFFFF_FFFF;

/// Returns the current session state, or the default (unlocked, on console) if it cannot be read.
///
/// `locked` is `true` if and only if the input desktop cannot be opened, which is also so while a
/// UAC prompt is up. `on_console` is `true` if there is no active console session, if this
/// process's session cannot be found out, or if this process is in the console session; it is
/// `false` while another user's session is in front. Never fails.
///
/// This reads the machine it runs on, so it has no doctest. It is covered by
/// `reading_the_real_session_works_in_any_state` in the unit tests of [`crate::system_state`],
/// which reads the real session on whichever host it runs on and asserts nothing about the state
/// itself: nothing checks either read against a known lock state.
#[must_use]
pub fn read() -> SessionState {
    // SAFETY: plain queries; the desktop handle is closed again
    let locked = unsafe {
        match OpenInputDesktop(DESKTOP_CONTROL_FLAGS(0), false, DESKTOP_SWITCHDESKTOP) {
            Ok(desktop) => {
                let _ = CloseDesktop(desktop);
                false
            }
            Err(_) => true,
        }
    };

    // SAFETY: plain queries with a valid out pointer
    let on_console = unsafe {
        let console = WTSGetActiveConsoleSessionId();
        let mut ours = 0_u32;
        console == NO_SESSION
            || ProcessIdToSessionId(GetCurrentProcessId(), &raw mut ours).is_err()
            || console == ours
    };

    SessionState { locked, on_console }
}
