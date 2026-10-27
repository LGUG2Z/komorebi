//! `komorebi.json`, the static configuration: [`StaticConfig`] and the [`MonitorConfig`] and
//! [`WorkspaceConfig`] it holds.
//!
//! The daemon reads the file when it starts: [`StaticConfig::preload`] makes the window manager
//! from it and starts watching the file, [`StaticConfig::postload`] gives each monitor and
//! workspace the configuration of its place once the monitors are known. When the file changes,
//! the watcher sends the daemon a `ReloadStaticConfiguration` message on its own socket, and
//! [`StaticConfig::reload`] applies the file to the window manager that is running. The other way
//! round, `StaticConfig::from(&WindowManager)` writes the window manager's settings as a
//! configuration, which is what `komorebic generate-static-config` prints.
//!
//! One `komorebi.json` is meant to work on every platform komorebi runs on, so a key that does
//! nothing on KDE Plasma is accepted and ignored, and a key that is not known at all is ignored
//! too, as serde does.
//!
//! A workspace's `workspace_rules` and `initial_workspace_rules` become the rules that send windows
//! to it (`WORKSPACE_MATCHING_RULES`), which are made again from the file each time it is read.
//!
//! The rules for particular applications are kept in files of their own, which the configuration
//! names with `app_specific_configuration_path` ([`ApplicationSpecificConfiguration`]); their
//! rules are added to the ones the configuration itself has.
//!
//! Settings that have no key are left to their defaults ([`DEFAULT_RESIZE_DELTA`],
//! [`DEFAULT_WORKSPACE_PADDING`] and the others in [`crate`]), and an optional key set to `null`
//! is the same as one that is left out.

use crate::DATA_DIR;
use crate::DEFAULT_CONTAINER_PADDING;
use crate::DEFAULT_MOUSE_FOLLOWS_FOCUS;
use crate::DEFAULT_RESIZE_DELTA;
use crate::DEFAULT_WORKSPACE_PADDING;
use crate::DISPLAY_INDEX_PREFERENCES;
use crate::FLOATING_APPLICATIONS;
use crate::FLOATING_WINDOW_TOGGLE_ASPECT_RATIO;
use crate::IGNORE_IDENTIFIERS;
use crate::LAYOUT_DEFAULTS;
use crate::MANAGE_IDENTIFIERS;
use crate::REGEX_IDENTIFIERS;
use crate::WORKSPACE_MATCHING_RULES;
use crate::animation::ANIMATION_DURATION_GLOBAL;
use crate::animation::ANIMATION_DURATION_PER_ANIMATION;
use crate::animation::ANIMATION_ENABLED_GLOBAL;
use crate::animation::ANIMATION_ENABLED_PER_ANIMATION;
use crate::animation::ANIMATION_FPS;
use crate::animation::ANIMATION_STYLE_GLOBAL;
use crate::animation::ANIMATION_STYLE_PER_ANIMATION;
use crate::animation::DEFAULT_ANIMATION_FPS;
use crate::animation::PerAnimationPrefixConfig;
use crate::border_manager;
use crate::core::Axis;
use crate::core::CrossBoundaryBehaviour;
use crate::core::DefaultLayout;
use crate::core::FloatingLayerBehaviour;
use crate::core::Layout;
use crate::core::LayoutDefaultEntry;
use crate::core::LayoutOptions;
use crate::core::MonocleFocusBehaviour;
use crate::core::MoveBehaviour;
use crate::core::OperationBehaviour;
use crate::core::Placement;
use crate::core::Rect;
use crate::core::SocketMessage;
use crate::core::WindowContainerBehaviour;
use crate::core::WindowHidingPosition;
use crate::core::WindowManagementBehaviour;
use crate::core::animation::AnimationStyle;
use crate::core::asc::ApplicationSpecificConfiguration;
use crate::core::asc::AscApplicationRulesOrSchema;
use crate::core::config_generation::MatchingRule;
use crate::core::config_generation::MatchingStrategy;
use crate::core::config_generation::WorkspaceMatchingRule;
use crate::core::pathext::ResolvedPathBuf;
use crate::monitor::Monitor;
use crate::ring::Ring;
use crate::window::AspectRatio;
use crate::window::PredefinedAspectRatio;
use crate::window_manager::WindowManager;
use crate::window_manager_event::WindowManagerEvent;
use crate::workspace::Workspace;
use color_eyre::eyre;
use crossbeam_channel::Receiver;
use hotwatch::EventKind;
use hotwatch::Hotwatch;
use parking_lot::Mutex;
use regex::Regex;
use serde::Deserialize;
use serde::Serialize;
use std::collections::HashMap;
use std::io::ErrorKind;
use std::io::Write;
use std::os::unix::net::UnixListener;
use std::os::unix::net::UnixStream;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::Ordering;

// serde_as must be before derive
#[serde_with::serde_as]
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
/// Workspace configuration
pub struct WorkspaceConfig {
    /// Name
    pub name: String,
    /// Layout
    #[serde(skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "schemars", schemars(extend("default" = DefaultLayout::BSP)))]
    pub layout: Option<DefaultLayout>,
    /// Layout-specific options
    #[serde(skip_serializing_if = "Option::is_none")]
    pub layout_options: Option<LayoutOptions>,
    /// Threshold-based layout options rules in the format of threshold => options.
    /// When container count >= threshold, the highest matching threshold's options
    /// fully replace the base `layout_options`.
    /// This follows the same threshold logic as `layout_rules`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub layout_options_rules: Option<HashMap<usize, LayoutOptions>>,
    // /// END OF LIFE FEATURE: Custom Layout (default: None)
    // #[serde(skip_serializing_if = "Option::is_none")]
    // #[serde_as(as = "Option<ResolvedPathBuf>")]
    // pub custom_layout: Option<PathBuf>,
    /// Layout rules in the format of threshold => layout
    #[serde(skip_serializing_if = "Option::is_none")]
    pub layout_rules: Option<HashMap<usize, DefaultLayout>>,
    /// Container padding (default: global)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub container_padding: Option<i32>,
    /// Workspace padding (default: global)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub workspace_padding: Option<i32>,
    /// Initial workspace application rules
    #[serde(skip_serializing_if = "Option::is_none")]
    pub initial_workspace_rules: Option<Vec<MatchingRule>>,
    /// Permanent workspace application rules
    #[serde(skip_serializing_if = "Option::is_none")]
    pub workspace_rules: Option<Vec<MatchingRule>>,
    /// Work area offset rules in the format of threshold => Rect (default: None)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub work_area_offset_rules: Option<HashMap<usize, Rect>>,
    /// Workspace specific work area offset
    #[serde(skip_serializing_if = "Option::is_none")]
    pub work_area_offset: Option<Rect>,
    /// Apply this monitor's window-based work area offset
    #[serde(skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "schemars", schemars(extend("default" = true)))]
    pub apply_window_based_work_area_offset: Option<bool>,
    /// Determine what happens when a new window is opened
    #[serde(skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "schemars", schemars(extend("default" = WindowContainerBehaviour::Create)))]
    pub window_container_behaviour: Option<WindowContainerBehaviour>,
    /// Window container behavior rules in the format of threshold => behavior
    #[serde(skip_serializing_if = "Option::is_none")]
    pub window_container_behaviour_rules: Option<HashMap<usize, WindowContainerBehaviour>>,
    /// Enable or disable float override, which makes it so every new window opens in floating mode
    #[serde(skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "schemars", schemars(extend("default" = false)))]
    pub float_override: Option<bool>,
    /// Enable or disable tiling for the workspace
    #[serde(skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "schemars", schemars(extend("default" = true)))]
    pub tile: Option<bool>,
    /// Specify an axis on which to flip the selected layout
    #[serde(skip_serializing_if = "Option::is_none")]
    pub layout_flip: Option<Axis>,
    /// Determine what happens to a new window when the Floating workspace layer is active
    #[serde(skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "schemars", schemars(extend("default" = FloatingLayerBehaviour::Tile)))]
    pub floating_layer_behaviour: Option<FloatingLayerBehaviour>,
}

