//! Reading a session back: `at`, `range`, `summary`, `locate` and `frames`.
//!
//! These read the session's timeline directly (see [`crate::timeline`]), not through an `Analysis`,
//! so they say what was recorded without interpreting it as the output of one komorebi or the
//! other. A time is a video timecode ([`crate::timecode`]) or any other form of
//! [`TimeSpec`](crate::analysis::timespec::TimeSpec): `logtime:15:46:22.743`, `wall:...`, `mach:...`,
//! `event:12`, `bug:...`, with an offset such as `+3s`.
//!
//! | Command | Asks | Prints |
//! |---|---|---|
//! | `at <time>` | [`timeline::query`] around the time (`--window`, default 1 s, either side) | the records found, a source at a time |
//! | `range <from> <to>` | [`timeline::query`] between two times | the same |
//! | `summary` | [`timeline::summary`] | a line for each bucket of time (`--bucket`) |
//! | `locate` | [`timeline::locate`] | where a UTC time, log line or event falls in video time and in each display's file |
//! | `frames <times>` | [`frames::extract`] | the path of a PNG for each time and display |
//!
//! `--app` and `--source` narrow what `at`, `range` and `summary` read. `--limit` is the most
//! records shown per source in text, and `0` means no limit. A finished session is required: one
//! still being recorded, or whose recorder died, is refused with what to do about it (`reindex`).
//!
//! With `--json` these commands print their data itself (the records, the buckets, the location),
//! not inside the envelope the investigation commands use; a failure is still reported as an
//! envelope.

use super::limit;
use super::load;
use super::print_json;
use crate::analysis::Analysis;
use crate::analysis::timespec::Anchor;
use crate::analysis::timespec::Edge;
use crate::analysis::timespec::Lookup;
use crate::analysis::timespec::TimeSpec;
use crate::frames;
use crate::report;
use crate::timecode;
use crate::timeline;
use crate::timeline::Filter;
use crate::timeline::Locator;
use chrono::DateTime;
use chrono::Utc;
use clap::Args;
use color_eyre::eyre;
use color_eyre::eyre::WrapErr;
use serde::Serialize;
use std::path::PathBuf;

/// The options `at` and `range` share: what to look at, and how to print it.
#[derive(Args, Debug)]
pub struct FilterArgs {
    /// Only this application's OS trace (exe name, case-insensitive, or a pid)
    #[arg(long, value_name = "EXE|PID")]
    pub app: Option<String>,
    /// Only these sources: komorebi-log, events, oslog, oslog:komorebi, oslog:system,
    /// oslog:app, `oslog:app:<exe>` (repeatable)
    #[arg(long = "source", value_name = "SOURCE")]
    pub sources: Vec<String>,
    /// Machine-readable JSON output
    #[arg(long)]
    pub json: bool,
    /// Most records to print per source in text mode (0 for no limit)
    #[arg(long, default_value_t = 100)]
    pub limit: usize,
}

impl FilterArgs {
    fn filter(&self) -> Filter {
        Filter {
            app: self.app.clone(),
            sources: self.sources.clone(),
        }
    }
}

/// The arguments of `at`.
#[derive(Args, Debug)]
pub struct AtArgs {
    /// Session directory
    pub session: PathBuf,
    /// Video timecode (12.5, 00:12.500, 1:02:03), or logtime:15:46:22.743, wall:..., event:12, ...
    pub time: String,
    /// How far either side of the timecode to look
    #[arg(long, default_value = "1s")]
    pub window: String,
    /// What to look at, and how to print it
    #[command(flatten)]
    pub filter: FilterArgs,
}

/// The arguments of `range`.
#[derive(Args, Debug)]
pub struct RangeArgs {
    /// Session directory
    pub session: PathBuf,
    /// Start: a video timecode, or logtime:15:46:22.743, wall:..., event:12, ...
    pub from: String,
    /// End: the same forms
    pub to: String,
    /// What to look at, and how to print it
    #[command(flatten)]
    pub filter: FilterArgs,
}

/// The arguments of `summary`.
#[derive(Args, Debug)]
pub struct SummaryArgs {
    /// Session directory
    pub session: PathBuf,
    /// Bucket size
    #[arg(long, default_value = "1s")]
    pub bucket: String,
    /// List every source's count instead of log/events/os totals
    #[arg(long)]
    pub by_source: bool,
    /// Only this application's OS trace
    #[arg(long, value_name = "EXE|PID")]
    pub app: Option<String>,
    /// Only these sources (repeatable)
    #[arg(long = "source", value_name = "SOURCE")]
    pub sources: Vec<String>,
    /// Machine-readable JSON output
    #[arg(long)]
    pub json: bool,
}

