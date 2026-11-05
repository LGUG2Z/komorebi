//! The OS trace on Windows.
//!
//! A Mac has a unified log to ask what the system did. Windows has no one place, so two things are
//! recorded into `oslog/` next to komorebi's own output, in the same shape the Mac's records have
//! so that the timeline, `search`, `at` and `trace --os` read them unchanged:
//!
//! - **what the OS told komorebi**: the `WinEvents` komorebi itself listens for (a window is shown,
//!   hidden, cloaked, focused, moved, resized, renamed, destroyed), tapped with a hook of the
//!   recorder's own, for every top-level window and not only the ones komorebi manages. komorebi
//!   hears these from the same hook and filters what it acts on; seeing the unfiltered stream is
//!   what answers "did Windows tell it?". `oslog/system.ndjson`, source `oslog:system`.
//! - **what Windows logged**: the System Event Log's warnings and errors, and the providers that
//!   talk about displays, power and the GPU (a display driver reset, a Kernel-Power event, a
//!   plug and play arrival). `oslog/eventlog.ndjson`, source `oslog:eventlog`.
//!
//! This module is the part that does not need Windows: the record format, deciding which noisy
//! events to fold together, reading an Event Log record's XML, and the files and counters. It
//! builds on every host, so a Windows session can be read anywhere. The taps themselves are in
//! [`crate::platform::trace`]; on a host other than Windows they start nothing.
//!
//! # Files written
//!
//! Both files are NDJSON: one JSON object per line. Each line has the keys of a Mac unified-log
//! record (see [`OsLogRecord`](crate::sources::oslog::OsLogRecord)), so every reader of the Mac's
//! trace reads these unchanged, with `hostNs` where the Mac has a tick count:
//!
//! | key | contents |
//! |---|---|
//! | `hostNs` | the time on the master clock, in nanoseconds (the master clock is `QueryPerformanceCounter` on Windows) |
//! | `timestamp` | the wall-clock time as `2026-10-27 12:34:56.123456+0000`, in UTC |
//! | `processID` | the process of the window or of the logger, `0` if unknown |
//! | `processImagePath` | the program's file name with `.exe` (`chrome.exe`), empty if it could not be found |
//! | `subsystem` | `winevent` in `system.ndjson`; the Event Log provider's name in `eventlog.ndjson` |
//! | `category` | the `WinEvent`'s name (`ObjectShow`), or the Event Log event id (`506`) |
//! | `messageType` | `Debug`, `Info`, `Error` or `Fault` |
//! | `eventMessage` | `<name> hwnd=0x<handle> class=<window class>` for a `WinEvent`; `<provider> event <id>: <data>` for an Event Log record |
//!
//! The manifest lists the two as streams named `winevents` and `eventlog` ([`TraceSet`]).
//!
//! # What is recorded
//!
//! - **`WinEvents`**: only events about a window itself (not the objects inside it), for top-level
//!   windows. Events about a child window are dropped; those of a window that no longer exists
//!   by the time it is looked at are kept, which keeps its destruction. Events of the recorder's
//!   own windows are not heard. The events of a window that arrive in a burst and say the same
//!   thing each time (a state, location, name, description or value change) are folded into one
//!   record per 100 ms ([`Coalescer`]); those records are `Debug` level and every other event is
//!   `Info`.
//! - **Event Log**: from the System log, the warnings and errors (levels 1 to 3) of any provider,
//!   and the `Microsoft-Windows-Kernel-Power`, `Microsoft-Windows-Kernel-PnP`,
//!   `Microsoft-Windows-DxgKrnl`, `Display` and `Microsoft-Windows-Dwm-Core` providers at the
//!   other levels. Only events logged after the recording started are heard. Windows' warning
//!   level is written as `Error`, because the Mac's `messageType` has no warning.
//!
//! # What is not guaranteed
//!
//! - A burst's folded count is written on the next record for the same window and event. If none
//!   follows, the events folded since the last record are counted nowhere in the file.
//! - The recorder does not list the windows that exist when it starts, so such a window appears
//!   in `system.ndjson` only when something happens to it.
//! - The Event Log XML is read by searching its text, not parsed: entities such as `&amp;` stay
//!   as they are in the record.

use crate::clock;
use crate::incidents::Draft;
use crate::sources::oslog::OsLogLevel;
use crate::sources::oslog::StreamInfo;
use crate::sources::oslog::StreamLive;
use crate::sources::oslog::StreamSpec;
use chrono::DateTime;
use chrono::Utc;
use color_eyre::eyre;
use parking_lot::Mutex;
use std::collections::HashMap;
use std::fs::File;
use std::io::BufWriter;
use std::io::Write;
use std::path::Path;
use std::sync::Arc;
use std::sync::atomic::AtomicU64;
use std::sync::atomic::Ordering;

