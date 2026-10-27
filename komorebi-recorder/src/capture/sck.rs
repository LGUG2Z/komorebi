//! The macOS capture backend: `ScreenCaptureKit` frames written straight to an `AVAssetWriter` MP4.
//!
//! This module exists on macOS only. [`SckBackend`] is the [`CaptureBackend`] and
//! [`DisplayCapture`] the [`RunningCapture`]; [`crate::platform::capture`] re-exports
//! `SckBackend` as the host's `Backend`, together with [`active_displays`], [`ensure_permission`],
//! [`has_permission`] and [`media_duration_s`]. The Objective-C interop for capturing is isolated
//! here (decoding recorded video is `av`).
//!
//! A frame's `CMSampleBuffer` presentation timestamp is on the host clock (the same clock as
//! `mach_absolute_time`), which is what lets the video line up exactly with the unified log. It is
//! converted to nanoseconds on the master clock with the host timebase, and the difference between
//! it and the time the callback ran is recorded in the segment's
//! [`LatencyStats`](crate::capture::LatencyStats).
//!
//! # How a segment is recorded
//!
//! 1. [`DisplayCapture::start`] finds the display among `ScreenCaptureKit`'s shareable content,
//!    builds a filter that leaves out this process and any other `komorebi-recorder` process, and
//!    configures the stream: the size, `fps` as the minimum frame interval, BGRA pixels, the cursor
//!    shown. It creates the `AVAssetWriter` (an MP4 cut into movie fragments every second, so a
//!    killed recorder leaves a file that plays up to the last fragment) and the frame log with its
//!    header row, and starts the stream.
//! 2. Each frame arrives on a serial dispatch queue. A callback that carries no new content (the
//!    frame status is idle, blank or suspended) is counted in
//!    [`CaptureStats::idle_callbacks`] and dropped. A real frame starts the writer's session at its
//!    timestamp if it is the first, and is appended and logged. If the writer is not ready for more
//!    media data the frame is dropped and counted in [`CaptureStats::dropped`]. A frame that is not
//!    later than the last thing appended is moved 1 ms after it.
//! 3. [`RunningCapture::keepalive`] re-submits the last frame when the display has shown nothing
//!    new for half a second and the stream has called back within the last two seconds. A static
//!    screen advances neither media time nor the fragments, so without this a crash would lose
//!    everything since the last change.
//! 4. [`RunningCapture::stop`] waits up to three seconds for `ScreenCaptureKit` to confirm that the
//!    stream stopped (not at all when it had already reported an error), lets a frame callback in
//!    flight finish, ends the writer's session at the segment's end time and waits up to ten
//!    seconds for the file to be finalized. A timeout is reported in
//!    [`DisplayResult::error`].
//!
//! The placement, frame-log and keep-alive rules of steps 2 and 3 are the ones in
//! [`crate::capture::segment`], written inline here with the same numbers. This module uses only
//! [`even`] and [`Latency`] from there.
//!
//! # Permission
//!
//! Capturing needs Screen Recording permission. [`has_permission`] asks without prompting and
//! [`ensure_permission`] prompts. The grant attaches to the responsible process (normally the
//! terminal the binary was launched from), exactly as it does for komorebi itself.
//!
//! # Tests
//!
//! The only unit test here is for [`is_recorder_application`]. Capturing needs a screen and the
//! permission, so [`DisplayCapture`] is exercised by `doctor --probe capture` and by recordings;
//! the policy above it is tested against a scripted backend in [`crate::capture::manager`].

