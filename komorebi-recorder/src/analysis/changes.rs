//! When, and where on the screen, the picture changed.
//!
//! komorebi's events say when it thought something happened; the video says when the screen
//! actually did. Comparing consecutive frames finds the stretches where pixels changed, how much
//! of the screen they covered and the box around them, so an investigator can see whether a layout
//! change was instant or took half a second, whether a window jumped, and what else on screen was
//! going on. Where an event falls inside a stretch of change it is named.
//!
//! [`Analysis::changes`] is what the `changes` command prints and what the viewer scans in the
//! background ([`Analysis::changes_until`]) to draw the screen lane of its timeline.
//! [`Analysis::video_gaps`] reports where a display had no video to compare at all.
//!
//! This is about pixels, not about komorebi's state: a [`ChangeSpan`] reports that the picture
//! moved, not what moved. The differences between two komorebi states are in [`super::diff`].
//!
//! # How a change is found
//!
//! Each display is decoded separately and every frame is compared with the one before it:
//!
//! - Frames are scaled to 240 pixels wide and reduced to brightness (Rec. 601 weights).
//! - A pixel has changed if its brightness moved by more than 24 (of 255).
//! - A frame is a change if the share of its pixels that changed is at least 0.05% divided by the
//!   `sensitivity` asked for: with a sensitivity of 2, changes half as large are noticed, and with
//!   0.5 only changes twice as large.
//! - Changed frames less than 0.15 s apart belong to one [`ChangeSpan`].
//! - A frame is compared with the one before it on the same display, across the start of a new
//!   segment too (a display that came back after being unplugged or asleep), so a screen that
//!   looks different after the break is a change at the boundary. A frame of another size than
//!   the one before it (a display that changed resolution) is not compared with it.
//!
//! Frames come when the screen changes, not on a schedule, so a display whose video starts less
//! than 0.5 s after the range starts, or stops less than 0.5 s before it ends, is not reported as
//! having a [`Gap`].
//!
//! Decoding needs a video decoder on the host: where there is none, the functions here that read
//! frames return an error saying so, and everything else in a session can still be read.

use super::Analysis;
use super::envelope;
use super::pixels;
use super::video::SessionVideo;
use crate::av::Scale;
use color_eyre::eyre;
use schemars::JsonSchema;
use serde::Serialize;

/// The schema name that identifies a [`ChangesData`] in an envelope (`changes/1`); listed by the
/// `schema` command.
pub const SCHEMA_CHANGES: &str = "changes/1";

/// Frames are compared at this width: enough to see a window move, cheap enough to scan minutes.
const COMPARE_WIDTH: u32 = 240;

/// A pixel counts as changed if its brightness moved by more than this (out of 255).
pub(super) const PIXEL_THRESHOLD: i32 = 24;

/// A frame is a change if at least this share of its pixels changed, at sensitivity 1.
pub(super) const SHARE_THRESHOLD: f64 = 0.0005;

/// Frames of one change closer together than this are one change.
const MERGE_GAP_S: f64 = 0.15;

/// A display whose first frame arrives this late, or whose video stops this early, is not
/// reported as having a gap: frames come when the screen changes, not on a schedule.
const MIN_GAP_S: f64 = 0.5;

/// An event this close to a change is said to coincide with it.
const COINCIDE_S: f64 = 0.25;

/// A stretch of video in which the picture of one display changed.
///
/// A span of a single changed frame has `start_video_s == end_video_s`.
#[derive(Debug, Clone, PartialEq, Serialize, JsonSchema)]
pub struct ChangeSpan {
    /// The id of the display, the same number as [`Monitor::id`](super::state::Monitor::id) and
    /// the `--display` of the recording.
    pub display: u32,
    /// When the first changed frame was on screen, in seconds of video time.
    pub start_video_s: f64,
    /// When the last changed frame was on screen, in seconds of video time.
    pub end_video_s: f64,
    /// Changed frames in the span: not the frames that fall between its start and end.
    pub frames: usize,
    /// The largest share of the screen that changed between two consecutive frames.
    pub peak_share: f64,
    /// The box around everything that changed, as fractions of the screen: `[x, y, width, height]`.
    /// The fractions are of the size of the last frame decoded for the display.
    pub bbox: [f64; 4],
    /// The same box in global display points, if the display's frame is known:
    /// `[x, y, width, height]`.
    pub bbox_points: Option<[f64; 4]>,
    /// The `seq` of each komorebi event received from 0.25 s before the span starts to 0.25 s
    /// after it ends. An event is received when its handler has finished.
    pub events: Vec<u64>,
}

