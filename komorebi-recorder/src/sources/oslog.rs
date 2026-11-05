//! The macOS unified log, captured as a small set of `log stream` child processes.
//!
//! Instead of one mixed firehose there is one stream per *seam* that matters, each teed
//! untouched into its own file under `oslog/`:
//!
//! | stream | predicate | shows |
//! |---|---|---|
//! | `komorebi` | `processID == <komorebi pid>` | system frameworks as the daemon sees them |
//! | `system` | AX / `HIServices` / workspace subsystems | the system side in between |
//! | `app:*` | `processID == <pid>`, one per managed app | the same frameworks from the app's side |
//!
//! Debug-level events only exist live (not in `log show`), which is why this streams rather than
//! querying afterwards. `log stream` does not need root; `<private>` values stay redacted.
//!
//! # Files written
//!
//! | file | stream | `source` label |
//! |---|---|---|
//! | `oslog/komorebi.ndjson` | komorebi's process | `oslog:komorebi` |
//! | `oslog/komorebi-<pid>.ndjson` | a komorebi that was started during the recording, one file per process after the first | `oslog:komorebi` |
//! | `oslog/system.ndjson` | the `system` stream | `oslog:system` |
//! | `oslog/apps/<exe>-<pid>.ndjson` | one managed process; `<exe>` is made safe for a file name by [`sanitize_file_name`] | `oslog:app:<exe>` |
//! | `oslog/all.ndjson` | an unfiltered, system-wide stream (`--oslog-all`) | `oslog:all` |
//!
//! Each file is NDJSON: one JSON object per line, exactly as `log stream --style ndjson` printed
//! it. Lines that do not start with `{` are not written (the `Filtering the log data using ...`
//! header is kept in the manifest's [`StreamInfo::header`] instead). [`OsLogRecord`] names the keys
//! the recorder reads from a record; a Mac record has many more, which stay in the file. A
//! `source` label is what the timeline and the investigation commands call the stream.
//!
//! A `log stream` child that exits is started again into the same file, appending, so nothing
//! already captured is touched; the gap is reported as an incident ([`StreamSet::reap`]). One that
//! dies within seconds of starting never worked (a bad predicate, no access to the log) and is
//! reported lost instead of restarted in a loop.
//!
//! # Hosts
//!
//! Only macOS records with this module: [`StreamSet`] runs `/usr/bin/log`. The reading side
//! ([`OsLogRecord`], [`extract_time_ns`]) is the same on every host, because the Windows OS trace
//! (`wintrace`) writes its records in this shape, under the same `oslog/` directory, so that a
//! session recorded on either host is read the same way. A record's time is either `machTimestamp`
//! (a tick count of the Mac's clock) or `hostNs` (already nanoseconds on the master clock).
//!
//! # The `system` predicate
//!
//! The default `system` predicate was chosen by measuring event volume (debug level, 15 s, a real
//! desktop running komorebi):
//!
//! | candidate | events/s | MB/min |
//! |---|---:|---:|
//! | `com.apple.accessibility*` | 0.4 | 0.02 |
//! | `com.apple.SkyLight` | 2.4 | 0.14 |
//! | axserver / `HIServices` / workspace | 0 | 0 |
//! | `com.apple.launchservices` (system-wide) | 497 | 29.0 |
//! | `com.apple.runningboard` (system-wide) | 289 | 16.0 |
//! | `WindowServer` process (all subsystems) | ~170 | ~10 |
//!
//! so the cheap, relevant ones are the default and the noisy daemons are opt-in with
//! `--system-process`. (The three subsystems named in the original Console filters are silent in
//! practice, but cost nothing and may carry traffic on other macOS versions.)

use crate::clock;
use crate::incidents::Draft;
use crate::incidents::kind;
use crate::platform::process;
use clap::ValueEnum;
use color_eyre::eyre;
use color_eyre::eyre::WrapErr;
use parking_lot::Mutex;
use serde::Deserialize;
use serde::Serialize;
use std::fs::File;
use std::io::BufRead;
use std::io::BufReader;
use std::io::BufWriter;
use std::io::Read;
use std::io::Write;
use std::path::Path;
use std::process::Child;
use std::process::Command;
use std::process::Stdio;
use std::sync::Arc;
use std::sync::atomic::AtomicU64;
use std::sync::atomic::Ordering;
use std::thread::JoinHandle;
use std::time::Duration;
use std::time::Instant;

/// The exact subsystems that carry the accessibility / window-server conversation, which the
/// `system` stream follows. [`system_predicate`] matches each of them.
pub const SYSTEM_SUBSYSTEMS: &[&str] = &[
    "com.apple.accessibility.axserver",
    "com.apple.HIServices",
    "com.apple.workspace",
    "com.apple.SkyLight",
];

/// Subsystem prefixes that are also part of it (`com.apple.accessibility.universalaccess`, ...).
/// [`system_predicate`] matches any subsystem that begins with one.
pub const SYSTEM_SUBSYSTEM_PREFIXES: &[&str] = &["com.apple.accessibility"];

/// How much of the unified log a stream asks for: the `--level` of `log stream`, which includes
/// events at and below the given level.
///
/// The default is [`OsLogLevel::Debug`]. Serialized, and spelled on the command line, in lowercase:
/// `default`, `info`, `debug`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize, ValueEnum)]
#[serde(rename_all = "lowercase")]
pub enum OsLogLevel {
    /// Only default-level events: the least.
    Default,
    /// Default-level and info-level events.
    Info,
    /// Default-, info- and debug-level events. Debug events exist only live, so this is the level
    /// that shows what the frameworks say about each call.
    #[default]
    Debug,
}

impl OsLogLevel {
    fn as_arg(self) -> &'static str {
        match self {
            OsLogLevel::Default => "default",
            OsLogLevel::Info => "info",
            OsLogLevel::Debug => "debug",
        }
    }
}

/// Which `log stream` child this is, and which timeline source its records belong to.
///
/// The spec is stored in the manifest, flattened into the stream's [`StreamInfo`]. Build one with
/// [`komorebi_spec`], [`system_spec`], [`app_spec`] or [`all_spec`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StreamSpec {
    /// The stream's name, unique within a recording: `komorebi` or `komorebi-<pid>`, `system`,
    /// `app:<exe>-<pid>` or `all`.
    pub name: String,
    /// The timeline source label: `oslog:komorebi`, `oslog:system`, `oslog:app:<exe>`, `oslog:all`.
    pub source: String,
    /// The `--predicate` given to `log stream`. Empty for a Windows trace stream, which has none.
    pub predicate: String,
    /// The `--level` given to `log stream`. A Windows trace stream records `debug` here, which
    /// means nothing there.
    pub level: OsLogLevel,
    /// Path of the raw ndjson relative to the session directory.
    pub file: String,
    /// The process the stream follows. `None` for a stream that follows no single process
    /// (`system`, `all`).
    pub pid: Option<i32>,
}