/// The arguments of `locate`: exactly one of `--utc`, `--log-line` and `--event-seq`.
#[derive(Args, Debug)]
#[command(group(clap::ArgGroup::new("what").required(true).multiple(false)))]
pub struct LocateArgs {
    /// Session directory
    pub session: PathBuf,
    /// A UTC time (RFC 3339), e.g. 2026-10-26T19:43:35.062613Z, or a time of day from the log,
    /// e.g. logtime:15:46:22.743
    #[arg(long, group = "what")]
    pub utc: Option<String>,
    /// A 1-based line number in the session's komorebi.log
    #[arg(long, group = "what")]
    pub log_line: Option<u64>,
    /// A subscriber event sequence number from events.jsonl
    #[arg(long, group = "what")]
    pub event_seq: Option<u64>,
    /// Machine-readable JSON output
    #[arg(long)]
    pub json: bool,
}

/// The arguments of `frames`.
#[derive(Args, Debug)]
pub struct FramesArgs {
    /// Session directory
    pub session: PathBuf,
    /// Video timecodes to extract
    #[arg(required = true)]
    pub times: Vec<String>,
    /// Only this display
    #[arg(long, value_name = "ID")]
    pub display: Option<u32>,
    /// Machine-readable JSON output
    #[arg(long)]
    pub json: bool,
}

#[derive(Serialize)]
struct QueryOutput<'a> {
    session: String,
    from_s: f64,
    to_s: f64,
    #[serde(skip_serializing_if = "Option::is_none")]
    center_s: Option<f64>,
    count: usize,
    records: &'a [timeline::Record],
}

/// When `--app` matched nothing, say which applications this session does have.
fn hint_unknown_app(manifest: &crate::session::Manifest, filter: &FilterArgs, empty: bool) {
    let Some(app) = &filter.app else {
        return;
    };

    let known = timeline::app_names(manifest);
    let is_known =
        app.parse::<i64>().is_ok() || known.iter().any(|name| name.eq_ignore_ascii_case(app));
    if empty && !is_known {
        eprintln!(
            "note: no application '{app}' was traced in this session; traced: {}",
            if known.is_empty() {
                "(none)".to_string()
            } else {
                known.join(", ")
            }
        );
    }
}

/// A lookup for names that point at nothing in the session: `event:12` is not understood here, and
/// `time_arg` opens the whole analysis for those instead.
struct NoItems;

impl NoItems {
    fn refuse<T>() -> eyre::Result<T> {
        Err(crate::analysis::envelope::usage(
            "this time names an item of the session",
        ))
    }
}

impl Lookup for NoItems {
    fn event_mach_ns(&self, _: u64, _: Edge) -> eyre::Result<u64> {
        Self::refuse()
    }
    fn log_mach_ns(&self, _: u64) -> eyre::Result<u64> {
        Self::refuse()
    }
    fn incident_mach_ns(&self, _: u64) -> eyre::Result<u64> {
        Self::refuse()
    }
    fn annotation_span(&self, _: &str) -> eyre::Result<(u64, Option<u64>)> {
        Self::refuse()
    }
}

/// Reads a time argument as seconds of video time.
///
/// Any [`TimeSpec`] is accepted: a video timecode (which is read exactly as [`timecode::parse`]
/// reads it, so existing usage is unchanged), a log time, `wall:`, `mach:`, and the names of events,
/// log lines, incidents and annotations, each with an optional offset. What does not parse as a
/// `TimeSpec` is tried as a timecode before giving up, and the `TimeSpec` error, which says what the
/// forms are, is the one reported.
fn time_arg(
    text: &str,
    dir: &std::path::Path,
    manifest: &crate::session::Manifest,
) -> eyre::Result<f64> {
    let spec = match text.parse::<TimeSpec>() {
        Ok(spec) => spec,
        Err(error) => return timecode::parse(text).map_err(|_| error),
    };

    match spec.anchor {
        // no clamping or round trip through the master clock: the same number as ever
        Anchor::Video(seconds) => Ok(seconds + spec.offset_s),
        Anchor::Item { .. } => Ok(Analysis::open(dir)?.resolve(text)?.video_s),
        _ => Ok(spec.resolve(manifest, &NoItems)?.video_s),
    }
}