use crate::capture::CaptureBackend;
use crate::capture::CaptureConfig;
use crate::capture::CaptureStats;
use crate::capture::Codec;
use crate::capture::DisplayDescriptor;
use crate::capture::DisplayResult;
use crate::capture::EndReason;
use crate::capture::RunningCapture;
use crate::capture::frames_file_name;
use crate::capture::segment::Latency;
use crate::capture::segment::even;
use crate::capture::video_file_name;
use crate::clock;
use block2::RcBlock;
use color_eyre::eyre;
use color_eyre::eyre::WrapErr;
use dispatch2::DispatchQueue;
use dispatch2::DispatchQueueAttr;
use dispatch2::DispatchRetained;
use objc2::AllocAnyThread;
use objc2::DefinedClass;
use objc2::define_class;
use objc2::msg_send;
use objc2::rc::Retained;
use objc2::runtime::AnyObject;
use objc2::runtime::NSObject;
use objc2::runtime::NSObjectProtocol;
use objc2::runtime::ProtocolObject;
use objc2_av_foundation::AVAsset;
use objc2_av_foundation::AVAssetWriter;
use objc2_av_foundation::AVAssetWriterInput;
use objc2_av_foundation::AVFileTypeMPEG4;
use objc2_av_foundation::AVMediaTypeVideo;
use objc2_av_foundation::AVVideoCodecKey;
use objc2_av_foundation::AVVideoCodecTypeH264;
use objc2_av_foundation::AVVideoCodecTypeHEVC;
use objc2_av_foundation::AVVideoHeightKey;
use objc2_av_foundation::AVVideoWidthKey;
use objc2_core_foundation::CFDictionary;
use objc2_core_foundation::CFRetained;
use objc2_core_graphics::CGDisplayBounds;
use objc2_core_graphics::CGDisplayCopyDisplayMode;
use objc2_core_graphics::CGDisplayIsAsleep;
use objc2_core_graphics::CGDisplayMode;
use objc2_core_graphics::CGError;
use objc2_core_graphics::CGGetActiveDisplayList;
use objc2_core_graphics::CGMainDisplayID;
use objc2_core_graphics::CGPreflightScreenCaptureAccess;
use objc2_core_graphics::CGRequestScreenCaptureAccess;
use objc2_core_media::CMClock;
use objc2_core_media::CMSampleBuffer;
use objc2_core_media::CMSampleTimingInfo;
use objc2_core_media::CMTime;
use objc2_core_media::kCMTimeInvalid;
use objc2_foundation::NSArray;
use objc2_foundation::NSDictionary;
use objc2_foundation::NSError;
use objc2_foundation::NSNumber;
use objc2_foundation::NSString;
use objc2_foundation::NSURL;
use objc2_screen_capture_kit::SCContentFilter;
use objc2_screen_capture_kit::SCFrameStatus;
use objc2_screen_capture_kit::SCRunningApplication;
use objc2_screen_capture_kit::SCShareableContent;
use objc2_screen_capture_kit::SCStream;
use objc2_screen_capture_kit::SCStreamConfiguration;
use objc2_screen_capture_kit::SCStreamDelegate;
use objc2_screen_capture_kit::SCStreamFrameInfoStatus;
use objc2_screen_capture_kit::SCStreamOutput;
use objc2_screen_capture_kit::SCStreamOutputType;
use objc2_screen_capture_kit::SCWindow;
use parking_lot::Mutex;
use std::ffi::c_void;
use std::fs::File;
use std::io::BufWriter;
use std::io::Write;
use std::path::Path;
use std::ptr::NonNull;
use std::sync::Arc;
use std::sync::atomic::Ordering;
use std::sync::mpsc;
use std::time::Duration;
use std::time::Instant;

/// `kCVPixelFormatType_32BGRA`
const PIXEL_FORMAT_BGRA: u32 = 0x4247_5241;

const CALLBACK_TIMEOUT: Duration = Duration::from_secs(10);

/// How often a movie fragment is flushed to disk, which bounds how much video a crash can lose.
const FRAGMENT_SECONDS: i64 = 1;

/// How long a display may go without a new frame before its last frame is re-submitted.
///
/// `ScreenCaptureKit` only produces frames when the screen *changes*, and movie fragments are cut
/// on media time. A static screen therefore advances neither: its fragments are never flushed, so
/// a crash would lose everything since the last change, and a display that never changed after
/// the first second would leave an unreadable file. Re-submitting the last frame costs a few
/// bytes (it is identical) and keeps the encoder and the file moving.
const KEEPALIVE_AFTER: Duration = Duration::from_millis(500);

/// A stream silent for longer than this is not kept alive (see `keepalive`).
const KEEPALIVE_STREAM_TIMEOUT_NS: u64 = 2_000_000_000;

/// How often the frame timestamp log is flushed, which bounds what a crash can lose from it.
const CSV_FLUSH_INTERVAL: Duration = Duration::from_secs(1);

/// Stopping a stream that is already dead has nothing to wait for; a live one answers quickly.
const STOP_TIMEOUT: Duration = Duration::from_secs(3);

/// Returns `true` if and only if Screen Recording permission is already granted. Never prompts.
#[must_use]
pub fn has_permission() -> bool {
    CGPreflightScreenCaptureAccess()
}

/// Makes sure this process has Screen Recording permission, prompting the user if it does not.
///
/// The grant attaches to the *responsible* process (normally the terminal the binary was
/// launched from), exactly as it does for komorebi itself.
///
/// # Errors
///
/// Returns an error when the permission is not granted, whether already or after the prompt this
/// call showed. The message names where to grant it (System Settings > Privacy & Security >
/// Screen & System Audio Recording) and tells the user to restart the recorder afterwards.
pub fn ensure_permission() -> eyre::Result<()> {
    // connect to the window server; without this ScreenCaptureKit asserts in a bare CLI process
    let _ = CGMainDisplayID();

    if CGPreflightScreenCaptureAccess() {
        return Ok(());
    }

    if CGRequestScreenCaptureAccess() {
        return Ok(());
    }

    Err(eyre::Report::msg(
        "Screen Recording permission is required: grant it to the terminal (or app) launching \
         komorebi-recorder in System Settings > Privacy & Security > Screen & System Audio \
         Recording, then restart it",
    ))
}

fn error_message(error: *mut NSError) -> Option<String> {
    // SAFETY: ScreenCaptureKit passes either null or a valid NSError for the duration of the
    // completion handler
    unsafe { error.as_ref() }.map(|error| error.localizedDescription().to_string())
}

/// Run an asynchronous `ScreenCaptureKit` call that reports through an `NSError` completion
/// handler and wait for it.
fn wait_for_completion(
    what: &str,
    timeout: Duration,
    call: impl FnOnce(&RcBlock<dyn Fn(*mut NSError)>),
) -> eyre::Result<()> {
    let (tx, rx) = mpsc::channel();
    let block = RcBlock::new(move |error: *mut NSError| {
        let _ = tx.send(error_message(error));
    });

    call(&block);

    match rx.recv_timeout(timeout) {
        Ok(None) => Ok(()),
        Ok(Some(message)) => Err(eyre::Report::msg(format!("{what} failed: {message}"))),
        Err(_) => Err(eyre::Report::msg(format!("{what} timed out"))),
    }
}

