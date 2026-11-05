//! The recorded video, addressed by session time and display.
//!
//! A display's video is one file per segment (a display that was unplugged or slept and came
//! back starts a new one), and a file's own time starts at its first frame. This maps a moment of
//! session video time on a display to the file and file time that has it, decodes frames, and says
//! when there is no video to show (the display was absent).
//!
//! [`SessionVideo`] is what the `changes` and `sheet` commands and the viewer's playback ask for
//! frames. It uses the decoder of `crate::av` (`AVFoundation` on macOS, Media Foundation on Windows),
//! so a host with neither cannot open a file; see `crate::av::supported`.
//!
//! # Times
//!
//! Every time here is *session video time*: seconds from the first frame of any display
//! ([`crate::session::Manifest::video_t0`]). A segment's file has its own time, which starts at its
//! first frame, and the manifest says how far into session time that is ([`DisplayEntry::offset_s`]):
//! session time `t` is at `t - offset_s` in the file. A moment is covered by a segment of a display
//! if it falls between the segment's start and end, with 50 ms of slack on each side, since
//! segment boundaries are sampled and not exact; where two segments of a display both cover a
//! moment, the later one is used ([`crate::session::Manifest::segment_at`]). A moment covered by
//! none is a gap (the display was unplugged, asleep, or not yet recorded) and the methods say so
//! with `None`, never with an invented frame.
//!
//! | To get | Use | Crosses segments |
//! |---|---|---|
//! | the frame on screen at one moment | [`SessionVideo::frame_at`] | no |
//! | the frames from one moment on, one at a time | [`SessionVideo::open_at`] | no |
//! | every frame of a range | [`SessionVideo::scan`] | yes |
//! | which displays have video, and whether one does at a moment | [`SessionVideo::displays`], [`SessionVideo::has_video_at`] | |
//!
//! # Example
//!
//! Needs `ffmpeg` to make a test video, and a host that can decode it, so it is only compiled here;
//! the tests of `tests/video.rs` (`a_sheet_shows_the_frame_that_was_on_screen_at_each_moment`,
//! `a_moment_a_display_had_no_video_is_skipped_not_invented`) run these calls through the `sheet`
//! command.
//!
//! ```rust,no_run
//! use komorebi_recorder::analysis::Analysis;
//! use komorebi_recorder::analysis::video::SessionVideo;
//! use komorebi_recorder::av::Scale;
//! use komorebi_recorder::testkit::SessionBuilder;
//!
//! // display 1 has video for 3 seconds from the start; display 2 only from 1 s to 3 s
//! let (session, _) = SessionBuilder::new("doc-video")
//!     .video(1, 0.0, 90, 30)
//!     .video(2, 1.0, 60, 30)
//!     .build();
//! let analysis = Analysis::open(&session.dir)?;
//! let video = SessionVideo::new(&analysis.manifest, analysis.guard());
//!
//! assert_eq!(video.displays(), [1, 2]);
//! assert!(video.has_video_at(1, 0.5));
//! assert!(!video.has_video_at(2, 0.5));
//!
//! // the frame on screen at 1.5 s, at most 128 pixels wide; never shown later than asked
//! let decoded = video.frame_at(1, 1.5, Scale::Width(128))?.expect("display 1 had video");
//! assert!(decoded.shown_s <= 1.5);
//!
//! // display 2 had none at 0.5 s
//! assert!(video.frame_at(2, 0.5, Scale::Width(128))?.is_none());
//! # Ok::<(), Box<dyn std::error::Error>>(())
//! ```

use super::guard::SessionPath;
use crate::av::Asset;
use crate::av::Frame;
use crate::av::Scale;
use crate::session::DisplayEntry;
use crate::session::Manifest;
use color_eyre::eyre;
use std::cell::RefCell;
use std::collections::HashMap;
use std::path::PathBuf;
use std::rc::Rc;

/// A decoded frame and when it was actually on screen.
pub struct DecodedFrame {
    /// The display the frame is of.
    pub display: u32,
    /// The segment of the display's video it came from.
    pub segment: u32,
    /// When the frame was shown, in session video time. Never later than what was asked for.
    ///
    /// A screen that holds still produces no new frames, so the frame that was on screen at a
    /// moment can have been put there some time before it.
    pub shown_s: f64,
    /// The decoded picture, scaled as asked.
    pub frame: Frame,
}

/// A started decode of one segment of one display. Frames come in order, each with the session
/// video time it was on screen.
///
/// Made by [`SessionVideo::open_at`]. A stream stays within its segment: it does not continue into
/// the next one.
pub struct Stream {
    reader: crate::av::Reader,
    offset_s: f64,
    /// The segment of the display's video it reads.
    pub segment: u32,
    /// Session video time at which the segment ends.
    pub end_s: f64,
}

