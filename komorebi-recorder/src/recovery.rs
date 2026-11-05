//! Rebuilding a session whose recorder died before it could finish.
//!
//! While recording, everything is written so that a crash, a `kill -9`, a power loss or a
//! panic loses as little as possible: video is fragmented mp4 (playable up to the last fragment),
//! the frame log and every raw source are flushed continuously, and the manifest is checkpointed
//! every few seconds with `status: "recording"`. Recovery turns that checkpoint into a normal,
//! queryable session: it closes the segments that were still open using the frame log, sets a
//! stop time from the latest evidence on disk, and marks the session `recovered`.
//!
//! This is what the `reindex` command does ([`reindex_session`]): recover, rebuild the timeline,
//! rewrite the session's `README.md`. A session that was finished properly is only re-indexed.
//! Recovery is the same on every host: the frame log and the manifest are the same on both, and
//! only the question "how long is this video file" is the host's (see [`recover`]).
//!
//! # What recovery trusts
//!
//! | Question | Answered from | Because |
//! |---|---|---|
//! | how much video survived | the video file's duration, if the host can read it | fragments are cut on a timer and the frame log is flushed on another, so either can be ahead |
//! | how many frames, and when | the frame log, up to the end the file gives | the frame log is authoritative for the time of each frame |
//! | when the recording ended | the latest of the last anchor, the end of the video, and the last event | nothing later is known to have happened |
//!
//! Without a readable video file, recovery falls back to the frame log and says that the file may
//! be unplayable. The end of a recovered session is approximate, and a little of its end may be
//! missing: the manifest gets a warning saying so.
//!
//! # The frame log
//!
//! `frames-<display>[-<n>].csv` is written by the capture, one per video segment. It is UTF-8
//! text, comma separated, with a header and one row for each frame:
//!
//! ```text
//! index,pts_mach_ns,received_mach_ns
//! 0,1000200000000,1000210000000
//! 1,1007000000000,1007010000000
//! -,1007600000000,1007600000000
//! ```
//!
//! | Column | Is |
//! |---|---|
//! | `index` | the frame's number in the segment, counting from 0, over real frames only; `-` for a keep-alive row |
//! | `pts_mach_ns` | the frame's time on the master clock, in nanoseconds: when it was shown |
//! | `received_mach_ns` | when the recorder received it, on the same clock; for a keep-alive row, when it was written |
//!
//! A keep-alive row (`index` is `-`) is the recorder re-submitting the last frame on an idle display
//! so that fragments keep being cut. It is not a new frame, but it shows the segment lasted until
//! its time. A recorder that died while writing can leave the last row cut short. [`read_frame_log`]
//! reads up to the tear.
//!
//! # Example
//!
//! ```rust
//! use komorebi_recorder::recovery::read_frame_log;
//!
//! let dir = std::env::temp_dir().join(format!("komorebi-recorder-doc-recovery-{}", std::process::id()));
//! std::fs::create_dir_all(&dir)?;
//! let path = dir.join("frames-2.csv");
//!
//! // what `kill -9` leaves behind: two frames, a keep-alive, and a last row cut short
//! std::fs::write(
//!     &path,
//!     "index,pts_mach_ns,received_mach_ns\n0,1000,1010\n1,2000,2010\n-,2600,2600\n2,3000",
//! )?;
//!
//! let log = read_frame_log(&path);
//! assert_eq!(log.frames, 2, "the keep-alive and the torn row are not frames");
//! assert_eq!(log.first_pts_mach_ns, Some(1000));
//! assert_eq!(log.last_pts_mach_ns, Some(2600), "the segment lasted until the keep-alive");
//! # std::fs::remove_dir_all(&dir)?;
//! # Ok::<(), Box<dyn std::error::Error>>(())
//! ```

use crate::capture::EndReason;
use crate::clock::ClockMap;
use crate::session::Moment;
use crate::session::STATUS_RECORDING;
use crate::session::STATUS_RECOVERED;
use crate::session::Session;
use crate::sources::komorebi_events::EventRow;
use crate::timeline;
use crate::timeline::BuildStats;
use color_eyre::eyre;
use std::path::Path;

