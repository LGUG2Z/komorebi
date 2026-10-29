//! Slicing komorebi's own `tracing` log for the duration of a recording.
//!
//! komorebi writes `komorebi_plaintext.log.<YYYY-MM-DD>` rolled daily at **UTC** midnight, so a
//! recording can span two files. komorebi for Mac writes it into its data directory; komorebi for
//! Windows writes it into the temp directory (its README names `%LOCALAPPDATA%`, but its code uses
//! the temp directory), so [`find_log_dir`] looks in both. Despite the name the text is not
//! entirely free of ANSI escape codes: they remain inside the fields of a span
//! (`focus_window{idx=0}`), so readers strip them before parsing. Line prefixes are RFC 3339 UTC
//! timestamps with microsecond precision. The recorder snapshots a byte offset when recording
//! starts ([`snapshot_start`]) and copies exactly the bytes written after it ([`LogTailer`]), so
//! multi-line panics and backtraces (which carry no timestamp of their own) are preserved.
//!
//! # The file written
//!
//! `komorebi.log` in the session directory ([`slice_path`]) is the slice: the bytes komorebi wrote
//! to its log while the recording ran, copied unchanged and in order, across a UTC midnight
//! included. It is plain text in komorebi's `tracing_subscriber::fmt` layout, one event per line:
//!
//! ```text
//! 2026-10-26T00:07:14.370960Z  INFO process_event: komorebi::process_event: processing event
//! ```
//!
//! Lines that carry no timestamp (a panic message, the frames of a backtrace) belong to the
//! timestamped line before them; [`index_lines`] gives them that line's time.
//!
//! Which bytes of which rolled file make up the slice is recorded in the manifest
//! (`session.json`, key `log_segments`) as [`LogSegment`]s. The directory they were read from is
//! `komorebi.log_dir` in the manifest when it is not komorebi's data directory.
//!
//! # What is not guaranteed
//!
//! - The slice holds only what komorebi's log level let it write. The recorder never changes that
//!   level; [`level_warning`] says when it is too quiet to explain a bug.
//! - A late write to an earlier day's file, made after the next day's file has begun to be
//!   copied, is dropped (see [`LogTailer`]).
//!
//! The recorder runs this on both hosts. The only host-dependent part is where the log is looked
//! for ([`log_dir_candidates`], chosen by the [`Flavor`] of the komorebi that is running).

use crate::analysis::flavor::Flavor;
use chrono::DateTime;
use chrono::Duration;
use chrono::NaiveDate;
use chrono::Utc;
use color_eyre::eyre;
use color_eyre::eyre::WrapErr;
use serde::Deserialize;
use serde::Serialize;
use std::collections::BTreeSet;
use std::fs::File;
use std::io::Read;
use std::io::Seek;
use std::io::SeekFrom;
use std::io::Write;
use std::path::Path;
use std::path::PathBuf;

/// The file name prefix of komorebi's plaintext log: the file for a day is `<prefix>.<YYYY-MM-DD>`.
pub const PLAINTEXT_PREFIX: &str = "komorebi_plaintext.log";

/// Returns the file name of the log that tracing's daily roller writes for a given UTC date.
///
/// # Example
///
/// ```rust
/// use chrono::NaiveDate;
/// use komorebi_recorder::sources::komorebi_log::log_file_name;
///
/// let date = NaiveDate::from_ymd_opt(2026, 10, 27).ok_or("not a date")?;
/// assert_eq!(log_file_name(date), "komorebi_plaintext.log.2026-10-27");
/// # Ok::<(), Box<dyn std::error::Error>>(())
/// ```
#[must_use]
pub fn log_file_name(date: NaiveDate) -> String {
    format!("{PLAINTEXT_PREFIX}.{}", date.format("%Y-%m-%d"))
}

/// Returns the directories the komorebi of this flavor may keep its log in, most likely first.
///
/// | flavor | candidates |
/// |---|---|
/// | [`Flavor::Macos`] | `data_dir` |
/// | [`Flavor::Windows`] | `temp_dir`, then `data_dir` |
///
/// # Example
///
/// ```rust
/// use komorebi_recorder::analysis::flavor::Flavor;
/// use komorebi_recorder::sources::komorebi_log::log_dir_candidates;
/// use std::path::{Path, PathBuf};
///
/// let (data, temp) = (Path::new("/data"), Path::new("/temp"));
/// assert_eq!(log_dir_candidates(Flavor::Macos, data, temp), [PathBuf::from("/data")]);
/// assert_eq!(
///     log_dir_candidates(Flavor::Windows, data, temp),
///     [PathBuf::from("/temp"), PathBuf::from("/data")]
/// );
/// ```
#[must_use]
pub fn log_dir_candidates(flavor: Flavor, data_dir: &Path, temp_dir: &Path) -> Vec<PathBuf> {
    match flavor {
        Flavor::Macos | Flavor::Kde => vec![data_dir.to_path_buf()],
        Flavor::Windows => vec![temp_dir.to_path_buf(), data_dir.to_path_buf()],
    }
}

