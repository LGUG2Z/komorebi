//! Human-readable rendering of query results. Everything here is a pure function of its inputs so
//! the output format is unit-testable; `--json` bypasses this module entirely.
//!
//! Each function takes the data an `Analysis` question (or the
//! timeline) answered with, and returns the text the command prints. The text is for a person or an
//! agent reading a terminal: its wording is not a stable interface. A program should ask for
//! `--json`, whose shape is named and versioned by the envelope's `schema`.
//!
//! | Command | Function | Renders |
//! |---|---|---|
//! | `at`, `range` | [`records_text`] (with [`group_by_source`]) | the records of a stretch of the recording, a source at a time |
//! | `summary` | [`summary_text`] | a digest of the recording in buckets of time |
//! | `locate` | [`locate_text`] | where a UTC time, log line or event falls in the video |
//! | `annotate`, `annotations`, and parts of `brief` and `overview` | [`annotation_text`] | a bug region, note or finding |
//! | `state` | `state_text` | `StateData` |
//! | `diff` | `diff_text` | `DiffData` |
//! | `events` | `events_text` | `EventsData` |
//! | `code` | `code_text` | `CodeData` |
//! | `trace` | `trace_text` | `TraceData` |
//! | `window` | `history_text` | `HistoryData` |
//! | `brief` | `brief_text` | `BriefData` |
//! | `overview` | `overview_text` | `OverviewData` |
//! | `search` | `search_text` | `SearchData` |
//! | `changes` | `changes_text` | `ChangesData` |
//! | `sheet` | `sheet_text` | `SheetData` |
//!
//! Times are written as timecodes (`00:12.500`, see [`crate::timecode::format`]) in video time.

use crate::analysis::answers::DiffData;
use crate::analysis::answers::EventsData;
use crate::analysis::answers::StateData;
use crate::analysis::brief::BriefData;
use crate::analysis::brief::RegionBrief;
use crate::analysis::changes::ChangesData;
use crate::analysis::code::CodeData;
use crate::analysis::history::HistoryData;
use crate::analysis::invariants::Severity;
use crate::analysis::invariants::Violation;
use crate::analysis::overview::Finding;
use crate::analysis::overview::OverviewData;
use crate::analysis::search::SearchData;
use crate::analysis::sheet::SheetData;
use crate::analysis::state::Container;
use crate::analysis::state::Window;
use crate::analysis::trace::TraceData;
use crate::analysis::trace::TraceLine;
use crate::analysis::views::EventView;
use crate::annotations::AnnotationView;
use crate::annotations::Kind;
use crate::timecode;
use crate::timeline::Bucket;
use crate::timeline::Located;
use crate::timeline::Record;
use crate::timeline::SOURCE_EVENTS;
use crate::timeline::SOURCE_KOMOREBI_LOG;
use crate::timeline::SOURCE_RECORDER;
use std::collections::BTreeMap;
use std::fmt::Write;

/// The order sources are listed in: komorebi's own view first, then the OS trace from the
/// daemon outwards to the apps.
fn source_rank(source: &str) -> (u8, String) {
    let rank = match source {
        // what happened to the recording environment frames everything else
        SOURCE_RECORDER => 0,
        SOURCE_KOMOREBI_LOG => 1,
        SOURCE_EVENTS => 2,
        "oslog:komorebi" => 3,
        "oslog:system" => 4,
        _ if source.starts_with("oslog:app:") => 5,
        _ => 6,
    };
    (rank, source.to_string())
}

/// Groups records by source, in display order.
///
/// The order is the recording environment first (what happened to the machine: displays, sleep),
/// then komorebi's log and events, then the OS trace from the daemon outwards: komorebi's own calls,
/// the system's, then each application's, by name. A source this does not know comes last. The
/// records of a group keep the order they had.
///
/// # Example
///
/// ```rust
/// use komorebi_recorder::report::group_by_source;
/// use komorebi_recorder::timeline::{Record, SOURCE_EVENTS, SOURCE_KOMOREBI_LOG, TimelineEntry};
///
/// let record = |source: &str, text: &str| Record {
///     entry: TimelineEntry {
///         video_s: 1.0,
///         mach_ns: 0,
///         wall_utc: None,
///         source: source.to_string(),
///         pid: None,
///         file: String::new(),
///         offset: 0,
///         len: 0,
///         line: 1,
///         seq: None,
///     },
///     text: text.to_string(),
/// };
///
/// let records = [
///     record("oslog:app:Ghostty", "an app"),
///     record(SOURCE_EVENTS, "an event"),
///     record("oslog:system", "the system"),
///     record(SOURCE_KOMOREBI_LOG, "a log line"),
/// ];
/// let order: Vec<String> = group_by_source(&records)
///     .into_iter()
///     .map(|(source, _)| source)
///     .collect();
/// assert_eq!(order, ["komorebi-log", "events", "oslog:system", "oslog:app:Ghostty"]);
/// ```
#[must_use]
pub fn group_by_source(records: &[Record]) -> Vec<(String, Vec<&Record>)> {
    let mut groups: BTreeMap<(u8, String), Vec<&Record>> = BTreeMap::new();
    for record in records {
        groups
            .entry(source_rank(&record.entry.source))
            .or_default()
            .push(record);
    }

    groups
        .into_iter()
        .map(|((_, source), records)| (source, records))
        .collect()
}

/// Renders the results of `at` and `range`: a heading with the number of records, then the records
/// a source at a time (in the order of [`group_by_source`]).
///
/// `center_s` is the video time the query was about; if it is given, each record also shows how far
/// from it the record is, as `(+0.100s)`. `limit_per_source` is the most records to show of each
/// source (`None` for all); a source with more reports how many it left out and how to see them.
/// With no records the text says nothing was recorded in the span.
///
/// # Example
///
/// ```rust
/// use komorebi_recorder::report::records_text;
/// use komorebi_recorder::timeline::{Record, SOURCE_KOMOREBI_LOG, TimelineEntry};
///
/// let log_line = |video_s: f64, text: &str| Record {
///     entry: TimelineEntry {
///         video_s,
///         mach_ns: 0,
///         wall_utc: None,
///         source: SOURCE_KOMOREBI_LOG.to_string(),
///         pid: None,
///         file: String::new(),
///         offset: 0,
///         len: 0,
///         line: 1,
///         seq: None,
///     },
///     text: text.to_string(),
/// };
///
/// let records = [log_line(12.4, "line a"), log_line(12.6, "line b")];
/// assert_eq!(
///     records_text("00:12.500 +-1s", &records, Some(12.5), Some(1)),
///     "\
/// 00:12.500 +-1s - 2 records
///
/// == komorebi-log (2) ==
/// 00:12.400 (-0.100s) line a
/// ... 1 more komorebi-log records (raise --limit or narrow with --source/--app)
/// ",
/// );
/// ```
#[must_use]
pub fn records_text(
    heading: &str,
    records: &[Record],
    center_s: Option<f64>,
    limit_per_source: Option<usize>,
) -> String {
    let mut out = String::new();
    let _ = writeln!(out, "{heading} - {} records", records.len());

    if records.is_empty() {
        let _ = writeln!(out, "(nothing was recorded in this span)");
        return out;
    }

    for (source, group) in group_by_source(records) {
        let _ = writeln!(out, "\n== {source} ({}) ==", group.len());
        let shown = limit_per_source.map_or(group.len(), |limit| limit.min(group.len()));

        for record in group.iter().take(shown) {
            let time = timecode::format(record.entry.video_s);
            match center_s {
                Some(center) => {
                    let _ = writeln!(
                        out,
                        "{time} ({:+.3}s) {}",
                        record.entry.video_s - center,
                        record.text
                    );
                }
                None => {
                    let _ = writeln!(out, "{time} {}", record.text);
                }
            }
        }

        if shown < group.len() {
            let _ = writeln!(
                out,
                "... {} more {source} records (raise --limit or narrow with --source/--app)",
                group.len() - shown
            );
        }
    }

    out
}

