//! Decoding recorded video on Linux with GStreamer.
//!
//! GStreamer is the OS media stack on Linux, as AVFoundation is on a Mac and Media Foundation on
//! Windows, so this decodes in process and does not run `ffmpeg`. Each decode is a pipeline,
//! `filesrc ! decodebin ! videoconvert ! videoscale ! <caps> ! appsink`: the picture arrives as BGRA
//! at the size wanted (the pipeline converts and scales, so a contact sheet never holds more pixels
//! than it shows). Starting a decode at a time seeks to the key frame before it and decodes forward,
//! and [`super::Selector`] says which of the decoded frames are the answer, as it does for Media
//! Foundation.
//!
//! An [`Asset`] keeps the path and what was read when it was opened (size, duration), not a
//! pipeline: every [`Asset::start`] builds a new one, so a file that changes after it was opened is
//! read as it is then.
//!
//! The pipeline needs a plugin that decodes the file's codec. The Nix package puts H.264 and HEVC
//! decoders (`gst-libav`) on the plugin path; a file whose codec has none says so in the error
//! from [`Asset::open`].
//!
//! # Tests
//!
//! The rule for which decoded frames answer a request is `Selector`, whose tests run on every
//! host. The tests here make a video with `ffmpeg` and read it back, and stop without one.

use super::Frame;
use super::Scale;
use super::Selector;
use color_eyre::eyre;
use color_eyre::eyre::WrapErr;
use gstreamer as gst;
use gstreamer::prelude::*;
use gstreamer_app as gst_app;
use gstreamer_video as gst_video;
use std::collections::VecDeque;
use std::path::Path;
use std::path::PathBuf;
use std::sync::OnceLock;

/// How long to wait for the pipeline to start (the first picture) or for one more frame.
const WAIT: gst::ClockTime = gst::ClockTime::from_seconds(15);

pub(crate) fn init() -> eyre::Result<()> {
    static INIT: OnceLock<Result<(), String>> = OnceLock::new();
    INIT.get_or_init(|| gst::init().map_err(|error| error.to_string()))
        .as_ref()
        .map(|_| ())
        .map_err(|why| eyre::Report::msg(format!("GStreamer could not start: {why}")))
}

/// The pipeline that decodes `path` to BGRA pictures of `size` (the video's own if `None`).
fn pipeline(
    path: &Path,
    size: Option<(u32, u32)>,
) -> eyre::Result<(gst::Pipeline, gst_app::AppSink)> {
    init()?;
    if !path.is_file() {
        eyre::bail!("{} does not exist", path.display());
    }

    let caps = match size {
        Some((width, height)) => format!("video/x-raw,format=BGRA,width={width},height={height}"),
        None => "video/x-raw,format=BGRA".to_string(),
    };
    // a bounded queue, so that a decode that is read slowly does not run ahead into memory, and
    // no clock, so that it decodes as fast as it is read
    let description = format!(
        "filesrc name=src ! decodebin ! videoconvert ! videoscale ! {caps} ! \
         appsink name=sink sync=false max-buffers=4 drop=false"
    );
    let pipeline = gst::parse::launch(&description)
        .wrap_err("could not build the GStreamer pipeline")?
        .downcast::<gst::Pipeline>()
        .map_err(|_| eyre::Report::msg("the GStreamer pipeline is not a pipeline"))?;
    pipeline
        .by_name("src")
        .ok_or_else(|| eyre::Report::msg("the pipeline has no file source"))?
        .set_property("location", path.to_string_lossy().as_ref());
    let sink = pipeline
        .by_name("sink")
        .ok_or_else(|| eyre::Report::msg("the pipeline has no sink"))?
        .downcast::<gst_app::AppSink>()
        .map_err(|_| eyre::Report::msg("the pipeline's sink is not an appsink"))?;
    Ok((pipeline, sink))
}

/// What GStreamer said went wrong, if it said, else `fallback`.
fn failure(pipeline: &gst::Pipeline, fallback: &str) -> eyre::Report {
    if let Some(bus) = pipeline.bus() {
        while let Some(message) = bus.pop() {
            if let gst::MessageView::Error(error) = message.view() {
                return eyre::Report::msg(format!(
                    "{fallback}: {}{}",
                    error.error(),
                    error
                        .debug()
                        .map(|debug| format!(" ({debug})"))
                        .unwrap_or_default()
                ));
            }
        }
    }
    eyre::Report::msg(fallback.to_string())
}

