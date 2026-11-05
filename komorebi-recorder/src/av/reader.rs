//! Decoding recorded video on macOS with `AVFoundation`'s `AVAssetReader`.
//!
//! [`Asset`] is a file's first video track and the facts about it; [`Asset::start`] configures an
//! `AVAssetReader` completely (the time range, then an output asking for BGRA pixels at the size
//! wanted) before it calls `startReading`, which is the only order `AVFoundation` accepts without
//! raising an Objective-C exception. The decoder converts and scales, so a request for small frames
//! decodes fewer pixels than the recording has. The reader returns the frame that was on screen at
//! the start of the range as its first, so this module needs no frame selection (the
//! Windows decoder uses `Selector` for that).
//!
//! Measured on an Apple Silicon Mac with HEVC recordings of real displays, at time of writing
//! (2026-10-26): the first frame at a time arrives in 14 to 28 ms once warm, frames come at 250 to
//! 970 a second depending on the size, and the first open of a file takes about a second. The
//! ignored test `real_video_decode_speed` measures it on a recording named by `KOMOREBI_VIDEO`.
//!
//! The unit tests build a video whose frames are numbered (with `ffmpeg`; they skip
//! with a message without it) and read the number back out of the pixels, so seeking, ranges and
//! scaling are checked on what was decoded.

use super::Frame;
use super::Scale;
use color_eyre::eyre;
use objc2::rc::Retained;
use objc2::runtime::AnyObject;
use objc2_av_foundation::AVAsset;
use objc2_av_foundation::AVAssetReader;
use objc2_av_foundation::AVAssetReaderOutput;
use objc2_av_foundation::AVAssetReaderStatus;
use objc2_av_foundation::AVAssetReaderTrackOutput;
use objc2_av_foundation::AVAssetTrack;
use objc2_av_foundation::AVMediaTypeVideo;
use objc2_core_media::CMTime;
use objc2_core_media::CMTimeRange;
use objc2_core_video::CVPixelBufferGetBaseAddress;
use objc2_core_video::CVPixelBufferGetBytesPerRow;
use objc2_core_video::CVPixelBufferGetHeight;
use objc2_core_video::CVPixelBufferGetWidth;
use objc2_core_video::CVPixelBufferLockBaseAddress;
use objc2_core_video::CVPixelBufferLockFlags;
use objc2_core_video::CVPixelBufferUnlockBaseAddress;
use objc2_foundation::NSDictionary;
use objc2_foundation::NSNumber;
use objc2_foundation::NSString;
use objc2_foundation::NSURL;
use std::path::Path;

/// `kCVPixelFormatType_32BGRA`
const PIXEL_FORMAT_BGRA: usize = 0x4247_5241;

/// The timescale used for the times handed to the decoder: finer than any frame rate.
const TIMESCALE: i32 = 60_000;

/// An opened video file: its first video track, its size and its duration.
///
/// Opening reads those facts and decodes nothing. Each [`Asset::start`] makes a new decode that
/// starts where it is told to.
pub struct Asset {
    asset: Retained<AVAsset>,
    track: Retained<AVAssetTrack>,
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
    /// - `path` is not valid UTF-8;
    /// - `path` is not a file that exists (`<path> does not exist`);
    /// - the file has no video track (`<path> has no video track`), which is also what a file that
    ///   is not a video gives;
    /// - `AVFoundation` does not provide the video media type.
    ///
    /// None of these aborts the process.
    pub fn open(path: &Path) -> eyre::Result<Self> {
        let text = path
            .to_str()
            .ok_or_else(|| eyre::Report::msg("the video path is not valid UTF-8"))?;
        if !path.is_file() {
            eyre::bail!("{} does not exist", path.display());
        }

        // SAFETY: plain object construction and property reads on live objects; the deprecated
        // synchronous track accessor is fine for a local file
        unsafe {
            let url = NSURL::fileURLWithPath(&NSString::from_str(text));
            let asset = AVAsset::assetWithURL(&url);

            let media_type = AVMediaTypeVideo
                .ok_or_else(|| eyre::Report::msg("AVMediaTypeVideo is unavailable"))?;
            #[allow(deprecated)]
            let tracks = asset.tracksWithMediaType(media_type);
            let track = tracks.firstObject().ok_or_else(|| {
                eyre::Report::msg(format!("{} has no video track", path.display()))
            })?;

            let size = track.naturalSize();
            let duration = asset.duration();
            let duration_s = if duration.timescale > 0 && duration.value >= 0 {
                duration.value as f64 / f64::from(duration.timescale)
            } else {
                0.0
            };

            Ok(Self {
                asset,
                track,
                width: size.width.round().max(0.0) as u32,
                height: size.height.round().max(0.0) as u32,
                duration_s,
            })
        }
    }

