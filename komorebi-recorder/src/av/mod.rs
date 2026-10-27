//! Decoding the recorded video.
//!
//! The recorder writes each display's video as a fragmented MP4 (see [`crate::capture`]). Reading
//! it back means asking the OS for the frames between two times, decoded to BGRA at a size of the
//! caller's choosing (the decoder scales, so a contact sheet never decodes more pixels than it
//! shows). This is the host axis of reading a session: the types below are the same everywhere and
//! the decoder behind them is the host's.
//!
//! | host | decoder | module |
//! |---|---|---|
//! | macOS | `AVFoundation`'s `AVAssetReader` | `reader` |
//! | Windows | Media Foundation's Source Reader | `mf` |
//! | any other | none: [`Asset::open`] fails | `unsupported` |
//!
//! The decoder modules are private. [`Asset`] and [`Reader`] are re-exports of the host's, and
//! [`supported`] says whether there is one. A host with no decoder can still read everything else
//! in a session.
//!
//! # Using it
//!
//! [`Asset::open`] a file, [`Asset::start`] a decode of a range of it at a [`Scale`], and call
//! [`Reader::next_frame`] until it returns `None`. The first frame is the one that was on screen at
//! the start of the range, and a recorded display only has frames where the screen changed, so a
//! range can hold few frames or one.
//!
//! ```rust,no_run
//! use komorebi_recorder::av::{Asset, Scale};
//! use std::path::Path;
//!
//! let asset = Asset::open(Path::new("video-1.mp4"))?;
//! let (width, height) = asset.size();
//! println!("{width}x{height}, {:.1} s", asset.duration_s());
//!
//! // The frames of the second second of the file, decoded 320 pixels wide.
//! let mut reader = asset.start(1.0, Some(2.0), Scale::Width(320))?;
//! while let Some(frame) = reader.next_frame()? {
//!     assert_eq!(frame.bgra.len(), frame.width as usize * frame.height as usize * 4);
//!     println!("frame at {:.3} s", frame.pts_s);
//! }
//! # Ok::<(), Box<dyn std::error::Error>>(())
//! ```
//!
//! The example needs a recorded video, so it is compiled and not run. What it does is covered on
//! macOS by the unit tests in `av::reader` (they build a video whose frames number themselves and
//! need `ffmpeg`), and on every host by the tests of the frame selection rule (see the private
//! `Selector`). Nothing exercises the Windows decoder without a Windows machine, where
//! `doctor --probe decode` measures it.
//!
//! # Why it is shaped this way
//!
//! `AVFoundation`'s objects are not `Send` and some calls raise an Objective-C exception instead of
//! returning an error when used in the wrong order, which would abort the process. The types here
//! make the wrong order impossible to write: the only way to a [`Reader`] is
//! [`Asset::start`], which configures everything before reading begins. A Media Foundation object
//! is likewise used only on the thread that made it, so on Windows too a decode is started and read
//! on one thread.
//!
//! # What this module does not do
//!
//! It does not decide which frames a recording means: choosing the moments to look at, comparing
//! frames and drawing contact sheets are done by `analysis` and `playback`, on top of this. It
//! decodes video only. It does not seek within a started decode: start another [`Reader`].

/// Returns `true` if and only if this host can decode the recorded video.
///
/// That is macOS and Windows. Elsewhere [`Asset::open`] always fails.
///
/// # Example
///
/// ```rust
/// assert_eq!(
///     komorebi_recorder::av::supported(),
///     cfg!(any(target_os = "macos", windows))
/// );
/// ```
#[must_use]
pub const fn supported() -> bool {
    cfg!(any(target_os = "macos", windows))
}

#[cfg(windows)]
mod mf;
#[cfg(target_os = "macos")]
mod reader;
#[cfg(not(any(target_os = "macos", windows)))]
mod unsupported;

#[cfg(windows)]
pub use mf::Asset;
#[cfg(windows)]
pub use mf::Reader;
#[cfg(target_os = "macos")]
pub use reader::Asset;
#[cfg(target_os = "macos")]
pub use reader::Reader;
#[cfg(not(any(target_os = "macos", windows)))]
pub use unsupported::Asset;
#[cfg(not(any(target_os = "macos", windows)))]
pub use unsupported::Reader;

