//! The on-disk session directory and its manifest (`session.json`).
//!
//! Raw data is never duplicated or rewritten: `timeline.jsonl` only *references* byte ranges in
//! the raw files, so streams stay separate and can be reconciled or re-indexed later.

use crate::analysis::flavor::Flavor;
use crate::clock::Anchor;
use crate::clock::Timebase;
use crate::platform::Host;
use crate::sources::komorebi_events::EventSummary;
use crate::sources::komorebi_log::DetectedLevel;
use crate::sources::komorebi_log::LogSegment;
use crate::sources::komorebi_version::KomorebiVersion;
use crate::sources::managed_apps::AppRecord;
use crate::sources::oslog::StreamInfo;
use chrono::DateTime;
use chrono::Utc;
use color_eyre::eyre;
use color_eyre::eyre::WrapErr;
use serde::Deserialize;
use serde::Serialize;
#[cfg(test)]
use serde_json::Value;
use std::path::Path;
use std::path::PathBuf;

/// The version of the manifest format this build writes and the newest it reads. [`Session::manifest`] refuses a
/// manifest with a greater one.
pub const SCHEMA_VERSION: u32 = 1;

/// The recorder is (or died while) recording; the manifest is a checkpoint.
pub const STATUS_RECORDING: &str = "recording";
/// Stopped normally and fully indexed.
pub const STATUS_COMPLETE: &str = "complete";
/// The recorder died; `reindex` rebuilt the session from what was on disk.
pub const STATUS_RECOVERED: &str = "recovered";

fn complete_status() -> String {
    STATUS_COMPLETE.to_string()
}
/// The name of the manifest in a session's directory.
pub const MANIFEST_FILE: &str = "session.json";
/// The name of the time-sorted index of every record in a session. It points into the raw files, which it never
/// replaces, so it can be rebuilt from them.
pub const TIMELINE_FILE: &str = "timeline.jsonl";

/// A point in time on both clocks.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Moment {
    /// Nanoseconds on the master clock.
    ///
    pub mach_ns: u64,
    /// Microseconds since the Unix epoch on the wall clock, read together with `mach_ns`.
    pub wall_us: i64,
}

impl Moment {
    #[must_use]
    /// Returns the current time on both clocks, read together as one [`Anchor`].
    pub fn now() -> Self {
        let anchor = Anchor::sample();
        Self {
            mach_ns: anchor.mach_ns,
            wall_us: anchor.wall_us,
        }
    }
}

/// What the recorder knew about komorebi: which one it was, whether and as which process it ran, and how it was
/// logging.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct KomorebiInfo {
    /// Which komorebi this was: they differ in what their events and log say. Sessions from
    /// before this was recorded were all of komorebi for Mac.
    #[serde(default)]
    pub flavor: Flavor,
    /// Whether a komorebi process was found, when the recording started or later in it.
    pub running: bool,
    /// The komorebi process seen most recently, if any.
    pub pid: Option<i32>,
    /// Every komorebi process seen during the recording, in order (it can be restarted).
    #[serde(default)]
    pub pids: Vec<i32>,
    /// The log level komorebi was running at, and how that was found out, for the process in `pid`.
    pub log_level: Option<DetectedLevel>,
    /// Which komorebi each of `pids` was, as its binary reported: what a log line means depends on
    /// the source it was built from.
    #[serde(default)]
    pub versions: Vec<KomorebiVersion>,
    /// The name this recorder registered as with komorebi; komorebi's DEBUG log names it when it
    /// pushes each notification, which ties an event to its handler exactly.
    #[serde(default)]
    pub subscriber: Option<String>,
    /// Levels that actually appear in the recorded slice of komorebi's log.
    pub observed_levels: Vec<String>,
    /// komorebi's data directory, where its sockets are.
    pub data_dir: String,
    /// The directory komorebi's plaintext log was read from, where that is not `data_dir` (komorebi
    /// for Windows logs into the temp directory).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub log_dir: Option<String>,
}

/// The outcome of the self-test that checks the video against the clocks.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Calibration {
    /// How the readings were taken.
    pub method: String,
    /// The readings the result is computed from.
    pub samples: Vec<CalibrationSample>,
    /// The largest absolute error among the samples, in milliseconds.
    pub max_abs_error_ms: f64,
    /// The largest error that still passes, in milliseconds.
    pub tolerance_ms: f64,
    /// Whether there were samples and `max_abs_error_ms` is within `tolerance_ms`.
    pub passed: bool,
}

