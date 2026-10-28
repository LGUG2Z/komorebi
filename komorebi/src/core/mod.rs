//! The vocabulary of the window manager: the enums and small structs that configuration,
//! commands and the layout code are written in.
//!
//! The layout types ([`DefaultLayout`], [`Rect`], [`OperationDirection`] and the rest) are
//! those of `komorebi-layouts`, re-exported here so that the rest of the crate has one place to
//! import from. [`pathext`] resolves environment variables in configured paths and
//! [`config_generation`] holds the rules that match applications by what they are called.
//!
//! [`SocketMessage`] is every message the daemon takes on its socket, [`StateQuery`] every
//! question [`SocketMessage::Query`] can ask, and [`SubscribeOptions`] what a subscriber can ask
//! of its notifications. A message that goes somewhere takes the directions of the layouts: an
//! [`OperationDirection`] is a side of the focused tile, and a [`CycleDirection`] the next or the
//! previous container in the order of the workspace.

use crate::core::pathext::ResolvedPathBuf;
use clap::ValueEnum;
use color_eyre::eyre;
use serde::Deserialize;
use serde::Serialize;
use std::num::NonZeroUsize;
use std::path::PathBuf;
use std::str::FromStr;
use strum::Display;
use strum::EnumString;

// Re-export types from komorebi-layouts
pub use komorebi_layouts::Arrangement;
pub use komorebi_layouts::Axis;
pub use komorebi_layouts::CycleDirection;
pub use komorebi_layouts::DEFAULT_RATIO;
pub use komorebi_layouts::DEFAULT_SECONDARY_RATIO;
pub use komorebi_layouts::DefaultLayout;
pub use komorebi_layouts::Direction;
pub use komorebi_layouts::GridLayoutOptions;
pub use komorebi_layouts::Layout;
pub use komorebi_layouts::LayoutDefaultEntry;
pub use komorebi_layouts::LayoutOptions;
pub use komorebi_layouts::MAX_RATIO;
pub use komorebi_layouts::MAX_RATIOS;
pub use komorebi_layouts::MIN_RATIO;
pub use komorebi_layouts::OperationDirection;
pub use komorebi_layouts::Rect;
pub use komorebi_layouts::ScrollingLayoutOptions;
pub use komorebi_layouts::Sizing;
pub use komorebi_layouts::validate_ratios;

// Keep local modules that aren't in komorebi-layouts
pub mod config_generation;
pub mod pathext;

#[derive(
    Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize, Display, EnumString, ValueEnum,
)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
/// Operation behavior for temporarily unmanaged and floating windows
pub enum OperationBehaviour {
    /// Process commands on temporarily unmanaged/floated windows
    #[default]
    Op,
    /// Ignore commands on temporarily unmanaged/floated windows
    NoOp,
}

#[derive(Clone, Copy, Debug, Default, Serialize, Deserialize, PartialEq)]
pub struct WindowManagementBehaviour {
    /// The current WindowContainerBehaviour to be used
    pub current_behaviour: WindowContainerBehaviour,
    /// Override of `current_behaviour` to open new windows as floating windows
    /// that can be later toggled to tiled, when false it will default to
    /// `current_behaviour` again.
    pub float_override: bool,
    /// Determines if a new window should be spawned floating when on the floating layer and the
    /// floating layer behavior is set to float. This value is always calculated when checking for
    /// the management behavior on a specific workspace.
    pub floating_layer_override: bool,
    /// The floating layer behavior to be used if the float override is being used
    pub floating_layer_behaviour: FloatingLayerBehaviour,
    /// The `Placement` to be used when toggling a window to float
    pub toggle_float_placement: Placement,
    /// The `Placement` to be used when spawning a window on the floating layer with the
    /// `FloatingLayerBehaviour` set to `FloatingLayerBehaviour::Float`
    pub floating_layer_placement: Placement,
    /// The `Placement` to be used when spawning a window with float override active
    pub float_override_placement: Placement,
    /// The `Placement` to be used when spawning a window that matches a 'floating_applications' rule
    pub float_rule_placement: Placement,
}

