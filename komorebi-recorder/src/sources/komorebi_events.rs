//! komorebi's subscriber-socket notifications: every window manager event together with the full
//! `State` it produced.
//!
//! The daemon pushes each notification as one JSON blob over a fresh unix-socket connection to a
//! listener the recorder binds, and connects *synchronously* per subscriber while holding its own
//! locks, so a slow subscriber stalls komorebi. The accept thread therefore does the bare minimum
//! (stamp the receive time, read to EOF, hand the bytes to a channel) and all parsing and disk IO
//! happens on a separate writer thread.
//!
//! The payload carries no timestamp, so the receive time is stamped here on both clocks. komorebi
//! sends the notification *after* its handler has run, so the receive time marks the **end** of
//! handling the event, not its start: in komorebi for Mac's log the `processing event` line is
//! when the handler began (and, at DEBUG, the `pushed notification to subscriber` line is when it
//! sent this). `komorebi-recorder trace` ties the two together.
//!
//! A notification is a JSON object with an `event` (what komorebi was told or did) and the `state`
//! (komorebi's whole `State` after it). This module is the same on both hosts and for both flavors
//! of komorebi. It stores the payload as komorebi sent it and does not read it: what an event
//! means is the business of the [`Rules`](crate::analysis::flavor::Rules) of the flavor that
//! recorded it (see [`crate::analysis::events`]).
//!
//! # Files written
//!
//! | file | contents |
//! |---|---|
//! | `events.jsonl` | one [`EventRow`] per line, one line per notification, in arrival order |
//! | `states/<id>.json` | komorebi's `State` as compact JSON, stored only when it differs from the last one stored; named by [`state_file_name`] |
//! | `state-start.json`, `state-end.json` | the `State` komorebi answered to a query at the start and the end of the recording ([`snapshot_state`]) |
//!
//! A line of `events.jsonl`, for a window manager event on komorebi for Mac:
//!
//! ```json
//! {"seq":7,"recv_wall_us":1793016000123456,"recv_mach_ns":5000000000,"event":{"content":[{"content":"AXWindowMoved","type":"Accessibility"},20934,3395],"type":"MoveEnd"},"state_id":2}
//! ```
//!
//! The file is flushed whenever the writer has caught up with the notifications that have arrived,
//! so a recording that is killed loses only what the writer had not yet caught up on.
//!
//! # Threads
//!
//! [`EventRecorder`] runs two threads. The accept thread (`komorebi-events-accept`) numbers and
//! stamps each notification and does nothing else. The writer thread (`komorebi-events-write`)
//! parses it, stores a new `State`, tells the app discovery about it ([`Discovery`]), runs the
//! window prober when one was given (`--probe-windows`, see `winprobe`) and appends the row. The
//! prober never runs on the accept thread: komorebi delivers a notification while holding its own
//! locks, so a query sent to komorebi from the accept thread could wait on a lock that is itself
//! waiting for the notification to be accepted.

use crate::clock;
use crate::komorebi_link;
use crate::komorebi_link::Message;
use crate::komorebi_link::SubscribeOptions;
use crate::komorebi_link::UnixListener;
use crate::komorebi_link::UnixStream;
use color_eyre::eyre;
use color_eyre::eyre::WrapErr;
use crossbeam_channel::Receiver;
use crossbeam_channel::Sender;
use serde::Deserialize;
use serde::Serialize;
use serde_json::Value;
use std::fs::File;
use std::io::BufWriter;
use std::io::Read;
use std::io::Write;
use std::path::Path;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::AtomicBool;
use std::sync::atomic::AtomicU64;
use std::sync::atomic::Ordering;
use std::thread::JoinHandle;
use std::time::Duration;

/// How long to keep accepting after asking komorebi to forget the recorder.
const UNSUBSCRIBE_DRAIN: Duration = Duration::from_millis(250);

fn subscribe_to_komorebi(name: &str) -> std::io::Result<()> {
    // unfiltered: every event is wanted, not just the ones that changed state
    komorebi_link::send_message(&Message::AddSubscriberSocketWithOptions(
        name.to_string(),
        SubscribeOptions {
            filter_state_changes: false,
        },
    ))
}

/// A notification as it arrived, before any parsing.
struct RawNotification {
    seq: u64,
    recv_wall_us: i64,
    recv_mach_ns: u64,
    bytes: Vec<u8>,
}