fn shareable_content() -> eyre::Result<Retained<SCShareableContent>> {
    ensure_permission()?;

    let (tx, rx) = mpsc::channel();
    let block = RcBlock::new(
        move |content: *mut SCShareableContent, error: *mut NSError| {
            // SAFETY: a non-null content pointer is a valid +0 object that we retain before the
            // handler returns
            let content = unsafe { Retained::retain(content) };
            let _ = tx.send((content.map(SendRetained), error_message(error)));
        },
    );

    // SAFETY: the block outlives the call and matches the expected signature
    unsafe { SCShareableContent::getShareableContentWithCompletionHandler(&block) };

    match rx.recv_timeout(CALLBACK_TIMEOUT) {
        Ok((Some(content), _)) => Ok(content.0),
        Ok((None, error)) => Err(eyre::Report::msg(format!(
            "could not enumerate displays: {}",
            error.unwrap_or_else(|| "unknown error".to_string())
        ))),
        Err(_) => Err(eyre::Report::msg("timed out enumerating displays")),
    }
}

/// An Objective-C object we promise is safe to hand across threads. Only used to move the result
/// of a completion handler to the waiting thread.
struct SendRetained<T>(Retained<T>);

// SAFETY: the wrapped object is only used by the thread that receives it
unsafe impl<T> Send for SendRetained<T> {}

/// Returns the playable duration of a video file as `AVFoundation` reads it, in seconds.
///
/// This is what recovery trusts for a file whose recorder died: the frame log is flushed on a
/// timer and can lag behind (or run ahead of) what actually reached the video. `ffprobe`'s
/// duration for a truncated fragmented MP4 under-reads, so it is not used for this.
///
/// Returns `None` if the path is not valid UTF-8, or `AVFoundation` reports no usable duration for
/// the file (a negative one, or one with no timescale).
#[must_use]
pub fn media_duration_s(path: &Path) -> Option<f64> {
    let path = path.to_str()?;
    // SAFETY: plain object construction and a property read on a live object
    let duration = unsafe {
        let url = NSURL::fileURLWithPath(&NSString::from_str(path));
        AVAsset::assetWithURL(&url).duration()
    };

    (duration.timescale > 0 && duration.value >= 0)
        .then(|| duration.value as f64 / f64::from(duration.timescale))
}

/// Returns `true` if and only if a running application is this recorder or another
/// `komorebi-recorder` process, which should be left out of the capture.
///
/// An application matches when its `pid` is `own_pid`, or when its `name` is `komorebi-recorder`
/// (compared without regard to ASCII case) as a whole: the viewer is a separate process, often open
/// beside a recording, and must stay out of the video too. Names that only start with it, and
/// komorebi itself, do not match.
///
/// # Example
///
/// ```rust
/// use komorebi_recorder::capture::sck::is_recorder_application;
///
/// // This process, by pid.
/// assert!(is_recorder_application(42, "whatever", 42));
/// // Another recorder, by name.
/// assert!(is_recorder_application(7, "Komorebi-Recorder", 42));
/// // Other applications, including komorebi itself and similarly named ones.
/// assert!(!is_recorder_application(7, "komorebi", 42));
/// assert!(!is_recorder_application(7, "komorebi-recorder-helper", 42));
/// ```
#[must_use]
pub fn is_recorder_application(pid: i32, name: &str, own_pid: i32) -> bool {
    pid == own_pid || name.eq_ignore_ascii_case("komorebi-recorder")
}

/// How many pixels the display really has for each point: the display mode's pixel width over its
/// width in points (`CGDisplayPixelsWide` would not do, it reports points for a scaled mode).
fn backing_scale(id: u32) -> f64 {
    let Some(mode) = CGDisplayCopyDisplayMode(id) else {
        return 1.0;
    };

    let (pixels, points) = (
        CGDisplayMode::pixel_width(Some(&mode)),
        CGDisplayMode::width(Some(&mode)),
    );
    if points > 0 && pixels > 0 {
        pixels as f64 / points as f64
    } else {
        1.0
    }
}

/// Returns the displays that are attached and drawable right now, from CoreGraphics.
///
/// This is deliberately not `ScreenCaptureKit`: it is synchronous and cheap enough to poll several
/// times a second, needs no permission, and is what tells us a display was unplugged or has gone
/// to sleep before any stream notices.
///
/// Each descriptor has its bounds in global logical points, whether the display is asleep, and its
/// backing scale (see [`DisplayDescriptor::scale`]). At most 32 displays are reported.
///
/// # Errors
///
/// Returns an error when `CGGetActiveDisplayList` fails; the message has its status code.
pub fn active_displays() -> eyre::Result<Vec<DisplayDescriptor>> {
    // a connection to the window server is required before any display call
    let _ = CGMainDisplayID();

    let mut ids = [0_u32; 32];
    let mut count = 0_u32;
    // SAFETY: both pointers are valid for the duration of the call and `ids` has room for
    // `ids.len()` entries
    let status =
        unsafe { CGGetActiveDisplayList(ids.len() as u32, ids.as_mut_ptr(), &raw mut count) };
    if status != CGError::Success {
        eyre::bail!("CGGetActiveDisplayList failed with {}", status.0);
    }

    Ok(ids[..(count as usize).min(ids.len())]
        .iter()
        .map(|id| {
            let bounds = CGDisplayBounds(*id);
            DisplayDescriptor {
                id: *id,
                frame: [
                    bounds.origin.x,
                    bounds.origin.y,
                    bounds.size.width,
                    bounds.size.height,
                ],
                width_points: bounds.size.width.round() as i64,
                height_points: bounds.size.height.round() as i64,
                asleep: CGDisplayIsAsleep(*id),
                scale: backing_scale(*id),
            }
        })
        .collect())
}

