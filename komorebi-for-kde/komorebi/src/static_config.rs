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
use crate::TRANSPARENCY_BLACKLIST;
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
use crate::backend::backend;
use crate::border_manager;
use crate::core::CrossBoundaryBehaviour;
use crate::core::Layout;
use crate::core::MoveBehaviour;
use crate::core::OperationBehaviour;
use crate::core::Placement;
use crate::core::Rect;
use crate::core::SocketMessage;
use crate::core::WindowContainerBehaviour;
use crate::core::WindowManagementBehaviour;
use crate::core::asc::ApplicationSpecificConfiguration;
use crate::core::asc::AscApplicationRulesOrSchema;
use crate::core::config_generation::MatchingRule;
use crate::core::config_generation::MatchingStrategy;
use crate::core::config_generation::WorkspaceMatchingRule;
use crate::monitor;
use crate::monitor::Monitor;
use crate::monitor_reconciliator;
use crate::ring::Ring;
use crate::theme_manager;
use crate::transparency_manager;
use crate::window::AspectRatio;
use crate::window::PredefinedAspectRatio;
use crate::window_manager::WindowManager;
use crate::window_manager_event::WindowManagerEvent;
use crate::workspace::Workspace;
use color_eyre::eyre;
use crossbeam_channel::Receiver;
use hotwatch::EventKind;
use hotwatch::Hotwatch;
use komorebi_themes::colour::Colour;
use parking_lot::Mutex;
use regex::Regex;
use std::collections::HashMap;
use std::io::ErrorKind;
use std::io::Write;
use std::os::unix::net::UnixListener;
use std::os::unix::net::UnixStream;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::Ordering;

// serde_as must be before derive
pub use komorebi_config::static_config::AnimationsConfig;
pub use komorebi_config::static_config::AppSpecificConfigurationPath;
pub use komorebi_config::static_config::BorderColours;
pub use komorebi_config::static_config::MonitorConfig;
pub use komorebi_config::static_config::StaticConfig;
pub use komorebi_config::static_config::ThemeOptions;
pub use komorebi_config::static_config::Wallpaper;
pub use komorebi_config::static_config::WorkspaceConfig;

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
            wallpaper: None,
            initial_window_placement_rules: value.initial_window_placement_rules.clone(),
            custom_layout: None,
            custom_layout_rules: None,
        }
    }
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
            wallpaper: value.wallpaper.clone(),
            floating_layer_behaviour: value.floating_layer_behaviour,
            window_hiding_position: None,
        }
    }
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
                unfocused_locked: Option::from(Colour::from(
                    border_manager::UNFOCUSED_LOCKED.load(Ordering::SeqCst),
                )),
            })
        };

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
            focus_follows_mouse: value.focus_follows_mouse,
            mouse_follows_focus: Option::from(value.mouse_follows_focus),
            app_specific_configuration_path: None,
            border_width: Option::from(border_manager::BORDER_WIDTH.load(Ordering::SeqCst)),
            border_offset: Option::from(border_manager::BORDER_OFFSET.load(Ordering::SeqCst)),
            border_radius: Option::from(border_manager::BORDER_RADIUS.load(Ordering::SeqCst)),
            border: Option::from(border_manager::BORDER_ENABLED.load(Ordering::SeqCst)),
            border_colours,
            transparency: Option::from(
                transparency_manager::TRANSPARENCY_ENABLED.load(Ordering::SeqCst),
            ),
            transparency_alpha: Option::from(
                transparency_manager::TRANSPARENCY_ALPHA.load(Ordering::SeqCst),
            ),
            transparency_ignore_rules: None,
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
            theme: None,
            // slow_application_compensation_time: Option::from(
            //     SLOW_APPLICATION_COMPENSATION_TIME.load(Ordering::SeqCst),
            // ),
            // slow_application_identifiers: Option::from(SLOW_APPLICATION_IDENTIFIERS.lock().clone()),
            // bar_configurations: None,
            // remove_titlebar_applications: Option::from(NO_TITLEBAR.lock().clone()),
            floating_window_aspect_ratio: Option::from(*FLOATING_WINDOW_TOGGLE_ASPECT_RATIO.lock()),
            // window_handling_behaviour: Option::from(WINDOW_HANDLING_BEHAVIOUR.load()),
            tabbed_applications: None,
            ..Default::default()
        }
    }
}

/// Applies the settings of a configuration that are not the window manager's own
trait ApplyGlobals {
    fn apply_globals(&mut self) -> eyre::Result<()>;
}

impl ApplyGlobals for StaticConfig {
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

