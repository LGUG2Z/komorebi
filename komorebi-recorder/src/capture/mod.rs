//! Screen capture: the plain data that describes a recording of a display, the two traits that
//! produce one, and the policy that keeps every display recording while the machine changes under
//! it.
//!
//! The unit of recording is a **segment**: one display, one contiguous span of time, one file. A
//! display that is unplugged and plugged back in, whose stream dies, or that changes size produces
//! several segments, each a complete, playable file. A segment is two files in the session
//! directory, named by [`video_file_name`] and [`frames_file_name`]:
//!
//! - `video-<display>[-<n>].mp4`, a fragmented MP4, so that a recorder that is killed leaves a
//!   file that plays up to its last fragment;
//! - `frames-<display>[-<n>].csv`, when each frame was shown, on the master clock.
//!
//! [`DisplayResult`] describes one segment. The session manifest stores them (see
//! [`crate::session::DisplayEntry`]).
//!
//! # Where things are
//!
//! | item | role |
//! |---|---|
//! | [`CaptureBackend`] | Where displays and captures come from: lists the displays that exist and starts a capture of one. |
//! | [`RunningCapture`] | One segment being recorded: its counters, a snapshot of it, a keep-alive hook and `stop`. |
//! | [`manager`] | [`DisplayManager`](manager::DisplayManager), which keeps every wanted display recording through hotplug, sleep, lock and stalled streams. It uses only the two traits above. |
//! | [`segment`] | The rules of one segment that do not depend on the OS: where a frame goes in the file's time, the frame log, when an idle display needs a frame repeated, how to size the encoding. |
//! | `sck` | The macOS backend: `ScreenCaptureKit` into an `AVAssetWriter`. Exists on macOS only. |
//! | `wgc` | The Windows backend: Windows.Graphics.Capture into a Media Foundation sink writer. Exists on Windows only. |
//!
//! The two backends are the only code here that calls a capture API. Everything else is plain Rust,
//! which is what makes display hotplug, sleep and wake testable: [`manager`] is tested against a scripted
//! [`CaptureBackend`] (docking, undocking, flapping and sleeping displays, stalled streams), since
//! real hardware cannot be unplugged from a test.
//!
//! # The host axis
//!
//! The rest of the crate does not name a backend. [`Capture`] is the host's backend
//! ([`crate::platform::capture::Backend`]) or [`NoDisplays`], for a recording made without video.
//! The two real backends differ in what the OS gives them:
//!
//! | | macOS | Windows |
//! |---|---|---|
//! | Capture API | `ScreenCaptureKit` | Windows.Graphics.Capture |
//! | File writer | `AVAssetWriter` | Media Foundation sink writer |
//! | Codec | HEVC or H.264 | H.264 only: [`Codec::Hevc`] is refused when a capture starts |
//! | Frame timestamps | the frame's presentation time, on `mach_absolute_time` | the frame's `SystemRelativeTime`, on the performance counter |
//! | A screen that is not changing | idle callbacks, about thirty a second, which are the only sign of life | no callbacks at all |
//! | Stall detection ([`RunningCapture::last_signal_mach_ns`]) | on | off: silence is not a sign of death |
//!
//! # Time
//!
//! Every `*_mach_ns` value in this module is a time on the master clock (see [`crate::clock`]):
//! nanoseconds on the host's monotonic clock. The `mach_` spelling is historical; the session
//! manifest's `host.clock` records which clock it was (`mach`, `qpc` or `std`).
//!
//! # What this module does not do
//!
//! It does not read recorded video back (`av` does), recover a session whose recorder was killed
//! (`recovery` does, from the frame log and the file), or notice that the machine slept (`power`
//! and the `recorder` loop do, and then ask [`manager::DisplayManager::restart_all`] to replace the
//! captures).

use color_eyre::eyre;
use parking_lot::Mutex;
use serde::Deserialize;
use serde::Serialize;
use std::path::Path;
use std::sync::Arc;
use std::sync::atomic::AtomicU64;
use std::sync::atomic::Ordering;

#[cfg(target_os = "linux")]
pub mod gst;
pub mod manager;
#[cfg(target_os = "macos")]
pub mod sck;
pub mod segment;
#[cfg(windows)]
pub mod wgc;

/// The video codec a segment is encoded with.
///
/// The text forms are `hevc` and `h264`. [`Display`](std::fmt::Display) writes them, `session.json`
/// stores them, and the `--codec` option of `record` accepts them.
///
/// The default is [`Codec::Hevc`] on a Mac and [`Codec::H264`] on Windows and Linux. The macOS backend
/// records either. The Windows and Linux backends record only H.264 and refuse [`Codec::Hevc`] when a
/// capture starts.
///
/// # Example
///
/// ```rust
/// use clap::ValueEnum;
/// use komorebi_recorder::capture::Codec;
///
/// // `Display`, `serde` and the command line agree on the spelling.
/// assert_eq!(Codec::Hevc.to_string(), "hevc");
/// assert_eq!(serde_json::to_string(&Codec::H264)?, r#""h264""#);
/// assert_eq!(serde_json::from_str::<Codec>(r#""hevc""#)?, Codec::Hevc);
/// let possible = Codec::H264.to_possible_value().expect("not skipped");
/// assert_eq!(possible.get_name(), "h264");
///
/// // The default depends on the host the crate was built for.
/// let expected = if cfg!(any(windows, target_os = "linux")) { Codec::H264 } else { Codec::Hevc };
/// assert_eq!(Codec::default(), expected);
/// # Ok::<(), Box<dyn std::error::Error>>(())
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, clap::ValueEnum)]
#[serde(rename_all = "lowercase")]
pub enum Codec {
    /// HEVC (H.265). Recorded on macOS only. Playing it back on Windows needs a decoder that
    /// Windows does not always have (the HEVC Video Extensions).
    Hevc,
    /// H.264. Recorded on both hosts, and decoded by Media Foundation everywhere.
    H264,
}

impl Default for Codec {
    /// HEVC on a Mac. H.264 on Windows: Media Foundation decodes it everywhere, HEVC needs the
    /// HEVC Video Extensions to be installed, and its MP4 writer cannot write HEVC without a
    /// hand-built sample description.
    fn default() -> Self {
        if cfg!(any(windows, target_os = "linux")) {
            Codec::H264
        } else {
            Codec::Hevc
        }
    }
}

