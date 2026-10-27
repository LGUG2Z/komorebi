//! The komorebi daemon: a tiling window manager for KDE Plasma.
//!
//! The daemon owns the layout of every window and asks the compositor, through a backend, to
//! put each one where the layout says.
//!
//! The backend is KWin's ([`komorebi::kwin`]): the daemon connects to the session bus, loads its
//! script into KWin and waits for the script to report the desktop. `--fake-backend` replaces it
//! with an in-memory desktop with one monitor and no windows, for development and testing.
//!
//! The daemon starts from `komorebi.json`: the file `--config` names, or else the one in
//! [`struct@HOME_DIR`] when there is one ([`komorebi::static_config`]), which it reloads whenever
//! it changes. Without a file it runs on its defaults. It loads the monitors and the windows
//! that are open, gives each monitor the configuration the file has for it, asks for the borders
//! it draws ([`komorebi::border_manager`]), tiles the windows, listens for messages on
//! `komorebi.sock` in [`struct@DATA_DIR`], handles the events the backend reports and the changes
//! of the monitors on threads of their own, and runs until it is stopped, with `SIGINT`,
//! `SIGTERM` or a `Stop` message.
//!
//! With a key binding file (`--keybindings`, or the first of `skhdrc`, `whkdrc` and `sxhkdrc` in
//! [`struct@HOME_DIR`]), the daemon also runs the engine of [`komorebi_hotkeys`]: it registers the
//! keys of the bindings that are not static shortcuts with KWin's global shortcut daemon and runs
//! their commands ([`komorebi::kwin::hotkeys`]). The bindings it takes are the engine's part of
//! the file's plan in `--keybindings-mode`, the mode `komorebic hotkeys compile --mode` wrote the
//! static shortcuts in.
//!
//! It logs to standard output and to a file a day in [`struct@LOG_DIR`], which `komorebic log`
//! follows.
//!
//! When it stops, it brings back the windows it hid (a stack's windows that were not in front,
//! except after a `StopIgnoreRestore` message), then connects to the socket of each subscriber
//! and closes the connection without writing anything, so a subscriber sees the daemon stop as a
//! connection with nothing on it.

use clap::Parser;
use clap::ValueEnum;
use color_eyre::eyre;
use komorebi::DATA_DIR;
use komorebi::HOME_DIR;
use komorebi::LOG_DIR;
use komorebi::backend;
use komorebi::backend::fake::FakeBackend;
use komorebi::border_manager;
use komorebi::core::pathext::replace_env_in_path;
use komorebi::monitor_reconciliator;
use komorebi::process_command::listen_for_commands;
use komorebi::process_event::listen_for_events;
use komorebi::static_config::StaticConfig;
use komorebi::window_manager::WindowManager;
use komorebi::window_manager_event_listener;
use komorebi_hotkeys::Format;
use komorebi_hotkeys::ParseOptions;
use komorebi_hotkeys::engine::EngineOptions;
use komorebi_hotkeys::plan::PlanMode;
use komorebi_hotkeys::plan::PlanOptions;
use parking_lot::Mutex;
use serde::Deserialize;
use std::net::Shutdown;
use std::os::unix::net::UnixStream;
use std::path::PathBuf;
use std::sync::Arc;
use tracing_appender::non_blocking::WorkerGuard;
use tracing_subscriber::EnvFilter;
use tracing_subscriber::layer::SubscriberExt;

