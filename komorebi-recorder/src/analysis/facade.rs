//! A session opened for investigation: everything the commands and the viewer ask, computed once.
//!
//! [`Analysis`] is the object every command asks questions of, and the one the viewer holds while a
//! session is open in its window. [`Analysis::open`] takes the directory of a finished session. Opening is
//! cheap; each part (events, the parsed log, the pairing of the two, states) is read the first
//! time it is needed and kept.
//!
//! # Flavor, not host
//!
//! An `Analysis` is shaped by the komorebi that was recorded, not by the host it is run on. The
//! manifest says which ([`Analysis::flavor`]), and everything that reads komorebi's own output
//! (events, `State`, log) goes through the [`Rules`](super::flavor::Rules) of that [`Flavor`], so a
//! session recorded on Windows opens on a Mac and the other way round.
//!
//! # Where the questions are
//!
//! This file holds the parts every question shares: the session, its clock, its events, its log,
//! the pairing of the two, and the state at a moment. The questions are further methods of
//! [`Analysis`], each defined next to the data it returns:
//!
//! | Question | Methods | Module |
//! |---|---|---|
//! | An event or a moment as a reader sees it | [`Analysis::event_view`], [`Analysis::moment`] | [`super::views`] |
//! | The state at a moment, what changed, which events | `state_answer`, `diff_answer`, `events_answer` | `analysis::answers` |
//! | What the user says is wrong, and what is around it | `brief` | `analysis::brief` |
//! | What stands out in the whole recording | `overview`, `findings`, `incidents`, `session_info`, `histogram` | `analysis::overview` |
//! | One event end to end, and the OS trace around it | `trace`, `os_trace` | `analysis::trace`, `analysis::os_trace` |
//! | One window's life | `window_history` | `analysis::history` |
//! | Where a word appears | `search` | `analysis::search` |
//! | Which line of komorebi's source wrote a log line | `code_for_lines`, `code_for_event`, `code_search` | `analysis::code` |
//! | When the picture changed, and a contact sheet | `changes`, `sheet` | `analysis::changes`, `analysis::sheet` |
//!
//! # What is kept, and what is read every time
//!
//! The events, the parsed log, the pairing and the state at the start of the recording are read
//! once, the first time something asks, and kept for the life of the `Analysis`; each state a
//! notification carries is parsed once and shared. So an `Analysis` describes the session as it was
//! when each part was first read, and does not notice a file changing afterwards. That is
//! harmless for a finished session. The annotations are the exception: they are written to while
//! people and agents work, so they are not kept, and each question that needs them reads
//! `annotations.jsonl` again.
//!
//! # Example
//!
//! ```rust
//! use komorebi_recorder::analysis::Analysis;
//! use komorebi_recorder::testkit::{SessionBuilder, mach_ns, state_with_windows};
//! use serde_json::json;
//!
//! // a recording in which komorebi received one command, two seconds in, and had one window
//! let (session, _manifest) = SessionBuilder::new("doc-analysis")
//!     .event(2.0, json!({"type": "Retile"}), Some(state_with_windows(&[(7, "Terminal")])))
//!     .log(2.0, "INFO", "komorebi::process_command: retile")
//!     .build();
//!
//! let analysis = Analysis::open(&session.dir)?;
//! assert_eq!(analysis.events().len(), 1);
//! assert_eq!(analysis.log().lines.len(), 1);
//!
//! // any time argument a command takes
//! assert_eq!(analysis.resolve("event:1.end")?.mach_ns, mach_ns(2.0));
//!
//! // the state at a moment is the one the latest notification before it carried
//! let state = analysis.state_at(mach_ns(3.0))?;
//! assert_eq!(state.snapshot.windows().len(), 1);
//! # Ok::<(), Box<dyn std::error::Error>>(())
//! ```