impl Stream {
    /// Returns the next frame and when it was shown, in session video time, or `None` at the end of
    /// the segment (or of the range the stream was opened with).
    ///
    /// # Errors
    ///
    /// Returns an error if the decoder fails.
    pub fn next_frame(&mut self) -> eyre::Result<Option<(f64, Frame)>> {
        Ok(self
            .reader
            .next_frame()?
            .map(|frame| (self.offset_s + frame.pts_s, frame)))
    }
}

/// The video of a session. Not `Send`: decoding objects belong to the thread that made them.
///
/// A file is opened the first time a frame of it is asked for and kept open for the life of the
/// `SessionVideo`, so asking for many frames of the same segment opens it once. It borrows the
/// session's [`Manifest`] and [`SessionPath`] (`Analysis::manifest` and `Analysis::guard`), and the
/// guard is what keeps a file named in the manifest from leading outside the session.
///
/// See the [module docs](self) for how session time maps to files and an example.
///
/// # Example: it stays on the thread that made it
///
/// The compiler refuses to send one to another thread:
///
/// ```rust,compile_fail,E0277
/// use komorebi_recorder::analysis::video::SessionVideo;
///
/// fn send_to_another_thread<T: Send>() {}
/// send_to_another_thread::<SessionVideo<'static>>();
/// ```
///
/// A thread that decodes makes its own, from a manifest and a guard that can be shared (an
/// [`Analysis`](super::Analysis) is `Send` and `Sync`):
///
/// ```rust
/// use komorebi_recorder::analysis::Analysis;
/// use komorebi_recorder::analysis::video::SessionVideo;
/// use komorebi_recorder::testkit::SessionBuilder;
/// use std::sync::Arc;
///
/// let (session, _) = SessionBuilder::new("doc-video-threads").build();
/// let analysis = Arc::new(Analysis::open(&session.dir)?);
///
/// let shared = Arc::clone(&analysis);
/// let displays = std::thread::spawn(move || {
///     SessionVideo::new(&shared.manifest, shared.guard()).displays()
/// })
/// .join()
/// .expect("the thread finished");
/// assert!(displays.is_empty(), "a session recorded without video has no displays with video");
/// # Ok::<(), Box<dyn std::error::Error>>(())
/// ```
pub struct SessionVideo<'a> {
    manifest: &'a Manifest,
    guard: &'a SessionPath,
    assets: RefCell<HashMap<PathBuf, Rc<Asset>>>,
}

impl<'a> SessionVideo<'a> {
    /// Returns the video of the session described by `manifest`, resolving its files through
    /// `guard`. Nothing is opened until a frame is asked for.
    #[must_use]
    pub fn new(manifest: &'a Manifest, guard: &'a SessionPath) -> Self {
        Self {
            manifest,
            guard,
            assets: RefCell::new(HashMap::new()),
        }
    }

    /// Returns the ids of the displays that have any video (a segment with at least one frame), in
    /// ascending order.
    #[must_use]
    pub fn displays(&self) -> Vec<u32> {
        let mut ids: Vec<u32> = self
            .manifest
            .displays
            .iter()
            .filter(|entry| entry.result.frames > 0)
            .map(|entry| entry.result.display.id)
            .collect();
        ids.sort_unstable();
        ids.dedup();
        ids
    }

    fn asset(&self, entry: &DisplayEntry) -> eyre::Result<Rc<Asset>> {
        let path = self.guard.resolve(&entry.result.file)?;
        if let Some(found) = self.assets.borrow().get(&path) {
            return Ok(found.clone());
        }

        let asset = Rc::new(Asset::open(&path)?);
        self.assets.borrow_mut().insert(path, asset.clone());
        Ok(asset)
    }

    /// Returns the frame of `display` on screen at session video time `video_s`, decoded at
    /// `scale`, or `None` if the display had no video then.
    ///
    /// The frame's [`DecodedFrame::shown_s`] is the time it was put on screen, which is never later
    /// than `video_s` and can be earlier. A time at or past the end of the file asks for its last
    /// frame, not for something beyond it. `None` is also returned if the decoder produces no frame
    /// for the moment. The display's video is looked up as [`SessionVideo`]'s module docs describe,
    /// so a moment in the slack at the edge of a segment is answered with that segment's nearest
    /// frame.
    ///
    /// # Errors
    ///
    /// Returns an error if the segment's file leads outside the session (a usage error), cannot be
    /// opened (it is missing or damaged, or the host cannot decode video at all), or cannot be
    /// decoded.
    pub fn frame_at(
        &self,
        display: u32,
        video_s: f64,
        scale: Scale,
    ) -> eyre::Result<Option<DecodedFrame>> {
        let Some(entry) = self.manifest.segment_at(display, video_s) else {
            return Ok(None);
        };

        let asset = self.asset(entry)?;
        // the last moment of a file is the last frame, not past it
        let file_s = (video_s - entry.offset_s)
            .max(0.0)
            .min((asset.duration_s() - 0.002).max(0.0));
        let mut reader = asset.start(file_s, Some(file_s + 0.5), scale)?;
        Ok(reader.next_frame()?.map(|frame| DecodedFrame {
            display,
            segment: entry.result.segment,
            shown_s: entry.offset_s + frame.pts_s,
            frame,
        }))
    }

