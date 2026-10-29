//! Recording one segment on Linux: frames from a source, encoded to fragmented MP4 by GStreamer.
//!
//! This is the half of a Linux capture that does not depend on where the frames come from. A
//! source (on KDE, the compositor's screencast, see `platform::linux`) hands each frame to a
//! [`Sink`] with the time the compositor made it on the master clock, and this module does what
//! the Windows backend does with a frame: place it in the file's time ([`Placement`]), write it,
//! log when it arrived ([`FrameLog`]) and measure how far its time lags its arrival ([`Latency`]).
//! A display that stops changing has its last frame repeated to keep the file moving, and the
//! segment's file is fragmented, so what was written before a crash can still be played.
//!
//! The encoder is `x264enc` and the container `mp4mux`, so only H.264 is recorded. The pipeline
//! scales a frame of an odd size, or one larger than H.264 allows, to one it can take, so the
//! video can be a pixel or more away from the display's size ([`DisplayResult::width_px`]).
//!
//! A source is started by [`DisplayCapture::start_with`], which is given a function that connects
//! it to the [`Sink`] and returns what keeps it alive: dropping that stops the source. The
//! tests of this module are a source of their own, which makes the frames up.

use super::CaptureConfig;
use super::CaptureStats;
use super::Codec;
use super::DisplayDescriptor;
use super::DisplayResult;
use super::EndReason;
use super::RunningCapture;
use super::frames_file_name;
use super::segment::FrameLog;
use super::segment::Latency;
use super::segment::Placement;
use super::segment::even;
use super::segment::h264_bitrate;
use super::segment::h264_size;
use super::segment::keepalive_due;
use super::segment::nominal_fps;
use super::video_file_name;
use crate::clock;
use color_eyre::eyre;
use color_eyre::eyre::WrapErr;
use gstreamer as gst;
use gstreamer::prelude::*;
use gstreamer_app as gst_app;
use parking_lot::Mutex;
use std::path::Path;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::Ordering;

/// One frame from a source: tightly packed BGRA (or BGRx) pixels, and when the compositor made it.
pub struct Frame {
    /// The pixels, `width * height * 4` bytes, top row first.
    pub buffer: gst::Buffer,
    /// The width of the picture in pixels.
    pub width: u32,
    /// The height of the picture in pixels.
    pub height: u32,
    /// When the frame was made, on the master clock in nanoseconds.
    pub pts_ns: u64,
}

/// What a source reports to: every frame it has, and the reason it can have no more.
///
/// Both may be called from any thread.
#[derive(Clone)]
pub struct Sink {
    frame: Arc<dyn Fn(Frame) + Send + Sync>,
    fail: Arc<dyn Fn(String) + Send + Sync>,
}

impl Sink {
    /// Hands a frame to the segment.
    pub fn frame(&self, frame: Frame) {
        (self.frame)(frame);
    }

    /// Says that the source has died, and why: the segment records the first reason it is told.
    pub fn fail(&self, reason: String) {
        (self.fail)(reason);
    }
}

/// How long to wait for the encoder to finish the file once it has all the frames.
const FINISH_WAIT: gst::ClockTime = gst::ClockTime::from_seconds(20);

/// How long the encoder's queue may be before a source is held up by it, in frames.
const QUEUED_FRAMES: u64 = 8;

/// The H.264 file of one segment, and the pipeline that writes it.
struct Writer {
    pipeline: gst::Pipeline,
    source: gst_app::AppSrc,
    /// How long a frame is shown if no other follows: what the last one is given.
    frame_ns: u64,
}

