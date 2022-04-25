#![warn(clippy::all, clippy::nursery, clippy::pedantic)]
#![allow(clippy::missing_errors_doc)]

use color_eyre::eyre::anyhow;
use color_eyre::Result;
use lazy_static::lazy_static;
use parking_lot::Mutex;
use schemars::JsonSchema;
use serde::Deserialize;
use serde::Serialize;
use std::collections::HashMap;
use std::fs::File;
use std::io::Write;
use std::path::PathBuf;
use std::process::Command;
use std::sync::atomic::AtomicBool;
use std::sync::atomic::AtomicU32;
use std::sync::atomic::Ordering;
use std::sync::Arc;
use which::which;
use winreg::enums::HKEY_CURRENT_USER;
use winreg::RegKey;

use komorebi_core::HidingBehaviour;
use komorebi_core::SocketMessage;

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

pub use process_command::listen_for_commands;
pub use process_event::listen_for_events;
pub use process_movement::listen_for_movements;
pub use window_manager::State;
pub use window_manager::WindowManager;
pub use window_manager_event::WindowManagerEvent;
pub use windows_api::WindowsApi;
pub use winevent_listener::WinEventListener;

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
    static ref HIDING_BEHAVIOUR: Arc<Mutex<HidingBehaviour>> =
        Arc::new(Mutex::new(HidingBehaviour::Minimize));
    pub static ref HOME_DIR: PathBuf = {
        if let Ok(home_path) = std::env::var("KOMOREBI_CONFIG_HOME") {
            let home = PathBuf::from(&home_path);

            if home.as_path().is_dir() {
                home
            } else {
                panic!(
                    "$Env:KOMOREBI_CONFIG_HOME is set to '{}', which is not a valid directory",
                    home_path
                );
            }
        } else {
            dirs::home_dir().expect("there is no home directory")
        }
    };
}

pub static CUSTOM_FFM: AtomicBool = AtomicBool::new(false);
pub static SESSION_ID: AtomicU32 = AtomicU32::new(0);

fn current_virtual_desktop() -> Option<Vec<u8>> {
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

pub fn load_configuration() -> Result<()> {
    let home = HOME_DIR.clone();

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

#[derive(Debug, Serialize, Deserialize, JsonSchema)]
#[serde(untagged)]
pub enum NotificationEvent {
    WindowManager(WindowManagerEvent),
    Socket(SocketMessage),
}

#[derive(Debug, Serialize, Deserialize, JsonSchema)]
pub struct Notification {
    pub event: NotificationEvent,
    pub state: State,
}

fn notify_subscribers(notification: &str) {
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
}