/// One reading of the on-screen clock: where in the video it was taken, and what the clock showed against what the
/// session predicted.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CalibrationSample {
    /// Where in the video the reading was taken, in seconds.
    pub video_s: f64,
    /// The master-clock time the session predicts for that moment, at millisecond resolution.
    pub predicted_mach_ns: u64,
    /// The master-clock time the on-screen clock showed in that frame, at millisecond resolution.
    pub observed_mach_ns: u64,
    /// Observed minus predicted, in milliseconds, wrapped to the period of the on-screen clock.
    pub error_ms: f64,
}

/// The manifest of a session (`session.json`): everything the other files in the directory need to be read, and the
/// summary of what the recording was.
///
/// A manifest is written when a recording starts and rewritten at checkpoints while it runs, so one with `status`
/// [`STATUS_RECORDING`] is either a recording in progress or one whose recorder died. [`Manifest::host`] says what it was
/// recorded on, and `komorebi.flavor` which komorebi it recorded.
///
/// # Example
///
/// ```rust
/// use komorebi_recorder::clock::Anchor;
/// use komorebi_recorder::session::Manifest;
/// use komorebi_recorder::session::Moment;
///
/// let anchor = Anchor::sample();
/// let start = Moment { mach_ns: anchor.mach_ns, wall_us: anchor.wall_us };
/// let manifest = Manifest::new(start, vec![anchor]);
///
/// // the first frame was captured a second after the recording started
/// let mut manifest = manifest;
/// manifest.video_t0_mach_ns = Some(start.mach_ns + 1_000_000_000);
/// assert_eq!(manifest.video_seconds(start.mach_ns + 3_500_000_000), 2.5);
/// assert_eq!(manifest.mach_at_video_seconds(2.5), start.mach_ns + 3_500_000_000);
/// ```
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Manifest {
    /// The version of this format ([`SCHEMA_VERSION`]) the manifest was written with.
    pub schema_version: u32,
    /// `recording`, `complete` or `recovered`.
    #[serde(default = "complete_status")]
    pub status: String,
    /// The version of `komorebi-recorder` that wrote the session.
    pub recorder_version: String,
    /// What the session was recorded on. Absent in sessions from before Windows support, which
    /// were all recorded on a Mac: use [`Manifest::host`], which knows that.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub host: Option<Host>,
    /// The macOS version of a Mac host. Superseded by `host.os_version`; still written on a Mac so
    /// that older builds of the tool read the session, and read from sessions that only have it.
    #[serde(default)]
    pub macos_version: Option<String>,
    /// The komorebi that was recorded.
    pub komorebi: KomorebiInfo,
    /// What one tick of the host clock is worth (`numer / denom` nanoseconds).
    pub timebase: Timebase,
    /// Paired readings of the wall and master clocks taken while recording, which map komorebi's log lines onto the
    /// master clock (see [`crate::clock::ClockMap`]).
    pub anchors: Vec<Anchor>,
    /// When the recording started.
    pub start: Moment,
    /// When the recording stopped, if it did: `None` while it runs and in a session whose recorder died before
    /// `reindex` recovered it.
    pub stop: Option<Moment>,
    /// Session video t=0 on the mach clock: the earliest first frame across displays.
    pub video_t0_mach_ns: Option<u64>,
    /// When video capture was stopped on the mach clock.
    pub video_end_mach_ns: Option<u64>,
    /// The byte ranges of komorebi's log that were recorded, which together make `komorebi.log`.
    pub log_segments: Vec<LogSegment>,
    /// The OS trace streams that were recorded, one per file under `oslog/`.
    pub oslog: Vec<StreamInfo>,
    /// The applications that own windows komorebi manages, which each get an OS trace stream of their own on a Mac.
    pub apps: Vec<AppRecord>,
    /// What was recorded of komorebi's subscriber events.
    pub events: EventSummary,
    /// `host-clock` when frame timestamps were verified to be on the mach clock.
    pub sync_quality: String,
    /// The result of `selftest`, if it was run on this session.
    pub calibration: Option<Calibration>,
    /// Things the recorder wants the reader to know about the recording, in words.
    pub warnings: Vec<String>,
}