impl Writer {
    /// Starts a pipeline that writes frames of `source` pixels to `path` as `size` pixels.
    fn create(path: &Path, source: (u32, u32), size: (u32, u32), fps: u32) -> eyre::Result<Self> {
        crate::av::init()?;
        let nominal = nominal_fps(fps);
        let kilobits = (h264_bitrate(size.0 as usize, size.1 as usize, nominal) / 1000).max(1);
        // no B-frames, so that a frame's time in the file is its decoding time as well; a key frame
        // about every two seconds of the frame rate declared
        let description = format!(
            "appsrc name=src format=time is-live=false block=true max-buffers={QUEUED_FRAMES} \
             caps=video/x-raw,format=BGRA,width={},height={},framerate={nominal}/1 ! \
             videoconvert ! videoscale ! video/x-raw,width={},height={} ! videoconvert ! \
             video/x-raw,format=I420 ! \
             x264enc bframes=0 tune=zerolatency speed-preset=veryfast key-int-max={} bitrate={kilobits} ! \
             h264parse ! mp4mux fragment-duration=1000 ! filesink name=file",
            source.0,
            source.1,
            size.0,
            size.1,
            nominal * 2,
        );
        let pipeline = gst::parse::launch(&description)
            .wrap_err("could not build the GStreamer pipeline that writes the video")?
            .downcast::<gst::Pipeline>()
            .map_err(|_| eyre::Report::msg("the GStreamer pipeline is not a pipeline"))?;
        pipeline
            .by_name("file")
            .ok_or_else(|| eyre::Report::msg("the pipeline has no file sink"))?
            .set_property("location", path.to_string_lossy().as_ref());
        let source = pipeline
            .by_name("src")
            .ok_or_else(|| eyre::Report::msg("the pipeline has no source"))?
            .downcast::<gst_app::AppSrc>()
            .map_err(|_| eyre::Report::msg("the pipeline's source is not an appsrc"))?;
        pipeline
            .set_state(gst::State::Playing)
            .wrap_err("GStreamer could not start writing the video (is the x264 plugin there?)")?;
        Ok(Self {
            pipeline,
            source,
            frame_ns: 1_000_000_000 / u64::from(nominal),
        })
    }

    /// Writes `buffer` to be shown `file_ns` nanoseconds after the file's first frame.
    fn write(&mut self, buffer: &gst::Buffer, file_ns: u64) -> eyre::Result<()> {
        let mut shown = buffer.copy();
        {
            let shown = shown
                .get_mut()
                .ok_or_else(|| eyre::Report::msg("a frame is shared and cannot be timed"))?;
            let time = gst::ClockTime::from_nseconds(file_ns);
            shown.set_pts(time);
            shown.set_dts(time);
            shown.set_duration(gst::ClockTime::from_nseconds(self.frame_ns));
        }
        self.source
            .push_buffer(shown)
            .map(|_| ())
            .map_err(|error| eyre::Report::msg(format!("the encoder refused a frame: {error}")))
    }

    /// Ends the stream and waits until the file is finished.
    fn finish(&mut self) -> eyre::Result<()> {
        let sent = self.source.end_of_stream();
        let waited = self.pipeline.bus().and_then(|bus| {
            bus.timed_pop_filtered(
                FINISH_WAIT,
                &[gst::MessageType::Eos, gst::MessageType::Error],
            )
        });
        let _ = self.pipeline.set_state(gst::State::Null);
        sent.map_err(|error| eyre::Report::msg(format!("could not end the video: {error}")))?;
        match waited.as_deref().map(gst::MessageRef::view) {
            Some(gst::MessageView::Eos(_)) => Ok(()),
            Some(gst::MessageView::Error(error)) => Err(eyre::Report::msg(format!(
                "the encoder failed: {}",
                error.error()
            ))),
            _ => Err(eyre::Report::msg(
                "the encoder did not finish the video in time",
            )),
        }
    }
}

struct State {
    /// Made when the first frame shows how large the display is.
    writer: Option<Writer>,
    /// The size of the frames, and of the video written from them, in pixels.
    source: (u32, u32),
    size: (u32, u32),
    placement: Placement,
    log: FrameLog,
    latency: Latency,
    /// The most recent frame, kept so that a still display can have it repeated.
    last_frame: Option<gst::Buffer>,
    started: bool,
    finished: bool,
}

struct Shared {
    stats: Arc<CaptureStats>,
    state: Mutex<State>,
    path: PathBuf,
    fps: u32,
}

/// What one frame from the source does: create the writer on the first, place it in time, write it,
/// log it.
fn on_frame(shared: &Shared, frame: Frame) {
    let received_ns = clock::mono_now_ns();
    shared.stats.callbacks.fetch_add(1, Ordering::Relaxed);
    shared
        .stats
        .last_callback_mach_ns
        .store(received_ns, Ordering::Relaxed);

    if let Err(error) = frame_into_file(shared, &frame, received_ns) {
        shared.stats.set_error(format!("{error:#}"), received_ns);
        shared.state.lock().finished = true;
    }
}

