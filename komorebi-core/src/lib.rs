#![warn(clippy::all, clippy::nursery, clippy::pedantic)]
#![allow(clippy::missing_errors_doc, clippy::use_self)]

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

pub use arrangement::Arrangement;
pub use arrangement::Axis;
pub use custom_layout::CustomLayout;
pub use cycle_direction::CycleDirection;
pub use default_layout::DefaultLayout;
pub use direction::Direction;
pub use layout::Layout;
pub use operation_direction::OperationDirection;
pub use rect::Rect;

pub mod arrangement;
pub mod config_generation;
pub mod custom_layout;
pub mod cycle_direction;
pub mod default_layout;
pub mod direction;
pub mod layout;
pub mod operation_direction;
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
    ResizeWindowEdge(OperationDirection, Sizing),
    ResizeWindowAxis(Axis, Sizing),
    UnstackWindow,
    CycleStack(CycleDirection),
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
    SendContainerToNamedWorkspace(String),
    MoveWorkspaceToMonitorNumber(usize),
    SwapWorkspacesToMonitorNumber(usize),
    ForceFocus,
    Close,
    Minimize,
    Promote,
    PromoteFocus,
    ToggleFloat,
    ToggleMonocle,
    ToggleMaximize,
    ToggleWindowContainerBehaviour,
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
    // Monitor and Workspace Commands
    MonitorIndexPreference(usize, i32, i32, i32, i32),
    DisplayIndexPreference(usize, String),
    EnsureWorkspaces(usize, usize),
    EnsureNamedWorkspaces(usize, Vec<String>),
    NewWorkspace,
    ToggleTiling,
    Stop,
    TogglePause,
    Retile,
    QuickSave,
    QuickLoad,
    Save(PathBuf),
    Load(PathBuf),
    CycleFocusMonitor(CycleDirection),
    CycleFocusWorkspace(CycleDirection),
    FocusMonitorNumber(usize),
    FocusLastWorkspace,
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
    ReloadStaticConfiguration(PathBuf),
    WatchConfiguration(bool),
    CompleteConfiguration,
    AltFocusHack(bool),
    ActiveWindowBorder(bool),
    ActiveWindowBorderColour(WindowKind, u32, u32, u32),
    ActiveWindowBorderWidth(i32),
    ActiveWindowBorderOffset(i32),
    InvisibleBorders(Rect),
    WorkAreaOffset(Rect),
    MonitorWorkAreaOffset(usize, Rect),
    ResizeDelta(i32),
    InitialWorkspaceRule(ApplicationIdentifier, String, usize, usize),
    InitialNamedWorkspaceRule(ApplicationIdentifier, String, String),
    WorkspaceRule(ApplicationIdentifier, String, usize, usize),
    NamedWorkspaceRule(ApplicationIdentifier, String, String),
    FloatRule(ApplicationIdentifier, String),
    ManageRule(ApplicationIdentifier, String),
    IdentifyObjectNameChangeApplication(ApplicationIdentifier, String),
    IdentifyTrayApplication(ApplicationIdentifier, String),
    IdentifyLayeredApplication(ApplicationIdentifier, String),
    IdentifyBorderOverflowApplication(ApplicationIdentifier, String),
    State,
    VisibleWindows,
    Query(StateQuery),
    FocusFollowsMouse(FocusFollowsMouseImplementation, bool),
    ToggleFocusFollowsMouse(FocusFollowsMouseImplementation),
    MouseFollowsFocus(bool),
    ToggleMouseFollowsFocus,
    RemoveTitleBar(ApplicationIdentifier, String),
    ToggleTitleBars,
    AddSubscriber(String),
    RemoveSubscriber(String),
    ApplicationSpecificConfigurationSchema,
    NotificationSchema,
    SocketSchema,
    StaticConfigSchema,
    GenerateStaticConfig,
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

#[derive(
    Copy, Clone, Debug, Serialize, Deserialize, Display, EnumString, ValueEnum, JsonSchema,
)]
#[strum(serialize_all = "snake_case")]
pub enum WindowKind {
    Single,
    Stack,
    Monocle,
}

#[derive(
    Copy, Clone, Debug, Serialize, Deserialize, Display, EnumString, ValueEnum, JsonSchema,
)]
#[strum(serialize_all = "snake_case")]
pub enum StateQuery {
    FocusedMonitorIndex,
    FocusedWorkspaceIndex,
    FocusedContainerIndex,
    FocusedWindowIndex,
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
#[strum(serialize_all = "snake_case")]
pub enum ApplicationIdentifier {
    #[serde(alias = "exe")]
    Exe,
    #[serde(alias = "class")]
    Class,
    #[serde(alias = "title")]
    Title,
}

#[derive(
    Copy, Clone, Debug, Serialize, Deserialize, Display, EnumString, ValueEnum, JsonSchema,
)]
#[strum(serialize_all = "snake_case")]
pub enum FocusFollowsMouseImplementation {
    /// A custom FFM implementation (slightly more CPU-intensive)
    Komorebi,
    /// The native (legacy) Windows FFM implementation
    Windows,
}

#[derive(
    Clone, Copy, Debug, Serialize, Deserialize, Display, EnumString, ValueEnum, JsonSchema,
)]
#[strum(serialize_all = "snake_case")]
pub enum WindowContainerBehaviour {
    /// Create a new container for each new window
    Create,
    /// Append new windows to the focused window container
    Append,
}

#[derive(
    Clone, Copy, Debug, Serialize, Deserialize, Display, EnumString, ValueEnum, JsonSchema,
)]
#[strum(serialize_all = "snake_case")]
pub enum MoveBehaviour {
    /// Swap the window container with the window container at the edge of the adjacent monitor
    Swap,
    /// Insert the window container into the focused workspace on the adjacent monitor
    Insert,
}

#[derive(
    Copy, Clone, Debug, Serialize, Deserialize, Display, EnumString, ValueEnum, JsonSchema,
)]
#[strum(serialize_all = "snake_case")]
pub enum HidingBehaviour {
    /// Use the SW_HIDE flag to hide windows when switching workspaces (has issues with Electron apps)
    Hide,
    /// Use the SW_MINIMIZE flag to hide windows when switching workspaces (has issues with frequent workspace switching)
    Minimize,
    /// Use the undocumented SetCloak Win32 function to hide windows when switching workspaces (has foregrounding issues)
    Cloak,
}

#[derive(
    Clone, Copy, Debug, Serialize, Deserialize, Display, EnumString, ValueEnum, JsonSchema,
)]
#[strum(serialize_all = "snake_case")]
pub enum OperationBehaviour {
    /// Process komorebic commands on temporarily unmanaged/floated windows
    Op,
    /// Ignore komorebic commands on temporarily unmanaged/floated windows
    NoOp,
}

#[derive(
    Clone, Copy, Debug, Serialize, Deserialize, Display, EnumString, ValueEnum, JsonSchema,
)]
#[strum(serialize_all = "snake_case")]
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
