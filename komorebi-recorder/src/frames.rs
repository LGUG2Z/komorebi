//! Extracting still frames from a session's video so the exact moment can be looked at.
//!
//! `ffmpeg` is used to extract stills, here, and by the self-test to read its clock strip; it is
//! never used for capture or for playback. Frames in the mp4 only exist where the screen
//! changed, so the frame returned for a timecode is the most recent one at or before it - which
//! is exactly what was on screen at that moment.
//!
//! This is the `frames` command: [`extract`] picks, for each time asked for, the segment of each
//! display that has video then (see [`crate::session::Manifest::segment_at`]), seeks to the right
//! place in that segment's file, and writes a PNG into the session's `frames/` directory. A display
//! with no video at that moment (unplugged, asleep, between segments) is reported, not an error.
//!
//! Nothing here is specific to a host or a komorebi flavor: the video is an mp4 either way, and
//! `ffmpeg` reads it. What differs by OS is only how to get `ffmpeg` ([`ffmpeg_install_hint`]).

use crate::session::Manifest;
use crate::session::Session;
use crate::timecode;
use color_eyre::eyre;
use color_eyre::eyre::WrapErr;
use serde::Serialize;
use std::path::PathBuf;
use std::process::Command;

/// A still frame that [`extract`] wrote.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ExtractedFrame {
    /// The id of the display the frame is of.
    pub display: u32,
    /// The time that was asked for, in seconds of video time.
    pub video_s: f64,
    /// The time within the display's own file.
    pub file_s: f64,
    /// Where the PNG was written: [`frame_path`] of the display and `video_s`.
    pub path: PathBuf,
}

/// Returns where the frame for `video_s` on `display` goes: `frames/frame-<display>-t<video_s>.png`
/// in the session directory.
///
/// The time is written in seconds with three decimals, zero padded to nine characters, so the
/// names of one display's frames sort in time order for any time below 100,000 seconds. The time
/// is the one asked for, not the time of the frame ffmpeg found.
///
/// # Example
///
/// ```rust
/// use komorebi_recorder::frames::frame_path;
/// use komorebi_recorder::session::Session;
/// use std::path::PathBuf;
///
/// let session = Session { dir: PathBuf::from("/s") };
/// let earlier = frame_path(&session, 2, 9.5);
/// let later = frame_path(&session, 2, 12.25);
/// assert_eq!(later, PathBuf::from("/s/frames/frame-2-t00012.250.png"));
/// assert!(earlier < later);
/// ```
#[must_use]
pub fn frame_path(session: &Session, display: u32, video_s: f64) -> PathBuf {
    session.path(format!("frames/frame-{display}-t{video_s:09.3}.png"))
}

/// Returns how to get `ffmpeg` on this OS, as a fragment to put in a sentence: `winget` or `scoop`
/// on Windows, `brew` or the `nix develop` shell on macOS, and the package manager elsewhere.
#[must_use]
pub fn ffmpeg_install_hint() -> &'static str {
    if cfg!(windows) {
        "e.g. `winget install Gyan.FFmpeg` or `scoop install ffmpeg`"
    } else if cfg!(target_os = "macos") {
        "e.g. `brew install ffmpeg`, or use `nix develop`, which provides it"
    } else {
        "with your package manager"
    }
}

/// Returns true if and only if `ffmpeg` can be run: `ffmpeg -version` is found on `PATH` and
/// succeeds.
#[must_use]
pub fn ffmpeg_available() -> bool {
    Command::new("ffmpeg")
        .arg("-version")
        .output()
        .is_ok_and(|output| output.status.success())
}

/// A display that was asked for but had no video at the requested moment.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Skipped {
    /// The id of the display.
    pub display: u32,
    /// The time that was asked for, in seconds of video time.
    pub video_s: f64,
    /// Why there was no video, as a sentence.
    pub reason: String,
}

/// What [`extract`] produced: the frames it wrote and the displays it had to leave out.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Extraction {
    /// The frames written, in the order of the times asked for and, within a time, of the displays'
    /// ids.
    pub frames: Vec<ExtractedFrame>,
    /// The displays that had no video at a time asked for. A time at which every display was
    /// skipped is an error, so each time asked for has at least one frame in `frames`.
    pub skipped: Vec<Skipped>,
}

