//! Decoding recorded video on Windows with Media Foundation's Source Reader.
//!
//! The reader is asked for BGRA frames at the size wanted (its video processor converts and
//! scales, so a contact sheet never decodes more pixels than it shows), seeks to the key frame
//! before the time asked for, and decodes forward; [`super::Selector`] says which of the decoded
//! frames are the answer. Media Foundation objects belong to the thread that made them as far as
//! this code is concerned, so a decode is started and read on one thread, as the Mac's are.
//!
//! An [`Asset`] keeps the path and what was read from the file when it was opened (size, duration,
//! whether it is HEVC), not an open reader: every [`Asset::start`] opens a new Source Reader, so a
//! file that changes after it was opened is read as it is then.
//!
//! The recorder writes H.264 on Windows, which Media Foundation decodes everywhere. An HEVC file (a
//! recording made on a Mac) needs a decoder Windows does not always have, and the error from
//! [`Asset::start`] tells the user to install the HEVC Video Extensions or to record with H.264.
//!
//! # Tests
//!
//! The rule for which decoded frames answer a request is `Selector`, and its tests run on every
//! host. Nothing in this module runs without Windows and a recorded video: `doctor --probe decode`
//! measures first-frame time, decode speed and seeking there, and the checkpoint W3 in
//! `docs/dev/windows-checkpoints.md` is its first run.

use super::Frame;
use super::Scale;
use super::Selector;
use crate::platform::media::pack;
use crate::platform::media::unpack;
use color_eyre::eyre;
use color_eyre::eyre::WrapErr;
use std::collections::VecDeque;
use std::path::Path;
use windows::Win32::Media::MediaFoundation::IMF2DBuffer;
use windows::Win32::Media::MediaFoundation::IMFAttributes;
use windows::Win32::Media::MediaFoundation::IMFSample;
use windows::Win32::Media::MediaFoundation::IMFSourceReader;
use windows::Win32::Media::MediaFoundation::MF_MT_DEFAULT_STRIDE;
use windows::Win32::Media::MediaFoundation::MF_MT_FRAME_SIZE;
use windows::Win32::Media::MediaFoundation::MF_MT_INTERLACE_MODE;
use windows::Win32::Media::MediaFoundation::MF_MT_MAJOR_TYPE;
use windows::Win32::Media::MediaFoundation::MF_MT_SUBTYPE;
use windows::Win32::Media::MediaFoundation::MF_PD_DURATION;
use windows::Win32::Media::MediaFoundation::MF_SOURCE_READER_ALL_STREAMS;
use windows::Win32::Media::MediaFoundation::MF_SOURCE_READER_ENABLE_VIDEO_PROCESSING;
use windows::Win32::Media::MediaFoundation::MF_SOURCE_READER_FIRST_VIDEO_STREAM;
use windows::Win32::Media::MediaFoundation::MF_SOURCE_READER_MEDIASOURCE;
use windows::Win32::Media::MediaFoundation::MF_SOURCE_READERF_ENDOFSTREAM;
use windows::Win32::Media::MediaFoundation::MFCreateAttributes;
use windows::Win32::Media::MediaFoundation::MFCreateMediaType;
use windows::Win32::Media::MediaFoundation::MFCreateSourceReaderFromURL;
use windows::Win32::Media::MediaFoundation::MFMediaType_Video;
use windows::Win32::Media::MediaFoundation::MFVideoFormat_HEVC;
use windows::Win32::Media::MediaFoundation::MFVideoFormat_RGB32;
use windows::Win32::Media::MediaFoundation::MFVideoInterlace_Progressive;
use windows::Win32::System::Com::StructuredStorage::PROPVARIANT;
use windows::Win32::System::Com::StructuredStorage::PropVariantToUInt64;
use windows::core::GUID;
use windows::core::Interface;
use windows::core::PCWSTR;

/// The first video stream, as the Source Reader numbers streams.
const VIDEO: u32 = MF_SOURCE_READER_FIRST_VIDEO_STREAM.0 as u32;

/// Media Foundation counts in hundred-nanosecond units.
const UNITS_PER_SECOND: f64 = 10_000_000.0;

fn open_reader(path: &Path) -> eyre::Result<IMFSourceReader> {
    crate::platform::media::start()?;
    if !path.is_file() {
        eyre::bail!("{} does not exist", path.display());
    }

    let wide: Vec<u16> = path
        .to_string_lossy()
        .encode_utf16()
        .chain(Some(0))
        .collect();
    // SAFETY: attribute setters and a creation call with the arguments they document; `wide`
    // outlives the call that reads it
    unsafe {
        let mut attributes: Option<IMFAttributes> = None;
        MFCreateAttributes(&raw mut attributes, 1)?;
        let attributes =
            attributes.ok_or_else(|| eyre::Report::msg("no Media Foundation attributes"))?;
        // the video processor converts the decoded picture to the format asked for and resizes it
        attributes.SetUINT32(&MF_SOURCE_READER_ENABLE_VIDEO_PROCESSING, 1)?;

        let reader = MFCreateSourceReaderFromURL(PCWSTR(wide.as_ptr()), &attributes)
            .wrap_err_with(|| format!("Windows cannot open {} as a video", path.display()))?;
        reader.SetStreamSelection(MF_SOURCE_READER_ALL_STREAMS.0 as u32, false)?;
        reader.SetStreamSelection(VIDEO, true)?;
        Ok(reader)
    }
}

