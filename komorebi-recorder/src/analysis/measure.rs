//! `measure`: a number that describes a region of the picture, at a fixed rate, over a time range.
//!
//! The video only has a frame when something on the screen changed, so its frame rate varies and
//! its frames are not evenly spaced. To plot or compare a measure over time it has to be on an
//! even timeline, so this one is: a sample every `1/hz` seconds from the start of the range to its
//! end (the first at the start, the last at or before the end), and the value at a sample is that
//! of the **last frame shown at or before the sample's time**, by the frame's pts on the session's
//! video clock. A frame stays on screen until the next one, so that is the picture that was visible
//! then; [`MeasureSample::shown_s`] says when it was put there. A sample before the display's
//! first frame, or in a stretch where the display had no video, has no value (`null`).
//!
//! The region is in display points ([`Crop`], as for `motion` and `blank`; the whole display if
//! none is given) and is cut from a frame decoded at a size that gives it 480 pixels across (never
//! more than 3840 for the display), so a measure is quantized to a pixel of that.
//!
//! # Statistics
//!
//! All are of the brightness of the region's pixels (Rec. 601 weights, 0 to 255,
//! [`pixels::luma`]):
//!
//! | Statistic | Name | Value |
//! |---|---|---|
//! | mean brightness | `mean` | the mean brightness of every pixel of the region, 0 to 255 |
//! | peak brightness | `peak` | the brightest pixel of the region, 0 to 255 |
//! | drawn width | `width` | the horizontal extent of the *non-background* pixels, in display points |
//!
//! **Drawn width** is defined so that it is simple and repeatable. The *background* is the
//! brightness that most pixels of the region have in that frame (the most frequent value of the
//! 0 to 255 brightness; of two equally frequent, the lower), or the value given by `--background`.
//! A pixel is *drawn* if its brightness differs from the background by more than 24 of 255 (the
//! threshold `changes` uses). The width is the distance from the left edge of the leftmost column
//! that has a drawn pixel to the right edge of the rightmost one, in points (one pixel of the
//! decoded frame is `display width in points / frame width in pixels` points); `0` if no pixel is
//! drawn. It is the width of the *envelope* of the drawing, not the sum of its parts, so two bars
//! with a gap between them have the width of both and the gap. It needs the display's frame to
//! convert pixels to points, as a region does.
//!
//! # Example
//!
//! The statistics of a synthetic frame, and the timeline built from frames at uneven times:
//!
//! ```rust
//! use komorebi_recorder::analysis::measure::{frame_stats, resample};
//!
//! // a 4 x 1 picture, one value of brightness per pixel: dark, dark, bright, dark
//! let stats = frame_stats(&[10, 10, 200, 10], 4, None);
//! assert_eq!(stats.peak, 200);
//! assert_eq!(stats.mean, 57.5);
//! assert_eq!((stats.drawn_columns, stats.first_drawn_column), (1, 2));
//!
//! // frames shown at 0.0 s and 0.5 s; sampled at 4 Hz from 0 to 1 s
//! let timeline = resample(&[(0.0, 'a'), (0.5, 'b')], 0.0, 1.0, 4.0)?;
//! let held: Vec<_> = timeline.iter().map(|(_, frame)| frame.map(|(_, v)| v)).collect();
//! assert_eq!(held, [Some('a'), Some('a'), Some('b'), Some('b'), Some('b')]);
//! # Ok::<(), Box<dyn std::error::Error>>(())
//! ```

use super::Analysis;
use super::changes::Gap;
use super::envelope;
use super::motion::ScanOptions;
use super::pixels;
use super::pixels::Crop;
use super::video::SessionVideo;
use crate::av::Scale;
use color_eyre::eyre;
use schemars::JsonSchema;
use serde::Serialize;
use std::str::FromStr;

/// The schema name that identifies a [`MeasureData`] in an envelope (`measure/1`).
pub const SCHEMA_MEASURE: &str = "measure/1";

/// The default number of samples per second.
pub const DEFAULT_HZ: f64 = 30.0;

/// The most samples of one display an answer may have.
pub const MAX_SAMPLES: usize = 200_000;

/// A pixel is drawn if its brightness differs from the background by more than this (of 255).
pub const DRAWN_THRESHOLD: i32 = super::changes::PIXEL_THRESHOLD;

