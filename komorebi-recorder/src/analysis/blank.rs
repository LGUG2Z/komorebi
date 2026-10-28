//! `blank`: when a region of a display was a blank patch of one colour, or frozen.
//!
//! The question behind it is "did the content of this window go white for a moment?". Every frame
//! of the range is cut to the region and measured, and two kinds of stretch are reported:
//!
//! - **blank**: the region is near-uniform, its brightness varying less than a threshold
//!   (variance of the 0 to 255 brightness, default 4.0: a standard deviation of 2), and the colour
//!   is named (`white`, `black`, `gray`, or a hex code). A run of blank frames of one colour is one
//!   stretch, which lasts until the next frame replaced it.
//! - **frozen**: consecutive frames in which the region is identical (no pixel differs by more than
//!   2 of 255). The video only has a frame when something on the screen changed, so a frozen
//!   region is one that stayed the same while something else on the display kept changing: a
//!   stretch from the frame that showed it to the last repeat. A display that held entirely
//!   still has no frames and so no frozen stretch.
//!
//! Stretches shorter than `min_duration_s` are dropped. A stretch is judged on frames decoded at a
//! size that gives the region 240 pixels across, so a few stray pixels in a large region are
//! averaged away; use a smaller region to see them.

use super::Analysis;
use super::changes::Gap;
use super::envelope;
use super::motion::ScanOptions;
use super::motion::scale_for;
use super::pixels;
use super::pixels::Crop;
use super::video::SessionVideo;
use crate::av::Scale;
use color_eyre::eyre;
use schemars::JsonSchema;
use serde::Serialize;

/// The schema name that identifies a [`BlankData`] in an envelope (`blank/1`).
pub const SCHEMA_BLANK: &str = "blank/1";

/// The default brightness variance under which a region is blank.
pub const DEFAULT_VARIANCE: f64 = 4.0;

/// The default shortest stretch reported, in seconds.
pub const DEFAULT_MIN_DURATION_S: f64 = 0.0;

/// A pixel that moved by no more than this (of 255) is the same pixel.
const FROZEN_TOLERANCE: i32 = 2;

/// What a [`Stretch`] is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum StretchKind {
    /// The region was one colour.
    Blank,
    /// The region was identical in consecutive frames.
    Frozen,
}

/// A stretch of video in which the region was blank, or frozen.
#[derive(Debug, Clone, PartialEq, Serialize, JsonSchema)]
pub struct Stretch {
    /// The id of the display.
    pub display: u32,
    /// Whether the region was blank or frozen.
    pub kind: StretchKind,
    /// When the stretch began, in seconds of video time.
    pub start_video_s: f64,
    /// When it ended, in seconds of video time: when a frame showed something else (a blank stretch
    /// that lasted to the end of the range ends there), or the last repeat for a frozen one.
    pub end_video_s: f64,
    /// `end_video_s - start_video_s`.
    pub duration_s: f64,
    /// For a blank stretch the frames that were blank; for a frozen one the frames that repeated
    /// the one before.
    pub frames: usize,
    /// For a blank stretch, the colour: `white`, `black`, `gray` or a hex code. `None` when frozen.
    pub colour: Option<String>,
    /// For a blank stretch, the mean colour `[red, green, blue]`. `None` when frozen.
    pub rgb: Option<[u8; 3]>,
}

/// Where a region was blank or frozen over a range: the `data` of `blank`.
#[derive(Debug, Clone, PartialEq, Serialize, JsonSchema)]
pub struct BlankData {
    /// The start of the range scanned, in seconds of video time.
    pub from_video_s: f64,
    /// The end of the range scanned, in seconds of video time.
    pub to_video_s: f64,
    /// The displays scanned.
    pub displays: Vec<u32>,
    /// The region, in global display points; `None` is the whole display.
    pub region: Option<Crop>,
    /// A region is blank when the variance of its brightness is below this.
    pub variance_threshold: f64,
    /// Stretches shorter than this were dropped, in seconds.
    pub min_duration_s: f64,
    /// How many frames were decoded and measured, over all displays.
    pub frames_examined: usize,
    /// The stretches, ordered by start, then display, then kind.
    pub stretches: Vec<Stretch>,
    /// Stretches of the range where a display had no video at all.
    pub gaps: Vec<Gap>,
}

