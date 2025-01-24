#![warn(clippy::all)]
#![allow(clippy::missing_errors_doc, clippy::use_self, clippy::doc_markdown)]

use std::path::Path;
use std::path::PathBuf;
use std::str::FromStr;

use clap::ValueEnum;
use color_eyre::eyre::anyhow;
use color_eyre::Result;
use schemars::JsonSchema;
use serde::Deserialize;
use serde::Serialize;
use strum::Display;
use strum::EnumString;

use crate::animation::prefix::AnimationPrefix;
use crate::KomorebiTheme;
pub use animation::AnimationStyle;
pub use arrangement::Arrangement;
pub use arrangement::Axis;
pub use custom_layout::CustomLayout;
pub use cycle_direction::CycleDirection;
pub use default_layout::DefaultLayout;
pub use direction::Direction;
pub use layout::Layout;
pub use operation_direction::OperationDirection;
pub use pathext::PathExt;
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

#[derive(Clone, Debug, Serialize, Deserialize, Display, JsonSchema)]
#[serde(tag = "type", content = "content")]
pub enum SocketMessage {
    // Window / Container Commands
    FocusWindow(OperationDirection),
    MoveWindow(OperationDirection),
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
    PromoteFocus,
    PromoteWindow(OperationDirection),
    EagerFocus(String),
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
    ChangeLayoutCustom(PathBuf),
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
    Save(PathBuf),
    Load(PathBuf),
    CycleFocusMonitor(CycleDirection),
    CycleFocusWorkspace(CycleDirection),
    FocusMonitorNumber(usize),
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
    WorkspaceLayoutCustom(usize, usize, PathBuf),
    NamedWorkspaceLayoutCustom(String, PathBuf),
    WorkspaceLayoutRule(usize, usize, usize, DefaultLayout),
    NamedWorkspaceLayoutRule(String, usize, DefaultLayout),
    WorkspaceLayoutCustomRule(usize, usize, usize, PathBuf),
    NamedWorkspaceLayoutCustomRule(String, usize, PathBuf),
    ClearWorkspaceLayoutRules(usize, usize),
    ClearNamedWorkspaceLayoutRules(String),
    // Configuration
    ReloadConfiguration,
    ReplaceConfiguration(PathBuf),
    ReloadStaticConfiguration(PathBuf),
    WatchConfiguration(bool),
    CompleteConfiguration,
    AltFocusHack(bool),
    Theme(KomorebiTheme),
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
    ResizeDelta(i32),
    InitialWorkspaceRule(ApplicationIdentifier, String, usize, usize),
    InitialNamedWorkspaceRule(ApplicationIdentifier, String, String),
    WorkspaceRule(ApplicationIdentifier, String, usize, usize),
    NamedWorkspaceRule(ApplicationIdentifier, String, String),
    ClearWorkspaceRules(usize, usize),
    ClearNamedWorkspaceRules(String),
    ClearAllWorkspaceRules,
    EnforceWorkspaceRules,
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
    pub fn as_bytes(&self) -> Result<Vec<u8>> {
        Ok(serde_json::to_string(self)?.as_bytes().to_vec())
    }
}

impl FromStr for SocketMessage {
    type Err = serde_json::Error;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        serde_json::from_str(s)
    }
}

#[derive(Default, Debug, Copy, Clone, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct SubscribeOptions {
    /// Only emit notifications when the window manager state has changed
    pub filter_state_changes: bool,
}

#[derive(
    Debug, Copy, Clone, Eq, PartialEq, Display, Serialize, Deserialize, JsonSchema, ValueEnum,
)]
pub enum StackbarMode {
    Always,
    Never,
    OnStack,
}

#[derive(
    Debug, Copy, Default, Clone, Eq, PartialEq, Display, Serialize, Deserialize, JsonSchema,
)]
pub enum StackbarLabel {
    #[default]
    Process,
    Title,
}

#[derive(
    Default,
    Copy,
    Clone,
    Debug,
    Eq,
    PartialEq,
    Display,
    Serialize,
    Deserialize,
    JsonSchema,
    ValueEnum,
)]
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
    Default,
    Copy,
    Clone,
    Debug,
    Eq,
    PartialEq,
    Display,
    Serialize,
    Deserialize,
    JsonSchema,
    ValueEnum,
)]
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
    Serialize,
    Deserialize,
    Display,
    EnumString,
    ValueEnum,
    JsonSchema,
    PartialEq,
    Eq,
    Hash,
)]
pub enum WindowKind {
    Single,
    Stack,
    Monocle,
    Unfocused,
    Floating,
}

#[derive(
    Copy, Clone, Debug, Serialize, Deserialize, Display, EnumString, ValueEnum, JsonSchema,
)]
pub enum StateQuery {
    FocusedMonitorIndex,
    FocusedWorkspaceIndex,
    FocusedContainerIndex,
    FocusedWindowIndex,
    FocusedWorkspaceName,
}