/// The real, `ScreenCaptureKit`-backed [`CaptureBackend`].
///
/// [`active_displays`](CaptureBackend::active_displays) is [`active_displays`], and
/// [`start`](CaptureBackend::start) starts a [`DisplayCapture`]. On a Mac,
/// [`crate::platform::capture::Backend`] is this type.
pub struct SckBackend;

impl CaptureBackend for SckBackend {
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
        Ok(Box::new(DisplayCapture::start(
            display, segment, config, dir,
        )?))
    }
}

/// Everything the frame callback mutates, guarded by a mutex so `stop` can finalize safely.
struct WriterState {
    writer: Retained<AVAssetWriter>,
    input: Retained<AVAssetWriterInput>,
    frames_csv: BufWriter<File>,
    last_flush: Instant,
    started: bool,
    finished: bool,
    frame_index: u64,
    latency: Latency,
    /// The most recent real frame, kept so an idle display can be kept alive.
    last_sample: Option<CFRetained<CMSampleBuffer>>,
    /// Presentation time (mach ns) of the last sample appended, real or keep-alive. Everything
    /// appended must be strictly later than this.
    last_append_ns: u64,
}

struct Shared {
    stats: Arc<CaptureStats>,
    state: Mutex<WriterState>,
}

// SAFETY: the Objective-C objects inside `WriterState` are only ever touched while holding the
// mutex, and `AVAssetWriter`/`AVAssetWriterInput` have no thread affinity
unsafe impl Send for Shared {}
// SAFETY: see above
unsafe impl Sync for Shared {}

/// The `SCStreamOutput` frame sink and `SCStreamDelegate` error sink.
struct OutputIvars {
    shared: Arc<Shared>,
}

define_class!(
    // SAFETY: NSObject has no subclassing requirements and this class does not implement Drop
    #[unsafe(super(NSObject))]
    #[name = "KomorebiRecorderStreamOutput"]
    #[ivars = OutputIvars]
    struct StreamOutput;

    unsafe impl NSObjectProtocol for StreamOutput {}

    unsafe impl SCStreamOutput for StreamOutput {
        #[unsafe(method(stream:didOutputSampleBuffer:ofType:))]
        fn stream_did_output_sample_buffer_of_type(
            &self,
            _stream: &SCStream,
            sample_buffer: &CMSampleBuffer,
            kind: SCStreamOutputType,
        ) {
            if kind == SCStreamOutputType::Screen {
                handle_frame(&self.ivars().shared, sample_buffer);
            }
        }
    }

    unsafe impl SCStreamDelegate for StreamOutput {
        #[unsafe(method(stream:didStopWithError:))]
        fn stream_did_stop_with_error(&self, _stream: &SCStream, error: &NSError) {
            self.ivars().shared.stats.set_error(
                format!("stream stopped: {}", error.localizedDescription()),
                clock::mono_now_ns(),
            );
        }
    }
);

impl StreamOutput {
    fn new(shared: Arc<Shared>) -> Retained<Self> {
        let this = Self::alloc().set_ivars(OutputIvars { shared });
        // SAFETY: NSObject's `init` is always valid
        unsafe { msg_send![super(this), init] }
    }
}

/// Read the `SCFrameStatus` attached to a sample buffer, if present.
///
/// # Safety
///
/// `sample` must be a valid sample buffer delivered by `ScreenCaptureKit`.
unsafe fn frame_status(sample: &CMSampleBuffer) -> Option<isize> {
    let attachments = unsafe { sample.sample_attachments_array(false) }?;
    if attachments.count() == 0 {
        return None;
    }

    let dictionary = unsafe { attachments.value_at_index(0) }.cast::<CFDictionary>();
    // SAFETY: element 0 of the attachments array is a CFDictionary
    let dictionary = unsafe { dictionary.as_ref() }?;
    let key = std::ptr::from_ref::<NSString>(unsafe { SCStreamFrameInfoStatus }).cast::<c_void>();
    let value = unsafe { dictionary.value(key) }.cast::<NSNumber>();
    // SAFETY: the status value is an NSNumber when present
    unsafe { value.as_ref() }.map(NSNumber::integerValue)
}