/// The `WinEvent` stream's file, relative to the session directory. It has the same name as the
/// `system` stream of the Mac's trace, and the same `oslog:system` source label.
pub const SYSTEM_FILE: &str = "oslog/system.ndjson";
/// The Event Log stream's file, relative to the session directory.
pub const EVENT_LOG_FILE: &str = "oslog/eventlog.ndjson";

/// How long a window's repeated noisy events are folded into one.
const COALESCE_NS: u64 = 100_000_000;

/// How many windows' last events are remembered before the oldest are forgotten.
const COALESCE_CAPACITY: usize = 4096;

/// `EVENT_*` from winuser.h and oleacc.h, for the events komorebi and the shell care about. Any
/// other is named by its number.
const WIN_EVENTS: &[(u32, &str)] = &[
    (0x0001, "SystemSound"),
    (0x0002, "SystemAlert"),
    (0x0003, "SystemForeground"),
    (0x0004, "SystemMenuStart"),
    (0x0005, "SystemMenuEnd"),
    (0x0006, "SystemMenuPopupStart"),
    (0x0007, "SystemMenuPopupEnd"),
    (0x0008, "SystemCaptureStart"),
    (0x0009, "SystemCaptureEnd"),
    (0x000A, "SystemMoveSizeStart"),
    (0x000B, "SystemMoveSizeEnd"),
    (0x000C, "SystemContextHelpStart"),
    (0x000D, "SystemContextHelpEnd"),
    (0x000E, "SystemDragDropStart"),
    (0x000F, "SystemDragDropEnd"),
    (0x0010, "SystemDialogStart"),
    (0x0011, "SystemDialogEnd"),
    (0x0012, "SystemScrollingStart"),
    (0x0013, "SystemScrollingEnd"),
    (0x0014, "SystemSwitchStart"),
    (0x0015, "SystemSwitchEnd"),
    (0x0016, "SystemMinimizeStart"),
    (0x0017, "SystemMinimizeEnd"),
    (0x8000, "ObjectCreate"),
    (0x8001, "ObjectDestroy"),
    (0x8002, "ObjectShow"),
    (0x8003, "ObjectHide"),
    (0x8004, "ObjectReorder"),
    (0x8005, "ObjectFocus"),
    (0x8006, "ObjectSelection"),
    (0x800A, "ObjectStateChange"),
    (0x800B, "ObjectLocationChange"),
    (0x800C, "ObjectNameChange"),
    (0x800D, "ObjectDescriptionChange"),
    (0x800E, "ObjectValueChange"),
    (0x800F, "ObjectParentChange"),
    (0x8017, "ObjectCloaked"),
    (0x8018, "ObjectUncloaked"),
];

/// Returns the name of a `WinEvent`: the `EVENT_*` constant of winuser.h and oleacc.h without its
/// prefix (`SystemForeground`, `ObjectShow`).
///
/// Names are known for the events komorebi and the shell care about. Any other is named by its
/// number, `Event0x` and four hexadecimal digits.
///
/// # Example
///
/// ```rust
/// use komorebi_recorder::sources::wintrace::win_event_name;
///
/// assert_eq!(win_event_name(0x0003), "SystemForeground");
/// assert_eq!(win_event_name(0x8017), "ObjectCloaked");
/// assert_eq!(win_event_name(0x7777), "Event0x7777");
/// ```
#[must_use]
pub fn win_event_name(code: u32) -> String {
    WIN_EVENTS
        .iter()
        .find(|(number, _)| *number == code)
        .map_or_else(
            || format!("Event0x{code:04X}"),
            |(_, name)| (*name).to_string(),
        )
}

/// Returns `true` if and only if the `WinEvent` arrives in bursts and says the same thing each
/// time.
///
/// These are the object state, location, name, description and value changes (`0x800A` to
/// `0x800E`), which a window that is being dragged, resized or redrawn sends in bursts. A
/// [`Coalescer`] folds them, and the records written for them are `Debug` level.
///
/// # Example
///
/// ```rust
/// use komorebi_recorder::sources::wintrace::{is_noisy, win_event_name};
///
/// assert!(is_noisy(0x800B), "{}", win_event_name(0x800B)); // ObjectLocationChange
/// assert!(!is_noisy(0x8002), "{}", win_event_name(0x8002)); // ObjectShow
/// ```
#[must_use]
pub fn is_noisy(code: u32) -> bool {
    // state, location, name, description and value changes
    matches!(code, 0x800A..=0x800E)
}

