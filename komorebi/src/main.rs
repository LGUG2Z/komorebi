#![warn(clippy::all, clippy::nursery, clippy::pedantic)]
#![allow(clippy::missing_errors_doc)]

use std::collections::HashMap;
use std::fs::File;
use std::io::Write;
use std::process::Command;
use std::sync::atomic::AtomicBool;
use std::sync::atomic::Ordering;
use std::sync::Arc;
#[cfg(feature = "deadlock_detection")]
use std::thread;
#[cfg(feature = "deadlock_detection")]
use std::time::Duration;

use clap::Parser;
use color_eyre::eyre::anyhow;
use color_eyre::Result;
use crossbeam_channel::Receiver;
use crossbeam_channel::Sender;
use lazy_static::lazy_static;
#[cfg(feature = "deadlock_detection")]
use parking_lot::deadlock;
use parking_lot::Mutex;
use serde::Serialize;
use sysinfo::SystemExt;
use tracing_appender::non_blocking::WorkerGuard;
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::EnvFilter;
use which::which;

use komorebi_core::SocketMessage;

use crate::process_command::listen_for_commands;
use crate::process_event::listen_for_events;
use crate::process_movement::listen_for_movements;
use crate::window_manager::State;
use crate::window_manager::WindowManager;
use crate::window_manager_event::WindowManagerEvent;
use crate::windows_api::WindowsApi;

#[macro_use]
mod ring;

mod container;
mod monitor;
mod process_command;
mod process_event;
mod process_movement;
mod set_window_position;
mod styles;
mod window;
mod window_manager;
mod window_manager_event;
mod windows_api;
mod windows_callbacks;
mod winevent;
mod winevent_listener;
mod workspace;

lazy_static! {
    static ref HIDDEN_HWNDS: Arc<Mutex<Vec<isize>>> = Arc::new(Mutex::new(vec![]));
    static ref LAYERED_EXE_WHITELIST: Arc<Mutex<Vec<String>>> =
        Arc::new(Mutex::new(vec!["steam.exe".to_string()]));
    static ref TRAY_AND_MULTI_WINDOW_IDENTIFIERS: Arc<Mutex<Vec<String>>> =
        Arc::new(Mutex::new(vec![
            "explorer.exe".to_string(),
            "firefox.exe".to_string(),
            "chrome.exe".to_string(),
            "idea64.exe".to_string(),
            "ApplicationFrameHost.exe".to_string(),
            "steam.exe".to_string(),
        ]));
    static ref OBJECT_NAME_CHANGE_ON_LAUNCH: Arc<Mutex<Vec<String>>> = Arc::new(Mutex::new(vec![
        "firefox.exe".to_string(),
        "idea64.exe".to_string(),
    ]));
    static ref WORKSPACE_RULES: Arc<Mutex<HashMap<String, (usize, usize)>>> =
        Arc::new(Mutex::new(HashMap::new()));
    static ref MANAGE_IDENTIFIERS: Arc<Mutex<Vec<String>>> = Arc::new(Mutex::new(vec![]));
    static ref FLOAT_IDENTIFIERS: Arc<Mutex<Vec<String>>> = Arc::new(Mutex::new(vec![
        // mstsc.exe creates these on Windows 11 when a WSL process is launched
        // https://github.com/LGUG2Z/komorebi/issues/74
        "OPContainerClass".to_string(),
        "IHWindowClass".to_string()
    ]));
    static ref BORDER_OVERFLOW_IDENTIFIERS: Arc<Mutex<Vec<String>>> = Arc::new(Mutex::new(vec![]));
    static ref WSL2_UI_PROCESSES: Arc<Mutex<Vec<String>>> = Arc::new(Mutex::new(vec![
        "X410.exe".to_string(),
        "mstsc.exe".to_string(),
        "vcxsrv.exe".to_string(),
    ]));
    static ref SUBSCRIPTION_PIPES: Arc<Mutex<HashMap<String, File>>> =
        Arc::new(Mutex::new(HashMap::new()));
}