/// Renders a `summary`: a line for each bucket of time.
///
/// By default a line counts the komorebi log lines (`log=`), the komorebi events (`events=`) and the
/// OS trace records of any application or the system (`os=`), and adds `recorder=` only for a
/// bucket in which something happened to the recording environment, so that it stands out. With
/// `by_source` it lists the count of every source instead. A bucket's first and last log line
/// follow it, the last only if it differs from the first.
///
/// # Example
///
/// ```rust
/// use komorebi_recorder::report::summary_text;
/// use komorebi_recorder::timeline::{Bucket, SOURCE_EVENTS, SOURCE_KOMOREBI_LOG};
/// use std::collections::BTreeMap;
///
/// let bucket = Bucket {
///     start_s: 12.0,
///     end_s: 13.0,
///     counts: BTreeMap::from([
///         (SOURCE_KOMOREBI_LOG.to_string(), 4),
///         (SOURCE_EVENTS.to_string(), 2),
///         ("oslog:app:Ghostty".to_string(), 30),
///         ("oslog:system".to_string(), 5),
///     ]),
///     first_log: Some("retiling".to_string()),
///     last_log: Some("retiling".to_string()),
/// };
///
/// assert_eq!(
///     summary_text(&[bucket], false),
///     "00:12.000 log=4 events=2 os=35\n    first: retiling\n",
/// );
/// ```
#[must_use]
pub fn summary_text(buckets: &[Bucket], by_source: bool) -> String {
    let mut out = String::new();
    if buckets.is_empty() {
        out.push_str("(nothing was recorded)\n");
        return out;
    }

    for bucket in buckets {
        let _ = write!(out, "{} ", timecode::format(bucket.start_s));

        if by_source {
            let counts = bucket
                .counts
                .iter()
                .map(|(source, count)| format!("{source}={count}"))
                .collect::<Vec<_>>()
                .join(" ");
            let _ = writeln!(out, "{counts}");
        } else {
            let sum = |predicate: &dyn Fn(&str) -> bool| -> u64 {
                bucket
                    .counts
                    .iter()
                    .filter(|(source, _)| predicate(source))
                    .map(|(_, count)| *count)
                    .sum()
            };
            let _ = write!(
                out,
                "log={} events={} os={}",
                sum(&|source| source == SOURCE_KOMOREBI_LOG),
                sum(&|source| source == SOURCE_EVENTS),
                sum(&|source| source.starts_with("oslog:")),
            );

            // something happened to the recording environment in this second: make it stand out
            let incidents = sum(&|source| source == SOURCE_RECORDER);
            if incidents > 0 {
                let _ = write!(out, " recorder={incidents}");
            }
            let _ = writeln!(out);
        }

        if let Some(first) = &bucket.first_log {
            let _ = writeln!(out, "    first: {first}");
        }

        if let Some(last) = &bucket.last_log
            && bucket.first_log.as_ref() != Some(last)
        {
            let _ = writeln!(out, "    last:  {last}");
        }
    }

    out
}

/// Renders a `locate`: what was found, where it falls in video time and UTC, and where in the video
/// file of each display that is.
///
/// A moment outside the recorded video is said to be. A display that was recorded at some point but
/// had no video then is named, with the reasons that can be.
///
/// # Example
///
/// ```rust
/// use komorebi_recorder::report::locate_text;
/// use komorebi_recorder::timeline::Located;
///
/// let located = Located {
///     description: "event #3".to_string(),
///     video_s: 12.5,
///     mach_ns: 17_500_000_000,
///     wall_utc: Some("2026-10-26T19:43:35.062613Z".to_string()),
///     in_video: true,
///     displays: Vec::new(),
///     without_video: vec![2],
/// };
///
/// let text = locate_text(&located);
/// assert!(text.starts_with("event #3\nvideo time: 00:12.500 (12.500s)\n"));
/// assert!(text.contains("utc:        2026-10-26T19:43:35.062613Z"));
/// assert!(text.contains("display 2: no video at this moment"));
/// ```
#[must_use]
pub fn locate_text(located: &Located) -> String {
    let mut out = String::new();
    let _ = writeln!(out, "{}", located.description);
    let _ = writeln!(
        out,
        "video time: {} ({:.3}s){}",
        timecode::format(located.video_s),
        located.video_s,
        if located.in_video {
            ""
        } else {
            "  (outside the recorded video)"
        }
    );

    if let Some(wall) = &located.wall_utc {
        let _ = writeln!(out, "utc:        {wall}");
    }

    for display in &located.displays {
        let _ = writeln!(
            out,
            "  display {}: {} at {} ({:.3}s)",
            display.display,
            display.file,
            timecode::format(display.file_s),
            display.file_s
        );
    }

    for display in &located.without_video {
        let _ = writeln!(
            out,
            "  display {display}: no video at this moment (unplugged, asleep or its stream had stopped; see the recorder source)"
        );
    }

    out
}

/// Renders one annotation: for a bug, the region and, verbatim, what the user said is wrong.
///
/// The first line says what it is, its id, the span in video time and its length, the displays and
/// the author. The lines after it appear when there is something to say: the title, the text (called
/// "what goes wrong" for a bug), what was expected, that the region is open-ended, that a bug needs
/// a description, what a finding is about and its evidence.
///
/// # Example
///
/// ```rust
/// use komorebi_recorder::annotations::fold;
/// use komorebi_recorder::report::annotation_text;
/// use komorebi_recorder::testkit::{SessionBuilder, mach_ns};
///
/// let (_session, manifest) = SessionBuilder::new("doc-annotation-text").build();
/// let text = format!(
///     r#"{{"v":1,"id":"a-00000001","op":"add","kind":"bug","start_mach_ns":{},"end_mach_ns":{},"text":"the window jumps"}}"#,
///     mach_ns(2.0),
///     mach_ns(5.0),
/// );
/// let bug = fold(&text).annotations.remove(0).view(&manifest);
///
/// assert_eq!(
///     annotation_text(&bug),
///     "bug a-00000001  00:02.000 -> 00:05.000 (3.000s)  any display  by user\n  what goes wrong:\n    the window jumps\n",
/// );
/// ```
#[must_use]
pub fn annotation_text(view: &AnnotationView) -> String {
    let mut out = String::new();
    let displays = if view.displays.is_empty() {
        "any display".to_string()
    } else {
        format!(
            "display {}",
            view.displays
                .iter()
                .map(ToString::to_string)
                .collect::<Vec<_>>()
                .join(", ")
        )
    };

    let _ = writeln!(
        out,
        "{} {}  {} -> {} ({:.3}s)  {displays}  by {}",
        view.kind,
        view.id,
        timecode::format(view.start_video_s),
        timecode::format(view.end_video_s),
        view.end_video_s - view.start_video_s,
        view.author
    );

    let indent = |text: &str| text.lines().collect::<Vec<_>>().join("\n    ");
    if !view.title.is_empty() {
        let _ = writeln!(out, "  title: {}", view.title);
    }
    if !view.text.trim().is_empty() {
        let label = match view.kind {
            Kind::Bug => "what goes wrong",
            Kind::Note => "note",
            Kind::Finding => "finding",
        };
        let _ = writeln!(out, "  {label}:\n    {}", indent(&view.text));
    }
    if let Some(expected) = &view.expected {
        let _ = writeln!(out, "  expected:\n    {}", indent(expected));
    }
    if view.open_ended {
        let _ = writeln!(
            out,
            "  open-ended: no end was set, so it runs to the end of the recording"
        );
    }
    if view.needs_description {
        let _ = writeln!(
            out,
            "  needs a description: add one with `annotate <session> edit {} --text ...`",
            view.id
        );
    }
    if let Some(about) = &view.about {
        let _ = writeln!(out, "  about: {about}");
    }
    if !view.refs.is_empty() {
        let refs: Vec<String> = view.refs.iter().map(ToString::to_string).collect();
        let _ = writeln!(out, "  evidence: {}", refs.join(", "));
    }

    out
}