/// One line of `events.jsonl`: a notification as komorebi sent it, with the time it arrived on
/// both clocks.
///
/// The file is JSON Lines: one object per line, one line per notification, in the order the
/// recorder accepted them. `seq`, `recv_wall_us` and `recv_mach_ns` are always present. Every
/// other key is left out of the line when it has no value, and reads back as `None` when absent.
///
/// A row is one of two shapes, and `event` and `parse_error` are never both present:
///
/// - **read**: the payload was valid JSON. `event` is its `event` value (`None` if the payload had
///   no `event` key) and `parse_error` and `raw` are absent.
/// - **unreadable**: the payload was not valid JSON. `event` is absent, `parse_error` gives the
///   reason and `raw` keeps the payload as text, so a notification that cannot be read is not lost.
///
/// # Example
///
/// A row is one line of text, and reads back as it was written:
///
/// ```rust
/// use komorebi_recorder::sources::komorebi_events::EventRow;
///
/// let line = r#"{"seq":7,"recv_wall_us":1793016000123456,"recv_mach_ns":5000000000,"event":{"type":"Show","content":[null,1]},"state_id":2}"#;
/// let row: EventRow = serde_json::from_str(line)?;
/// assert_eq!((row.seq, row.state_id), (7, Some(2)));
/// assert_eq!(row.event.as_ref().and_then(|event| event["type"].as_str()), Some("Show"));
///
/// // keys without a value are left out of the line: there is no `parse_error`, `raw` or `probe`
/// let written = serde_json::to_string(&row)?;
/// assert!(!written.contains("parse_error") && !written.contains("raw") && !written.contains("probe"));
/// assert_eq!(serde_json::from_str::<EventRow>(&written)?.seq, 7);
/// # Ok::<(), Box<dyn std::error::Error>>(())
/// ```
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EventRow {
    /// The notification's place in arrival order, counting from 1 for each recorder. The accept
    /// thread numbers every non-empty notification it receives.
    pub seq: u64,
    /// When the notification was accepted, in microseconds since the Unix epoch on the wall clock.
    pub recv_wall_us: i64,
    /// When the notification was accepted, in nanoseconds on the host's master monotonic clock
    /// (see [`crate::clock::mono_now_ns`]). komorebi notifies after its handler has run, so this
    /// is when handling the event ended.
    pub recv_mach_ns: u64,
    /// The `event` value of the payload, re-serialized from the parsed JSON (komorebi's key order
    /// and whitespace are not kept). Its shape is komorebi's and differs by flavor: a window
    /// manager event, a command or a monitor change. Absent in an unreadable row.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub event: Option<Value>,
    /// The id of the stored state snapshot (`states/<id>.json`) in effect after this event: the
    /// most recent snapshot stored at or before this row. `None` until the first snapshot is
    /// stored.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub state_id: Option<u64>,
    /// Why the payload was not valid JSON, as `serde_json` words it. Present only in an
    /// unreadable row.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parse_error: Option<String>,
    /// The payload as text, with any invalid UTF-8 replaced. Present only in an unreadable row.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub raw: Option<String>,
}

/// Something the app-discovery logic wants to know about as it happens, sent by the writer thread.
pub enum Discovery {
    /// The state changed: this is the `State` that was just stored as a new snapshot.
    State(Arc<Value>),
    /// A window manager event mentioned this process id (see `managed_apps::event_pid`).
    Pid(i32),
}

/// Running totals of what the writer thread has handled, readable from any thread while the
/// recording goes on.
///
/// Each counter only grows and is updated with relaxed ordering, so a reader sees a recent value,
/// not a value that is consistent with the others. [`EventSummary`] is these totals at the end.
#[derive(Debug, Default)]
pub struct EventCounters {
    /// Notifications the writer has dequeued, readable or not.
    pub events: AtomicU64,
    /// `State` snapshots the writer has stored in `states/`.
    pub states: AtomicU64,
    /// Bytes of payload in those notifications.
    pub bytes: AtomicU64,
    /// Notifications whose payload was not valid JSON.
    pub parse_errors: AtomicU64,
}