/// An opened video file: its path, size and duration.
///
/// Opening reads those facts and decodes nothing. Each [`Asset::start`] opens a new Source
/// Reader on the file and starts where it is told to.
pub struct Asset {
    path: std::path::PathBuf,
    width: u32,
    height: u32,
    duration_s: f64,
    /// Whether the video is HEVC, which needs a decoder Windows does not always have.
    hevc: bool,
}

impl Asset {
    /// Opens a video file.
    ///
    /// # Errors
    ///
    /// Returns an error when:
    ///
    /// - Windows Runtime or Media Foundation cannot be started;
    /// - `path` is not a file that exists (`<path> does not exist`);
    /// - Windows cannot open the file as a video (`Windows cannot open <path> as a video`);
    /// - the file has no video track (`<path> has no video track`), or its frame size cannot be
    ///   read.
    pub fn open(path: &Path) -> eyre::Result<Self> {
        let reader = open_reader(path)?;

        // SAFETY: property reads on a live reader
        unsafe {
            let native = reader
                .GetNativeMediaType(VIDEO, 0)
                .wrap_err_with(|| format!("{} has no video track", path.display()))?;
            let (width, height) = unpack(native.GetUINT64(&MF_MT_FRAME_SIZE)?);
            let hevc = native
                .GetGUID(&MF_MT_SUBTYPE)
                .is_ok_and(|subtype| subtype == MFVideoFormat_HEVC);

            let duration = reader
                .GetPresentationAttribute(MF_SOURCE_READER_MEDIASOURCE.0 as u32, &MF_PD_DURATION)
                .ok()
                .and_then(|value| PropVariantToUInt64(&value).ok())
                .map_or(0.0, |units| units as f64 / UNITS_PER_SECOND);

            Ok(Self {
                path: path.to_path_buf(),
                width,
                height,
                duration_s: duration,
                hevc,
            })
        }
    }

    /// Returns the size the video was recorded at, as `(width, height)` in pixels.
    #[must_use]
    pub fn size(&self) -> (u32, u32) {
        (self.width, self.height)
    }

    /// Returns the duration Media Foundation reports for the file, in seconds, or `0.0` if it
    /// reports none (some truncated files).
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
    /// The Source Reader is asked for BGRA frames of the size `scale` asks for (see [`Scale`]).
    /// When `from_s` is not `0` it seeks to the key frame at or before it, and the frames from
    /// there up to `from_s` are decoded and passed over: a seek costs the frames between a key
    /// frame and the time asked for.
    ///
    /// # Errors
    ///
    /// Returns an error when:
    ///
    /// - the file cannot be opened again (see [`Asset::open`]);
    /// - the video is HEVC and Windows has no HEVC decoder (the message tells the user to install
    ///   the HEVC Video Extensions or to record with `--codec h264`);
    /// - Windows cannot decode the video to pictures of that size and format;
    /// - the seek to `from_s` fails (`could not seek in the video`).
    pub fn start(&self, from_s: f64, to_s: Option<f64>, scale: Scale) -> eyre::Result<Reader> {
        let from = from_s.max(0.0);
        let end = to_s.unwrap_or(self.duration_s).max(from);
        let (width, height) = match scale {
            Scale::Native => (self.width, self.height),
            Scale::Width(wide) => {
                let wide = wide.clamp(2, self.width.max(2));
                let high = (u64::from(wide) * u64::from(self.height)
                    / u64::from(self.width.max(1)))
                .max(2);
                // codecs like even sizes
                (wide & !1, (high as u32) & !1)
            }
        };

        let reader = open_reader(&self.path)?;
        // SAFETY: setters on a media type just created and a reader just opened
        unsafe {
            let wanted = MFCreateMediaType()?;
            wanted.SetGUID(&MF_MT_MAJOR_TYPE, &MFMediaType_Video)?;
            wanted.SetGUID(&MF_MT_SUBTYPE, &MFVideoFormat_RGB32)?;
            wanted.SetUINT32(&MF_MT_INTERLACE_MODE, MFVideoInterlace_Progressive.0 as u32)?;
            wanted.SetUINT64(&MF_MT_FRAME_SIZE, pack(width, height))?;
            // a positive stride is a top-down picture; Media Foundation's default for RGB is the
            // other way up
            wanted.SetUINT32(&MF_MT_DEFAULT_STRIDE, width * 4)?;

            if let Err(error) = reader.SetCurrentMediaType(VIDEO, None, &wanted) {
                if self.hevc {
                    eyre::bail!(
                        "this video is HEVC and Windows has no HEVC decoder installed: install \
                         the \"HEVC Video Extensions\" from the Microsoft Store, or record with \
                         --codec h264 ({error})"
                    );
                }
                return Err(error).wrap_err("Windows cannot decode this video to pictures");
            }

            if from > 0.0 {
                // to the key frame at or before it; the frames up to `from` are decoded and passed
                // over by the selector
                reader
                    .SetCurrentPosition(
                        &GUID::zeroed(),
                        &PROPVARIANT::from((from * UNITS_PER_SECOND) as i64),
                    )
                    .wrap_err("could not seek in the video")?;
            }
        }

        Ok(Reader {
            reader,
            selector: Selector::new(from, end),
            ready: VecDeque::new(),
            ended: false,
        })
    }
}