fn rect_text(rect: &Option<crate::analysis::state::Rect>) -> String {
    rect.as_ref()
        .map_or_else(|| "rect unknown".to_string(), |rect| rect.describe())
}

fn window_line(window: &Window, marker: &str) -> String {
    format!(
        "{marker} window {} {} \"{}\"  {}",
        window.id,
        window.exe,
        window.title,
        rect_text(&window.rect)
    )
}

fn container_lines(out: &mut String, index: usize, container: &Container, focused: bool) {
    for (tab, window) in container.windows.iter().enumerate() {
        let marker = if tab == 0 {
            format!("{index:>3}{}", if focused { "*" } else { " " })
        } else {
            "    ".to_string()
        };
        let front = if container.windows.len() > 1 && tab == container.focused {
            " (front tab)"
        } else {
            ""
        };
        let _ = writeln!(out, "      {}{front}", window_line(window, &marker));
    }
}

fn violation_lines(out: &mut String, violations: &[Violation]) {
    for violation in violations {
        let severity = match violation.severity {
            Severity::Error => "error",
            Severity::Warn => "warn ",
            Severity::Info => "info ",
        };
        let _ = writeln!(
            out,
            "  {severity} {}: {}",
            violation.kind, violation.message
        );
    }
}

/// Renders the answer to `state`: the moment and which notification the state is from, the focus,
/// each monitor with its workspaces and the windows in them, the windows asked about, and what looks
/// wrong.
///
/// The heading says how old the state is. If it is more than five seconds old a note says that
/// komorebi can change state without a notification, so it may be out of date. A workspace that is
/// showing is marked, and workspaces that are not showing and were left out of the answer are listed
/// with their window counts. A window that matched what was asked about says where it is, where it
/// really is and, for a tiled window, where the layout says it should be.
///
/// # Example
///
/// ```rust
/// use komorebi_recorder::analysis::Analysis;
/// use komorebi_recorder::analysis::answers::StateQuery;
/// use komorebi_recorder::report::state_text;
/// use komorebi_recorder::testkit::{SessionBuilder, mach_ns, state_with_windows};
/// use serde_json::json;
///
/// let (session, _) = SessionBuilder::new("doc-state-text")
///     .event(2.0, json!({"type": "Retile"}), Some(state_with_windows(&[(7, "Terminal")])))
///     .build();
/// let analysis = Analysis::open(&session.dir)?;
/// let data = analysis.state_answer(mach_ns(2.5), &StateQuery::default())?;
///
/// let text = state_text(&data);
/// assert!(text.starts_with("state at 00:02.500  (as of event #1, 500 ms before)\n"));
/// assert!(text.contains("workspace one [showing]"));
/// assert!(text.contains("window 7 Terminal"));
/// assert!(text.contains("nothing looks wrong in this state"));
/// # Ok::<(), Box<dyn std::error::Error>>(())
/// ```
#[must_use]
pub fn state_text(data: &StateData) -> String {
    let mut out = String::new();
    let age = data.origin.age_ms.map_or_else(String::new, |age| {
        if age < 1000.0 {
            format!(", {age:.0} ms before")
        } else {
            format!(", {:.1} s before", age / 1000.0)
        }
    });
    let source = match (data.origin.source.as_str(), data.origin.seq) {
        ("event", Some(seq)) => format!("as of event #{seq}{age}"),
        _ => format!("the state when recording began{age}"),
    };
    let _ = writeln!(out, "state at {}  ({source})", data.at.timecode);
    if data.origin.age_ms.is_some_and(|age| age > 5000.0) {
        let _ = writeln!(
            out,
            "note: nothing was reported for a while, and komorebi can change state without a \
             notification, so this may be out of date"
        );
    }

    if let Some(focus) = &data.focus {
        let _ = writeln!(
            out,
            "focus: window {} on monitor {} workspace {}",
            focus
                .window_id
                .map_or_else(|| "(none)".to_string(), |id| id.to_string()),
            focus.monitor_id,
            focus.workspace
        );
    }
    if data.paused {
        let _ = writeln!(out, "komorebi is PAUSED");
    }

    for monitor in &data.monitors {
        let _ = writeln!(
            out,
            "\nmonitor {} \"{}\"  {}",
            monitor.id,
            monitor.device,
            monitor.size.describe()
        );

        for (index, workspace) in monitor.workspaces.iter().enumerate() {
            let showing = data.workspaces.iter().any(|summary| {
                summary.monitor_id == monitor.id
                    && summary.name == workspace.name
                    && summary.showing
            });
            let _ = writeln!(
                out,
                "  workspace {}{}  {}{}",
                workspace.name,
                if showing { " [showing]" } else { "" },
                workspace.layout,
                if workspace.layer == "Floating" {
                    "  [floating layer]"
                } else {
                    ""
                },
            );
            let _ = index;

            for (container_index, container) in workspace.containers.iter().enumerate() {
                container_lines(
                    &mut out,
                    container_index,
                    container,
                    showing && container_index == workspace.focused,
                );
            }
            if let Some(monocle) = &workspace.monocle {
                let _ = writeln!(out, "      monocle:");
                container_lines(&mut out, 0, monocle, true);
            }
            if let Some(window) = &workspace.maximized {
                let _ = writeln!(out, "{}", window_line(window, "      maximized"));
            }
            for window in &workspace.floating {
                let _ = writeln!(out, "{}", window_line(window, "      floating"));
            }
        }
    }

    let hidden: Vec<String> = data
        .workspaces
        .iter()
        .filter(|summary| {
            !summary.showing
                && !data.monitors.iter().any(|monitor| {
                    monitor.id == summary.monitor_id
                        && monitor
                            .workspaces
                            .iter()
                            .any(|workspace| workspace.name == summary.name)
                })
        })
        .map(|summary| format!("{} ({})", summary.name, summary.windows))
        .collect();
    if !hidden.is_empty() {
        let _ = writeln!(
            out,
            "\nnot showing (windows): {}  (--all to list them)",
            hidden.join(", ")
        );
    }

    for found in &data.matches {
        let _ = writeln!(
            out,
            "\nwindow {} {} \"{}\"\n  where:    {}\n  really at: {}{}",
            found.window.id,
            found.window.exe,
            found.window.title,
            found.location.describe(),
            rect_text(&found.window.rect),
            if found.hidden_position {
                "  (parked off screen)"
            } else {
                ""
            },
        );
        if let Some(layout) = &found.layout_rect {
            let _ = writeln!(out, "  layout says: {}", layout.describe());
        }
    }

    if data.violations.is_empty() {
        let _ = writeln!(out, "\nnothing looks wrong in this state");
    } else {
        let _ = writeln!(out, "\nwrong in this state:");
        violation_lines(&mut out, &data.violations);
    }

    out
}

