use crate::border_manager;
use crate::border_manager::ZOrder;
use crate::border_manager::IMPLEMENTATION;
use crate::border_manager::STYLE;
use crate::border_manager::Z_ORDER;
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
use crate::transparency_manager;
use crate::window;
use crate::window_manager::WindowManager;
use crate::window_manager_event::WindowManagerEvent;
use crate::windows_api::WindowsApi;
use crate::workspace::Workspace;
use crate::CrossBoundaryBehaviour;
use crate::ANIMATION_DURATION;
use crate::ANIMATION_ENABLED;
use crate::ANIMATION_FPS;
use crate::ANIMATION_STYLE;
use crate::DATA_DIR;
use crate::DEFAULT_CONTAINER_PADDING;
use crate::DEFAULT_WORKSPACE_PADDING;
use crate::DISPLAY_INDEX_PREFERENCES;
use crate::FLOAT_IDENTIFIERS;
use crate::HIDING_BEHAVIOUR;
use crate::LAYERED_WHITELIST;
use crate::MANAGE_IDENTIFIERS;
use crate::MONITOR_INDEX_PREFERENCES;
use crate::OBJECT_NAME_CHANGE_ON_LAUNCH;
use crate::REGEX_IDENTIFIERS;
use crate::TRAY_AND_MULTI_WINDOW_IDENTIFIERS;
use crate::WINDOWS_11;
use crate::WORKSPACE_RULES;

use crate::core::config_generation::ApplicationConfiguration;
use crate::core::config_generation::ApplicationConfigurationGenerator;
use crate::core::config_generation::ApplicationOptions;
use crate::core::config_generation::IdWithIdentifier;
use crate::core::config_generation::MatchingRule;
use crate::core::config_generation::MatchingStrategy;
use crate::core::resolve_home_path;
use crate::core::AnimationStyle;
use crate::core::ApplicationIdentifier;
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
    /// Border colour when the container is unfocused
    pub unfocused: Option<Colour>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct WorkspaceConfig {
    /// Name
    pub name: String,
    /// Layout (default: BSP)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub layout: Option<DefaultLayout>,
    /// Custom Layout (default: None)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub custom_layout: Option<PathBuf>,
    /// Layout rules (default: None)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub layout_rules: Option<HashMap<usize, DefaultLayout>>,
    /// Layout rules (default: None)
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
    pub initial_workspace_rules: Option<Vec<IdWithIdentifier>>,
    /// Permanent workspace application rules
    #[serde(skip_serializing_if = "Option::is_none")]
    pub workspace_rules: Option<Vec<IdWithIdentifier>>,
    /// Apply this monitor's window-based work area offset (default: true)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub apply_window_based_work_area_offset: Option<bool>,
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

        let workspace_rules = WORKSPACE_RULES.lock();
        let mut initial_ws_rules = vec![];
        let mut ws_rules = vec![];

        for (identifier, (_, _, is_initial)) in &*workspace_rules {
            if identifier.ends_with("exe") {
                let rule = IdWithIdentifier {
                    kind: ApplicationIdentifier::Exe,
                    id: identifier.clone(),
                    matching_strategy: None,
                };

                if *is_initial {
                    initial_ws_rules.push(rule);
                } else {
                    ws_rules.push(rule);
                }
            }
        }

        let initial_ws_rules = if initial_ws_rules.is_empty() {
            None
        } else {
            Option::from(initial_ws_rules)
        };
        let ws_rules = if ws_rules.is_empty() {
            None
        } else {
            Option::from(ws_rules)
        };

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
            layout: match value.layout() {
                Layout::Default(layout) => Option::from(*layout),
                // TODO: figure out how we might resolve file references in the future
                Layout::Custom(_) => None,
            },
            custom_layout: None,
            layout_rules: Option::from(layout_rules),
            // TODO: figure out how we might resolve file references in the future
            custom_layout_rules: None,
            container_padding,
            workspace_padding,
            initial_workspace_rules: initial_ws_rules,
            workspace_rules: ws_rules,
            apply_window_based_work_area_offset: Some(value.apply_window_based_work_area_offset()),
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
/// The `komorebi.json` static configuration file reference for `v0.1.28`
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
    /// Determine what happens when a window is moved across a monitor boundary (default: Swap)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cross_monitor_move_behaviour: Option<MoveBehaviour>,
    /// Determine what happens when an action is called on a window at a monitor boundary (default: Monitor)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cross_boundary_behaviour: Option<CrossBoundaryBehaviour>,
    /// Determine what happens when commands are sent while an unmanaged window is in the foreground (default: Op)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub unmanaged_window_operation_behaviour: Option<OperationBehaviour>,
    /// Determine focus follows mouse implementation (default: None)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub focus_follows_mouse: Option<FocusFollowsMouseImplementation>,
    /// Enable or disable mouse follows focus (default: true)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub mouse_follows_focus: Option<bool>,
    /// Path to applications.yaml from komorebi-application-specific-configurations (default: None)
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
    /// Active window border z-order (default: System)
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
    pub float_rules: Option<Vec<MatchingRule>>,
    /// Individual window force-manage rules
    #[serde(skip_serializing_if = "Option::is_none")]
    pub manage_rules: Option<Vec<MatchingRule>>,
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
}