/// What to do with one more of a window's events, as [`Coalescer::offer`] decides.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Offer {
    /// Record it, saying how many like it were folded into it since the last one recorded.
    Emit {
        /// How many events of the same window and kind were folded since the last one recorded.
        /// `0` for an event that was not part of a burst.
        folded: u32,
    },
    /// It is one of a burst: counted, not recorded.
    Fold,
}

/// Folds a window's repeated noisy events into one record per 100 ms.
///
/// This keeps dragging a window from writing a record per pixel. An event that is not noisy
/// ([`is_noisy`]) is never folded: a window shown, hidden, focused or destroyed is always its own
/// record.
///
/// A window and a kind of event are folded separately: a burst of location changes of one window
/// does not fold its name changes, or another window's location changes. The memory is bounded: it
/// holds at most 4096 pairs of a window and a kind, and when that fills it forgets the pairs not
/// heard from in the last second, and all of them if that is not enough.
///
/// # Example
///
/// ```rust
/// use komorebi_recorder::sources::wintrace::{Coalescer, Offer};
///
/// let mut coalescer = Coalescer::default();
/// let (window, moved) = (0x1234, 0x800B); // ObjectLocationChange
///
/// // the first of a burst is recorded, the rest within 100 ms are folded
/// assert_eq!(coalescer.offer(1_000_000_000, window, moved), Offer::Emit { folded: 0 });
/// for step in 1..=5 {
///     assert_eq!(coalescer.offer(1_000_000_000 + step * 10_000_000, window, moved), Offer::Fold);
/// }
///
/// // the next one after the burst is recorded, and says how many were folded before it
/// assert_eq!(coalescer.offer(1_300_000_000, window, moved), Offer::Emit { folded: 5 });
///
/// // a window shown (0x8002) is never folded, however fast
/// assert_eq!(coalescer.offer(1_300_000_001, window, 0x8002), Offer::Emit { folded: 0 });
/// assert_eq!(coalescer.offer(1_300_000_002, window, 0x8002), Offer::Emit { folded: 0 });
/// ```
#[derive(Debug, Default)]
pub struct Coalescer {
    last: HashMap<(isize, u32), (u64, u32)>,
}

impl Coalescer {
    /// Decides what to do with an event of `window` of kind `code` (a `WinEvent` number) heard at
    /// `now_ns`, on the master clock.
    ///
    /// A noisy event ([`is_noisy`]) is folded if the last one of the same window and kind that was
    /// recorded was less than 100 ms before `now_ns`, and otherwise recorded with the number
    /// folded since. Any other event is recorded with `0` folded.
    pub fn offer(&mut self, now_ns: u64, window: isize, code: u32) -> Offer {
        if !is_noisy(code) {
            return Offer::Emit { folded: 0 };
        }

        if self.last.len() >= COALESCE_CAPACITY {
            // a long-running recording has seen many windows come and go
            self.last
                .retain(|_, (at, _)| now_ns.saturating_sub(*at) < 10 * COALESCE_NS);
            if self.last.len() >= COALESCE_CAPACITY {
                self.last.clear();
            }
        }

        let entry = self.last.entry((window, code)).or_insert((0, 0));
        if entry.0 != 0 && now_ns.saturating_sub(entry.0) < COALESCE_NS {
            entry.1 += 1;
            return Offer::Fold;
        }

        let folded = std::mem::take(&mut entry.1);
        entry.0 = now_ns;
        Offer::Emit { folded }
    }
}

/// One record of the OS trace, as it is written: [`to_line`](TraceRecord::to_line) gives its line
/// of the NDJSON file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TraceRecord {
    /// The time on the master clock, in nanoseconds.
    pub host_ns: u64,
    /// The wall-clock time, in microseconds since the Unix epoch.
    pub wall_us: i64,
    /// The process of the window (`WinEvent`) or of the logger (Event Log); `0` if unknown.
    pub pid: i64,
    /// The program, e.g. `chrome.exe`; empty if it could not be found.
    pub process: String,
    /// `winevent` or the Event Log provider.
    pub subsystem: String,
    /// The `WinEvent`'s name, or the Event Log event id.
    pub category: String,
    /// `Debug`, `Info`, `Error`, `Fault`: the names the Mac's records use.
    pub level: &'static str,
    /// The text of the record: `<name> hwnd=0x<handle> class=<class>` for a `WinEvent`, or the
    /// provider, event id and data for an Event Log record.
    pub message: String,
}