fn escape_predicate(value: &str) -> String {
    value.replace('\\', "\\\\").replace('"', "\\\"")
}

/// Returns the predicate that selects one process's events: `processID == <pid>`.
///
/// # Example
///
/// ```rust
/// use komorebi_recorder::sources::oslog::pid_predicate;
///
/// assert_eq!(pid_predicate(42), "processID == 42");
/// ```
#[must_use]
pub fn pid_predicate(pid: i32) -> String {
    format!("processID == {pid}")
}

/// Returns the predicate for the `system` stream: the AX/workspace subsystems plus any explicitly
/// requested daemons.
///
/// It is `subsystem IN {...}` over [`SYSTEM_SUBSYSTEMS`], then `OR subsystem BEGINSWITH "<prefix>"`
/// for each of [`SYSTEM_SUBSYSTEM_PREFIXES`], then `OR process == "<name>"` for each name in
/// `extra_processes`. A name is quoted, with `\` and `"` in it escaped. The daemons are opt-in
/// because they are noisy (see the module docs for the measured volumes).
///
/// # Example
///
/// ```rust
/// use komorebi_recorder::sources::oslog::system_predicate;
///
/// let plain = system_predicate(&[]);
/// assert!(plain.starts_with("subsystem IN {"));
/// assert!(plain.contains(r#"OR subsystem BEGINSWITH "com.apple.accessibility""#));
/// assert!(!plain.contains("process =="));
///
/// let with_daemon = system_predicate(&["WindowServer".to_string()]);
/// assert!(with_daemon.ends_with(r#" OR process == "WindowServer""#));
/// ```
#[must_use]
pub fn system_predicate(extra_processes: &[String]) -> String {
    let subsystems = SYSTEM_SUBSYSTEMS
        .iter()
        .map(|subsystem| format!("\"{subsystem}\""))
        .collect::<Vec<_>>()
        .join(", ");

    let mut predicate = format!("subsystem IN {{{subsystems}}}");
    for prefix in SYSTEM_SUBSYSTEM_PREFIXES {
        predicate.push_str(&format!(" OR subsystem BEGINSWITH \"{prefix}\""));
    }

    for process in extra_processes {
        predicate.push_str(&format!(" OR process == \"{}\"", escape_predicate(process)));
    }

    predicate
}

/// Makes a process name safe to use as a file name.
///
/// Every character other than an ASCII letter or digit, `-`, `_` and `.` becomes `_`, and leading
/// and trailing `.` and `_` are trimmed, so the result cannot name a parent directory. A name with
/// nothing left is `app`.
///
/// # Example
///
/// ```rust
/// use komorebi_recorder::sources::oslog::sanitize_file_name;
///
/// assert_eq!(sanitize_file_name("Brave Beta"), "Brave_Beta");
/// assert_eq!(sanitize_file_name("../../etc/passwd"), "etc_passwd");
/// assert_eq!(sanitize_file_name("///"), "app");
/// ```
#[must_use]
pub fn sanitize_file_name(name: &str) -> String {
    let cleaned: String = name
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.') {
                c
            } else {
                '_'
            }
        })
        .collect();

    let trimmed = cleaned.trim_matches(|c| c == '.' || c == '_');
    if trimmed.is_empty() {
        "app".to_string()
    } else {
        trimmed.to_string()
    }
}

/// Returns the stream for komorebi's process.
///
/// komorebi can be restarted during a recording; each new process gets its own file
/// (`oslog/komorebi-<pid>.ndjson`) under the same `oslog:komorebi` source, and the first keeps the
/// plain name. `first` says whether this is the first komorebi process of the recording.
///
/// # Example
///
/// ```rust
/// use komorebi_recorder::sources::oslog::{OsLogLevel, komorebi_spec};
///
/// let first = komorebi_spec(100, OsLogLevel::Debug, true);
/// assert_eq!((first.name.as_str(), first.file.as_str()), ("komorebi", "oslog/komorebi.ndjson"));
///
/// // komorebi was restarted: same source, a file of its own
/// let restarted = komorebi_spec(200, OsLogLevel::Debug, false);
/// assert_eq!(restarted.source, first.source);
/// assert_eq!(restarted.file, "oslog/komorebi-200.ndjson");
/// assert_eq!(restarted.predicate, "processID == 200");
/// ```
#[must_use]
pub fn komorebi_spec(pid: i32, level: OsLogLevel, first: bool) -> StreamSpec {
    StreamSpec {
        name: if first {
            "komorebi".to_string()
        } else {
            format!("komorebi-{pid}")
        },
        source: "oslog:komorebi".to_string(),
        predicate: pid_predicate(pid),
        level,
        file: if first {
            "oslog/komorebi.ndjson".to_string()
        } else {
            format!("oslog/komorebi-{pid}.ndjson")
        },
        pid: Some(pid),
    }
}

/// Returns the `system` stream for `predicate` (see [`system_predicate`]): source `oslog:system`,
/// file `oslog/system.ndjson`, following no single process.
#[must_use]
pub fn system_spec(predicate: String, level: OsLogLevel) -> StreamSpec {
    StreamSpec {
        name: "system".to_string(),
        source: "oslog:system".to_string(),
        predicate,
        level,
        file: "oslog/system.ndjson".to_string(),
        pid: None,
    }
}

/// Returns the stream for one managed process: named `app:<name>-<pid>`, source `oslog:app:<name>`
/// and file `oslog/apps/<name>-<pid>.ndjson` with the name made safe by [`sanitize_file_name`].
///
/// # Example
///
/// ```rust
/// use komorebi_recorder::sources::oslog::{OsLogLevel, app_spec};
///
/// let spec = app_spec("Brave Beta", 1234, OsLogLevel::Debug);
/// assert_eq!(spec.name, "app:Brave Beta-1234");
/// assert_eq!(spec.source, "oslog:app:Brave Beta");
/// assert_eq!(spec.file, "oslog/apps/Brave_Beta-1234.ndjson");
/// assert_eq!(spec.predicate, "processID == 1234");
/// assert_eq!(spec.pid, Some(1234));
/// ```
#[must_use]
pub fn app_spec(name: &str, pid: i32, level: OsLogLevel) -> StreamSpec {
    let file_name = sanitize_file_name(name);
    StreamSpec {
        name: format!("app:{name}-{pid}"),
        source: format!("oslog:app:{name}"),
        predicate: pid_predicate(pid),
        level,
        file: format!("oslog/apps/{file_name}-{pid}.ndjson"),
        pid: Some(pid),
    }
}