/// The region is decoded to have this many pixels across.
const REGION_PIXELS: f64 = 480.0;

/// The widest a frame is decoded.
const MAX_WIDTH: u32 = 3840;

/// What is measured of a region; see the [module docs](self) for each.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum Statistic {
    /// The mean brightness of the region, 0 to 255.
    Mean,
    /// The brightest pixel of the region, 0 to 255.
    Peak,
    /// The horizontal extent of the non-background pixels, in display points.
    Width,
}

impl Statistic {
    /// Every statistic, in the order a table has them.
    pub const ALL: [Self; 3] = [Self::Mean, Self::Peak, Self::Width];

    /// The name used on the command line and in JSON.
    #[must_use]
    pub fn name(self) -> &'static str {
        match self {
            Self::Mean => "mean",
            Self::Peak => "peak",
            Self::Width => "width",
        }
    }
}

impl FromStr for Statistic {
    type Err = String;

    /// Parses `mean`, `peak` or `width`.
    ///
    /// # Example
    ///
    /// ```rust
    /// use komorebi_recorder::analysis::measure::Statistic;
    ///
    /// assert_eq!("peak".parse::<Statistic>(), Ok(Statistic::Peak));
    /// assert!("median".parse::<Statistic>().is_err());
    /// ```
    fn from_str(text: &str) -> Result<Self, String> {
        Self::ALL
            .into_iter()
            .find(|statistic| statistic.name() == text)
            .ok_or_else(|| format!("{text:?} is not a statistic; use mean, peak or width"))
    }
}

/// What one frame showed in the region, in pixels of the decoded frame.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct FrameStats {
    /// The mean brightness, 0 to 255.
    pub mean: f64,
    /// The brightest pixel, 0 to 255.
    pub peak: u8,
    /// The columns from the leftmost with a drawn pixel to the rightmost, counted; 0 if none.
    pub drawn_columns: u32,
    /// The leftmost column with a drawn pixel (0 if none is drawn).
    pub first_drawn_column: u32,
}

/// The most frequent value of `luma`; the lowest of equally frequent ones; 0 for no pixels.
///
/// # Example
///
/// ```rust
/// use komorebi_recorder::analysis::measure::background;
///
/// assert_eq!(background(&[9, 3, 9, 3, 7]), 3);
/// assert_eq!(background(&[]), 0);
/// ```
#[must_use]
pub fn background(luma: &[u8]) -> u8 {
    let mut counts = [0_usize; 256];
    for value in luma {
        counts[usize::from(*value)] += 1;
    }

    let mut best = 0;
    for (value, count) in counts.iter().enumerate() {
        if *count > counts[best] {
            best = value;
        }
    }
    best as u8
}

/// Measures the brightness of a patch `width` pixels across: its mean, its peak, and the columns
/// that the non-background pixels span (see the [module docs](self)). `background_override` is the
/// brightness to treat as the background instead of the patch's most frequent one.
#[must_use]
pub fn frame_stats(luma: &[u8], width: u32, background_override: Option<u8>) -> FrameStats {
    let sum: u64 = luma.iter().map(|value| u64::from(*value)).sum();
    let reference = i32::from(background_override.unwrap_or_else(|| background(luma)));
    let width = width.max(1);
    let (mut first, mut last) = (u32::MAX, 0_u32);
    for (index, value) in luma.iter().enumerate() {
        if (i32::from(*value) - reference).abs() > DRAWN_THRESHOLD {
            let column = index as u32 % width;
            first = first.min(column);
            last = last.max(column);
        }
    }

    let drawn = first != u32::MAX;
    FrameStats {
        mean: sum as f64 / luma.len().max(1) as f64,
        peak: luma.iter().copied().max().unwrap_or(0),
        drawn_columns: if drawn { last - first + 1 } else { 0 },
        first_drawn_column: if drawn { first } else { 0 },
    }
}