/// A stretch of a range in which a display had no video at all.
///
/// A display has none while it is unplugged or asleep, or after its capture failed.
#[derive(Debug, Clone, PartialEq, Serialize, JsonSchema)]
pub struct Gap {
    /// The id of the display.
    pub display: u32,
    /// Where the gap starts, in seconds of video time.
    pub from_video_s: f64,
    /// Where the gap ends, in seconds of video time.
    pub to_video_s: f64,
}

/// Where the picture changed on each display over a range of the recording: the `data` of the
/// `changes` command.
#[derive(Debug, Clone, PartialEq, Serialize, JsonSchema)]
pub struct ChangesData {
    /// The start of the range that was scanned, in seconds of video time.
    pub from_video_s: f64,
    /// The end of the range that was scanned, in seconds of video time.
    pub to_video_s: f64,
    /// The displays that were scanned, in the order asked for.
    pub displays: Vec<u32>,
    /// How many frames were decoded and looked at, over all displays. A scan that was cancelled
    /// counts only what it got through.
    pub frames_examined: usize,
    /// The changes, ordered by start time and then by display.
    pub spans: Vec<ChangeSpan>,
    /// Stretches of the range where a display had no video at all (unplugged, asleep). See
    /// [`Analysis::video_gaps`].
    pub gaps: Vec<Gap>,
}

/// One stretch of change being built: shared by `changes` and `motion`.
pub(super) struct Open {
    pub(super) start: f64,
    pub(super) end: f64,
    pub(super) frames: usize,
    pub(super) peak: f64,
    /// The largest mean brightness difference between two consecutive frames in the stretch.
    pub(super) peak_delta: f64,
    /// `(min_x, min_y, max_x, max_y)` in pixels of the decoded frame.
    pub(super) bounds: (u32, u32, u32, u32),
}

/// Groups changed frames into stretches: frames less than [`MERGE_GAP_S`] apart are one.
#[derive(Default)]
pub(super) struct Segments {
    open: Option<Open>,
    finished: Vec<Open>,
}

impl Segments {
    /// Adds a changed frame shown at `shown_s`, `share` of whose compared pixels changed, whose
    /// mean brightness difference from the frame before was `delta` and whose changed pixels lie
    /// in `bounds`.
    pub(super) fn push(
        &mut self,
        shown_s: f64,
        share: f64,
        delta: f64,
        bounds: (u32, u32, u32, u32),
    ) {
        match &mut self.open {
            Some(span) if shown_s - span.end <= MERGE_GAP_S => {
                span.end = shown_s;
                span.frames += 1;
                span.peak = span.peak.max(share);
                span.peak_delta = span.peak_delta.max(delta);
                span.bounds = (
                    span.bounds.0.min(bounds.0),
                    span.bounds.1.min(bounds.1),
                    span.bounds.2.max(bounds.2),
                    span.bounds.3.max(bounds.3),
                );
            }
            _ => {
                self.finished.extend(self.open.take());
                self.open = Some(Open {
                    start: shown_s,
                    end: shown_s,
                    frames: 1,
                    peak: share,
                    peak_delta: delta,
                    bounds,
                });
            }
        }
    }

    /// The stretches, in order, including the one still open.
    pub(super) fn finish(mut self) -> Vec<Open> {
        self.finished.extend(self.open.take());
        self.finished
    }
}

/// How [`track`] looks for change.
pub(super) struct Tracking<'a> {
    /// The share of the compared pixels that must change for a frame to count.
    pub(super) threshold: f64,
    /// Only compare this part of the frame, as fractions `[x, y, width, height]` of it.
    pub(super) crop: Option<[f64; 4]>,
    /// What size to decode at.
    pub(super) scale: Scale,
    /// Stop early once this is set.
    pub(super) cancel: Option<&'a std::sync::atomic::AtomicBool>,
}

