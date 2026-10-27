//! What this host can capture: the displays, the permission to capture them, the capture backend,
//! and the duration of a recorded video.
//!
//! Screen capture on Windows is Windows.Graphics.Capture into a Media Foundation writer; the code
//! lives in [`crate::capture::wgc`] and [`Backend`] is the host-neutral name for it. What this
//! module itself does is list the monitors and read their scale.
//!
//! | item | on Windows |
//! |---|---|
//! | [`supported`] | always `true` |
//! | [`has_permission`] | always `true`: there is nothing to grant |
//! | [`ensure_permission`] | always succeeds |
//! | [`active_displays`] | the monitors `EnumDisplayMonitors` lists, in physical pixels |
//! | [`main_display_id`], [`main_display`] | the primary monitor |
//! | [`media_duration_s`] | the duration of a video file, from Media Foundation |
//! | [`Backend`] | [`WgcBackend`](crate::capture::wgc::WgcBackend), the [`CaptureBackend`](crate::capture::CaptureBackend) that records with Windows.Graphics.Capture |
//!
//! The same items exist on macOS, which needs the Screen Recording permission and reports displays
//! in points. Windows-only here: [`become_per_monitor_dpi_aware`] and [`monitor_handle`].

use crate::capture::DisplayDescriptor;
use color_eyre::eyre;
use std::path::Path;
use windows::Win32::Foundation::LPARAM;
use windows::Win32::Foundation::RECT;
use windows::Win32::Graphics::Gdi::EnumDisplayMonitors;
use windows::Win32::Graphics::Gdi::GetMonitorInfoW;
use windows::Win32::Graphics::Gdi::HDC;
use windows::Win32::Graphics::Gdi::HMONITOR;
use windows::Win32::Graphics::Gdi::MONITORINFO;
use windows::Win32::UI::HiDpi::DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2;
use windows::Win32::UI::HiDpi::GetDpiForMonitor;
use windows::Win32::UI::HiDpi::MDT_EFFECTIVE_DPI;
use windows::Win32::UI::HiDpi::SetProcessDpiAwarenessContext;
use windows::core::BOOL;

/// `MONITORINFO::dwFlags` for the primary monitor.
const MONITORINFOF_PRIMARY: u32 = 1;

/// Windows reports 96 dpi for a display at 100 %.
const BASE_DPI: f64 = 96.0;

/// Makes this process per-monitor DPI aware (v2), so that every size and position it reads is in
/// physical pixels, the same as komorebi's. Fails harmlessly when the process already is.
///
/// This must run before the process creates a window or reads a size: `platform::init` calls it
/// first thing in `main`, on Windows.
pub fn become_per_monitor_dpi_aware() {
    // SAFETY: no pointers involved
    let _ = unsafe { SetProcessDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2) };
}

/// Returns `true` if and only if this host can capture the screen. It is always `true` on Windows
/// (and on macOS), and `false` on a host that can only investigate a session.
#[must_use]
pub fn supported() -> bool {
    true
}

/// Returns `true` if and only if this process may capture the screen. It always may.
///
/// There is nothing to grant, because Windows lets a process capture the screen without asking.
/// macOS needs the Screen Recording permission.
#[must_use]
pub fn has_permission() -> bool {
    true
}

/// Makes sure this process may capture the screen, asking the user if it must. On Windows it
/// needs nothing, so this always succeeds and asks nothing.
///
/// # Errors
///
/// Never fails on this host. (On macOS it fails until the Screen Recording permission is granted.)
pub fn ensure_permission() -> eyre::Result<()> {
    Ok(())
}

struct Found {
    monitor: HMONITOR,
    /// Left, top, right, bottom in virtual-screen pixels.
    rect: [i32; 4],
    primary: bool,
}

unsafe extern "system" fn collect(
    monitor: HMONITOR,
    _context: HDC,
    _rect: *mut RECT,
    found: LPARAM,
) -> BOOL {
    // SAFETY: `found` is the address of the vector `monitors` passed to `EnumDisplayMonitors`
    let found = unsafe { &mut *(found.0 as *mut Vec<Found>) };
    let mut info = MONITORINFO {
        cbSize: size_of::<MONITORINFO>() as u32,
        ..Default::default()
    };
    // SAFETY: `info` is a valid, correctly sized out structure
    if unsafe { GetMonitorInfoW(monitor, &raw mut info) }.as_bool() {
        found.push(Found {
            monitor,
            rect: [
                info.rcMonitor.left,
                info.rcMonitor.top,
                info.rcMonitor.right,
                info.rcMonitor.bottom,
            ],
            primary: info.dwFlags & MONITORINFOF_PRIMARY != 0,
        });
    }

    BOOL(1)
}