fn frame_into_file(shared: &Shared, frame: &Frame, received_ns: u64) -> eyre::Result<()> {
    let mut state = shared.state.lock();
    if state.finished {
        return Ok(());
    }

    let size = (frame.width, frame.height);
    if state.writer.is_none() {
        let wanted = h264_size(even(f64::from(size.0)), even(f64::from(size.1)));
        let wanted = (u32::try_from(wanted.0)?, u32::try_from(wanted.1)?);
        state.writer = Some(Writer::create(&shared.path, size, wanted, shared.fps)?);
        state.source = size;
        state.size = wanted;
    } else if size != state.source {
        // the display changed size under us: the manager sees that too, and starts a new segment
        eyre::bail!(
            "the display changed size to {}x{} while recording {}x{}",
            size.0,
            size.1,
            state.source.0,
            state.source.1
        );
    }

    if !state.started {
        state.started = true;
        shared
            .stats
            .first_pts_mach_ns
            .store(frame.pts_ns, Ordering::Relaxed);
    }

    let placed = state.placement.frame(frame.pts_ns);
    let file_ns = u64::try_from(placed.file_time).unwrap_or(0) * 100;
    state
        .writer
        .as_mut()
        .ok_or_else(|| eyre::Report::msg("the video has no writer"))?
        .write(&frame.buffer, file_ns)?;
    state.last_frame = Some(frame.buffer.clone());
    state
        .latency
        .record(received_ns as i64 - placed.pts_ns as i64);
    state.log.frame(placed.pts_ns, received_ns);

    shared.stats.frames.fetch_add(1, Ordering::Relaxed);
    shared
        .stats
        .last_pts_mach_ns
        .store(placed.pts_ns, Ordering::Relaxed);
    Ok(())
}

/// One display's segment: a source, and the video it is being written to.
pub struct DisplayCapture {
    descriptor: DisplayDescriptor,
    segment: u32,
    file: String,
    frames_file: String,
    path: PathBuf,
    /// Kept for as long as the capture runs; dropping it stops the source.
    stream: Mutex<Option<Box<dyn Send>>>,
    shared: Arc<Shared>,
}

impl DisplayCapture {
    /// Starts a segment of `display` in `dir`, with a source that `connect` starts.
    ///
    /// `connect` is given the [`Sink`] to call with every frame, and returns what keeps the
    /// source running; the capture drops it when it stops. Returns once `connect` has, without
    /// waiting for a frame: `video_t0_mach_ns` is `None` until one arrives. The files are
    /// `video-<display>[-<n>].mp4` and `frames-<display>[-<n>].csv` in `dir`.
    ///
    /// # Errors
    ///
    /// Returns an error when:
    ///
    /// - `config.codec` is [`Codec::Hevc`]: only H.264 is recorded on Linux;
    /// - GStreamer cannot start;
    /// - the frame log cannot be created in `dir`;
    /// - `connect` fails.
    ///
    /// A failure to write the video itself (the encoder is missing, the file cannot be created) is
    /// found when the first frame arrives, and is the capture's error then.
    pub fn start_with(
        display: &DisplayDescriptor,
        segment: u32,
        config: &CaptureConfig,
        dir: &Path,
        connect: impl FnOnce(Sink) -> eyre::Result<Box<dyn Send>>,
    ) -> eyre::Result<Self> {
        if config.codec == Codec::Hevc {
            eyre::bail!(
                "HEVC cannot be recorded on Linux yet (the video is written with x264); use \
                 --codec h264"
            );
        }
        crate::av::init()?;

        let file = video_file_name(display.id, segment);
        let frames_file = frames_file_name(display.id, segment);
        let path = dir.join(&file);
        let log = FrameLog::create(&dir.join(&frames_file))
            .wrap_err_with(|| format!("could not create {frames_file}"))?;

        let shared = Arc::new(Shared {
            stats: Arc::new(CaptureStats::default()),
            state: Mutex::new(State {
                writer: None,
                source: (0, 0),
                size: (0, 0),
                placement: Placement::default(),
                log,
                latency: Latency::default(),
                last_frame: None,
                started: false,
                finished: false,
            }),
            path: path.clone(),
            fps: config.fps,
        });

        let frames = shared.clone();
        let failures = shared.clone();
        let stream = connect(Sink {
            frame: Arc::new(move |frame| on_frame(&frames, frame)),
            fail: Arc::new(move |reason| {
                failures.stats.set_error(reason, clock::mono_now_ns());
            }),
        })?;

        Ok(Self {
            descriptor: display.clone(),
            segment,
            file,
            frames_file,
            path,
            stream: Mutex::new(Some(stream)),
            shared,
        })
    }

