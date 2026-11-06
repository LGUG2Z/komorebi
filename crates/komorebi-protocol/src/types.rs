//! The payload types of `SocketMessage`.

use clap::ValueEnum;
use serde::Deserialize;
use serde::Serialize;
use strum::Display;
use strum::EnumString;

/// The options of a subscriber, which `SocketMessage::AddSubscriberSocketWithOptions` sets
///
/// A subscriber added with `SocketMessage::AddSubscriberSocket` has the default options, and is
/// sent a notification for every event and message the window manager handles. With
/// `filter_state_changes`, it is sent one only when the event or the message changed the
/// state, and always when a window is shown, when a subscriber is added (so a new subscriber
/// receives the state as soon as it subscribes), when a theme is set (the colors of the borders
/// are not part of the state) and when the configuration is reloaded.
///
/// # Example
///
/// ```rust
/// use komorebi_protocol::types::SubscribeOptions;
///
/// let options = SubscribeOptions {
///     filter_state_changes: true,
/// };
/// assert_eq!(
///     serde_json::to_string(&options)?,
///     r#"{"filter_state_changes":true}"#,
/// );
///
/// assert!(!SubscribeOptions::default().filter_state_changes);
///
/// # Ok::<(), Box<dyn std::error::Error>>(())
/// ```
#[derive(Default, Debug, Copy, Clone, Eq, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
pub struct SubscribeOptions {
    /// Only emit notifications when the window manager state has changed
    pub filter_state_changes: bool,
}

/// A question about the window manager that `SocketMessage::Query` asks, answered with one
/// value as text: an index, the name of a layout or of a container kind, or the version of the
/// daemon.
#[derive(Copy, Clone, Debug, Serialize, Deserialize, Display, EnumString, ValueEnum)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
pub enum StateQuery {
    FocusedMonitorIndex,
    FocusedWorkspaceIndex,
    FocusedContainerIndex,
    FocusedWindowIndex,
    FocusedWorkspaceName,
    FocusedWorkspaceLayout,
    FocusedContainerKind,
    Version,
}

#[derive(
    Copy,
    Clone,
    Debug,
    Default,
    Serialize,
    Deserialize,
    Display,
    EnumString,
    ValueEnum,
    PartialEq,
    Eq,
    Hash,
)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
/// What a window is to the layout, which decides the color of its border
pub enum WindowKind {
    /// Single window
    Single,
    /// Stack container
    Stack,
    /// Monocle container
    Monocle,
    #[default]
    /// Unfocused window
    Unfocused,
    /// Unfocused locked container
    UnfocusedLocked,
    /// Floating window
    Floating,
}

#[derive(
    Copy, Clone, Debug, Eq, PartialEq, Serialize, Deserialize, Display, EnumString, ValueEnum,
)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
/// Application identifier
pub enum ApplicationIdentifier {
    /// Executable name
    #[serde(alias = "exe")]
    Exe,
    // the description is part of the checked-in schemas, which differ per platform
    #[cfg_attr(target_os = "windows", doc = " Class")]
    #[cfg_attr(
        target_os = "macos",
        doc = " Accessibility subrole or role, such as AXStandardWindow"
    )]
    #[cfg_attr(
        not(any(target_os = "windows", target_os = "macos")),
        doc = " Role or subrole"
    )]
    #[serde(alias = "class")]
    Class,
    #[serde(alias = "title")]
    /// Window title
    Title,
    /// Executable path
    #[serde(alias = "path")]
    Path,
}

#[derive(
    Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize, Display, EnumString, ValueEnum,
)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
/// Move behaviour when the operation works across a monitor boundary
pub enum MoveBehaviour {
    /// Swap the window container with the window container at the edge of the adjacent monitor
    #[default]
    Swap,
    /// Insert the window container into the focused workspace on the adjacent monitor
    Insert,
    /// Do nothing if trying to move a window container in the direction of an adjacent monitor
    NoOp,
}

#[derive(
    Clone, Copy, Debug, Default, Serialize, Deserialize, Display, EnumString, ValueEnum, PartialEq,
)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
/// Behaviour when focusing in a direction while a monocle container is active
pub enum MonocleFocusBehaviour {
    /// Cycle the monocle container to the next/previous container in the workspace
    Cycle,
    /// Do nothing, allowing focus to fall through to cross-monitor logic
    #[default]
    NoOp,
}

#[derive(
    Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize, Display, EnumString, ValueEnum,
)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
/// Operation behaviour for temporarily unmanaged and floating windows
pub enum OperationBehaviour {
    /// Process commands on temporarily unmanaged/floated windows
    #[default]
    Op,
    /// Ignore commands on temporarily unmanaged/floated windows
    NoOp,
}

// Payloads of messages that not every platform handles (see `platforms`): borders, stackbars,
// window hiding and focus follows mouse are Windows features, the last one also exists on KDE.

#[derive(Debug, Copy, Clone, Eq, PartialEq, Display, Serialize, Deserialize, ValueEnum)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
/// Stackbar mode
pub enum StackbarMode {
    /// Always show
    Always,
    /// Never show
    Never,
    /// Show on stack
    OnStack,
}

#[derive(Debug, Copy, Default, Clone, Eq, PartialEq, Display, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
/// Starbar label
pub enum StackbarLabel {
    #[default]
    /// Process name
    Process,
    /// Window title
    Title,
}

#[derive(
    Default, Copy, Clone, Debug, Eq, PartialEq, Display, Serialize, Deserialize, ValueEnum,
)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
/// Border style
pub enum BorderStyle {
    #[default]
    /// Use the system border style
    System,
    /// Use the Windows 11-style rounded borders
    Rounded,
    /// Use the Windows 10-style square borders
    Square,
}

#[derive(
    Default, Copy, Clone, Debug, Eq, PartialEq, Display, Serialize, Deserialize, ValueEnum,
)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
/// Border style
pub enum BorderImplementation {
    #[default]
    /// Use the adjustable komorebi border implementation
    Komorebi,
    /// Use the thin Windows accent border implementation
    Windows,
}

#[derive(Copy, Clone, Debug, PartialEq, Serialize, Deserialize, Display, EnumString, ValueEnum)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
/// Focus follows mouse implementation
// KDE's compositor does the focusing: its description says so (it is part of the checked-in schema)
#[cfg_attr(target_os = "linux", doc = "")]
#[cfg_attr(
    target_os = "linux",
    doc = " The two exist on Windows. On KDE the compositor does it, so both ask KWin for its"
)]
#[cfg_attr(target_os = "linux", doc = " \"focus follows mouse\" focus policy.")]
pub enum FocusFollowsMouseImplementation {
    /// Custom FFM implementation (slightly more CPU-intensive)
    Komorebi,
    /// Native (legacy) Windows FFM implementation
    Windows,
}

#[derive(Copy, Clone, Debug, Serialize, Deserialize, Display, EnumString, ValueEnum, PartialEq)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
/// Window hiding behaviour
pub enum HidingBehaviour {
    /// END OF LIFE FEATURE: Use the `SW_HIDE` flag to hide windows when switching workspaces (has issues with Electron apps)
    #[deprecated(note = "End of life feature")]
    Hide,
    /// Use the `SW_MINIMIZE` flag to hide windows when switching workspaces (has issues with frequent workspace switching)
    Minimize,
    /// Use the undocumented SetCloak Win32 function to hide windows when switching workspaces
    Cloak,
}
