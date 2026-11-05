//! The parts of recording one segment that do not depend on which OS captured it.
//!
//! They are where each frame is placed in the file's time, the log of when each arrived, when an
//! idle display needs a frame repeated to keep its file moving, and how to size the encoding.
//!
//! A backend feeds frames in with their timestamps on the master clock and gets back where to put
//! them, so the rules that make a segment's file and its frame log agree are in one place and are
//! tested without a screen.
//!
//! | item | what it decides |
//! |---|---|
//! | [`Placement`] | where each frame, and each repeat of the last frame, goes in the file's time, and that the encoder never sees a time that is not later than the last |
//! | [`FrameLog`] | the `frames-<display>[-<n>].csv` file: when each frame was shown and received, and a row for each repeat |
//! | [`keepalive_due`], [`KEEPALIVE_AFTER`], [`KEEPALIVE_SOURCE_TIMEOUT_NS`] | when a display that has shown nothing new needs its last frame repeated |
//! | [`Latency`] | the clock-unification check, as [`LatencyStats`] |
//! | [`even`], [`h264_size`], [`h264_bitrate`], [`rebased_100ns`] | the encoding's size and bitrate, and the file's unit of time |
//!
//! # Who uses it
//!
//! The Windows backend (`wgc`) uses all of it. The macOS backend (`sck`) uses only [`even`] and
//! [`Latency`]: it applies the same placement, frame-log and keep-alive rules inline, with the same
//! numbers and private copies of the constants. The unit tests here therefore cover what
//! the Windows backend relies on, and the macOS equivalents are not covered by them.
//!
//! # Time
//!
//! Every `*_ns` argument and result is a time on the master clock (see [`crate::clock`]):
//! nanoseconds on the host's monotonic clock. A time in the file itself, as in
//! [`Placed::file_time`], is in the 100 ns units Media Foundation counts in, measured from the
//! segment's first frame.

use crate::capture::LatencyStats;
use std::fs::File;
use std::io::BufWriter;
use std::io::Write;
use std::path::Path;
use std::time::Duration;
use std::time::Instant;

/// How long a display must have shown nothing new before its last frame is repeated: 500 ms.
///
/// The repeat means a crash would lose only what is in the encoder's buffers: a screen that never
/// changed after the first second would otherwise leave a file that never got past its first
/// fragment. Repeating a frame costs a few bytes (it is identical). See [`keepalive_due`].
pub const KEEPALIVE_AFTER: Duration = Duration::from_millis(500);

/// How long a source may be silent and still be kept alive, in nanoseconds: 2 seconds.
///
/// A source silent for longer than this is not kept alive: repeating the last frame of a capture
/// that has silently died would make a frozen video look healthy while the screen moves on. See
/// [`keepalive_due`].
pub const KEEPALIVE_SOURCE_TIMEOUT_NS: u64 = 2_000_000_000;

/// How often the frame log is flushed while frames arrive: one second.
///
/// This bounds what a crash can lose from the log. See [`FrameLog`] for what is not covered.
pub const LOG_FLUSH_INTERVAL: Duration = Duration::from_secs(1);

/// How much later than the last thing written the next must be, at the least.
const MIN_STEP_NS: u64 = 1_000_000;

/// Returns `value` rounded to a whole number and then down to an even number, and never less than
/// two: video codecs want even sizes.
///
/// # Example
///
/// ```rust
/// use komorebi_recorder::capture::segment::even;
///
/// assert_eq!(even(1920.0), 1920);
/// assert_eq!(even(1919.0), 1918);
/// assert_eq!(even(1920.6), 1920);
/// assert_eq!(even(1921.6), 1922);
/// // Never below two, whatever it is given.
/// assert_eq!(even(1.0), 2);
/// assert_eq!(even(0.0), 2);
/// assert_eq!(even(-640.0), 2);
/// ```
#[must_use]
pub fn even(value: f64) -> usize {
    let value = value.round().max(2.0) as usize;
    value & !1
}

/// The longest side H.264 can have: level 5.2, which is as far as hardware encoders go, allows
/// 4096 x 2304.
const H264_LONG_SIDE: f64 = 4096.0;
/// The longest the shorter side of a frame can be: see [`H264_LONG_SIDE`].
const H264_SHORT_SIDE: f64 = 2304.0;

