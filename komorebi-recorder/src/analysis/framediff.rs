//! `framediff`: how two frames differ, in numbers and as a picture.
//!
//! Two frames are decoded (the ones on screen at two moments of one recording, or one moment of
//! each of two recordings), optionally cut to a region of the display, reduced to brightness and
//! compared pixel by pixel. The answer is how much changed (the share of pixels that moved by more
//! than 24 of 255, and the mean and largest brightness difference), the box around the change, and,
//! if asked, a PNG of the compared area in which the changed pixels are red.
//!
//! Frames are decoded at a width of 1280 pixels unless asked otherwise (never more than the
//! video has), so a region is compared at that resolution. Two recordings must have been made at
//! the same size to be compared.

use super::Analysis;
use super::envelope;
use super::pixels;
use super::pixels::Crop;
use super::sheet::Canvas;
use super::video::SessionVideo;
use crate::av::Scale;
use color_eyre::eyre;
use schemars::JsonSchema;
use serde::Serialize;
use std::path::Path;

/// The schema name that identifies a [`FrameDiffData`] in an envelope (`framediff/1`).
pub const SCHEMA_FRAMEDIFF: &str = "framediff/1";

/// The width frames are decoded at by default.
pub const DEFAULT_WIDTH: u32 = 1280;

/// One frame to compare: a moment on a display of a recording.
#[derive(Clone, Copy)]
pub struct FrameAt<'a> {
    /// The recording.
    pub analysis: &'a Analysis,
    /// Its video, made on this thread.
    pub video: &'a SessionVideo<'a>,
    /// The display.
    pub display: u32,
    /// The moment, in seconds of video time of that recording.
    pub time_s: f64,
}

/// What to compare and what to write.
#[derive(Debug, Clone, Copy)]
pub struct FrameDiffOptions<'a> {
    /// Only this part of the display, in global display points (of the first frame's display);
    /// the whole display if `None`.
    pub crop: Option<Crop>,
    /// The width to decode frames at.
    pub width: u32,
    /// Where to write the PNG of the change, if wanted.
    pub png: Option<&'a Path>,
}

/// How two frames differ: the `data` of `framediff`.
#[derive(Debug, Clone, PartialEq, Serialize, JsonSchema)]
pub struct FrameDiffData {
    /// The display of the first frame.
    pub display: u32,
    /// The display of the second frame (differs only when `against` was used).
    pub display_b: u32,
    /// The moment asked for the first frame, in seconds of video time.
    pub time_a_s: f64,
    /// When the frame that was on screen then was shown.
    pub shown_a_s: f64,
    /// The moment asked for the second frame, in seconds of video time.
    pub time_b_s: f64,
    /// When the frame that was on screen then was shown.
    pub shown_b_s: f64,
    /// The other recording, if the second frame is from one.
    pub against: Option<String>,
    /// The region compared, in global display points; `None` is the whole display.
    pub region: Option<Crop>,
    /// The width of the compared area, in pixels.
    pub width: u32,
    /// The height of the compared area, in pixels.
    pub height: u32,
    /// How many pixels moved by more than 24 of 255 in brightness.
    pub changed_pixels: usize,
    /// The share of the compared pixels that changed, 0 to 1.
    pub changed_share: f64,
    /// The mean absolute brightness difference over the compared pixels, 0 to 255.
    pub mean_delta: f64,
    /// The largest brightness difference of any pixel, 0 to 255.
    pub peak_delta: u8,
    /// The box around the changed pixels as fractions of the display: `[x, y, width, height]`.
    /// `None` if nothing changed.
    pub bbox: Option<[f64; 4]>,
    /// The same box in global display points.
    pub bbox_points: Option<[f64; 4]>,
    /// Where the PNG was written, if one was asked for.
    pub png: Option<String>,
}