/// `2026-10-27 12:34:56.123456+0000`: the shape the Mac's records carry, which is what the readers
/// pick the time of day out of.
fn timestamp(wall_us: i64) -> String {
    DateTime::<Utc>::from_timestamp_micros(wall_us)
        .map(|time| time.format("%Y-%m-%d %H:%M:%S%.6f%z").to_string())
        .unwrap_or_default()
}

impl TraceRecord {
    /// Returns the record as a line of the NDJSON file, without a line terminator. The field names
    /// are the Mac's unified log's, so the readers of that need no second path, with `hostNs`
    /// where the Mac has a tick count: the time is on the master clock already, whichever OS wrote
    /// it.
    ///
    /// The keys are `hostNs`, `timestamp` (from `wall_us`, as `2026-10-26 12:00:00.123456+0000`),
    /// `processID`, `processImagePath`, `subsystem`, `category`, `messageType` and `eventMessage`.
    /// A line reads back as an [`OsLogRecord`](crate::sources::oslog::OsLogRecord).
    ///
    /// # Example
    ///
    /// ```rust
    /// use komorebi_recorder::sources::oslog::OsLogRecord;
    /// use komorebi_recorder::sources::wintrace::TraceRecord;
    ///
    /// let record = TraceRecord {
    ///     host_ns: 5_000_000_000,
    ///     wall_us: 1_793_016_000_123_456,
    ///     pid: 4242,
    ///     process: "chrome.exe".to_string(),
    ///     subsystem: "winevent".to_string(),
    ///     category: "ObjectShow".to_string(),
    ///     level: "Info",
    ///     message: "ObjectShow hwnd=0x1f2a class=Chrome_WidgetWin_1".to_string(),
    /// };
    ///
    /// assert_eq!(
    ///     record.to_line(),
    ///     r#"{"category":"ObjectShow","eventMessage":"ObjectShow hwnd=0x1f2a class=Chrome_WidgetWin_1","hostNs":5000000000,"messageType":"Info","processID":4242,"processImagePath":"chrome.exe","subsystem":"winevent","timestamp":"2026-10-26 12:00:00.123456+0000"}"#
    /// );
    ///
    /// // the readers of the Mac's trace read it
    /// let read = OsLogRecord::parse(&record.to_line()).ok_or("not a record")?;
    /// assert_eq!((read.pid, read.host_ns), (4242, Some(5_000_000_000)));
    /// # Ok::<(), Box<dyn std::error::Error>>(())
    /// ```
    #[must_use]
    pub fn to_line(&self) -> String {
        serde_json::json!({
            "hostNs": self.host_ns,
            "timestamp": timestamp(self.wall_us),
            "processID": self.pid,
            "processImagePath": self.process,
            "subsystem": self.subsystem,
            "category": self.category,
            "messageType": self.level,
            "eventMessage": self.message,
        })
        .to_string()
    }
}

/// What an Event Log record says, from its XML ([`summarize_event_xml`]).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EventLogSummary {
    /// The provider's name (`Microsoft-Windows-Kernel-Power`).
    pub provider: String,
    /// The event id, as the text of the record's `EventID` element (`506`).
    pub event_id: String,
    /// `Fault` for Windows level 1 (critical), `Error` for 2 (error) and 3 (warning), `Info` for
    /// every other level.
    pub level: &'static str,
    /// The process that logged it, where the record says.
    pub pid: Option<i64>,
    /// The record's data, `name=value` where it is named.
    pub data: Vec<String>,
}

/// The text between `<tag ...>` and `</tag>`, the first time it appears.
fn element<'a>(xml: &'a str, tag: &str) -> Option<&'a str> {
    let open = xml.find(&format!("<{tag}"))?;
    let start = xml[open..].find('>')? + open + 1;
    let end = xml[start..].find(&format!("</{tag}>"))? + start;
    Some(&xml[start..end])
}

/// An attribute's value, in single or double quotes.
fn attribute<'a>(xml: &'a str, name: &str) -> Option<&'a str> {
    for quote in ['\'', '"'] {
        let key = format!("{name}={quote}");
        if let Some(at) = xml.find(&key) {
            let start = at + key.len();
            let end = xml[start..].find(quote)? + start;
            return Some(&xml[start..end]);
        }
    }

    None
}