/// Returns the size to encode a screen of `width` x `height` pixels at: the screen's own size
/// (made even) when H.264 can take it, and otherwise the largest even size with the same shape
/// that it can.
///
/// An H.264 encoder refuses a frame bigger than 4096 x 2304 (either way round), which a 5K
/// ultrawide or an 8K display is. Only the Windows backend calls it: the macOS encoder scales for
/// itself.
///
/// # Example
///
/// ```rust
/// use komorebi_recorder::capture::segment::h264_size;
///
/// assert_eq!(h264_size(1920, 1080), (1920, 1080));
/// assert_eq!(h264_size(3840, 2160), (3840, 2160));
/// // Odd sizes are made even, as ever.
/// assert_eq!(h264_size(1919, 1081), (1918, 1080));
/// // A 5K ultrawide is brought down to the width H.264 allows.
/// assert_eq!(h264_size(5120, 2160), (4096, 1728));
/// // And so is a tall one.
/// assert_eq!(h264_size(2160, 5120), (1728, 4096));
/// assert_eq!(h264_size(7680, 4320), (4096, 2304));
/// ```
#[must_use]
pub fn h264_size(width: usize, height: usize) -> (usize, usize) {
    let (width, height) = (width as f64, height as f64);
    let (long, short) = (width.max(height), width.min(height));
    let scale = (H264_LONG_SIDE / long)
        .min(H264_SHORT_SIDE / short)
        .min(1.0);
    (even(width * scale), even(height * scale))
}

/// The frame rate declared to the encoder when the capture is not capped (`fps` of `0`).
///
/// Frames are stamped with the times they arrived, so this only sets the bitrate, the key frame
/// spacing and the duration given to the last frame.
pub const UNCAPPED_NOMINAL_FPS: u32 = 60;

/// Returns the frame rate to declare to the encoder for a capture configured with `fps`.
///
/// # Example
///
/// ```rust
/// use komorebi_recorder::capture::segment::{nominal_fps, UNCAPPED_NOMINAL_FPS};
///
/// assert_eq!(nominal_fps(24), 24);
/// assert_eq!(nominal_fps(0), UNCAPPED_NOMINAL_FPS);
/// ```
#[must_use]
pub fn nominal_fps(fps: u32) -> u32 {
    if fps == 0 { UNCAPPED_NOMINAL_FPS } else { fps }
}

/// Returns the shortest time between frames to ask the Windows capture for, in 100 ns units.
///
/// With a cap that is a frame at `fps`; with none (`fps` of `0`) it is a millisecond, which is
/// shorter than the refresh of any display.
///
/// # Example
///
/// ```rust
/// use komorebi_recorder::capture::segment::min_update_interval_100ns;
///
/// assert_eq!(min_update_interval_100ns(30), 333_333);
/// assert_eq!(min_update_interval_100ns(0), 10_000);
/// ```
#[must_use]
pub fn min_update_interval_100ns(fps: u32) -> i64 {
    if fps == 0 {
        10_000
    } else {
        10_000_000 / i64::from(fps)
    }
}

/// Returns the bitrate to encode a screen of this size at, in bits per second.
///
/// Screen content is mostly flat and changes in bursts, so this is about a tenth of a bit per pixel
/// per frame at the frame rate cap, which is generous for text and keeps a 4K display under a
/// hardware encoder's usual limit. The result is clamped to between 1.5 and 40 megabits per
/// second, and an `fps` of `0` counts as `1`.
///
/// Only the Windows backend calls it: the macOS backend does not set a bitrate.
///
/// # Example
///
/// ```rust
/// use komorebi_recorder::capture::segment::h264_bitrate;
///
/// // A tenth of a bit per pixel per frame: 1920 * 1080 * 30 * 0.1.
/// assert_eq!(h264_bitrate(1920, 1080, 30), 6_220_800);
/// assert!(h264_bitrate(3840, 2160, 30) > h264_bitrate(1920, 1080, 30));
///
/// // A floor for a tiny screen and a ceiling for a huge one.
/// assert_eq!(h264_bitrate(100, 100, 1), 1_500_000);
/// assert_eq!(h264_bitrate(7680, 4320, 60), 40_000_000);
/// ```
#[must_use]
pub fn h264_bitrate(width: usize, height: usize, fps: u32) -> u32 {
    let wanted = width as f64 * height as f64 * f64::from(fps.max(1)) * 0.1;
    wanted.clamp(1_500_000.0, 40_000_000.0) as u32
}

