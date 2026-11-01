//! The komorebi daemon's library: the vocabulary the window manager is written in, the data
//! structures it is built from, and the window manager.
//!
//! The desktop is modeled as monitors ([`monitor::Monitor`]), each with a ring of workspaces
//! ([`workspace::Workspace`]), each with a ring of containers ([`container::Container`]), each with
//! a ring of windows ([`window::Window`]). A workspace tiles its containers by its layout, and a
//! window is moved by asking the [`backend`].
//!
//! The [`window_manager::WindowManager`] keeps that model. It is loaded from what the backend
//! reports ([`desktop`]) and changed by what the compositor reports about windows
//! ([`window_manager_event`], [`process_event`]), which reaches it through
//! [`window_manager_event_listener`].
//!
//! On KDE Plasma the backend is [`kwin::KWinBackend`], which talks to a script it loads into KWin.
//! The compositor also draws what the windows look like: [`border_manager`] asks the backend for
//! the borders, [`theme_manager`] colors them from a theme, [`animation`] holds how windows are
//! animated going to their places, and [`transparency_manager`] makes the windows that do not have
//! the focus see-through. A monitor or a workspace can also have a wallpaper
//! ([`static_config::Wallpaper`]), which the backend has the Plasma shell show when the
//! workspace comes into view ([`backend::Backend::set_wallpaper`]).
//! When the compositor reports that a monitor changed size, [`monitor_reconciliator`] reads the
//! monitors again and tiles the windows in the area each has now. When a monitor is unplugged it
//! moves what was on it to the monitors that are left ([`reconcile`]), keeps the monitor for when
//! it is plugged in again, and adds a monitor it has not seen. A window that something else moves
//! to another output is followed to that monitor ([`reconcile`]).
//!
//! The daemon also listens on a socket, `komorebi.sock` in [`struct@DATA_DIR`], for the messages
//! in [`core::SocketMessage`]: [`process_command`] reads them and answers, with the window
//! manager's [`state::State`] for one.
//!
//! A program can subscribe to the daemon's notifications with a socket of its own in
//! [`struct@DATA_DIR`]: after each event, each message and each change of the monitors the window
//! manager handles, [`notify_subscribers`] sends every subscriber a [`Notification`] of it, with
//! the state.
//!
//! The checks of `komorebic check`, which read `komorebi.json` without starting the daemon, are in
//! [`check`].

use crate::core::ApplicationIdentifier;
use crate::core::SocketMessage;
use crate::core::SubscribeOptions;
use crate::core::config_generation::IdWithIdentifier;
use crate::core::config_generation::MatchingRule;
use crate::core::config_generation::MatchingStrategy;
use crate::core::config_generation::WorkspaceMatchingRule;
use crate::monitor_reconciliator::MonitorNotification;
use crate::state::State;
use crate::window_manager_event::WindowManagerEvent;
use color_eyre::eyre;
use core::pathext::PathExt;
use lazy_static::lazy_static;
use parking_lot::Mutex;
use regex::Regex;
use serde::Deserialize;
use serde::Serialize;
use std::collections::HashMap;
use std::collections::HashSet;
use std::os::unix::net::UnixStream;
use std::path::PathBuf;
use std::sync::Arc;

#[macro_use]
extern crate komorebi_collections;

pub use komorebi_collections::ring;

pub mod animation;
pub mod backend;
pub mod border_manager;
pub mod check;
pub mod container;
pub mod core;
pub mod daemon;
pub mod desktop;
pub mod kwin;
pub use komorebi_collections::lockable_sequence;
pub mod monitor;
pub mod monitor_reconciliator;
pub mod process_command;
pub mod process_event;
pub mod reconcile;
pub mod scrolling;
pub mod settings;
pub mod state;
pub mod static_config;
pub mod theme_manager;
pub mod transparency_manager;
pub mod window;
pub mod window_manager;
pub mod window_manager_event;
pub mod window_manager_event_listener;
pub mod workspace;

/// The error the window manager returns when it talks to the desktop: a report, since every
/// failure comes from the one backend.
pub type LibraryError = eyre::Report;