pub(super) fn at(args: &AtArgs) -> eyre::Result<()> {
    let (session, manifest) = load(&args.session)?;
    let center = time_arg(&args.time, &args.session, &manifest)?;
    let window = timecode::parse_span(&args.window)?;
    let (from, to) = (center - window, center + window);

    let records = timeline::query(&session, &manifest, from, to, &args.filter.filter())?;

    if args.filter.json {
        return print_json(&QueryOutput {
            session: session.dir.display().to_string(),
            from_s: from,
            to_s: to,
            center_s: Some(center),
            count: records.len(),
            records: &records,
        });
    }

    hint_unknown_app(&manifest, &args.filter, records.is_empty());

    let heading = format!(
        "{} (window +-{window}s: {} to {})",
        timecode::format(center),
        timecode::format(from),
        timecode::format(to)
    );
    print!(
        "{}",
        report::records_text(&heading, &records, Some(center), limit(args.filter.limit))
    );
    Ok(())
}

pub(super) fn range(args: &RangeArgs) -> eyre::Result<()> {
    let (session, manifest) = load(&args.session)?;
    let from = time_arg(&args.from, &args.session, &manifest)?;
    let to = time_arg(&args.to, &args.session, &manifest)?;
    if to < from {
        eyre::bail!(
            "the range end {} is before its start {}",
            args.to,
            args.from
        );
    }

    let records = timeline::query(&session, &manifest, from, to, &args.filter.filter())?;

    if args.filter.json {
        return print_json(&QueryOutput {
            session: session.dir.display().to_string(),
            from_s: from,
            to_s: to,
            center_s: None,
            count: records.len(),
            records: &records,
        });
    }

    hint_unknown_app(&manifest, &args.filter, records.is_empty());

    let heading = format!("{} to {}", timecode::format(from), timecode::format(to));
    print!(
        "{}",
        report::records_text(&heading, &records, None, limit(args.filter.limit))
    );
    Ok(())
}

pub(super) fn summary(args: &SummaryArgs) -> eyre::Result<()> {
    let (session, manifest) = load(&args.session)?;
    let bucket = timecode::parse_span(&args.bucket)?;
    let filter = Filter {
        app: args.app.clone(),
        sources: args.sources.clone(),
    };
    let buckets = timeline::summary(&session, &manifest, bucket, &filter)?;

    if args.json {
        return print_json(&buckets);
    }

    print!("{}", report::summary_text(&buckets, args.by_source));
    Ok(())
}

pub(super) fn locate(args: &LocateArgs) -> eyre::Result<()> {
    let (session, manifest) = load(&args.session)?;
    let locator = if let Some(utc) = &args.utc {
        let time: DateTime<Utc> = if utc.starts_with("logtime:") || utc.starts_with("log-time:") {
            // a time of day from the log: on the session's date, through the master clock
            let moment = time_arg(utc, &args.session, &manifest)?;
            let wall_us = crate::clock::ClockMap::new(manifest.anchors.clone())
                .mach_to_wall(manifest.mach_at_video_seconds(moment))
                .ok_or_else(|| {
                    crate::analysis::envelope::unusable("the session has no clock anchors")
                })?;
            DateTime::from_timestamp_micros(wall_us).ok_or_else(|| {
                crate::analysis::envelope::unusable(format!("{wall_us} is not a time"))
            })?
        } else {
            DateTime::parse_from_rfc3339(utc)
                .wrap_err_with(|| {
                    format!(
                        "'{utc}' is not an RFC 3339 time like 2026-10-26T19:43:35.062613Z, or a log time like logtime:15:46:22.743"
                    )
                })?
                .with_timezone(&Utc)
        };
        Locator::Utc(time)
    } else if let Some(line) = args.log_line {
        Locator::LogLine(line)
    } else if let Some(seq) = args.event_seq {
        Locator::EventSeq(seq)
    } else {
        eyre::bail!("one of --utc, --log-line or --event-seq is required");
    };

    let located = timeline::locate(&session, &manifest, &locator)?;
    if args.json {
        return print_json(&located);
    }

    print!("{}", report::locate_text(&located));
    Ok(())
}

pub(super) fn frames_command(args: &FramesArgs) -> eyre::Result<()> {
    let (session, manifest) = load(&args.session)?;
    let times = args
        .times
        .iter()
        .map(|time| time_arg(time, &args.session, &manifest))
        .collect::<eyre::Result<Vec<_>>>()?;

    let extraction = frames::extract(&session, &manifest, &times, args.display)?;
    if args.json {
        return print_json(&extraction);
    }

    for frame in &extraction.frames {
        println!(
            "{}  display {}  {}",
            timecode::format(frame.video_s),
            frame.display,
            frame.path.display()
        );
    }

    for skipped in &extraction.skipped {
        eprintln!(
            "note: {}  display {}: {}",
            timecode::format(skipped.video_s),
            skipped.display,
            skipped.reason
        );
    }

    Ok(())
}
