use crate::animation::PerAnimationPrefixConfig;
use crate::animation::ANIMATION_DURATION_GLOBAL;
use crate::animation::ANIMATION_DURATION_PER_ANIMATION;
use crate::animation::ANIMATION_ENABLED_GLOBAL;
use crate::animation::ANIMATION_ENABLED_PER_ANIMATION;
use crate::animation::ANIMATION_FPS;
use crate::animation::ANIMATION_STYLE_GLOBAL;
use crate::animation::ANIMATION_STYLE_PER_ANIMATION;
use crate::animation::DEFAULT_ANIMATION_FPS;
use crate::border_manager;
use crate::border_manager::ZOrder;
use crate::border_manager::IMPLEMENTATION;
use crate::border_manager::STYLE;
use crate::colour::Colour;
use crate::core::BorderImplementation;
use crate::core::StackbarLabel;
use crate::core::StackbarMode;
use crate::current_virtual_desktop;
use crate::monitor::Monitor;
use crate::monitor_reconciliator;
use crate::ring::Ring;
use crate::stackbar_manager::STACKBAR_FOCUSED_TEXT_COLOUR;
use crate::stackbar_manager::STACKBAR_FONT_FAMILY;
use crate::stackbar_manager::STACKBAR_FONT_SIZE;
use crate::stackbar_manager::STACKBAR_LABEL;
use crate::stackbar_manager::STACKBAR_MODE;
use crate::stackbar_manager::STACKBAR_TAB_BACKGROUND_COLOUR;
use crate::stackbar_manager::STACKBAR_TAB_HEIGHT;
use crate::stackbar_manager::STACKBAR_TAB_WIDTH;
use crate::stackbar_manager::STACKBAR_UNFOCUSED_TEXT_COLOUR;
use crate::theme_manager;
use crate::transparency_manager;
use crate::window;
use crate::window_manager::WindowManager;
use crate::window_manager_event::WindowManagerEvent;
use crate::windows_api::WindowsApi;
use crate::workspace::Workspace;
use crate::AspectRatio;
use crate::Axis;
use crate::CrossBoundaryBehaviour;
use crate::PredefinedAspectRatio;
use crate::DATA_DIR;
use crate::DEFAULT_CONTAINER_PADDING;
use crate::DEFAULT_WORKSPACE_PADDING;
use crate::DISPLAY_INDEX_PREFERENCES;
use crate::FLOATING_APPLICATIONS;
use crate::FLOATING_WINDOW_TOGGLE_ASPECT_RATIO;
use crate::HIDING_BEHAVIOUR;
use crate::IGNORE_IDENTIFIERS;
use crate::LAYERED_WHITELIST;
use crate::MANAGE_IDENTIFIERS;
use crate::MONITOR_INDEX_PREFERENCES;
use crate::NO_TITLEBAR;
use crate::OBJECT_NAME_CHANGE_ON_LAUNCH;
use crate::OBJECT_NAME_CHANGE_TITLE_IGNORE_LIST;
use crate::REGEX_IDENTIFIERS;
use crate::SLOW_APPLICATION_COMPENSATION_TIME;
use crate::SLOW_APPLICATION_IDENTIFIERS;
use crate::TRANSPARENCY_BLACKLIST;
use crate::TRAY_AND_MULTI_WINDOW_IDENTIFIERS;
use crate::WINDOWS_11;
use crate::WORKSPACE_MATCHING_RULES;

use crate::asc::ApplicationSpecificConfiguration;
use crate::asc::AscApplicationRulesOrSchema;
use crate::config_generation::WorkspaceMatchingRule;
use crate::core::config_generation::ApplicationConfiguration;
use crate::core::config_generation::ApplicationConfigurationGenerator;
use crate::core::config_generation::ApplicationOptions;
use crate::core::config_generation::MatchingRule;
use crate::core::config_generation::MatchingStrategy;
use crate::core::resolve_home_path;
use crate::core::AnimationStyle;
use crate::core::BorderStyle;
use crate::core::DefaultLayout;
use crate::core::FocusFollowsMouseImplementation;
use crate::core::HidingBehaviour;
use crate::core::Layout;
use crate::core::MoveBehaviour;
use crate::core::OperationBehaviour;
use crate::core::Rect;
use crate::core::SocketMessage;
use crate::core::WindowContainerBehaviour;
use crate::core::WindowManagementBehaviour;
use color_eyre::Result;
use crossbeam_channel::Receiver;
use hotwatch::EventKind;
use hotwatch::Hotwatch;
use parking_lot::Mutex;
use regex::Regex;
use schemars::JsonSchema;
use serde::Deserialize;
use serde::Serialize;
use std::collections::HashMap;
use std::collections::HashSet;
use std::io::ErrorKind;
use std::io::Write;
use std::path::PathBuf;
use std::sync::atomic::Ordering;
use std::sync::Arc;
use uds_windows::UnixListener;
use uds_windows::UnixStream;

#[derive(Debug, Serialize, Deserialize, JsonSchema)]
pub struct BorderColours {
    /// Border colour when the container contains a single window
    pub single: Option<Colour>,
    /// Border colour when the container contains multiple windows
    pub stack: Option<Colour>,
    /// Border colour when the container is in monocle mode
    pub monocle: Option<Colour>,
    /// Border colour when the container is in floating mode
    pub floating: Option<Colour>,
    /// Border colour when the container is unfocused
    pub unfocused: Option<Colour>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq)]