/// Returns how long after `first_pts_ns` the time `pts_ns` is, in the 100 ns units Media Foundation
/// counts in.
///
/// This is the time a frame has in the segment's file, whose time zero is its first frame. A
/// `pts_ns` that is not later than `first_pts_ns` gives `0`; a fraction of a unit is dropped.
///
/// # Example
///
/// ```rust
/// use komorebi_recorder::capture::segment::rebased_100ns;
///
/// // 33 ms after the first frame is 330 000 units of 100 ns.
/// assert_eq!(rebased_100ns(5_033_000_000, 5_000_000_000), 330_000);
/// assert_eq!(rebased_100ns(5_000_000_000, 5_000_000_000), 0);
/// assert_eq!(rebased_100ns(4_000_000_000, 5_000_000_000), 0);
/// ```
#[must_use]
pub fn rebased_100ns(pts_ns: u64, first_pts_ns: u64) -> i64 {
    i64::try_from(pts_ns.saturating_sub(first_pts_ns) / 100).unwrap_or(i64::MAX)
}

/// Where each thing appended to a segment goes in time.
///
/// The encoder rejects a frame that is not later than the one before it. Frames carry the time the
/// display produced them, which runs slightly ahead of when they are received, so a keep-alive
/// stamped "now" can land a hair after the next real frame; the real frame is moved just past it
/// rather than losing it or the segment.
///
/// It is guaranteed that every [`Placed::pts_ns`] it returns is later than the one before. A
/// frame that is not later than the last thing placed is moved to 1 ms after it, and a repeat is
/// never less than 1 ms after it. The first frame defines the start of the segment and is never
/// moved. A `Placement` is per segment: start a new one for the next.
///
/// # Example
///
/// ```rust
/// use komorebi_recorder::capture::segment::Placement;
///
/// let mut placement = Placement::default();
/// assert_eq!(placement.first_ns(), None);
///
/// // The first frame is time zero of the file; 33 ms later is 330 000 units of 100 ns.
/// let first = placement.frame(5_000_000_000);
/// assert_eq!(first.file_time, 0);
/// let next = placement.frame(5_033_000_000);
/// assert_eq!(next.file_time, 330_000);
/// assert!(!next.moved);
///
/// // A repeat stamped "now" ran a hair ahead of the next real frame, which is moved just past it
/// // instead of being rejected.
/// placement.repeat(5_600_000_000);
/// let late = placement.frame(5_599_000_000);
/// assert!(late.moved);
/// assert_eq!(late.pts_ns, 5_601_000_000);
/// assert_eq!(placement.last_ns(), 5_601_000_000);
/// ```
#[derive(Debug, Default)]
pub struct Placement {
    first_ns: Option<u64>,
    last_ns: u64,
}

/// Where something was placed: what [`Placement::frame`] and [`Placement::repeat`] return.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Placed {
    /// On the master clock, after any moving to keep it in order. This is the time to log.
    pub pts_ns: u64,
    /// Within the segment's file, in 100 ns units: zero for the first. This is the time to hand to
    /// the encoder.
    pub file_time: i64,
    /// Whether it was moved from the time it was given.
    pub moved: bool,
}

impl Placement {
    /// The master-clock time of the segment's first frame, once it has one.
    #[must_use]
    pub fn first_ns(&self) -> Option<u64> {
        self.first_ns
    }

    /// The master-clock time of the last thing appended (`0` before any).
    #[must_use]
    pub fn last_ns(&self) -> u64 {
        self.last_ns
    }

    /// Places a real frame produced at `pts_ns`. The first one defines the start of the segment.
    ///
    /// A frame that is not later than the last thing placed is moved to 1 ms after it
    /// ([`Placed::moved`] is then `true`).
    pub fn frame(&mut self, pts_ns: u64) -> Placed {
        let first = *self.first_ns.get_or_insert(pts_ns);
        let adjusted = if self.last_ns > 0 && pts_ns <= self.last_ns {
            self.last_ns + MIN_STEP_NS
        } else {
            pts_ns
        };

        self.last_ns = adjusted;
        Placed {
            pts_ns: adjusted,
            file_time: rebased_100ns(adjusted, first),
            moved: adjusted != pts_ns,
        }
    }

