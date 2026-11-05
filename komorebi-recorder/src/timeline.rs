//! The merged, sorted index over every raw file in a session, and the queries on top of it.
//!
//! `timeline.jsonl` holds one line per record: its time on every clock plus a *reference*
//! (`file`, `offset`, `len`) into a raw file. Raw data is never copied or rewritten, so the index
//! can be rebuilt at any time (`reindex`) and the streams stay separate.
//!
//! Queries binary-search the sorted file by byte offset, so asking about one second of a
//! multi-gigabyte session does not load the index into memory.
//!
//! # The pieces
//!
//! | Item | Does |
//! |---|---|
//! | [`build`] | reads the raw files and the manifest, and writes `timeline.jsonl` |
//! | [`query`] | the records whose time is in a range of video time, optionally narrowed by a [`Filter`] |
//! | [`summary`] | a per-bucket digest of the whole recording ([`Bucket`]) |
//! | [`locate`] | the reverse: a [`Locator`] (a UTC time, a log line, an event) to its video time ([`Located`]) |
//! | [`render`], [`strip_ansi`], [`format_wall_us`] | pure formatting of a record, of text, and of a time |
//!
//! This module reads a session and is the same on every host and for either komorebi flavor. The
//! host that recorded it shows only in what is in the raw files (the OS trace of a Mac, or of
//! Windows) and in the tick size of the manifest's timebase. Which komorebi produced the log and
//! events is not looked at here: it is a matter for `analysis`.
//!
//! # The file: `timeline.jsonl`
//!
//! The index is UTF-8, one JSON object per line ([`TimelineEntry`]), each line ending in `\n`, and
//! is sorted by `mach_ns`, ascending. Records with the same `mach_ns` are ordered by the order in
//! which their source was first met while building (`komorebi-log`, `events`, `recorder`, then the
//! OS trace files in order of path), then by file, then by `offset`. [`build`] writes it to
//! `timeline.jsonl.tmp` and renames that over `timeline.jsonl`, so a reader never sees a
//! half-written index.
//!
//! | Field | JSON | Is |
//! |---|---|---|
//! | `video_s` | number | the record's time in video time: seconds since the first captured frame, negative before it. It is the manifest's `video_seconds` of `mach_ns` as the manifest was when the index was built |
//! | `mach_ns` | integer | the record's time on the master clock, in nanoseconds (see [`crate::clock`]) |
//! | `wall_utc` | string or `null` | the same time as RFC 3339 UTC with microseconds, `2027-01-15T08:00:02.000000Z`, worked out from the manifest's anchors. `null` if that cannot be done, which is if the manifest has none |
//! | `source` | string | which stream the record is from, see below |
//! | `pid` | integer, left out if unknown | the process the record is about: an OS trace record's `processID`, or the process id a window manager event carries |
//! | `file` | string | the raw file, relative to the session directory, always with `/` separators |
//! | `offset` | integer | the byte offset in `file` of the start of the record |
//! | `len` | integer | how many bytes the record is, including its line terminator |
//! | `line` | integer | the record's 1-based line number in `file` |
//! | `seq` | integer, left out for all but events | the subscriber event sequence number |
//!
//! `file`, `offset` and `len` are the reference: the raw record is `len` bytes at `offset` of
//! `file`, with a trailing `\n` or `\r\n` removed when it is shown. Nothing but the reference
//! points into the raw files, so a raw file edited by hand needs a `reindex`, and an index older
//! than its raw files can point into the wrong bytes.
//!
//! # The sources
//!
//! | `source` | Raw file | Time of a record |
//! |---|---|---|
//! | `komorebi-log` ([`SOURCE_KOMOREBI_LOG`]) | `komorebi.log` | the line's UTC time, through the manifest's anchors ([`crate::clock::ClockMap`]) |
//! | `events` ([`SOURCE_EVENTS`]) | `events.jsonl` | `recv_mach_ns`, stamped on the master clock when the recorder received it |
//! | `recorder` ([`SOURCE_RECORDER`]) | `recorder.jsonl` | the incident's `mach_ns` (see [`crate::incidents`]) |
//! | `oslog:komorebi`, `oslog:system`, `oslog:eventlog`, `oslog:all`, `oslog:app:<exe>` | every `.ndjson` file in `oslog/` and `oslog/apps/` | `hostNs` if the line has one (the Windows OS trace), else its `machTimestamp` ticks converted with the manifest's timebase |
//!
//! The OS trace sources keep the `oslog:` prefix on every host, though on Windows it is not the
//! unified log. A file's source is the one the manifest lists for it; for a file it does not list,
//! such as one added by hand, it comes from the file's name: `komorebi.ndjson` and
//! `komorebi-<pid>.ndjson` are `oslog:komorebi`, `system.ndjson`, `eventlog.ndjson` and
//! `all.ndjson` are `oslog:system`, `oslog:eventlog` and `oslog:all`, and any other is
//! `oslog:app:<exe>`, with a trailing `-<pid>` taken off.
//!
//! A line is left out of the index if it has no time: a line of `komorebi.log` before the first
//! timestamped one, and every line of it if the manifest has no anchors; a row of `events.jsonl`
//! or `recorder.jsonl` that is not valid JSON of the right shape, such as one cut short by a crash;
//! an OS trace line with neither `hostNs` nor `machTimestamp`. Continuation lines of
//! `komorebi.log` (a panic message, a backtrace) are in the index, with the time of the line they
//! follow.
//!
//! # Cost
//!
//! [`build`] holds one compact record per index entry in memory while it sorts, and reads
//! `komorebi.log` whole. [`query`] does not load the index: it finds its starting place by binary
//! search on byte offsets, and reads only the lines in the range. [`summary`] reads the whole
//! index, one line at a time. [`locate`] does not use the index at all: it reads `komorebi.log` or
//! `events.jsonl`, and the manifest.
//!
//! # Example
//!
//! Building the index of a small session, and asking it questions:
//!
//! ```rust
//! use chrono::Utc;
//! use komorebi_recorder::clock::Anchor;
//! use komorebi_recorder::session::{Manifest, Moment, Session, TIMELINE_FILE};
//! use komorebi_recorder::timeline::{self, Filter, Locator, TimelineEntry};
//!
//! let dir = std::env::temp_dir().join(format!("komorebi-recorder-doc-timeline-{}", std::process::id()));
//! let _ = std::fs::remove_dir_all(&dir);
//! let session = Session::create(Some(dir.clone()), Utc::now())?;
//!
//! // the wall clock read 2027-01-15T08:00:00Z when the master clock read 100 s, which is also
//! // when the first video frame was captured: video time 0 is master clock time 100 s
//! let anchor = Anchor { wall_us: 1_800_000_000_000_000, mach_ns: 100_000_000_000 };
//! let start = Moment { mach_ns: anchor.mach_ns, wall_us: anchor.wall_us };
//! let mut manifest = Manifest::new(start, vec![anchor]);
//! manifest.video_t0_mach_ns = Some(anchor.mach_ns);
//! manifest.video_end_mach_ns = Some(110_000_000_000);
//! session.write_manifest(&manifest)?;
//!
//! // komorebi's log is stamped with the wall clock: this line is 2 s into the video
//! std::fs::write(
//!     session.path("komorebi.log"),
//!     "2027-01-15T08:00:02.000000Z  INFO komorebi::process_event: processing event: Show\n",
//! )?;
//! // an event is stamped with the master clock when it is received, 10 ms later
//! std::fs::write(
//!     session.path("events.jsonl"),
//!     concat!(
//!         r#"{"seq":1,"recv_wall_us":1800000002010000,"recv_mach_ns":102010000000,"#,
//!         r#""event":{"type":"Show","content":[{"type":"Accessibility","content":"AXWindowCreated"},111,7]}}"#,
//!         "\n",
//!     ),
//! )?;
//!
//! let stats = timeline::build(&session)?;
//! assert_eq!(stats.entries, 2);
//! assert_eq!(stats.by_source["komorebi-log"], 1);
//! assert_eq!(stats.by_source["events"], 1);
//!
//! // the index: one entry per record, sorted by time, pointing into the raw files
//! let index = std::fs::read_to_string(session.path(TIMELINE_FILE))?;
//! let entries: Vec<TimelineEntry> = index
//!     .lines()
//!     .map(serde_json::from_str)
//!     .collect::<Result<_, _>>()?;
//! assert_eq!(entries[0].source, "komorebi-log");
//! assert_eq!(entries[0].video_s, 2.0);
//! assert_eq!(entries[0].wall_utc.as_deref(), Some("2027-01-15T08:00:02.000000Z"));
//! assert_eq!((entries[0].file.as_str(), entries[0].offset, entries[0].line), ("komorebi.log", 0, 1));
//! assert_eq!(entries[1].source, "events");
//! assert_eq!(entries[1].pid, Some(111));
//! assert_eq!(entries[1].seq, Some(1));
//!
//! // what happened between 1.5 s and 2.5 s into the video?
//! let records = timeline::query(&session, &manifest, 1.5, 2.5, &Filter::default())?;
//! assert_eq!(records.len(), 2);
//! assert!(records[0].text.ends_with("processing event: Show"));
//! assert!(records[1].text.starts_with("#1 {"));
//!
//! // only the events
//! let only_events = Filter { sources: vec!["events".to_string()], ..Filter::default() };
//! assert_eq!(timeline::query(&session, &manifest, 0.0, 10.0, &only_events)?.len(), 1);
//!
//! // and the reverse: where in the video is line 1 of komorebi's log?
//! let located = timeline::locate(&session, &manifest, &Locator::LogLine(1))?;
//! assert_eq!(located.video_s, 2.0);
//! assert!(located.in_video);
//! # std::fs::remove_dir_all(&dir)?;
//! # Ok::<(), Box<dyn std::error::Error>>(())
//! ```

