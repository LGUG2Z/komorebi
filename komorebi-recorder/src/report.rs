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