lazy_static! {
    /// Where komorebi looks for `komorebi.json` and the files next to it: `$KOMOREBI_CONFIG_HOME`
    /// if that is set, else `~/.config/komorebi`
    ///
    /// # Panics
    ///
    /// `$KOMOREBI_CONFIG_HOME` is set and is not a directory, or there is no home directory.
    pub static ref HOME_DIR: PathBuf = {
        std::env::var("KOMOREBI_CONFIG_HOME").map_or_else(
            |_| {
                dirs::home_dir()
                    .expect("there is no home directory")
                    .join(".config")
                    .join("komorebi")
            },
            |home_path| {
                let home = home_path.replace_env();

                assert!(
                    home.is_dir(),
                    "$KOMOREBI_CONFIG_HOME is set to \"{home_path}\", which is not a valid directory"
                );

                home
            },
        )
    };
    /// Where komorebi keeps the files it needs while it runs, such as the script it loads into
    /// KWin, its command socket and the sockets of its subscribers: `$XDG_RUNTIME_DIR/komorebi`,
    /// which is per-user, on tmpfs and removed at logout. Without a runtime directory (a bare ssh
    /// session, or the tests on a Mac) it falls back to the local data directory.
    pub static ref DATA_DIR: PathBuf = std::env::var_os("XDG_RUNTIME_DIR")
        .filter(|dir| !dir.is_empty())
        .map(|dir| PathBuf::from(dir).join("komorebi"))
        .unwrap_or_else(|| {
            dirs::data_local_dir()
                .expect("there is no local data directory")
                .join("komorebi")
        });
    /// Where the logs live: persistent, unlike [`struct@DATA_DIR`]. The daemon writes a file a day
    /// here, `komorebi.log.YYYY-MM-DD`, and the same without colors in
    /// `komorebi_plaintext.log.YYYY-MM-DD`.
    pub static ref LOG_DIR: PathBuf = dirs::data_local_dir()
        .expect("there is no local data directory")
        .join("komorebi");
    /// The subscribers: the name of each one's socket, and its path in [`struct@DATA_DIR`]
    pub static ref SUBSCRIPTION_SOCKETS: Arc<Mutex<HashMap<String, PathBuf>>> =
        Arc::new(Mutex::new(HashMap::new()));
    /// The options of the subscribers that were added with options, by the name of their socket
    ///
    /// An entry is not removed with its subscriber: a subscriber that is added again under the
    /// same name, without options, keeps the options it had.
    pub static ref SUBSCRIPTION_SOCKET_OPTIONS: Arc<Mutex<HashMap<String, SubscribeOptions>>> =
        Arc::new(Mutex::new(HashMap::new()));
    /// The rules that send the windows of an application to a workspace: when they open
    /// (`initial_only`) or whenever the window manager looks at them
    static ref WORKSPACE_MATCHING_RULES: Arc<Mutex<Vec<WorkspaceMatchingRule>>> =
        Arc::new(Mutex::new(Vec::new()));
    /// The compiled regular expression of each rule whose matching strategy is `Regex`, by the
    /// rule's id
    static ref REGEX_IDENTIFIERS: Arc<Mutex<HashMap<String, Regex>>> =
        Arc::new(Mutex::new(HashMap::new()));
    /// Windows that are tiled even when an ignore rule covers them (`ManageRule`)
    static ref MANAGE_IDENTIFIERS: Arc<Mutex<Vec<MatchingRule>>> = Arc::new(Mutex::new(vec![]));
    /// Windows that are never made see-through, whatever the rules of the transparency say
    pub static ref TRANSPARENCY_BLACKLIST: Arc<Mutex<Vec<MatchingRule>>> = Arc::new(Mutex::new(vec![]));
    /// Windows that are never tiled, unless a manage rule says otherwise. Most of what would belong
    /// here (the panel, krunner, popups) is not a normal window and is filtered before rules run.
    pub static ref IGNORE_IDENTIFIERS: Arc<Mutex<Vec<MatchingRule>>> = Arc::new(Mutex::new(
        ["plasmashell", "org.kde.plasmashell", "krunner", "org.kde.krunner"]
            .into_iter()
            .map(|class| MatchingRule::Simple(IdWithIdentifier {
                kind: ApplicationIdentifier::Class,
                id: String::from(class),
                matching_strategy: Option::from(MatchingStrategy::Equals),
            }))
            .collect()
    ));
    /// The float rules `SessionFloatRule` added, which `ClearSessionFloatRules` takes out of
    /// [`struct@FLOATING_APPLICATIONS`] again
    static ref SESSION_FLOATING_APPLICATIONS: Arc<Mutex<Vec<MatchingRule>>> =
        Arc::new(Mutex::new(Vec::new()));
    /// Windows that are managed as floating windows from the moment they open
    static ref FLOATING_APPLICATIONS: Arc<Mutex<Vec<MatchingRule>>> = Arc::new(Mutex::new(vec![]));
    /// Window classes that are never managed, whatever the rules say
    static ref PERMAIGNORE_CLASSES: Arc<Mutex<Vec<String>>> = Arc::new(Mutex::new(vec![]));
    /// The windows `UnmanageFocusedWindow` took out of the layout: komorebi leaves them alone
    /// until `ManageFocusedWindow` takes them back, or until they close
    static ref UNMANAGED_WINDOW_IDS: Arc<Mutex<Vec<u32>>> = Arc::new(Mutex::new(vec![]));
    /// The windows komorebi took out of sight (the back of a stack, on the parking desktop), so
    /// that what the compositor reports about them is not mistaken for something the user did
    pub static ref HIDDEN_WINDOWS: Arc<Mutex<HashSet<u32>>> = Arc::new(Mutex::new(HashSet::new()));
}