impl FrameDiffData {
    /// Renders the answer as text.
    ///
    /// # Example
    ///
    /// ```rust
    /// use komorebi_recorder::analysis::framediff::FrameDiffData;
    ///
    /// let data = FrameDiffData {
    ///     display: 1, display_b: 1, time_a_s: 3.2, shown_a_s: 3.18, time_b_s: 3.5, shown_b_s: 3.47,
    ///     against: None, region: None, width: 1280, height: 540, changed_pixels: 6912,
    ///     changed_share: 0.01, mean_delta: 2.5, peak_delta: 200, bbox: Some([0.1, 0.1, 0.5, 0.5]),
    ///     bbox_points: None, png: None,
    /// };
    /// assert!(data.to_text().contains("1.00% of the pixels changed"));
    /// ```
    #[must_use]
    pub fn to_text(&self) -> String {
        use crate::timecode::format as time;
        use std::fmt::Write;

        let mut out = String::new();
        let _ = writeln!(
            out,
            "display {}: {} (frame shown {}) against {} (frame shown {}){}",
            self.display,
            time(self.time_a_s),
            time(self.shown_a_s),
            time(self.time_b_s),
            time(self.shown_b_s),
            self.against
                .as_ref()
                .map_or_else(String::new, |other| format!(" of {other}")),
        );
        let _ = writeln!(
            out,
            "{:.2}% of the pixels changed ({} of {}x{}), mean brightness delta {:.2}, peak {}",
            self.changed_share * 100.0,
            self.changed_pixels,
            self.width,
            self.height,
            self.mean_delta,
            self.peak_delta,
        );
        match (self.bbox, self.bbox_points) {
            (Some(bbox), points) => {
                let _ = write!(
                    out,
                    "box of change {:.0}%,{:.0}% {:.0}%x{:.0}% of the display",
                    bbox[0] * 100.0,
                    bbox[1] * 100.0,
                    bbox[2] * 100.0,
                    bbox[3] * 100.0,
                );
                if let Some(p) = points {
                    let _ = write!(
                        out,
                        " ({:.0},{:.0} {:.0}x{:.0} points)",
                        p[0], p[1], p[2], p[3]
                    );
                }
                out.push('\n');
            }
            (None, _) => out.push_str("nothing changed\n"),
        }
        if let Some(path) = &self.png {
            let _ = writeln!(out, "wrote {path}");
        }
        out
    }
}

fn grab(at: &FrameAt, scale: Scale, what: &str) -> eyre::Result<super::video::DecodedFrame> {
    at.video
        .frame_at(at.display, at.time_s, scale)?
        .ok_or_else(|| {
            envelope::not_found(format!(
                "display {} had no video at {} ({what})",
                at.display,
                crate::timecode::format(at.time_s)
            ))
        })
}

