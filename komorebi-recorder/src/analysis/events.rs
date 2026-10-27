//! komorebi's subscriber events, understood.
//!
//! `events.jsonl` keeps each notification's JSON as komorebi serialized it. Three kinds reach a
//! subscriber and they look different on the wire:
//!
//! - window-manager events, from the OS: `{"type":"MoveEnd","content":[{"type":"Accessibility",
//!   "content":"AXWindowMoved"},<pid>,<window id>]}`
//! - commands, from `komorebic` or a hotkey daemon: `{"type":"FocusWorkspaceNumber","content":0}`
//! - monitor changes: `{"DisplayConnectionChange":<display id>}`
//!
//! The shapes above are komorebi for Mac's. komorebi for Windows serializes the same three kinds
//! differently (a window handle and a `WinEvent` in place of a process id and an accessibility
//! notification), so reading one is up to the `Rules` of the flavor that recorded the session
//! ([`super::flavor`]). This module defines what a read event looks like, an [`EventRecord`] with
//! the name, process and window pulled out, and `load`, which reads a session's whole
//! `events.jsonl` through those rules.
//!
//! What an event *was* (a command, a drag, an echo of komorebi's own move) is not decided here;
//! [`super::classify`] labels each [`EventRecord`]. Tying an event to the log lines of the handler
//! that produced it is done by `blocks::pair`. Both read the events this module returns.

use super::flavor::Rules;
use crate::session::Session;
use crate::sources::komorebi_events::EventRow;
use crate::sources::winprobe::WindowFacts;
use color_eyre::eyre;
use color_eyre::eyre::WrapErr;
use schemars::JsonSchema;
use serde::Serialize;
use serde_json::Value;

/// Which of the kinds of notification a subscriber receives an event is, or that it could not be
/// read.
///
/// Serialized in kebab case (`window-manager`, `command`, `monitor`, `unknown`).
/// `Rules::describe_event` decides the family of an event, so the same JSON can be in a different
/// family for the two flavors (a bare string is a command for komorebi for Mac and, if it is
/// not a virtual desktop notification, [`Family::Unknown`] for komorebi for Windows).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, JsonSchema)]
#[serde(rename_all = "kebab-case")]
pub enum Family {
    /// Something the operating system reported about a window or application.
    WindowManager,
    /// A command sent to komorebi.
    Command,
    /// A display was connected, disconnected or resized. On Windows, also a notification about the
    /// machine (a lock, a sleep, a wake) or a virtual desktop.
    Monitor,
    /// The notification could not be read.
    Unknown,
}

/// One notification from komorebi, read by the `Rules` of the flavor that sent it.
///
/// The fields are what the analysis asks of an event whatever the flavor: what kind it is, what it
/// is called, which process or window it is about, and what the OS said. A field is `None` when the
/// event does not say, which is not the same as the event being about nothing. The `exe` is left
/// out of the serialized form when it is `None`.
///
/// # Example
///
/// ```rust
/// use komorebi_recorder::analysis::events::Family;
/// use komorebi_recorder::analysis::flavor::Flavor;
/// use serde_json::json;
///
/// let info = Flavor::Macos.rules().describe_event(&json!({
///     "type": "FocusWorkspaceNumber",
///     "content": 2,
/// }));
/// assert_eq!(info.family, Family::Command);
/// assert_eq!(info.name, "FocusWorkspaceNumber");
/// assert_eq!(info.argument.as_deref(), Some("2"));
/// assert_eq!(info.summary(), "FocusWorkspaceNumber 2");
/// ```
#[derive(Debug, Clone, PartialEq, Eq, Serialize, JsonSchema)]
pub struct EventInfo {
    /// Which kind of notification this is.
    pub family: Family,
    /// `MoveEnd`, `FocusWorkspaceNumber`, `DisplayConnectionChange`, ... For an event that could
    /// not be read, `unreadable`.
    pub name: String,
    /// The notification the OS reported, e.g. `AXWindowMoved`; window-manager events only.
    pub notification: Option<String>,
    /// The process the event is about. Window-manager events of komorebi for Mac have one;
    /// komorebi for Windows events never do (they have a window handle instead).
    pub pid: Option<i32>,
    /// The window the event is about: a window id on a Mac (a focus change may name none) or a
    /// handle on Windows.
    pub window_id: Option<u32>,
    /// The program of the window the event is about, where the event names it (komorebi for Windows
    /// does; a process id is all the Mac gives).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub exe: Option<String>,
    /// A command's argument or a monitor event's display id, as compact JSON. Longer than 80
    /// characters, it is cut to 79 and ends in `…`.
    pub argument: Option<String>,
}