/// Returns the unfiltered, system-wide stream (`--oslog-all`): source `oslog:all`, file
/// `oslog/all.ndjson`. It is very large (about a gigabyte a minute, measured on a real desktop).
#[must_use]
pub fn all_spec(level: OsLogLevel) -> StreamSpec {
    StreamSpec {
        name: "all".to_string(),
        source: "oslog:all".to_string(),
        // `log stream` requires a predicate to be unfiltered-by-process; this matches everything
        predicate: "processID > 0".to_string(),
        level,
        file: "oslog/all.ndjson".to_string(),
        pid: None,
    }
}

/// What the recorder keeps of each unified-log record for indexing and display: the keys it reads
/// from a line of an `oslog/` file.
///
/// The same keys are read from a Mac's `log stream` record and from a Windows trace record
/// (`wintrace`). Every key is optional on reading: one that is missing reads as `0`, empty or
/// `None`. The names in brackets are the JSON keys.
///
/// # Example
///
/// A Mac record carries `machTimestamp`, a Windows record carries `hostNs`:
///
/// ```rust
/// use komorebi_recorder::sources::oslog::OsLogRecord;
///
/// let mac = OsLogRecord::parse(
///     r#"{"machTimestamp":29042701633017,"timestamp":"2026-10-26 12:48:36.736892-0700","processID":41666,"processImagePath":"/usr/local/bin/komorebi","subsystem":"com.apple.defaults","category":"User Defaults","messageType":"Debug","eventMessage":"found no value for key X"}"#,
/// )
/// .ok_or("not a record")?;
/// assert_eq!((mac.pid, mac.process_name()), (41666, "komorebi"));
/// assert_eq!((mac.subsystem.as_str(), mac.mach_ticks), ("com.apple.defaults", 29_042_701_633_017));
/// assert_eq!(mac.host_ns, None);
///
/// let windows = OsLogRecord::parse(
///     r#"{"hostNs":5000000000,"processID":4242,"processImagePath":"chrome.exe","subsystem":"winevent","eventMessage":"ObjectShow hwnd=0x1f2a"}"#,
/// )
/// .ok_or("not a record")?;
/// assert_eq!(windows.host_ns, Some(5_000_000_000));
/// assert_eq!(windows.process_name(), "chrome.exe");
/// # Ok::<(), Box<dyn std::error::Error>>(())
/// ```
#[derive(Debug, Clone, Deserialize)]
pub struct OsLogRecord {
    /// `machTimestamp`: the Mac's tick count of the master clock (not nanoseconds: it is converted
    /// with the session's [`Timebase`](crate::clock::Timebase)). `0` where the record has none,
    /// which is every record the Windows trace writes.
    #[serde(rename = "machTimestamp", default)]
    pub mach_ticks: u64,
    /// `hostNs`: the record's time on the master clock, in nanoseconds, where the writer already
    /// had it there (the Windows OS trace); the Mac's records carry `machTimestamp` instead.
    #[serde(rename = "hostNs", default)]
    pub host_ns: Option<u64>,
    /// `timestamp`: the record's wall-clock time as text, `2026-10-26 12:48:36.736892-0700`: local
    /// time with a UTC offset on a Mac, UTC (`+0000`) in a Windows trace record.
    #[serde(default)]
    pub timestamp: String,
    /// `processID`: the process that logged the record.
    #[serde(rename = "processID", default)]
    pub pid: i64,
    /// `processImagePath`: the program's path on a Mac; a bare program name (`chrome.exe`) in a
    /// Windows trace record, empty if the program could not be found.
    #[serde(rename = "processImagePath", default)]
    pub process_image_path: String,
    /// `subsystem`: the unified log subsystem (`com.apple.SkyLight`); `winevent` or the Event Log
    /// provider's name in a Windows trace record.
    #[serde(default)]
    pub subsystem: String,
    /// `category`: the log category within the subsystem; the `WinEvent`'s name or the Event Log
    /// event id in a Windows trace record.
    #[serde(default)]
    pub category: String,
    /// `messageType`: `Default`, `Info`, `Debug`, `Error` or `Fault`. A Windows trace record uses
    /// `Debug`, `Info`, `Error` and `Fault`.
    #[serde(rename = "messageType", default)]
    pub message_type: String,
    /// `eventMessage`: the message text, with `<private>` values redacted on a Mac.
    #[serde(rename = "eventMessage", default)]
    pub message: String,
}

impl OsLogRecord {
    /// Parses one line of an `oslog/` file.
    ///
    /// Returns `None` if the line does not start with `{` (such as the `Filtering the log data
    /// ...` header of `log stream`) or is not valid JSON, or if a key the record reads has the
    /// wrong type.
    ///
    /// # Example
    ///
    /// ```rust
    /// use komorebi_recorder::sources::oslog::OsLogRecord;
    ///
    /// assert!(OsLogRecord::parse(r#"{"processID":1,"eventMessage":"hello"}"#).is_some());
    /// assert!(OsLogRecord::parse(r#"Filtering the log data using "processID == 1""#).is_none());
    /// assert!(OsLogRecord::parse("").is_none());
    /// assert!(OsLogRecord::parse("{not json").is_none());
    /// ```
    #[must_use]
    pub fn parse(line: &str) -> Option<Self> {
        if !line.starts_with('{') {
            return None;
        }

        serde_json::from_str(line).ok()
    }

    /// Returns the record's time on the master clock, in nanoseconds: `hostNs` where the record has
    /// it, else `machTimestamp` converted with the timebase of the machine this runs on.
    ///
    /// The conversion uses this machine's timebase, which is only right for a record written on
    /// this machine. To read a session recorded elsewhere, use [`extract_time_ns`] with the
    /// session's own timebase (the manifest's `timebase`).
    #[must_use]
    pub fn mach_ns(&self) -> u64 {
        self.host_ns
            .unwrap_or_else(|| clock::timebase().ticks_to_ns(self.mach_ticks))
    }

