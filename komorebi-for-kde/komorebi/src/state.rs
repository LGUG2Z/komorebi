//! [`State`], the window manager as the daemon shows it: every monitor, with its workspaces, their
//! containers and the windows in them, and the settings that apply to all of them.
//!
//! The daemon writes it as JSON in answer to [`SocketMessage::State`], and sends it to its
//! subscribers in every [`Notification`]. [`State::has_been_modified`] decides whether an event or
//! a message changed it, for the subscribers that want a notification only then.
//!
//! [`GlobalState`] is what the daemon has besides the window manager, the rules, the transparency
//! and the data directory, which it writes in answer to `GlobalState`.
//!
//! [`SocketMessage::State`]: crate::core::SocketMessage::State
//! [`Notification`]: crate::Notification

use crate::DATA_DIR;
use crate::DISPLAY_INDEX_PREFERENCES;
use crate::HOME_DIR;
use crate::IGNORE_IDENTIFIERS;
use crate::MANAGE_IDENTIFIERS;
use crate::TRANSPARENCY_BLACKLIST;
use crate::WORKSPACE_MATCHING_RULES;
use crate::core::FocusFollowsMouseImplementation;
use crate::core::MonocleFocusBehaviour;
use crate::core::MoveBehaviour;
use crate::core::OperationBehaviour;
use crate::core::Rect;
use crate::core::WindowContainerBehaviour;
use crate::core::config_generation::MatchingRule;
use crate::core::config_generation::WorkspaceMatchingRule;
use crate::monitor::Monitor;
use crate::ring::Ring;
use crate::window_manager::WindowManager;
use crate::workspace::Workspace;
use serde::Deserialize;
use serde::Serialize;
use std::collections::HashMap;
use std::collections::VecDeque;
use std::path::PathBuf;

/// What the daemon has that is not part of the window manager, as it answers
/// [`SocketMessage::GlobalState`](crate::core::SocketMessage::GlobalState): the ignore and the manage
/// rules, what the transparency is, the indices the user gave the monitors, the workspace rules,
/// and the configuration and the data directories
///
/// # Example
///
/// ```rust
/// use komorebi::state::GlobalState;
///
/// let state = GlobalState::default();
/// // plasmashell and krunner are ignored from the start
/// let json = serde_json::to_string(&state.ignore_identifiers)?;
/// assert!(json.contains(r#""id":"org.kde.plasmashell""#), "{json}");
/// assert!(state.manage_identifiers.is_empty());
/// // nothing is see-through until it is asked for
/// assert!(!state.transparency_enabled);
/// assert_eq!(state.transparency_alpha, 200);
/// assert!(state.display_index_preferences.is_empty());
/// assert!(state.workspace_rules.is_empty());
/// assert_eq!(state.configuration_dir, *komorebi::HOME_DIR);
/// assert_eq!(state.data_dir, *komorebi::DATA_DIR);
///
/// # Ok::<(), Box<dyn std::error::Error>>(())
/// ```
#[allow(clippy::struct_excessive_bools)]
#[derive(Debug, Serialize, Deserialize)]
pub struct GlobalState {
    // pub border_enabled: bool,
    // pub border_colours: BorderColours,
    // pub border_style: BorderStyle,
    // pub border_offset: i32,
    // pub border_width: i32,
    // pub stackbar_mode: StackbarMode,
    // pub stackbar_label: StackbarLabel,
    // pub stackbar_focused_text_colour: Colour,
    // pub stackbar_unfocused_text_colour: Colour,
    // pub stackbar_tab_background_colour: Colour,
    // pub stackbar_tab_width: i32,
    // pub stackbar_height: i32,
    // pub remove_titlebars: bool,
    /// Whether the windows that do not have the focus are see-through
    pub transparency_enabled: bool,
    /// How opaque a see-through window is, from 0 to 255
    pub transparency_alpha: u8,
    /// The rules for the windows that are never see-through
    pub transparency_blacklist: Vec<MatchingRule>,
    // #[serde(alias = "float_identifiers")]
    pub ignore_identifiers: Vec<MatchingRule>,
    pub manage_identifiers: Vec<MatchingRule>,
    // pub layered_whitelist: Vec<MatchingRule>,
    // pub tray_and_multi_window_identifiers: Vec<MatchingRule>,
    // pub name_change_on_launch_identifiers: Vec<MatchingRule>,
    // pub monitor_index_preferences: HashMap<usize, Rect>,
    pub display_index_preferences: HashMap<usize, String>,
    // pub ignored_duplicate_monitor_serial_ids: Vec<String>,
    pub workspace_rules: Vec<WorkspaceMatchingRule>,
    // pub window_hiding_behaviour: HidingBehaviour,
    pub configuration_dir: PathBuf,
    pub data_dir: PathBuf,
    // pub custom_ffm: bool,
}

