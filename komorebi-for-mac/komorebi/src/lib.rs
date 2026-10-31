#![warn(clippy::all)]

use crate::accessibility::error::AccessibilityError;
use crate::core::ApplicationIdentifier;
use crate::core::DefaultLayout;
use crate::core::LayoutDefaultEntry;
use crate::core::SocketMessage;
use crate::core::SubscribeOptions;
use crate::core::WindowHidingPosition;
use crate::core::config_generation::IdWithIdentifier;
use crate::core::config_generation::MatchingRule;
use crate::core::config_generation::MatchingStrategy;
use crate::core::config_generation::WorkspaceMatchingRule;
use crate::core_graphics::error::CoreGraphicsError;
use crate::monitor_reconciliator::MonitorNotification;
use crate::state::State;
use crate::window::AspectRatio;
use crate::window::PredefinedAspectRatio;
use crate::window_manager_event::WindowManagerEvent;
use color_eyre::eyre;
use core::pathext::PathExt;
use lazy_static::lazy_static;
use objc2_application_services::AXObserver;
use objc2_application_services::AXUIElement;
use objc2_core_foundation::CFArray;
use objc2_core_foundation::CFDictionary;
use objc2_core_foundation::CFRetained;
use objc2_core_foundation::CFRunLoop;
use objc2_core_foundation::CFString;
use objc2_core_foundation::CGPoint;
use objc2_core_foundation::CGRect;
use objc2_core_foundation::CGSize;
use parking_lot::Mutex;
use parking_lot::RwLock;
use regex::Regex;
use serde::Deserialize;
use serde::Serialize;
use std::collections::HashMap;
use std::io::Write;
use std::ops::Deref;
use std::os::unix::net::UnixStream;
use std::panic;
use std::path::PathBuf;
use std::ptr::NonNull;
use std::sync::Arc;
use std::sync::atomic::AtomicI32;

#[macro_use]
pub mod ring;

pub mod accessibility;
pub mod animation;
pub mod app_kit_notification_constants;
pub mod application;
pub mod arrival;
pub mod border_manager;
pub mod container;
pub mod core;
pub mod core_graphics;
pub mod display_reconfiguration_listener;
pub mod input_event_listener;
pub mod ioreg;
pub mod lockable_sequence;
pub mod macos_api;
pub mod monitor;
pub mod monitor_reconciliator;
pub mod notification_center_listener;
pub mod pan;
pub mod process_command;
pub mod process_event;
pub mod reaper;
pub mod scrolling;
pub mod settle;
pub mod skylight;
pub mod splash;
pub mod state;
pub mod static_config;
pub mod theme_manager;
pub mod window;
pub mod window_manager;
pub mod window_manager_event;
pub mod window_manager_event_listener;
pub mod workspace;
pub mod workspace_reconciliator;