fn has_komorebi_log(dir: &Path) -> bool {
    std::fs::read_dir(dir).is_ok_and(|entries| {
        entries.filter_map(Result::ok).any(|entry| {
            entry
                .file_name()
                .to_string_lossy()
                .starts_with(PLAINTEXT_PREFIX)
        })
    })
}

/// Returns the directory komorebi is logging into: the first of [`log_dir_candidates`] that has a
/// log in it, else the most likely one.
///
/// If there is none, komorebi may not have started yet, and will write its log in the most likely
/// directory when it does. A directory has a log in it if it holds any file whose name starts with
/// [`PLAINTEXT_PREFIX`]. A directory that cannot be read has none.
///
/// # Example
///
/// Where no candidate has a log, the answer is the first candidate, which is where komorebi for
/// Windows will write one:
///
/// ```rust
/// use komorebi_recorder::analysis::flavor::Flavor;
/// use komorebi_recorder::sources::komorebi_log::find_log_dir;
/// use std::path::{Path, PathBuf};
///
/// let dir = find_log_dir(
///     Flavor::Windows,
///     Path::new("/no/such/data-dir"),
///     Path::new("/no/such/temp-dir"),
/// );
/// assert_eq!(dir, PathBuf::from("/no/such/temp-dir"));
/// ```
///
/// The case where a log exists in either directory is covered by
/// `the_log_dir_is_where_a_log_is_else_the_likeliest_place` in this file's unit tests.
#[must_use]
pub fn find_log_dir(flavor: Flavor, data_dir: &Path, temp_dir: &Path) -> PathBuf {
    let candidates = log_dir_candidates(flavor, data_dir, temp_dir);
    candidates
        .iter()
        .find(|dir| has_komorebi_log(dir))
        .or_else(|| candidates.first())
        .cloned()
        .unwrap_or_else(|| data_dir.to_path_buf())
}

/// Where in komorebi's log the recording started: the UTC date of the file komorebi was writing
/// and that file's length at that moment.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LogStart {
    /// The UTC date of the rolled file that was being written (see [`log_file_name`]).
    pub date: NaiveDate,
    /// The length in bytes of that file when the recording started: the first byte of the slice.
    /// `0` if the file did not exist yet.
    pub offset: u64,
}

/// One contiguous byte range of one rolled log file, as the manifest (`log_segments`) records it.
///
/// The slice in `komorebi.log` is the segments' bytes one after the other, in date order.
///
/// # Example
///
/// ```rust
/// use komorebi_recorder::sources::komorebi_log::LogSegment;
///
/// let segment = LogSegment {
///     file: "komorebi_plaintext.log.2026-10-26".to_string(),
///     start_offset: 7,
///     end_offset: 25,
///     out_offset: 0,
/// };
/// assert_eq!(segment.len(), 18);
/// assert!(!segment.is_empty());
/// ```
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LogSegment {
    /// The rolled file's name (see [`log_file_name`]), within the directory komorebi logs into.
    /// That is komorebi's data directory unless the manifest's `komorebi.log_dir` says otherwise.
    pub file: String,
    /// The offset in that file of the segment's first byte.
    pub start_offset: u64,
    /// The offset in that file one past the segment's last byte: how far the file had been copied.
    pub end_offset: u64,
    /// Where this segment begins inside the session's `komorebi.log`.
    pub out_offset: u64,
}

impl LogSegment {
    /// Returns the number of bytes in the segment: `end_offset - start_offset`, or `0` if the
    /// end is not after the start.
    #[must_use]
    pub fn len(&self) -> u64 {
        self.end_offset.saturating_sub(self.start_offset)
    }