use crate::clock::ClockMap;
use crate::incidents::INCIDENTS_FILE;
use crate::incidents::Incident;
use crate::session::Manifest;
use crate::session::Session;
use crate::session::TIMELINE_FILE;
use crate::sources::komorebi_events::EventRow;
use crate::sources::komorebi_log;
use crate::sources::managed_apps;
use crate::sources::oslog;
use crate::sources::oslog::OsLogRecord;
use chrono::DateTime;
use chrono::Utc;
use color_eyre::eyre;
use color_eyre::eyre::WrapErr;
use serde::Deserialize;
use serde::Serialize;
use std::collections::BTreeMap;
use std::collections::HashMap;
use std::fs::File;
use std::io::BufRead;
use std::io::BufReader;
use std::io::BufWriter;
use std::io::Read;
use std::io::Seek;
use std::io::SeekFrom;
use std::io::Write;

/// The timeline source of komorebi's log: `komorebi-log`.
pub const SOURCE_KOMOREBI_LOG: &str = "komorebi-log";
/// The timeline source of the subscriber's events: `events`.
pub const SOURCE_EVENTS: &str = "events";
/// The timeline source of incidents: `recorder` (see [`crate::incidents`]).
pub use crate::incidents::SOURCE_RECORDER;

/// One line of `timeline.jsonl`: a record's time on every clock, and where the record is.
///
/// The fields are written in the order they are declared here. The format of the file, with what
/// each field means, is in the [module docs](self#the-file-timelinejsonl).
///
/// # Example
///
/// ```rust
/// use komorebi_recorder::timeline::TimelineEntry;
///
/// // `pid` and `seq` are left out when there is none, and `wall_utc` is `null` when unknown
/// let entry = TimelineEntry {
///     video_s: 2.5,
///     mach_ns: 102_500_000_000,
///     wall_utc: None,
///     source: "recorder".to_string(),
///     pid: None,
///     file: "recorder.jsonl".to_string(),
///     offset: 120,
///     len: 98,
///     line: 2,
///     seq: None,
/// };
/// assert_eq!(
///     serde_json::to_string(&entry)?,
///     r#"{"video_s":2.5,"mach_ns":102500000000,"wall_utc":null,"source":"recorder","file":"recorder.jsonl","offset":120,"len":98,"line":2}"#
/// );
///
/// // and a line reads back as it was written
/// let back: TimelineEntry = serde_json::from_str(&serde_json::to_string(&entry)?)?;
/// assert_eq!(back, entry);
/// # Ok::<(), serde_json::Error>(())
/// ```
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TimelineEntry {
    /// Seconds since the first video frame (negative before it).
    pub video_s: f64,
    /// The record's time on the master clock, in nanoseconds.
    pub mach_ns: u64,
    /// RFC 3339 UTC with microseconds.
    pub wall_utc: Option<String>,
    /// Which stream the record is from: `komorebi-log`, `events`, `recorder`, or an OS trace
    /// source starting `oslog:`.
    pub source: String,
    /// The process the record is about, where it has one: an OS trace record's `processID`, or the
    /// process id carried by a window manager event.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pid: Option<i64>,
    /// Raw file, relative to the session directory.
    pub file: String,
    /// The byte offset of the start of the record in `file`.
    pub offset: u64,
    /// How many bytes the record is in `file`, including its line terminator.
    pub len: u64,
    /// 1-based line number in the raw file.
    pub line: u64,
    /// The subscriber event sequence number, for `events` records.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub seq: Option<u64>,
}

/// A timeline entry together with the raw text it points at.
///
/// A query returns these. Serialized, a record is its entry's fields followed by `text`.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Record {
    /// The index entry.
    #[serde(flatten)]
    pub entry: TimelineEntry,
    /// The raw record, rendered for reading (see [`render`]).
    pub text: String,
}

/// Compact in-memory form used only while building.
struct Pending {
    mach_ns: u64,
    source: u32,
    file: u32,
    pid: Option<i64>,
    offset: u64,
    len: u64,
    line: u64,
    seq: Option<u64>,
}

#[derive(Default)]
struct Interner {
    names: Vec<String>,
    ids: HashMap<String, u32>,
}

impl Interner {
    fn id(&mut self, name: &str) -> u32 {
        if let Some(id) = self.ids.get(name) {
            return *id;
        }

        let id = self.names.len() as u32;
        self.names.push(name.to_string());
        self.ids.insert(name.to_string(), id);
        id
    }
}

/// What [`build`] put in the index.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct BuildStats {
    /// How many entries the index has.
    pub entries: u64,
    /// How many of those are from each source, by source name. A source with no entries is not
    /// listed.
    pub by_source: BTreeMap<String, u64>,
}

fn wall_utc(clock: &ClockMap, mach_ns: u64) -> Option<String> {
    let wall_us = clock.mach_to_wall(mach_ns)?;
    format_wall_us(wall_us)
}

/// Returns a wall clock time as RFC 3339 UTC with microseconds and a `Z`, or `None` if it is
/// beyond what can be represented as a date.
///
/// The argument is in microseconds since the Unix epoch, as `wall_us` is throughout a session. This
/// is the form of `wall_utc` in the index.
///
/// # Example
///
/// ```rust
/// use komorebi_recorder::timeline::format_wall_us;
///
/// assert_eq!(
///     format_wall_us(1_800_000_000_250_000).as_deref(),
///     Some("2027-01-15T08:00:00.250000Z")
/// );
/// assert_eq!(format_wall_us(0).as_deref(), Some("1970-01-01T00:00:00.000000Z"));
/// assert_eq!(format_wall_us(i64::MAX), None);
/// ```
#[must_use]
pub fn format_wall_us(wall_us: i64) -> Option<String> {
    DateTime::<Utc>::from_timestamp_micros(wall_us)
        .map(|time| time.format("%Y-%m-%dT%H:%M:%S%.6fZ").to_string())
}

/// Which timeline source an unknown file under `oslog/` belongs to, from its name alone. Used for
/// hand-added files and when the manifest does not list a stream.
fn source_for_oslog_file(relative: &str) -> String {
    let name = relative.strip_prefix("oslog/").unwrap_or(relative);
    match name {
        "komorebi.ndjson" => "oslog:komorebi".to_string(),
        _ if name.starts_with("komorebi-") && name.ends_with(".ndjson") => {
            "oslog:komorebi".to_string()
        }
        "system.ndjson" => "oslog:system".to_string(),
        "eventlog.ndjson" => "oslog:eventlog".to_string(),
        "all.ndjson" => "oslog:all".to_string(),
        _ => {
            let stem = name
                .strip_prefix("apps/")
                .unwrap_or(name)
                .trim_end_matches(".ndjson");
            let exe = stem.rsplit_once('-').map_or(stem, |(exe, _pid)| exe);
            format!("oslog:app:{exe}")
        }
    }
}

fn list_oslog_files(session: &Session) -> Vec<String> {
    let mut files = Vec::new();
    for sub in ["oslog", "oslog/apps"] {
        let Ok(entries) = std::fs::read_dir(session.path(sub)) else {
            continue;
        };

        for entry in entries.flatten() {
            let name = entry.file_name().to_string_lossy().into_owned();
            if name.ends_with(".ndjson") {
                files.push(format!("{sub}/{name}"));
            }
        }
    }

    files.sort();
    files
}

