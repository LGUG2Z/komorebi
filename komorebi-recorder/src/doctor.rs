//! `doctor`: what this machine can and cannot do for a recording, and probes that measure the
//! things a recording leans on.
//!
//! Everything a bug report about the recorder itself needs is here, as data: run `doctor --json`
//! and send the output. The probes (`--probe clock|capture|decode|window`) each answer one
//! question that cannot be settled by reading code, such as whether the host clock keeps counting
//! while the machine is asleep, and print what they measured whether or not it was good news.
//!
//! This is the `doctor` command ([`crate::cli::DoctorArgs`]). It sits above everything a recording
//! uses (the clocks, komorebi's socket, capture, decoding, the recorder), because it asks each of
//! them what it can do on this machine. It is the same on every host, and answers for the host it
//! runs on: a recording made here is of the komorebi that runs here ([`Flavor::of_host`]).
//!
//! | | Answers | Result |
//! |---|---|---|
//! | [`report`] | what this machine can do for a recording | [`Report`], as [`text`] or as JSON |
//! | [`probe_clock`], [`analyse_clock`] | does the master clock keep counting while the machine sleeps | [`ClockProbe`] |
//! | [`probe_capture`] | what a short recording of the screen alone produces | [`CaptureProbe`] |
//! | [`probe_decode`] | how fast a video decodes, and whether seeking is exact | [`DecodeProbe`] |
//! | [`probe_window`] | would komorebi (for Windows) manage this window, and why | [`WindowProbe`] |
//!
//! A probe's result is a [`ProbeReport`], which [`probe_text`] writes for a person. Both kinds of
//! result are `Serialize` and `JsonSchema`: the `--json` output is the [`Report`] or the
//! [`ProbeReport`] in the agent envelope, with the schema names [`SCHEMA_DOCTOR`] and
//! [`SCHEMA_PROBE`].
//!
//! # What is covered by a test
//!
//! The pure parts have unit tests: [`analyse_clock`] (the verdict for a sleep that paused the
//! clock, one that did not, and one in between, and for no sleep), the old-Windows warning, and a
//! [`report`] that describes this machine and round trips to JSON. [`probe_capture`],
//! [`probe_decode`] and [`probe_window`] need a screen, a video, or komorebi, and no test covers
//! them: the commands are the check, and `docs/dev/windows-checkpoints.md` says how on Windows.
//!
//! # Example
//!
//! The clock probe's verdict, from readings made up to look like a machine that slept for a minute
//! while its master clock paused:
//!
//! ```rust
//! use komorebi_recorder::doctor::{analyse_clock, Reading};
//!
//! let reading = |wall_s: i64, master_s: u64| Reading {
//!     wall_us: (1_800_000_000 + wall_s) * 1_000_000,
//!     mono_ns: master_s * 1_000_000_000,
//!     reference: vec![],
//! };
//!
//! // 60 seconds go by on the wall clock between the second and third readings, and none on the master
//! let probe = analyse_clock(&[reading(0, 0), reading(1, 1), reading(61, 1), reading(62, 2)]);
//! assert_eq!(probe.gaps.len(), 1);
//! assert_eq!(probe.gaps[0].wall_s, 60.0);
//! assert_eq!(probe.gaps[0].master_s, 0.0);
//! assert!(probe.verdict.contains("PAUSED"));
//! ```

use crate::analysis::flavor::Flavor;
use crate::capture::LatencyStats;
use crate::clock;
use crate::clock::ReferenceClock;
use crate::komorebi_link;
use crate::platform;
use crate::platform::Host;
use crate::sources::komorebi_log;
use crate::sources::komorebi_log::DetectedLevel;
use crate::sources::komorebi_version::KomorebiVersion;
use schemars::JsonSchema;
use serde::Serialize;
use std::path::Path;
use std::path::PathBuf;
use std::time::Duration;

/// The schema name of `doctor --json`: [`Report`] in the agent envelope.
pub const SCHEMA_DOCTOR: &str = "doctor/1";
/// The schema name of `doctor --probe <name> --json`: [`ProbeReport`] in the agent envelope.
pub const SCHEMA_PROBE: &str = "probe/1";

/// How long a unix socket path may be before `bind` refuses it (104 bytes on macOS, 108 on
/// Windows and Linux), less the terminator and a margin.
const SOCKET_PATH_LIMIT: usize = 100;

/// What this machine can and cannot do for a recording: [`report`] returns one.
///
/// It is data first: a bug report about the recorder itself is this, as JSON. Whatever is wrong or
/// worth knowing is also said in words, in `warnings`, so that nothing depends on reading the rest.
#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct Report {
    /// The recorder, and the host it is on.
    pub recorder: RecorderInfo,
    /// The komorebi that runs here, and whether the recorder can reach it.
    pub komorebi: KomorebiReport,
    /// What the screen capture can do.
    pub capture: CaptureReport,
    /// The first line of `ffmpeg -version`, if `ffmpeg` is on `PATH` and runs; `None` if it is
    /// not. `frames` and `selftest` need it, and recording and playback do not.
    pub ffmpeg: Option<String>,
    /// komorebi's data directory, and whether recordings can go in it.
    pub data_dir: DataDirReport,
    /// The displays the OS reports.
    pub displays: DisplaysReport,
    /// The master clock, and the other clocks next to it.
    pub clock: ClockReport,
    /// Everything above that is wrong or worth knowing, in words.
    pub warnings: Vec<String>,
}

/// The recorder that ran `doctor`, and the machine it is on.
#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct RecorderInfo {
    /// The recorder's version, `CARGO_PKG_VERSION` of the build.
    pub version: String,
    /// The host the recorder is running on (see [`Host::current`]): its OS, architecture, OS
    /// version and master clock.
    pub host: Host,
    /// The path of the running executable, `None` if the OS cannot say.
    pub executable: Option<String>,
}

/// komorebi as the recorder finds it on this machine.
#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct KomorebiReport {
    /// Which komorebi a recording made here would be of.
    pub flavor: Flavor,
    /// Whether a komorebi daemon is running: a process named `komorebi` (`komorebi.exe` on
    /// Windows).
    pub running: bool,
    /// The running daemon's process id. `None` if it is not running.
    pub pid: Option<i32>,
    /// The path of the running daemon's binary, `None` if it is not running or the OS does not say.
    pub exe: Option<String>,
    /// What the binary says about itself when asked `--version`: its version, branch and commit.
    /// `None` if it is not running, does not answer within 2 seconds, or answers with something
    /// that is not understood.
    pub version: Option<KomorebiVersion>,
    /// How verbose the daemon's log is, and how that was worked out. `None` if it is not
    /// running.
    pub log_level: Option<DetectedLevel>,
    /// Whether the recorder's windows will have a title, and why (see [`crate::komorebi_ignore`]).
    pub window_title: String,
    /// komorebi's command socket.
    pub socket: SocketReport,
    /// Where komorebi's plaintext log is, and the newest files in it.
    pub log_dir: String,
    /// The newest plaintext log files in `log_dir`, up to three, newest first by name (the dates
    /// in the names sort).
    pub log_files: Vec<LogFile>,
}

