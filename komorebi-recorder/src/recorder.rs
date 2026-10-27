//! The recording lifecycle: start every source in the right order, keep recording through
//! whatever happens to the machine, then stop everything in reverse and index it.
//!
//! **Start order** makes the non-video sources a superset of the video span:
//! anchor -> OS log streams (wait until they announce they are filtering) -> subscriber socket and
//! initial `State` -> komorebi log offset -> video.
//!
//! **Stop order** is the reverse: video -> a short grace period for komorebi's `non_blocking` log
//! writer -> final log sync, final `State`, unsubscribe, terminate `log stream` -> finalize -> index.
//!
//! **While recording**, a laptop gets docked, undocked, put to sleep, woken with the lid opened and
//! locked, and none of that may lose what was already captured. The loop therefore watches for
//! each of those and carries on:
//!
//! - displays are reconciled several times a second ([`DisplayManager`]): a vanished display's
//!   video is finalized on the spot and a returning one starts a new segment;
//! - the wall and mach clocks are compared every iteration, so a sleep is noticed the moment the
//!   machine wakes and the anchors on either side of it are recorded exactly;
//! - `log stream` children that died are restarted into the same file, and a komorebi that exited
//!   or was restarted is re-subscribed to and its new process traced;
//! - everything is written as it arrives and the manifest is checkpointed every few seconds, so a
//!   crash or power loss leaves a session that `reindex` can recover.

use crate::analysis::flavor::Flavor;
use crate::capture::Capture;
use crate::capture::CaptureConfig;
use crate::capture::manager::DisplayManager;
use crate::capture::manager::Follow;
use crate::clock::Anchor;
use crate::clock::JumpDetector;
use crate::incidents::Draft;
use crate::incidents::IncidentLog;
use crate::incidents::kind;
use crate::lock::RecorderLock;
use crate::platform::capture;
use crate::power::PowerTracker;
use crate::session::KomorebiInfo;
use crate::session::Manifest;
use crate::session::Moment;
use crate::session::STATUS_COMPLETE;
use crate::session::STATUS_RECORDING;
use crate::session::Session;
use crate::sources::komorebi_events;
use crate::sources::komorebi_events::Discovery;
use crate::sources::komorebi_events::EventRecorder;
use crate::sources::komorebi_log;
use crate::sources::komorebi_log::LogTailer;
use crate::sources::managed_apps;
use crate::sources::managed_apps::AppRecord;
use crate::sources::managed_apps::AppTracker;
use crate::sources::oslog;
use crate::sources::oslog::OsLogLevel;
use crate::sources::oslog::StreamLive;
use crate::sources::oslog::StreamSet;
use crate::sources::wintrace::TraceSet;
use crate::system_state;
use crate::system_state::SessionTracker;
use crate::timeline;
use chrono::Utc;
use clap::ValueEnum;
use color_eyre::eyre;
use crossbeam_channel::Receiver;
use crossbeam_channel::RecvTimeoutError;
use parking_lot::Mutex;
use serde_json::Value;
use std::collections::BTreeMap;
use std::collections::BTreeSet;
use std::collections::HashMap;
use std::collections::HashSet;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::AtomicBool;
use std::sync::atomic::Ordering;
use std::time::Duration;
use std::time::Instant;
use sysinfo::ProcessRefreshKind;
use sysinfo::ProcessesToUpdate;
use sysinfo::System;
use sysinfo::UpdateKind;

/// How often the clock anchors are refreshed and the manifest checkpointed.
const ANCHOR_INTERVAL: Duration = Duration::from_secs(5);
/// How often attached displays are reconciled with what is recording.
const DISPLAY_POLL_INTERVAL: Duration = Duration::from_millis(250);
/// How often the session lock state is read.
const SESSION_POLL_INTERVAL: Duration = Duration::from_millis(500);
/// How often log streams, komorebi and its log file are checked.
const HOUSEKEEPING_INTERVAL: Duration = Duration::from_secs(1);
/// How often to look for komorebi while it is not running.
const KOMOREBI_SEARCH_INTERVAL: Duration = Duration::from_secs(2);
const LOG_STREAM_READY_TIMEOUT: Duration = Duration::from_secs(2);
/// komorebi's tracing output goes through a `non_blocking` writer, so lines lag a few ms.
const LOG_FLUSH_GRACE: Duration = Duration::from_millis(500);

/// How often stream rates are checked, and the rate above which a stream is called out.
const RATE_CHECK_INTERVAL: Duration = Duration::from_secs(2);
const ABNORMAL_EVENTS_PER_SECOND: f64 = 1500.0;

/// Which applications get their own OS trace stream.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, ValueEnum)]
pub enum AppsMode {
    /// Every process owning a window in komorebi's `State`, discovered live.
    #[default]
    Managed,
    /// Every process with an ordinary window, to also cover windows komorebi ignores.
    AllGui,
    /// No per-app streams.
    None,
}

/// What to record of the OS trace, and how.
#[derive(Debug, Clone)]
pub struct OsLogOptions {
    /// `false` disables the unified log entirely.
    pub enabled: bool,
    /// Which applications get a stream of their own.
    pub apps: AppsMode,
    /// Also run one unfiltered stream (system-wide debug volume is very large).
    pub all: bool,
    /// The level the streams are asked for.
    pub level: OsLogLevel,
    /// Replaces the default predicate of the `system` stream.
    pub predicate: Option<String>,
    /// Daemons to add to the `system` stream (e.g. `WindowServer`; high volume).
    pub system_processes: Vec<String>,
    /// Programs, by name, to give a stream of their own even if komorebi does not manage them.
    pub trace_processes: Vec<String>,
    /// Processes, by id, to give a stream of their own even if komorebi does not manage them.
    pub trace_pids: Vec<i32>,
    /// App names that never get a stream.
    pub exclude: Vec<String>,
}

impl Default for OsLogOptions {
    fn default() -> Self {
        Self {
            enabled: true,
            apps: AppsMode::default(),
            all: false,
            level: OsLogLevel::Debug,
            predicate: None,
            system_processes: Vec::new(),
            trace_processes: Vec::new(),
            trace_pids: Vec::new(),
            exclude: Vec::new(),
        }
    }
}

/// What to record, and where to put it.
#[derive(Debug, Clone, Default)]
pub struct RecordOptions {
    /// The session directory to create. `None` makes one named for the UTC time under `recordings/` in komorebi's data
    /// directory.
    pub out: Option<PathBuf>,
    /// Display ids to record; empty means every display, including any connected later.
    pub displays: Vec<u32>,
    /// How the screen is captured.
    pub capture: CaptureConfig,
    /// What is recorded of the OS trace.
    pub oslog: OsLogOptions,
    /// Stop after this long. `None` records until a stop is requested.
    pub duration: Option<Duration>,
    /// Do not touch komorebi at all: no subscription, state snapshots or log slice.
    pub no_komorebi: bool,
    /// Do not capture the screen: everything else is recorded, on the same clock. For a headless
    /// machine, or when what is on the screen must not be recorded.
    pub no_video: bool,
}

/// Where a recording is in its life.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Phase {
    /// Nothing has been started.
    #[default]
    Idle,
    /// The sources are being started, in order.
    Starting,
    /// Everything is recording.
    Recording,
    /// The sources are being stopped and the files finalized.
    Stopping,
    /// The recording is complete and indexed.
    Finished,
    /// The recording could not be made: [`LiveSnapshot::error`] says why.
    Failed,
}