impl Default for GlobalState {
    fn default() -> Self {
        Self {
            transparency_enabled: crate::transparency_manager::TRANSPARENCY_ENABLED
                .load(std::sync::atomic::Ordering::SeqCst),
            transparency_alpha: crate::transparency_manager::TRANSPARENCY_ALPHA
                .load(std::sync::atomic::Ordering::SeqCst),
            transparency_blacklist: TRANSPARENCY_BLACKLIST.lock().clone(),
            ignore_identifiers: IGNORE_IDENTIFIERS.lock().clone(),
            manage_identifiers: MANAGE_IDENTIFIERS.lock().clone(),
            display_index_preferences: DISPLAY_INDEX_PREFERENCES.read().clone(),
            workspace_rules: WORKSPACE_MATCHING_RULES.lock().clone(),
            configuration_dir: HOME_DIR.clone(),
            data_dir: DATA_DIR.clone(),
        }
    }
}

/// A copy of the window manager's monitors, with their workspaces, containers and windows, and
/// of its settings, which serializes to JSON
///
/// # Example
///
/// ```rust
/// use std::path::PathBuf;
/// use std::sync::Arc;
///
/// use komorebi::backend::fake::FakeBackend;
/// use komorebi::backend::with_backend;
/// use komorebi::state::State;
/// use komorebi::window_manager::WindowManager;
///
/// let fake = Arc::new(FakeBackend::with_one_monitor());
/// fake.open_window(9, "foot", "foot", "a terminal");
/// let (_sender, events) = crossbeam_channel::bounded(1);
/// let socket = PathBuf::from(format!("/tmp/komorebi-state-doc-{}.sock", std::process::id()));
///
/// let state = with_backend(fake, || -> color_eyre::Result<State> {
///     let mut wm = WindowManager::new(events, Some(socket.clone()))?;
///     wm.init()?;
///     Ok(State::from(&wm))
/// })?;
/// # std::fs::remove_file(&socket)?;
///
/// let workspace = &state.monitors.elements()[0].workspaces()[0];
/// assert_eq!(workspace.containers()[0].windows()[0].id, 9);
///
/// # Ok::<(), Box<dyn std::error::Error>>(())
/// ```
#[allow(clippy::struct_excessive_bools)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct State {
    pub monitors: Ring<Monitor>,
    pub monitor_usr_idx_map: HashMap<usize, usize>,
    pub is_paused: bool,
    pub resize_delta: i32,
    pub new_window_behaviour: WindowContainerBehaviour,
    pub float_override: bool,
    pub cross_monitor_move_behaviour: MoveBehaviour,
    pub monocle_focus_behaviour: MonocleFocusBehaviour,
    pub unmanaged_window_operation_behaviour: OperationBehaviour,
    pub work_area_offset: Option<Rect>,
    pub focus_follows_mouse: Option<FocusFollowsMouseImplementation>,
    pub mouse_follows_focus: bool,
}