    /// Returns the size the video was recorded at, as `(width, height)` in pixels.
    #[must_use]
    pub fn size(&self) -> (u32, u32) {
        (self.width, self.height)
    }

    /// Returns the duration `AVFoundation` reports for the file, in seconds, or `0.0` if it reports
    /// none.
    ///
    /// For a fragmented MP4 that was never finalized (a recorder that was killed) this is the
    /// playable duration, which is why recovery trusts it over `ffprobe`'s.
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
    /// `from_s` unless `to_s` is given. The time range given to the decoder is counted in units of
    /// 1/60000 of a second. A range past the end of the file is not an error: it gives at most the
    /// last frame.
    ///
    /// Frames are converted to BGRA and scaled by the decoder to the size `scale` asks for (see
    /// [`Scale`]).
    ///
    /// # Errors
    ///
    /// Returns an error when `AVFoundation` cannot make a reader for the file, rejects the output
    /// configuration, or cannot start decoding. The messages carry the description `AVFoundation`
    /// gives.
    pub fn start(&self, from_s: f64, to_s: Option<f64>, scale: Scale) -> eyre::Result<Reader> {
        let from = from_s.max(0.0);
        let end = to_s.unwrap_or(self.duration_s).max(from);
        let (width, height) = match scale {
            Scale::Native => (self.width, self.height),
            Scale::Width(wide) => {
                let wide = wide.clamp(2, self.width.max(2));
                let high = (u64::from(wide) * u64::from(self.height) / u64::from(self.width.max(1)))
                    .max(2) as u32;
                // codecs like even sizes
                (wide & !1, high & !1)
            }
        };

        // SAFETY: every object is live; the reader is configured completely (time range, output)
        // before `startReading`, which is the order AVFoundation requires
        unsafe {
            let reader =
                AVAssetReader::assetReaderWithAsset_error(&self.asset).map_err(|error| {
                    eyre::Report::msg(format!(
                        "could not read the video: {}",
                        error.localizedDescription()
                    ))
                })?;

            let start = CMTime::new((from * f64::from(TIMESCALE)).round() as i64, TIMESCALE);
            let span = CMTime::new(
                ((end - from) * f64::from(TIMESCALE)).ceil() as i64 + 1,
                TIMESCALE,
            );
            reader.setTimeRange(CMTimeRange {
                start,
                duration: span,
            });

            let format = NSNumber::new_usize(PIXEL_FORMAT_BGRA);
            let wide = NSNumber::new_usize(width as usize);
            let high = NSNumber::new_usize(height as usize);
            let keys = [
                &*NSString::from_str("PixelFormatType"),
                &*NSString::from_str("Width"),
                &*NSString::from_str("Height"),
            ];
            let values: [&AnyObject; 3] = [format.as_ref(), wide.as_ref(), high.as_ref()];
            let settings: Retained<NSDictionary<NSString, AnyObject>> =
                NSDictionary::from_slices(&keys, &values);

            let output = AVAssetReaderTrackOutput::assetReaderTrackOutputWithTrack_outputSettings(
                &self.track,
                Some(&settings),
            );
            // frames are only used once, in order
            output.setAlwaysCopiesSampleData(false);

            let as_output: &AVAssetReaderOutput = &output;
            if !reader.canAddOutput(as_output) {
                eyre::bail!("the video decoder rejected the output configuration");
            }
            reader.addOutput(as_output);

            if !reader.startReading() {
                let why = reader.error().map_or_else(
                    || "unknown error".to_string(),
                    |error| error.localizedDescription().to_string(),
                );
                eyre::bail!("could not start decoding the video: {why}");
            }

            Ok(Reader { reader, output })
        }
    }
}

/// A started decode. Dropping it cancels the decode.
///
/// Made only by [`Asset::start`], so it has always been configured completely before reading
/// began. It moves forward only: to look at another time, start another decode.
pub struct Reader {
    reader: Retained<AVAssetReader>,
    output: Retained<AVAssetReaderTrackOutput>,
}

