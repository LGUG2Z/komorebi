//! `record`, `gui` and `displays`: the arguments of a recording and the headless recorder.
//!
//! [`RecordArgs`] is the one set of options for both ways of recording: `record` runs without a
//! window and `gui` opens the recorder window with the same options as its starting settings.
//! [`RecordArgs::to_options`] turns them into the [`RecordOptions`] that [`recorder::record`] takes.
//!
//! `record` installs a Ctrl-C handler that asks the recorder to stop (`--duration` stops it too),
//! runs the recorder on a thread of its own, and meanwhile prints to standard error: the
//! session directory and any start-up warnings once, then a line of counters (time, frames, bytes
//! of komorebi's log, events, OS trace events) that is rewritten every half second. When the
//! recorder has finished it prints the session directory, the length of the video, what was
//! recorded, any warnings and how to query the session. The recorder itself, and what it keeps
//! recording through (monitors coming and going, sleep, lock), is in [`crate::recorder`]. `displays`
//! lists the displays that can be recorded, one a line: the id (what `--display` takes), the size in
//! points and the position.
//!
//! The commands need the outside world (a screen, and komorebi unless `--no-komorebi`), so they have
//! no example; [`RecordArgs::to_options`] does.

use crate::capture::CaptureConfig;
use crate::capture::Codec;
use crate::platform::capture;
use crate::recorder;
use crate::recorder::AppsMode;
use crate::recorder::Live;
use crate::recorder::OsLogOptions;
use crate::recorder::RecordOptions;
use crate::session::Session;
use crate::sources::oslog::OsLogLevel;
use crate::timecode;
use clap::Args;
use color_eyre::eyre;
use color_eyre::eyre::WrapErr;
use std::path::PathBuf;
use std::time::Duration;

/// The options of a recording, shared by `record` and `gui`.
///
/// The default is a recording with no flags given: every display, frames as fast as the display
/// refreshes at logical resolution, komorebi and the OS trace included, no time limit.
#[derive(Args, Clone, Debug)]
pub struct RecordArgs {
    /// Session directory to create [default: in komorebi's data directory, under recordings/,
    /// named by the UTC time]
    #[arg(long)]
    pub out: Option<PathBuf>,
    /// Stop automatically after this many seconds
    #[arg(long, value_name = "SECONDS")]
    pub duration: Option<f64>,
    /// Display id to record (repeatable; see `displays`) [default: every display]
    #[arg(long = "display", value_name = "ID")]
    pub displays: Vec<u32>,
    /// Maximum video frame rate, 0 for as fast as the display refreshes (frames are only produced
    /// when the screen changes)
    #[arg(long, default_value_t = 0)]
    pub fps: u32,
    /// Record at native pixel resolution instead of logical points
    #[arg(long)]
    pub retina: bool,
    /// Video codec
    #[arg(long, value_enum, default_value_t = Codec::default())]
    pub codec: Codec,
    /// Do not capture komorebi's log or events (video and OS trace only)
    #[arg(long)]
    pub no_komorebi: bool,
    /// komorebi for Windows hides a window by cloaking it in place, which its state cannot show.
    /// So each window event is followed by a read of what Windows says about that window (visible,
    /// cloaked, minimized, where it is) and, now and then, komorebi's own answer to "would you manage
    /// this window, and why", so that a window komorebi shows but Windows has hidden (or the
    /// reverse) can be found. This is what a recording on Windows does unless told not to
    /// (`--no-probe-windows`); on a Mac there is nothing to read, and asking for it is an error
    #[arg(long, conflicts_with = "no_probe_windows")]
    pub probe_windows: bool,
    /// Do not read what Windows says about each window (see `--probe-windows`, which Windows
    /// recordings do by default): the recording then cannot show a window komorebi has in front that
    /// Windows has hidden, such as a ghost tile
    #[arg(long)]
    pub no_probe_windows: bool,
    /// Also sample every top-level window's rect, visibility and minimized (and, on Windows,
    /// cloaked) state at HZ samples a second [default when given without a value: 30; 1 to 120]
    /// into `windows.jsonl`, so that "which window was where in this frame" has an answer for
    /// every frame; `sheet --windows` draws it. Written only when something changed, plus a full
    /// row each second, so a still desktop costs almost nothing. Off unless asked for
    #[arg(
        long,
        value_name = "HZ",
        num_args = 0..=1,
        require_equals = true,
        default_missing_value = "30"
    )]
    pub window_track: Option<u32>,
    /// Do not capture the screen: record everything else (komorebi's events and log, the OS
    /// trace and incidents) on the same clock
    #[arg(long, conflicts_with = "displays")]
    pub no_video: bool,
    /// Do not capture the OS trace at all (the unified log on macOS)
    #[arg(long)]
    pub no_oslog: bool,
    /// Which applications get their own OS trace file
    #[arg(long, value_enum, default_value_t = AppsMode::Managed)]
    pub oslog_apps: AppsMode,
    /// Also capture one unfiltered system-wide stream (very large)
    #[arg(long)]
    pub oslog_all: bool,
    /// Unified log level to capture
    #[arg(long, value_enum, default_value_t = OsLogLevel::Debug)]
    pub oslog_level: OsLogLevel,
    /// Replace the predicate of the `system` OS trace stream
    #[arg(long, value_name = "PREDICATE")]
    pub predicate: Option<String>,
    /// Add a system daemon to the `system` stream, e.g. `WindowServer` (repeatable; high volume)
    #[arg(long = "system-process", value_name = "NAME")]
    pub system_processes: Vec<String>,
    /// Also trace an application by name (repeatable)
    #[arg(long = "trace-process", value_name = "NAME")]
    pub trace_processes: Vec<String>,
    /// Also trace a process by pid (repeatable)
    #[arg(long = "trace-pid", value_name = "PID")]
    pub trace_pids: Vec<i32>,
    /// Never trace this application (repeatable), e.g. a chatty one
    #[arg(long = "oslog-exclude", value_name = "NAME")]
    pub oslog_exclude: Vec<String>,
}