    /// Returns the program's name: the last component of `processImagePath`, split at `/` or `\`.
    /// A path on a Mac and a bare name on Windows both give the program's file name.
    #[must_use]
    pub fn process_name(&self) -> &str {
        // a path on a Mac, a bare program name on Windows (or a path with backslashes)
        self.process_image_path
            .rsplit(['/', '\\'])
            .next()
            .unwrap_or(&self.process_image_path)
    }
}

/// Returns a raw line's time on the master clock in nanoseconds, without a full JSON parse.
///
/// This is its `hostNs` where it has one, else its `machTimestamp` tick count converted with
/// `timebase`, which is the session's (a session is read on whichever machine, so the reader's own
/// timebase would be wrong).
///
/// `None` if the line has neither key followed by digits.
///
/// # Example
///
/// ```rust
/// use komorebi_recorder::clock::Timebase;
/// use komorebi_recorder::sources::oslog::extract_time_ns;
///
/// // a Mac record: ticks, converted with the session's timebase (here 125/3, as on Apple silicon)
/// let timebase = Timebase { numer: 125, denom: 3 };
/// assert_eq!(extract_time_ns(br#"{"machTimestamp":300,"processID":1}"#, timebase), Some(12_500));
///
/// // a Windows record: already nanoseconds, whatever the timebase
/// assert_eq!(extract_time_ns(br#"{"hostNs":5000000000}"#, timebase), Some(5_000_000_000));
///
/// assert_eq!(extract_time_ns(br#"{"nothing":1}"#, timebase), None);
/// ```
#[must_use]
pub fn extract_time_ns(line: &[u8], timebase: clock::Timebase) -> Option<u64> {
    extract_digits(line, b"\"hostNs\":")
        .or_else(|| extract_mach_ticks(line).map(|ticks| timebase.ticks_to_ns(ticks)))
}

fn extract_digits(line: &[u8], key: &[u8]) -> Option<u64> {
    let start = line.windows(key.len()).position(|window| window == key)? + key.len();
    let digits: Vec<u8> = line[start..]
        .iter()
        .copied()
        .take_while(u8::is_ascii_digit)
        .collect();

    std::str::from_utf8(&digits).ok()?.parse().ok()
}

/// Cheaply pulls `"machTimestamp":<digits>` out of a raw line without a full JSON parse, and
/// returns the tick count. The first such key in the line is the one read.
///
/// # Example
///
/// ```rust
/// use komorebi_recorder::sources::oslog::extract_mach_ticks;
///
/// assert_eq!(
///     extract_mach_ticks(br#"{"machTimestamp":29042701633017,"processID":41666}"#),
///     Some(29_042_701_633_017)
/// );
/// assert_eq!(extract_mach_ticks(br#"{"nothing":1}"#), None);
/// ```
#[must_use]
pub fn extract_mach_ticks(line: &[u8]) -> Option<u64> {
    const KEY: &[u8] = b"\"machTimestamp\":";
    let start = line.windows(KEY.len()).position(|window| window == KEY)? + KEY.len();
    let digits: Vec<u8> = line[start..]
        .iter()
        .copied()
        .take_while(u8::is_ascii_digit)
        .collect();

    std::str::from_utf8(&digits).ok()?.parse().ok()
}

#[derive(Debug, Default)]
struct Counters {
    lines: AtomicU64,
    bytes: AtomicU64,
    /// `0` until the stream's header line has been seen.
    ready_mach_ns: AtomicU64,
    first_event_mach_ns: AtomicU64,
    last_event_mach_ns: AtomicU64,
}

/// The outcome of one stream, recorded in the session manifest (`session.json`, key `oslog`).
///
/// The stream's [`StreamSpec`] is flattened into it, so a manifest entry has the spec's keys
/// (`name`, `source`, `predicate`, `level`, `file`, `pid`) and then the ones below. Times are
/// nanoseconds on the host's master monotonic clock. The Windows OS trace reports its two files
/// the same way (see `wintrace`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StreamInfo {
    /// Which stream this is.
    #[serde(flatten)]
    pub spec: StreamSpec,
    /// When the `log stream` child was spawned (master clock).
    pub spawned_mach_ns: u64,
    /// When it announced it was filtering; events before this are not in the file. `None` if it
    /// never did.
    pub ready_mach_ns: Option<u64>,
    /// The `Filtering the log data using ...` line `log stream` printed. Only the final report
    /// has it; a checkpoint taken while recording does not.
    pub header: Option<String>,
    /// How many records are in the file.
    pub events: u64,
    /// How many bytes of records are in the file.
    pub bytes: u64,
    /// The time of the first record in the file, from its `machTimestamp`. `None` if there is
    /// none.
    pub first_event_mach_ns: Option<u64>,
    /// The time of the last record written so far, from its `machTimestamp`. `None` if there is
    /// none.
    pub last_event_mach_ns: Option<u64>,
    /// How many times the `log stream` child died and was brought back. Each restart leaves a
    /// gap in the file whose length is in the incident log.
    #[serde(default)]
    pub restarts: u32,
    /// Why the stream is not what was asked for, if it is not: the reason it was lost, how the
    /// child failed, or what it wrote to its standard error. `None` for a healthy stream.
    pub error: Option<String>,
}

/// A running stream's live counters, for the UI.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StreamLive {
    /// The stream's name ([`StreamSpec::name`]).
    pub name: String,
    /// Records written so far.
    pub events: u64,
    /// Bytes of records written so far.
    pub bytes: u64,
}

struct Running {
    spec: StreamSpec,
    child: Child,
    reader: Option<JoinHandle<Option<String>>>,
    stderr: Option<JoinHandle<String>>,
    counters: Arc<Counters>,
    spawned_mach_ns: u64,
    started: Instant,
    restarts: u32,
    /// Set once the stream has died for good: the reason, and no further restarts.
    lost: Option<String>,
}

/// How many times a stream that keeps dying is brought back before giving up on it.
const MAX_RESTARTS: u32 = 20;

/// A stream that dies sooner than this after starting has not been working at all (a bad
/// predicate, no access to the log), so restarting it would only loop.
const MIN_HEALTHY_RUNTIME: Duration = Duration::from_secs(2);

/// What a started `log stream` child consists of.
struct Launched {
    child: Child,
    reader: JoinHandle<Option<String>>,
    stderr: JoinHandle<String>,
    spawned_mach_ns: u64,
}