use super::blocks;
use super::blocks::Block;
use super::blocks::Pair;
use super::classify;
use super::classify::Classification;
use super::envelope;
use super::events;
use super::events::EventRecord;
use super::flavor::Flavor;
use super::guard::SessionPath;
use super::logparse;
use super::logparse::Line;
use super::state::Snapshot;
use super::timespec::Edge;
use super::timespec::Lookup;
use super::timespec::Resolved;
use super::timespec::TimeSpec;
use crate::annotations;
use crate::clock::ClockMap;
use crate::incidents::INCIDENTS_FILE;
use crate::incidents::Incident;
use crate::session::Manifest;
use crate::session::STATUS_RECORDING;
use crate::session::Session;
use crate::sources::komorebi_events::state_file_name;
use crate::timeline;
use color_eyre::eyre;
use color_eyre::eyre::WrapErr;
use std::collections::HashMap;
use std::path::Path;
use std::sync::Arc;
use std::sync::Mutex;
use std::sync::OnceLock;

/// The parsed komorebi log of a session.
///
/// Read from `komorebi.log` by [`Analysis::log`]. A session with no log, or one that cannot be
/// read, has an empty `LogData`.
pub struct LogData {
    /// Every line of the file, in file order. A `Line` has the 1-based number of its line in the
    /// file.
    pub lines: Vec<Line>,
    /// The handler blocks the lines form (`analysis::blocks::build`), in the order their headers
    /// were logged.
    pub blocks: Vec<Block>,
}

/// Which handler produced which event, and what kind of event it was.
///
/// Made once by [`Analysis::pairing`].
pub struct Pairing {
    /// The name the recorder subscribed as to komorebi.
    ///
    /// It is the one the manifest recorded. A session that did not record it has it worked out from
    /// which of komorebi's `pushed notification` log lines line up with the events, and `None` if
    /// that finds nothing.
    pub subscriber: Option<String>,
    /// One per event, in event order.
    pub pairs: Vec<Pair>,
    /// One per event, in event order: what kind of event it was and why.
    pub classes: Vec<Classification>,
}

/// Where the state at a moment came from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StateSource {
    /// The notification of this event.
    Event {
        /// The event's sequence number.
        seq: u64,
        /// When the notification was received, in nanoseconds on the host's master monotonic
        /// clock.
        recv_mach_ns: u64,
    },
    /// The state recorded when the recording started, before any event.
    Start,
}

/// The state komorebi was in at a moment, and where that knowledge came from.
///
/// Returned by [`Analysis::state_at`].
pub struct StateAt {
    /// The state, parsed. Shared with the cache of the [`Analysis`] that returned it.
    pub snapshot: Arc<Snapshot>,
    /// The id of the recorded state (`states/<id>.json`); `None` for [`StateSource::Start`].
    pub state_id: Option<u64>,
    /// Which notification, or the start of the recording, this state is as of.
    pub source: StateSource,
}

/// A finished session, opened to be asked questions.
///
/// It holds the [`Session`] and its [`Manifest`], and reads the rest of the session lazily and at
/// most once (see the [module docs](self) for what is kept). The questions themselves are methods
/// defined next to the data they answer; this type has the parts they share. The komorebi that
/// was recorded decides how its output is read: see [`Analysis::flavor`].
///
/// A file name that comes from the session's own data, such as a video segment's file in the
/// manifest, is resolved through [`Analysis::guard`], so that it cannot lead outside the session.
///
/// # Example
///
/// ```rust
/// use komorebi_recorder::analysis::Analysis;
/// use komorebi_recorder::analysis::envelope::{ExitKind, exit_kind};
/// use komorebi_recorder::session::STATUS_RECORDING;
/// use komorebi_recorder::testkit::SessionBuilder;
///
/// let (session, mut manifest) = SessionBuilder::new("doc-analysis-open").build();
/// assert!(Analysis::open(&session.dir).is_ok());
///
/// // a session whose manifest still says `recording` cannot be analyzed yet
/// manifest.status = STATUS_RECORDING.to_string();
/// session.write_manifest(&manifest)?;
/// let error = Analysis::open(&session.dir).err().expect("not finished");
/// assert_eq!(exit_kind(&error), ExitKind::Unusable);
/// # Ok::<(), Box<dyn std::error::Error>>(())
/// ```
pub struct Analysis {
    /// The session directory.
    pub session: Session,
    /// The manifest, read when the session was opened.
    pub manifest: Manifest,
    guard: SessionPath,
    clock: ClockMap,
    events: OnceLock<(Vec<EventRecord>, Vec<String>)>,
    log: OnceLock<LogData>,
    pairing: OnceLock<Pairing>,
    start_state: OnceLock<Option<Arc<Snapshot>>>,
    states: Mutex<HashMap<u64, Arc<Snapshot>>>,
}