            if let Some(unfocused_locked) = colours.unfocused_locked {
                border_manager::UNFOCUSED_LOCKED
                    .store(u32::from(unfocused_locked), Ordering::SeqCst);
            }
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
        transparency_manager::TRANSPARENCY_ENABLED
            .store(self.transparency.unwrap_or(false), Ordering::SeqCst);
        transparency_manager::TRANSPARENCY_ALPHA
            .store(self.transparency_alpha.unwrap_or(200), Ordering::SeqCst);

        // the order every other place takes them in (floating, transparency, ignore, manage, regex):
        // a thread that takes two in another order can wait for one that holds what it needs
        let mut floating_applications = FLOATING_APPLICATIONS.lock();
        let mut transparency_blacklist = TRANSPARENCY_BLACKLIST.lock();
        let mut ignore_identifiers = IGNORE_IDENTIFIERS.lock();
        let mut manage_identifiers = MANAGE_IDENTIFIERS.lock();
        let mut regex_identifiers = REGEX_IDENTIFIERS.lock();
        // let mut tray_and_multi_window_identifiers = TRAY_AND_MULTI_WINDOW_IDENTIFIERS.lock();
        // let mut object_name_change_identifiers = OBJECT_NAME_CHANGE_ON_LAUNCH.lock();
        // let mut object_name_change_title_ignore_list = OBJECT_NAME_CHANGE_TITLE_IGNORE_LIST.lock();
        // let mut layered_identifiers = LAYERED_WHITELIST.lock();
        // let mut slow_application_identifiers = SLOW_APPLICATION_IDENTIFIERS.lock();
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
        if let Some(rules) = &mut self.transparency_ignore_rules {
            populate_rules(rules, &mut transparency_blacklist, &mut regex_identifiers)?;
        }
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
        if let Some(theme) = &self.theme {
            theme_manager::send_notification(theme.clone());
        }

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
}

/// What loading a configuration needs from the window manager: the part of `StaticConfig`
/// that cannot be an inherent impl now that the type lives in `komorebi-config`.
pub trait StaticConfigExt: Sized {
    /// Makes the window manager from the file at `path`, and starts watching the file
    fn preload(
        path: &PathBuf,
        incoming: Receiver<WindowManagerEvent>,
        unix_listener: Option<UnixListener>,
    ) -> eyre::Result<WindowManager>;

    /// Gives the monitors of the window manager what the file says about them
    fn postload(path: &PathBuf, wm: &Arc<Mutex<WindowManager>>) -> eyre::Result<()>;

    /// Replaces the configuration of the window manager with the file at `path`
    fn reload(path: &PathBuf, wm: &mut WindowManager) -> eyre::Result<()>;
}

