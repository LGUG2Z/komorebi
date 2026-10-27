//! The komorebi daemon: a tiling window manager for KDE Plasma.
//!
//! The daemon owns the layout of every window and asks the compositor, through a backend, to
//! put each one where the layout says.
//!
//! The backend is chosen on the command line. `--fake-backend` is an in-memory desktop with one
//! monitor and no windows, for development and testing. Without a backend the daemon exits with
//! a usage error. With one, it loads the monitors and the windows that are open, tiles them, and
//! runs until it is stopped with `SIGINT` or `SIGTERM`.

use clap::CommandFactory;
use clap::Parser;
use clap::ValueEnum;
use clap::error::ErrorKind;
use color_eyre::eyre;
use komorebi::backend;
use komorebi::backend::fake::FakeBackend;
use komorebi::window_manager::WindowManager;
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
    /// Run against an in-memory desktop with one monitor and no windows, for development and
    /// testing. Windows and monitors are not real.
    #[clap(long)]
    fake_backend: bool,
}

fn main() -> eyre::Result<()> {
    let opts: Opts = Opts::parse();
    setup(opts.log_level)?;

    if opts.fake_backend {
        tracing::warn!("running against an in-memory desktop; nothing here is a real window");
        backend::set_backend(Arc::new(FakeBackend::with_one_monitor()));
    } else {
        Opts::command()
            .error(
                ErrorKind::MissingRequiredArgument,
                "a backend has to be chosen: pass --fake-backend to run against an in-memory desktop",
            )
            .exit();
    }

    let mut wm = WindowManager::new()?;
    wm.init()?;
    wm.retile_all()?;

    let (quit_sender, quit_receiver) = std::sync::mpsc::channel();
    ctrlc::set_handler(move || {
        let _ = quit_sender.send(());
    })
    .expect("could not set ctrl-c handler");

    // this thread only waits to be told to stop
    let _ = quit_receiver.recv();
    tracing::info!("stop signal received");

    backend::backend().shutdown();

    std::process::exit(130);
}