impl std::fmt::Display for Codec {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Codec::Hevc => f.write_str("hevc"),
            Codec::H264 => f.write_str("h264"),
        }
    }
}

/// How every segment of a recording is captured: the frame rate, the resolution and the codec.
///
/// One configuration applies to every display and every segment of a recording.
/// [`DisplayManager::new`](manager::DisplayManager::new) takes it once and hands it to
/// [`CaptureBackend::start`] for each segment.
///
/// The default is no frame rate cap (frames as fast as the display refreshes), not retina, and
/// [`Codec::default`].
///
/// # Example
///
/// ```rust
/// use komorebi_recorder::capture::{CaptureConfig, Codec};
///
/// let config = CaptureConfig::default();
/// assert_eq!(config.fps, 0);
/// assert!(!config.retina);
/// assert_eq!(config.codec, Codec::default());
/// ```
#[derive(Debug, Clone, Default)]
pub struct CaptureConfig {
    /// The most frames a second to capture. `0` means no cap: as many as the display refreshes.
    ///
    /// Both backends treat this as an upper bound, and emit frames only when the content changes.
    /// `ScreenCaptureKit` is given it as the minimum frame interval; on Windows it is the minimum
    /// update interval of the capture session (which needs Windows 11 22H2, and on an older
    /// Windows the capture is held to 60 frames a second whatever this is). On Windows it is also
    /// the rate declared to the encoder, which sets the bitrate (see [`segment::h264_bitrate`]),
    /// the key frame spacing and the duration given to the last frame; with no cap that is
    /// [`segment::UNCAPPED_NOMINAL_FPS`]. On a Mac the encoder never sees it.
    pub fps: u32,
    /// Whether to capture at native pixel resolution rather than logical points.
    ///
    /// Read by the macOS backend only: a display `width_points` wide is captured at
    /// `width_points * scale` pixels, where `scale` is the point-to-pixel factor `ScreenCaptureKit`
    /// reports for it (see [`DisplayResult::scale`]). Windows frames are always native pixels.
    pub retina: bool,
    /// The codec to encode with. See [`Codec`] for what each host accepts.
    pub codec: Codec,
}

/// A display that can be captured, as the OS described it at one moment.
///
/// A descriptor is a snapshot. [`CaptureBackend::active_displays`] returns a fresh list each time
/// it is asked, and [`DisplayManager`](manager::DisplayManager) compares the list with the
/// descriptor each running capture started with to find displays that left, fell asleep or changed
/// size. Equality compares every field, position included; [`DisplayDescriptor::size_differs`]
/// compares only what invalidates a capture.
///
/// # Example
///
/// A descriptor read from a manifest that has no `asleep` or `scale` fields gets the defaults:
///
/// ```rust
/// use komorebi_recorder::capture::DisplayDescriptor;
///
/// let display: DisplayDescriptor = serde_json::from_str(
///     r#"{"id": 2, "frame": [0.0, 0.0, 3840.0, 1620.0], "width_points": 3840, "height_points": 1620}"#,
/// )?;
/// assert!(!display.asleep);
/// assert_eq!(display.scale, 1.0);
/// # Ok::<(), Box<dyn std::error::Error>>(())
/// ```
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DisplayDescriptor {
    /// The OS's identifier for the display: the `CGDirectDisplayID` on a Mac, and on Windows the
    /// `HMONITOR` handle value (truncated to 32 bits), which is also the id komorebi for Windows
    /// gives the monitor. [`DisplayManager`](manager::DisplayManager) treats an id that reappears
    /// after it was gone as the same display returning.
    pub id: u32,
    /// `[x, y, width, height]` of the display in the global desktop's coordinates: logical points
    /// on a Mac, physical pixels on Windows (the left and top of its monitor rectangle, then its
    /// width and height).
    pub frame: [f64; 4],
    /// The width of `frame`, rounded to a whole number.
    pub width_points: i64,
    /// The height of `frame`, rounded to a whole number.
    pub height_points: i64,
    /// Attached but asleep. A sleeping display cannot be captured and its stream stalls, so it is
    /// treated as absent until it wakes. The Windows host always reports `false`.
    #[serde(default)]
    pub asleep: bool,
    /// The display's scaling factor. On a Mac, how many pixels the display has for each point of
    /// `frame`: the display mode's pixel width over its width in points, which is `2.0` on a
    /// Retina display. On Windows, the display's effective DPI over 96 (`1.5` at 150% scaling);
    /// `frame` is already in physical pixels there, so it is not a factor to convert `frame` by.
    /// Absent in a manifest means `1.0`.
    #[serde(default = "unit_scale")]
    pub scale: f64,
}

fn unit_scale() -> f64 {
    1.0
}

impl DisplayDescriptor {
    /// Returns `true` if and only if the display's size differs from `other`'s.
    ///
    /// A change of size (a resolution change, or the same display id coming back as a different
    /// monitor) invalidates a running capture, whereas a change of position (rearranging displays)
    /// does not. Only `width_points` and `height_points` are compared.
    ///
    /// # Example
    ///
    /// ```rust
    /// use komorebi_recorder::capture::DisplayDescriptor;
    ///
    /// let display = |x: f64, width: i64, height: i64| DisplayDescriptor {
    ///     id: 1,
    ///     frame: [x, 0.0, width as f64, height as f64],
    ///     width_points: width,
    ///     height_points: height,
    ///     asleep: false,
    ///     scale: 1.0,
    /// };
    ///
    /// // Rearranged: same size, new position.
    /// assert!(!display(0.0, 1800, 1169).size_differs(&display(-1800.0, 1800, 1169)));
    /// // A resolution change.
    /// assert!(display(0.0, 1800, 1169).size_differs(&display(0.0, 1512, 982)));
    /// ```
    #[must_use]
    pub fn size_differs(&self, other: &Self) -> bool {
        self.width_points != other.width_points || self.height_points != other.height_points
    }
}

