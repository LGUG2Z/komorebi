//! The `komorebi.json` static configuration file: one `StaticConfig` with every key any
//! platform reads. A key that only some platforms read says so in its schema
//! (`x-komorebi-platforms`); a daemon reports the keys it does not read when it loads a file.

use std::collections::HashMap;
use std::path::PathBuf;

use komorebi_layouts::Axis;
use komorebi_layouts::DefaultLayout;
use komorebi_layouts::LayoutDefaultEntry;
use komorebi_layouts::LayoutOptions;
use komorebi_layouts::Rect;
use komorebi_protocol::animation::AnimationStyle;
use komorebi_protocol::pathext::ResolvedPathBuf;
use komorebi_protocol::pathext::resolve_option_hashmap_usize_path;
use komorebi_protocol::types::BorderImplementation;
use komorebi_protocol::types::BorderStyle;
use komorebi_protocol::types::FocusFollowsMouseImplementation;
use komorebi_protocol::types::HidingBehaviour;
use komorebi_protocol::types::MonocleFocusBehaviour;
use komorebi_protocol::types::MoveBehaviour;
use komorebi_protocol::types::OperationBehaviour;
use komorebi_protocol::types::StackbarLabel;
use komorebi_protocol::types::StackbarMode;
use komorebi_themes::KomorebiTheme;
use komorebi_themes::colour::Colour;
use serde::Deserialize;
use serde::Serialize;