/// The totals of an [`EventCounters`], as the manifest (`session.json`, key `events`) stores them.
///
/// The recorder stores the totals so far at each checkpoint and the final totals when it stops;
/// [`EventRecorder::stop`] returns them.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct EventSummary {
    /// Notifications handled, readable or not: the number of lines in `events.jsonl`.
    pub events: u64,
    /// `State` snapshots stored: the number of files in `states/`.
    pub state_snapshots: u64,
    /// Bytes of payload received, before parsing.
    pub bytes: u64,
    /// Notifications whose payload was not valid JSON (their rows have `parse_error`).
    pub parse_errors: u64,
}

/// Returns the path of the file that stores state snapshot `id`, relative to the session
/// directory: `states/<id>.json`, with `<id>` zero padded to six digits.
///
/// Ids count from 1 in the order the snapshots were stored, so file names sort in that order up to
/// 999999 snapshots; an id with more digits is not padded further.
///
/// # Example
///
/// ```rust
/// use komorebi_recorder::sources::komorebi_events::state_file_name;
///
/// assert_eq!(state_file_name(1), "states/000001.json");
/// assert_eq!(state_file_name(123_456), "states/123456.json");
/// ```
#[must_use]
pub fn state_file_name(id: u64) -> String {
    format!("states/{id:06}.json")
}

/// Records komorebi's subscriber notifications into a session directory until it is stopped.
///
/// A recorder binds a listener socket in komorebi's data directory, asks komorebi to send it every
/// notification, and writes `events.jsonl` and `states/` (see [`crate::sources::komorebi_events`]
/// for the files and the threads). It registers under the name `komorebi-recorder-<pid>`, which
/// komorebi's DEBUG log repeats when it pushes a notification, so an event can be tied to its
/// handler.
///
/// The subscription asks for every event, not only the ones that changed komorebi's state.
///
/// Stopping, or dropping a recorder that was not stopped, asks komorebi to forget the subscriber
/// and then keeps accepting for a moment before it closes the listener. If komorebi was in the
/// middle of a notification when it was told to forget the recorder, a reader that had already gone
/// would make its write fail, and that failure aborts delivery of the event to komorebi's other
/// subscribers (a status bar, for one).
///
/// # Example
///
/// Recording needs a komorebi socket and a session directory, so this example only compiles. The
/// writer's behavior is covered by `the_writer_dedupes_unchanged_states_and_tolerates_garbage`
/// and `the_writer_stores_what_the_prober_read_beside_the_event_it_is_about` in this file's unit
/// tests. The accept thread and the subscription handshake have no unit test: they are exercised
/// by recording against a running komorebi.
///
/// ```rust,no_run
/// use komorebi_recorder::sources::komorebi_events::EventRecorder;
/// use std::path::Path;
///
/// let recorder = EventRecorder::start(Path::new("session"), None)?;
/// if !recorder.is_subscribed() {
///     // komorebi was not running: register again once it is
///     let _ = recorder.subscribe();
/// }
///
/// // ... record ...
///
/// let summary = recorder.stop();
/// println!("{} events, {} states", summary.events, summary.state_snapshots);
/// # Ok::<(), Box<dyn std::error::Error>>(())
/// ```
pub struct EventRecorder {
    name: String,
    socket: PathBuf,
    subscribed: Arc<AtomicBool>,
    stop: Arc<AtomicBool>,
    counters: Arc<EventCounters>,
    accept: Option<JoinHandle<()>>,
    writer: Option<JoinHandle<()>>,
}