impl BlankData {
    /// Renders the answer as text, one line for each stretch.
    ///
    /// # Example
    ///
    /// ```rust
    /// use komorebi_recorder::analysis::blank::{BlankData, Stretch, StretchKind};
    ///
    /// let data = BlankData {
    ///     from_video_s: 0.0,
    ///     to_video_s: 6.0,
    ///     displays: vec![1],
    ///     region: None,
    ///     variance_threshold: 4.0,
    ///     min_duration_s: 0.0,
    ///     frames_examined: 50,
    ///     stretches: vec![Stretch {
    ///         display: 1,
    ///         kind: StretchKind::Blank,
    ///         start_video_s: 3.28,
    ///         end_video_s: 3.5,
    ///         duration_s: 0.22,
    ///         frames: 3,
    ///         colour: Some("white".to_string()),
    ///         rgb: Some([255, 255, 255]),
    ///     }],
    ///     gaps: vec![],
    /// };
    /// assert!(data.to_text().contains("blank (white)"));
    /// ```
    #[must_use]
    pub fn to_text(&self) -> String {
        use crate::timecode::format as time;
        use std::fmt::Write;

        let mut out = String::new();
        let _ = writeln!(
            out,
            "{} frames measured on display(s) {} from {} to {}: {} stretch(es) blank or frozen",
            self.frames_examined,
            self.displays
                .iter()
                .map(ToString::to_string)
                .collect::<Vec<_>>()
                .join(", "),
            time(self.from_video_s),
            time(self.to_video_s),
            self.stretches.len(),
        );
        for stretch in &self.stretches {
            let what = match (&stretch.kind, &stretch.colour) {
                (StretchKind::Blank, Some(colour)) => format!("blank ({colour})"),
                (StretchKind::Blank, None) => "blank".to_string(),
                (StretchKind::Frozen, _) => "frozen".to_string(),
            };
            let _ = writeln!(
                out,
                "display {}  {} - {} ({:.0} ms, {} frames)  {what}",
                stretch.display,
                time(stretch.start_video_s),
                time(stretch.end_video_s),
                stretch.duration_s * 1000.0,
                stretch.frames,
            );
        }
        for gap in &self.gaps {
            let _ = writeln!(
                out,
                "display {} has no video from {} to {}",
                gap.display,
                time(gap.from_video_s),
                time(gap.to_video_s)
            );
        }
        out
    }
}

/// What one frame showed in the region.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Sample {
    /// When the frame was shown, in seconds of video time.
    pub shown_s: f64,
    /// The variance of the region's brightness.
    pub variance: f64,
    /// The mean colour of the region.
    pub rgb: [u8; 3],
    /// Whether the region is identical to what the previous frame showed.
    pub identical: bool,
}

/// Finds the blank and frozen stretches in the samples of one display, which are in time order.
/// `to_s` is where the range ends: a blank stretch still going at the last sample lasts to it.
///
/// # Example
///
/// ```rust
/// use komorebi_recorder::analysis::blank::{Sample, StretchKind, stretches};
///
/// let white = |shown_s, identical| Sample { shown_s, variance: 0.0, rgb: [255, 255, 255], identical };
/// let busy = |shown_s| Sample { shown_s, variance: 900.0, rgb: [90, 80, 70], identical: false };
///
/// // busy until 1.0 s, white from 1.0 s to 1.4 s, busy again
/// let found = stretches(1, &[busy(0.5), white(1.0, false), white(1.2, true), busy(1.4)], 2.0, 4.0, 0.0);
/// assert_eq!(found.len(), 2);
/// assert_eq!(found[0].kind, StretchKind::Blank);
/// assert_eq!(found[0].colour.as_deref(), Some("white"));
/// assert!((found[0].duration_s - 0.4).abs() < 1e-9);
/// assert_eq!(found[1].kind, StretchKind::Frozen);
/// ```
#[must_use]
pub fn stretches(
    display: u32,
    samples: &[Sample],
    to_s: f64,
    variance: f64,
    min_duration_s: f64,
) -> Vec<Stretch> {
    let mut found = Vec::new();

    // blank: runs of blank frames of one colour
    let mut index = 0;
    while index < samples.len() {
        let sample = &samples[index];
        if sample.variance >= variance {
            index += 1;
            continue;
        }

        let name = pixels::colour_name(sample.rgb);
        let mut last = index;
        while samples
            .get(last + 1)
            .is_some_and(|next| next.variance < variance && pixels::colour_name(next.rgb) == name)
        {
            last += 1;
        }

        let run = &samples[index..=last];
        let end = samples
            .get(last + 1)
            .map_or(to_s.max(samples[last].shown_s), |next| next.shown_s);
        let mean = |channel: usize| {
            (run.iter().map(|s| f64::from(s.rgb[channel])).sum::<f64>() / run.len() as f64).round()
                as u8
        };
        found.push(Stretch {
            display,
            kind: StretchKind::Blank,
            start_video_s: sample.shown_s,
            end_video_s: end,
            duration_s: end - sample.shown_s,
            frames: run.len(),
            colour: Some(name),
            rgb: Some([mean(0), mean(1), mean(2)]),
        });
        index = last + 1;
    }

    // frozen: runs of frames identical to the one before, from that one
    let mut index = 1;
    while index < samples.len() {
        if !samples[index].identical {
            index += 1;
            continue;
        }

        let mut last = index;
        while samples.get(last + 1).is_some_and(|next| next.identical) {
            last += 1;
        }
        let (start, end) = (samples[index - 1].shown_s, samples[last].shown_s);
        found.push(Stretch {
            display,
            kind: StretchKind::Frozen,
            start_video_s: start,
            end_video_s: end,
            duration_s: end - start,
            frames: last - index + 1,
            colour: None,
            rgb: None,
        });
        index = last + 1;
    }

    found.retain(|stretch| stretch.duration_s >= min_duration_s);
    found.sort_by(|a, b| {
        a.start_video_s
            .total_cmp(&b.start_video_s)
            .then((a.kind as u8).cmp(&(b.kind as u8)))
    });
    found
}