/// A set of concurrently running `log stream` children, each writing one stream's file.
///
/// The recorder owns one set per recording and drives it:
///
/// 1. [`spawn`](Self::spawn) starts a stream, and [`wait_ready`](Self::wait_ready) waits until every
///    stream is filtering, so that no event the recording cares about is missed at the start.
/// 2. [`reap`](Self::reap) is called regularly. It restarts a child that died, and reports the
///    streams that cannot be kept alive.
/// 3. [`snapshot`](Self::snapshot) and [`checkpoint`](Self::checkpoint) read the state without
///    stopping anything, for the live display and the manifest.
/// 4. [`stop`](Self::stop) ends every child and returns what each stream captured. Dropping a set
///    that still has children stops them, so abandoning a recording never leaves a `log stream`
///    behind.
///
/// Every method takes `&self`: the set is safe to use from more than one thread.
///
/// Only macOS can run the children, because they are `/usr/bin/log`; on any other host
/// [`spawn`](Self::spawn) returns an error. Each child runs in a process group of its own
/// ([`crate::platform::process::isolate`]), so a terminal Ctrl-C reaches only the recorder, which
/// then stops them in order instead of losing their last records.
///
/// # Example
///
/// Streams need the unified log, so this example only compiles. The behavior is covered by these
/// unit tests in this file, which skip themselves where `log stream` is not usable:
/// `a_log_stream_that_dies_is_restarted_into_the_same_file`,
/// `a_log_stream_that_never_worked_is_reported_lost_not_restarted_forever`,
/// `tee_keeps_records_and_splits_off_the_header` and `the_first_ready_time_survives_a_restart`.
///
/// ```rust,no_run
/// use komorebi_recorder::sources::oslog::{OsLogLevel, StreamSet, app_spec};
/// use std::path::Path;
/// use std::time::Duration;
///
/// let set = StreamSet::new(Path::new("session"));
/// set.spawn(app_spec("Slack", 1234, OsLogLevel::Debug))?;
///
/// // wait until `log stream` says it is filtering
/// let not_ready = set.wait_ready(Duration::from_secs(5));
/// assert!(not_ready.is_empty(), "not ready: {not_ready:?}");
///
/// // ... record, calling `set.reap()` now and then ...
///
/// for stream in set.stop() {
///     println!("{}: {} events in {}", stream.spec.name, stream.events, stream.spec.file);
/// }
/// # Ok::<(), Box<dyn std::error::Error>>(())
/// ```
pub struct StreamSet {
    session_dir: std::path::PathBuf,
    running: Mutex<Vec<Running>>,
}

impl StreamSet {
    /// Creates an empty set whose streams write under `session_dir`. Nothing is started.
    #[must_use]
    pub fn new(session_dir: &Path) -> Self {
        Self {
            session_dir: session_dir.to_path_buf(),
            running: Mutex::new(Vec::new()),
        }
    }

    /// Whether a stream with this name was already spawned.
    #[must_use]
    pub fn has(&self, name: &str) -> bool {
        self.running
            .lock()
            .iter()
            .any(|running| running.spec.name == name)
    }

    /// Whether a stream for this process id was already spawned.
    #[must_use]
    pub fn has_pid(&self, pid: i32) -> bool {
        self.running
            .lock()
            .iter()
            .any(|running| running.spec.pid == Some(pid))
    }

