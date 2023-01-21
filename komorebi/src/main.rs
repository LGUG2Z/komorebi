#![warn(clippy::all, clippy::nursery, clippy::pedantic)]
#![allow(clippy::missing_errors_doc, clippy::redundant_pub_crate)]

use std::collections::HashMap;
use std::fs::File;
use std::io::Write;
use std::net::TcpStream;
use std::path::PathBuf;
use std::process::Command;
use std::sync::atomic::AtomicBool;
use std::sync::atomic::AtomicI32;
use std::sync::atomic::AtomicIsize;
use std::sync::atomic::AtomicU32;
use std::sync::atomic::Ordering;
use std::sync::Arc;
#[cfg(feature = "deadlock_detection")]
use std::time::Duration;

use clap::Parser;
use color_eyre::eyre::anyhow;
use color_eyre::Result;
use crossbeam_channel::Receiver;
use crossbeam_channel::Sender;
use crossbeam_utils::Backoff;
use lazy_static::lazy_static;
use os_info::Version;
#[cfg(feature = "deadlock_detection")]
use parking_lot::deadlock;
use parking_lot::Mutex;
use schemars::JsonSchema;
use serde::Serialize;
use sysinfo::Process;
use sysinfo::ProcessExt;
use sysinfo::SystemExt;
use tracing_appender::non_blocking::WorkerGuard;
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::EnvFilter;
use which::which;
use winreg::enums::HKEY_CURRENT_USER;
use winreg::RegKey;

use crate::hidden::Hidden;
use komorebi_core::HidingBehaviour;
use komorebi_core::Rect;
use komorebi_core::SocketMessage;

use crate::process_command::listen_for_commands;
use crate::process_command::listen_for_commands_tcp;
use crate::process_event::listen_for_events;
use crate::process_movement::listen_for_movements;
use crate::window_manager::State;
use crate::window_manager::WindowManager;
use crate::window_manager_event::WindowManagerEvent;
use crate::windows_api::WindowsApi;

#[macro_use]
mod ring;

mod border;
mod container;
mod hidden;
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
    static ref LAYERED_WHITELIST: Arc<Mutex<Vec<String>>> =
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
    static ref MONITOR_INDEX_PREFERENCES: Arc<Mutex<HashMap<usize, Rect>>> =
        Arc::new(Mutex::new(HashMap::new()));
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
        "vcxsrv.exe".to_string(),
    ]));
    static ref SUBSCRIPTION_PIPES: Arc<Mutex<HashMap<String, File>>> =
        Arc::new(Mutex::new(HashMap::new()));
    static ref TCP_CONNECTIONS: Arc<Mutex<HashMap<String, TcpStream>>> =
        Arc::new(Mutex::new(HashMap::new()));
    static ref HIDING_BEHAVIOUR: Arc<Mutex<HidingBehaviour>> =
        Arc::new(Mutex::new(HidingBehaviour::Minimize));
    static ref HOME_DIR: PathBuf = {
        std::env::var("KOMOREBI_CONFIG_HOME").map_or_else(|_| dirs::home_dir().expect("there is no home directory"), |home_path| {
            let home = PathBuf::from(&home_path);

            if home.as_path().is_dir() {
                home
            } else {
                panic!(
                    "$Env:KOMOREBI_CONFIG_HOME is set to '{}', which is not a valid directory",
                    home_path
                );
            }
        })
    };
    static ref DATA_DIR: PathBuf = dirs::data_local_dir().expect("there is no local data directory").join("komorebi");
    static ref AHK_V1_EXE: String = {
        let mut ahk_v1: String = String::from("autohotkey.exe");

        if let Ok(komorebi_ahk_v1_exe) = std::env::var("KOMOREBI_AHK_V1_EXE") {
            if which(&komorebi_ahk_v1_exe).is_ok() {
                ahk_v1 = komorebi_ahk_v1_exe;
            }
        }

        ahk_v1
    };
    static ref AHK_V2_EXE: String = {
        let mut ahk_v2: String = String::from("AutoHotkey64.exe");

        if let Ok(komorebi_ahk_v2_exe) = std::env::var("KOMOREBI_AHK_V2_EXE") {
            if which(&komorebi_ahk_v2_exe).is_ok() {
                ahk_v2 = komorebi_ahk_v2_exe;
            }
        }

        ahk_v2
    };

    static ref WINDOWS_11: bool = {
        matches!(
            os_info::get().version(),
            Version::Semantic(_, _, x) if x >= &22000
        )
    };

    static ref BORDER_RECT: Arc<Mutex<Rect>> =
        Arc::new(Mutex::new(Rect::default()));

    static ref BORDER_OFFSET: Arc<Mutex<Option<Rect>>> =
        Arc::new(Mutex::new(None));
}