fn setup(log_level: LogLevel) -> eyre::Result<(WorkerGuard, WorkerGuard)> {
    if std::env::var("RUST_LIB_BACKTRACE").is_err() {
        unsafe {
            std::env::set_var("RUST_LIB_BACKTRACE", "1");
        }
    }

    color_eyre::install()?;

    if std::env::var("RUST_LOG").is_err() {
        unsafe {
            std::env::set_var(
                "RUST_LOG",
                match log_level {
                    LogLevel::Error => "komorebi=error",
                    LogLevel::Warn => "komorebi=warn",
                    LogLevel::Info => "komorebi=info",
                    LogLevel::Debug => "komorebi=debug",
                    LogLevel::Trace => "komorebi=trace",
                },
            );
        }
    }

    std::fs::create_dir_all(&*LOG_DIR)?;
    let appender = tracing_appender::rolling::daily(&*LOG_DIR, "komorebi_plaintext.log");
    let color_appender = tracing_appender::rolling::daily(&*LOG_DIR, "komorebi.log");
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
    std::panic::set_hook(Box::new(|panic| {
        panic.location().map_or_else(
            || {
                tracing::error!(message = %panic);
            },
            |location| {
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
    std::thread::spawn(move || {
        loop {
            tracing::info!("running deadlock detector");
            std::thread::sleep(std::time::Duration::from_secs(5));
            let deadlocks = parking_lot::deadlock::check_deadlock();
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

/// [`Format`], for the command line
#[derive(Clone, Copy, ValueEnum)]
enum KeyFormat {
    Skhd,
    Whkd,
    Sxhkd,
}

impl From<KeyFormat> for Format {
    fn from(format: KeyFormat) -> Self {
        match format {
            KeyFormat::Skhd => Format::Skhd,
            KeyFormat::Whkd => Format::Whkd,
            KeyFormat::Sxhkd => Format::Sxhkd,
        }
    }
}

/// [`PlanMode`], for the command line
#[derive(Clone, Copy, ValueEnum)]
enum KeyPlanMode {
    Auto,
    Engine,
    Static,
}

impl From<PlanMode> for KeyPlanMode {
    fn from(mode: PlanMode) -> Self {
        match mode {
            PlanMode::Auto => KeyPlanMode::Auto,
            PlanMode::Engine => KeyPlanMode::Engine,
            PlanMode::Static => KeyPlanMode::Static,
        }
    }
}

impl From<KeyPlanMode> for PlanMode {
    fn from(mode: KeyPlanMode) -> Self {
        match mode {
            KeyPlanMode::Auto => PlanMode::Auto,
            KeyPlanMode::Engine => PlanMode::Engine,
            KeyPlanMode::Static => PlanMode::Static,
        }
    }
}

/// A tiling window manager for KDE Plasma.
#[derive(Parser)]
#[clap(author, about, version = version::LONG_VERSION)]
struct Opts {
    /// Level of log output verbosity
    #[clap(long, value_enum, default_value_t=LogLevel::Info)]
    log_level: LogLevel,
    /// Path to a static configuration JSON file
    #[clap(short, long)]
    #[clap(value_parser = replace_env_in_path)]
    config: Option<PathBuf>,
    /// Run against an in-memory desktop instead of KWin, for development and testing of the
    /// socket interface. Windows and monitors are not real.
    #[clap(long, hide = true)]
    fake_backend: bool,
    /// A key binding file (skhdrc, whkdrc or sxhkdrc) whose bindings komorebi registers with KDE
    /// itself [default: the first of skhdrc, whkdrc and sxhkdrc in the komorebi config directory,
    /// if there is one]
    #[clap(long)]
    keybindings: Option<PathBuf>,
    /// The format of the key binding file [default: from the end of its name]
    #[clap(long, value_enum)]
    keybindings_format: Option<KeyFormat>,
    /// Which bindings of the file komorebi registers: those that `komorebic hotkeys compile` in
    /// the same mode does not write as static shortcuts
    #[clap(long, value_enum, default_value_t = PlanMode::default().into())]
    keybindings_mode: KeyPlanMode,
}

/// Reads the key binding file, and starts the engine for the bindings that are not static
/// shortcuts. Does nothing when there is no file, or nothing in it for the engine. An engine that
/// cannot start is logged, and the window manager runs without keys.
///
/// # Errors
///
/// The file cannot be read, or its format is neither given nor in its name.
fn keybindings(opts: &Opts) -> eyre::Result<()> {
    let file = match &opts.keybindings {
        Some(file) => file.clone(),
        None => {
            let found = [Format::Skhd, Format::Whkd, Format::Sxhkd]
                .iter()
                .map(|format| HOME_DIR.join(format.file_name()))
                .find(|path| path.is_file());
            match found {
                Some(file) => file,
                None => return Ok(()),
            }
        }
    };
    let format = match opts.keybindings_format {
        Some(format) => format.into(),
        None => {
            let name = file
                .file_name()
                .and_then(|n| n.to_str())
                .unwrap_or_default();
            [Format::Skhd, Format::Whkd, Format::Sxhkd]
                .into_iter()
                .find(|format| name.ends_with(format.file_name()))
                .ok_or_else(|| {
                    eyre::Report::msg(format!(
                        "{} does not end with skhdrc, whkdrc or sxhkdrc: give its --keybindings-format",
                        file.display()
                    ))
                })?
        }
    };

    let text = std::fs::read_to_string(&file).map_err(|error| {
        eyre::Report::msg(format!("could not read {}: {error}", file.display()))
    })?;
    let parsed = komorebi_hotkeys::parse(format, &text, &ParseOptions::default());
    let plan = komorebi_hotkeys::plan::plan(
        &parsed.keymap,
        &PlanOptions {
            mode: opts.keybindings_mode.into(),
        },
    );
    let diagnostics = parsed.diagnostics.iter().chain(&plan.diagnostics);
    for diagnostic in diagnostics.clone() {
        tracing::debug!("{}: {diagnostic}", file.display());
    }
    let engine: usize = plan.engine.modes.iter().map(|m| m.bindings.len()).sum();
    tracing::info!(
        "read {}: {engine} bindings for the engine, {} static shortcuts, {} diagnostics \
         (`komorebic hotkeys compile --dry-run` shows them)",
        file.display(),
        plan.static_files.len(),
        diagnostics.count()
    );
    if plan.needs_engine()
        && let Err(error) = komorebi::kwin::hotkeys::start(plan.engine, EngineOptions::default())
    {
        tracing::error!("the key bindings are not active: {error}");
    }
    Ok(())
}

fn main() -> eyre::Result<()> {
    let opts: Opts = Opts::parse();
    let (_guard, _color_guard) = setup(opts.log_level.clone())?;

    if !DATA_DIR.is_dir() {
        std::fs::create_dir_all(&*DATA_DIR)?;
    }

    if opts.fake_backend {
        tracing::warn!("running against an in-memory desktop; nothing here is a real window");
        backend::set_backend(Arc::new(FakeBackend::with_one_monitor()));
    } else {
        tracing::info!("connecting to KWin");
        backend::set_backend(komorebi::kwin::connect()?);
    }

    #[cfg(feature = "deadlock_detection")]
    detect_deadlocks();

    let static_config = opts.config.clone().map_or_else(
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

    let wm = if let Some(config) = &static_config {
        tracing::info!(
            "creating window manager from static configuration file: {}",
            config.display()
        );

        Arc::new(Mutex::new(StaticConfig::preload(
            config,
            window_manager_event_listener::event_rx(),
            None,
        )?))
    } else {
        Arc::new(Mutex::new(WindowManager::new(
            window_manager_event_listener::event_rx(),
            None,
        )?))
    };

    wm.lock().init()?;

    if let Some(config) = &static_config {
        StaticConfig::postload(config, &wm)?;
    }

    // the configuration may have given monitors more workspaces than they started with (and named
    // them): the compositor needs a desktop for each
    wm.lock().sync_workspace_desktops();

    // how windows are to look (borders, animation) is handed to the compositor before the first
    // layout, which leaves room for a border that is being drawn
    border_manager::send_notification();

    wm.lock().retile_all(false)?;

    // and what the windows that are already there look like: their border colors, how see-through
    // they are
    {
        let mut wm = wm.lock();
        border_manager::update_colours(&wm);
        komorebi::transparency_manager::apply(&mut wm);
    }

    monitor_reconciliator::listen_for_notifications(wm.clone())?;

    listen_for_commands(wm.clone());
    listen_for_events(wm.clone());

    // the keys need KWin's global shortcut daemon, and the in-memory desktop has none
    if !opts.fake_backend {
        keybindings(&opts)?;
    }

    let (quit_sender, quit_receiver) = std::sync::mpsc::channel();
    ctrlc::set_handler(move || {
        let _ = quit_sender.send(());
    })
    .expect("could not set ctrl-c handler");

    // everything runs on its own threads; this one only waits to be told to stop
    let _ = quit_receiver.recv();
    tracing::info!("stop signal received");

    {
        let mut wm = wm.lock();
        wm.restore_all_windows(false)?;
        komorebi::transparency_manager::restore(&mut wm);
    }
    backend::backend().shutdown();

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