pub struct WorkspaceConfig {
    /// Name
    pub name: String,
    /// Layout (default: BSP)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub layout: Option<DefaultLayout>,
    /// END OF LIFE FEATURE: Custom Layout (default: None)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub custom_layout: Option<PathBuf>,
    /// Layout rules in the format of threshold => layout (default: None)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub layout_rules: Option<HashMap<usize, DefaultLayout>>,
    /// END OF LIFE FEATURE: Custom layout rules (default: None)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub custom_layout_rules: Option<HashMap<usize, PathBuf>>,
    /// Container padding (default: global)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub container_padding: Option<i32>,
    /// Container padding (default: global)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub workspace_padding: Option<i32>,
    /// Initial workspace application rules
    #[serde(skip_serializing_if = "Option::is_none")]
    pub initial_workspace_rules: Option<Vec<MatchingRule>>,
    /// Permanent workspace application rules
    #[serde(skip_serializing_if = "Option::is_none")]
    pub workspace_rules: Option<Vec<MatchingRule>>,
    /// Apply this monitor's window-based work area offset (default: true)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub apply_window_based_work_area_offset: Option<bool>,
    /// Determine what happens when a new window is opened (default: Create)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub window_container_behaviour: Option<WindowContainerBehaviour>,
    /// Window container behaviour rules in the format of threshold => behaviour (default: None)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub window_container_behaviour_rules: Option<HashMap<usize, WindowContainerBehaviour>>,
    /// Enable or disable float override, which makes it so every new window opens in floating mode (default: false)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub float_override: Option<bool>,
    /// Specify an axis on which to flip the selected layout (default: None)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub layout_flip: Option<Axis>,
}

