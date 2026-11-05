//! The OS trace around a komorebi event: what the rest of the system did because of it.
//!
//! A komorebi event is the start of a conversation: komorebi calls into the OS to move windows and
//! the OS answers, the applications whose windows moved react, and some of that reaches the OS
//! trace the recorder captured next to komorebi's own output. [`Analysis::os_trace`] answers "what
//! happened around this event?" from that trace, in two stretches:
//!
//! - **during the handler**, from the moment its handler began (where komorebi's log says) to the
//!   moment the notification was received, which komorebi sends once the handler is done:
//!   komorebi's own calls into the OS, and whatever the system did meanwhile;
//! - **after it**, for a short while: the applications and the system reacting.
//!
//! The records are grouped by where they came from (komorebi's own process, the system, each
//! application). That the records fall in the window does not prove the event caused them: another
//! event's handler may overlap it, and an application does things on its own. Both are said, with
//! the other events that were received in the window, rather than left to be assumed.
//!
//! It is what `trace --os` adds to a trace and what the viewer's inspector shows under an event.
//!
//! # Flavor
//!
//! The OS trace is whatever the recording host captured, and [`Analysis::os_trace`] groups it by
//! the flavor of komorebi the session recorded, because that decides what the streams hold:
//!
//! | session | the trace has | groups |
//! |---|---|---|
//! | komorebi for Mac (recorded on a Mac) | a stream for komorebi's process, one for the system, one for each application | one per stream: `komorebi`, `system`, each application by name |
//! | komorebi for Windows (recorded on Windows) | one stream of what the OS told every window (`WinEvent`s), and the Event Log | one per program found in the record, with komorebi's own windows (`komorebi.exe`) apart from everyone else's; the Event Log as `eventlog` |
//!
//! A group's [`pid`](OsGroup::pid) is set only for an application, since the system is many
//! processes. Records from the recorder itself (`komorebi-recorder`) are left out, the same way
//! the recorder is kept out of the video.
//!
//! # Limits
//!
//! The window is the handler's duration plus one second after the notification. At most 40
//! records are listed for each group and 400 in all, in time order; the counts are of everything,
//! and the notes say when records were left out. At most 12 overlapping events are named.
//!
//! The trace has to have been recorded: a session made without one (`--no-oslog`), or on a host
//! that has none, answers with no groups and a note saying so.

use super::Analysis;
use super::envelope;
use super::timespec::Edge;
use super::timespec::Lookup;
use crate::timeline;
use crate::timeline::Filter;
use color_eyre::eyre;
use schemars::JsonSchema;
use serde::Serialize;

/// How long after a notification the reaction to it is looked for.
const REACTION_NS: u64 = 1_000_000_000;
/// How far back from the notification to look when komorebi's log does not say when the handler
/// began (it logs the start of a handler only at a verbose level).
const UNKNOWN_HANDLER_NS: u64 = 250_000_000;
/// Most records listed for one source; the rest are counted.
const PER_GROUP: usize = 40;
/// Most records listed in all.
const TOTAL: usize = 400;
/// Most other events named as having overlapped the window.
const OVERLAPS_LISTED: usize = 12;

/// Where a group of OS trace records came from.
///
/// The groups of an [`OsTraceData`] are ordered by kind in the order of the variants: komorebi
/// first, then the system, then applications, then the rest.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, JsonSchema)]
#[serde(rename_all = "kebab-case")]
pub enum GroupKind {
    /// komorebi's own process: its calls into the OS.
    Komorebi,
    /// System daemons and frameworks: the accessibility and window-server conversation.
    System,
    /// One application komorebi manages (or was told to trace).
    App,
    /// Anything else the recording traced.
    Other,
}

/// Whether a record fell while komorebi's handler was running or after its notification.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, JsonSchema)]
#[serde(rename_all = "kebab-case")]
pub enum Phase {
    /// While komorebi's handler was running.
    During,
    /// After the notification was sent.
    After,
}