/// An opened video file: its path, size and duration.
///
/// Opening reads those facts and decodes one picture. Each [`Asset::start`] builds a new pipeline
/// on the file and starts where it is told to.
pub struct Asset {
    path: PathBuf,
    width: u32,
    height: u32,
    duration_s: f64,
}

impl Asset {
    /// Opens a video file.
    ///
    /// # Errors
    ///
    /// Returns an error when:
    ///
    /// - GStreamer cannot start;
    /// - `path` is not a file that exists (`<path> does not exist`);
    /// - GStreamer cannot decode the file as a video, for example because no plugin decodes its
    ///   codec (`could not open <path> as a video`, with GStreamer's reason).
    pub fn open(path: &Path) -> eyre::Result<Self> {
        let (pipeline, sink) = pipeline(path, None)?;
        let opened = || -> eyre::Result<Self> {
            pipeline
                .set_state(gst::State::Paused)
                .wrap_err("GStreamer could not start decoding")?;
            let sample = sink.try_pull_preroll(WAIT).ok_or_else(|| {
                failure(
                    &pipeline,
                    &format!("could not open {} as a video", path.display()),
                )
            })?;
            let info = sample
                .caps()
                .ok_or_else(|| eyre::Report::msg("the video has no picture format"))
                .and_then(|caps| {
                    gst_video::VideoInfo::from_caps(caps).wrap_err("the video has no picture size")
                })?;
            let duration_s = pipeline
                .query_duration::<gst::ClockTime>()
                .map_or(0.0, |duration| duration.nseconds() as f64 / 1e9);
            Ok(Self {
                path: path.to_path_buf(),
                width: info.width(),
                height: info.height(),
                duration_s,
            })
        };
        let asset = opened();
        let _ = pipeline.set_state(gst::State::Null);
        asset
    }

    /// Returns the size the video was recorded at, as `(width, height)` in pixels.
    #[must_use]
    pub fn size(&self) -> (u32, u32) {
        (self.width, self.height)
    }

    /// Returns the duration GStreamer reports for the file, in seconds, or `0.0` if it reports
    /// none (some truncated files).
    #[must_use]
    pub fn duration_s(&self) -> f64 {
        self.duration_s
    }

    /// Begins decoding at `from_s` seconds into the file, up to `to_s` if given (else the end).
    ///
    /// The first frame returned is the one on screen at `from_s`, which can be earlier than it. A
    /// negative `from_s` is `0`. The end is `to_s`, or the file's
    /// [`duration_s`](Asset::duration_s) if it is `None`, and is never before `from_s`: a file
    /// whose duration is unknown (`0.0`) therefore decodes little more than the frame on screen at
    /// `from_s` unless `to_s` is given. A frame at exactly the end is returned, and none after it.
    ///
    /// The pipeline gives BGRA frames of the size `scale` asks for (see [`Scale`]). When `from_s`
    /// is not `0` it seeks to the key frame at or before it, and the frames from there up to
    /// `from_s` are decoded and passed over: a seek costs the frames between a key frame and the
    /// time asked for.
    ///
    /// # Errors
    ///
    /// Returns an error when:
    ///
    /// - the file cannot be opened again (see [`Asset::open`]);
    /// - the seek to `from_s` fails (`could not seek in the video`).
    pub fn start(&self, from_s: f64, to_s: Option<f64>, scale: Scale) -> eyre::Result<Reader> {
        let from = from_s.max(0.0);
        let end = to_s.unwrap_or(self.duration_s).max(from);
        let size = match scale {
            Scale::Native => None,
            Scale::Width(wide) => {
                let wide = wide.clamp(2, self.width.max(2));
                let high = (u64::from(wide) * u64::from(self.height)
                    / u64::from(self.width.max(1)))
                .max(2);
                // codecs like even sizes
                Some((wide & !1, (high as u32) & !1))
            }
        };

        let (pipeline, sink) = pipeline(&self.path, size)?;
        let started = || -> eyre::Result<()> {
            pipeline
                .set_state(gst::State::Paused)
                .wrap_err("GStreamer could not start decoding")?;
            // wait until the first picture is ready: only then can the pipeline seek
            if sink.try_pull_preroll(WAIT).is_none() {
                return Err(failure(&pipeline, "could not start decoding the video"));
            }
            if from > 0.0 {
                // to the key frame at or before it; the frames up to `from` are decoded and
                // passed over by the selector
                pipeline
                    .seek_simple(
                        gst::SeekFlags::FLUSH
                            | gst::SeekFlags::KEY_UNIT
                            | gst::SeekFlags::SNAP_BEFORE,
                        gst::ClockTime::from_nseconds((from * 1e9) as u64),
                    )
                    .wrap_err("could not seek in the video")?;
            }
            pipeline
                .set_state(gst::State::Playing)
                .wrap_err("GStreamer could not start decoding")?;
            Ok(())
        };
        if let Err(error) = started() {
            let _ = pipeline.set_state(gst::State::Null);
            return Err(error);
        }

        Ok(Reader {
            pipeline,
            sink,
            selector: Selector::new(from, end),
            ready: VecDeque::new(),
            ended: false,
        })
    }
}

