use std::str::FromStr;

use clap::Clap;
use color_eyre::Result;
use serde::Deserialize;
use serde::Serialize;
use strum::Display;
use strum::EnumString;

pub use cycle_direction::CycleDirection;
pub use layout::Layout;
pub use layout::LayoutFlip;
pub use operation_direction::OperationDirection;
pub use rect::Rect;

pub mod cycle_direction;
pub mod layout;
pub mod operation_direction;
pub mod rect;

#[derive(Clone, Debug, Serialize, Deserialize, Display)]
pub enum SocketMessage {
    // Window / Container Commands
    FocusWindow(OperationDirection),
    MoveWindow(OperationDirection),
    StackWindow(OperationDirection),
    ResizeWindow(OperationDirection, Sizing),
    UnstackWindow,
    CycleStack(CycleDirection),
    MoveContainerToMonitorNumber(usize),
    MoveContainerToWorkspaceNumber(usize),
    Promote,
    ToggleFloat,
    ToggleMonocle,
    // Current Workspace Commands
    AdjustContainerPadding(Sizing, i32),
    AdjustWorkspacePadding(Sizing, i32),
    ChangeLayout(Layout),
    FlipLayout(LayoutFlip),
    // Monitor and Workspace Commands
    EnsureWorkspaces(usize, usize),
    NewWorkspace,
    ToggleTiling,
    Stop,
    TogglePause,
    Retile,
    FocusMonitorNumber(usize),
    FocusWorkspaceNumber(usize),
    ContainerPadding(usize, usize, i32),
    WorkspacePadding(usize, usize, i32),
    WorkspaceTiling(usize, usize, bool),
    WorkspaceName(usize, usize, String),
    WorkspaceLayout(usize, usize, Layout),
    // Configuration
    ReloadConfiguration,
    WatchConfiguration(bool),
    FloatClass(String),
    FloatExe(String),
    FloatTitle(String),
    State,
    FocusFollowsMouse(bool),
}

impl SocketMessage {
    pub fn as_bytes(&self) -> Result<Vec<u8>> {
        Ok(serde_json::to_string(self)?.as_bytes().to_vec())
    }

    pub fn from_bytes(bytes: &[u8]) -> Result<Self> {
        Ok(serde_json::from_slice(bytes)?)
    }
}

impl FromStr for SocketMessage {
    type Err = serde_json::Error;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        serde_json::from_str(s)
    }
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize, Display, EnumString)]
#[strum(serialize_all = "snake_case")]
#[derive(Clap)]
pub enum Sizing {
    Increase,
    Decrease,
}

impl Sizing {
    pub fn adjust_by(&self, value: i32, adjustment: i32) -> i32 {
        match self {
            Sizing::Increase => value + adjustment,
            Sizing::Decrease => {
                if value > 0 && value - adjustment >= 0 {
                    value - adjustment
                } else {
                    value
                }
            }
        }
    }
}