impl Analysis {
    /// Opens a finished session. One that is still being recorded, or whose recorder died, cannot
    /// be analyzed yet and says how to proceed.
    ///
    /// Only the manifest (`session.json`) is read here; everything else is read when a question
    /// first needs it. A session whose status is `complete` or `recovered` can be opened.
    ///
    /// # Errors
    ///
    /// - a not-found error if `dir` does not exist, or is not a session (it has no `session.json`);
    /// - an error if the manifest cannot be read, is not valid, or was written by a newer
    ///   `komorebi-recorder` than this one;
    /// - an unusable error if the manifest says the session is still `recording`. If a recorder
    ///   holds the session's lock the message says to stop the recording first. Otherwise (the lock
    ///   is not held, or the session has none) the recorder was killed or crashed, and the error
    ///   carries the hint to recover the session with `komorebi-recorder reindex`.
    ///
    /// Not-found and unusable errors are what `komorebi-recorder` exits with code 3 and 4 on (see
    /// `analysis::envelope::ExitKind`).
    pub fn open(dir: &Path) -> eyre::Result<Self> {
        let guard = SessionPath::new(dir)?;
        let session = Session::open(guard.root())?;
        let manifest = session.manifest()?;

        if manifest.status == STATUS_RECORDING {
            let shown = session.dir.display();
            return Err(match crate::lock::recorder(&session.dir) {
                crate::lock::Recorder::Running(_) => envelope::unusable(format!(
                    "{shown} is still being recorded; stop the recording first"
                )),
                _ => envelope::with_hint(
                    envelope::unusable(format!(
                        "{shown} was left unfinished: its recorder was killed or crashed"
                    )),
                    format!("recover it with `komorebi-recorder reindex {shown}`"),
                ),
            });
        }

        let clock = ClockMap::new(manifest.anchors.clone());
        Ok(Self {
            session,
            manifest,
            guard,
            clock,
            events: OnceLock::new(),
            log: OnceLock::new(),
            pairing: OnceLock::new(),
            start_state: OnceLock::new(),
            states: Mutex::new(HashMap::new()),
        })
    }