/// The size to decode frames at.
///
/// The decoder does the scaling, so a request for small frames decodes fewer pixels than the
/// recording has.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Scale {
    /// The size the video was recorded at ([`Asset::size`]).
    Native,
    /// This wide, keeping the aspect ratio. The width is limited to between 2 and the recorded
    /// width (a frame is never scaled up), and the height follows from the aspect ratio. Both are
    /// rounded down to an even number, as video codecs want, and are never less than 2.
    Width(u32),
}

/// One decoded frame, tightly packed BGRA (no row padding).
///
/// The `bgra` field is `width * height * 4` bytes: the blue, green, red and alpha bytes of each
/// pixel, row by row from the top.
#[derive(Debug, Clone, PartialEq)]
pub struct Frame {
    /// When the frame is shown, in seconds from the start of *this file*. The file's time zero is
    /// its first frame, which was captured at [`DisplayResult::video_t0_mach_ns`]; the session's
    /// video time of a file is offset from it by [`DisplayEntry::offset_s`].
    ///
    /// [`DisplayResult::video_t0_mach_ns`]: crate::capture::DisplayResult::video_t0_mach_ns
    /// [`DisplayEntry::offset_s`]: crate::session::DisplayEntry::offset_s
    pub pts_s: f64,
    /// The width of the picture in pixels.
    pub width: u32,
    /// The height of the picture in pixels.
    pub height: u32,
    /// The pixels: `width * height * 4` bytes, BGRA, top row first. Alpha is opaque on Windows,
    /// where the decoder leaves it undefined.
    pub bgra: Vec<u8>,
}

/// Picks the frames a request for `from..=end` is answered with from the decoded frames of a file,
/// given in order.
///
/// A decoder that seeks can only land on a key frame before the time asked for and decodes forward
/// from there, so the frames before `from` have to be told from the one that was on screen at
/// `from`, which is the last of them: it is the first frame the request gets, and it can be earlier
/// than `from`. Frames after `end` are not wanted and the first of them ends the request.
///
/// The Windows decoder (`mf`) uses it, because Media Foundation's Source Reader seeks to the key
/// frame before the position and decodes forward. The macOS decoder (`reader`) does not: an
/// `AVAssetReader` given a time range starts at the frame on screen at its start. The rule is
/// host-independent, so its tests run on every host.
///
/// # Protocol
///
/// Create it with the request's `from` and `end` in seconds of the file's time, [`push`] each
/// decoded frame in order and collect what comes back, stop as soon as [`done`] is `true`, and if
/// the decoder runs out of frames first, collect [`finish`].
///
/// ```text
/// request 1.25..=2.0; the decoder lands on a key frame at 0.0 and decodes forward
///
/// push(0.0) -> []          held: before the start, it may be the one on screen
/// push(0.5) -> []          held instead of 0.0
/// push(1.0) -> []          held instead of 0.5
/// push(1.5) -> [1.0, 1.5]  the first frame at or after the start: the held 1.0 was on screen at
///                          1.25, so it is the request's first frame
/// push(2.0) -> [2.0]       a frame exactly at the end is still wanted
/// push(2.5) -> []          later than the end: done() becomes true
/// ```
///
/// Edge cases, each covered by a unit test of this module:
///
/// - a frame exactly at `from` is the first frame, and the held one before it is dropped;
/// - a request after the last frame of a still display is answered by [`finish`] with the last
///   frame before `from`, because nothing changed after it;
/// - a request before the first frame is answered from the first frame;
/// - an empty file answers with nothing.
///
/// [`push`]: Selector::push
/// [`done`]: Selector::done
/// [`finish`]: Selector::finish
#[derive(Debug)]
#[cfg_attr(not(windows), allow(dead_code))]
pub(crate) struct Selector {
    from_s: f64,
    end_s: f64,
    started: bool,
    /// The most recent frame before `from`.
    pending: Option<Frame>,
    done: bool,
}