/// Renders the answer to `diff`: the two moments, a line for each change, and the violations that
/// appeared or were resolved between them.
///
/// # Example
///
/// ```rust
/// use komorebi_recorder::analysis::Analysis;
/// use komorebi_recorder::report::diff_text;
/// use komorebi_recorder::testkit::{SessionBuilder, mach_ns, state_with_windows};
/// use serde_json::json;
///
/// let (session, _) = SessionBuilder::new("doc-diff-text")
///     .event(2.0, json!({"type": "Retile"}), Some(state_with_windows(&[(7, "Terminal")])))
///     .event(4.0, json!({"type": "Retile"}), Some(state_with_windows(&[(7, "Terminal"), (8, "Browser")])))
///     .build();
/// let analysis = Analysis::open(&session.dir)?;
///
/// let text = diff_text(&analysis.diff_answer(mach_ns(3.0), mach_ns(5.0))?);
/// assert!(text.starts_with("from 00:03.000  to 00:05.000\n"));
/// assert!(text.contains("Browser \"window 8\" (window 8) appeared on monitor 1 workspace one"));
/// # Ok::<(), Box<dyn std::error::Error>>(())
/// ```
#[must_use]
pub fn diff_text(data: &DiffData) -> String {
    let mut out = String::new();
    let _ = writeln!(out, "from {}  to {}", data.from.timecode, data.to.timecode);
    if data.changes.is_empty() {
        let _ = writeln!(out, "no difference between the two states");
    }
    for change in &data.changes {
        let _ = writeln!(out, "  {}", change.text);
    }
    if !data.violations_appeared.is_empty() {
        let _ = writeln!(out, "\nwrong now, and not before:");
        violation_lines(&mut out, &data.violations_appeared);
    }
    if !data.violations_resolved.is_empty() {
        let _ = writeln!(out, "\nwrong before, and not now:");
        violation_lines(&mut out, &data.violations_resolved);
    }
    out
}

/// Renders the answer to `events`: how many of the session's events matched and a line for each one
/// returned.
///
/// A line has the sequence number, the time, the class, what the event was, where the log has its
/// handler at least how long the handler ran and how many errors (`E`) and warnings (`W`) it logged,
/// and whether the state changed. With `verbose` a line follows each event saying why it was classified
/// as it was.
///
/// # Example
///
/// ```rust
/// use komorebi_recorder::analysis::Analysis;
/// use komorebi_recorder::analysis::answers::EventQuery;
/// use komorebi_recorder::report::events_text;
/// use komorebi_recorder::testkit::SessionBuilder;
/// use serde_json::json;
///
/// let (session, _) = SessionBuilder::new("doc-events-text")
///     .event(2.0, json!({"type": "Retile"}), None)
///     .build();
/// let analysis = Analysis::open(&session.dir)?;
///
/// let text = events_text(&analysis.events_answer(&EventQuery::default()).data, false);
/// assert!(text.contains("\n#1     00:02.000 r  command"));
/// assert!(text.contains("note: 1 of 1 events have no `pushed notification"));
/// # Ok::<(), Box<dyn std::error::Error>>(())
/// ```
#[must_use]
pub fn events_text(data: &EventsData, verbose: bool) -> String {
    let mut out = String::new();
    let _ = writeln!(
        out,
        "{} of {} events{}",
        data.matching,
        data.total,
        data.subscriber
            .as_ref()
            .map_or_else(String::new, |name| format!(
                " (recorded as subscriber {name})"
            ))
    );
    let _ = writeln!(
        out,
        "times: p = when komorebi pushed the notification, r = when the recorder received it (no pushed line in the log){}",
        data.komorebi_time_note
            .as_ref()
            .map_or_else(String::new, |note| format!("\nnote: {note}"))
    );

    for event in &data.events {
        let handler = event.handler.as_ref().map_or_else(String::new, |handler| {
            let ms = handler
                .handler_ms
                .map_or_else(String::new, |ms| format!(" {ms:.0} ms"));
            let problems = if handler.errors + handler.warnings > 0 {
                format!(" {}E/{}W", handler.errors, handler.warnings)
            } else {
                String::new()
            };
            format!("  handler{ms}{problems}")
        });
        let received = received_text(event);
        let state = match event.state_changed {
            Some(true) => "  state changed",
            Some(false) => "  state unchanged",
            None => "",
        };
        let _ = writeln!(
            out,
            "#{:<5} {}{}  {:<9}  {}{handler}{received}{state}",
            event.seq,
            timecode::format(event.komorebi_time.video_s),
            source_marker(event),
            format!("{:?}", event.class).to_lowercase(),
            event.summary,
        );
        if verbose {
            let _ = writeln!(out, "         {}", event.reason);
        }
    }

    out
}

/// Renders the answer to `code`: for each log line asked about, the statement in komorebi's source
/// that wrote it, and for a search, the statements that matched.
///
/// The text opens with a warning if the source is not the version that was recorded. For each log
/// line it gives how sure the match is, the code around it and what the part of komorebi it came
/// from does. A line that no statement in the indexed source explains says so. A match is `exact`, `likely`,
/// `ambiguous` (with how many other statements read the same) or `generic` (the statement only
/// prints a value, so the match says little).
///
/// # Example
///
/// ```rust
/// use komorebi_recorder::analysis::Analysis;
/// use komorebi_recorder::report::code_text;
/// use komorebi_recorder::testkit::SessionBuilder;
///
/// let (session, _) = SessionBuilder::new("doc-code-text")
///     .log(2.0, "INFO", "process_command: komorebi::process_command: processing command: Retile")
///     .build();
/// let analysis = Analysis::open(&session.dir)?;
///
/// // the log line is named whether or not this build has komorebi's source to match it against
/// let data = analysis.code_for_lines("log:1", &[1], 3, None)?;
/// assert!(code_text(&data).contains("log:1  "));
/// # Ok::<(), Box<dyn std::error::Error>>(())
/// ```
#[must_use]
pub fn code_text(data: &CodeData) -> String {
    let mut out = String::new();
    if let Some(warning) = &data.skew.warning {
        let _ = writeln!(out, "WARNING: {warning}\n");
    }

    for line in &data.lines {
        let time = line.video_s.map_or_else(String::new, |seconds| {
            format!("  {}", timecode::format(seconds))
        });
        let _ = writeln!(
            out,
            "log:{}{time}  {}  {}\n  {}",
            line.line,
            line.level.as_deref().unwrap_or(""),
            line.target.as_deref().unwrap_or(""),
            line.message,
        );

        match &line.source {
            Some(source) => {
                let note = match source.confidence {
                    crate::analysis::catalog::Confidence::Exact => "exact".to_string(),
                    crate::analysis::catalog::Confidence::Likely => "likely".to_string(),
                    crate::analysis::catalog::Confidence::Ambiguous => format!(
                        "ambiguous: {} other statement(s) read the same",
                        source.alternatives.len()
                    ),
                    crate::analysis::catalog::Confidence::Generic => {
                        "generic: the statement only prints a value, so this says little"
                            .to_string()
                    }
                };
                let _ = writeln!(
                    out,
                    "  -> {}  in {}  ({note}){}",
                    source.statement.location(),
                    source.statement.qualified_function(),
                    if line.file_changed {
                        "  [this file differs from the recorded komorebi]"
                    } else {
                        ""
                    }
                );
                for other in &source.alternatives {
                    let _ = writeln!(
                        out,
                        "     or {}  in {}",
                        other.location(),
                        other.qualified_function()
                    );
                }
            }
            None => {
                let _ = writeln!(out, "  -> no statement in the indexed source writes this");
            }
        }

        if let Some(snippet) = &line.snippet {
            for (number, text) in &snippet.lines {
                let marker = if *number == snippet.line { ">" } else { " " };
                let _ = writeln!(out, "     {number:>5} {marker} {text}");
            }
        }
        if let Some(role) = &line.role {
            let _ = writeln!(
                out,
                "  role: {role}{}",
                if line.changes_state_silently {
                    "  [CHANGES STATE WITHOUT A NOTIFICATION]"
                } else {
                    ""
                }
            );
        }
        out.push('\n');
    }

    for statement in &data.statements {
        let _ = writeln!(
            out,
            "{}  {}  in {}\n  {} {}",
            statement.location(),
            statement.level,
            statement.qualified_function(),
            statement.target,
            statement.template
        );
    }

    out
}