#[derive(
    Copy,
    Clone,
    Debug,
    Eq,
    PartialEq,
    Serialize,
    Deserialize,
    Display,
    EnumString,
    ValueEnum,
    JsonSchema,
)]
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

#[derive(
    Copy,
    Clone,
    Debug,
    PartialEq,
    Serialize,
    Deserialize,
    Display,
    EnumString,
    ValueEnum,
    JsonSchema,
)]
pub enum FocusFollowsMouseImplementation {
    /// A custom FFM implementation (slightly more CPU-intensive)
    Komorebi,
    /// The native (legacy) Windows FFM implementation
    Windows,
}

#[derive(Clone, Copy, Debug, Default, Serialize, Deserialize, JsonSchema, PartialEq)]
pub struct WindowManagementBehaviour {
    /// The current WindowContainerBehaviour to be used
    pub current_behaviour: WindowContainerBehaviour,
    /// Override of `current_behaviour` to open new windows as floating windows
    /// that can be later toggled to tiled, when false it will default to
    /// `current_behaviour` again.
    pub float_override: bool,
}

#[derive(
    Clone,
    Copy,
    Debug,
    Default,
    Serialize,
    Deserialize,
    Display,
    EnumString,
    ValueEnum,
    JsonSchema,
    PartialEq,
)]
pub enum WindowContainerBehaviour {
    /// Create a new container for each new window
    #[default]
    Create,
    /// Append new windows to the focused window container
    Append,
}

#[derive(
    Clone,
    Copy,
    Debug,
    PartialEq,
    Serialize,
    Deserialize,
    Display,
    EnumString,
    ValueEnum,
    JsonSchema,
)]
pub enum MoveBehaviour {
    /// Swap the window container with the window container at the edge of the adjacent monitor
    Swap,
    /// Insert the window container into the focused workspace on the adjacent monitor
    Insert,
    /// Do nothing if trying to move a window container in the direction of an adjacent monitor
    NoOp,
}

#[derive(
    Clone, Copy, Debug, Serialize, Deserialize, Display, EnumString, ValueEnum, JsonSchema,
)]
pub enum CrossBoundaryBehaviour {
    /// Attempt to perform actions across a workspace boundary
    Workspace,
    /// Attempt to perform actions across a monitor boundary
    Monitor,
}

#[derive(
    Copy, Clone, Debug, Serialize, Deserialize, Display, EnumString, ValueEnum, JsonSchema,
)]
pub enum HidingBehaviour {
    /// Use the SW_HIDE flag to hide windows when switching workspaces (has issues with Electron apps)
    Hide,
    /// Use the SW_MINIMIZE flag to hide windows when switching workspaces (has issues with frequent workspace switching)
    Minimize,
    /// Use the undocumented SetCloak Win32 function to hide windows when switching workspaces
    Cloak,
}

#[derive(
    Clone,
    Copy,
    Debug,
    PartialEq,
    Serialize,
    Deserialize,
    Display,
    EnumString,
    ValueEnum,
    JsonSchema,
)]
pub enum OperationBehaviour {
    /// Process komorebic commands on temporarily unmanaged/floated windows
    Op,
    /// Ignore komorebic commands on temporarily unmanaged/floated windows
    NoOp,
}

#[derive(
    Clone, Copy, Debug, Serialize, Deserialize, Display, EnumString, ValueEnum, JsonSchema,
)]
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

pub fn resolve_home_path<P: AsRef<Path>>(path: P) -> Result<PathBuf> {
    let mut resolved_path = PathBuf::new();
    let mut resolved = false;
    for c in path.as_ref().components() {
        match c {
            std::path::Component::Normal(c)
                if (c == "~" || c == "$Env:USERPROFILE" || c == "$HOME") && !resolved =>
            {
                let home = dirs::home_dir().ok_or_else(|| anyhow!("there is no home directory"))?;

                resolved_path.extend(home.components());
                resolved = true;
            }

            std::path::Component::Normal(c) if (c == "$Env:KOMOREBI_CONFIG_HOME") && !resolved => {
                let komorebi_config_home =
                    PathBuf::from(std::env::var("KOMOREBI_CONFIG_HOME").ok().ok_or_else(|| {
                        anyhow!("there is no KOMOREBI_CONFIG_HOME environment variable set")
                    })?);

                resolved_path.extend(komorebi_config_home.components());
                resolved = true;
            }

            _ => resolved_path.push(c),
        }
    }

    let parent = resolved_path
        .parent()
        .ok_or_else(|| anyhow!("cannot parse parent directory"))?;

    Ok(if parent.is_dir() {
        let file = resolved_path
            .components()
            .last()
            .ok_or_else(|| anyhow!("cannot parse filename"))?;
        dunce::canonicalize(parent)?.join(file)
    } else {
        resolved_path
    })
}