#[derive(Debug, Serialize, Deserialize, JsonSchema)]
pub struct AnimationsConfig {
    /// Enable or disable animations (default: false)
    enabled: bool,
    /// Set the animation duration in ms (default: 250)
    duration: Option<u64>,
    /// Set the animation style (default: Linear)
    style: Option<AnimationStyle>,
    /// Set the animation FPS (default: 60)
    fps: Option<u64>,
}
#[derive(Copy, Clone, Debug, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "type")]
pub enum KomorebiTheme {
    /// A theme from catppuccin-egui
    Catppuccin {
        /// Name of the Catppuccin theme
        name: komorebi_themes::Catppuccin,
        /// Border colour when the container contains a single window (default: Blue)
        single_border: Option<komorebi_themes::CatppuccinValue>,
        /// Border colour when the container contains multiple windows (default: Green)
        stack_border: Option<komorebi_themes::CatppuccinValue>,
        /// Border colour when the container is in monocle mode (default: Pink)
        monocle_border: Option<komorebi_themes::CatppuccinValue>,
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
        /// Name of the Base16 theme
        name: komorebi_themes::Base16,
        /// Border colour when the container contains a single window (default: Base0D)
        single_border: Option<komorebi_themes::Base16Value>,
        /// Border colour when the container contains multiple windows (default: Base0B)
        stack_border: Option<komorebi_themes::Base16Value>,
        /// Border colour when the container is in monocle mode (default: Base0F)
        monocle_border: Option<komorebi_themes::Base16Value>,
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
    pub fn aliases(raw: &str) {
        let mut map = HashMap::new();
        map.insert("border", ["active_window_border"]);
        map.insert("border_width", ["active_window_border_width"]);
        map.insert("border_offset", ["active_window_border_offset"]);
        map.insert("border_colours", ["active_window_border_colours"]);
        map.insert("border_style", ["active_window_border_style"]);

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
        let deprecated_options = ["invisible_borders"];
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

        let mut to_remove = vec![];
        let mut to_add_initial = vec![];
        let mut to_add_persistent = vec![];

        let workspace_rules = WORKSPACE_RULES.lock();
        for (m_idx, m) in monitors.iter().enumerate() {
            for (w_idx, w) in m.workspaces.iter().enumerate() {
                if let Some(rules) = &w.initial_workspace_rules {
                    for iwsr in rules {
                        for (identifier, (monitor_idx, workspace_idx, _)) in &*workspace_rules {
                            if iwsr.id.eq(identifier)
                                && (*monitor_idx != m_idx || *workspace_idx != w_idx)
                            {
                                to_remove.push((m_idx, w_idx, iwsr.id.clone()));
                            }
                        }
                    }
                }

                for (identifier, (monitor_idx, workspace_idx, initial)) in &*workspace_rules {
                    if *initial && (*monitor_idx == m_idx && *workspace_idx == w_idx) {
                        to_add_initial.push((m_idx, w_idx, identifier.clone()));
                    }
                }

                if let Some(rules) = &w.workspace_rules {
                    for wsr in rules {
                        for (identifier, (monitor_idx, workspace_idx, _)) in &*workspace_rules {
                            if wsr.id.eq(identifier)
                                && (*monitor_idx != m_idx || *workspace_idx != w_idx)
                            {
                                to_remove.push((m_idx, w_idx, wsr.id.clone()));
                            }
                        }
                    }
                }

                for (identifier, (monitor_idx, workspace_idx, initial)) in &*workspace_rules {
                    if !*initial && (*monitor_idx == m_idx && *workspace_idx == w_idx) {
                        to_add_persistent.push((m_idx, w_idx, identifier.clone()));
                    }
                }
            }
        }

        for (m_idx, w_idx, id) in to_remove {
            if let Some(monitor) = monitors.get_mut(m_idx) {
                if let Some(workspace) = monitor.workspaces.get_mut(w_idx) {
                    if workspace.workspace_rules.is_none() {
                        workspace.workspace_rules = Some(vec![]);
                    }

                    if let Some(rules) = &mut workspace.workspace_rules {
                        rules.retain(|r| r.id != id);
                        for (monitor_idx, workspace_idx, id) in &to_add_persistent {
                            if m_idx == *monitor_idx && w_idx == *workspace_idx {
                                rules.push(IdWithIdentifier {
                                    kind: ApplicationIdentifier::Exe,
                                    id: id.clone(),
                                    matching_strategy: None,
                                })
                            }
                        }

                        rules.dedup();
                    }

                    if workspace.initial_workspace_rules.is_none() {
                        workspace.workspace_rules = Some(vec![]);
                    }

                    if let Some(rules) = &mut workspace.initial_workspace_rules {
                        rules.retain(|r| r.id != id);
                        for (monitor_idx, workspace_idx, id) in &to_add_initial {
                            if m_idx == *monitor_idx && w_idx == *workspace_idx {
                                rules.push(IdWithIdentifier {
                                    kind: ApplicationIdentifier::Exe,
                                    id: id.clone(),
                                    matching_strategy: None,
                                })
                            }
                        }

                        rules.dedup();
                    }
                }
            }
        }

        let border_colours = if border_manager::FOCUSED.load(Ordering::SeqCst) == 0 {
            None
        } else {
            Option::from(BorderColours {
                single: Option::from(Colour::from(border_manager::FOCUSED.load(Ordering::SeqCst))),
                stack: Option::from(Colour::from(border_manager::STACK.load(Ordering::SeqCst))),
                monocle: Option::from(Colour::from(border_manager::MONOCLE.load(Ordering::SeqCst))),
                unfocused: Option::from(Colour::from(
                    border_manager::UNFOCUSED.load(Ordering::SeqCst),
                )),
            })
        };

        Self {
            invisible_borders: None,
            resize_delta: Option::from(value.resize_delta),
            window_container_behaviour: Option::from(value.window_container_behaviour),
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
            border_style: Option::from(STYLE.load()),
            border_z_order: Option::from(Z_ORDER.load()),
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
            float_rules: None,
            manage_rules: None,
            border_overflow_applications: None,
            tray_and_multi_window_applications: None,
            layered_applications: None,
            object_name_change_applications: None,
            monitor_index_preferences: Option::from(MONITOR_INDEX_PREFERENCES.lock().clone()),
            display_index_preferences: Option::from(DISPLAY_INDEX_PREFERENCES.lock().clone()),
            stackbar: None,
            animation: None,
            theme: None,
        }
    }
}

impl StaticConfig {
    #[allow(clippy::cognitive_complexity, clippy::too_many_lines)]
    fn apply_globals(&mut self) -> Result<()> {
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
            ANIMATION_ENABLED.store(animations.enabled, Ordering::SeqCst);
            ANIMATION_DURATION.store(animations.duration.unwrap_or(250), Ordering::SeqCst);
            ANIMATION_FPS.store(animations.fps.unwrap_or(60), Ordering::SeqCst);
            let mut animation_style = ANIMATION_STYLE.lock();
            *animation_style = animations.style.unwrap_or(AnimationStyle::Linear);
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

            border_manager::send_notification();
        }

