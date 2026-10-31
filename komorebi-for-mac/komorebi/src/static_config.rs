use crate::CoreFoundationRunLoop;
use crate::DATA_DIR;
use crate::DEFAULT_MOUSE_FOLLOWS_FOCUS;
use crate::DEFAULT_RESIZE_DELTA;
use crate::FLOATING_APPLICATIONS;
use crate::IGNORE_IDENTIFIERS;
use crate::MANAGE_IDENTIFIERS;
use crate::REGEX_IDENTIFIERS;
use crate::TABBED_APPLICATIONS;
use crate::TITLELESS_APPLICATIONS;
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
use crate::core::ApplicationIdentifier;
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
use crate::current_space_id;
use crate::monitor;
use crate::monitor::Monitor;
use crate::monitor_reconciliator;
use crate::ring::Ring;
use crate::theme_manager;
use crate::window_manager::WindowManager;
use crate::window_manager_event::WindowManagerEvent;
use crate::workspace::Workspace;
use color_eyre::eyre;
use crossbeam_channel::Receiver;
use hotwatch::EventKind;
use hotwatch::Hotwatch;
use komorebi_settings::shared::SharedDelta;
use komorebi_themes::colour::Colour;
use objc2_core_foundation::CFRetained;
use objc2_core_foundation::CFRunLoop;
use parking_lot::Mutex;
use regex::Regex;
use std::collections::HashMap;
use std::io::ErrorKind;
use std::io::Write;
use std::os::unix::net::UnixListener;
use std::os::unix::net::UnixStream;
use std::path::Path;
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

        let default_container_padding = crate::settings::container_padding();
        let default_workspace_padding = crate::settings::workspace_padding();

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
            custom_layout: None,
            custom_layout_rules: None,
            initial_window_placement_rules: None,
        }
    }
}