/// How many pixels `ResizeWindowEdge` and `ResizeWindowAxis` resize by, when `komorebi.json` does
/// not say
pub static DEFAULT_RESIZE_DELTA: i32 = komorebi_settings::shared::DEFAULT_RESIZE_DELTA;
/// Whether the pointer goes to the window that is given the focus, when `komorebi.json` does not
/// say
pub static DEFAULT_MOUSE_FOLLOWS_FOCUS: bool =
    komorebi_settings::shared::DEFAULT_MOUSE_FOLLOWS_FOCUS;

/// The event, the message or the change of the monitors a [`Notification`] is about
///
/// A [`WindowManagerEvent`] or a [`SocketMessage`] is written with its `type`, and a
/// [`MonitorNotification`] as its variant, `{"Resize":0}`, with no tag around either.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[serde(untagged)]
pub enum NotificationEvent {
    /// An event the compositor reported about a window
    WindowManager(WindowManagerEvent),
    /// A message the daemon took on its socket
    Socket(SocketMessage),
    /// A change of the monitors the compositor reported
    Monitor(MonitorNotification),
}

/// A notification that [`notify_subscribers`] sends each subscriber: an event, a message or a
/// change of the monitors the window manager handled, and its state after it
///
/// # Example
///
/// A subscriber reads a notification as JSON:
///
/// ```rust
/// use komorebi::Notification;
/// use komorebi::NotificationEvent;
/// use komorebi::core::SocketMessage;
/// use komorebi::monitor_reconciliator::MonitorNotification;
/// use komorebi::window_manager_event::WindowManagerEvent;
///
/// let state = r#"{"monitors":{"elements":[],"focused":0},"monitor_usr_idx_map":{},"is_paused":false,"resize_delta":50,"new_window_behaviour":"Create","float_override":false,"cross_monitor_move_behaviour":"Swap","monocle_focus_behaviour":"NoOp","unmanaged_window_operation_behaviour":"Op","work_area_offset":null,"mouse_follows_focus":true}"#;
///
/// let shown = format!(
///     r#"{{"event":{{"type":"Show","content":[{{"type":"KWin","content":"WindowAdded"}},{{"id":7}}]}},"state":{state}}}"#
/// );
/// let notification: Notification = serde_json::from_str(&shown)?;
/// assert!(matches!(
///     notification.event,
///     NotificationEvent::WindowManager(WindowManagerEvent::Show(_, window)) if window.id == 7
/// ));
///
/// let retiled = format!(r#"{{"event":{{"type":"Retile"}},"state":{state}}}"#);
/// let notification: Notification = serde_json::from_str(&retiled)?;
/// assert!(matches!(
///     notification.event,
///     NotificationEvent::Socket(SocketMessage::Retile)
/// ));
/// assert!(notification.state.mouse_follows_focus);
///
/// let resized = format!(r#"{{"event":{{"Resize":3}},"state":{state}}}"#);
/// let notification: Notification = serde_json::from_str(&resized)?;
/// assert!(matches!(
///     notification.event,
///     NotificationEvent::Monitor(MonitorNotification::Resize(3))
/// ));
///
/// # Ok::<(), Box<dyn std::error::Error>>(())
/// ```
#[derive(Clone, Debug, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
pub struct Notification {
    /// The event, the message or the change of the monitors the window manager handled
    pub event: NotificationEvent,
    /// The window manager's state after it
    pub state: State,
}