    /// Returns `true` if and only if the segment holds no bytes.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

fn file_len(path: &Path) -> u64 {
    std::fs::metadata(path).map_or(0, |metadata| metadata.len())
}

/// Records the current end of today's (UTC) log file, where `now` says what today is.
///
/// The returned [`LogStart`] has the UTC date of `now` and the length of that day's file in `dir`,
/// or `0` if there is no such file yet.
///
/// # Example
///
/// ```rust
/// use chrono::{TimeZone, Utc};
/// use komorebi_recorder::sources::komorebi_log::snapshot_start;
/// use std::path::Path;
///
/// let now = Utc.with_ymd_and_hms(2026, 10, 27, 12, 0, 0).single().ok_or("not a time")?;
/// let start = snapshot_start(Path::new("/no/such/log-dir"), now);
/// assert_eq!(start.date.to_string(), "2026-10-27");
/// assert_eq!(start.offset, 0);
/// # Ok::<(), Box<dyn std::error::Error>>(())
/// ```
#[must_use]
pub fn snapshot_start(dir: &Path, now: DateTime<Utc>) -> LogStart {
    let date = now.date_naive();
    LogStart {
        date,
        offset: file_len(&dir.join(log_file_name(date))),
    }
}

/// Works out which byte ranges of which files cover the log from `start` up to `end_now`,
/// including every whole file for any UTC dates in between.
///
/// The first file is read from `start.offset`; later ones from the beginning. A file that does not
/// exist (komorebi was not logging that day), or has nothing new in it, contributes no segment.
/// The segments are in date order and their `out_offset`s are cumulative, so they tile the slice.
/// Segments never start before `start.date`, even if `end_now` is earlier (the clock stepped back).
///
/// # Example
///
/// With no log, there is nothing to copy:
///
/// ```rust
/// use chrono::{TimeZone, Utc};
/// use komorebi_recorder::sources::komorebi_log::{plan_segments, snapshot_start};
/// use std::path::Path;
///
/// let dir = Path::new("/no/such/log-dir");
/// let begin = Utc.with_ymd_and_hms(2026, 10, 27, 12, 0, 0).single().ok_or("not a time")?;
/// let end = Utc.with_ymd_and_hms(2026, 10, 28, 1, 0, 0).single().ok_or("not a time")?;
/// let start = snapshot_start(dir, begin);
/// assert!(plan_segments(dir, &start, end).is_empty());
/// # Ok::<(), Box<dyn std::error::Error>>(())
/// ```
///
/// The cases with files, a recording that crosses a UTC midnight among them, are covered by
/// `follows_across_a_utc_midnight_rollover` and `a_missing_log_yields_no_segments_until_it_appears`
/// in this file's unit tests.
#[must_use]
pub fn plan_segments(dir: &Path, start: &LogStart, end_now: DateTime<Utc>) -> Vec<LogSegment> {
    let end_date = end_now.date_naive().max(start.date);
    let mut segments = Vec::new();
    let mut date = start.date;
    let mut out_offset = 0;

    while date <= end_date {
        let file = log_file_name(date);
        let path = dir.join(&file);
        let start_offset = if date == start.date { start.offset } else { 0 };
        let end_offset = file_len(&path);

        // a file that does not exist (komorebi was not logging that day) contributes nothing
        if path.exists() && end_offset > start_offset {
            let segment = LogSegment {
                file,
                start_offset,
                end_offset,
                out_offset,
            };
            out_offset += segment.len();
            segments.push(segment);
        }

        date += Duration::days(1);
    }

    segments
}

/// Copies the recording's slice of komorebi's log into the session's `komorebi.log` as it grows,
/// instead of in one go at the end, so a crash loses at most the last moment of it.
///
/// Rolled files are followed in date order. Once a later day's file has begun to be copied,
/// earlier files are frozen: a straggling late write to yesterday's file cannot be appended
/// without scrambling the order of the slice, and it would be a handful of lines at most.
///
/// The tailer copies byte ranges, not lines, and only ever appends to `komorebi.log`. It never
/// reads komorebi's log before the offset in [`LogStart`].
///
/// # Example
///
/// Following a log takes komorebi's log files and a session directory, so this example only
/// compiles. The behavior is covered by these unit tests in this file: `follows_a_file_as_it_grows`,
/// `follows_across_a_utc_midnight_rollover`,
/// `a_straggling_write_to_yesterdays_file_does_not_scramble_the_slice` and
/// `a_missing_log_yields_no_segments_until_it_appears`.
///
/// ```rust,no_run
/// use chrono::Utc;
/// use komorebi_recorder::sources::komorebi_log::{LogTailer, slice_path, snapshot_start};
/// use std::path::Path;
///
/// let log_dir = Path::new("/path/to/komorebi/log-dir");
/// let session = Path::new("session");
///
/// let start = snapshot_start(log_dir, Utc::now());
/// let mut tailer = LogTailer::create(log_dir, start, &slice_path(session))?;
///
/// // ... on a timer while recording ...
/// let appended = tailer.sync(Utc::now())?;
/// println!("{appended} bytes of komorebi's log copied; {} segment(s)", tailer.segments().len());
/// # Ok::<(), Box<dyn std::error::Error>>(())
/// ```
pub struct LogTailer {
    dir: PathBuf,
    start: LogStart,
    out: File,
    out_len: u64,
    segments: Vec<LogSegment>,
}

impl LogTailer {
    /// Starts following the log in `dir` from `start`, writing the slice to `out`.
    ///
    /// `out` is created, or truncated if it exists. Nothing is read from the log until the first
    /// [`sync`](Self::sync).
    ///
    /// # Errors
    ///
    /// Returns an error if `out` cannot be created.
    pub fn create(dir: &Path, start: LogStart, out: &Path) -> eyre::Result<Self> {
        Ok(Self {
            dir: dir.to_path_buf(),
            start,
            out: File::create(out)
                .wrap_err_with(|| format!("could not create {}", out.display()))?,
            out_len: 0,
            segments: Vec::new(),
        })
    }

