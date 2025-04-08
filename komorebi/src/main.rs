#![warn(clippy::all)]
#![allow(
    clippy::missing_errors_doc,
    clippy::redundant_pub_crate,
    clippy::significant_drop_tightening,
    clippy::significant_drop_in_scrutinee,
    clippy::doc_markdown
)]

use std::env::temp_dir;
use std::net::Shutdown;
use std::path::PathBuf;
use std::sync::atomic::Ordering;
use std::sync::Arc;
#[cfg(feature = "deadlock_detection")]
use std::time::Duration;

use clap::Parser;
use clap::ValueEnum;
use color_eyre::Result;
use crossbeam_utils::Backoff;
use komorebi::animation::AnimationEngine;
use komorebi::animation::ANIMATION_ENABLED_GLOBAL;
use komorebi::animation::ANIMATION_ENABLED_PER_ANIMATION;
use komorebi::replace_env_in_path;
#[cfg(feature = "deadlock_detection")]
use parking_lot::deadlock;
use parking_lot::Mutex;
use serde::Deserialize;
use sysinfo::Process;
use sysinfo::ProcessesToUpdate;
use tracing_appender::non_blocking::WorkerGuard;
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::EnvFilter;
use uds_windows::UnixStream;

use komorebi::border_manager;
use komorebi::focus_manager;
use komorebi::load_configuration;
use komorebi::monitor_reconciliator;
use komorebi::process_command::listen_for_commands;
use komorebi::process_command::listen_for_commands_tcp;
use komorebi::process_event::listen_for_events;
use komorebi::process_movement::listen_for_movements;
use komorebi::reaper;
use komorebi::stackbar_manager;
use komorebi::static_config::StaticConfig;
use komorebi::theme_manager;
use komorebi::transparency_manager;
use komorebi::window_manager::State;
use komorebi::window_manager::WindowManager;
use komorebi::windows_api::WindowsApi;
use komorebi::winevent_listener;
use komorebi::CUSTOM_FFM;
use komorebi::DATA_DIR;
use komorebi::HOME_DIR;
use komorebi::INITIAL_CONFIGURATION_LOADED;
use komorebi::SESSION_ID;

fn setup(log_level: LogLevel) -> Result<(WorkerGuard, WorkerGuard)> {
    if std::env::var("RUST_LIB_BACKTRACE").is_err() {
        std::env::set_var("RUST_LIB_BACKTRACE", "1");
    }

    color_eyre::install()?;

    if std::env::var("RUST_LOG").is_err() {
        std::env::set_var(
            "RUST_LOG",
            match log_level {
                LogLevel::Error => "error",
                LogLevel::Warn => "warn",
                LogLevel::Info => "info",
                LogLevel::Debug => "debug",
                LogLevel::Trace => "trace",
            },
        );
    }

    let appender = tracing_appender::rolling::daily(std::env::temp_dir(), "komorebi_plaintext.log");
    let color_appender = tracing_appender::rolling::daily(std::env::temp_dir(), "komorebi.log");
    let (non_blocking, guard) = tracing_appender::non_blocking(appender);
    let (color_non_blocking, color_guard) = tracing_appender::non_blocking(color_appender);

    tracing::subscriber::set_global_default(
        tracing_subscriber::fmt::Subscriber::builder()
            .with_env_filter(EnvFilter::from_default_env())
            .finish()
            .with(
                tracing_subscriber::fmt::Layer::default()
                    .with_writer(non_blocking)
                    .with_ansi(false),
            )
            .with(
                tracing_subscriber::fmt::Layer::default()
                    .with_writer(color_non_blocking)
                    .with_ansi(true),
            ),
    )?;

    // https://github.com/tokio-rs/tracing/blob/master/examples/examples/panic_hook.rs
    // Set a panic hook that records the panic as a `tracing` event at the
    // `ERROR` verbosity level.
    //
    // If we are currently in a span when the panic occurred, the logged event
    // will include the current span, allowing the context in which the panic
    // occurred to be recorded.
    std::panic::set_hook(Box::new(|panic| {
        // If the panic has a source location, record it as structured fields.
        panic.location().map_or_else(
            || {
                tracing::error!(message = %panic);
            },
            |location| {
                // On nightly Rust, where the `PanicInfo` type also exposes a
                // `message()` method returning just the message, we could record
                // just the message instead of the entire `fmt::Display`
                // implementation, avoiding the duplciated location
                tracing::error!(
                    message = %panic,
                    panic.file = location.file(),
                    panic.line = location.line(),
                    panic.column = location.column(),
                );
            },
        );
    }));

    Ok((guard, color_guard))
}