/// komorebi's command socket, and the recorder's.
#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct SocketReport {
    /// `komorebi.sock`, which commands are sent to.
    pub path: String,
    /// Whether the socket file exists. A file left behind by a komorebi that died exists too.
    pub exists: bool,
    /// Whether komorebi answered a `State` query, and with what if not.
    ///
    /// Only asked if komorebi is running; `false` if it is not.
    pub answers_state: bool,
    /// Why komorebi did not answer, when it is running and did not: the error, or that the reply
    /// was not JSON. `None` if it answered, and if it is not running.
    pub detail: Option<String>,
    /// Where the recorder's socket would be, and whether that path is short enough to bind.
    ///
    /// The length of that path in bytes. The recorder's subscriber socket is
    /// `komorebi-recorder-<pid>` in komorebi's data directory.
    pub subscriber_path_length: usize,
    /// Whether `subscriber_path_length` is at most 100 bytes, which leaves room under the limit of
    /// a unix socket path (104 bytes on macOS, 108 on Windows and Linux). If it does not fit, the
    /// recorder cannot listen for komorebi's events.
    pub subscriber_path_fits: bool,
}

/// A komorebi plaintext log file.
#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct LogFile {
    /// The file name, `komorebi_plaintext.log` and a date.
    pub name: String,
    /// The size of the file in bytes, 0 if it cannot be read.
    pub bytes: u64,
}

/// What the screen capture and video playback can do on this machine.
#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct CaptureReport {
    /// Whether this build can capture the screen on this host at all.
    pub supported: bool,
    /// Whether the OS lets this process capture right now (macOS Screen Recording permission;
    /// always true on Windows).
    pub permitted: bool,
    /// Whether this build can decode and play back a recording's video on this host.
    pub video_decoding: bool,
    /// A sentence saying that the OS has not let this process capture, when it has not. `None` if
    /// it has.
    pub note: Option<String>,
}

/// komorebi's data directory, where sockets live and recordings go by default.
#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct DataDirReport {
    /// The directory (see [`komorebi_link::data_dir`]).
    pub path: String,
    /// Whether it exists. komorebi creates it, so one that does not exist means komorebi has not
    /// run as this user, or keeps its data elsewhere.
    pub exists: bool,
    /// Whether a file could be created in it. This is found out by creating one, `.doctor-<pid>`,
    /// and removing it at once.
    pub writable: bool,
    /// Where a recording goes unless told otherwise: `recordings/` in the data directory.
    pub recordings: String,
}

/// The displays the OS reports.
#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct DisplaysReport {
    /// The displays that are attached, in the order the host's capture lists them.
    pub displays: Vec<DisplayInfo>,
    /// Why the displays could not be listed, if they could not. `displays` is then empty.
    pub error: Option<String>,
}

/// One display.
#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct DisplayInfo {
    /// The OS's id for the display.
    pub id: u32,
    /// `[x, y, width, height]` in the OS's own units: points on macOS, physical pixels on Windows.
    pub frame: [f64; 4],
    /// How many pixels the display has for each unit of `frame`: 2.0 on a Retina display on macOS,
    /// and 1.0 on Windows, whose frames are already in pixels.
    pub scale: f64,
    /// Whether the OS treats it as the main display.
    pub primary: bool,
    /// Whether it is attached but asleep, and so not capturable until it wakes.
    pub asleep: bool,
}

/// The master clock, as it was read while the report was made.
#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct ClockReport {
    /// Which clock the master clock is on this host (see [`crate::clock`]).
    pub kind: platform::ClockKind,
    /// Nanoseconds one tick of the master clock is worth, as `numer / denom`.
    pub timebase: String,
    /// The smallest step seen between back-to-back reads of the master clock, in nanoseconds,
    /// over 2000 reads. 0 if no read differed from the one before.
    pub resolution_ns: u64,
    /// Whether the master clock never went backwards over those reads.
    pub monotonic: bool,
    /// A reading of the master clock, in nanoseconds.
    pub now_ns: u64,
    /// A reading of the wall clock, in microseconds since the Unix epoch.
    pub wall_now_us: i64,
    /// The host's other clocks, read at the same time (see [`ReferenceClock`]).
    pub reference: Vec<ReferenceClock>,
}