impl From<&WindowManager> for State {
    fn from(wm: &WindowManager) -> Self {
        // Every field of a monitor and of a workspace is copied by name here, so that what does
        // not need to be shown in the state can be left out of the copy.
        let mut stripped_monitors = Ring::default();
        *stripped_monitors.elements_mut() = wm
            .monitors()
            .iter()
            .map(|monitor| Monitor {
                id: monitor.id,
                device: monitor.device.clone(),
                serial_number_id: monitor.serial_number_id.clone(),
                size: monitor.size,
                work_area_size: monitor.work_area_size,
                work_area_offset: monitor.work_area_offset,
                window_based_work_area_offset: monitor.window_based_work_area_offset,
                window_based_work_area_offset_limit: monitor.window_based_work_area_offset_limit,
                workspaces: {
                    let mut ws = Ring::default();
                    *ws.elements_mut() = monitor
                        .workspaces()
                        .iter()
                        .map(|workspace| Workspace {
                            name: workspace.name.clone(),
                            containers: workspace.containers.clone(),
                            monocle_container: workspace.monocle_container.clone(),
                            monocle_container_restore_idx: workspace.monocle_container_restore_idx,
                            maximized_window: workspace.maximized_window,
                            maximized_window_restore_idx: workspace.maximized_window_restore_idx,
                            floating_windows: workspace.floating_windows.clone(),
                            layout: workspace.layout.clone(),
                            layout_options: workspace.layout_options,
                            layout_rules: workspace.layout_rules.clone(),
                            layout_options_rules: workspace.layout_options_rules.clone(),
                            layout_defaults_cache: workspace.layout_defaults_cache.clone(),
                            layout_flip: workspace.layout_flip,
                            workspace_padding: workspace.workspace_padding,
                            container_padding: workspace.container_padding,
                            latest_layout: workspace.latest_layout.clone(),
                            resize_dimensions: workspace.resize_dimensions.clone(),
                            tile: workspace.tile,
                            work_area_offset: workspace.work_area_offset,
                            work_area_offset_rules: workspace.work_area_offset_rules.clone(),
                            apply_window_based_work_area_offset: workspace
                                .apply_window_based_work_area_offset,
                            window_container_behaviour: workspace.window_container_behaviour,
                            window_container_behaviour_rules: workspace
                                .window_container_behaviour_rules
                                .clone(),
                            float_override: workspace.float_override,
                            layer: workspace.layer,
                            floating_layer_behaviour: workspace.floating_layer_behaviour,
                            globals: workspace.globals,
                            wallpaper: workspace.wallpaper.clone(),
                            workspace_config: None,
                            preselected_container_idx: None,
                            initial_window_placement_rules: workspace
                                .initial_window_placement_rules
                                .clone(),
                            promotion_swap_container_idx: None,
                            scrolling: None,
                            scrolling_placed: None,
                        })
                        .collect::<VecDeque<_>>();
                    ws.focus(monitor.workspaces.focused_idx());
                    ws
                },
                last_focused_workspace: monitor.last_focused_workspace,
                // workspace_names: monitor.workspace_names.clone(),
                container_padding: monitor.container_padding,
                workspace_padding: monitor.workspace_padding,
                wallpaper: monitor.wallpaper.clone(),
                floating_layer_behaviour: monitor.floating_layer_behaviour,
            })
            .collect::<VecDeque<_>>();
        stripped_monitors.focus(wm.monitors.focused_idx());

        Self {
            monitors: stripped_monitors,
            monitor_usr_idx_map: wm.monitor_usr_idx_map.clone(),
            is_paused: wm.is_paused,
            work_area_offset: wm.work_area_offset,
            resize_delta: wm.resize_delta,
            new_window_behaviour: wm.window_management_behaviour.current_behaviour,
            float_override: wm.window_management_behaviour.float_override,
            cross_monitor_move_behaviour: wm.cross_monitor_move_behaviour,
            monocle_focus_behaviour: wm.monocle_focus_behaviour,
            focus_follows_mouse: wm.focus_follows_mouse,
            mouse_follows_focus: wm.mouse_follows_focus,
            unmanaged_window_operation_behaviour: wm.unmanaged_window_operation_behaviour,
        }
    }
}

impl State {
    /// Returns true if and only if the window manager's monitors (with their workspaces,
    /// containers and windows), whether it is paused, its new window behavior, its float override,
    /// its monocle focus behavior, its unmanaged window operation behavior, its work area offset,
    /// or its focus follows mouse or mouse follows focus setting differ from this state's
    ///
    /// A window is compared by its id only, so a window that moved or was renamed is not a
    /// change.
    pub fn has_been_modified(&self, wm: &WindowManager) -> bool {
        let new = Self::from(wm);

        if self.monitors != new.monitors {
            return true;
        }

        if self.is_paused != new.is_paused {
            return true;
        }

        if self.new_window_behaviour != new.new_window_behaviour {
            return true;
        }

        if self.float_override != new.float_override {
            return true;
        }

        if self.cross_monitor_move_behaviour != new.cross_monitor_move_behaviour {
            return true;
        }

        if self.monocle_focus_behaviour != new.monocle_focus_behaviour {
            return true;
        }

        if self.unmanaged_window_operation_behaviour != new.unmanaged_window_operation_behaviour {
            return true;
        }

        if self.work_area_offset != new.work_area_offset {
            return true;
        }

        if self.focus_follows_mouse != new.focus_follows_mouse {
            return true;
        }

        if self.mouse_follows_focus != new.mouse_follows_focus {
            return true;
        }

        false
    }
}