/// Builds `timeline.jsonl` from the raw files and the manifest.
///
/// Every raw file the session has is read (see the sources in the [module docs](self#the-sources)),
/// each record is given its time on the master clock, and the entries are written sorted, replacing
/// any existing index. A raw file the session does not have is skipped, so a session with no
/// events, say, has no `events` entries. The raw files are only read. The index is written to
/// `timeline.jsonl.tmp` and renamed over `timeline.jsonl`, so the old index stays whole until the
/// new one is.
///
/// Building twice from the same raw files and manifest gives the same index (the test
/// `reindex_is_idempotent_and_recovers_a_deleted_timeline`).
///
/// # Errors
///
/// Returns an error if
///
/// * the manifest cannot be read (see [`Session::manifest`]);
/// * a raw file that exists cannot be read;
/// * `timeline.jsonl.tmp` cannot be created or written;
/// * the finished file cannot be renamed over `timeline.jsonl`.
///
/// # Example
///
/// See the [module example](self#example), which builds the index of a session and reads it back.
pub fn build(session: &Session) -> eyre::Result<BuildStats> {
    let manifest = session.manifest()?;
    let clock = ClockMap::new(manifest.anchors.clone());
    let mut sources = Interner::default();
    let mut files = Interner::default();
    let mut pending: Vec<Pending> = Vec::new();

    // komorebi's own log: UTC wall clock -> mach via the anchors
    let log_path = session.path("komorebi.log");
    if log_path.is_file() {
        let bytes = std::fs::read(&log_path)?;
        let source = sources.id(SOURCE_KOMOREBI_LOG);
        let file = files.id("komorebi.log");
        for line in komorebi_log::index_lines(&bytes) {
            let Some(timestamp) = line.timestamp else {
                continue;
            };
            let Some(mach_ns) = clock.wall_to_mach(timestamp.timestamp_micros()) else {
                continue;
            };

            pending.push(Pending {
                mach_ns,
                source,
                file,
                pid: None,
                offset: line.offset,
                len: line.len,
                line: line.number,
                seq: None,
            });
        }
    }

    // subscriber events: stamped on the mach clock at receipt
    let events_path = session.path("events.jsonl");
    if events_path.is_file() {
        let source = sources.id(SOURCE_EVENTS);
        let file = files.id("events.jsonl");
        let mut reader = BufReader::new(File::open(&events_path)?);
        let mut buffer = String::new();
        let (mut offset, mut number) = (0_u64, 0_u64);
        loop {
            buffer.clear();
            let read = reader.read_line(&mut buffer)?;
            if read == 0 {
                break;
            }

            number += 1;
            if let Ok(row) = serde_json::from_str::<EventRow>(&buffer) {
                pending.push(Pending {
                    mach_ns: row.recv_mach_ns,
                    source,
                    file,
                    pid: row
                        .event
                        .as_ref()
                        .and_then(managed_apps::event_pid)
                        .map(i64::from),
                    offset,
                    len: read as u64,
                    line: number,
                    seq: Some(row.seq),
                });
            }

            offset += read as u64;
        }
    }

    // incidents (display changes, sleep/wake, restarts): stamped on the mach clock when noticed
    let incidents_path = session.path(INCIDENTS_FILE);
    if incidents_path.is_file() {
        let source = sources.id(SOURCE_RECORDER);
        let file = files.id(INCIDENTS_FILE);
        let mut reader = BufReader::new(File::open(&incidents_path)?);
        let mut buffer = String::new();
        let (mut offset, mut number) = (0_u64, 0_u64);
        loop {
            buffer.clear();
            let read = reader.read_line(&mut buffer)?;
            if read == 0 {
                break;
            }

            number += 1;
            if let Ok(incident) = serde_json::from_str::<Incident>(&buffer) {
                pending.push(Pending {
                    mach_ns: incident.mach_ns,
                    source,
                    file,
                    pid: None,
                    offset,
                    len: read as u64,
                    line: number,
                    seq: None,
                });
            }

            offset += read as u64;
        }
    }

    // unified log streams: already on the mach clock
    let manifest_sources: HashMap<&str, &str> = manifest
        .oslog
        .iter()
        .map(|info| (info.spec.file.as_str(), info.spec.source.as_str()))
        .collect();

    for relative in list_oslog_files(session) {
        let source_name = manifest_sources.get(relative.as_str()).map_or_else(
            || source_for_oslog_file(&relative),
            |source| (*source).to_string(),
        );
        let source = sources.id(&source_name);
        let file = files.id(&relative);

        let mut reader = BufReader::with_capacity(256 * 1024, File::open(session.path(&relative))?);
        let mut buffer = Vec::new();
        let (mut offset, mut number) = (0_u64, 0_u64);
        loop {
            buffer.clear();
            let read = reader.read_until(b'\n', &mut buffer)?;
            if read == 0 {
                break;
            }

            number += 1;
            // converted with the session's own timebase, not this machine's: a session recorded on
            // one OS is read on another
            if let Some(mach_ns) = oslog::extract_time_ns(&buffer, manifest.timebase) {
                pending.push(Pending {
                    mach_ns,
                    source,
                    file,
                    pid: extract_pid(&buffer),
                    offset,
                    len: read as u64,
                    line: number,
                    seq: None,
                });
            }

            offset += read as u64;
        }
    }

    pending.sort_by_key(|entry| (entry.mach_ns, entry.source, entry.file, entry.offset));

    let path = session.path(TIMELINE_FILE);
    let temporary = session.path(format!("{TIMELINE_FILE}.tmp"));
    let mut writer = BufWriter::new(
        File::create(&temporary)
            .wrap_err_with(|| format!("could not create {}", temporary.display()))?,
    );

    let mut stats = BuildStats::default();
    for entry in &pending {
        let source = &sources.names[entry.source as usize];
        let row = TimelineEntry {
            video_s: manifest.video_seconds(entry.mach_ns),
            mach_ns: entry.mach_ns,
            wall_utc: wall_utc(&clock, entry.mach_ns),
            source: source.clone(),
            pid: entry.pid,
            file: files.names[entry.file as usize].clone(),
            offset: entry.offset,
            len: entry.len,
            line: entry.line,
            seq: entry.seq,
        };

        writeln!(writer, "{}", serde_json::to_string(&row)?)?;
        stats.entries += 1;
        *stats.by_source.entry(source.clone()).or_default() += 1;
    }

    writer.flush()?;
    drop(writer);
    std::fs::rename(&temporary, &path)?;
    Ok(stats)
}

/// Pull `"processID":<digits>` out of a raw ndjson line.
fn extract_pid(line: &[u8]) -> Option<i64> {
    const KEY: &[u8] = b"\"processID\":";
    let start = line.windows(KEY.len()).position(|window| window == KEY)? + KEY.len();
    let digits: Vec<u8> = line[start..]
        .iter()
        .copied()
        .take_while(|byte| byte.is_ascii_digit() || *byte == b'-')
        .collect();

    std::str::from_utf8(&digits).ok()?.parse().ok()
}

/// Narrowing for queries: which sources, and which application, to keep.
///
/// The default keeps every record. When both fields are set, a record must pass both.
///
/// # Example
///
/// The two fields combine, and `app` keeps an application's OS trace records only, never the
/// komorebi log or events that merely mention it:
///
/// ```rust
/// use komorebi_recorder::timeline::Filter;
///
/// // the OS trace of one application, by exe name (any case) or by process id
/// let ghostty = Filter { app: Some("ghostty".to_string()), sources: vec![] };
/// let by_pid = Filter { app: Some("111".to_string()), ..Filter::default() };
///
/// // two sources: komorebi's log, and the OS trace of the komorebi process
/// let two = Filter {
///     sources: vec!["komorebi-log".to_string(), "oslog:komorebi".to_string()],
///     ..Filter::default()
/// };
/// # let _ = (ghostty, by_pid, two);
/// ```
///
/// The behavior is covered by the tests `source_filter_supports_prefixes` and
/// `app_filter_returns_only_that_apps_records`.
#[derive(Debug, Clone, Default)]
pub struct Filter {
    /// An app's exe name (case-insensitive) or a pid.
    ///
    /// Keeps only OS trace records (sources starting `oslog:`) of that app: those with its process
    /// id, or whose source is `oslog:app:<name>`. A name is looked up in the manifest's apps to
    /// find its process ids.
    pub app: Option<String>,
    /// Sources to keep: `komorebi-log`, `events`, `oslog` (all OS trace), `oslog:komorebi`,
    /// `oslog:system`, `oslog:app` (all apps) or `oslog:app:<exe>`. Empty keeps everything.
    ///
    /// A name matches a source if it equals it, ignoring case, or is a prefix of it that ends
    /// where the source has a `:`. So `oslog` keeps `oslog:system`, but `osl` keeps nothing.
    pub sources: Vec<String>,
}

fn source_matches(source: &str, wanted: &str) -> bool {
    source.eq_ignore_ascii_case(wanted)
        || source.get(..wanted.len()).is_some_and(|prefix| {
            prefix.eq_ignore_ascii_case(wanted)
                && source.as_bytes().get(wanted.len()) == Some(&b':')
        })
}

/// The pids and source labels an `--app` argument refers to, using the manifest's app list.
fn resolve_app(manifest: &Manifest, app: &str) -> (Vec<i64>, String) {
    if let Ok(pid) = app.parse::<i64>() {
        return (vec![pid], String::new());
    }

    let pids = manifest
        .apps
        .iter()
        .filter(|record| record.exe.eq_ignore_ascii_case(app))
        .map(|record| i64::from(record.pid))
        .collect();

    (pids, format!("oslog:app:{app}"))
}

impl Filter {
    fn accepts(&self, entry: &TimelineEntry, app: Option<&(Vec<i64>, String)>) -> bool {
        if !self.sources.is_empty()
            && !self
                .sources
                .iter()
                .any(|wanted| source_matches(&entry.source, wanted))
        {
            return false;
        }

        if let Some((pids, source)) = app {
            let by_pid = entry.pid.is_some_and(|pid| pids.contains(&pid));
            let by_source = !source.is_empty() && entry.source.eq_ignore_ascii_case(source);
            // `--app` is "only this app's records": the komorebi log and events are not that
            // app's, even though events may mention its pid
            let is_app_data = entry.source.starts_with("oslog:");
            return is_app_data && (by_pid || by_source);
        }

        true
    }
}