impl Manifest {
    #[must_use]
    /// Returns the manifest of a recording that has just started, on this host: its host and komorebi flavor are the
    /// ones this process runs on, its status is [`STATUS_RECORDING`] and it has nothing recorded yet.
    pub fn new(start: Moment, anchors: Vec<Anchor>) -> Self {
        let host = Host::current();
        Self {
            schema_version: SCHEMA_VERSION,
            status: STATUS_RECORDING.to_string(),
            recorder_version: env!("CARGO_PKG_VERSION").to_string(),
            macos_version: host.is_macos().then(|| host.os_version.clone()).flatten(),
            host: Some(host),
            komorebi: KomorebiInfo {
                flavor: Flavor::of_host(),
                ..KomorebiInfo::default()
            },
            timebase: crate::clock::timebase(),
            anchors,
            start,
            stop: None,
            video_t0_mach_ns: None,
            video_end_mach_ns: None,
            log_segments: Vec::new(),
            oslog: Vec::new(),
            apps: Vec::new(),
            events: EventSummary::default(),
            sync_quality: "host-clock".to_string(),
            calibration: None,
            warnings: Vec::new(),
        }
    }

    /// What this session was recorded on.
    #[must_use]
    pub fn host(&self) -> Host {
        self.host
            .clone()
            .unwrap_or_else(|| Host::legacy_macos(self.macos_version.clone()))
    }

    /// The master clock value that is `t = 0` in video time. Falls back to the recording start if
    /// no frame was ever captured, so timelines remain usable for the non-video sources.
    #[must_use]
    pub fn video_t0(&self) -> u64 {
        self.video_t0_mach_ns.unwrap_or(self.start.mach_ns)
    }

    #[must_use]
    /// Returns the video time, in seconds, of a master-clock time. It is negative before the first frame.
    pub fn video_seconds(&self, mach_ns: u64) -> f64 {
        (i128::from(mach_ns) - i128::from(self.video_t0())) as f64 / 1e9
    }

    #[must_use]
    /// Returns the master-clock time of `video_s` seconds into the video, which is zero for a time before the clock
    /// started.
    pub fn mach_at_video_seconds(&self, video_s: f64) -> u64 {
        let ns = i128::from(self.video_t0()) + (video_s * 1e9).round() as i128;
        ns.max(0) as u64
    }

    /// The span of video time covered by the recording, `(0, duration)`. The end is the moment
    /// video capture stopped (frames are variable-rate, so the last frame can be earlier).
    #[must_use]
    pub fn video_span(&self) -> (f64, f64) {
        let end = self
            .video_end_mach_ns
            .or(self.stop.map(|stop| stop.mach_ns))
            .map_or(0.0, |end| self.video_seconds(end));

        (0.0, end.max(0.0))
    }
}

/// A session directory on disk.
#[derive(Debug, Clone)]
pub struct Session {
    /// The directory the session is in.
    pub dir: PathBuf,
}

/// The default parent of session directories: komorebi's data dir, under `recordings/`.
#[must_use]
pub fn default_recordings_dir() -> PathBuf {
    crate::komorebi_link::data_dir().join("recordings")
}

impl Session {
    /// Creates a fresh session directory: `base` if given, otherwise
    /// `<komorebi data dir>/recordings/<YYYYmmddTHHMMSSZ>/`.
    ///
    /// # Errors
    ///
    /// Returns an error if the directory already contains a session, or cannot be created.
    pub fn create(base: Option<PathBuf>, now: DateTime<Utc>) -> eyre::Result<Self> {
        let dir = base.unwrap_or_else(|| {
            default_recordings_dir().join(now.format("%Y%m%dT%H%M%SZ").to_string())
        });

        if dir.join(MANIFEST_FILE).exists() {
            eyre::bail!(
                "{} already contains a session; choose a different --out",
                dir.display()
            );
        }

        for sub in ["oslog/apps", "states"] {
            std::fs::create_dir_all(dir.join(sub))
                .wrap_err_with(|| format!("could not create {}", dir.join(sub).display()))?;
        }

        Ok(Self { dir })
    }

    /// Opens the session in `dir`.
    ///
    /// # Errors
    ///
    /// Returns an error if `dir` has no manifest: it is not a session.
    pub fn open(dir: impl Into<PathBuf>) -> eyre::Result<Self> {
        let dir = dir.into();
        if !dir.join(MANIFEST_FILE).is_file() {
            eyre::bail!(
                "{} is not a recording session (no {MANIFEST_FILE})",
                dir.display()
            );
        }

        Ok(Self { dir })
    }