impl Reader {
    /// Returns the next frame, or `None` at the end of the range.
    ///
    /// Frames come in order, and a sample with no picture is skipped.
    ///
    /// # Errors
    ///
    /// Returns an error when decoding fails (`decoding failed`, with `AVFoundation`'s description),
    /// or the decoder returns a frame with no pixels.
    pub fn next_frame(&mut self) -> eyre::Result<Option<Frame>> {
        // SAFETY: the reader has been started (the only way to hold a `Reader`); the sample
        // buffer and the pixel buffer it holds are live for as long as `sample` is
        unsafe {
            let Some(sample) = self.output.copyNextSampleBuffer() else {
                return match self.reader.status() {
                    AVAssetReaderStatus::Failed => {
                        let why = self.reader.error().map_or_else(
                            || "unknown error".to_string(),
                            |error| error.localizedDescription().to_string(),
                        );
                        Err(eyre::Report::msg(format!("decoding failed: {why}")))
                    }
                    _ => Ok(None),
                };
            };

            let time = sample.presentation_time_stamp();
            let pts_s = if time.timescale > 0 {
                time.value as f64 / f64::from(time.timescale)
            } else {
                0.0
            };

            let Some(image) = sample.image_buffer() else {
                // a marker sample with no picture: skip it
                return self.next_frame();
            };
            let pixels = image.as_ref();

            CVPixelBufferLockBaseAddress(pixels, CVPixelBufferLockFlags::ReadOnly);
            let width = CVPixelBufferGetWidth(pixels);
            let height = CVPixelBufferGetHeight(pixels);
            let stride = CVPixelBufferGetBytesPerRow(pixels);
            let base = CVPixelBufferGetBaseAddress(pixels).cast::<u8>();

            let row = width * 4;
            let mut bgra = Vec::with_capacity(row * height);
            if base.is_null() {
                CVPixelBufferUnlockBaseAddress(pixels, CVPixelBufferLockFlags::ReadOnly);
                eyre::bail!("the decoder returned a frame with no pixels");
            }
            for y in 0..height {
                let line = std::slice::from_raw_parts(base.add(y * stride), row);
                bgra.extend_from_slice(line);
            }
            CVPixelBufferUnlockBaseAddress(pixels, CVPixelBufferLockFlags::ReadOnly);

            Ok(Some(Frame {
                pts_s,
                width: width as u32,
                height: height as u32,
                bgra,
            }))
        }
    }
}

