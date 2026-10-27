//! Things that happened to the *recording environment* while recording: a display was unplugged,
//! the machine slept and woke, the screen locked, a log stream was restarted, komorebi restarted.
//!
//! They are appended to `recorder.jsonl` the moment they are noticed (and flushed, so a crash
//! loses nothing) and indexed into the timeline as the `recorder` source. That puts "display 1
//! disconnected" or "woke after 42 s asleep" right next to the log lines and OS trace around it,
//! which is usually the first thing needed to explain a gap or a strange layout.
//!
//! The same on both hosts: which incidents a host can produce differs (the OS announces a lid
//! closing on Windows, and nothing does on macOS), but a recording from either is read the same
//! way, and the file does not say which host wrote it.
//!
//! # Who writes what
//!
//! A part of the recorder that notices something builds a [`Draft`] with a [`kind`] and a
//! sentence, and the recording loop hands it to the [`IncidentLog`], which timestamps and writes
//! it. The kinds are produced by:
//!
//! | Kinds | Noticed by |
//! |---|---|
//! | [`kind::CAPTURE_STARTED`], [`kind::CAPTURE_STOPPED`], [`kind::CAPTURE_FAILED`], [`kind::CAPTURE_RECOVERED`], the `display-*` kinds | the capture manager, which follows the displays |
//! | [`kind::SUSPEND`], [`kind::WAKE`], [`kind::LID_CLOSED`], [`kind::LID_OPENED`], [`kind::DISPLAY_ASLEEP`], [`kind::DISPLAY_AWAKE`] | `PowerTracker`, from what the OS announces |
//! | [`kind::WAKE`], [`kind::CLOCK_STEP`] | the recording loop, from the clocks disagreeing (see [`crate::clock::JumpDetector`]) |
//! | the `screen-*` and `session-*` kinds | `SessionTracker`, polling the session |
//! | [`kind::KOMOREBI_EXITED`], [`kind::KOMOREBI_STARTED`] | the recording loop, following komorebi's process |
//! | [`kind::LOG_STREAM_RESTARTED`], [`kind::LOG_STREAM_LOST`] | the OS trace |
//! | [`kind::RECORDER`] | the recorder itself, about itself |
//!
//! # The file
//!
//! `recorder.jsonl` is UTF-8, one JSON object per line ([`Incident`]), in the order they were
//! written. That is not always the order they happened in, because a [`Draft`] can be dated before
//! it was noticed. The timeline index sorts them in with every other source by `mach_ns`.
//!
//! ```json
//! {"mach_ns":104500000000,"wall_us":1800000004500000,"kind":"display-disconnected","detail":"display 1 disconnected","display":1}
//! ```
//!
//! # Example
//!
//! ```rust
//! use komorebi_recorder::incidents::{kind, Draft, Incident, IncidentLog, INCIDENTS_FILE};
//!
//! let dir = std::env::temp_dir().join(format!("komorebi-recorder-doc-incidents-{}", std::process::id()));
//! std::fs::create_dir_all(&dir)?;
//!
//! let mut log = IncidentLog::create(&dir)?;
//! let incident = log.record(
//!     Draft::new(kind::DISPLAY_DISCONNECTED, "display 1 disconnected").display(1),
//! );
//!
//! // the line is on disk already, without a flush or a drop
//! let text = std::fs::read_to_string(dir.join(INCIDENTS_FILE))?;
//! let on_disk: Incident = serde_json::from_str(text.trim_end())?;
//! assert_eq!(on_disk, incident);
//! assert_eq!(on_disk.kind, "display-disconnected");
//! assert_eq!(on_disk.display, Some(1));
//! # std::fs::remove_dir_all(&dir)?;
//! # Ok::<(), Box<dyn std::error::Error>>(())
//! ```

use crate::clock;
use serde::Deserialize;
use serde::Serialize;
use std::collections::VecDeque;
use std::fs::File;
use std::io::BufWriter;
use std::io::Write;
use std::path::Path;

/// The file in a session directory that incidents are appended to: `recorder.jsonl`.
///
/// See the [module docs](self) for the format of a line, which is [`Incident`].
pub const INCIDENTS_FILE: &str = "recorder.jsonl";
/// The name of the timeline source that incidents are indexed as: `recorder`.
///
/// It is the `source` of every incident's entry in the timeline index, and the name a query's
/// source filter uses to ask for them.
pub const SOURCE_RECORDER: &str = "recorder";