/// One record of the OS trace.
#[derive(Debug, Clone, PartialEq, Serialize, JsonSchema)]
pub struct OsRecord {
    /// When the record was logged, in seconds of video time.
    pub video_s: f64,
    /// Whether it fell during the handler or after the notification.
    pub phase: Phase,
    /// The process that logged it, if the record has one.
    pub pid: Option<i64>,
    /// The record as `at` would show it: time, pid, process, subsystem and message.
    pub text: String,
    /// The file of the session the record is in, relative to the session directory, to read the
    /// whole record.
    pub file: String,
    /// The line of `file` the record is on, counting from 1.
    pub line: u64,
}

/// The records of one source.
#[derive(Debug, Clone, PartialEq, Serialize, JsonSchema)]
pub struct OsGroup {
    /// Where the records came from.
    pub kind: GroupKind,
    /// `komorebi`, `system` or the application's name; on a Windows session, the program's name
    /// (`chrome.exe`) or `eventlog`.
    pub label: String,
    /// The source as `at --source` takes it, e.g. `oslog:app:Ghostty`. On a Windows session
    /// several groups share the one stream `oslog:system`, so it does not tell them apart: the
    /// `label` does.
    pub source: String,
    /// The process of an application's records (that of the group's first record); `None` for the
    /// other kinds of group, which are many processes.
    pub pid: Option<i64>,
    /// How many records the group had while komorebi's handler ran.
    pub during: usize,
    /// How many records the group had after the notification, within the reaction window.
    pub after: usize,
    /// The first of them in time order: at most 40 for a group, and 400 over all groups. Fewer
    /// than `during + after` when some were left out ([`OsGroup::omitted`]).
    pub records: Vec<OsRecord>,
}

impl OsGroup {
    /// Returns how many records the group had in the whole window, `during + after`, whether or
    /// not they are listed.
    #[must_use]
    pub fn total(&self) -> usize {
        self.during + self.after
    }

    /// Returns how many records were counted but not listed.
    #[must_use]
    pub fn omitted(&self) -> usize {
        self.total().saturating_sub(self.records.len())
    }
}

/// The OS trace around one komorebi event: the `os_trace` of a `trace --os`.
#[derive(Debug, Clone, PartialEq, Serialize, JsonSchema)]
pub struct OsTraceData {
    /// The event.
    pub seq: u64,
    /// When the handler began, in seconds of video time. If komorebi's log does not say, it is
    /// taken to be 250 ms before the notification.
    pub started_s: f64,
    /// When the notification was received, which is when komorebi finished the handler, in seconds
    /// of video time.
    pub received_s: f64,
    /// When the reaction window ended, 1 s after the notification, in seconds of video time.
    pub until_s: f64,
    /// Every record in the window, listed or not, without the recorder's own.
    pub total: usize,
    /// The records by source: komorebi's own first, then the system, then the applications, then
    /// anything else, the busiest first within each.
    pub groups: Vec<OsGroup>,
    /// Other events received inside the window, whose handlers may have caused some of this: the
    /// first 12, in the order received.
    pub overlapping_events: Vec<u64>,
    /// Anything missing or worth knowing about the answer: that the handler's start is a guess,
    /// that the session has no OS trace, that records were left out.
    pub notes: Vec<String>,
}

/// Whether a rendered record is from a recorder process (this one, or another): what the recorder
/// itself does while it watches is not part of what happened, and it is left out of the video the
/// same way. A rendered record reads `<time> [<pid>] <process> <subsystem/category>: <message>`.
fn is_from_a_recorder(text: &str) -> bool {
    text.split_once("] ")
        .is_some_and(|(_, rest)| rest.starts_with("komorebi-recorder"))
}

/// The program a rendered record is from: `<time> [<pid>] <process> ...`.
fn process_of(text: &str) -> Option<&str> {
    text.split_once("] ")?.1.split_whitespace().next()
}