/// How far a segment's frame timestamps lag behind the moment each frame was received.
///
/// Each frame is stamped with the time the display produced it and each callback records when it
/// arrived. The difference is `received - stamped`, in nanoseconds: positive when the stamp is
/// earlier than the arrival (the usual case), negative when the stamp is ahead of it. If the
/// timestamps were not on the master clock the numbers would be off by seconds or more, so
/// [`LatencyStats::looks_like_host_clock`] doubles as the clock-unification check.
///
/// Built by [`segment::Latency`], which both the backends use, and stored in
/// [`DisplayResult::latency`]. A segment with no frames has the default, all zeros.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize, schemars::JsonSchema,
)]
pub struct LatencyStats {
    /// How many frames were measured.
    pub samples: u64,
    /// The smallest `received - stamped`, in nanoseconds. Negative when a frame was stamped ahead
    /// of its arrival.
    pub min_ns: i64,
    /// The largest `received - stamped`, in nanoseconds.
    pub max_ns: i64,
    /// The mean `received - stamped`, in nanoseconds, truncated toward zero.
    pub mean_ns: i64,
}

impl LatencyStats {
    /// Returns `true` if and only if the measured latencies are plausible for timestamps on the
    /// master clock.
    ///
    /// That is the case when there are no samples (nothing to doubt), or when no frame was
    /// stamped more than 100 ms ahead of its arrival (`min_ns >= -100 ms`) and none arrived 2 s or
    /// more after its stamp (`max_ns < 2 s`). The bounds only have to separate "the same clock"
    /// from "a different clock", which is off by seconds or more: on a Mac with real displays the
    /// measured range was about -7.5 ms to 196 ms, the late end being the first frame while the
    /// encoder spins up.
    ///
    /// # Example
    ///
    /// ```rust
    /// use komorebi_recorder::capture::LatencyStats;
    ///
    /// // Measured on a real display.
    /// let measured = LatencyStats {
    ///     samples: 229,
    ///     min_ns: -7_506_167,
    ///     max_ns: 196_015_958,
    ///     mean_ns: 6_524_913,
    /// };
    /// assert!(measured.looks_like_host_clock());
    ///
    /// // Stamps from another clock, here one an hour off, are not plausible.
    /// let wrong = LatencyStats {
    ///     samples: 10,
    ///     min_ns: 3_600_000_000_000,
    ///     max_ns: 3_600_100_000_000,
    ///     mean_ns: 3_600_050_000_000,
    /// };
    /// assert!(!wrong.looks_like_host_clock());
    ///
    /// // No frames, nothing to doubt.
    /// assert!(LatencyStats::default().looks_like_host_clock());
    /// ```
    #[must_use]
    pub fn looks_like_host_clock(&self) -> bool {
        // Frames are stamped at (or marginally ahead of) the display refresh that produced them:
        // measured -7.5 ms at the early end and ~200 ms at the late end (the first frame, while
        // the encoder spins up). A *different* clock would be off by seconds or more, so these
        // bounds only have to separate "same clock" from "not the same clock".
        self.samples == 0 || (self.min_ns >= -100_000_000 && self.max_ns < 2_000_000_000)
    }
}

/// Why a capture segment ended.
///
/// Stored in [`DisplayResult::end_reason`] and written to `session.json` in kebab-case
/// (`display-disconnected`, `stream-error`, ...). [`Display`](std::fmt::Display) writes a phrase
/// for people (`display disconnected`), which the incident log uses.
///
/// # Example
///
/// ```rust
/// use komorebi_recorder::capture::EndReason;
///
/// assert_eq!(
///     serde_json::to_string(&EndReason::DisplayDisconnected)?,
///     r#""display-disconnected""#
/// );
/// assert_eq!(EndReason::Resumed.to_string(), "machine resumed from sleep");
/// # Ok::<(), Box<dyn std::error::Error>>(())
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum EndReason {
    /// The recording was stopped normally.
    Stopped,
    /// The display went away (unplugged, undocked, lid closed): it was missing from the display
    /// list.
    DisplayDisconnected,
    /// The display went to sleep; a new segment starts when it wakes.
    DisplayAsleep,
    /// The capture reported an error, or showed no sign of life for too long (see
    /// [`RunningCapture::last_signal_mach_ns`]). On a Mac that is `ScreenCaptureKit` stopping the
    /// stream (sleep, lock, a display reconfiguration, ...). On Windows it is the display's capture
    /// item closing, the GPU being reset or removed, a frame of a different size, or the encoder
    /// rejecting a frame.
    StreamError,
    /// The display's size changed, so the segment was closed and a new one begun.
    Reconfigured,
    /// The machine slept and woke. Capture streams are not trusted across a sleep (they can stall
    /// without reporting anything), so each is replaced with a fresh one by
    /// [`manager::DisplayManager::restart_all`].
    Resumed,
    /// The recorder died before finalizing the segment, which was recovered from the fragmented
    /// file afterwards (`recovery` sets this; no capture does).
    Interrupted,
}

impl std::fmt::Display for EndReason {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            EndReason::Stopped => "stopped",
            EndReason::DisplayDisconnected => "display disconnected",
            EndReason::DisplayAsleep => "display asleep",
            EndReason::StreamError => "stream error",
            EndReason::Reconfigured => "display reconfigured",
            EndReason::Resumed => "machine resumed from sleep",
            EndReason::Interrupted => "interrupted",
        })
    }
}

/// Returns the file name of a segment's video.
///
/// The first segment of a display (`segment` 0 or 1) has the plain name `video-<display>.mp4`, so a
/// session with one segment per display has no numbers in its file names. Later segments are
/// `video-<display>-<segment>.mp4`.
///
/// # Example
///
/// ```rust
/// use komorebi_recorder::capture::video_file_name;
///
/// assert_eq!(video_file_name(2, 1), "video-2.mp4");
/// assert_eq!(video_file_name(2, 3), "video-2-3.mp4");
///
/// // The display id and the segment number cannot be confused for one another.
/// assert_ne!(video_file_name(1, 2), video_file_name(12, 1));
/// ```
#[must_use]
pub fn video_file_name(display: u32, segment: u32) -> String {
    if segment <= 1 {
        format!("video-{display}.mp4")
    } else {
        format!("video-{display}-{segment}.mp4")
    }
}