/// Builds the report: what this machine can and cannot do for a recording.
///
/// It looks at the machine and changes nothing that lasts. It lists the processes to find komorebi,
/// runs `komorebi --version` and `ffmpeg -version`, asks komorebi for its state if it is running,
/// asks the OS for the displays and for permission to capture, and reads the clocks about two
/// thousand times. The one thing it writes is a small file, `.doctor-<pid>`, in komorebi's data
/// directory, which it removes at once, to find out whether the directory is writable. Nothing in
/// it fails: what cannot be found out is in the report as a field, and as a warning in words.
///
/// # Example
///
/// ```rust
/// use komorebi_recorder::doctor::report;
///
/// let report = report();
/// assert_eq!(report.recorder.host.os, std::env::consts::OS);
/// assert!(report.clock.monotonic);
/// ```
///
/// The same is checked, with the JSON, by the test
/// `the_report_describes_this_machine_and_roundtrips_to_json`.
#[must_use]
pub fn report() -> Report {
    let host = Host::current();
    let flavor = Flavor::of_host();
    let mut warnings = Vec::new();

    let data_dir = komorebi_link::data_dir();
    let log_dir = komorebi_log::find_log_dir(flavor, data_dir, &std::env::temp_dir());

    let process = crate::recorder::find_komorebi();
    let version = process
        .as_ref()
        .and_then(|process| process.exe.as_deref().map(|exe| (exe, process.pid)))
        .and_then(|(exe, pid)| crate::sources::komorebi_version::query(exe, Some(pid)));
    let socket = socket_report(data_dir, process.is_some(), &mut warnings);

    if let Some(process) = &process
        && let Some(warning) = komorebi_log::level_warning(&process.level.level)
    {
        warnings.push(warning);
    }

    let komorebi = KomorebiReport {
        flavor,
        running: process.is_some(),
        pid: process.as_ref().map(|process| process.pid),
        exe: process
            .as_ref()
            .and_then(|process| process.exe.as_ref())
            .map(|exe| exe.display().to_string()),
        version,
        log_level: process.as_ref().map(|process| process.level.clone()),
        window_title: crate::komorebi_ignore::describe_here(),
        socket,
        log_files: newest_logs(&log_dir),
        log_dir: log_dir.display().to_string(),
    };
    if process.is_none() {
        warnings.push(
            "komorebi is not running: a recording would have the screen but nothing from komorebi"
                .to_string(),
        );
    } else if komorebi.log_files.is_empty() {
        warnings.push(format!(
            "no komorebi_plaintext.log.* in {}: komorebi's log would not be recorded",
            komorebi.log_dir
        ));
    }

    let displays = displays_report(&mut warnings);
    let supported = platform::capture::supported();
    if !supported {
        warnings.push(format!(
            "recording the screen is not built for {} yet; `record --no-video` works",
            host.describe()
        ));
    }

    let permitted = platform::capture::has_permission();
    let capture = CaptureReport {
        supported,
        permitted,
        video_decoding: crate::av::supported(),
        note: (!permitted).then(permission_note),
    };
    if supported && !permitted && cfg!(target_os = "linux") {
        warnings.push(permission_note());
    }
    if !capture.video_decoding {
        warnings.push(format!(
            "playing back a recording's video is not built for {} yet",
            host.describe()
        ));
    }

    if let Some(warning) = own_windows_warning(&host) {
        warnings.push(warning);
    }

    let ffmpeg = ffmpeg_version();
    if ffmpeg.is_none() {
        warnings.push(
            "ffmpeg is not on PATH: `frames` and `selftest` need it (recording and playback do not)"
                .to_string(),
        );
    }

    let data = data_dir_report(data_dir);
    if !data.exists {
        warnings.push(format!(
            "{} does not exist: komorebi has not run as this user, or keeps its data elsewhere",
            data.path
        ));
    } else if !data.writable {
        warnings.push(format!("{} is not writable", data.path));
    }

    let clock = clock_report();
    if !clock.monotonic {
        warnings.push("the master clock went backwards while it was being read".to_string());
    }

    Report {
        recorder: RecorderInfo {
            version: env!("CARGO_PKG_VERSION").to_string(),
            host,
            executable: std::env::current_exe()
                .ok()
                .map(|exe| exe.display().to_string()),
        },
        komorebi,
        capture,
        ffmpeg,
        data_dir: data,
        displays,
        clock,
        warnings,
    }
}

fn socket_report(data_dir: &Path, running: bool, warnings: &mut Vec<String>) -> SocketReport {
    let path = komorebi_link::socket_path(komorebi_link::KOMOREBI_SOCKET);
    let subscriber =
        komorebi_link::socket_path(&format!("komorebi-recorder-{}", std::process::id()));
    let subscriber_path_length = subscriber.as_os_str().len();
    let subscriber_path_fits = subscriber_path_length <= SOCKET_PATH_LIMIT;
    if !subscriber_path_fits {
        warnings.push(format!(
            "{} is {subscriber_path_length} bytes long, too long for a unix socket: the recorder \
             could not listen for komorebi's events",
            data_dir.display()
        ));
    }

    let (answers_state, detail) = if running {
        match komorebi_link::query_state() {
            Ok(state) => match serde_json::from_str::<serde_json::Value>(&state) {
                Ok(_) => (true, None),
                Err(error) => (false, Some(format!("the reply was not JSON: {error}"))),
            },
            Err(error) => (false, Some(error.to_string())),
        }
    } else {
        (false, None)
    };
    if running && !answers_state {
        warnings.push(format!(
            "komorebi is running but did not answer a state query: {}",
            detail.as_deref().unwrap_or("no reason given")
        ));
    }

    SocketReport {
        exists: path.exists(),
        path: path.display().to_string(),
        answers_state,
        detail,
        subscriber_path_length,
        subscriber_path_fits,
    }
}

fn newest_logs(dir: &Path) -> Vec<LogFile> {
    let mut found: Vec<LogFile> = std::fs::read_dir(dir)
        .into_iter()
        .flatten()
        .filter_map(Result::ok)
        .filter(|entry| {
            entry
                .file_name()
                .to_string_lossy()
                .starts_with(komorebi_log::PLAINTEXT_PREFIX)
        })
        .map(|entry| LogFile {
            name: entry.file_name().to_string_lossy().into_owned(),
            bytes: entry.metadata().map_or(0, |metadata| metadata.len()),
        })
        .collect();
    // the dates in the names sort
    found.sort_by(|a, b| b.name.cmp(&a.name));
    found.truncate(3);
    found
}

fn displays_report(warnings: &mut Vec<String>) -> DisplaysReport {
    let main = platform::capture::main_display_id();
    match platform::capture::active_displays() {
        Ok(displays) => {
            if displays.is_empty() {
                warnings.push("the OS reports no displays".to_string());
            }
            DisplaysReport {
                displays: displays
                    .into_iter()
                    .map(|display| DisplayInfo {
                        id: display.id,
                        frame: display.frame,
                        scale: display.scale,
                        primary: display.id == main,
                        asleep: display.asleep,
                    })
                    .collect(),
                error: None,
            }
        }
        Err(error) => DisplaysReport {
            displays: Vec::new(),
            error: Some(format!("{error:#}")),
        },
    }
}

/// The first Windows build whose windows can ask to be left out of a screen capture (version 2004).
const EXCLUDE_FROM_CAPTURE_BUILD: u32 = 19041;

/// Say so when this Windows is too old for the recorder's windows to keep themselves out of the
/// video: the only option there blanks them out instead.
fn own_windows_warning(host: &Host) -> Option<String> {
    if !host.is_windows() {
        return None;
    }

    let build: u32 = host
        .os_version
        .as_deref()?
        .rsplit('.')
        .next()?
        .parse()
        .ok()?;
    (build < EXCLUDE_FROM_CAPTURE_BUILD).then(|| {
        format!(
            "this Windows (build {build}) is older than Windows 10 version 2004: the recorder's own \
             windows cannot keep themselves out of the video and will appear in it"
        )
    })
}

fn ffmpeg_version() -> Option<String> {
    let output = std::process::Command::new("ffmpeg")
        .arg("-version")
        .output()
        .ok()
        .filter(|output| output.status.success())?;
    String::from_utf8_lossy(&output.stdout)
        .lines()
        .next()
        .map(str::to_string)
}

fn data_dir_report(path: &Path) -> DataDirReport {
    let exists = path.is_dir();
    let probe = path.join(format!(".doctor-{}", std::process::id()));
    let writable = exists && std::fs::write(&probe, b"x").is_ok();
    let _ = std::fs::remove_file(&probe);

    DataDirReport {
        path: path.display().to_string(),
        exists,
        writable,
        recordings: crate::session::default_recordings_dir()
            .display()
            .to_string(),
    }
}