impl Default for RecordArgs {
    fn default() -> Self {
        Self {
            out: None,
            duration: None,
            displays: Vec::new(),
            fps: 0,
            retina: false,
            codec: Codec::default(),
            no_komorebi: false,
            no_video: false,
            probe_windows: false,
            no_probe_windows: false,
            window_track: None,
            no_oslog: false,
            oslog_apps: AppsMode::Managed,
            oslog_all: false,
            oslog_level: OsLogLevel::Debug,
            predicate: None,
            system_processes: Vec::new(),
            trace_processes: Vec::new(),
            trace_pids: Vec::new(),
            oslog_exclude: Vec::new(),
        }
    }
}

impl RecordArgs {
    /// Returns these options as the [`RecordOptions`] that [`recorder::record`] takes.
    ///
    /// `--no-oslog` becomes `oslog.enabled = false`, `--duration` becomes a
    /// [`Duration`](std::time::Duration), and the options that name a display, application or
    /// process are copied as they are. Nothing is checked here: a display id that does not exist is
    /// found out when recording starts.
    ///
    /// # Example
    ///
    /// ```rust
    /// use komorebi_recorder::cli::RecordArgs;
    /// use std::time::Duration;
    ///
    /// let options = RecordArgs {
    ///     duration: Some(2.5),
    ///     displays: vec![1],
    ///     no_oslog: true,
    ///     ..RecordArgs::default()
    /// }
    /// .to_options();
    ///
    /// assert_eq!(options.duration, Some(Duration::from_millis(2500)));
    /// assert_eq!(options.displays, [1]);
    /// assert!(!options.oslog.enabled);
    /// assert_eq!(options.capture.fps, 0);
    /// ```
    #[must_use]
    pub fn to_options(&self) -> RecordOptions {
        RecordOptions {
            out: self.out.clone(),
            displays: self.displays.clone(),
            capture: CaptureConfig {
                fps: self.fps,
                retina: self.retina,
                codec: self.codec,
            },
            oslog: OsLogOptions {
                enabled: !self.no_oslog,
                apps: self.oslog_apps,
                all: self.oslog_all,
                level: self.oslog_level,
                predicate: self.predicate.clone(),
                system_processes: self.system_processes.clone(),
                trace_processes: self.trace_processes.clone(),
                trace_pids: self.trace_pids.clone(),
                exclude: self.oslog_exclude.clone(),
            },
            duration: self.duration.map(Duration::from_secs_f64),
            no_komorebi: self.no_komorebi,
            no_video: self.no_video,
            probe_windows: self.probes_windows(),
            window_track: self.window_track,
        }
    }