    /// Returns the log statements of the komorebi that recorded the session, as indexed from its
    /// source when this program was built.
    ///
    /// The catalog is the one for the session's [flavor](Analysis::flavor), and is empty if this
    /// build was made without that komorebi's source. It is the checkout the build saw, which may
    /// not be the commit that was recorded: see [`Analysis::skew`].
    #[must_use]
    pub fn catalog(&self) -> &'static super::catalog::Catalog {
        super::catalog::of(self.flavor())
    }

    /// Returns how the indexed source ([`Analysis::catalog`]) compares with the komorebi that
    /// ran.
    ///
    /// The repository is found by `annotations::default_code_root`; the commands that take a
    /// `--komorebi-src` use [`Analysis::skew_in`].
    #[must_use]
    pub fn skew(&self) -> super::catalog::Skew {
        self.skew_in(crate::annotations::default_code_root(self.flavor()).as_deref())
    }

    /// Returns [`Analysis::skew`] with the checkout of komorebi's repository given.
    ///
    /// The checkout is what the comparison asks which files differ between the recorded commit and
    /// the indexed one. With `None` there is no repository to ask.
    #[must_use]
    pub fn skew_in(&self, root: Option<&std::path::Path>) -> super::catalog::Skew {
        super::catalog::skew(self.flavor(), &self.manifest.komorebi.versions, root)
    }

    /// Returns the komorebi that recorded the session, which decides how its output is read.
    ///
    /// It is the flavor in the manifest ([`Flavor::Macos`] for a session that does not say), and
    /// not the one of the host this program runs on.
    #[must_use]
    pub fn flavor(&self) -> Flavor {
        self.manifest.komorebi.flavor
    }

    /// Returns the guard that resolves file names inside the session directory, refusing absolute
    /// paths, `..` and symlinks that lead out.
    #[must_use]
    pub fn guard(&self) -> &SessionPath {
        &self.guard
    }

    /// Returns the clock map of the session: conversions between the host's master monotonic clock
    /// and the wall clock, from the anchors in the manifest.
    #[must_use]
    pub fn clock(&self) -> &ClockMap {
        &self.clock
    }

    fn loaded_events(&self) -> &(Vec<EventRecord>, Vec<String>) {
        self.events.get_or_init(|| {
            events::load(&self.session, self.flavor().rules()).unwrap_or_else(|error| {
                (
                    Vec::new(),
                    vec![format!("could not read the events: {error:#}")],
                )
            })
        })
    }

    /// Returns every komorebi notification, in the order received: by receive time, and by sequence
    /// number among events received at the same time.
    ///
    /// The events are read from `events.jsonl` the first time. A session with no events file has
    /// none, without a warning. A line that cannot be read is skipped and counted in
    /// [`Analysis::warnings`]; if the file itself cannot be read the result is empty and
    /// [`Analysis::warnings`] reports it.
    #[must_use]
    pub fn events(&self) -> &[EventRecord] {
        &self.loaded_events().0
    }

    /// Returns things that went wrong reading the session's files, to be shown with any answer.
    ///
    /// These are, in this order: a warning if the rules for the session's [flavor](Analysis::flavor)
    /// have not been checked against a recording of that komorebi
    /// ([`Rules::verified`](super::flavor::Rules::verified)); and what went wrong reading the
    /// events (see [`Analysis::events`]). Problems with the log are not reported: a log that is
    /// missing or unreadable is read as empty ([`Analysis::log`]).
    ///
    /// The list is empty when nothing went wrong, which is the usual case.
    #[must_use]
    pub fn warnings(&self) -> Vec<String> {
        let mut warnings = Vec::new();
        let flavor = self.flavor();
        if !flavor.rules().verified() {
            warnings.push(format!(
                "this session recorded komorebi for {flavor}, which this build reads from \
                 komorebi's source and has not yet checked against a recording of it: event \
                 names, window handles and the grouping of log lines are probably right, but if \
                 something looks wrong, that is the first suspect"
            ));
        }

        warnings.extend(self.loaded_events().1.iter().cloned());
        warnings
    }

    /// Returns the position in [`Analysis::events`] of the event with this sequence number, or
    /// `None` if the session has no such event.
    ///
    /// This is a linear search. The position is not the sequence number: events are ordered by
    /// receive time, and a session may be missing events.
    #[must_use]
    pub fn event_index(&self, seq: u64) -> Option<usize> {
        self.events().iter().position(|event| event.seq == seq)
    }

    /// Returns komorebi's log, parsed, with its handler blocks.
    ///
    /// It is read from `komorebi.log` the first time. A session with no log, or one that cannot be
    /// read, has an empty one, and nothing reports it: check [`LogData::lines`].
    #[must_use]
    pub fn log(&self) -> &LogData {
        self.log.get_or_init(|| {
            let bytes = std::fs::read(self.session.path("komorebi.log")).unwrap_or_default();
            let lines = logparse::parse(&bytes);
            let blocks = blocks::build(&lines, &self.clock, self.flavor().rules());
            LogData { lines, blocks }
        })
    }

    /// Returns which handler block produced each event and what kind of event each was.
    ///
    /// This reads the events and the log if they have not been read. The result is made once.
    #[must_use]
    pub fn pairing(&self) -> &Pairing {
        self.pairing.get_or_init(|| {
            let log = self.log();
            let events = self.events();
            // the recorder wrote its own name into the manifest; older sessions are worked out from
            // which of komorebi's "pushed notification" lines line up with the events
            let subscriber = self
                .manifest
                .komorebi
                .subscriber
                .clone()
                .or_else(|| blocks::infer_subscriber(&log.blocks, events));
            Pairing {
                pairs: blocks::pair(events, &log.blocks, subscriber.as_deref()),
                classes: classify::classify(events, self.flavor().rules()),
                subscriber,
            }
        })
    }

    /// Returns whether the state changed at the event at `index` of [`Analysis::events`]: whether
    /// its state id differs from the previous event's.
    ///
    /// The answer is `None` when it is not known: `index` is out of range, is `0` (there is no
    /// previous event to compare with), or either event has no state id.
    #[must_use]
    pub fn state_changed(&self, index: usize) -> Option<bool> {
        let events = self.events();
        let (this, before) = (
            events.get(index)?.state_id?,
            events.get(index.checked_sub(1)?)?.state_id?,
        );
        Some(this != before)
    }

    /// Returns the recorded state with this id (the file `states/000001.json` for id 1), parsed.
    ///
    /// A state is parsed once; later calls with the same id return the same shared value. The ids
    /// are the `state_id` of the events and of [`StateAt`].
    ///
    /// # Errors
    ///
    /// Returns a not-found error if there is no such state, or its file cannot be read, and an
    /// error if the file is not valid JSON. A state that is JSON but not shaped as expected does not
    /// fail: it is read tolerantly, with what is missing left empty.
    pub fn snapshot(&self, state_id: u64) -> eyre::Result<Arc<Snapshot>> {
        if let Some(found) = self
            .states
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .get(&state_id)
        {
            return Ok(found.clone());
        }

        let path = self.guard.resolve(&state_file_name(state_id))?;
        let text = std::fs::read_to_string(&path)
            .map_err(|_| envelope::not_found(format!("there is no recorded state {state_id}")))?;
        let value: serde_json::Value = serde_json::from_str(&text)
            .wrap_err_with(|| format!("{} is not valid JSON", path.display()))?;
        let snapshot = Arc::new(Snapshot::parse(self.flavor(), &value));
        self.states
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .insert(state_id, snapshot.clone());
        Ok(snapshot)
    }

    fn start_snapshot(&self) -> Option<Arc<Snapshot>> {
        self.start_state
            .get_or_init(|| {
                let text = std::fs::read_to_string(self.session.path("state-start.json")).ok()?;
                let value: serde_json::Value = serde_json::from_str(&text).ok()?;
                Some(Arc::new(Snapshot::parse(self.flavor(), &value)))
            })
            .clone()
    }

    /// Returns the state komorebi was in at `mach_ns`, in nanoseconds on the host's master
    /// monotonic clock: the one in the latest notification before it, or the starting state if
    /// there has been none. It is the state as of that notification, so it can be out of date if
    /// komorebi changed state without sending one since.
    ///
    /// A notification received at exactly `mach_ns` counts as before it. Only a notification that
    /// carries a state counts. [`StateAt::source`] names the one that was used, and how old it was is
    /// the difference between `mach_ns` and its `recv_mach_ns`.
    ///
    /// # Errors
    ///
    /// Returns a not-found error if no state was recorded before this moment (komorebi was not
    /// running, or had sent no notification yet and the recording has no starting state), and the
    /// errors of [`Analysis::snapshot`] if the state's file is missing or invalid.
    ///
    /// # Example
    ///
    /// ```rust
    /// use komorebi_recorder::analysis::Analysis;
    /// use komorebi_recorder::analysis::facade::StateSource;
    /// use komorebi_recorder::testkit::{SessionBuilder, mach_ns, state_with_windows};
    /// use serde_json::json;
    ///
    /// let (session, _) = SessionBuilder::new("doc-state-at")
    ///     .event(2.0, json!({"type": "Retile"}), Some(state_with_windows(&[(7, "Terminal")])))
    ///     .build();
    /// let analysis = Analysis::open(&session.dir)?;
    ///
    /// // before the first notification there is nothing to say
    /// assert!(analysis.state_at(mach_ns(1.0)).is_err());
    ///
    /// // after it, the state it carried, as of that event
    /// let found = analysis.state_at(mach_ns(3.5))?;
    /// assert_eq!(found.state_id, Some(1));
    /// assert_eq!(
    ///     found.source,
    ///     StateSource::Event { seq: 1, recv_mach_ns: mach_ns(2.0) },
    /// );
    /// # Ok::<(), Box<dyn std::error::Error>>(())
    /// ```
    pub fn state_at(&self, mach_ns: u64) -> eyre::Result<StateAt> {
        let events = self.events();
        let latest = events
            .iter()
            .rev()
            .find(|event| event.recv_mach_ns <= mach_ns && event.state_id.is_some());

        if let Some(event) = latest {
            let state_id = event.state_id.unwrap_or_default();
            return Ok(StateAt {
                snapshot: self.snapshot(state_id)?,
                state_id: Some(state_id),
                source: StateSource::Event {
                    seq: event.seq,
                    recv_mach_ns: event.recv_mach_ns,
                },
            });
        }

        self.start_snapshot()
            .map(|snapshot| StateAt {
                snapshot,
                state_id: None,
                source: StateSource::Start,
            })
            .ok_or_else(|| {
                envelope::not_found(
                    "no state was recorded before this moment (komorebi was not running, or \
                     had sent no notification yet)",
                )
            })
    }

    /// Resolves any time argument a command takes: `12.5`, `1:02.5`, `wall:<RFC 3339>`,
    /// `mach:<ns>`, `event:<seq>`, `log:<line>`, `incident:<n>` or `bug:<id>`, each optionally
    /// followed by `.start` or `.end` and an offset such as `+3s`. The forms are those of
    /// `analysis::timespec`.
    ///
    /// Resolving never guesses: a name that is not there is an error. An annotation without an end
    /// ends when the recording did. An event's start is when its handler began, where the log
    /// says (see the `Lookup` implementation below), and its end is when its notification was
    /// received.
    ///
    /// # Errors
    ///
    /// Returns a usage error if the text is empty or is not one of those forms. Returns a
    /// not-found error if it names an event, log line, incident or annotation the session does not
    /// have (a log line without a time included), and an unusable error for a `wall:` time when the
    /// session has no clock anchors.
    ///
    /// # Example
    ///
    /// ```rust
    /// use komorebi_recorder::analysis::Analysis;
    /// use komorebi_recorder::testkit::{SessionBuilder, mach_ns};
    /// use serde_json::json;
    ///
    /// let (session, _) = SessionBuilder::new("doc-resolve")
    ///     .event(2.0, json!({"type": "Retile"}), None)
    ///     .build();
    /// let analysis = Analysis::open(&session.dir)?;
    ///
    /// assert_eq!(analysis.resolve("2.5")?.video_s, 2.5);
    /// assert_eq!(analysis.resolve("event:1.end+1s")?.video_s, 3.0);
    /// assert_eq!(analysis.resolve("event:1.end")?.mach_ns, mach_ns(2.0));
    ///
    /// let error = analysis.resolve("event:2").unwrap_err();
    /// assert_eq!(error.to_string(), "there is no event #2");
    /// # Ok::<(), Box<dyn std::error::Error>>(())
    /// ```
    pub fn resolve(&self, text: &str) -> eyre::Result<Resolved> {
        text.parse::<TimeSpec>()?.resolve(&self.manifest, self)
    }

    /// Returns `mach_ns` (nanoseconds on the host's master monotonic clock) as seconds of video
    /// time: negative before the first frame of the recording.
    #[must_use]
    pub fn video_s(&self, mach_ns: u64) -> f64 {
        self.manifest.video_seconds(mach_ns)
    }

    /// Returns `mach_ns` as a UTC time in RFC 3339 form with microseconds, or `None` if the session
    /// has no clock anchors to convert with.
    #[must_use]
    pub fn wall_utc(&self, mach_ns: u64) -> Option<String> {
        self.clock
            .mach_to_wall(mach_ns)
            .and_then(timeline::format_wall_us)
    }
}