impl From<&Workspace> for WorkspaceConfig {
    fn from(value: &Workspace) -> Self {
        let mut layout_rules = HashMap::new();
        for (threshold, layout) in &value.layout_rules {
            match layout {
                Layout::Default(value) => {
                    layout_rules.insert(*threshold, *value);
                }
            }
        }
        let layout_rules = (!layout_rules.is_empty()).then_some(layout_rules);

        let mut work_area_offset_rules = HashMap::new();
        for (threshold, rect) in &value.work_area_offset_rules {
            work_area_offset_rules.insert(*threshold, *rect);
        }
        let work_area_offset_rules =
            (!work_area_offset_rules.is_empty()).then_some(work_area_offset_rules);

        let mut window_container_behaviour_rules = HashMap::new();
        for (threshold, behaviour) in value.window_container_behaviour_rules.iter().flatten() {
            window_container_behaviour_rules.insert(*threshold, *behaviour);
        }

        let default_container_padding = DEFAULT_CONTAINER_PADDING.load(Ordering::SeqCst);
        let default_workspace_padding = DEFAULT_WORKSPACE_PADDING.load(Ordering::SeqCst);

        let container_padding = value.container_padding.and_then(|container_padding| {
            if container_padding == default_container_padding {
                None
            } else {
                Option::from(container_padding)
            }
        });

        let workspace_padding = value.workspace_padding.and_then(|workspace_padding| {
            if workspace_padding == default_workspace_padding {
                None
            } else {
                Option::from(workspace_padding)
            }
        });

        let tile = if value.tile { None } else { Some(false) };

        Self {
            name: value
                .name
                .clone()
                .unwrap_or_else(|| String::from("unnamed")),
            layout: value
                .tile
                .then_some(match value.layout {
                    Layout::Default(layout) => Option::from(layout),
                })
                .flatten(),
            layout_options: {
                tracing::debug!(
                    "Parsing workspace config - layout_options: {:?}",
                    value.layout_options
                );
                value.layout_options
            },
            layout_options_rules: if value.layout_options_rules.is_empty() {
                None
            } else {
                Some(value.layout_options_rules.iter().copied().collect())
            },
            // custom_layout: value
            //     .workspace_config
            //     .as_ref()
            //     .and_then(|c| c.custom_layout.clone()),
            layout_rules,
            container_padding,
            workspace_padding,
            initial_workspace_rules: value
                .workspace_config
                .as_ref()
                .and_then(|c| c.initial_workspace_rules.clone()),
            workspace_rules: value
                .workspace_config
                .as_ref()
                .and_then(|c| c.workspace_rules.clone()),
            work_area_offset_rules,
            work_area_offset: value.work_area_offset,
            apply_window_based_work_area_offset: Some(value.apply_window_based_work_area_offset),
            window_container_behaviour: value.window_container_behaviour,
            window_container_behaviour_rules: if window_container_behaviour_rules.is_empty() {
                None
            } else {
                Some(window_container_behaviour_rules)
            },
            float_override: value.float_override,
            tile,
            layout_flip: value.layout_flip,
            floating_layer_behaviour: value.floating_layer_behaviour,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
/// Monitor configuration
pub struct MonitorConfig {
    /// Workspace configurations
    pub workspaces: Vec<WorkspaceConfig>,
    /// Monitor-specific work area offset
    #[serde(skip_serializing_if = "Option::is_none")]
    pub work_area_offset: Option<Rect>,
    /// Window based work area offset
    #[serde(skip_serializing_if = "Option::is_none")]
    pub window_based_work_area_offset: Option<Rect>,
    /// Open window limit after which the window based work area offset will no longer be applied
    #[serde(skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "schemars", schemars(extend("default" = 1)))]
    pub window_based_work_area_offset_limit: Option<isize>,
    /// Container padding
    #[serde(skip_serializing_if = "Option::is_none")]
    pub container_padding: Option<i32>,
    /// Workspace padding (default: global)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub workspace_padding: Option<i32>,
    /// Determine what happens to a new window when the Floating workspace layer is active
    #[serde(skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "schemars", schemars(extend("default" = FloatingLayerBehaviour::Tile)))]
    pub floating_layer_behaviour: Option<FloatingLayerBehaviour>,
    /// Accepted so that a configuration written for another platform loads, and ignored: there is
    /// no hiding position on KDE Plasma
    #[serde(skip_serializing_if = "Option::is_none")]
    pub window_hiding_position: Option<WindowHidingPosition>,
}

impl From<&Monitor> for MonitorConfig {
    fn from(value: &Monitor) -> Self {
        let mut workspaces = vec![];
        for w in value.workspaces() {
            workspaces.push(WorkspaceConfig::from(w));
        }

        let default_container_padding = DEFAULT_CONTAINER_PADDING.load(Ordering::SeqCst);
        let default_workspace_padding = DEFAULT_WORKSPACE_PADDING.load(Ordering::SeqCst);

        let container_padding = value.container_padding.and_then(|container_padding| {
            if container_padding == default_container_padding {
                None
            } else {
                Option::from(container_padding)
            }
        });

        let workspace_padding = value.workspace_padding.and_then(|workspace_padding| {
            if workspace_padding == default_workspace_padding {
                None
            } else {
                Option::from(workspace_padding)
            }
        });

        Self {
            workspaces,
            work_area_offset: value.work_area_offset,
            window_based_work_area_offset: value.window_based_work_area_offset,
            window_based_work_area_offset_limit: Some(value.window_based_work_area_offset_limit),
            container_padding,
            workspace_padding,
            floating_layer_behaviour: value.floating_layer_behaviour,
            window_hiding_position: None,
        }
    }
}

#[serde_with::serde_as]
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[serde(untagged)]
/// Path(s) to application-specific configuration file(s)
pub enum AppSpecificConfigurationPath {
    /// A single applications.json file
    Single(#[serde_as(as = "ResolvedPathBuf")] PathBuf),
    /// Multiple applications.json files
    Multiple(#[serde_as(as = "Vec<ResolvedPathBuf>")] Vec<PathBuf>),
}

#[serde_with::serde_as]
#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
/// The `komorebi.json` static configuration file reference for komorebi for KDE Plasma
pub struct StaticConfig {
    // /// DISCOURAGED: Minimum width for a window to be eligible for tiling
    // #[serde(skip_serializing_if = "Option::is_none")]
    // pub minimum_window_width: Option<i32>,
    // /// DISCOURAGED: Minimum height for a window to be eligible for tiling
    // #[serde(skip_serializing_if = "Option::is_none")]
    // pub minimum_window_height: Option<i32>,
    /// Delta to resize windows by
    #[serde(skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "schemars", schemars(extend("default" = DEFAULT_RESIZE_DELTA)))]
    pub resize_delta: Option<i32>,
    /// Determine what happens when a new window is opened
    #[serde(skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "schemars", schemars(extend("default" = WindowContainerBehaviour::Create)))]
    pub window_container_behaviour: Option<WindowContainerBehaviour>,
    /// Enable or disable float override, which makes it so every new window opens in floating mode
    #[serde(skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "schemars", schemars(extend("default" = false)))]
    pub float_override: Option<bool>,
    /// Determines what happens on a new window when on the `FloatingLayer`
    #[serde(skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "schemars", schemars(extend("default" = FloatingLayerBehaviour::Tile)))]
    pub floating_layer_behaviour: Option<FloatingLayerBehaviour>,
    /// Determines the placement of a new window when toggling to float
    #[serde(skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "schemars", schemars(extend("default" = Placement::CenterAndResize)))]
    pub toggle_float_placement: Option<Placement>,
    /// Determines the `Placement` to be used when spawning a window on the floating layer with the
    /// `FloatingLayerBehaviour` set to `FloatingLayerBehaviour::Float`
    #[serde(skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "schemars", schemars(extend("default" = Placement::Center)))]
    pub floating_layer_placement: Option<Placement>,
    /// Determines the `Placement` to be used when spawning a window with float override active
    #[serde(skip_serializing_if = "Option::is_none")]
    pub float_override_placement: Option<Placement>,
    /// Determines the `Placement` to be used when spawning a window that matches a
    /// 'floating_applications' rule
    #[serde(skip_serializing_if = "Option::is_none")]
    pub float_rule_placement: Option<Placement>,
    /// Determine what happens when a window is moved across a monitor boundary
    #[serde(skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "schemars", schemars(extend("default" = MoveBehaviour::Swap)))]
    pub cross_monitor_move_behaviour: Option<MoveBehaviour>,
    /// Determine what happens when an action is called on a window at a monitor boundary
    #[serde(skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "schemars", schemars(extend("default" = CrossBoundaryBehaviour::Monitor)))]
    pub cross_boundary_behaviour: Option<CrossBoundaryBehaviour>,
    /// Determine what happens when focusing in a direction while a monocle container is active
    #[serde(skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "schemars", schemars(extend("default" = MonocleFocusBehaviour::NoOp)))]
    pub monocle_focus_behaviour: Option<MonocleFocusBehaviour>,
    /// Determine what happens when commands are sent while an unmanaged window is in the foreground
    #[serde(skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "schemars", schemars(extend("default" = OperationBehaviour::Op)))]
    pub unmanaged_window_operation_behaviour: Option<OperationBehaviour>,
    /// Enable or disable mouse follows focus
    #[serde(skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "schemars", schemars(extend("default" = DEFAULT_MOUSE_FOLLOWS_FOCUS)))]
    pub mouse_follows_focus: Option<bool>,
    /// Path to an applications.json, or a list of them, with rules for the applications that
    /// need them: which windows to ignore, to manage and to float
    #[serde(skip_serializing_if = "Option::is_none")]
    pub app_specific_configuration_path: Option<AppSpecificConfigurationPath>,
    /// Width of window borders
    #[serde(skip_serializing_if = "Option::is_none")]
    #[serde(alias = "active_window_border_width")]
    #[cfg_attr(feature = "schemars", schemars(extend("default" = border_manager::BORDER_WIDTH)))]
    pub border_width: Option<i32>,
    /// Offset of window borders
    #[serde(skip_serializing_if = "Option::is_none")]
    #[serde(alias = "active_window_border_offset")]
    #[cfg_attr(feature = "schemars", schemars(extend("default" = border_manager::BORDER_OFFSET)))]
    pub border_offset: Option<i32>,
    /// Radius of window borders
    #[serde(skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "schemars", schemars(extend("default" = border_manager::BORDER_RADIUS)))]
    pub border_radius: Option<i32>,
    /// Display window borders
    #[serde(skip_serializing_if = "Option::is_none")]
    #[serde(alias = "active_window_border")]
    #[cfg_attr(feature = "schemars", schemars(extend("default" = border_manager::BORDER_ENABLED)))]
    pub border: Option<bool>,
    // /// Active window border style (default: System)
    // #[serde(skip_serializing_if = "Option::is_none")]
    // #[serde(alias = "active_window_border_style")]
    // pub border_style: Option<BorderStyle>,
    // /// Active window border implementation (default: Komorebi)
    // #[serde(skip_serializing_if = "Option::is_none")]
    // pub border_implementation: Option<BorderImplementation>,
    /// Global default workspace padding
    #[serde(skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "schemars", schemars(extend("default" = DEFAULT_WORKSPACE_PADDING)))]
    pub default_workspace_padding: Option<i32>,
    /// Global default container padding
    #[serde(skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "schemars", schemars(extend("default" = DEFAULT_CONTAINER_PADDING)))]
    pub default_container_padding: Option<i32>,
    /// Per-layout default options and rules, keyed by layout name.
    /// Applied as fallback when a workspace does not define its own layout_options or layout_options_rules.
    /// If a workspace defines either setting, all global defaults for that layout are completely replaced.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub layout_defaults: Option<HashMap<DefaultLayout, LayoutDefaultEntry>>,
    /// Monitor and workspace configurations
    #[serde(skip_serializing_if = "Option::is_none")]
    pub monitors: Option<Vec<MonitorConfig>>,
    // /// Which Windows signal to use when hiding windows (default: Cloak)
    // #[serde(skip_serializing_if = "Option::is_none")]
    // pub window_hiding_behaviour: Option<HidingBehaviour>,
    /// Global work area (space used for tiling) offset
    #[serde(skip_serializing_if = "Option::is_none")]
    pub global_work_area_offset: Option<Rect>,
    /// Individual window floating rules
    #[serde(skip_serializing_if = "Option::is_none")]
    #[serde(alias = "float_rules")]
    pub ignore_rules: Option<Vec<MatchingRule>>,
    /// Individual window force-manage rules
    #[serde(skip_serializing_if = "Option::is_none")]
    pub manage_rules: Option<Vec<MatchingRule>>,
    /// Identify applications which should be managed as floating windows
    #[serde(skip_serializing_if = "Option::is_none")]
    pub floating_applications: Option<Vec<MatchingRule>>,
    /// Accepted so that a configuration written for another platform loads, and ignored: a KWin
    /// window is always a window
    #[serde(skip_serializing_if = "Option::is_none")]
    pub titleless_applications: Option<Vec<String>>,
    /// Accepted so that a configuration written for another platform loads, and ignored: a KWin
    /// window has no native tabs
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tabbed_applications: Option<Vec<String>>,
    // /// Identify border overflow applications
    // #[serde(skip_serializing_if = "Option::is_none")]
    // pub border_overflow_applications: Option<Vec<MatchingRule>>,
    // /// Identify tray and multi-window applications
    // #[serde(skip_serializing_if = "Option::is_none")]
    // pub tray_and_multi_window_applications: Option<Vec<MatchingRule>>,
    // /// Identify applications that have the WS_EX_LAYERED extended window style
    // #[serde(skip_serializing_if = "Option::is_none")]
    // pub layered_applications: Option<Vec<MatchingRule>>,
    // /// Identify applications that send EVENT_OBJECT_NAMECHANGE on launch (very rare)
    // #[serde(skip_serializing_if = "Option::is_none")]
    // pub object_name_change_applications: Option<Vec<MatchingRule>>,
    // /// Do not process EVENT_OBJECT_NAMECHANGE events as Show events for identified applications matching these title regexes
    // #[serde(skip_serializing_if = "Option::is_none")]
    // pub object_name_change_title_ignore_list: Option<Vec<String>>,
    // /// Set monitor index preferences
    // #[serde(skip_serializing_if = "Option::is_none")]
    // pub monitor_index_preferences: Option<HashMap<usize, Rect>>,
    /// Set display index preferences
    #[serde(skip_serializing_if = "Option::is_none")]
    pub display_index_preferences: Option<HashMap<usize, String>>,
    // /// Stackbar configuration options
    // #[serde(skip_serializing_if = "Option::is_none")]
    // pub stackbar: Option<StackbarConfig>,
    /// Animations configuration options
    #[serde(skip_serializing_if = "Option::is_none")]
    pub animation: Option<AnimationsConfig>,
    /// Aspect ratio to resize with when toggling floating mode for a window
    #[serde(skip_serializing_if = "Option::is_none")]
    pub floating_window_aspect_ratio: Option<AspectRatio>,
    // /// Which Windows API behaviour to use when manipulating windows (default: Sync)
    // #[serde(skip_serializing_if = "Option::is_none")]
    // pub window_handling_behaviour: Option<WindowHandlingBehaviour>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
/// Animation configuration
pub struct AnimationsConfig {
    /// Enable or disable animations
    #[cfg_attr(feature = "schemars", schemars(extend("default" = PerAnimationPrefixConfig::Global(false))))]
    pub enabled: PerAnimationPrefixConfig<bool>,
    /// Set the animation duration in ms
    #[serde(skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "schemars", schemars(extend("default" = PerAnimationPrefixConfig::Global(250))))]
    pub duration: Option<PerAnimationPrefixConfig<u64>>,
    /// Set the animation style
    #[serde(skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "schemars", schemars(extend("default" = PerAnimationPrefixConfig::Global(AnimationStyle::Linear))))]
    pub style: Option<PerAnimationPrefixConfig<AnimationStyle>>,
    /// Set the animation FPS
    #[serde(skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "schemars", schemars(extend("default" = DEFAULT_ANIMATION_FPS)))]
    pub fps: Option<u64>,
}

// #[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
// #[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
// pub struct TabsConfig {
//     /// Width of a stackbar tab
//     #[serde(skip_serializing_if = "Option::is_none")]
//     pub width: Option<i32>,
//     /// Focused tab text colour
//     #[serde(skip_serializing_if = "Option::is_none")]
//     pub focused_text: Option<Colour>,
//     /// Unfocused tab text colour
//     #[serde(skip_serializing_if = "Option::is_none")]
//     pub unfocused_text: Option<Colour>,
//     /// Tab background colour
//     #[serde(skip_serializing_if = "Option::is_none")]
//     pub background: Option<Colour>,
//     /// Font family
//     #[serde(skip_serializing_if = "Option::is_none")]
//     pub font_family: Option<String>,
//     /// Font size
//     #[serde(skip_serializing_if = "Option::is_none")]
//     pub font_size: Option<i32>,
// }

// #[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
// #[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
// pub struct StackbarConfig {
//     /// Stackbar height
//     #[serde(skip_serializing_if = "Option::is_none")]
//     pub height: Option<i32>,
//     /// Stackbar label
//     #[serde(skip_serializing_if = "Option::is_none")]
//     pub label: Option<StackbarLabel>,
//     /// Stackbar mode (default: Never)
//     #[serde(skip_serializing_if = "Option::is_none")]
//     pub mode: Option<StackbarMode>,
//     /// Stackbar tab configuration options
//     #[serde(skip_serializing_if = "Option::is_none")]
//     pub tabs: Option<TabsConfig>,
// }

impl From<&WindowManager> for StaticConfig {
    #[allow(clippy::too_many_lines)]
    fn from(value: &WindowManager) -> Self {
        let mut monitors = vec![];
        for m in value.monitors() {
            monitors.push(MonitorConfig::from(m));
        }

        Self {
            // invisible_borders: None,
            resize_delta: Option::from(value.resize_delta),
            window_container_behaviour: Option::from(
                value.window_management_behaviour.current_behaviour,
            ),
            float_override: Option::from(value.window_management_behaviour.float_override),
            floating_layer_behaviour: Option::from(
                value.window_management_behaviour.floating_layer_behaviour,
            ),
            toggle_float_placement: Option::from(
                value.window_management_behaviour.toggle_float_placement,
            ),
            floating_layer_placement: Option::from(
                value.window_management_behaviour.floating_layer_placement,
            ),
            float_override_placement: Option::from(
                value.window_management_behaviour.float_override_placement,
            ),
            float_rule_placement: Option::from(
                value.window_management_behaviour.float_rule_placement,
            ),
            cross_monitor_move_behaviour: Option::from(value.cross_monitor_move_behaviour),
            cross_boundary_behaviour: Option::from(value.cross_boundary_behaviour),
            monocle_focus_behaviour: Option::from(value.monocle_focus_behaviour),
            unmanaged_window_operation_behaviour: Option::from(
                value.unmanaged_window_operation_behaviour,
            ),
            // minimum_window_height: Some(window::MINIMUM_HEIGHT.load(Ordering::SeqCst)),
            // minimum_window_width: Some(window::MINIMUM_WIDTH.load(Ordering::SeqCst)),
            mouse_follows_focus: Option::from(value.mouse_follows_focus),
            app_specific_configuration_path: None,
            border_width: Option::from(border_manager::BORDER_WIDTH.load(Ordering::SeqCst)),
            border_offset: Option::from(border_manager::BORDER_OFFSET.load(Ordering::SeqCst)),
            border_radius: Option::from(border_manager::BORDER_RADIUS.load(Ordering::SeqCst)),
            border: Option::from(border_manager::BORDER_ENABLED.load(Ordering::SeqCst)),
            // border_style: Option::from(STYLE.load()),
            // border_z_order: None,
            // border_implementation: Option::from(IMPLEMENTATION.load()),
            default_workspace_padding: Option::from(
                DEFAULT_WORKSPACE_PADDING.load(Ordering::SeqCst),
            ),
            default_container_padding: Option::from(
                DEFAULT_CONTAINER_PADDING.load(Ordering::SeqCst),
            ),
            layout_defaults: {
                let guard = LAYOUT_DEFAULTS.lock();
                if guard.is_empty() {
                    None
                } else {
                    Some(guard.clone())
                }
            },
            monitors: Option::from(monitors),
            // window_hiding_behaviour: Option::from(*HIDING_BEHAVIOUR.lock()),
            global_work_area_offset: value.work_area_offset,
            ignore_rules: None,
            floating_applications: None,
            titleless_applications: None,
            manage_rules: None,
            // border_overflow_applications: None,
            // tray_and_multi_window_applications: None,
            // layered_applications: None,
            // object_name_change_applications: None,
            // object_name_change_title_ignore_list: None,
            // monitor_index_preferences: Option::from(MONITOR_INDEX_PREFERENCES.lock().clone()),
            display_index_preferences: Option::from(DISPLAY_INDEX_PREFERENCES.read().clone()),
            // stackbar: None,
            animation: None,
            // slow_application_compensation_time: Option::from(
            //     SLOW_APPLICATION_COMPENSATION_TIME.load(Ordering::SeqCst),
            // ),
            // slow_application_identifiers: Option::from(SLOW_APPLICATION_IDENTIFIERS.lock().clone()),
            // bar_configurations: None,
            // remove_titlebar_applications: Option::from(NO_TITLEBAR.lock().clone()),
            floating_window_aspect_ratio: Option::from(*FLOATING_WINDOW_TOGGLE_ASPECT_RATIO.lock()),
            // window_handling_behaviour: Option::from(WINDOW_HANDLING_BEHAVIOUR.load()),
            tabbed_applications: None,
        }
    }
}

impl StaticConfig {
    #[allow(clippy::cognitive_complexity, clippy::too_many_lines)]
    fn apply_globals(&mut self) -> eyre::Result<()> {
        *FLOATING_WINDOW_TOGGLE_ASPECT_RATIO.lock() = self
            .floating_window_aspect_ratio
            .unwrap_or(AspectRatio::Predefined(PredefinedAspectRatio::Standard));

        // if let Some(monitor_index_preferences) = &self.monitor_index_preferences {
        //     let mut preferences = MONITOR_INDEX_PREFERENCES.lock();
        //     preferences.clone_from(monitor_index_preferences);
        // }
        //

        if let Some(display_index_preferences) = &self.display_index_preferences {
            let mut preferences = DISPLAY_INDEX_PREFERENCES.write();
            preferences.clone_from(display_index_preferences);
        }

        // if let Some(behaviour) = self.window_hiding_behaviour {
        //     let mut window_hiding_behaviour = HIDING_BEHAVIOUR.lock();
        //     *window_hiding_behaviour = behaviour;
        // }

        // if let Some(height) = self.minimum_window_height {
        //     window::MINIMUM_HEIGHT.store(height, Ordering::SeqCst);
        // }
        //
        // if let Some(width) = self.minimum_window_width {
        //     window::MINIMUM_WIDTH.store(width, Ordering::SeqCst);
        // }

        if let Some(animations) = &self.animation {
            match &animations.enabled {
                PerAnimationPrefixConfig::Prefix(enabled) => {
                    ANIMATION_ENABLED_PER_ANIMATION.lock().clone_from(enabled);
                }
                PerAnimationPrefixConfig::Global(enabled) => {
                    ANIMATION_ENABLED_GLOBAL.store(*enabled, Ordering::SeqCst);
                    ANIMATION_ENABLED_PER_ANIMATION.lock().clear();
                }
            }

            match &animations.style {
                Some(PerAnimationPrefixConfig::Prefix(style)) => {
                    ANIMATION_STYLE_PER_ANIMATION.lock().clone_from(style);
                }
                Some(PerAnimationPrefixConfig::Global(style)) => {
                    let mut animation_style = ANIMATION_STYLE_GLOBAL.lock();
                    *animation_style = *style;
                    ANIMATION_STYLE_PER_ANIMATION.lock().clear();
                }
                None => {}
            }

            match &animations.duration {
                Some(PerAnimationPrefixConfig::Prefix(duration)) => {
                    ANIMATION_DURATION_PER_ANIMATION.lock().clone_from(duration);
                }
                Some(PerAnimationPrefixConfig::Global(duration)) => {
                    ANIMATION_DURATION_GLOBAL.store(*duration, Ordering::SeqCst);
                    ANIMATION_DURATION_PER_ANIMATION.lock().clear();
                }
                None => {}
            }

            ANIMATION_FPS.store(
                animations.fps.unwrap_or(DEFAULT_ANIMATION_FPS),
                Ordering::SeqCst,
            );
        }

        if let Some(container) = self.default_container_padding {
            DEFAULT_CONTAINER_PADDING.store(container, Ordering::SeqCst);
        }

        if let Some(workspace) = self.default_workspace_padding {
            DEFAULT_WORKSPACE_PADDING.store(workspace, Ordering::SeqCst);
        }

        if let Some(defaults) = &self.layout_defaults {
            *LAYOUT_DEFAULTS.lock() = defaults.clone();
        } else {
            LAYOUT_DEFAULTS.lock().clear();
        }

        if let Some(border_width) = self.border_width {
            border_manager::BORDER_WIDTH.store(border_width, Ordering::SeqCst);
        }

        if let Some(border_offset) = self.border_offset {
            border_manager::BORDER_OFFSET.store(border_offset, Ordering::SeqCst);
        }

        if let Some(border_radius) = self.border_radius {
            border_manager::BORDER_RADIUS.store(border_radius, Ordering::SeqCst);
        }

        if let Some(border_enabled) = self.border {
            border_manager::BORDER_ENABLED.store(border_enabled, Ordering::SeqCst);
        }

        // STYLE.store(self.border_style.unwrap_or_default());
        //
        // if !*WINDOWS_11
        //     && matches!(
        //         self.border_implementation.unwrap_or_default(),
        //         BorderImplementation::Windows
        //     )
        // {
        //     tracing::error!(
        //         "BorderImplementation::Windows is only supported on Windows 11 and above"
        //     );
        // } else {
        //     IMPLEMENTATION.store(self.border_implementation.unwrap_or_default());
        //     match IMPLEMENTATION.load() {
        //         BorderImplementation::Komorebi => {
        //             border_manager::destroy_all_borders()?;
        //         }
        //         BorderImplementation::Windows => {
        //             // TODO: figure out how to call wm.remove_all_accents here
        //         }
        //     }
        //
        //     border_manager::send_notification(None);
        // }
        //
        let mut ignore_identifiers = IGNORE_IDENTIFIERS.lock();
        let mut regex_identifiers = REGEX_IDENTIFIERS.lock();
        let mut manage_identifiers = MANAGE_IDENTIFIERS.lock();
        // let mut tray_and_multi_window_identifiers = TRAY_AND_MULTI_WINDOW_IDENTIFIERS.lock();
        // let mut object_name_change_identifiers = OBJECT_NAME_CHANGE_ON_LAUNCH.lock();
        // let mut object_name_change_title_ignore_list = OBJECT_NAME_CHANGE_TITLE_IGNORE_LIST.lock();
        // let mut layered_identifiers = LAYERED_WHITELIST.lock();
        // let mut slow_application_identifiers = SLOW_APPLICATION_IDENTIFIERS.lock();
        let mut floating_applications = FLOATING_APPLICATIONS.lock();
        // let mut no_titlebar_applications = NO_TITLEBAR.lock();

        if let Some(rules) = &mut self.ignore_rules {
            populate_rules(rules, &mut ignore_identifiers, &mut regex_identifiers)?;
        }

        if let Some(rules) = &mut self.floating_applications {
            populate_rules(rules, &mut floating_applications, &mut regex_identifiers)?;
        }

        if let Some(rules) = &mut self.manage_rules {
            populate_rules(rules, &mut manage_identifiers, &mut regex_identifiers)?;
        }

        // `tabbed_applications` and `titleless_applications` work around how macOS exposes native
        // tabs and untitled windows. They are accepted so one configuration works on every
        // platform, and ignored: a KWin window is always a window.
        if self.tabbed_applications.is_some() || self.titleless_applications.is_some() {
            tracing::debug!("ignoring tabbed_applications and titleless_applications on KDE");
        }

        // if let Some(rules) = &mut self.object_name_change_applications {
        //     populate_rules(
        //         rules,
        //         &mut object_name_change_identifiers,
        //         &mut regex_identifiers,
        //     )?;
        // }
        //
        // if let Some(regexes) = &mut self.object_name_change_title_ignore_list {
        //     let mut updated = vec![];
        //     for r in regexes {
        //         if let Ok(regex) = Regex::new(r) {
        //             updated.push(regex);
        //         }
        //     }
        //
        //     *object_name_change_title_ignore_list = updated;
        // }
        //
        // if let Some(rules) = &mut self.layered_applications {
        //     populate_rules(rules, &mut layered_identifiers, &mut regex_identifiers)?;
        // }
        //
        // if let Some(rules) = &mut self.tray_and_multi_window_applications {
        //     populate_rules(
        //         rules,
        //         &mut tray_and_multi_window_identifiers,
        //         &mut regex_identifiers,
        //     )?;
        // }
        //
        // if let Some(rules) = &mut self.slow_application_identifiers {
        //     populate_rules(
        //         rules,
        //         &mut slow_application_identifiers,
        //         &mut regex_identifiers,
        //     )?;
        // }
        //
        // if let Some(rules) = &mut self.remove_titlebar_applications {
        //     populate_rules(rules, &mut no_titlebar_applications, &mut regex_identifiers)?;
        // }
        //
        // if let Some(stackbar) = &self.stackbar {
        //     if let Some(height) = &stackbar.height {
        //         STACKBAR_TAB_HEIGHT.store(*height, Ordering::SeqCst);
        //     }
        //
        //     if let Some(label) = &stackbar.label {
        //         STACKBAR_LABEL.store(*label);
        //     }
        //
        //     if let Some(mode) = &stackbar.mode {
        //         STACKBAR_MODE.store(*mode);
        //     }
        //
        //     #[allow(clippy::assigning_clones)]
        //     if let Some(tabs) = &stackbar.tabs {
        //         if let Some(background) = &tabs.background {
        //             STACKBAR_TAB_BACKGROUND_COLOUR.store((*background).into(), Ordering::SeqCst);
        //         }
        //
        //         if let Some(colour) = &tabs.focused_text {
        //             STACKBAR_FOCUSED_TEXT_COLOUR.store((*colour).into(), Ordering::SeqCst);
        //         }
        //
        //         if let Some(colour) = &tabs.unfocused_text {
        //             STACKBAR_UNFOCUSED_TEXT_COLOUR.store((*colour).into(), Ordering::SeqCst);
        //         }
        //
        //         if let Some(width) = &tabs.width {
        //             STACKBAR_TAB_WIDTH.store(*width, Ordering::SeqCst);
        //         }
        //
        //         STACKBAR_FONT_SIZE.store(tabs.font_size.unwrap_or(0), Ordering::SeqCst);
        //         *STACKBAR_FONT_FAMILY.lock() = tabs.font_family.clone();
        //     }
        // }
        //
        if let Some(path) = &self.app_specific_configuration_path {
            match path {
                AppSpecificConfigurationPath::Single(path) => handle_asc_file(
                    path,
                    &mut ignore_identifiers,
                    &mut manage_identifiers,
                    &mut floating_applications,
                    &mut regex_identifiers,
                )?,
                AppSpecificConfigurationPath::Multiple(paths) => {
                    for path in paths {
                        handle_asc_file(
                            path,
                            &mut ignore_identifiers,
                            &mut manage_identifiers,
                            &mut floating_applications,
                            &mut regex_identifiers,
                        )?
                    }
                }
            }
        }

        // if let Some(behaviour) = self.window_handling_behaviour {
        //     WINDOW_HANDLING_BEHAVIOUR.store(behaviour);
        // }

        Ok(())
    }

    /// Parses the text of a `komorebi.json`
    ///
    /// Comments and trailing commas are allowed, and keys that are not known are ignored.
    ///
    /// # Errors
    ///
    /// The text is not JSON, or a value is not what its key takes.
    ///
    /// # Example
    ///
    /// ```rust
    /// use komorebi::static_config::StaticConfig;
    ///
    /// let config = StaticConfig::read_raw(
    ///     r#"{
    ///         // a comment, and a trailing comma
    ///         "resize_delta": 100,
    ///         "a_key_that_is_not_known": true,
    ///     }"#,
    /// )?;
    /// assert_eq!(config.resize_delta, Some(100));
    ///
    /// assert!(StaticConfig::read_raw(r#"{ "resize_delta": "wide" }"#).is_err());
    ///
    /// # Ok::<(), Box<dyn std::error::Error>>(())
    /// ```
    pub fn read_raw(raw: &str) -> eyre::Result<Self> {
        Ok(serde_json::from_str(raw)?)
    }

    /// Reads the file at `path`, as [`StaticConfig::read_raw`] reads text
    ///
    /// # Errors
    ///
    /// The file cannot be read, or it is not a configuration.
    pub fn read(path: &PathBuf) -> eyre::Result<Self> {
        let content = std::fs::read_to_string(path)?;
        serde_json::from_str(&content).map_err(Into::into)
    }

    /// Makes the window manager from the file at `path`, and starts watching the file
    ///
    /// The settings that are not the window manager's own (the default padding, the rules and the
    /// others of [`crate`]) are applied first. The window manager listens on `unix_listener`, or
    /// when that is `None` on `komorebi.sock` in [`struct@DATA_DIR`], where a socket file that is
    /// left over is removed first. It has no monitors yet: [`WindowManager::init`] loads them, and
    /// [`StaticConfig::postload`] gives them what the file says about them.
    ///
    /// A change to the file, or its removal, makes the watcher write a `ReloadStaticConfiguration`
    /// message to `komorebi.sock` in [`struct@DATA_DIR`].
    ///
    /// # Errors
    ///
    /// - The file cannot be read or is not a configuration ([`StaticConfig::read`]).
    /// - A rule has a regular expression that is not valid.
    /// - The socket file that is left over cannot be removed, or the socket cannot be bound.
    /// - The file cannot be watched.
    #[allow(clippy::too_many_lines)]
    pub fn preload(
        path: &PathBuf,
        incoming: Receiver<WindowManagerEvent>,
        unix_listener: Option<UnixListener>,
    ) -> eyre::Result<WindowManager> {
        let mut value = Self::read(path)?;
        value.apply_globals()?;

        let listener = match unix_listener {
            Some(listener) => listener,
            None => {
                let socket = DATA_DIR.join("komorebi.sock");

                match std::fs::remove_file(&socket) {
                    Ok(()) => {}
                    Err(error) => match error.kind() {
                        ErrorKind::NotFound => {}
                        _ => {
                            return Err(error.into());
                        }
                    },
                };

                UnixListener::bind(&socket)?
            }
        };

        let mut wm = WindowManager {
            monitors: Ring::default(),
            monitor_usr_idx_map: HashMap::new(),
            incoming_events: incoming,
            command_listener: listener,
            is_paused: false,
            // virtual_desktop_id: current_virtual_desktop(),
            work_area_offset: value.global_work_area_offset,
            window_management_behaviour: WindowManagementBehaviour {
                current_behaviour: value
                    .window_container_behaviour
                    .unwrap_or(WindowContainerBehaviour::Create),
                float_override: value.float_override.unwrap_or_default(),
                floating_layer_override: false, // this value is always automatically calculated
                floating_layer_behaviour: value.floating_layer_behaviour.unwrap_or_default(),
                toggle_float_placement: value
                    .toggle_float_placement
                    .unwrap_or(Placement::CenterAndResize),
                floating_layer_placement: value
                    .floating_layer_placement
                    .unwrap_or(Placement::Center),
                float_override_placement: value.float_override_placement.unwrap_or(Placement::None),
                float_rule_placement: value.float_rule_placement.unwrap_or(Placement::None),
            },
            cross_monitor_move_behaviour: value
                .cross_monitor_move_behaviour
                .unwrap_or(MoveBehaviour::Swap),
            cross_boundary_behaviour: value
                .cross_boundary_behaviour
                .unwrap_or(CrossBoundaryBehaviour::Monitor),
            monocle_focus_behaviour: value.monocle_focus_behaviour.unwrap_or_default(),
            unmanaged_window_operation_behaviour: value
                .unmanaged_window_operation_behaviour
                .unwrap_or(OperationBehaviour::Op),
            resize_delta: value.resize_delta.unwrap_or(DEFAULT_RESIZE_DELTA),
            mouse_follows_focus: value
                .mouse_follows_focus
                .unwrap_or(DEFAULT_MOUSE_FOLLOWS_FOCUS),
            hotwatch: Hotwatch::new()?,
            minimized_windows: HashMap::new(),
            already_moved_window_handles: Default::default(),
            known_window_ids: HashMap::new(),
        };

        let bytes = SocketMessage::ReloadStaticConfiguration(path.clone()).as_bytes()?;

        wm.hotwatch.watch(path, move |event| match event.kind {
            // an editor either writes the file in place or replaces it with a new one, which is a
            // modification in the first case and a removal in the second
            EventKind::Modify(_) | EventKind::Remove(_) => {
                let socket = DATA_DIR.join("komorebi.sock");
                let mut stream =
                    UnixStream::connect(socket).expect("could not connect to komorebi.sock");
                stream
                    .write_all(&bytes)
                    .expect("could not write to komorebi.sock");
            }
            _ => {}
        })?;

        Ok(wm)
    }

    /// Gives each monitor of `wm`, once [`WindowManager::init`] has loaded them, and each of their
    /// workspaces the configuration the file has for them
    ///
    /// A monitor takes the configuration at the index `display_index_preferences` gives its serial
    /// number id, or else the first configuration that no monitor has taken. It gets as many
    /// workspaces as the configuration lists, which are named and laid out as it says. A monitor
    /// that has no configuration keeps what it has.
    ///
    /// # Errors
    ///
    /// The file cannot be read or is not a configuration ([`StaticConfig::read`]).
    pub fn postload(path: &PathBuf, wm: &Arc<Mutex<WindowManager>>) -> eyre::Result<()> {
        let mut value = Self::read(path)?;
        let mut wm = wm.lock();

        let configs_with_preference: Vec<_> =
            DISPLAY_INDEX_PREFERENCES.read().keys().copied().collect();
        let mut configs_used = Vec::new();

        let mut workspace_matching_rules = WORKSPACE_MATCHING_RULES.lock();
        workspace_matching_rules.clear();
        drop(workspace_matching_rules);

        let offset = wm.work_area_offset;
        for (i, monitor) in wm.monitors_mut().iter_mut().enumerate() {
            let preferred_config_idx = {
                let display_index_preferences = DISPLAY_INDEX_PREFERENCES.read();

                display_index_preferences.iter().find_map(|(c_idx, id)| {
                    if monitor.serial_number_id.eq(id) {
                        Some(*c_idx)
                    } else {
                        None
                    }
                })
            };

            let idx = preferred_config_idx.or({
                // Monitor without preferred config idx.
                // Get index of first config that is not a preferred config of some other monitor
                // and that has not been used yet. This might return `None` as well, in that case
                // this monitor won't have a config tied to it and will use the default values.
                let m_config_count = value
                    .monitors
                    .as_ref()
                    .map(|ms| ms.len())
                    .unwrap_or_default();
                (0..m_config_count)
                    .find(|i| !configs_with_preference.contains(i) && !configs_used.contains(i))
            });

            if let Some(monitor_config) = value
                .monitors
                .as_mut()
                .and_then(|ms| idx.and_then(|i| ms.get_mut(i)))
            {
                if let Some(used_config_idx) = idx {
                    configs_used.push(used_config_idx);
                }

                monitor.ensure_workspace_count(monitor_config.workspaces.len());
                monitor.work_area_offset = monitor_config.work_area_offset;
                monitor.window_based_work_area_offset =
                    monitor_config.window_based_work_area_offset;
                monitor.window_based_work_area_offset_limit = monitor_config
                    .window_based_work_area_offset_limit
                    .unwrap_or(1);
                monitor.container_padding = monitor_config.container_padding;
                monitor.workspace_padding = monitor_config.workspace_padding;
                monitor.floating_layer_behaviour = monitor_config.floating_layer_behaviour;

                monitor.update_workspaces_globals(offset);
                for (j, ws) in monitor.workspaces_mut().iter_mut().enumerate() {
                    if let Some(workspace_config) = monitor_config.workspaces.get_mut(j) {
                        ws.load_static_config(workspace_config, value.layout_defaults.as_ref())?;
                    }
                }

                let mut workspace_matching_rules = WORKSPACE_MATCHING_RULES.lock();
                for (j, ws) in monitor_config.workspaces.iter().enumerate() {
                    if let Some(rules) = &ws.workspace_rules {
                        for r in rules {
                            workspace_matching_rules.push(WorkspaceMatchingRule {
                                monitor_index: i,
                                workspace_index: j,
                                matching_rule: r.clone(),
                                initial_only: false,
                            });
                        }
                    }

                    if let Some(rules) = &ws.initial_workspace_rules {
                        for r in rules {
                            workspace_matching_rules.push(WorkspaceMatchingRule {
                                monitor_index: i,
                                workspace_index: j,
                                matching_rule: r.clone(),
                                initial_only: true,
                            });
                        }
                    }
                }
            }
        }

        wm.enforce_workspace_rules()?;

        if value.border == Some(true) {
            border_manager::BORDER_ENABLED.store(true, Ordering::SeqCst);
        }

        Ok(())
    }

    /// Applies the file at `path` to `wm`, which is running, and tiles it again
    ///
    /// What [`StaticConfig::preload`] and [`StaticConfig::postload`] do for a window manager that
    /// starts, this does for one that has monitors and windows: its settings, then the
    /// configuration of each monitor and workspace. A monitor that already has a work area offset
    /// keeps it. The windows stay where they are in the workspaces.
    ///
    /// # Errors
    ///
    /// - The file cannot be read or is not a configuration ([`StaticConfig::read`]); nothing has
    ///   changed then.
    /// - A rule has a regular expression that is not valid; the settings before it have changed.
    pub fn reload(path: &PathBuf, wm: &mut WindowManager) -> eyre::Result<()> {
        let mut value = Self::read(path)?;

        value.apply_globals()?;

        let configs_with_preference: Vec<_> =
            DISPLAY_INDEX_PREFERENCES.read().keys().copied().collect();
        let mut configs_used = Vec::new();

        let mut workspace_matching_rules = WORKSPACE_MATCHING_RULES.lock();
        workspace_matching_rules.clear();
        drop(workspace_matching_rules);

        let offset = wm.work_area_offset;
        for (i, monitor) in wm.monitors_mut().iter_mut().enumerate() {
            let preferred_config_idx = {
                let display_index_preferences = DISPLAY_INDEX_PREFERENCES.read();

                display_index_preferences.iter().find_map(|(c_idx, id)| {
                    if monitor.serial_number_id.eq(id) {
                        Some(*c_idx)
                    } else {
                        None
                    }
                })
            };

            let idx = preferred_config_idx.or({
                // Monitor without preferred config idx.
                // Get index of first config that is not a preferred config of some other monitor
                // and that has not been used yet. This might return `None` as well, in that case
                // this monitor won't have a config tied to it and will use the default values.
                let m_config_count = value
                    .monitors
                    .as_ref()
                    .map(|ms| ms.len())
                    .unwrap_or_default();
                (0..m_config_count)
                    .find(|i| !configs_with_preference.contains(i) && !configs_used.contains(i))
            });
            if let Some(monitor_config) = value
                .monitors
                .as_ref()
                .and_then(|ms| idx.and_then(|i| ms.get(i)))
            {
                if let Some(used_config_idx) = idx {
                    configs_used.push(used_config_idx);
                }

                monitor.ensure_workspace_count(monitor_config.workspaces.len());
                if monitor.work_area_offset.is_none() {
                    monitor.work_area_offset = monitor_config.work_area_offset;
                }
                monitor.window_based_work_area_offset =
                    monitor_config.window_based_work_area_offset;
                monitor.window_based_work_area_offset_limit = monitor_config
                    .window_based_work_area_offset_limit
                    .unwrap_or(1);
                monitor.container_padding = monitor_config.container_padding;
                monitor.workspace_padding = monitor_config.workspace_padding;
                monitor.floating_layer_behaviour = monitor_config.floating_layer_behaviour;

                monitor.update_workspaces_globals(offset);

                for (j, ws) in monitor.workspaces_mut().iter_mut().enumerate() {
                    if let Some(workspace_config) = monitor_config.workspaces.get(j) {
                        ws.load_static_config(workspace_config, value.layout_defaults.as_ref())?;
                    }
                }

                let mut workspace_matching_rules = WORKSPACE_MATCHING_RULES.lock();
                for (j, ws) in monitor_config.workspaces.iter().enumerate() {
                    if let Some(rules) = &ws.workspace_rules {
                        for r in rules {
                            workspace_matching_rules.push(WorkspaceMatchingRule {
                                monitor_index: i,
                                workspace_index: j,
                                matching_rule: r.clone(),
                                initial_only: false,
                            });
                        }
                    }

                    if let Some(rules) = &ws.initial_workspace_rules {
                        for r in rules {
                            workspace_matching_rules.push(WorkspaceMatchingRule {
                                monitor_index: i,
                                workspace_index: j,
                                matching_rule: r.clone(),
                                initial_only: true,
                            });
                        }
                    }
                }
            }
        }

        wm.enforce_workspace_rules()?;

        if let Some(border_enabled) = value.border {
            border_manager::BORDER_ENABLED.store(border_enabled, Ordering::SeqCst);
        }

        wm.window_management_behaviour.current_behaviour =
            value.window_container_behaviour.unwrap_or_default();
        wm.window_management_behaviour.float_override = value.float_override.unwrap_or_default();
        wm.window_management_behaviour.floating_layer_behaviour =
            value.floating_layer_behaviour.unwrap_or_default();
        wm.window_management_behaviour.toggle_float_placement = value
            .toggle_float_placement
            .unwrap_or(Placement::CenterAndResize);
        wm.window_management_behaviour.floating_layer_placement =
            value.floating_layer_placement.unwrap_or(Placement::Center);
        wm.window_management_behaviour.float_override_placement =
            value.float_override_placement.unwrap_or(Placement::None);
        wm.window_management_behaviour.float_rule_placement =
            value.float_rule_placement.unwrap_or(Placement::None);
        wm.cross_monitor_move_behaviour = value.cross_monitor_move_behaviour.unwrap_or_default();
        wm.cross_boundary_behaviour = value.cross_boundary_behaviour.unwrap_or_default();
        wm.monocle_focus_behaviour = value.monocle_focus_behaviour.unwrap_or_default();
        wm.unmanaged_window_operation_behaviour = value
            .unmanaged_window_operation_behaviour
            .unwrap_or_default();
        wm.resize_delta = value.resize_delta.unwrap_or(DEFAULT_RESIZE_DELTA);
        wm.mouse_follows_focus = value
            .mouse_follows_focus
            .unwrap_or(DEFAULT_MOUSE_FOLLOWS_FOCUS);
        wm.work_area_offset = value.global_work_area_offset;

        let monitor_count = wm.monitors().len();

        for i in 0..monitor_count {
            wm.update_focused_workspace_by_monitor_idx(i)?;
        }

        // to update the borders
        wm.retile_all(true)?;

        Ok(())
    }
}

fn populate_rules(
    matching_rules: &mut Vec<MatchingRule>,
    identifiers: &mut Vec<MatchingRule>,
    regex_identifiers: &mut HashMap<String, Regex>,
) -> eyre::Result<()> {
    for matching_rule in matching_rules {
        if !identifiers.contains(matching_rule) {
            match matching_rule {
                MatchingRule::Simple(simple) => {
                    if simple.matching_strategy.is_none() {
                        simple.matching_strategy = Option::from(MatchingStrategy::Legacy);
                    }

                    if matches!(simple.matching_strategy, Some(MatchingStrategy::Regex)) {
                        let re = Regex::new(&simple.id)?;
                        regex_identifiers.insert(simple.id.clone(), re);
                    }
                }
                MatchingRule::Composite(composite) => {
                    for rule in composite {
                        if rule.matching_strategy.is_none() {
                            rule.matching_strategy = Option::from(MatchingStrategy::Legacy);
                        }

                        if matches!(rule.matching_strategy, Some(MatchingStrategy::Regex)) {
                            let re = Regex::new(&rule.id)?;
                            regex_identifiers.insert(rule.id.clone(), re);
                        }
                    }
                }
            }
            identifiers.push(matching_rule.clone());
        }
    }

    Ok(())
}

/// Adds the rules of the applications.json at `path` to the lists the window manager decides by
///
/// A file that is not there is skipped with a warning: one `komorebi.json` is meant to work on
/// every platform, and the file it points to may be one that another platform has
/// (`applications.mac.json` on a KDE desktop). A file that is there and is not an applications.json
/// is an error. Only a file with the extension `json` is read. The rules to ignore, to manage and
/// to float of every application in the file are added; the other kinds of rule that file can
/// have (`titleless` and `tabbed`) are about how another platform exposes windows, and are
/// ignored.
#[allow(clippy::too_many_arguments)]
fn handle_asc_file(
    path: &PathBuf,
    ignore_identifiers: &mut Vec<MatchingRule>,
    manage_identifiers: &mut Vec<MatchingRule>,
    floating_applications: &mut Vec<MatchingRule>,
    regex_identifiers: &mut HashMap<String, Regex>,
) -> eyre::Result<()> {
    // One komorebi.json is meant to work on every platform, and the file it points to is one that
    // another platform has (applications.mac.json on a KDE desktop): warn and carry on
    if !path.exists() {
        tracing::warn!(
            "the application specific configuration {} does not exist, ignoring it",
            path.display()
        );
        return Ok(());
    }

    match path.extension() {
        Some(ext) if ext.to_string_lossy().to_string().as_str() == "json" => {
            tracing::info!("loading applications.json from: {}", path.display());
            let mut asc = ApplicationSpecificConfiguration::load(path)?;

            for entry in asc.values_mut() {
                match entry {
                    AscApplicationRulesOrSchema::Schema(_) => {}
                    AscApplicationRulesOrSchema::AscApplicationRules(entry) => {
                        if let Some(rules) = &mut entry.ignore {
                            populate_rules(rules, ignore_identifiers, regex_identifiers)?;
                        }

                        if let Some(rules) = &mut entry.manage {
                            populate_rules(rules, manage_identifiers, regex_identifiers)?;
                        }

                        if let Some(rules) = &mut entry.floating {
                            populate_rules(rules, floating_applications, regex_identifiers)?;
                        }
                    }
                }
            }
        }
        _ => {}
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_keys_that_do_nothing_on_kde_are_accepted_and_ignored() {
        let config = StaticConfig::read_raw(
            r#"{
                "titleless_applications": ["a"],
                "tabbed_applications": ["b"],
                "a_key_of_another_platform": true,
                "monitors": [
                    { "window_hiding_position": "BottomRight", "workspaces": [{ "name": "one" }] }
                ]
            }"#,
        )
        .unwrap();

        assert_eq!(config.titleless_applications, Some(vec![String::from("a")]));
        assert_eq!(config.tabbed_applications, Some(vec![String::from("b")]));
        assert_eq!(
            config.monitors.unwrap()[0].window_hiding_position,
            Some(WindowHidingPosition::BottomRight)
        );
    }

    #[test]
    fn float_rules_is_another_name_for_ignore_rules() {
        let rules = r#"[{ "kind": "Exe", "id": "a", "matching_strategy": "Equals" }]"#;

        let named = StaticConfig::read_raw(&format!(r#"{{ "ignore_rules": {rules} }}"#)).unwrap();
        let aliased = StaticConfig::read_raw(&format!(r#"{{ "float_rules": {rules} }}"#)).unwrap();

        assert_eq!(named, aliased);
        assert_eq!(named.ignore_rules.unwrap().len(), 1);
    }

    #[test]
    fn a_setting_that_is_null_is_a_setting_that_is_left_out() {
        let config =
            StaticConfig::read_raw(r#"{ "resize_delta": null, "monitors": null }"#).unwrap();

        assert_eq!(config, StaticConfig::default());
    }

    #[test]
    fn a_value_of_the_wrong_kind_is_an_error_that_names_the_key() {
        let error = StaticConfig::read_raw(r#"{ "resize_delta": "wide" }"#).unwrap_err();

        assert!(error.to_string().contains("invalid type"), "{error}");
    }

    #[test]
    fn a_file_that_is_not_there_is_an_error() {
        let path = PathBuf::from("/nonexistent/komorebi.json");

        assert!(StaticConfig::read(&path).is_err());
    }

    #[test]
    fn a_missing_application_specific_configuration_is_skipped_not_fatal() {
        let (mut ignore, mut manage, mut floating) = (Vec::new(), Vec::new(), Vec::new());
        let mut regexes = HashMap::new();

        handle_asc_file(
            &PathBuf::from(
                "/nonexistent/komorebi-application-specific-configuration/applications.mac.json",
            ),
            &mut ignore,
            &mut manage,
            &mut floating,
            &mut regexes,
        )
        .unwrap();

        assert!(ignore.is_empty() && manage.is_empty() && floating.is_empty());
    }

    #[test]
    fn an_application_specific_configuration_that_exists_but_is_broken_is_still_an_error() {
        let path = PathBuf::from(format!(
            "/tmp/komorebi-asc-test-{}.json",
            std::process::id()
        ));
        std::fs::write(&path, "{ not json").unwrap();

        let result = handle_asc_file(
            &path,
            &mut Vec::new(),
            &mut Vec::new(),
            &mut Vec::new(),
            &mut HashMap::new(),
        );
        let _ = std::fs::remove_file(&path);

        assert!(result.is_err());
    }
}