fn clock_report() -> ClockReport {
    let (mut monotonic, mut resolution) = (true, u64::MAX);
    let mut last = clock::mono_now_ns();
    for _ in 0..2_000 {
        let now = clock::mono_now_ns();
        monotonic &= now >= last;
        if now > last {
            resolution = resolution.min(now - last);
        }
        last = now;
    }

    let timebase = clock::timebase();
    ClockReport {
        kind: clock_kind(),
        timebase: format!("{}/{}", timebase.numer, timebase.denom),
        resolution_ns: if resolution == u64::MAX {
            0
        } else {
            resolution
        },
        monotonic,
        now_ns: clock::mono_now_ns(),
        wall_now_us: clock::wall_now_us(),
        reference: clock::reference_clocks(),
    }
}

/// What is missing when the screen cannot be captured, for this host.
fn permission_note() -> String {
    if cfg!(target_os = "linux") {
        let exe = std::env::current_exe().map_or_else(
            |_| "komorebi-recorder".to_string(),
            |path| path.display().to_string(),
        );
        format!(
            "KWin offers its screencast only to a program whose desktop file lists it: write \
             ~/.local/share/applications/komorebi-recorder.desktop with Type=Application, \
             Name=komorebi-recorder, Exec={exe}, NoDisplay=true and \
             X-KDE-Wayland-Interfaces=zkde_screencast_unstable_v1, then start the recorder again \
             (see docs/PLATFORM.md)"
        )
    } else {
        "screen recording permission has not been granted to this terminal or app".to_string()
    }
}

fn clock_kind() -> platform::ClockKind {
    platform::clock::KIND
}

/// Returns the report as text for a person: the sections of [`Report`] as short lines, and the
/// warnings last, under `things to know`.
///
/// The text has the same sections as the JSON in fewer words, so that a report can be read at a
/// glance and pasted. It leaves out some of the fields (the subscriber socket's path length, the
/// reference clocks, the default recordings directory). The JSON has all of them, and is what a
/// bug report should include.
///
/// # Example
///
/// ```rust
/// use komorebi_recorder::doctor::{report, text};
///
/// let report = report();
/// let text = text(&report);
/// assert!(text.starts_with("komorebi-recorder "));
/// assert!(text.contains("\nclock\n"));
/// ```
#[must_use]
pub fn text(report: &Report) -> String {
    use std::fmt::Write as _;

    let mut out = String::new();
    let yes = |value: bool| if value { "yes" } else { "no" };

    let _ = writeln!(
        out,
        "komorebi-recorder {} on {} ({})",
        report.recorder.version,
        report.recorder.host.describe(),
        report.recorder.host.arch
    );

    let k = &report.komorebi;
    let _ = writeln!(out, "\nkomorebi for {}", k.flavor);
    let _ = writeln!(out, "  running:      {}", yes(k.running));
    let _ = writeln!(out, "  windows:      {}", k.window_title);
    if let Some(pid) = k.pid {
        let _ = writeln!(out, "  pid:          {pid}");
    }
    if let Some(exe) = &k.exe {
        let _ = writeln!(out, "  binary:       {exe}");
    }
    if let Some(version) = &k.version {
        let _ = writeln!(
            out,
            "  version:      {} {} {}",
            version.version,
            version.branch.as_deref().unwrap_or(""),
            version.commit.as_deref().unwrap_or("")
        );
    }
    if let Some(level) = &k.log_level {
        let _ = writeln!(
            out,
            "  log level:    {} (from {})",
            level.level, level.source
        );
    }
    let _ = writeln!(
        out,
        "  socket:       {} ({}; answers a state query: {})",
        k.socket.path,
        if k.socket.exists { "exists" } else { "missing" },
        yes(k.socket.answers_state)
    );
    let _ = writeln!(out, "  log in:       {}", k.log_dir);
    for file in &k.log_files {
        let _ = writeln!(out, "                {} ({} bytes)", file.name, file.bytes);
    }

    let _ = writeln!(out, "\ncapture");
    let _ = writeln!(out, "  can record:   {}", yes(report.capture.supported));
    let _ = writeln!(out, "  permitted:    {}", yes(report.capture.permitted));
    let _ = writeln!(
        out,
        "  can decode:   {}",
        yes(report.capture.video_decoding)
    );
    let _ = writeln!(
        out,
        "  ffmpeg:       {}",
        report.ffmpeg.as_deref().unwrap_or("not found")
    );

    let _ = writeln!(out, "\ndisplays");
    for display in &report.displays.displays {
        let [x, y, width, height] = display.frame;
        let _ = writeln!(
            out,
            "  {}{}  {width}x{height} at ({x}, {y})  scale {:.2}{}",
            display.id,
            if display.primary { " (primary)" } else { "" },
            display.scale,
            if display.asleep { "  asleep" } else { "" }
        );
    }
    if let Some(error) = &report.displays.error {
        let _ = writeln!(out, "  could not list them: {error}");
    }

    let _ = writeln!(out, "\ndata");
    let _ = writeln!(
        out,
        "  {}  (exists: {}, writable: {})",
        report.data_dir.path,
        yes(report.data_dir.exists),
        yes(report.data_dir.writable)
    );

    let c = &report.clock;
    let _ = writeln!(out, "\nclock");
    let _ = writeln!(
        out,
        "  {:?}, tick {} ns, resolution {} ns, monotonic: {}",
        c.kind,
        c.timebase,
        c.resolution_ns,
        yes(c.monotonic)
    );

    if !report.warnings.is_empty() {
        let _ = writeln!(out, "\nthings to know");
        for warning in &report.warnings {
            let _ = writeln!(out, "  - {warning}");
        }
    }

    out
}

// ---- probes -------------------------------------------------------------------------------------

/// What a probe found.
///
/// Serialized, the result of the probe is next to a `probe` field that names it
/// (`clock`, `capture`, `decode` or `window`), which is how a reader of the JSON tells which probe
/// it has.
///
/// # Example
///
/// ```rust
/// use komorebi_recorder::doctor::{ProbeReport, WindowProbe};
///
/// let probe = ProbeReport::Window(WindowProbe {
///     hwnd: 131_072,
///     komorebi_says: "would manage".to_string(),
/// });
/// let json = serde_json::to_value(&probe)?;
/// assert_eq!(json["probe"], "window");
/// assert_eq!(json["hwnd"], 131_072);
/// # Ok::<(), serde_json::Error>(())
/// ```
#[derive(Debug, Clone, Serialize, JsonSchema)]
#[serde(tag = "probe", rename_all = "kebab-case")]
pub enum ProbeReport {
    /// What the master clock does across a sleep.
    Clock(ClockProbe),
    /// What a recording of the screen alone produced.
    Capture(CaptureProbe),
    /// How a video decoded.
    Decode(DecodeProbe),
    /// What komorebi says about a window.
    Window(WindowProbe),
}