#[cfg(feature = "deadlock_detection")]
#[tracing::instrument]
fn detect_deadlocks() {
    // Create a background thread which checks for deadlocks every 10s
    std::thread::spawn(move || loop {
        tracing::info!("running deadlock detector");
        std::thread::sleep(Duration::from_secs(5));
        let deadlocks = deadlock::check_deadlock();
        if deadlocks.is_empty() {
            continue;
        }

        tracing::error!("{} deadlocks detected", deadlocks.len());
        for (i, threads) in deadlocks.iter().enumerate() {
            tracing::error!("deadlock #{}", i);
            for t in threads {
                tracing::error!("thread id: {:#?}", t.thread_id());
                tracing::error!("{:#?}", t.backtrace());
            }
        }
    });
}

#[derive(Default, Deserialize, ValueEnum, Clone)]
#[serde(rename_all = "snake_case")]
enum LogLevel {
    Error,
    Warn,
    #[default]
    Info,
    Debug,
    Trace,
}

#[derive(Parser)]
#[clap(author, about, version = komorebi::build::CLAP_LONG_VERSION)]
struct Opts {
    /// Allow the use of komorebi's custom focus-follows-mouse implementation
    #[clap(short, long = "ffm")]
    focus_follows_mouse: bool,
    /// Wait for 'komorebic complete-configuration' to be sent before processing events
    #[clap(short, long)]
    await_configuration: bool,
    /// Start a TCP server on the given port to allow the direct sending of SocketMessages
    #[clap(short, long)]
    tcp_port: Option<usize>,
    /// Path to a static configuration JSON file
    #[clap(short, long)]
    #[clap(value_parser = replace_env_in_path)]
    config: Option<PathBuf>,
    /// Do not attempt to auto-apply a dumped state temp file from a previously running instance of komorebi
    #[clap(long)]
    clean_state: bool,
    /// Level of log output verbosity
    #[clap(long, value_enum, default_value_t=LogLevel::Info)]
    log_level: LogLevel,
}

