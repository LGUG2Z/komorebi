pub mod border;
pub mod com;
#[macro_use]
pub mod ring;
pub mod colour;
pub mod container;
pub mod hidden;
pub mod monitor;
pub mod process_command;
pub mod process_event;
pub mod process_movement;
pub mod set_window_position;
pub mod stackbar;
pub mod static_config;
pub mod styles;
pub mod window;
pub mod window_manager;
pub mod window_manager_event;
pub mod windows_api;
pub mod windows_callbacks;
pub mod winevent;
pub mod winevent_listener;
pub mod workspace;

use lazy_static::lazy_static;
use std::collections::HashMap;
use std::collections::VecDeque;
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

pub use hidden::*;
pub use process_command::*;
pub use process_event::*;
pub use stackbar::*;
pub use static_config::*;
pub use window_manager::*;
pub use window_manager_event::*;
pub use windows_api::WindowsApi;
pub use windows_api::*;

use color_eyre::Result;
use komorebi_core::config_generation::IdWithIdentifier;
use komorebi_core::config_generation::MatchingRule;
use komorebi_core::config_generation::MatchingStrategy;
use komorebi_core::ApplicationIdentifier;
use komorebi_core::HidingBehaviour;
use komorebi_core::Rect;
use komorebi_core::SocketMessage;
use os_info::Version;
use parking_lot::Mutex;
use regex::Regex;
use schemars::JsonSchema;
use serde::Deserialize;
use serde::Serialize;
use uds_windows::UnixStream;
use which::which;
use winreg::enums::HKEY_CURRENT_USER;
use winreg::RegKey;

type WorkspaceRule = (usize, usize, bool);

