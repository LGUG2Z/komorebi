//! The small enums that `komorebi.json` uses to choose how a window or a container behaves.
//!
//! Some belong to one platform only (`WindowHidingPosition` to KDE and macOS,
//! `WindowHandlingBehaviour` and `ZOrder` to Windows): they are defined here all the same,
//! and a daemon that does not read the key says so when it loads the file.

use clap::ValueEnum;
use serde::Deserialize;
use serde::Serialize;
use strum::Display;
use strum::EnumString;

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

impl FloatingLayerBehaviour {
    pub fn should_float(&self) -> bool {
        match self {
            FloatingLayerBehaviour::Tile => false,
            FloatingLayerBehaviour::Float => true,
        }
    }
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

#[derive(Debug, Copy, Clone, Display, Serialize, Deserialize, PartialEq)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
/// Z Order (https://learn.microsoft.com/en-us/windows/win32/api/winuser/nf-winuser-setwindowpos)
pub enum ZOrder {
    /// HWND_TOP
    ///
    /// Places the window at the top of the Z order.
    Top,
    /// HWND_NOTOPMOST
    ///
    /// Places the window above all non-topmost windows (that is, behind all topmost windows).
    /// This flag has no effect if the window is already a non-topmost window.
    NoTopMost,
    /// HWND_BOTTOM
    ///
    /// Places the window at the bottom of the Z order. If the hWnd parameter identifies a topmost window,
    /// the window loses its topmost status and is placed at the bottom of all other windows.
    Bottom,
    /// HWND_TOPMOST
    ///
    /// Places the window above all non-topmost windows.
    /// The window maintains its topmost position even when it is deactivated.
    TopMost,
}

impl From<ZOrder> for isize {
    fn from(val: ZOrder) -> Self {
        match val {
            ZOrder::Top => 0,
            ZOrder::NoTopMost => -2,
            ZOrder::Bottom => 1,
            ZOrder::TopMost => -1,
        }
    }
}
