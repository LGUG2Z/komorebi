//! Capturing the screen on Linux: KDE's screencast of each output, written by GStreamer.
//!
//! The displays are the compositor's outputs ([`super::wayland::outputs`]). A display's `id` is
//! the FNV-1a hash of the output's name, which is how komorebi for KDE makes the id of a monitor
//! that reports no serial number (it hashes the make, model and serial when there is one, which a
//! Wayland client is not told), so the two agree for most monitors and may not for one with a
//! serial number. `frame` is the output's place in the layout in logical pixels, which is where
//! komorebi has it, and `scale` is how many pixels the video has for each of them.
//!
//! What is captured needs the compositor's trust: see [`has_permission`].

use super::screencast::Source;
use super::wayland;
use crate::capture::CaptureBackend;
use crate::capture::CaptureConfig;
use crate::capture::DisplayDescriptor;
use crate::capture::RunningCapture;
use crate::capture::gst::DisplayCapture;
use color_eyre::eyre;
use std::path::Path;

/// FNV-1a, the hash komorebi for KDE makes a monitor's id with.
fn stable_id(text: &str) -> u32 {
    let mut hash: u32 = 0x811c_9dc5;
    for byte in text.bytes() {
        hash ^= u32::from(byte);
        hash = hash.wrapping_mul(0x0100_0193);
    }
    hash
}

/// Returns the display that an output is.
fn display_of(output: &wayland::Output) -> DisplayDescriptor {
    let (x, y) = output.position.unwrap_or((0, 0));
    // an output with no `xdg_output` is laid out by its mode and its integer scale
    let (width, height) = output.logical_size.unwrap_or((
        output.mode.0 / output.scale.max(1),
        output.mode.1 / output.scale.max(1),
    ));
    let scale = if width > 0 {
        f64::from(output.mode.0) / f64::from(width)
    } else {
        1.0
    };
    DisplayDescriptor {
        id: stable_id(&output.label()),
        frame: [
            f64::from(x),
            f64::from(y),
            f64::from(width),
            f64::from(height),
        ],
        width_points: i64::from(width),
        height_points: i64::from(height),
        asleep: false,
        scale,
    }
}

/// Returns `true` if and only if this host can capture the screen: it is built for it.
#[must_use]
pub fn supported() -> bool {
    true
}

/// Returns `true` if and only if KWin has been told to leave the recorder's own window out of the
/// screencast (see [`super::exclude`]).
#[must_use]
pub fn own_window_excluded() -> bool {
    super::exclude::installed()
}

/// Returns `true` if and only if the compositor offers this program its screencast.
///
/// KWin offers `zkde_screencast_unstable_v1` only to a program whose desktop file asks for it
/// (`X-KDE-Wayland-Interfaces`), so this is `false` until that file is installed, and on a
/// compositor that is not KWin.
#[must_use]
pub fn has_permission() -> bool {
    wayland::screencast_offered()
}

/// Makes sure the compositor offers this program its screencast.
///
/// Nothing is asked of the user: KWin decides by the desktop file, and there is nothing to click.
///
/// # Errors
///
/// Returns an error that says what is missing when there is no compositor, or it offers no
/// screencast to this program.
pub fn ensure_permission() -> eyre::Result<()> {
    wayland::outputs()?;
    // the recorder's own window is left out of the video; if that cannot be arranged the video has
    // the window in it, which `doctor` says
    let _ = super::exclude::ensure();
    if wayland::screencast_offered() {
        Ok(())
    } else {
        eyre::bail!(
            "the compositor does not offer a screencast to komorebi-recorder: KWin does that only \
             for a program whose desktop file lists zkde_screencast_unstable_v1 in \
             X-KDE-Wayland-Interfaces; `komorebi-recorder doctor` says how to install it"
        )
    }
}

/// Returns the displays the compositor has now.
///
/// # Errors
///
/// Returns an error when there is no compositor to ask.
pub fn active_displays() -> eyre::Result<Vec<DisplayDescriptor>> {
    Ok(wayland::outputs()?.iter().map(display_of).collect())
}

/// Returns the id of the main display (the one at the top left of the layout, as KDE has no other
/// notion of a main display here), or `0` if there are none.
#[must_use]
pub fn main_display_id() -> u32 {
    main_display().map_or(0, |display| display.id)
}

/// Returns the main display: the one nearest the top left of the layout.
///
/// # Errors
///
/// Returns an error when there are no displays.
pub fn main_display() -> eyre::Result<DisplayDescriptor> {
    active_displays()?
        .into_iter()
        .min_by(|a, b| {
            (a.frame[0], a.frame[1])
                .partial_cmp(&(b.frame[0], b.frame[1]))
                .unwrap_or(std::cmp::Ordering::Equal)
        })
        .ok_or_else(|| eyre::Report::msg("there is no display"))
}

/// Returns the playable duration of a video file as GStreamer reads it, in seconds.
#[must_use]
pub fn media_duration_s(path: &Path) -> Option<f64> {
    crate::av::Asset::open(path)
        .ok()
        .map(|asset| asset.duration_s())
        .filter(|duration| *duration > 0.0)
}

/// The Linux [`CaptureBackend`]: outputs from the compositor, and a screencast of each.
pub struct Backend;

impl CaptureBackend for Backend {
    fn active_displays(&self) -> eyre::Result<Vec<DisplayDescriptor>> {
        active_displays()
    }

    fn start(
        &self,
        display: &DisplayDescriptor,
        segment: u32,
        config: &CaptureConfig,
        dir: &Path,
    ) -> eyre::Result<Box<dyn RunningCapture>> {
        // the output is found again by the id it was given, which is the hash of its name
        let output = wayland::outputs()?
            .into_iter()
            .find(|output| stable_id(&output.label()) == display.id)
            .ok_or_else(|| {
                eyre::Report::msg(format!(
                    "display {} is not available for capture (yet)",
                    display.id
                ))
            })?;
        Ok(Box::new(DisplayCapture::start_with(
            display,
            segment,
            config,
            dir,
            |sink| Ok(Box::new(Source::open(output.global, sink)?)),
        )?))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn output(name: &str, mode: (i32, i32), scale: i32) -> wayland::Output {
        wayland::Output {
            global: 1,
            name: name.to_string(),
            mode,
            scale,
            ..wayland::Output::default()
        }
    }

    #[test]
    fn a_display_has_the_id_komorebi_for_kde_gives_its_monitor() {
        // the same FNV-1a of the same text as komorebi for KDE's own test of its ids
        assert_eq!(stable_id(""), 0x811c_9dc5);
        assert_eq!(stable_id("a"), 0xe40c_292c);
        assert_eq!(
            display_of(&output("DP-1", (1920, 1080), 1)).id,
            stable_id("DP-1")
        );
    }

    #[test]
    fn an_output_with_no_xdg_output_is_laid_out_by_its_scale() {
        let display = display_of(&output("eDP-1", (2880, 1800), 2));
        assert_eq!(display.frame, [0.0, 0.0, 1440.0, 900.0]);
        assert_eq!((display.width_points, display.height_points), (1440, 900));
        assert!((display.scale - 2.0).abs() < 1e-9);
    }

    #[test]
    fn a_fractional_scale_is_what_the_mode_is_over_the_layout() {
        let mut wide = output("DP-2", (3840, 2160), 2);
        wide.position = Some((1536, 0));
        wide.logical_size = Some((2560, 1440));
        let display = display_of(&wide);
        assert_eq!(display.frame, [1536.0, 0.0, 2560.0, 1440.0]);
        assert!((display.scale - 1.5).abs() < 1e-9);
    }
}