lazy_static! {
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
    pub static ref DATA_DIR: PathBuf = dirs::data_local_dir()
        .expect("there is no local data directory")
        .join("komorebi");
    pub static ref SUBSCRIPTION_SOCKETS: Arc<Mutex<HashMap<String, PathBuf>>> =
        Arc::new(Mutex::new(HashMap::new()));
    pub static ref SUBSCRIPTION_SOCKET_OPTIONS: Arc<Mutex<HashMap<String, SubscribeOptions>>> =
        Arc::new(Mutex::new(HashMap::new()));
    static ref FLOATING_WINDOW_TOGGLE_ASPECT_RATIO: Arc<Mutex<AspectRatio>> = Arc::new(Mutex::new(
        AspectRatio::Predefined(PredefinedAspectRatio::Widescreen)
    ));
    static ref DISPLAY_INDEX_PREFERENCES: Arc<RwLock<HashMap<usize, String>>> =
        Arc::new(RwLock::new(HashMap::new()));
    static ref WINDOW_RESTORE_POSITIONS: Arc<Mutex<HashMap<u32, CGRect>>> =
        Arc::new(Mutex::new(HashMap::new()));
    static ref WORKSPACE_MATCHING_RULES: Arc<Mutex<Vec<WorkspaceMatchingRule>>> =
        Arc::new(Mutex::new(Vec::new()));
    static ref REGEX_IDENTIFIERS: Arc<Mutex<HashMap<String, Regex>>> =
        Arc::new(Mutex::new(HashMap::new()));
    static ref MANAGE_IDENTIFIERS: Arc<Mutex<Vec<MatchingRule>>> = Arc::new(Mutex::new(vec![]));
    static ref IGNORE_IDENTIFIERS: Arc<Mutex<Vec<MatchingRule>>> = Arc::new(Mutex::new(vec![
        MatchingRule::Simple(IdWithIdentifier {
            kind: ApplicationIdentifier::Exe,
            id: String::from("Spotlight"),
            matching_strategy: Option::from(MatchingStrategy::Equals),
        }),
        MatchingRule::Simple(IdWithIdentifier {
            kind: ApplicationIdentifier::Exe,
            id: String::from("komorebi-bar"),
            matching_strategy: Option::from(MatchingStrategy::Equals),
        })
    ]));
    static ref SESSION_FLOATING_APPLICATIONS: Arc<Mutex<Vec<MatchingRule>>> =
        Arc::new(Mutex::new(Vec::new()));
    static ref FLOATING_APPLICATIONS: Arc<Mutex<Vec<MatchingRule>>> =
        Arc::new(Mutex::new(vec![MatchingRule::Simple(IdWithIdentifier {
            kind: ApplicationIdentifier::Exe,
            id: String::from("komorebi-shortcuts.exe"),
            matching_strategy: Option::from(MatchingStrategy::Equals),
        })]));
    static ref PERMAIGNORE_CLASSES: Arc<Mutex<Vec<String>>> =
        Arc::new(Mutex::new(vec!["AXDialog".to_string(),]));
    static ref TABBED_APPLICATIONS: Arc<Mutex<Vec<String>>> = Arc::new(Mutex::new(Vec::new()));
    static ref DO_NOT_OBSERVE_APPLICATIONS: Arc<Mutex<Vec<String>>> = Arc::new(Mutex::new(vec![
        "Spotlight".to_string(),
        "komorebi".to_string(),
        "komorebic".to_string()
    ]));
    static ref TITLELESS_APPLICATIONS: Arc<Mutex<Vec<String>>> = Arc::new(Mutex::new(Vec::new()));
    static ref UNMANAGED_WINDOW_IDS: Arc<Mutex<Vec<u32>>> = Arc::new(Mutex::new(vec![]));
    pub static ref LAYOUT_DEFAULTS: Arc<Mutex<HashMap<DefaultLayout, LayoutDefaultEntry>>> =
        Arc::new(Mutex::new(HashMap::new()));
}

pub static DEFAULT_WORKSPACE_PADDING: AtomicI32 = AtomicI32::new(5);
pub static DEFAULT_CONTAINER_PADDING: AtomicI32 = AtomicI32::new(5);
pub static DEFAULT_RESIZE_DELTA: i32 = 50;
pub static DEFAULT_MOUSE_FOLLOWS_FOCUS: bool = true;

pub const PUBLIC_KEY: [u8; 32] = [
    0x5a, 0x69, 0x4a, 0xe1, 0x3c, 0x4b, 0xc8, 0x4e, 0xc3, 0x79, 0x0f, 0xab, 0x27, 0x6b, 0x7e, 0xdd,
    0x6b, 0x39, 0x6f, 0xa2, 0xc3, 0x9f, 0x3d, 0x48, 0xf2, 0x72, 0x56, 0x41, 0x1b, 0xc8, 0x08, 0xdb,
];

#[derive(Default, Debug, Clone, PartialEq, Deserialize)]
pub struct License {
    #[serde(rename = "hasValidSubscription")]
    pub has_valid_subscription: bool,
    pub timestamp: i64,
    #[serde(rename = "currentEndPeriod")]
    pub current_end_period: Option<i64>,
    pub signature: String,
}