fn monitors() -> Vec<Found> {
    let mut found: Vec<Found> = Vec::new();
    // SAFETY: `found` outlives the call and `collect` only touches it during it
    let _ =
        unsafe { EnumDisplayMonitors(None, None, Some(collect), LPARAM(&raw mut found as isize)) };
    found
}

/// Returns the monitor with this id, if it is still attached. The id is the one
/// [`active_displays`] gives a display: the monitor's `HMONITOR`.
///
/// Windows only: the capture backend needs the handle to create a capture item for a display.
#[must_use]
pub fn monitor_handle(id: u32) -> Option<HMONITOR> {
    monitors()
        .into_iter()
        .map(|found| found.monitor)
        .find(|monitor| id_of(*monitor) == id)
}

/// A monitor's id: its `HMONITOR`, which is the id komorebi for Windows gives it too. Handles are
/// 32-bit values even in a 64-bit process.
fn id_of(monitor: HMONITOR) -> u32 {
    monitor.0 as usize as u32
}

fn scale_of(monitor: HMONITOR) -> f64 {
    let (mut x, mut y) = (0_u32, 0_u32);
    // SAFETY: both are valid out pointers for the duration of the call
    let known = unsafe { GetDpiForMonitor(monitor, MDT_EFFECTIVE_DPI, &raw mut x, &raw mut y) };
    if known.is_ok() && x > 0 {
        f64::from(x) / BASE_DPI
    } else {
        1.0
    }
}

/// Returns the displays attached right now.
///
/// A display's `id` is its `HMONITOR` as a `u32`, which is the id komorebi for Windows gives it
/// too. Its `frame` is `[left, top, width, height]` in the virtual screen's physical pixels, so
/// `width_points` and `height_points` are its width and height in pixels and `scale` is the
/// monitor's effective DPI over 96 (`1.0` if Windows will not say). `asleep` is always `false`. On
/// macOS the frame is in points and a sleeping display is reported as asleep.
///
/// # Errors
///
/// Never fails on this host: a monitor list that cannot be read is an empty list. (On macOS it
/// fails if CoreGraphics cannot list the displays.)
pub fn active_displays() -> eyre::Result<Vec<DisplayDescriptor>> {
    Ok(monitors()
        .into_iter()
        .map(|found| {
            let [left, top, right, bottom] = found.rect;
            let (width, height) = (right - left, bottom - top);
            DisplayDescriptor {
                id: id_of(found.monitor),
                frame: [
                    f64::from(left),
                    f64::from(top),
                    f64::from(width),
                    f64::from(height),
                ],
                width_points: i64::from(width),
                height_points: i64::from(height),
                asleep: false,
                scale: scale_of(found.monitor),
            }
        })
        .collect())
}

/// Returns the id of the primary display (the same id as [`DisplayDescriptor::id`] of that display
/// in [`active_displays`]), or `0` if there are none.
#[must_use]
pub fn main_display_id() -> u32 {
    monitors()
        .iter()
        .find(|found| found.primary)
        .map_or(0, |found| id_of(found.monitor))
}

/// Returns the primary display.
///
/// # Errors
///
/// Returns an error if the primary display is not among the displays attached, which includes
/// there being none.
pub fn main_display() -> eyre::Result<DisplayDescriptor> {
    let main = main_display_id();
    active_displays()?
        .into_iter()
        .find(|display| display.id == main)
        .ok_or_else(|| eyre::Report::msg("the primary display is not available for capture"))
}

/// Returns the playable duration of a video file as Media Foundation reads it, in seconds.
///
/// This is what recovery trusts for a file whose recorder died: the frame log is flushed on a
/// timer and can lag behind (or run ahead of) what actually reached the video. A file whose
/// duration it cannot read (some truncated ones), or that is not longer than zero, is `None`, and
/// recovery says it could not tell. On macOS the duration comes from `AVFoundation`.
///
/// Media Foundation's reader is not part of the build yet, so this is always `None`.
#[must_use]
pub fn media_duration_s(path: &Path) -> Option<f64> {
    let _ = path;
    None
}

/// The [`CaptureBackend`](crate::capture::CaptureBackend) of this host: Windows.Graphics.Capture.
pub use crate::capture::wgc::WgcBackend as Backend;
