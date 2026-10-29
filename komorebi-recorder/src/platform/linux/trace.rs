//! The OS trace on Linux: the journal, as it is written.
//!
//! The compositor and the shell log to the journal, and so do the things that go wrong around a
//! window manager (a GPU that resets, a PipeWire that restarts, the portal). Two `journalctl`
//! children follow it from now on, as `-o json`, and each entry becomes a record of the same shape
//! as the Mac's unified log and the Windows trace ([`TraceRecord`]):
//!
//! | stream | what it follows | in the manifest |
//! |---|---|---|
//! | `system` | the user's journal, of the programs that put windows on the screen: `kwin_wayland`, `plasmashell`, `komorebi` | `journal` |
//! | `event_log` | every priority of warning and worse, from the system journal and the user's | `journal-warnings` |
//!
//! The time of an entry is `__MONOTONIC_TIMESTAMP`, which is the clock the master clock reads
//! (`CLOCK_MONOTONIC`) when the entry was written, so it is on the master clock as it is. A user
//! who may not read the system journal (not in `systemd-journal`) has the user's only, and the
//! reason is in the problems.
//!
//! The children end when the taps are dropped.

use crate::platform::process;
use crate::sources::wintrace::TraceRecord;
use crate::sources::wintrace::TraceStream;
use serde_json::Value;
use std::io::BufRead;
use std::io::BufReader;
use std::process::Child;
use std::process::Command;
use std::process::Stdio;
use std::sync::Arc;
use std::thread::JoinHandle;

/// What the manifest calls the stream of the programs that put windows on the screen.
pub const SYSTEM_NAME: &str = "journal";
/// What the manifest calls the stream of warnings.
pub const EVENT_LOG_NAME: &str = "journal-warnings";
/// What the first stream is, for the manifest.
pub const SYSTEM_ABOUT: &str = "journalctl --user -f: kwin_wayland, plasmashell and komorebi";
/// What the second stream is, for the manifest.
pub const EVENT_LOG_ABOUT: &str = "journalctl -f -p warning: the system's warnings and errors";

/// The running taps, which listen until dropped.
pub struct Taps {
    children: Vec<Child>,
    readers: Vec<JoinHandle<()>>,
}

impl Drop for Taps {
    fn drop(&mut self) {
        for child in &mut self.children {
            process::terminate(child);
        }
        for child in &mut self.children {
            let _ = child.wait();
        }
        // the pipes are closed with the children, which ends each reader
        for reader in self.readers.drain(..) {
            let _ = reader.join();
        }
    }
}

/// How a journal priority is named in a record: the names the Mac's records use.
fn level_of(priority: &str) -> &'static str {
    match priority {
        "0" | "1" | "2" => "Fault",
        "3" => "Error",
        "7" => "Debug",
        _ => "Info",
    }
}

fn text<'a>(entry: &'a Value, key: &str) -> Option<&'a str> {
    entry.get(key).and_then(Value::as_str)
}

/// Returns the record that one line of `journalctl -o json` is, or `None` for a line that is not
/// an entry, or has no time on the master clock, or has a message that is not text.
///
#[must_use]
pub fn record_of(line: &str) -> Option<TraceRecord> {
    let entry: Value = serde_json::from_str(line).ok()?;
    let host_us: u64 = text(&entry, "__MONOTONIC_TIMESTAMP")?.parse().ok()?;
    let wall_us: i64 = text(&entry, "__REALTIME_TIMESTAMP")?.parse().ok()?;
    // a message that is not text (a binary one) is an array of bytes, which says nothing here
    let message = text(&entry, "MESSAGE")?.to_string();
    let process = text(&entry, "_COMM")
        .or_else(|| text(&entry, "SYSLOG_IDENTIFIER"))
        .unwrap_or_default();
    Some(TraceRecord {
        host_ns: host_us.saturating_mul(1000),
        wall_us,
        pid: text(&entry, "_PID")
            .and_then(|pid| pid.parse().ok())
            .unwrap_or(0),
        process: process.to_string(),
        subsystem: "journal".to_string(),
        category: text(&entry, "SYSLOG_IDENTIFIER")
            .or_else(|| text(&entry, "_SYSTEMD_UNIT"))
            .unwrap_or_default()
            .to_string(),
        level: level_of(text(&entry, "PRIORITY").unwrap_or("6")),
        message,
    })
}