fn trace_line_text(out: &mut String, line: &TraceLine) {
    let time = line.video_s.map_or_else(String::new, |seconds| {
        format!(" {}", timecode::format(seconds))
    });
    let _ = writeln!(
        out,
        "  log:{:<6}{time} {:<5} {}{}",
        line.line,
        line.level.as_deref().unwrap_or(""),
        if line.spans.is_empty() {
            String::new()
        } else {
            format!("[{}] ", line.spans)
        },
        line.message
    );
    if let Some(source) = &line.source {
        let _ = writeln!(
            out,
            "          from {} in {}{}{}",
            source.location,
            source.function,
            if source.file_changed {
                "  [file differs from the recorded komorebi]"
            } else {
                ""
            },
            if line.changes_state_silently {
                "  [changes state without a notification]"
            } else {
                ""
            }
        );
    }
}

/// Renders the answer to `trace`: the event, what its handler logged, and each change to the state
/// with who is held to have made it.
///
/// The text also has what other threads logged meanwhile, the evidence for each attribution, and
/// what looks wrong after the event that did not before. When the data has them, it adds what
/// Windows said about the event's window (a recording made with `--probe-windows`), the OS trace
/// around the event (`trace --os`) grouped by where it came from, and notes such as a slow handler.
///
/// # Example
///
/// ```rust
/// use komorebi_recorder::analysis::Analysis;
/// use komorebi_recorder::report::trace_text;
/// use komorebi_recorder::testkit::{SessionBuilder, state_with_windows};
/// use serde_json::json;
///
/// let (session, _) = SessionBuilder::new("doc-trace-text")
///     .event(2.0, json!({"type": "Retile"}), Some(state_with_windows(&[(7, "Terminal")])))
///     .build();
/// let analysis = Analysis::open(&session.dir)?;
///
/// let text = trace_text(&analysis.trace(1)?);
/// assert!(text.starts_with("event #1  received 00:02.000  Retile\n  command ("));
/// # Ok::<(), Box<dyn std::error::Error>>(())
/// ```
#[must_use]
pub fn trace_text(data: &TraceData) -> String {
    let mut out = String::new();
    let event = &data.event;
    let _ = writeln!(
        out,
        "event #{}  received {}  {}\n  {} ({})",
        event.seq,
        timecode::format(event.video_s),
        event.summary,
        format!("{:?}", event.class).to_lowercase(),
        event.reason
    );
    if let Some(handler) = &event.handler {
        let problems = if handler.errors + handler.warnings > 0 {
            format!(
                ", {} error(s), {} warning(s)",
                handler.errors, handler.warnings
            )
        } else {
            String::new()
        };
        let tail = format!("{}{problems}", pushed_text(handler));
        let _ = writeln!(
            out,
            "  handler logged {} line(s) from {}, began {}{}",
            handler.lines,
            timecode::format(handler.started_video_s),
            handler.handler_ms.map_or_else(String::new, |ms| {
                if handler
                    .delivery_ms
                    .is_some_and(|delivery| delivery >= DELIVERY_NOTE_MS)
                {
                    format!("{ms:.0} ms before komorebi sent the notification")
                } else {
                    format!("{ms:.0} ms before it was received")
                }
            }),
            tail
        );
    }

    if !data.handler.is_empty() {
        let _ = writeln!(out, "\nwhat the handler logged:");
        for line in &data.handler {
            trace_line_text(&mut out, line);
        }
    }
    if !data.ambient.is_empty() {
        let _ = writeln!(out, "\nlogged by other threads meanwhile:");
        for line in &data.ambient {
            trace_line_text(&mut out, line);
        }
    }

    if data.changes.is_empty() {
        let _ = writeln!(out, "\nthe state did not change");
    } else {
        let _ = writeln!(out, "\nwhat changed, and who did it:");
        for traced in &data.changes {
            let attribution = &traced.attribution;
            let _ = writeln!(
                out,
                "  {}\n      -> {} ({}): {}",
                traced.change.text, attribution.actor, attribution.confidence, attribution.reason
            );
            if !attribution.evidence.is_empty() {
                let lines: Vec<String> = attribution
                    .evidence
                    .iter()
                    .map(|line| format!("log:{line}"))
                    .collect();
                let _ = writeln!(out, "         evidence: {}", lines.join(", "));
            }
        }
    }

    if !data.violations_appeared.is_empty() {
        let _ = writeln!(out, "\nwrong after this event, and not before:");
        violation_lines(&mut out, &data.violations_appeared);
    }
    if !data.violations_resolved.is_empty() {
        let _ = writeln!(out, "\nwrong before this event, and not after:");
        violation_lines(&mut out, &data.violations_resolved);
    }
    if let Some(probe) = &data.window_probe {
        let facts = &probe.facts;
        let _ = writeln!(
            out,
            "\nwhat Windows said about window {} ({:.0} ms after the event):",
            facts.hwnd, probe.read_after_ms
        );
        let _ = writeln!(
            out,
            "  exists {}, visible {}, cloaked {}, minimized {}{}",
            facts.exists,
            facts.visible,
            facts.cloaked,
            facts.minimized,
            facts
                .rect
                .map_or_else(String::new, |[left, top, right, bottom]| format!(
                    ", at {}x{} ({left}, {top})",
                    right - left,
                    bottom - top
                ))
        );
        if let Some(says) = &facts.komorebi_says {
            let _ = writeln!(out, "  komorebi says:");
            for line in says.lines() {
                let _ = writeln!(out, "    {line}");
            }
        }
        if probe.violations.is_empty() {
            let _ = writeln!(out, "  which agrees with komorebi's state");
        } else {
            violation_lines(&mut out, &probe.violations);
        }
    }
    if let Some(os) = &data.os_trace {
        os_trace_text(&mut out, os);
    }
    if !data.notes.is_empty() {
        let _ = writeln!(out, "\nnotes:");
        for note in &data.notes {
            let _ = writeln!(out, "  - {note}");
        }
    }

    out
}

/// The OS trace around an event, grouped by where it came from.
fn os_trace_text(out: &mut String, os: &crate::analysis::os_trace::OsTraceData) {
    use crate::analysis::os_trace::Phase;

    let _ = writeln!(
        out,
        "\nthe OS trace around it: {} record(s) from {} (the handler began) to {}, of which {} \
         after it was received:",
        os.total,
        timecode::format(os.started_s),
        timecode::format(os.until_s),
        os.groups.iter().map(|group| group.after).sum::<usize>(),
    );
    for group in &os.groups {
        let pid = group
            .pid
            .map_or_else(String::new, |pid| format!(" [{pid}]"));
        let _ = writeln!(
            out,
            "  {}{pid}: {} during the handler, {} after",
            group.label, group.during, group.after
        );
        for record in &group.records {
            let _ = writeln!(
                out,
                "    {}{} {}",
                timecode::format(record.video_s),
                if record.phase == Phase::After {
                    "+"
                } else {
                    " "
                },
                record.text
            );
        }
        if group.omitted() > 0 {
            let _ = writeln!(out, "    ... {} more ({})", group.omitted(), group.source);
        }
    }
    if !os.overlapping_events.is_empty() {
        let seqs: Vec<String> = os
            .overlapping_events
            .iter()
            .map(|seq| format!("#{seq}"))
            .collect();
        let _ = writeln!(out, "  other events in this window: {}", seqs.join(", "));
    }
    for note in &os.notes {
        let _ = writeln!(out, "  - {note}");
    }
}