impl EventInfo {
    /// Returns one line saying what this is: `MoveEnd pid 20934 AXWindowMoved`.
    ///
    /// The line is the name, then the program, the argument, `pid <pid>`, `window <id>` and the
    /// notification, each only if there is one, separated by spaces.
    ///
    /// # Example
    ///
    /// ```rust
    /// use komorebi_recorder::analysis::flavor::Flavor;
    /// use serde_json::json;
    ///
    /// let moved = Flavor::Macos.rules().describe_event(&json!({
    ///     "type": "MoveEnd",
    ///     "content": [{"type": "Accessibility", "content": "AXWindowMoved"}, 20934, 4711]
    /// }));
    /// assert_eq!(moved.summary(), "MoveEnd pid 20934 window 4711 AXWindowMoved");
    ///
    /// let display = Flavor::Macos.rules().describe_event(&json!({"DisplayConnectionChange": 1}));
    /// assert_eq!(display.summary(), "DisplayConnectionChange 1");
    /// ```
    #[must_use]
    pub fn summary(&self) -> String {
        let mut text = self.name.clone();
        if let Some(exe) = &self.exe {
            text.push(' ');
            text.push_str(exe);
        }
        if let Some(argument) = &self.argument {
            text.push(' ');
            text.push_str(argument);
        }
        if let Some(pid) = self.pid {
            text.push_str(&format!(" pid {pid}"));
        }
        if let Some(window_id) = self.window_id {
            text.push_str(&format!(" window {window_id}"));
        }
        if let Some(notification) = &self.notification {
            text.push(' ');
            text.push_str(notification);
        }
        text
    }
}

pub(crate) fn compact(value: &Value) -> String {
    let text = value.to_string();
    if text.chars().count() > 80 {
        let cut: String = text.chars().take(79).collect();
        format!("{cut}…")
    } else {
        text
    }
}

/// One recorded notification: a line of `events.jsonl` with its JSON read into an [`EventInfo`].
///
/// [`EventRecord::info`] is flattened when a record is serialized, so a record's JSON has the
/// `family`, `name` and the other fields of [`EventInfo`] beside the record's fields.
#[derive(Debug, Clone, PartialEq, Serialize, JsonSchema)]
pub struct EventRecord {
    /// The notification's sequence number in the session: 1 for the first the recorder accepted,
    /// counting up. It is what `event:<seq>` names.
    pub seq: u64,
    /// When the recorder received the notification, in nanoseconds on the host's master monotonic
    /// clock (`mach_` is a historical spelling; the manifest's `host.clock` says `mach`, `qpc` or
    /// `std`). For a window-manager event this is when its handler finished, because komorebi
    /// notifies subscribers after the handler body.
    pub recv_mach_ns: u64,
    /// The same moment on the wall clock, in microseconds since the Unix epoch.
    pub recv_wall_us: i64,
    /// The state snapshot after this event (`states/<id>.json`); the same id as the previous event
    /// means the state did not change.
    pub state_id: Option<u64>,
    /// The notification, as the flavor's rules read it.
    #[serde(flatten)]
    pub info: EventInfo,
    /// Why the notification's JSON could not be parsed, if it could not. Such a record has
    /// [`Family::Unknown`] and the name `unreadable`.
    pub parse_error: Option<String>,
    /// The facts Windows reported about the event's window just after it, when the recording was
    /// made with `--probe-windows`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub probe: Option<WindowFacts>,
}

impl EventRecord {
    /// Returns the record for one line of `events.jsonl`, reading its event with `rules`.
    ///
    /// A row with no event (its notification could not be parsed as JSON) gives
    /// [`Family::Unknown`] and the name `unreadable`, not an error; the row's other fields are
    /// kept.
    ///
    /// # Example
    ///
    /// ```rust
    /// use komorebi_recorder::analysis::events::{EventRecord, Family};
    /// use komorebi_recorder::analysis::flavor::Flavor;
    /// use komorebi_recorder::sources::komorebi_events::EventRow;
    ///
    /// let row: EventRow = serde_json::from_str(
    ///     r#"{"seq": 7, "recv_wall_us": 0, "recv_mach_ns": 30, "state_id": 4,
    ///         "event": {"type": "Retile"}}"#,
    /// )?;
    /// let record = EventRecord::from_row(&row, Flavor::Macos.rules());
    /// assert_eq!(record.seq, 7);
    /// assert_eq!(record.state_id, Some(4));
    /// assert_eq!(record.info.family, Family::Command);
    ///
    /// // a notification that was not JSON has no event: it is unreadable, and says why
    /// let row: EventRow = serde_json::from_str(
    ///     r#"{"seq": 8, "recv_wall_us": 0, "recv_mach_ns": 40, "parse_error": "expected value"}"#,
    /// )?;
    /// let record = EventRecord::from_row(&row, Flavor::Macos.rules());
    /// assert_eq!(record.info.family, Family::Unknown);
    /// assert_eq!(record.info.name, "unreadable");
    /// assert_eq!(record.parse_error.as_deref(), Some("expected value"));
    /// # Ok::<(), Box<dyn std::error::Error>>(())
    /// ```
    #[must_use]
    pub fn from_row(row: &EventRow, rules: &dyn Rules) -> Self {
        let info = row.event.as_ref().map_or_else(
            || EventInfo {
                family: Family::Unknown,
                name: "unreadable".to_string(),
                notification: None,
                pid: None,
                window_id: None,
                exe: None,
                argument: None,
            },
            |event| rules.describe_event(event),
        );

        Self {
            seq: row.seq,
            recv_mach_ns: row.recv_mach_ns,
            recv_wall_us: row.recv_wall_us,
            state_id: row.state_id,
            info,
            parse_error: row.parse_error.clone(),
            probe: row.probe.clone(),
        }
    }
}