/// Returns the file name of a segment's frame timestamp log.
///
/// It is named like the segment's video (see [`video_file_name`]), with `frames-` and `.csv` in
/// place of `video-` and `.mp4`.
///
/// # Example
///
/// ```rust
/// use komorebi_recorder::capture::frames_file_name;
///
/// assert_eq!(frames_file_name(2, 1), "frames-2.csv");
/// assert_eq!(frames_file_name(2, 3), "frames-2-3.csv");
/// ```
#[must_use]
pub fn frames_file_name(display: u32, segment: u32) -> String {
    if segment <= 1 {
        format!("frames-{display}.csv")
    } else {
        format!("frames-{display}-{segment}.csv")
    }
}

/// One contiguous recording of one display: a display that is unplugged and replugged, or whose
/// stream dies, produces several of these, each a complete, playable file.
///
/// A result is returned by [`RunningCapture::stop`] when the segment is over and by
/// [`RunningCapture::partial`] while it is still running. A running segment has no
/// [`end_reason`](DisplayResult::end_reason) and no
/// [`video_end_mach_ns`](DisplayResult::video_end_mach_ns). The session manifest stores every
/// result (see [`crate::session::DisplayEntry`]).
///
/// The serialized form is what `session.json` stores for each segment. `segment`,
/// `video_end_mach_ns`, `end_reason` and `idle_callbacks` may be absent when it is read, and take
/// their defaults.
///
/// # Example
///
/// ```rust
/// use komorebi_recorder::capture::DisplayResult;
///
/// // A result with none of the optional fields: segment 1, still open, not idle-counted.
/// let result: DisplayResult = serde_json::from_str(r#"{
///     "display": {"id": 2, "frame": [0.0, 0.0, 3840.0, 1620.0],
///                 "width_points": 3840, "height_points": 1620},
///     "scale": 1.0, "width_px": 3840, "height_px": 1620, "codec": "hevc",
///     "file": "video-2.mp4", "frames_file": "frames-2.csv",
///     "video_t0_mach_ns": 5, "frames": 3, "dropped": 0,
///     "latency": {"samples": 0, "min_ns": 0, "max_ns": 0, "mean_ns": 0},
///     "error": null
/// }"#)?;
/// assert_eq!(result.segment, 1);
/// assert_eq!(result.video_end_mach_ns, None);
/// assert_eq!(result.end_reason, None);
/// assert_eq!(result.idle_callbacks, 0);
/// # Ok::<(), Box<dyn std::error::Error>>(())
/// ```
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DisplayResult {
    /// The display as the OS described it when the segment began.
    pub display: DisplayDescriptor,
    /// The segment's number: 1-based, per display. Segment `n` of display `d` is the file
    /// [`video_file_name`]`(d, n)`.
    #[serde(default = "first_segment")]
    pub segment: u32,
    /// How many pixels the video has for each unit of `display.frame`. `1.0`, except for a Mac
    /// recording made at native resolution (see [`CaptureConfig::retina`]), where it is the
    /// point-to-pixel factor `ScreenCaptureKit` reports. Always `1.0` on Windows.
    pub scale: f64,
    /// The width of the video in pixels. Both backends round it to an even number, as video codecs
    /// want.
    pub width_px: usize,
    /// The height of the video in pixels, rounded to an even number as `width_px` is.
    pub height_px: usize,
    /// The codec the file is encoded with.
    pub codec: Codec,
    /// The video's file name, relative to the session directory (see [`video_file_name`]).
    pub file: String,
    /// The frame log's file name, relative to the session directory (see [`frames_file_name`]).
    pub frames_file: String,
    /// Master-clock time of the first frame in the file, if any frame was received. This is the
    /// instant that is time zero of the file. `None` means no frame arrived.
    pub video_t0_mach_ns: Option<u64>,
    /// Master-clock time this segment ended (frames are variable-rate, so the last frame can be
    /// earlier). `None` while it is still being recorded.
    #[serde(default)]
    pub video_end_mach_ns: Option<u64>,
    /// Why the segment ended. `None` while it is still being recorded.
    #[serde(default)]
    pub end_reason: Option<EndReason>,
    /// How many real frames were written. A repeat of the last frame of a still display (which
    /// keeps the file moving) is not counted.
    pub frames: u64,
    /// How many frames were discarded because the writer could not take them. Counted by the
    /// macOS backend; the Windows backend does not count drops and always reports `0`.
    pub dropped: u64,
    /// Callbacks that carried no new content. A healthy stream on a static screen still makes
    /// these, so a segment with frames but none of these, or vice versa, tells you which way a
    /// "frozen video" report went wrong. Always `0` on Windows, which does not call back without
    /// a new frame.
    #[serde(default)]
    pub idle_callbacks: u64,
    /// How far the frames' timestamps lagged behind their arrival (the clock check).
    pub latency: LatencyStats,
    /// The error that affected the segment, if any: the first error the capture reported, or a
    /// failure to stop the capture or finalize the file. `"no frames were captured"` for a segment
    /// that never received a frame. `None` for a segment that went well, and for one still being
    /// recorded that has had no error yet.
    pub error: Option<String>,
}

fn first_segment() -> u32 {
    1
}