/// What the frame log of a segment says about it.
///
/// Rows whose index is `-` are keep-alive re-submissions of the last frame on an idle display: they
/// count towards how long the segment lasted but are not new frames.
///
/// The default is a log with no frames, which is also what reading a log that is missing gives.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct FrameLog {
    /// How many real frames the log has: rows that are not keep-alives.
    pub frames: u64,
    /// The time of the first real frame, in nanoseconds on the master clock. `None` if the log has
    /// no frames.
    pub first_pts_mach_ns: Option<u64>,
    /// The time of the last row, a keep-alive or a frame, in nanoseconds on the master clock. It
    /// is when the segment was last known to be alive. `None` if the log has no rows.
    pub last_pts_mach_ns: Option<u64>,
}

/// Reads a `frames-*.csv`, ignoring the header and any torn or malformed row (the last row of a
/// log that was being written when the recorder died).
///
/// A row counts only if it has exactly three fields, and the second and third are whole numbers.
/// A missing or unreadable file is a log with no frames, not an error: the caller decides what
/// that means (recovery says that no frames had been recorded). See the [module docs](self) for
/// the format.
///
/// # Example
///
/// ```rust
/// use komorebi_recorder::recovery::{read_frame_log, FrameLog};
///
/// // a log that does not exist has no frames
/// let missing = std::env::temp_dir().join("komorebi-recorder-doc-no-such-frames.csv");
/// assert_eq!(read_frame_log(&missing), FrameLog::default());
/// ```
#[must_use]
pub fn read_frame_log(path: &Path) -> FrameLog {
    read_frame_log_until(path, u64::MAX)
}

/// Like [`read_frame_log`], but ignoring rows later than `until_pts_mach_ns`: the part of the
/// log that never made it into the video.
///
/// Reading stops at the first row whose `pts_mach_ns` is greater than `until_pts_mach_ns`, so it
/// relies on the rows being in time order, as a capture writes them.
///
/// # Example
///
/// ```rust
/// use komorebi_recorder::recovery::read_frame_log_until;
///
/// let dir = std::env::temp_dir().join(format!("komorebi-recorder-doc-until-{}", std::process::id()));
/// std::fs::create_dir_all(&dir)?;
/// let path = dir.join("frames-1.csv");
/// std::fs::write(
///     &path,
///     "index,pts_mach_ns,received_mach_ns\n0,1000,1010\n1,2000,2010\n2,3000,3010\n",
/// )?;
///
/// // the video only holds what was shown up to 2500: the third frame did not make it
/// let log = read_frame_log_until(&path, 2_500);
/// assert_eq!(log.frames, 2);
/// assert_eq!(log.last_pts_mach_ns, Some(2000));
/// # std::fs::remove_dir_all(&dir)?;
/// # Ok::<(), Box<dyn std::error::Error>>(())
/// ```
#[must_use]
pub fn read_frame_log_until(path: &Path, until_pts_mach_ns: u64) -> FrameLog {
    let Ok(text) = std::fs::read_to_string(path) else {
        return FrameLog::default();
    };

    let mut log = FrameLog::default();
    for line in text.lines().skip(1) {
        let mut fields = line.split(',');
        let (Some(index), Some(pts), Some(received), None) =
            (fields.next(), fields.next(), fields.next(), fields.next())
        else {
            continue;
        };

        let (Ok(pts), Ok(_)) = (pts.parse::<u64>(), received.parse::<u64>()) else {
            continue;
        };

        if pts > until_pts_mach_ns {
            break;
        }

        if index != "-" {
            // a keep-alive can never be the first row, so the first row is a real frame
            log.frames += 1;
            log.first_pts_mach_ns.get_or_insert(pts);
        }
        log.last_pts_mach_ns = Some(pts);
    }

    log
}

/// One frame at the capture frame rate cap, used to give the last recovered frame a duration.
const FRAME_NS: u64 = 33_333_333;

/// What [`reindex_session`] did.
#[derive(Debug)]
pub struct Reindexed {
    /// Notes describing a recovery, empty if the session had been finished properly.
    pub recovered: Vec<String>,
    /// What went into the rebuilt timeline index: how many entries, and how many from each source.
    pub stats: BuildStats,
}