use crate::animation::PerAnimationPrefixConfig;
use crate::aspect_ratio::AspectRatio;
use crate::behaviour::CrossBoundaryBehaviour;
use crate::behaviour::FloatingLayerBehaviour;
use crate::behaviour::Placement;
use crate::behaviour::WindowContainerBehaviour;
use crate::behaviour::WindowHandlingBehaviour;
use crate::behaviour::WindowHidingPosition;
use crate::behaviour::ZOrder;
use crate::defaults;
use crate::matching::MatchingRule;
use crate::placement::InitialWindowPlacementRules;

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
/// Border colours for different container states
pub struct BorderColours {
    /// Border colour when the container contains a single window
    #[serde(skip_serializing_if = "Option::is_none")]
    pub single: Option<Colour>,
    /// Border colour when the container contains multiple windows
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stack: Option<Colour>,
    /// Border colour when the container is in monocle mode
    #[serde(skip_serializing_if = "Option::is_none")]
    pub monocle: Option<Colour>,
    /// Border colour when the container is in floating mode
    #[serde(skip_serializing_if = "Option::is_none")]
    pub floating: Option<Colour>,
    /// Border colour when the container is unfocused
    #[serde(skip_serializing_if = "Option::is_none")]
    pub unfocused: Option<Colour>,
    /// Border colour when the container is unfocused and locked
    #[serde(skip_serializing_if = "Option::is_none")]
    pub unfocused_locked: Option<Colour>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
/// Theme options
pub struct ThemeOptions {
    /// Specify Light or Dark variant for theme generation
    #[serde(skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "schemars", schemars(extend("default" = komorebi_themes::ThemeVariant::Dark)))]
    pub theme_variant: Option<komorebi_themes::ThemeVariant>,
    /// Border colour when the container contains a single window
    #[serde(skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "schemars", schemars(extend("default" = komorebi_themes::Base16Value::Base0D)))]
    pub single_border: Option<komorebi_themes::Base16Value>,
    /// Border colour when the container contains multiple windows
    #[serde(skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "schemars", schemars(extend("default" = komorebi_themes::Base16Value::Base0B)))]
    pub stack_border: Option<komorebi_themes::Base16Value>,
    /// Border colour when the container is in monocle mode
    #[serde(skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "schemars", schemars(extend("default" = komorebi_themes::Base16Value::Base0F)))]
    pub monocle_border: Option<komorebi_themes::Base16Value>,
    /// Border colour when the window is floating
    #[serde(skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "schemars", schemars(extend("default" = komorebi_themes::Base16Value::Base09)))]
    pub floating_border: Option<komorebi_themes::Base16Value>,
    /// Border colour when the container is unfocused
    #[serde(skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "schemars", schemars(extend("default" = komorebi_themes::Base16Value::Base01)))]
    pub unfocused_border: Option<komorebi_themes::Base16Value>,
    /// Border colour when the container is unfocused and locked
    #[serde(skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "schemars", schemars(extend("default" = komorebi_themes::Base16Value::Base08)))]
    pub unfocused_locked_border: Option<komorebi_themes::Base16Value>,
    /// Stackbar focused tab text colour
    #[serde(skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "schemars", schemars(extend("default" = komorebi_themes::Base16Value::Base0B)))]
    #[cfg_attr(feature = "schemars", schemars(extend("x-komorebi-platforms" = ["windows"])))]
    pub stackbar_focused_text: Option<komorebi_themes::Base16Value>,
    /// Stackbar unfocused tab text colour
    #[serde(skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "schemars", schemars(extend("default" = komorebi_themes::Base16Value::Base05)))]
    #[cfg_attr(feature = "schemars", schemars(extend("x-komorebi-platforms" = ["windows"])))]
    pub stackbar_unfocused_text: Option<komorebi_themes::Base16Value>,
    /// Stackbar tab background colour
    #[serde(skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "schemars", schemars(extend("default" = komorebi_themes::Base16Value::Base01)))]
    #[cfg_attr(feature = "schemars", schemars(extend("x-komorebi-platforms" = ["windows"])))]
    pub stackbar_background: Option<komorebi_themes::Base16Value>,
    /// Komorebi status bar accent
    #[serde(skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "schemars", schemars(extend("default" = komorebi_themes::Base16Value::Base0D)))]
    pub bar_accent: Option<komorebi_themes::Base16Value>,
}

#[serde_with::serde_as]
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
/// Wallpaper configuration
pub struct Wallpaper {
    /// Path to the wallpaper image file
    #[serde_as(as = "ResolvedPathBuf")]
    pub path: PathBuf,
    /// Generate and apply Base16 theme for this wallpaper
    #[serde(skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "schemars", schemars(extend("default" = true)))]
    pub generate_theme: Option<bool>,
    /// Specify Light or Dark variant for theme generation
    #[serde(skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "schemars", schemars(extend("default" = komorebi_themes::ThemeVariant::Dark)))]
    pub theme_options: Option<ThemeOptions>,
}

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
    /// END OF LIFE FEATURE: Custom Layout
    #[cfg_attr(feature = "schemars", schemars(extend("deprecated" = true)))]
    #[serde(skip_serializing_if = "Option::is_none")]
    #[serde_as(as = "Option<ResolvedPathBuf>")]
    #[cfg_attr(feature = "schemars", schemars(extend("x-komorebi-platforms" = ["windows"])))]
    pub custom_layout: Option<PathBuf>,
    /// Layout rules in the format of threshold => layout
    #[serde(skip_serializing_if = "Option::is_none")]
    pub layout_rules: Option<HashMap<usize, DefaultLayout>>,
    /// Work area offset rules in the format of threshold => Rect (default: None)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub work_area_offset_rules: Option<HashMap<usize, Rect>>,
    /// END OF LIFE FEATURE: Custom layout rules
    #[cfg_attr(feature = "schemars", schemars(extend("deprecated" = true)))]
    #[serde(skip_serializing_if = "Option::is_none")]
    #[serde(deserialize_with = "resolve_option_hashmap_usize_path", default)]
    #[cfg_attr(feature = "schemars", schemars(extend("x-komorebi-platforms" = ["windows"])))]
    pub custom_layout_rules: Option<HashMap<usize, PathBuf>>,
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
    /// Window container behaviour rules in the format of threshold => behaviour
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
    /// Specify a wallpaper for this workspace
    #[serde(skip_serializing_if = "Option::is_none")]
    pub wallpaper: Option<Wallpaper>,
    /// Initial window placement rules that determine where new tiled windows are placed
    /// in the container list. Can be a placement target (a `WindowPlacement` string like
    /// `"Primary"` or a 1-based container index), or a map of placement targets to matching rules.
    #[serde(skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "schemars", schemars(extend("x-komorebi-platforms" = ["windows", "kde"])))]
    pub initial_window_placement_rules: Option<InitialWindowPlacementRules>,
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
    /// Container padding (default: global)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub container_padding: Option<i32>,
    /// Workspace padding (default: global)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub workspace_padding: Option<i32>,
    /// Specify a wallpaper for this monitor
    #[serde(skip_serializing_if = "Option::is_none")]
    pub wallpaper: Option<Wallpaper>,
    /// Determine what happens to a new window when the Floating workspace layer is active
    #[serde(skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "schemars", schemars(extend("default" = FloatingLayerBehaviour::Tile)))]
    pub floating_layer_behaviour: Option<FloatingLayerBehaviour>,
    /// Determine which position windows should be hidden at on this monitor
    #[serde(skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "schemars", schemars(extend("x-komorebi-platforms" = ["kde", "macos"])))]
    pub window_hiding_position: Option<WindowHidingPosition>,
}