#[must_use]
pub fn current_space_id() -> Option<u64> {
    panic::catch_unwind(|| unsafe { skylight::CGSGetActiveSpace(skylight::CGSMainConnectionID()) })
        .ok()
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[serde(untagged)]
pub enum NotificationEvent {
    WindowManager(WindowManagerEvent),
    Socket(SocketMessage),
    Monitor(MonitorNotification),
    // TODO: See if we want reaper notifications as well
    // // TODO: See if we're actually gonna use this
    // VirtualDesktop(VirtualDesktopNotification),
}

// #[derive(Debug, Copy, Clone, Serialize, Deserialize, PartialEq)]
// #[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
// pub enum VirtualDesktopNotification {
//     EnteredAssociatedVirtualDesktop,
//     LeftAssociatedVirtualDesktop,
// }

#[derive(Clone, Debug, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
pub struct Notification {
    pub event: NotificationEvent,
    pub state: State,
}

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
            // | NotificationEvent::WindowManager(WindowManagerEvent::TitleUpdate(_, _))
            | NotificationEvent::WindowManager(WindowManagerEvent::Show(_, _)) // | NotificationEvent::WindowManager(WindowManagerEvent::Uncloak(_, _))
    );

    let notification = &serde_json::to_string(&notification)?;
    let mut stale_sockets = vec![];
    let mut sockets = SUBSCRIPTION_SOCKETS.lock();
    let options = SUBSCRIPTION_SOCKET_OPTIONS.lock();

    for (socket, path) in &mut *sockets {
        let apply_state_filter = (*options)
            .get(socket)
            .copied()
            .unwrap_or_default()
            .filter_state_changes;

        if !apply_state_filter || state_has_been_modified || is_override_event {
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
    }

    for socket in stale_sockets {
        tracing::warn!("removing stale subscription: {socket}");
        sockets.remove(&socket);
        let socket_path = DATA_DIR.join(socket);
        if let Err(error) = std::fs::remove_file(&socket_path) {
            tracing::error!(
                "could not remove stale subscriber socket file at {}: {error}",
                socket_path.display()
            )
        }
    }

    Ok(())
}

#[derive(Debug, Clone)]
pub struct CoreFoundationRunLoop(pub CFRetained<CFRunLoop>);
unsafe impl Sync for CoreFoundationRunLoop {}
unsafe impl Send for CoreFoundationRunLoop {}
impl Deref for CoreFoundationRunLoop {
    type Target = CFRunLoop;

