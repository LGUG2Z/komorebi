#![warn(clippy::all, clippy::nursery, clippy::pedantic)]
#![allow(clippy::missing_errors_doc)]

use clap::Parser;
use color_eyre::Result;
use crossbeam_channel::Receiver;
use crossbeam_channel::Sender;
#[cfg(feature = "deadlock_detection")]
use parking_lot::deadlock;
use parking_lot::Mutex;
use std::sync::atomic::Ordering;
use std::sync::Arc;
#[cfg(feature = "deadlock_detection")]
use std::thread;
#[cfg(feature = "deadlock_detection")]
use std::time::Duration;
use sysinfo::Process;
use sysinfo::ProcessExt;
use sysinfo::SystemExt;
use tracing_appender::non_blocking::WorkerGuard;
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::EnvFilter;

use komorebi::listen_for_commands;
use komorebi::listen_for_events;
use komorebi::listen_for_movements;
use komorebi::load_configuration;
use komorebi::WinEventListener;
use komorebi::WindowManager;
use komorebi::WindowManagerEvent;
use komorebi::WindowsApi;
use komorebi::CUSTOM_FFM;
use komorebi::HOME_DIR;
use komorebi::SESSION_ID;

fn setup() -> Result<(WorkerGuard, WorkerGuard)> {
    if std::env::var("RUST_LIB_BACKTRACE").is_err() {
        std::env::set_var("RUST_LIB_BACKTRACE", "1");
    }

    color_eyre::install()?;

    if std::env::var("RUST_LOG").is_err() {
        std::env::set_var("RUST_LOG", "info");
    }

    let home = HOME_DIR.clone();
    let appender = tracing_appender::rolling::never(home, "komorebi.log");
    let color_appender = tracing_appender::rolling::never(std::env::temp_dir(), "komorebi.log");
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
    thread::spawn(move || loop {
        tracing::info!("running deadlock detector");
        thread::sleep(Duration::from_secs(5));
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

#[derive(Parser)]
#[clap(author, about, version)]
struct Opts {
    /// Allow the use of komorebi's custom focus-follows-mouse implementation
    #[clap(long = "ffm")]
    focus_follows_mouse: bool,
}

#[tracing::instrument]
fn main() -> Result<()> {
    let opts: Opts = Opts::parse();
    CUSTOM_FFM.store(opts.focus_follows_mouse, Ordering::SeqCst);

    let arg_count = std::env::args().count();
    let has_valid_args = arg_count == 1 || (arg_count == 2 && CUSTOM_FFM.load(Ordering::SeqCst));

    if has_valid_args {
        let session_id = WindowsApi::process_id_to_session_id()?;
        SESSION_ID.store(session_id, Ordering::SeqCst);

        let mut system = sysinfo::System::new_all();
        system.refresh_processes();

        let matched_procs: Vec<&Process> = system.processes_by_name("komorebi.exe").collect();

        if matched_procs.len() > 1 {
            let mut shim_is_active = false;
            for proc in matched_procs {
                if proc.root().ends_with("shims") {
                    shim_is_active = true;
                }
            }

            if !shim_is_active {
                tracing::error!("komorebi.exe is already running, please exit the existing process before starting a new one");
                std::process::exit(1);
            }
        }

        // File logging worker guard has to have an assignment in the main fn to work
        let (_guard, _color_guard) = setup()?;

        #[cfg(feature = "deadlock_detection")]
        detect_deadlocks();

        let process_id = WindowsApi::current_process_id();
        WindowsApi::allow_set_foreground_window(process_id)?;

        let (outgoing, incoming): (Sender<WindowManagerEvent>, Receiver<WindowManagerEvent>) =
            crossbeam_channel::unbounded();

        let winevent_listener = WinEventListener::new(Arc::new(Mutex::new(outgoing)));
        winevent_listener.start();

        let wm = Arc::new(Mutex::new(WindowManager::new(Arc::new(Mutex::new(
            incoming,
        )))?));

        wm.lock().init()?;
        listen_for_commands(wm.clone());
        listen_for_events(wm.clone());

        if CUSTOM_FFM.load(Ordering::SeqCst) {
            listen_for_movements(wm.clone());
        }

        load_configuration()?;

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

        wm.lock().restore_all_windows();

        if WindowsApi::focus_follows_mouse()? {
            WindowsApi::disable_focus_follows_mouse()?;
        }

        std::process::exit(130);
    }

    Ok(())
}
