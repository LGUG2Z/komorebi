#![warn(clippy::all)]
#![allow(clippy::missing_errors_doc, clippy::use_self, clippy::doc_markdown)]
#![allow(deprecated)] // allow deprecated variants like HidingBehaviour::Hide to be used in derive macros

use std::num::NonZeroUsize;
use std::path::PathBuf;
use std::str::FromStr;

use clap::ValueEnum;
use color_eyre::eyre;
use serde::Deserialize;
use serde::Serialize;
use strum::Display;
use strum::EnumString;

use crate::KomorebiTheme;
use crate::animation::prefix::AnimationPrefix;

// Re-export everything from komorebi-layouts
pub use komorebi_layouts::Arrangement;
pub use komorebi_layouts::Axis;
pub use komorebi_layouts::Column;
pub use komorebi_layouts::ColumnSplit;
pub use komorebi_layouts::ColumnSplitWithCapacity;
pub use komorebi_layouts::ColumnWidth;
pub use komorebi_layouts::CustomLayout;
pub use komorebi_layouts::CycleDirection;
pub use komorebi_layouts::DEFAULT_RATIO;
pub use komorebi_layouts::DEFAULT_SECONDARY_RATIO;
pub use komorebi_layouts::DefaultLayout;
pub use komorebi_layouts::Direction;
pub use komorebi_layouts::GridLayoutOptions;
pub use komorebi_layouts::Layout;
pub use komorebi_layouts::LayoutOptions;
pub use komorebi_layouts::MAX_RATIO;
pub use komorebi_layouts::MAX_RATIOS;
pub use komorebi_layouts::MIN_RATIO;
pub use komorebi_layouts::OperationDirection;
pub use komorebi_layouts::Rect;
pub use komorebi_layouts::ScrollingLayoutOptions;
pub use komorebi_layouts::Sizing;
pub use komorebi_layouts::validate_ratios;

// Local modules and exports
pub use animation::AnimationStyle;
pub use pathext::PathExt;
pub use pathext::ResolvedPathBuf;
pub use pathext::replace_env_in_path;
pub use pathext::resolve_option_hashmap_usize_path;