fn handle_frame(shared: &Shared, sample: &CMSampleBuffer) {
    let received_ns = clock::mono_now_ns();
    shared.stats.callbacks.fetch_add(1, Ordering::Relaxed);
    shared
        .stats
        .last_callback_mach_ns
        .store(received_ns, Ordering::Relaxed);

    // SAFETY: `sample` is a live sample buffer for the duration of the callback
    let (pts, pts_ns) = unsafe {
        if !sample.is_valid() || !sample.data_is_ready() {
            return;
        }

        // idle/blank/suspended frames carry no new content
        if let Some(status) = frame_status(sample)
            && status != SCFrameStatus::Complete.0
        {
            shared.stats.idle_callbacks.fetch_add(1, Ordering::Relaxed);
            return;
        }

        if sample.image_buffer().is_none() {
            return;
        }

        let pts = sample.presentation_time_stamp();
        let ticks = CMClock::convert_host_time_to_system_units(pts);
        (pts, clock::timebase().ticks_to_ns(ticks))
    };

    let mut state = shared.state.lock();
    if state.finished {
        return;
    }

    if !state.started {
        // SAFETY: the writer is configured and has an input attached
        let started = unsafe {
            if state.writer.startWriting() {
                state.writer.startSessionAtSourceTime(pts);
                true
            } else {
                false
            }
        };

        if !started {
            shared.stats.set_error(
                format!("could not start writing: {}", writer_error(&state)),
                received_ns,
            );
            state.finished = true;
            return;
        }

        state.started = true;
        shared
            .stats
            .first_pts_mach_ns
            .store(pts_ns, Ordering::Relaxed);
    }

    // SAFETY: the input belongs to a writer that is currently writing
    let ready = unsafe { state.input.isReadyForMoreMediaData() };
    if !ready {
        shared.stats.dropped.fetch_add(1, Ordering::Relaxed);
        return;
    }

    // A keep-alive may have been stamped a hair ahead of this real frame (frame timestamps run
    // slightly ahead of when they are received). The writer rejects out-of-order samples, so move
    // the frame just past it rather than lose the frame or the segment.
    let mut pts_ns = pts_ns;
    let adjusted = if pts_ns <= state.last_append_ns {
        pts_ns = state.last_append_ns + 1_000_000;
        retimed(sample, pts_ns)
    } else {
        None
    };

    // SAFETY: as above
    let appended = unsafe {
        match &adjusted {
            Some(copy) => state.input.appendSampleBuffer(copy),
            None => state.input.appendSampleBuffer(sample),
        }
    };

    if !appended {
        shared.stats.set_error(
            format!("could not append frame: {}", writer_error(&state)),
            received_ns,
        );
        state.finished = true;
        return;
    }

    state.last_append_ns = pts_ns;
    // SAFETY: `sample` is a valid sample buffer; retaining it keeps it alive past the callback
    state.last_sample = Some(unsafe { CFRetained::retain(NonNull::from(sample)) });

    let index = state.frame_index;
    state.frame_index += 1;
    state.latency.record(received_ns as i64 - pts_ns as i64);

    // a failed csv write must not take down the capture; the mp4 is the source of truth
    let _ = writeln!(state.frames_csv, "{index},{pts_ns},{received_ns}");
    if state.last_flush.elapsed() >= CSV_FLUSH_INTERVAL {
        // flushed on whole lines, so a crash never leaves a torn row behind
        let _ = state.frames_csv.flush();
        state.last_flush = Instant::now();
    }

    shared.stats.frames.fetch_add(1, Ordering::Relaxed);
    shared
        .stats
        .last_pts_mach_ns
        .store(pts_ns, Ordering::Relaxed);
}

/// A copy of `sample` presented at `pts_ns` on the host clock.
fn retimed(sample: &CMSampleBuffer, pts_ns: u64) -> Option<CFRetained<CMSampleBuffer>> {
    let ticks = clock::timebase().ns_to_ticks(pts_ns);
    // SAFETY: plain value construction and a Core Media call whose out pointer is valid; on
    // success the returned buffer is +1 and ownership is taken by `CFRetained::from_raw`
    unsafe {
        let timing = CMSampleTimingInfo {
            duration: kCMTimeInvalid,
            presentationTimeStamp: CMClock::make_host_time_from_system_units(ticks),
            decodeTimeStamp: kCMTimeInvalid,
        };
        let mut out: *mut CMSampleBuffer = std::ptr::null_mut();
        let status = CMSampleBuffer::create_copy_with_new_timing(
            None,
            sample,
            1,
            &raw const timing,
            NonNull::from(&mut out),
        );

        if status != 0 {
            return None;
        }

        NonNull::new(out).map(|out| CFRetained::from_raw(out))
    }
}

fn writer_error(state: &WriterState) -> String {
    // SAFETY: plain property reads on a live object
    unsafe {
        state.writer.error().map_or_else(
            || "unknown error".to_string(),
            |e| e.localizedDescription().to_string(),
        )
    }
}

/// An in-progress capture of one display: one segment, recorded by `ScreenCaptureKit` into an
/// `AVAssetWriter`.
///
/// Built by [`DisplayCapture::start`] and used through [`RunningCapture`]. Its liveness signal is
/// the time of the last callback of any kind, idle ones included, or its start time before the
/// first arrives, so [`DisplayManager`](crate::capture::manager::DisplayManager) can tell a stalled
/// stream. [`stop`](RunningCapture::stop) always returns a [`DisplayResult`]: a segment that never
/// received a frame has the error `no frames were captured`.
pub struct DisplayCapture {
    descriptor: DisplayDescriptor,
    segment: u32,
    started_mach_ns: u64,
    scale: f64,
    width_px: usize,
    height_px: usize,
    codec: Codec,
    file: String,
    frames_file: String,
    stream: Retained<SCStream>,
    // kept alive for as long as the stream may call into it
    _output: Retained<StreamOutput>,
    queue: DispatchRetained<DispatchQueue>,
    shared: Arc<Shared>,
}