impl Analysis {
    /// Finds where the region of `options` (the whole display if it has none) was blank, or
    /// frozen, on each display, as the [module](self) describes.
    ///
    /// `variance` is the brightness variance under which a region is blank (see
    /// [`DEFAULT_VARIANCE`]); stretches shorter than `min_duration_s` are dropped. Stops early,
    /// without an error, once `options.cancel` is set.
    ///
    /// # Errors
    ///
    /// Returns an error if `variance` is negative or `min_duration_s` is, or the region does not
    /// overlap a display or a display's frame is unknown (the usage kind), or a video file cannot
    /// be resolved, opened or decoded, or the host has no decoder.
    ///
    /// # Example
    ///
    /// This needs recorded video, so it is not run here; `tests/video.rs` runs it through the
    /// `blank` command.
    ///
    /// ```rust,no_run
    /// use komorebi_recorder::analysis::Analysis;
    /// use komorebi_recorder::analysis::blank::{DEFAULT_MIN_DURATION_S, DEFAULT_VARIANCE};
    /// use komorebi_recorder::analysis::motion::ScanOptions;
    /// use komorebi_recorder::analysis::pixels::Crop;
    /// use komorebi_recorder::analysis::video::SessionVideo;
    ///
    /// let analysis = Analysis::open(std::path::Path::new("session-dir"))?;
    /// let video = SessionVideo::new(&analysis.manifest, analysis.guard());
    /// let data = analysis.blank(
    ///     &video,
    ///     &ScanOptions {
    ///         displays: &video.displays(),
    ///         from_s: 0.0,
    ///         to_s: 60.0,
    ///         crop: Some(Crop::parse("500,300,1200,800")?),
    ///         cancel: None,
    ///     },
    ///     DEFAULT_VARIANCE,
    ///     DEFAULT_MIN_DURATION_S,
    /// )?;
    /// print!("{}", data.to_text());
    /// # Ok::<(), Box<dyn std::error::Error>>(())
    /// ```
    pub fn blank(
        &self,
        video: &SessionVideo,
        options: &ScanOptions,
        variance: f64,
        min_duration_s: f64,
    ) -> eyre::Result<BlankData> {
        if variance < 0.0 || min_duration_s < 0.0 {
            return Err(envelope::usage(
                "--variance and --min-ms cannot be negative",
            ));
        }

        let mut found = Vec::new();
        let mut examined = 0;
        for display in options.displays {
            let (crop, scale) = match &options.crop {
                Some(crop) => {
                    let fractions = self.crop_fractions(*display, crop)?;
                    (Some(fractions), scale_for(fractions[2]))
                }
                None => (None, Scale::Width(240)),
            };

            let mut samples: Vec<Sample> = Vec::new();
            let mut previous: Option<(Vec<u8>, u32, u32)> = None;
            examined += video.scan(
                *display,
                options.from_s,
                options.to_s,
                scale,
                &mut |decoded| {
                    if options
                        .cancel
                        .is_some_and(|cancel| cancel.load(std::sync::atomic::Ordering::Relaxed))
                    {
                        return false;
                    }

                    let (width, height) = (decoded.frame.width, decoded.frame.height);
                    let rect = crop.map(|crop| pixels::pixel_rect(crop, width, height));
                    let (patch, row) = match rect {
                        Some(rect) => (
                            pixels::crop_bgra(&decoded.frame.bgra, width, rect),
                            rect.width,
                        ),
                        None => (decoded.frame.bgra.clone(), width),
                    };
                    let now = pixels::luma(&patch);
                    let identical = previous.as_ref().is_some_and(|(before, w, h)| {
                        (*w, *h) == (width, height)
                            && pixels::delta(before, &now, row, FROZEN_TOLERANCE).changed == 0
                    });
                    let measured = pixels::evenness(&patch);
                    samples.push(Sample {
                        shown_s: decoded.shown_s,
                        variance: measured.variance,
                        rgb: measured.rgb,
                        identical,
                    });
                    previous = Some((now, width, height));
                    true
                },
            )?;

            found.extend(stretches(
                *display,
                &samples,
                options.to_s,
                variance,
                min_duration_s,
            ));
        }

        found.sort_by(|a, b| {
            a.start_video_s
                .total_cmp(&b.start_video_s)
                .then(a.display.cmp(&b.display))
        });

        Ok(BlankData {
            from_video_s: options.from_s,
            to_video_s: options.to_s,
            displays: options.displays.to_vec(),
            region: options.crop,
            variance_threshold: variance,
            min_duration_s,
            frames_examined: examined,
            stretches: found,
            gaps: self.video_gaps(options.displays, options.from_s, options.to_s),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample(shown_s: f64, variance: f64, rgb: [u8; 3], identical: bool) -> Sample {
        Sample {
            shown_s,
            variance,
            rgb,
            identical,
        }
    }

    #[test]
    fn a_blank_stretch_lasts_until_the_frame_that_replaced_it() {
        let samples = [
            sample(0.0, 800.0, [90, 90, 90], false),
            sample(1.0, 0.5, [250, 250, 250], false),
            sample(1.5, 0.5, [250, 250, 250], false),
            sample(2.0, 800.0, [90, 90, 90], false),
        ];
        let found = stretches(7, &samples, 5.0, 4.0, 0.0);
        assert_eq!(found.len(), 1);
        assert_eq!((found[0].start_video_s, found[0].end_video_s), (1.0, 2.0));
        assert_eq!(found[0].frames, 2);
        assert_eq!(found[0].display, 7);
    }

    #[test]
    fn a_blank_stretch_at_the_end_lasts_to_the_end_of_the_range() {
        let samples = [
            sample(0.0, 800.0, [9; 3], false),
            sample(4.0, 0.0, [0; 3], false),
        ];
        let found = stretches(1, &samples, 6.0, 4.0, 0.0);
        assert_eq!(found[0].colour.as_deref(), Some("black"));
        assert_eq!(found[0].end_video_s, 6.0);
    }

    #[test]
    fn a_change_of_blank_colour_is_two_stretches() {
        let samples = [
            sample(0.0, 0.0, [255; 3], false),
            sample(1.0, 0.0, [0; 3], false),
        ];
        let found = stretches(1, &samples, 2.0, 4.0, 0.0);
        let colours: Vec<_> = found.iter().filter_map(|s| s.colour.as_deref()).collect();
        assert_eq!(colours, ["white", "black"]);
    }

    #[test]
    fn frozen_runs_start_at_the_frame_that_was_repeated() {
        let samples = [
            sample(0.0, 800.0, [90; 3], false),
            sample(0.1, 800.0, [90; 3], false),
            sample(0.2, 800.0, [90; 3], true),
            sample(0.3, 800.0, [90; 3], true),
            sample(0.4, 800.0, [90; 3], false),
        ];
        let found = stretches(1, &samples, 1.0, 4.0, 0.0);
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].kind, StretchKind::Frozen);
        assert_eq!((found[0].start_video_s, found[0].end_video_s), (0.1, 0.3));
        assert_eq!(found[0].frames, 2);
    }

    #[test]
    fn short_stretches_are_dropped() {
        let samples = [
            sample(0.0, 0.0, [255; 3], false),
            sample(0.05, 800.0, [90; 3], false),
        ];
        assert!(stretches(1, &samples, 1.0, 4.0, 0.1).is_empty());
        assert_eq!(stretches(1, &samples, 1.0, 4.0, 0.0).len(), 1);
    }

    #[test]
    fn a_synthetic_white_patch_is_measured_as_blank_white() {
        let white = vec![255_u8; 4 * 4 * 4];
        let measured = pixels::evenness(&white);
        assert!(measured.variance < DEFAULT_VARIANCE);
        assert_eq!(pixels::colour_name(measured.rgb), "white");
    }
}