/// One reading of every clock at once: the wall clock, the master clock and the reference clocks,
/// taken one after another.
#[derive(Debug, Clone)]
pub struct Reading {
    /// The wall clock, in microseconds since the Unix epoch.
    pub wall_us: i64,
    /// The master clock, in nanoseconds.
    pub mono_ns: u64,
    /// The host's other clocks (see [`ReferenceClock`]).
    pub reference: Vec<ReferenceClock>,
}

impl Reading {
    /// Reads every clock now.
    #[must_use]
    pub fn take() -> Self {
        Self {
            wall_us: clock::wall_now_us(),
            mono_ns: clock::mono_now_ns(),
            reference: clock::reference_clocks(),
        }
    }
}

/// A stretch of the probe in which the wall clock ran far ahead of the sampling schedule, which is
/// what a sleep (or the process being frozen) looks like.
#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct Gap {
    /// Seconds into the probe at which it ended, by the wall clock, from the first reading.
    pub at_s: f64,
    /// How far the wall clock moved over the stretch, in seconds.
    pub wall_s: f64,
    /// How far the master clock moved over the same stretch.
    pub master_s: f64,
    /// And each reference clock: its name, and how far it moved, in seconds. A reference clock that
    /// was not read at both ends of the stretch is left out.
    pub reference_s: Vec<(String, f64)>,
}

/// What a clock probe found: the sleeps it saw, and what the master clock did across them.
#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct ClockProbe {
    /// How long the probe ran, by the wall clock, in seconds.
    pub seconds: f64,
    /// How many readings it took.
    pub samples: usize,
    /// The sleeps it saw: stretches in which the wall clock moved far more than the sampling
    /// interval. There are none if the machine did not sleep (or freeze) while it ran.
    pub gaps: Vec<Gap>,
    /// What the gaps say about the master clock, in words.
    pub verdict: String,
}

/// A gap is a stretch where the wall clock moved this much more than one sampling interval.
const GAP_THRESHOLD_S: f64 = 1.5;

/// Finds the sleeps in a series of readings and says what the master clock did across them.
///
/// A sleep is a gap: two consecutive readings between which the wall clock moved 1.5 seconds or
/// more. The probe reads about every 250 ms, so that is far more than a sampling interval, and is
/// what a sleep looks like; so is the process being frozen. For each gap it records how far the wall
/// clock, the master clock and each reference clock moved. The verdict compares the master clock with
/// the wall clock over all the gaps together:
///
/// | The master clock moved | The verdict |
/// |---|---|
/// | at least 90 % as far as the wall clock | `KEPT COUNTING`: a sleep cannot be noticed by comparing it with the wall clock, and the recorder must listen for power events instead |
/// | at most 10 % as far | `PAUSED`: comparing it with the wall clock finds a sleep |
/// | between | neither: not safe to assume either |
///
/// With no gap the verdict says that nothing was learned, and to run the probe again with a sleep
/// in it. `seconds` is the wall clock time from the first reading to the last, and 0 for no
/// readings.
///
/// # Example: a machine that slept, and a probe that saw nothing
///
/// ```rust
/// use komorebi_recorder::doctor::{analyse_clock, Reading};
///
/// let reading = |wall_s: i64, master_s: u64| Reading {
///     wall_us: (1_800_000_000 + wall_s) * 1_000_000,
///     mono_ns: master_s * 1_000_000_000,
///     reference: vec![],
/// };
///
/// // the wall clock ran 60 seconds ahead of the master clock: it paused
/// let paused = analyse_clock(&[reading(0, 0), reading(1, 1), reading(61, 1), reading(62, 2)]);
/// assert!(paused.verdict.contains("PAUSED"));
///
/// // the master clock kept pace with the wall clock through the sleep: it kept counting
/// let counting = analyse_clock(&[reading(0, 0), reading(1, 1), reading(61, 61), reading(62, 62)]);
/// assert!(counting.verdict.contains("KEPT COUNTING"));
///
/// // halfway: neither
/// let neither = analyse_clock(&[reading(0, 0), reading(100, 50)]);
/// assert!(neither.verdict.contains("neither paused nor counting"));
///
/// // no sleep: nothing learned
/// let none = analyse_clock(&[reading(0, 0), reading(1, 1), reading(2, 2)]);
/// assert!(none.gaps.is_empty());
/// assert!(none.verdict.contains("no sleep happened"));
/// ```
#[must_use]
pub fn analyse_clock(readings: &[Reading]) -> ClockProbe {
    let mut gaps = Vec::new();
    let first = readings.first();

    for pair in readings.windows(2) {
        let (before, after) = (&pair[0], &pair[1]);
        let wall_s = (after.wall_us - before.wall_us) as f64 / 1e6;
        if wall_s < GAP_THRESHOLD_S {
            continue;
        }

        gaps.push(Gap {
            at_s: first.map_or(0.0, |first| (after.wall_us - first.wall_us) as f64 / 1e6),
            wall_s,
            master_s: after.mono_ns.saturating_sub(before.mono_ns) as f64 / 1e9,
            reference_s: after
                .reference
                .iter()
                .filter_map(|now| {
                    let then = before
                        .reference
                        .iter()
                        .find(|clock| clock.name == now.name)?;
                    Some((
                        now.name.clone(),
                        now.ns.saturating_sub(then.ns) as f64 / 1e9,
                    ))
                })
                .collect(),
        });
    }

    let verdict = match gaps.first() {
        None => format!(
            "no sleep happened during the probe, so nothing was learned about the master clock: \
             run it again and put the machine to sleep (and wake it) while it runs; it saw no \
             wall-clock jump of {GAP_THRESHOLD_S} s or more"
        ),
        Some(_) => {
            let slept: f64 = gaps.iter().map(|gap| gap.wall_s).sum();
            let counted: f64 = gaps.iter().map(|gap| gap.master_s).sum();
            if counted >= slept * 0.9 {
                format!(
                    "the master clock KEPT COUNTING through {slept:.1} s of sleep (it moved \
                     {counted:.1} s): a sleep cannot be noticed by comparing it with the wall \
                     clock, and the recorder must listen for power events instead"
                )
            } else if counted <= slept * 0.1 {
                format!(
                    "the master clock PAUSED through {slept:.1} s of sleep (it moved {counted:.1} \
                     s): comparing it with the wall clock finds a sleep"
                )
            } else {
                format!(
                    "the master clock moved {counted:.1} s across {slept:.1} s of wall time: \
                     neither paused nor counting, so it is not safe to assume either"
                )
            }
        }
    };

    ClockProbe {
        seconds: first.zip(readings.last()).map_or(0.0, |(first, last)| {
            (last.wall_us - first.wall_us) as f64 / 1e6
        }),
        samples: readings.len(),
        gaps,
        verdict,
    }
}