pub mod animation;
pub mod asc;
pub mod config_generation;
pub mod pathext;

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
    CycleStack(CycleDirection),
    CycleStackIndex(CycleDirection),
    FocusStackWindow(usize),
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
    ForceFocus,
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
    ToggleMonocle,
    ToggleMaximize,
    ToggleWindowContainerBehaviour,
    ToggleFloatOverride,
    WindowHidingBehaviour(HidingBehaviour),
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
    LayoutRatios(Option<Vec<f32>>, Option<Vec<f32>>),
    ScrollingLayoutColumns(NonZeroUsize),
    ChangeLayoutCustom(#[serde_as(as = "ResolvedPathBuf")] PathBuf),
    FlipLayout(Axis),
    ToggleWorkspaceWindowContainerBehaviour,
    ToggleWorkspaceFloatOverride,
    // Monitor and Workspace Commands
    MonitorIndexPreference(usize, i32, i32, i32, i32),
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
    WorkspaceLayoutCustom(usize, usize, #[serde_as(as = "ResolvedPathBuf")] PathBuf),
    NamedWorkspaceLayoutCustom(String, #[serde_as(as = "ResolvedPathBuf")] PathBuf),
    WorkspaceLayoutRule(usize, usize, usize, DefaultLayout),
    NamedWorkspaceLayoutRule(String, usize, DefaultLayout),
    WorkspaceLayoutCustomRule(
        usize,
        usize,
        usize,
        #[serde_as(as = "ResolvedPathBuf")] PathBuf,
    ),
    NamedWorkspaceLayoutCustomRule(String, usize, #[serde_as(as = "ResolvedPathBuf")] PathBuf),
    ClearWorkspaceLayoutRules(usize, usize),
    ClearNamedWorkspaceLayoutRules(String),
    ToggleWorkspaceLayer,
    // Configuration
    ReloadConfiguration,
    ReplaceConfiguration(#[serde_as(as = "ResolvedPathBuf")] PathBuf),
    ReloadStaticConfiguration(#[serde_as(as = "ResolvedPathBuf")] PathBuf),
    WatchConfiguration(bool),
    CompleteConfiguration,
    AltFocusHack(bool),
    Theme(Box<KomorebiTheme>),
    Animation(bool, Option<AnimationPrefix>),
    AnimationDuration(u64, Option<AnimationPrefix>),
    AnimationFps(u64),
    AnimationStyle(AnimationStyle, Option<AnimationPrefix>),
    #[serde(alias = "ActiveWindowBorder")]
    Border(bool),
    #[serde(alias = "ActiveWindowBorderColour")]
    BorderColour(WindowKind, u32, u32, u32),
    #[serde(alias = "ActiveWindowBorderStyle")]
    BorderStyle(BorderStyle),
    BorderWidth(i32),
    BorderOffset(i32),
    BorderImplementation(BorderImplementation),
    Transparency(bool),
    ToggleTransparency,
    TransparencyAlpha(u8),
    InvisibleBorders(Rect),
    StackbarMode(StackbarMode),
    StackbarLabel(StackbarLabel),
    StackbarFocusedTextColour(u32, u32, u32),
    StackbarUnfocusedTextColour(u32, u32, u32),
    StackbarBackgroundColour(u32, u32, u32),
    StackbarHeight(i32),
    StackbarTabWidth(i32),
    StackbarFontSize(i32),
    StackbarFontFamily(Option<String>),
    WorkAreaOffset(Rect),
    MonitorWorkAreaOffset(usize, Rect),
    WorkspaceWorkAreaOffset(usize, usize, Rect),
    ToggleWindowBasedWorkAreaOffset,
    ResizeDelta(i32),
    InitialWorkspaceRule(ApplicationIdentifier, String, usize, usize),
    InitialNamedWorkspaceRule(ApplicationIdentifier, String, String),
    WorkspaceRule(ApplicationIdentifier, String, usize, usize),
    NamedWorkspaceRule(ApplicationIdentifier, String, String),
    ClearWorkspaceRules(usize, usize),
    ClearNamedWorkspaceRules(String),
    ClearAllWorkspaceRules,
    EnforceWorkspaceRules,
    SessionFloatRule,
    SessionFloatRules,
    ClearSessionFloatRules,
    #[serde(alias = "FloatRule")]
    IgnoreRule(ApplicationIdentifier, String),
    ManageRule(ApplicationIdentifier, String),
    IdentifyObjectNameChangeApplication(ApplicationIdentifier, String),
    IdentifyTrayApplication(ApplicationIdentifier, String),
    IdentifyLayeredApplication(ApplicationIdentifier, String),
    IdentifyBorderOverflowApplication(ApplicationIdentifier, String),
    State,
    GlobalState,
    VisibleWindows,
    MonitorInformation,
    Query(StateQuery),
    FocusFollowsMouse(FocusFollowsMouseImplementation, bool),
    ToggleFocusFollowsMouse(FocusFollowsMouseImplementation),
    MouseFollowsFocus(bool),
    ToggleMouseFollowsFocus,
    RemoveTitleBar(ApplicationIdentifier, String),
    ToggleTitleBars,
    AddSubscriberSocket(String),
    AddSubscriberSocketWithOptions(String, SubscribeOptions),
    RemoveSubscriberSocket(String),
    AddSubscriberPipe(String),
    RemoveSubscriberPipe(String),
    ApplicationSpecificConfigurationSchema,
    NotificationSchema,
    SocketSchema,
    StaticConfigSchema,
    GenerateStaticConfig,
    DebugWindow(isize),
}

impl SocketMessage {
    pub fn as_bytes(&self) -> eyre::Result<Vec<u8>> {
        Ok(serde_json::to_string(self)?.as_bytes().to_vec())
    }
}

impl FromStr for SocketMessage {
    type Err = serde_json::Error;

    fn from_str(s: &str) -> eyre::Result<Self, Self::Err> {
        serde_json::from_str(s)
    }
}

#[derive(Default, Debug, Copy, Clone, Eq, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
pub struct SubscribeOptions {
    /// Only emit notifications when the window manager state has changed
    pub filter_state_changes: bool,
}

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
/// Window kind
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
    Copy, Clone, Debug, Eq, PartialEq, Serialize, Deserialize, Display, EnumString, ValueEnum,
)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
/// Application identifier
pub enum ApplicationIdentifier {
    /// Executable name
    #[serde(alias = "exe")]
    Exe,
    /// Class
    #[serde(alias = "class")]
    Class,
    #[serde(alias = "title")]
    /// Window title
    Title,
    /// Executable path
    #[serde(alias = "path")]
    Path,
}

#[derive(Copy, Clone, Debug, PartialEq, Serialize, Deserialize, Display, EnumString, ValueEnum)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
/// Focus follows mouse implementation
pub enum FocusFollowsMouseImplementation {
    /// Custom FFM implementation (slightly more CPU-intensive)
    Komorebi,
    /// Native (legacy) Windows FFM implementation
    Windows,
}

#[derive(Clone, Copy, Debug, Default, Serialize, Deserialize, PartialEq)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
/// Window management behaviour
pub struct WindowManagementBehaviour {
    /// The current [`WindowContainerBehaviour`] to be used
    pub current_behaviour: WindowContainerBehaviour,
    /// Override of `current_behaviour` to open new windows as floating windows
    /// that can be later toggled to tiled, when false it will default to
    /// `current_behaviour` again.
    pub float_override: bool,
    /// Determines if a new window should be spawned floating when on the floating layer and the
    /// floating layer behaviour is set to float. This value is always calculated when checking for
    /// the management behaviour on a specific workspace.
    pub floating_layer_override: bool,
    /// The floating layer behaviour to be used if the float override is being used
    pub floating_layer_behaviour: FloatingLayerBehaviour,
    /// The `Placement` to be used when toggling a window to float
    pub toggle_float_placement: Placement,
    /// The `Placement` to be used when spawning a window on the floating layer with the
    /// `FloatingLayerBehaviour` set to `FloatingLayerBehaviour::Float`
    pub floating_layer_placement: Placement,
    /// The `Placement` to be used when spawning a window with float override active
    pub float_override_placement: Placement,
    /// The `Placement` to be used when spawning a window that matches a `floating_applications` rule
    pub float_rule_placement: Placement,
}

#[derive(
    Clone, Copy, Debug, Default, Serialize, Deserialize, Display, EnumString, ValueEnum, PartialEq,
)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
/// Window container behaviour when a new window is opened
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
/// Floating layer behaviour when a new window is opened
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
/// Placement behaviour for floating windows
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
/// Behaviour when an action would cross a monitor boundary
pub enum CrossBoundaryBehaviour {
    /// Attempt to perform actions across a workspace boundary
    Workspace,
    /// Attempt to perform actions across a monitor boundary
    #[default]
    Monitor,
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

#[derive(
    Clone, Copy, Debug, Default, Serialize, Deserialize, Display, EnumString, ValueEnum, PartialEq,
)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
/// Window handling behaviour
pub enum WindowHandlingBehaviour {
    #[default]
    /// Synchronous
    Sync,
    /// Asynchronous
    Async,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn deserializes() {
        // Set a variable for testing
        unsafe {
            std::env::set_var("VAR", "VALUE");
        }

        let json = r#"{"type":"WorkspaceLayoutCustomRule","content":[0,0,0,"/path/%VAR%/d"]}"#;
        let message: SocketMessage = serde_json::from_str(json).unwrap();

        let SocketMessage::WorkspaceLayoutCustomRule(
            _workspace_index,
            _workspace_number,
            _monitor_index,
            path,
        ) = message
        else {
            panic!("Expected WorkspaceLayoutCustomRule");
        };

        assert_eq!(path, PathBuf::from("/path/VALUE/d"));
    }
}