/// Compares the frame `a` with the frame `b`, as the [module](self) describes.
///
/// The region is placed on the display of `a` (so it is in the points of that recording) and
/// applied as fractions to both.
///
/// # Errors
///
/// Returns an error if:
///
/// - the region does not overlap the display, or its frame is unknown (the usage kind);
/// - a display had no video at its moment (not found);
/// - the two frames are not the same size, which happens when the recordings were made at
///   different sizes (the unusable kind);
/// - a video cannot be opened or decoded, the host has no decoder, or the PNG cannot be written.
///
/// # Example
///
/// This needs recorded video, so it is not run here; `tests/video.rs` runs it through the
/// `framediff` command.
///
/// ```rust,no_run
/// use komorebi_recorder::analysis::Analysis;
/// use komorebi_recorder::analysis::framediff::{DEFAULT_WIDTH, FrameAt, FrameDiffOptions, framediff};
/// use komorebi_recorder::analysis::video::SessionVideo;
///
/// let analysis = Analysis::open(std::path::Path::new("session-dir"))?;
/// let video = SessionVideo::new(&analysis.manifest, analysis.guard());
/// let at = |time_s| FrameAt { analysis: &analysis, video: &video, display: 1, time_s };
/// let data = framediff(
///     &at(3.2),
///     &at(3.5),
///     &FrameDiffOptions { crop: None, width: DEFAULT_WIDTH, png: None },
/// )?;
/// print!("{}", data.to_text());
/// # Ok::<(), Box<dyn std::error::Error>>(())
/// ```
pub fn framediff(
    a: &FrameAt,
    b: &FrameAt,
    options: &FrameDiffOptions,
) -> eyre::Result<FrameDiffData> {
    let fractions = options
        .crop
        .as_ref()
        .map(|crop| a.analysis.crop_fractions(a.display, crop))
        .transpose()?;
    let scale = Scale::Width(options.width.max(2));
    let (first, second) = (grab(a, scale, "first")?, grab(b, scale, "second")?);
    let (width, height) = (first.frame.width, first.frame.height);
    if (second.frame.width, second.frame.height) != (width, height) {
        return Err(envelope::unusable(format!(
            "the frames are not the same size ({width}x{height} against {}x{}): the recordings were made at different sizes",
            second.frame.width, second.frame.height
        )));
    }

    let rect = fractions.map_or(
        pixels::PixelRect {
            x: 0,
            y: 0,
            width,
            height,
        },
        |fractions| pixels::pixel_rect(fractions, width, height),
    );
    let before = pixels::crop_luma(&pixels::luma(&first.frame.bgra), width, rect);
    let after = pixels::crop_luma(&pixels::luma(&second.frame.bgra), width, rect);
    let found = pixels::delta(&before, &after, rect.width, super::changes::PIXEL_THRESHOLD);

    let bbox = found.bounds.map(|(min_x, min_y, max_x, max_y)| {
        super::changes::fractions_of(
            (
                min_x + rect.x,
                min_y + rect.y,
                max_x + rect.x,
                max_y + rect.y,
            ),
            Some((width, height)),
        )
    });

    if let Some(path) = options.png {
        write_png(
            path,
            &second.frame.bgra,
            width,
            rect,
            &before,
            &after,
            found.bounds,
        )?;
    }

    Ok(FrameDiffData {
        display: a.display,
        display_b: b.display,
        time_a_s: a.time_s,
        shown_a_s: first.shown_s,
        time_b_s: b.time_s,
        shown_b_s: second.shown_s,
        against: (!std::ptr::eq(a.analysis, b.analysis))
            .then(|| b.analysis.session.dir.display().to_string()),
        region: options.crop,
        width: rect.width,
        height: rect.height,
        changed_pixels: found.changed,
        changed_share: found.share(),
        mean_delta: found.mean,
        peak_delta: found.peak,
        bbox,
        bbox_points: bbox
            .and_then(|bbox| super::changes::points_of(a.analysis.display_frame(a.display), bbox)),
        png: options.png.map(|path| path.display().to_string()),
    })
}

/// Writes the compared area of the second frame, dimmed, with the pixels that changed in red and
/// a yellow box around them.
fn write_png(
    path: &Path,
    bgra: &[u8],
    width: u32,
    rect: pixels::PixelRect,
    before: &[u8],
    after: &[u8],
    bounds: Option<(u32, u32, u32, u32)>,
) -> eyre::Result<()> {
    let area = pixels::crop_bgra(bgra, width, rect);
    let mut canvas = Canvas::new(rect.width, rect.height);
    for row in 0..rect.height {
        for column in 0..rect.width {
            let at = (row * rect.width + column) as usize;
            let changed = (i32::from(before[at]) - i32::from(after[at])).abs()
                > super::changes::PIXEL_THRESHOLD;
            let color = if changed {
                [255, 40, 40]
            } else {
                let grey = (u32::from(area[at * 4 + 2]) * 299
                    + u32::from(area[at * 4 + 1]) * 587
                    + u32::from(area[at * 4]) * 114)
                    / 1000;
                let dimmed = (grey * 2 / 5) as u8;
                [dimmed, dimmed, dimmed]
            };
            canvas.put(i64::from(column), i64::from(row), color, 1.0);
        }
    }
    if let Some((min_x, min_y, max_x, max_y)) = bounds {
        canvas.border(
            min_x,
            min_y,
            max_x - min_x + 1,
            max_y - min_y + 1,
            1,
            [255, 220, 0],
        );
    }

    canvas.save(path)
}