/// Resolves the names a `TimeSpec` can use against the files of the session.
///
/// | Name | Resolves to |
/// |---|---|
/// | `event:<seq>` or `.start` | when the event's handler began, from the log; the time the event was received if the log has no block for it. It is clamped to no earlier than the previous event's receipt and (where there is room) before this one's, so that the state at the start is exactly the state the previous event left |
/// | `event:<seq>.end` | when the notification was received, which is when the handler finished |
/// | `log:<line>` | the line's timestamp (lines are counted from 1) |
/// | `incident:<n>` | the `n`th line of `incidents.jsonl` (counted from 1) |
/// | `bug:<id>`, `.start`, `.end` | the annotation's start or end; an open region ends when the recording did |
///
/// Each of them that names something the session lacks is a not-found error. Annotations are read
/// from the file each time.
impl Lookup for Analysis {
    fn event_mach_ns(&self, seq: u64, edge: Edge) -> eyre::Result<u64> {
        let index = self
            .event_index(seq)
            .ok_or_else(|| envelope::not_found(format!("there is no event #{seq}")))?;
        let event = &self.events()[index];

        if edge == Edge::End {
            return Ok(event.recv_mach_ns);
        }

        // The start of an event is when its handler began, where the log says. It is clamped to
        // after the previous notification and before this one, so that the state at the start is
        // exactly the state the previous event left and never already includes this event.
        let began = self
            .pairing()
            .pairs
            .get(index)
            .and_then(|pair| pair.block)
            .and_then(|id| self.log().blocks.get(id))
            .map_or(event.recv_mach_ns, |block| block.header_mach_ns);
        let previous = index
            .checked_sub(1)
            .map_or(0, |before| self.events()[before].recv_mach_ns);
        Ok(began
            .min(event.recv_mach_ns.saturating_sub(1))
            .max(previous))
    }