impl EventRecorder {
    /// Listens for komorebi's notifications, writing `events.jsonl` and `states/` into `dir`, and
    /// subscribes to komorebi if it is running.
    ///
    /// komorebi not running is not an error: the listener is bound regardless, so that when it
    /// starts (or restarts, which forgets every subscriber) a later [`subscribe`](Self::subscribe)
    /// picks the notifications up without losing the listener or the events file.
    /// [`is_subscribed`](Self::is_subscribed) reports whether the first attempt succeeded.
    ///
    /// # Errors
    ///
    /// Returns an error if
    ///
    /// - `dir/states` cannot be created,
    /// - the listener cannot be bound: a stale socket file of an earlier subscriber that cannot be
    ///   removed, or komorebi's data directory does not exist (as when komorebi has never run) or
    ///   cannot be written to (a unix socket path is also limited in length, to about 104 bytes on
    ///   macOS),
    /// - `dir/events.jsonl` cannot be created, or
    /// - a thread cannot be spawned.
    ///
    /// An error from the subscription itself is not one of them.
    pub fn start(dir: &Path, discovery: Option<Sender<Discovery>>) -> eyre::Result<Self> {
        std::fs::create_dir_all(dir.join("states"))?;

        let name = format!("komorebi-recorder-{}", std::process::id());
        let socket = komorebi_link::socket_path(&name);
        let listener = komorebi_link::bind_subscriber(&socket)?;

        let subscribed = Arc::new(AtomicBool::new(subscribe_to_komorebi(&name).is_ok()));

        let events_file = File::create(dir.join("events.jsonl"))?;
        let (tx, rx) = crossbeam_channel::unbounded::<RawNotification>();
        let stop = Arc::new(AtomicBool::new(false));
        let counters = Arc::new(EventCounters::default());

        let accept_stop = stop.clone();
        let accept = std::thread::Builder::new()
            .name("komorebi-events-accept".to_string())
            .spawn(move || accept_loop(&listener, &tx, &accept_stop))?;

        let writer_counters = counters.clone();
        let states_dir = dir.join("states");
        let writer = std::thread::Builder::new()
            .name("komorebi-events-write".to_string())
            .spawn(move || {
                write_loop(
                    &rx,
                    events_file,
                    &states_dir,
                    &writer_counters,
                    discovery.as_ref(),
                )
            })?;

        Ok(Self {
            name,
            socket,
            subscribed,
            stop,
            counters,
            accept: Some(accept),
            writer: Some(writer),
        })
    }

    /// Returns the live counters, shared with the writer thread. They keep counting until the
    /// recorder is stopped.
    #[must_use]
    pub fn counters(&self) -> Arc<EventCounters> {
        self.counters.clone()
    }

    /// The name this recorder registers under with komorebi, `komorebi-recorder-<pid>` for this
    /// process. It names the recorder's socket in komorebi's data directory and is what komorebi's
    /// DEBUG log says when it pushes a notification here.
    #[must_use]
    pub fn subscriber_name(&self) -> &str {
        &self.name
    }

    /// Returns `true` if and only if the most recent attempt to register with komorebi succeeded.
    ///
    /// This is the outcome of the last [`start`](Self::start) or [`subscribe`](Self::subscribe),
    /// not a live check: a komorebi that has restarted has forgotten the recorder, and this stays
    /// `true` until the next call to [`subscribe`](Self::subscribe) says otherwise.
    #[must_use]
    pub fn is_subscribed(&self) -> bool {
        self.subscribed.load(Ordering::Relaxed)
    }

    /// (Re-)registers with komorebi. Safe to call at any time; komorebi forgets its subscribers
    /// when it restarts, so the recorder calls this when it sees a new komorebi.
    ///
    /// # Errors
    ///
    /// Returns an error if komorebi's socket cannot be connected to (komorebi is not running), or
    /// the message cannot be written within one second. [`is_subscribed`](Self::is_subscribed)
    /// then reads `false`.
    pub fn subscribe(&self) -> std::io::Result<()> {
        let result = subscribe_to_komorebi(&self.name);
        self.subscribed.store(result.is_ok(), Ordering::Relaxed);
        result
    }

    /// Unsubscribes, drains everything already received, closes the listener and returns the
    /// totals.
    ///
    /// This takes about a quarter of a second longer than the writer needs: the recorder keeps
    /// accepting for that long after asking komorebi to forget it (see [`EventRecorder`]). The
    /// socket file is removed. When this returns, every notification accepted has been written to
    /// `events.jsonl` and flushed.
    pub fn stop(mut self) -> EventSummary {
        self.shutdown();
        EventSummary {
            events: self.counters.events.load(Ordering::Relaxed),
            state_snapshots: self.counters.states.load(Ordering::Relaxed),
            bytes: self.counters.bytes.load(Ordering::Relaxed),
            parse_errors: self.counters.parse_errors.load(Ordering::Relaxed),
        }
    }

