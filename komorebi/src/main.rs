//! The komorebi daemon: a tiling window manager for KDE Plasma.
//!
//! The daemon owns the layout of every window and asks the compositor, through a backend, to
//! put each one where the layout says.
//!
//! The backend is KWin's ([`komorebi::kwin`]): the daemon connects to the session bus, loads its
//! script into KWin and waits for the script to report the desktop. `--fake-backend` replaces it
//! with an in-memory desktop with one monitor and no windows, for development and testing. The
//! daemon then loads the monitors and the windows that are open, tiles them, listens for
//! messages on `komorebi.sock` in [`struct@DATA_DIR`], handles the events the backend reports on
//! a thread of their own, and runs until it is stopped, with `SIGINT`, `SIGTERM` or a `Stop`
//! message.

use clap::Parser;
use clap::ValueEnum;
use color_eyre::eyre;
use komorebi::DATA_DIR;
use komorebi::backend;
use komorebi::backend::fake::FakeBackend;
use komorebi::process_command::listen_for_commands;
use komorebi::process_event::listen_for_events;
use komorebi::window_manager::WindowManager;
use komorebi::window_manager_event_listener;
use parking_lot::Mutex;
use serde::Deserialize;
use std::sync::Arc;
use tracing_subscriber::EnvFilter;

fn setup(log_level: LogLevel) -> eyre::Result<()> {
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

    tracing::subscriber::set_global_default(
        tracing_subscriber::fmt::Subscriber::builder()
            .with_env_filter(EnvFilter::from_default_env())
            .finish(),
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

    Ok(())
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

/// A tiling window manager for KDE Plasma.
#[derive(Parser)]
#[clap(author, about, version = version::LONG_VERSION)]
struct Opts {
    /// Level of log output verbosity
    #[clap(long, value_enum, default_value_t=LogLevel::Info)]
    log_level: LogLevel,
    /// Run against an in-memory desktop instead of KWin, for development and testing of the
    /// socket interface. Windows and monitors are not real.
    #[clap(long, hide = true)]
    fake_backend: bool,
}

fn main() -> eyre::Result<()> {
    let opts: Opts = Opts::parse();
    setup(opts.log_level)?;

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

    let wm = Arc::new(Mutex::new(WindowManager::new(
        window_manager_event_listener::event_rx(),
        None,
    )?));

    wm.lock().init()?;
    wm.lock().retile_all()?;

    listen_for_commands(wm.clone());
    listen_for_events(wm.clone());

    let (quit_sender, quit_receiver) = std::sync::mpsc::channel();
    ctrlc::set_handler(move || {
        let _ = quit_sender.send(());
    })
    .expect("could not set ctrl-c handler");

    // everything runs on its own threads; this one only waits to be told to stop
    let _ = quit_receiver.recv();
    tracing::info!("stop signal received");

    backend::backend().shutdown();

    let socket = DATA_DIR.join("komorebi.sock");
    let _ = std::fs::remove_file(socket);

    std::process::exit(130);
}