    /// Start `log stream` for `spec`, writing its records to the spec's file: truncating it for a
    /// new stream, appending for a restart so that nothing already captured is touched.
    fn launch(
        &self,
        spec: &StreamSpec,
        counters: &Arc<Counters>,
        append: bool,
    ) -> eyre::Result<Launched> {
        let path = self.session_dir.join(&spec.file);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)
                .wrap_err_with(|| format!("could not create {}", parent.display()))?;
        }

        let file = if append {
            std::fs::OpenOptions::new()
                .create(true)
                .append(true)
                .open(&path)
        } else {
            File::create(&path)
        }
        .wrap_err_with(|| format!("could not open {}", path.display()))?;

        let spawned_mach_ns = clock::mono_now_ns();
        let mut command = Command::new("/usr/bin/log");
        command
            .args([
                "stream",
                "--style",
                "ndjson",
                "--level",
                spec.level.as_arg(),
            ])
            .args(["--predicate", &spec.predicate])
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        // own process group, so a terminal Ctrl-C reaches only us and we can drain and stop these
        // children in order instead of losing their tail
        process::isolate(&mut command);
        let mut child = command.spawn().wrap_err("could not run /usr/bin/log")?;

        let stdout = child
            .stdout
            .take()
            .ok_or_else(|| eyre::Report::msg("log stream has no stdout"))?;
        let mut stderr = child
            .stderr
            .take()
            .ok_or_else(|| eyre::Report::msg("log stream has no stderr"))?;

        let reader_counters = counters.clone();
        let reader = std::thread::Builder::new()
            .name(format!("oslog-{}", spec.name))
            .spawn(move || tee_stream(stdout, file, &reader_counters))
            .wrap_err("could not start the log reader thread")?;

        let stderr_reader = std::thread::spawn(move || {
            let mut text = String::new();
            let _ = stderr.read_to_string(&mut text);
            text
        });

        Ok(Launched {
            child,
            reader,
            stderr: stderr_reader,
            spawned_mach_ns,
        })
    }

    /// Starts `log stream` for `spec` and adds it to the set.
    ///
    /// The stream's file (`spec.file`, under the session directory) is created, with its
    /// directories, and truncated if it exists. The call returns as soon as the child is started:
    /// that the predicate works and the stream is filtering is shown later, by
    /// [`wait_ready`](Self::wait_ready) and [`reap`](Self::reap). Spawning a name that is already
    /// in the set is not checked; use [`has`](Self::has) or [`has_pid`](Self::has_pid) first.
    ///
    /// # Errors
    ///
    /// Returns an error if
    ///
    /// - a directory for the file cannot be created, or the file cannot be opened,
    /// - `/usr/bin/log` cannot be run, which is the case on every host but macOS, or its output
    ///   pipes cannot be taken, or
    /// - the thread that copies its output cannot be started.
    pub fn spawn(&self, spec: StreamSpec) -> eyre::Result<()> {
        let counters = Arc::new(Counters::default());
        let launched = self.launch(&spec, &counters, false)?;

        self.running.lock().push(Running {
            spec,
            child: launched.child,
            reader: Some(launched.reader),
            stderr: Some(launched.stderr),
            counters,
            spawned_mach_ns: launched.spawned_mach_ns,
            started: Instant::now(),
            restarts: 0,
            lost: None,
        });

        Ok(())
    }

    /// Notices `log stream` children that have exited on their own (logd restarting, the machine
    /// waking from sleep) and brings them back, appending to the same file so nothing already
    /// captured is lost. Events during the gap cannot be recovered, which is reported.
    ///
    /// Returns an incident for each stream that needed attention, for the caller to record:
    ///
    /// - [`kind::LOG_STREAM_RESTARTED`], for a child
    ///   that had run for at least two seconds and was started again. It says how long before the
    ///   exit the last event was captured, when there was one.
    /// - [`kind::LOG_STREAM_LOST`] for a child that
    ///   exited within two seconds of starting, or of its last restart (it has not worked at all:
    ///   a bad predicate, no access to the log), that has already been restarted 20 times, or that
    ///   could not be started again. A lost stream is not tried again and not reported again, and
    ///   its reason is the [`StreamInfo::error`] of its reports.
    ///
    /// A healthy stream returns nothing.
    pub fn reap(&self) -> Vec<Draft> {
        let mut drafts = Vec::new();
        let mut running = self.running.lock();

        for stream in running.iter_mut().filter(|stream| stream.lost.is_none()) {
            let Ok(Some(status)) = stream.child.try_wait() else {
                continue;
            };

            let exited_at = clock::mono_now_ns();
            let _ = stream.reader.take().map(JoinHandle::join);
            let stderr = stream
                .stderr
                .take()
                .and_then(|reader| reader.join().ok())
                .unwrap_or_default();
            let stderr = stderr.trim();

            let healthy = stream.started.elapsed() >= MIN_HEALTHY_RUNTIME;
            if !healthy || stream.restarts >= MAX_RESTARTS {
                let reason = if healthy {
                    format!(
                        "log stream kept dying (restarted {} times)",
                        stream.restarts
                    )
                } else {
                    format!("log stream exited immediately with {status}")
                };
                let reason = if stderr.is_empty() {
                    reason
                } else {
                    format!("{reason}: {stderr}")
                };

                drafts.push(Draft::new(
                    kind::LOG_STREAM_LOST,
                    format!("OS trace stream '{}': {reason}", stream.spec.name),
                ));
                stream.lost = Some(reason);
                continue;
            }

            let counters = stream.counters.clone();
            match self.launch(&stream.spec, &counters, true) {
                Ok(launched) => {
                    stream.child = launched.child;
                    stream.reader = Some(launched.reader);
                    stream.stderr = Some(launched.stderr);
                    stream.started = Instant::now();
                    stream.restarts += 1;

                    let last_event = counters.last_event_mach_ns.load(Ordering::Relaxed);
                    let gap = if last_event == 0 {
                        String::new()
                    } else {
                        format!(
                            "; events between {:.1}s before this and the restart are missing",
                            exited_at.saturating_sub(last_event) as f64 / 1e9
                        )
                    };
                    drafts.push(Draft::new(
                        kind::LOG_STREAM_RESTARTED,
                        format!(
                            "OS trace stream '{}' exited ({status}) and was restarted (restart {}){gap}",
                            stream.spec.name, stream.restarts
                        ),
                    ));
                }
                Err(error) => {
                    let reason = format!("could not restart log stream: {error:#}");
                    drafts.push(Draft::new(
                        kind::LOG_STREAM_LOST,
                        format!("OS trace stream '{}': {reason}", stream.spec.name),
                    ));
                    stream.lost = Some(reason);
                }
            }
        }

        drafts
    }

    /// Blocks until every stream has announced that it is filtering, or `timeout` passes. Returns
    /// the names of streams that were not ready in time; the vector is empty when all were.
    ///
    /// A stream is ready once `log stream` has printed its header line. A stream that dies before
    /// it is filtering never becomes ready, so it is named when the time is up.
    pub fn wait_ready(&self, timeout: Duration) -> Vec<String> {
        let deadline = Instant::now() + timeout;
        loop {
            let pending: Vec<String> = self
                .running
                .lock()
                .iter()
                .filter(|running| running.counters.ready_mach_ns.load(Ordering::Relaxed) == 0)
                .map(|running| running.spec.name.clone())
                .collect();

            if pending.is_empty() || Instant::now() >= deadline {
                return pending;
            }

            std::thread::sleep(Duration::from_millis(10));
        }
    }

    /// Returns each running stream's live counters, in the order they were spawned.
    #[must_use]
    pub fn snapshot(&self) -> Vec<StreamLive> {
        self.running
            .lock()
            .iter()
            .map(|running| StreamLive {
                name: running.spec.name.clone(),
                events: running.counters.lines.load(Ordering::Relaxed),
                bytes: running.counters.bytes.load(Ordering::Relaxed),
            })
            .collect()
    }

    fn info(stream: &Running, header: Option<String>, error: Option<String>) -> StreamInfo {
        let counters = &stream.counters;
        let nonzero = |value: u64| (value != 0).then_some(value);

        StreamInfo {
            spec: stream.spec.clone(),
            spawned_mach_ns: stream.spawned_mach_ns,
            ready_mach_ns: nonzero(counters.ready_mach_ns.load(Ordering::Relaxed)),
            header,
            events: counters.lines.load(Ordering::Relaxed),
            bytes: counters.bytes.load(Ordering::Relaxed),
            first_event_mach_ns: nonzero(counters.first_event_mach_ns.load(Ordering::Relaxed)),
            last_event_mach_ns: nonzero(counters.last_event_mach_ns.load(Ordering::Relaxed)),
            restarts: stream.restarts,
            error: error.or_else(|| stream.lost.clone()),
        }
    }

    /// Returns every stream's state so far, without stopping anything (for manifest checkpoints).
    /// The `header` of each is `None` and its `error` is set only for a stream that was lost.
    #[must_use]
    pub fn checkpoint(&self) -> Vec<StreamInfo> {
        self.running
            .lock()
            .iter()
            .map(|stream| Self::info(stream, None, None))
            .collect()
    }

    /// Terminates every child, drains what they had buffered, and reports each stream's outcome.
    ///
    /// Each child is sent `SIGTERM` and given three seconds to exit before it is killed. The
    /// [`StreamInfo::error`] of a stream is set if its child failed for a reason other than being
    /// told to stop, or wrote to its standard error. The set is empty afterwards, so a second call
    /// returns nothing.
    pub fn stop(&self) -> Vec<StreamInfo> {
        let mut running = std::mem::take(&mut *self.running.lock());

        for stream in &mut running {
            process::terminate(&mut stream.child);
        }

        running
            .iter_mut()
            .map(|stream| {
                let deadline = Instant::now() + Duration::from_secs(3);
                let status = loop {
                    match stream.child.try_wait() {
                        Ok(Some(status)) => break Some(status),
                        Ok(None) if Instant::now() < deadline => {
                            std::thread::sleep(Duration::from_millis(10));
                        }
                        _ => {
                            let _ = stream.child.kill();
                            break stream.child.wait().ok();
                        }
                    }
                };

                let header = stream
                    .reader
                    .take()
                    .and_then(|reader| reader.join().ok().flatten());
                let stderr = stream
                    .stderr
                    .take()
                    .and_then(|reader| reader.join().ok())
                    .unwrap_or_default();

                // SIGTERM is how we stop it, so only other failures are interesting
                let mut error = None;
                if stream.lost.is_none()
                    && let Some(status) = status
                    && !status.success()
                    && !process::was_terminated(&status)
                {
                    error = Some(format!("log stream exited with {status}"));
                }

                let stderr = stderr.trim();
                if stream.lost.is_none() && !stderr.is_empty() {
                    error = Some(match error {
                        Some(existing) => format!("{existing}: {stderr}"),
                        None => stderr.to_string(),
                    });
                }

                Self::info(stream, header, error)
            })
            .collect()
    }
}