/// Live counters, readable from any thread while a capture is running.
///
/// A [`RunningCapture`] shares one with the callbacks that feed it and hands it out through
/// [`RunningCapture::stats`]. [`manager::DisplayManager`] reads the error to tell that a stream
/// died and the counters for its [totals](manager::DisplayManager::totals). A time of `0` means
/// "not yet" in the atomic fields. The accessor of the same name turns that into an `Option`, and
/// is the one to prefer.
///
/// Only the first error is kept (see [`CaptureStats::set_error`]).
///
/// # Example
///
/// ```rust
/// use komorebi_recorder::capture::CaptureStats;
/// use std::sync::atomic::Ordering;
///
/// let stats = CaptureStats::default();
/// assert_eq!(stats.first_pts_mach_ns(), None);
///
/// // A capture callback records a frame.
/// stats.first_pts_mach_ns.store(5_000, Ordering::Relaxed);
/// stats.frames.fetch_add(1, Ordering::Relaxed);
/// assert_eq!(stats.first_pts_mach_ns(), Some(5_000));
///
/// // Two errors: the second is a consequence of the first, and is dropped.
/// stats.set_error("stream stopped".into(), 9_000);
/// stats.set_error("could not append frame".into(), 9_500);
/// assert_eq!(stats.error().as_deref(), Some("stream stopped"));
/// assert_eq!(stats.error_at_mach_ns(), Some(9_000));
/// ```
#[derive(Debug, Default)]
pub struct CaptureStats {
    /// Real frames written so far.
    pub frames: AtomicU64,
    /// Frames discarded because the writer could not take them. Only the macOS backend counts
    /// these.
    pub dropped: AtomicU64,
    /// The master-clock time of the first frame, `0` until the first frame arrives. See
    /// [`CaptureStats::first_pts_mach_ns()`].
    pub first_pts_mach_ns: AtomicU64,
    /// The master-clock time of the last real frame written, after any adjustment to keep it
    /// later than what came before, `0` until a frame arrives. See
    /// [`CaptureStats::last_pts_mach_ns()`].
    pub last_pts_mach_ns: AtomicU64,
    /// Callbacks of any kind, including "idle" ones that carry no new content. `ScreenCaptureKit`
    /// keeps calling while a stream is healthy even when the screen is not changing, which makes
    /// this the liveness signal a static display otherwise lacks.
    pub callbacks: AtomicU64,
    /// Callbacks that reported no new content (status idle/blank/suspended). Only the macOS
    /// backend counts these.
    pub idle_callbacks: AtomicU64,
    /// When the last callback of any kind arrived (`0` for none yet), on the master clock. See
    /// [`CaptureStats::last_callback_mach_ns()`].
    pub last_callback_mach_ns: AtomicU64,
    /// When the first error was reported (`0` for none), on the master clock, so a dead stream's
    /// segment can end at the moment it died rather than whenever it was noticed. See
    /// [`CaptureStats::error_at_mach_ns()`].
    pub error_at_mach_ns: AtomicU64,
    error: Mutex<Option<String>>,
}

impl CaptureStats {
    /// Returns the master-clock time of the first frame, or `None` before one has arrived.
    #[must_use]
    pub fn first_pts_mach_ns(&self) -> Option<u64> {
        match self.first_pts_mach_ns.load(Ordering::Relaxed) {
            0 => None,
            ns => Some(ns),
        }
    }

    /// Returns when the last callback of any kind arrived, or `None` if none has.
    #[must_use]
    pub fn last_callback_mach_ns(&self) -> Option<u64> {
        match self.last_callback_mach_ns.load(Ordering::Relaxed) {
            0 => None,
            ns => Some(ns),
        }
    }

    /// Returns the master-clock time of the last real frame written, or `None` before one has
    /// arrived.
    #[must_use]
    pub fn last_pts_mach_ns(&self) -> Option<u64> {
        match self.last_pts_mach_ns.load(Ordering::Relaxed) {
            0 => None,
            ns => Some(ns),
        }
    }

    /// Returns the first error reported, or `None` if there has been none.
    #[must_use]
    pub fn error(&self) -> Option<String> {
        self.error.lock().clone()
    }

    /// Returns when the first error was reported, on the master clock, or `None` if there has
    /// been none.
    #[must_use]
    pub fn error_at_mach_ns(&self) -> Option<u64> {
        match self.error_at_mach_ns.load(Ordering::Relaxed) {
            0 => None,
            ns => Some(ns),
        }
    }

    /// Records the first error and the time it happened; later ones are consequences of it, and
    /// are ignored.
    pub fn set_error(&self, message: String, at_mach_ns: u64) {
        let mut error = self.error.lock();
        if error.is_none() {
            *error = Some(message);
            self.error_at_mach_ns.store(at_mach_ns, Ordering::Relaxed);
        }
    }
}