/// What the UI shows while recording.
#[derive(Debug, Clone, Default)]
pub struct LiveSnapshot {
    /// Where the recording is in its life.
    pub phase: Phase,
    /// Seconds since the recording began.
    pub elapsed_s: f64,
    /// Video frames written so far, across displays.
    pub frames: u64,
    /// Frames the capture dropped so far, across displays.
    pub dropped: u64,
    /// How many displays are being recorded right now, and which are being retried.
    pub displays_recording: usize,
    /// The displays that are wanted but whose capture could not be started, which are tried again.
    pub displays_retrying: Vec<u32>,
    /// Bytes of komorebi's log recorded so far.
    pub log_bytes: u64,
    /// komorebi events recorded so far.
    pub events: u64,
    /// OS trace records recorded so far, across streams.
    pub oslog_events: u64,
    /// Bytes of OS trace recorded so far, across streams.
    pub oslog_bytes: u64,
    /// The OS trace streams, with what each has recorded.
    pub streams: Vec<StreamLive>,
    /// What the recorder has warned about so far, in words.
    pub warnings: Vec<String>,
    /// The most recent things that happened to the recording environment, newest last.
    pub incidents: Vec<String>,
    /// The session directory, once it has been created.
    pub session_dir: Option<PathBuf>,
    /// Why the recording failed, when [`phase`](Self::phase) is [`Phase::Failed`].
    pub error: Option<String>,
}

/// Shared between the recording thread and whoever drives it (the UI or ctrl-c handler).
///
/// The driver asks for a stop and reads snapshots; the recording thread publishes them and checks for the stop.
///
/// # Example
///
/// ```rust
/// use komorebi_recorder::recorder::Live;
/// use komorebi_recorder::recorder::Phase;
///
/// let live = Live::new();
/// assert_eq!(live.snapshot().phase, Phase::Idle);
/// assert!(!live.stop_requested());
///
/// live.request_stop();
/// assert!(live.stop_requested());
///
/// // ready for another recording
/// live.reset();
/// assert!(!live.stop_requested());
/// ```
#[derive(Default)]
pub struct Live {
    snapshot: Mutex<LiveSnapshot>,
    stop: AtomicBool,
}

impl Live {
    #[must_use]
    /// Returns a handle for a recording that has not started.
    pub fn new() -> Arc<Self> {
        Arc::new(Self::default())
    }

    #[must_use]
    /// Returns a copy of what the recording has published.
    pub fn snapshot(&self) -> LiveSnapshot {
        self.snapshot.lock().clone()
    }

    /// Asks the recording to stop. It stops, finalizing everything, the next time its loop looks.
    pub fn request_stop(&self) {
        self.stop.store(true, Ordering::SeqCst);
    }

    #[must_use]
    /// Returns `true` if and only if a stop has been requested since the last [`reset`](Self::reset).
    pub fn stop_requested(&self) -> bool {
        self.stop.load(Ordering::SeqCst)
    }

    /// Clears the stop request and the snapshot, ready for another recording.
    pub fn reset(&self) {
        self.stop.store(false, Ordering::SeqCst);
        *self.snapshot.lock() = LiveSnapshot::default();
    }

    fn update(&self, change: impl FnOnce(&mut LiveSnapshot)) {
        change(&mut self.snapshot.lock());
    }
}

/// komorebi's process, as found on this machine.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KomorebiProcess {
    /// The process id.
    pub pid: i32,
    /// The log level it is running at, and how that was found out.
    pub level: komorebi_log::DetectedLevel,
    /// The binary it is running, which can be asked what version it is.
    pub exe: Option<PathBuf>,
}

/// Whether a process name is komorebi's daemon (`komorebi.exe` on Windows).
fn is_komorebi(name: &std::ffi::OsStr) -> bool {
    name == "komorebi" || name == "komorebi.exe"
}

/// Finds the running komorebi daemon and works out how verbose it is, or returns `None` if there is none.
#[must_use]
pub fn find_komorebi() -> Option<KomorebiProcess> {
    let mut system = System::new();
    system.refresh_processes_specifics(
        ProcessesToUpdate::All,
        true,
        ProcessRefreshKind::nothing()
            .with_cmd(UpdateKind::Always)
            .with_environ(UpdateKind::Always)
            .with_exe(UpdateKind::Always),
    );

    let process = system
        .processes()
        .values()
        .find(|process| is_komorebi(process.name()))?;

    let strings = |values: &[std::ffi::OsString]| -> Vec<String> {
        values
            .iter()
            .map(|value| value.to_string_lossy().into_owned())
            .collect()
    };

    Some(KomorebiProcess {
        pid: i32::try_from(process.pid().as_u32()).ok()?,
        level: komorebi_log::level_from_process(
            &strings(process.cmd()),
            &strings(process.environ()),
        ),
        exe: process.exe().map(std::path::Path::to_path_buf),
    })
}

/// Ask a komorebi process which version it is.
fn komorebi_version_of(
    komorebi: &KomorebiProcess,
) -> Option<crate::sources::komorebi_version::KomorebiVersion> {
    crate::sources::komorebi_version::query(komorebi.exe.as_deref()?, Some(komorebi.pid))
}

/// Whether `pid` is still the komorebi daemon.
fn komorebi_is_running(pid: i32) -> bool {
    let Ok(pid) = u32::try_from(pid) else {
        return false;
    };

    let pid = sysinfo::Pid::from_u32(pid);
    let mut system = System::new();
    system.refresh_processes(ProcessesToUpdate::Some(&[pid]), true);
    system
        .process(pid)
        .is_some_and(|process| is_komorebi(process.name()))
}

/// What changed about komorebi's process.
#[derive(Debug, Clone, PartialEq, Eq)]
enum KomorebiChange {
    Exited(i32),
    Started(KomorebiProcess),
}

/// Follows komorebi's process across the recording: it can exit, be restarted, or not be running
/// at all when recording begins.
struct KomorebiWatch {
    enabled: bool,
    current: Option<i32>,
    known: Vec<i32>,
    next_search: Instant,
}

impl KomorebiWatch {
    fn new(enabled: bool, initial: Option<&KomorebiProcess>) -> Self {
        Self {
            enabled,
            current: initial.map(|komorebi| komorebi.pid),
            known: initial.map(|komorebi| komorebi.pid).into_iter().collect(),
            next_search: Instant::now() + KOMOREBI_SEARCH_INTERVAL,
        }
    }

    fn check(&mut self) -> Option<KomorebiChange> {
        if !self.enabled {
            return None;
        }

        if let Some(pid) = self.current {
            if komorebi_is_running(pid) {
                return None;
            }

            self.current = None;
            self.next_search = Instant::now();
            return Some(KomorebiChange::Exited(pid));
        }

        if Instant::now() < self.next_search {
            return None;
        }

        self.next_search = Instant::now() + KOMOREBI_SEARCH_INTERVAL;
        let found = find_komorebi()?;
        self.current = Some(found.pid);
        self.known.push(found.pid);
        Some(KomorebiChange::Started(found))
    }
}

/// A stream producing events faster than is useful to read.
#[derive(Debug, Clone, PartialEq)]
struct AbnormalStream {
    name: String,
    events_per_second: f64,
    megabytes_per_minute: f64,
}