/// Reads the clocks every `interval` for `seconds`, then says what they did ([`analyse_clock`]).
///
/// The probe is only useful if the machine sleeps while it runs: the verdict says so if it did not.
/// It always takes one reading at the start, and then one after each `interval` until `seconds`
/// have gone by, so a `seconds` of zero, or less, takes the first reading only.
///
/// # Panics
///
/// Panics if `seconds` is infinite, or so large that it cannot be added to the current time. A
/// negative or `NaN` `seconds` does not panic, and is taken as zero.
///
/// # Example
///
/// ```rust
/// use komorebi_recorder::doctor::probe_clock;
/// use std::time::Duration;
///
/// // a short probe: a reading at the start, and one after each interval
/// let probe = probe_clock(0.2, Duration::from_millis(50));
/// assert!(probe.samples >= 2);
/// assert!(probe.seconds > 0.0);
/// ```
#[must_use]
pub fn probe_clock(seconds: f64, interval: Duration) -> ClockProbe {
    let deadline = std::time::Instant::now() + Duration::from_secs_f64(seconds.max(0.0));
    let mut readings = vec![Reading::take()];
    while std::time::Instant::now() < deadline {
        std::thread::sleep(interval);
        readings.push(Reading::take());
    }

    analyse_clock(&readings)
}

/// What a capture probe found: what came out of the capture pipeline, for each video segment.
#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct CaptureProbe {
    /// The session directory the probe recorded into. It is kept.
    pub session: String,
    /// How long the probe was asked to record, in seconds.
    pub seconds: f64,
    /// Each video segment the recording made: one for each display, and more if a display went
    /// away and came back, or its stream died, while the probe ran.
    pub displays: Vec<CaptureDisplay>,
}

/// What the capture pipeline produced for one video segment.
#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct CaptureDisplay {
    /// The id of the display.
    pub display: u32,
    /// The segment: 1-based, per display.
    pub segment: u32,
    /// How many frames were written.
    pub frames: u64,
    /// How many frames were dropped.
    pub dropped: u64,
    /// How many callbacks carried no new content. A healthy stream on a static screen still makes
    /// these, which is how a frozen video is told from an idle screen.
    pub idle_callbacks: u64,
    /// The width of the video, in pixels.
    pub width_px: usize,
    /// The height of the video, in pixels.
    pub height_px: usize,
    /// The video file, relative to the session directory.
    pub file: String,
    /// The size of the file on disk, in bytes. 0 if it cannot be read.
    pub file_bytes: u64,
    /// What the OS says the file holds, if it can play it.
    pub file_duration_s: Option<f64>,
    /// How far the frames' timestamps lag the moment the capture saw them. A large or
    /// negative lag means the timestamps are not on the master clock.
    pub latency: LatencyStats,
    /// Whether `latency` is what a frame timestamp on the master clock looks like (see
    /// [`LatencyStats::looks_like_host_clock`]). `false` means the video's times cannot be
    /// trusted to line up with the rest of a session.
    pub looks_like_host_clock: bool,
    /// Why the segment ended, as words (`stopped`, `display disconnected`, ...). `None` if it
    /// has no end reason.
    pub end_reason: Option<String>,
    /// The error the capture hit, if it hit one.
    pub error: Option<String>,
}

/// Records the screen alone for `seconds` and reports what came out of the pipeline. Leave the
/// screen still for part of it: a static screen is the case that has gone wrong before.
///
/// This is a real recording, with no komorebi and no OS trace, of every display (including any
/// connected while it runs), into `out` (or next to the other recordings if `None`). The session it
/// makes is kept, and its path is in the result.
///
/// # Errors
///
/// Returns an error if the recording fails, for any reason `record` would: the OS has not let
/// this process capture (on macOS, the Screen Recording permission), no display can be recorded,
/// this host cannot capture the screen at all (the `unsupported` host), or the session directory
/// cannot be created. It also fails if the session it made cannot be opened afterwards.
///
/// # Panics
///
/// Panics if `seconds` is negative, `NaN`, or too large to be a [`Duration`].
///
/// # Example
///
/// ```rust,no_run
/// use komorebi_recorder::doctor::probe_capture;
///
/// // record for 10 seconds, and say what came out
/// let probe = probe_capture(10.0, None)?;
/// for display in &probe.displays {
///     println!("display {}: {} frames, {} idle callbacks", display.display, display.frames, display.idle_callbacks);
/// }
/// # Ok::<(), color_eyre::eyre::Report>(())
/// ```
///
/// This needs a screen, and the OS's permission to record it, so it is not run, and no automated
/// test covers it: `komorebi-recorder doctor --probe capture` is the check.
pub fn probe_capture(seconds: f64, out: Option<PathBuf>) -> color_eyre::eyre::Result<CaptureProbe> {
    use crate::recorder::Live;
    use crate::recorder::OsLogOptions;
    use crate::recorder::RecordOptions;

    let options = RecordOptions {
        out,
        duration: Some(Duration::from_secs_f64(seconds)),
        no_komorebi: true,
        oslog: OsLogOptions {
            enabled: false,
            ..OsLogOptions::default()
        },
        ..RecordOptions::default()
    };
    let live = Live::new();
    let dir = crate::recorder::record(&options, &live)?;

    let session = crate::session::Session::open(&dir)?;
    let manifest = session.manifest()?;
    let displays = manifest
        .displays
        .iter()
        .map(|entry| {
            let result = &entry.result;
            let path = session.path(&result.file);
            CaptureDisplay {
                display: result.display.id,
                segment: result.segment,
                frames: result.frames,
                dropped: result.dropped,
                idle_callbacks: result.idle_callbacks,
                width_px: result.width_px,
                height_px: result.height_px,
                file: result.file.clone(),
                file_bytes: std::fs::metadata(&path).map_or(0, |metadata| metadata.len()),
                file_duration_s: platform::capture::media_duration_s(&path),
                latency: result.latency,
                looks_like_host_clock: result.latency.looks_like_host_clock(),
                end_reason: result.end_reason.map(|reason| reason.to_string()),
                error: result.error.clone(),
            }
        })
        .collect();

    Ok(CaptureProbe {
        session: dir.display().to_string(),
        seconds,
        displays,
    })
}