#[tracing::instrument]
#[allow(clippy::cognitive_complexity)]
fn main() -> Result<()> {
    let opts: Opts = Opts::parse();
    CUSTOM_FFM.store(opts.focus_follows_mouse, Ordering::SeqCst);

    let process_id = WindowsApi::current_process_id();
    WindowsApi::allow_set_foreground_window(process_id)?;
    WindowsApi::set_process_dpi_awareness_context()?;

    let session_id = WindowsApi::process_id_to_session_id()?;
    SESSION_ID.store(session_id, Ordering::SeqCst);

    let mut system = sysinfo::System::new();
    system.refresh_processes(ProcessesToUpdate::All, true);

    let matched_procs: Vec<&Process> = system.processes_by_name("komorebi.exe".as_ref()).collect();

    if matched_procs.len() > 1 {
        let mut len = matched_procs.len();
        for proc in matched_procs {
            if let Some(executable_path) = proc.exe() {
                if executable_path.to_string_lossy().contains("shims") {
                    len -= 1;
                }
            }
        }

        if len > 1 {
            tracing::error!("komorebi.exe is already running, please exit the existing process before starting a new one");
            std::process::exit(1);
        }
    }

    // File logging worker guard has to have an assignment in the main fn to work
    let (_guard, _color_guard) = setup(opts.log_level)?;

    WindowsApi::foreground_lock_timeout()?;

    winevent_listener::start();

    #[cfg(feature = "deadlock_detection")]
    detect_deadlocks();

    let static_config = opts.config.map_or_else(
        || {
            let komorebi_json = HOME_DIR.join("komorebi.json");
            if komorebi_json.is_file() {
                Option::from(komorebi_json)
            } else {
                None
            }
        },
        Option::from,
    );

    std::fs::create_dir_all(&*DATA_DIR)?;

    let wm = if let Some(config) = &static_config {
        tracing::info!(
            "creating window manager from static configuration file: {}",
            config.display()
        );

        Arc::new(Mutex::new(StaticConfig::preload(
            config,
            winevent_listener::event_rx(),
            None,
        )?))
    } else {
        Arc::new(Mutex::new(WindowManager::new(
            winevent_listener::event_rx(),
            None,
        )?))
    };

    wm.lock().init()?;

    if let Some(config) = &static_config {
        StaticConfig::postload(config, &wm)?;
    }

    if !opts.await_configuration && !INITIAL_CONFIGURATION_LOADED.load(Ordering::SeqCst) {
        INITIAL_CONFIGURATION_LOADED.store(true, Ordering::SeqCst);
    };

    if static_config.is_none() {
        std::thread::spawn(|| load_configuration().expect("could not load configuration"));

        if opts.await_configuration {
            let backoff = Backoff::new();
            while !INITIAL_CONFIGURATION_LOADED.load(Ordering::SeqCst) {
                backoff.snooze();
            }
        }
    }

    let dumped_state = temp_dir().join("komorebi.state.json");

    if !opts.clean_state && dumped_state.is_file() {
        if let Ok(state) = serde_json::from_str(&std::fs::read_to_string(&dumped_state)?) {
            wm.lock().apply_state(state);
        } else {
            tracing::warn!(
                "cannot apply state from {}; state struct is not up to date",
                dumped_state.display()
            );
        }
    }

    wm.lock().retile_all(false)?;

    border_manager::listen_for_notifications(wm.clone());
    stackbar_manager::listen_for_notifications(wm.clone());
    transparency_manager::listen_for_notifications(wm.clone());
    monitor_reconciliator::listen_for_notifications(wm.clone())?;
    reaper::listen_for_notifications(wm.clone(), wm.lock().known_hwnds.clone());
    focus_manager::listen_for_notifications(wm.clone());
    theme_manager::listen_for_notifications();

    listen_for_commands(wm.clone());

    if let Some(port) = opts.tcp_port {
        listen_for_commands_tcp(wm.clone(), port);
    }

    listen_for_events(wm.clone());

    if CUSTOM_FFM.load(Ordering::SeqCst) {
        listen_for_movements(wm.clone());
    }

    let (ctrlc_sender, ctrlc_receiver) = crossbeam_channel::bounded(1);
    ctrlc::set_handler(move || {
        ctrlc_sender
            .send(())
            .expect("could not send signal on ctrl-c channel");
    })?;

    ctrlc_receiver
        .recv()
        .expect("could not receive signal on ctrl-c channel");

    tracing::error!("received ctrl-c, restoring all hidden windows and terminating process");

    let state = State::from(&*wm.lock());
    std::fs::write(dumped_state, serde_json::to_string_pretty(&state)?)?;

    ANIMATION_ENABLED_PER_ANIMATION.lock().clear();
    ANIMATION_ENABLED_GLOBAL.store(false, Ordering::SeqCst);
    wm.lock().restore_all_windows(false)?;
    AnimationEngine::wait_for_all_animations();

    if WindowsApi::focus_follows_mouse()? {
        WindowsApi::disable_focus_follows_mouse()?;
    }

    let sockets = komorebi::SUBSCRIPTION_SOCKETS.lock();
    for path in (*sockets).values() {
        if let Ok(stream) = UnixStream::connect(path) {
            stream.shutdown(Shutdown::Both)?;
        }
    }

    let socket = DATA_DIR.join("komorebi.sock");
    let _ = std::fs::remove_file(socket);

    std::process::exit(130);
}