    /// Copies whatever komorebi has written since the last sync, and returns the number of bytes
    /// appended to `komorebi.log` (`0` if there was nothing new).
    ///
    /// `now` decides which UTC dates' files are in play (see [`plan_segments`]). Every segment that
    /// has bytes is flushed before this returns.
    ///
    /// # Errors
    ///
    /// Returns an error if a log file that is to be copied cannot be opened or read, or if
    /// `komorebi.log` cannot be written or flushed.
    pub fn sync(&mut self, now: DateTime<Utc>) -> eyre::Result<u64> {
        let planned = plan_segments(&self.dir, &self.start, now);
        let mut appended = 0;

        for plan in planned {
            let index = match self.segments.iter().position(|seg| seg.file == plan.file) {
                Some(index) => index,
                None => {
                    self.segments.push(LogSegment {
                        file: plan.file.clone(),
                        start_offset: plan.start_offset,
                        end_offset: plan.start_offset,
                        out_offset: self.out_len,
                    });
                    self.segments.len() - 1
                }
            };

            // a later file is already being followed: this one is frozen
            if index + 1 < self.segments.len() {
                continue;
            }

            let copied_to = self.segments[index].end_offset;
            if plan.end_offset <= copied_to {
                continue;
            }

            let path = self.dir.join(&plan.file);
            let mut file =
                File::open(&path).wrap_err_with(|| format!("could not open {}", path.display()))?;
            file.seek(SeekFrom::Start(copied_to))?;
            let copied = std::io::copy(&mut file.take(plan.end_offset - copied_to), &mut self.out)?;

            self.out.flush()?;
            self.segments[index].end_offset = copied_to + copied;
            self.out_len += copied;
            appended += copied;
        }

        Ok(appended)
    }

    /// Returns what has been copied so far, as the segments the manifest records. Each segment's
    /// `end_offset` is how far its file has been copied, not the file's current length.
    #[must_use]
    pub fn segments(&self) -> &[LogSegment] {
        &self.segments
    }
}

/// Parses the RFC 3339 timestamp that starts a log line, as UTC.
///
/// Returns `None` for a line that does not start with one: a continuation line (a panic message,
/// a backtrace frame) or anything else.
///
/// # Example
///
/// ```rust
/// use komorebi_recorder::sources::komorebi_log::parse_timestamp;
///
/// let line = "2026-10-26T00:07:14.370960Z  INFO process_event: komorebi::process_event: processing event";
/// let timestamp = parse_timestamp(line).ok_or("no timestamp")?;
/// assert_eq!(timestamp.to_rfc3339(), "2026-10-26T00:07:14.370960+00:00");
///
/// assert_eq!(parse_timestamp("   0: backtrace::capture"), None);
/// assert_eq!(parse_timestamp("2026-10-26 00:07:14 not rfc3339"), None);
/// # Ok::<(), Box<dyn std::error::Error>>(())
/// ```
#[must_use]
pub fn parse_timestamp(line: &str) -> Option<DateTime<Utc>> {
    let token = line.split_whitespace().next()?;
    // cheap reject before the real parser: `YYYY-MM-DDT...`
    if token.len() < 20
        || token.as_bytes().get(4) != Some(&b'-')
        || token.as_bytes().get(10) != Some(&b'T')
    {
        return None;
    }

    DateTime::parse_from_rfc3339(token)
        .ok()
        .map(|timestamp| timestamp.with_timezone(&Utc))
}

/// Returns the level token of a `tracing_subscriber::fmt` line: its second whitespace-separated
/// token, if that is one of `ERROR`, `WARN`, `INFO`, `DEBUG` or `TRACE`.
///
/// The line is not checked for a timestamp first; [`observed_levels`] does that.
///
/// # Example
///
/// ```rust
/// use komorebi_recorder::sources::komorebi_log::parse_level;
///
/// let line = "2026-10-26T00:07:14.370960Z  INFO process_event: komorebi::process_event: processing event";
/// assert_eq!(parse_level(line), Some("INFO"));
/// assert_eq!(parse_level("   0: backtrace::capture"), None);
/// ```
#[must_use]
pub fn parse_level(line: &str) -> Option<&str> {
    let level = line.split_whitespace().nth(1)?;
    matches!(level, "ERROR" | "WARN" | "INFO" | "DEBUG" | "TRACE").then_some(level)
}

/// A single line of the slice with its place in the file and its (possibly inherited) time.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LogLine {
    /// 1-based line number within the slice.
    pub number: u64,
    /// The byte offset of the line's first byte within the slice.
    pub offset: u64,
    /// The length of the line in bytes, including its line terminator.
    pub len: u64,
    /// The line's own timestamp, or the previous timestamped line's for continuation lines.
    /// `None` for lines before the first timestamped one.
    pub timestamp: Option<DateTime<Utc>>,
    /// `true` if the line has no timestamp of its own.
    pub is_continuation: bool,
}