impl DisplayCapture {
    /// Starts capturing `display` into `dir` as the given segment, excluding this process's own
    /// windows.
    ///
    /// Returns once `ScreenCaptureKit` confirms the stream started, without waiting for a frame:
    /// `video_t0_mach_ns` is `None` until one arrives. The video is `config.codec` at the
    /// display's size in points, or at its native pixels when `config.retina` is set, rounded to
    /// even numbers. The files are `video-<display>[-<n>].mp4` and `frames-<display>[-<n>].csv`
    /// in `dir`.
    ///
    /// # Errors
    ///
    /// Returns an error when:
    ///
    /// - Screen Recording permission is missing ([`ensure_permission`]);
    /// - `ScreenCaptureKit` cannot list its shareable content, or takes more than ten seconds to
    ///   (`could not enumerate displays`);
    /// - `display` is not among that content, which is the usual case for a display that has only
    ///   just appeared or is not ready (`display <id> is not available for capture (yet)`);
    /// - the video writer cannot be created: `dir` is not valid UTF-8, `AVFoundation` refuses the
    ///   file or the codec, or the writer rejects the input configuration;
    /// - the frame log cannot be created in `dir`;
    /// - the frame output cannot be attached to the stream;
    /// - the stream does not start, or does not within ten seconds. The two files created for the
    ///   segment are removed in this case.
    ///
    /// A caller that retries gets the same segment number again: see [`CaptureBackend::start`].
    pub fn start(
        display: &DisplayDescriptor,
        segment: u32,
        config: &CaptureConfig,
        dir: &Path,
    ) -> eyre::Result<Self> {
        let content = shareable_content()?;

        // SAFETY: plain property reads on live objects
        let (sc_display, own_apps) = unsafe {
            let displays = content.displays();
            let sc_display = displays
                .iter()
                .find(|candidate| candidate.displayID() == display.id)
                .ok_or_else(|| {
                    eyre::Report::msg(format!(
                        "display {} is not available for capture (yet)",
                        display.id
                    ))
                })?;

            // this process, and any other komorebi-recorder (the viewer is its own process and is
            // often open beside a recording): neither belongs in the video
            let own_pid = std::process::id() as i32;
            let own_apps: Vec<Retained<SCRunningApplication>> = content
                .applications()
                .iter()
                .filter(|app| {
                    is_recorder_application(
                        app.processID(),
                        &app.applicationName().to_string(),
                        own_pid,
                    )
                })
                .collect();

            (sc_display, own_apps)
        };

        // SAFETY: the filter is built from live objects, and the arrays outlive the call
        let filter = unsafe {
            let excluded = NSArray::from_retained_slice(&own_apps);
            let no_windows: Retained<NSArray<SCWindow>> = NSArray::new();
            SCContentFilter::initWithDisplay_excludingApplications_exceptingWindows(
                SCContentFilter::alloc(),
                &sc_display,
                &excluded,
                &no_windows,
            )
        };

        // SAFETY: plain property read on a live object
        let scale = if config.retina {
            f64::from(unsafe { SCShareableContent::infoForFilter(&filter).pointPixelScale() })
        } else {
            1.0
        };

        let width_px = even(display.width_points as f64 * scale);
        let height_px = even(display.height_points as f64 * scale);

        let stream_config = unsafe {
            let stream_config = SCStreamConfiguration::new();
            stream_config.setWidth(width_px);
            stream_config.setHeight(height_px);
            stream_config.setMinimumFrameInterval(CMTime::new(1, config.fps.max(1) as i32));
            stream_config.setPixelFormat(PIXEL_FORMAT_BGRA);
            stream_config.setShowsCursor(true);
            stream_config.setQueueDepth(6);
            stream_config
        };

        let file = video_file_name(display.id, segment);
        let frames_file = frames_file_name(display.id, segment);
        let writer = create_writer(&dir.join(&file), config.codec, width_px, height_px)?;
        let mut frames_csv = BufWriter::new(
            File::create(dir.join(&frames_file))
                .wrap_err_with(|| format!("could not create {frames_file}"))?,
        );
        writeln!(frames_csv, "index,pts_mach_ns,received_mach_ns")?;
        frames_csv.flush()?;

        let stats = Arc::new(CaptureStats::default());
        let shared = Arc::new(Shared {
            stats,
            state: Mutex::new(WriterState {
                writer: writer.0,
                input: writer.1,
                frames_csv,
                last_flush: Instant::now(),
                started: false,
                finished: false,
                frame_index: 0,
                latency: Latency::default(),
                last_sample: None,
                last_append_ns: 0,
            }),
        });

        let output = StreamOutput::new(shared.clone());
        let queue = DispatchQueue::new("komorebi-recorder.frames", DispatchQueueAttr::SERIAL);

        // SAFETY: all arguments are live objects; the output and queue are retained by `Self`
        let stream = unsafe {
            let stream = SCStream::initWithFilter_configuration_delegate(
                SCStream::alloc(),
                &filter,
                &stream_config,
                Some(ProtocolObject::from_ref(&*output)),
            );

            stream
                .addStreamOutput_type_sampleHandlerQueue_error(
                    ProtocolObject::from_ref(&*output),
                    SCStreamOutputType::Screen,
                    Some(&queue),
                )
                .map_err(|error| {
                    eyre::Report::msg(format!(
                        "could not attach the frame output: {}",
                        error.localizedDescription()
                    ))
                })?;

            stream
        };

        // SAFETY: the stream is fully configured
        let started = wait_for_completion(
            "starting the screen capture",
            CALLBACK_TIMEOUT,
            |block| unsafe {
                stream.startCaptureWithCompletionHandler(Some(block));
            },
        );

        if let Err(error) = started {
            // nothing was captured, so leave no empty segment files behind
            let mut state = shared.state.lock();
            state.finished = true;
            // SAFETY: the writer never started
            unsafe { state.writer.cancelWriting() };
            drop(state);
            let _ = std::fs::remove_file(dir.join(&file));
            let _ = std::fs::remove_file(dir.join(&frames_file));
            return Err(error);
        }

        Ok(Self {
            descriptor: display.clone(),
            segment,
            started_mach_ns: clock::mono_now_ns(),
            scale,
            width_px,
            height_px,
            codec: config.codec,
            file,
            frames_file,
            stream,
            _output: output,
            queue,
            shared,
        })
    }

