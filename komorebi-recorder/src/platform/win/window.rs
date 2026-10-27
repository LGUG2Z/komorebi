//! This program's own windows, on Windows, and what Windows says about any window.
//!
//! The recorder window and the viewer must not be in the video they are recording, and komorebi
//! must not tile them. A Mac records by application and excludes the recorder's by name; Windows
//! captures a whole monitor, so each window asks to be left out of captures
//! (`WDA_EXCLUDEFROMCAPTURE`, Windows 10 version 2004 and later; before that the only option
//! blanks the window in the video, which is worse, so it is not used).
//!
//! | item | for |
//! |---|---|
//! | [`exclude_from_capture`] | asking for this process's windows to be left out of captures |
//! | [`own_window_handles`] | the handles of those windows, as komorebi knows windows |
//!
//! This module is not part of the facade: [`crate::platform::window`], which is shared by every
//! host, calls it. Nothing tests it automatically; it acts on real windows, and is checked on a
//! real machine (see `docs/dev/windows-checkpoints.md`).

use std::ffi::c_void;
use windows::Win32::Foundation::HWND;
use windows::Win32::Foundation::LPARAM;
use windows::Win32::System::Threading::GetCurrentProcessId;
use windows::Win32::UI::WindowsAndMessaging::EnumWindows;
use windows::Win32::UI::WindowsAndMessaging::GetWindowThreadProcessId;
use windows::Win32::UI::WindowsAndMessaging::IsWindowVisible;
use windows::Win32::UI::WindowsAndMessaging::SetWindowDisplayAffinity;
use windows::Win32::UI::WindowsAndMessaging::WDA_EXCLUDEFROMCAPTURE;
use windows::core::BOOL;

unsafe extern "system" fn collect(window: HWND, found: LPARAM) -> BOOL {
    // SAFETY: `found` is the address of the vector `own_windows` passed to `EnumWindows`
    let found = unsafe { &mut *(found.0 as *mut Vec<isize>) };
    let mut pid = 0_u32;
    // SAFETY: `window` is a handle `EnumWindows` just gave us
    unsafe { GetWindowThreadProcessId(window, Some(&raw mut pid)) };
    // SAFETY: no preconditions
    if pid == unsafe { GetCurrentProcessId() } && unsafe { IsWindowVisible(window) }.as_bool() {
        found.push(window.0 as isize);
    }

    BOOL(1)
}

fn own_windows() -> Vec<isize> {
    let mut found: Vec<isize> = Vec::new();
    // SAFETY: `found` outlives the call and `collect` only touches it during it
    let _ = unsafe { EnumWindows(Some(collect), LPARAM(&raw mut found as isize)) };
    found
}

/// Returns the handles of this process's visible top-level windows, as komorebi knows windows: the
/// window handle as a `u32`, the number komorebi for Windows calls a window's `hwnd`.
#[must_use]
pub fn own_window_handles() -> Vec<u32> {
    own_windows()
        .into_iter()
        .map(|window| window as usize as u32)
        .collect()
}

/// Asks for each of this process's visible top-level windows to be left out of screen captures
/// (`WDA_EXCLUDEFROMCAPTURE`), and returns how many accepted. None accept on a Windows too old for
/// it (before Windows 10 version 2004). A window created after the call is not covered until it is
/// called again.
pub fn exclude_from_capture() -> usize {
    own_windows()
        .into_iter()
        .filter(|window| {
            // SAFETY: a window of this process, which is the only kind this may be called for
            unsafe {
                SetWindowDisplayAffinity(HWND(*window as *mut c_void), WDA_EXCLUDEFROMCAPTURE)
            }
            .is_ok()
        })
        .count()
}