        transparency_manager::TRANSPARENCY_ENABLED
            .store(self.transparency.unwrap_or(false), Ordering::SeqCst);
        transparency_manager::TRANSPARENCY_ALPHA
            .store(self.transparency_alpha.unwrap_or(200), Ordering::SeqCst);

        let mut float_identifiers = FLOAT_IDENTIFIERS.lock();
        let mut regex_identifiers = REGEX_IDENTIFIERS.lock();
        let mut manage_identifiers = MANAGE_IDENTIFIERS.lock();
        let mut tray_and_multi_window_identifiers = TRAY_AND_MULTI_WINDOW_IDENTIFIERS.lock();
        let mut object_name_change_identifiers = OBJECT_NAME_CHANGE_ON_LAUNCH.lock();
        let mut layered_identifiers = LAYERED_WHITELIST.lock();

        if let Some(rules) = &mut self.float_rules {
            populate_rules(rules, &mut float_identifiers, &mut regex_identifiers)?;
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
            let (
                single_border,
                stack_border,
                monocle_border,
                unfocused_border,
                stackbar_focused_text,
                stackbar_unfocused_text,
                stackbar_background,
            ) = match theme {
                KomorebiTheme::Catppuccin {
                    name,
                    single_border,
                    stack_border,
                    monocle_border,
                    unfocused_border,
                    stackbar_focused_text,
                    stackbar_unfocused_text,
                    stackbar_background,
                    ..
                } => {
                    let single_border = single_border
                        .unwrap_or(komorebi_themes::CatppuccinValue::Blue)
                        .color32(name.as_theme());

                    let stack_border = stack_border
                        .unwrap_or(komorebi_themes::CatppuccinValue::Green)
                        .color32(name.as_theme());

                    let monocle_border = monocle_border
                        .unwrap_or(komorebi_themes::CatppuccinValue::Pink)
                        .color32(name.as_theme());

                    let unfocused_border = unfocused_border
                        .unwrap_or(komorebi_themes::CatppuccinValue::Base)
                        .color32(name.as_theme());

                    let stackbar_focused_text = stackbar_focused_text
                        .unwrap_or(komorebi_themes::CatppuccinValue::Green)
                        .color32(name.as_theme());

                    let stackbar_unfocused_text = stackbar_unfocused_text
                        .unwrap_or(komorebi_themes::CatppuccinValue::Text)
                        .color32(name.as_theme());

                    let stackbar_background = stackbar_background
                        .unwrap_or(komorebi_themes::CatppuccinValue::Base)
                        .color32(name.as_theme());

                    (
                        single_border,
                        stack_border,
                        monocle_border,
                        unfocused_border,
                        stackbar_focused_text,
                        stackbar_unfocused_text,
                        stackbar_background,
                    )
                }
                KomorebiTheme::Base16 {
                    name,
                    single_border,
                    stack_border,
                    monocle_border,
                    unfocused_border,
                    stackbar_focused_text,
                    stackbar_unfocused_text,
                    stackbar_background,
                    ..
                } => {
                    let single_border = single_border
                        .unwrap_or(komorebi_themes::Base16Value::Base0D)
                        .color32(*name);

                    let stack_border = stack_border
                        .unwrap_or(komorebi_themes::Base16Value::Base0B)
                        .color32(*name);

                    let monocle_border = monocle_border
                        .unwrap_or(komorebi_themes::Base16Value::Base0F)
                        .color32(*name);

                    let unfocused_border = unfocused_border
                        .unwrap_or(komorebi_themes::Base16Value::Base01)
                        .color32(*name);

                    let stackbar_focused_text = stackbar_focused_text
                        .unwrap_or(komorebi_themes::Base16Value::Base0B)
                        .color32(*name);

                    let stackbar_unfocused_text = stackbar_unfocused_text
                        .unwrap_or(komorebi_themes::Base16Value::Base05)
                        .color32(*name);

                    let stackbar_background = stackbar_background
                        .unwrap_or(komorebi_themes::Base16Value::Base01)
                        .color32(*name);

                    (
                        single_border,
                        stack_border,
                        monocle_border,
                        unfocused_border,
                        stackbar_focused_text,
                        stackbar_unfocused_text,
                        stackbar_background,
                    )
                }
            };

            border_manager::FOCUSED.store(u32::from(Colour::from(single_border)), Ordering::SeqCst);
            border_manager::MONOCLE
                .store(u32::from(Colour::from(monocle_border)), Ordering::SeqCst);
            border_manager::STACK.store(u32::from(Colour::from(stack_border)), Ordering::SeqCst);
            border_manager::UNFOCUSED
                .store(u32::from(Colour::from(unfocused_border)), Ordering::SeqCst);

            STACKBAR_TAB_BACKGROUND_COLOUR.store(
                u32::from(Colour::from(stackbar_background)),
                Ordering::SeqCst,
            );

            STACKBAR_FOCUSED_TEXT_COLOUR.store(
                u32::from(Colour::from(stackbar_focused_text)),
                Ordering::SeqCst,
            );

            STACKBAR_UNFOCUSED_TEXT_COLOUR.store(
                u32::from(Colour::from(stackbar_unfocused_text)),
                Ordering::SeqCst,
            );
        }