/// What [`track`] found on one display.
pub(super) struct Tracked {
    pub(super) spans: Vec<Open>,
    /// The size of the last frame decoded.
    pub(super) dimensions: Option<(u32, u32)>,
    pub(super) examined: usize,
}

/// Decodes `display` over `from_s..=to_s` and finds the stretches in which it changed: the engine
/// of `changes` and `motion`, described in the [module](self) documentation.
pub(super) fn track(
    video: &SessionVideo,
    display: u32,
    from_s: f64,
    to_s: f64,
    how: &Tracking,
) -> eyre::Result<Tracked> {
    let mut previous: Option<(Vec<u8>, u32, u32)> = None;
    let mut segments = Segments::default();

    let examined = video.scan(display, from_s, to_s, how.scale, &mut |decoded| {
        if how
            .cancel
            .is_some_and(|cancel| cancel.load(std::sync::atomic::Ordering::Relaxed))
        {
            return false;
        }
        let (width, height) = (decoded.frame.width, decoded.frame.height);
        let whole = pixels::luma(&decoded.frame.bgra);
        let rect = how.crop.map(|crop| pixels::pixel_rect(crop, width, height));
        let now = match rect {
            Some(rect) => pixels::crop_luma(&whole, width, rect),
            None => whole,
        };
        let (row, origin) = rect.map_or((width, (0, 0)), |rect| (rect.width, (rect.x, rect.y)));

        if let Some((before, before_width, before_height)) = &previous
            && (*before_width, *before_height) == (width, height)
        {
            let found = pixels::delta(before, &now, row, PIXEL_THRESHOLD);
            let share = found.share();
            if share >= how.threshold {
                let (min_x, min_y, max_x, max_y) = found.bounds.unwrap_or((0, 0, 0, 0));
                let (min_x, min_y, max_x, max_y) = (
                    min_x + origin.0,
                    min_y + origin.1,
                    max_x + origin.0,
                    max_y + origin.1,
                );
                segments.push(
                    decoded.shown_s,
                    share,
                    found.mean,
                    (min_x, min_y, max_x, max_y),
                );
            }
        }

        previous = Some((now, width, height));
        true
    })?;

    Ok(Tracked {
        spans: segments.finish(),
        dimensions: previous.map(|(_, width, height)| (width, height)),
        examined,
    })
}

/// The box `bounds` (pixels) as fractions of a `dimensions` frame: `[x, y, width, height]`.
pub(super) fn fractions_of(
    bounds: (u32, u32, u32, u32),
    dimensions: Option<(u32, u32)>,
) -> [f64; 4] {
    let (width, height) = dimensions.unwrap_or((1, 1));
    let (w, h) = (f64::from(width), f64::from(height));
    [
        f64::from(bounds.0) / w,
        f64::from(bounds.1) / h,
        f64::from(bounds.2 - bounds.0 + 1) / w,
        f64::from(bounds.3 - bounds.1 + 1) / h,
    ]
}

/// Fractions of a display's frame `[x, y, width, height]` in global points.
pub(super) fn points_of(frame_box: Option<[f64; 4]>, bbox: [f64; 4]) -> Option<[f64; 4]> {
    frame_box.map(|[x, y, frame_width, frame_height]| {
        [
            x + bbox[0] * frame_width,
            y + bbox[1] * frame_height,
            bbox[2] * frame_width,
            bbox[3] * frame_height,
        ]
    })
}