/// Compare each stream's totals against an earlier baseline and return the ones whose rate since
/// then exceeds the threshold.
fn abnormal_streams(
    baseline: &HashMap<String, (u64, u64)>,
    now: &[StreamLive],
    elapsed: Duration,
) -> Vec<AbnormalStream> {
    let seconds = elapsed.as_secs_f64();
    if seconds <= 0.0 {
        return Vec::new();
    }

    now.iter()
        .filter_map(|stream| {
            let (events, bytes) = baseline.get(&stream.name).copied().unwrap_or((0, 0));
            let events_per_second = stream.events.saturating_sub(events) as f64 / seconds;
            let megabytes_per_minute =
                stream.bytes.saturating_sub(bytes) as f64 / seconds * 60.0 / 1_048_576.0;

            (events_per_second > ABNORMAL_EVENTS_PER_SECOND).then(|| AbnormalStream {
                name: stream.name.clone(),
                events_per_second,
                megabytes_per_minute,
            })
        })
        .collect()
}

/// Watches stream rates and says so, once per stream, when one is abnormal.
struct RateWatch {
    checked: Instant,
    baseline: HashMap<String, (u64, u64)>,
    warned: HashSet<String>,
}

impl RateWatch {
    fn new() -> Self {
        Self {
            checked: Instant::now(),
            baseline: HashMap::new(),
            warned: HashSet::new(),
        }
    }

    fn check(&mut self, streams: &[StreamLive]) -> Vec<String> {
        if self.checked.elapsed() < RATE_CHECK_INTERVAL {
            return Vec::new();
        }

        let mut warnings = Vec::new();
        for stream in abnormal_streams(&self.baseline, streams, self.checked.elapsed()) {
            if self.warned.insert(stream.name.clone()) {
                warnings.push(format!(
                    "OS trace stream '{}' is producing ~{:.0} events/s (~{:.0} MB/min); \
                     `log stream` can drop events under extreme load, so consider --oslog-exclude",
                    stream.name, stream.events_per_second, stream.megabytes_per_minute
                ));
            }
        }

        self.baseline = streams
            .iter()
            .map(|stream| (stream.name.clone(), (stream.events, stream.bytes)))
            .collect();
        self.checked = Instant::now();
        warnings
    }
}

fn process_name(pid: i32) -> Option<String> {
    let pid = sysinfo::Pid::from_u32(u32::try_from(pid).ok()?);
    let mut system = System::new();
    system.refresh_processes(ProcessesToUpdate::Some(&[pid]), true);
    system
        .process(pid)
        .map(|process| process.name().to_string_lossy().into_owned())
}

/// Everything that decides whether a process gets its own stream.
struct AppStreams {
    options: OsLogOptions,
    streams: StreamSet,
    /// The Windows OS trace, where `streams` (the Mac's `log stream` children) does not apply.
    trace: Option<TraceSet>,
    tracker: AppTracker,
    own_pid: i32,
    warnings: Vec<String>,
}

impl AppStreams {
    fn excluded(&self, name: &str) -> bool {
        self.options
            .exclude
            .iter()
            .any(|excluded| excluded.eq_ignore_ascii_case(name))
    }

    fn spawn_for(&mut self, app: &AppRecord) {
        if app.pid == self.own_pid || self.streams.has_pid(app.pid) || self.excluded(&app.exe) {
            return;
        }

        let spec = oslog::app_spec(&app.exe, app.pid, self.options.level);
        let file = spec.file.clone();
        match self.streams.spawn(spec) {
            Ok(()) => self.tracker.set_stream_file(app.pid, &file),
            Err(error) => self.warnings.push(format!(
                "could not trace {} ({}): {error}",
                app.exe, app.pid
            )),
        }
    }

    fn observe_state(&mut self, state: &Value) {
        if self.options.apps == AppsMode::None {
            return;
        }

        let owners = managed_apps::window_owners();
        let resolved = managed_apps::resolve(&managed_apps::collect_windows(state), &owners);
        for app in self.tracker.observe(resolved, "state") {
            self.spawn_for(&app);
        }
    }

    fn observe_pid(&mut self, pid: i32, origin: &str) {
        if self.options.apps == AppsMode::None || self.tracker.contains(pid) {
            return;
        }

        let name = managed_apps::window_owners()
            .values()
            .find(|owner| owner.pid == pid)
            .map(|owner| owner.name.clone())
            .filter(|name| !name.is_empty())
            .or_else(|| process_name(pid));

        // the process may already have gone; there is nothing to trace then
        let Some(name) = name else {
            return;
        };

        if let Some(app) = self.tracker.observe_pid(pid, name, origin) {
            self.spawn_for(&app);
        }
    }
}

fn drain_discovery(discovery: &Receiver<Discovery>, apps: &mut AppStreams, wait: Duration) {
    let mut first = true;
    loop {
        let message = if first {
            first = false;
            match discovery.recv_timeout(wait) {
                Ok(message) => Some(message),
                Err(RecvTimeoutError::Timeout) => None,
                Err(RecvTimeoutError::Disconnected) => {
                    // komorebi is not being watched, so nothing will ever arrive; still pace the loop
                    std::thread::sleep(wait);
                    None
                }
            }
        } else {
            discovery.try_recv().ok()
        };

        match message {
            Some(Discovery::State(state)) => apps.observe_state(&state),
            Some(Discovery::Pid(pid)) => apps.observe_pid(pid, "event"),
            None => return,
        }
    }
}

/// Runs one complete recording. Blocks until stopped (via [`Live::request_stop`] or the duration)
/// and returns the finished session directory.
///
/// `live` is how the caller watches it and asks it to stop. On a host that is not a Mac the OS trace has no
/// per-application streams, and a host with no OS trace records without one.
///
/// # Errors
///
/// Returns an error if the recording could not be started or finished: screen capture is wanted and permission is
/// missing, no display can be listed or none could be recorded, a display asked for does not exist, the session
/// directory cannot be created or already holds a session, or the manifest cannot be written.
pub fn record(options: &RecordOptions, live: &Live) -> eyre::Result<PathBuf> {
    live.update(|snapshot| snapshot.phase = Phase::Starting);

    // A host with no OS trace records without one (and `begin` says so). The per-application
    // streams are the Mac's `log stream` children; Windows has one trace for every window.
    let adjusted;
    let options = if cfg!(target_os = "macos") {
        options
    } else {
        adjusted = RecordOptions {
            oslog: OsLogOptions {
                enabled: options.oslog.enabled && crate::platform::HAS_OS_TRACE,
                apps: AppsMode::None,
                ..options.oslog.clone()
            },
            ..options.clone()
        };
        &adjusted
    };

    let result = Recording::begin(options, live).map(|mut recording| {
        recording.run();
        recording.finish()
    });
    let result = result.and_then(|finished| finished);

    live.update(|snapshot| match &result {
        Ok(_) => snapshot.phase = Phase::Finished,
        Err(error) => {
            snapshot.phase = Phase::Failed;
            snapshot.error = Some(format!("{error:#}"));
        }
    });

    result
}

/// What to record about a clock jump: the anchors on either side of it (exactly what maps wall
/// time through the discontinuity) and the incident describing it.
fn jump_response(jump: &crate::clock::Jump) -> (Vec<Anchor>, Draft) {
    let draft = if jump.seconds >= 0.0 {
        Draft::new(
            kind::WAKE,
            format!(
                "the machine was suspended for about {:.1}s (or the clock stepped forward); the host \
                 clock paused, so video time skips it, and anything running was frozen for the duration",
                jump.seconds
            ),
        )
    } else {
        Draft::new(
            kind::CLOCK_STEP,
            format!("the wall clock was stepped back by {:.1}s", -jump.seconds),
        )
    };

    (vec![jump.before, jump.after], draft)
}