    fn shutdown(&mut self) {
        // tell komorebi first so it stops connecting while we wind down
        let _ = komorebi_link::send_message(&Message::RemoveSubscriberSocket(self.name.clone()));

        // Keep reading for a moment. If komorebi was mid-notification when we unsubscribed, a
        // reader that has already gone away would make its write fail, which aborts delivery of
        // that event to its *other* subscribers (e.g. the bar). Draining lets it finish cleanly.
        std::thread::sleep(UNSUBSCRIBE_DRAIN);

        self.stop.store(true, Ordering::SeqCst);
        // wake the blocking accept so it observes the stop flag
        let _ = UnixStream::connect(&self.socket);

        if let Some(accept) = self.accept.take() {
            let _ = accept.join();
        }

        // the accept thread owned the only sender, so the writer drains and ends
        if let Some(writer) = self.writer.take() {
            let _ = writer.join();
        }

        let _ = std::fs::remove_file(&self.socket);
    }
}

impl Drop for EventRecorder {
    fn drop(&mut self) {
        if self.accept.is_some() || self.writer.is_some() {
            self.shutdown();
        }
    }
}

fn accept_loop(listener: &UnixListener, tx: &Sender<RawNotification>, stop: &AtomicBool) {
    let mut seq = 0_u64;

    loop {
        let (mut stream, _) = match listener.accept() {
            Ok(accepted) => accepted,
            Err(_) if stop.load(Ordering::SeqCst) => return,
            Err(_) => {
                std::thread::sleep(Duration::from_millis(10));
                continue;
            }
        };

        // stamp before anything else: this is the number that matters
        let recv_mach_ns = clock::mono_now_ns();
        let recv_wall_us = clock::wall_now_us();

        if stop.load(Ordering::SeqCst) {
            return;
        }

        let _ = stream.set_read_timeout(Some(Duration::from_secs(2)));
        let mut bytes = Vec::with_capacity(64 * 1024);
        let _ = stream.read_to_end(&mut bytes);
        if bytes.is_empty() {
            continue;
        }

        seq += 1;
        if tx
            .send(RawNotification {
                seq,
                recv_wall_us,
                recv_mach_ns,
                bytes,
            })
            .is_err()
        {
            return;
        }
    }
}

fn write_loop(
    rx: &Receiver<RawNotification>,
    events_file: File,
    states_dir: &Path,
    counters: &EventCounters,
    discovery: Option<&Sender<Discovery>>,
) {
    let mut events = BufWriter::new(events_file);
    let mut previous_state: Option<Value> = None;
    let mut state_id = 0_u64;

    while let Ok(raw) = rx.recv() {
        counters.events.fetch_add(1, Ordering::Relaxed);
        counters
            .bytes
            .fetch_add(raw.bytes.len() as u64, Ordering::Relaxed);

        let row = match serde_json::from_slice::<Value>(&raw.bytes) {
            Ok(mut value) => {
                let event = value.get_mut("event").map(Value::take);
                let state = value.get_mut("state").map(Value::take);

                if let Some(state) = state
                    && previous_state.as_ref() != Some(&state)
                {
                    state_id += 1;
                    write_state(states_dir, state_id, &state);
                    counters.states.fetch_add(1, Ordering::Relaxed);

                    if let Some(discovery) = discovery {
                        let _ = discovery.send(Discovery::State(Arc::new(state.clone())));
                    }

                    previous_state = Some(state);
                }

                EventRow {
                    seq: raw.seq,
                    recv_wall_us: raw.recv_wall_us,
                    recv_mach_ns: raw.recv_mach_ns,
                    event,
                    state_id: (state_id > 0).then_some(state_id),
                    parse_error: None,
                    raw: None,
                }
            }
            Err(error) => {
                counters.parse_errors.fetch_add(1, Ordering::Relaxed);
                EventRow {
                    seq: raw.seq,
                    recv_wall_us: raw.recv_wall_us,
                    recv_mach_ns: raw.recv_mach_ns,
                    event: None,
                    state_id: (state_id > 0).then_some(state_id),
                    parse_error: Some(error.to_string()),
                    raw: Some(String::from_utf8_lossy(&raw.bytes).into_owned()),
                }
            }
        };

        if let Ok(line) = serde_json::to_string(&row) {
            let _ = writeln!(events, "{line}");
        }

        // events are sparse; keep the file current so a crash loses nothing
        if rx.is_empty() {
            let _ = events.flush();
        }
    }

    let _ = events.flush();
}

fn write_state(states_dir: &Path, id: u64, state: &Value) {
    let path = states_dir.join(format!("{id:06}.json"));
    if let Ok(text) = serde_json::to_string(state) {
        let _ = std::fs::write(path, text);
    }
}