/// Reads what matters out of an Event Log record's XML without an XML parser: the provider, the
/// event id, the level, and the data it carries.
///
/// The text is searched, not parsed, so it is right for the XML Windows renders for an event
/// (attributes in single or double quotes) and does not decode entities. The data is each `Data`
/// element's trimmed text, as `name=value` when the element has a `Name` attribute; an empty one
/// is left out. The process is the `ProcessID` of the `Execution` element.
///
/// Returns `None` if the text has no `Provider` with a `Name`, or no `EventID`.
///
/// # Example
///
/// ```rust
/// use komorebi_recorder::sources::wintrace::summarize_event_xml;
///
/// let xml = "<Event xmlns='http://schemas.microsoft.com/win/2004/08/events/event'>\
///     <System><Provider Name='Microsoft-Windows-Kernel-Power' Guid='{331C3B3A-2005-44C2-AC5E-77220C37D6B4}'/>\
///     <EventID>506</EventID><Level>4</Level><Execution ProcessID='4' ThreadID='8'/></System>\
///     <EventData><Data Name='Reason'>3</Data><Data Name='Detail'> entering </Data><Data/><Data>plain</Data></EventData></Event>";
///
/// let summary = summarize_event_xml(xml).ok_or("not an event")?;
/// assert_eq!(summary.provider, "Microsoft-Windows-Kernel-Power");
/// assert_eq!(summary.event_id, "506");
/// assert_eq!(summary.level, "Info");
/// assert_eq!(summary.pid, Some(4));
/// assert_eq!(summary.data, ["Reason=3", "Detail=entering", "plain"]);
///
/// // an error stays an error, and something that is not an event is nothing
/// let error = xml.replace("<Level>4</Level>", "<Level>2</Level>");
/// assert_eq!(summarize_event_xml(&error).map(|summary| summary.level), Some("Error"));
/// assert_eq!(summarize_event_xml("not xml"), None);
/// # Ok::<(), Box<dyn std::error::Error>>(())
/// ```
#[must_use]
pub fn summarize_event_xml(xml: &str) -> Option<EventLogSummary> {
    let provider_tag = &xml[xml.find("<Provider")?..];
    let provider = attribute(provider_tag.split('>').next()?, "Name")?.to_string();
    let event_id = element(xml, "EventID")?.trim().to_string();
    // 1 critical, 2 error, 3 warning, 4 information, 5 verbose (0: no level, which is information)
    let level = match element(xml, "Level").map(str::trim) {
        Some("1") => "Fault",
        Some("2") => "Error",
        Some("3") => "Error",
        _ => "Info",
    };

    let mut data = Vec::new();
    let mut rest = xml;
    while let Some(at) = rest.find("<Data") {
        rest = &rest[at..];
        let Some(close) = rest.find('>') else { break };
        let tag = &rest[..close];
        let name = attribute(tag, "Name");
        // `<Data/>` has no text
        if tag.ends_with('/') {
            rest = &rest[close..];
            continue;
        }

        let Some(end) = rest[close..].find("</Data>") else {
            break;
        };
        let value = rest[close + 1..close + end].trim();
        if !value.is_empty() {
            data.push(match name {
                Some(name) => format!("{name}={value}"),
                None => value.to_string(),
            });
        }
        rest = &rest[close + end..];
    }

    let pid = xml
        .find("<Execution")
        .and_then(|at| attribute(xml[at..].split('>').next()?, "ProcessID"))
        .and_then(|pid| pid.parse().ok());

    Some(EventLogSummary {
        provider,
        event_id,
        level,
        pid,
        data,
    })
}

#[derive(Debug, Default)]
struct Counters {
    lines: AtomicU64,
    bytes: AtomicU64,
    first_ns: AtomicU64,
    last_ns: AtomicU64,
}

/// One file of the OS trace, written to by whichever thread hears something.
///
/// A stream writes its records as NDJSON lines ([`TraceRecord::to_line`]) through a buffered
/// writer behind a lock, and counts what it writes for the manifest. It can be shared between
/// threads (it is handed out in an [`Arc`]). Written lines are visible to other readers only after
/// [`flush`](TraceStream::flush); [`TraceSet::checkpoint`] does that.
///
/// A stream needs a directory to write into, so it has no doctest. It is covered by
/// `a_stream_counts_what_it_writes_and_reports_it_for_the_manifest` in this file's unit tests.
pub struct TraceStream {
    spec: StreamSpec,
    started_ns: u64,
    out: Mutex<BufWriter<File>>,
    counters: Counters,
}