impl StaticConfigExt for StaticConfig {
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
    fn preload(
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
            focus_follows_mouse: value.focus_follows_mouse,
            opacity_applied: HashMap::new(),
            mouse_follows_focus: value
                .mouse_follows_focus
                .unwrap_or(DEFAULT_MOUSE_FOLLOWS_FOCUS),
            hotwatch: Hotwatch::new()?,
            minimized_windows: HashMap::new(),
            pending_move_op: Arc::new(None),
            pending_resize_op: Arc::new(None),
            live_resize: None,
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
    fn postload(path: &PathBuf, wm: &Arc<Mutex<WindowManager>>) -> eyre::Result<()> {
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
                monitor.wallpaper = monitor_config.wallpaper.clone();
                monitor.floating_layer_behaviour = monitor_config.floating_layer_behaviour;

                monitor.update_workspaces_globals(offset);
                for (j, ws) in monitor.workspaces_mut().iter_mut().enumerate() {
                    if let Some(workspace_config) = monitor_config.workspaces.get_mut(j) {
                        ws.load_static_config(workspace_config, value.layout_defaults.as_ref())?;
                    }
                }

                // Check if this monitor config is the preferred config for this monitor and store
                // a copy of the monitor itself on the monitor cache if it is.
                if idx == preferred_config_idx {
                    let id = &monitor.serial_number_id;
                    monitor_reconciliator::insert_in_monitor_cache(id, monitor.clone());
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
        // and cache a monitor with those configs with the specific `serial_number_id` so that when
        // those devices are connected later we can use the correct config from the cache.
        if configs_with_preference.len() > configs_used.len() {
            for i in configs_with_preference
                .iter()
                .filter(|i| !configs_used.contains(i))
            {
                let id = {
                    let display_index_preferences = DISPLAY_INDEX_PREFERENCES.read();
                    display_index_preferences.get(i).cloned()
                };
                if let (Some(id), Some(monitor_config)) =
                    (id, value.monitors.as_ref().and_then(|ms| ms.get(*i)))
                {
                    // The device and serial_number_id can be empty here since once the monitor
                    // with this preferred index actually connects the `load_monitor_information`
                    // function will update these fields.
                    let mut m =
                        monitor::new(0, Rect::default(), Rect::default(), "".into(), "".into());

                    m.ensure_workspace_count(monitor_config.workspaces.len());
                    m.work_area_offset = monitor_config.work_area_offset;
                    m.window_based_work_area_offset = monitor_config.window_based_work_area_offset;
                    m.window_based_work_area_offset_limit = monitor_config
                        .window_based_work_area_offset_limit
                        .unwrap_or(1);
                    m.container_padding = monitor_config.container_padding;
                    m.workspace_padding = monitor_config.workspace_padding;
                    m.floating_layer_behaviour = monitor_config.floating_layer_behaviour;

                    m.update_workspaces_globals(offset);

                    for (j, ws) in m.workspaces_mut().iter_mut().enumerate() {
                        if let Some(workspace_config) = monitor_config.workspaces.get(j) {
                            ws.load_static_config(
                                workspace_config,
                                value.layout_defaults.as_ref(),
                            )?;
                        }
                    }

                    monitor_reconciliator::insert_in_monitor_cache(&id, m);
                }
            }
        }

        wm.enforce_workspace_rules()?;

        if value.border == Some(true) {
            border_manager::BORDER_ENABLED.store(true, Ordering::SeqCst);
        }

        // the window manager was made from the configuration, before KWin could be asked anything
        wm.focus_follows_mouse = value.focus_follows_mouse;
        if let Err(error) = backend().set_focus_follows_mouse(wm.focus_follows_mouse.is_some()) {
            tracing::warn!("could not set focus follows mouse: {error}");
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
    fn reload(path: &PathBuf, wm: &mut WindowManager) -> eyre::Result<()> {
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
                monitor.wallpaper = monitor_config.wallpaper.clone();
                monitor.floating_layer_behaviour = monitor_config.floating_layer_behaviour;

                monitor.update_workspaces_globals(offset);

                for (j, ws) in monitor.workspaces_mut().iter_mut().enumerate() {
                    if let Some(workspace_config) = monitor_config.workspaces.get(j) {
                        ws.load_static_config(workspace_config, value.layout_defaults.as_ref())?;
                    }
                }

                // Check if this monitor config is the preferred config for this monitor and store
                // a copy of the monitor itself on the monitor cache if it is.
                if idx == preferred_config_idx {
                    let id = monitor.serial_number_id.clone();
                    monitor_reconciliator::insert_in_monitor_cache(&id, monitor.clone());
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
        // and cache a monitor with those configs with the specific `serial_number_id` so that when
        // those devices are connected later we can use the correct config from the cache.
        if configs_with_preference.len() > configs_used.len() {
            for i in configs_with_preference
                .iter()
                .filter(|i| !configs_used.contains(i))
            {
                let id = {
                    let display_index_preferences = DISPLAY_INDEX_PREFERENCES.read();
                    display_index_preferences.get(i).cloned()
                };
                if let (Some(id), Some(monitor_config)) =
                    (id, value.monitors.as_ref().and_then(|ms| ms.get(*i)))
                {
                    // The device and serial_number_id can be empty here since once the monitor
                    // with this preferred index actually connects the `load_monitor_information`
                    // function will update these fields.
                    let mut m =
                        monitor::new(0, Rect::default(), Rect::default(), "".into(), "".into());

                    m.ensure_workspace_count(monitor_config.workspaces.len());
                    m.work_area_offset = monitor_config.work_area_offset;
                    m.window_based_work_area_offset = monitor_config.window_based_work_area_offset;
                    m.window_based_work_area_offset_limit = monitor_config
                        .window_based_work_area_offset_limit
                        .unwrap_or(1);
                    m.container_padding = monitor_config.container_padding;
                    m.workspace_padding = monitor_config.workspace_padding;
                    m.floating_layer_behaviour = monitor_config.floating_layer_behaviour;

                    m.update_workspaces_globals(offset);

                    for (j, ws) in m.workspaces_mut().iter_mut().enumerate() {
                        if let Some(workspace_config) = monitor_config.workspaces.get(j) {
                            ws.load_static_config(
                                workspace_config,
                                value.layout_defaults.as_ref(),
                            )?;
                        }
                    }

                    monitor_reconciliator::insert_in_monitor_cache(&id, m);
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
        wm.focus_follows_mouse = value.focus_follows_mouse;
        if let Err(error) = backend().set_focus_follows_mouse(wm.focus_follows_mouse.is_some()) {
            tracing::warn!("could not set focus follows mouse: {error}");
        }

        let monitor_count = wm.monitors().len();

        for i in 0..monitor_count {
            wm.update_focused_workspace_by_monitor_idx(i)?;
            let ws_idx = wm.focused_workspace_idx_for_monitor_idx(i)?;
            wm.apply_wallpaper_for_monitor_workspace(i, ws_idx)?;
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
    use crate::core::WindowHidingPosition;

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
