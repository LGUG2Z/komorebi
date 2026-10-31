#![warn(clippy::all)]

use clap::Parser;
use clap::ValueEnum;
use color_eyre::eyre;
use color_eyre::eyre::OptionExt;
use komorebi::CoreFoundationRunLoop;
use komorebi::DATA_DIR;
use komorebi::HOME_DIR;
use komorebi::border_manager;
use komorebi::core::pathext::replace_env_in_path;
use komorebi::display_reconfiguration_listener::DisplayReconfigurationListener;
use komorebi::input_event_listener;
use komorebi::input_event_listener::InputEventListener;
use komorebi::monitor_reconciliator;
use komorebi::notification_center_listener::NotificationCenterListener;
use komorebi::process_command::listen_for_commands;
use komorebi::process_event::listen_for_events;
use komorebi::reaper;
use komorebi::settle;
use komorebi::static_config::StaticConfig;
use komorebi::theme_manager;
use komorebi::window_manager::WindowManager;
use komorebi::window_manager_event_listener;
use komorebi::workspace_reconciliator;
use objc2::MainThreadMarker;
use objc2::rc::autoreleasepool;
use objc2_app_kit::NSApplication;
use objc2_app_kit::NSEventMask;
use objc2_application_services::AXIsProcessTrusted;
use objc2_core_foundation::CFRunLoop;
use objc2_core_foundation::kCFRunLoopDefaultMode;
use objc2_core_graphics::CGPreflightScreenCaptureAccess;
use objc2_core_graphics::CGRequestScreenCaptureAccess;
use objc2_foundation::NSDate;
use objc2_foundation::NSDefaultRunLoopMode;
use parking_lot::Mutex;
use serde::Deserialize;
use std::net::Shutdown;
use std::os::unix::net::UnixStream;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::AtomicBool;
use std::sync::atomic::Ordering;
use std::sync::mpsc;
use std::time::Duration;
use std::time::Instant;
use sysinfo::Process;
use sysinfo::ProcessesToUpdate;
use tracing_appender::non_blocking::WorkerGuard;
use tracing_subscriber::EnvFilter;
use tracing_subscriber::layer::SubscriberExt;

/// How often the event tap is looked at when the system has not said that it turned it off
const TAP_CHECK_INTERVAL: Duration = Duration::from_secs(5);

fn check_permissions() -> eyre::Result<()> {
    unsafe {
        // check for screen capture access - this is needed to read window titles
        if !CGPreflightScreenCaptureAccess() {
            // if providing the dialog box failed, exit
            if !CGRequestScreenCaptureAccess() {
                eyre::bail!("failed to request screen capability");
            }
        }

        if !AXIsProcessTrusted() {
            eyre::bail!("komorebi needs to be added as a trusted accessibility process");
        }

        Ok(())
    }
}

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

    let appender = tracing_appender::rolling::daily(&*DATA_DIR, "komorebi_plaintext.log");
    let color_appender = tracing_appender::rolling::daily(&*DATA_DIR, "komorebi.log");
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

#[derive(Parser)]
#[clap(author, about, version = version::LONG_VERSION)]
struct Opts {
    /// Path to a static configuration JSON file
    #[clap(short, long)]
    #[clap(value_parser = replace_env_in_path)]
    config: Option<PathBuf>,
    // /// Do not attempt to auto-apply a dumped state temp file from a previously running instance of komorebi
    // #[clap(long)]
    // clean_state: bool,
    /// Level of log output verbosity
    #[clap(long, value_enum, default_value_t=LogLevel::Info)]
    log_level: LogLevel,
}