/// The times of a timeline: `from_s`, then every `1/hz` seconds, up to and including `to_s` if it
/// falls on one.
///
/// # Errors
///
/// Returns a usage error if `hz` is not above zero and finite, the range is backwards, or the
/// timeline would have more than [`MAX_SAMPLES`] samples.
///
/// # Example
///
/// ```rust
/// use komorebi_recorder::analysis::measure::timeline;
///
/// assert_eq!(timeline(1.0, 2.0, 4.0)?, [1.0, 1.25, 1.5, 1.75, 2.0]);
/// assert_eq!(timeline(0.0, 0.3, 10.0)?.len(), 4);
/// assert!(timeline(0.0, 1.0, 0.0).is_err());
/// # Ok::<(), Box<dyn std::error::Error>>(())
/// ```
pub fn timeline(from_s: f64, to_s: f64, hz: f64) -> eyre::Result<Vec<f64>> {
    if !(hz > 0.0 && hz.is_finite()) {
        return Err(envelope::usage("--hz must be above zero"));
    }
    if to_s < from_s {
        return Err(envelope::usage("the end of the range is before its start"));
    }

    let count = ((to_s - from_s) * hz + 1e-9).floor() + 1.0;
    if count > MAX_SAMPLES as f64 {
        return Err(envelope::usage(format!(
            "{count} samples is more than the {MAX_SAMPLES} allowed: use a shorter range or a lower --hz"
        )));
    }

    Ok((0..count as usize)
        .map(|index| from_s + index as f64 / hz)
        .collect())
}

/// A time of a timeline and the frame held then: its `shown_s` and value, if there was one.
pub type Held<T> = (f64, Option<(f64, T)>);

/// Puts `frames` (each a `(shown_s, value)`, in time order) on the [`timeline`] of `from_s`,
/// `to_s` and `hz`: each time gets `Some((shown_s, value))` of the last frame shown at or before
/// it, or `None` before the first frame.
///
/// # Errors
///
/// As [`timeline`].
pub fn resample<T: Copy>(
    frames: &[(f64, T)],
    from_s: f64,
    to_s: f64,
    hz: f64,
) -> eyre::Result<Vec<Held<T>>> {
    let mut next = 0;
    Ok(timeline(from_s, to_s, hz)?
        .into_iter()
        .map(|time| {
            // a hair of slack, so a frame at exactly a sample time is not lost to rounding
            while frames
                .get(next)
                .is_some_and(|(shown, _)| *shown <= time + 1e-9)
            {
                next += 1;
            }
            (time, next.checked_sub(1).map(|held| frames[held]))
        })
        .collect())
}

/// One sample of the timeline of a display.
#[derive(Debug, Clone, PartialEq, Serialize, JsonSchema)]
pub struct MeasureSample {
    /// The time of the sample, in seconds of video time.
    pub video_s: f64,
    /// When the frame the values are of was put on screen, in seconds of video time; never later
    /// than `video_s`. `None` if there was no frame yet or the display had no video.
    pub shown_s: Option<f64>,
    /// One value for each of [`MeasureData::statistics`], in that order; `None` where `shown_s` is.
    pub values: Vec<Option<f64>>,
}

/// The timeline of one display.
#[derive(Debug, Clone, PartialEq, Serialize, JsonSchema)]
pub struct MeasureSeries {
    /// The id of the display.
    pub display: u32,
    /// How many frames were decoded and measured for it.
    pub frames_examined: usize,
    /// The samples, `1/hz` apart.
    pub samples: Vec<MeasureSample>,
}

/// A statistic of a region on a fixed-rate timeline: the `data` of `measure`.
#[derive(Debug, Clone, PartialEq, Serialize, JsonSchema)]
pub struct MeasureData {
    /// The start of the range, in seconds of video time.
    pub from_video_s: f64,
    /// The end of the range, in seconds of video time.
    pub to_video_s: f64,
    /// Samples per second.
    pub hz: f64,
    /// The region, in global display points; `None` is the whole display.
    pub region: Option<Crop>,
    /// What each of a sample's `values` is, in order. `mean` and `peak` are brightness from 0 to
    /// 255; `width` is in display points.
    pub statistics: Vec<Statistic>,
    /// The brightness treated as the background for `width`, if it was given; otherwise the most
    /// frequent brightness of each frame's region.
    pub background: Option<u8>,
    /// The timeline of each display.
    pub series: Vec<MeasureSeries>,
    /// Stretches of the range where a display had no video at all.
    pub gaps: Vec<Gap>,
}

