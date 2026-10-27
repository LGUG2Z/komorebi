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
//! | [`inspect`] | what Windows says about any window, for the per-event probe |
//!
//! This module is not part of the facade: [`crate::platform::window`], which is shared by every
//! host, calls it. Nothing tests it automatically; it acts on real windows, and is checked on a
//! real machine (see `docs/dev/windows-checkpoints.md`).

use crate::sources::winprobe::WindowFacts;
use std::ffi::c_void;
use windows::Win32::Foundation::HWND;
use windows::Win32::Foundation::LPARAM;
use windows::Win32::Foundation::RECT;
use windows::Win32::Graphics::Dwm::DWMWA_CLOAKED;
use windows::Win32::Graphics::Dwm::DWMWA_EXTENDED_FRAME_BOUNDS;
use windows::Win32::Graphics::Dwm::DwmGetWindowAttribute;
use windows::Win32::System::Threading::GetCurrentProcessId;
use windows::Win32::UI::WindowsAndMessaging::EnumWindows;
use windows::Win32::UI::WindowsAndMessaging::GetWindowRect;
use windows::Win32::UI::WindowsAndMessaging::GetWindowThreadProcessId;
use windows::Win32::UI::WindowsAndMessaging::IsIconic;
use windows::Win32::UI::WindowsAndMessaging::IsWindow;
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

/// Returns what Windows says about the window with handle `hwnd` right now: whether it exists, is
/// visible, cloaked or minimized, and where it is. The rect is the visible frame
/// (`DWMWA_EXTENDED_FRAME_BOUNDS`, or `GetWindowRect` if that cannot be read), which is how
/// komorebi reads one, so the two can be compared.
///
/// This is always `Some`: a handle that names no window is a [`WindowFacts`] with `exists` false
/// and nothing else read. `at_ns` and `komorebi_says` are left for the caller to fill in. It is
/// reached through [`crate::platform::window::inspect`].
#[must_use]
pub fn inspect(hwnd: u32) -> Option<WindowFacts> {
    let window = HWND(hwnd as usize as *mut c_void);
    let mut facts = WindowFacts {
        hwnd,
        at_ns: 0,
        exists: false,
        visible: false,
        cloaked: false,
        minimized: false,
        rect: None,
        komorebi_says: None,
    };

    // SAFETY: queries about a handle that may be stale, which they report as a failure; every out
    // pointer is valid for its call
    unsafe {
        if !IsWindow(Some(window)).as_bool() {
            return Some(facts);
        }

        facts.exists = true;
        facts.visible = IsWindowVisible(window).as_bool();
        facts.minimized = IsIconic(window).as_bool();

        // non-zero for a window cloaked by the shell (another virtual desktop), by its application,
        // or by komorebi
        let mut cloaked = 0_u32;
        let _ = DwmGetWindowAttribute(
            window,
            DWMWA_CLOAKED,
            (&raw mut cloaked).cast(),
            size_of::<u32>() as u32,
        );
        facts.cloaked = cloaked != 0;

        let mut frame = RECT::default();
        let framed = DwmGetWindowAttribute(
            window,
            DWMWA_EXTENDED_FRAME_BOUNDS,
            (&raw mut frame).cast(),
            size_of::<RECT>() as u32,
        )
        .is_ok();
        if framed || GetWindowRect(window, &raw mut frame).is_ok() {
            facts.rect = Some([frame.left, frame.top, frame.right, frame.bottom]);
        }
    }

    Some(facts)
}