    fn log_mach_ns(&self, line: u64) -> eyre::Result<u64> {
        let log = self.log();
        let parsed = usize::try_from(line.saturating_sub(1))
            .ok()
            .filter(|_| line >= 1)
            .and_then(|index| log.lines.get(index))
            .ok_or_else(|| {
                envelope::not_found(format!(
                    "komorebi.log has {} lines; there is no line {line}",
                    log.lines.len()
                ))
            })?;

        parsed
            .wall_us
            .and_then(|wall_us| self.clock.wall_to_mach(wall_us))
            .ok_or_else(|| envelope::not_found(format!("log line {line} has no time")))
    }

    fn incident_mach_ns(&self, number: u64) -> eyre::Result<u64> {
        let text = std::fs::read_to_string(self.session.path(INCIDENTS_FILE))
            .map_err(|_| envelope::not_found("the session recorded no incidents"))?;
        usize::try_from(number.saturating_sub(1))
            .ok()
            .filter(|_| number >= 1)
            .and_then(|index| text.lines().nth(index))
            .and_then(|line| serde_json::from_str::<Incident>(line).ok())
            .map(|incident| incident.mach_ns)
            .ok_or_else(|| envelope::not_found(format!("there is no incident {number}")))
    }

    fn annotation_span(&self, id: &str) -> eyre::Result<(u64, Option<u64>)> {
        let loaded = annotations::load(&self.session.dir)?;
        loaded
            .get(id)
            .map(|annotation| (annotation.start_mach_ns, annotation.end_mach_ns))
            .ok_or_else(|| envelope::not_found(format!("there is no annotation {id}")))
    }
}