    fn deref(&self) -> &Self::Target {
        self.0.as_ref()
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct AccessibilityUiElement(pub CFRetained<AXUIElement>);
unsafe impl Sync for AccessibilityUiElement {}
unsafe impl Send for AccessibilityUiElement {}
impl Deref for AccessibilityUiElement {
    type Target = AXUIElement;

    fn deref(&self) -> &Self::Target {
        self.0.as_ref()
    }
}

impl Default for AccessibilityUiElement {
    fn default() -> Self {
        Self(unsafe { AXUIElement::new_system_wide() })
    }
}

#[derive(Debug, Clone, PartialEq, Default)]
pub struct AccessibilityObserver(pub Option<CFRetained<AXObserver>>);
unsafe impl Sync for AccessibilityObserver {}
unsafe impl Send for AccessibilityObserver {}
impl Deref for AccessibilityObserver {
    type Target = AXObserver;

    fn deref(&self) -> &Self::Target {
        self.0.as_ref().expect("must have an AXObserver")
    }
}

pub fn cf_array_as<T>(array: &CFArray) -> impl Iterator<Item = NonNull<T>> + use<'_, T> {
    let count = CFArray::count(array);
    (0..count).flat_map(move |idx| {
        NonNull::new(unsafe { CFArray::value_at_index(array, idx).cast_mut() })
            .map(|ptr| ptr.cast::<T>())
    })
}

pub fn cf_dictionary_value<T>(dict: &CFDictionary, key: &CFString) -> Option<NonNull<T>> {
    let ptr = unsafe { CFDictionary::value(dict, NonNull::from(key).as_ptr().cast()) };
    NonNull::new(ptr.cast_mut()).map(|ptr| ptr.cast::<T>())
}

#[derive(thiserror::Error, Debug)]
pub enum LibraryError {
    #[error(transparent)]
    Accessibility(#[from] AccessibilityError),
    #[error(transparent)]
    CoreGraphics(#[from] CoreGraphicsError),
    #[error(transparent)]
    Eyre(#[from] eyre::Error),
}

pub fn hidden_frame_bottom_left(screen_frame: CGRect, window_size: CGSize) -> CGRect {
    let visible_sliver: f64 = 1.0;
    let origin_x = screen_frame.origin.x - (window_size.width - visible_sliver);
    let origin_y = screen_frame.origin.y + screen_frame.size.height - visible_sliver;

    CGRect::new(CGPoint::new(origin_x, origin_y), window_size)
}

pub fn hidden_frame_bottom_right(screen_frame: CGRect, window_size: CGSize) -> CGRect {
    let visible_sliver: f64 = 1.0;
    let origin_x = screen_frame.origin.x + screen_frame.size.width - visible_sliver;
    let origin_y = screen_frame.origin.y + screen_frame.size.height - visible_sliver;

    CGRect::new(CGPoint::new(origin_x, origin_y), window_size)
}

/// Where a window is hidden for the given hiding position
pub fn hidden_frame(
    hiding_position: WindowHidingPosition,
    screen_frame: CGRect,
    window_size: CGSize,
) -> CGRect {
    match hiding_position {
        WindowHidingPosition::BottomLeft => hidden_frame_bottom_left(screen_frame, window_size),
        WindowHidingPosition::BottomRight => hidden_frame_bottom_right(screen_frame, window_size),
    }
}

/// How much of a window that has scrolled out of view stays on screen while it slides away
pub const SCROLLING_EDGE_SLIVER: f64 = 5.0;

/// Where a window that has scrolled out of view comes to rest at the edge of the screen
///
/// The window keeps its height and its size, and stays at the same height up the screen, so that it
/// slides out sideways rather than cutting across the screen to the corner it is eventually hidden
/// in. Only a sliver of it stays on screen, which is what stops macOS from moving it back.
pub fn scrolling_edge_sliver(
    screen_frame: CGRect,
    window_frame: CGRect,
    hiding_position: WindowHidingPosition,
) -> CGRect {
    let origin_x = match hiding_position {
        WindowHidingPosition::BottomLeft => {
            screen_frame.origin.x - (window_frame.size.width - SCROLLING_EDGE_SLIVER)
        }
        WindowHidingPosition::BottomRight => {
            screen_frame.origin.x + screen_frame.size.width - SCROLLING_EDGE_SLIVER
        }
    };

    CGRect::new(
        CGPoint::new(origin_x, window_frame.origin.y),
        window_frame.size,
    )
}

/// Whether any display other than the one with `screen_frame` shows part of `frame`
///
/// A window that slides out of view towards a neighbouring display would be seen on it.
pub fn is_visible_on_another_display(frame: CGRect, screen_frame: CGRect) -> bool {
    core_graphics::CoreGraphicsApi::connected_display_ids()
        .unwrap_or_default()
        .into_iter()
        .map(core_graphics::CoreGraphicsApi::display_bounds)
        .filter(|bounds| bounds.origin != screen_frame.origin || bounds.size != screen_frame.size)
        .any(|bounds| core_graphics::CoreGraphicsApi::contains_rect(frame, bounds))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rect(x: f64, y: f64, width: f64, height: f64) -> CGRect {
        CGRect::new(CGPoint::new(x, y), CGSize::new(width, height))
    }

    const SCREEN: fn() -> CGRect = || rect(0.0, 0.0, 3840.0, 2160.0);

    #[test]
    fn the_edge_sliver_of_a_window_that_overhangs_the_left_edge() {
        // Slack in a column that is partly past the left edge: its corner is on no display, but
        // where it rests only depends on the display that it belongs to
        let frame = rect(-570.0, 60.0, 1840.0, 2000.0);
        let edge = scrolling_edge_sliver(SCREEN(), frame, WindowHidingPosition::BottomLeft);

        assert_eq!(edge.origin.x, -1835.0);
        assert_eq!(edge.origin.y, 60.0);
        assert_eq!(edge.size, frame.size);
    }

    #[test]
    fn the_edge_sliver_of_a_window_that_overhangs_the_right_edge() {
        let frame = rect(2500.0, 60.0, 1875.0, 2000.0);
        let edge = scrolling_edge_sliver(SCREEN(), frame, WindowHidingPosition::BottomRight);

        assert_eq!(edge.origin.x, 3835.0);
        assert_eq!(edge.origin.y, 60.0);
        assert_eq!(edge.size, frame.size);
    }

    #[test]
    fn a_window_above_the_screen_rests_at_the_same_height() {
        let frame = rect(-300.0, -100.0, 1275.0, 900.0);
        let edge = scrolling_edge_sliver(SCREEN(), frame, WindowHidingPosition::BottomLeft);

        assert_eq!(edge.origin.x, -1270.0);
        assert_eq!(edge.origin.y, -100.0);
    }

    #[test]
    fn the_edge_sliver_is_found_on_a_display_that_is_not_at_the_origin() {
        let screen = rect(-1920.0, 0.0, 1920.0, 1080.0);
        let frame = rect(-2000.0, 40.0, 800.0, 600.0);

        let left = scrolling_edge_sliver(screen, frame, WindowHidingPosition::BottomLeft);
        assert_eq!(left.origin.x, -1920.0 - 795.0);

        let right = scrolling_edge_sliver(screen, frame, WindowHidingPosition::BottomRight);
        assert_eq!(right.origin.x, -5.0);
    }
}
