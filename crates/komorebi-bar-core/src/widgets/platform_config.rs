//! The configuration types of the widgets only the Windows bar runs (keyboard, media, system
//! tray). They are data without any platform code, so that `WidgetConfig` can name every widget
//! and the schema is one union that each platform's view prunes (see `schema`).

use crate::config::LabelPrefix;
use crate::config::MediaDisplayFormat;
use komorebi_config::matching::MatchingStrategy;
use serde::Deserialize;
use serde::Serialize;

#[derive(Copy, Clone, Debug, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
/// Keyboard widget configuration
pub struct KeyboardConfig {
    /// Enable the Input widget
    pub enable: bool,
    /// Data refresh interval
    #[cfg_attr(feature = "schemars", schemars(extend("default" = 10)))]
    pub data_refresh_interval: Option<u64>,
    /// Display label prefix
    pub label_prefix: Option<LabelPrefix>,
}

#[derive(Copy, Clone, Debug, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
/// Media widget configuration
pub struct MediaConfig {
    /// Enable the Media widget
    pub enable: bool,
    /// Display format of the media widget (defaults to IconAndText)
    pub display: Option<MediaDisplayFormat>,
}

#[derive(Copy, Clone, Debug, Default, Serialize, Deserialize, PartialEq)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
/// Position of the overflow toggle button
pub enum OverflowTogglePosition {
    /// Toggle button appears on the left side (before visible icons)
    Left,
    /// Toggle button appears on the right side (after visible icons)
    #[default]
    Right,
}

#[derive(Copy, Clone, Debug, Serialize, Deserialize, PartialEq)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
/// Where to place a systray button (refresh, info, shortcuts, etc.)
pub enum ButtonPosition {
    /// Show in the main visible area
    Visible,
    /// Show in the overflow/hidden section
    Overflow,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[serde(untagged)]
/// A field value with an optional matching strategy.
///
/// A plain string uses exact (case-insensitive) matching.
/// An object with `value` and `matching_strategy` uses the specified strategy.
pub enum FieldMatch {
    /// Exact case-insensitive match
    Exact(String),
    /// Match using a specific strategy
    WithStrategy {
        /// The value to match against
        value: String,
        /// How to match (Equals, StartsWith, EndsWith, Contains, Regex, etc.)
        matching_strategy: MatchingStrategy,
    },
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[serde(untagged)]
/// Rule for matching a systray icon to hide
///
/// A plain string matches the exe name (backward compatible).
/// An object with optional `exe`, `tooltip`, and/or `guid` fields
/// uses AND logic: all specified fields must match.
/// Each field can be a plain string (exact match) or an object with
/// `value` and `matching_strategy` for advanced matching.
pub enum HiddenIconRule {
    /// Match by exe name (case-insensitive, exact)
    Exe(String),
    /// Match by one or more properties (all specified fields must match)
    Match {
        /// Exe name to match
        exe: Option<FieldMatch>,
        /// Tooltip text to match
        tooltip: Option<FieldMatch>,
        /// Icon GUID to match (most stable identifier across restarts)
        guid: Option<FieldMatch>,
    },
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
/// System tray widget configuration
pub struct SystrayConfig {
    /// Enable the System Tray widget
    pub enable: bool,
    /// A list of rules for icons to hide from the system tray
    ///
    /// Each entry can be a plain string (matches exe name, case-insensitive)
    /// or an object with optional `exe`, `tooltip`, and/or `guid` fields
    /// (all specified fields must match, AND logic, case-insensitive).
    ///
    /// Run komorebi-bar with RUST_LOG=info to see the properties of all
    /// systray icons in the log output.
    pub hidden_icons: Option<Vec<HiddenIconRule>>,
    /// Position of the overflow toggle button (default: Right)
    pub overflow_toggle_position: Option<OverflowTogglePosition>,
    /// Show an info button that opens a floating panel listing all systray icons
    /// with their exe, tooltip, and GUID. Set to "Visible" to show it in the main
    /// area, or "Overflow" to show it in the hidden/overflow section.
    pub info_button: Option<ButtonPosition>,
    /// Interval in seconds to automatically check for and remove stale icons
    /// whose owning process has exited. Clamped between 30 and 600 seconds.
    /// Defaults to 60. Set to 0 to disable.
    pub stale_icons_check_interval: Option<u64>,
    /// Show a refresh button that manually triggers stale icon cleanup.
    /// Set to "Visible" to show it in the main area, or "Overflow" to
    /// show it in the hidden/overflow section.
    pub refresh_button: Option<ButtonPosition>,
    /// Show a button that toggles komorebi-shortcuts (kills the process if
    /// running, starts it otherwise). Set to "Visible" to show it in the main
    /// area, or "Overflow" to show it in the hidden/overflow section.
    pub shortcuts_button: Option<ButtonPosition>,
}