impl Analysis {
    /// Finds where the picture changed on each of `displays` in `from_s..=to_s` (seconds of video
    /// time).
    ///
    /// `sensitivity` scales how little has to change for a frame to count: the default of the
    /// `changes` command is `1.0`, a larger number notices smaller changes and a smaller one only
    /// larger changes. How a change is found is described in the [module](self) documentation.
    /// A display with no video in the range contributes no spans and is reported in
    /// [`ChangesData::gaps`]; so is one that is not in the session at all.
    ///
    /// This decodes every frame of the range, so its cost is that of decoding the video, which was
    /// measured at 250 to 970 frames per second on macOS (HEVC, depending on frame size). `video`
    /// belongs to the thread that made it (a `SessionVideo` is not `Send`), so a scan on another
    /// thread builds a new `SessionVideo`.
    ///
    /// # Errors
    ///
    /// Returns an error if:
    ///
    /// - `sensitivity` is not above zero (the usage exit kind);
    /// - a display's video file cannot be resolved inside the session, opened or decoded (missing,
    ///   truncated or unreadable), or the host has no video decoder.
    ///
    /// # Example
    ///
    /// This needs recorded video, so it is not run here. It is covered by
    /// `tests/video.rs::the_picture_changing_is_found_where_it_changed_and_not_where_it_held_still`
    /// and `tests/video.rs::a_display_with_no_video_for_part_of_the_range_has_a_gap`, which build
    /// sessions with numbered videos and run the `changes` command over them.
    ///
    /// ```rust,no_run
    /// use komorebi_recorder::analysis::Analysis;
    /// use komorebi_recorder::analysis::guard::SessionPath;
    /// use komorebi_recorder::analysis::video::SessionVideo;
    ///
    /// let analysis = Analysis::open(std::path::Path::new("session-dir"))?;
    /// let guard = SessionPath::new(&analysis.session.dir)?;
    /// let video = SessionVideo::new(&analysis.manifest, &guard);
    ///
    /// let data = analysis.changes(&video, &video.displays(), 0.0, 60.0, 1.0)?;
    /// for span in &data.spans {
    ///     println!(
    ///         "display {}: {:.2} s to {:.2} s, events {:?}",
    ///         span.display, span.start_video_s, span.end_video_s, span.events
    ///     );
    /// }
    /// # Ok::<(), Box<dyn std::error::Error>>(())
    /// ```
    pub fn changes(
        &self,
        video: &SessionVideo,
        displays: &[u32],
        from_s: f64,
        to_s: f64,
        sensitivity: f64,
    ) -> eyre::Result<ChangesData> {
        self.changes_until(video, displays, from_s, to_s, sensitivity, None)
    }

    /// Finds where the picture changed, like [`Analysis::changes`], and stops early with what it
    /// has found so far once `cancel` is set: for a scan that runs in the background and whose
    /// result is no longer wanted.
    ///
    /// `cancel` is checked once per decoded frame, so a scan stops within a frame of it being set.
    /// A cancelled scan is not an error: it returns the spans of what it got through, and
    /// [`ChangesData::frames_examined`] says how far that was. With `None` it runs to the end,
    /// which is what [`Analysis::changes`] does.
    ///
    /// # Errors
    ///
    /// The same as [`Analysis::changes`].
    pub fn changes_until(
        &self,
        video: &SessionVideo,
        displays: &[u32],
        from_s: f64,
        to_s: f64,
        sensitivity: f64,
        cancel: Option<&std::sync::atomic::AtomicBool>,
    ) -> eyre::Result<ChangesData> {
        if sensitivity <= 0.0 {
            return Err(envelope::usage("--sensitivity must be above zero"));
        }

        let threshold = SHARE_THRESHOLD / sensitivity;
        let mut spans = Vec::new();
        let mut examined = 0;

        for display in displays {
            let frame_box = self
                .manifest
                .displays
                .iter()
                .find(|entry| entry.result.display.id == *display)
                .map(|entry| entry.result.display.frame);

            let tracked = track(
                video,
                *display,
                from_s,
                to_s,
                &Tracking {
                    threshold,
                    crop: None,
                    scale: Scale::Width(COMPARE_WIDTH),
                    cancel,
                },
            )?;
            examined += tracked.examined;

            for span in tracked.spans {
                let bbox = fractions_of(span.bounds, tracked.dimensions);
                let events = self
                    .events()
                    .iter()
                    .filter(|event| {
                        let at = self.video_s(event.recv_mach_ns);
                        at >= span.start - COINCIDE_S && at <= span.end + COINCIDE_S
                    })
                    .map(|event| event.seq)
                    .collect();

                spans.push(ChangeSpan {
                    display: *display,
                    start_video_s: span.start,
                    end_video_s: span.end,
                    frames: span.frames,
                    peak_share: span.peak,
                    bbox,
                    bbox_points: points_of(frame_box, bbox),
                    events,
                });
            }
        }

        spans.sort_by(|a, b| {
            a.start_video_s
                .total_cmp(&b.start_video_s)
                .then(a.display.cmp(&b.display))
        });

        Ok(ChangesData {
            from_video_s: from_s,
            to_video_s: to_s,
            displays: displays.to_vec(),
            frames_examined: examined,
            spans,
            gaps: self.video_gaps(displays, from_s, to_s),
        })
    }