/// Renders the answer to `window`: the windows the query matched, then an entry for each event that
/// came with a change to one of them.
///
/// An entry says where the window was afterwards, where it really was and what changed. An entry for
/// an event that was about the window but changed nothing about it says so. If more
/// entries matched than the data holds, the last line says how many more.
///
/// # Example
///
/// ```rust
/// use komorebi_recorder::analysis::Analysis;
/// use komorebi_recorder::report::history_text;
/// use komorebi_recorder::testkit::{SessionBuilder, state_with_windows};
/// use serde_json::json;
///
/// let (session, _) = SessionBuilder::new("doc-history-text")
///     .event(2.0, json!({"type": "Retile"}), Some(state_with_windows(&[(7, "Terminal")])))
///     .build();
/// let analysis = Analysis::open(&session.dir)?;
///
/// let text = history_text(&analysis.window_history("Terminal", None, None)?);
/// assert!(text.starts_with("'Terminal' matched: Terminal"));
/// # Ok::<(), Box<dyn std::error::Error>>(())
/// ```
#[must_use]
pub fn history_text(data: &HistoryData) -> String {
    let mut out = String::new();
    let names: Vec<String> = data
        .windows
        .iter()
        .map(|window| format!("{} \"{}\" (window {})", window.exe, window.title, window.id))
        .collect();
    let _ = writeln!(out, "'{}' matched: {}", data.query, names.join("; "));

    for entry in &data.entries {
        let place = entry
            .location
            .as_ref()
            .map_or_else(|| "gone".to_string(), |location| location.describe());
        let _ = writeln!(
            out,
            "#{:<5} {}  window {}  {}\n         after: {}  {}{}",
            entry.seq,
            timecode::format(entry.video_s),
            entry.window_id,
            entry.event,
            place,
            rect_text(&entry.rect),
            if entry.hidden {
                "  (parked off screen)"
            } else {
                ""
            },
        );
        for change in &entry.changes {
            let _ = writeln!(out, "         - {change}");
        }
        if entry.changes.is_empty() && entry.about_window {
            let _ = writeln!(
                out,
                "         - the event was about this window; nothing about it changed"
            );
        }
    }

    if data.matching > data.entries.len() {
        let _ = writeln!(out, "... {} more", data.matching - data.entries.len());
    }

    out
}

/// How long a notification must have been in transit for the text to say when komorebi sent it.
const DELIVERY_NOTE_MS: f64 = 100.0;

/// `  pushed 00:01.850`: when komorebi sent the notification, if it was well before it was received.
fn pushed_text(handler: &crate::analysis::views::HandlerView) -> String {
    match (handler.pushed_video_s, handler.delivery_ms) {
        (Some(pushed), Some(delivery)) if delivery >= DELIVERY_NOTE_MS => {
            format!(
                "  pushed {} ({delivery:.0} ms before received)",
                timecode::format(pushed)
            )
        }
        _ => String::new(),
    }
}

/// `p` after a time when it is when komorebi pushed the notification, `r` when it is when the
/// recorder received it (the log has no pushed line, so that is all there is).
fn source_marker(event: &EventView) -> &'static str {
    match event.komorebi_time.source {
        crate::analysis::views::TimeSource::Pushed => " p",
        crate::analysis::views::TimeSource::Received => " r",
    }
}

/// `  received 00:05.000`: when the recorder got the notification, if that was well after komorebi
/// pushed it.
fn received_text(event: &EventView) -> String {
    let delay_ms = (event.video_s - event.komorebi_time.video_s) * 1000.0;
    if event.komorebi_time.source == crate::analysis::views::TimeSource::Pushed
        && delay_ms >= DELIVERY_NOTE_MS
    {
        format!(
            "  received {} ({delay_ms:.0} ms after pushed)",
            timecode::format(event.video_s)
        )
    } else {
        String::new()
    }
}

fn event_line(event: &EventView) -> String {
    let handler = event.handler.as_ref().map_or_else(String::new, |handler| {
        let ms = handler
            .handler_ms
            .map_or_else(String::new, |ms| format!(" {ms:.0} ms"));
        let problems = if handler.errors + handler.warnings > 0 {
            format!(" {}E/{}W", handler.errors, handler.warnings)
        } else {
            String::new()
        };
        format!("  handler{ms}{problems}")
    });
    let state = match event.state_changed {
        Some(true) => "  state changed",
        _ => "",
    };
    format!(
        "#{:<5} {}  {:<9}  {}{handler}{state}",
        event.seq,
        timecode::format(event.video_s),
        format!("{:?}", event.class).to_lowercase(),
        event.summary
    )
}

fn finding_line(finding: &Finding) -> String {
    let severity = match finding.severity {
        Severity::Error => "error",
        Severity::Warn => "warn ",
        Severity::Info => "info ",
    };
    let bugs = if finding.bugs.is_empty() {
        String::new()
    } else {
        "  [in or just before a bug region]".to_string()
    };
    format!(
        "{severity} {}  {}  {}{bugs}",
        timecode::format(finding.start_video_s),
        finding.kind,
        finding.summary
    )
}

fn region_text(out: &mut String, region: &RegionBrief) {
    let _ = writeln!(
        out,
        "\nlooking at {} to {} (the region, from a few seconds before it)",
        region.from.timecode, region.to.timecode
    );

    let classes: Vec<String> = region
        .events
        .by_class
        .iter()
        .map(|(class, count)| format!("{count} {class}"))
        .collect();
    let _ = writeln!(
        out,
        "events: {} ({}{})",
        region.events.total,
        classes.join(", "),
        if region.events.echoes_hidden > 0 {
            format!(
                "; {} echoes of komorebi's own moves not listed",
                region.events.echoes_hidden
            )
        } else {
            String::new()
        }
    );
    for event in &region.events.listed {
        let _ = writeln!(out, "  {}", event_line(event));
    }
    if region.events.truncated {
        let _ = writeln!(out, "  ... more: use `events` with --from/--to to page");
    }

    if !region.suspects.is_empty() {
        let _ = writeln!(out, "\nmost worth tracing first:");
        for (index, suspect) in region.suspects.iter().enumerate() {
            let _ = writeln!(
                out,
                "  {}. event #{} {} (score {}): {}\n     {}",
                index + 1,
                suspect.event.seq,
                suspect.event.summary,
                suspect.score,
                suspect.reasons.join("; "),
                suspect.next
            );
        }
    }

    if !region.findings.is_empty() {
        let _ = writeln!(out, "\nwhat looked wrong or unusual:");
        for finding in &region.findings {
            let _ = writeln!(out, "  {}", finding_line(finding));
        }
    }

    for (label, state) in [
        ("as it began", &region.state_at_start),
        ("as it ended", &region.state_at_end),
    ] {
        match state {
            Some(state) => {
                let _ = writeln!(
                    out,
                    "\nkomorebi's state {label} ({}): {}",
                    state.at.timecode, state.summary
                );
                if state.violations.is_empty() {
                    let _ = writeln!(out, "  nothing looks wrong");
                } else {
                    violation_lines(out, &state.violations);
                }
            }
            None => {
                let _ = writeln!(out, "\nkomorebi's state {label}: not recorded");
            }
        }
    }

    if !region.changes.is_empty() {
        let _ = writeln!(
            out,
            "\nwhat changed between them ({} total):",
            region.changes_total
        );
        for change in &region.changes {
            let _ = writeln!(out, "  {}", change.text);
        }
    }

    if !region.incidents.is_empty() {
        let _ = writeln!(out, "\nwhat happened to the machine nearby:");
        for incident in &region.incidents {
            let _ = writeln!(
                out,
                "  {}  {}{}: {}",
                timecode::format(incident.video_s),
                incident.kind,
                incident
                    .display
                    .map_or_else(String::new, |display| format!(" (display {display})")),
                incident.detail
            );
        }
    }
}