/// Extracts a frame at each of `times` from every display (or just `display`) that has video then.
///
/// A display that was unplugged, asleep or between segments at that moment is reported in
/// `skipped` rather than failing the others.
///
/// `times` are in seconds of video time (see [`crate::timecode`]). For each, and each display
/// wanted, the segment with video at that time is found with
/// [`Manifest::segment_at`](crate::session::Manifest::segment_at), which allows 50 ms either side
/// of a segment's span. `ffmpeg` is asked for one frame at that time in the segment's file
/// (`file_s`, never below zero), and writes it to [`frame_path`]. Because the video has frames only
/// where the screen changed, that frame is the latest one at or before the time. An existing frame
/// at the same path is overwritten.
///
/// `display` limits the extraction to one display id; `None` is every display recorded.
///
/// # Errors
///
/// Returns an error if
///
/// * `ffmpeg` cannot be run (the message says how to install it for this OS);
/// * the session has no recorded video, or none for `display`;
/// * a time is before the first frame or more than half a second after the end of the recorded
///   video (the message gives the span);
/// * no display has video at a time (the message names the displays);
/// * the `frames/` directory cannot be created, or `ffmpeg` fails or writes no file (the message
///   has what `ffmpeg` said).
///
/// Frames already written for earlier times stay on disk when a later time fails.
///
/// # Example
///
/// ```rust,no_run
/// use komorebi_recorder::frames::extract;
/// use komorebi_recorder::session::Session;
///
/// let session = Session::open("/path/to/recordings/20261026T120000Z")?;
/// let manifest = session.manifest()?;
///
/// // what was on screen at 12.5 s and at 14 s, on every display that had video then
/// let extraction = extract(&session, &manifest, &[12.5, 14.0], None)?;
/// for frame in &extraction.frames {
///     println!("display {} at {:.3} s into its file: {}", frame.display, frame.file_s, frame.path.display());
/// }
/// # Ok::<(), Box<dyn std::error::Error>>(())
/// ```
///
/// This needs `ffmpeg` and a recorded session, so it is not run, and no test covers it: the
/// `frames` command is the check. What it can be checked against without video is the file name
/// ([`frame_path`], and the test `frame_paths_sort_chronologically`) and the segment lookup
/// (`Manifest::segment_at`, in the tests of `timeline`).
pub fn extract(
    session: &Session,
    manifest: &Manifest,
    times: &[f64],
    display: Option<u32>,
) -> eyre::Result<Extraction> {
    if !ffmpeg_available() {
        eyre::bail!(
            "ffmpeg was not found on PATH; install it ({}) to extract frames",
            ffmpeg_install_hint()
        );
    }

    let ids: Vec<u32> = manifest
        .display_ids()
        .into_iter()
        .filter(|id| display.is_none_or(|wanted| wanted == *id))
        .collect();

    if ids.is_empty() {
        eyre::bail!(match display {
            Some(id) => format!("display {id} has no recorded video in this session"),
            None => "this session has no recorded video".to_string(),
        });
    }

    std::fs::create_dir_all(session.path("frames"))?;
    let (_, duration) = manifest.video_span();
    let mut extraction = Extraction {
        frames: Vec::new(),
        skipped: Vec::new(),
    };

    for &video_s in times {
        if video_s < 0.0 || video_s > duration + 0.5 {
            eyre::bail!(
                "{} is outside the recorded video (00:00.000 to {})",
                timecode::format(video_s),
                timecode::format(duration)
            );
        }

        let before = extraction.frames.len();
        for &id in &ids {
            let Some(entry) = manifest.segment_at(id, video_s) else {
                extraction.skipped.push(Skipped {
                    display: id,
                    video_s,
                    reason: "no video for this display at that moment (it was unplugged, asleep, \
                             or its stream had stopped)"
                        .to_string(),
                });
                continue;
            };

            let file_s = (video_s - entry.offset_s).max(0.0);
            let path = frame_path(session, id, video_s);
            let output = Command::new("ffmpeg")
                .args(["-y", "-v", "error", "-ss", &format!("{file_s:.3}"), "-i"])
                .arg(session.path(&entry.result.file))
                .args(["-frames:v", "1"])
                .arg(&path)
                .output()
                .wrap_err("could not run ffmpeg")?;

            if !output.status.success() || !path.is_file() {
                eyre::bail!(
                    "ffmpeg could not extract a frame at {} from {}: {}",
                    timecode::format(file_s),
                    entry.result.file,
                    String::from_utf8_lossy(&output.stderr).trim()
                );
            }

            extraction.frames.push(ExtractedFrame {
                display: id,
                video_s,
                file_s,
                path,
            });
        }

        if extraction.frames.len() == before {
            eyre::bail!(
                "no display has video at {}: {}",
                timecode::format(video_s),
                extraction
                    .skipped
                    .iter()
                    .filter(|skipped| skipped.video_s == video_s)
                    .map(|skipped| format!("display {}", skipped.display))
                    .collect::<Vec<_>>()
                    .join(", ")
            );
        }
    }

    Ok(extraction)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn frame_paths_sort_chronologically() {
        let session = Session {
            dir: PathBuf::from("/s"),
        };
        let a = frame_path(&session, 2, 9.5);
        let b = frame_path(&session, 2, 12.25);
        assert_eq!(a, PathBuf::from("/s/frames/frame-2-t00009.500.png"));
        assert_eq!(b, PathBuf::from("/s/frames/frame-2-t00012.250.png"));
        assert!(a < b);
    }
}
