//! What this host can capture: the displays, the permission to capture them, the capture backend,
//! and the duration of a recorded video.
//!
//! Screen capture on macOS is `ScreenCaptureKit` and `AVFoundation`; the code lives in
//! [`crate::capture::sck`] and this is the host-neutral name for it.
//!
//! | item | on macOS |
//! |---|---|
//! | [`supported`] | always `true` |
//! | [`has_permission`] | whether Screen Recording permission is granted; never prompts |
//! | [`ensure_permission`] | prompts for the permission if it is not granted, and fails until it is |
//! | [`active_displays`] | the displays CoreGraphics lists as active, in points |
//! | [`main_display_id`], [`main_display`] | the main display |
//! | [`media_duration_s`] | the duration of a video file, from `AVFoundation` |
//! | [`Backend`] | [`SckBackend`](crate::capture::sck::SckBackend), the [`CaptureBackend`](crate::capture::CaptureBackend) that records with `ScreenCaptureKit` |
//!
//! The same items exist on Windows. A display's `frame` is in points on a Mac and in pixels on
//! Windows ([`DisplayDescriptor::scale`] converts), and Windows needs no permission.

use crate::capture::DisplayDescriptor;
use color_eyre::eyre;
use objc2_core_graphics::CGMainDisplayID;

/// The [`CaptureBackend`](crate::capture::CaptureBackend) of this host: `ScreenCaptureKit`.
pub use crate::capture::sck::SckBackend as Backend;
/// Returns the displays attached right now, from CoreGraphics, with frames in points. It fails
/// only if CoreGraphics cannot list them; see [`crate::capture::sck::active_displays`].
pub use crate::capture::sck::active_displays;
/// Makes sure this process may capture the screen, prompting for the Screen Recording permission
/// if it must, and fails until it is granted; see [`crate::capture::sck::ensure_permission`].
pub use crate::capture::sck::ensure_permission;
/// Returns `true` if and only if the Screen Recording permission is already granted. It never
/// prompts; see [`crate::capture::sck::has_permission`].
pub use crate::capture::sck::has_permission;
/// Returns the duration of a video file in seconds as `AVFoundation` reads it, or `None` if it
/// cannot be read; see [`crate::capture::sck::media_duration_s`].
pub use crate::capture::sck::media_duration_s;

/// Returns `true` if and only if this host can capture the screen. It is always `true` on macOS
/// (and on Windows), and `false` on a host that can only investigate a session.
#[must_use]
pub fn supported() -> bool {
    true
}

/// Returns the id of the display the OS treats as the main one (`CGMainDisplayID`).
///
/// This is the same id as [`DisplayDescriptor::id`] of that display in [`active_displays`]. On
/// Windows it is the primary display's, or `0` if there are none.
#[must_use]
pub fn main_display_id() -> u32 {
    CGMainDisplayID()
}

/// Returns the main display among the currently attached ones.
///
/// # Errors
///
/// Returns an error if the displays cannot be listed (see [`active_displays`]), or if the main
/// display is not among them (the displays can change between the two reads).
pub fn main_display() -> eyre::Result<DisplayDescriptor> {
    let main = main_display_id();
    active_displays()?
        .into_iter()
        .find(|display| display.id == main)
        .ok_or_else(|| eyre::Report::msg("the main display is not available for capture"))
}