/// What a decode probe found: how fast a video decodes, and whether seeking is exact.
#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct DecodeProbe {
    /// The video file that was decoded.
    pub file: String,
    /// The width of the video, in pixels.
    pub width: u32,
    /// The height of the video, in pixels.
    pub height: u32,
    /// The duration the decoder reports, in seconds.
    pub duration_s: f64,
    /// How long it took from opening the file to the first decoded frame, in milliseconds. The
    /// first decode on a machine can be slower than later ones.
    pub first_frame_ms: f64,
    /// How many frames were decoded, up to 300.
    pub frames_decoded: usize,
    /// How many frames a second were decoded after the first, at native size. 0 if only one
    /// frame was decoded.
    pub decode_fps: f64,
    /// For each time asked for, how far the first frame back was from it (seconds); a seek that
    /// lands more than a frame away is not frame exact.
    pub seeks: Vec<Seek>,
}

/// The result of one seek in a decode probe.
#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct Seek {
    /// The time asked for, in seconds into the file.
    pub asked_s: f64,
    /// The time of the first frame that came back, in seconds into the file. `None` if no frame
    /// came back.
    pub got_s: Option<f64>,
}

/// Decodes a video file: how long the first frame takes, how fast frames come after it, and whether
/// seeking lands where it was asked to.
///
/// The file is opened and decoded from the start at native size: the time to the first frame, then
/// up to 300 frames for the speed. It then seeks to a quarter, a half and three quarters of the
/// way through, at a reduced width of 320 pixels, and records the time of the first frame that
/// comes back each time.
///
/// # Errors
///
/// Returns an error if the file cannot be opened or is not a video the host can decode, or if
/// decoding fails part of the way through (see [`crate::av`]).
///
/// # Example
///
/// ```rust,no_run
/// use komorebi_recorder::doctor::probe_decode;
/// use std::path::Path;
///
/// let probe = probe_decode(Path::new("/path/to/video-1.mp4"))?;
/// println!("first frame in {:.0} ms, then {:.0} frames/s", probe.first_frame_ms, probe.decode_fps);
/// for seek in &probe.seeks {
///     println!("asked for {:.3} s, got {:?}", seek.asked_s, seek.got_s);
/// }
/// # Ok::<(), color_eyre::eyre::Report>(())
/// ```
///
/// This needs a video file, so it is not run, and no test of this module covers it.
pub fn probe_decode(file: &Path) -> color_eyre::eyre::Result<DecodeProbe> {
    use crate::av::Asset;
    use crate::av::Scale;
    use std::time::Instant;

    let asset = Asset::open(file)?;
    let (width, height) = asset.size();
    let duration_s = asset.duration_s();

    let started = Instant::now();
    let mut reader = asset.start(0.0, None, Scale::Native)?;
    let first = reader.next_frame()?;
    let first_frame_ms = started.elapsed().as_secs_f64() * 1e3;

    let mut frames_decoded = usize::from(first.is_some());
    let timer = Instant::now();
    while frames_decoded < 300 {
        if reader.next_frame()?.is_none() {
            break;
        }
        frames_decoded += 1;
    }
    let decode_fps = if frames_decoded > 1 {
        (frames_decoded - 1) as f64 / timer.elapsed().as_secs_f64().max(1e-9)
    } else {
        0.0
    };

    let mut seeks = Vec::new();
    for fraction in [0.25, 0.5, 0.75] {
        let asked_s = duration_s * fraction;
        let got_s = asset
            .start(asked_s, None, Scale::Width(320))?
            .next_frame()?
            .map(|frame| frame.pts_s);
        seeks.push(Seek { asked_s, got_s });
    }

    Ok(DecodeProbe {
        file: file.display().to_string(),
        width,
        height,
        duration_s,
        first_frame_ms,
        frames_decoded,
        decode_fps,
        seeks,
    })
}

/// What komorebi says about one window.
#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct WindowProbe {
    /// The window handle that was asked about.
    pub hwnd: u32,
    /// komorebi's own answer to "would you manage this window, and why": its verdict and the rules
    /// that matched, verbatim.
    pub komorebi_says: String,
}

/// Asks komorebi (for Windows) about the window with this handle.
///
/// This sends komorebi a `DebugWindow` message ([`komorebi_link::Message::DebugWindow`]) and
/// returns its answer as it was written: komorebi's verdict on whether it would manage the window,
/// and the rules that decided it. An empty answer is reported as that: komorebi may be busy or
/// paused, or know no such window.
///
/// # Errors
///
/// Returns an error if komorebi cannot be asked: it is not running, or its socket cannot be used
/// or does not answer (`could not ask komorebi; is it running?`, with the I/O error under it).
///
/// # Example
///
/// ```rust,no_run
/// use komorebi_recorder::doctor::probe_window;
///
/// let probe = probe_window(131_072)?;
/// println!("komorebi on window {}: {}", probe.hwnd, probe.komorebi_says);
/// # Ok::<(), color_eyre::eyre::Report>(())
/// ```
///
/// This needs komorebi for Windows running, so it is not run, and no test covers it. The message
/// it sends is covered by the test `messages_are_spelled_as_komorebi_reads_them` of
/// [`komorebi_link`].
pub fn probe_window(hwnd: u32) -> color_eyre::eyre::Result<WindowProbe> {
    use color_eyre::eyre::WrapErr;

    let reply = komorebi_link::send_query(&komorebi_link::Message::DebugWindow(hwnd))
        .wrap_err("could not ask komorebi; is it running?")?;
    Ok(WindowProbe {
        hwnd,
        komorebi_says: if reply.trim().is_empty() {
            "komorebi gave no answer (it may be busy, paused, or know no such window)".to_string()
        } else {
            reply
        },
    })
}