impl From<&Monitor> for MonitorConfig {
    fn from(value: &Monitor) -> Self {
        let mut workspaces = vec![];
        for w in value.workspaces() {
            workspaces.push(WorkspaceConfig::from(w));
        }

        let default_container_padding = crate::settings::container_padding();
        let default_workspace_padding = crate::settings::workspace_padding();

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
            window_hiding_position: Some(value.window_hiding_position),
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
            // focus_follows_mouse: value.focus_follows_mouse,
            mouse_follows_focus: Option::from(value.mouse_follows_focus),
            app_specific_configuration_path: None,
            border_width: Option::from(border_manager::BORDER_WIDTH.load(Ordering::SeqCst)),
            border_offset: Option::from(border_manager::BORDER_OFFSET.load(Ordering::SeqCst)),
            border_radius: Option::from(border_manager::BORDER_RADIUS.load(Ordering::SeqCst)),
            border: Option::from(border_manager::BORDER_ENABLED.load(Ordering::SeqCst)),
            border_colours,
            // transparency: Option::from(
            //     transparency_manager::TRANSPARENCY_ENABLED.load(Ordering::SeqCst),
            // ),
            // transparency_alpha: Option::from(
            //     transparency_manager::TRANSPARENCY_ALPHA.load(Ordering::SeqCst),
            // ),
            // transparency_ignore_rules: None,
            // border_style: Option::from(STYLE.load()),
            // border_z_order: None,
            // border_implementation: Option::from(IMPLEMENTATION.load()),
            default_workspace_padding: Option::from(crate::settings::workspace_padding()),
            default_container_padding: Option::from(crate::settings::container_padding()),
            layout_defaults: {
                let guard = crate::settings::layout_defaults();
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
            display_index_preferences: Option::from(crate::settings::display_index_preferences()),
            // stackbar: None,
            animation: None,
            theme: None,
            // slow_application_compensation_time: Option::from(
            //     SLOW_APPLICATION_COMPENSATION_TIME.load(Ordering::SeqCst),
            // ),
            // slow_application_identifiers: Option::from(SLOW_APPLICATION_IDENTIFIERS.lock().clone()),
            // bar_configurations: None,
            // remove_titlebar_applications: Option::from(NO_TITLEBAR.lock().clone()),
            floating_window_aspect_ratio: Option::from(
                crate::settings::floating_window_aspect_ratio(),
            ),
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
        // The groups that have moved to `Settings` (the two default paddings, the aspect ratio of a
        // window toggled to floating, `layout_defaults` and `display_index_preferences`) are applied
        // as one update, with the rules the statics had: a missing padding or display index
        // preferences key keeps the value, a missing `layout_defaults` clears it and a missing
        // aspect ratio is Standard (`SharedDelta::apply`).
        crate::settings::settings().update(|settings| {
            SharedDelta {
                default_container_padding: self.default_container_padding,
                default_workspace_padding: self.default_workspace_padding,
                floating_window_aspect_ratio: self.floating_window_aspect_ratio,
                layout_defaults: self.layout_defaults.clone(),
                display_index_preferences: self.display_index_preferences.clone(),
                ..SharedDelta::default()
            }
            .apply(&mut settings.shared);
        });

        // if let Some(monitor_index_preferences) = &self.monitor_index_preferences {
        //     let mut preferences = MONITOR_INDEX_PREFERENCES.lock();
        //     preferences.clone_from(monitor_index_preferences);
        // }
        //

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
        // transparency_manager::TRANSPARENCY_ENABLED
        //     .store(self.transparency.unwrap_or(false), Ordering::SeqCst);
        // transparency_manager::TRANSPARENCY_ALPHA
        //     .store(self.transparency_alpha.unwrap_or(200), Ordering::SeqCst);

        // the order every other place takes them in (floating, ignore, manage, regex; see lib.rs):
        // a thread that takes two in another order can wait for one that holds what it needs
        let mut floating_applications = FLOATING_APPLICATIONS.lock();
        let mut ignore_identifiers = IGNORE_IDENTIFIERS.lock();
        let mut manage_identifiers = MANAGE_IDENTIFIERS.lock();
        let mut regex_identifiers = REGEX_IDENTIFIERS.lock();
        // let mut tray_and_multi_window_identifiers = TRAY_AND_MULTI_WINDOW_IDENTIFIERS.lock();
        // let mut object_name_change_identifiers = OBJECT_NAME_CHANGE_ON_LAUNCH.lock();
        // let mut object_name_change_title_ignore_list = OBJECT_NAME_CHANGE_TITLE_IGNORE_LIST.lock();
        // let mut layered_identifiers = LAYERED_WHITELIST.lock();
        // let mut transparency_blacklist = TRANSPARENCY_BLACKLIST.lock();
        // let mut slow_application_identifiers = SLOW_APPLICATION_IDENTIFIERS.lock();
        // let mut no_titlebar_applications = NO_TITLEBAR.lock();
        let mut titleless_applications = TITLELESS_APPLICATIONS.lock();
        let mut tabbed_applications = TABBED_APPLICATIONS.lock();

        if let Some(rules) = &mut self.ignore_rules {
            populate_rules(rules, &mut ignore_identifiers, &mut regex_identifiers)?;
        }

        if let Some(rules) = &mut self.floating_applications {
            populate_rules(rules, &mut floating_applications, &mut regex_identifiers)?;
        }

        if let Some(rules) = &mut self.manage_rules {
            populate_rules(rules, &mut manage_identifiers, &mut regex_identifiers)?;
        }

        if let Some(rules) = &mut self.tabbed_applications {
            for r in rules {
                if !tabbed_applications.contains(r) {
                    tabbed_applications.push(r.to_string());
                }
            }
        }

        if let Some(rules) = &mut self.titleless_applications {
            for r in rules {
                if !titleless_applications.contains(r) {
                    titleless_applications.push(r.to_string());
                }
            }
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
        // if let Some(rules) = &mut self.transparency_ignore_rules {
        //     populate_rules(rules, &mut transparency_blacklist, &mut regex_identifiers)?;
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
                    &mut tabbed_applications,
                    &mut titleless_applications,
                    &mut regex_identifiers,
                )?,
                AppSpecificConfigurationPath::Multiple(paths) => {
                    for path in paths {
                        handle_asc_file(
                            path,
                            &mut ignore_identifiers,
                            &mut manage_identifiers,
                            &mut floating_applications,
                            &mut tabbed_applications,
                            &mut titleless_applications,
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
        path: &Path,
        incoming: Receiver<WindowManagerEvent>,
        unix_listener: Option<UnixListener>,
        run_loop: &CFRetained<CFRunLoop>,
    ) -> eyre::Result<WindowManager>;

    /// Gives the monitors of the window manager what the file says about them
    fn postload(path: &Path, wm: &Arc<Mutex<WindowManager>>) -> eyre::Result<()>;

    /// Replaces the configuration of the window manager with the file at `path`
    fn reload(path: &Path, wm: &mut WindowManager) -> eyre::Result<()>;
}

impl StaticConfigExt for StaticConfig {
    #[allow(clippy::too_many_lines)]
    fn preload(
        path: &Path,
        incoming: Receiver<WindowManagerEvent>,
        unix_listener: Option<UnixListener>,
        run_loop: &CFRetained<CFRunLoop>,
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
            monitor_usr_idx_map: HashMap::new(),
            applications: Default::default(),
            incoming_events: incoming,
            command_listener: listener,
            space_id: current_space_id(),
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
            run_loop: CoreFoundationRunLoop(run_loop.clone()),
            minimized_windows: HashMap::new(),
            pending_move_op: Arc::new(None),
            pending_resize_op: Arc::new(None),
            live_resize: None,
            pan: None,
            already_moved_window_handles: Default::default(),
            known_window_ids: HashMap::new(),
        };

        let bytes = SocketMessage::ReloadStaticConfiguration(path.to_path_buf()).as_bytes()?;

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

    fn postload(path: &Path, wm: &Arc<Mutex<WindowManager>>) -> eyre::Result<()> {
        let mut value = Self::read(path)?;
        let mut wm = wm.lock();

        let configs_with_preference: Vec<_> = crate::settings::display_index_preferences()
            .keys()
            .copied()
            .collect();
        let mut configs_used = Vec::new();

        let mut workspace_matching_rules = WORKSPACE_MATCHING_RULES.lock();
        workspace_matching_rules.clear();
        drop(workspace_matching_rules);

        let offset = wm.work_area_offset;
        for (i, monitor) in wm.monitors_mut().iter_mut().enumerate() {
            let preferred_config_idx = {
                let display_index_preferences = crate::settings::display_index_preferences();

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
                monitor.window_hiding_position =
                    monitor_config.window_hiding_position.unwrap_or_default();

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
                    let display_index_preferences = crate::settings::display_index_preferences();
                    display_index_preferences.get(i).cloned()
                };
                if let (Some(id), Some(monitor_config)) =
                    (id, value.monitors.as_ref().and_then(|ms| ms.get(*i)))
                {
                    // The name, device, device_id and serial_number_id can be empty here since
                    // once the monitor with this preferred index actually connects the
                    // `load_monitor_information` function will update these fields.
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
                    m.window_hiding_position =
                        monitor_config.window_hiding_position.unwrap_or_default();

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

        Ok(())
    }

    fn reload(path: &Path, wm: &mut WindowManager) -> eyre::Result<()> {
        let mut value = Self::read(path)?;

        value.apply_globals()?;

        let configs_with_preference: Vec<_> = crate::settings::display_index_preferences()
            .keys()
            .copied()
            .collect();
        let mut configs_used = Vec::new();

        let mut workspace_matching_rules = WORKSPACE_MATCHING_RULES.lock();
        workspace_matching_rules.clear();
        drop(workspace_matching_rules);

        let offset = wm.work_area_offset;
        for (i, monitor) in wm.monitors_mut().iter_mut().enumerate() {
            let preferred_config_idx = {
                let display_index_preferences = crate::settings::display_index_preferences();

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
                monitor.window_hiding_position =
                    monitor_config.window_hiding_position.unwrap_or_default();

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
                    let display_index_preferences = crate::settings::display_index_preferences();
                    display_index_preferences.get(i).cloned()
                };
                if let (Some(id), Some(monitor_config)) =
                    (id, value.monitors.as_ref().and_then(|ms| ms.get(*i)))
                {
                    // The name, device, device_id and serial_number_id can be empty here since
                    // once the monitor with this preferred index actually connects the
                    // `load_monitor_information` function will update these fields.
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
                    m.window_hiding_position =
                        monitor_config.window_hiding_position.unwrap_or_default();

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
        // wm.focus_follows_mouse = value.focus_follows_mouse;

        // match wm.focus_follows_mouse {
        //     None => WindowsApi::disable_focus_follows_mouse()?,
        //     Some(FocusFollowsMouseImplementation::Windows) => {
        //         WindowsApi::enable_focus_follows_mouse()?;
        //     }
        //     Some(FocusFollowsMouseImplementation::Komorebi) => {}
        // };

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

#[allow(clippy::too_many_arguments)]
fn handle_asc_file(
    path: &PathBuf,
    ignore_identifiers: &mut Vec<MatchingRule>,
    manage_identifiers: &mut Vec<MatchingRule>,
    floating_applications: &mut Vec<MatchingRule>,
    tabbed_applications: &mut Vec<String>,
    titleless_applications: &mut Vec<String>,
    regex_identifiers: &mut HashMap<String, Regex>,
) -> eyre::Result<()> {
    match path.extension() {
        None => {}
        Some(ext) => {
            #[allow(clippy::collapsible_match)]
            // collapsing the nested if into a guard would restructure a 40-line block
            if ext.to_string_lossy().to_string().as_str() == "json" {
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

                            if let Some(rules) = &entry.tabbed {
                                for rule in rules {
                                    if let MatchingRule::Simple(simple) = rule
                                        && matches!(simple.kind, ApplicationIdentifier::Exe)
                                        && !tabbed_applications.contains(&simple.id)
                                    {
                                        tabbed_applications.push(simple.id.clone());
                                    }
                                }
                            }

                            if let Some(rules) = &entry.titleless {
                                for rule in rules {
                                    if let MatchingRule::Simple(simple) = rule
                                        && matches!(simple.kind, ApplicationIdentifier::Exe)
                                        && !titleless_applications.contains(&simple.id)
                                    {
                                        titleless_applications.push(simple.id.clone());
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
    }

    Ok(())
}