/// Asks komorebi for its current `State`, writes the reply to `path` as komorebi sent it, and
/// returns the parsed value.
///
/// An empty reply is asked for again a few times (see [`crate::komorebi_link::query_state`]). The
/// file is written only after the reply has been parsed, so a reply that is not JSON leaves no
/// file behind. This is how the recorder writes `state-start.json` and `state-end.json`.
///
/// This talks to a running komorebi, so it has no doctest. The query and its retries are covered
/// by `an_empty_state_reply_is_asked_again` in [`crate::komorebi_link`]'s unit tests.
///
/// # Errors
///
/// Returns an error if
///
/// - komorebi cannot be reached (it is not running, or its socket cannot be read),
/// - komorebi answers with nothing every time it is asked (on komorebi for Windows it does that
///   while it is busy, paused, or showing another virtual desktop),
/// - the reply is not valid JSON, or
/// - `path` cannot be written.
///
/// Asking again later is the workaround for the first two.
pub fn snapshot_state(path: &Path) -> eyre::Result<Value> {
    let response = komorebi_link::query_state().map_err(|error| eyre::Report::new(error))?;
    let value: Value =
        serde_json::from_str(&response).wrap_err("komorebi returned invalid state JSON")?;
    std::fs::write(path, &response)?;
    Ok(value)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn state_ids_map_to_ordered_file_names() {
        assert_eq!(state_file_name(1), "states/000001.json");
        assert_eq!(state_file_name(123_456), "states/123456.json");
    }

    #[test]
    fn event_rows_roundtrip_and_skip_empty_fields() {
        let row = EventRow {
            seq: 3,
            recv_wall_us: 10,
            recv_mach_ns: 20,
            event: Some(serde_json::json!({"type": "Show"})),
            state_id: Some(2),
            parse_error: None,
            raw: None,
        };
        let text = serde_json::to_string(&row).expect("serialize");
        assert!(!text.contains("parse_error"));
        let back: EventRow = serde_json::from_str(&text).expect("parse");
        assert_eq!(back.seq, 3);
        assert_eq!(back.state_id, Some(2));
    }

    #[test]
    fn the_writer_dedupes_unchanged_states_and_tolerates_garbage() {
        let dir =
            std::env::temp_dir().join(format!("komorebi-recorder-events-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("states")).expect("dir");

        let (tx, rx) = crossbeam_channel::unbounded();
        let notification = |seq: u64, state: &str| RawNotification {
            seq,
            recv_wall_us: seq as i64,
            recv_mach_ns: seq * 1000,
            bytes: format!(r#"{{"event":{{"type":"Show","content":[null,1]}},"state":{state}}}"#)
                .into_bytes(),
        };

        tx.send(notification(1, r#"{"a":1}"#)).expect("send");
        tx.send(notification(2, r#"{"a":1}"#)).expect("send");
        tx.send(notification(3, r#"{"a":2}"#)).expect("send");
        tx.send(RawNotification {
            seq: 4,
            recv_wall_us: 4,
            recv_mach_ns: 4000,
            bytes: b"not json".to_vec(),
        })
        .expect("send");
        drop(tx);

        let counters = EventCounters::default();
        write_loop(
            &rx,
            File::create(dir.join("events.jsonl")).expect("file"),
            &dir.join("states"),
            &counters,
            None,
        );

        assert_eq!(counters.events.load(Ordering::Relaxed), 4);
        assert_eq!(
            counters.states.load(Ordering::Relaxed),
            2,
            "an unchanged state is not stored twice"
        );
        assert_eq!(counters.parse_errors.load(Ordering::Relaxed), 1);

        let rows: Vec<EventRow> = std::fs::read_to_string(dir.join("events.jsonl"))
            .expect("events")
            .lines()
            .map(|line| serde_json::from_str(line).expect("row"))
            .collect();
        assert_eq!(
            rows.iter().map(|row| row.state_id).collect::<Vec<_>>(),
            vec![Some(1), Some(1), Some(2), Some(2)]
        );
        assert!(rows[3].parse_error.is_some());
        assert_eq!(rows[3].raw.as_deref(), Some("not json"));

        assert!(dir.join("states/000001.json").exists());
        assert!(dir.join("states/000002.json").exists());
        assert!(!dir.join("states/000003.json").exists());
    }
}