pub static CUSTOM_FFM: AtomicBool = AtomicBool::new(false);

fn setup() -> Result<(WorkerGuard, WorkerGuard)> {
    if std::env::var("RUST_LIB_BACKTRACE").is_err() {
        std::env::set_var("RUST_LIB_BACKTRACE", "1");
    }

    color_eyre::install()?;

    if std::env::var("RUST_LOG").is_err() {
        std::env::set_var("RUST_LOG", "info");
    }

    let home = dirs::home_dir().ok_or_else(|| anyhow!("there is no home directory"))?;
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
        if let Some(location) = panic.location() {
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
        } else {
            tracing::error!(message = %panic);
        }
    }));

    Ok((guard, color_guard))
}

pub fn load_configuration() -> Result<()> {
    let home = dirs::home_dir().ok_or_else(|| anyhow!("there is no home directory"))?;

    let mut config_v1 = home.clone();
    config_v1.push("komorebi.ahk");

    let mut config_v2 = home;
    config_v2.push("komorebi.ahk2");

    if config_v1.exists() && which("autohotkey.exe").is_ok() {
        tracing::info!(
            "loading configuration file: {}",
            config_v1
                .as_os_str()
                .to_str()
                .ok_or_else(|| anyhow!("cannot convert path to string"))?
        );

        Command::new("autohotkey.exe")
            .arg(config_v1.as_os_str())
            .output()?;
    } else if config_v2.exists() && which("AutoHotkey64.exe").is_ok() {
        tracing::info!(
            "loading configuration file: {}",
            config_v2
                .as_os_str()
                .to_str()
                .ok_or_else(|| anyhow!("cannot convert path to string"))?
        );

        Command::new("AutoHotkey64.exe")
            .arg(config_v2.as_os_str())
            .output()?;
    };

    Ok(())
}

#[derive(Debug, Serialize)]
#[serde(untagged)]
pub enum NotificationEvent {
    WindowManager(WindowManagerEvent),
    Socket(SocketMessage),
}

#[derive(Debug, Serialize)]
pub struct Notification {
    pub event: NotificationEvent,
    pub state: State,
}

pub fn notify_subscribers(notification: &str) -> Result<()> {
    let mut stale_subscriptions = vec![];
    let mut subscriptions = SUBSCRIPTION_PIPES.lock();
    for (subscriber, pipe) in subscriptions.iter_mut() {
        match writeln!(pipe, "{}", notification) {
            Ok(_) => {
                tracing::debug!("pushed notification to subscriber: {}", subscriber);
            }
            Err(error) => {
                // ERROR_FILE_NOT_FOUND
                // 2 (0x2)
                // The system cannot find the file specified.

                // ERROR_NO_DATA
                // 232 (0xE8)
                // The pipe is being closed.

                // Remove the subscription; the process will have to subscribe again
                if let Some(2 | 232) = error.raw_os_error() {
                    let subscriber_cl = subscriber.clone();
                    stale_subscriptions.push(subscriber_cl);
                }
            }
        }
    }

    for subscriber in stale_subscriptions {
        tracing::warn!("removing stale subscription: {}", subscriber);
        subscriptions.remove(&subscriber);
    }

    Ok(())
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
        let mut system = sysinfo::System::new_all();
        system.refresh_processes();

        if system.process_by_name("komorebi.exe").len() > 1 {
            tracing::error!("komorebi.exe is already running, please exit the existing process before starting a new one");
            std::process::exit(1);
        }

        // File logging worker guard has to have an assignment in the main fn to work
        let (_guard, _color_guard) = setup()?;

        #[cfg(feature = "deadlock_detection")]
        detect_deadlocks();

        let process_id = WindowsApi::current_process_id();
        WindowsApi::allow_set_foreground_window(process_id)?;

        let (outgoing, incoming): (Sender<WindowManagerEvent>, Receiver<WindowManagerEvent>) =
            crossbeam_channel::unbounded();

        let winevent_listener = winevent_listener::new(Arc::new(Mutex::new(outgoing)));
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