impl TraceStream {
    /// Creates the stream's file `file` under `dir` (and `dir/oslog`), truncating it if it exists.
    ///
    /// `name` and `source` are what the manifest records for the stream (see
    /// [`StreamSpec`]): for example `winevents` and `oslog:system`. The stream has no predicate and
    /// follows no single process.
    ///
    /// # Errors
    ///
    /// Returns an error if `dir/oslog` or the file cannot be created.
    pub fn create(dir: &Path, name: &str, source: &str, file: &str) -> eyre::Result<Arc<Self>> {
        std::fs::create_dir_all(dir.join("oslog"))?;
        Ok(Arc::new(Self {
            spec: StreamSpec {
                name: name.to_string(),
                source: source.to_string(),
                predicate: String::new(),
                level: OsLogLevel::Debug,
                file: file.to_string(),
                pid: None,
            },
            started_ns: clock::mono_now_ns(),
            out: Mutex::new(BufWriter::new(File::create(dir.join(file))?)),
            counters: Counters::default(),
        }))
    }

    /// Appends `record` as one line and counts it.
    ///
    /// A failed write is dropped and not counted: it must not take the recording down, and the
    /// counters say what was kept.
    pub fn write(&self, record: &TraceRecord) {
        let line = record.to_line();
        let mut out = self.out.lock();
        // a failed write must not take the recording down; the counters say what was kept
        if writeln!(out, "{line}").is_err() {
            return;
        }

        self.counters.lines.fetch_add(1, Ordering::Relaxed);
        self.counters
            .bytes
            .fetch_add(line.len() as u64 + 1, Ordering::Relaxed);
        let _ = self.counters.first_ns.compare_exchange(
            0,
            record.host_ns,
            Ordering::Relaxed,
            Ordering::Relaxed,
        );
        self.counters
            .last_ns
            .store(record.host_ns, Ordering::Relaxed);
    }

    /// Make what has been written readable from outside: for the manifest checkpoints, so a
    /// recording killed now can still be recovered with what this holds.
    pub fn flush(&self) {
        let _ = self.out.lock().flush();
    }

    /// Returns the live counters of the stream, for the display.
    #[must_use]
    pub fn live(&self) -> StreamLive {
        StreamLive {
            name: self.spec.name.clone(),
            events: self.counters.lines.load(Ordering::Relaxed),
            bytes: self.counters.bytes.load(Ordering::Relaxed),
        }
    }

    /// Returns the stream as the manifest records it. `header` says what the stream is, in words,
    /// and `error` is why it is not what was asked for, if it is not.
    ///
    /// The stream counts as ready from the moment it was created: `spawned_mach_ns` and
    /// `ready_mach_ns` are that time, and `restarts` is `0`. `first_event_mach_ns` and
    /// `last_event_mach_ns` are the `host_ns` of the first and last record written, `None` if none
    /// was.
    #[must_use]
    pub fn info(&self, header: &str, error: Option<String>) -> StreamInfo {
        let nonzero = |value: u64| (value != 0).then_some(value);
        StreamInfo {
            spec: self.spec.clone(),
            spawned_mach_ns: self.started_ns,
            ready_mach_ns: Some(self.started_ns),
            header: Some(header.to_string()),
            events: self.counters.lines.load(Ordering::Relaxed),
            bytes: self.counters.bytes.load(Ordering::Relaxed),
            first_event_mach_ns: nonzero(self.counters.first_ns.load(Ordering::Relaxed)),
            last_event_mach_ns: nonzero(self.counters.last_ns.load(Ordering::Relaxed)),
            restarts: 0,
            error,
        }
    }
}

/// The two files of the Windows OS trace and the taps that fill them.
///
/// A set creates both files, starts the taps of the host ([`crate::platform::trace::start`]: on
/// Windows a `WinEvent` hook and two Event Log subscriptions, on any other host nothing) and
/// reports each stream to the manifest. The recorder creates one on a host that has an OS trace but
/// no `log stream`.
///
/// A tap that cannot start does not stop the recording: the other carries on, the reason is in
/// [`problems`](TraceSet::problems), and [`drafts`](TraceSet::drafts) turns it into incidents.
///
/// Starting a set needs a directory and, for the taps, Windows, so there is no doctest. The record
/// format, the folding, the XML reading and the stream counters it is built from are covered by the
/// unit tests of this file; the taps are covered by none, and are checked on a real machine (see
/// `docs/dev/windows-checkpoints.md`).
pub struct TraceSet {
    /// The `WinEvent` stream: `oslog/system.ndjson`, named `winevents` in the manifest.
    pub system: Arc<TraceStream>,
    /// The Event Log stream: `oslog/eventlog.ndjson`, named `eventlog` in the manifest.
    pub event_log: Arc<TraceStream>,
    /// Why a tap could not be started, in words; the recording goes on without it.
    problems: Vec<String>,
    taps: Option<crate::platform::trace::Taps>,
}