    /// Returns `true` if and only if `display` has video at session video time `video_s`: a segment
    /// of it covers the moment (see the [module docs](self)). It does not open any file.
    #[must_use]
    pub fn has_video_at(&self, display: u32, video_s: f64) -> bool {
        self.manifest.segment_at(display, video_s).is_some()
    }

    /// Starts decoding the segment of `display` that has video at `video_s`, beginning with the
    /// frame on screen then and running to `to_s` (or the end of the segment). `None` if the
    /// display had no video then.
    ///
    /// The stream ends at the end of the segment even if `to_s` is later: use
    /// [`SessionVideo::scan`] to read across segments. A `to_s` before `video_s` is read as
    /// `video_s`.
    ///
    /// # Errors
    ///
    /// Returns an error under the same conditions as [`SessionVideo::frame_at`].
    pub fn open_at(
        &self,
        display: u32,
        video_s: f64,
        to_s: Option<f64>,
        scale: Scale,
    ) -> eyre::Result<Option<Stream>> {
        let Some(entry) = self.manifest.segment_at(display, video_s) else {
            return Ok(None);
        };

        let asset = self.asset(entry)?;
        let start = (video_s - entry.offset_s)
            .max(0.0)
            .min((asset.duration_s() - 0.002).max(0.0));
        let stop = to_s.map(|to| (to - entry.offset_s).max(start));
        Ok(Some(Stream {
            reader: asset.start(start, stop, scale)?,
            offset_s: entry.offset_s,
            segment: entry.result.segment,
            end_s: entry.end_s.unwrap_or_else(|| self.manifest.video_span().1),
        }))
    }

    /// Decodes every frame of `display` from `from_s` to `to_s`, across segments, in order, calling
    /// `visit` with each until it returns `false`. Returns how many frames were visited, counting
    /// the one `visit` stopped on.
    ///
    /// A frame shown before `from_s` is visited only if it was shown less than half a second before
    /// it: the decoder starts at the frame that was on screen at `from_s`, which can be earlier than
    /// it, and that frame is not visited if it is older than that. No frame shown after `to_s` is
    /// visited. A gap between segments is not visited: there is nothing to decode there.
    ///
    /// # Errors
    ///
    /// Returns an error under the same conditions as [`SessionVideo::frame_at`], for the first
    /// segment that fails. Frames visited before it stay visited.
    pub fn scan(
        &self,
        display: u32,
        from_s: f64,
        to_s: f64,
        scale: Scale,
        visit: &mut dyn FnMut(&DecodedFrame) -> bool,
    ) -> eyre::Result<usize> {
        let mut segments: Vec<&DisplayEntry> = self
            .manifest
            .displays
            .iter()
            .filter(|entry| entry.result.display.id == display && entry.result.frames > 0)
            .collect();
        segments.sort_by(|a, b| a.offset_s.total_cmp(&b.offset_s));
        let (_, session_end) = self.manifest.video_span();

        let mut visited = 0;
        for entry in segments {
            let end = entry.end_s.unwrap_or(session_end);
            if end < from_s || entry.offset_s > to_s {
                continue;
            }

            let asset = self.asset(entry)?;
            let start = (from_s - entry.offset_s).max(0.0);
            let stop = (to_s - entry.offset_s).min(asset.duration_s());
            let mut reader = asset.start(start, Some(stop), scale)?;
            while let Some(frame) = reader.next_frame()? {
                let shown_s = entry.offset_s + frame.pts_s;
                // the decoder starts at the frame on screen at `from_s`, which can be before it
                if shown_s + 1e-6 < from_s && shown_s + 0.5 < from_s {
                    continue;
                }
                if shown_s > to_s {
                    break;
                }

                visited += 1;
                let decoded = DecodedFrame {
                    display,
                    segment: entry.result.segment,
                    shown_s,
                    frame,
                };
                if !visit(&decoded) {
                    return Ok(visited);
                }
            }
        }

        Ok(visited)
    }
}