/// Where a record belongs.
///
/// On a Mac each application has a stream of its own and the source says it. On Windows the OS
/// trace is one stream of what the OS told every window, so the program is read from the record:
/// komorebi's own windows (its borders, its bar) apart from everyone else's.
fn group_for(windows: bool, source: &str, text: &str) -> (GroupKind, String, String) {
    if windows
        && source == "oslog:system"
        && let Some(process) = process_of(text).filter(|process| !process.is_empty())
    {
        let kind = if process.eq_ignore_ascii_case("komorebi.exe")
            || process.eq_ignore_ascii_case("komorebi")
        {
            GroupKind::Komorebi
        } else {
            GroupKind::App
        };
        return (kind, process.to_string(), format!("{source}\u{0}{process}"));
    }

    let (kind, label) = group_of(source);
    (kind, label, source.to_string())
}

/// Where a source's records belong.
fn group_of(source: &str) -> (GroupKind, String) {
    match source {
        "oslog:komorebi" => (GroupKind::Komorebi, "komorebi".to_string()),
        "oslog:system" => (GroupKind::System, "system".to_string()),
        other => match other.strip_prefix("oslog:app:") {
            Some(app) => (GroupKind::App, app.to_string()),
            None => (
                GroupKind::Other,
                other.strip_prefix("oslog:").unwrap_or(other).to_string(),
            ),
        },
    }
}

fn rank(kind: GroupKind) -> u8 {
    match kind {
        GroupKind::Komorebi => 0,
        GroupKind::System => 1,
        GroupKind::App => 2,
        GroupKind::Other => 3,
    }
}

impl Analysis {
    /// Whether the session has any OS trace at all, however it got there.
    pub(super) fn has_os_trace(&self) -> bool {
        !self.manifest.oslog.is_empty()
            || ["oslog", "oslog/apps"].iter().any(|dir| {
                std::fs::read_dir(self.session.path(dir)).is_ok_and(|entries| {
                    entries
                        .flatten()
                        .any(|entry| entry.file_name().to_string_lossy().ends_with(".ndjson"))
                })
            })
    }

