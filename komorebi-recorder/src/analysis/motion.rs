//! `motion`: when the picture of a display, or of a part of it, was moving.
//!
//! This is [`changes`](super::changes) with a region: the same frame-to-frame comparison, but it
//! can be limited to a rectangle of a display ([`ScanOptions::crop`], in display points), and each
//! segment of activity reports how hard the picture moved as well as how much of it did.
//!
//! A segment is a run of frames in which at least 0.05% (divided by the sensitivity) of the
//! compared pixels changed by more than 24 of 255 brightness, with frames closer than 0.15 s
//! belonging to one segment; see [`super::changes`] for the details. Per segment it reports:
//!
//! - `peak_share`: the largest share of the compared pixels that changed between two frames;
//! - `peak_mean_delta`: the largest mean brightness difference (0 to 255) between two frames,
//!   which tells a faint flicker over the whole region from a few pixels flipping fully.
//!
//! Where a region is given the compared pixels are those of the region only, so `peak_share` is a
//! share of the region. The box of change is still given as a fraction of the display and in
//! display points, so it can be used with `--region` of the other commands.
//!
//! A region is cut from a frame decoded wide enough to give it at least 240 pixels across, so a
//! small region costs more to scan than a whole display.

use super::Analysis;
use super::changes::Gap;
use super::changes::Tracking;
use super::changes::fractions_of;
use super::changes::points_of;
use super::changes::track;
use super::envelope;
use super::pixels::Crop;
use super::video::SessionVideo;
use crate::av::Scale;
use color_eyre::eyre;
use schemars::JsonSchema;
use serde::Serialize;
use std::sync::atomic::AtomicBool;

/// The schema name that identifies a [`MotionData`] in an envelope (`motion/1`).
pub const SCHEMA_MOTION: &str = "motion/1";

/// Whole displays are compared at this width, like `changes`.
const WHOLE_WIDTH: u32 = 240;

/// A region is decoded wide enough to have this many pixels across.
const REGION_PIXELS: f64 = 240.0;

/// The widest a frame is decoded for a region.
const MAX_WIDTH: u32 = 3840;

/// What to scan: shared by `motion` and `blank`.
///
/// `crop` is where a window's rect goes, once a window track can say where a window was: it is in
/// the same global display points as a `bbox_points`, so a [`Crop::from_rect`] of a window's rect
/// is all it takes.
#[derive(Debug, Clone, Copy)]
pub struct ScanOptions<'a> {
    /// The displays to scan; each must be a display of the session.
    pub displays: &'a [u32],
    /// The start of the range, in seconds of video time.
    pub from_s: f64,
    /// The end of the range, in seconds of video time.
    pub to_s: f64,
    /// Only this part of each display, in global display points; the whole display if `None`.
    pub crop: Option<Crop>,
    /// Set from another thread to stop early with what was found so far.
    pub cancel: Option<&'a AtomicBool>,
}

/// A stretch of video in which the picture of a display, or of a region of it, was moving.
#[derive(Debug, Clone, PartialEq, Serialize, JsonSchema)]
pub struct MotionSegment {
    /// The id of the display.
    pub display: u32,
    /// When the first changed frame was on screen, in seconds of video time.
    pub start_video_s: f64,
    /// When the last changed frame was on screen, in seconds of video time.
    pub end_video_s: f64,
    /// `end_video_s - start_video_s`; zero for a single changed frame.
    pub duration_s: f64,
    /// Changed frames in the segment.
    pub frames: usize,
    /// The largest share of the compared pixels (the region's, if there is one) that changed
    /// between two consecutive frames.
    pub peak_share: f64,
    /// The largest mean brightness difference, 0 to 255, over the compared pixels between two
    /// consecutive frames.
    pub peak_mean_delta: f64,
    /// The box around everything that changed, as fractions of the display: `[x, y, width, height]`.
    pub bbox: [f64; 4],
    /// The same box in global display points, if the display's frame is known.
    pub bbox_points: Option<[f64; 4]>,
    /// The `seq` of each komorebi event received within 0.25 s of the segment.
    pub events: Vec<u64>,
}

/// Where a display, or a region of it, was moving over a range: the `data` of `motion`.
#[derive(Debug, Clone, PartialEq, Serialize, JsonSchema)]
pub struct MotionData {
    /// The start of the range scanned, in seconds of video time.
    pub from_video_s: f64,
    /// The end of the range scanned, in seconds of video time.
    pub to_video_s: f64,
    /// The displays scanned.
    pub displays: Vec<u32>,
    /// The region compared, in global display points, if the scan was limited to one.
    pub region: Option<Crop>,
    /// The sensitivity asked for.
    pub sensitivity: f64,
    /// How many frames were decoded and looked at, over all displays. A cancelled scan counts
    /// only what it got through.
    pub frames_examined: usize,
    /// The segments of activity, ordered by start and then display.
    pub segments: Vec<MotionSegment>,
    /// Stretches of the range where a display had no video at all.
    pub gaps: Vec<Gap>,
}