/// A capture of one display that is in progress: one segment, from its first frame until it is
/// stopped.
///
/// A [`CaptureBackend`] returns one from [`CaptureBackend::start`]. [`manager::DisplayManager`]
/// keeps it while the display is recording, calls [`keepalive`](RunningCapture::keepalive) on every
/// poll, reads its [`stats`](RunningCapture::stats) and
/// [`last_signal_mach_ns`](RunningCapture::last_signal_mach_ns) to tell a dead or stalled stream,
/// and ends it with [`stop`](RunningCapture::stop), which consumes it.
///
/// # What an implementor writes
///
/// | method | required | what it is for |
/// |---|---|---|
/// | [`display_id`](RunningCapture::display_id), [`segment`](RunningCapture::segment) | yes | which display and which segment this is |
/// | [`stats`](RunningCapture::stats) | yes | the live counters and the first error, shared with the capture's callbacks |
/// | [`partial`](RunningCapture::partial) | yes | a snapshot of the segment so far, for manifest checkpoints |
/// | [`stop`](RunningCapture::stop) | yes | ends the segment and finalizes the file |
/// | [`last_signal_mach_ns`](RunningCapture::last_signal_mach_ns) | no: `None` | the liveness signal; without one a stalled capture is never noticed |
/// | [`keepalive`](RunningCapture::keepalive) | no: does nothing | repeats the last frame of an idle display |
///
/// The compiler enforces the required methods: an implementation that leaves one out does not
/// compile, here for `stop`.
///
/// ```rust,compile_fail,E0046
/// # use komorebi_recorder::capture::{CaptureStats, DisplayResult, RunningCapture};
/// # use std::sync::Arc;
/// struct Recording;
///
/// impl RunningCapture for Recording {
///     fn display_id(&self) -> u32 {
///         1
///     }
///     fn segment(&self) -> u32 {
///         1
///     }
///     fn stats(&self) -> Arc<CaptureStats> {
///         Arc::default()
///     }
///     fn partial(&self) -> DisplayResult {
///         unimplemented!()
///     }
///     // no `stop`
/// }
/// ```
///
/// # Example
///
/// A capture that writes nothing, to show the whole contract. The two provided methods are left as
/// they are, so it reports no sign of life and has nothing to keep alive:
///
/// ```rust
/// use komorebi_recorder::capture::{
///     CaptureStats, Codec, DisplayDescriptor, DisplayResult, EndReason, LatencyStats,
///     RunningCapture,
/// };
/// use std::sync::Arc;
///
/// struct Recording {
///     display: DisplayDescriptor,
///     segment: u32,
///     stats: Arc<CaptureStats>,
/// }
/// # impl Recording {
/// #     fn result(&self, reason: Option<EndReason>, end: Option<u64>) -> DisplayResult {
/// #         use komorebi_recorder::capture::{frames_file_name, video_file_name};
/// #         DisplayResult {
/// #             display: self.display.clone(),
/// #             segment: self.segment,
/// #             scale: 1.0,
/// #             width_px: 1920,
/// #             height_px: 1080,
/// #             codec: Codec::H264,
/// #             file: video_file_name(self.display.id, self.segment),
/// #             frames_file: frames_file_name(self.display.id, self.segment),
/// #             video_t0_mach_ns: self.stats.first_pts_mach_ns(),
/// #             video_end_mach_ns: end,
/// #             end_reason: reason,
/// #             frames: 0,
/// #             dropped: 0,
/// #             idle_callbacks: 0,
/// #             latency: LatencyStats::default(),
/// #             error: self.stats.error(),
/// #         }
/// #     }
/// # }
///
/// impl RunningCapture for Recording {
///     fn display_id(&self) -> u32 {
///         self.display.id
///     }
///     fn segment(&self) -> u32 {
///         self.segment
///     }
///     fn stats(&self) -> Arc<CaptureStats> {
///         self.stats.clone()
///     }
///     fn partial(&self) -> DisplayResult {
///         self.result(None, None)
///     }
///     fn stop(self: Box<Self>, reason: EndReason, end_mach_ns: Option<u64>) -> DisplayResult {
///         self.result(Some(reason), end_mach_ns)
///     }
/// }
///
/// let display = DisplayDescriptor {
///     id: 2,
///     frame: [0.0, 0.0, 1920.0, 1080.0],
///     width_points: 1920,
///     height_points: 1080,
///     asleep: false,
///     scale: 1.0,
/// };
/// let capture: Box<dyn RunningCapture> =
///     Box::new(Recording { display, segment: 1, stats: Arc::default() });
///
/// assert_eq!(capture.last_signal_mach_ns(), None);
/// capture.keepalive(1_000_000_000);
///
/// // A running segment has no end yet.
/// let so_far = capture.partial();
/// assert_eq!((so_far.end_reason, so_far.video_end_mach_ns), (None, None));
///
/// // Stopping consumes the capture and returns the final description of the segment.
/// let result = capture.stop(EndReason::Stopped, None);
/// assert_eq!(result.end_reason, Some(EndReason::Stopped));
/// assert_eq!(result.file, "video-2.mp4");
/// ```
pub trait RunningCapture {
    /// Returns the id of the display being captured ([`DisplayDescriptor::id`]).
    fn display_id(&self) -> u32;
    /// Returns this capture's segment number ([`DisplayResult::segment`]).
    fn segment(&self) -> u32;
    /// Returns the live counters, shared with the capture's callbacks.
    ///
    /// The first error a capture reports goes here ([`CaptureStats::set_error`]), which is how
    /// [`manager::DisplayManager`] learns that a stream died.
    fn stats(&self) -> Arc<CaptureStats>;
    /// Returns what is known about this segment so far, without ending it (for checkpoints).
    ///
    /// The result has no [`end_reason`](DisplayResult::end_reason) and no
    /// [`video_end_mach_ns`](DisplayResult::video_end_mach_ns), and carries any error reported so
    /// far.
    fn partial(&self) -> DisplayResult;
    /// Returns when this capture last showed a sign of life: any callback from the capture API,
    /// even an idle one, or its start time if none has arrived yet. `None` if the backend cannot
    /// tell, in which case the capture is never considered stalled.
    ///
    /// The macOS backend reports it, because `ScreenCaptureKit` keeps calling back about thirty
    /// times a second on a healthy stream even when the screen is not changing. The Windows
    /// backend returns `None`: Windows.Graphics.Capture sends no callbacks while the picture does
    /// not change, so silence there is not a sign of death. The default is `None`.
    fn last_signal_mach_ns(&self) -> Option<u64> {
        None
    }
    /// Gives an idle capture the chance to keep itself moving. `now_ns` is the master-clock time
    /// of the call.
    ///
    /// Called on every poll of [`manager::DisplayManager`] (the recorder polls several times a
    /// second) for every live capture. A display whose screen is not changing produces no frames,
    /// which would leave its file unflushed; this is where an idle capture keeps itself moving, by
    /// repeating its last frame. The default does nothing.
    fn keepalive(&self, _now_ns: u64) {}
    /// Ends the segment, finalizes its file and returns the final description of it.
    ///
    /// `end_mach_ns` is when the segment really ended, if that is earlier than now (a stream that
    /// died). Both backends clamp it to no earlier than the last frame written and no later than
    /// the moment of the call, and end the file at that time, so that a display that stopped
    /// changing does not leave a file that ends at its last change.
    ///
    /// This cannot fail. A problem stopping the capture or finalizing the file is recorded in
    /// [`DisplayResult::error`], and a segment that never received a frame is marked there. It
    /// blocks until the file is finalized.
    fn stop(self: Box<Self>, reason: EndReason, end_mach_ns: Option<u64>) -> DisplayResult;
}