fn read_entry_line(
    reader: &mut BufReader<File>,
    buffer: &mut String,
) -> eyre::Result<Option<(TimelineEntry, u64)>> {
    buffer.clear();
    let read = reader.read_line(buffer)?;
    if read == 0 {
        return Ok(None);
    }

    let entry: TimelineEntry = serde_json::from_str(buffer.trim_end())
        .wrap_err("timeline.jsonl contains an invalid line; run `reindex`")?;
    Ok(Some((entry, read as u64)))
}

/// The byte offset of the first line whose `mach_ns` is `>= target`, or the file length.
fn lower_bound(file: &mut BufReader<File>, size: u64, target: u64) -> eyre::Result<u64> {
    let mut buffer = String::new();

    // the first line start at or after `position`
    let line_start_at_or_after = |file: &mut BufReader<File>, position: u64| -> eyre::Result<u64> {
        if position == 0 {
            return Ok(0);
        }

        file.seek(SeekFrom::Start(position - 1))?;
        let mut skipped = Vec::new();
        let read = file.read_until(b'\n', &mut skipped)?;
        Ok(position - 1 + read as u64)
    };

    let (mut low, mut high) = (0_u64, size);
    while low < high {
        let middle = low + (high - low) / 2;
        let start = line_start_at_or_after(file, middle)?;
        if start >= high {
            high = middle;
            continue;
        }

        file.seek(SeekFrom::Start(start))?;
        match read_entry_line(file, &mut buffer)? {
            Some((entry, length)) => {
                if entry.mach_ns < target {
                    low = start + length;
                } else {
                    high = start;
                }
            }
            None => high = middle,
        }
    }

    Ok(low)
}

/// Raw files opened on demand while rendering records.
struct RawFiles<'a> {
    session: &'a Session,
    open: HashMap<String, File>,
}

impl<'a> RawFiles<'a> {
    fn new(session: &'a Session) -> Self {
        Self {
            session,
            open: HashMap::new(),
        }
    }

    fn read(&mut self, entry: &TimelineEntry) -> eyre::Result<String> {
        if !self.open.contains_key(&entry.file) {
            let path = self.session.path(&entry.file);
            let file =
                File::open(&path).wrap_err_with(|| format!("could not open {}", path.display()))?;
            self.open.insert(entry.file.clone(), file);
        }

        let file = self
            .open
            .get_mut(&entry.file)
            .ok_or_else(|| eyre::Report::msg("raw file vanished"))?;
        file.seek(SeekFrom::Start(entry.offset))?;
        let mut bytes = vec![0_u8; entry.len as usize];
        file.read_exact(&mut bytes)?;
        Ok(String::from_utf8_lossy(&bytes)
            .trim_end_matches(['\n', '\r'])
            .to_string())
    }
}

const EVENT_TEXT_LIMIT: usize = 400;

/// Renders a raw record for reading: one line of text for the entry `entry`, whose raw text is
/// `raw`.
///
/// What it does depends on the entry's source:
///
/// | Source | Rendered as |
/// |---|---|
/// | `komorebi-log` | the raw line with ANSI escapes removed ([`strip_ansi`]) |
/// | `recorder` | `<kind>: <detail>` |
/// | `events` | `#<seq> <event as JSON>`, then `  state=states/<id>.json` if the event stored a state (the id is zero padded to 6 digits) |
/// | anything else, the OS trace | `<time> [<pid>] <process> <subsystem>/<category>: <message>`: the time of day of the record's timestamp, and the subsystem and category left out when empty; a multi-line message is folded onto one line with ` ⏎ ` |
///
/// Text that does not parse as its source's kind of record is shown as it is, cut to 400
/// characters with a `…`, rather than being an error. An event is also cut to 400 characters.
///
/// # Example
///
/// ```rust
/// use komorebi_recorder::timeline::{render, TimelineEntry};
///
/// let entry = |source: &str| TimelineEntry {
///     video_s: 2.0,
///     mach_ns: 102_000_000_000,
///     wall_utc: None,
///     source: source.to_string(),
///     pid: None,
///     file: "x".to_string(),
///     offset: 0,
///     len: 0,
///     line: 1,
///     seq: None,
/// };
///
/// // an incident
/// let incident = r#"{"mach_ns":1,"wall_us":1,"kind":"display-disconnected","detail":"display 1 disconnected"}"#;
/// assert_eq!(render(&entry("recorder"), incident), "display-disconnected: display 1 disconnected");
///
/// // an event, with the state it stored
/// let event = r#"{"seq":7,"recv_wall_us":1,"recv_mach_ns":1,"event":{"type":"Retile"},"state_id":3}"#;
/// assert_eq!(
///     render(&entry("events"), event),
///     r#"#7 {"type":"Retile"}  state=states/000003.json"#
/// );
///
/// // a log line, with the escapes komorebi's tracing leaves inside span fields
/// assert_eq!(
///     render(&entry("komorebi-log"), "INFO focus{\u{1b}[3midx\u{1b}[0m=2}: x"),
///     "INFO focus{idx=2}: x"
/// );
/// ```
#[must_use]
pub fn render(entry: &TimelineEntry, raw: &str) -> String {
    if entry.source == SOURCE_KOMOREBI_LOG {
        return strip_ansi(raw);
    }

    if entry.source == SOURCE_RECORDER {
        return match serde_json::from_str::<Incident>(raw) {
            Ok(incident) => format!("{}: {}", incident.kind, incident.detail),
            Err(_) => truncate(raw, EVENT_TEXT_LIMIT),
        };
    }

    if entry.source == SOURCE_EVENTS {
        return match serde_json::from_str::<EventRow>(raw) {
            Ok(row) => {
                let event = row
                    .event
                    .map_or_else(|| "<unparsed>".to_string(), |event| event.to_string());
                let event = truncate(&event, EVENT_TEXT_LIMIT);
                let state = row
                    .state_id
                    .map_or_else(String::new, |id| format!("  state=states/{id:06}.json"));
                format!("#{} {event}{state}", row.seq)
            }
            Err(_) => truncate(raw, EVENT_TEXT_LIMIT),
        };
    }

    match OsLogRecord::parse(raw) {
        Some(record) => {
            let time = record
                .timestamp
                .split_whitespace()
                .nth(1)
                .unwrap_or(&record.timestamp);
            let place = match (record.subsystem.is_empty(), record.category.is_empty()) {
                (true, _) => String::new(),
                (false, true) => format!(" {}", record.subsystem),
                (false, false) => format!(" {}/{}", record.subsystem, record.category),
            };
            format!(
                "{time} [{}] {}{place}: {}",
                record.pid,
                record.process_name(),
                compact_lines(&record.message)
            )
        }
        None => truncate(raw, EVENT_TEXT_LIMIT),
    }
}

/// Removes ANSI escape sequences from `text`.
///
/// komorebi's plaintext log still contains them inside span fields (tracing shares the formatted
/// fields between its two layers); the raw file is left as-is and only the rendering is cleaned.
///
/// What is removed is a CSI sequence: an escape character followed by `[`, up to and including the
/// first character in `@` to `~`. An escape character that is not followed by `[` is left in the
/// text. A CSI sequence cut off by the end of the text takes the rest of the text with it.
///
/// # Example
///
/// ```rust
/// use komorebi_recorder::timeline::strip_ansi;
///
/// assert_eq!(
///     strip_ansi("INFO focus_container{\u{1b}[3midx\u{1b}[0m\u{1b}[2m=\u{1b}[0m2}: x"),
///     "INFO focus_container{idx=2}: x"
/// );
/// assert_eq!(strip_ansi("plain"), "plain");
/// assert_eq!(strip_ansi("trailing \u{1b}"), "trailing \u{1b}");
/// ```
#[must_use]
pub fn strip_ansi(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut chars = text.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '\u{1b}' && chars.peek() == Some(&'[') {
            chars.next();
            // a CSI sequence ends at the first byte in 0x40..=0x7e
            for next in chars.by_ref() {
                if ('@'..='~').contains(&next) {
                    break;
                }
            }
        } else {
            out.push(c);
        }
    }

    out
}

/// Fold a multi-line message onto one line, marking the breaks.
fn compact_lines(text: &str) -> String {
    let mut lines = text.lines().map(str::trim).filter(|line| !line.is_empty());
    let mut out = lines.next().unwrap_or_default().to_string();
    for line in lines {
        out.push_str(" ⏎ ");
        out.push_str(line);
    }

    out
}

fn truncate(text: &str, limit: usize) -> String {
    if text.chars().count() <= limit {
        return text.to_string();
    }

    let mut shortened: String = text.chars().take(limit).collect();
    shortened.push('…');
    shortened
}