impl MotionData {
    /// The total time covered by segments, in seconds, counting each display's segments (they
    /// never overlap within a display) and adding displays together.
    #[must_use]
    pub fn active_s(&self) -> f64 {
        self.segments.iter().map(|segment| segment.duration_s).sum()
    }

    /// Renders the answer as text, one line for each segment.
    ///
    /// # Example
    ///
    /// ```rust
    /// use komorebi_recorder::analysis::motion::{MotionData, MotionSegment};
    ///
    /// let data = MotionData {
    ///     from_video_s: 0.0,
    ///     to_video_s: 6.0,
    ///     displays: vec![1],
    ///     region: None,
    ///     sensitivity: 1.0,
    ///     frames_examined: 100,
    ///     segments: vec![MotionSegment {
    ///         display: 1,
    ///         start_video_s: 1.0,
    ///         end_video_s: 1.5,
    ///         duration_s: 0.5,
    ///         frames: 10,
    ///         peak_share: 0.25,
    ///         peak_mean_delta: 12.5,
    ///         bbox: [0.0, 0.0, 1.0, 1.0],
    ///         bbox_points: None,
    ///         events: vec![],
    ///     }],
    ///     gaps: vec![],
    /// };
    /// let text = data.to_text();
    /// assert!(text.starts_with("100 frames compared on display(s) 1"));
    /// assert!(text.contains("peak 25.0% of the pixels, mean delta 12.5"));
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
            "{} frames compared on display(s) {}{region} from {} to {}: {} segment(s) of motion, {:.2} s in all",
            self.frames_examined,
            self.displays
                .iter()
                .map(ToString::to_string)
                .collect::<Vec<_>>()
                .join(", "),
            time(self.from_video_s),
            time(self.to_video_s),
            self.segments.len(),
            self.active_s(),
        );
        for segment in &self.segments {
            let events = if segment.events.is_empty() {
                String::new()
            } else {
                format!(
                    "  events {}",
                    segment
                        .events
                        .iter()
                        .map(|seq| format!("#{seq}"))
                        .collect::<Vec<_>>()
                        .join(" ")
                )
            };
            let _ = writeln!(
                out,
                "display {}  {} - {} ({:.2} s, {} frames)  peak {:.1}% of the pixels, mean delta {:.1}  box {:.0}%,{:.0}% {:.0}%x{:.0}%{events}",
                segment.display,
                time(segment.start_video_s),
                time(segment.end_video_s),
                segment.duration_s,
                segment.frames,
                segment.peak_share * 100.0,
                segment.peak_mean_delta,
                segment.bbox[0] * 100.0,
                segment.bbox[1] * 100.0,
                segment.bbox[2] * 100.0,
                segment.bbox[3] * 100.0,
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

/// The width to decode a frame at so that a region of fractional width `fraction` is at least
/// [`REGION_PIXELS`] across.
pub(super) fn scale_for(fraction: f64) -> Scale {
    let wanted = (REGION_PIXELS / fraction.max(1e-6)).ceil();
    Scale::Width((wanted as u32).clamp(WHOLE_WIDTH, MAX_WIDTH))
}

impl Analysis {
    /// The frame of a display in global points, `[x, y, width, height]`, if the session knows it.
    pub(super) fn display_frame(&self, display: u32) -> Option<[f64; 4]> {
        self.manifest
            .displays
            .iter()
            .find(|entry| entry.result.display.id == display)
            .map(|entry| entry.result.display.frame)
    }

    /// Turns a crop in points into the fractions of `display`.
    pub(super) fn crop_fractions(&self, display: u32, crop: &Crop) -> eyre::Result<[f64; 4]> {
        let frame = self.display_frame(display).ok_or_else(|| {
            envelope::usage(format!(
                "display {display} has no known frame, so a region in points cannot be placed on it"
            ))
        })?;
        crop.fractions(frame)
    }

    /// Finds the segments of activity on each display of `options`, in the whole display or in
    /// `options.crop`, with `sensitivity` as for [`Analysis::changes`].
    ///
    /// Stops early, without an error, once `options.cancel` is set; [`MotionData::frames_examined`]
    /// says how far it got. A display with no video in the range contributes no segments and is
    /// reported in [`MotionData::gaps`].
    ///
    /// # Errors
    ///
    /// Returns an error if:
    ///
    /// - `sensitivity` is not above zero (the usage exit kind);
    /// - the crop does not overlap a display, or a display's frame is unknown (the usage kind);
    /// - a video file cannot be resolved inside the session, opened or decoded, or the host has no
    ///   decoder.
    ///
    /// # Example
    ///
    /// This needs recorded video, so it is not run here. `tests/video.rs` runs it through the
    /// `motion` command on sessions with numbered videos.
    ///
    /// ```rust,no_run
    /// use komorebi_recorder::analysis::Analysis;
    /// use komorebi_recorder::analysis::motion::ScanOptions;
    /// use komorebi_recorder::analysis::pixels::Crop;
    /// use komorebi_recorder::analysis::video::SessionVideo;
    ///
    /// let analysis = Analysis::open(std::path::Path::new("session-dir"))?;
    /// let video = SessionVideo::new(&analysis.manifest, analysis.guard());
    /// let data = analysis.motion(
    ///     &video,
    ///     &ScanOptions {
    ///         displays: &video.displays(),
    ///         from_s: 0.0,
    ///         to_s: 60.0,
    ///         crop: Some(Crop::parse("100,100,800,600")?),
    ///         cancel: None,
    ///     },
    ///     1.0,
    /// )?;
    /// print!("{}", data.to_text());
    /// # Ok::<(), Box<dyn std::error::Error>>(())
    /// ```
    pub fn motion(
        &self,
        video: &SessionVideo,
        options: &ScanOptions,
        sensitivity: f64,
    ) -> eyre::Result<MotionData> {
        if sensitivity <= 0.0 {
            return Err(envelope::usage("--sensitivity must be above zero"));
        }

        let threshold = super::changes::SHARE_THRESHOLD / sensitivity;
        let mut segments = Vec::new();
        let mut examined = 0;
        for display in options.displays {
            let (crop, scale) = match &options.crop {
                Some(crop) => {
                    let fractions = self.crop_fractions(*display, crop)?;
                    (Some(fractions), scale_for(fractions[2]))
                }
                None => (None, Scale::Width(WHOLE_WIDTH)),
            };
            let tracked = track(
                video,
                *display,
                options.from_s,
                options.to_s,
                &Tracking {
                    threshold,
                    crop,
                    scale,
                    cancel: options.cancel,
                },
            )?;
            examined += tracked.examined;

            let frame_box = self.display_frame(*display);
            for span in tracked.spans {
                let bbox = fractions_of(span.bounds, tracked.dimensions);
                segments.push(MotionSegment {
                    display: *display,
                    start_video_s: span.start,
                    end_video_s: span.end,
                    duration_s: span.end - span.start,
                    frames: span.frames,
                    peak_share: span.peak,
                    peak_mean_delta: span.peak_delta,
                    bbox,
                    bbox_points: points_of(frame_box, bbox),
                    events: self.events_between(span.start, span.end),
                });
            }
        }

        segments.sort_by(|a, b| {
            a.start_video_s
                .total_cmp(&b.start_video_s)
                .then(a.display.cmp(&b.display))
        });

        Ok(MotionData {
            from_video_s: options.from_s,
            to_video_s: options.to_s,
            displays: options.displays.to_vec(),
            region: options.crop,
            sensitivity,
            frames_examined: examined,
            segments,
            gaps: self.video_gaps(options.displays, options.from_s, options.to_s),
        })
    }

    /// The `seq` of each event received within a quarter of a second of `start_s..=end_s`.
    fn events_between(&self, start_s: f64, end_s: f64) -> Vec<u64> {
        self.events()
            .iter()
            .filter(|event| {
                let at = self.video_s(event.recv_mach_ns);
                at >= start_s - 0.25 && at <= end_s + 0.25
            })
            .map(|event| event.seq)
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_narrow_region_is_decoded_wider_but_never_past_the_limit() {
        assert_eq!(scale_for(1.0), Scale::Width(240));
        assert_eq!(scale_for(0.5), Scale::Width(480));
        assert_eq!(scale_for(0.0001), Scale::Width(MAX_WIDTH));
    }

    #[test]
    fn active_time_adds_up_the_segments() {
        let segment = |start: f64, end: f64| MotionSegment {
            display: 1,
            start_video_s: start,
            end_video_s: end,
            duration_s: end - start,
            frames: 1,
            peak_share: 0.0,
            peak_mean_delta: 0.0,
            bbox: [0.0; 4],
            bbox_points: None,
            events: vec![],
        };
        let data = MotionData {
            from_video_s: 0.0,
            to_video_s: 10.0,
            displays: vec![1],
            region: None,
            sensitivity: 1.0,
            frames_examined: 0,
            segments: vec![segment(1.0, 1.5), segment(3.0, 4.0)],
            gaps: vec![],
        };
        assert_eq!(data.active_s(), 1.5);
    }
}