/// Sends `notification` to every subscriber in [`struct@SUBSCRIPTION_SOCKETS`], and removes the
/// subscribers whose socket cannot be connected to
///
/// Each subscriber is sent the notification as JSON on a new connection to its socket, which is
/// closed after it. A subscriber with `filter_state_changes` in its [`SubscribeOptions`] is sent it
/// only if `state_has_been_modified`, or if the notification is about a window being shown, a
/// subscriber being added, a change of the monitors, a theme being set (the colors of the borders
/// are not part of the [`State`]) or the configuration being reloaded.
///
/// A subscriber that cannot be connected to (its program has exited, or its socket was removed)
/// is removed, with its socket file, and is not tried again: it has to subscribe again. The
/// notification is written without a timeout, with the window manager locked, so a subscriber that
/// listens and never reads can hold up the window manager when a notification does not fit in the
/// socket's buffer.
///
/// # Errors
///
/// - The notification cannot be serialized.
/// - The notification cannot be written to a subscriber that was connected to. The subscribers
///   after it are not sent the notification, and none is removed.
///
/// # Example
///
/// ```rust
/// # let runtime = format!("/tmp/komorebi-notify-doc-{}", std::process::id());
/// # std::fs::create_dir_all(format!("{runtime}/komorebi"))?;
/// # unsafe { std::env::set_var("XDG_RUNTIME_DIR", &runtime) }; // the sockets are in it
/// use std::io::Read;
/// use std::os::unix::net::UnixListener;
///
/// use komorebi::DATA_DIR;
/// use komorebi::Notification;
/// use komorebi::NotificationEvent;
/// use komorebi::SUBSCRIPTION_SOCKETS;
/// use komorebi::core::SocketMessage;
/// use komorebi::notify_subscribers;
/// use komorebi::state::State;
///
/// // a subscriber that listens, and one whose program has exited
/// let bar = UnixListener::bind(DATA_DIR.join("bar.sock"))?;
/// SUBSCRIPTION_SOCKETS.lock().extend([
///     (String::from("bar.sock"), DATA_DIR.join("bar.sock")),
///     (String::from("gone.sock"), DATA_DIR.join("gone.sock")),
/// ]);
///
/// let state: State = serde_json::from_str(
///     r#"{"monitors":{"elements":[],"focused":0},"monitor_usr_idx_map":{},"is_paused":false,"resize_delta":50,"new_window_behaviour":"Create","float_override":false,"cross_monitor_move_behaviour":"Swap","monocle_focus_behaviour":"NoOp","unmanaged_window_operation_behaviour":"Op","work_area_offset":null,"mouse_follows_focus":true}"#,
/// )?;
/// let event = NotificationEvent::Socket(SocketMessage::Retile);
/// notify_subscribers(Notification { event, state }, false)?;
///
/// let mut json = String::new();
/// bar.accept()?.0.read_to_string(&mut json)?;
/// assert!(json.starts_with(r#"{"event":{"type":"Retile"},"state":{"#), "{json}");
/// assert!(!SUBSCRIPTION_SOCKETS.lock().contains_key("gone.sock"));
///
/// # std::fs::remove_dir_all(&runtime)?;
/// # Ok::<(), Box<dyn std::error::Error>>(())
/// ```
pub fn notify_subscribers(
    notification: Notification,
    state_has_been_modified: bool,
) -> eyre::Result<()> {
    let is_override_event = matches!(
        notification.event,
        NotificationEvent::Monitor(_)
            | NotificationEvent::Socket(SocketMessage::AddSubscriberSocket(_))
            | NotificationEvent::Socket(SocketMessage::AddSubscriberSocketWithOptions(_, _))
            | NotificationEvent::Socket(SocketMessage::Theme(_))
            | NotificationEvent::Socket(SocketMessage::ReloadStaticConfiguration(_))
            | NotificationEvent::WindowManager(WindowManagerEvent::Show(_, _))
    );

    let notification = &serde_json::to_string(&notification)?;
    let mut sockets = SUBSCRIPTION_SOCKETS.lock();
    let options = SUBSCRIPTION_SOCKET_OPTIONS.lock();

    komorebi_transport::notify_sockets(
        notification,
        &mut sockets,
        &options,
        state_has_been_modified || is_override_event,
        &DATA_DIR,
        |path| UnixStream::connect(path),
    )?;

    Ok(())
}