/// A "next due" timer for the periodic jobs in the recording loop.
struct Every {
    interval: Duration,
    next: Instant,
}

impl Every {
    fn new(interval: Duration) -> Self {
        Self {
            interval,
            next: Instant::now() + interval,
        }
    }

    fn due(&mut self) -> bool {
        if Instant::now() < self.next {
            return false;
        }

        self.next = Instant::now() + self.interval;
        true
    }

    /// Make the next [`due`](Self::due) fire immediately.
    fn now(&mut self) {
        self.next = Instant::now();
    }
}

/// A recording in progress.
struct Recording<'a> {
    options: &'a RecordOptions,
    live: &'a Live,
    session: Session,
    /// Held for the whole recording so other commands can tell it is alive; see [`crate::lock`].
    _lock: Option<RecorderLock>,
    manifest: Manifest,
    anchors: Vec<Anchor>,
    data_dir: PathBuf,
    incidents: IncidentLog,
    apps: AppStreams,
    tailer: Option<LogTailer>,
    events: Option<EventRecorder>,
    discovery_rx: Receiver<Discovery>,
    displays: DisplayManager<Capture>,
    komorebi: KomorebiWatch,
    jumps: JumpDetector,
    /// Where the OS tells us of sleep, wake, the lid and the displays' power, if it can.
    power: Option<crate::platform::power::Watcher>,
    power_tracker: PowerTracker,
    /// When the captures were last replaced because the machine slept, however it was noticed.
    last_replaced: Option<Instant>,
    session_state: SessionTracker,
    warnings: Vec<String>,
    rates: RateWatch,
    began: Instant,
    anchor_timer: Every,
    display_timer: Every,
    session_timer: Every,
    housekeeping_timer: Every,
}