    /// Returns whether the recording reads what Windows says about each window.
    ///
    /// It does on Windows, unless `--no-probe-windows` says not to, and it does not elsewhere,
    /// where there is nothing to read: `--probe-windows` asked for outright is kept as asked, so
    /// that a host that cannot do it says so instead of recording without it. The recorder window
    /// takes its options from [`RecordArgs::default`], so it follows the same rule.
    ///
    /// # Example
    ///
    /// ```rust
    /// use komorebi_recorder::cli::RecordArgs;
    ///
    /// // the default follows the host: Windows can be asked, a Mac cannot
    /// assert_eq!(RecordArgs::default().probes_windows(), cfg!(windows));
    ///
    /// // and can be turned off
    /// let off = RecordArgs {
    ///     no_probe_windows: true,
    ///     ..RecordArgs::default()
    /// };
    /// assert!(!off.probes_windows());
    ///
    /// // asking for it where it cannot be done is kept, to be refused when recording starts
    /// let asked = RecordArgs {
    ///     probe_windows: true,
    ///     ..RecordArgs::default()
    /// };
    /// assert!(asked.probes_windows());
    /// ```
    #[must_use]
    pub fn probes_windows(&self) -> bool {
        !self.no_probe_windows && (self.probe_windows || cfg!(windows))
    }
}

pub(super) fn displays() -> eyre::Result<()> {
    for display in capture::active_displays()? {
        println!(
            "{}  {}x{} points at ({}, {})",
            display.id,
            display.width_points,
            display.height_points,
            display.frame[0],
            display.frame[1]
        );
    }

    Ok(())
}

pub(super) fn record(args: &RecordArgs) -> eyre::Result<()> {
    let live = Live::new();
    let handler_live = live.clone();
    ctrlc::set_handler(move || handler_live.request_stop())
        .wrap_err("could not install the Ctrl-C handler")?;

    let options = args.to_options();
    let worker_live = live.clone();
    let worker = std::thread::Builder::new()
        .name("recorder".to_string())
        .spawn(move || recorder::record(&options, &worker_live))?;

    let mut announced = false;
    while !worker.is_finished() {
        std::thread::sleep(Duration::from_millis(500));
        let snapshot = live.snapshot();

        if snapshot.phase == recorder::Phase::Recording {
            if !announced {
                announced = true;
                if let Some(dir) = &snapshot.session_dir {
                    eprintln!("recording to {} (Ctrl-C to stop)", dir.display());
                }
                for warning in &snapshot.warnings {
                    eprintln!("warning: {warning}");
                }
            }

            eprint!(
                "\r  {}  frames {}  komorebi log {} B  events {}  OS trace {} events   ",
                timecode::format(snapshot.elapsed_s),
                snapshot.frames,
                snapshot.log_bytes,
                snapshot.events,
                snapshot.oslog_events,
            );
        }
    }

    eprintln!();
    let dir = worker
        .join()
        .map_err(|_| eyre::Report::msg("the recording thread panicked"))??;

    let session = Session::open(&dir)?;
    let manifest = session.manifest()?;
    let (_, duration) = manifest.video_span();
    println!("session: {}", dir.display());
    if manifest.displays.is_empty() {
        println!("length:  {duration:.1}s (no video)");
    } else {
        println!("video:   {duration:.1}s");
    }
    println!(
        "recorded: {} komorebi log bytes, {} events, {} OS trace events across {} streams",
        manifest.log_segments.iter().map(|s| s.len()).sum::<u64>(),
        manifest.events.events,
        manifest
            .oslog
            .iter()
            .map(|stream| stream.events)
            .sum::<u64>(),
        manifest.oslog.len(),
    );
    for warning in &manifest.warnings {
        println!("warning: {warning}");
    }
    println!(
        "\nquery it:  komorebi-recorder at {} <timecode>\n           komorebi-recorder summary {}",
        dir.display(),
        dir.display()
    );

    Ok(())
}