/// Closes out `session` if its recorder died, then rebuilds its timeline and README from the raw
/// files.
///
/// This is everything `komorebi-recorder reindex` does, shared so that other commands can bring a
/// session up to date the same way. It is [`recover`], then [`timeline::build`], then rewriting the
/// session's `README.md` from the manifest. It is safe to run again, on a recovered session or a
/// finished one: only the first pass recovers anything, and rebuilding the timeline from the same
/// raw files gives the same index.
///
/// Like [`recover`], this does not check that the session's recorder is gone.
///
/// # Errors
///
/// Returns an error if
///
/// * the manifest cannot be read, parsed, or is from a newer version of the recorder than this one
///   (see [`recover`]);
/// * the recovered manifest cannot be written (see [`recover`]);
/// * the timeline cannot be rebuilt (see [`timeline::build`]);
/// * the `README.md` cannot be written.
///
/// A failure after the recovery step leaves the session recovered, without a rebuilt timeline: run
/// it again to rebuild.
///
/// # Example
///
/// ```rust,no_run
/// use komorebi_recorder::recovery::reindex_session;
/// use komorebi_recorder::session::Session;
///
/// let session = Session::open("/path/to/recordings/20261026T120000Z")?;
/// let reindexed = reindex_session(&session)?;
/// for note in &reindexed.recovered {
///     println!("recovered: {note}");
/// }
/// println!("indexed {} records", reindexed.stats.entries);
/// # Ok::<(), Box<dyn std::error::Error>>(())
/// ```
///
/// This needs a session directory, so it is not run. The steps it chains are covered by the tests
/// `an_interrupted_session_is_closed_out_from_its_frame_logs` (recovery) and
/// `reindex_is_idempotent_and_recovers_a_deleted_timeline` (the rebuild, in `timeline`).
pub fn reindex_session(session: &Session) -> eyre::Result<Reindexed> {
    let recovered = recover(session)?;
    let stats = timeline::build(session)?;
    session.write_readme(&session.manifest()?)?;
    Ok(Reindexed { recovered, stats })
}

/// Recovers `session` if it was left unfinished, asking the host how long each video file really
/// is.
///
/// Returns notes describing what was done, empty if the session was already complete. A session
/// needs recovery if and only if its manifest says `recording`: the recorder checkpoints it that
/// way every few seconds, and writes `complete` when it finishes. For such a session, each video
/// segment that has no end reason is closed: its frame count, its end, and an end reason of
/// [`EndReason::Interrupted`] are written into the manifest, from the frame log and the file's
/// duration; segments that already ended are left alone. The recording's stop time is set to the
/// latest evidence on disk, the status becomes `recovered`, and a warning says the end is
/// approximate.
///
/// The duration of a video file is [`crate::platform::capture::media_duration_s`]: what the host's
/// media framework reads from the file, which is `AVFoundation` on macOS and Media Foundation on
/// Windows. It is the truth about how much video survived, because the file and the frame log are
/// flushed on separate timers. A host that cannot read it (the `unsupported` host, or a file the
/// framework cannot open) makes recovery fall back to the frame log.
///
/// This does not check that the recorder is really gone. A caller that may be handed a session
/// still being recorded asks [`crate::lock::recorder`] first.
///
/// # Errors
///
/// Returns an error if the manifest cannot be read, cannot be parsed, or was written by a newer
/// version of the recorder (schema version), or if the recovered manifest cannot be written.
/// Nothing is changed on disk before the final write.
///
/// # Example
///
/// ```rust,no_run
/// use komorebi_recorder::recovery::recover;
/// use komorebi_recorder::session::Session;
///
/// let session = Session::open("/path/to/recordings/20261026T120000Z")?;
/// for note in recover(&session)? {
///     println!("{note}");
/// }
/// # Ok::<(), Box<dyn std::error::Error>>(())
/// ```
///
/// This reads real video files, so it is not run. [`recover_with`] takes the duration lookup as an
/// argument, and is what the tests use: see its example.
pub fn recover(session: &Session) -> eyre::Result<Vec<String>> {
    recover_with(session, &crate::platform::capture::media_duration_s)
}