impl<'a> Recording<'a> {
    /// Start every source, in the order that makes the non-video sources a superset of the video.
    #[allow(clippy::too_many_lines)]
    fn begin(options: &'a RecordOptions, live: &'a Live) -> eyre::Result<Self> {
        // without video there is no screen to ask about, or permission to ask for
        let present = if options.no_video {
            Vec::new()
        } else {
            capture::ensure_permission()?;
            capture::active_displays()?
        };
        let follow = if options.displays.is_empty() {
            Follow::All
        } else {
            for id in &options.displays {
                if !present.iter().any(|display| display.id == *id) {
                    eyre::bail!(
                        "display {id} not found; available: {}",
                        present
                            .iter()
                            .map(|display| display.id.to_string())
                            .collect::<Vec<_>>()
                            .join(", ")
                    );
                }
            }
            Follow::Only(options.displays.iter().copied().collect::<BTreeSet<_>>())
        };

        if present.is_empty() && !options.no_video {
            eyre::bail!("there are no active displays to record");
        }

        let session = Session::create(options.out.clone(), Utc::now())?;
        live.update(|snapshot| snapshot.session_dir = Some(session.dir.clone()));
        // best effort: failing to lock must not stop a recording
        let lock = RecorderLock::acquire(&session.dir).ok();

        let first_anchor = Anchor::sample();
        let start = Moment {
            mach_ns: first_anchor.mach_ns,
            wall_us: first_anchor.wall_us,
        };
        let mut manifest = Manifest::new(start, vec![first_anchor]);
        let mut warnings: Vec<String> = Vec::new();
        if !crate::platform::HAS_OS_TRACE {
            warnings.push(format!(
                "there is no OS trace on {} yet, so this recording has komorebi's own events and \
                 log and (unless --no-video) the screen, but nothing from the OS or other apps",
                manifest.host().describe()
            ));
        }
        let data_dir = crate::komorebi_link::data_dir().to_path_buf();
        let log_dir =
            komorebi_log::find_log_dir(Flavor::of_host(), &data_dir, &std::env::temp_dir());
        let incidents = IncidentLog::create(&session.dir)?;

        // --- komorebi ---------------------------------------------------------------------------
        let komorebi_process = if options.no_komorebi {
            None
        } else {
            find_komorebi()
        };
        manifest.komorebi = KomorebiInfo {
            flavor: Flavor::of_host(),
            running: komorebi_process.is_some(),
            pid: komorebi_process.as_ref().map(|komorebi| komorebi.pid),
            pids: komorebi_process
                .as_ref()
                .map(|komorebi| komorebi.pid)
                .into_iter()
                .collect(),
            log_level: komorebi_process
                .as_ref()
                .map(|komorebi| komorebi.level.clone()),
            versions: komorebi_process
                .as_ref()
                .and_then(komorebi_version_of)
                .into_iter()
                .collect(),
            subscriber: None,
            observed_levels: Vec::new(),
            data_dir: data_dir.display().to_string(),
            log_dir: (log_dir != data_dir).then(|| log_dir.display().to_string()),
        };

        match &komorebi_process {
            None if options.no_komorebi => {}
            None => warnings.push(
                "komorebi is not running: video and the system OS trace are recorded, and komorebi \
                 will be picked up if it starts"
                    .to_string(),
            ),
            Some(komorebi) => {
                if let Some(warning) = komorebi_log::level_warning(&komorebi.level.level) {
                    warnings.push(warning);
                }
            }
        }

        let start_state: Option<Value> = if komorebi_process.is_some() {
            match komorebi_events::snapshot_state(&session.path("state-start.json")) {
                Ok(state) => Some(state),
                Err(error) => {
                    warnings.push(format!("could not snapshot the starting state: {error:#}"));
                    None
                }
            }
        } else {
            None
        };

        // --- OS trace ---------------------------------------------------------------------------
        let mut apps = AppStreams {
            options: options.oslog.clone(),
            streams: StreamSet::new(&session.dir),
            trace: None,
            tracker: AppTracker::default(),
            own_pid: i32::try_from(std::process::id()).unwrap_or(0),
            warnings: Vec::new(),
        };

        let mut trace_drafts: Vec<Draft> = Vec::new();
        // Windows has no `log stream`: its own taps (see `sources::wintrace`) fill the same files
        if options.oslog.enabled && !cfg!(target_os = "macos") {
            match TraceSet::start(&session.dir) {
                Ok(trace) => {
                    trace_drafts.extend(trace.drafts());
                    apps.trace = Some(trace);
                }
                Err(error) => apps
                    .warnings
                    .push(format!("the OS trace could not be started: {error:#}")),
            }
        }

        if options.oslog.enabled && cfg!(target_os = "macos") {
            let level = options.oslog.level;

            if let Some(komorebi) = &komorebi_process {
                apps.tracker
                    .observe_pid(komorebi.pid, "komorebi".to_string(), "komorebi");
                if let Err(error) =
                    apps.streams
                        .spawn(oslog::komorebi_spec(komorebi.pid, level, true))
                {
                    apps.warnings
                        .push(format!("could not trace komorebi: {error:#}"));
                }
            }

            let predicate = options
                .oslog
                .predicate
                .clone()
                .unwrap_or_else(|| oslog::system_predicate(&options.oslog.system_processes));
            if let Err(error) = apps.streams.spawn(oslog::system_spec(predicate, level)) {
                apps.warnings
                    .push(format!("could not start the system OS trace: {error:#}"));
            }

            if options.oslog.all
                && let Err(error) = apps.streams.spawn(oslog::all_spec(level))
            {
                apps.warnings.push(format!(
                    "could not start the unfiltered OS trace: {error:#}"
                ));
            }

            // seed per-app streams from the starting state so they are ready before video starts
            if let Some(state) = &start_state {
                apps.observe_state(state);
            }

            let owners = managed_apps::window_owners();
            if options.oslog.apps == AppsMode::AllGui {
                for (pid, name) in managed_apps::gui_app_pids(&owners) {
                    if let Some(app) = apps.tracker.observe_pid(pid, name, "all-gui") {
                        apps.spawn_for(&app);
                    }
                }
            }

            for name in &options.oslog.trace_processes {
                let mut found = false;
                for owner in owners
                    .values()
                    .filter(|owner| owner.name.eq_ignore_ascii_case(name))
                {
                    found = true;
                    if let Some(app) =
                        apps.tracker
                            .observe_pid(owner.pid, owner.name.clone(), "explicit")
                    {
                        apps.spawn_for(&app);
                    }
                }

                if !found {
                    apps.warnings.push(format!(
                        "--trace-process {name}: no running process with a window has that name"
                    ));
                }
            }

            for pid in &options.oslog.trace_pids {
                let name = process_name(*pid).unwrap_or_else(|| pid.to_string());
                if let Some(app) = apps.tracker.observe_pid(*pid, name, "explicit") {
                    apps.spawn_for(&app);
                }
            }

            for name in apps.streams.wait_ready(LOG_STREAM_READY_TIMEOUT) {
                apps.warnings.push(format!(
                    "OS trace stream '{name}' was not ready within {}s; its first events may be missing",
                    LOG_STREAM_READY_TIMEOUT.as_secs()
                ));
            }
        }

        // --- komorebi events and log offset -------------------------------------------------------
        let (discovery_tx, discovery_rx) = crossbeam_channel::unbounded();
        let events = if options.no_komorebi {
            None
        } else {
            match EventRecorder::start(&session.dir, Some(discovery_tx)) {
                Ok(recorder) => {
                    manifest.komorebi.subscriber = Some(recorder.subscriber_name().to_string());
                    Some(recorder)
                }
                Err(error) => {
                    apps.warnings
                        .push(format!("komorebi events are not being recorded: {error:#}"));
                    None
                }
            }
        };

        let tailer = if options.no_komorebi {
            None
        } else {
            let log_start = komorebi_log::snapshot_start(&log_dir, Utc::now());
            match LogTailer::create(&log_dir, log_start, &komorebi_log::slice_path(&session.dir)) {
                Ok(tailer) => Some(tailer),
                Err(error) => {
                    warnings.push(format!("komorebi's log is not being copied: {error:#}"));
                    None
                }
            }
        };

        session.write_manifest(&manifest)?;

        // --- video --------------------------------------------------------------------------------
        let mut displays = DisplayManager::new(
            Capture::new(!options.no_video),
            options.capture.clone(),
            session.dir.clone(),
            follow,
        );
        let drafts = displays.start(crate::clock::mono_now_ns())?;

        let mut incidents = incidents;
        incidents.record_all(trace_drafts);
        if displays.recording_count() == 0 && !options.no_video {
            // everything started so far is torn down by the Drop impls
            let reasons: Vec<String> = drafts.iter().map(|draft| draft.detail.clone()).collect();
            eyre::bail!("no display could be recorded: {}", reasons.join("; "));
        }
        incidents.record(Draft::new(
            kind::RECORDER,
            format!(
                "recording started {}; komorebi {}",
                if options.no_video {
                    "without video".to_string()
                } else {
                    format!("on {} display(s)", displays.recording_count())
                },
                komorebi_process.as_ref().map_or_else(
                    || "not running".to_string(),
                    |komorebi| format!("pid {}", komorebi.pid)
                )
            ),
        ));
        incidents.record_all(drafts);

        let power = match crate::platform::power::start() {
            Ok(watcher) => watcher,
            Err(error) => {
                warnings.push(format!(
                    "could not listen for sleep, wake and the lid ({error:#}): a sleep is found \
                     from the clocks instead, if they show one"
                ));
                None
            }
        };

        let mut recording = Self {
            options,
            live,
            session,
            _lock: lock,
            manifest,
            anchors: vec![first_anchor],
            data_dir,
            incidents,
            apps,
            tailer,
            events,
            discovery_rx,
            displays,
            komorebi: KomorebiWatch::new(!options.no_komorebi, komorebi_process.as_ref()),
            jumps: JumpDetector::new(Anchor::sample()),
            power,
            power_tracker: PowerTracker::default(),
            last_replaced: None,
            session_state: SessionTracker::default(),
            warnings,
            rates: RateWatch::new(),
            began: Instant::now(),
            anchor_timer: Every::new(ANCHOR_INTERVAL),
            display_timer: Every::new(DISPLAY_POLL_INTERVAL),
            session_timer: Every::new(SESSION_POLL_INTERVAL),
            housekeeping_timer: Every::new(HOUSEKEEPING_INTERVAL),
        };

        // learn the starting lock state so only *changes* are reported
        let _ = recording.session_state.observe(system_state::read());

        live.update(|snapshot| snapshot.phase = Phase::Recording);
        recording.checkpoint();
        Ok(recording)
    }

    fn log(&mut self, drafts: Vec<Draft>) {
        self.incidents.record_all(drafts);
    }

    /// Run until stopped, riding out whatever happens to the machine.
    fn run(&mut self) {
        loop {
            if self.live.stop_requested()
                || self
                    .options
                    .duration
                    .is_some_and(|limit| self.began.elapsed() >= limit)
            {
                return;
            }

            // new apps appearing; also paces the loop
            drain_discovery(
                &self.discovery_rx,
                &mut self.apps,
                Duration::from_millis(50),
            );

            // 0. what the OS has told us: the machine slept or woke, the lid moved, the displays
            // went dark. Before the clocks are compared, so that a sleep it announced is handled
            // as that and not again as a jump.
            let heard = self
                .power
                .as_ref()
                .map(crate::platform::power::Watcher::drain)
                .unwrap_or_default();
            for event in heard {
                let reaction = self.power_tracker.observe(event);
                self.react(reaction);
            }

            // 1. did the machine sleep (or the clock step) since the last iteration?
            if let Some(jump) = self.jumps.observe(Anchor::sample()) {
                self.on_jump(jump);
            }

            // 2. docked, undocked, or woke with different displays
            if self.display_timer.due() {
                let drafts = self.displays.poll(crate::clock::mono_now_ns());
                self.log(drafts);
            }

            // 3. screen locked / unlocked, user switched
            if self.session_timer.due() {
                let drafts = self.session_state.observe(system_state::read());
                self.log(drafts);
            }

            if self.housekeeping_timer.due() {
                self.housekeeping();
            }

            if self.anchor_timer.due() {
                self.anchors.push(Anchor::sample());
                self.checkpoint();
            }

            self.update_live();
        }
    }