/// Follows the journal with `arguments`, writing each entry to `stream`.
fn follow(arguments: &[&str], stream: Arc<TraceStream>) -> Result<(Child, JoinHandle<()>), String> {
    let mut command = Command::new("journalctl");
    command
        .args(["-f", "-n", "0", "-o", "json", "--no-pager"])
        .args(arguments)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null());
    let mut child = process::isolate(&mut command)
        .spawn()
        .map_err(|error| format!("could not run journalctl: {error}"))?;
    let stdout = child
        .stdout
        .take()
        .ok_or_else(|| "journalctl has no output".to_string())?;
    let reader = std::thread::Builder::new()
        .name("komorebi-recorder-journal".to_string())
        .spawn(move || {
            for line in BufReader::new(stdout).lines().map_while(Result::ok) {
                if let Some(record) = record_of(&line) {
                    stream.write(&record);
                }
            }
        })
        .map_err(|error| format!("could not start the reader: {error}"))?;
    Ok((child, reader))
}

/// Starts following the journal, into `system` and `event_log`.
///
/// Returns the taps, or `None` if neither could start, and why each that could not start could
/// not, in words that begin with the stream's name; the recording goes on without it.
#[must_use]
pub fn start(system: Arc<TraceStream>, event_log: Arc<TraceStream>) -> (Option<Taps>, Vec<String>) {
    let mut taps = Taps {
        children: Vec::new(),
        readers: Vec::new(),
    };
    let mut problems = Vec::new();

    let wanted = [
        (
            SYSTEM_NAME,
            system,
            &[
                "--user",
                "_COMM=kwin_wayland",
                "+",
                "_COMM=plasmashell",
                "+",
                "_COMM=komorebi",
            ][..],
        ),
        (EVENT_LOG_NAME, event_log, &["-p", "warning"][..]),
    ];
    for (name, stream, arguments) in wanted {
        match follow(arguments, stream) {
            Ok((child, reader)) => {
                taps.children.push(child);
                taps.readers.push(reader);
            }
            Err(why) => problems.push(format!("{name}: {why}")),
        }
    }

    ((!taps.children.is_empty()).then_some(taps), problems)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_entry_is_a_record_on_the_master_clock() {
        let line = r#"{"__MONOTONIC_TIMESTAMP":"5000000","__REALTIME_TIMESTAMP":"1793016000123456","_PID":"812","_COMM":"kwin_wayland","SYSLOG_IDENTIFIER":"kwin_wayland","PRIORITY":"4","MESSAGE":"Atom 0 not found"}"#;
        let record = record_of(line).expect("an entry");
        assert_eq!(record.host_ns, 5_000_000_000);
        assert_eq!((record.pid, record.process.as_str()), (812, "kwin_wayland"));
        assert_eq!(record.level, "Info");
        assert_eq!(record.message, "Atom 0 not found");
        assert!(record_of("not json").is_none());
    }

    #[test]
    fn priorities_are_named_as_the_macs_records_are() {
        assert_eq!(level_of("0"), "Fault");
        assert_eq!(level_of("3"), "Error");
        assert_eq!(level_of("4"), "Info");
        assert_eq!(level_of("6"), "Info");
        assert_eq!(level_of("7"), "Debug");
    }

    #[test]
    fn an_entry_with_a_binary_message_is_left_out() {
        let line =
            r#"{"__MONOTONIC_TIMESTAMP":"1","__REALTIME_TIMESTAMP":"2","MESSAGE":[104,105]}"#;
        assert!(record_of(line).is_none());
    }

    #[test]
    fn an_entry_without_a_process_is_named_by_its_identifier() {
        let line = r#"{"__MONOTONIC_TIMESTAMP":"1000","__REALTIME_TIMESTAMP":"2","SYSLOG_IDENTIFIER":"pipewire","MESSAGE":"restarted"}"#;
        let record = record_of(line).expect("an entry");
        assert_eq!(record.process, "pipewire");
        assert_eq!(record.category, "pipewire");
        assert_eq!(record.pid, 0);
        assert_eq!(record.host_ns, 1_000_000);
    }
}