    #[must_use]
    /// Returns `relative` inside the session's directory.
    pub fn path(&self, relative: impl AsRef<Path>) -> PathBuf {
        self.dir.join(relative)
    }

    /// Reads the session's manifest.
    ///
    /// # Errors
    ///
    /// Returns an error if the manifest cannot be read, is not a valid manifest, or was written by a newer
    /// `komorebi-recorder` than this one ([`SCHEMA_VERSION`] is greater than what this build reads).
    pub fn manifest(&self) -> eyre::Result<Manifest> {
        let path = self.path(MANIFEST_FILE);
        let text = std::fs::read_to_string(&path)
            .wrap_err_with(|| format!("could not read {}", path.display()))?;
        let manifest: Manifest = serde_json::from_str(&text)
            .wrap_err_with(|| format!("{} is not a valid session manifest", path.display()))?;

        if manifest.schema_version > SCHEMA_VERSION {
            eyre::bail!(
                "{} was written by a newer komorebi-recorder (schema {}, this build reads up to {})",
                path.display(),
                manifest.schema_version,
                SCHEMA_VERSION
            );
        }

        Ok(manifest)
    }

    /// Writes the manifest. It is written to a temporary file and renamed over the old one, so a reader or a crash never
    /// sees a half-written manifest.
    ///
    /// # Errors
    ///
    /// Returns an error if the manifest cannot be serialized or the files cannot be written.
    pub fn write_manifest(&self, manifest: &Manifest) -> eyre::Result<()> {
        let path = self.path(MANIFEST_FILE);
        let temporary = self.path(format!("{MANIFEST_FILE}.tmp"));
        std::fs::write(&temporary, serde_json::to_string_pretty(manifest)?)?;
        // rename so a reader (or a crash) never sees a half-written manifest
        std::fs::rename(&temporary, &path)?;
        Ok(())
    }

    /// Writes the generated `README.md` for this session (see [`readme`]).
    ///
    /// # Errors
    ///
    /// Returns an error if the file cannot be written.
    pub fn write_readme(&self, manifest: &Manifest) -> eyre::Result<()> {
        std::fs::write(self.path("README.md"), readme(&self.dir, manifest))?;
        Ok(())
    }
}

