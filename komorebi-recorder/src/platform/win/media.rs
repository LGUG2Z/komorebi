//! Starting Media Foundation, and packing the values it keeps in one 64-bit attribute.
//!
//! The code that writes video (`crate::capture::wgc`) and the code that reads it back
//! (`crate::av`) both need Windows Runtime and Media Foundation running: [`start`] does that once
//! for the process. [`pack`] and [`unpack`] build and take apart the attributes that hold a size
//! or a ratio.
//!
//! This module exists only on Windows. It is reached as `crate::platform::media`, and the facade
//! has no counterpart on other hosts: nothing there writes or reads video through Media
//! Foundation. Nothing tests it automatically; it is exercised by recording and by playing back
//! video on Windows.

use color_eyre::eyre;
use std::sync::OnceLock;
use windows::Win32::Media::MediaFoundation::MF_VERSION;
use windows::Win32::Media::MediaFoundation::MFSTARTUP_FULL;
use windows::Win32::Media::MediaFoundation::MFStartup;
use windows::Win32::System::WinRT::RO_INIT_MULTITHREADED;
use windows::Win32::System::WinRT::RoInitialize;

/// Starts Windows Runtime and Media Foundation for this process, once. Never shut down: the process
/// is short-lived and a restarted capture or decoder would only start it again.
///
/// The first call does the work and every later call returns the same outcome, so callers need not
/// coordinate. A thread that Windows Runtime was already initialized on, or in a different
/// apartment, is fine.
///
/// # Errors
///
/// Returns an error if Media Foundation cannot be started (`MFStartup` fails), with Windows'
/// reason. The failure is kept: later calls fail the same way without trying again.
pub fn start() -> eyre::Result<()> {
    static STARTED: OnceLock<Result<(), String>> = OnceLock::new();
    STARTED
        .get_or_init(|| {
            // SAFETY: plain initialisation calls; a thread that is already initialised, or in a
            // different apartment, is fine and says so with an error that is ignored
            unsafe {
                let _ = RoInitialize(RO_INIT_MULTITHREADED);
                MFStartup(MF_VERSION, MFSTARTUP_FULL).map_err(|error| error.to_string())
            }
        })
        .clone()
        .map_err(|error| eyre::Report::msg(format!("could not start Media Foundation: {error}")))
}

/// Packs two 32-bit values into the one 64-bit attribute Media Foundation uses for a size or a
/// ratio.
///
/// `high` goes in the upper 32 bits and `low` in the lower. The inline helpers `MFSetAttributeSize`
/// and `MFSetAttributeRatio` do this, and are not bound. A frame size is `pack(width, height)` and
/// a ratio `pack(numerator, denominator)`.
///
/// [`unpack`] undoes it: `unpack(pack(a, b)) == (a, b)` for every `a` and `b`.
#[must_use]
pub const fn pack(high: u32, low: u32) -> u64 {
    ((high as u64) << 32) | low as u64
}

/// Returns the two halves of such an attribute, `(high, low)`: the inverse of [`pack`].
#[must_use]
pub const fn unpack(value: u64) -> (u32, u32) {
    ((value >> 32) as u32, value as u32)
}