/// Splits a slice into lines, giving continuation lines (panic messages, backtraces) the
/// timestamp of the line they follow.
///
/// The lines tile the slice: their lengths add up to its length. A final line with no terminator
/// is included.
///
/// # Example
///
/// ```rust
/// use komorebi_recorder::sources::komorebi_log::index_lines;
///
/// let slice = "2026-10-26T00:00:01.000000Z ERROR komorebi: panicked\n   0: frame\n2026-10-26T00:00:02.000000Z  INFO next\n";
/// let lines = index_lines(slice.as_bytes());
///
/// assert_eq!(lines.len(), 3);
/// assert!(lines[1].is_continuation);
/// assert_eq!(lines[1].timestamp, lines[0].timestamp, "inherited from the line it follows");
/// assert_ne!(lines[2].timestamp, lines[0].timestamp);
/// assert_eq!(lines.iter().map(|line| line.len).sum::<u64>(), slice.len() as u64);
/// ```
#[must_use]
pub fn index_lines(slice: &[u8]) -> Vec<LogLine> {
    let mut lines = Vec::new();
    let mut offset = 0_usize;
    let mut current: Option<DateTime<Utc>> = None;

    for (index, raw) in slice.split_inclusive(|byte| *byte == b'\n').enumerate() {
        let number = index as u64 + 1;
        let text = String::from_utf8_lossy(raw);
        let own = parse_timestamp(&text);
        let is_continuation = own.is_none();
        if own.is_some() {
            current = own;
        }

        lines.push(LogLine {
            number,
            offset: offset as u64,
            len: raw.len() as u64,
            timestamp: current,
            is_continuation,
        });

        offset += raw.len();
    }

    lines
}

/// Returns the distinct levels that appear in a slice of log text, on lines that start with a
/// timestamp.
///
/// # Example
///
/// ```rust
/// use komorebi_recorder::sources::komorebi_log::observed_levels;
///
/// let text = "2026-10-26T00:00:01.000000Z  INFO a\n   continuation\n2026-10-26T00:00:02.000000Z DEBUG b\n";
/// let levels: Vec<String> = observed_levels(text).into_iter().collect();
/// assert_eq!(levels, ["DEBUG", "INFO"]);
/// ```
#[must_use]
pub fn observed_levels(text: &str) -> BTreeSet<String> {
    text.lines()
        .filter(|line| parse_timestamp(line).is_some())
        .filter_map(parse_level)
        .map(str::to_string)
        .collect()
}

/// How komorebi's log verbosity was determined, as the manifest (`komorebi.log_level`) records it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
pub struct DetectedLevel {
    /// The level, in lowercase: `error`, `warn`, `info`, `debug` or `trace`. When `RUST_LOG` is
    /// set but names no level, its value as it is.
    pub level: String,
    /// Where the level came from: `RUST_LOG`, `--log-level` or `default`.
    pub source: String,
}

/// Works out komorebi's log level from its command line and environment.
///
/// This mirrors the logic in `komorebi/src/main.rs`: `RUST_LOG` wins if it was already set,
/// otherwise `--log-level`, which defaults to `info`. `args` are komorebi's command-line arguments
/// and `environ` its environment as `KEY=VALUE` strings. Of a `RUST_LOG` such as
/// `info,komorebi=debug`, the most verbose level named by any of its comma-separated directives is
/// the level; directives that name no level are ignored.
///
/// # Example
///
/// ```rust
/// use komorebi_recorder::sources::komorebi_log::level_from_process;
///
/// let args = ["komorebi".to_string(), "--log-level".to_string(), "info".to_string()];
///
/// // the flag, when the environment says nothing
/// let detected = level_from_process(&args, &[]);
/// assert_eq!((detected.level.as_str(), detected.source.as_str()), ("info", "--log-level"));
///
/// // RUST_LOG wins over the flag, and the most verbose directive wins within it
/// let environ = ["RUST_LOG=warn,komorebi=debug".to_string()];
/// let detected = level_from_process(&args, &environ);
/// assert_eq!((detected.level.as_str(), detected.source.as_str()), ("debug", "RUST_LOG"));
///
/// // neither: komorebi's default
/// assert_eq!(level_from_process(&["komorebi".to_string()], &[]).source, "default");
/// ```
#[must_use]
pub fn level_from_process(args: &[String], environ: &[String]) -> DetectedLevel {
    if let Some(value) = environ
        .iter()
        .find_map(|entry| entry.strip_prefix("RUST_LOG="))
    {
        // directives look like `komorebi=trace` or `info,komorebi=debug`; the most verbose wins
        let level = value
            .split(',')
            .filter_map(|directive| directive.rsplit('=').next())
            .filter_map(normalize_level)
            .max_by_key(|level| level_rank(level))
            .map_or_else(|| value.to_string(), str::to_string);

        return DetectedLevel {
            level,
            source: "RUST_LOG".to_string(),
        };
    }

    let mut iter = args.iter();
    while let Some(arg) = iter.next() {
        let value = if let Some(value) = arg.strip_prefix("--log-level=") {
            Some(value)
        } else if arg == "--log-level" {
            iter.next().map(String::as_str)
        } else {
            None
        };

        if let Some(level) = value.and_then(normalize_level) {
            return DetectedLevel {
                level: level.to_string(),
                source: "--log-level".to_string(),
            };
        }
    }

    DetectedLevel {
        level: "info".to_string(),
        source: "default".to_string(),
    }
}