#[derive(
    Clone, Copy, Debug, Default, Serialize, Deserialize, Display, EnumString, ValueEnum, PartialEq,
)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
/// Window container behavior when a new window is opened
pub enum WindowContainerBehaviour {
    /// Create a new container for each new window
    #[default]
    Create,
    /// Append new windows to the focused window container
    Append,
}

#[derive(
    Clone, Copy, Debug, Default, Serialize, Deserialize, Display, EnumString, ValueEnum, PartialEq,
)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
/// Floating layer behavior when a new window is opened
pub enum FloatingLayerBehaviour {
    /// Tile new windows (unless they match a float rule or float override is active)
    #[default]
    Tile,
    /// Float new windows
    Float,
}

#[derive(
    Clone, Copy, Debug, Default, Serialize, Deserialize, Display, EnumString, ValueEnum, PartialEq,
)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
/// Window hiding position
pub enum WindowHidingPosition {
    /// Hide windows in the bottom left corner of the screen
    #[default]
    BottomLeft,
    /// Hide windows in the bottom right corner of the screen
    BottomRight,
}

#[derive(
    Clone, Copy, Debug, Default, Serialize, Deserialize, Display, EnumString, ValueEnum, PartialEq,
)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
/// Placement behavior for floating windows
pub enum Placement {
    /// Does not change the size or position of the window
    #[default]
    None,
    /// Center the window without changing the size
    Center,
    /// Center the window and resize it according to the `AspectRatio`
    CenterAndResize,
}

impl FloatingLayerBehaviour {
    pub fn should_float(&self) -> bool {
        match self {
            FloatingLayerBehaviour::Tile => false,
            FloatingLayerBehaviour::Float => true,
        }
    }
}

impl Placement {
    pub fn should_center(&self) -> bool {
        match self {
            Placement::None => false,
            Placement::Center | Placement::CenterAndResize => true,
        }
    }

    pub fn should_resize(&self) -> bool {
        match self {
            Placement::None | Placement::Center => false,
            Placement::CenterAndResize => true,
        }
    }
}

#[derive(Copy, Clone, Debug, PartialEq, Serialize, Deserialize, Display, EnumString, ValueEnum)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
/// Focus follows mouse implementation
///
/// The two exist on Windows. On KDE the compositor does it, so both ask KWin for its
/// "focus follows mouse" focus policy.
pub enum FocusFollowsMouseImplementation {
    /// Custom FFM implementation (slightly more CPU-intensive)
    Komorebi,
    /// Native (legacy) Windows FFM implementation
    Windows,
}

#[derive(
    Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize, Display, EnumString, ValueEnum,
)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
/// Move behavior when the operation works across a monitor boundary
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
/// Behavior when an action would cross a monitor boundary
pub enum CrossBoundaryBehaviour {
    /// Attempt to perform actions across a workspace boundary
    Workspace,
    /// Attempt to perform actions across a monitor boundary
    #[default]
    Monitor,
}