/// A started decode. Dropping it cancels the decode.
///
/// Made only by [`Asset::start`]. It moves forward only: to look at another time, start another
/// decode. The frames it returns are the ones a `Selector` picks from what the pipeline decodes,
/// so they begin with the frame on screen at the start of the range.
pub struct Reader {
    pipeline: gst::Pipeline,
    sink: gst_app::AppSink,
    selector: Selector,
    ready: VecDeque<Frame>,
    ended: bool,
}

impl Drop for Reader {
    fn drop(&mut self) {
        let _ = self.pipeline.set_state(gst::State::Null);
    }
}

/// When a decoded frame is shown, in the file's own time: the time zero is its first frame.
///
/// The buffer's timestamp is in the time of the media, which an MP4's edit list (which `ffmpeg` and
/// the recorder write to hide the frames an encoder delays) puts later than the time the frame is
/// shown at. The stream time, which the sample's segment gives, is the second.
fn shown_at(sample: &gst::Sample, buffer: &gst::BufferRef) -> Option<gst::ClockTime> {
    let pts = buffer.pts()?;
    sample
        .segment()
        .and_then(|segment| segment.downcast_ref::<gst::format::Time>())
        .and_then(|segment| segment.to_stream_time(pts))
        .or(Some(pts))
}

/// One decoded sample as a tightly packed BGRA frame.
fn frame_of(sample: &gst::Sample) -> eyre::Result<Frame> {
    let caps = sample
        .caps()
        .ok_or_else(|| eyre::Report::msg("a decoded frame has no picture format"))?;
    let info = gst_video::VideoInfo::from_caps(caps).wrap_err("a decoded frame has no size")?;
    let buffer = sample
        .buffer()
        .ok_or_else(|| eyre::Report::msg("a decoded frame has no picture"))?;
    let map = buffer
        .map_readable()
        .wrap_err("could not read a decoded frame")?;

    let (width, height) = (info.width() as usize, info.height() as usize);
    let stride = usize::try_from(info.stride()[0]).unwrap_or(width * 4);
    let data = map.as_slice();
    if data.len() < stride * height.saturating_sub(1) + width * 4 {
        eyre::bail!("a decoded frame is smaller than its size says");
    }

    // the rows may be padded; the frame is not
    let mut bgra = Vec::with_capacity(width * height * 4);
    for row in 0..height {
        let start = row * stride;
        bgra.extend_from_slice(&data[start..start + width * 4]);
    }
    // what draws the picture reads the fourth byte as transparency, and a source with no alpha
    // may leave it as it likes
    for pixel in bgra.as_chunks_mut::<4>().0 {
        pixel[3] = 255;
    }

    Ok(Frame {
        pts_s: shown_at(sample, buffer).map_or(0.0, |time| time.nseconds() as f64 / 1e9),
        width: width as u32,
        height: height as u32,
        bgra,
    })
}

impl Reader {
    /// The next decoded frame of the file, or `None` at its end.
    fn decode(&mut self) -> eyre::Result<Option<Frame>> {
        match self.sink.try_pull_sample(WAIT) {
            Some(sample) => frame_of(&sample).map(Some),
            None if self.sink.is_eos() => Ok(None),
            None => Err(failure(&self.pipeline, "could not read a frame")),
        }
    }