/// Returns every record whose video time is in `from_s..=to_s`, narrowed by `filter`, in time
/// order.
///
/// `from_s` and `to_s` are seconds of video time, and both ends are included: an instant
/// (`from_s == to_s`) selects the records at exactly that time. A range with `from_s` after `to_s`
/// selects nothing. The range is turned into master clock times with `manifest`, and records are
/// selected by their `mach_ns`. The index is searched by byte offset, so only the lines in the range
/// are read, and each record's raw text is read from its raw file and rendered with [`render`].
///
/// # Errors
///
/// Returns an error if
///
/// * `timeline.jsonl` does not exist. The message says to run `komorebi-recorder reindex`;
/// * a line of the index is not valid, which the message says `reindex` fixes;
/// * a raw file the index points into cannot be opened, or is shorter than the entry says, which
///   means the raw file changed since the index was built: `reindex` rebuilds it.
///
/// # Example
///
/// See the [module example](self#example), which asks for a range and for one source.
///
/// The edges are covered by the tests `range_endpoints_are_inclusive_and_binary_search_is_exact`
/// and `a_missing_timeline_explains_how_to_recover`.
pub fn query(
    session: &Session,
    manifest: &Manifest,
    from_s: f64,
    to_s: f64,
    filter: &Filter,
) -> eyre::Result<Vec<Record>> {
    let path = session.path(TIMELINE_FILE);
    let file = File::open(&path).wrap_err_with(|| {
        format!(
            "{} does not exist; run `komorebi-recorder reindex`",
            path.display()
        )
    })?;
    let size = file.metadata()?.len();
    let mut reader = BufReader::new(file);

    let from_mach = manifest.mach_at_video_seconds(from_s);
    let to_mach = manifest.mach_at_video_seconds(to_s);
    let start = lower_bound(&mut reader, size, from_mach)?;
    reader.seek(SeekFrom::Start(start))?;

    let app = filter.app.as_deref().map(|app| resolve_app(manifest, app));
    let mut raw = RawFiles::new(session);
    let mut records = Vec::new();
    let mut buffer = String::new();

    while let Some((entry, _)) = read_entry_line(&mut reader, &mut buffer)? {
        if entry.mach_ns > to_mach {
            break;
        }

        if !filter.accepts(&entry, app.as_ref()) {
            continue;
        }

        let text = render(&entry, &raw.read(&entry)?);
        records.push(Record { entry, text });
    }

    Ok(records)
}

/// Returns the distinct application names (exe names) in the manifest's list of apps, sorted.
///
/// Every app in the list is returned, whether or not an OS trace stream was recorded for it.
///
/// # Example
///
/// ```rust
/// use komorebi_recorder::clock::Anchor;
/// use komorebi_recorder::session::{Manifest, Moment};
/// use komorebi_recorder::sources::managed_apps::AppRecord;
/// use komorebi_recorder::timeline::app_names;
///
/// let start = Moment { mach_ns: 1, wall_us: 1 };
/// let mut manifest = Manifest::new(start, vec![Anchor { wall_us: 1, mach_ns: 1 }]);
/// let app = |pid: i32, exe: &str| AppRecord { pid, exe: exe.to_string(), ..AppRecord::default() };
/// manifest.apps = vec![app(222, "Ghostty"), app(111, "Brave Beta"), app(333, "Ghostty")];
///
/// assert_eq!(app_names(&manifest), ["Brave Beta", "Ghostty"]);
/// ```
#[must_use]
pub fn app_names(manifest: &Manifest) -> Vec<String> {
    let mut names: Vec<String> = manifest.apps.iter().map(|app| app.exe.clone()).collect();
    names.sort();
    names.dedup();
    names
}

/// A digest of one stretch of the recording: how many records each source has in it, and the first
/// and last komorebi log line.
///
/// [`summary`] returns these, one per stretch that has at least one record.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Bucket {
    /// The start of the bucket, in seconds of video time. The bucket includes it.
    pub start_s: f64,
    /// The end of the bucket, in seconds of video time. The bucket does not include it: it is the
    /// start of the next.
    pub end_s: f64,
    /// How many records each source has in the bucket, by source name.
    pub counts: BTreeMap<String, u64>,
    /// The first komorebi log line in the bucket, as raw text cut to 160 characters. It is not
    /// rendered: ANSI escapes in it are not removed. `None` if the bucket has no log line.
    pub first_log: Option<String>,
    /// The last komorebi log line in the bucket, in the same form as `first_log`.
    pub last_log: Option<String>,
}

/// Returns a digest of the whole recording, one [`Bucket`] for each `bucket_s` seconds of video
/// time that has any record, narrowed by `filter`.
///
/// The buckets are in time order. Bucket `n` covers `n * bucket_s` up to, but not including,
/// `(n + 1) * bucket_s`, and `n` is negative before the first frame. A stretch with no records has
/// no bucket. A `bucket_s` below one millisecond is taken as one millisecond. A record is placed by
/// the `video_s` it has in the index, so the buckets are those of the manifest the index was built
/// from.
///
/// This reads the whole index, one line at a time, and the raw text of every komorebi log line.
///
/// # Errors
///
/// Returns an error if
///
/// * `timeline.jsonl` does not exist. The message says to run `komorebi-recorder reindex`;
/// * a line of the index is not valid;
/// * a raw file a log line points into cannot be opened, or is shorter than the entry says.
///
/// # Example
///
/// See the module example for building an index. The buckets are covered by the test
/// `summary_buckets_every_second`.
pub fn summary(
    session: &Session,
    manifest: &Manifest,
    bucket_s: f64,
    filter: &Filter,
) -> eyre::Result<Vec<Bucket>> {
    let bucket_s = bucket_s.max(0.001);
    let path = session.path(TIMELINE_FILE);
    let mut reader = BufReader::new(File::open(&path).wrap_err_with(|| {
        format!(
            "{} does not exist; run `komorebi-recorder reindex`",
            path.display()
        )
    })?);

    let app = filter.app.as_deref().map(|app| resolve_app(manifest, app));
    let mut raw = RawFiles::new(session);
    let mut buckets: BTreeMap<i64, Bucket> = BTreeMap::new();
    let mut buffer = String::new();

    while let Some((entry, _)) = read_entry_line(&mut reader, &mut buffer)? {
        if !filter.accepts(&entry, app.as_ref()) {
            continue;
        }

        let index = (entry.video_s / bucket_s).floor() as i64;
        let bucket = buckets.entry(index).or_insert_with(|| Bucket {
            start_s: index as f64 * bucket_s,
            end_s: (index + 1) as f64 * bucket_s,
            counts: BTreeMap::new(),
            first_log: None,
            last_log: None,
        });

        *bucket.counts.entry(entry.source.clone()).or_default() += 1;

        if entry.source == SOURCE_KOMOREBI_LOG {
            let line = truncate(&raw.read(&entry)?, 160);
            if bucket.first_log.is_none() {
                bucket.first_log = Some(line.clone());
            }
            bucket.last_log = Some(line);
        }
    }

    Ok(buckets.into_values().collect())
}

/// What to look up in reverse: something that has a time, to be placed in the video.
///
/// [`locate`] takes one.
#[derive(Debug, Clone)]
pub enum Locator {
    /// A wall clock time. It is placed on the master clock with the manifest's anchors.
    Utc(DateTime<Utc>),
    /// A line of `komorebi.log`, by its 1-based line number: the line's own time, or for a
    /// continuation line (part of a panic message or a backtrace) the time of the line before it.
    LogLine(u64),
    /// An event in `events.jsonl`, by its subscriber sequence number (`seq`): the time it was
    /// received.
    EventSeq(u64),
}

/// A moment located in video time, and where it falls in each display's video.
///
/// [`locate`] returns one.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Located {
    /// What was located, as a sentence: the time, the log line with its text, or the event.
    pub description: String,
    /// The moment in seconds of video time. It is outside `0..=duration` if the moment is before
    /// the first frame or after the end.
    pub video_s: f64,
    /// The moment on the master clock, in nanoseconds.
    pub mach_ns: u64,
    /// The moment as RFC 3339 UTC with microseconds, worked out from the manifest's anchors.
    pub wall_utc: Option<String>,
    /// Whether `video_s` is within the recorded video: from 0 to its duration, both included.
    pub in_video: bool,
    /// Where it falls in the file of each display that has video at that moment.
    pub displays: Vec<DisplayTime>,
    /// Displays that were recorded at some point but have no video at this moment (unplugged,
    /// asleep, or a stream that had died).
    pub without_video: Vec<u32>,
}

/// Where a moment falls in the video of one display.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct DisplayTime {
    /// The id of the display.
    pub display: u32,
    /// The segment of the display's video that has the moment: 1-based, per display. A display
    /// that was unplugged, or whose stream died, has several.
    pub segment: u32,
    /// The segment's video file, relative to the session directory.
    pub file: String,
    /// The moment in seconds into that file. It can be slightly below zero: a segment is taken to
    /// have video from 50 ms before its first frame.
    pub file_s: f64,
}