    fn result(
        &self,
        reason: Option<EndReason>,
        end_ns: Option<u64>,
        error: Option<String>,
    ) -> DisplayResult {
        let state = self.shared.state.lock();
        let stats = &self.shared.stats;
        // before the first frame the video has no size of its own: the display's, in pixels
        let (width, height) = if state.size == (0, 0) {
            let scale = self.descriptor.scale.max(1.0);
            (
                even(self.descriptor.frame[2] * scale),
                even(self.descriptor.frame[3] * scale),
            )
        } else {
            (state.size.0 as usize, state.size.1 as usize)
        };
        DisplayResult {
            display: self.descriptor.clone(),
            segment: self.segment,
            // the video is the display's pixels, whatever its scale
            scale: 1.0,
            width_px: width,
            height_px: height,
            codec: Codec::H264,
            file: self.file.clone(),
            frames_file: self.frames_file.clone(),
            video_t0_mach_ns: stats.first_pts_mach_ns(),
            video_end_mach_ns: end_ns,
            end_reason: reason,
            frames: state.log.frames(),
            dropped: stats.dropped.load(Ordering::Relaxed),
            idle_callbacks: 0,
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

    /// A compositor sends nothing while the picture does not change, so silence is not a sign of
    /// death here, as on Windows.
    fn last_signal_mach_ns(&self) -> Option<u64> {
        None
    }

    fn keepalive(&self, now_ns: u64) {
        let mut state = self.shared.state.lock();
        if !state.started || state.finished {
            return;
        }
        if !keepalive_due(now_ns, state.placement.last_ns(), None) {
            return;
        }

        let Some(last) = state.last_frame.clone() else {
            return;
        };
        let Some(placed) = state.placement.repeat(now_ns) else {
            return;
        };
        let file_ns = u64::try_from(placed.file_time).unwrap_or(0) * 100;
        let wrote = match state.writer.as_mut() {
            Some(writer) => writer.write(&last, file_ns),
            None => return,
        };
        if let Err(error) = wrote {
            self.shared.stats.set_error(format!("{error:#}"), now_ns);
            state.finished = true;
            return;
        }

        // not a new frame, and marked so
        state.log.repeat(placed.pts_ns, now_ns);
    }

    fn stop(self: Box<Self>, reason: EndReason, end_mach_ns: Option<u64>) -> DisplayResult {
        // no more frames: stops the source, then waits out a frame in flight by taking the lock
        drop(self.stream.lock().take());

        let mut error = None;
        let end_ns = {
            let mut state = self.shared.state.lock();
            state.finished = true;
            state.log.flush();

            // A still display produced no frames, so without an explicit end the file would stop
            // at the last change and late timecodes would fall off it. A segment that ended early
            // (a dead capture) ends when it died, never before its last frame.
            let now = clock::mono_now_ns();
            let end_ns = end_mach_ns
                .unwrap_or(now)
                .min(now)
                .max(state.placement.last_ns());

            if state.started {
                if let Some(last) = state.last_frame.clone()
                    && end_ns > state.placement.last_ns()
                    && let Some(placed) = state.placement.repeat(end_ns)
                {
                    let file_ns = u64::try_from(placed.file_time).unwrap_or(0) * 100;
                    let wrote = match state.writer.as_mut() {
                        Some(writer) => writer.write(&last, file_ns),
                        None => Ok(()),
                    };
                    match wrote {
                        Ok(()) => state.log.repeat(placed.pts_ns, now),
                        Err(write_error) => error = Some(format!("{write_error:#}")),
                    }
                    state.log.flush();
                }

                if let Some(writer) = state.writer.as_mut()
                    && let Err(finish_error) = writer.finish()
                {
                    error.get_or_insert_with(|| format!("{finish_error:#}"));
                }
            } else {
                // never began: there is no video, and the empty file is no use to anyone
                drop(state);
                let _ = std::fs::remove_file(&self.path);
                return self.result(
                    Some(reason),
                    Some(end_ns),
                    Some("no frames were captured".to_string()),
                );
            }

            end_ns
        };

        let error = error.or_else(|| self.shared.stats.error());
        self.result(Some(reason), Some(end_ns), error)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::av::Asset;
    use crate::av::Scale;

    fn display() -> DisplayDescriptor {
        DisplayDescriptor {
            id: 7,
            frame: [0.0, 0.0, 128.0, 64.0],
            width_points: 128,
            height_points: 64,
            asleep: false,
            scale: 1.0,
        }
    }

    /// A frame of `width` x `height` whose every pixel has the blue of `shade`.
    fn frame(width: u32, height: u32, shade: u8, pts_ns: u64) -> Frame {
        let mut pixels = vec![0_u8; (width * height * 4) as usize];
        for pixel in pixels.chunks_mut(4) {
            pixel.copy_from_slice(&[shade, 40, 40, 255]);
        }
        Frame {
            buffer: gst::Buffer::from_slice(pixels),
            width,
            height,
            pts_ns,
        }
    }

    /// Records `count` frames, 33 ms apart and stamped as they are sent, of a made-up source of `width` x `height`.
    fn record(dir: &Path, width: u32, height: u32, count: u32) -> DisplayResult {
        let slot: Arc<Mutex<Option<Sink>>> = Arc::new(Mutex::new(None));
        let held = slot.clone();
        let capture = DisplayCapture::start_with(
            &display(),
            1,
            &CaptureConfig {
                codec: Codec::H264,
                ..CaptureConfig::default()
            },
            dir,
            move |sink| {
                *held.lock() = Some(sink);
                Ok(Box::new(()))
            },
        )
        .expect("start");

        let sink = slot.lock().clone().expect("a source");
        for index in 0..count {
            std::thread::sleep(std::time::Duration::from_millis(33));
            sink.frame(frame(
                width,
                height,
                (index * 8) as u8,
                clock::mono_now_ns(),
            ));
        }
        Box::new(capture).stop(EndReason::Stopped, None)
    }

    fn scratch(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "komorebi-recorder-gst-capture-{name}-{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("a temporary directory");
        dir
    }

    #[test]
    fn frames_are_written_to_a_video_that_decodes_and_a_log_that_counts_them() {
        let dir = scratch("roundtrip");
        let result = record(&dir, 128, 64, 12);

        assert_eq!(result.error, None);
        assert_eq!(result.frames, 12);
        assert_eq!((result.width_px, result.height_px), (128, 64));
        assert_eq!(result.codec, Codec::H264);
        assert!(result.video_t0_mach_ns.is_some());
        assert!(result.latency.looks_like_host_clock());

        let log = std::fs::read_to_string(dir.join(&result.frames_file)).expect("frame log");
        assert!(log.lines().count() >= 12, "{log}");

        let asset = Asset::open(&dir.join(&result.file)).expect("the video opens");
        assert_eq!(asset.size(), (128, 64));
        let mut reader = asset.start(0.0, None, Scale::Native).expect("start");
        let mut frames = 0;
        let mut first = None;
        while let Some(decoded) = reader.next_frame().expect("a frame") {
            first.get_or_insert(decoded.bgra[..4].to_vec());
            frames += 1;
        }
        assert!(frames >= 12, "{frames} frames decoded");
        // the first frame was made blue 0 (and green and red 40): lossy, so near
        let first = first.expect("a first frame");
        assert!(first[0] < 40 && first[1].abs_diff(40) < 20, "{first:?}");
    }

    #[test]
    fn a_display_of_an_odd_size_is_recorded_a_pixel_smaller() {
        let dir = scratch("odd");
        let result = record(&dir, 129, 65, 6);
        assert_eq!(result.error, None);
        assert_eq!((result.width_px, result.height_px), (128, 64));
        let asset = Asset::open(&dir.join(&result.file)).expect("the video opens");
        assert_eq!(asset.size(), (128, 64));
    }

    #[test]
    fn a_segment_that_never_had_a_frame_says_so_and_leaves_no_file() {
        let dir = scratch("none");
        let capture = DisplayCapture::start_with(
            &display(),
            1,
            &CaptureConfig {
                codec: Codec::H264,
                ..CaptureConfig::default()
            },
            &dir,
            |_| Ok(Box::new(())),
        )
        .expect("start");
        let result = Box::new(capture).stop(EndReason::Stopped, None);
        assert_eq!(result.error.as_deref(), Some("no frames were captured"));
        assert_eq!(result.video_t0_mach_ns, None);
        assert!(!dir.join(&result.file).exists());
    }

    #[test]
    fn hevc_is_refused() {
        let dir = scratch("hevc");
        let error = DisplayCapture::start_with(
            &display(),
            1,
            &CaptureConfig {
                codec: Codec::Hevc,
                ..CaptureConfig::default()
            },
            &dir,
            |_| Ok(Box::new(())),
        )
        .err()
        .expect("an error");
        assert!(error.to_string().contains("--codec h264"), "{error}");
    }
}