#[derive(
    Clone, Copy, Debug, Default, Serialize, Deserialize, Display, EnumString, ValueEnum, PartialEq,
)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
/// Behavior when focusing in a direction while a monocle container is active
pub enum MonocleFocusBehaviour {
    /// Cycle the monocle container to the next/previous container in the workspace
    Cycle,
    /// Do nothing, allowing focus to fall through to cross-monitor logic
    #[default]
    NoOp,
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
    /// Role or subrole
    #[serde(alias = "class")]
    Class,
    /// Window title
    #[serde(alias = "title")]
    Title,
    /// Executable path
    #[serde(alias = "path")]
    Path,
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
pub enum WindowKind {
    Single,
    Stack,
    Monocle,
    #[default]
    Unfocused,
    UnfocusedLocked,
    Floating,
}

/// A question about the window manager that [`SocketMessage::Query`] asks, answered with one
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

/// The options of a subscriber, which [`SocketMessage::AddSubscriberSocketWithOptions`] sets
///
/// A subscriber added with [`SocketMessage::AddSubscriberSocket`] has the default options, and is
/// sent a notification for every event and message the window manager handles. With
/// `filter_state_changes`, it is sent one only when the event or the message changed the
/// [`State`](crate::state::State), and always when a window is shown and when a subscriber is
/// added, so a new subscriber receives the state as soon as it subscribes.
///
/// # Example
///
/// ```rust
/// use komorebi::core::SocketMessage;
/// use komorebi::core::SubscribeOptions;
///
/// let options = SubscribeOptions {
///     filter_state_changes: true,
/// };
/// let message = SocketMessage::AddSubscriberSocketWithOptions(String::from("bar.sock"), options);
/// assert_eq!(
///     String::from_utf8(message.as_bytes()?)?,
///     r#"{"type":"AddSubscriberSocketWithOptions","content":["bar.sock",{"filter_state_changes":true}]}"#,
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

/// A message to the daemon, as it is written on its socket: JSON, with the variant's name in
/// `type` and its arguments, if it has any, in `content`.
///
/// # Example
///
/// ```rust
/// use komorebi::core::SocketMessage;
/// use komorebi::core::StateQuery;
///
/// let query = SocketMessage::Query(StateQuery::FocusedWorkspaceIndex);
/// assert_eq!(
///     String::from_utf8(query.as_bytes()?)?,
///     r#"{"type":"Query","content":"FocusedWorkspaceIndex"}"#,
/// );
///
/// let stop: SocketMessage = r#"{"type":"Stop"}"#.parse()?;
/// assert!(matches!(stop, SocketMessage::Stop));
///
/// # Ok::<(), Box<dyn std::error::Error>>(())
/// ```
// serde_as must be before derive
#[serde_with::serde_as]
#[derive(Clone, Debug, Serialize, Deserialize, Display)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[serde(tag = "type", content = "content")]
pub enum SocketMessage {
    // Window / Container Commands
    FocusWindow(OperationDirection),
    MoveWindow(OperationDirection),
    PreselectDirection(OperationDirection),
    CancelPreselect,
    CycleFocusWindow(CycleDirection),
    CycleMoveWindow(CycleDirection),
    StackWindow(OperationDirection),
    UnstackWindow,
    StackAll,
    UnstackAll,
    ResizeWindowEdge(OperationDirection, Sizing),
    ResizeWindowAxis(Axis, Sizing),
    MoveContainerToLastWorkspace,
    SendContainerToLastWorkspace,
    MoveContainerToMonitorNumber(usize),
    CycleMoveContainerToMonitor(CycleDirection),
    MoveContainerToWorkspaceNumber(usize),
    MoveContainerToNamedWorkspace(String),
    CycleMoveContainerToWorkspace(CycleDirection),
    SendContainerToMonitorNumber(usize),
    CycleSendContainerToMonitor(CycleDirection),
    SendContainerToWorkspaceNumber(usize),
    CycleSendContainerToWorkspace(CycleDirection),
    SendContainerToMonitorWorkspaceNumber(usize, usize),
    MoveContainerToMonitorWorkspaceNumber(usize, usize),
    SendContainerToNamedWorkspace(String),
    CycleMoveWorkspaceToMonitor(CycleDirection),
    MoveWorkspaceToMonitorNumber(usize),
    SwapWorkspacesToMonitorNumber(usize),
    // ForceFocus,
    Close,
    Minimize,
    Promote,
    PromoteSwap,
    PromoteFocus,
    PromoteWindow(OperationDirection),
    EagerFocus(String),
    LockMonitorWorkspaceContainer(usize, usize, usize),
    UnlockMonitorWorkspaceContainer(usize, usize, usize),
    ToggleLock,
    ToggleFloat,
    ToggleWindowContainerBehaviour,
    ToggleFloatOverride,
    ToggleCrossMonitorMoveBehaviour,
    CrossMonitorMoveBehaviour(MoveBehaviour),
    UnmanagedWindowOperationBehaviour(OperationBehaviour),
    // Current Workspace Commands
    ManageFocusedWindow,
    UnmanageFocusedWindow,
    AdjustContainerPadding(Sizing, i32),
    AdjustWorkspacePadding(Sizing, i32),
    ChangeLayout(DefaultLayout),
    CycleLayout(CycleDirection),
    ScrollingLayoutColumns(NonZeroUsize),
    LayoutRatios(Option<Vec<f32>>, Option<Vec<f32>>),
    // ChangeLayoutCustom(#[serde_as(as = "ResolvedPathBuf")] PathBuf),
    FlipLayout(Axis),
    ToggleWorkspaceWindowContainerBehaviour,
    ToggleWorkspaceFloatOverride,
    // Monitor and Workspace Commands
    // MonitorIndexPreference(usize, i32, i32, i32, i32),
    DisplayIndexPreference(usize, String),
    EnsureWorkspaces(usize, usize),
    EnsureNamedWorkspaces(usize, Vec<String>),
    NewWorkspace,
    ToggleTiling,
    Stop,
    StopIgnoreRestore,
    TogglePause,
    Retile,
    RetileWithResizeDimensions,
    QuickSave,
    QuickLoad,
    Save(#[serde_as(as = "ResolvedPathBuf")] PathBuf),
    Load(#[serde_as(as = "ResolvedPathBuf")] PathBuf),
    CycleFocusMonitor(CycleDirection),
    CycleFocusWorkspace(CycleDirection),
    CycleFocusEmptyWorkspace(CycleDirection),
    FocusMonitorNumber(usize),
    FocusMonitorAtCursor,
    FocusLastWorkspace,
    CloseWorkspace,
    FocusWorkspaceNumber(usize),
    FocusWorkspaceNumbers(usize),
    FocusMonitorWorkspaceNumber(usize, usize),
    FocusNamedWorkspace(String),
    ContainerPadding(usize, usize, i32),
    NamedWorkspaceContainerPadding(String, i32),
    FocusedWorkspaceContainerPadding(i32),
    WorkspacePadding(usize, usize, i32),
    NamedWorkspacePadding(String, i32),
    FocusedWorkspacePadding(i32),
    WorkspaceTiling(usize, usize, bool),
    NamedWorkspaceTiling(String, bool),
    WorkspaceName(usize, usize, String),
    WorkspaceLayout(usize, usize, DefaultLayout),
    NamedWorkspaceLayout(String, DefaultLayout),
    // WorkspaceLayoutCustom(usize, usize, #[serde_as(as = "ResolvedPathBuf")] PathBuf),
    // NamedWorkspaceLayoutCustom(String, #[serde_as(as = "ResolvedPathBuf")] PathBuf),
    WorkAreaOffset(Rect),
    MonitorWorkAreaOffset(usize, Rect),
    WorkspaceWorkAreaOffset(usize, usize, Rect),
    ToggleWindowBasedWorkAreaOffset,
    ResizeDelta(i32),
    SessionFloatRule,
    SessionFloatRules,
    ClearSessionFloatRules,
    #[serde(alias = "FloatRule")]
    IgnoreRule(ApplicationIdentifier, String),
    ManageRule(ApplicationIdentifier, String),
    // IdentifyObjectNameChangeApplication(ApplicationIdentifier, String),
    // IdentifyTrayApplication(ApplicationIdentifier, String),
    // IdentifyLayeredApplication(ApplicationIdentifier, String),
    // IdentifyBorderOverflowApplication(ApplicationIdentifier, String),
    State,
    GlobalState,
    MonitorInformation,
    Query(StateQuery),
    MouseFollowsFocus(bool),
    ToggleMouseFollowsFocus,
    // RemoveTitleBar(ApplicationIdentifier, String),
    // ToggleTitleBars,
    AddSubscriberSocket(String),
    AddSubscriberSocketWithOptions(String, SubscribeOptions),
    RemoveSubscriberSocket(String),
    // AddSubscriberPipe(String),
    // RemoveSubscriberPipe(String),
    DebugWindow(u32),
}

impl SocketMessage {
    /// Returns the message as JSON, the way the daemon reads it from its socket
    pub fn as_bytes(&self) -> eyre::Result<Vec<u8>> {
        Ok(serde_json::to_string(self)?.as_bytes().to_vec())
    }
}

impl FromStr for SocketMessage {
    type Err = serde_json::Error;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        serde_json::from_str(s)
    }
}