/// Where displays and captures come from.
///
/// The seam between the recording policy and the OS. [`manager::DisplayManager`] is generic over
/// it, so the policy is tested against a scripted backend and the OS code is the two methods below.
/// It is implemented by the macOS backend (`sck`), the Windows backend (`wgc`), [`NoDisplays`],
/// and [`Capture`], which forwards to one of the other two. [`crate::platform::capture::Backend`]
/// names the real one for the host.
///
/// # The contract
///
/// - [`active_displays`](CaptureBackend::active_displays) is polled several times a second, so it
///   must be cheap.
/// - An error from it means "could not ask", not "there are no displays".
///   [`manager::DisplayManager`] keeps what is running and reports it once.
/// - [`start`](CaptureBackend::start) is given segment numbers that are 1-based and per display.
///   A number is used up only by a capture that started: after a failed start, the next attempt
///   gets the same number.
/// - A started capture writes `video-<display>[-<n>].mp4` and `frames-<display>[-<n>].csv` into
///   the directory, named by [`video_file_name`] and [`frames_file_name`].
///
/// The compiler enforces that both methods are written:
///
/// ```rust,compile_fail,E0046
/// # use color_eyre::eyre;
/// # use komorebi_recorder::capture::{CaptureBackend, DisplayDescriptor};
/// struct Screen;
///
/// impl CaptureBackend for Screen {
///     fn active_displays(&self) -> eyre::Result<Vec<DisplayDescriptor>> {
///         Ok(Vec::new())
///     }
///     // no `start`
/// }
/// ```
///
/// # Example
///
/// A machine with one display, whose capture does nothing. [`RunningCapture`] shows what the
/// capture itself implements; here it is out of sight.
///
/// ```rust
/// use color_eyre::eyre;
/// use komorebi_recorder::capture::{
///     CaptureBackend, CaptureConfig, DisplayDescriptor, RunningCapture,
/// };
/// use std::path::Path;
/// # use komorebi_recorder::capture::{
/// #     CaptureStats, Codec, DisplayResult, EndReason, LatencyStats, frames_file_name,
/// #     video_file_name,
/// # };
/// # use std::sync::Arc;
/// #
/// # struct Recording {
/// #     display: DisplayDescriptor,
/// #     segment: u32,
/// # }
/// #
/// # impl Recording {
/// #     fn result(&self, reason: Option<EndReason>, end: Option<u64>) -> DisplayResult {
/// #         DisplayResult {
/// #             display: self.display.clone(),
/// #             segment: self.segment,
/// #             scale: 1.0,
/// #             width_px: 1920,
/// #             height_px: 1080,
/// #             codec: Codec::H264,
/// #             file: video_file_name(self.display.id, self.segment),
/// #             frames_file: frames_file_name(self.display.id, self.segment),
/// #             video_t0_mach_ns: None,
/// #             video_end_mach_ns: end,
/// #             end_reason: reason,
/// #             frames: 0,
/// #             dropped: 0,
/// #             idle_callbacks: 0,
/// #             latency: LatencyStats::default(),
/// #             error: None,
/// #         }
/// #     }
/// # }
/// #
/// # impl RunningCapture for Recording {
/// #     fn display_id(&self) -> u32 {
/// #         self.display.id
/// #     }
/// #     fn segment(&self) -> u32 {
/// #         self.segment
/// #     }
/// #     fn stats(&self) -> Arc<CaptureStats> {
/// #         Arc::default()
/// #     }
/// #     fn partial(&self) -> DisplayResult {
/// #         self.result(None, None)
/// #     }
/// #     fn stop(self: Box<Self>, reason: EndReason, end: Option<u64>) -> DisplayResult {
/// #         self.result(Some(reason), end)
/// #     }
/// # }
///
/// struct OneDisplay(DisplayDescriptor);
///
/// impl CaptureBackend for OneDisplay {
///     fn active_displays(&self) -> eyre::Result<Vec<DisplayDescriptor>> {
///         Ok(vec![self.0.clone()])
///     }
///
///     fn start(
///         &self,
///         display: &DisplayDescriptor,
///         segment: u32,
///         _config: &CaptureConfig,
///         _dir: &Path,
///     ) -> eyre::Result<Box<dyn RunningCapture>> {
///         if display.id != self.0.id {
///             eyre::bail!("display {} is not available for capture (yet)", display.id);
///         }
///         Ok(Box::new(Recording { display: display.clone(), segment }))
///     }
/// }
///
/// let screen = DisplayDescriptor {
///     id: 2,
///     frame: [0.0, 0.0, 1920.0, 1080.0],
///     width_points: 1920,
///     height_points: 1080,
///     asleep: false,
///     scale: 1.0,
/// };
/// let backend = OneDisplay(screen.clone());
///
/// let displays = backend.active_displays()?;
/// assert_eq!(displays, vec![screen.clone()]);
///
/// let capture = backend.start(&displays[0], 1, &CaptureConfig::default(), Path::new("."))?;
/// assert_eq!((capture.display_id(), capture.segment()), (2, 1));
///
/// // A display that is not attached cannot be started.
/// let missing = DisplayDescriptor { id: 9, ..screen };
/// assert!(backend.start(&missing, 1, &CaptureConfig::default(), Path::new(".")).is_err());
/// # Ok::<(), Box<dyn std::error::Error>>(())
/// ```
pub trait CaptureBackend {
    /// Returns the displays that exist right now, asleep ones included (see
    /// [`DisplayDescriptor::asleep`]). Must be cheap: it is polled several times a second.
    ///
    /// # Errors
    ///
    /// Returns an error when the OS cannot be asked for its displays. The caller treats that as
    /// "unknown", not as "none": [`manager::DisplayManager::poll`] changes nothing and reports it
    /// once.
    fn active_displays(&self) -> eyre::Result<Vec<DisplayDescriptor>>;

    /// Starts capturing `display` into `dir` as segment number `segment`, and returns the running
    /// capture.
    ///
    /// # Errors
    ///
    /// Returns an error when the capture cannot begin: the display is not available for capture
    /// (yet), the screen is locked, a permission is missing, or the file cannot be created. The
    /// caller does not give up. [`manager::DisplayManager`] retries with a backoff and reports the
    /// outage once.
    fn start(
        &self,
        display: &DisplayDescriptor,
        segment: u32,
        config: &CaptureConfig,
        dir: &Path,
    ) -> eyre::Result<Box<dyn RunningCapture>>;
}

/// The backend of a recording made without video: no display exists, so nothing is ever captured
/// and nothing about the screen is asked of the OS.
///
/// [`active_displays`](CaptureBackend::active_displays) is always an empty list, and
/// [`start`](CaptureBackend::start) always fails. [`Capture::new`] builds it for
/// `record --no-video`.
///
/// # Example
///
/// ```rust
/// use komorebi_recorder::capture::{CaptureBackend, CaptureConfig, DisplayDescriptor, NoDisplays};
/// use std::path::Path;
///
/// assert!(NoDisplays.active_displays()?.is_empty());
///
/// let display = DisplayDescriptor {
///     id: 1,
///     frame: [0.0; 4],
///     width_points: 1,
///     height_points: 1,
///     asleep: false,
///     scale: 1.0,
/// };
/// let error = NoDisplays
///     .start(&display, 1, &CaptureConfig::default(), Path::new("."))
///     .err()
///     .expect("there is nothing to start");
/// assert_eq!(error.to_string(), "this recording has no video");
/// # Ok::<(), Box<dyn std::error::Error>>(())
/// ```
pub struct NoDisplays;

impl CaptureBackend for NoDisplays {
    fn active_displays(&self) -> eyre::Result<Vec<DisplayDescriptor>> {
        Ok(Vec::new())
    }