#[cfg_attr(not(windows), allow(dead_code))]
impl Selector {
    /// Creates a selector for the frames shown from `from_s` to `end_s`, both in seconds of the
    /// file's time.
    pub(crate) fn new(from_s: f64, end_s: f64) -> Self {
        Self {
            from_s,
            end_s,
            started: false,
            pending: None,
            done: false,
        }
    }

    /// Returns `true` if and only if the request is answered in full: nothing more is wanted.
    pub(crate) fn done(&self) -> bool {
        self.done
    }

    fn within(&mut self, frame: Frame) -> Vec<Frame> {
        if frame.pts_s > self.end_s {
            self.done = true;
            Vec::new()
        } else {
            vec![frame]
        }
    }

    /// Offers the next decoded frame and returns the frames that are ready to be handed out, in
    /// order: none, one, or the held frame followed by this one.
    ///
    /// Returns nothing once the request is [done](Selector::done).
    pub(crate) fn push(&mut self, frame: Frame) -> Vec<Frame> {
        if self.done {
            return Vec::new();
        }

        if !self.started {
            if frame.pts_s < self.from_s {
                self.pending = Some(frame);
                return Vec::new();
            }

            self.started = true;
            // a frame exactly at `from` is the one on screen then; only when the first at or after
            // it is later does the one before it still hold the screen
            let before = self.pending.take().filter(|_| frame.pts_s > self.from_s);
            let mut ready: Vec<Frame> = before.into_iter().collect();
            ready.extend(self.within(frame));
            return ready;
        }

        self.within(frame)
    }

    /// Tells the selector that the file has no more frames, and returns what is left to hand out.
    ///
    /// If nothing later was found, the frame that was on screen at `from` is the answer (a still
    /// display, where no frame follows). After a frame at or after `from` has been pushed, nothing
    /// is left.
    pub(crate) fn finish(&mut self) -> Vec<Frame> {
        self.done = true;
        if self.started {
            return Vec::new();
        }

        self.started = true;
        self.pending.take().into_iter().collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn frame(pts_s: f64) -> Frame {
        Frame {
            pts_s,
            width: 2,
            height: 2,
            bgra: vec![0; 16],
        }
    }

    fn times(frames: &[Frame]) -> Vec<f64> {
        frames.iter().map(|frame| frame.pts_s).collect()
    }

    fn run(from: f64, end: f64, decoded: &[f64]) -> Vec<f64> {
        let mut selector = Selector::new(from, end);
        let mut out = Vec::new();
        for pts in decoded {
            out.extend(selector.push(frame(*pts)));
            if selector.done() {
                return times(&out);
            }
        }
        out.extend(selector.finish());
        times(&out)
    }

    #[test]
    fn the_first_frame_is_the_one_that_was_on_screen_at_the_start() {
        // decoding began at a key frame at 0.0; the request starts at 1.25
        assert_eq!(
            run(1.25, 2.0, &[0.0, 0.5, 1.0, 1.5, 2.0, 2.5]),
            [1.0, 1.5, 2.0]
        );
    }

    #[test]
    fn a_frame_exactly_at_the_start_is_the_first() {
        assert_eq!(run(1.0, 1.5, &[0.0, 1.0, 1.5, 2.0]), [1.0, 1.5]);
    }

    #[test]
    fn nothing_after_the_end_is_returned_and_the_request_stops_there() {
        let mut selector = Selector::new(0.0, 1.0);
        assert_eq!(times(&selector.push(frame(0.5))), [0.5]);
        assert_eq!(times(&selector.push(frame(1.5))), Vec::<f64>::new());
        assert!(selector.done());
        assert!(selector.push(frame(1.6)).is_empty());
    }

    #[test]
    fn a_still_display_answers_with_the_last_frame_before_the_start() {
        // the screen did not change after 1.0 (and the file ends): that is what was on screen at 5
        assert_eq!(run(5.0, 6.0, &[0.0, 1.0]), [1.0]);
    }

    #[test]
    fn a_request_before_the_first_frame_gets_the_first_frame() {
        assert_eq!(run(0.0, 1.0, &[0.2, 0.6]), [0.2, 0.6]);
    }

    #[test]
    fn an_empty_file_answers_with_nothing() {
        assert_eq!(run(0.0, 1.0, &[]), Vec::<f64>::new());
    }
}