/// The kinds of incident, as written to `recorder.jsonl`.
///
/// Each constant is the `kind` of an [`Incident`], spelled in lowercase words joined by hyphens.
/// The `kind` is a plain string in the file and in [`Incident`], and a [`Draft`] takes any
/// `&'static str`: these constants are the kinds this crate writes, and keep their spellings in one
/// place, but nothing stops a writer passing another string, or a reader meeting one.
pub mod kind {
    /// A capture of a display started, as a new segment of its video. `display` is set.
    pub const CAPTURE_STARTED: &str = "capture-started";
    /// A capture segment was finalized for a reason that is not about the display itself.
    ///
    /// Its stream stalled or failed, or the machine woke and every capture is being replaced. The
    /// detail says why, and which segment file it was.
    pub const CAPTURE_STOPPED: &str = "capture-stopped";
    /// A capture could not be started. It is tried again with growing delays, and this is
    /// reported once per outage.
    pub const CAPTURE_FAILED: &str = "capture-failed";
    /// A capture started again, as a new segment, after one that had stopped on a stream failure
    /// or had failed to start.
    pub const CAPTURE_RECOVERED: &str = "capture-recovered";
    /// A display appeared that had not been seen during this recording (a monitor was plugged
    /// in, or a laptop was docked). Its capture starts after a short settling time.
    pub const DISPLAY_CONNECTED: &str = "display-connected";
    /// A display went away (unplugged, undocked, lid closed), and its video segment was finalized
    /// on the spot.
    pub const DISPLAY_DISCONNECTED: &str = "display-disconnected";
    /// A display's size changed, so its segment was closed and a new one begun.
    pub const DISPLAY_RECONFIGURED: &str = "display-reconfigured";
    /// A display is still attached but asleep or turned off. A sleeping display counts as absent:
    /// its segment was finalized, and it is not captured until it wakes.
    pub const DISPLAY_ASLEEP: &str = "display-asleep";
    /// A display is available again after being asleep or gone: one that had been seen before
    /// returned, or the OS said the displays were turned on.
    pub const DISPLAY_AWAKE: &str = "display-awake";
    /// The machine is about to sleep or hibernate, as the OS announced it. Nothing is recorded
    /// until it wakes.
    pub const SUSPEND: &str = "suspend";
    /// The machine woke, either as the OS announced it or as the clocks showed it (the wall clock
    /// moved on while the master clock did not). The detail says how long the machine slept.
    pub const WAKE: &str = "wake";
    /// The laptop lid was closed, as the OS announced it.
    pub const LID_CLOSED: &str = "lid-closed";
    /// The laptop lid was opened, as the OS announced it.
    pub const LID_OPENED: &str = "lid-opened";
    /// The wall clock was stepped backwards. (A step forwards cannot be told from a sleep, and is
    /// reported as [`WAKE`].)
    pub const CLOCK_STEP: &str = "clock-step";
    /// The screen was locked. Video may pause or restart until it is unlocked.
    pub const SCREEN_LOCKED: &str = "screen-locked";
    /// The screen was unlocked.
    pub const SCREEN_UNLOCKED: &str = "screen-unlocked";
    /// Another user's session came to the front (fast user switching), so this one is in the
    /// background and cannot be captured.
    pub const SESSION_INACTIVE: &str = "session-inactive";
    /// This user's session is in front again.
    pub const SESSION_ACTIVE: &str = "session-active";
    /// The komorebi process exited. Its events and OS trace stop until it runs again.
    pub const KOMOREBI_EXITED: &str = "komorebi-exited";
    /// A komorebi process started while recording, and the recorder follows it.
    pub const KOMOREBI_STARTED: &str = "komorebi-started";
    /// An OS trace stream exited and was started again. Events in the gap between are missing.
    pub const LOG_STREAM_RESTARTED: &str = "log-stream-restarted";
    /// An OS trace stream, or one of the taps that make it up, could not be started or restarted:
    /// the OS trace is incomplete.
    pub const LOG_STREAM_LOST: &str = "log-stream-lost";
    /// A note from the recorder itself: that recording started, or that it could not do something
    /// (list the displays, subscribe to komorebi, trace it).
    pub const RECORDER: &str = "recorder";
}

/// One line of `recorder.jsonl`: something that happened to the recording environment, and when.
///
/// An incident is written by [`IncidentLog::record`], with the time it happened on both clocks.
/// The fields are, in the order they are written, `mach_ns`, `wall_us`, `kind`, `detail`, and
/// `display` when there is one.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Incident {
    /// When it happened, in nanoseconds on the master clock (see [`crate::clock`]). That can be
    /// earlier than when it was noticed.
    pub mach_ns: u64,
    /// When it happened by the wall clock, in microseconds since the Unix epoch, worked out from
    /// how long ago `mach_ns` was.
    pub wall_us: i64,
    /// What kind of incident, one of the constants in [`kind`].
    pub kind: String,
    /// A sentence saying what happened, for a person.
    pub detail: String,
    /// The display the incident is about, when it is about one. Left out of the file when there is
    /// none, and `None` when it is not there.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub display: Option<u32>,
}