fn normalize_level(level: &str) -> Option<&'static str> {
    match level.to_ascii_lowercase().as_str() {
        "error" => Some("error"),
        "warn" => Some("warn"),
        "info" => Some("info"),
        "debug" => Some("debug"),
        "trace" => Some("trace"),
        _ => None,
    }
}

fn level_rank(level: &str) -> u8 {
    match level {
        "error" => 0,
        "warn" => 1,
        "info" => 2,
        "debug" => 3,
        _ => 4,
    }
}

/// Returns a user-facing warning if the level is too quiet to explain a bug: anything quieter
/// than `debug` (`error`, `warn` or `info`, in any case). A level that is not recognized gets none.
///
/// # Example
///
/// ```rust
/// use komorebi_recorder::sources::komorebi_log::level_warning;
///
/// let warning = level_warning("info").ok_or("no warning")?;
/// assert!(warning.contains("--log-level trace"));
/// assert!(level_warning("debug").is_none());
/// assert!(level_warning("TRACE").is_none());
/// # Ok::<(), Box<dyn std::error::Error>>(())
/// ```
#[must_use]
pub fn level_warning(level: &str) -> Option<String> {
    (level_rank(&level.to_ascii_lowercase()) < level_rank("debug")).then(|| {
        format!(
            "komorebi is logging at {}; restart it with `--log-level trace` for full detail \
             (the recorder never changes komorebi's log level itself)",
            level.to_ascii_uppercase()
        )
    })
}

