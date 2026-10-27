//! The vocabulary of the window manager: the enums and small structs that configuration,
//! commands and the layout code are written in.
//!
//! The layout types ([`DefaultLayout`], [`Rect`], [`OperationDirection`] and the rest) are
//! those of `komorebi-layouts`, re-exported here so that the rest of the crate has one place to
//! import from. [`pathext`] resolves environment variables in configured paths and
//! [`config_generation`] holds the rules that match applications by what they are called.

use clap::ValueEnum;
use serde::Deserialize;
use serde::Serialize;
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