impl MeasureData {
    /// Renders the answer as a table, one row for each sample.
    ///
    /// # Example
    ///
    /// ```rust
    /// use komorebi_recorder::analysis::measure::{MeasureData, MeasureSample, MeasureSeries, Statistic};
    ///
    /// let data = MeasureData {
    ///     from_video_s: 1.0,
    ///     to_video_s: 1.5,
    ///     hz: 2.0,
    ///     region: None,
    ///     statistics: vec![Statistic::Mean, Statistic::Width],
    ///     background: None,
    ///     series: vec![MeasureSeries {
    ///         display: 1,
    ///         frames_examined: 2,
    ///         samples: vec![
    ///             MeasureSample { video_s: 1.0, shown_s: Some(0.9), values: vec![Some(12.5), Some(40.0)] },
    ///             MeasureSample { video_s: 1.5, shown_s: None, values: vec![None, None] },
    ///         ],
    ///     }],
    ///     gaps: vec![],
    /// };
    /// let text = data.to_text();
    /// assert!(text.contains("00:01.000"));
    /// assert!(text.contains("12.50"));
    /// ```
    #[must_use]
    pub fn to_text(&self) -> String {
        use crate::timecode::format as time;
        use std::fmt::Write;

        let mut out = String::new();
        let region = self.region.map_or_else(String::new, |crop| {
            format!(
                " in region {},{} {}x{}",
                crop.x, crop.y, crop.width, crop.height
            )
        });
        let _ = writeln!(
            out,
            "{} at {} Hz{region} from {} to {}",
            self.statistics
                .iter()
                .map(|statistic| statistic.name())
                .collect::<Vec<_>>()
                .join(", "),
            self.hz,
            time(self.from_video_s),
            time(self.to_video_s),
        );
        for series in &self.series {
            let _ = writeln!(
                out,
                "display {} ({} frames measured)",
                series.display, series.frames_examined
            );
            let mut header = format!("{:>10} {:>10}", "time", "shown");
            for statistic in &self.statistics {
                let _ = write!(header, " {:>10}", statistic.name());
            }
            let _ = writeln!(out, "{header}");
            for sample in &series.samples {
                let mut line = format!(
                    "{:>10} {:>10}",
                    time(sample.video_s),
                    sample.shown_s.map_or_else(|| "-".to_string(), time)
                );
                for value in &sample.values {
                    let _ = match value {
                        Some(value) => write!(line, " {value:>10.2}"),
                        None => write!(line, " {:>10}", "-"),
                    };
                }
                let _ = writeln!(out, "{line}");
            }
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

/// What to measure.
#[derive(Debug, Clone, Copy)]
pub struct MeasureOptions<'a> {
    /// The displays, range and region (the cancel flag is honoured too).
    pub scan: ScanOptions<'a>,
    /// What to measure, in the order of a sample's values; not empty.
    pub statistics: &'a [Statistic],
    /// Samples per second.
    pub hz: f64,
    /// The brightness to treat as the background for `width`, instead of the most frequent one.
    pub background: Option<u8>,
}

impl Analysis {
    /// Measures the region of `options` (the whole display if it has none) on each display, on the
    /// fixed-rate timeline the [module](self) describes.
    ///
    /// Stops early, without an error, once the scan's cancel flag is set; the samples it did not
    /// get to have no value.
    ///
    /// # Errors
    ///
    /// Returns an error if:
    ///
    /// - no statistic was asked for, `hz` is not above zero, or the timeline would be longer than
    ///   [`MAX_SAMPLES`] (the usage kind);
    /// - the region does not overlap a display, or a display's frame is unknown (the usage kind);
    /// - a video file cannot be resolved inside the session, opened or decoded, or the host has no
    ///   decoder.
    ///
    /// # Example
    ///
    /// This needs recorded video, so it is not run here; `tests/video.rs` runs it through the
    /// `measure` command.
    ///
    /// ```rust,no_run
    /// use komorebi_recorder::analysis::Analysis;
    /// use komorebi_recorder::analysis::measure::{DEFAULT_HZ, MeasureOptions, Statistic};
    /// use komorebi_recorder::analysis::motion::ScanOptions;
    /// use komorebi_recorder::analysis::pixels::Crop;
    /// use komorebi_recorder::analysis::video::SessionVideo;
    ///
    /// let analysis = Analysis::open(std::path::Path::new("session-dir"))?;
    /// let video = SessionVideo::new(&analysis.manifest, analysis.guard());
    /// let data = analysis.measure(
    ///     &video,
    ///     &MeasureOptions {
    ///         scan: ScanOptions {
    ///             displays: &video.displays(),
    ///             from_s: 0.0,
    ///             to_s: 10.0,
    ///             crop: Some(Crop::parse("100,100,800,600")?),
    ///             cancel: None,
    ///         },
    ///         statistics: &Statistic::ALL,
    ///         hz: DEFAULT_HZ,
    ///         background: None,
    ///     },
    /// )?;
    /// print!("{}", data.to_text());
    /// # Ok::<(), Box<dyn std::error::Error>>(())
    /// ```
    pub fn measure(
        &self,
        video: &SessionVideo,
        options: &MeasureOptions,
    ) -> eyre::Result<MeasureData> {
        if options.statistics.is_empty() {
            return Err(envelope::usage("measure needs at least one statistic"));
        }
        let (from_s, to_s) = (options.scan.from_s, options.scan.to_s);
        // fail on a bad rate or range before decoding anything
        timeline(from_s, to_s, options.hz)?;

        let mut series = Vec::new();
        for display in options.scan.displays {
            let frame = self.display_frame(*display);
            if frame.is_none()
                && (options.scan.crop.is_some() || options.statistics.contains(&Statistic::Width))
            {
                return Err(envelope::usage(format!(
                    "display {display} has no known frame, so a region or a width in points cannot be placed on it"
                )));
            }
            let (fractions, scale) = match &options.scan.crop {
                Some(crop) => {
                    let fractions = self.crop_fractions(*display, crop)?;
                    (fractions, scale_for(fractions[2]))
                }
                None => ([0.0, 0.0, 1.0, 1.0], scale_for(1.0)),
            };

            // what each frame showed: (shown_s, stats, points per pixel of the decoded frame)
            let mut frames: Vec<(f64, (FrameStats, f64))> = Vec::new();
            let measure_frame = |decoded: &super::video::DecodedFrame| {
                let (width, height) = (decoded.frame.width, decoded.frame.height);
                let rect = pixels::pixel_rect(fractions, width, height);
                let patch = pixels::crop_luma(&pixels::luma(&decoded.frame.bgra), width, rect);
                let points = frame.map_or(1.0, |frame| frame[2] / f64::from(width.max(1)));
                (
                    decoded.shown_s,
                    (frame_stats(&patch, rect.width, options.background), points),
                )
            };

            // the frame on screen when the range starts may have been shown long before it, which
            // a scan does not visit, so it is asked for
            let mut examined = 0;
            if let Some(seed) = video.frame_at(*display, from_s, scale)? {
                examined += 1;
                frames.push(measure_frame(&seed));
            }
            let seeded = frames.last().map_or(f64::NEG_INFINITY, |(shown, _)| *shown);
            let mut scanned = Vec::new();
            examined += video.scan(*display, from_s, to_s, scale, &mut |decoded| {
                if options
                    .scan
                    .cancel
                    .is_some_and(|cancel| cancel.load(std::sync::atomic::Ordering::Relaxed))
                {
                    return false;
                }
                if decoded.shown_s > seeded + 1e-9 {
                    scanned.push(measure_frame(decoded));
                }
                true
            })?;
            frames.extend(scanned);

            let samples = resample(&frames, from_s, to_s, options.hz)?
                .into_iter()
                .map(|(time, held)| {
                    // the last frame of a segment is not on screen once the display has no video
                    let held = held.filter(|_| video.has_video_at(*display, time));
                    MeasureSample {
                        video_s: time,
                        shown_s: held.map(|(shown, _)| shown),
                        values: options
                            .statistics
                            .iter()
                            .map(|statistic| {
                                held.map(|(_, (stats, points))| match statistic {
                                    Statistic::Mean => stats.mean,
                                    Statistic::Peak => f64::from(stats.peak),
                                    Statistic::Width => f64::from(stats.drawn_columns) * points,
                                })
                            })
                            .collect(),
                    }
                })
                .collect();
            series.push(MeasureSeries {
                display: *display,
                frames_examined: examined,
                samples,
            });
        }

        Ok(MeasureData {
            from_video_s: from_s,
            to_video_s: to_s,
            hz: options.hz,
            region: options.scan.crop,
            statistics: options.statistics.to_vec(),
            background: options.background,
            series,
            gaps: self.video_gaps(options.scan.displays, from_s, to_s),
        })
    }
}

/// The width to decode a frame at so that a region of fractional width `fraction` has
/// [`REGION_PIXELS`] across.
fn scale_for(fraction: f64) -> Scale {
    let wanted = (REGION_PIXELS / fraction.max(1e-6)).ceil();
    Scale::Width((wanted as u32).clamp(REGION_PIXELS as u32, MAX_WIDTH))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_uniform_patch_has_no_drawn_width() {
        let stats = frame_stats(&[80; 12], 4, None);
        assert_eq!((stats.mean, stats.peak, stats.drawn_columns), (80.0, 80, 0));
    }

    #[test]
    fn the_drawn_width_spans_from_the_first_to_the_last_drawn_column() {
        // 6 x 2: background 0; bright pixels in columns 1 (row 0) and 4 (row 1)
        let mut patch = [0_u8; 12];
        patch[1] = 255;
        patch[6 + 4] = 100;
        let stats = frame_stats(&patch, 6, None);
        assert_eq!((stats.first_drawn_column, stats.drawn_columns), (1, 4));
        assert_eq!(stats.peak, 255);
        assert!((stats.mean - 355.0 / 12.0).abs() < 1e-9);
    }

    #[test]
    fn a_change_of_no_more_than_the_threshold_is_not_drawn() {
        let stats = frame_stats(&[0, 0, 0, 24, 25, 0, 0, 0], 8, None);
        assert_eq!((stats.first_drawn_column, stats.drawn_columns), (4, 1));
    }

    #[test]
    fn the_background_is_the_most_frequent_brightness_and_can_be_given() {
        // mostly white, with a dark bar of 2 pixels
        let patch = [255, 255, 10, 10, 255, 255, 255, 255];
        assert_eq!(frame_stats(&patch, 8, None).drawn_columns, 2);
        // taking black as the background makes the white the drawing
        assert_eq!(frame_stats(&patch, 8, Some(0)).drawn_columns, 8 - 2 + 2);
        assert_eq!(background(&[1, 2]), 1, "a tie is the lower");
    }

    #[test]
    fn the_timeline_is_fixed_rate_and_includes_the_end_when_it_falls_on_a_sample() {
        assert_eq!(timeline(0.0, 1.0, 2.0).unwrap(), [0.0, 0.5, 1.0]);
        assert_eq!(timeline(0.0, 0.9, 2.0).unwrap(), [0.0, 0.5]);
        assert_eq!(timeline(2.0, 2.0, 30.0).unwrap(), [2.0]);
    }

    #[test]
    fn a_bad_rate_range_or_size_is_refused() {
        assert!(timeline(0.0, 1.0, -1.0).is_err());
        assert!(timeline(0.0, 1.0, f64::NAN).is_err());
        assert!(timeline(2.0, 1.0, 30.0).is_err());
        assert!(timeline(0.0, 1e6, 30.0).is_err());
    }

    #[test]
    fn a_sample_takes_the_last_frame_shown_at_or_before_it() {
        // irregular frames: a burst, then a long hold
        let frames = [(0.10, 1), (0.12, 2), (0.14, 3), (1.0, 4)];
        let held = resample(&frames, 0.0, 1.0, 4.0).unwrap();
        let values: Vec<_> = held.iter().map(|(_, h)| h.map(|(_, v)| v)).collect();
        // samples at 0, .25, .5, .75, 1.0
        assert_eq!(values, [None, Some(3), Some(3), Some(3), Some(4)]);
        assert_eq!(held[1].1, Some((0.14, 3)));
    }

    #[test]
    fn a_frame_exactly_on_a_sample_time_counts_for_it() {
        let held = resample(&[(0.5, 'x')], 0.0, 0.5, 2.0).unwrap();
        assert_eq!(held[1].1, Some((0.5, 'x')));
        assert_eq!(held[0].1, None);
    }

    #[test]
    fn no_frames_leave_every_sample_empty() {
        let held = resample::<u8>(&[], 0.0, 1.0, 3.0).unwrap();
        assert_eq!(held.len(), 4);
        assert!(held.iter().all(|(_, frame)| frame.is_none()));
    }

    #[test]
    fn statistics_are_named_the_way_they_are_parsed() {
        for statistic in Statistic::ALL {
            assert_eq!(statistic.name().parse::<Statistic>(), Ok(statistic));
        }
    }
}