    /// Places a repeat of the last frame at `now_ns`. `None` before there is a first frame to
    /// repeat.
    ///
    /// A repeat is never earlier than 1 ms after the last thing placed, so one asked for "now"
    /// right after a frame that ran ahead of the clock is moved later ([`Placed::moved`]).
    pub fn repeat(&mut self, now_ns: u64) -> Option<Placed> {
        let first = self.first_ns?;
        let adjusted = now_ns.max(self.last_ns + MIN_STEP_NS);
        self.last_ns = adjusted;
        Some(Placed {
            pts_ns: adjusted,
            file_time: rebased_100ns(adjusted, first),
            moved: adjusted != now_ns,
        })
    }
}

/// Returns `true` if and only if an idle segment is due a repeated frame.
///
/// Only while the source is demonstrably alive (`source_signal_ns` is when it last did anything,
/// `None` if the backend cannot tell, which counts as alive) and the segment has been still for
/// [`KEEPALIVE_AFTER`]. A source silent for [`KEEPALIVE_SOURCE_TIMEOUT_NS`] or longer is not alive.
/// `last_append_ns` is the master-clock time of the last thing written to the segment, as
/// [`Placement::last_ns`] reports it.
///
/// # Example
///
/// ```rust
/// use komorebi_recorder::capture::segment::keepalive_due;
///
/// let half_second = 500_000_000;
/// // Still for half a second, and the source is alive or cannot say.
/// assert!(keepalive_due(half_second, 0, None));
/// assert!(keepalive_due(half_second, 0, Some(half_second - 1)));
/// // Not still for long enough.
/// assert!(!keepalive_due(half_second - 1, 0, None));
/// // The source has been silent for 5 seconds: a dead capture is not made to look alive.
/// assert!(!keepalive_due(5_000_000_000, 0, Some(0)));
/// ```
#[must_use]
pub fn keepalive_due(now_ns: u64, last_append_ns: u64, source_signal_ns: Option<u64>) -> bool {
    let alive = source_signal_ns
        .is_none_or(|last| now_ns.saturating_sub(last) < KEEPALIVE_SOURCE_TIMEOUT_NS);
    let still_for = now_ns.saturating_sub(last_append_ns);
    alive && still_for >= u64::try_from(KEEPALIVE_AFTER.as_nanos()).unwrap_or(u64::MAX)
}

/// Accumulates how far the frames' times lag behind the moment they were received.
///
/// If the frames were not on the host clock this would be wildly off, which makes it the
/// clock-unification check. [`Latency::finish`] reads out a [`LatencyStats`] without ending the
/// accumulation, so a segment can be summarized while it is still recording.
///
/// # Example
///
/// ```rust
/// use komorebi_recorder::capture::segment::Latency;
///
/// let mut latency = Latency::default();
/// // No frames, nothing to doubt.
/// assert!(latency.finish().looks_like_host_clock());
///
/// // Received minus stamped, in nanoseconds: the first frame was stamped 5 ms ahead.
/// for ns in [-5_000_000, 10_000_000, 30_000_000] {
///     latency.record(ns);
/// }
/// let stats = latency.finish();
/// assert_eq!((stats.samples, stats.min_ns, stats.max_ns), (3, -5_000_000, 30_000_000));
/// assert_eq!(stats.mean_ns, 11_666_666);
/// assert!(stats.looks_like_host_clock());
/// ```
#[derive(Default)]
pub struct Latency {
    samples: u64,
    min_ns: i64,
    max_ns: i64,
    sum_ns: i128,
}

impl Latency {
    /// Adds one frame's latency: the time it was received minus the time it is stamped with, in
    /// nanoseconds. Negative when the stamp is ahead of the arrival.
    pub fn record(&mut self, latency_ns: i64) {
        if self.samples == 0 {
            self.min_ns = latency_ns;
            self.max_ns = latency_ns;
        } else {
            self.min_ns = self.min_ns.min(latency_ns);
            self.max_ns = self.max_ns.max(latency_ns);
        }

        self.samples += 1;
        self.sum_ns += i128::from(latency_ns);
    }

    /// Returns the statistics of the latencies recorded so far. With none, all zeros.
    #[must_use]
    pub fn finish(&self) -> LatencyStats {
        LatencyStats {
            samples: self.samples,
            min_ns: self.min_ns,
            max_ns: self.max_ns,
            mean_ns: if self.samples == 0 {
                0
            } else {
                (self.sum_ns / i128::from(self.samples)) as i64
            },
        }
    }
}

