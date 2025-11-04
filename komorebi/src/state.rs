use crate::BorderColours;
use crate::BorderStyle;
use crate::CURRENT_VIRTUAL_DESKTOP;
use crate::CUSTOM_FFM;
use crate::DATA_DIR;
use crate::DISPLAY_INDEX_PREFERENCES;
use crate::DUPLICATE_MONITOR_SERIAL_IDS;
use crate::FocusFollowsMouseImplementation;
use crate::HIDING_BEHAVIOUR;
use crate::HOME_DIR;
use crate::HidingBehaviour;
use crate::IGNORE_IDENTIFIERS;
use crate::LAYERED_WHITELIST;
use crate::MANAGE_IDENTIFIERS;
use crate::MONITOR_INDEX_PREFERENCES;
use crate::MoveBehaviour;
use crate::OBJECT_NAME_CHANGE_ON_LAUNCH;
use crate::OperationBehaviour;
use crate::REMOVE_TITLEBARS;
use crate::Rect;
use crate::StackbarLabel;
use crate::StackbarMode;
use crate::TRANSPARENCY_BLACKLIST;
use crate::TRAY_AND_MULTI_WINDOW_IDENTIFIERS;
use crate::WORKSPACE_MATCHING_RULES;
use crate::WindowContainerBehaviour;
use crate::WindowManager;
use crate::border_manager;
use crate::border_manager::STYLE;
use crate::config_generation::MatchingRule;
use crate::config_generation::WorkspaceMatchingRule;
use crate::monitor::Monitor;
use crate::ring::Ring;
use crate::stackbar_manager::STACKBAR_FOCUSED_TEXT_COLOUR;
use crate::stackbar_manager::STACKBAR_LABEL;
use crate::stackbar_manager::STACKBAR_MODE;
use crate::stackbar_manager::STACKBAR_TAB_BACKGROUND_COLOUR;
use crate::stackbar_manager::STACKBAR_TAB_HEIGHT;
use crate::stackbar_manager::STACKBAR_TAB_WIDTH;
use crate::stackbar_manager::STACKBAR_UNFOCUSED_TEXT_COLOUR;
use crate::transparency_manager::TRANSPARENCY_ALPHA;
use crate::transparency_manager::TRANSPARENCY_ENABLED;
use crate::workspace::Workspace;
use komorebi_themes::colour::Colour;
use komorebi_themes::colour::Rgb;
use serde::Deserialize;
use serde::Serialize;
use std::collections::HashMap;
use std::collections::VecDeque;
use std::path::PathBuf;
use std::sync::atomic::Ordering;

#[allow(clippy::struct_excessive_bools)]
#[derive(Clone, Debug, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
pub struct State {
    pub monitors: Ring<Monitor>,
    pub monitor_usr_idx_map: HashMap<usize, usize>,
    pub is_paused: bool,
    pub resize_delta: i32,
    pub new_window_behaviour: WindowContainerBehaviour,
    pub float_override: bool,
    pub cross_monitor_move_behaviour: MoveBehaviour,
    pub unmanaged_window_operation_behaviour: OperationBehaviour,
    pub work_area_offset: Option<Rect>,
    pub focus_follows_mouse: Option<FocusFollowsMouseImplementation>,
    pub mouse_follows_focus: bool,
    pub has_pending_raise_op: bool,
    pub virtual_desktop_id: Option<Vec<u8>>,
}

impl State {
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

        if self.has_pending_raise_op != new.has_pending_raise_op {
            return true;
        }

        false
    }
}

#[allow(clippy::struct_excessive_bools)]
#[derive(Debug, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
pub struct GlobalState {
    pub border_enabled: bool,
    pub border_colours: BorderColours,
    pub border_style: BorderStyle,
    pub border_offset: i32,
    pub border_width: i32,
    pub stackbar_mode: StackbarMode,
    pub stackbar_label: StackbarLabel,
    pub stackbar_focused_text_colour: Colour,
    pub stackbar_unfocused_text_colour: Colour,
    pub stackbar_tab_background_colour: Colour,
    pub stackbar_tab_width: i32,
    pub stackbar_height: i32,
    pub transparency_enabled: bool,
    pub transparency_alpha: u8,
    pub transparency_blacklist: Vec<MatchingRule>,
    pub remove_titlebars: bool,
    #[serde(alias = "float_identifiers")]
    pub ignore_identifiers: Vec<MatchingRule>,
    pub manage_identifiers: Vec<MatchingRule>,
    pub layered_whitelist: Vec<MatchingRule>,
    pub tray_and_multi_window_identifiers: Vec<MatchingRule>,
    pub name_change_on_launch_identifiers: Vec<MatchingRule>,
    pub monitor_index_preferences: HashMap<usize, Rect>,
    pub display_index_preferences: HashMap<usize, String>,
    pub ignored_duplicate_monitor_serial_ids: Vec<String>,
    pub workspace_rules: Vec<WorkspaceMatchingRule>,
    pub window_hiding_behaviour: HidingBehaviour,
    pub configuration_dir: PathBuf,
    pub data_dir: PathBuf,
    pub custom_ffm: bool,
    pub current_virtual_desktop_id: Option<Vec<u8>>,
}