#[serde_with::serde_as]
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[serde(untagged)]
/// Path(s) to application-specific configuration file(s)
pub enum AppSpecificConfigurationPath {
    /// A single `applications.json` file
    Single(#[serde_as(as = "ResolvedPathBuf")] PathBuf),
    /// Multiple `applications.json` files
    Multiple(#[serde_as(as = "Vec<ResolvedPathBuf>")] Vec<PathBuf>),
}

#[serde_with::serde_as]
#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
/// The `komorebi.json` static configuration file reference for `v0.2.0`
pub struct StaticConfig {
    /// DEPRECATED from v0.1.22: no longer required
    #[cfg_attr(feature = "schemars", schemars(extend("deprecated" = true)))]
    #[serde(skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "schemars", schemars(extend("x-komorebi-platforms" = ["windows"])))]
    pub invisible_borders: Option<Rect>,
    /// DISCOURAGED: Minimum width for a window to be eligible for tiling
    #[serde(skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "schemars", schemars(extend("x-komorebi-platforms" = ["windows"])))]
    pub minimum_window_width: Option<i32>,
    /// DISCOURAGED: Minimum height for a window to be eligible for tiling
    #[serde(skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "schemars", schemars(extend("x-komorebi-platforms" = ["windows"])))]
    pub minimum_window_height: Option<i32>,
    /// Delta to resize windows by
    #[serde(skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "schemars", schemars(extend("default" = defaults::DEFAULT_RESIZE_DELTA)))]
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
    /// `floating_applications` rule
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
    /// END OF LIFE FEATURE: Use https://github.com/LGUG2Z/masir instead
    #[cfg_attr(feature = "schemars", schemars(extend("deprecated" = true)))]
    #[serde(skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "schemars", schemars(extend("x-komorebi-platforms" = ["windows", "kde"])))]
    pub focus_follows_mouse: Option<FocusFollowsMouseImplementation>,
    /// Enable or disable mouse follows focus
    #[serde(skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "schemars", schemars(extend("default" = defaults::DEFAULT_MOUSE_FOLLOWS_FOCUS)))]
    pub mouse_follows_focus: Option<bool>,
    /// Path to applications.json from komorebi-application-specific-configurations
    #[serde(skip_serializing_if = "Option::is_none")]
    pub app_specific_configuration_path: Option<AppSpecificConfigurationPath>,
    /// Width of window borders
    #[serde(skip_serializing_if = "Option::is_none")]
    #[serde(alias = "active_window_border_width")]
    #[cfg_attr(feature = "schemars", schemars(extend("default" = defaults::BORDER_WIDTH)))]
    pub border_width: Option<i32>,
    /// Offset of window borders
    #[serde(skip_serializing_if = "Option::is_none")]
    #[serde(alias = "active_window_border_offset")]
    #[cfg_attr(feature = "schemars", schemars(extend("default" = defaults::BORDER_OFFSET)))]
    pub border_offset: Option<i32>,
    /// Radius of window borders
    #[serde(skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "schemars", schemars(extend("default" = defaults::BORDER_RADIUS)))]
    #[cfg_attr(feature = "schemars", schemars(extend("x-komorebi-platforms" = ["kde", "macos"])))]
    pub border_radius: Option<i32>,
    /// Display window borders
    #[serde(skip_serializing_if = "Option::is_none")]
    #[serde(alias = "active_window_border")]
    #[cfg_attr(feature = "schemars", schemars(extend("default" = defaults::BORDER_ENABLED)))]
    pub border: Option<bool>,
    /// Window border colours for different container types (has no effect if [`theme`] is defined)
    #[serde(skip_serializing_if = "Option::is_none")]
    #[serde(alias = "active_window_border_colours")]
    pub border_colours: Option<BorderColours>,
    /// Window border style
    #[serde(skip_serializing_if = "Option::is_none")]
    #[serde(alias = "active_window_border_style")]
    #[cfg_attr(feature = "schemars", schemars(extend("default" = BorderStyle::System)))]
    #[cfg_attr(feature = "schemars", schemars(extend("x-komorebi-platforms" = ["windows"])))]
    pub border_style: Option<BorderStyle>,
    /// DEPRECATED from v0.1.31: no longer required
    #[cfg_attr(feature = "schemars", schemars(extend("deprecated" = true)))]
    #[serde(skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "schemars", schemars(extend("x-komorebi-platforms" = ["windows"])))]
    pub border_z_order: Option<ZOrder>,
    /// Window border implementation
    #[serde(skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "schemars", schemars(extend("default" = BorderImplementation::Komorebi)))]
    #[cfg_attr(feature = "schemars", schemars(extend("x-komorebi-platforms" = ["windows"])))]
    pub border_implementation: Option<BorderImplementation>,
    /// Add transparency to unfocused windows
    #[serde(skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "schemars", schemars(extend("default" = defaults::TRANSPARENCY_ENABLED)))]
    #[cfg_attr(feature = "schemars", schemars(extend("x-komorebi-platforms" = ["windows", "kde"])))]
    pub transparency: Option<bool>,
    /// Alpha value for unfocused window transparency [[0-255]]
    #[serde(skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "schemars", schemars(extend("default" = defaults::TRANSPARENCY_ALPHA)))]
    #[cfg_attr(feature = "schemars", schemars(extend("x-komorebi-platforms" = ["windows", "kde"])))]
    pub transparency_alpha: Option<u8>,
    /// Individual window transparency ignore rules
    #[serde(skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "schemars", schemars(extend("x-komorebi-platforms" = ["windows", "kde"])))]
    pub transparency_ignore_rules: Option<Vec<MatchingRule>>,
    /// Global default workspace padding
    #[serde(skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "schemars", schemars(extend("default" = defaults::DEFAULT_WORKSPACE_PADDING)))]
    pub default_workspace_padding: Option<i32>,
    /// Global default container padding
    #[serde(skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "schemars", schemars(extend("default" = defaults::DEFAULT_CONTAINER_PADDING)))]
    pub default_container_padding: Option<i32>,
    /// Per-layout default options and rules, keyed by layout name.
    /// Applied as fallback when a workspace does not define its own layout_options or layout_options_rules.
    /// If a workspace defines either setting, all global defaults for that layout are completely replaced.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub layout_defaults: Option<HashMap<DefaultLayout, LayoutDefaultEntry>>,
    /// Monitor and workspace configurations
    #[serde(skip_serializing_if = "Option::is_none")]
    pub monitors: Option<Vec<MonitorConfig>>,
    /// Which Windows signal to use when hiding windows
    #[serde(skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "schemars", schemars(extend("default" = HidingBehaviour::Cloak)))]
    #[cfg_attr(feature = "schemars", schemars(extend("x-komorebi-platforms" = ["windows"])))]
    pub window_hiding_behaviour: Option<HidingBehaviour>,
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
    /// Identify applications which should be managed despite not reporting titles to the system
    #[serde(skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "schemars", schemars(extend("x-komorebi-platforms" = ["kde", "macos"])))]
    pub titleless_applications: Option<Vec<String>>,
    /// Identify applications which use native tabs for special handling
    #[serde(skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "schemars", schemars(extend("x-komorebi-platforms" = ["kde", "macos"])))]
    pub tabbed_applications: Option<Vec<String>>,
    /// Identify border overflow applications
    #[serde(skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "schemars", schemars(extend("x-komorebi-platforms" = ["windows"])))]
    pub border_overflow_applications: Option<Vec<MatchingRule>>,
    /// Identify tray and multi-window applications
    #[serde(skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "schemars", schemars(extend("x-komorebi-platforms" = ["windows"])))]
    pub tray_and_multi_window_applications: Option<Vec<MatchingRule>>,
    /// Identify applications that have the `WS_EX_LAYERED` extended window style
    #[serde(skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "schemars", schemars(extend("x-komorebi-platforms" = ["windows"])))]
    pub layered_applications: Option<Vec<MatchingRule>>,
    /// Identify applications that send `EVENT_OBJECT_NAMECHANGE` on launch (very rare)
    #[serde(skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "schemars", schemars(extend("x-komorebi-platforms" = ["windows"])))]
    pub object_name_change_applications: Option<Vec<MatchingRule>>,
    /// Do not process `EVENT_OBJECT_NAMECHANGE` events as Show events for identified applications matching these title regexes
    #[serde(skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "schemars", schemars(extend("x-komorebi-platforms" = ["windows"])))]
    pub object_name_change_title_ignore_list: Option<Vec<String>>,
    /// Set monitor index preferences
    #[serde(skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "schemars", schemars(extend("x-komorebi-platforms" = ["windows"])))]
    pub monitor_index_preferences: Option<HashMap<usize, Rect>>,
    /// Set display index preferences
    #[serde(skip_serializing_if = "Option::is_none")]
    pub display_index_preferences: Option<HashMap<usize, String>>,
    /// Stackbar configuration options
    #[serde(skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "schemars", schemars(extend("x-komorebi-platforms" = ["windows"])))]
    pub stackbar: Option<StackbarConfig>,
    /// Animations configuration options
    #[serde(skip_serializing_if = "Option::is_none")]
    pub animation: Option<AnimationsConfig>,
    /// Theme configuration options
    ///
    /// If a theme is specified, `border_colours` will have no effect
    #[serde(skip_serializing_if = "Option::is_none")]
    pub theme: Option<KomorebiTheme>,
    /// Identify applications which are slow to send initial event notifications
    #[serde(skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "schemars", schemars(extend("x-komorebi-platforms" = ["windows"])))]
    pub slow_application_identifiers: Option<Vec<MatchingRule>>,
    /// How long to wait when compensating for slow applications, in milliseconds
    #[serde(skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "schemars", schemars(extend("default" = defaults::SLOW_APPLICATION_COMPENSATION_TIME)))]
    #[cfg_attr(feature = "schemars", schemars(extend("x-komorebi-platforms" = ["windows"])))]
    pub slow_application_compensation_time: Option<u64>,
    /// Komorebi status bar configuration files for multiple instances on different monitors
    // this option is a little special because it is only consumed by komorebic
    #[serde(skip_serializing_if = "Option::is_none")]
    #[serde_as(as = "Option<Vec<ResolvedPathBuf>>")]
    #[cfg_attr(feature = "schemars", schemars(extend("x-komorebi-platforms" = ["windows"])))]
    pub bar_configurations: Option<Vec<PathBuf>>,
    /// HEAVILY DISCOURAGED: Identify applications for which komorebi should forcibly remove title bars
    #[serde(skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "schemars", schemars(extend("x-komorebi-platforms" = ["windows"])))]
    pub remove_titlebar_applications: Option<Vec<MatchingRule>>,
    /// Aspect ratio to resize with when toggling floating mode for a window
    #[serde(skip_serializing_if = "Option::is_none")]
    pub floating_window_aspect_ratio: Option<AspectRatio>,
    /// Which Windows API behaviour to use when manipulating windows
    #[serde(skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "schemars", schemars(extend("default" = WindowHandlingBehaviour::Sync)))]
    #[cfg_attr(feature = "schemars", schemars(extend("x-komorebi-platforms" = ["windows"])))]
    pub window_handling_behaviour: Option<WindowHandlingBehaviour>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
/// Animations configuration options
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
    #[cfg_attr(feature = "schemars", schemars(extend("default" = defaults::ANIMATION_FPS)))]
    pub fps: Option<u64>,
    /// Render movement animations on a GPU-composited ghost surface (recommended).
    /// When false, falls back to the legacy per-frame MoveWindow path.
    #[serde(skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "schemars", schemars(extend("default" = false)))]
    #[cfg_attr(feature = "schemars", schemars(extend("x-komorebi-platforms" = ["windows"])))]
    pub ghost_movement: Option<bool>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
/// Stackbar tabs configuration
pub struct TabsConfig {
    /// Width of a stackbar tab
    #[serde(skip_serializing_if = "Option::is_none")]
    pub width: Option<i32>,
    /// Focused tab text colour
    #[serde(skip_serializing_if = "Option::is_none")]
    pub focused_text: Option<Colour>,
    /// Unfocused tab text colour
    #[serde(skip_serializing_if = "Option::is_none")]
    pub unfocused_text: Option<Colour>,
    /// Tab background colour
    #[serde(skip_serializing_if = "Option::is_none")]
    pub background: Option<Colour>,
    /// Font family
    #[serde(skip_serializing_if = "Option::is_none")]
    pub font_family: Option<String>,
    /// Font size
    #[serde(skip_serializing_if = "Option::is_none")]
    pub font_size: Option<i32>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
/// Stackbar configuration
pub struct StackbarConfig {
    /// Stackbar height
    #[serde(skip_serializing_if = "Option::is_none")]
    pub height: Option<i32>,
    /// Stackbar label
    #[serde(skip_serializing_if = "Option::is_none")]
    pub label: Option<StackbarLabel>,
    /// Stackbar mode
    #[serde(skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "schemars", schemars(extend("default" = StackbarMode::Never)))]
    pub mode: Option<StackbarMode>,
    /// Stackbar tab configuration options
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tabs: Option<TabsConfig>,
}

impl StaticConfig {
    /// Reads a configuration from text, as `komorebi.json` is read: comments and trailing commas
    /// are accepted, and a key that is not known is ignored.
    ///
    /// ```rust
    /// use komorebi_config::static_config::StaticConfig;
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
    ///
    /// # Errors
    ///
    /// The text is not a configuration.
    pub fn read_raw(raw: &str) -> color_eyre::Result<Self> {
        Ok(serde_json::from_str(raw)?)
    }

    /// Reads the file at `path`, as [`StaticConfig::read_raw`] reads text
    ///
    /// # Errors
    ///
    /// The file cannot be read, or it is not a configuration.
    pub fn read(path: &std::path::Path) -> color_eyre::Result<Self> {
        let content = std::fs::read_to_string(path)?;
        serde_json::from_str(&content).map_err(Into::into)
    }
    /// Prints the features of `raw` that are end-of-life
    pub fn end_of_life(raw: &str) {
        let features = vec![
            "focus_follows_mouse",
            "custom_layout",
            "custom_layout_rules",
        ];

        let mut display = false;

        for feature in features {
            if raw.contains(feature) {
                if !display {
                    display = true;
                    println!("\n\"{feature}\" is now end-of-life");
                } else {
                    println!(r#""{feature}" is now end-of-life"#);
                }
            }
        }

        if display {
            println!(
                "\nEnd-of-life features will not receive any further bug fixes or updates; they should not be used\n"
            )
        }
    }

    /// Prints the keys of `raw` that have been renamed
    pub fn aliases(raw: &str) {
        let mut map = HashMap::new();
        map.insert("border", ["active_window_border"]);
        map.insert("border_width", ["active_window_border_width"]);
        map.insert("border_offset", ["active_window_border_offset"]);
        map.insert("border_colours", ["active_window_border_colours"]);
        map.insert("border_style", ["active_window_border_style"]);
        map.insert("applications.json", ["applications.yaml"]);
        map.insert("ignore_rules", ["float_rules"]);

        let mut display = false;

        for aliases in map.values() {
            for a in aliases {
                if raw.contains(a) {
                    display = true;
                }
            }
        }

        if display {
            println!(
                "\nYour configuration file contains some options that have been renamed or deprecated:\n"
            );
            for (canonical, aliases) in map {
                for alias in aliases {
                    if raw.contains(alias) {
                        println!(r#""{alias}" is now "{canonical}""#);
                    }
                }
            }
        }
    }

    /// Prints the keys and values of `raw` that are deprecated
    pub fn deprecated(raw: &str) {
        let deprecated_options = ["invisible_borders", "border_z_order"];
        let deprecated_variants = vec![
            ("Hide", "window_hiding_behaviour", "Cloak"),
            ("Minimize", "window_hiding_behaviour", "Cloak"),
        ];

        for option in deprecated_options {
            if raw.contains(option) {
                println!(r#""{option}" is deprecated and can be removed"#);
            }
        }

        for (variant, option, recommended) in deprecated_variants {
            if raw.contains(option) && raw.contains(variant) {
                println!(
                    r#"The "{variant}" option for "{option}" is deprecated and can be removed or replaced with "{recommended}""#
                );
            }
        }
    }
}