        if let Some(path) = &self.app_specific_configuration_path {
            let path = resolve_home_path(path)?;
            let content = std::fs::read_to_string(path)?;
            let asc = ApplicationConfigurationGenerator::load(&content)?;

            for mut entry in asc {
                if let Some(rules) = &mut entry.float_identifiers {
                    populate_rules(rules, &mut float_identifiers, &mut regex_identifiers)?;
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

        Ok(())
    }

    pub fn read(path: &PathBuf) -> Result<Self> {
        let content = std::fs::read_to_string(path)?;
        let value: Self = serde_json::from_str(&content)?;
        Ok(value)
    }

    #[allow(clippy::too_many_lines)]
    pub fn preload(
        path: &PathBuf,
        incoming: Receiver<WindowManagerEvent>,
    ) -> Result<WindowManager> {
        let content = std::fs::read_to_string(path)?;
        let mut value: Self = serde_json::from_str(&content)?;
        value.apply_globals()?;

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

        let listener = UnixListener::bind(&socket)?;

        let mut wm = WindowManager {
            monitors: Ring::default(),
            incoming_events: incoming,
            command_listener: listener,
            is_paused: false,
            virtual_desktop_id: current_virtual_desktop(),
            work_area_offset: value.global_work_area_offset,
            window_container_behaviour: value
                .window_container_behaviour
                .unwrap_or(WindowContainerBehaviour::Create),
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
            pending_move_op: None,
            already_moved_window_handles: Arc::new(Mutex::new(HashSet::new())),
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
        let content = std::fs::read_to_string(path)?;
        let value: Self = serde_json::from_str(&content)?;
        let mut wm = wm.lock();

        if let Some(monitors) = value.monitors {
            for (i, monitor) in monitors.iter().enumerate() {
                {
                    let display_index_preferences = DISPLAY_INDEX_PREFERENCES.lock();
                    if let Some(device_id) = display_index_preferences.get(&i) {
                        monitor_reconciliator::insert_in_monitor_cache(device_id, monitor.clone());
                    }
                }

                if let Some(m) = wm.monitors_mut().get_mut(i) {
                    m.ensure_workspace_count(monitor.workspaces.len());
                    m.set_work_area_offset(monitor.work_area_offset);
                    m.set_window_based_work_area_offset(monitor.window_based_work_area_offset);
                    m.set_window_based_work_area_offset_limit(
                        monitor.window_based_work_area_offset_limit.unwrap_or(1),
                    );

                    for (j, ws) in m.workspaces_mut().iter_mut().enumerate() {
                        ws.load_static_config(
                            monitor
                                .workspaces
                                .get(j)
                                .expect("no static workspace config"),
                        )?;
                    }
                }

                for (j, ws) in monitor.workspaces.iter().enumerate() {
                    if let Some(rules) = &ws.workspace_rules {
                        for r in rules {
                            wm.handle_workspace_rules(&r.id, i, j, false)?;
                        }
                    }

                    if let Some(rules) = &ws.initial_workspace_rules {
                        for r in rules {
                            wm.handle_workspace_rules(&r.id, i, j, true)?;
                        }
                    }
                }
            }
        }

        if value.border == Some(true) {
            border_manager::BORDER_ENABLED.store(true, Ordering::SeqCst);
        }

        Ok(())
    }

    pub fn reload(path: &PathBuf, wm: &mut WindowManager) -> Result<()> {
        let content = std::fs::read_to_string(path)?;
        let mut value: Self = serde_json::from_str(&content)?;

        value.apply_globals()?;

        if let Some(monitors) = value.monitors {
            for (i, monitor) in monitors.iter().enumerate() {
                if let Some(m) = wm.monitors_mut().get_mut(i) {
                    m.ensure_workspace_count(monitor.workspaces.len());
                    if m.work_area_offset().is_none() {
                        m.set_work_area_offset(monitor.work_area_offset);
                    }
                    m.set_window_based_work_area_offset(monitor.window_based_work_area_offset);
                    m.set_window_based_work_area_offset_limit(
                        monitor.window_based_work_area_offset_limit.unwrap_or(1),
                    );

                    for (j, ws) in m.workspaces_mut().iter_mut().enumerate() {
                        ws.load_static_config(
                            monitor
                                .workspaces
                                .get(j)
                                .expect("no static workspace config"),
                        )?;
                    }
                }

                for (j, ws) in monitor.workspaces.iter().enumerate() {
                    if let Some(rules) = &ws.workspace_rules {
                        for r in rules {
                            wm.handle_workspace_rules(&r.id, i, j, false)?;
                        }
                    }

                    if let Some(rules) = &ws.initial_workspace_rules {
                        for r in rules {
                            wm.handle_workspace_rules(&r.id, i, j, true)?;
                        }
                    }
                }
            }
        }

        if let Some(enabled) = value.border {
            border_manager::BORDER_ENABLED.store(enabled, Ordering::SeqCst);
        }

        if let Some(val) = value.window_container_behaviour {
            wm.window_container_behaviour = val;
        }

        if let Some(val) = value.cross_monitor_move_behaviour {
            wm.cross_monitor_move_behaviour = val;
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