    fn start(
        &self,
        _display: &DisplayDescriptor,
        _segment: u32,
        _config: &CaptureConfig,
        _dir: &Path,
    ) -> eyre::Result<Box<dyn RunningCapture>> {
        Err(eyre::Report::msg("this recording has no video"))
    }
}

/// The host's capture, or none at all (`record --no-video`).
///
/// This is the [`CaptureBackend`] the recorder runs [`manager::DisplayManager`] with. It forwards
/// every call to the backend it wraps, so the choice between recording video and not is made once,
/// when it is built, and nothing downstream branches on it.
pub enum Capture {
    /// The backend of the host the recorder is running on: `ScreenCaptureKit` on macOS,
    /// Windows.Graphics.Capture on Windows, and on any other host one whose methods fail with a
    /// message that recording is not supported there.
    Host(crate::platform::capture::Backend),
    /// No video: see [`NoDisplays`].
    Nothing(NoDisplays),
}

impl Capture {
    /// Returns the host's backend if `video` is `true`, and [`NoDisplays`] otherwise.
    ///
    /// Building either asks the OS nothing: neither permission nor displays.
    ///
    /// # Example
    ///
    /// ```rust
    /// use komorebi_recorder::capture::{Capture, CaptureBackend};
    ///
    /// // A recording without video has no displays to follow, and no screen is asked about.
    /// let capture = Capture::new(false);
    /// assert!(matches!(capture, Capture::Nothing(_)));
    /// assert!(capture.active_displays()?.is_empty());
    ///
    /// // With video it is the host's backend. Listing its displays would ask the OS, so it is
    /// // only built here.
    /// assert!(matches!(Capture::new(true), Capture::Host(_)));
    /// # Ok::<(), Box<dyn std::error::Error>>(())
    /// ```
    #[must_use]
    pub fn new(video: bool) -> Self {
        if video {
            Capture::Host(crate::platform::capture::Backend)
        } else {
            Capture::Nothing(NoDisplays)
        }
    }
}

impl CaptureBackend for Capture {
    fn active_displays(&self) -> eyre::Result<Vec<DisplayDescriptor>> {
        match self {
            Capture::Host(backend) => backend.active_displays(),
            Capture::Nothing(backend) => backend.active_displays(),
        }
    }

    fn start(
        &self,
        display: &DisplayDescriptor,
        segment: u32,
        config: &CaptureConfig,
        dir: &Path,
    ) -> eyre::Result<Box<dyn RunningCapture>> {
        match self {
            Capture::Host(backend) => backend.start(display, segment, config, dir),
            Capture::Nothing(backend) => backend.start(display, segment, config, dir),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_recording_without_video_has_no_displays_to_follow() {
        let backend = Capture::new(false);
        assert!(backend.active_displays().expect("displays").is_empty());
        let display = DisplayDescriptor {
            id: 1,
            frame: [0.0; 4],
            width_points: 1,
            height_points: 1,
            asleep: false,
            scale: 1.0,
        };
        assert!(
            backend
                .start(&display, 1, &CaptureConfig::default(), Path::new("."))
                .is_err()
        );
    }

    #[test]
    fn host_clock_check_accepts_measured_latencies_and_rejects_other_clocks() {
        // measured on a real display
        let measured = LatencyStats {
            samples: 229,
            min_ns: -7_506_167,
            max_ns: 196_015_958,
            mean_ns: 6_524_913,
        };
        assert!(measured.looks_like_host_clock());

        // a different clock (e.g. wall time vs mach time) is off by far more than this
        let wrong = LatencyStats {
            samples: 10,
            min_ns: 3_600_000_000_000,
            max_ns: 3_600_100_000_000,
            mean_ns: 3_600_050_000_000,
        };
        assert!(!wrong.looks_like_host_clock());

        let negative = LatencyStats {
            samples: 10,
            min_ns: -3_000_000_000,
            max_ns: -2_900_000_000,
            mean_ns: -2_950_000_000,
        };
        assert!(!negative.looks_like_host_clock());

        assert!(
            LatencyStats::default().looks_like_host_clock(),
            "no frames, nothing to doubt"
        );
    }

    #[test]
    fn segment_file_names_keep_the_first_segment_plain() {
        assert_eq!(video_file_name(2, 1), "video-2.mp4");
        assert_eq!(frames_file_name(2, 1), "frames-2.csv");
        assert_eq!(video_file_name(2, 2), "video-2-2.mp4");
        assert_eq!(frames_file_name(2, 3), "frames-2-3.csv");
        // a display id that ends in the same digits as a segment number cannot collide
        assert_ne!(video_file_name(1, 2), video_file_name(12, 1));
    }

    #[test]
    fn only_a_size_change_invalidates_a_capture() {
        let display = |x: f64, w: i64, h: i64| DisplayDescriptor {
            id: 1,
            frame: [x, 0.0, w as f64, h as f64],
            width_points: w,
            height_points: h,
            asleep: false,
            scale: 1.0,
        };

        assert!(!display(0.0, 1800, 1169).size_differs(&display(-1800.0, 1800, 1169)));
        assert!(display(0.0, 1800, 1169).size_differs(&display(0.0, 1512, 982)));
    }

    #[test]
    fn only_the_first_error_is_kept() {
        let stats = CaptureStats::default();
        assert_eq!(stats.error(), None);
        stats.set_error("first".into(), 100);
        stats.set_error("second".into(), 200);
        assert_eq!(stats.error().as_deref(), Some("first"));
        assert_eq!(stats.error_at_mach_ns(), Some(100));
    }

    #[test]
    fn old_manifests_without_segment_fields_still_parse() {
        let old = r#"{
            "display": {"id": 2, "frame": [0.0, 0.0, 3840.0, 1620.0], "width_points": 3840, "height_points": 1620},
            "scale": 1.0, "width_px": 3840, "height_px": 1620, "codec": "hevc",
            "file": "video-2.mp4", "frames_file": "frames-2.csv",
            "video_t0_mach_ns": 5, "frames": 3, "dropped": 0,
            "latency": {"samples": 0, "min_ns": 0, "max_ns": 0, "mean_ns": 0},
            "error": null
        }"#;
        let result: DisplayResult = serde_json::from_str(old).expect("old format parses");
        assert_eq!(result.segment, 1);
        assert_eq!(result.video_end_mach_ns, None);
        assert_eq!(result.end_reason, None);
    }
}