    fn result(
        &self,
        reason: Option<EndReason>,
        end_mach_ns: Option<u64>,
        error: Option<String>,
    ) -> DisplayResult {
        let state = self.shared.state.lock();
        let stats = &self.shared.stats;

        DisplayResult {
            display: self.descriptor.clone(),
            segment: self.segment,
            scale: self.scale,
            width_px: self.width_px,
            height_px: self.height_px,
            codec: self.codec,
            file: self.file.clone(),
            frames_file: self.frames_file.clone(),
            video_t0_mach_ns: stats.first_pts_mach_ns(),
            video_end_mach_ns: end_mach_ns,
            end_reason: reason,
            frames: state.frame_index,
            dropped: stats.dropped.load(Ordering::Relaxed),
            idle_callbacks: stats.idle_callbacks.load(Ordering::Relaxed),
            latency: state.latency.finish(),
            error,
        }
    }
}

impl RunningCapture for DisplayCapture {
    fn display_id(&self) -> u32 {
        self.descriptor.id
    }

    fn segment(&self) -> u32 {
        self.segment
    }

    fn stats(&self) -> Arc<CaptureStats> {
        self.shared.stats.clone()
    }

    fn partial(&self) -> DisplayResult {
        self.result(None, None, self.shared.stats.error())
    }

    fn last_signal_mach_ns(&self) -> Option<u64> {
        Some(
            self.shared
                .stats
                .last_callback_mach_ns()
                .unwrap_or(self.started_mach_ns),
        )
    }

    fn keepalive(&self, now_ns: u64) {
        // Only keep a display alive while its stream is demonstrably alive. Re-submitting the last
        // frame of a stream that has silently died would produce a frozen video that looks
        // healthy while the screen moves on.
        let alive = self
            .shared
            .stats
            .last_callback_mach_ns()
            .is_some_and(|last| now_ns.saturating_sub(last) < KEEPALIVE_STREAM_TIMEOUT_NS);
        if !alive {
            return;
        }

        let mut state = self.shared.state.lock();
        if !state.started || state.finished {
            return;
        }

        let idle_ns = now_ns.saturating_sub(state.last_append_ns);
        if idle_ns < u64::try_from(KEEPALIVE_AFTER.as_nanos()).unwrap_or(u64::MAX) {
            return;
        }

        let Some(last) = state.last_sample.clone() else {
            return;
        };

        let pts_ns = now_ns.max(state.last_append_ns + 1_000_000);
        let Some(copy) = retimed(&last, pts_ns) else {
            return;
        };

        // SAFETY: the input belongs to a writer that is currently writing
        if !unsafe { state.input.isReadyForMoreMediaData() } {
            return;
        }

        // SAFETY: as above
        if !unsafe { state.input.appendSampleBuffer(&copy) } {
            self.shared.stats.set_error(
                format!(
                    "could not append keep-alive frame: {}",
                    writer_error(&state)
                ),
                now_ns,
            );
            state.finished = true;
            return;
        }

        state.last_append_ns = pts_ns;
        // not a new frame, and marked so: it keeps the log's idea of the segment's end honest
        let _ = writeln!(state.frames_csv, "-,{pts_ns},{now_ns}");
    }