lazy_static! {
    static ref HIDDEN_HWNDS: Arc<Mutex<Vec<isize>>> = Arc::new(Mutex::new(vec![]));
    static ref LAYERED_WHITELIST: Arc<Mutex<Vec<MatchingRule>>> = Arc::new(Mutex::new(vec![
        MatchingRule::Simple(IdWithIdentifier {
            kind: ApplicationIdentifier::Exe,
            id: String::from("steam.exe"),
            matching_strategy: Option::from(MatchingStrategy::Equals),
        }),
    ]));
    static ref TRAY_AND_MULTI_WINDOW_IDENTIFIERS: Arc<Mutex<Vec<MatchingRule>>> =
        Arc::new(Mutex::new(vec![
            MatchingRule::Simple(IdWithIdentifier {
                kind: ApplicationIdentifier::Exe,
                id: String::from("explorer.exe"),
                matching_strategy: Option::from(MatchingStrategy::Equals),
            }),
            MatchingRule::Simple(IdWithIdentifier {
                kind: ApplicationIdentifier::Exe,
                id: String::from("firefox.exe"),
                matching_strategy: Option::from(MatchingStrategy::Equals),
            }),
            MatchingRule::Simple(IdWithIdentifier {
                kind: ApplicationIdentifier::Exe,
                id: String::from("chrome.exe"),
                matching_strategy: Option::from(MatchingStrategy::Equals),
            }),
            MatchingRule::Simple(IdWithIdentifier {
                kind: ApplicationIdentifier::Exe,
                id: String::from("idea64.exe"),
                matching_strategy: Option::from(MatchingStrategy::Equals),
            }),
            MatchingRule::Simple(IdWithIdentifier {
                kind: ApplicationIdentifier::Exe,
                id: String::from("ApplicationFrameHost.exe"),
                matching_strategy: Option::from(MatchingStrategy::Equals),
            }),
            MatchingRule::Simple(IdWithIdentifier {
                kind: ApplicationIdentifier::Exe,
                id: String::from("steam.exe"),
                matching_strategy: Option::from(MatchingStrategy::Equals),
            })
        ]));
    static ref OBJECT_NAME_CHANGE_ON_LAUNCH: Arc<Mutex<Vec<MatchingRule>>> = Arc::new(Mutex::new(vec![
        MatchingRule::Simple(IdWithIdentifier {
            kind: ApplicationIdentifier::Exe,
            id: String::from("firefox.exe"),
            matching_strategy: Option::from(MatchingStrategy::Equals),
        }),
        MatchingRule::Simple(IdWithIdentifier {
            kind: ApplicationIdentifier::Exe,
            id: String::from("idea64.exe"),
            matching_strategy: Option::from(MatchingStrategy::Equals),
        }),
    ]));
    static ref MONITOR_INDEX_PREFERENCES: Arc<Mutex<HashMap<usize, Rect>>> =
        Arc::new(Mutex::new(HashMap::new()));
    static ref DISPLAY_INDEX_PREFERENCES: Arc<Mutex<HashMap<usize, String>>> =
        Arc::new(Mutex::new(HashMap::new()));
    static ref WORKSPACE_RULES: Arc<Mutex<HashMap<String, WorkspaceRule>>> =
        Arc::new(Mutex::new(HashMap::new()));
    static ref REGEX_IDENTIFIERS: Arc<Mutex<HashMap<String, Regex>>> =
        Arc::new(Mutex::new(HashMap::new()));
    static ref MANAGE_IDENTIFIERS: Arc<Mutex<Vec<MatchingRule>>> = Arc::new(Mutex::new(vec![]));
    static ref FLOAT_IDENTIFIERS: Arc<Mutex<Vec<MatchingRule>>> = Arc::new(Mutex::new(vec![
        // mstsc.exe creates these on Windows 11 when a WSL process is launched
        // https://github.com/LGUG2Z/komorebi/issues/74
        MatchingRule::Simple(IdWithIdentifier {
            kind: ApplicationIdentifier::Class,
            id: String::from("OPContainerClass"),
            matching_strategy: Option::from(MatchingStrategy::Equals),
        }),
        MatchingRule::Simple(IdWithIdentifier {
            kind: ApplicationIdentifier::Class,
            id: String::from("IHWindowClass"),
            matching_strategy: Option::from(MatchingStrategy::Equals),
        })
    ]));

    static ref PERMAIGNORE_CLASSES: Arc<Mutex<Vec<String>>> = Arc::new(Mutex::new(vec![
        "Chrome_RenderWidgetHostHWND".to_string(),
    ]));
    static ref BORDER_OVERFLOW_IDENTIFIERS: Arc<Mutex<Vec<MatchingRule>>> = Arc::new(Mutex::new(vec![]));
    static ref WSL2_UI_PROCESSES: Arc<Mutex<Vec<String>>> = Arc::new(Mutex::new(vec![
        "X410.exe".to_string(),
        "vcxsrv.exe".to_string(),
    ]));
    static ref SUBSCRIPTION_PIPES: Arc<Mutex<HashMap<String, File>>> =
        Arc::new(Mutex::new(HashMap::new()));
    static ref SUBSCRIPTION_SOCKETS: Arc<Mutex<HashMap<String, PathBuf>>> =
        Arc::new(Mutex::new(HashMap::new()));
    static ref TCP_CONNECTIONS: Arc<Mutex<HashMap<String, TcpStream>>> =
        Arc::new(Mutex::new(HashMap::new()));
    static ref HIDING_BEHAVIOUR: Arc<Mutex<HidingBehaviour>> =
        Arc::new(Mutex::new(HidingBehaviour::Minimize));
    pub static ref HOME_DIR: PathBuf = {
        std::env::var("KOMOREBI_CONFIG_HOME").map_or_else(|_| dirs::home_dir().expect("there is no home directory"), |home_path| {
            let home = PathBuf::from(&home_path);

            if home.as_path().is_dir() {
                home
            } else {
                panic!(
                    "$Env:KOMOREBI_CONFIG_HOME is set to '{home_path}', which is not a valid directory",
                );
            }
        })
    };
    pub static ref DATA_DIR: PathBuf = dirs::data_local_dir().expect("there is no local data directory").join("komorebi");
    pub static ref AHK_EXE: String = {
        let mut ahk: String = String::from("autohotkey.exe");

        if let Ok(komorebi_ahk_exe) = std::env::var("KOMOREBI_AHK_EXE") {
            if which(&komorebi_ahk_exe).is_ok() {
                ahk = komorebi_ahk_exe;
            }
        }

        ahk
    };
    static ref WINDOWS_11: bool = {
        matches!(
            os_info::get().version(),
            Version::Semantic(_, _, x) if x >= &22000
        )
    };

    static ref BORDER_RECT: Arc<Mutex<Rect>> =
        Arc::new(Mutex::new(Rect::default()));


    // Use app-specific titlebar removal options where possible
    // eg. Windows Terminal, IntelliJ IDEA, Firefox
    static ref NO_TITLEBAR: Arc<Mutex<Vec<String>>> = Arc::new(Mutex::new(vec![]));

    static ref STACKBAR_MODE: Arc<Mutex<StackbarMode >> = Arc::new(Mutex::new(StackbarMode::Never));
    static ref WINDOWS_BY_BAR_HWNDS: Arc<Mutex<HashMap<isize, VecDeque<isize>>>> =
        Arc::new(Mutex::new(HashMap::new()));

}