/// `frames-<display>[-<n>].csv`: every frame's time on the master clock and when it was received,
/// and a `-` row for each repeat.
///
/// It is authoritative for when each frame was shown, whatever the file's own times say.
///
/// There is one row per line, `<index>,<pts_ns>,<received_ns>`, with both times in nanoseconds on
/// the master clock. A repeat of the last frame, which keeps the file moving while the display is
/// still, has `-` in place of an index and is not counted as a frame:
///
/// ```text
/// 0,5000000000,5000900000
/// 1,5033000000,5034000000
/// -,5600000000,5600000000
/// ```
///
/// [`FrameLog::create`] writes no header row. The reader of the file (`recovery`) skips the first
/// line of the file as a header; the macOS backend writes `index,pts_mach_ns,received_mach_ns`
/// there itself, and the Windows backend does not.
///
/// # Durability
///
/// Rows are buffered. [`FrameLog::frame`] flushes when [`LOG_FLUSH_INTERVAL`] has passed since the
/// last flush, and [`FrameLog::flush`] flushes at once. [`FrameLog::repeat`] does not flush, so
/// while a display is still the rows for its repeats stay in memory until the next real frame or
/// [`FrameLog::flush`]. A crash can lose what is still buffered, and can leave the last row of the
/// file cut short if the buffer filled in the middle of it, so a reader has to ignore a row that
/// does not parse (`recovery` does). A write that fails is ignored: the video is the source of
/// truth.
///
/// # Example
///
/// ```rust
/// use komorebi_recorder::capture::segment::FrameLog;
///
/// let path = std::env::temp_dir()
///     .join(format!("komorebi-recorder-doc-frame-log-{}.csv", std::process::id()));
/// let mut log = FrameLog::create(&path)?;
/// log.frame(100, 105); // stamped 100, received at 105
/// log.repeat(700, 710); // the display was still: the last frame repeated
/// log.frame(900, 903);
/// log.flush();
///
/// assert_eq!(log.frames(), 2, "a repeat is not a frame");
/// assert_eq!(std::fs::read_to_string(&path)?, "0,100,105\n-,700,710\n1,900,903\n");
/// # std::fs::remove_file(&path)?;
/// # Ok::<(), Box<dyn std::error::Error>>(())
/// ```
pub struct FrameLog {
    out: BufWriter<File>,
    last_flush: Instant,
    index: u64,
}

impl FrameLog {
    /// Creates the log at `path`, truncating a file that is already there. Nothing is written
    /// until the first row.
    ///
    /// # Errors
    ///
    /// Returns the I/O error if the file cannot be created: the directory does not exist, or the
    /// process may not write there.
    pub fn create(path: &Path) -> std::io::Result<Self> {
        Ok(Self {
            out: BufWriter::new(File::create(path)?),
            last_flush: Instant::now(),
            index: 0,
        })
    }

    /// Returns how many real frames have been logged. Repeats are not counted.
    #[must_use]
    pub fn frames(&self) -> u64 {
        self.index
    }

    /// Logs a real frame stamped `pts_ns` and received at `received_ns`, both on the master clock.
    ///
    /// A failed write must not take down the capture: the video is the source of truth and this is
    /// how it is read. So a failed write is not reported and the row is lost, and the frame is
    /// still counted by [`FrameLog::frames`].
    pub fn frame(&mut self, pts_ns: u64, received_ns: u64) {
        let _ = writeln!(self.out, "{},{pts_ns},{received_ns}", self.index);
        self.index += 1;
        self.flush_if_due();
    }

    /// Logs a repeat of the last frame at `pts_ns`, made at `now_ns`: not a new frame, and marked
    /// so, but it keeps the log's idea of where the segment ends honest.
    pub fn repeat(&mut self, pts_ns: u64, now_ns: u64) {
        let _ = writeln!(self.out, "-,{pts_ns},{now_ns}");
    }

    fn flush_if_due(&mut self) {
        if self.last_flush.elapsed() >= LOG_FLUSH_INTERVAL {
            // on whole lines, so a crash never leaves a torn row behind
            let _ = self.out.flush();
            self.last_flush = Instant::now();
        }
    }