impl Drop for StreamSet {
    fn drop(&mut self) {
        // never leave `log stream` children behind if recording is abandoned
        if !self.running.lock().is_empty() {
            let _ = self.stop();
        }
    }
}

/// Copy `log stream`'s stdout into `file`, keeping only ndjson records in the file, and return the
/// non-JSON "Filtering the log data using ..." header if one was printed.
fn tee_stream(stdout: impl Read, file: File, counters: &Counters) -> Option<String> {
    let mut reader = BufReader::with_capacity(64 * 1024, stdout);
    let mut writer = BufWriter::with_capacity(64 * 1024, file);
    let mut line = Vec::with_capacity(4096);
    let mut header = None;

    loop {
        line.clear();
        match reader.read_until(b'\n', &mut line) {
            Ok(0) | Err(_) => break,
            Ok(_) => {}
        }

        if line.first() == Some(&b'{') {
            if writer.write_all(&line).is_err() {
                break;
            }

            counters.lines.fetch_add(1, Ordering::Relaxed);
            counters
                .bytes
                .fetch_add(line.len() as u64, Ordering::Relaxed);

            if let Some(ticks) = extract_mach_ticks(&line) {
                let ns = clock::timebase().ticks_to_ns(ticks);
                let _ = counters.first_event_mach_ns.compare_exchange(
                    0,
                    ns,
                    Ordering::Relaxed,
                    Ordering::Relaxed,
                );
                counters.last_event_mach_ns.store(ns, Ordering::Relaxed);
            }
        } else if header.is_none() {
            header = Some(String::from_utf8_lossy(&line).trim().to_string());
            // only the first: later headers are restarts, and the manifest wants when events
            // started being captured
            let _ = counters.ready_mach_ns.compare_exchange(
                0,
                clock::mono_now_ns(),
                Ordering::Relaxed,
                Ordering::Relaxed,
            );
        }

        // flush whenever we have caught up with the pipe so a crash loses almost nothing
        if reader.buffer().is_empty() {
            let _ = writer.flush();
        }
    }

    let _ = writer.flush();
    header
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The shape `log stream --style ndjson` emits, captured from a real run (trimmed).
    const FIXTURE: &str = r#"{"timezoneName":"","messageType":"Debug","eventType":"logEvent","source":null,"formatString":"found no value for key %{public}@","userID":501,"activityIdentifier":0,"subsystem":"com.apple.defaults","category":"User Defaults","threadID":132217281,"senderImageUUID":"ADB84143-D965-3B3D-9BA5-EAB165A96490","backtrace":{"frames":[{"imageOffset":2047056,"imageUUID":"ADB84143"}]},"bootUUID":"","processImagePath":"\/Users\/lgug2z\/.cargo\/bin\/komorebi","timestamp":"2026-10-26 12:48:36.736892-0700","senderImagePath":"\/System\/Library\/Frameworks\/CoreFoundation","machTimestamp":29042701633017,"eventMessage":"found no value for key NSPersistentUIShowQuietSafeQuitStatus","processImageUUID":"X","traceID":40312880901652996,"processID":41666,"senderProgramCounter":2047056,"parentActivityIdentifier":0}"#;

    #[test]
    fn parses_a_real_record() {
        let record = OsLogRecord::parse(FIXTURE).expect("record");
        assert_eq!(record.mach_ticks, 29_042_701_633_017);
        assert_eq!(record.pid, 41666);
        assert_eq!(record.subsystem, "com.apple.defaults");
        assert_eq!(record.category, "User Defaults");
        assert_eq!(record.process_name(), "komorebi");
        assert!(record.message.starts_with("found no value"));
        assert_eq!(record.timestamp, "2026-10-26 12:48:36.736892-0700");
    }

    #[test]
    fn non_json_lines_are_not_records() {
        assert!(OsLogRecord::parse("Filtering the log data using \"processID == 1\"").is_none());
        assert!(OsLogRecord::parse("").is_none());
        assert!(OsLogRecord::parse("{not json").is_none());
    }

    #[test]
    fn extracts_mach_ticks_without_a_full_parse() {
        assert_eq!(
            extract_mach_ticks(FIXTURE.as_bytes()),
            Some(29_042_701_633_017)
        );
        assert_eq!(extract_mach_ticks(b"{\"nothing\":1}"), None);
    }

    #[test]
    fn builds_predicates() {
        assert_eq!(pid_predicate(42), "processID == 42");
        let system = system_predicate(&[]);
        for subsystem in SYSTEM_SUBSYSTEMS {
            assert!(system.contains(subsystem));
        }
        assert!(system.contains("subsystem BEGINSWITH \"com.apple.accessibility\""));
        assert!(!system.contains("process =="));

        let with_daemons =
            system_predicate(&["WindowServer".to_string(), "weird\"name".to_string()]);
        assert!(with_daemons.contains(" OR process == \"WindowServer\""));
        assert!(with_daemons.contains("weird\\\"name"));
    }

    #[test]
    fn sanitizes_app_names_for_files() {
        assert_eq!(sanitize_file_name("Brave Beta"), "Brave_Beta");
        assert_eq!(sanitize_file_name("IntelliJ IDEA"), "IntelliJ_IDEA");
        assert_eq!(sanitize_file_name("../../etc/passwd"), "etc_passwd");
        assert_eq!(sanitize_file_name("///"), "app");
        assert_eq!(sanitize_file_name(""), "app");
    }

    #[test]
    fn app_specs_are_labelled_by_exe_and_pid() {
        let spec = app_spec("Brave Beta", 1234, OsLogLevel::Debug);
        assert_eq!(spec.name, "app:Brave Beta-1234");
        assert_eq!(spec.source, "oslog:app:Brave Beta");
        assert_eq!(spec.file, "oslog/apps/Brave_Beta-1234.ndjson");
        assert_eq!(spec.predicate, "processID == 1234");
    }

    #[test]
    fn tee_keeps_records_and_splits_off_the_header() {
        let dir =
            std::env::temp_dir().join(format!("komorebi-recorder-tee-{}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("dir");
        let path = dir.join("out.ndjson");

        let input = format!("Filtering the log data using \"x\"\n{FIXTURE}\n{FIXTURE}\n");
        let counters = Counters::default();
        let header = tee_stream(
            input.as_bytes(),
            File::create(&path).expect("file"),
            &counters,
        );

        assert_eq!(
            header.as_deref(),
            Some("Filtering the log data using \"x\"")
        );
        assert_eq!(counters.lines.load(Ordering::Relaxed), 2);
        assert_ne!(counters.ready_mach_ns.load(Ordering::Relaxed), 0);
        assert_ne!(counters.first_event_mach_ns.load(Ordering::Relaxed), 0);

        let written = std::fs::read_to_string(&path).expect("read");
        assert_eq!(written.lines().count(), 2);
        assert!(written.lines().all(|line| line.starts_with('{')));
    }

    fn temp(name: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "komorebi-recorder-oslog-{name}-{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("oslog/apps")).expect("dir");
        dir
    }

    /// A real `log stream` on launchd that has become ready, or `None` where the unified log is
    /// not readable (a sandboxed build), in which case there is nothing to test.
    #[cfg(target_os = "macos")]
    fn ready_stream(dir: &Path) -> Option<StreamSet> {
        let set = StreamSet::new(dir);
        let usable = set.spawn(app_spec("launchd", 1, OsLogLevel::Debug)).is_ok()
            && set.wait_ready(Duration::from_secs(5)).is_empty();
        if !usable {
            eprintln!("skipping: `log stream` is not usable from this environment");
            let _ = set.stop();
            return None;
        }

        Some(set)
    }

    #[cfg(target_os = "macos")]
    fn child_pid(set: &StreamSet) -> i32 {
        set.running.lock()[0].child.id() as i32
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn a_log_stream_that_dies_is_restarted_into_the_same_file() {
        let dir = temp("restart");
        let Some(set) = ready_stream(&dir) else {
            return;
        };

        // only a stream that had been healthy for a while is worth bringing back
        std::thread::sleep(MIN_HEALTHY_RUNTIME + Duration::from_millis(300));
        let file = dir.join("oslog/apps/launchd-1.ndjson");
        std::fs::OpenOptions::new()
            .append(true)
            .open(&file)
            .and_then(|mut file| {
                std::io::Write::write_all(&mut file, b"{\"marker\":\"before the restart\"}\n")
            })
            .expect("marker");

        // logd restarts, or the machine wakes: the child is gone
        // SAFETY: signalling a child process this test spawned
        unsafe { libc::kill(child_pid(&set), libc::SIGKILL) };
        std::thread::sleep(Duration::from_millis(300));

        let drafts = set.reap();
        assert_eq!(drafts.len(), 1, "{drafts:?}");
        assert_eq!(drafts[0].kind, kind::LOG_STREAM_RESTARTED);
        assert!(
            drafts[0].detail.contains("restarted"),
            "{}",
            drafts[0].detail
        );

        // it is running again, and what was captured before is untouched
        assert!(set.wait_ready(Duration::from_secs(5)).is_empty());
        assert_eq!(set.checkpoint()[0].restarts, 1);
        assert!(set.reap().is_empty(), "the replacement is healthy");

        let infos = set.stop();
        assert_eq!(infos[0].restarts, 1);
        assert!(infos[0].error.is_none(), "{:?}", infos[0].error);
        let text = std::fs::read_to_string(&file).expect("read");
        assert!(
            text.contains("before the restart"),
            "the restart must append, not truncate"
        );
    }

    #[test]
    fn a_log_stream_that_never_worked_is_reported_lost_not_restarted_forever() {
        let dir = temp("lost");
        let set = StreamSet::new(&dir);
        let spec = StreamSpec {
            name: "bad".to_string(),
            source: "oslog:bad".to_string(),
            predicate: "this is not ( a predicate".to_string(),
            level: OsLogLevel::Debug,
            file: "oslog/bad.ndjson".to_string(),
            pid: None,
        };

        if set.spawn(spec).is_err() {
            eprintln!("skipping: /usr/bin/log cannot be run from this environment");
            return;
        }

        // an invalid predicate makes `log stream` exit straight away
        std::thread::sleep(Duration::from_millis(800));

        let drafts = set.reap();
        assert_eq!(drafts.len(), 1, "{drafts:?}");
        assert_eq!(drafts[0].kind, kind::LOG_STREAM_LOST);
        assert!(
            drafts[0].detail.contains("exited immediately"),
            "{}",
            drafts[0].detail
        );

        // and it is not tried again, nor reported again
        assert!(set.reap().is_empty());
        let infos = set.stop();
        assert_eq!(infos[0].restarts, 0);
        assert!(
            infos[0]
                .error
                .as_deref()
                .is_some_and(|error| error.contains("exited immediately"))
        );
    }

    #[test]
    fn the_first_ready_time_survives_a_restart() {
        let counters = Counters::default();
        let _ =
            counters
                .ready_mach_ns
                .compare_exchange(0, 111, Ordering::Relaxed, Ordering::Relaxed);

        // a restarted stream's header must not overwrite when events first started being captured
        let input = "Filtering the log data using \"x\"\n";
        let dir = temp("ready");
        tee_stream(
            input.as_bytes(),
            File::create(dir.join("a.ndjson")).expect("file"),
            &counters,
        );
        assert_eq!(counters.ready_mach_ns.load(Ordering::Relaxed), 111);
    }
}