    /// Do what hearing from the OS calls for (see [`crate::power`]).
    fn react(&mut self, reaction: crate::power::Reaction) {
        self.anchors.extend(reaction.anchors);
        self.log(reaction.incidents);

        // streams are not trusted across a sleep (they can stall without saying so), so each
        // capture is finalized and a fresh one started; unless that was just done because the
        // clocks showed the sleep first
        if reaction.replace_captures && !self.replaced_recently() {
            self.replace_captures();
        }
        if reaction.poll_displays {
            self.display_timer.now();
        }
        if reaction.poll_session {
            self.session_timer.now();
        }
        if reaction.checkpoint {
            self.checkpoint();
        }
    }

    fn replaced_recently(&self) -> bool {
        self.last_replaced
            .is_some_and(|at| at.elapsed() < Duration::from_secs(5))
    }

    fn replace_captures(&mut self) {
        let drafts = self.displays.restart_all(
            crate::clock::mono_now_ns(),
            crate::capture::EndReason::Resumed,
        );
        self.last_replaced = Some(Instant::now());
        self.log(drafts);
    }

    /// The machine was suspended (or its clock stepped): bracket it, say so, and look at
    /// everything that may have changed while we were not running.
    fn on_jump(&mut self, jump: crate::clock::Jump) {
        // a sleep the OS announced has already been recorded and acted on; the anchors either side
        // of the jump are still wanted, the rest would say it twice
        let announced = jump.seconds >= 0.0
            && self.power.is_some()
            && self
                .last_replaced
                .is_some_and(|at| at.elapsed() < Duration::from_secs(30));
        if announced {
            self.anchors.extend([jump.before, jump.after]);
            return;
        }

        let (anchors, draft) = jump_response(&jump);
        self.anchors.extend(anchors);
        self.log(vec![draft]);

        // A sleep can leave a ScreenCaptureKit stream stalled without any error, so replace every
        // capture with a fresh one rather than trust it; each segment is finalized first.
        if jump.seconds >= 0.0 && !self.replaced_recently() {
            self.replace_captures();
        }

        // look at everything now rather than at the next tick
        self.display_timer.now();
        self.session_timer.now();
        self.housekeeping_timer.now();
        self.anchor_timer.now();
    }

    /// Once-a-second jobs: restart dead log streams, follow komorebi, sync its log.
    fn housekeeping(&mut self) {
        let drafts = self.apps.streams.reap();
        self.log(drafts);

        if let Some(change) = self.komorebi.check() {
            self.on_komorebi_change(change);
        }

        if let Some(tailer) = &mut self.tailer
            && let Err(error) = tailer.sync(Utc::now())
        {
            self.warn_once(format!("komorebi's log could not be copied: {error:#}"));
        }
    }

    fn on_komorebi_change(&mut self, change: KomorebiChange) {
        match change {
            KomorebiChange::Exited(pid) => {
                self.log(vec![Draft::new(
                    kind::KOMOREBI_EXITED,
                    format!(
                        "komorebi (pid {pid}) exited; its events and OS trace stop until it is \
                         running again"
                    ),
                )]);
            }
            KomorebiChange::Started(komorebi) => {
                let mut drafts = vec![Draft::new(
                    kind::KOMOREBI_STARTED,
                    format!(
                        "komorebi started (pid {}, log level {} from {})",
                        komorebi.pid, komorebi.level.level, komorebi.level.source
                    ),
                )];

                // komorebi forgets its subscribers when it restarts
                if let Some(events) = &self.events
                    && let Err(error) = events.subscribe()
                {
                    drafts.push(Draft::new(
                        kind::RECORDER,
                        format!("could not subscribe to the new komorebi: {error}"),
                    ));
                }

                if self.options.oslog.enabled && cfg!(target_os = "macos") {
                    self.apps
                        .tracker
                        .observe_pid(komorebi.pid, "komorebi".to_string(), "komorebi");
                    let first = !self.apps.streams.has("komorebi");
                    if let Err(error) = self.apps.streams.spawn(oslog::komorebi_spec(
                        komorebi.pid,
                        self.options.oslog.level,
                        first,
                    )) {
                        drafts.push(Draft::new(
                            kind::RECORDER,
                            format!("could not trace the new komorebi: {error:#}"),
                        ));
                    }
                }

                if let Some(warning) = komorebi_log::level_warning(&komorebi.level.level) {
                    self.warn_once(warning);
                }

                if let Some(version) = komorebi_version_of(&komorebi) {
                    self.manifest.komorebi.versions.push(version);
                }

                self.manifest.komorebi.running = true;
                self.manifest.komorebi.pid = Some(komorebi.pid);
                self.manifest.komorebi.log_level = Some(komorebi.level);
                self.manifest.komorebi.pids = self.komorebi.known.clone();

                self.log(drafts);
            }
        }
    }

    fn warn_once(&mut self, warning: String) {
        if !self.warnings.contains(&warning) {
            self.warnings.push(warning);
        }
    }

    fn app_records(&self, infos: &[oslog::StreamInfo]) -> Vec<AppRecord> {
        let mut records = self.apps.tracker.records();
        records.retain(|record| record.origin != "komorebi");
        for record in &mut records {
            if let Some(info) = infos.iter().find(|info| info.spec.pid == Some(record.pid)) {
                record.events = info.events;
                record.bytes = info.bytes;
            }
        }

        records
    }

    /// Write the manifest as it stands. Called every few seconds: if the recorder dies, this is
    /// what `reindex` recovers from.
    fn checkpoint(&mut self) {
        let mut infos = self.apps.streams.checkpoint();
        infos.extend(
            self.apps
                .trace
                .as_ref()
                .map(TraceSet::checkpoint)
                .unwrap_or_default(),
        );

        self.manifest.status = STATUS_RECORDING.to_string();
        self.manifest.anchors.clone_from(&self.anchors);
        self.manifest.apply_segments(self.displays.checkpoint());
        self.manifest.oslog.clone_from(&infos);
        self.manifest.apps = self.app_records(&infos);
        if let Some(tailer) = &self.tailer {
            self.manifest.log_segments = tailer.segments().to_vec();
        }

        if let Some(events) = &self.events {
            let counters = events.counters();
            self.manifest.events = komorebi_events::EventSummary {
                events: counters.events.load(Ordering::Relaxed),
                state_snapshots: counters.states.load(Ordering::Relaxed),
                bytes: counters.bytes.load(Ordering::Relaxed),
                parse_errors: counters.parse_errors.load(Ordering::Relaxed),
            };
        }

        self.manifest.warnings = self.all_warnings();
        let _ = self.session.write_manifest(&self.manifest);
    }

    fn all_warnings(&self) -> Vec<String> {
        let mut warnings = self.warnings.clone();
        warnings.extend(self.apps.warnings.iter().cloned());
        warnings
    }