/// Renders the answer to `brief`: what the user says is wrong comes first, word for word.
///
/// The text has, in order: each bug region as the user reported it (or, if none was marked, a
/// line saying so); the facts of the session and any warnings, including that komorebi's source
/// is not the version that was recorded; for each region, what the recording shows around it and
/// the commands to run next (or, with no region, the same over the whole recording); conclusions
/// already recorded and notes; and what to do next.
///
/// # Example
///
/// ```rust
/// use komorebi_recorder::analysis::Analysis;
/// use komorebi_recorder::annotations::{Kind, New, add};
/// use komorebi_recorder::report::brief_text;
/// use komorebi_recorder::testkit::{SessionBuilder, mach_ns};
/// use serde_json::json;
///
/// let (session, _) = SessionBuilder::new("doc-brief-text")
///     .event(2.0, json!({"type": "Retile"}), None)
///     .build();
/// add(&session.dir, New {
///     kind: Some(Kind::Bug),
///     start_mach_ns: mach_ns(1.0),
///     end_mach_ns: Some(mach_ns(3.0)),
///     text: "the window jumps".to_string(),
///     ..New::default()
/// })?;
///
/// let text = brief_text(&Analysis::open(&session.dir)?.brief()?);
/// // the user's words come first
/// assert!(text.starts_with("BUG, as the user reported it:\nbug "));
/// assert!(text.contains("\n    the window jumps\n"));
/// # Ok::<(), Box<dyn std::error::Error>>(())
/// ```
#[must_use]
pub fn brief_text(data: &BriefData) -> String {
    let mut out = String::new();

    if data.bugs.is_empty() {
        let _ = writeln!(out, "NO BUG REGION WAS MARKED in this recording.\n");
    }
    for bug in &data.bugs {
        let _ = writeln!(out, "BUG, as the user reported it:");
        out.push_str(&annotation_text(&bug.bug));
    }

    let session = &data.session;
    let displays: Vec<String> = session
        .displays
        .iter()
        .map(|display| {
            format!(
                "{} ({}x{}, {} segment(s))",
                display.id, display.width_px, display.height_px, display.segments
            )
        })
        .collect();
    let _ = writeln!(
        out,
        "\nsession {}\n  {} s of video, displays: {}\n  komorebi: {}{}\n  {} events, {} log lines, {} incidents; video sync: {}",
        session.dir,
        session.duration_s as i64,
        if displays.is_empty() {
            "none".to_string()
        } else {
            displays.join(", ")
        },
        if session.komorebi.is_empty() {
            "version unknown".to_string()
        } else {
            session.komorebi.join("; ")
        },
        session
            .komorebi_log_level
            .as_ref()
            .map_or_else(String::new, |level| format!(", logging at {level}")),
        session.events,
        session.log_lines,
        session.incidents,
        session.sync_quality
    );
    for warning in &session.warnings {
        let _ = writeln!(out, "  warning: {warning}");
    }
    if let Some(warning) = &data.skew.warning {
        let _ = writeln!(out, "\nWARNING: {warning}");
    }

    for bug in &data.bugs {
        let _ = writeln!(out, "\n=== {} ===", bug.bug.id);
        region_text(&mut out, &bug.region);
        let _ = writeln!(out, "\nnext:");
        for command in &bug.commands {
            let _ = writeln!(out, "  {command}");
        }
    }
    if let Some(region) = &data.whole_session {
        let _ = writeln!(out, "\n=== the whole recording ===");
        region_text(&mut out, region);
    }

    if !data.findings_recorded.is_empty() {
        let _ = writeln!(out, "\nconclusions already recorded:");
        for finding in &data.findings_recorded {
            out.push_str(&annotation_text(finding));
        }
    }
    if !data.notes.is_empty() {
        let _ = writeln!(out, "\nnotes:");
        for note in &data.notes {
            out.push_str(&annotation_text(note));
        }
    }

    if !data.next.is_empty() {
        let _ = writeln!(out, "\n");
        for line in &data.next {
            let _ = writeln!(out, "{line}");
        }
    }

    out
}

/// Renders the answer to `overview`: the session's size, the bug regions the user marked, the
/// findings most worth looking at first, and a histogram of when things happened.
///
/// Each finding has the command to dig into it. The histogram has a row for each bucket of time with something in it: a bar scaled to the
/// busiest bucket's number of events, then the counts of events, log errors and incidents.
///
/// # Example
///
/// ```rust
/// use komorebi_recorder::analysis::Analysis;
/// use komorebi_recorder::report::overview_text;
/// use komorebi_recorder::testkit::SessionBuilder;
/// use serde_json::json;
///
/// let (session, _) = SessionBuilder::new("doc-overview-text")
///     .event(2.0, json!({"type": "Retile"}), None)
///     .build();
/// let text = overview_text(&Analysis::open(&session.dir)?.overview(None)?);
/// assert!(text.contains(": 10 s of video, 1 events, 0 log lines, 0 incidents"));
/// assert!(text.contains("when things happened (events | errors | incidents):"));
/// # Ok::<(), Box<dyn std::error::Error>>(())
/// ```
#[must_use]
pub fn overview_text(data: &OverviewData) -> String {
    let mut out = String::new();
    let session = &data.session;
    let _ = writeln!(
        out,
        "session {}: {} s of video, {} events, {} log lines, {} incidents",
        session.dir,
        session.duration_s as i64,
        session.events,
        session.log_lines,
        session.incidents
    );

    for bug in &data.bugs {
        let _ = writeln!(out, "\nbug region:");
        out.push_str(&annotation_text(bug));
    }

    let _ = writeln!(
        out,
        "\nfindings, most worth looking at first ({} of {}):",
        data.findings.len(),
        data.findings_total
    );
    for finding in &data.findings {
        let _ = writeln!(out, "  {}", finding_line(finding));
        if let Some(next) = &finding.next {
            let _ = writeln!(out, "         {next}");
        }
    }

    let peak = data
        .histogram
        .iter()
        .map(|bucket| bucket.events)
        .max()
        .unwrap_or(0)
        .max(1);
    let _ = writeln!(out, "\nwhen things happened (events | errors | incidents):");
    for bucket in &data.histogram {
        if bucket.events + bucket.log_errors + bucket.incidents == 0 {
            continue;
        }
        let bar = "#".repeat((bucket.events * 30).div_ceil(peak));
        let _ = writeln!(
            out,
            "  {} {:<30} {:>4} {:>3} {:>2}",
            timecode::format(bucket.start_s),
            bar,
            bucket.events,
            bucket.log_errors,
            bucket.incidents
        );
    }

    out
}

/// Renders the answer to `search`: how many hits there were and where it looked, then a line for
/// each hit.
///
/// A line has the hit's time, its source, the reference to give another command (`log:345`,
/// `event:12`) and the first line of its text.
///
/// # Example
///
/// ```rust
/// use komorebi_recorder::analysis::Analysis;
/// use komorebi_recorder::analysis::budget::Budget;
/// use komorebi_recorder::analysis::search::SearchQuery;
/// use komorebi_recorder::report::search_text;
/// use komorebi_recorder::testkit::SessionBuilder;
///
/// let (session, _) = SessionBuilder::new("doc-search-text")
///     .log(2.0, "INFO", "komorebi::process_command: retiling the workspace")
///     .build();
/// let analysis = Analysis::open(&session.dir)?;
///
/// let found = analysis.search(
///     &SearchQuery { pattern: "retiling".to_string(), ..SearchQuery::default() },
///     &Budget::unlimited(),
/// )?;
/// let text = search_text(&found);
/// assert!(text.starts_with("1 hit(s) for 'retiling' in "));
/// assert!(text.contains("log:1"));
/// # Ok::<(), Box<dyn std::error::Error>>(())
/// ```
#[must_use]
pub fn search_text(data: &SearchData) -> String {
    let mut out = String::new();
    let _ = writeln!(
        out,
        "{} hit(s) for '{}' in {}",
        data.matching,
        data.query,
        data.searched.join(", ")
    );
    for hit in &data.hits {
        let _ = writeln!(
            out,
            "{}  {:<14} {:<22} {}",
            timecode::format(hit.video_s),
            hit.source,
            hit.reference,
            hit.text.lines().next().unwrap_or("")
        );
    }
    out
}