impl Drop for Reader {
    fn drop(&mut self) {
        // SAFETY: cancelling a started reader is always allowed
        unsafe { self.reader.cancelReading() };
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testkit;

    const FPS: u32 = 30;
    const FRAMES: u32 = 90;

    fn video(name: &str) -> Option<std::path::PathBuf> {
        let dir = std::env::temp_dir().join(format!(
            "komorebi-recorder-av-{name}-{}",
            std::process::id()
        ));
        std::fs::create_dir_all(&dir).expect("dir");
        let path = dir.join("numbered.mp4");
        if testkit::write_numbered_video(&path, FRAMES, FPS) {
            Some(path)
        } else {
            eprintln!("skipped: ffmpeg is not available to make a test video");
            None
        }
    }

    fn decode_all(asset: &Asset, from: f64, to: Option<f64>, scale: Scale) -> Vec<(f64, u32)> {
        let mut reader = asset.start(from, to, scale).expect("start");
        let mut frames = Vec::new();
        while let Some(frame) = reader.next_frame().expect("frame") {
            frames.push((
                frame.pts_s,
                testkit::numbered_frame_index(&frame.bgra, frame.width, frame.height),
            ));
        }
        frames
    }

    #[test]
    fn decoding_from_the_start_yields_every_frame_in_order() {
        let Some(path) = video("all") else { return };
        let asset = Asset::open(&path).expect("open");

        assert_eq!(
            asset.size(),
            (testkit::NUMBERED_WIDTH, testkit::NUMBERED_HEIGHT)
        );
        assert!((asset.duration_s() - f64::from(FRAMES) / f64::from(FPS)).abs() < 0.1);

        let frames = decode_all(&asset, 0.0, None, Scale::Native);
        assert_eq!(frames.len(), FRAMES as usize);
        for (expected, (pts, index)) in frames.iter().enumerate() {
            assert_eq!(
                *index, expected as u32,
                "frame {expected} was read as {index}"
            );
            assert!(
                (pts - expected as f64 / f64::from(FPS)).abs() < 0.002,
                "frame {expected} at {pts}"
            );
        }
    }

    #[test]
    fn starting_midway_begins_with_the_frame_on_screen_then() {
        let Some(path) = video("seek") else { return };
        let asset = Asset::open(&path).expect("open");

        // 1.0 s is frame 30; 1.02 s is still frame 30 (frame 31 appears at 1.0333)
        for (from, expected) in [(1.0, 30), (1.02, 30), (2.5, 75), (0.0, 0)] {
            let frames = decode_all(&asset, from, Some(from + 0.2), Scale::Native);
            assert_eq!(frames[0].1, expected, "starting at {from} s");
        }
    }

    #[test]
    fn a_range_stops_where_it_was_asked_to() {
        let Some(path) = video("range") else { return };
        let asset = Asset::open(&path).expect("open");

        let frames = decode_all(&asset, 1.0, Some(1.5), Scale::Native);
        let first = frames.first().expect("first").1;
        let last = frames.last().expect("last").1;
        assert_eq!(first, 30);
        assert!((44..=46).contains(&last), "the range ended at frame {last}");
    }

    #[test]
    fn the_decoder_scales() {
        let Some(path) = video("scale") else { return };
        let asset = Asset::open(&path).expect("open");

        let mut reader = asset
            .start(1.0, Some(1.1), Scale::Width(64))
            .expect("start");
        let frame = reader.next_frame().expect("frame").expect("a frame");
        assert_eq!((frame.width, frame.height), (64, 32));
        assert_eq!(frame.bgra.len(), 64 * 32 * 4);
        assert_eq!(
            testkit::numbered_frame_index(&frame.bgra, frame.width, frame.height),
            30
        );
    }

    #[test]
    fn reading_past_the_end_is_the_end_not_an_error() {
        let Some(path) = video("past") else { return };
        let asset = Asset::open(&path).expect("open");

        let frames = decode_all(&asset, 100.0, None, Scale::Native);
        assert!(frames.len() <= 1, "{} frames past the end", frames.len());
    }

    #[test]
    fn a_file_that_is_not_a_video_is_an_error_not_an_abort() {
        let dir =
            std::env::temp_dir().join(format!("komorebi-recorder-av-bad-{}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("dir");
        let text = dir.join("not-a-video.mp4");
        std::fs::write(&text, "this is not a video").expect("file");

        assert!(Asset::open(&text).is_err());
        assert!(Asset::open(&dir.join("missing.mp4")).is_err());
    }

    /// Measures decoding a real recording: `KOMOREBI_VIDEO=<file> cargo test ... -- --ignored
    /// --nocapture`. Prints how long it takes to get the first frame at a time (the latency of
    /// scrubbing) and how fast frames come once started (the rate of playing).
    #[test]
    #[ignore = "needs a real recording in KOMOREBI_VIDEO"]
    fn real_video_decode_speed() {
        use std::time::Instant;

        let Some(path) = std::env::var_os("KOMOREBI_VIDEO") else {
            eprintln!("set KOMOREBI_VIDEO to a recorded video file");
            return;
        };
        let opened = Instant::now();
        let asset = Asset::open(std::path::Path::new(&path)).expect("open");
        eprintln!(
            "open {:?}: {}x{}, {:.1} s",
            opened.elapsed(),
            asset.size().0,
            asset.size().1,
            asset.duration_s()
        );

        for scale in [Scale::Width(320), Scale::Width(1200), Scale::Native] {
            let mut first = Vec::new();
            for index in 0..8 {
                let at = asset.duration_s() * (f64::from(index) + 0.5) / 8.0;
                let started = Instant::now();
                let mut reader = asset.start(at, Some(at + 0.5), scale).expect("start");
                let frame = reader.next_frame().expect("frame").expect("a frame");
                first.push(started.elapsed().as_secs_f64() * 1000.0);
                assert!(
                    frame.pts_s <= at + 0.05,
                    "the first frame is at or before the time asked"
                );
            }
            first.sort_by(f64::total_cmp);

            let started = Instant::now();
            let mut reader = asset.start(1.0, Some(3.0), scale).expect("start");
            let mut count = 0;
            let mut size = (0, 0);
            while let Some(frame) = reader.next_frame().expect("frame") {
                count += 1;
                size = (frame.width, frame.height);
            }
            let seconds = started.elapsed().as_secs_f64();
            eprintln!(
                "{scale:?} ({}x{}): first frame median {:.0} ms (min {:.0}, max {:.0}); {count} frames in {:.2} s = {:.0} fps",
                size.0,
                size.1,
                first[first.len() / 2],
                first[0],
                first[first.len() - 1],
                seconds,
                f64::from(count) / seconds
            );
        }
    }
}