pub static INITIAL_CONFIGURATION_LOADED: AtomicBool = AtomicBool::new(false);
pub static CUSTOM_FFM: AtomicBool = AtomicBool::new(false);
pub static SESSION_ID: AtomicU32 = AtomicU32::new(0);
pub static ALT_FOCUS_HACK: AtomicBool = AtomicBool::new(false);
pub static BORDER_ENABLED: AtomicBool = AtomicBool::new(false);
pub static BORDER_HWND: AtomicIsize = AtomicIsize::new(0);
pub static BORDER_HIDDEN: AtomicBool = AtomicBool::new(false);
pub static BORDER_COLOUR_SINGLE: AtomicU32 = AtomicU32::new(0);
pub static BORDER_COLOUR_STACK: AtomicU32 = AtomicU32::new(0);
pub static BORDER_COLOUR_CURRENT: AtomicU32 = AtomicU32::new(0);
pub static BORDER_WIDTH: AtomicI32 = AtomicI32::new(20);
// 0 0 0 aka pure black, I doubt anyone will want this as a border colour
pub const TRANSPARENCY_COLOUR: u32 = 0;

pub static HIDDEN_HWND: AtomicIsize = AtomicIsize::new(0);

fn setup() -> Result<(WorkerGuard, WorkerGuard)> {
    if std::env::var("RUST_LIB_BACKTRACE").is_err() {
        std::env::set_var("RUST_LIB_BACKTRACE", "1");
    }

    color_eyre::install()?;

    if std::env::var("RUST_LOG").is_err() {
        std::env::set_var("RUST_LOG", "info");
    }

    let appender = tracing_appender::rolling::never(DATA_DIR.clone(), "komorebi.log");
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

pub fn load_configuration() -> Result<()> {
    let home = HOME_DIR.clone();

    let mut config_v1 = home.clone();
    config_v1.push("komorebi.ahk");

    let mut config_v2 = home;
    config_v2.push("komorebi.ahk2");

    if config_v1.exists() && which(&*AHK_V1_EXE).is_ok() {
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
    } else if config_v2.exists() && which(&*AHK_V2_EXE).is_ok() {
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
    }

    Ok(())
}

pub fn current_virtual_desktop() -> Option<Vec<u8>> {
    let hkcu = RegKey::predef(HKEY_CURRENT_USER);

    // This is the path on Windows 10
    let mut current = hkcu
        .open_subkey(format!(
            r#"SOFTWARE\Microsoft\Windows\CurrentVersion\Explorer\SessionInfo\{}\VirtualDesktops"#,
            SESSION_ID.load(Ordering::SeqCst)
        ))
        .ok()
        .and_then(
            |desktops| match desktops.get_raw_value("CurrentVirtualDesktop") {
                Ok(current) => Option::from(current.bytes),
                Err(_) => None,
            },
        );

    // This is the path on Windows 11
    if current.is_none() {
        current = hkcu
            .open_subkey(r#"SOFTWARE\Microsoft\Windows\CurrentVersion\Explorer\VirtualDesktops"#)
            .ok()
            .and_then(
                |desktops| match desktops.get_raw_value("CurrentVirtualDesktop") {
                    Ok(current) => Option::from(current.bytes),
                    Err(_) => None,
                },
            );
    }

    // For Win10 users that do not use virtual desktops, the CurrentVirtualDesktop value will not
    // exist until one has been created in the task view

    // The registry value will also not exist on user login if virtual desktops have been created
    // but the task view has not been initiated

    // In both of these cases, we return None, and the virtual desktop validation will never run. In
    // the latter case, if the user desires this validation after initiating the task view, komorebi
    // should be restarted, and then when this // fn runs again for the first time, it will pick up
    // the value of CurrentVirtualDesktop and validate against it accordingly
    current
}

#[derive(Debug, Serialize, JsonSchema)]
#[serde(untagged)]
pub enum NotificationEvent {
    WindowManager(WindowManagerEvent),
    Socket(SocketMessage),
}

#[derive(Debug, Serialize, JsonSchema)]
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

#[derive(Parser)]
#[clap(author, about, version)]
struct Opts {
    /// Allow the use of komorebi's custom focus-follows-mouse implementation
    #[clap(action, short, long = "ffm")]
    focus_follows_mouse: bool,
    /// Wait for 'komorebic complete-configuration' to be sent before processing events
    #[clap(action, short, long)]
    await_configuration: bool,
    /// Start a TCP server on the given port to allow the direct sending of SocketMessages
    #[clap(action, short, long)]
    tcp_port: Option<usize>,
}

#[tracing::instrument]
#[allow(clippy::nonminimal_bool)]
fn main() -> Result<()> {
    let opts: Opts = Opts::parse();
    CUSTOM_FFM.store(opts.focus_follows_mouse, Ordering::SeqCst);

    let arg_count = std::env::args().count();

    let has_valid_args = arg_count == 1
        || (arg_count == 2
            && (opts.await_configuration || opts.focus_follows_mouse || opts.tcp_port.is_some()))
        || (arg_count == 3 && opts.await_configuration && opts.focus_follows_mouse)
        || (arg_count == 3 && opts.tcp_port.is_some() && opts.focus_follows_mouse)
        || (arg_count == 3 && opts.tcp_port.is_some() && opts.await_configuration)
        || (arg_count == 4
            && (opts.focus_follows_mouse && opts.await_configuration && opts.tcp_port.is_some()));

    if has_valid_args {
        let process_id = WindowsApi::current_process_id();
        WindowsApi::allow_set_foreground_window(process_id)?;
        WindowsApi::set_process_dpi_awareness_context()?;

        let session_id = WindowsApi::process_id_to_session_id()?;
        SESSION_ID.store(session_id, Ordering::SeqCst);

        let mut system = sysinfo::System::new_all();
        system.refresh_processes();

        let matched_procs: Vec<&Process> = system.processes_by_name("komorebi.exe").collect();

        if matched_procs.len() > 1 {
            let mut len = matched_procs.len();
            for proc in matched_procs {
                if proc.root().ends_with("shims") {
                    len -= 1;
                }
            }

            if len > 1 {
                tracing::error!("komorebi.exe is already running, please exit the existing process before starting a new one");
                std::process::exit(1);
            }
        }

        // File logging worker guard has to have an assignment in the main fn to work
        let (_guard, _color_guard) = setup()?;

        #[cfg(feature = "deadlock_detection")]
        detect_deadlocks();

        let (outgoing, incoming): (Sender<WindowManagerEvent>, Receiver<WindowManagerEvent>) =
            crossbeam_channel::unbounded();

        let winevent_listener = winevent_listener::new(Arc::new(Mutex::new(outgoing)));
        winevent_listener.start();

        Hidden::create("komorebi-hidden")?;

        let wm = Arc::new(Mutex::new(WindowManager::new(Arc::new(Mutex::new(
            incoming,
        )))?));

        wm.lock().init()?;
        listen_for_commands(wm.clone());

        if !opts.await_configuration && !INITIAL_CONFIGURATION_LOADED.load(Ordering::SeqCst) {
            INITIAL_CONFIGURATION_LOADED.store(true, Ordering::SeqCst);
        };

        if let Some(port) = opts.tcp_port {
            listen_for_commands_tcp(wm.clone(), port);
        }

        std::thread::spawn(|| {
            load_configuration().expect("could not load configuration");
        });

        if opts.await_configuration {
            let backoff = Backoff::new();
            while !INITIAL_CONFIGURATION_LOADED.load(Ordering::SeqCst) {
                backoff.snooze();
            }
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

        wm.lock().restore_all_windows();

        if WindowsApi::focus_follows_mouse()? {
            WindowsApi::disable_focus_follows_mouse()?;
        }

        std::process::exit(130);
    }

    Ok(())
}