/// Returns the result of a probe as text for a person.
///
/// A clock probe lists each gap, with how far each clock moved, and ends with the verdict; a capture
/// probe lists each segment with its frames, drops, file and latency, and says `(NOT on the host
/// clock)` where the timestamps do not look like the master clock's; a decode probe gives the time
/// to the first frame, the speed and each seek; a window probe gives komorebi's answer.
///
/// # Example
///
/// ```rust
/// use komorebi_recorder::doctor::{probe_text, ProbeReport, WindowProbe};
///
/// let probe = ProbeReport::Window(WindowProbe {
///     hwnd: 131_072,
///     komorebi_says: "would manage".to_string(),
/// });
/// assert_eq!(probe_text(&probe), "komorebi on window 131072:\nwould manage\n");
/// ```
#[must_use]
pub fn probe_text(probe: &ProbeReport) -> String {
    use std::fmt::Write as _;

    let mut out = String::new();
    match probe {
        ProbeReport::Clock(probe) => {
            let _ = writeln!(
                out,
                "read the clocks {} times over {:.0} s",
                probe.samples, probe.seconds
            );
            for gap in &probe.gaps {
                let _ = writeln!(
                    out,
                    "  at {:.0} s: wall clock +{:.1} s, master clock +{:.1} s{}",
                    gap.at_s,
                    gap.wall_s,
                    gap.master_s,
                    gap.reference_s
                        .iter()
                        .map(|(name, seconds)| format!(", {name} +{seconds:.1} s"))
                        .collect::<String>()
                );
            }
            let _ = writeln!(out, "{}", probe.verdict);
        }
        ProbeReport::Capture(probe) => {
            let _ = writeln!(
                out,
                "recorded {:.0} s into {}",
                probe.seconds, probe.session
            );
            for display in &probe.displays {
                let _ = writeln!(
                    out,
                    "  display {} segment {}: {}x{}  {} frames, {} dropped, {} idle callbacks; {} \
                     ({} bytes, plays {}); latency {} .. {} ms{}{}",
                    display.display,
                    display.segment,
                    display.width_px,
                    display.height_px,
                    display.frames,
                    display.dropped,
                    display.idle_callbacks,
                    display.file,
                    display.file_bytes,
                    display
                        .file_duration_s
                        .map_or("no".to_string(), |seconds| format!("{seconds:.2} s")),
                    display.latency.min_ns / 1_000_000,
                    display.latency.max_ns / 1_000_000,
                    if display.looks_like_host_clock {
                        ""
                    } else {
                        " (NOT on the host clock)"
                    },
                    display
                        .error
                        .as_ref()
                        .map(|error| format!("; error: {error}"))
                        .unwrap_or_default(),
                );
            }
        }
        ProbeReport::Decode(probe) => {
            let _ = writeln!(
                out,
                "{}: {}x{}, {:.2} s\n  first frame {:.0} ms, then {:.0} frames/s ({} frames)",
                probe.file,
                probe.width,
                probe.height,
                probe.duration_s,
                probe.first_frame_ms,
                probe.decode_fps,
                probe.frames_decoded
            );
            for seek in &probe.seeks {
                let _ = writeln!(
                    out,
                    "  asked for {:.3} s, got {}",
                    seek.asked_s,
                    seek.got_s
                        .map_or("no frame".to_string(), |got| format!("{got:.3} s"))
                );
            }
        }
        ProbeReport::Window(probe) => {
            let _ = writeln!(
                out,
                "komorebi on window {}:\n{}",
                probe.hwnd, probe.komorebi_says
            );
        }
    }

    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn reading(wall_s: f64, master_s: f64, biased_s: f64, unbiased_s: f64) -> Reading {
        let ns = |seconds: f64| (seconds * 1e9) as u64;
        Reading {
            wall_us: (1_800_000_000.0 + wall_s) as i64 * 1_000_000
                + ((wall_s.fract()) * 1e6) as i64,
            mono_ns: ns(master_s),
            reference: vec![
                ReferenceClock {
                    name: "biased".to_string(),
                    counts_sleep: true,
                    ns: ns(biased_s),
                },
                ReferenceClock {
                    name: "unbiased".to_string(),
                    counts_sleep: false,
                    ns: ns(unbiased_s),
                },
            ],
        }
    }

    #[test]
    fn a_probe_with_no_sleep_says_it_learned_nothing() {
        let readings: Vec<Reading> = (0..8)
            .map(|step| {
                let at = f64::from(step) * 0.25;
                reading(at, at, at, at)
            })
            .collect();

        let probe = analyse_clock(&readings);
        assert!(probe.gaps.is_empty());
        assert!(
            probe.verdict.contains("no sleep happened"),
            "{}",
            probe.verdict
        );
        assert_eq!(probe.samples, 8);
    }

    #[test]
    fn a_clock_that_paused_through_a_sleep_is_called_paused() {
        // 60 s asleep: the wall clock and the biased clock move, the master and unbiased do not
        let readings = vec![
            reading(0.0, 0.0, 0.0, 0.0),
            reading(0.25, 0.25, 0.25, 0.25),
            reading(60.5, 0.5, 60.5, 0.5),
            reading(60.75, 0.75, 60.75, 0.75),
        ];

        let probe = analyse_clock(&readings);
        assert_eq!(probe.gaps.len(), 1);
        let gap = &probe.gaps[0];
        assert!((gap.wall_s - 60.25).abs() < 0.01, "{gap:?}");
        assert!((gap.master_s - 0.25).abs() < 0.01, "{gap:?}");
        assert!(probe.verdict.contains("PAUSED"), "{}", probe.verdict);
        assert!(
            gap.reference_s
                .iter()
                .any(|(name, s)| name == "biased" && *s > 60.0)
        );
        assert!(
            gap.reference_s
                .iter()
                .any(|(name, s)| name == "unbiased" && *s < 1.0)
        );
    }

    #[test]
    fn a_clock_that_kept_counting_is_called_counting() {
        let readings = vec![
            reading(0.0, 0.0, 0.0, 0.0),
            reading(60.25, 60.25, 60.25, 0.25),
            reading(60.5, 60.5, 60.5, 0.5),
        ];

        let probe = analyse_clock(&readings);
        assert!(probe.verdict.contains("KEPT COUNTING"), "{}", probe.verdict);
        assert!(probe.verdict.contains("power events"));
    }

    #[test]
    fn a_clock_in_between_is_not_called_either() {
        let readings = vec![
            reading(0.0, 0.0, 0.0, 0.0),
            reading(100.0, 50.0, 100.0, 0.0),
        ];
        let probe = analyse_clock(&readings);
        assert!(
            probe.verdict.contains("neither paused nor counting"),
            "{}",
            probe.verdict
        );
    }

    #[test]
    fn an_old_windows_is_warned_about_its_own_windows_being_in_the_video() {
        let host = |version: &str| Host {
            os: "windows".to_string(),
            arch: "x86_64".to_string(),
            os_version: Some(version.to_string()),
            clock: platform::ClockKind::Qpc,
        };

        assert!(own_windows_warning(&host("10.0.18363")).is_some());
        assert_eq!(own_windows_warning(&host("10.0.19041")), None);
        assert_eq!(own_windows_warning(&host("10.0.26100")), None);
        // a Mac does not need this, and an unreadable version is not guessed at
        assert_eq!(own_windows_warning(&Host::legacy_macos(None)), None);
        assert_eq!(own_windows_warning(&host("not a version")), None);
    }

    #[test]
    fn the_report_describes_this_machine_and_roundtrips_to_json() {
        let report = report();
        assert_eq!(report.recorder.host.os, std::env::consts::OS);
        assert!(report.clock.monotonic);
        assert!(report.clock.resolution_ns > 0);

        let json = serde_json::to_value(&report).expect("json");
        assert_eq!(
            json["komorebi"]["flavor"],
            Flavor::of_host().to_string().to_lowercase()
        );
        assert!(!text(&report).is_empty());
    }
}