/// The generated `README.md`: what every file is and the exact commands to query this session.
#[must_use]
pub fn readme(dir: &Path, manifest: &Manifest) -> String {
    let session = shell_quote(&dir.display().to_string());
    let host = manifest.host();
    let (_, duration) = manifest.video_span();
    let mut out = String::new();

    out.push_str("# komorebi-recorder session\n\n");
    out.push_str(&format!(
        "Recorded with komorebi-recorder {} on {}.\nVideo duration: about {:.1}s. Video time `00:00.000` is the first captured frame.\n\n",
        manifest.recorder_version,
        manifest.host().describe(),
        duration
    ));

    out.push_str("## Investigating a bug\n\n");
    out.push_str(&format!(
        "Start with what the user marked and described, then what the recording shows around it:\n\n\
         ```sh\n\
         komorebi-recorder brief {session}\n\
         komorebi-recorder guide        # how to investigate, step by step\n\
         komorebi-recorder view {session}   # the same, in a window you can play and scrub\n\
         ```\n\n\
         Every command takes `--json`. `annotate` adds a bug region (a start, an end and what is wrong)\n\
         to `annotations.jsonl`; `state`, `diff`, `events`, `trace`, `window`, `code`, `search`,\n\
         `changes` and `sheet` dig in.\n\n"
    ));

    out.push_str("## How to read it\n\n");
    out.push_str(
        "Every second of video maps to the exact records from every other source. Use the\n\
         `komorebi-recorder` CLI from this binary (this directory is self-contained):\n\n",
    );
    out.push_str(&format!(
        "```sh\n\
         # what happened around 12.5s into the video? (komorebi log + events + OS trace, +-1s)\n\
         komorebi-recorder at {session} 12.5 --window 1s\n\
         # narrow to one app's OS trace, or one source\n\
         komorebi-recorder at {session} 00:12.500 --app Ghostty\n\
         komorebi-recorder at {session} 12.5 --source events\n\
         # a span\n\
         komorebi-recorder range {session} 10 15\n\
         # a per-second digest of the whole recording\n\
         komorebi-recorder summary {session}\n\
         # reverse: a log line / event / UTC time -> video timecode\n\
         komorebi-recorder locate {session} --log-line 120\n\
         komorebi-recorder locate {session} --event-seq 7\n\
         komorebi-recorder locate {session} --utc 2026-10-26T19:43:35.062613Z\n\
         # extract PNG frames to look at (needs ffmpeg)\n\
         komorebi-recorder frames {session} 12.5 14.0\n\
         # rebuild timeline.jsonl from the raw files\n\
         komorebi-recorder reindex {session}\n\
         ```\n\n\
         Add `--json` to any query for machine-readable output.\n\n"
    ));

    out.push_str("## Files\n\n");
    out.push_str("| File | What it is |\n|---|---|\n");
    out.push_str("| `session.json` | Manifest: versions, displays, clock anchors, log segments, OS-trace streams and apps, warnings |\n");
    out.push_str("| `video-<displayID>[-<n>].mp4` | Screen recording of one display. Variable frame rate: frames only exist when the screen changed. A display that was unplugged, whose stream stopped, or that changed resolution has several numbered segments, each a complete file; `session.json` lists every segment with its time span |\n");
    out.push_str("| `frames-<displayID>[-<n>].csv` | Every frame's host-clock timestamp (`pts_mach_ns`) and when it was received |\n");
    out.push_str("| `annotations.jsonl` | Bug regions (a start, an end and what is wrong), notes and findings, appended by the viewer and `annotate`. Never rewritten |\n");
    out.push_str("| `recorder.jsonl` | What happened to the recording environment: displays connected/disconnected, sleep/wake, screen lock, restarts of log streams or komorebi. Shown as the `recorder` source |\n");
    out.push_str("| `komorebi.log` | The exact byte slice of komorebi's own log for this recording (UTC timestamps) |\n");
    out.push_str("| `events.jsonl` | Every komorebi subscriber notification: `{seq, recv_wall_us, recv_mach_ns, event, state_id}` |\n");
    out.push_str("| `states/<id>.json` | Full komorebi `State`, stored only when it changed (referenced by `state_id`) |\n");
    out.push_str(
        "| `state-start.json`, `state-end.json` | `State` when recording started / stopped |\n",
    );
    out.push_str(os_trace_rows(&host));
    out.push_str("| `timeline.jsonl` | Merged index of every record, sorted by time, pointing into the raw files |\n\n");

    out.push_str("## Gaps are expected\n\n");
    out.push_str(
        "Docking, undocking, sleep, wake and screen lock change what can be recorded. The recorder keeps\n\
         going through all of them: a display that disappears has its video finalized on the spot and a new\n\
         segment starts when it returns, and a `log stream` or komorebi that goes away is picked up again.\n\
         Whatever happened is in `recorder.jsonl` (`komorebi-recorder at <session> <time> --source recorder`).\n",
    );
    out.push_str(if host.is_windows() {
        "Whether the host clock counts the time a machine sleeps is measured, not assumed: `recorder.jsonl`\n\
         says for each sleep how long it was by the wall clock and how far the host clock moved.\n\n"
    } else {
        "While the machine sleeps the host clock pauses, so video time skips the sleep rather than showing\n\
         a gap.\n\n"
    });

    if manifest.status != STATUS_COMPLETE {
        out.push_str("## This session was recovered\n\n");
        out.push_str(
            "The recorder did not finish normally. Everything that had been written was kept (video is\n\
             fragmented so it plays up to the last fragment) and `reindex` rebuilt this session from it.\n\
             The end of the recording is approximate and a second or two of video may be missing.\n\n",
        );
    }

    out.push_str("## Clocks\n\n");
    out.push_str(if host.is_windows() {
        "The master clock is the host performance counter (`QueryPerformanceCounter`, ns). Video frame\n\
         timestamps are on it directly, and the OS trace's records carry it as `hostNs`."
    } else {
        "The master clock is the host monotonic clock (`mach_absolute_time`, ns). Video frame\n\
         timestamps and the unified log's `machTimestamp` are on it directly."
    });
    out.push_str(
        " komorebi's log lines\n\
         (UTC wall clock) are mapped onto it with the `anchors` in `session.json`. Subscriber events\n\
         are stamped on receipt, and komorebi sends them after handling the event, so a stamp is when\n\
         the handler *finished*; the start of the handler is in `komorebi.log` (`komorebi-recorder trace`\n\
         ties the two together).\n\n",
    );

    if !manifest.warnings.is_empty() {
        out.push_str("## Warnings recorded\n\n");
        for warning in &manifest.warnings {
            out.push_str(&format!("- {warning}\n"));
        }
        out.push('\n');
    }

    out.push_str("## Privacy\n\n");
    out.push_str(if host.is_windows() {
        "The video contains whatever was on screen, and komorebi's events, state and log contain window\n\
         titles and program names. Review this directory before sharing it.\n"
    } else {
        "The video contains whatever was on screen, and application logs can contain window titles\n\
         and URLs. Review this directory before sharing it. `<private>` values in the OS log are\n\
         redacted by macOS (no sudo or SIP changes are made by this tool).\n"
    });

    out
}