impl Default for GlobalState {
    fn default() -> Self {
        Self {
            border_enabled: border_manager::BORDER_ENABLED.load(Ordering::SeqCst),
            border_colours: BorderColours {
                single: Option::from(Colour::Rgb(Rgb::from(
                    border_manager::FOCUSED.load(Ordering::SeqCst),
                ))),
                stack: Option::from(Colour::Rgb(Rgb::from(
                    border_manager::STACK.load(Ordering::SeqCst),
                ))),
                monocle: Option::from(Colour::Rgb(Rgb::from(
                    border_manager::MONOCLE.load(Ordering::SeqCst),
                ))),
                floating: Option::from(Colour::Rgb(Rgb::from(
                    border_manager::FLOATING.load(Ordering::SeqCst),
                ))),
                unfocused: Option::from(Colour::Rgb(Rgb::from(
                    border_manager::UNFOCUSED.load(Ordering::SeqCst),
                ))),
                unfocused_locked: Option::from(Colour::Rgb(Rgb::from(
                    border_manager::UNFOCUSED_LOCKED.load(Ordering::SeqCst),
                ))),
            },
            border_style: STYLE.load(),
            border_offset: border_manager::BORDER_OFFSET.load(Ordering::SeqCst),
            border_width: border_manager::BORDER_WIDTH.load(Ordering::SeqCst),
            stackbar_mode: STACKBAR_MODE.load(),
            stackbar_label: STACKBAR_LABEL.load(),
            stackbar_focused_text_colour: Colour::Rgb(Rgb::from(
                STACKBAR_FOCUSED_TEXT_COLOUR.load(Ordering::SeqCst),
            )),
            stackbar_unfocused_text_colour: Colour::Rgb(Rgb::from(
                STACKBAR_UNFOCUSED_TEXT_COLOUR.load(Ordering::SeqCst),
            )),
            stackbar_tab_background_colour: Colour::Rgb(Rgb::from(
                STACKBAR_TAB_BACKGROUND_COLOUR.load(Ordering::SeqCst),
            )),
            stackbar_tab_width: STACKBAR_TAB_WIDTH.load(Ordering::SeqCst),
            stackbar_height: STACKBAR_TAB_HEIGHT.load(Ordering::SeqCst),
            transparency_enabled: TRANSPARENCY_ENABLED.load(Ordering::SeqCst),
            transparency_alpha: TRANSPARENCY_ALPHA.load(Ordering::SeqCst),
            transparency_blacklist: TRANSPARENCY_BLACKLIST.lock().clone(),
            remove_titlebars: REMOVE_TITLEBARS.load(Ordering::SeqCst),
            ignore_identifiers: IGNORE_IDENTIFIERS.lock().clone(),
            manage_identifiers: MANAGE_IDENTIFIERS.lock().clone(),
            layered_whitelist: LAYERED_WHITELIST.lock().clone(),
            tray_and_multi_window_identifiers: TRAY_AND_MULTI_WINDOW_IDENTIFIERS.lock().clone(),
            name_change_on_launch_identifiers: OBJECT_NAME_CHANGE_ON_LAUNCH.lock().clone(),
            monitor_index_preferences: MONITOR_INDEX_PREFERENCES.lock().clone(),
            display_index_preferences: DISPLAY_INDEX_PREFERENCES.read().clone(),
            ignored_duplicate_monitor_serial_ids: DUPLICATE_MONITOR_SERIAL_IDS.read().clone(),
            workspace_rules: WORKSPACE_MATCHING_RULES.lock().clone(),
            window_hiding_behaviour: *HIDING_BEHAVIOUR.lock(),
            configuration_dir: HOME_DIR.clone(),
            data_dir: DATA_DIR.clone(),
            custom_ffm: CUSTOM_FFM.load(Ordering::SeqCst),
            current_virtual_desktop_id: CURRENT_VIRTUAL_DESKTOP.lock().clone(),
        }
    }
}

impl From<&WindowManager> for State {
    fn from(wm: &WindowManager) -> Self {
        // This is used to remove any information that doesn't need to be passed on to subscribers
        // or to be shown with the `komorebic state` command. Currently it is only removing the
        // `workspace_config` field from every workspace, but more stripping can be added later if
        // needed.
        let mut stripped_monitors = Ring::default();
        *stripped_monitors.elements_mut() = wm
            .monitors()
            .iter()
            .map(|monitor| Monitor {
                id: monitor.id,
                name: monitor.name.clone(),
                device: monitor.device.clone(),
                device_id: monitor.device_id.clone(),
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
                            layout_flip: workspace.layout_flip,
                            workspace_padding: workspace.workspace_padding,
                            container_padding: workspace.container_padding,
                            latest_layout: workspace.latest_layout.clone(),
                            resize_dimensions: workspace.resize_dimensions.clone(),
                            tile: workspace.tile,
                            work_area_offset: workspace.work_area_offset,
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
                            promotion_swap_container_idx: None,
                        })
                        .collect::<VecDeque<_>>();
                    ws.focus(monitor.workspaces.focused_idx());
                    ws
                },
                last_focused_workspace: monitor.last_focused_workspace,
                workspace_names: monitor.workspace_names.clone(),
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
            focus_follows_mouse: wm.focus_follows_mouse,
            mouse_follows_focus: wm.mouse_follows_focus,
            has_pending_raise_op: wm.has_pending_raise_op,
            unmanaged_window_operation_behaviour: wm.unmanaged_window_operation_behaviour,
            virtual_desktop_id: wm.virtual_desktop_id.clone(),
        }
    }
}
