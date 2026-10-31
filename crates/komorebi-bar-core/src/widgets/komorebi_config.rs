//! The configuration of the komorebi widgets (workspaces, layout, focused container, ...) and
//! the layout kind the layout widget shows: data only, shared by both bars.

use crate::config::DisplayFormat;
use crate::config::WorkspacesDisplayFormat;
use color_eyre::eyre;
use serde::Deserialize;
use serde::Deserializer;
use serde::Serialize;
use serde::de::Error;
use serde_json::from_str;
use std::collections::BTreeMap;
use std::fmt::Display;
use std::fmt::Formatter;

#[derive(Clone, Debug, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
/// Komorebi widget configuration
pub struct KomorebiConfig {
    /// Configure the Workspaces widget
    pub workspaces: Option<KomorebiWorkspacesConfig>,
    /// Configure the Layout widget
    pub layout: Option<KomorebiLayoutConfig>,
    /// Configure the Workspace Layer widget
    pub workspace_layer: Option<KomorebiWorkspaceLayerConfig>,
    /// Configure the Focused Container widget
    #[serde(alias = "focused_window")]
    pub focused_container: Option<KomorebiFocusedContainerConfig>,
    /// Configure the Locked Container widget
    pub locked_container: Option<KomorebiLockedContainerConfig>,
    /// Configure the Configuration Switcher widget
    pub configuration_switcher: Option<KomorebiConfigurationSwitcherConfig>,
}

#[derive(Copy, Clone, Debug, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
/// Komorebi widget workspaces configuration
pub struct KomorebiWorkspacesConfig {
    /// Enable the Komorebi Workspaces widget
    pub enable: bool,
    /// Hide workspaces without any windows
    pub hide_empty_workspaces: bool,
    /// Display format of the workspace
    pub display: Option<WorkspacesDisplayFormat>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
/// Komorebi widget layout configuration
pub struct KomorebiLayoutConfig {
    /// Enable the Komorebi Layout widget
    pub enable: bool,
    /// List of layout options
    pub options: Option<Vec<KomorebiLayout>>,
    /// Display format of the current layout
    pub display: Option<DisplayFormat>,
}

#[derive(Copy, Clone, Debug, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
/// Komorebi widget workspace layer configuration
pub struct KomorebiWorkspaceLayerConfig {
    /// Enable the Komorebi Workspace Layer widget
    pub enable: bool,
    /// Display format of the current layer
    pub display: Option<DisplayFormat>,
    /// Show the widget event if the layer is Tiling
    pub show_when_tiling: Option<bool>,
}

#[derive(Copy, Clone, Debug, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
/// Komorebi widget focused container configuration
pub struct KomorebiFocusedContainerConfig {
    /// Enable the Komorebi Focused Container widget
    pub enable: bool,
    /// DEPRECATED: use `display` instead (Show the icon of the currently focused container)
    #[deprecated(note = "Use `display` instead")]
    pub show_icon: Option<bool>,
    /// Display format of the currently focused container
    pub display: Option<DisplayFormat>,
}

#[derive(Copy, Clone, Debug, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
/// Komorebi widget locked container configuration
pub struct KomorebiLockedContainerConfig {
    /// Enable the Komorebi Locked Container widget
    pub enable: bool,
    /// Display format of the current locked state
    pub display: Option<DisplayFormat>,
    /// Show the widget event if the layer is unlocked
    pub show_when_unlocked: Option<bool>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
/// Komorebi widget configuration switcher configuration
pub struct KomorebiConfigurationSwitcherConfig {
    /// Enable the Komorebi Configurations widget
    pub enable: bool,
    /// A map of display friendly name => path to configuration.json
    pub configurations: BTreeMap<String, String>,
}

#[derive(Copy, Clone, Debug, Serialize, PartialEq)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[serde(untagged)]
/// Komorebi layout kind
pub enum KomorebiLayout {
    /// Predefined layout
    #[cfg_attr(feature = "schemars", schemars(title = "Default"))]
    Default(komorebi_layouts::DefaultLayout),
    /// Monocle mode
    Monocle,
    /// Floating layer
    Floating,
    /// Paused
    Paused,
    /// Custom layout
    Custom,
}

impl<'de> Deserialize<'de> for KomorebiLayout {
    fn deserialize<D>(deserializer: D) -> eyre::Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let s: String = String::deserialize(deserializer)?;

        // Attempt to deserialize the string as a DefaultLayout
        if let Ok(default_layout) = from_str::<komorebi_layouts::DefaultLayout>(&format!("\"{s}\""))
        {
            return Ok(KomorebiLayout::Default(default_layout));
        }

        // Handle other cases
        match s.as_str() {
            "Monocle" => Ok(KomorebiLayout::Monocle),
            "Floating" => Ok(KomorebiLayout::Floating),
            "Paused" => Ok(KomorebiLayout::Paused),
            "Custom" => Ok(KomorebiLayout::Custom),
            _ => Err(Error::custom(format!("Invalid layout: {s}"))),
        }
    }
}

impl Display for KomorebiLayout {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            KomorebiLayout::Default(layout) => write!(f, "{layout}"),
            KomorebiLayout::Monocle => write!(f, "Monocle"),
            KomorebiLayout::Floating => write!(f, "Floating"),
            KomorebiLayout::Paused => write!(f, "Paused"),
            KomorebiLayout::Custom => write!(f, "Custom"),
        }
    }
}