/// The OS trace files a session of this host has, as rows of the README's table of files.
fn os_trace_rows(host: &Host) -> &'static str {
    if host.is_windows() {
        "| `oslog/system.ndjson` | What the OS told komorebi: Windows' WinEvents for every top-level window (shown, hidden, cloaked, focused, moved, renamed, destroyed), repeated moves folded together |\n\
         | `oslog/eventlog.ndjson` | The System Event Log: warnings, errors, and the providers that talk about displays, power and the GPU |\n"
    } else {
        "| `oslog/komorebi.ndjson` | macOS unified log for the komorebi process (system frameworks as the daemon sees them) |\n\
         | `oslog/system.ndjson` | macOS unified log: accessibility / HIServices / workspace subsystems |\n\
         | `oslog/apps/<exe>-<pid>.ndjson` | macOS unified log for one managed application process |\n"
    }
}

/// `value` as one argument to a shell: left alone if it needs no quoting, else in the quotes this
/// machine's shells understand (single quotes in a unix shell and `PowerShell`, which cmd.exe does not
/// read, so double quotes where the host is Windows).
fn shell_quote(value: &str) -> String {
    quote_for(cfg!(windows), value)
}

fn quote_for(windows: bool, value: &str) -> String {
    let plain = |c: char| {
        c.is_ascii_alphanumeric()
            || matches!(c, '/' | '.' | '-' | '_' | ':' | '+')
            || (windows && c == '\\')
    };
    if value.chars().all(plain) {
        value.to_string()
    } else if windows {
        format!("\"{}\"", value.replace('"', "\\\""))
    } else {
        format!("'{}'", value.replace('\'', "'\\''"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;

    fn manifest() -> Manifest {
        let start = Moment {
            mach_ns: 1_000_000_000_000,
            wall_us: 1_800_000_000_000_000,
        };
        Manifest::new(
            start,
            vec![Anchor {
                wall_us: start.wall_us,
                mach_ns: start.mach_ns,
            }],
        )
    }

    #[test]
    fn paths_are_quoted_the_way_the_hosts_shell_reads_them() {
        assert_eq!(quote_for(false, "/tmp/a b"), "'/tmp/a b'");
        assert_eq!(quote_for(false, "it's"), r"'it'\''s'");
        assert_eq!(quote_for(false, "/tmp/plain"), "/tmp/plain");

        // cmd.exe does not read single quotes; both it and PowerShell read double ones
        assert_eq!(
            quote_for(true, r"C:\Users\me\komorebi"),
            r"C:\Users\me\komorebi"
        );
        assert_eq!(
            quote_for(true, r"C:\Users\Jane Doe\x"),
            r#""C:\Users\Jane Doe\x""#
        );
    }

    #[test]
    fn the_readme_describes_the_os_trace_the_hosts_session_has() {
        let mut manifest = manifest();
        manifest.host = Some(Host {
            os: "windows".to_string(),
            arch: "x86_64".to_string(),
            os_version: Some("10.0.26100".to_string()),
            clock: crate::platform::ClockKind::Qpc,
        });
        let text = readme(Path::new("/tmp/s"), &manifest);
        assert!(text.contains("oslog/eventlog.ndjson"));
        assert!(text.contains("QueryPerformanceCounter"));
        assert!(!text.contains("unified log"), "{text}");

        let mac = readme(Path::new("/tmp/s"), &self::tests::manifest());
        assert!(mac.contains("macOS unified log"));
    }

    #[test]
    fn a_manifest_from_before_hosts_were_recorded_was_recorded_on_a_mac() {
        let mut value = serde_json::to_value(manifest()).expect("serialize");
        let object = value.as_object_mut().expect("object");
        object.remove("host");
        object.insert("macos_version".into(), "26.0".into());

        let old: Manifest = serde_json::from_value(value).expect("an old manifest parses");
        let host = old.host();
        assert!(host.is_macos());
        assert_eq!(host.os_version.as_deref(), Some("26.0"));
        assert_eq!(host.clock, crate::platform::ClockKind::Mach);
    }

    #[test]
    fn a_new_manifest_records_the_host_it_was_made_on() {
        let host = manifest().host();
        assert_eq!(host.os, std::env::consts::OS);
        assert_eq!(host.clock, crate::platform::clock::KIND);
    }

    #[test]
    fn video_time_is_relative_to_the_first_frame() {
        let mut manifest = manifest();
        manifest.video_t0_mach_ns = Some(1_002_000_000_000);

        assert!((manifest.video_seconds(1_004_500_000_000) - 2.5).abs() < 1e-9);
        assert!((manifest.video_seconds(1_001_000_000_000) + 1.0).abs() < 1e-9);
        assert_eq!(manifest.mach_at_video_seconds(2.5), 1_004_500_000_000);
    }

    #[test]
    fn video_time_falls_back_to_the_recording_start() {
        let manifest = manifest();
        assert_eq!(manifest.video_t0(), 1_000_000_000_000);
    }

    #[test]
    fn manifests_roundtrip_through_json() {
        let manifest = manifest();
        let text = serde_json::to_string_pretty(&manifest).expect("serialize");
        let back: Manifest = serde_json::from_str(&text).expect("parse");
        assert_eq!(back, manifest);
    }

    #[test]
    fn creates_opens_and_refuses_to_overwrite_a_session() {
        let base =
            std::env::temp_dir().join(format!("komorebi-recorder-session-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&base);
        let now = Utc
            .with_ymd_and_hms(2026, 10, 26, 19, 43, 35)
            .single()
            .expect("time");

        let session = Session::create(Some(base.clone()), now).expect("create");
        assert!(session.path("oslog/apps").is_dir());
        assert!(session.path("states").is_dir());
        assert!(Session::open(&base).is_err(), "no manifest yet");

        session.write_manifest(&manifest()).expect("write");
        assert!(!session.path("session.json.tmp").exists());
        assert_eq!(
            Session::open(&base)
                .expect("open")
                .manifest()
                .expect("manifest"),
            manifest()
        );
        assert!(
            Session::create(Some(base), now).is_err(),
            "must not clobber a session"
        );
    }

    #[test]
    fn rejects_manifests_from_the_future() {
        let base =
            std::env::temp_dir().join(format!("komorebi-recorder-future-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&base);
        let session = Session::create(Some(base), Utc::now()).expect("create");

        let mut future = manifest();
        future.schema_version = SCHEMA_VERSION + 1;
        session.write_manifest(&future).expect("write");
        let error = session.manifest().expect_err("too new").to_string();
        assert!(error.contains("newer komorebi-recorder"), "{error}");
    }

    #[test]
    fn the_readme_embeds_the_session_path_and_every_query() {
        let text = readme(Path::new("/tmp/a b/session"), &manifest());
        assert!(
            text.contains("'/tmp/a b/session'"),
            "paths with spaces are quoted"
        );
        for command in [
            "at ", "range ", "summary ", "locate ", "frames ", "reindex ", "brief ", "guide",
        ] {
            assert!(
                text.contains(&format!("komorebi-recorder {command}")),
                "{command}"
            );
        }
    }

    #[test]
    fn manifests_from_before_segments_and_status_existed_still_load() {
        // exactly the shape a pre-segment recorder wrote: no `status`, no segment fields
        let mut value = serde_json::to_value(manifest()).expect("value");
        let object = value.as_object_mut().expect("object");
        object.remove("status");
        object.remove("video_end_mach_ns");
        object
            .get_mut("komorebi")
            .and_then(Value::as_object_mut)
            .expect("komorebi")
            .remove("pids");

        let old: Manifest = serde_json::from_value(value).expect("an old manifest parses");
        assert_eq!(
            old.status, STATUS_COMPLETE,
            "an old manifest was a finished recording"
        );
        assert!(old.komorebi.pids.is_empty());
    }
}