/// Renders the answer to `changes`: how many frames were compared, then a line for each stretch of
/// change, and the stretches in which a display had no video.
///
/// A stretch of change has its display, times and length, the most of the screen that changed
/// between two frames, the box around what changed as percentages of the screen, and the events
/// received during it.
///
/// # Example
///
/// ```rust
/// use komorebi_recorder::analysis::changes::{ChangeSpan, ChangesData, Gap};
/// use komorebi_recorder::report::changes_text;
///
/// let data = ChangesData {
///     from_video_s: 0.0,
///     to_video_s: 4.0,
///     displays: vec![1],
///     frames_examined: 120,
///     spans: vec![ChangeSpan {
///         display: 1,
///         start_video_s: 1.0,
///         end_video_s: 1.5,
///         frames: 15,
///         peak_share: 0.25,
///         bbox: [0.5, 0.0, 0.5, 1.0],
///         bbox_points: None,
///         events: vec![3],
///     }],
///     gaps: vec![Gap { display: 1, from_video_s: 3.0, to_video_s: 4.0 }],
/// };
///
/// let text = changes_text(&data);
/// assert!(text.starts_with("120 frames compared on display(s) 1 from 00:00.000 to 00:04.000: 1 stretch(es) of change\n"));
/// assert!(text.contains("up to 25.0% of the screen"));
/// assert!(text.ends_with("display 1 has no video from 00:03.000 to 00:04.000\n"));
/// ```
#[must_use]
pub fn changes_text(data: &ChangesData) -> String {
    let mut out = String::new();
    let _ = writeln!(
        out,
        "{} frames compared on display(s) {} from {} to {}: {} stretch(es) of change",
        data.frames_examined,
        data.displays
            .iter()
            .map(ToString::to_string)
            .collect::<Vec<_>>()
            .join(", "),
        timecode::format(data.from_video_s),
        timecode::format(data.to_video_s),
        data.spans.len()
    );
    for span in &data.spans {
        let events = if span.events.is_empty() {
            String::new()
        } else {
            format!(
                "  events {}",
                span.events
                    .iter()
                    .map(|seq| format!("#{seq}"))
                    .collect::<Vec<_>>()
                    .join(" ")
            )
        };
        let _ = writeln!(
            out,
            "display {}  {} - {} ({:.2} s, {} frames)  up to {:.1}% of the screen  box {:.0}%,{:.0}% {:.0}%x{:.0}%{events}",
            span.display,
            timecode::format(span.start_video_s),
            timecode::format(span.end_video_s),
            span.end_video_s - span.start_video_s,
            span.frames,
            span.peak_share * 100.0,
            span.bbox[0] * 100.0,
            span.bbox[1] * 100.0,
            span.bbox[2] * 100.0,
            span.bbox[3] * 100.0,
        );
    }
    for gap in &data.gaps {
        let _ = writeln!(
            out,
            "display {} has no video from {} to {}",
            gap.display,
            timecode::format(gap.from_video_s),
            timecode::format(gap.to_video_s)
        );
    }
    out
}

/// Renders the answer to `sheet`: where the image was written, its size and how many frames it
/// has, then a line for each moment that had no frame and why.
///
/// # Example
///
/// ```rust
/// use komorebi_recorder::analysis::sheet::{SheetData, Skipped};
/// use komorebi_recorder::report::sheet_text;
///
/// let data = SheetData {
///     path: "sheets/00-01.000-00-09.000.png".to_string(),
///     width: 1450,
///     height: 560,
///     frames: Vec::new(),
///     skipped: vec![Skipped {
///         display: 2,
///         video_s: 0.5,
///         reason: "the display had no video then".to_string(),
///     }],
/// };
///
/// assert_eq!(
///     sheet_text(&data),
///     "sheets/00-01.000-00-09.000.png  (1450x560, 0 frames)\nno frame: display 2 at 00:00.500: the display had no video then\n",
/// );
/// ```
#[must_use]
pub fn sheet_text(data: &SheetData) -> String {
    let mut out = String::new();
    let _ = writeln!(
        out,
        "{}  ({}x{}, {} frames)",
        data.path,
        data.width,
        data.height,
        data.frames.len()
    );
    for skipped in &data.skipped {
        let _ = writeln!(
            out,
            "no frame: display {} at {}: {}",
            skipped.display,
            timecode::format(skipped.video_s),
            skipped.reason
        );
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::timeline::TimelineEntry;

    fn record(source: &str, video_s: f64, text: &str) -> Record {
        Record {
            entry: TimelineEntry {
                video_s,
                mach_ns: 0,
                wall_utc: None,
                source: source.to_string(),
                pid: None,
                file: String::new(),
                offset: 0,
                len: 0,
                line: 1,
                seq: None,
            },
            text: text.to_string(),
        }
    }

    #[test]
    fn groups_in_display_order() {
        let records = vec![
            record("oslog:app:Ghostty", 1.0, "g"),
            record("oslog:system", 1.0, "s"),
            record(SOURCE_EVENTS, 1.0, "e"),
            record("oslog:komorebi", 1.0, "k"),
            record(SOURCE_KOMOREBI_LOG, 1.0, "l"),
            record("oslog:app:Brave", 1.0, "b"),
            record(SOURCE_RECORDER, 1.0, "r"),
        ];

        let order: Vec<String> = group_by_source(&records)
            .into_iter()
            .map(|(source, _)| source)
            .collect();
        assert_eq!(
            order,
            [
                SOURCE_RECORDER,
                SOURCE_KOMOREBI_LOG,
                SOURCE_EVENTS,
                "oslog:komorebi",
                "oslog:system",
                "oslog:app:Brave",
                "oslog:app:Ghostty",
            ]
        );
    }

    #[test]
    fn at_output_shows_offsets_and_truncates_per_source() {
        let records = vec![
            record(SOURCE_KOMOREBI_LOG, 12.4, "line a"),
            record(SOURCE_KOMOREBI_LOG, 12.5, "line b"),
            record(SOURCE_KOMOREBI_LOG, 12.6, "line c"),
        ];
        let text = records_text("00:12.500 +-1s", &records, Some(12.5), Some(2));

        assert!(text.contains("3 records"));
        assert!(text.contains("== komorebi-log (3) =="));
        assert!(text.contains("00:12.400 (-0.100s) line a"), "{text}");
        assert!(text.contains("00:12.600 (+0.100s)") || text.contains("... 1 more"));
        assert!(text.contains("... 1 more komorebi-log records"));
        assert!(!text.contains("line c"));
    }

    #[test]
    fn empty_results_say_so() {
        assert!(records_text("x", &[], None, None).contains("nothing was recorded"));
    }

    #[test]
    fn summary_collapses_oslog_sources_unless_asked() {
        let bucket = Bucket {
            start_s: 12.0,
            end_s: 13.0,
            counts: BTreeMap::from([
                (SOURCE_KOMOREBI_LOG.to_string(), 4),
                (SOURCE_EVENTS.to_string(), 2),
                ("oslog:app:Ghostty".to_string(), 30),
                ("oslog:system".to_string(), 5),
            ]),
            first_log: Some("first".to_string()),
            last_log: Some("last".to_string()),
        };

        let collapsed = summary_text(std::slice::from_ref(&bucket), false);
        assert!(
            collapsed.contains("00:12.000 log=4 events=2 os=35"),
            "{collapsed}"
        );
        assert!(collapsed.contains("first: first"));
        assert!(collapsed.contains("last:  last"));

        assert!(!collapsed.contains("recorder="), "no incidents, no column");

        let detailed = summary_text(&[bucket], true);
        assert!(detailed.contains("oslog:app:Ghostty=30"));
    }

    #[test]
    fn summary_flags_a_second_in_which_the_environment_changed() {
        let bucket = Bucket {
            start_s: 40.0,
            end_s: 41.0,
            counts: BTreeMap::from([
                (SOURCE_KOMOREBI_LOG.to_string(), 1),
                (SOURCE_RECORDER.to_string(), 2),
            ]),
            first_log: None,
            last_log: None,
        };

        let text = summary_text(&[bucket], false);
        assert!(
            text.contains("00:40.000 log=1 events=0 os=0 recorder=2"),
            "{text}"
        );
    }
}