    /// Writes what is buffered to the file. A failed write is ignored, as in [`FrameLog::frame`].
    pub fn flush(&mut self) {
        let _ = self.out.flush();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sizes_are_even_and_never_below_two() {
        assert_eq!(even(1920.0), 1920);
        assert_eq!(even(1919.0), 1918);
        assert_eq!(even(1920.6), 1920);
        assert_eq!(even(0.0), 2);
        assert_eq!(even(1.0), 2);
    }

    #[test]
    fn the_bitrate_follows_the_pixels_within_what_an_encoder_takes() {
        let hd = h264_bitrate(1920, 1080, 30);
        let uhd = h264_bitrate(3840, 2160, 30);
        assert!((5_000_000..10_000_000).contains(&hd), "{hd}");
        assert!(uhd > hd);
        assert_eq!(
            h264_bitrate(100, 100, 1),
            1_500_000,
            "a floor for a tiny one"
        );
        assert_eq!(
            h264_bitrate(7680, 4320, 60),
            40_000_000,
            "a ceiling for a huge one"
        );
    }

    #[test]
    fn the_first_frame_is_time_zero_and_the_rest_follow_it() {
        let mut placement = Placement::default();
        let first = placement.frame(5_000_000_000);
        assert_eq!(first.file_time, 0);
        assert_eq!(placement.first_ns(), Some(5_000_000_000));

        let next = placement.frame(5_033_000_000);
        assert_eq!(
            next.file_time, 330_000,
            "33 ms is 330000 hundred-nanosecond units"
        );
        assert!(!next.moved);
    }

    #[test]
    fn a_frame_not_later_than_the_last_is_moved_just_past_it() {
        let mut placement = Placement::default();
        placement.frame(1_000_000_000);
        // a repeat stamped "now" ran a hair ahead of the next real frame
        placement.repeat(1_600_000_000);
        let late = placement.frame(1_599_000_000);

        assert!(late.moved);
        assert_eq!(late.pts_ns, 1_601_000_000);
        assert!(late.file_time > 0);
    }

    #[test]
    fn every_placement_is_strictly_later_than_the_one_before() {
        let mut placement = Placement::default();
        let mut last = 0;
        for pts in [10, 20, 20, 15, 30, 30, 31] {
            let placed = placement.frame(pts * 1_000_000);
            assert!(
                placed.pts_ns > last || last == 0,
                "{pts}: {placed:?} after {last}"
            );
            last = placed.pts_ns;
        }
    }

    #[test]
    fn nothing_can_be_repeated_before_the_first_frame() {
        assert_eq!(Placement::default().repeat(1_000_000_000), None);
    }

    #[test]
    fn a_repeat_is_never_earlier_than_what_it_repeats() {
        let mut placement = Placement::default();
        placement.frame(2_000_000_000);
        let repeat = placement.repeat(1_000_000_000).expect("there is a frame");
        assert!(repeat.pts_ns > 2_000_000_000);
    }

    #[test]
    fn a_still_display_is_kept_alive_only_while_its_source_is() {
        let half = 500_000_000;
        // still for long enough, source alive or unable to say
        assert!(keepalive_due(2 * half, 0, None));
        assert!(keepalive_due(2 * half, 0, Some(2 * half - 1)));
        // not still for long enough
        assert!(!keepalive_due(half - 1, 0, None));
        // source silent for over two seconds: a dead capture is not made to look alive
        assert!(!keepalive_due(10 * half, 0, Some(0)));
    }

    #[test]
    fn latency_tracks_the_range_and_mean_and_says_what_clock_it_is() {
        let mut latency = Latency::default();
        assert!(
            latency.finish().looks_like_host_clock(),
            "no frames, nothing to doubt"
        );
        for ns in [-5_000_000, 10_000_000, 30_000_000] {
            latency.record(ns);
        }
        let stats = latency.finish();
        assert_eq!(
            (stats.samples, stats.min_ns, stats.max_ns),
            (3, -5_000_000, 30_000_000)
        );
        assert_eq!(stats.mean_ns, 11_666_666);
        assert!(stats.looks_like_host_clock());
    }

    #[test]
    fn the_frame_log_writes_frames_and_marks_repeats() {
        let path =
            std::env::temp_dir().join(format!("komorebi-framelog-{}.csv", std::process::id()));
        let mut log = FrameLog::create(&path).expect("create");
        log.frame(100, 105);
        log.repeat(700, 710);
        log.frame(900, 903);
        log.flush();

        assert_eq!(log.frames(), 2, "a repeat is not a frame");
        assert_eq!(
            std::fs::read_to_string(&path).expect("read"),
            "0,100,105\n-,700,710\n1,900,903\n"
        );
        let _ = std::fs::remove_file(&path);
    }
}