impl From<&Workspace> for WorkspaceConfig {
    fn from(value: &Workspace) -> Self {
        let mut layout_rules = HashMap::new();
        for (threshold, layout) in value.layout_rules() {
            match layout {
                Layout::Default(value) => {
                    layout_rules.insert(*threshold, *value);
                }
                Layout::Custom(_) => {}
            }
        }
        let layout_rules = (!layout_rules.is_empty()).then_some(layout_rules);

        let mut window_container_behaviour_rules = HashMap::new();
        for (threshold, behaviour) in value.window_container_behaviour_rules().iter().flatten() {
            window_container_behaviour_rules.insert(*threshold, *behaviour);
        }

        let default_container_padding = DEFAULT_CONTAINER_PADDING.load(Ordering::SeqCst);
        let default_workspace_padding = DEFAULT_WORKSPACE_PADDING.load(Ordering::SeqCst);

        let container_padding = value.container_padding().and_then(|container_padding| {
            if container_padding == default_container_padding {
                None
            } else {
                Option::from(container_padding)
            }
        });

        let workspace_padding = value.workspace_padding().and_then(|workspace_padding| {
            if workspace_padding == default_workspace_padding {
                None
            } else {
                Option::from(workspace_padding)
            }
        });

        Self {
            name: value
                .name()
                .clone()
                .unwrap_or_else(|| String::from("unnamed")),
            layout: value
                .tile()
                .then_some(match value.layout() {
                    Layout::Default(layout) => Option::from(*layout),
                    Layout::Custom(_) => None,
                })
                .flatten(),
            custom_layout: value
                .workspace_config()
                .as_ref()
                .and_then(|c| c.custom_layout.clone()),
            layout_rules,
            custom_layout_rules: value
                .workspace_config()
                .as_ref()
                .and_then(|c| c.custom_layout_rules.clone()),
            container_padding,
            workspace_padding,
            initial_workspace_rules: value
                .workspace_config()
                .as_ref()
                .and_then(|c| c.initial_workspace_rules.clone()),
            workspace_rules: value
                .workspace_config()
                .as_ref()
                .and_then(|c| c.workspace_rules.clone()),
            apply_window_based_work_area_offset: Some(value.apply_window_based_work_area_offset()),
            window_container_behaviour: *value.window_container_behaviour(),
            window_container_behaviour_rules: Option::from(window_container_behaviour_rules),
            float_override: *value.float_override(),
            layout_flip: value.layout_flip(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct MonitorConfig {
    /// Workspace configurations
    pub workspaces: Vec<WorkspaceConfig>,
    /// Monitor-specific work area offset (default: None)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub work_area_offset: Option<Rect>,
    /// Window based work area offset (default: None)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub window_based_work_area_offset: Option<Rect>,
    /// Open window limit after which the window based work area offset will no longer be applied (default: 1)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub window_based_work_area_offset_limit: Option<isize>,
}

impl From<&Monitor> for MonitorConfig {
    fn from(value: &Monitor) -> Self {
        let mut workspaces = vec![];
        for w in value.workspaces() {
            workspaces.push(WorkspaceConfig::from(w));
        }

        Self {
            workspaces,
            work_area_offset: value.work_area_offset(),
            window_based_work_area_offset: value.window_based_work_area_offset(),
            window_based_work_area_offset_limit: Some(value.window_based_work_area_offset_limit()),
        }
    }
}

#[derive(Debug, Serialize, Deserialize, JsonSchema)]
/// The `komorebi.json` static configuration file reference for `v0.1.34`
pub struct StaticConfig {
    /// DEPRECATED from v0.1.22: no longer required
    #[serde(skip_serializing_if = "Option::is_none")]
    pub invisible_borders: Option<Rect>,
    /// DISCOURAGED: Minimum width for a window to be eligible for tiling
    #[serde(skip_serializing_if = "Option::is_none")]
    pub minimum_window_width: Option<i32>,
    /// DISCOURAGED: Minimum height for a window to be eligible for tiling
    #[serde(skip_serializing_if = "Option::is_none")]
    pub minimum_window_height: Option<i32>,
    /// Delta to resize windows by (default 50)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub resize_delta: Option<i32>,
    /// Determine what happens when a new window is opened (default: Create)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub window_container_behaviour: Option<WindowContainerBehaviour>,
    /// Enable or disable float override, which makes it so every new window opens in floating mode
    /// (default: false)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub float_override: Option<bool>,
    /// Determine what happens when a window is moved across a monitor boundary (default: Swap)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cross_monitor_move_behaviour: Option<MoveBehaviour>,
    /// Determine what happens when an action is called on a window at a monitor boundary (default: Monitor)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cross_boundary_behaviour: Option<CrossBoundaryBehaviour>,
    /// Determine what happens when commands are sent while an unmanaged window is in the foreground (default: Op)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub unmanaged_window_operation_behaviour: Option<OperationBehaviour>,
    /// END OF LIFE FEATURE: Use https://github.com/LGUG2Z/masir instead
    #[serde(skip_serializing_if = "Option::is_none")]
    pub focus_follows_mouse: Option<FocusFollowsMouseImplementation>,
    /// Enable or disable mouse follows focus (default: true)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub mouse_follows_focus: Option<bool>,
    /// Path to applications.json from komorebi-application-specific-configurations (default: None)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub app_specific_configuration_path: Option<PathBuf>,
    /// Width of the window border (default: 8)
    #[serde(skip_serializing_if = "Option::is_none")]
    #[serde(alias = "active_window_border_width")]
    pub border_width: Option<i32>,
    /// Offset of the window border (default: -1)
    #[serde(skip_serializing_if = "Option::is_none")]
    #[serde(alias = "active_window_border_offset")]
    pub border_offset: Option<i32>,
    /// Display an active window border (default: false)
    #[serde(skip_serializing_if = "Option::is_none")]
    #[serde(alias = "active_window_border")]
    pub border: Option<bool>,
    /// Active window border colours for different container types
    #[serde(skip_serializing_if = "Option::is_none")]
    #[serde(alias = "active_window_border_colours")]
    pub border_colours: Option<BorderColours>,
    /// Active window border style (default: System)
    #[serde(skip_serializing_if = "Option::is_none")]
    #[serde(alias = "active_window_border_style")]
    pub border_style: Option<BorderStyle>,
    /// DEPRECATED from v0.1.31: no longer required
    #[serde(skip_serializing_if = "Option::is_none")]
    pub border_z_order: Option<ZOrder>,
    /// Active window border implementation (default: Komorebi)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub border_implementation: Option<BorderImplementation>,
    /// Add transparency to unfocused windows (default: false)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub transparency: Option<bool>,
    /// Alpha value for unfocused window transparency [[0-255]] (default: 200)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub transparency_alpha: Option<u8>,
    /// Individual window transparency ignore rules
    #[serde(skip_serializing_if = "Option::is_none")]
    pub transparency_ignore_rules: Option<Vec<MatchingRule>>,
    /// Global default workspace padding (default: 10)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub default_workspace_padding: Option<i32>,
    /// Global default container padding (default: 10)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub default_container_padding: Option<i32>,
    /// Monitor and workspace configurations
    #[serde(skip_serializing_if = "Option::is_none")]
    pub monitors: Option<Vec<MonitorConfig>>,
    /// Which Windows signal to use when hiding windows (default: Cloak)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub window_hiding_behaviour: Option<HidingBehaviour>,
    /// Global work area (space used for tiling) offset (default: None)
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
    /// Identify border overflow applications
    #[serde(skip_serializing_if = "Option::is_none")]
    pub border_overflow_applications: Option<Vec<MatchingRule>>,
    /// Identify tray and multi-window applications
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tray_and_multi_window_applications: Option<Vec<MatchingRule>>,
    /// Identify applications that have the WS_EX_LAYERED extended window style
    #[serde(skip_serializing_if = "Option::is_none")]
    pub layered_applications: Option<Vec<MatchingRule>>,
    /// Identify applications that send EVENT_OBJECT_NAMECHANGE on launch (very rare)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub object_name_change_applications: Option<Vec<MatchingRule>>,
    /// Do not process EVENT_OBJECT_NAMECHANGE events as Show events for identified applications matching these title regexes
    #[serde(skip_serializing_if = "Option::is_none")]
    pub object_name_change_title_ignore_list: Option<Vec<String>>,
    /// Set monitor index preferences
    #[serde(skip_serializing_if = "Option::is_none")]
    pub monitor_index_preferences: Option<HashMap<usize, Rect>>,
    /// Set display index preferences
    #[serde(skip_serializing_if = "Option::is_none")]
    pub display_index_preferences: Option<HashMap<usize, String>>,
    /// Stackbar configuration options
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stackbar: Option<StackbarConfig>,
    /// Animations configuration options
    #[serde(skip_serializing_if = "Option::is_none")]
    pub animation: Option<AnimationsConfig>,
    /// Theme configuration options
    #[serde(skip_serializing_if = "Option::is_none")]
    pub theme: Option<KomorebiTheme>,
    /// Identify applications which are slow to send initial event notifications
    #[serde(skip_serializing_if = "Option::is_none")]
    pub slow_application_identifiers: Option<Vec<MatchingRule>>,
    /// How long to wait when compensating for slow applications, in milliseconds (default: 20)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub slow_application_compensation_time: Option<u64>,
    /// Komorebi status bar configuration files for multiple instances on different monitors
    #[serde(skip_serializing_if = "Option::is_none")]
    // this option is a little special because it is only consumed by komorebic
    pub bar_configurations: Option<Vec<PathBuf>>,
    /// HEAVILY DISCOURAGED: Identify applications for which komorebi should forcibly remove title bars
    #[serde(skip_serializing_if = "Option::is_none")]
    pub remove_titlebar_applications: Option<Vec<MatchingRule>>,
    /// Aspect ratio to resize with when toggling floating mode for a window
    #[serde(skip_serializing_if = "Option::is_none")]
    pub floating_window_aspect_ratio: Option<AspectRatio>,
}

#[derive(Debug, Serialize, Deserialize, JsonSchema)]
pub struct AnimationsConfig {
    /// Enable or disable animations (default: false)
    enabled: PerAnimationPrefixConfig<bool>,
    /// Set the animation duration in ms (default: 250)
    duration: Option<PerAnimationPrefixConfig<u64>>,
    /// Set the animation style (default: Linear)
    style: Option<PerAnimationPrefixConfig<AnimationStyle>>,
    /// Set the animation FPS (default: 60)
    fps: Option<u64>,
}
#[derive(Copy, Clone, Debug, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "palette")]
pub enum KomorebiTheme {
    /// A theme from catppuccin-egui
    Catppuccin {
        /// Name of the Catppuccin theme (theme previews: https://github.com/catppuccin/catppuccin)
        name: komorebi_themes::Catppuccin,
        /// Border colour when the container contains a single window (default: Blue)
        single_border: Option<komorebi_themes::CatppuccinValue>,
        /// Border colour when the container contains multiple windows (default: Green)
        stack_border: Option<komorebi_themes::CatppuccinValue>,
        /// Border colour when the container is in monocle mode (default: Pink)
        monocle_border: Option<komorebi_themes::CatppuccinValue>,
        /// Border colour when the window is floating (default: Yellow)
        floating_border: Option<komorebi_themes::CatppuccinValue>,
        /// Border colour when the container is unfocused (default: Base)
        unfocused_border: Option<komorebi_themes::CatppuccinValue>,
        /// Stackbar focused tab text colour (default: Green)
        stackbar_focused_text: Option<komorebi_themes::CatppuccinValue>,
        /// Stackbar unfocused tab text colour (default: Text)
        stackbar_unfocused_text: Option<komorebi_themes::CatppuccinValue>,
        /// Stackbar tab background colour (default: Base)
        stackbar_background: Option<komorebi_themes::CatppuccinValue>,
        /// Komorebi status bar accent (default: Blue)
        bar_accent: Option<komorebi_themes::CatppuccinValue>,
    },
    /// A theme from base16-egui-themes
    Base16 {
        /// Name of the Base16 theme (theme previews: https://tinted-theming.github.io/base16-gallery)
        name: komorebi_themes::Base16,
        /// Border colour when the container contains a single window (default: Base0D)
        single_border: Option<komorebi_themes::Base16Value>,
        /// Border colour when the container contains multiple windows (default: Base0B)
        stack_border: Option<komorebi_themes::Base16Value>,
        /// Border colour when the container is in monocle mode (default: Base0F)
        monocle_border: Option<komorebi_themes::Base16Value>,
        /// Border colour when the window is floating (default: Base09)
        floating_border: Option<komorebi_themes::Base16Value>,
        /// Border colour when the container is unfocused (default: Base01)
        unfocused_border: Option<komorebi_themes::Base16Value>,
        /// Stackbar focused tab text colour (default: Base0B)
        stackbar_focused_text: Option<komorebi_themes::Base16Value>,
        /// Stackbar unfocused tab text colour (default: Base05)
        stackbar_unfocused_text: Option<komorebi_themes::Base16Value>,
        /// Stackbar tab background colour (default: Base01)
        stackbar_background: Option<komorebi_themes::Base16Value>,
        /// Komorebi status bar accent (default: Base0D)
        bar_accent: Option<komorebi_themes::Base16Value>,
    },
}

impl StaticConfig {
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
            println!("\nEnd-of-life features will not receive any further bug fixes or updates; they should not be used\n")
        }
    }

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
            println!("\nYour configuration file contains some options that have been renamed or deprecated:\n");
            for (canonical, aliases) in map {
                for alias in aliases {
                    if raw.contains(alias) {
                        println!(r#""{alias}" is now "{canonical}""#);
                    }
                }
            }
        }
    }

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

#[derive(Debug, Serialize, Deserialize, JsonSchema)]
pub struct TabsConfig {
    /// Width of a stackbar tab
    width: Option<i32>,
    /// Focused tab text colour
    focused_text: Option<Colour>,
    /// Unfocused tab text colour
    unfocused_text: Option<Colour>,
    /// Tab background colour
    background: Option<Colour>,
    /// Font family
    font_family: Option<String>,
    /// Font size
    font_size: Option<i32>,
}

#[derive(Debug, Serialize, Deserialize, JsonSchema)]
pub struct StackbarConfig {
    /// Stackbar height
    pub height: Option<i32>,
    /// Stackbar label
    pub label: Option<StackbarLabel>,
    /// Stackbar mode
    pub mode: Option<StackbarMode>,
    /// Stackbar tab configuration options
    pub tabs: Option<TabsConfig>,
}

impl From<&WindowManager> for StaticConfig {
    #[allow(clippy::too_many_lines)]
    fn from(value: &WindowManager) -> Self {
        let mut monitors = vec![];
        for m in value.monitors() {
            monitors.push(MonitorConfig::from(m));
        }

        let border_colours = if border_manager::FOCUSED.load(Ordering::SeqCst) == 0 {
            None
        } else {
            Option::from(BorderColours {
                single: Option::from(Colour::from(border_manager::FOCUSED.load(Ordering::SeqCst))),
                stack: Option::from(Colour::from(border_manager::STACK.load(Ordering::SeqCst))),
                monocle: Option::from(Colour::from(border_manager::MONOCLE.load(Ordering::SeqCst))),
                floating: Option::from(Colour::from(
                    border_manager::FLOATING.load(Ordering::SeqCst),
                )),
                unfocused: Option::from(Colour::from(
                    border_manager::UNFOCUSED.load(Ordering::SeqCst),
                )),
            })
        };

        Self {
            invisible_borders: None,
            resize_delta: Option::from(value.resize_delta),
            window_container_behaviour: Option::from(
                value.window_management_behaviour.current_behaviour,
            ),
            float_override: Option::from(value.window_management_behaviour.float_override),
            cross_monitor_move_behaviour: Option::from(value.cross_monitor_move_behaviour),
            cross_boundary_behaviour: Option::from(value.cross_boundary_behaviour),
            unmanaged_window_operation_behaviour: Option::from(
                value.unmanaged_window_operation_behaviour,
            ),
            minimum_window_height: Some(window::MINIMUM_HEIGHT.load(Ordering::SeqCst)),
            minimum_window_width: Some(window::MINIMUM_WIDTH.load(Ordering::SeqCst)),
            focus_follows_mouse: value.focus_follows_mouse,
            mouse_follows_focus: Option::from(value.mouse_follows_focus),
            app_specific_configuration_path: None,
            border_width: Option::from(border_manager::BORDER_WIDTH.load(Ordering::SeqCst)),
            border_offset: Option::from(border_manager::BORDER_OFFSET.load(Ordering::SeqCst)),
            border: Option::from(border_manager::BORDER_ENABLED.load(Ordering::SeqCst)),
            border_colours,
            transparency: Option::from(
                transparency_manager::TRANSPARENCY_ENABLED.load(Ordering::SeqCst),
            ),
            transparency_alpha: Option::from(
                transparency_manager::TRANSPARENCY_ALPHA.load(Ordering::SeqCst),
            ),
            transparency_ignore_rules: None,
            border_style: Option::from(STYLE.load()),
            border_z_order: None,
            border_implementation: Option::from(IMPLEMENTATION.load()),
            default_workspace_padding: Option::from(
                DEFAULT_WORKSPACE_PADDING.load(Ordering::SeqCst),
            ),
            default_container_padding: Option::from(
                DEFAULT_CONTAINER_PADDING.load(Ordering::SeqCst),
            ),
            monitors: Option::from(monitors),
            window_hiding_behaviour: Option::from(*HIDING_BEHAVIOUR.lock()),
            global_work_area_offset: value.work_area_offset,
            ignore_rules: None,
            floating_applications: None,
            manage_rules: None,
            border_overflow_applications: None,
            tray_and_multi_window_applications: None,
            layered_applications: None,
            object_name_change_applications: Option::from(
                OBJECT_NAME_CHANGE_ON_LAUNCH.lock().clone(),
            ),
            object_name_change_title_ignore_list: Option::from(
                OBJECT_NAME_CHANGE_TITLE_IGNORE_LIST
                    .lock()
                    .clone()
                    .iter()
                    .map(|r| r.to_string())
                    .collect::<Vec<_>>(),
            ),
            monitor_index_preferences: Option::from(MONITOR_INDEX_PREFERENCES.lock().clone()),
            display_index_preferences: Option::from(DISPLAY_INDEX_PREFERENCES.lock().clone()),
            stackbar: None,
            animation: None,
            theme: None,
            slow_application_compensation_time: Option::from(
                SLOW_APPLICATION_COMPENSATION_TIME.load(Ordering::SeqCst),
            ),
            slow_application_identifiers: Option::from(SLOW_APPLICATION_IDENTIFIERS.lock().clone()),
            bar_configurations: None,
            remove_titlebar_applications: Option::from(NO_TITLEBAR.lock().clone()),
            floating_window_aspect_ratio: Option::from(*FLOATING_WINDOW_TOGGLE_ASPECT_RATIO.lock()),
        }
    }
}

impl StaticConfig {
    #[allow(clippy::cognitive_complexity, clippy::too_many_lines)]
    fn apply_globals(&mut self) -> Result<()> {
        *FLOATING_WINDOW_TOGGLE_ASPECT_RATIO.lock() = self
            .floating_window_aspect_ratio
            .unwrap_or(AspectRatio::Predefined(PredefinedAspectRatio::Standard));

        if let Some(monitor_index_preferences) = &self.monitor_index_preferences {
            let mut preferences = MONITOR_INDEX_PREFERENCES.lock();
            preferences.clone_from(monitor_index_preferences);
        }

        if let Some(display_index_preferences) = &self.display_index_preferences {
            let mut preferences = DISPLAY_INDEX_PREFERENCES.lock();
            preferences.clone_from(display_index_preferences);
        }

        if let Some(behaviour) = self.window_hiding_behaviour {
            let mut window_hiding_behaviour = HIDING_BEHAVIOUR.lock();
            *window_hiding_behaviour = behaviour;
        }

        if let Some(height) = self.minimum_window_height {
            window::MINIMUM_HEIGHT.store(height, Ordering::SeqCst);
        }

        if let Some(width) = self.minimum_window_width {
            window::MINIMUM_WIDTH.store(width, Ordering::SeqCst);
        }

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

        border_manager::BORDER_WIDTH.store(self.border_width.unwrap_or(8), Ordering::SeqCst);
        border_manager::BORDER_OFFSET.store(self.border_offset.unwrap_or(-1), Ordering::SeqCst);

        if let Some(enabled) = &self.border {
            border_manager::BORDER_ENABLED.store(*enabled, Ordering::SeqCst);
        }

        if let Some(colours) = &self.border_colours {
            if let Some(single) = colours.single {
                border_manager::FOCUSED.store(u32::from(single), Ordering::SeqCst);
            }

            if let Some(stack) = colours.stack {
                border_manager::STACK.store(u32::from(stack), Ordering::SeqCst);
            }

            if let Some(monocle) = colours.monocle {
                border_manager::MONOCLE.store(u32::from(monocle), Ordering::SeqCst);
            }

            if let Some(floating) = colours.floating {
                border_manager::FLOATING.store(u32::from(floating), Ordering::SeqCst);
            }

            if let Some(unfocused) = colours.unfocused {
                border_manager::UNFOCUSED.store(u32::from(unfocused), Ordering::SeqCst);
            }
        }

        STYLE.store(self.border_style.unwrap_or_default());

        if !*WINDOWS_11
            && matches!(
                self.border_implementation.unwrap_or_default(),
                BorderImplementation::Windows
            )
        {
            tracing::error!(
                "BorderImplementation::Windows is only supported on Windows 11 and above"
            );
        } else {
            IMPLEMENTATION.store(self.border_implementation.unwrap_or_default());
            match IMPLEMENTATION.load() {
                BorderImplementation::Komorebi => {
                    border_manager::destroy_all_borders()?;
                }
                BorderImplementation::Windows => {
                    // TODO: figure out how to call wm.remove_all_accents here
                }
            }

            border_manager::send_notification(None);
        }

        transparency_manager::TRANSPARENCY_ENABLED
            .store(self.transparency.unwrap_or(false), Ordering::SeqCst);
        transparency_manager::TRANSPARENCY_ALPHA
            .store(self.transparency_alpha.unwrap_or(200), Ordering::SeqCst);

        let mut ignore_identifiers = IGNORE_IDENTIFIERS.lock();
        let mut regex_identifiers = REGEX_IDENTIFIERS.lock();
        let mut manage_identifiers = MANAGE_IDENTIFIERS.lock();
        let mut tray_and_multi_window_identifiers = TRAY_AND_MULTI_WINDOW_IDENTIFIERS.lock();
        let mut object_name_change_identifiers = OBJECT_NAME_CHANGE_ON_LAUNCH.lock();
        let mut object_name_change_title_ignore_list = OBJECT_NAME_CHANGE_TITLE_IGNORE_LIST.lock();
        let mut layered_identifiers = LAYERED_WHITELIST.lock();
        let mut transparency_blacklist = TRANSPARENCY_BLACKLIST.lock();
        let mut slow_application_identifiers = SLOW_APPLICATION_IDENTIFIERS.lock();
        let mut floating_applications = FLOATING_APPLICATIONS.lock();
        let mut no_titlebar_applications = NO_TITLEBAR.lock();

        if let Some(rules) = &mut self.ignore_rules {
            populate_rules(rules, &mut ignore_identifiers, &mut regex_identifiers)?;
        }

        if let Some(rules) = &mut self.floating_applications {
            populate_rules(rules, &mut floating_applications, &mut regex_identifiers)?;
        }

        if let Some(rules) = &mut self.manage_rules {
            populate_rules(rules, &mut manage_identifiers, &mut regex_identifiers)?;
        }

        if let Some(rules) = &mut self.object_name_change_applications {
            populate_rules(
                rules,
                &mut object_name_change_identifiers,
                &mut regex_identifiers,
            )?;
        }

        if let Some(regexes) = &mut self.object_name_change_title_ignore_list {
            let mut updated = vec![];
            for r in regexes {
                if let Ok(regex) = Regex::new(r) {
                    updated.push(regex);
                }
            }

            *object_name_change_title_ignore_list = updated;
        }

        if let Some(rules) = &mut self.layered_applications {
            populate_rules(rules, &mut layered_identifiers, &mut regex_identifiers)?;
        }

        if let Some(rules) = &mut self.tray_and_multi_window_applications {
            populate_rules(
                rules,
                &mut tray_and_multi_window_identifiers,
                &mut regex_identifiers,
            )?;
        }

        if let Some(rules) = &mut self.transparency_ignore_rules {
            populate_rules(rules, &mut transparency_blacklist, &mut regex_identifiers)?;
        }

        if let Some(rules) = &mut self.slow_application_identifiers {
            populate_rules(
                rules,
                &mut slow_application_identifiers,
                &mut regex_identifiers,
            )?;
        }

        if let Some(rules) = &mut self.remove_titlebar_applications {
            populate_rules(rules, &mut no_titlebar_applications, &mut regex_identifiers)?;
        }

        if let Some(stackbar) = &self.stackbar {
            if let Some(height) = &stackbar.height {
                STACKBAR_TAB_HEIGHT.store(*height, Ordering::SeqCst);
            }

            if let Some(label) = &stackbar.label {
                STACKBAR_LABEL.store(*label);
            }

            if let Some(mode) = &stackbar.mode {
                STACKBAR_MODE.store(*mode);
            }

            #[allow(clippy::assigning_clones)]
            if let Some(tabs) = &stackbar.tabs {
                if let Some(background) = &tabs.background {
                    STACKBAR_TAB_BACKGROUND_COLOUR.store((*background).into(), Ordering::SeqCst);
                }

                if let Some(colour) = &tabs.focused_text {
                    STACKBAR_FOCUSED_TEXT_COLOUR.store((*colour).into(), Ordering::SeqCst);
                }

                if let Some(colour) = &tabs.unfocused_text {
                    STACKBAR_UNFOCUSED_TEXT_COLOUR.store((*colour).into(), Ordering::SeqCst);
                }

                if let Some(width) = &tabs.width {
                    STACKBAR_TAB_WIDTH.store(*width, Ordering::SeqCst);
                }

                STACKBAR_FONT_SIZE.store(tabs.font_size.unwrap_or(0), Ordering::SeqCst);
                *STACKBAR_FONT_FAMILY.lock() = tabs.font_family.clone();
            }
        }

        if let Some(theme) = &self.theme {
            theme_manager::send_notification(*theme);
        }

        if let Some(path) = &self.app_specific_configuration_path {
            match path.extension() {
                None => {}
                Some(ext) => match ext.to_string_lossy().to_string().as_str() {
                    "yaml" => {
                        tracing::info!("loading applications.yaml from: {}", path.display());
                        let path = resolve_home_path(path)?;
                        let content = std::fs::read_to_string(path)?;
                        let asc = ApplicationConfigurationGenerator::load(&content)?;

                        for mut entry in asc {
                            if let Some(rules) = &mut entry.ignore_identifiers {
                                populate_rules(
                                    rules,
                                    &mut ignore_identifiers,
                                    &mut regex_identifiers,
                                )?;
                            }

                            if let Some(ref options) = entry.options {
                                let options = options.clone();
                                for o in options {
                                    match o {
                                        ApplicationOptions::ObjectNameChange => {
                                            populate_option(
                                                &mut entry,
                                                &mut object_name_change_identifiers,
                                                &mut regex_identifiers,
                                            )?;
                                        }
                                        ApplicationOptions::Layered => {
                                            populate_option(
                                                &mut entry,
                                                &mut layered_identifiers,
                                                &mut regex_identifiers,
                                            )?;
                                        }
                                        ApplicationOptions::TrayAndMultiWindow => {
                                            populate_option(
                                                &mut entry,
                                                &mut tray_and_multi_window_identifiers,
                                                &mut regex_identifiers,
                                            )?;
                                        }
                                        ApplicationOptions::Force => {
                                            populate_option(
                                                &mut entry,
                                                &mut manage_identifiers,
                                                &mut regex_identifiers,
                                            )?;
                                        }
                                        ApplicationOptions::BorderOverflow => {} // deprecated
                                    }
                                }
                            }
                        }
                    }
                    "json" => {
                        tracing::info!("loading applications.json from: {}", path.display());
                        let path = resolve_home_path(path)?;
                        let mut asc = ApplicationSpecificConfiguration::load(&path)?;

                        for entry in asc.values_mut() {
                            match entry {
                                AscApplicationRulesOrSchema::Schema(_) => {}
                                AscApplicationRulesOrSchema::AscApplicationRules(entry) => {
                                    if let Some(rules) = &mut entry.ignore {
                                        populate_rules(
                                            rules,
                                            &mut ignore_identifiers,
                                            &mut regex_identifiers,
                                        )?;
                                    }

                                    if let Some(rules) = &mut entry.manage {
                                        populate_rules(
                                            rules,
                                            &mut manage_identifiers,
                                            &mut regex_identifiers,
                                        )?;
                                    }

                                    if let Some(rules) = &mut entry.floating {
                                        populate_rules(
                                            rules,
                                            &mut floating_applications,
                                            &mut regex_identifiers,
                                        )?;
                                    }

                                    if let Some(rules) = &mut entry.transparency_ignore {
                                        populate_rules(
                                            rules,
                                            &mut transparency_blacklist,
                                            &mut regex_identifiers,
                                        )?;
                                    }

                                    if let Some(rules) = &mut entry.tray_and_multi_window {
                                        populate_rules(
                                            rules,
                                            &mut tray_and_multi_window_identifiers,
                                            &mut regex_identifiers,
                                        )?;
                                    }

                                    if let Some(rules) = &mut entry.layered {
                                        populate_rules(
                                            rules,
                                            &mut layered_identifiers,
                                            &mut regex_identifiers,
                                        )?;
                                    }

                                    if let Some(rules) = &mut entry.object_name_change {
                                        populate_rules(
                                            rules,
                                            &mut object_name_change_identifiers,
                                            &mut regex_identifiers,
                                        )?;
                                    }

                                    if let Some(rules) = &mut entry.slow_application {
                                        populate_rules(
                                            rules,
                                            &mut slow_application_identifiers,
                                            &mut regex_identifiers,
                                        )?;
                                    }
                                }
                            }
                        }
                    }
                    _ => {}
                },
            }
        }

        Ok(())
    }

    pub fn read_raw(raw: &str) -> Result<Self> {
        Ok(serde_json::from_str(raw)?)
    }

    pub fn read(path: &PathBuf) -> Result<Self> {
        let content = std::fs::read_to_string(path)?;
        let mut value: Self = serde_json::from_str(&content)?;

        if let Some(path) = &mut value.app_specific_configuration_path {
            *path = resolve_home_path(&*path)?;
        }

        if let Some(monitors) = &mut value.monitors {
            for m in monitors {
                for w in &mut m.workspaces {
                    if let Some(path) = &mut w.custom_layout {
                        *path = resolve_home_path(&*path)?;
                    }

                    if let Some(map) = &mut w.custom_layout_rules {
                        for path in map.values_mut() {
                            *path = resolve_home_path(&*path)?;
                        }
                    }
                }
            }
        }

        if let Some(bar_configurations) = &mut value.bar_configurations {
            for path in bar_configurations {
                *path = resolve_home_path(&*path)?;
            }
        }

        Ok(value)
    }

    #[allow(clippy::too_many_lines)]
    pub fn preload(
        path: &PathBuf,
        incoming: Receiver<WindowManagerEvent>,
        unix_listener: Option<UnixListener>,
    ) -> Result<WindowManager> {
        let mut value = Self::read(path)?;
        value.apply_globals()?;

        let listener = match unix_listener {
            Some(listener) => listener,
            None => {
                let socket = DATA_DIR.join("komorebi.sock");

                match std::fs::remove_file(&socket) {
                    Ok(()) => {}
                    Err(error) => match error.kind() {
                        // Doing this because ::exists() doesn't work reliably on Windows via IntelliJ
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
            incoming_events: incoming,
            command_listener: listener,
            is_paused: false,
            virtual_desktop_id: current_virtual_desktop(),
            work_area_offset: value.global_work_area_offset,
            window_management_behaviour: WindowManagementBehaviour {
                current_behaviour: value
                    .window_container_behaviour
                    .unwrap_or(WindowContainerBehaviour::Create),
                float_override: value.float_override.unwrap_or_default(),
            },
            cross_monitor_move_behaviour: value
                .cross_monitor_move_behaviour
                .unwrap_or(MoveBehaviour::Swap),
            cross_boundary_behaviour: value
                .cross_boundary_behaviour
                .unwrap_or(CrossBoundaryBehaviour::Monitor),
            unmanaged_window_operation_behaviour: value
                .unmanaged_window_operation_behaviour
                .unwrap_or(OperationBehaviour::Op),
            resize_delta: value.resize_delta.unwrap_or(50),
            focus_follows_mouse: value.focus_follows_mouse,
            mouse_follows_focus: value.mouse_follows_focus.unwrap_or(true),
            hotwatch: Hotwatch::new()?,
            has_pending_raise_op: false,
            pending_move_op: Arc::new(None),
            already_moved_window_handles: Arc::new(Mutex::new(HashSet::new())),
            uncloack_to_ignore: 0,
        };

        match value.focus_follows_mouse {
            None => WindowsApi::disable_focus_follows_mouse()?,
            Some(FocusFollowsMouseImplementation::Windows) => {
                WindowsApi::enable_focus_follows_mouse()?;
            }
            Some(FocusFollowsMouseImplementation::Komorebi) => {}
        };

        let bytes = SocketMessage::ReloadStaticConfiguration(path.clone()).as_bytes()?;

        wm.hotwatch.watch(path, move |event| match event.kind {
            // Editing in Notepad sends a NoticeWrite while editing in (Neo)Vim sends
            // a NoticeRemove, presumably because of the use of swap files?
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

    pub fn postload(path: &PathBuf, wm: &Arc<Mutex<WindowManager>>) -> Result<()> {
        let value = Self::read(path)?;
        let mut wm = wm.lock();

        let configs_with_preference: Vec<_> =
            DISPLAY_INDEX_PREFERENCES.lock().keys().copied().collect();
        let mut configs_used = Vec::new();

        let mut workspace_matching_rules = WORKSPACE_MATCHING_RULES.lock();
        workspace_matching_rules.clear();
        drop(workspace_matching_rules);

        for (i, monitor) in wm.monitors_mut().iter_mut().enumerate() {
            let preferred_config_idx = {
                let display_index_preferences = DISPLAY_INDEX_PREFERENCES.lock();
                let c_idx = display_index_preferences
                    .iter()
                    .find_map(|(c_idx, m_id)| (monitor.device_id() == m_id).then_some(*c_idx));
                c_idx
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
                // Check if this monitor config is the preferred config for this monitor and store
                // a copy of the config on the monitor cache if it is.
                if idx == preferred_config_idx {
                    monitor_reconciliator::insert_in_monitor_cache(
                        monitor.device_id(),
                        monitor_config.clone(),
                    );
                }

                if let Some(used_config_idx) = idx {
                    configs_used.push(used_config_idx);
                }

                monitor.ensure_workspace_count(monitor_config.workspaces.len());
                monitor.set_work_area_offset(monitor_config.work_area_offset);
                monitor.set_window_based_work_area_offset(
                    monitor_config.window_based_work_area_offset,
                );
                monitor.set_window_based_work_area_offset_limit(
                    monitor_config
                        .window_based_work_area_offset_limit
                        .unwrap_or(1),
                );

                for (j, ws) in monitor.workspaces_mut().iter_mut().enumerate() {
                    if let Some(workspace_config) = monitor_config.workspaces.get(j) {
                        ws.load_static_config(workspace_config)?;
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

        // Check for configs that should be tied to a specific display that isn't loaded right now
        // and cache those configs with the specific `device_id` so that when those devices are
        // connected later we can use the correct config from the cache.
        if configs_with_preference.len() > configs_used.len() {
            for i in configs_with_preference
                .iter()
                .filter(|i| !configs_used.contains(i))
            {
                let device_id = {
                    let display_index_preferences = DISPLAY_INDEX_PREFERENCES.lock();
                    display_index_preferences.get(i).cloned()
                };
                if let (Some(device_id), Some(monitor_config)) =
                    (device_id, value.monitors.as_ref().and_then(|ms| ms.get(*i)))
                {
                    monitor_reconciliator::insert_in_monitor_cache(
                        &device_id,
                        monitor_config.clone(),
                    );
                }
            }
        }

        wm.enforce_workspace_rules()?;

        if value.border == Some(true) {
            border_manager::BORDER_ENABLED.store(true, Ordering::SeqCst);
        }

        Ok(())
    }

    pub fn reload(path: &PathBuf, wm: &mut WindowManager) -> Result<()> {
        let mut value = Self::read(path)?;

        value.apply_globals()?;

        let configs_with_preference: Vec<_> =
            DISPLAY_INDEX_PREFERENCES.lock().keys().copied().collect();
        let mut configs_used = Vec::new();

        let mut workspace_matching_rules = WORKSPACE_MATCHING_RULES.lock();
        workspace_matching_rules.clear();
        drop(workspace_matching_rules);

        for (i, monitor) in wm.monitors_mut().iter_mut().enumerate() {
            let preferred_config_idx = {
                let display_index_preferences = DISPLAY_INDEX_PREFERENCES.lock();
                let c_idx = display_index_preferences
                    .iter()
                    .find_map(|(c_idx, m_id)| (monitor.device_id() == m_id).then_some(*c_idx));
                c_idx
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
                // Check if this monitor config is the preferred config for this monitor and store
                // a copy of the config on the monitor cache if it is.
                if idx == preferred_config_idx {
                    monitor_reconciliator::insert_in_monitor_cache(
                        monitor.device_id(),
                        monitor_config.clone(),
                    );
                }

                if let Some(used_config_idx) = idx {
                    configs_used.push(used_config_idx);
                }

                monitor.ensure_workspace_count(monitor_config.workspaces.len());
                if monitor.work_area_offset().is_none() {
                    monitor.set_work_area_offset(monitor_config.work_area_offset);
                }
                monitor.set_window_based_work_area_offset(
                    monitor_config.window_based_work_area_offset,
                );
                monitor.set_window_based_work_area_offset_limit(
                    monitor_config
                        .window_based_work_area_offset_limit
                        .unwrap_or(1),
                );

                for (j, ws) in monitor.workspaces_mut().iter_mut().enumerate() {
                    if let Some(workspace_config) = monitor_config.workspaces.get(j) {
                        ws.load_static_config(workspace_config)?;
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

        // Check for configs that should be tied to a specific display that isn't loaded right now
        // and cache those configs with the specific `device_id` so that when those devices are
        // connected later we can use the correct config from the cache.
        if configs_with_preference.len() > configs_used.len() {
            for i in configs_with_preference
                .iter()
                .filter(|i| !configs_used.contains(i))
            {
                let device_id = {
                    let display_index_preferences = DISPLAY_INDEX_PREFERENCES.lock();
                    display_index_preferences.get(i).cloned()
                };
                if let (Some(device_id), Some(monitor_config)) =
                    (device_id, value.monitors.as_ref().and_then(|ms| ms.get(*i)))
                {
                    monitor_reconciliator::insert_in_monitor_cache(
                        &device_id,
                        monitor_config.clone(),
                    );
                }
            }
        }

        wm.enforce_workspace_rules()?;

        if let Some(enabled) = value.border {
            border_manager::BORDER_ENABLED.store(enabled, Ordering::SeqCst);
        }

        if let Some(val) = value.window_container_behaviour {
            wm.window_management_behaviour.current_behaviour = val;
        }

        if let Some(val) = value.float_override {
            wm.window_management_behaviour.float_override = val;
        }

        if let Some(val) = value.cross_monitor_move_behaviour {
            wm.cross_monitor_move_behaviour = val;
        }

        if let Some(val) = value.cross_boundary_behaviour {
            wm.cross_boundary_behaviour = val;
        }

        if let Some(val) = value.unmanaged_window_operation_behaviour {
            wm.unmanaged_window_operation_behaviour = val;
        }

        if let Some(val) = value.resize_delta {
            wm.resize_delta = val;
        }

        if let Some(val) = value.mouse_follows_focus {
            wm.mouse_follows_focus = val;
        }

        wm.work_area_offset = value.global_work_area_offset;

        match value.focus_follows_mouse {
            None => WindowsApi::disable_focus_follows_mouse()?,
            Some(FocusFollowsMouseImplementation::Windows) => {
                WindowsApi::enable_focus_follows_mouse()?;
            }
            Some(FocusFollowsMouseImplementation::Komorebi) => {}
        };

        wm.focus_follows_mouse = value.focus_follows_mouse;

        let monitor_count = wm.monitors().len();

        for i in 0..monitor_count {
            wm.update_focused_workspace_by_monitor_idx(i)?;
        }

        Ok(())
    }
}

fn populate_option(
    entry: &mut ApplicationConfiguration,
    identifiers: &mut Vec<MatchingRule>,
    regex_identifiers: &mut HashMap<String, Regex>,
) -> Result<()> {
    if entry.identifier.matching_strategy.is_none() {
        entry.identifier.matching_strategy = Option::from(MatchingStrategy::Legacy);
    }

    let rule = MatchingRule::Simple(entry.identifier.clone());

    if !identifiers.contains(&rule) {
        identifiers.push(rule);

        if matches!(
            entry.identifier.matching_strategy,
            Some(MatchingStrategy::Regex)
        ) {
            let re = Regex::new(&entry.identifier.id)?;
            regex_identifiers.insert(entry.identifier.id.clone(), re);
        }
    }

    Ok(())
}

fn populate_rules(
    matching_rules: &mut Vec<MatchingRule>,
    identifiers: &mut Vec<MatchingRule>,
    regex_identifiers: &mut HashMap<String, Regex>,
) -> Result<()> {
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