/// [`recover`] with the file-duration lookup supplied, so it can be tested without video files.
///
/// `duration_of` is given the path of a segment's video file, and returns its duration in seconds,
/// or `None` if it cannot say. A duration of zero or less is taken as `None`.
///
/// # Errors
///
/// The same as [`recover`].
///
/// # Example
///
/// ```rust
/// use chrono::Utc;
/// use komorebi_recorder::clock::Anchor;
/// use komorebi_recorder::recovery::recover_with;
/// use komorebi_recorder::session::{Manifest, Moment, Session, STATUS_RECOVERED, STATUS_RECORDING};
/// use std::path::Path;
///
/// let dir = std::env::temp_dir().join(format!("komorebi-recorder-doc-recover-{}", std::process::id()));
/// let _ = std::fs::remove_dir_all(&dir);
/// let session = Session::create(Some(dir.clone()), Utc::now())?;
///
/// // the checkpoint of a recorder that died: still `recording`, with no displays recorded yet
/// let start = Moment { mach_ns: 1_000_000_000_000, wall_us: 1_800_000_000_000_000 };
/// let manifest = Manifest::new(
///     start,
///     vec![Anchor { wall_us: start.wall_us, mach_ns: start.mach_ns }],
/// );
/// assert_eq!(manifest.status, STATUS_RECORDING);
/// session.write_manifest(&manifest)?;
///
/// let notes = recover_with(&session, &|_: &Path| None)?;
/// assert_eq!(notes, ["session marked as recovered"]);
///
/// let recovered = session.manifest()?;
/// assert_eq!(recovered.status, STATUS_RECOVERED);
/// assert!(recovered.warnings[0].contains("stopped unexpectedly"));
///
/// // a second pass has nothing to do
/// assert!(recover_with(&session, &|_: &Path| None)?.is_empty());
/// # std::fs::remove_dir_all(&dir)?;
/// # Ok::<(), Box<dyn std::error::Error>>(())
/// ```
///
/// Segments with frame logs, torn rows and video durations are covered by the tests
/// `an_interrupted_session_is_closed_out_from_its_frame_logs` and
/// `an_unreadable_video_file_falls_back_to_the_frame_log_and_says_so`.
pub fn recover_with(
    session: &Session,
    duration_of: &dyn Fn(&Path) -> Option<f64>,
) -> eyre::Result<Vec<String>> {
    let mut manifest = session.manifest()?;
    if manifest.status != STATUS_RECORDING {
        return Ok(Vec::new());
    }

    let mut notes = Vec::new();
    let mut results: Vec<_> = manifest
        .displays
        .iter()
        .map(|entry| entry.result.clone())
        .collect();

    for result in &mut results {
        if result.end_reason.is_some() {
            continue;
        }

        let frames_path = session.path(&result.frames_file);
        let whole_log = read_frame_log(&frames_path);
        let t0 = result.video_t0_mach_ns.or(whole_log.first_pts_mach_ns);

        // The video file is the truth about how much was kept: fragments are cut on a timer, and
        // the frame log is flushed on another, so either can be ahead of the other. Without a
        // readable file, fall back to the log.
        let file_duration = duration_of(&session.path(&result.file)).filter(|d| *d > 0.0);
        let end = match (t0, file_duration) {
            (Some(t0), Some(duration)) => Some(t0 + (duration * 1e9) as u64),
            _ => whole_log.last_pts_mach_ns.map(|last| last + FRAME_NS),
        };

        let log = end.map_or(whole_log, |end| read_frame_log_until(&frames_path, end));
        result.frames = log.frames;
        result.video_t0_mach_ns = t0;
        result.video_end_mach_ns = end;
        result.end_reason = Some(EndReason::Interrupted);

        notes.push(match (log.frames, file_duration) {
            (0, _) => format!("{}: no frames had been recorded", result.file),
            (frames, Some(duration)) => format!(
                "{}: recovered {frames} frames, {duration:.1}s of video from the file",
                result.file
            ),
            (frames, None) => format!(
                "{}: {frames} frames are in the frame log but the video file could not be read, so it may be unplayable",
                result.file
            ),
        });
    }

    manifest.apply_segments(results);

    // the recording ended no earlier than the latest thing we can see on disk
    let clock = ClockMap::new(manifest.anchors.clone());
    let mut latest = manifest
        .anchors
        .iter()
        .map(|a| a.mach_ns)
        .max()
        .unwrap_or(manifest.start.mach_ns);
    latest = latest.max(manifest.video_end_mach_ns.unwrap_or(0));
    if let Ok(text) = std::fs::read_to_string(session.path("events.jsonl")) {
        let last_event = text
            .lines()
            .filter_map(|line| serde_json::from_str::<EventRow>(line).ok())
            .map(|row| row.recv_mach_ns)
            .max();
        latest = latest.max(last_event.unwrap_or(0));
    }

    let wall_us = clock.mach_to_wall(latest).unwrap_or(manifest.start.wall_us);
    manifest.stop = Some(Moment {
        mach_ns: latest,
        wall_us,
    });

    manifest.status = STATUS_RECOVERED.to_string();
    manifest.warnings.push(
        "the recorder stopped unexpectedly (crash, kill or power loss); this session was recovered \
         from what was on disk, so its end is approximate and a little of the end may be missing"
            .to_string(),
    );
    notes.push("session marked as recovered".to_string());

    session.write_manifest(&manifest)?;
    Ok(notes)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::capture::Codec;
    use crate::capture::DisplayDescriptor;
    use crate::capture::DisplayResult;
    use crate::capture::LatencyStats;
    use crate::clock::Anchor;
    use crate::session::Manifest;
    use chrono::Utc;

    fn temp(name: &str) -> std::path::PathBuf {
        std::env::temp_dir().join(format!(
            "komorebi-recorder-recovery-{name}-{}",
            std::process::id()
        ))
    }

    fn live_segment(display: u32, segment: u32, t0: u64) -> DisplayResult {
        DisplayResult {
            display: DisplayDescriptor {
                id: display,
                frame: [0.0, 0.0, 100.0, 100.0],
                width_points: 100,
                height_points: 100,
                asleep: false,
                scale: 1.0,
            },
            segment,
            scale: 1.0,
            width_px: 100,
            height_px: 100,
            codec: Codec::Hevc,
            file: crate::capture::video_file_name(display, segment),
            frames_file: crate::capture::frames_file_name(display, segment),
            video_t0_mach_ns: Some(t0),
            video_end_mach_ns: None,
            end_reason: None,
            frames: 3,
            dropped: 0,
            idle_callbacks: 0,
            latency: LatencyStats::default(),
            error: None,
        }
    }

    #[test]
    fn a_frame_log_with_a_torn_last_row_is_read_up_to_the_tear() {
        let dir = temp("csv");
        std::fs::create_dir_all(&dir).expect("dir");
        let path = dir.join("frames-2.csv");
        // what `kill -9` left behind in a real recording: the last row cut mid-field
        std::fs::write(
            &path,
            "index,pts_mach_ns,received_mach_ns\n0,1000,1010\n1,2000,2010\n2,3000",
        )
        .expect("write");

        let log = read_frame_log(&path);
        assert_eq!(log.frames, 2, "the torn row is not counted");
        assert_eq!(log.first_pts_mach_ns, Some(1000));
        assert_eq!(log.last_pts_mach_ns, Some(2000));

        assert_eq!(
            read_frame_log(&dir.join("missing.csv")),
            FrameLog::default()
        );
    }

    #[test]
    fn keep_alive_rows_extend_the_segment_but_are_not_frames() {
        let dir = temp("keepalive");
        std::fs::create_dir_all(&dir).expect("dir");
        let path = dir.join("frames-1.csv");
        std::fs::write(
            &path,
            "index,pts_mach_ns,received_mach_ns\n0,1000,1010\n1,2000,2010\n-,2600,2600\n-,3100,3100\n",
        )
        .expect("write");

        let log = read_frame_log(&path);
        assert_eq!(log.frames, 2, "only the real frames");
        assert_eq!(log.first_pts_mach_ns, Some(1000));
        assert_eq!(
            log.last_pts_mach_ns,
            Some(3100),
            "the segment lasted until the last keep-alive"
        );
    }

    #[test]
    fn an_interrupted_session_is_closed_out_from_its_frame_logs() {
        let dir = temp("session");
        let _ = std::fs::remove_dir_all(&dir);
        let session = Session::create(Some(dir.clone()), Utc::now()).expect("create");

        // a checkpoint taken 10s in: one finished segment, one still open when the recorder died
        let start = Moment {
            mach_ns: 1_000_000_000_000,
            wall_us: 1_800_000_000_000_000,
        };
        let mut manifest = Manifest::new(
            start,
            vec![
                Anchor {
                    wall_us: start.wall_us,
                    mach_ns: start.mach_ns,
                },
                Anchor {
                    wall_us: start.wall_us + 5_000_000,
                    mach_ns: start.mach_ns + 5_000_000_000,
                },
            ],
        );
        let mut finished = live_segment(1, 1, 1_000_100_000_000);
        finished.video_end_mach_ns = Some(1_003_000_000_000);
        finished.end_reason = Some(EndReason::DisplayDisconnected);
        manifest.apply_segments(vec![finished, live_segment(2, 1, 1_000_200_000_000)]);
        assert_eq!(manifest.status, STATUS_RECORDING);
        session.write_manifest(&manifest).expect("write");

        // the open segment's frame log, as left on disk (torn last row)
        std::fs::write(
            session.path("frames-2.csv"),
            "index,pts_mach_ns,received_mach_ns\n0,1000200000000,1\n1,1007000000000,2\n2,1008000000000,3\n3,1009000",
        )
        .expect("csv");
        // an event shortly after the last frame
        std::fs::write(
            session.path("events.jsonl"),
            "{\"seq\":1,\"recv_wall_us\":1,\"recv_mach_ns\":1009500000000}\n",
        )
        .expect("events");

        // the video file on disk holds 7.5s of the 8s the frame log covers
        let notes = recover_with(&session, &|path: &Path| {
            path.ends_with("video-2.mp4").then_some(7.5)
        })
        .expect("recover");
        assert!(
            notes
                .iter()
                .any(|note| note.contains("video-2.mp4: recovered 2 frames, 7.5s")),
            "{notes:?}"
        );

        let recovered = session.manifest().expect("manifest");
        assert_eq!(recovered.status, STATUS_RECOVERED);
        assert!(
            recovered
                .warnings
                .iter()
                .any(|w| w.contains("stopped unexpectedly"))
        );

        let open = recovered
            .displays
            .iter()
            .find(|entry| entry.result.display.id == 2)
            .expect("segment");
        assert_eq!(open.result.end_reason, Some(EndReason::Interrupted));
        // the 3rd frame (at +8s) is past the 7.5s of video that survived, so it is not counted
        assert_eq!(open.result.frames, 2);
        assert_eq!(
            open.result.video_end_mach_ns,
            Some(1_000_200_000_000 + 7_500_000_000)
        );
        assert!(open.end_s.is_some());

        let closed = recovered
            .displays
            .iter()
            .find(|entry| entry.result.display.id == 1)
            .expect("segment");
        assert_eq!(
            closed.result.end_reason,
            Some(EndReason::DisplayDisconnected),
            "left alone"
        );

        // the stop time is the latest evidence: the event, not the stale 5s anchor
        assert_eq!(recovered.stop.expect("stop").mach_ns, 1_009_500_000_000);
        assert!(recovered.video_end_mach_ns.is_some());

        // and a second pass does nothing
        assert!(
            recover_with(&session, &|_: &Path| None)
                .expect("again")
                .is_empty()
        );
    }

    #[test]
    fn an_unreadable_video_file_falls_back_to_the_frame_log_and_says_so() {
        let dir = temp("unreadable");
        let _ = std::fs::remove_dir_all(&dir);
        let session = Session::create(Some(dir), Utc::now()).expect("create");
        let start = Moment {
            mach_ns: 1_000_000_000_000,
            wall_us: 1_800_000_000_000_000,
        };
        let mut manifest = Manifest::new(
            start,
            vec![Anchor {
                wall_us: start.wall_us,
                mach_ns: start.mach_ns,
            }],
        );
        manifest.apply_segments(vec![live_segment(2, 1, 1_000_100_000_000)]);
        session.write_manifest(&manifest).expect("write");
        std::fs::write(
            session.path("frames-2.csv"),
            "index,pts_mach_ns,received_mach_ns\n0,1000100000000,1\n1,1002100000000,2\n",
        )
        .expect("csv");

        let notes = recover_with(&session, &|_: &Path| None).expect("recover");
        assert!(
            notes.iter().any(|note| note.contains("could not be read")),
            "{notes:?}"
        );

        let recovered = session.manifest().expect("manifest");
        assert_eq!(recovered.displays[0].result.frames, 2);
        assert_eq!(
            recovered.displays[0].result.video_end_mach_ns,
            Some(1_002_100_000_000 + FRAME_NS)
        );
    }

    #[test]
    fn a_complete_session_is_left_alone() {
        let dir = temp("complete");
        let _ = std::fs::remove_dir_all(&dir);
        let session = Session::create(Some(dir), Utc::now()).expect("create");
        let start = Moment {
            mach_ns: 1,
            wall_us: 1,
        };
        let mut manifest = Manifest::new(start, vec![]);
        manifest.status = crate::session::STATUS_COMPLETE.to_string();
        session.write_manifest(&manifest).expect("write");

        assert!(
            recover_with(&session, &|_: &Path| None)
                .expect("recover")
                .is_empty()
        );
        assert_eq!(session.manifest().expect("manifest"), manifest);
    }
}