fn main() -> eyre::Result<()> {
    let opts: Opts = Opts::parse();
    let (_guard, _color_guard) = setup(opts.log_level)?;

    let mut system = sysinfo::System::new();
    system.refresh_processes(ProcessesToUpdate::All, true);

    let matched_procs: Vec<&Process> = system
        .processes_by_exact_name("komorebi".as_ref())
        .collect();

    if matched_procs.len() > 1 {
        tracing::error!(
            "komorebi is already running, please exit the existing process before starting a new one"
        );
        std::process::exit(1);
    }

    check_permissions()?;

    if !DATA_DIR.is_dir() {
        std::fs::create_dir_all(&*DATA_DIR)?;
    }

    let mtm = MainThreadMarker::new().ok_or_eyre("failed to create main thread marker")?;
    // apparently this establishes the window server connection which is needed super early on tahoe
    let app = NSApplication::sharedApplication(mtm);
    // this lets us complete initialization without using a blocking method like run
    app.finishLaunching();

    let _notification_center_listener = NotificationCenterListener::init();
    let _display_reconfiguration_listener = DisplayReconfigurationListener::init();

    let run_loop = CFRunLoop::current().ok_or_eyre("couldn't get CFRunLoop::current")?;
    let mut input_listener = InputEventListener::init(&run_loop);
    let mut last_tap_check = Instant::now();

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

    let wm = if let Some(config) = &static_config {
        tracing::info!(
            "creating window manager from static configuration file: {}",
            config.display()
        );

        Arc::new(Mutex::new(StaticConfig::preload(
            config,
            window_manager_event_listener::event_rx(),
            None,
            &run_loop,
        )?))
    } else {
        Arc::new(Mutex::new(WindowManager::new(
            &run_loop,
            window_manager_event_listener::event_rx(),
            None,
        )?))
    };

    wm.lock().init()?;

    if let Some(config) = &static_config {
        StaticConfig::postload(config, &wm)?;
    }

    wm.lock().retile_all(false)?;

    border_manager::listen_for_notifications(wm.clone(), CoreFoundationRunLoop(run_loop.clone()));
    theme_manager::listen_for_notifications();
    monitor_reconciliator::listen_for_notifications(wm.clone())?;
    reaper::listen_for_notifications(wm.clone());
    settle::listen_for_settling(wm.clone());
    workspace_reconciliator::listen_for_notifications(wm.clone());

    listen_for_commands(wm.clone());
    listen_for_events(wm.clone());

    let quit_ctrlc = Arc::new(AtomicBool::new(false));
    let quit_thread = quit_ctrlc.clone();

    std::thread::spawn(move || {
        let (ctrlc_sender, ctrlc_receiver) = mpsc::channel();
        ctrlc::set_handler(move || {
            ctrlc_sender
                .send(())
                .expect("could not send signal on ctrl-c channel");
        })
        .expect("could not set ctrl-c handler");

        ctrlc_receiver
            .recv()
            .expect("could not receive signal on ctrl-c channel");

        tracing::info!("ctrl-c signal received");
        quit_ctrlc.store(true, Ordering::Relaxed);
    });

    tracing::info!("starting CFRunLoop to receive observer notifications");

    loop {
        if quit_thread.load(Ordering::Relaxed) {
            tracing::info!("stopping CFRunLoop");
            break;
        }

        // the tap is looked after when the system says that it turned it off, and every so often
        // regardless, since there is nobody to say when it is lost across sleep
        if input_event_listener::take_disabled() || last_tap_check.elapsed() >= TAP_CHECK_INTERVAL {
            last_tap_check = Instant::now();

            match input_listener.as_mut() {
                Some(listener) => listener.ensure_enabled(&run_loop),
                // there may be permission now that there was not
                None => input_listener = InputEventListener::init(&run_loop),
            }
        }

        autoreleasepool(|_| {
            // process NSApplication events explicitly - this is what makes display
            // reconfiguration callbacks work on tahoe
            if let Some(event) = unsafe {
                app.nextEventMatchingMask_untilDate_inMode_dequeue(
                    NSEventMask::Any,
                    Some(&NSDate::dateWithTimeIntervalSinceNow(0.1)),
                    NSDefaultRunLoopMode,
                    true,
                )
            } {
                app.sendEvent(&event);
            }

            // this gets our observer notification callbacks firing
            unsafe { CFRunLoop::run_in_mode(kCFRunLoopDefaultMode, 0.1, false) };
        });
    }

    // nothing may move a window once they have all been put back where they were found
    komorebi::animation::animator::shutdown();

    wm.lock().restore_all_windows(false)?;

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
