//! The non-video capture sources: what komorebi and the operating system said while a recording
//! ran.
//!
//! Each source writes its own raw files into the session directory and never rewrites them
//! afterwards. The merged index over them (`timeline.jsonl`) only points into these files, so the
//! file names and record formats written here are part of the session format: a session recorded
//! by any build is read by every other, and a session recorded on one host is investigated on the
//! other.
//!
//! | module | files it writes | recorded on |
//! |---|---|---|
//! | [`komorebi_events`] | `events.jsonl` (one JSON object per line), `states/<id>.json` | both hosts |
//! | `komorebi_log` | `komorebi.log` (a byte-for-byte slice of komorebi's own log) | both hosts |
//! | `oslog` | `oslog/*.ndjson`, one `log stream` per seam (komorebi, the system, each managed app) | macOS |
//! | `wintrace` | `oslog/system.ndjson` (`WinEvents`) and `oslog/eventlog.ndjson` (Event Log) | Windows |
//! | `winprobe` | no file of its own: a `probe` key on rows of `events.jsonl` | Windows, with `--probe-windows` |
//! | `komorebi_version` | no file: asks komorebi's binary what it is, for the manifest | both hosts |
//! | `managed_apps` | no file: decides which processes get a trace of their own | macOS |
//!
//! The types that a source reports about itself to the manifest (`session.json`) are defined
//! beside it: `EventSummary`, `LogSegment`, `StreamInfo`, `KomorebiVersion` and `AppRecord`.
//!
//! # What this layer does not do
//!
//! - It does not understand komorebi's vocabulary. A source copies bytes and stamps times; reading
//!   an event or a log line is the job of [`crate::analysis`], through the rules of the
//!   [`Flavor`](crate::analysis::flavor::Flavor) that the manifest records.
//! - It does not index or merge. `timeline` builds the index when a recording ends, and `reindex`
//!   can rebuild it from these files alone.
//! - It does not decide when to start or stop. The `recorder` starts every source, checkpoints
//!   their counters into the manifest and stops them in order.
//!
//! # Clocks
//!
//! A source stamps what it receives on the master clock ([`crate::clock`]) wherever komorebi's own
//! text carries no time of its own. The `*_mach_ns` names in the records keep their historical
//! spelling: they hold nanoseconds on the host's master monotonic clock, and the manifest's
//! `host.clock` says which clock that was.

pub mod komorebi_events;
pub mod komorebi_log;
pub mod komorebi_version;
pub mod managed_apps;
pub mod oslog;
pub mod winprobe;
pub mod wintrace;