/// Returns the path of `komorebi.log` inside a session directory: the concatenated slice.
///
/// # Example
///
/// ```rust
/// use komorebi_recorder::sources::komorebi_log::slice_path;
/// use std::path::Path;
///
/// assert_eq!(slice_path(Path::new("session")), Path::new("session/komorebi.log"));
/// ```
#[must_use]
pub fn slice_path(session_dir: &Path) -> PathBuf {
    session_dir.join("komorebi.log")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn komorebi_for_mac_logs_in_its_data_dir_and_komorebi_for_windows_in_temp() {
        let (data, temp) = (Path::new("/data"), Path::new("/temp"));
        assert_eq!(
            log_dir_candidates(Flavor::Macos, data, temp),
            [PathBuf::from("/data")]
        );
        assert_eq!(
            log_dir_candidates(Flavor::Windows, data, temp),
            [PathBuf::from("/temp"), PathBuf::from("/data")]
        );
    }

    #[test]
    fn the_log_dir_is_where_a_log_is_else_the_likeliest_place() {
        let (data, temp) = (temp_dir("logdir-data"), temp_dir("logdir-temp"));

        // nobody has logged yet: where komorebi for Windows will
        assert_eq!(find_log_dir(Flavor::Windows, &data, &temp), temp);

        // a log in the data dir (the README's old location) is found there
        std::fs::write(data.join("komorebi_plaintext.log.2026-10-27"), "x").expect("log");
        assert_eq!(find_log_dir(Flavor::Windows, &data, &temp), data);

        // but temp wins when both have one: it is where the code writes
        std::fs::write(temp.join("komorebi_plaintext.log.2026-10-27"), "x").expect("log");
        assert_eq!(find_log_dir(Flavor::Windows, &data, &temp), temp);

        // the Mac only ever looks in its data dir
        assert_eq!(find_log_dir(Flavor::Macos, &data, &temp), data);
    }

    use chrono::TimeZone;

    fn utc(y: i32, m: u32, d: u32, h: u32, min: u32, s: u32) -> DateTime<Utc> {
        Utc.with_ymd_and_hms(y, m, d, h, min, s)
            .single()
            .expect("valid time")
    }

    fn temp_dir(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "komorebi-recorder-test-{name}-{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("create temp dir");
        dir
    }

    #[test]
    fn parses_tracing_timestamps() {
        let line = "2026-10-26T00:07:14.370960Z  INFO process_event: komorebi::process_event: processing event";
        let timestamp = parse_timestamp(line).expect("timestamp");
        assert_eq!(
            timestamp,
            utc(2026, 10, 26, 0, 7, 14) + Duration::microseconds(370_960)
        );
        assert_eq!(timestamp.to_rfc3339(), "2026-10-26T00:07:14.370960+00:00");
        assert_eq!(parse_level(line), Some("INFO"));
    }

    #[test]
    fn rejects_non_timestamped_lines() {
        for line in [
            "",
            "   0: backtrace::capture",
            "thread 'main' panicked at src/lib.rs:1:1:",
            "2026-10-26 00:07:14 not rfc3339",
        ] {
            assert_eq!(parse_timestamp(line), None, "{line:?}");
        }
    }

    #[test]
    fn continuation_lines_inherit_the_previous_timestamp() {
        let text = "2026-10-26T00:00:01.000000Z ERROR komorebi: panicked\n   0: frame\n   1: frame\n2026-10-26T00:00:02.000000Z  INFO next\n";
        let lines = index_lines(text.as_bytes());
        assert_eq!(lines.len(), 4);
        assert!(!lines[0].is_continuation);
        assert!(lines[1].is_continuation && lines[2].is_continuation);
        assert_eq!(lines[1].timestamp, lines[0].timestamp);
        assert_eq!(lines[2].timestamp, lines[0].timestamp);
        assert_ne!(lines[3].timestamp, lines[0].timestamp);
        assert_eq!(lines[1].offset, lines[0].len);
        assert_eq!(
            lines.iter().map(|line| line.len).sum::<u64>(),
            text.len() as u64
        );
    }

    #[test]
    fn leading_untimestamped_lines_have_no_time() {
        let lines = index_lines(b"orphan\n2026-10-26T00:00:02.000000Z  INFO x\n");
        assert_eq!(lines[0].timestamp, None);
        assert!(lines[1].timestamp.is_some());
    }

    fn slice_text(dir: &Path) -> String {
        std::fs::read_to_string(dir.join("komorebi.log")).expect("slice")
    }

    #[test]
    fn follows_a_file_as_it_grows() {
        let dir = temp_dir("single");
        let path = dir.join(log_file_name(utc(2026, 10, 26, 12, 0, 0).date_naive()));
        std::fs::write(&path, "before\n").expect("write");

        let start = snapshot_start(&dir, utc(2026, 10, 26, 12, 0, 0));
        assert_eq!(start.offset, 7);
        let mut tailer = LogTailer::create(&dir, start, &dir.join("komorebi.log")).expect("create");

        assert_eq!(
            tailer.sync(utc(2026, 10, 26, 12, 0, 1)).expect("sync"),
            0,
            "nothing new yet"
        );

        std::fs::write(&path, "before\nduring 1\n").expect("write");
        assert_eq!(tailer.sync(utc(2026, 10, 26, 12, 0, 2)).expect("sync"), 9);
        assert_eq!(
            slice_text(&dir),
            "during 1\n",
            "what was already there is not part of the slice"
        );

        std::fs::write(&path, "before\nduring 1\nduring 2\n").expect("write");
        tailer.sync(utc(2026, 10, 26, 12, 0, 3)).expect("sync");
        tailer
            .sync(utc(2026, 10, 26, 12, 0, 4))
            .expect("sync again is a no-op");
        assert_eq!(slice_text(&dir), "during 1\nduring 2\n");

        let segments = tailer.segments();
        assert_eq!(segments.len(), 1);
        assert_eq!((segments[0].start_offset, segments[0].end_offset), (7, 25));
        assert_eq!(segments[0].len(), 18);
    }

    #[test]
    fn follows_across_a_utc_midnight_rollover() {
        let dir = temp_dir("rollover");
        let day1 = utc(2026, 10, 25, 23, 59, 50);
        let day1_path = dir.join(log_file_name(day1.date_naive()));
        let day2_path = dir.join(log_file_name(utc(2026, 10, 26, 0, 0, 0).date_naive()));
        std::fs::write(&day1_path, "old line\n").expect("write");

        let start = snapshot_start(&dir, day1);
        let mut tailer = LogTailer::create(&dir, start, &dir.join("komorebi.log")).expect("create");

        std::fs::write(&day1_path, "old line\nstart here\n").expect("write");
        tailer.sync(utc(2026, 10, 25, 23, 59, 55)).expect("sync");

        // komorebi appended to day 1, then rolled to day 2 at midnight UTC
        std::fs::write(&day1_path, "old line\nstart here\nlast of day 1\n").expect("write");
        std::fs::write(&day2_path, "first of day 2\n").expect("write");
        tailer.sync(utc(2026, 10, 26, 0, 0, 1)).expect("sync");

        std::fs::write(&day2_path, "first of day 2\nsecond of day 2\n").expect("write");
        tailer.sync(utc(2026, 10, 26, 0, 0, 10)).expect("sync");

        assert_eq!(
            slice_text(&dir),
            "start here\nlast of day 1\nfirst of day 2\nsecond of day 2\n"
        );

        let segments = tailer.segments();
        assert_eq!(segments.len(), 2);
        assert_eq!(segments[0].file, "komorebi_plaintext.log.2026-10-25");
        assert_eq!(segments[1].file, "komorebi_plaintext.log.2026-10-26");
        assert_eq!(segments[1].start_offset, 0);
        assert_eq!(segments[1].out_offset, segments[0].len());
    }

    #[test]
    fn a_straggling_write_to_yesterdays_file_does_not_scramble_the_slice() {
        let dir = temp_dir("straggler");
        let day1 = utc(2026, 10, 25, 23, 59, 50);
        let day1_path = dir.join(log_file_name(day1.date_naive()));
        let day2_path = dir.join(log_file_name(utc(2026, 10, 26, 0, 0, 0).date_naive()));
        std::fs::write(&day1_path, "").expect("write");

        let start = snapshot_start(&dir, day1);
        let mut tailer = LogTailer::create(&dir, start, &dir.join("komorebi.log")).expect("create");

        std::fs::write(&day1_path, "a\n").expect("write");
        std::fs::write(&day2_path, "b\n").expect("write");
        tailer.sync(utc(2026, 10, 26, 0, 0, 1)).expect("sync");

        // a late flush lands in day 1 after day 2 was already being followed
        std::fs::write(&day1_path, "a\nlate\n").expect("write");
        std::fs::write(&day2_path, "b\nc\n").expect("write");
        tailer.sync(utc(2026, 10, 26, 0, 0, 2)).expect("sync");

        assert_eq!(
            slice_text(&dir),
            "a\nb\nc\n",
            "the late line is dropped, the order is kept"
        );
    }

    #[test]
    fn a_missing_log_yields_no_segments_until_it_appears() {
        let dir = temp_dir("missing");
        let start = snapshot_start(&dir, utc(2026, 10, 26, 12, 0, 0));
        assert_eq!(start.offset, 0);
        assert!(plan_segments(&dir, &start, utc(2026, 10, 26, 12, 1, 0)).is_empty());

        let mut tailer = LogTailer::create(&dir, start, &dir.join("komorebi.log")).expect("create");
        assert_eq!(tailer.sync(utc(2026, 10, 26, 12, 0, 30)).expect("sync"), 0);
        assert!(tailer.segments().is_empty());

        // komorebi starts logging mid-recording
        std::fs::write(
            dir.join(log_file_name(utc(2026, 10, 26, 12, 0, 0).date_naive())),
            "hello\n",
        )
        .expect("write");
        tailer.sync(utc(2026, 10, 26, 12, 1, 0)).expect("sync");
        assert_eq!(slice_text(&dir), "hello\n");
    }

    #[test]
    fn detects_levels_from_the_command_line() {
        let args = |list: &[&str]| {
            list.iter()
                .map(|arg| (*arg).to_string())
                .collect::<Vec<_>>()
        };
        let none = args(&[]);

        assert_eq!(
            level_from_process(&args(&["komorebi"]), &none).level,
            "info"
        );
        assert_eq!(
            level_from_process(&args(&["komorebi", "--log-level", "trace"]), &none),
            DetectedLevel {
                level: "trace".into(),
                source: "--log-level".into()
            }
        );
        assert_eq!(
            level_from_process(&args(&["komorebi", "--log-level=debug"]), &none).level,
            "debug"
        );
    }

    #[test]
    fn rust_log_wins_over_the_flag() {
        let args = vec![
            "komorebi".to_string(),
            "--log-level".to_string(),
            "info".to_string(),
        ];
        let environ = vec!["HOME=/x".to_string(), "RUST_LOG=komorebi=trace".to_string()];
        let detected = level_from_process(&args, &environ);
        assert_eq!(
            detected,
            DetectedLevel {
                level: "trace".into(),
                source: "RUST_LOG".into()
            }
        );

        let environ = vec!["RUST_LOG=warn,komorebi=debug".to_string()];
        assert_eq!(level_from_process(&args, &environ).level, "debug");
    }

    #[test]
    fn warns_only_when_the_level_is_too_quiet() {
        assert!(
            level_warning("info")
                .expect("warning")
                .contains("--log-level trace")
        );
        assert!(level_warning("warn").is_some());
        assert!(level_warning("debug").is_none());
        assert!(level_warning("TRACE").is_none());
    }

    #[test]
    fn observes_levels_in_a_slice() {
        let text = "2026-10-26T00:00:01.000000Z  INFO a\n   continuation\n2026-10-26T00:00:02.000000Z DEBUG b\n";
        let levels = observed_levels(text);
        assert!(levels.contains("INFO") && levels.contains("DEBUG"));
        assert_eq!(levels.len(), 2);
    }
}