    fn stop(self: Box<Self>, reason: EndReason, end_mach_ns: Option<u64>) -> DisplayResult {
        let mut error = None;
        let already_dead = self.shared.stats.error().is_some();

        // A stream that ScreenCaptureKit has already killed has nothing left to stop, and asking
        // would only make us wait out the timeout.
        if !already_dead
            && let Err(stop_error) = wait_for_completion(
                "stopping the screen capture",
                STOP_TIMEOUT,
                // SAFETY: the stream was started by `start`
                |block| unsafe {
                    self.stream.stopCaptureWithCompletionHandler(Some(block));
                },
            )
        {
            error = Some(stop_error.to_string());
        }

        // let any in-flight frame callback finish before the writer is finalized
        self.queue.exec_sync(|| {});

        let end_ns = {
            let mut state = self.shared.state.lock();
            state.finished = true;
            let _ = state.frames_csv.flush();

            // Frames only exist when the screen changes, so without an explicit end the file
            // would stop at the last change and late timecodes would fall off the end. A segment
            // that ended early (a dead stream) ends when it died, never before its last frame.
            let now = clock::mono_now_ns();
            let end_ns = end_mach_ns
                .unwrap_or(now)
                .min(now)
                .max(state.last_append_ns);

            if state.started {
                // SAFETY: the writer is currently writing
                let finished = unsafe {
                    let ticks = clock::timebase().ns_to_ticks(end_ns);
                    state
                        .writer
                        .endSessionAtSourceTime(CMClock::make_host_time_from_system_units(ticks));
                    state.input.markAsFinished();
                    let (tx, rx) = mpsc::channel();
                    let block = RcBlock::new(move || {
                        let _ = tx.send(());
                    });
                    state.writer.finishWritingWithCompletionHandler(&block);
                    rx.recv_timeout(CALLBACK_TIMEOUT).is_ok()
                };

                if !finished {
                    error.get_or_insert_with(|| "timed out finalizing the video".to_string());
                } else if let Some(message) = unsafe { state.writer.error() } {
                    error.get_or_insert_with(|| message.localizedDescription().to_string());
                }
            } else {
                // SAFETY: the writer never started, so there is nothing to finalize
                unsafe { state.writer.cancelWriting() };
                error.get_or_insert_with(|| "no frames were captured".to_string());
            }

            end_ns
        };

        let error = error.or_else(|| self.shared.stats.error());
        self.result(Some(reason), Some(end_ns), error)
    }
}

fn create_writer(
    path: &Path,
    codec: Codec,
    width: usize,
    height: usize,
) -> eyre::Result<(Retained<AVAssetWriter>, Retained<AVAssetWriterInput>)> {
    let path = path
        .to_str()
        .ok_or_else(|| eyre::Report::msg("session path is not valid UTF-8"))?;

    // SAFETY: all statics are provided by AVFoundation and all arguments are live objects
    unsafe {
        let url = NSURL::fileURLWithPath(&NSString::from_str(path));
        let file_type =
            AVFileTypeMPEG4.ok_or_else(|| eyre::Report::msg("AVFileTypeMPEG4 is unavailable"))?;
        let writer =
            AVAssetWriter::assetWriterWithURL_fileType_error(&url, file_type).map_err(|error| {
                eyre::Report::msg(format!(
                    "could not create the video writer: {}",
                    error.localizedDescription()
                ))
            })?;

        let codec_name: &NSString = match codec {
            Codec::Hevc => AVVideoCodecTypeHEVC,
            Codec::H264 => AVVideoCodecTypeH264,
        }
        .ok_or_else(|| eyre::Report::msg("the requested video codec is unavailable"))?;

        let width = NSNumber::new_usize(width);
        let height = NSNumber::new_usize(height);
        let keys = [
            AVVideoCodecKey.ok_or_else(|| eyre::Report::msg("AVVideoCodecKey is unavailable"))?,
            AVVideoWidthKey.ok_or_else(|| eyre::Report::msg("AVVideoWidthKey is unavailable"))?,
            AVVideoHeightKey.ok_or_else(|| eyre::Report::msg("AVVideoHeightKey is unavailable"))?,
        ];
        let values: [&AnyObject; 3] = [codec_name.as_ref(), width.as_ref(), height.as_ref()];
        let settings: Retained<NSDictionary<NSString, AnyObject>> =
            NSDictionary::from_slices(&keys, &values);

        let media_type =
            AVMediaTypeVideo.ok_or_else(|| eyre::Report::msg("AVMediaTypeVideo is unavailable"))?;
        let input = AVAssetWriterInput::assetWriterInputWithMediaType_outputSettings(
            media_type,
            Some(&settings),
        );
        input.setExpectsMediaDataInRealTime(true);

        // Write the file as movie fragments so that if the recorder is killed or crashes, a
        // display is yanked mid-write, or the machine loses power, everything up to the last
        // fragment is still a playable video instead of an unfinalized file with no index.
        writer.setMovieFragmentInterval(CMTime::new(FRAGMENT_SECONDS, 1));

        if !writer.canAddInput(&input) {
            eyre::bail!("the video writer rejected the input configuration ({codec})");
        }

        writer.addInput(&input);
        Ok((writer, input))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_recorder_process_is_left_out_of_the_capture_and_nothing_else() {
        // this process, by pid
        assert!(is_recorder_application(42, "whatever", 42));
        // another recorder (a viewer opened beside a recording), by name
        assert!(is_recorder_application(7, "komorebi-recorder", 42));
        assert!(is_recorder_application(7, "Komorebi-Recorder", 42));
        // other applications, including komorebi itself and similarly named ones
        assert!(!is_recorder_application(7, "komorebi", 42));
        assert!(!is_recorder_application(7, "komorebi-recorder-helper", 42));
        assert!(!is_recorder_application(7, "Ghostty", 42));
    }
}