pub static DEFAULT_WORKSPACE_PADDING: AtomicI32 = AtomicI32::new(10);
pub static DEFAULT_CONTAINER_PADDING: AtomicI32 = AtomicI32::new(10);

pub static INITIAL_CONFIGURATION_LOADED: AtomicBool = AtomicBool::new(false);
pub static CUSTOM_FFM: AtomicBool = AtomicBool::new(false);
pub static SESSION_ID: AtomicU32 = AtomicU32::new(0);
pub static BORDER_ENABLED: AtomicBool = AtomicBool::new(false);
pub static BORDER_HWND: AtomicIsize = AtomicIsize::new(0);
pub static BORDER_HIDDEN: AtomicBool = AtomicBool::new(false);
pub static BORDER_COLOUR_SINGLE: AtomicU32 = AtomicU32::new(0);
pub static BORDER_COLOUR_STACK: AtomicU32 = AtomicU32::new(0);
pub static BORDER_COLOUR_MONOCLE: AtomicU32 = AtomicU32::new(0);
pub static BORDER_COLOUR_CURRENT: AtomicU32 = AtomicU32::new(0);
pub static BORDER_WIDTH: AtomicI32 = AtomicI32::new(8);
pub static BORDER_OFFSET: AtomicI32 = AtomicI32::new(-1);

// 0 0 0 aka pure black, I doubt anyone will want this as a border colour
pub const TRANSPARENCY_COLOUR: u32 = 0;
pub static REMOVE_TITLEBARS: AtomicBool = AtomicBool::new(false);

pub static HIDDEN_HWND: AtomicIsize = AtomicIsize::new(0);

pub static STACKBAR_FOCUSED_TEXT_COLOUR: AtomicU32 = AtomicU32::new(16777215); // white
pub static STACKBAR_UNFOCUSED_TEXT_COLOUR: AtomicU32 = AtomicU32::new(11776947); // gray text
pub static STACKBAR_TAB_BACKGROUND_COLOUR: AtomicU32 = AtomicU32::new(3355443); // gray
pub static STACKBAR_TAB_HEIGHT: AtomicI32 = AtomicI32::new(40);
pub static STACKBAR_TAB_WIDTH: AtomicI32 = AtomicI32::new(200);

#[must_use]
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
            .open_subkey(r"SOFTWARE\Microsoft\Windows\CurrentVersion\Explorer\VirtualDesktops")
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

pub fn notify_subscribers(notification: &str) -> Result<()> {
    let mut stale_sockets = vec![];
    let mut sockets = SUBSCRIPTION_SOCKETS.lock();

    for (socket, path) in &mut *sockets {
        match UnixStream::connect(path) {
            Ok(mut stream) => {
                tracing::debug!("pushed notification to subscriber: {socket}");
                stream.write_all(notification.as_bytes())?;
            }
            Err(_) => {
                stale_sockets.push(socket.clone());
            }
        }
    }

    for socket in stale_sockets {
        tracing::warn!("removing stale subscription: {socket}");
        sockets.remove(&socket);
    }

    let mut stale_pipes = vec![];
    let mut pipes = SUBSCRIPTION_PIPES.lock();
    for (subscriber, pipe) in &mut *pipes {
        match writeln!(pipe, "{notification}") {
            Ok(()) => {
                tracing::debug!("pushed notification to subscriber: {subscriber}");
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
                    stale_pipes.push(subscriber.clone());
                }
            }
        }
    }

    for subscriber in stale_pipes {
        tracing::warn!("removing stale subscription: {}", subscriber);
        pipes.remove(&subscriber);
    }

    Ok(())
}

pub fn load_configuration() -> Result<()> {
    let config_pwsh = HOME_DIR.join("komorebi.ps1");
    let config_ahk = HOME_DIR.join("komorebi.ahk");

    if config_pwsh.exists() {
        let powershell_exe = if which("pwsh.exe").is_ok() {
            "pwsh.exe"
        } else {
            "powershell.exe"
        };

        tracing::info!("loading configuration file: {}", config_pwsh.display());

        Command::new(powershell_exe)
            .arg(config_pwsh.as_os_str())
            .output()?;
    } else if config_ahk.exists() && which(&*AHK_EXE).is_ok() {
        tracing::info!("loading configuration file: {}", config_ahk.display());

        Command::new(&*AHK_EXE)
            .arg(config_ahk.as_os_str())
            .output()?;
    }

    Ok(())
}