impl TraceSet {
    /// Creates the files and starts listening. A tap that cannot start is reported in
    /// [`TraceSet::problems`] and the other carries on.
    ///
    /// # Errors
    ///
    /// Returns an error if either file cannot be created (see [`TraceStream::create`]). A tap that
    /// fails to start is not an error.
    pub fn start(dir: &Path) -> eyre::Result<Self> {
        let system = TraceStream::create(
            dir,
            crate::platform::trace::SYSTEM_NAME,
            "oslog:system",
            SYSTEM_FILE,
        )?;
        let event_log = TraceStream::create(
            dir,
            crate::platform::trace::EVENT_LOG_NAME,
            "oslog:eventlog",
            EVENT_LOG_FILE,
        )?;

        let (taps, problems) = crate::platform::trace::start(system.clone(), event_log.clone());
        Ok(Self {
            system,
            event_log,
            problems,
            taps,
        })
    }

    /// Returns why a tap could not be started, in words, one entry for each: they begin with
    /// `winevents` or `eventlog`, naming the tap. Empty when both started.
    #[must_use]
    pub fn problems(&self) -> &[String] {
        &self.problems
    }

    /// Returns the live counters of the two streams, the `WinEvent` one first.
    #[must_use]
    pub fn snapshot(&self) -> Vec<StreamLive> {
        vec![self.system.live(), self.event_log.live()]
    }

    fn infos(&self) -> Vec<StreamInfo> {
        let error = |name: &str| {
            self.problems
                .iter()
                .find(|problem| problem.starts_with(name))
                .cloned()
        };
        vec![
            self.system.info(
                crate::platform::trace::SYSTEM_ABOUT,
                error(crate::platform::trace::SYSTEM_NAME),
            ),
            self.event_log.info(
                crate::platform::trace::EVENT_LOG_ABOUT,
                error(crate::platform::trace::EVENT_LOG_NAME),
            ),
        ]
    }

    /// Flushes both files and returns the streams' state so far, the `WinEvent` one first, for a
    /// manifest checkpoint. A stream whose tap could not start carries the reason as its `error`.
    #[must_use]
    pub fn checkpoint(&self) -> Vec<StreamInfo> {
        self.system.flush();
        self.event_log.flush();
        self.infos()
    }

    /// Stops listening, flushes both files and reports what each stream held. Nothing is heard
    /// after this returns.
    pub fn stop(&mut self) -> Vec<StreamInfo> {
        // dropping the taps unhooks them: nothing arrives after this
        self.taps = None;
        self.checkpoint()
    }