    fn update_live(&mut self) {
        let (frames, dropped) = self.displays.totals();
        let log_bytes = self.tailer.as_ref().map_or(0, |tailer| {
            tailer
                .segments()
                .iter()
                .map(komorebi_log::LogSegment::len)
                .sum()
        });
        let mut streams = self.apps.streams.snapshot();
        streams.extend(
            self.apps
                .trace
                .as_ref()
                .map(TraceSet::snapshot)
                .unwrap_or_default(),
        );
        let event_count = self
            .events
            .as_ref()
            .map_or(0, |events| events.counters().events.load(Ordering::Relaxed));

        for warning in self.rates.check(&streams) {
            self.warn_once(warning);
        }

        let warnings = self.all_warnings();
        let incidents = self
            .incidents
            .recent()
            .iter()
            .map(|incident| {
                let at = self.manifest.video_seconds(incident.mach_ns);
                format!(
                    "{} {}: {}",
                    crate::timecode::format(at.max(0.0)),
                    incident.kind,
                    incident.detail
                )
            })
            .collect();

        self.live.update(|snapshot| {
            snapshot.elapsed_s = self.began.elapsed().as_secs_f64();
            snapshot.frames = frames;
            snapshot.dropped = dropped;
            snapshot.displays_recording = self.displays.recording_count();
            snapshot.displays_retrying = self.displays.retrying();
            snapshot.log_bytes = log_bytes;
            snapshot.events = event_count;
            snapshot.oslog_events = streams.iter().map(|stream| stream.events).sum();
            snapshot.oslog_bytes = streams.iter().map(|stream| stream.bytes).sum();
            snapshot.streams = streams;
            snapshot.warnings = warnings;
            snapshot.incidents = incidents;
        });
    }

    /// Stop everything in reverse order, then finalize and index the session.
    fn finish(mut self) -> eyre::Result<PathBuf> {
        self.live
            .update(|snapshot| snapshot.phase = Phase::Stopping);
        let video_end = Moment::now();
        let results = self.displays.shutdown();

        std::thread::sleep(LOG_FLUSH_GRACE);
        let stop = Moment::now();
        self.anchors.push(Anchor {
            wall_us: stop.wall_us,
            mach_ns: stop.mach_ns,
        });

        if let Some(tailer) = &mut self.tailer
            && let Err(error) = tailer.sync(Utc::now())
        {
            self.warnings
                .push(format!("could not copy komorebi's log: {error:#}"));
        }

        if self.komorebi.current.is_some()
            && let Err(error) =
                komorebi_events::snapshot_state(&self.session.path("state-end.json"))
        {
            self.warnings
                .push(format!("could not snapshot the ending state: {error:#}"));
        }

        let event_summary = self
            .events
            .take()
            .map(EventRecorder::stop)
            .unwrap_or_default();
        let mut stream_infos = self.apps.streams.stop();
        if let Some(trace) = &mut self.apps.trace {
            stream_infos.extend(trace.stop());
        }

        // --- manifest -----------------------------------------------------------------------------
        self.manifest.status = STATUS_COMPLETE.to_string();
        self.manifest.stop = Some(stop);
        self.manifest.anchors = std::mem::take(&mut self.anchors);

        let mut warnings = self.all_warnings();
        for result in &results {
            if let Some(error) = &result.error
                && result.end_reason == Some(crate::capture::EndReason::Stopped)
            {
                warnings.push(format!("{}: {error}", result.file));
            }

            if !result.latency.looks_like_host_clock() {
                self.manifest.sync_quality = "unverified".to_string();
                warnings.push(format!(
                    "{}: frame timestamps do not look like the host clock (latency {} to {} ms); \
                     video/log alignment is unreliable",
                    result.file,
                    result.latency.min_ns / 1_000_000,
                    result.latency.max_ns / 1_000_000,
                ));
            }
        }

        self.manifest.apply_segments(results);
        // the session ends when capture stopped, not when the last display's last segment did
        self.manifest.video_end_mach_ns = Some(video_end.mach_ns);

        self.manifest.log_segments = self
            .tailer
            .as_ref()
            .map(|tailer| tailer.segments().to_vec())
            .unwrap_or_default();
        if let Ok(text) = std::fs::read_to_string(komorebi_log::slice_path(&self.session.dir)) {
            self.manifest.komorebi.observed_levels =
                komorebi_log::observed_levels(&text).into_iter().collect();
        }

        self.manifest.events = event_summary;
        self.manifest.apps = self.app_records(&stream_infos);

        for window in self.apps.tracker.unresolved() {
            warnings.push(format!(
                "komorebi window {} ({}) could not be mapped to a process, so it has no OS trace stream",
                window.id,
                window.exe.as_deref().unwrap_or("unknown app")
            ));
        }

        for info in &stream_infos {
            if let Some(error) = &info.error {
                warnings.push(format!("OS trace stream '{}': {error}", info.spec.name));
            }
        }

        if let Some(summary) = self.incident_summary() {
            warnings.push(summary);
        }

        self.manifest.oslog = stream_infos;
        self.manifest.warnings = warnings;

        self.session.write_manifest(&self.manifest)?;
        timeline::build(&self.session)?;
        self.session.write_readme(&self.manifest)?;

        let _ = &self.data_dir;
        Ok(self.session.dir.clone())
    }