/// An incident that has not been timestamped yet. The time it *happened* can be earlier than the
/// time it was noticed (a stream that died, a wake detected a moment later).
///
/// A draft is built with [`Draft::new`] and the builder methods [`display`](Draft::display) and
/// [`at`](Draft::at), then handed to [`IncidentLog::record`], which gives it its times.
///
/// # Example
///
/// ```rust
/// use komorebi_recorder::incidents::{kind, Draft};
///
/// let draft = Draft::new(kind::CAPTURE_STOPPED, "the stream stalled")
///     .display(2)
///     .at(5_000_000_000);
///
/// assert_eq!(draft.kind, "capture-stopped");
/// assert_eq!(draft.display, Some(2));
/// assert_eq!(draft.mach_ns, Some(5_000_000_000));
///
/// // without `at`, the incident is dated when it is recorded
/// assert_eq!(Draft::new(kind::RECORDER, "recording started").mach_ns, None);
/// ```
///
/// # Example: the kind is a static string
///
/// A kind is a `&'static str`, which a constant in [`kind`] is: it is part of the file's format,
/// not something to build at run time. A `String` is refused by the compiler:
///
/// ```rust,compile_fail,E0308
/// use komorebi_recorder::incidents::Draft;
///
/// let kind = format!("display-{}", "disconnected");
/// let _draft = Draft::new(kind, "display 1 disconnected");
/// ```
///
/// A constant, or any string literal, is accepted:
///
/// ```rust
/// use komorebi_recorder::incidents::{kind, Draft};
///
/// let _draft = Draft::new(kind::DISPLAY_DISCONNECTED, "display 1 disconnected");
/// ```
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Draft {
    /// The kind, one of the constants in [`kind`].
    pub kind: &'static str,
    /// A sentence saying what happened.
    pub detail: String,
    /// The display it is about, if it is about one.
    pub display: Option<u32>,
    /// When it happened, in nanoseconds on the master clock, if that is not now.
    pub mach_ns: Option<u64>,
}

impl Draft {
    /// Returns a draft of an incident of `kind`, described by `detail`, that is about no display
    /// in particular and happened when it is recorded.
    #[must_use]
    pub fn new(kind: &'static str, detail: impl Into<String>) -> Self {
        Self {
            kind,
            detail: detail.into(),
            display: None,
            mach_ns: None,
        }
    }

    /// Returns the draft, about the display `display`.
    #[must_use]
    pub fn display(mut self, display: u32) -> Self {
        self.display = Some(display);
        self
    }

    /// Returns the draft, dated `mach_ns` nanoseconds on the master clock instead of when it is
    /// recorded. A time in the future is not believed: [`IncidentLog::record`] dates it now.
    #[must_use]
    pub fn at(mut self, mach_ns: u64) -> Self {
        self.mach_ns = Some(mach_ns);
        self
    }
}

/// How many recent incidents to keep for the UI.
const RECENT: usize = 8;

/// The incident file of a recording in progress, open for appending.
///
/// Every incident is written and flushed the moment it is recorded, because incidents are rare and
/// a crash is exactly when they matter most. The log also keeps the last few incidents in memory,
/// and a count, for the recorder window to show.
///
/// # Example: an incident dated before it was noticed
///
/// ```rust
/// use komorebi_recorder::clock::mono_now_ns;
/// use komorebi_recorder::incidents::{kind, Draft, IncidentLog};
///
/// let dir = std::env::temp_dir().join(format!("komorebi-recorder-doc-dated-{}", std::process::id()));
/// std::fs::create_dir_all(&dir)?;
/// let mut log = IncidentLog::create(&dir)?;
///
/// // a stream that died 5 seconds ago, noticed now
/// let died = mono_now_ns() - 5_000_000_000;
/// let incident = log.record(Draft::new(kind::CAPTURE_STOPPED, "stream died").at(died));
/// assert_eq!(incident.mach_ns, died);
///
/// // a time in the future is not believed
/// let future = log.record(Draft::new(kind::WAKE, "woke").at(u64::MAX));
/// assert!(future.mach_ns <= mono_now_ns());
///
/// assert_eq!(log.count(), 2);
/// # std::fs::remove_dir_all(&dir)?;
/// # Ok::<(), Box<dyn std::error::Error>>(())
/// ```
pub struct IncidentLog {
    writer: BufWriter<File>,
    recent: VecDeque<Incident>,
    count: u64,
}