/// A started decode. Dropping it cancels the decode.
///
/// Made only by [`Asset::start`]. It moves forward only: to look at another time, start another
/// decode. The frames it returns are the ones a `Selector` picks from what the Source Reader
/// decodes, so they begin with the frame on screen at the start of the range.
pub struct Reader {
    reader: IMFSourceReader,
    selector: Selector,
    ready: VecDeque<Frame>,
    ended: bool,
}

/// One decoded sample as a tightly packed BGRA frame.
fn frame_of(reader: &IMFSourceReader, sample: &IMFSample, time: i64) -> eyre::Result<Frame> {
    // SAFETY: the sample is live; the buffer is locked for exactly the copy and unlocked after
    unsafe {
        let current = reader.GetCurrentMediaType(VIDEO)?;
        let (width, height) = unpack(current.GetUINT64(&MF_MT_FRAME_SIZE)?);
        let (width, height) = (width as usize, height as usize);

        let buffer = sample.ConvertToContiguousBuffer()?;
        let mut bgra = vec![0_u8; width * height * 4];

        if let Ok(flat) = buffer.cast::<IMF2DBuffer>() {
            let (mut first, mut pitch) = (std::ptr::null_mut::<u8>(), 0_i32);
            flat.Lock2D(&raw mut first, &raw mut pitch)?;
            for row in 0..height {
                // the pitch is negative for a picture stored bottom-up, and `first` is then the
                // top row all the same
                let source = first.offset(row as isize * pitch as isize);
                std::ptr::copy_nonoverlapping(
                    source,
                    bgra.as_mut_ptr().add(row * width * 4),
                    width * 4,
                );
            }
            flat.Unlock2D()?;
        } else {
            let (mut data, mut length) = (std::ptr::null_mut::<u8>(), 0_u32);
            buffer.Lock(&raw mut data, None, Some(&raw mut length))?;
            let usable = (length as usize).min(bgra.len());
            std::ptr::copy_nonoverlapping(data, bgra.as_mut_ptr(), usable);
            buffer.Unlock()?;
        }

        // RGB32 leaves the fourth byte of each pixel undefined, and what draws the picture reads
        // it as transparency
        for pixel in bgra.as_chunks_mut::<4>().0 {
            pixel[3] = 255;
        }

        Ok(Frame {
            pts_s: time as f64 / UNITS_PER_SECOND,
            width: width as u32,
            height: height as u32,
            bgra,
        })
    }
}

impl Reader {
    /// The next decoded frame of the file, or `None` at its end.
    fn decode(&mut self) -> eyre::Result<Option<Frame>> {
        loop {
            let (mut flags, mut time) = (0_u32, 0_i64);
            let mut sample: Option<IMFSample> = None;
            // SAFETY: the reader was set up by `start`; the out pointers are valid for the call
            unsafe {
                self.reader.ReadSample(
                    VIDEO,
                    0,
                    None,
                    Some(&raw mut flags),
                    Some(&raw mut time),
                    Some(&raw mut sample),
                )
            }
            .wrap_err("could not read a frame")?;

            if flags & MF_SOURCE_READERF_ENDOFSTREAM.0 as u32 != 0 {
                return Ok(None);
            }

            // a tick with no picture, in a gap of the file: nothing yet
            let Some(sample) = sample else {
                continue;
            };
            return frame_of(&self.reader, &sample, time).map(Some);
        }
    }

    /// Returns the next frame, or `None` at the end of the range.
    ///
    /// Frames come in order. Each is BGRA with the alpha byte set to opaque, because Media
    /// Foundation leaves it undefined and what draws the picture reads it as transparency.
    ///
    /// # Errors
    ///
    /// Returns an error when the Source Reader fails to read a sample (`could not read a frame`),
    /// or a decoded sample cannot be read as a picture.
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