/// Returns the video time of a moment, and which segment of each display's video has it.
///
/// This is the reverse of [`query`]: given a UTC time, a komorebi log line or an event, it says
/// where in the video that was. The time is placed on the master clock, and `manifest` turns it into
/// video time and finds, for each display, the segment (file) that has video then, and how far into
/// that file it is. A display with a recording but no video at that moment (unplugged, asleep, or a
/// stream that had died) is listed in [`Located::without_video`] instead.
///
/// A moment outside the recording is located all the same, with [`Located::in_video`] false.
///
/// # Errors
///
/// Returns an error if
///
/// * the locator is a [`Locator::Utc`] or a [`Locator::LogLine`] and the manifest has no clock
///   anchors, so a wall clock time cannot be placed;
/// * the locator is a [`Locator::LogLine`] and `komorebi.log` cannot be read, or has no such line
///   (the message says how many lines it has; line 0 does not exist), or the line has no timestamp
///   and none precedes it;
/// * the locator is a [`Locator::EventSeq`] and `events.jsonl` cannot be read, or has no event with
///   that sequence number.
///
/// # Example
///
/// See the [module example](self#example), which locates a log line. The segments are covered by
/// the tests `locate_inverts_the_queries` and
/// `locate_names_the_segment_per_display_and_notes_a_display_with_no_video`.
pub fn locate(session: &Session, manifest: &Manifest, locator: &Locator) -> eyre::Result<Located> {
    let clock = ClockMap::new(manifest.anchors.clone());

    let (description, mach_ns) = match locator {
        Locator::Utc(time) => {
            let mach = clock
                .wall_to_mach(time.timestamp_micros())
                .ok_or_else(|| eyre::Report::msg("the session has no clock anchors"))?;
            (
                format!("UTC {}", time.format("%Y-%m-%dT%H:%M:%S%.6fZ")),
                mach,
            )
        }
        Locator::LogLine(number) => {
            let bytes = std::fs::read(session.path("komorebi.log"))
                .wrap_err("could not read komorebi.log")?;
            let lines = komorebi_log::index_lines(&bytes);
            let line = lines
                .get(usize::try_from(number.saturating_sub(1))?)
                .filter(|_| *number >= 1)
                .ok_or_else(|| {
                    eyre::Report::msg(format!(
                        "komorebi.log has {} lines; there is no line {number}",
                        lines.len()
                    ))
                })?;
            let timestamp = line.timestamp.ok_or_else(|| {
                eyre::Report::msg(format!(
                    "line {number} has no timestamp and none precedes it"
                ))
            })?;
            let mach = clock
                .wall_to_mach(timestamp.timestamp_micros())
                .ok_or_else(|| eyre::Report::msg("the session has no clock anchors"))?;
            let text = &bytes[line.offset as usize..(line.offset + line.len) as usize];
            (
                format!(
                    "komorebi.log line {number}: {}",
                    truncate(String::from_utf8_lossy(text).trim_end(), 120)
                ),
                mach,
            )
        }
        Locator::EventSeq(seq) => {
            let text = std::fs::read_to_string(session.path("events.jsonl"))
                .wrap_err("could not read events.jsonl")?;
            let row = text
                .lines()
                .filter_map(|line| serde_json::from_str::<EventRow>(line).ok())
                .find(|row| row.seq == *seq)
                .ok_or_else(|| eyre::Report::msg(format!("there is no event #{seq}")))?;
            let summary = row
                .event
                .as_ref()
                .map_or_else(String::new, |event| truncate(&event.to_string(), 120));
            (format!("event #{seq}: {summary}"), row.recv_mach_ns)
        }
    };

    let video_s = manifest.video_seconds(mach_ns);
    let (_, duration) = manifest.video_span();

    // for each display, the segment (file) that has video at this moment, or why there is none
    let mut displays = Vec::new();
    let mut without_video = Vec::new();
    for id in manifest.display_ids() {
        match manifest.segment_at(id, video_s) {
            Some(entry) => displays.push(DisplayTime {
                display: id,
                segment: entry.result.segment,
                file: entry.result.file.clone(),
                file_s: video_s - entry.offset_s,
            }),
            None => without_video.push(id),
        }
    }

    Ok(Located {
        description,
        video_s,
        mach_ns,
        wall_utc: wall_utc(&clock, mach_ns),
        in_video: (0.0..=duration).contains(&video_s),
        displays,
        without_video,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::clock::Anchor;
    use crate::session::Moment;
    use crate::sources::oslog::OsLogLevel;

    /// A synthetic but realistic session: 10s of video starting at mach 100s.
    struct Fixture {
        session: Session,
        manifest: Manifest,
    }

    const T0_MACH_NS: u64 = 100_000_000_000;
    /// wall = mach + this offset (microseconds)
    const WALL_OFFSET_US: i64 = 1_800_000_000_000_000 - 100_000_000;

    fn wall_for_video_s(video_s: f64) -> i64 {
        (T0_MACH_NS as i64 / 1_000) + (video_s * 1e6) as i64 + WALL_OFFSET_US
    }

    fn rfc3339(wall_us: i64) -> String {
        format_wall_us(wall_us).expect("time")
    }

    fn oslog_line(video_s: f64, pid: i64, process: &str, message: &str) -> String {
        let tb = crate::clock::timebase();
        let ticks = tb.ns_to_ticks(T0_MACH_NS + (video_s * 1e9) as u64);
        format!(
            r#"{{"machTimestamp":{ticks},"timestamp":"2026-10-26 12:00:00.000000-0700","processID":{pid},"processImagePath":"/Applications/{process}","subsystem":"com.apple.accessibility.axserver","category":"AX","messageType":"Debug","eventMessage":"{message}"}}"#
        )
    }

    fn fixture(name: &str) -> Fixture {
        let dir = std::env::temp_dir().join(format!(
            "komorebi-recorder-timeline-{name}-{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        let session = Session::create(Some(dir), Utc::now()).expect("create");

        let start = Moment {
            mach_ns: T0_MACH_NS - 1_000_000_000,
            wall_us: wall_for_video_s(-1.0),
        };
        let end = Moment {
            mach_ns: T0_MACH_NS + 11_000_000_000,
            wall_us: wall_for_video_s(11.0),
        };
        let mut manifest = Manifest::new(
            start,
            vec![
                Anchor {
                    wall_us: start.wall_us,
                    mach_ns: start.mach_ns,
                },
                Anchor {
                    wall_us: wall_for_video_s(5.0),
                    mach_ns: T0_MACH_NS + 5_000_000_000,
                },
                Anchor {
                    wall_us: end.wall_us,
                    mach_ns: end.mach_ns,
                },
            ],
        );
        manifest.stop = Some(end);
        manifest.video_t0_mach_ns = Some(T0_MACH_NS);
        manifest.video_end_mach_ns = Some(T0_MACH_NS + 10_000_000_000);

        // komorebi.log: one line per second at 2s, 3s and 4s, with a 2-line panic after 3s
        let log = format!(
            "{} INFO komorebi::process_event: processing event: Show at 2s\n\
             {} INFO komorebi::process_event: processing event: FocusChange at 3s\n\
             thread 'main' panicked at src/x.rs:1:1:\n\
             \x20  0: backtrace frame\n\
             {} DEBUG komorebi::window_manager: at 4s\n",
            rfc3339(wall_for_video_s(2.0)),
            rfc3339(wall_for_video_s(3.0)),
            rfc3339(wall_for_video_s(4.0)),
        );
        std::fs::write(session.path("komorebi.log"), log).expect("log");

        // events at 2.01s and 3.01s
        let event = |seq: u64, video_s: f64, kind: &str, pid: i64| {
            serde_json::json!({
                "seq": seq,
                "recv_wall_us": wall_for_video_s(video_s),
                "recv_mach_ns": T0_MACH_NS + (video_s * 1e9) as u64,
                "event": {"type": kind, "content": [{"type": "Accessibility", "content": "X"}, pid, 7]},
                "state_id": 1,
            })
            .to_string()
        };
        std::fs::write(
            session.path("events.jsonl"),
            format!(
                "{}\n{}\n",
                event(1, 2.01, "Show", 111),
                event(2, 3.01, "FocusChange", 222)
            ),
        )
        .expect("events");

        // oslog: komorebi, system, and two apps
        let write_stream = |file: &str, lines: Vec<String>| {
            std::fs::write(session.path(file), lines.join("\n") + "\n").expect("stream");
        };
        write_stream(
            "oslog/komorebi.ndjson",
            vec![
                oslog_line(2.02, 999, "komorebi", "ax call"),
                oslog_line(8.0, 999, "komorebi", "late"),
            ],
        );
        write_stream(
            "oslog/system.ndjson",
            vec![oslog_line(2.5, 50, "axserver", "serve")],
        );
        write_stream(
            "oslog/apps/Ghostty-111.ndjson",
            vec![
                oslog_line(2.03, 111, "Ghostty", "tab changed"),
                oslog_line(3.02, 111, "Ghostty", "tab changed again"),
            ],
        );
        write_stream(
            "oslog/apps/Brave_Beta-222.ndjson",
            vec![oslog_line(3.03, 222, "Brave", "other app")],
        );

        let spec = |name: &str, source: &str, file: &str, pid: Option<i32>| oslog::StreamInfo {
            spec: oslog::StreamSpec {
                name: name.into(),
                source: source.into(),
                predicate: "x".into(),
                level: OsLogLevel::Debug,
                file: file.into(),
                pid,
            },
            spawned_mach_ns: 0,
            ready_mach_ns: None,
            header: None,
            events: 1,
            bytes: 1,
            first_event_mach_ns: None,
            last_event_mach_ns: None,
            restarts: 0,
            error: None,
        };
        manifest.oslog = vec![
            spec(
                "komorebi",
                "oslog:komorebi",
                "oslog/komorebi.ndjson",
                Some(999),
            ),
            spec("system", "oslog:system", "oslog/system.ndjson", None),
            spec(
                "app:Ghostty-111",
                "oslog:app:Ghostty",
                "oslog/apps/Ghostty-111.ndjson",
                Some(111),
            ),
            spec(
                "app:Brave Beta-222",
                "oslog:app:Brave Beta",
                "oslog/apps/Brave_Beta-222.ndjson",
                Some(222),
            ),
        ];
        manifest.apps = vec![
            managed_apps::AppRecord {
                pid: 111,
                exe: "Ghostty".into(),
                window_ids: vec![1],
                first_seen_mach_ns: 0,
                origin: "state".into(),
                ..managed_apps::AppRecord::default()
            },
            managed_apps::AppRecord {
                pid: 222,
                exe: "Brave Beta".into(),
                window_ids: vec![2],
                first_seen_mach_ns: 0,
                origin: "state".into(),
                ..managed_apps::AppRecord::default()
            },
        ];

        session.write_manifest(&manifest).expect("manifest");
        build(&session).expect("build");
        Fixture { session, manifest }
    }

    fn texts(records: &[Record]) -> Vec<String> {
        records.iter().map(|record| record.text.clone()).collect()
    }

    #[test]
    fn the_timeline_is_sorted_and_complete() {
        let fixture = fixture("sorted");
        let text = std::fs::read_to_string(fixture.session.path(TIMELINE_FILE)).expect("timeline");
        let entries: Vec<TimelineEntry> = text
            .lines()
            .map(|line| serde_json::from_str(line).expect("entry"))
            .collect();

        // 5 log lines (the 2 continuation lines inherit) + 2 events + 2 + 1 + 2 + 1 oslog
        assert_eq!(entries.len(), 5 + 2 + 6);
        assert!(
            entries
                .windows(2)
                .all(|pair| pair[0].mach_ns <= pair[1].mach_ns)
        );
        assert_eq!(
            entries
                .iter()
                .filter(|e| e.source == SOURCE_KOMOREBI_LOG)
                .count(),
            5
        );
        assert!(entries.iter().any(|e| e.source == "oslog:app:Brave Beta"));
    }

    #[test]
    fn at_returns_every_source_within_the_window() {
        let fixture = fixture("at");
        let records = query(
            &fixture.session,
            &fixture.manifest,
            1.5,
            3.5,
            &Filter::default(),
        )
        .expect("query");
        let sources: std::collections::BTreeSet<&str> =
            records.iter().map(|r| r.entry.source.as_str()).collect();

        for source in [
            SOURCE_KOMOREBI_LOG,
            SOURCE_EVENTS,
            "oslog:komorebi",
            "oslog:system",
            "oslog:app:Ghostty",
        ] {
            assert!(sources.contains(source), "missing {source} in {sources:?}");
        }

        // outside the window: the 8s komorebi line and the 4s log line
        let all = texts(&records).join("\n");
        assert!(!all.contains("late"));
        assert!(!all.contains("at 4s"));
        assert!(all.contains("processing event: Show at 2s"));
    }

    #[test]
    fn continuation_lines_share_the_previous_time_and_stay_in_the_window() {
        let fixture = fixture("continuation");
        let records = query(
            &fixture.session,
            &fixture.manifest,
            2.9,
            3.1,
            &Filter {
                sources: vec![SOURCE_KOMOREBI_LOG.into()],
                app: None,
            },
        )
        .expect("query");

        let lines = texts(&records);
        assert_eq!(lines.len(), 3, "{lines:?}");
        assert!(lines[1].contains("panicked"));
        assert!(lines[2].contains("backtrace frame"));
    }

    #[test]
    fn app_filter_returns_only_that_apps_records() {
        let fixture = fixture("app");
        let filter = Filter {
            app: Some("ghostty".into()),
            sources: vec![],
        };
        let records =
            query(&fixture.session, &fixture.manifest, 0.0, 10.0, &filter).expect("query");

        assert_eq!(records.len(), 2);
        assert!(
            records
                .iter()
                .all(|r| r.entry.source == "oslog:app:Ghostty" && r.entry.pid == Some(111))
        );
        assert!(texts(&records).iter().all(|text| text.contains("Ghostty")));

        // by pid reaches the same records, and never the komorebi log or events that mention it
        let by_pid = query(
            &fixture.session,
            &fixture.manifest,
            0.0,
            10.0,
            &Filter {
                app: Some("111".into()),
                sources: vec![],
            },
        )
        .expect("query");
        assert_eq!(by_pid.len(), 2);
    }

    #[test]
    fn source_filter_supports_prefixes() {
        let fixture = fixture("source");
        let run = |sources: &[&str]| {
            query(
                &fixture.session,
                &fixture.manifest,
                0.0,
                10.0,
                &Filter {
                    app: None,
                    sources: sources.iter().map(|s| (*s).to_string()).collect(),
                },
            )
            .expect("query")
            .len()
        };

        assert_eq!(run(&["events"]), 2);
        assert_eq!(run(&["oslog"]), 6);
        assert_eq!(run(&["oslog:app"]), 3);
        assert_eq!(run(&["oslog:app:Ghostty"]), 2);
        assert_eq!(run(&["oslog:komorebi", "events"]), 4);
        assert_eq!(run(&["osl"]), 0, "a source prefix must end at a colon");
    }

    #[test]
    fn range_endpoints_are_inclusive_and_binary_search_is_exact() {
        let fixture = fixture("range");
        let run = |from: f64, to: f64| {
            query(
                &fixture.session,
                &fixture.manifest,
                from,
                to,
                &Filter::default(),
            )
            .expect("query")
            .len()
        };

        assert_eq!(run(-100.0, 100.0), 13);
        assert_eq!(run(100.0, 200.0), 0);
        assert_eq!(run(-10.0, -5.0), 0);
        assert_eq!(
            run(8.0, 8.0),
            1,
            "an instant selects the record at exactly that time"
        );
        // the first record of the whole timeline is at 2.0s
        assert_eq!(run(2.0, 2.0), 1);
        assert_eq!(run(1.99, 2.005), 1);
    }

    #[test]
    fn the_wall_clock_log_maps_through_the_anchors() {
        let fixture = fixture("anchors");
        let records = query(
            &fixture.session,
            &fixture.manifest,
            0.0,
            10.0,
            &Filter {
                sources: vec![SOURCE_KOMOREBI_LOG.into()],
                app: None,
            },
        )
        .expect("query");

        let show = records
            .iter()
            .find(|r| r.text.contains("Show at 2s"))
            .expect("show");
        assert!(
            (show.entry.video_s - 2.0).abs() < 1e-3,
            "{}",
            show.entry.video_s
        );
        let focus = records
            .iter()
            .find(|r| r.text.contains("FocusChange at 3s"))
            .expect("focus");
        assert!(
            (focus.entry.video_s - 3.0).abs() < 1e-3,
            "{}",
            focus.entry.video_s
        );
    }

    #[test]
    fn locate_inverts_the_queries() {
        let fixture = fixture("locate");

        let by_line =
            locate(&fixture.session, &fixture.manifest, &Locator::LogLine(2)).expect("line");
        assert!((by_line.video_s - 3.0).abs() < 1e-3);
        assert!(by_line.in_video);
        assert!(by_line.description.contains("FocusChange at 3s"));

        // a continuation line has the time of the line it follows
        let continuation =
            locate(&fixture.session, &fixture.manifest, &Locator::LogLine(4)).expect("line");
        assert!((continuation.video_s - 3.0).abs() < 1e-3);

        let by_event =
            locate(&fixture.session, &fixture.manifest, &Locator::EventSeq(1)).expect("event");
        assert!((by_event.video_s - 2.01).abs() < 1e-6);

        let utc: DateTime<Utc> = rfc3339(wall_for_video_s(6.5)).parse().expect("utc");
        let by_utc = locate(&fixture.session, &fixture.manifest, &Locator::Utc(utc)).expect("utc");
        assert!((by_utc.video_s - 6.5).abs() < 1e-3);

        let outside: DateTime<Utc> = rfc3339(wall_for_video_s(60.0)).parse().expect("utc");
        assert!(
            !locate(&fixture.session, &fixture.manifest, &Locator::Utc(outside))
                .expect("utc")
                .in_video
        );

        assert!(locate(&fixture.session, &fixture.manifest, &Locator::LogLine(99)).is_err());
        assert!(locate(&fixture.session, &fixture.manifest, &Locator::LogLine(0)).is_err());
        assert!(locate(&fixture.session, &fixture.manifest, &Locator::EventSeq(99)).is_err());
    }

    #[test]
    fn at_and_locate_round_trip() {
        let fixture = fixture("roundtrip");
        // pick every komorebi log line, locate it, then query at that time: it must come back
        for number in [1_u64, 2, 5] {
            let located = locate(
                &fixture.session,
                &fixture.manifest,
                &Locator::LogLine(number),
            )
            .expect("locate");
            let records = query(
                &fixture.session,
                &fixture.manifest,
                located.video_s - 0.001,
                located.video_s + 0.001,
                &Filter {
                    sources: vec![SOURCE_KOMOREBI_LOG.into()],
                    app: None,
                },
            )
            .expect("query");
            assert!(
                records.iter().any(|record| record.entry.line == number),
                "line {number} not found at {}s",
                located.video_s
            );
        }
    }

    #[test]
    fn summary_buckets_every_second() {
        let fixture = fixture("summary");
        let buckets =
            summary(&fixture.session, &fixture.manifest, 1.0, &Filter::default()).expect("summary");

        let at = |start: f64| {
            buckets
                .iter()
                .find(|b| (b.start_s - start).abs() < 1e-9)
                .expect("bucket")
        };
        assert_eq!(at(2.0).counts.get(SOURCE_KOMOREBI_LOG), Some(&1));
        assert_eq!(at(2.0).counts.get(SOURCE_EVENTS), Some(&1));
        assert!(
            at(2.0)
                .first_log
                .as_deref()
                .is_some_and(|line| line.contains("Show at 2s"))
        );
        // the 3s bucket holds the FocusChange line and both panic lines
        assert_eq!(at(3.0).counts.get(SOURCE_KOMOREBI_LOG), Some(&3));
        assert!(
            at(3.0)
                .last_log
                .as_deref()
                .is_some_and(|line| line.contains("backtrace frame"))
        );

        let total: u64 = buckets.iter().flat_map(|b| b.counts.values()).sum();
        assert_eq!(total, 13);
    }

    #[test]
    fn reindex_is_idempotent_and_recovers_a_deleted_timeline() {
        let fixture = fixture("reindex");
        let first = std::fs::read_to_string(fixture.session.path(TIMELINE_FILE)).expect("timeline");
        std::fs::remove_file(fixture.session.path(TIMELINE_FILE)).expect("remove");
        build(&fixture.session).expect("rebuild");
        let second =
            std::fs::read_to_string(fixture.session.path(TIMELINE_FILE)).expect("timeline");
        assert_eq!(first, second);
    }

    #[test]
    fn hand_added_oslog_files_are_indexed_by_name() {
        let fixture = fixture("handadded");
        std::fs::write(
            fixture.session.path("oslog/apps/Finder-333.ndjson"),
            oslog_line(5.0, 333, "Finder", "hand added") + "\n",
        )
        .expect("write");
        let stats = build(&fixture.session).expect("rebuild");
        assert_eq!(stats.by_source.get("oslog:app:Finder"), Some(&1));
    }

    #[test]
    fn render_formats_each_source() {
        let fixture = fixture("render");
        let records = query(
            &fixture.session,
            &fixture.manifest,
            2.0,
            2.1,
            &Filter::default(),
        )
        .expect("query");

        let event = records
            .iter()
            .find(|r| r.entry.source == SOURCE_EVENTS)
            .expect("event");
        assert!(event.text.starts_with("#1 {"), "{}", event.text);
        assert!(event.text.contains("states/000001.json"));

        let os = records
            .iter()
            .find(|r| r.entry.source == "oslog:komorebi")
            .expect("os");
        assert!(
            os.text
                .contains("[999] komorebi com.apple.accessibility.axserver/AX: ax call"),
            "{}",
            os.text
        );
    }

    #[test]
    fn rendering_cleans_ansi_and_folds_multiline_messages() {
        assert_eq!(
            strip_ansi("INFO focus_container{\u{1b}[3midx\u{1b}[0m\u{1b}[2m=\u{1b}[0m2}: x"),
            "INFO focus_container{idx=2}: x"
        );
        assert_eq!(strip_ansi("plain"), "plain");
        assert_eq!(strip_ansi("trailing \u{1b}"), "trailing \u{1b}");
        assert_eq!(
            compact_lines("Resource lookup\n\tRequest : a\n\tResult : None"),
            "Resource lookup ⏎ Request : a ⏎ Result : None"
        );
        assert_eq!(compact_lines(""), "");
    }

    #[test]
    fn a_missing_timeline_explains_how_to_recover() {
        let fixture = fixture("missing");
        std::fs::remove_file(fixture.session.path(TIMELINE_FILE)).expect("remove");
        let error = query(
            &fixture.session,
            &fixture.manifest,
            0.0,
            1.0,
            &Filter::default(),
        )
        .expect_err("no timeline")
        .to_string();
        assert!(error.contains("reindex"), "{error}");
    }

    #[test]
    fn locate_names_the_segment_per_display_and_notes_a_display_with_no_video() {
        let fixture = fixture("segments");
        let mut manifest = fixture.manifest.clone();

        let segment = |display: u32, number: u32, t0_s: f64, end_s: f64| {
            let mut result = crate::capture::DisplayResult {
                display: crate::capture::DisplayDescriptor {
                    id: display,
                    frame: [0.0, 0.0, 10.0, 10.0],
                    width_points: 10,
                    height_points: 10,
                    asleep: false,
                    scale: 1.0,
                },
                segment: number,
                scale: 1.0,
                width_px: 10,
                height_px: 10,
                codec: crate::capture::Codec::Hevc,
                file: crate::capture::video_file_name(display, number),
                frames_file: crate::capture::frames_file_name(display, number),
                video_t0_mach_ns: Some(T0_MACH_NS + (t0_s * 1e9) as u64),
                video_end_mach_ns: Some(T0_MACH_NS + (end_s * 1e9) as u64),
                end_reason: Some(crate::capture::EndReason::Stopped),
                frames: 5,
                dropped: 0,
                idle_callbacks: 0,
                latency: crate::capture::LatencyStats::default(),
                error: None,
            };
            result.frames = 5;
            result
        };

        // display 1: 0-4s, unplugged, back 8-10s; display 2: the whole time
        manifest.apply_segments(vec![
            segment(1, 1, 0.0, 4.0),
            segment(2, 1, 0.0, 10.0),
            segment(1, 2, 8.0, 10.0),
        ]);
        manifest.video_end_mach_ns = Some(T0_MACH_NS + 10_000_000_000);

        // log line 2 is at 3.0s: both displays have video
        let located = locate(&fixture.session, &manifest, &Locator::LogLine(2)).expect("locate");
        assert!(located.without_video.is_empty());
        let one = located
            .displays
            .iter()
            .find(|d| d.display == 1)
            .expect("display 1");
        assert_eq!((one.segment, one.file.as_str()), (1, "video-1.mp4"));

        // event 2 is at 3.01s too; pick a moment in the gap with a UTC time at 6s
        let utc: DateTime<Utc> = rfc3339(wall_for_video_s(6.0)).parse().expect("utc");
        let gap = locate(&fixture.session, &manifest, &Locator::Utc(utc)).expect("locate");
        assert_eq!(gap.without_video, vec![1], "display 1 was unplugged at 6s");
        assert_eq!(gap.displays.len(), 1);
        assert_eq!(gap.displays[0].display, 2);

        // after display 1 returns, the second segment is named and the time is within *its* file
        let utc: DateTime<Utc> = rfc3339(wall_for_video_s(9.0)).parse().expect("utc");
        let back = locate(&fixture.session, &manifest, &Locator::Utc(utc)).expect("locate");
        let one = back
            .displays
            .iter()
            .find(|d| d.display == 1)
            .expect("display 1");
        assert_eq!((one.segment, one.file.as_str()), (2, "video-1-2.mp4"));
        assert!(
            (one.file_s - 1.0).abs() < 1e-3,
            "9s is 1s into a segment that began at 8s: {}",
            one.file_s
        );
    }

    #[test]
    fn incidents_appear_in_the_timeline_next_to_everything_else() {
        let fixture = fixture("incidents");
        std::fs::write(
            fixture.session.path(INCIDENTS_FILE),
            format!(
                "{}\n",
                serde_json::json!({
                    "mach_ns": T0_MACH_NS + 2_500_000_000_u64,
                    "wall_us": wall_for_video_s(2.5),
                    "kind": "display-disconnected",
                    "detail": "display 1 disconnected",
                    "display": 1,
                })
            ),
        )
        .expect("write");
        build(&fixture.session).expect("build");

        let records = query(
            &fixture.session,
            &fixture.manifest,
            2.0,
            3.0,
            &Filter {
                app: None,
                sources: vec![SOURCE_RECORDER.into()],
            },
        )
        .expect("query");
        assert_eq!(records.len(), 1);
        assert_eq!(
            records[0].text,
            "display-disconnected: display 1 disconnected"
        );

        // and in an unfiltered `at`, with the komorebi log around it
        let all = query(
            &fixture.session,
            &fixture.manifest,
            2.0,
            3.0,
            &Filter::default(),
        )
        .expect("query");
        assert!(all.iter().any(|r| r.entry.source == SOURCE_RECORDER));
        assert!(all.iter().any(|r| r.entry.source == SOURCE_KOMOREBI_LOG));
    }
}