impl IncidentLog {
    /// Creates `recorder.jsonl` in `session_dir`, ready to append to.
    ///
    /// An existing `recorder.jsonl` is truncated.
    ///
    /// # Errors
    ///
    /// Returns the I/O error if the file cannot be created: `session_dir` does not exist, or it
    /// cannot be written to.
    pub fn create(session_dir: &Path) -> std::io::Result<Self> {
        Ok(Self {
            writer: BufWriter::new(File::create(session_dir.join(INCIDENTS_FILE))?),
            recent: VecDeque::new(),
            count: 0,
        })
    }

    /// Timestamps, writes and flushes an incident, and returns it as it was written.
    ///
    /// The incident happened at the draft's [`mach_ns`](Draft::mach_ns), or now if the draft has
    /// none, and never later than now. Its wall clock time is the current wall clock time minus
    /// how long ago that was.
    ///
    /// This does not fail. An error while writing or flushing is ignored: the incident is still
    /// returned, counted and kept for [`recent`](IncidentLog::recent), but may not be in the file.
    pub fn record(&mut self, draft: Draft) -> Incident {
        let now_mach = clock::mono_now_ns();
        let mach_ns = draft.mach_ns.unwrap_or(now_mach).min(now_mach);
        // the wall time it happened at, derived from how long ago that was
        let ago_us = i64::try_from(now_mach.saturating_sub(mach_ns) / 1_000).unwrap_or(0);
        let incident = Incident {
            mach_ns,
            wall_us: clock::wall_now_us() - ago_us,
            kind: draft.kind.to_string(),
            detail: draft.detail,
            display: draft.display,
        };

        if let Ok(line) = serde_json::to_string(&incident) {
            // incidents are rare and precious: never buffer them
            let _ = writeln!(self.writer, "{line}");
            let _ = self.writer.flush();
        }

        self.count += 1;
        if self.recent.len() == RECENT {
            self.recent.pop_front();
        }
        self.recent.push_back(incident.clone());
        incident
    }

    /// Records each draft in turn, as [`record`](IncidentLog::record) does.
    pub fn record_all(&mut self, drafts: impl IntoIterator<Item = Draft>) {
        for draft in drafts {
            self.record(draft);
        }
    }

    /// Returns how many incidents this log has recorded.
    #[must_use]
    pub fn count(&self) -> u64 {
        self.count
    }

    /// Returns the last 8 incidents recorded, oldest first.
    #[must_use]
    pub fn recent(&self) -> Vec<Incident> {
        self.recent.iter().cloned().collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp(name: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "komorebi-recorder-incidents-{name}-{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("dir");
        dir
    }

    #[test]
    fn incidents_are_on_disk_the_moment_they_are_recorded() {
        let dir = temp("flush");
        let mut log = IncidentLog::create(&dir).expect("create");

        log.record(Draft::new(kind::DISPLAY_DISCONNECTED, "display 1 went away").display(1));

        // no flush or drop needed: a crash right now must not lose it
        let text = std::fs::read_to_string(dir.join(INCIDENTS_FILE)).expect("read");
        let incident: Incident =
            serde_json::from_str(text.lines().next().expect("line")).expect("parse");
        assert_eq!(incident.kind, "display-disconnected");
        assert_eq!(incident.display, Some(1));
        assert_eq!(log.count(), 1);
    }

    #[test]
    fn an_incident_can_be_dated_before_it_was_noticed() {
        let dir = temp("dated");
        let mut log = IncidentLog::create(&dir).expect("create");

        let before = clock::mono_now_ns();
        let earlier = before - 5_000_000_000;
        let incident = log.record(Draft::new(kind::CAPTURE_STOPPED, "stream died").at(earlier));

        assert_eq!(incident.mach_ns, earlier);
        // the wall time is five seconds before now
        let skew = (clock::wall_now_us() - incident.wall_us - 5_000_000).abs();
        assert!(skew < 100_000, "wall time was {skew} us off");
    }

    #[test]
    fn an_incident_cannot_be_dated_in_the_future() {
        let dir = temp("future");
        let mut log = IncidentLog::create(&dir).expect("create");
        let incident = log.record(Draft::new(kind::WAKE, "x").at(u64::MAX));
        assert!(incident.mach_ns <= clock::mono_now_ns());
    }

    #[test]
    fn only_the_most_recent_incidents_are_kept_for_the_ui() {
        let dir = temp("recent");
        let mut log = IncidentLog::create(&dir).expect("create");
        for index in 0..20 {
            log.record(Draft::new(kind::RECORDER, format!("incident {index}")));
        }

        let recent = log.recent();
        assert_eq!(recent.len(), RECENT);
        assert_eq!(recent.last().expect("last").detail, "incident 19");
        assert_eq!(log.count(), 20);
    }
}