    /// Returns the stretches of `from_s..=to_s` (seconds of video time) in which each of
    /// `displays` has no video, ordered by display and then by time.
    ///
    /// Only the manifest is read, no video is decoded. A segment of a display that recorded no
    /// frames does not count as video. A stretch shorter than 0.5 s is not a gap, because a display
    /// whose screen does not change produces no frames. A display that is not in the session has
    /// no video anywhere, so it gets one gap that is the whole range.
    ///
    /// # Example
    ///
    /// ```rust
    /// use komorebi_recorder::analysis::Analysis;
    /// use komorebi_recorder::analysis::changes::Gap;
    /// use komorebi_recorder::testkit::SessionBuilder;
    ///
    /// // a ten second recording that has no video for display 2
    /// let (session, _) = SessionBuilder::new("doc-video-gaps").length(10.0).build();
    /// let analysis = Analysis::open(&session.dir)?;
    ///
    /// assert_eq!(
    ///     analysis.video_gaps(&[2], 0.0, 10.0),
    ///     [Gap { display: 2, from_video_s: 0.0, to_video_s: 10.0 }],
    /// );
    /// # Ok::<(), Box<dyn std::error::Error>>(())
    /// ```
    ///
    /// A display that has video for part of the range, and a gap after it, is covered by
    /// `tests/video.rs::a_display_with_no_video_for_part_of_the_range_has_a_gap`.
    #[must_use]
    pub fn video_gaps(&self, displays: &[u32], from_s: f64, to_s: f64) -> Vec<Gap> {
        let mut gaps = Vec::new();
        for display in displays {
            let mut covered: Vec<(f64, f64)> = self
                .manifest
                .displays
                .iter()
                .filter(|entry| entry.result.display.id == *display && entry.result.frames > 0)
                .map(|entry| {
                    (
                        entry.offset_s,
                        entry.end_s.unwrap_or_else(|| self.manifest.video_span().1),
                    )
                })
                .collect();
            covered.sort_by(|a, b| a.0.total_cmp(&b.0));

            let mut cursor = from_s;
            for (start, end) in covered {
                if start > cursor + MIN_GAP_S && cursor < to_s {
                    gaps.push(Gap {
                        display: *display,
                        from_video_s: cursor,
                        to_video_s: start.min(to_s),
                    });
                }
                cursor = cursor.max(end);
            }
            if cursor + MIN_GAP_S < to_s {
                gaps.push(Gap {
                    display: *display,
                    from_video_s: cursor,
                    to_video_s: to_s,
                });
            }
        }

        gaps
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn frames_close_together_are_one_stretch_and_a_pause_starts_another() {
        let mut segments = Segments::default();
        segments.push(1.0, 0.10, 5.0, (4, 4, 5, 5));
        segments.push(1.1, 0.20, 9.0, (2, 6, 3, 8)); // 0.1 s later: the same stretch
        segments.push(2.0, 0.05, 1.0, (0, 0, 0, 0)); // a second later: another

        let found = segments.finish();
        assert_eq!(found.len(), 2);
        assert_eq!(
            (found[0].start, found[0].end, found[0].frames),
            (1.0, 1.1, 2)
        );
        assert_eq!((found[0].peak, found[0].peak_delta), (0.20, 9.0));
        assert_eq!(found[0].bounds, (2, 4, 5, 8));
        assert_eq!((found[1].start, found[1].frames), (2.0, 1));
    }

    #[test]
    fn a_box_is_fractions_of_the_frame_and_then_points_of_the_display() {
        let bbox = fractions_of((10, 5, 19, 9), Some((100, 50)));
        assert_eq!(bbox, [0.1, 0.1, 0.1, 0.1]);
        assert_eq!(
            points_of(Some([1000.0, 0.0, 2000.0, 1000.0]), bbox),
            Some([1200.0, 100.0, 200.0, 100.0])
        );
        assert_eq!(points_of(None, bbox), None);
    }
}
