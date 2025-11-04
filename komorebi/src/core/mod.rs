#![warn(clippy::all)]
#![allow(clippy::missing_errors_doc, clippy::use_self, clippy::doc_markdown)]

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
pub use animation::AnimationStyle;
pub use arrangement::Arrangement;
pub use arrangement::Axis;
pub use custom_layout::Column;
pub use custom_layout::ColumnSplit;
pub use custom_layout::ColumnSplitWithCapacity;
pub use custom_layout::ColumnWidth;
pub use custom_layout::CustomLayout;
pub use cycle_direction::CycleDirection;
pub use default_layout::DefaultLayout;
pub use direction::Direction;
pub use layout::Layout;
pub use operation_direction::OperationDirection;
pub use pathext::PathExt;
pub use pathext::ResolvedPathBuf;
pub use pathext::replace_env_in_path;
pub use pathext::resolve_option_hashmap_usize_path;
pub use rect::Rect;

pub mod animation;
pub mod arrangement;
pub mod asc;
pub mod config_generation;
pub mod custom_layout;
pub mod cycle_direction;
pub mod default_layout;
pub mod direction;
pub mod layout;
pub mod operation_direction;
pub mod pathext;
pub mod rect;

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
pub enum StackbarMode {
    Always,
    Never,
    OnStack,
}

#[derive(Debug, Copy, Default, Clone, Eq, PartialEq, Display, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
pub enum StackbarLabel {
    #[default]
    Process,
    Title,
}

#[derive(
    Default, Copy, Clone, Debug, Eq, PartialEq, Display, Serialize, Deserialize, ValueEnum,
)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
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
pub enum WindowKind {
    Single,
    Stack,
    Monocle,
    #[default]
    Unfocused,
    UnfocusedLocked,
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
pub enum ApplicationIdentifier {
    #[serde(alias = "exe")]
    Exe,
    #[serde(alias = "class")]
    Class,
    #[serde(alias = "title")]
    Title,
    #[serde(alias = "path")]
    Path,
}

#[derive(Copy, Clone, Debug, PartialEq, Serialize, Deserialize, Display, EnumString, ValueEnum)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
pub enum FocusFollowsMouseImplementation {
    /// A custom FFM implementation (slightly more CPU-intensive)
    Komorebi,
    /// The native (legacy) Windows FFM implementation
    Windows,
}

#[derive(Clone, Copy, Debug, Default, Serialize, Deserialize, PartialEq)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
pub struct WindowManagementBehaviour {
    /// The current WindowContainerBehaviour to be used
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
    /// The `Placement` to be used when spawning a window that matches a 'floating_applications' rule
    pub float_rule_placement: Placement,
}

#[derive(
    Clone, Copy, Debug, Default, Serialize, Deserialize, Display, EnumString, ValueEnum, PartialEq,
)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
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
pub enum CrossBoundaryBehaviour {
    /// Attempt to perform actions across a workspace boundary
    Workspace,
    /// Attempt to perform actions across a monitor boundary
    #[default]
    Monitor,
}

#[derive(Copy, Clone, Debug, Serialize, Deserialize, Display, EnumString, ValueEnum, PartialEq)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
pub enum HidingBehaviour {
    /// END OF LIFE FEATURE: Use the SW_HIDE flag to hide windows when switching workspaces (has issues with Electron apps)
    Hide,
    /// Use the SW_MINIMIZE flag to hide windows when switching workspaces (has issues with frequent workspace switching)
    Minimize,
    /// Use the undocumented SetCloak Win32 function to hide windows when switching workspaces
    Cloak,
}

#[derive(
    Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize, Display, EnumString, ValueEnum,
)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
pub enum OperationBehaviour {
    /// Process komorebic commands on temporarily unmanaged/floated windows
    #[default]
    Op,
    /// Ignore komorebic commands on temporarily unmanaged/floated windows
    NoOp,
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize, Display, EnumString, ValueEnum)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
pub enum Sizing {
    Increase,
    Decrease,
}

impl Sizing {
    #[must_use]
    pub const fn adjust_by(&self, value: i32, adjustment: i32) -> i32 {
        match self {
            Self::Increase => value + adjustment,
            Self::Decrease => {
                if value > 0 && value - adjustment >= 0 {
                    value - adjustment
                } else {
                    value
                }
            }
        }
    }
}

#[derive(
    Clone, Copy, Debug, Default, Serialize, Deserialize, Display, EnumString, ValueEnum, PartialEq,
)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
pub enum WindowHandlingBehaviour {
    #[default]
    Sync,
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