    /// Returns the next frame, or `None` at the end of the range.
    ///
    /// Frames come in order. Each is BGRA with the alpha byte set to opaque.
    ///
    /// # Errors
    ///
    /// Returns an error when the pipeline fails or stops giving frames before the end of the file
    /// (`could not read a frame`, with GStreamer's reason), or a decoded frame cannot be read as a
    /// picture.
    pub fn next_frame(&mut self) -> eyre::Result<Option<Frame>> {
        loop {
            if let Some(frame) = self.ready.pop_front() {
                return Ok(Some(frame));
            }
            if self.selector.done() {
                return Ok(None);
            }

            let more = if self.ended {
                Vec::new()
            } else if let Some(frame) = self.decode()? {
                self.selector.push(frame)
            } else {
                self.ended = true;
                self.selector.finish()
            };

            if self.ended && more.is_empty() {
                return Ok(None);
            }
            self.ready.extend(more);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testkit::NUMBERED_HEIGHT;
    use crate::testkit::NUMBERED_WIDTH;
    use crate::testkit::write_numbered_video;

    fn video(name: &str, frames: u32, fps: u32) -> Option<PathBuf> {
        let dir =
            std::env::temp_dir().join(format!("komorebi-recorder-gst-{}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("a temporary directory");
        let path = dir.join(name);
        write_numbered_video(&path, frames, fps).then_some(path)
    }

    #[test]
    fn a_video_is_opened_and_its_size_and_duration_are_read() {
        let Some(path) = video("open.mp4", 30, 30) else {
            return;
        };
        let asset = Asset::open(&path).expect("open");
        assert_eq!(asset.size(), (NUMBERED_WIDTH, NUMBERED_HEIGHT));
        assert!(
            (asset.duration_s() - 1.0).abs() < 0.2,
            "duration {}",
            asset.duration_s()
        );
    }

    #[test]
    fn every_frame_is_decoded_in_order_at_the_size_of_the_video() {
        let Some(path) = video("all.mp4", 30, 30) else {
            return;
        };
        let asset = Asset::open(&path).expect("open");
        let mut reader = asset.start(0.0, None, Scale::Native).expect("start");
        let mut times = Vec::new();
        while let Some(frame) = reader.next_frame().expect("a frame") {
            assert_eq!(
                (frame.width, frame.height),
                (NUMBERED_WIDTH, NUMBERED_HEIGHT)
            );
            assert_eq!(frame.bgra.len(), (frame.width * frame.height * 4) as usize);
            assert!(frame.bgra.chunks(4).all(|pixel| pixel[3] == 255));
            times.push(frame.pts_s);
        }
        assert_eq!(
            times.len(),
            30,
            "duration {} frames at {times:?}",
            asset.duration_s()
        );
        assert!(times.windows(2).all(|pair| pair[0] < pair[1]));
    }

    #[test]
    fn a_decode_that_starts_late_begins_with_the_frame_on_screen_then() {
        let Some(path) = video("seek.mp4", 90, 30) else {
            return;
        };
        let asset = Asset::open(&path).expect("open");
        let mut reader = asset.start(1.5, Some(2.0), Scale::Native).expect("start");
        let first = reader.next_frame().expect("a frame").expect("one");
        // the frame on screen at 1.5 s, which is at or just before it
        assert!(
            first.pts_s <= 1.5 && first.pts_s > 1.5 - 1.0 / 30.0 - 0.001,
            "first frame at {}",
            first.pts_s
        );
        let mut last = first.pts_s;
        while let Some(frame) = reader.next_frame().expect("a frame") {
            last = frame.pts_s;
        }
        assert!(last <= 2.0 + 0.001 && last > 1.9, "last frame at {last}");
    }

    #[test]
    fn frames_are_scaled_by_the_decoder() {
        let Some(path) = video("scale.mp4", 10, 30) else {
            return;
        };
        let asset = Asset::open(&path).expect("open");
        let mut reader = asset.start(0.0, None, Scale::Width(64)).expect("start");
        let frame = reader.next_frame().expect("a frame").expect("one");
        assert_eq!((frame.width, frame.height), (64, 32));
    }

    #[test]
    fn a_file_that_is_not_there_says_so() {
        let error = Asset::open(Path::new("/nonexistent/video.mp4"))
            .err()
            .expect("an error");
        assert!(error.to_string().contains("does not exist"), "{error}");
    }
}