    /// One line saying the recording environment changed, with counts by kind.
    fn incident_summary(&self) -> Option<String> {
        let text =
            std::fs::read_to_string(self.session.path(crate::incidents::INCIDENTS_FILE)).ok()?;
        let mut counts: BTreeMap<String, u64> = BTreeMap::new();
        for incident in text
            .lines()
            .filter_map(|line| serde_json::from_str::<crate::incidents::Incident>(line).ok())
        {
            // the start-of-recording note and routine start/stop are not "changes"
            if matches!(
                incident.kind.as_str(),
                kind::RECORDER | kind::CAPTURE_STARTED | kind::CAPTURE_RECOVERED
            ) {
                continue;
            }
            *counts.entry(incident.kind).or_default() += 1;
        }

        if counts.is_empty() {
            return None;
        }

        let parts: Vec<String> = counts
            .into_iter()
            .map(|(kind, count)| format!("{kind} x{count}"))
            .collect();
        Some(format!(
            "the recording environment changed while recording ({}); video has gaps where displays were \
             unavailable. See `--source recorder`",
            parts.join(", ")
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn state_with_window(id: u32, exe: &str) -> Value {
        serde_json::json!({
            "monitors": {"elements": [{"workspaces": {"elements": [{
                "containers": {"elements": [{"windows": {"elements": [
                    {"id": id, "details": {"exe": exe, "title": "t"}}
                ]}}]}
            }]}}]}
        })
    }

    fn apps_in(dir: &std::path::Path, options: OsLogOptions, own_pid: i32) -> AppStreams {
        AppStreams {
            options,
            streams: StreamSet::new(dir),
            trace: None,
            tracker: AppTracker::default(),
            own_pid,
            warnings: Vec::new(),
        }
    }

    fn temp(name: &str) -> PathBuf {
        let dir =
            std::env::temp_dir().join(format!("komorebi-recorder-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("oslog/apps")).expect("dir");
        dir
    }

    /// The whole discovery chain against the real machine, without opening or moving anything:
    /// a state mentioning a real window -> CGWindowList(OptionAll) -> pid -> a real `log stream`
    /// child writing its own file; and an event mentioning a pid does the same.
    #[test]
    fn discovery_starts_a_stream_for_a_newly_seen_process() {
        let dir = temp("discovery");

        // sandboxed builds may not be allowed to read the unified log; there is nothing to test
        // there, so probe first and skip rather than fail
        let probe = StreamSet::new(&dir);
        if probe
            .spawn(oslog::app_spec("probe", 1, OsLogLevel::Debug))
            .is_err()
            || !probe.wait_ready(Duration::from_secs(5)).is_empty()
        {
            eprintln!("skipping: `log stream` is not usable from this environment");
            let _ = probe.stop();
            return;
        }
        let _ = probe.stop();

        let mut apps = apps_in(&dir, OsLogOptions::default(), 0);
        let (tx, rx) = crossbeam_channel::unbounded();

        // launchd always exists, so an event naming pid 1 must get a stream
        tx.send(Discovery::Pid(1)).expect("send");

        // a real window from the window server, if this machine has any
        let owners = managed_apps::window_owners();
        let real = owners
            .iter()
            .find(|(_, owner)| owner.layer == 0 && owner.pid > 1)
            .map(|(id, owner)| (*id, owner.pid, owner.name.clone()));
        if let Some((id, _, name)) = &real {
            tx.send(Discovery::State(Arc::new(state_with_window(*id, name))))
                .expect("send");
        }

        drain_discovery(&rx, &mut apps, Duration::from_millis(10));

        assert!(apps.streams.has_pid(1), "the event pid got no stream");
        if let Some((_, pid, _)) = &real {
            assert!(
                apps.streams.has_pid(*pid),
                "the window's owner {pid} got no stream"
            );
        }

        // seeing the same things again must not spawn duplicates
        let before = apps.streams.snapshot().len();
        tx.send(Discovery::Pid(1)).expect("send");
        drain_discovery(&rx, &mut apps, Duration::from_millis(10));
        assert_eq!(apps.streams.snapshot().len(), before);

        assert!(
            apps.streams.wait_ready(Duration::from_secs(5)).is_empty(),
            "streams never became ready"
        );

        let records = apps.tracker.records();
        assert!(records.iter().all(|record| record.stream_file.is_some()));
        assert!(apps.warnings.is_empty(), "{:?}", apps.warnings);

        let infos = apps.streams.stop();
        let pids: BTreeSet<_> = infos.iter().map(|info| info.spec.pid).collect();
        assert!(pids.contains(&Some(1)));
        for info in &infos {
            assert!(
                dir.join(&info.spec.file).is_file(),
                "{} missing",
                info.spec.file
            );
            assert!(info.error.is_none(), "{:?}", info.error);
            assert!(
                info.header
                    .as_deref()
                    .is_some_and(|header| header.starts_with("Filtering"))
            );
        }
    }

    fn live(name: &str, events: u64, bytes: u64) -> StreamLive {
        StreamLive {
            name: name.to_string(),
            events,
            bytes,
        }
    }

    #[test]
    fn only_streams_above_the_threshold_are_abnormal() {
        let baseline = HashMap::from([
            ("komorebi".to_string(), (1_000, 100_000)),
            ("app:Chatty-1".to_string(), (0, 0)),
        ]);
        let now = [
            // 900 events in 2s = 450/s: fine
            live("komorebi", 1_900, 190_000),
            // 6000 events and 12 MiB in 2s = 3000/s, 360 MiB/min
            live("app:Chatty-1", 6_000, 12 * 1_048_576),
            // a stream that appeared since the baseline counts from zero
            live("app:New-2", 10, 1_000),
        ];

        let abnormal = abnormal_streams(&baseline, &now, Duration::from_secs(2));
        assert_eq!(abnormal.len(), 1);
        assert_eq!(abnormal[0].name, "app:Chatty-1");
        assert!((abnormal[0].events_per_second - 3_000.0).abs() < 1e-6);
        assert!((abnormal[0].megabytes_per_minute - 360.0).abs() < 1e-6);

        assert!(abnormal_streams(&baseline, &now, Duration::ZERO).is_empty());
    }

    #[test]
    fn a_pid_that_is_not_komorebi_is_not_running_komorebi() {
        assert!(!komorebi_is_running(1), "launchd is not komorebi");
        assert!(!komorebi_is_running(-5));
        assert!(!komorebi_is_running(i32::MAX));
    }

    #[test]
    fn the_komorebi_watch_is_inert_when_disabled() {
        let mut watch = KomorebiWatch::new(false, None);
        for _ in 0..3 {
            assert_eq!(watch.check(), None);
        }
    }

    #[test]
    fn a_vanished_komorebi_is_reported_once_and_then_searched_for() {
        // a pid that is certainly not komorebi stands in for a komorebi that has exited
        let gone = KomorebiProcess {
            exe: None,
            pid: i32::MAX,
            level: komorebi_log::DetectedLevel {
                level: "info".into(),
                source: "default".into(),
            },
        };
        let mut watch = KomorebiWatch::new(true, Some(&gone));

        assert_eq!(watch.check(), Some(KomorebiChange::Exited(i32::MAX)));
        assert_eq!(watch.current, None);
        assert_eq!(watch.known, vec![i32::MAX]);

        // searching for a replacement is rate-limited, and finding the real daemon (or not) is
        // up to the machine; what matters is that exiting is not reported again
        for _ in 0..3 {
            assert!(!matches!(watch.check(), Some(KomorebiChange::Exited(_))));
        }
    }

    #[test]
    fn excluded_and_own_processes_never_get_streams() {
        let dir = temp("exclude");
        let mut apps = apps_in(
            &dir,
            OsLogOptions {
                exclude: vec!["Chatty".to_string()],
                ..OsLogOptions::default()
            },
            4242,
        );

        for (pid, name) in [(4242, "Self"), (5000, "chatty")] {
            let record = AppRecord {
                pid,
                exe: name.to_string(),
                ..AppRecord::default()
            };
            apps.spawn_for(&record);
        }

        assert!(apps.streams.snapshot().is_empty());
    }

    #[test]
    fn apps_mode_none_ignores_discovery() {
        let dir = temp("none");
        let mut apps = apps_in(
            &dir,
            OsLogOptions {
                apps: AppsMode::None,
                ..OsLogOptions::default()
            },
            0,
        );

        apps.observe_pid(1, "event");
        apps.observe_state(&state_with_window(1, "x"));
        assert!(apps.streams.snapshot().is_empty());
    }

    #[test]
    fn a_wake_is_recorded_with_the_anchors_on_both_sides_of_it() {
        let before = Anchor {
            wall_us: 1_000_000,
            mach_ns: 5_000_000_000,
        };
        let after = Anchor {
            wall_us: 1_000_000 + 3_600_000_000,
            mach_ns: 5_050_000_000,
        };
        let jump = crate::clock::Jump {
            seconds: 3_600.0,
            before,
            after,
        };

        let (anchors, draft) = jump_response(&jump);
        assert_eq!(anchors, vec![before, after]);
        assert_eq!(draft.kind, kind::WAKE);
        assert!(draft.detail.contains("3600.0s"), "{}", draft.detail);

        // and the clock map built from them is right on either side of the sleep
        let map = crate::clock::ClockMap::new(anchors);
        assert_eq!(map.wall_to_mach(1_000_000), Some(5_000_000_000));
        assert_eq!(map.wall_to_mach(after.wall_us), Some(5_050_000_000));
    }

    #[test]
    fn a_backwards_clock_step_is_not_called_a_wake() {
        let anchor = Anchor {
            wall_us: 1,
            mach_ns: 1,
        };
        let (_, draft) = jump_response(&crate::clock::Jump {
            seconds: -30.0,
            before: anchor,
            after: anchor,
        });
        assert_eq!(draft.kind, kind::CLOCK_STEP);
        assert!(draft.detail.contains("30.0s"));
    }

    #[test]
    fn periodic_timers_fire_when_due_and_can_be_forced() {
        let mut timer = Every::new(Duration::from_secs(3600));
        assert!(!timer.due());
        timer.now();
        assert!(timer.due());
        assert!(!timer.due(), "and then wait a full interval again");
    }
}