    /// Returns a [`kind::LOG_STREAM_LOST`](crate::incidents::kind::LOG_STREAM_LOST) incident for
    /// each problem (`the OS trace is incomplete: <problem>`), for the incident log. Empty when both
    /// taps started.
    #[must_use]
    pub fn drafts(&self) -> Vec<Draft> {
        self.problems
            .iter()
            .map(|problem| {
                Draft::new(
                    crate::incidents::kind::LOG_STREAM_LOST,
                    format!("the OS trace is incomplete: {problem}"),
                )
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn win_events_are_named_and_unknown_ones_are_numbered() {
        assert_eq!(win_event_name(0x0003), "SystemForeground");
        assert_eq!(win_event_name(0x800B), "ObjectLocationChange");
        assert_eq!(win_event_name(0x8017), "ObjectCloaked");
        assert_eq!(win_event_name(0x7777), "Event0x7777");
    }

    #[test]
    fn a_burst_for_one_window_is_one_record_that_says_how_many_it_stands_for() {
        let mut coalescer = Coalescer::default();
        let moved = 0x800B;
        let window = 0x1234;

        assert_eq!(
            coalescer.offer(1_000_000_000, window, moved),
            Offer::Emit { folded: 0 }
        );
        for step in 1..=5 {
            assert_eq!(
                coalescer.offer(1_000_000_000 + step * 10_000_000, window, moved),
                Offer::Fold
            );
        }

        // the burst is over: the next one is recorded and says how many were folded before it
        assert_eq!(
            coalescer.offer(1_300_000_000, window, moved),
            Offer::Emit { folded: 5 }
        );
    }

    #[test]
    fn windows_and_events_are_folded_separately_and_only_noisy_ones_at_all() {
        let mut coalescer = Coalescer::default();
        assert_eq!(coalescer.offer(1, 1, 0x800B), Offer::Emit { folded: 0 });
        // another window, and another kind of event, are their own
        assert_eq!(coalescer.offer(2, 2, 0x800B), Offer::Emit { folded: 0 });
        assert_eq!(coalescer.offer(3, 1, 0x800C), Offer::Emit { folded: 0 });

        // a window shown or hidden or focused is always its own record, however fast
        for _ in 0..3 {
            assert_eq!(coalescer.offer(4, 1, 0x8002), Offer::Emit { folded: 0 });
            assert_eq!(coalescer.offer(4, 1, 0x0003), Offer::Emit { folded: 0 });
        }
    }

    #[test]
    fn the_memory_of_windows_is_bounded() {
        let mut coalescer = Coalescer::default();
        for window in 0..(COALESCE_CAPACITY as isize * 3) {
            coalescer.offer(1_000_000_000, window, 0x800B);
        }
        assert!(coalescer.last.len() <= COALESCE_CAPACITY);
    }

    #[test]
    fn a_record_is_a_line_the_existing_readers_understand() {
        let record = TraceRecord {
            host_ns: 5_000_000_000,
            wall_us: 1_793_016_000_123_456,
            pid: 4242,
            process: "chrome.exe".to_string(),
            subsystem: "winevent".to_string(),
            category: "ObjectLocationChange".to_string(),
            level: "Info",
            message: "hwnd=0x1f2a class=Chrome_WidgetWin_1 (+3 folded)".to_string(),
        };

        let parsed = crate::sources::oslog::OsLogRecord::parse(&record.to_line())
            .expect("the Mac's reader parses it");
        assert_eq!(parsed.pid, 4242);
        assert_eq!(parsed.process_name(), "chrome.exe");
        assert_eq!(parsed.subsystem, "winevent");
        assert_eq!(parsed.category, "ObjectLocationChange");
        assert_eq!(parsed.host_ns, Some(5_000_000_000));
        assert!(parsed.timestamp.starts_with("2026-10-26 12:00:00.123456"));
        assert_eq!(
            crate::sources::oslog::extract_time_ns(
                record.to_line().as_bytes(),
                crate::clock::Timebase { numer: 1, denom: 1 }
            ),
            Some(5_000_000_000)
        );
    }

    const EVENT_XML: &str = "<Event xmlns='http://schemas.microsoft.com/win/2004/08/events/event'>\
        <System><Provider Name='Microsoft-Windows-Kernel-Power' Guid='{331C3B3A-2005-44C2-AC5E-77220C37D6B4}'/>\
        <EventID>506</EventID><Level>4</Level><TimeCreated SystemTime='2026-10-27T12:00:00.000000000Z'/><Execution ProcessID='4' ThreadID='8'/></System>\
        <EventData><Data Name='Reason'>3</Data><Data Name='Detail'> entering </Data><Data/><Data>plain</Data></EventData></Event>";

    #[test]
    fn an_event_log_record_is_read_for_its_provider_id_level_and_data() {
        let summary = summarize_event_xml(EVENT_XML).expect("summary");
        assert_eq!(summary.provider, "Microsoft-Windows-Kernel-Power");
        assert_eq!(summary.event_id, "506");
        assert_eq!(summary.level, "Info");
        assert_eq!(summary.pid, Some(4));
        assert_eq!(summary.data, ["Reason=3", "Detail=entering", "plain"]);
    }

    #[test]
    fn errors_and_faults_keep_their_severity_and_junk_is_not_a_record() {
        let error = EVENT_XML.replace("<Level>4</Level>", "<Level>2</Level>");
        assert_eq!(summarize_event_xml(&error).expect("summary").level, "Error");
        let critical = EVENT_XML.replace("<Level>4</Level>", "<Level>1</Level>");
        assert_eq!(
            summarize_event_xml(&critical).expect("summary").level,
            "Fault"
        );
        assert_eq!(summarize_event_xml("not xml"), None);
    }

    #[test]
    fn a_stream_counts_what_it_writes_and_reports_it_for_the_manifest() {
        let dir = std::env::temp_dir().join(format!("komorebi-wintrace-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let stream =
            TraceStream::create(&dir, "winevents", "oslog:system", SYSTEM_FILE).expect("create");

        for host_ns in [10, 30] {
            stream.write(&TraceRecord {
                host_ns,
                wall_us: 1,
                pid: 1,
                process: "a.exe".to_string(),
                subsystem: "winevent".to_string(),
                category: "ObjectShow".to_string(),
                level: "Info",
                message: "hwnd=0x1".to_string(),
            });
        }
        stream.flush();

        let info = stream.info("hook", None);
        assert_eq!(
            (
                info.events,
                info.first_event_mach_ns,
                info.last_event_mach_ns
            ),
            (2, Some(10), Some(30))
        );
        assert_eq!(info.spec.source, "oslog:system");
        let text = std::fs::read_to_string(dir.join(SYSTEM_FILE)).expect("file");
        assert_eq!(text.lines().count(), 2);
        assert_eq!(stream.live().events, 2);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