/// Returns every event of a session, in the order they were received, with the warnings from
/// reading them.
///
/// The events are the lines of the session's `events.jsonl`, read with `rules` and sorted by
/// `recv_mach_ns`, then by `seq`. A session with no `events.jsonl` has no events, which is not an
/// error. A line that is not a valid row (as opposed to a row whose notification could not be
/// parsed, which is kept as an unreadable event) is skipped, and the warnings say how many were:
/// `N unreadable lines of events.jsonl were skipped`. Blank lines are ignored silently.
///
/// # Errors
///
/// Returns an error if `events.jsonl` exists and cannot be read: `could not read <path>`, with
/// the I/O error as its cause. A missing file is not one of those cases.
///
/// # Example
///
/// Reading a session needs a session directory on disk, so this example is only compiled. It is
/// covered by `loading_sorts_by_receipt_and_skips_what_it_cannot_read` in this module's tests.
///
/// ```rust,no_run
/// use komorebi_recorder::analysis::events;
/// use komorebi_recorder::analysis::flavor::Flavor;
/// use komorebi_recorder::session::Session;
///
/// let session = Session::open("/path/to/session")?;
/// let (records, warnings) = events::load(&session, Flavor::Macos.rules())?;
/// for record in &records {
///     println!("#{} {}", record.seq, record.info.summary());
/// }
/// assert!(warnings.is_empty());
/// # Ok::<(), Box<dyn std::error::Error>>(())
/// ```
pub fn load(session: &Session, rules: &dyn Rules) -> eyre::Result<(Vec<EventRecord>, Vec<String>)> {
    let path = session.path("events.jsonl");
    let text = match std::fs::read_to_string(&path) {
        Ok(text) => text,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return Ok((Vec::new(), Vec::new()));
        }
        Err(error) => {
            return Err(error).wrap_err_with(|| format!("could not read {}", path.display()));
        }
    };

    let mut records = Vec::new();
    let mut skipped = 0_usize;
    for line in text.lines().filter(|line| !line.trim().is_empty()) {
        match serde_json::from_str::<EventRow>(line) {
            Ok(row) => records.push(EventRecord::from_row(&row, rules)),
            Err(_) => skipped += 1,
        }
    }

    records.sort_by_key(|record| (record.recv_mach_ns, record.seq));
    let warnings = if skipped > 0 {
        vec![format!(
            "{skipped} unreadable lines of events.jsonl were skipped"
        )]
    } else {
        Vec::new()
    };
    Ok((records, warnings))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::analysis::flavor::Flavor;

    #[test]
    fn loading_sorts_by_receipt_and_skips_what_it_cannot_read() {
        let dir =
            std::env::temp_dir().join(format!("komorebi-recorder-events-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let session = Session::create(Some(dir.clone()), chrono::Utc::now()).expect("create");
        std::fs::write(
            session.path("events.jsonl"),
            concat!(
                "{\"seq\":2,\"recv_wall_us\":0,\"recv_mach_ns\":20,\"event\":{\"type\":\"Retile\"}}\n",
                "garbage\n",
                "{\"seq\":1,\"recv_wall_us\":0,\"recv_mach_ns\":10,\"state_id\":4}\n",
            ),
        )
        .expect("events");

        let (records, warnings) = load(&session, Flavor::Macos.rules()).expect("load");
        assert_eq!(
            records.iter().map(|record| record.seq).collect::<Vec<_>>(),
            [1, 2]
        );
        assert_eq!(records[0].state_id, Some(4));
        assert_eq!(records[0].info.family, Family::Unknown);
        assert_eq!(records[1].info.name, "Retile");
        assert_eq!(warnings.len(), 1);
    }
}