    /// Returns the OS trace around event `seq`: what komorebi called, what the system did and how
    /// the applications reacted. See the [module](self) for what that does and does not show.
    ///
    /// The window runs from when the handler began (where komorebi's log shows it, else 250 ms
    /// before the notification) to 1 s after the notification, and the records in it are split into
    /// [`Phase::During`] and [`Phase::After`] at the notification.
    ///
    /// A session with no OS trace, or no records in the window, is not an error: the result has no
    /// groups and a note saying which it is.
    ///
    /// # Errors
    ///
    /// Returns an error if:
    ///
    /// - there is no event numbered `seq` (the not-found exit kind);
    /// - the timeline index (`timeline.jsonl`) is missing, in which case `komorebi-recorder
    ///   reindex` rebuilds it, or a trace file it points at cannot be read.
    ///
    /// # Example
    ///
    /// ```rust
    /// use komorebi_recorder::analysis::Analysis;
    /// use komorebi_recorder::analysis::os_trace::GroupKind;
    /// use komorebi_recorder::analysis::os_trace::Phase;
    /// use komorebi_recorder::testkit::SessionBuilder;
    /// use serde_json::json;
    ///
    /// // komorebi's handler began at 1.8 s (its header is in the log) and was received at 2.0 s
    /// let (session, _) = SessionBuilder::new("doc-os-trace")
    ///     .log(
    ///         1.8,
    ///         "INFO",
    ///         "process_event: komorebi::process_event: processing event: FocusChange for process 10 with notification AXApplicationActivated",
    ///     )
    ///     .event(
    ///         2.0,
    ///         json!({"type": "FocusChange", "content": [{"type": "Accessibility", "content": "AXApplicationActivated"}, 10, null]}),
    ///         None,
    ///     )
    ///     // komorebi's own call, while the handler ran
    ///     .os_record(1.85, "komorebi", 100, "komorebi", "AXUIElementSetAttributeValue")
    ///     // an application reacting afterwards
    ///     .os_record(2.30, "Ghostty", 300, "Ghostty", "window resized")
    ///     .build();
    /// let analysis = Analysis::open(&session.dir)?;
    ///
    /// let data = analysis.os_trace(1)?;
    /// assert_eq!(data.total, 2);
    /// assert!((data.started_s - 1.8).abs() < 0.01);
    ///
    /// let groups: Vec<_> = data.groups.iter().map(|group| (group.kind, group.label.as_str())).collect();
    /// assert_eq!(groups, [(GroupKind::Komorebi, "komorebi"), (GroupKind::App, "Ghostty")]);
    /// assert_eq!(data.groups[0].records[0].phase, Phase::During);
    /// assert_eq!(data.groups[1].records[0].phase, Phase::After);
    /// assert_eq!(data.groups[1].pid, Some(300));
    ///
    /// // an event that does not exist is an error
    /// assert!(analysis.os_trace(99).is_err());
    /// # Ok::<(), Box<dyn std::error::Error>>(())
    /// ```
    ///
    /// # Example: a Windows session
    ///
    /// A session recorded from komorebi for Windows has one stream of everything the OS told every
    /// window, which is grouped by program:
    ///
    /// ```rust
    /// use komorebi_recorder::analysis::Analysis;
    /// use komorebi_recorder::analysis::flavor::Flavor;
    /// use komorebi_recorder::analysis::os_trace::GroupKind;
    /// use komorebi_recorder::testkit::SessionBuilder;
    /// use komorebi_recorder::testkit::windows_window;
    /// use serde_json::json;
    ///
    /// let (session, _) = SessionBuilder::new("doc-os-trace-windows")
    ///     .flavor(Flavor::Windows)
    ///     .event(
    ///         2.0,
    ///         json!({"type": "FocusChange", "content": ["SystemForeground", windows_window(7, "chrome.exe", Some([0, 0, 1, 1]))]}),
    ///         None,
    ///     )
    ///     .os_record(1.95, "system", 100, "komorebi.exe", "ObjectLocationChange hwnd=0x1")
    ///     .os_record(1.99, "system", 300, "chrome.exe", "ObjectFocus hwnd=0x7")
    ///     .os_record(2.10, "system", 300, "chrome.exe", "ObjectLocationChange hwnd=0x7")
    ///     .build();
    /// let analysis = Analysis::open(&session.dir)?;
    ///
    /// let data = analysis.os_trace(1)?;
    /// let groups: Vec<_> = data
    ///     .groups
    ///     .iter()
    ///     .map(|group| (group.kind, group.label.as_str(), group.total()))
    ///     .collect();
    /// assert_eq!(
    ///     groups,
    ///     [(GroupKind::Komorebi, "komorebi.exe", 1), (GroupKind::App, "chrome.exe", 2)]
    /// );
    /// # Ok::<(), Box<dyn std::error::Error>>(())
    /// ```
    ///
    /// Also covered by the unit tests of this module, among them
    /// `the_records_around_an_event_are_grouped_by_source_and_split_by_phase`,
    /// `a_handler_the_log_shows_decides_where_the_window_starts`,
    /// `a_session_without_an_os_trace_says_so_instead_of_showing_nothing` and
    /// `on_windows_one_stream_of_everything_is_grouped_by_program`.
    pub fn os_trace(&self, seq: u64) -> eyre::Result<OsTraceData> {
        let index = self
            .event_index(seq)
            .ok_or_else(|| envelope::not_found(format!("there is no event #{seq}")))?;
        let event = &self.events()[index];

        let handler_known = self
            .pairing()
            .pairs
            .get(index)
            .is_some_and(|pair| pair.block.is_some());
        let started_ns = if handler_known {
            self.event_mach_ns(seq, Edge::Start)?
        } else {
            event.recv_mach_ns.saturating_sub(UNKNOWN_HANDLER_NS)
        };
        let received_ns = event.recv_mach_ns.max(started_ns);
        let until_ns = received_ns.saturating_add(REACTION_NS);
        let to_s = |ns: u64| self.manifest.video_seconds(ns);

        let overlapping: Vec<u64> = self
            .events()
            .iter()
            .filter(|other| other.seq != seq)
            .filter(|other| other.recv_mach_ns >= started_ns && other.recv_mach_ns <= until_ns)
            .map(|other| other.seq)
            .collect();

        let filter = Filter {
            app: None,
            sources: vec!["oslog".to_string()],
        };
        let records = timeline::query(
            &self.session,
            &self.manifest,
            to_s(started_ns),
            to_s(until_ns),
            &filter,
        )?;

        let mut notes = Vec::new();
        if !handler_known {
            notes.push(format!(
                "komorebi's log has no handler for this event, so when it began is not known: the \
                 {} ms before it was received are taken to be the handler (komorebi logs the start \
                 of a handler at debug level and above)",
                UNKNOWN_HANDLER_NS / 1_000_000
            ));
        }
        if !self.has_os_trace() {
            notes.push(
                "this session has no OS trace: it was recorded without one (`--no-oslog`), or on \
                 a host that has none, so there is nothing to show"
                    .to_string(),
            );
        } else if records.is_empty() {
            notes.push(
                "the OS trace has no records in this window: the streams may have been quiet, or \
                 one of them may have been restarting (see the recorder source)"
                    .to_string(),
            );
        }
        if !overlapping.is_empty() {
            notes.push(format!(
                "{} other event(s) were received in this window, so some of what is listed may \
                 belong to their handlers",
                overlapping.len()
            ));
        }

        let before = records.len();
        let records: Vec<_> = records
            .into_iter()
            .filter(|record| !is_from_a_recorder(&record.text))
            .collect();
        if records.len() < before {
            notes.push(format!(
                "{} record(s) from komorebi-recorder itself are left out",
                before - records.len()
            ));
        }

        let windows = self.flavor() == super::flavor::Flavor::Windows;
        let total = records.len();
        // each group's key: its source, and on Windows the program as well
        let mut keys: Vec<String> = Vec::new();
        let mut groups: Vec<OsGroup> = Vec::new();
        let mut listed = 0_usize;
        for record in records {
            let phase = if record.entry.mach_ns <= received_ns {
                Phase::During
            } else {
                Phase::After
            };
            let (kind, label, key) = group_for(windows, &record.entry.source, &record.text);
            let at = match keys.iter().position(|known| *known == key) {
                Some(at) => at,
                None => {
                    keys.push(key);
                    groups.push(OsGroup {
                        kind,
                        label,
                        source: record.entry.source.clone(),
                        // one process only for an application; the system is many
                        pid: (kind == GroupKind::App)
                            .then_some(record.entry.pid)
                            .flatten(),
                        during: 0,
                        after: 0,
                        records: Vec::new(),
                    });
                    groups.len() - 1
                }
            };

            let group = &mut groups[at];
            match phase {
                Phase::During => group.during += 1,
                Phase::After => group.after += 1,
            }
            if group.records.len() < PER_GROUP && listed < TOTAL {
                listed += 1;
                group.records.push(OsRecord {
                    video_s: record.entry.video_s,
                    phase,
                    pid: record.entry.pid,
                    text: record.text,
                    file: record.entry.file,
                    line: record.entry.line,
                });
            }
        }

        // komorebi's own calls first, then the system, then the applications, busiest first
        groups.sort_by(|a, b| {
            rank(a.kind)
                .cmp(&rank(b.kind))
                .then(b.total().cmp(&a.total()))
                .then_with(|| a.label.cmp(&b.label))
        });
        if groups.iter().any(|group| group.omitted() > 0) {
            notes.push(format!(
                "only the first {PER_GROUP} records of each source are listed; the counts are of \
                 everything, and `at <session> <time> --source <source>` shows the rest"
            ));
        }

        Ok(OsTraceData {
            seq,
            started_s: to_s(started_ns),
            received_s: to_s(received_ns),
            until_s: to_s(until_ns),
            total,
            groups,
            overlapping_events: overlapping.into_iter().take(OVERLAPS_LISTED).collect(),
            notes,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testkit::SessionBuilder;
    use serde_json::json;

    #[test]
    fn sources_are_grouped_by_where_they_came_from() {
        assert_eq!(
            group_of("oslog:komorebi"),
            (GroupKind::Komorebi, "komorebi".to_string())
        );
        assert_eq!(
            group_of("oslog:system"),
            (GroupKind::System, "system".to_string())
        );
        assert_eq!(
            group_of("oslog:app:Ghostty"),
            (GroupKind::App, "Ghostty".to_string())
        );
        assert_eq!(group_of("oslog:all"), (GroupKind::Other, "all".to_string()));
    }

    /// Each test gets a directory of its own: they run in parallel.
    fn session(name: &str, with_trace: bool) -> Analysis {
        let mut builder = SessionBuilder::new(&format!("os-trace-{name}"))
            .event(
                2.0,
                json!({"type": "FocusWorkspaceNumber", "content": 1}),
                Some(json!({"monitors": {"elements": []}})),
            )
            .event(
                6.0,
                json!({"type": "Retile"}),
                Some(json!({"monitors": {"elements": []}})),
            );
        if with_trace {
            builder = builder
                .os_record(
                    1.95,
                    "komorebi",
                    100,
                    "komorebi",
                    "AXUIElementSetAttributeValue",
                )
                .os_record(1.99, "system", 200, "WindowServer", "moving window 7")
                .os_record(2.30, "Ghostty", 300, "Ghostty", "window resized")
                .os_record(2.60, "Ghostty", 300, "Ghostty", "redraw")
                // the recorder watching: not part of what happened
                .os_record(2.10, "system", 999, "komorebi-recorder", "SLSGetDisplays")
                // after the reaction window
                .os_record(4.5, "Ghostty", 300, "Ghostty", "a long time later")
                // before the handler began
                .os_record(0.5, "system", 200, "WindowServer", "unrelated");
        }
        Analysis::open(&builder.build().0.dir).expect("open")
    }

    #[test]
    fn the_records_around_an_event_are_grouped_by_source_and_split_by_phase() {
        let analysis = session("grouped", true);
        let seq = analysis.events()[0].seq;
        let data = analysis.os_trace(seq).expect("os trace");

        assert!(data.started_s <= data.received_s && data.received_s < data.until_s);
        assert_eq!(data.total, 4, "{data:#?}");
        assert!(
            data.notes
                .iter()
                .any(|note| note.contains("komorebi-recorder itself")),
            "{:?}",
            data.notes
        );
        assert!(
            data.groups
                .iter()
                .flat_map(|group| &group.records)
                .all(|record| !record.text.contains("SLSGetDisplays"))
        );
        assert_eq!(data.groups[1].pid, None, "the system is not one process");
        assert_eq!(data.groups[2].pid, Some(300));

        let labels: Vec<_> = data
            .groups
            .iter()
            .map(|group| (group.kind, group.label.as_str()))
            .collect();
        assert_eq!(
            labels,
            [
                (GroupKind::Komorebi, "komorebi"),
                (GroupKind::System, "system"),
                (GroupKind::App, "Ghostty")
            ]
        );

        let ghostty = &data.groups[2];
        assert_eq!(ghostty.total(), 2);
        assert!(ghostty.after >= 1, "{ghostty:#?}");
        assert!(ghostty.records[0].text.contains("window resized"));
        assert_eq!(ghostty.omitted(), 0);
        assert!(
            data.groups
                .iter()
                .flat_map(|group| &group.records)
                .all(|record| !record.text.contains("a long time later")
                    && !record.text.contains("unrelated"))
        );
    }

    #[test]
    fn a_handler_the_log_shows_decides_where_the_window_starts() {
        let (session, _) = SessionBuilder::new("os-trace-known-handler")
            .log(
                1.8,
                "INFO",
                "process_event: komorebi::process_event: processing event: FocusChange for process 10 with notification AXApplicationActivated",
            )
            .event(
                2.0,
                json!({"type": "FocusChange", "content": [{"type": "Accessibility", "content": "AXApplicationActivated"}, 10, null]}),
                Some(json!({"monitors": {"elements": []}})),
            )
            // inside the 250 ms an unknown handler would be assumed to span, but before this one
            .os_record(1.78, "system", 200, "WindowServer", "before the handler began")
            .os_record(1.85, "komorebi", 100, "komorebi", "during the handler")
            .build();
        let analysis = Analysis::open(&session.dir).expect("open");
        let data = analysis
            .os_trace(analysis.events()[0].seq)
            .expect("os trace");

        assert!((data.started_s - 1.8).abs() < 0.01, "{}", data.started_s);
        assert_eq!(data.total, 1, "{data:#?}");
        assert_eq!(data.groups[0].label, "komorebi");
        assert_eq!(data.groups[0].records[0].phase, Phase::During);
        assert!(
            !data.notes.iter().any(|note| note.contains("not known")),
            "{:?}",
            data.notes
        );
    }

    #[test]
    fn a_session_without_an_os_trace_says_so_instead_of_showing_nothing() {
        let analysis = session("without", false);
        let seq = analysis.events()[0].seq;
        let data = analysis.os_trace(seq).expect("os trace");

        assert_eq!(data.total, 0);
        assert!(data.groups.is_empty());
        assert!(
            data.notes.iter().any(|note| note.contains("no OS trace")),
            "{:?}",
            data.notes
        );
    }

    #[test]
    fn on_windows_one_stream_of_everything_is_grouped_by_program() {
        use crate::analysis::flavor::Flavor;

        let (session, _) = SessionBuilder::new("os-trace-windows-groups")
            .flavor(Flavor::Windows)
            .event(
                2.0,
                json!({"type": "FocusChange", "content": ["SystemForeground",
                    {"hwnd": 7, "title": "t", "exe": "chrome.exe", "class": "C",
                     "rect": {"left": 0, "top": 0, "right": 1, "bottom": 1}}]}),
                Some(json!({"monitors": {"elements": []}})),
            )
            .os_record(
                1.95,
                "system",
                100,
                "komorebi.exe",
                "ObjectLocationChange hwnd=0x1",
            )
            .os_record(1.99, "system", 300, "chrome.exe", "ObjectFocus hwnd=0x7")
            .os_record(
                2.10,
                "system",
                300,
                "chrome.exe",
                "ObjectLocationChange hwnd=0x7",
            )
            .os_record(2.20, "system", 500, "notepad.exe", "ObjectShow hwnd=0x9")
            .os_record(
                2.05,
                "eventlog",
                4,
                "System",
                "Microsoft-Windows-Kernel-Power event 506",
            )
            .build();
        let analysis = Analysis::open(&session.dir).expect("open");
        let data = analysis
            .os_trace(analysis.events()[0].seq)
            .expect("os trace");

        let groups: Vec<_> = data
            .groups
            .iter()
            .map(|group| (group.kind, group.label.as_str(), group.total()))
            .collect();
        assert_eq!(
            groups,
            [
                (GroupKind::Komorebi, "komorebi.exe", 1),
                (GroupKind::App, "chrome.exe", 2),
                (GroupKind::App, "notepad.exe", 1),
                (GroupKind::Other, "eventlog", 1),
            ],
            "{data:#?}"
        );
    }

    #[test]
    fn an_event_that_does_not_exist_is_not_found() {
        let analysis = session("missing", true);
        let error = analysis.os_trace(9_999).expect_err("no such event");
        assert_eq!(envelope::exit_kind(&error), envelope::ExitKind::NotFound);
    }
}
