use std::collections::HashMap;
use std::collections::HashSet;
use std::collections::VecDeque;
use std::env::temp_dir;
use std::fs::OpenOptions;
use std::io::ErrorKind;
use std::net::Shutdown;
use std::num::NonZeroUsize;
use std::path::Path;
use std::path::PathBuf;
use std::sync::atomic::Ordering;
use std::sync::Arc;

use color_eyre::eyre::anyhow;
use color_eyre::eyre::bail;
use color_eyre::Result;
use crossbeam_channel::Receiver;
use hotwatch::notify::ErrorKind as NotifyErrorKind;
use hotwatch::EventKind;
use hotwatch::Hotwatch;
use parking_lot::Mutex;
use serde::Deserialize;
use serde::Serialize;
use uds_windows::UnixListener;
use uds_windows::UnixStream;

use crate::animation::AnimationEngine;
use crate::animation::ANIMATION_ENABLED_GLOBAL;
use crate::animation::ANIMATION_ENABLED_PER_ANIMATION;
use crate::core::config_generation::MatchingRule;
use crate::core::custom_layout::CustomLayout;
use crate::core::Arrangement;
use crate::core::Axis;
use crate::core::BorderImplementation;
use crate::core::BorderStyle;
use crate::core::CycleDirection;
use crate::core::DefaultLayout;
use crate::core::FocusFollowsMouseImplementation;
use crate::core::HidingBehaviour;
use crate::core::Layout;
use crate::core::MoveBehaviour;
use crate::core::OperationBehaviour;
use crate::core::OperationDirection;
use crate::core::Rect;
use crate::core::Sizing;
use crate::core::StackbarLabel;
use crate::core::WindowContainerBehaviour;
use crate::core::WindowManagementBehaviour;

use crate::border_manager;
use crate::border_manager::BORDER_OFFSET;
use crate::border_manager::BORDER_WIDTH;
use crate::border_manager::STYLE;
use crate::config_generation::WorkspaceMatchingRule;
use crate::container::Container;
use crate::core::StackbarMode;
use crate::current_virtual_desktop;
use crate::load_configuration;
use crate::monitor::Monitor;
use crate::ring::Ring;
use crate::should_act;
use crate::should_act_individual;
use crate::stackbar_manager::STACKBAR_FOCUSED_TEXT_COLOUR;
use crate::stackbar_manager::STACKBAR_LABEL;
use crate::stackbar_manager::STACKBAR_MODE;
use crate::stackbar_manager::STACKBAR_TAB_BACKGROUND_COLOUR;
use crate::stackbar_manager::STACKBAR_TAB_HEIGHT;
use crate::stackbar_manager::STACKBAR_TAB_WIDTH;
use crate::stackbar_manager::STACKBAR_UNFOCUSED_TEXT_COLOUR;
use crate::static_config::StaticConfig;
use crate::transparency_manager;
use crate::transparency_manager::TRANSPARENCY_ALPHA;
use crate::transparency_manager::TRANSPARENCY_ENABLED;
use crate::window::Window;
use crate::window_manager_event::WindowManagerEvent;
use crate::windows_api::WindowsApi;
use crate::winevent_listener;
use crate::workspace::Workspace;
use crate::workspace::WorkspaceLayer;
use crate::BorderColours;
use crate::Colour;
use crate::CrossBoundaryBehaviour;
use crate::Rgb;
use crate::CUSTOM_FFM;
use crate::DATA_DIR;
use crate::DISPLAY_INDEX_PREFERENCES;
use crate::DUPLICATE_MONITOR_SERIAL_IDS;
use crate::HIDING_BEHAVIOUR;
use crate::HOME_DIR;
use crate::IGNORE_IDENTIFIERS;
use crate::LAYERED_WHITELIST;
use crate::MANAGE_IDENTIFIERS;
use crate::MONITOR_INDEX_PREFERENCES;
use crate::NO_TITLEBAR;
use crate::OBJECT_NAME_CHANGE_ON_LAUNCH;
use crate::REGEX_IDENTIFIERS;
use crate::REMOVE_TITLEBARS;
use crate::SUBSCRIPTION_SOCKETS;
use crate::TRANSPARENCY_BLACKLIST;
use crate::TRAY_AND_MULTI_WINDOW_IDENTIFIERS;
use crate::WORKSPACE_MATCHING_RULES;

#[derive(Debug)]
pub struct WindowManager {
    pub monitors: Ring<Monitor>,
    pub monitor_usr_idx_map: HashMap<usize, usize>,
    pub incoming_events: Receiver<WindowManagerEvent>,
    pub command_listener: UnixListener,
    pub is_paused: bool,
    pub work_area_offset: Option<Rect>,
    pub resize_delta: i32,
    pub window_management_behaviour: WindowManagementBehaviour,
    pub cross_monitor_move_behaviour: MoveBehaviour,
    pub cross_boundary_behaviour: CrossBoundaryBehaviour,
    pub unmanaged_window_operation_behaviour: OperationBehaviour,
    pub focus_follows_mouse: Option<FocusFollowsMouseImplementation>,
    pub mouse_follows_focus: bool,
    pub hotwatch: Hotwatch,
    pub virtual_desktop_id: Option<Vec<u8>>,
    pub has_pending_raise_op: bool,
    pub pending_move_op: Arc<Option<(usize, usize, isize)>>,
    pub already_moved_window_handles: Arc<Mutex<HashSet<isize>>>,
    pub uncloack_to_ignore: usize,
    /// Maps each known window hwnd to the (monitor, workspace) index pair managing it
    pub known_hwnds: HashMap<isize, (usize, usize)>,
}

#[allow(clippy::struct_excessive_bools)]
#[derive(Debug, Serialize, Deserialize)]
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
        }
    }
}

impl AsRef<Self> for WindowManager {
    fn as_ref(&self) -> &Self {
        self
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
        }
    }
}

impl_ring_elements!(WindowManager, Monitor);

#[derive(Debug, Clone, Copy)]
struct EnforceWorkspaceRuleOp {
    hwnd: isize,
    origin_monitor_idx: usize,
    origin_workspace_idx: usize,
    target_monitor_idx: usize,
    target_workspace_idx: usize,
    floating: bool,
}
impl EnforceWorkspaceRuleOp {
    const fn is_origin(&self, monitor_idx: usize, workspace_idx: usize) -> bool {
        self.origin_monitor_idx == monitor_idx && self.origin_workspace_idx == workspace_idx
    }

    const fn is_target(&self, monitor_idx: usize, workspace_idx: usize) -> bool {
        self.target_monitor_idx == monitor_idx && self.target_workspace_idx == workspace_idx
    }

    const fn is_enforced(&self) -> bool {
        (self.origin_monitor_idx == self.target_monitor_idx)
            && (self.origin_workspace_idx == self.target_workspace_idx)
    }
}

impl WindowManager {
    #[tracing::instrument]
    pub fn new(
        incoming: Receiver<WindowManagerEvent>,
        custom_socket_path: Option<PathBuf>,
    ) -> Result<Self> {
        let socket = custom_socket_path.unwrap_or_else(|| DATA_DIR.join("komorebi.sock"));

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

        Ok(Self {
            monitors: Ring::default(),
            monitor_usr_idx_map: HashMap::new(),
            incoming_events: incoming,
            command_listener: listener,
            is_paused: false,
            virtual_desktop_id: current_virtual_desktop(),
            work_area_offset: None,
            window_management_behaviour: WindowManagementBehaviour::default(),
            cross_monitor_move_behaviour: MoveBehaviour::Swap,
            cross_boundary_behaviour: CrossBoundaryBehaviour::Monitor,
            unmanaged_window_operation_behaviour: OperationBehaviour::Op,
            resize_delta: 50,
            focus_follows_mouse: None,
            mouse_follows_focus: true,
            hotwatch: Hotwatch::new()?,
            has_pending_raise_op: false,
            pending_move_op: Arc::new(None),
            already_moved_window_handles: Arc::new(Mutex::new(HashSet::new())),
            uncloack_to_ignore: 0,
            known_hwnds: HashMap::new(),
        })
    }

    #[tracing::instrument(skip(self))]
    pub fn init(&mut self) -> Result<()> {
        tracing::info!("initialising");
        WindowsApi::load_monitor_information(self)?;
        WindowsApi::load_workspace_information(&mut self.monitors)
    }

    #[tracing::instrument(skip(self, state))]
    pub fn apply_state(&mut self, state: State) {
        let mut can_apply = true;

        let state_monitors_len = state.monitors.elements().len();
        let current_monitors_len = self.monitors.elements().len();
        if state_monitors_len != current_monitors_len {
            tracing::warn!(
                "cannot apply state from {}; state file has {state_monitors_len} monitors, but only {current_monitors_len} are currently connected",
                temp_dir().join("komorebi.state.json").to_string_lossy()
            );

            return;
        }

        for monitor in state.monitors.elements() {
            for workspace in monitor.workspaces() {
                for container in workspace.containers() {
                    for window in container.windows() {
                        if window.exe().is_err() {
                            can_apply = false;
                            break;
                        }
                    }
                }

                if let Some(window) = workspace.maximized_window() {
                    if window.exe().is_err() {
                        can_apply = false;
                        break;
                    }
                }

                if let Some(container) = workspace.monocle_container() {
                    for window in container.windows() {
                        if window.exe().is_err() {
                            can_apply = false;
                            break;
                        }
                    }
                }

                for window in workspace.floating_windows() {
                    if window.exe().is_err() {
                        can_apply = false;
                        break;
                    }
                }
            }
        }

        if can_apply {
            tracing::info!(
                "applying state from {}",
                temp_dir().join("komorebi.state.json").to_string_lossy()
            );

            let offset = self.work_area_offset;
            let mouse_follows_focus = self.mouse_follows_focus;
            for (monitor_idx, monitor) in self.monitors_mut().iter_mut().enumerate() {
                let mut focused_workspace = 0;
                for (workspace_idx, workspace) in monitor.workspaces_mut().iter_mut().enumerate() {
                    if let Some(state_monitor) = state.monitors.elements().get(monitor_idx) {
                        if let Some(state_workspace) = state_monitor.workspaces().get(workspace_idx)
                        {
                            // to make sure padding changes get applied for users after a quick restart
                            let container_padding = workspace.container_padding();
                            let workspace_padding = workspace.workspace_padding();

                            *workspace = state_workspace.clone();

                            workspace.set_container_padding(container_padding);
                            workspace.set_workspace_padding(workspace_padding);

                            if state_monitor.focused_workspace_idx() == workspace_idx {
                                focused_workspace = workspace_idx;
                            }
                        }
                    }
                }

                if let Err(error) = monitor.focus_workspace(focused_workspace) {
                    tracing::warn!(
                        "cannot focus workspace '{focused_workspace}' on monitor '{monitor_idx}' from {}: {}",
                        temp_dir().join("komorebi.state.json").to_string_lossy(),
                        error,
                    );
                }

                if let Err(error) = monitor.load_focused_workspace(mouse_follows_focus) {
                    tracing::warn!(
                        "cannot load focused workspace '{focused_workspace}' on monitor '{monitor_idx}' from {}: {}",
                        temp_dir().join("komorebi.state.json").to_string_lossy(),
                        error,
                    );
                }

                if let Err(error) = monitor.update_focused_workspace(offset) {
                    tracing::warn!(
                        "cannot update workspace '{focused_workspace}' on monitor '{monitor_idx}' from {}: {}",
                        temp_dir().join("komorebi.state.json").to_string_lossy(),
                        error,
                    );
                }
            }

            let focused_monitor_idx = state.monitors.focused_idx();
            let focused_workspace_idx = state
                .monitors
                .elements()
                .get(focused_monitor_idx)
                .map(|m| m.focused_workspace_idx())
                .unwrap_or_default();

            if let Err(error) = self.focus_monitor(focused_monitor_idx) {
                tracing::warn!(
                    "cannot focus monitor '{focused_monitor_idx}' from {}: {}",
                    temp_dir().join("komorebi.state.json").to_string_lossy(),
                    error,
                );
            }

            if let Err(error) = self.focus_workspace(focused_workspace_idx) {
                tracing::warn!(
                    "cannot focus workspace '{focused_workspace_idx}' on monitor '{focused_monitor_idx}' from {}: {}",
                    temp_dir().join("komorebi.state.json").to_string_lossy(),
                    error,
                );
            }

            if let Err(error) = self.update_focused_workspace(true, true) {
                tracing::warn!(
                    "cannot update focused workspace '{focused_workspace_idx}' on monitor '{focused_monitor_idx}' from {}: {}",
                    temp_dir().join("komorebi.state.json").to_string_lossy(),
                    error,
                );
            }
        } else {
            tracing::warn!(
                "cannot apply state from {}; some windows referenced in the state file no longer exist",
                temp_dir().join("komorebi.state.json").to_string_lossy()
            );
        }
    }

    #[tracing::instrument]
    pub fn reload_configuration() {
        tracing::info!("reloading configuration");
        std::thread::spawn(|| load_configuration().expect("could not load configuration"));
    }

    #[tracing::instrument(skip(self))]
    pub fn reload_static_configuration(&mut self, pathbuf: &PathBuf) -> Result<()> {
        tracing::info!("reloading static configuration");
        StaticConfig::reload(pathbuf, self)
    }

    pub fn window_management_behaviour(
        &self,
        monitor_idx: usize,
        workspace_idx: usize,
    ) -> WindowManagementBehaviour {
        if let Some(monitor) = self.monitors().get(monitor_idx) {
            if let Some(workspace) = monitor.workspaces().get(workspace_idx) {
                let current_behaviour =
                    if let Some(behaviour) = workspace.window_container_behaviour() {
                        if workspace.containers().is_empty()
                            && matches!(behaviour, WindowContainerBehaviour::Append)
                        {
                            // You can't append to an empty workspace
                            WindowContainerBehaviour::Create
                        } else {
                            *behaviour
                        }
                    } else if workspace.containers().is_empty()
                        && matches!(
                            self.window_management_behaviour.current_behaviour,
                            WindowContainerBehaviour::Append
                        )
                    {
                        // You can't append to an empty workspace
                        WindowContainerBehaviour::Create
                    } else {
                        self.window_management_behaviour.current_behaviour
                    };

                let float_override = if let Some(float_override) = workspace.float_override() {
                    *float_override
                } else {
                    self.window_management_behaviour.float_override
                };

                let floating_layer_behaviour =
                    if let Some(behaviour) = workspace.floating_layer_behaviour() {
                        behaviour
                    } else {
                        monitor
                            .floating_layer_behaviour()
                            .unwrap_or(self.window_management_behaviour.floating_layer_behaviour)
                    };

                // If the workspace layer is `Floating` and the floating layer behaviour should
                // float then change floating_layer_override to true so that new windows spawn
                // as floating
                let floating_layer_override = matches!(workspace.layer, WorkspaceLayer::Floating)
                    && floating_layer_behaviour.should_float();

                return WindowManagementBehaviour {
                    current_behaviour,
                    float_override,
                    floating_layer_override,
                    floating_layer_behaviour,
                    toggle_float_placement: self.window_management_behaviour.toggle_float_placement,
                    floating_layer_placement: self
                        .window_management_behaviour
                        .floating_layer_placement,
                    float_override_placement: self
                        .window_management_behaviour
                        .float_override_placement,
                    float_rule_placement: self.window_management_behaviour.float_rule_placement,
                };
            }
        }

        WindowManagementBehaviour {
            current_behaviour: WindowContainerBehaviour::Create,
            float_override: self.window_management_behaviour.float_override,
            floating_layer_override: self.window_management_behaviour.floating_layer_override,
            floating_layer_behaviour: self.window_management_behaviour.floating_layer_behaviour,
            toggle_float_placement: self.window_management_behaviour.toggle_float_placement,
            floating_layer_placement: self.window_management_behaviour.floating_layer_placement,
            float_override_placement: self.window_management_behaviour.float_override_placement,
            float_rule_placement: self.window_management_behaviour.float_rule_placement,
        }
    }

    #[tracing::instrument(skip(self))]
    pub fn watch_configuration(&mut self, enable: bool) -> Result<()> {
        let config_pwsh = HOME_DIR.join("komorebi.ps1");
        let config_ahk = HOME_DIR.join("komorebi.ahk");

        if config_pwsh.exists() {
            self.configure_watcher(enable, config_pwsh)?;
        } else if config_ahk.exists() {
            self.configure_watcher(enable, config_ahk)?;
        }

        Ok(())
    }

    fn configure_watcher(&mut self, enable: bool, config: PathBuf) -> Result<()> {
        if enable {
            tracing::info!("watching configuration for changes: {}", config.display());
            // Always make absolutely sure that there isn't an already existing watch, because
            // hotwatch allows multiple watches to be registered for the same path
            match self.hotwatch.unwatch(&config) {
                Ok(()) => {}
                Err(error) => match error {
                    hotwatch::Error::Notify(ref notify_error) => match notify_error.kind {
                        NotifyErrorKind::WatchNotFound => {}
                        _ => return Err(error.into()),
                    },
                    error @ hotwatch::Error::Io(_) => return Err(error.into()),
                },
            }

            self.hotwatch.watch(config, |event| match event.kind {
                // Editing in Notepad sends a NoticeWrite while editing in (Neo)Vim sends
                // a NoticeRemove, presumably because of the use of swap files?
                EventKind::Modify(_) | EventKind::Remove(_) => {
                    std::thread::spawn(|| {
                        load_configuration().expect("could not load configuration");
                    });
                }
                _ => {}
            })?;
        } else {
            tracing::info!(
                "no longer watching configuration for changes: {}",
                config.display()
            );

            self.hotwatch.unwatch(config)?;
        };

        Ok(())
    }

    pub fn monitor_idx_in_direction(&self, direction: OperationDirection) -> Option<usize> {
        let current_monitor_size = self.focused_monitor_size().ok()?;

        for (idx, monitor) in self.monitors.elements().iter().enumerate() {
            match direction {
                OperationDirection::Left => {
                    if monitor.size().left + monitor.size().right == current_monitor_size.left {
                        return Option::from(idx);
                    }
                }
                OperationDirection::Right => {
                    if current_monitor_size.right + current_monitor_size.left == monitor.size().left
                    {
                        return Option::from(idx);
                    }
                }
                OperationDirection::Up => {
                    if monitor.size().top + monitor.size().bottom == current_monitor_size.top {
                        return Option::from(idx);
                    }
                }
                OperationDirection::Down => {
                    if current_monitor_size.top + current_monitor_size.bottom == monitor.size().top
                    {
                        return Option::from(idx);
                    }
                }
            }
        }

        None
    }

    /// Calculates the direction of a move across monitors given a specific monitor index
    pub fn direction_from_monitor_idx(
        &self,
        target_monitor_idx: usize,
    ) -> Option<OperationDirection> {
        let current_monitor_idx = self.focused_monitor_idx();
        if current_monitor_idx == target_monitor_idx {
            return None;
        }

        let current_monitor_size = self.focused_monitor_size().ok()?;
        let target_monitor_size = *self.monitors().get(target_monitor_idx)?.size();

        if target_monitor_size.left + target_monitor_size.right == current_monitor_size.left {
            return Some(OperationDirection::Left);
        }
        if current_monitor_size.right + current_monitor_size.left == target_monitor_size.left {
            return Some(OperationDirection::Right);
        }
        if target_monitor_size.top + target_monitor_size.bottom == current_monitor_size.top {
            return Some(OperationDirection::Up);
        }
        if current_monitor_size.top + current_monitor_size.bottom == target_monitor_size.top {
            return Some(OperationDirection::Down);
        }

        None
    }

    #[allow(clippy::too_many_arguments)]
    #[tracing::instrument(skip(self), level = "debug")]
    fn add_window_handle_to_move_based_on_workspace_rule(
        &self,
        window_title: &String,
        hwnd: isize,
        origin_monitor_idx: usize,
        origin_workspace_idx: usize,
        target_monitor_idx: usize,
        target_workspace_idx: usize,
        floating: bool,
        to_move: &mut Vec<EnforceWorkspaceRuleOp>,
    ) -> () {
        tracing::trace!(
            "{} should be on monitor {}, workspace {}",
            window_title,
            target_monitor_idx,
            target_workspace_idx
        );

        // Create an operation outline and save it for later in the fn
        to_move.push(EnforceWorkspaceRuleOp {
            hwnd,
            origin_monitor_idx,
            origin_workspace_idx,
            target_monitor_idx,
            target_workspace_idx,
            floating,
        });
    }

    #[tracing::instrument(skip(self), level = "debug")]
    pub fn enforce_workspace_rules(&mut self) -> Result<()> {
        let mut to_move = vec![];

        let focused_monitor_idx = self.focused_monitor_idx();
        let focused_workspace_idx = self
            .monitors()
            .get(focused_monitor_idx)
            .ok_or_else(|| anyhow!("there is no monitor with that index"))?
            .focused_workspace_idx();

        // scope mutex locks to avoid deadlock if should_update_focused_workspace evaluates to true
        // at the end of this function
        {
            let workspace_matching_rules = WORKSPACE_MATCHING_RULES.lock();
            let regex_identifiers = REGEX_IDENTIFIERS.lock();
            // Go through all the monitors and workspaces
            for (i, monitor) in self.monitors().iter().enumerate() {
                for (j, workspace) in monitor.workspaces().iter().enumerate() {
                    // And all the visible windows (at the top of a container)
                    for window in workspace.visible_windows().into_iter().flatten() {
                        let mut already_moved_window_handles =
                            self.already_moved_window_handles.lock();

                        if let (Ok(exe_name), Ok(title), Ok(class), Ok(path)) =
                            (window.exe(), window.title(), window.class(), window.path())
                        {
                            for rule in &*workspace_matching_rules {
                                let matched = match &rule.matching_rule {
                                    MatchingRule::Simple(r) => should_act_individual(
                                        &title,
                                        &exe_name,
                                        &class,
                                        &path,
                                        r,
                                        &regex_identifiers,
                                    ),
                                    MatchingRule::Composite(r) => {
                                        let mut composite_results = vec![];
                                        for identifier in r {
                                            composite_results.push(should_act_individual(
                                                &title,
                                                &exe_name,
                                                &class,
                                                &path,
                                                identifier,
                                                &regex_identifiers,
                                            ));
                                        }

                                        composite_results.iter().all(|&x| x)
                                    }
                                };

                                if matched {
                                    let floating = workspace.floating_windows().contains(window);

                                    if rule.initial_only {
                                        if !already_moved_window_handles.contains(&window.hwnd) {
                                            already_moved_window_handles.insert(window.hwnd);

                                            self.add_window_handle_to_move_based_on_workspace_rule(
                                                &window.title()?,
                                                window.hwnd,
                                                i,
                                                j,
                                                rule.monitor_index,
                                                rule.workspace_index,
                                                floating,
                                                &mut to_move,
                                            );
                                        }
                                    } else {
                                        self.add_window_handle_to_move_based_on_workspace_rule(
                                            &window.title()?,
                                            window.hwnd,
                                            i,
                                            j,
                                            rule.monitor_index,
                                            rule.workspace_index,
                                            floating,
                                            &mut to_move,
                                        );
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }

        // Only retain operations where the target is not the current workspace
        to_move.retain(|op| !op.is_target(focused_monitor_idx, focused_workspace_idx));
        // Only retain operations where the rule has not already been enforced
        to_move.retain(|op| !op.is_enforced());

        let mut should_update_focused_workspace = false;

        // Parse the operation and remove any windows that are not placed according to their rules
        for op in &to_move {
            let target_area = *self
                .monitors_mut()
                .get_mut(op.target_monitor_idx)
                .ok_or_else(|| anyhow!("there is no monitor with that index"))?
                .work_area_size();

            let origin_monitor = self
                .monitors_mut()
                .get_mut(op.origin_monitor_idx)
                .ok_or_else(|| anyhow!("there is no monitor with that index"))?;

            let origin_area = *origin_monitor.work_area_size();

            let origin_workspace = origin_monitor
                .workspaces_mut()
                .get_mut(op.origin_workspace_idx)
                .ok_or_else(|| anyhow!("there is no workspace with that index"))?;

            let mut window = Window::from(op.hwnd);

            // If it is a floating window move it to the target area
            if op.floating {
                window.move_to_area(&origin_area, &target_area)?;
            }

            // Hide the window we are about to remove if it is on the currently focused workspace
            if op.is_origin(focused_monitor_idx, focused_workspace_idx) {
                window.hide();
                should_update_focused_workspace = true;
            }

            origin_workspace.remove_window(op.hwnd)?;
        }

        // Parse the operation again and associate those removed windows with the workspace that
        // their rules have defined for them
        for op in &to_move {
            let target_monitor = self
                .monitors_mut()
                .get_mut(op.target_monitor_idx)
                .ok_or_else(|| anyhow!("there is no monitor with that index"))?;

            // The very first time this fn is called, the workspace might not even exist yet
            if target_monitor
                .workspaces()
                .get(op.target_workspace_idx)
                .is_none()
            {
                // If it doesn't, let's make sure it does for the next step
                target_monitor.ensure_workspace_count(op.target_workspace_idx + 1);
            }

            let target_workspace = target_monitor
                .workspaces_mut()
                .get_mut(op.target_workspace_idx)
                .ok_or_else(|| anyhow!("there is no workspace with that index"))?;

            if op.floating {
                target_workspace
                    .floating_windows_mut()
                    .push_back(Window::from(op.hwnd));
            } else {
                //TODO(alex-ds13): should this take into account the target workspace
                //`window_container_behaviour`?
                //In the case above a floating window should always be moved as floating,
                //because it was set as so either manually by the user or by a
                //`floating_applications` rule so it should stay that way. But a tiled window
                //when moving to another workspace by a `workspace_rule` should honor that
                //workspace `window_container_behaviour` in my opinion! Maybe this should be done
                //on the `new_container_for_window` function instead.
                target_workspace.new_container_for_window(Window::from(op.hwnd));
            }
        }

        // Only re-tile the focused workspace if we need to
        if should_update_focused_workspace {
            self.update_focused_workspace(false, false)?;
        }

        Ok(())
    }

    #[tracing::instrument(skip(self))]
    pub fn retile_all(&mut self, preserve_resize_dimensions: bool) -> Result<()> {
        let offset = self.work_area_offset;

        for monitor in self.monitors_mut() {
            let offset = if monitor.work_area_offset().is_some() {
                monitor.work_area_offset()
            } else {
                offset
            };

            let focused_workspace_idx = monitor.focused_workspace_idx();
            monitor.update_workspace_globals(focused_workspace_idx, offset);

            let hmonitor = monitor.id();
            let monitor_wp = monitor.wallpaper.clone();
            let workspace = monitor
                .focused_workspace_mut()
                .ok_or_else(|| anyhow!("there is no workspace"))?;

            // Reset any resize adjustments if we want to force a retile
            if !preserve_resize_dimensions {
                for resize in workspace.resize_dimensions_mut() {
                    *resize = None;
                }
            }

            if workspace.wallpaper().is_some() || monitor_wp.is_some() {
                if let Err(error) = workspace.apply_wallpaper(hmonitor, &monitor_wp) {
                    tracing::error!("failed to apply wallpaper: {}", error);
                }
            }

            workspace.update()?;
        }

        Ok(())
    }

    #[tracing::instrument(skip(self))]
    pub fn manage_focused_window(&mut self) -> Result<()> {
        let hwnd = WindowsApi::foreground_window()?;
        let event = WindowManagerEvent::Manage(Window::from(hwnd));
        Ok(winevent_listener::event_tx().send(event)?)
    }

    #[tracing::instrument(skip(self))]
    pub fn unmanage_focused_window(&mut self) -> Result<()> {
        let hwnd = WindowsApi::foreground_window()?;
        let event = WindowManagerEvent::Unmanage(Window::from(hwnd));
        Ok(winevent_listener::event_tx().send(event)?)
    }

    #[tracing::instrument(skip(self))]
    pub fn raise_window_at_cursor_pos(&mut self) -> Result<()> {
        let mut hwnd = None;

        let workspace = self.focused_workspace()?;
        // first check the focused workspace
        if let Some(container_idx) = workspace.container_idx_from_current_point() {
            if let Some(container) = workspace.containers().get(container_idx) {
                if let Some(window) = container.focused_window() {
                    hwnd = Some(window.hwnd);
                }
            }
        }

        // then check all workspaces
        if hwnd.is_none() {
            for monitor in self.monitors() {
                for ws in monitor.workspaces() {
                    if let Some(container_idx) = ws.container_idx_from_current_point() {
                        if let Some(container) = ws.containers().get(container_idx) {
                            if let Some(window) = container.focused_window() {
                                hwnd = Some(window.hwnd);
                            }
                        }
                    }
                }
            }
        }

        // finally try matching the other way using a hwnd returned from the cursor pos
        if hwnd.is_none() {
            let cursor_pos_hwnd = WindowsApi::window_at_cursor_pos()?;

            for monitor in self.monitors() {
                for ws in monitor.workspaces() {
                    if ws.container_for_window(cursor_pos_hwnd).is_some() {
                        hwnd = Some(cursor_pos_hwnd);
                    }
                }
            }
        }

        if let Some(hwnd) = hwnd {
            if self.has_pending_raise_op
                    || self.focused_window()?.hwnd == hwnd
                    // Sometimes we need this check, because the focus may have been given by a click
                    // to a non-window such as the taskbar or system tray, and komorebi doesn't know that
                    // the focused window of the workspace is not actually focused by the OS at that point
                    || WindowsApi::foreground_window()? == hwnd
            {
                return Ok(());
            }

            let event = WindowManagerEvent::Raise(Window::from(hwnd));
            self.has_pending_raise_op = true;
            winevent_listener::event_tx().send(event)?;
        } else {
            tracing::debug!(
                "not raising unknown window: {}",
                Window::from(WindowsApi::window_at_cursor_pos()?)
            );
        }

        Ok(())
    }

    #[tracing::instrument(skip(self))]
    pub fn transfer_window(
        &mut self,
        origin: (usize, usize, isize),
        target: (usize, usize, usize),
    ) -> Result<()> {
        let (origin_monitor_idx, origin_workspace_idx, w_hwnd) = origin;
        let (target_monitor_idx, target_workspace_idx, target_container_idx) = target;

        let origin_workspace = self
            .monitors_mut()
            .get_mut(origin_monitor_idx)
            .ok_or_else(|| anyhow!("cannot get monitor idx"))?
            .workspaces_mut()
            .get_mut(origin_workspace_idx)
            .ok_or_else(|| anyhow!("cannot get workspace idx"))?;

        let origin_container_idx = origin_workspace
            .container_for_window(w_hwnd)
            .and_then(|c| origin_workspace.containers().iter().position(|cc| cc == c));

        if let Some(origin_container_idx) = origin_container_idx {
            // Moving normal container window
            self.transfer_container(
                (
                    origin_monitor_idx,
                    origin_workspace_idx,
                    origin_container_idx,
                ),
                (
                    target_monitor_idx,
                    target_workspace_idx,
                    target_container_idx,
                ),
            )?;
        } else if let Some(idx) = origin_workspace
            .floating_windows()
            .iter()
            .position(|w| w.hwnd == w_hwnd)
        {
            // Moving floating window
            // There is no need to physically move the floating window between areas with
            // `move_to_area` because the user already did that, so we only need to transfer the
            // window to the target `floating_windows`
            if let Some(floating_window) = origin_workspace.floating_windows_mut().remove(idx) {
                let target_workspace = self
                    .monitors_mut()
                    .get_mut(target_monitor_idx)
                    .ok_or_else(|| anyhow!("there is no monitor at this idx"))?
                    .focused_workspace_mut()
                    .ok_or_else(|| anyhow!("there is no focused workspace for this monitor"))?;

                target_workspace
                    .floating_windows_mut()
                    .push_back(floating_window);
            }
        } else if origin_workspace
            .monocle_container()
            .as_ref()
            .and_then(|monocle| monocle.focused_window().map(|w| w.hwnd == w_hwnd))
            .unwrap_or_default()
        {
            // Moving monocle container
            if let Some(monocle_idx) = origin_workspace.monocle_container_restore_idx() {
                let origin_workspace = self
                    .monitors_mut()
                    .get_mut(origin_monitor_idx)
                    .ok_or_else(|| anyhow!("there is no monitor at this idx"))?
                    .workspaces_mut()
                    .get_mut(origin_workspace_idx)
                    .ok_or_else(|| anyhow!("there is no workspace for this monitor"))?;
                let mut uncloack_amount = 0;
                for container in origin_workspace.containers_mut() {
                    container.restore();
                    uncloack_amount += 1;
                }
                origin_workspace.reintegrate_monocle_container()?;

                self.transfer_container(
                    (origin_monitor_idx, origin_workspace_idx, monocle_idx),
                    (
                        target_monitor_idx,
                        target_workspace_idx,
                        target_container_idx,
                    ),
                )?;
                // After we restore the origin workspace, some windows that were cloacked
                // by the monocle might now be uncloacked which would trigger a workspace
                // reconciliation since the focused monitor would be different from origin.
                // That workspace reconciliation would focus the window on the origin monitor.
                // So we need to ignore the uncloak events produced by the origin workspace
                // restore to avoid that issue.
                self.uncloack_to_ignore = uncloack_amount;
            }
        } else if origin_workspace
            .maximized_window()
            .as_ref()
            .map(|max| max.hwnd == w_hwnd)
            .unwrap_or_default()
        {
            // Moving maximized_window
            if let Some(maximized_idx) = origin_workspace.maximized_window_restore_idx() {
                self.focus_monitor(origin_monitor_idx)?;
                let origin_monitor = self
                    .focused_monitor_mut()
                    .ok_or_else(|| anyhow!("there is no origin monitor"))?;
                origin_monitor.focus_workspace(origin_workspace_idx)?;
                self.unmaximize_window()?;
                self.focus_monitor(target_monitor_idx)?;
                let target_monitor = self
                    .focused_monitor_mut()
                    .ok_or_else(|| anyhow!("there is no target monitor"))?;
                target_monitor.focus_workspace(target_workspace_idx)?;

                self.transfer_container(
                    (origin_monitor_idx, origin_workspace_idx, maximized_idx),
                    (
                        target_monitor_idx,
                        target_workspace_idx,
                        target_container_idx,
                    ),
                )?;
            }
        }
        Ok(())
    }

    #[tracing::instrument(skip(self))]
    pub fn transfer_container(
        &mut self,
        origin: (usize, usize, usize),
        target: (usize, usize, usize),
    ) -> Result<()> {
        let (origin_monitor_idx, origin_workspace_idx, origin_container_idx) = origin;
        let (target_monitor_idx, target_workspace_idx, target_container_idx) = target;

        let origin_container = self
            .monitors_mut()
            .get_mut(origin_monitor_idx)
            .ok_or_else(|| anyhow!("there is no monitor at this index"))?
            .workspaces_mut()
            .get_mut(origin_workspace_idx)
            .ok_or_else(|| anyhow!("there is no workspace at this index"))?
            .remove_container(origin_container_idx)
            .ok_or_else(|| anyhow!("there is no container at this index"))?;

        let target_workspace = self
            .monitors_mut()
            .get_mut(target_monitor_idx)
            .ok_or_else(|| anyhow!("there is no monitor at this index"))?
            .workspaces_mut()
            .get_mut(target_workspace_idx)
            .ok_or_else(|| anyhow!("there is no workspace at this index"))?;

        target_workspace
            .containers_mut()
            .insert(target_container_idx, origin_container);

        target_workspace.focus_container(target_container_idx);

        Ok(())
    }

    #[tracing::instrument(skip(self))]
    pub fn swap_containers(
        &mut self,
        origin: (usize, usize, usize),
        target: (usize, usize, usize),
    ) -> Result<()> {
        let (origin_monitor_idx, origin_workspace_idx, origin_container_idx) = origin;
        let (target_monitor_idx, target_workspace_idx, target_container_idx) = target;

        let origin_container_is_valid = self
            .monitors_mut()
            .get_mut(origin_monitor_idx)
            .ok_or_else(|| anyhow!("there is no monitor at this index"))?
            .workspaces_mut()
            .get_mut(origin_workspace_idx)
            .ok_or_else(|| anyhow!("there is no workspace at this index"))?
            .containers()
            .get(origin_container_idx)
            .is_some();

        let target_container_is_valid = self
            .monitors_mut()
            .get_mut(target_monitor_idx)
            .ok_or_else(|| anyhow!("there is no monitor at this index"))?
            .workspaces_mut()
            .get_mut(target_workspace_idx)
            .ok_or_else(|| anyhow!("there is no workspace at this index"))?
            .containers()
            .get(origin_container_idx)
            .is_some();

        if origin_container_is_valid && target_container_is_valid {
            let origin_container = self
                .monitors_mut()
                .get_mut(origin_monitor_idx)
                .ok_or_else(|| anyhow!("there is no monitor at this index"))?
                .workspaces_mut()
                .get_mut(origin_workspace_idx)
                .ok_or_else(|| anyhow!("there is no workspace at this index"))?
                .remove_container(origin_container_idx)
                .ok_or_else(|| anyhow!("there is no container at this index"))?;

            let target_container = self
                .monitors_mut()
                .get_mut(target_monitor_idx)
                .ok_or_else(|| anyhow!("there is no monitor at this index"))?
                .workspaces_mut()
                .get_mut(target_workspace_idx)
                .ok_or_else(|| anyhow!("there is no workspace at this index"))?
                .remove_container(target_container_idx);

            self.monitors_mut()
                .get_mut(target_monitor_idx)
                .ok_or_else(|| anyhow!("there is no monitor at this index"))?
                .workspaces_mut()
                .get_mut(target_workspace_idx)
                .ok_or_else(|| anyhow!("there is no workspace at this index"))?
                .containers_mut()
                .insert(target_container_idx, origin_container);

            if let Some(target_container) = target_container {
                self.monitors_mut()
                    .get_mut(origin_monitor_idx)
                    .ok_or_else(|| anyhow!("there is no monitor at this index"))?
                    .workspaces_mut()
                    .get_mut(origin_workspace_idx)
                    .ok_or_else(|| anyhow!("there is no workspace at this index"))?
                    .containers_mut()
                    .insert(origin_container_idx, target_container);
            }
        }

        Ok(())
    }

    #[tracing::instrument(skip(self))]
    pub fn update_focused_workspace(
        &mut self,
        follow_focus: bool,
        trigger_focus: bool,
    ) -> Result<()> {
        tracing::info!("updating");

        let offset = self.work_area_offset;

        self.focused_monitor_mut()
            .ok_or_else(|| anyhow!("there is no monitor"))?
            .update_focused_workspace(offset)?;

        if follow_focus {
            if let Some(window) = self.focused_workspace()?.maximized_window() {
                if trigger_focus {
                    window.focus(self.mouse_follows_focus)?;
                }
            } else if let Some(container) = self.focused_workspace()?.monocle_container() {
                if let Some(window) = container.focused_window() {
                    if trigger_focus {
                        window.focus(self.mouse_follows_focus)?;
                    }
                }
            } else if let Ok(window) = self.focused_window_mut() {
                if trigger_focus {
                    window.focus(self.mouse_follows_focus)?;
                }
            } else {
                let desktop_window = Window::from(WindowsApi::desktop_window()?);

                let rect = self.focused_monitor_size()?;
                WindowsApi::center_cursor_in_rect(&rect)?;

                match WindowsApi::raise_and_focus_window(desktop_window.hwnd) {
                    Ok(()) => {}
                    Err(error) => {
                        tracing::warn!("{} {}:{}", error, file!(), line!());
                    }
                }
            }
        } else {
            if self.focused_workspace()?.is_empty() {
                let desktop_window = Window::from(WindowsApi::desktop_window()?);

                match WindowsApi::raise_and_focus_window(desktop_window.hwnd) {
                    Ok(()) => {}
                    Err(error) => {
                        tracing::warn!("{} {}:{}", error, file!(), line!());
                    }
                }
            }

            // if we passed false for follow_focus and there is a container on the workspace
            if self.focused_container_mut().is_ok() {
                // and we have a stack with >1 windows
                if self.focused_container_mut()?.windows().len() > 1
                    // and we don't have a maxed window
                    && self.focused_workspace()?.maximized_window().is_none()
                    // and we don't have a monocle container
                    && self.focused_workspace()?.monocle_container().is_none()
                    // and we don't have any floating windows that should show on top
                    && self.focused_workspace()?.floating_windows().is_empty()
                {
                    if let Ok(window) = self.focused_window_mut() {
                        if trigger_focus {
                            window.focus(self.mouse_follows_focus)?;
                        }
                    }
                }
            }
        }

        Ok(())
    }

    #[tracing::instrument(skip(self))]
    pub fn resize_window(
        &mut self,
        direction: OperationDirection,
        sizing: Sizing,
        delta: i32,
        update: bool,
    ) -> Result<()> {
        let mouse_follows_focus = self.mouse_follows_focus;
        let mut focused_monitor_work_area = self.focused_monitor_work_area()?;
        let workspace = self.focused_workspace_mut()?;

        match workspace.layer() {
            WorkspaceLayer::Floating => {
                let workspace = self.focused_workspace()?;
                let focused_hwnd = WindowsApi::foreground_window()?;

                let border_offset = BORDER_OFFSET.load(Ordering::SeqCst);
                let border_width = BORDER_WIDTH.load(Ordering::SeqCst);
                focused_monitor_work_area.left += border_offset;
                focused_monitor_work_area.left += border_width;
                focused_monitor_work_area.top += border_offset;
                focused_monitor_work_area.top += border_width;
                focused_monitor_work_area.right -= border_offset * 2;
                focused_monitor_work_area.right -= border_width * 2;
                focused_monitor_work_area.bottom -= border_offset * 2;
                focused_monitor_work_area.bottom -= border_width * 2;

                for window in workspace.floating_windows().iter() {
                    if window.hwnd == focused_hwnd {
                        let mut rect = WindowsApi::window_rect(window.hwnd)?;
                        match (direction, sizing) {
                            (OperationDirection::Left, Sizing::Increase) => {
                                if rect.left - delta < focused_monitor_work_area.left {
                                    rect.left = focused_monitor_work_area.left;
                                } else {
                                    rect.left -= delta;
                                }
                            }
                            (OperationDirection::Left, Sizing::Decrease) => {
                                rect.left += delta;
                            }
                            (OperationDirection::Right, Sizing::Increase) => {
                                if rect.left + rect.right + delta * 2
                                    > focused_monitor_work_area.left
                                        + focused_monitor_work_area.right
                                {
                                    rect.right = focused_monitor_work_area.left
                                        + focused_monitor_work_area.right
                                        - rect.left;
                                } else {
                                    rect.right += delta * 2;
                                }
                            }
                            (OperationDirection::Right, Sizing::Decrease) => {
                                rect.right -= delta * 2;
                            }
                            (OperationDirection::Up, Sizing::Increase) => {
                                if rect.top - delta < focused_monitor_work_area.top {
                                    rect.top = focused_monitor_work_area.top;
                                } else {
                                    rect.top -= delta;
                                }
                            }
                            (OperationDirection::Up, Sizing::Decrease) => {
                                rect.top += delta;
                            }
                            (OperationDirection::Down, Sizing::Increase) => {
                                if rect.top + rect.bottom + delta * 2
                                    > focused_monitor_work_area.top
                                        + focused_monitor_work_area.bottom
                                {
                                    rect.bottom = focused_monitor_work_area.top
                                        + focused_monitor_work_area.bottom
                                        - rect.top;
                                } else {
                                    rect.bottom += delta * 2;
                                }
                            }
                            (OperationDirection::Down, Sizing::Decrease) => {
                                rect.bottom -= delta * 2;
                            }
                        }

                        WindowsApi::position_window(window.hwnd, &rect, false, true)?;
                        if mouse_follows_focus {
                            WindowsApi::center_cursor_in_rect(&rect)?;
                        }

                        break;
                    }
                }
            }
            WorkspaceLayer::Tiling => {
                match workspace.layout() {
                    Layout::Default(layout) => {
                        tracing::info!("resizing window");
                        let len = NonZeroUsize::new(workspace.containers().len())
                            .ok_or_else(|| anyhow!("there must be at least one container"))?;
                        let focused_idx = workspace.focused_container_idx();
                        let focused_idx_resize = workspace
                            .resize_dimensions()
                            .get(focused_idx)
                            .ok_or_else(|| {
                                anyhow!("there is no resize adjustment for this container")
                            })?;

                        if direction
                            .destination(
                                workspace.layout().as_boxed_direction().as_ref(),
                                workspace.layout_flip(),
                                focused_idx,
                                len,
                            )
                            .is_some()
                        {
                            let unaltered = layout.calculate(
                                &focused_monitor_work_area,
                                len,
                                workspace.container_padding(),
                                workspace.layout_flip(),
                                &[],
                                workspace.focused_container_idx(),
                                workspace.layout_options(),
                                workspace.latest_layout(),
                            );

                            let mut direction = direction;

                            // We only ever want to operate on the unflipped Rect positions when resizing, then we
                            // can flip them however they need to be flipped once the resizing has been done
                            if let Some(flip) = workspace.layout_flip() {
                                match flip {
                                    Axis::Horizontal => {
                                        if matches!(direction, OperationDirection::Left)
                                            || matches!(direction, OperationDirection::Right)
                                        {
                                            direction = direction.opposite();
                                        }
                                    }
                                    Axis::Vertical => {
                                        if matches!(direction, OperationDirection::Up)
                                            || matches!(direction, OperationDirection::Down)
                                        {
                                            direction = direction.opposite();
                                        }
                                    }
                                    Axis::HorizontalAndVertical => direction = direction.opposite(),
                                }
                            }

                            let resize = layout.resize(
                                unaltered
                                    .get(focused_idx)
                                    .ok_or_else(|| anyhow!("there is no last layout"))?,
                                focused_idx_resize,
                                direction,
                                sizing,
                                delta,
                            );

                            workspace.resize_dimensions_mut()[focused_idx] = resize;

                            return if update {
                                self.update_focused_workspace(false, false)
                            } else {
                                Ok(())
                            };
                        }

                        tracing::warn!("cannot resize container in this direction");
                    }
                    Layout::Custom(_) => {
                        tracing::warn!("containers cannot be resized when using custom layouts");
                    }
                }
            }
        }

        Ok(())
    }

    #[tracing::instrument(skip(self))]
    pub fn stop(&mut self, ignore_restore: bool) -> Result<()> {
        tracing::info!(
            "received stop command, restoring all hidden windows and terminating process"
        );

        let state = &State::from(&*self);
        std::fs::write(
            temp_dir().join("komorebi.state.json"),
            serde_json::to_string_pretty(&state)?,
        )?;

        ANIMATION_ENABLED_PER_ANIMATION.lock().clear();
        ANIMATION_ENABLED_GLOBAL.store(false, Ordering::SeqCst);
        self.restore_all_windows(ignore_restore)?;
        AnimationEngine::wait_for_all_animations();

        if WindowsApi::focus_follows_mouse()? {
            WindowsApi::disable_focus_follows_mouse()?;
        }

        let sockets = SUBSCRIPTION_SOCKETS.lock();
        for path in (*sockets).values() {
            if let Ok(stream) = UnixStream::connect(path) {
                stream.shutdown(Shutdown::Both)?;
            }
        }

        let socket = DATA_DIR.join("komorebi.sock");
        let _ = std::fs::remove_file(socket);

        std::process::exit(0)
    }

    #[tracing::instrument(skip(self))]
    pub fn restore_all_windows(&mut self, ignore_restore: bool) -> Result<()> {
        tracing::info!("restoring all hidden windows");

        let no_titlebar = NO_TITLEBAR.lock();
        let regex_identifiers = REGEX_IDENTIFIERS.lock();
        let known_transparent_hwnds = transparency_manager::known_hwnds();
        let border_implementation = border_manager::IMPLEMENTATION.load();

        for monitor in self.monitors_mut() {
            for workspace in monitor.workspaces_mut() {
                if let Some(monocle) = workspace.monocle_container() {
                    for window in monocle.windows() {
                        if matches!(border_implementation, BorderImplementation::Windows) {
                            window.remove_accent()?;
                        }
                    }
                }

                for containers in workspace.containers_mut() {
                    for window in containers.windows_mut() {
                        let should_remove_titlebar_for_window = should_act(
                            &window.title().unwrap_or_default(),
                            &window.exe().unwrap_or_default(),
                            &window.class().unwrap_or_default(),
                            &window.path().unwrap_or_default(),
                            &no_titlebar,
                            &regex_identifiers,
                        )
                        .is_some();

                        if should_remove_titlebar_for_window {
                            window.add_title_bar()?;
                        }

                        if known_transparent_hwnds.contains(&window.hwnd) {
                            window.opaque()?;
                        }

                        if matches!(border_implementation, BorderImplementation::Windows) {
                            window.remove_accent()?;
                        }

                        if !ignore_restore {
                            window.restore();
                        }
                    }
                }
            }
        }

        Ok(())
    }

    #[tracing::instrument(skip(self))]
    pub fn remove_all_accents(&mut self) -> Result<()> {
        tracing::info!("removing all window accents");

        for monitor in self.monitors() {
            for workspace in monitor.workspaces() {
                if let Some(monocle) = workspace.monocle_container() {
                    for window in monocle.windows() {
                        window.remove_accent()?
                    }
                }

                for containers in workspace.containers() {
                    for window in containers.windows() {
                        window.remove_accent()?;
                    }
                }
            }
        }

        Ok(())
    }

    #[tracing::instrument(skip(self))]
    fn handle_unmanaged_window_behaviour(&self) -> Result<()> {
        if matches!(
            self.unmanaged_window_operation_behaviour,
            OperationBehaviour::NoOp
        ) {
            let workspace = self.focused_workspace()?;
            let focused_hwnd = WindowsApi::foreground_window()?;
            if !workspace.contains_managed_window(focused_hwnd) {
                bail!("ignoring commands while active window is not managed by komorebi");
            }
        }

        Ok(())
    }

    /// Check for an existing wallpaper definition on the workspace/monitor index pair and apply it
    /// if it exists
    #[tracing::instrument(skip(self))]
    pub fn apply_wallpaper_for_monitor_workspace(
        &mut self,
        monitor_idx: usize,
        workspace_idx: usize,
    ) -> Result<()> {
        let monitor = self
            .monitors_mut()
            .get_mut(monitor_idx)
            .ok_or_else(|| anyhow!("there is no monitor"))?;

        let hmonitor = monitor.id();
        let monitor_wp = monitor.wallpaper.clone();

        let workspace = monitor
            .workspaces()
            .get(workspace_idx)
            .ok_or_else(|| anyhow!("there is no workspace"))?;

        workspace.apply_wallpaper(hmonitor, &monitor_wp)
    }

    pub fn update_focused_workspace_by_monitor_idx(&mut self, idx: usize) -> Result<()> {
        let offset = self.work_area_offset;

        self.monitors_mut()
            .get_mut(idx)
            .ok_or_else(|| anyhow!("there is no monitor"))?
            .update_focused_workspace(offset)
    }

    #[tracing::instrument(skip(self))]
    pub fn swap_monitor_workspaces(&mut self, first_idx: usize, second_idx: usize) -> Result<()> {
        tracing::info!("swaping monitors");
        if first_idx == second_idx {
            return Ok(());
        }
        let mouse_follows_focus = self.mouse_follows_focus;
        let offset = self.work_area_offset;
        let first_focused_workspace = {
            let first_monitor = self
                .monitors()
                .get(first_idx)
                .ok_or_else(|| anyhow!("There is no monitor"))?;
            first_monitor.focused_workspace_idx()
        };

        let second_focused_workspace = {
            let second_monitor = self
                .monitors()
                .get(second_idx)
                .ok_or_else(|| anyhow!("There is no monitor"))?;
            second_monitor.focused_workspace_idx()
        };

        // Swap workspaces between the first and second monitors

        let first_workspaces = self
            .monitors_mut()
            .get_mut(first_idx)
            .ok_or_else(|| anyhow!("There is no monitor"))?
            .remove_workspaces();

        let second_workspaces = self
            .monitors_mut()
            .get_mut(second_idx)
            .ok_or_else(|| anyhow!("There is no monitor"))?
            .remove_workspaces();

        self.monitors_mut()
            .get_mut(first_idx)
            .ok_or_else(|| anyhow!("There is no monitor"))?
            .workspaces_mut()
            .extend(second_workspaces);

        self.monitors_mut()
            .get_mut(second_idx)
            .ok_or_else(|| anyhow!("There is no monitor"))?
            .workspaces_mut()
            .extend(first_workspaces);

        // Set the focused workspaces for the first and second monitors
        if let Some(first_monitor) = self.monitors_mut().get_mut(first_idx) {
            first_monitor.update_workspaces_globals(offset);
            first_monitor.focus_workspace(second_focused_workspace)?;
            first_monitor.load_focused_workspace(mouse_follows_focus)?;
        }

        if let Some(second_monitor) = self.monitors_mut().get_mut(second_idx) {
            second_monitor.update_workspaces_globals(offset);
            second_monitor.focus_workspace(first_focused_workspace)?;
            second_monitor.load_focused_workspace(mouse_follows_focus)?;
        }

        self.update_focused_workspace_by_monitor_idx(second_idx)?;
        self.update_focused_workspace_by_monitor_idx(first_idx)
    }

    #[tracing::instrument(skip(self))]
    pub fn swap_focused_monitor(&mut self, idx: usize) -> Result<()> {
        tracing::info!("swapping focused monitor");

        let focused_monitor_idx = self.focused_monitor_idx();
        let mouse_follows_focus = self.mouse_follows_focus;

        self.swap_monitor_workspaces(focused_monitor_idx, idx)?;

        self.update_focused_workspace(mouse_follows_focus, true)
    }

    #[tracing::instrument(skip(self))]
    pub fn move_container_to_monitor(
        &mut self,
        monitor_idx: usize,
        workspace_idx: Option<usize>,
        follow: bool,
        move_direction: Option<OperationDirection>,
    ) -> Result<()> {
        self.handle_unmanaged_window_behaviour()?;

        tracing::info!("moving container");

        let focused_monitor_idx = self.focused_monitor_idx();

        if focused_monitor_idx == monitor_idx {
            if let Some(workspace_idx) = workspace_idx {
                return self.move_container_to_workspace(workspace_idx, follow, None);
            }
        }

        let offset = self.work_area_offset;
        let mouse_follows_focus = self.mouse_follows_focus;

        let monitor = self
            .focused_monitor_mut()
            .ok_or_else(|| anyhow!("there is no monitor"))?;

        let current_area = *monitor.work_area_size();

        let workspace = monitor
            .focused_workspace_mut()
            .ok_or_else(|| anyhow!("there is no workspace"))?;

        if workspace.maximized_window().is_some() {
            bail!("cannot move native maximized window to another monitor or workspace");
        }

        let foreground_hwnd = WindowsApi::foreground_window()?;
        let floating_window_index = workspace
            .floating_windows()
            .iter()
            .position(|w| w.hwnd == foreground_hwnd);

        let floating_window =
            floating_window_index.and_then(|idx| workspace.floating_windows_mut().remove(idx));
        let container = if floating_window_index.is_none() {
            Some(
                workspace
                    .remove_focused_container()
                    .ok_or_else(|| anyhow!("there is no container"))?,
            )
        } else {
            None
        };
        monitor.update_focused_workspace(offset)?;

        let target_monitor = self
            .monitors_mut()
            .get_mut(monitor_idx)
            .ok_or_else(|| anyhow!("there is no monitor"))?;

        let mut should_load_workspace = false;
        if let Some(workspace_idx) = workspace_idx {
            if workspace_idx != target_monitor.focused_workspace_idx() {
                target_monitor.focus_workspace(workspace_idx)?;
                should_load_workspace = true;
            }
        }
        let target_workspace = target_monitor
            .focused_workspace_mut()
            .ok_or_else(|| anyhow!("there is no focused workspace on target monitor"))?;

        if target_workspace.monocle_container().is_some() {
            for container in target_workspace.containers_mut() {
                container.restore();
            }

            for window in target_workspace.floating_windows_mut() {
                window.restore();
            }

            target_workspace.reintegrate_monocle_container()?;
        }

        if let Some(window) = floating_window {
            target_workspace.floating_windows_mut().push_back(window);
            target_workspace.set_layer(WorkspaceLayer::Floating);
            Window::from(window.hwnd)
                .move_to_area(&current_area, target_monitor.work_area_size())?;
        } else if let Some(container) = container {
            let container_hwnds = container
                .windows()
                .iter()
                .map(|w| w.hwnd)
                .collect::<Vec<_>>();

            target_workspace.set_layer(WorkspaceLayer::Tiling);

            if let Some(direction) = move_direction {
                target_monitor.add_container_with_direction(container, workspace_idx, direction)?;
            } else {
                target_monitor.add_container(container, workspace_idx)?;
            }

            if let Some(workspace) = target_monitor.focused_workspace() {
                if !*workspace.tile() {
                    for hwnd in container_hwnds {
                        Window::from(hwnd)
                            .move_to_area(&current_area, target_monitor.work_area_size())?;
                    }
                }
            }
        } else {
            bail!("failed to find a window to move");
        }

        if should_load_workspace {
            target_monitor.load_focused_workspace(mouse_follows_focus)?;
        }
        target_monitor.update_focused_workspace(offset)?;

        // this second one is for DPI changes when the target is another monitor
        // if we don't do this the layout on the other monitor could look funny
        // until it is interacted with again
        target_monitor.update_focused_workspace(offset)?;

        if follow {
            self.focus_monitor(monitor_idx)?;
        }

        self.update_focused_workspace(self.mouse_follows_focus, true)?;

        Ok(())
    }

    #[tracing::instrument(skip(self))]
    pub fn move_container_to_workspace(
        &mut self,
        idx: usize,
        follow: bool,
        direction: Option<OperationDirection>,
    ) -> Result<()> {
        self.handle_unmanaged_window_behaviour()?;

        tracing::info!("moving container");

        let mouse_follows_focus = self.mouse_follows_focus;
        let monitor = self
            .focused_monitor_mut()
            .ok_or_else(|| anyhow!("there is no monitor"))?;

        monitor.move_container_to_workspace(idx, follow, direction)?;
        monitor.load_focused_workspace(mouse_follows_focus)?;

        self.update_focused_workspace(mouse_follows_focus, true)?;

        Ok(())
    }

    pub fn remove_focused_workspace(&mut self) -> Option<Workspace> {
        let focused_monitor: &mut Monitor = self.focused_monitor_mut()?;
        let focused_workspace_idx = focused_monitor.focused_workspace_idx();
        let workspace = focused_monitor.remove_workspace_by_idx(focused_workspace_idx);
        if let Err(error) = focused_monitor.focus_workspace(focused_workspace_idx.saturating_sub(1))
        {
            tracing::error!(
                "Error focusing previous workspace while removing the focused workspace: {}",
                error
            );
        }
        workspace
    }

    #[tracing::instrument(skip(self))]
    pub fn move_workspace_to_monitor(&mut self, idx: usize) -> Result<()> {
        tracing::info!("moving workspace");
        let mouse_follows_focus = self.mouse_follows_focus;
        let offset = self.work_area_offset;
        let workspace = self
            .remove_focused_workspace()
            .ok_or_else(|| anyhow!("there is no workspace"))?;

        {
            let target_monitor: &mut Monitor = self
                .monitors_mut()
                .get_mut(idx)
                .ok_or_else(|| anyhow!("there is no monitor"))?;

            target_monitor.workspaces_mut().push_back(workspace);
            target_monitor.update_workspaces_globals(offset);
            target_monitor.focus_workspace(target_monitor.workspaces().len().saturating_sub(1))?;
            target_monitor.load_focused_workspace(mouse_follows_focus)?;
        }

        self.focus_monitor(idx)?;
        self.update_focused_workspace(mouse_follows_focus, true)
    }

    #[tracing::instrument(skip(self))]
    pub fn focus_floating_window_in_direction(
        &mut self,
        direction: OperationDirection,
    ) -> Result<()> {
        let mouse_follows_focus = self.mouse_follows_focus;
        let focused_workspace = self.focused_workspace_mut()?;

        let mut target_idx = None;
        let len = focused_workspace.floating_windows().len();

        if len > 1 {
            let focused_hwnd = WindowsApi::foreground_window()?;
            let focused_rect = WindowsApi::window_rect(focused_hwnd)?;
            match direction {
                OperationDirection::Left => {
                    let mut windows_in_direction = focused_workspace
                        .floating_windows()
                        .iter()
                        .enumerate()
                        .flat_map(|(idx, w)| {
                            (w.hwnd != focused_hwnd)
                                .then_some(WindowsApi::window_rect(w.hwnd).ok().map(|r| (idx, r)))
                        })
                        .flatten()
                        .flat_map(|(idx, r)| {
                            (r.left < focused_rect.left)
                                .then_some((idx, i32::abs(r.left - focused_rect.left)))
                        })
                        .collect::<Vec<_>>();

                    // Sort by distance to focused
                    windows_in_direction.sort_by_key(|(_, d)| (*d as f32 * 1000.0).trunc() as i32);

                    if let Some((idx, _)) = windows_in_direction.first() {
                        target_idx = Some(*idx);
                    }
                }
                OperationDirection::Right => {
                    let mut windows_in_direction = focused_workspace
                        .floating_windows()
                        .iter()
                        .enumerate()
                        .flat_map(|(idx, w)| {
                            (w.hwnd != focused_hwnd)
                                .then_some(WindowsApi::window_rect(w.hwnd).ok().map(|r| (idx, r)))
                        })
                        .flatten()
                        .flat_map(|(idx, r)| {
                            (r.left > focused_rect.left)
                                .then_some((idx, i32::abs(r.left - focused_rect.left)))
                        })
                        .collect::<Vec<_>>();

                    // Sort by distance to focused
                    windows_in_direction.sort_by_key(|(_, d)| (*d as f32 * 1000.0).trunc() as i32);

                    if let Some((idx, _)) = windows_in_direction.first() {
                        target_idx = Some(*idx);
                    }
                }
                OperationDirection::Up => {
                    let mut windows_in_direction = focused_workspace
                        .floating_windows()
                        .iter()
                        .enumerate()
                        .flat_map(|(idx, w)| {
                            (w.hwnd != focused_hwnd)
                                .then_some(WindowsApi::window_rect(w.hwnd).ok().map(|r| (idx, r)))
                        })
                        .flatten()
                        .flat_map(|(idx, r)| {
                            (r.top < focused_rect.top)
                                .then_some((idx, i32::abs(r.top - focused_rect.top)))
                        })
                        .collect::<Vec<_>>();

                    // Sort by distance to focused
                    windows_in_direction.sort_by_key(|(_, d)| (*d as f32 * 1000.0).trunc() as i32);

                    if let Some((idx, _)) = windows_in_direction.first() {
                        target_idx = Some(*idx);
                    }
                }
                OperationDirection::Down => {
                    let mut windows_in_direction = focused_workspace
                        .floating_windows()
                        .iter()
                        .enumerate()
                        .flat_map(|(idx, w)| {
                            (w.hwnd != focused_hwnd)
                                .then_some(WindowsApi::window_rect(w.hwnd).ok().map(|r| (idx, r)))
                        })
                        .flatten()
                        .flat_map(|(idx, r)| {
                            (r.top > focused_rect.top)
                                .then_some((idx, i32::abs(r.top - focused_rect.top)))
                        })
                        .collect::<Vec<_>>();

                    // Sort by distance to focused
                    windows_in_direction.sort_by_key(|(_, d)| (*d as f32 * 1000.0).trunc() as i32);

                    if let Some((idx, _)) = windows_in_direction.first() {
                        target_idx = Some(*idx);
                    }
                }
            };
        }

        if let Some(idx) = target_idx {
            focused_workspace.floating_windows.focus(idx);
            if let Some(window) = focused_workspace.floating_windows().get(idx) {
                window.focus(mouse_follows_focus)?;
            }
            return Ok(());
        }

        let mut cross_monitor_monocle_or_max = false;

        let workspace_idx = self.focused_workspace_idx()?;

        // this is for when we are scrolling across workspaces like PaperWM
        if matches!(
            self.cross_boundary_behaviour,
            CrossBoundaryBehaviour::Workspace
        ) && matches!(
            direction,
            OperationDirection::Left | OperationDirection::Right
        ) {
            let workspace_count = if let Some(monitor) = self.focused_monitor() {
                monitor.workspaces().len()
            } else {
                1
            };

            let next_idx = match direction {
                OperationDirection::Left => match workspace_idx {
                    0 => workspace_count - 1,
                    n => n - 1,
                },
                OperationDirection::Right => match workspace_idx {
                    n if n == workspace_count - 1 => 0,
                    n => n + 1,
                },
                _ => workspace_idx,
            };

            self.focus_workspace(next_idx)?;

            if let Ok(focused_workspace) = self.focused_workspace_mut() {
                if focused_workspace.monocle_container().is_none() {
                    match direction {
                        OperationDirection::Left => match focused_workspace.layout() {
                            Layout::Default(layout) => {
                                let target_index =
                                    layout.rightmost_index(focused_workspace.containers().len());
                                focused_workspace.focus_container(target_index);
                            }
                            Layout::Custom(_) => {
                                focused_workspace.focus_container(
                                    focused_workspace.containers().len().saturating_sub(1),
                                );
                            }
                        },
                        OperationDirection::Right => match focused_workspace.layout() {
                            Layout::Default(layout) => {
                                let target_index =
                                    layout.leftmost_index(focused_workspace.containers().len());
                                focused_workspace.focus_container(target_index);
                            }
                            Layout::Custom(_) => {
                                focused_workspace.focus_container(0);
                            }
                        },
                        _ => {}
                    };
                }
            }

            return Ok(());
        }

        // if there is no floating_window in that direction for this workspace
        let monitor_idx = self
            .monitor_idx_in_direction(direction)
            .ok_or_else(|| anyhow!("there is no container or monitor in this direction"))?;

        self.focus_monitor(monitor_idx)?;
        let mouse_follows_focus = self.mouse_follows_focus;

        if let Ok(focused_workspace) = self.focused_workspace_mut() {
            if let Some(window) = focused_workspace.maximized_window() {
                window.focus(mouse_follows_focus)?;
                cross_monitor_monocle_or_max = true;
            } else if let Some(monocle) = focused_workspace.monocle_container() {
                if let Some(window) = monocle.focused_window() {
                    window.focus(mouse_follows_focus)?;
                    cross_monitor_monocle_or_max = true;
                }
            } else if focused_workspace.layer() == &WorkspaceLayer::Tiling {
                match direction {
                    OperationDirection::Left => match focused_workspace.layout() {
                        Layout::Default(layout) => {
                            let target_index =
                                layout.rightmost_index(focused_workspace.containers().len());
                            focused_workspace.focus_container(target_index);
                        }
                        Layout::Custom(_) => {
                            focused_workspace.focus_container(
                                focused_workspace.containers().len().saturating_sub(1),
                            );
                        }
                    },
                    OperationDirection::Right => match focused_workspace.layout() {
                        Layout::Default(layout) => {
                            let target_index =
                                layout.leftmost_index(focused_workspace.containers().len());
                            focused_workspace.focus_container(target_index);
                        }
                        Layout::Custom(_) => {
                            focused_workspace.focus_container(0);
                        }
                    },
                    _ => {}
                };
            }
        }

        if !cross_monitor_monocle_or_max {
            let ws = self.focused_workspace_mut()?;
            if ws.is_empty() {
                // This is to remove focus from the previous monitor
                let desktop_window = Window::from(WindowsApi::desktop_window()?);

                match WindowsApi::raise_and_focus_window(desktop_window.hwnd) {
                    Ok(()) => {}
                    Err(error) => {
                        tracing::warn!("{} {}:{}", error, file!(), line!());
                    }
                }
            } else if ws.layer() == &WorkspaceLayer::Floating && !ws.floating_windows().is_empty() {
                if let Some(window) = ws.focused_floating_window() {
                    window.focus(self.mouse_follows_focus)?;
                }
            } else {
                ws.set_layer(WorkspaceLayer::Tiling);
                if let Ok(focused_window) = self.focused_window() {
                    focused_window.focus(self.mouse_follows_focus)?;
                }
            }
        }

        Ok(())
    }

    #[tracing::instrument(skip(self))]
    pub fn focus_container_in_direction(&mut self, direction: OperationDirection) -> Result<()> {
        self.handle_unmanaged_window_behaviour()?;

        let workspace = self.focused_workspace()?;
        let workspace_idx = self.focused_workspace_idx()?;

        tracing::info!("focusing container");

        let new_idx =
            if workspace.maximized_window().is_some() || workspace.monocle_container().is_some() {
                None
            } else {
                workspace.new_idx_for_direction(direction)
            };

        let mut cross_monitor_monocle_or_max = false;

        // this is for when we are scrolling across workspaces like PaperWM
        if new_idx.is_none()
            && matches!(
                self.cross_boundary_behaviour,
                CrossBoundaryBehaviour::Workspace
            )
            && matches!(
                direction,
                OperationDirection::Left | OperationDirection::Right
            )
        {
            let workspace_count = if let Some(monitor) = self.focused_monitor() {
                monitor.workspaces().len()
            } else {
                1
            };

            let next_idx = match direction {
                OperationDirection::Left => match workspace_idx {
                    0 => workspace_count - 1,
                    n => n - 1,
                },
                OperationDirection::Right => match workspace_idx {
                    n if n == workspace_count - 1 => 0,
                    n => n + 1,
                },
                _ => workspace_idx,
            };

            self.focus_workspace(next_idx)?;

            if let Ok(focused_workspace) = self.focused_workspace_mut() {
                if focused_workspace.monocle_container().is_none() {
                    match direction {
                        OperationDirection::Left => match focused_workspace.layout() {
                            Layout::Default(layout) => {
                                let target_index =
                                    layout.rightmost_index(focused_workspace.containers().len());
                                focused_workspace.focus_container(target_index);
                            }
                            Layout::Custom(_) => {
                                focused_workspace.focus_container(
                                    focused_workspace.containers().len().saturating_sub(1),
                                );
                            }
                        },
                        OperationDirection::Right => match focused_workspace.layout() {
                            Layout::Default(layout) => {
                                let target_index =
                                    layout.leftmost_index(focused_workspace.containers().len());
                                focused_workspace.focus_container(target_index);
                            }
                            Layout::Custom(_) => {
                                focused_workspace.focus_container(0);
                            }
                        },
                        _ => {}
                    };
                }
            }

            return Ok(());
        }

        // if there is no container in that direction for this workspace
        match new_idx {
            None => {
                let monitor_idx = self
                    .monitor_idx_in_direction(direction)
                    .ok_or_else(|| anyhow!("there is no container or monitor in this direction"))?;

                self.focus_monitor(monitor_idx)?;
                let mouse_follows_focus = self.mouse_follows_focus;

                if let Ok(focused_workspace) = self.focused_workspace_mut() {
                    if let Some(window) = focused_workspace.maximized_window() {
                        window.focus(mouse_follows_focus)?;
                        cross_monitor_monocle_or_max = true;
                    } else if let Some(monocle) = focused_workspace.monocle_container() {
                        if let Some(window) = monocle.focused_window() {
                            window.focus(mouse_follows_focus)?;
                            cross_monitor_monocle_or_max = true;
                        }
                    } else if focused_workspace.layer() == &WorkspaceLayer::Tiling {
                        match direction {
                            OperationDirection::Left => match focused_workspace.layout() {
                                Layout::Default(layout) => {
                                    let target_index = layout
                                        .rightmost_index(focused_workspace.containers().len());
                                    focused_workspace.focus_container(target_index);
                                }
                                Layout::Custom(_) => {
                                    focused_workspace.focus_container(
                                        focused_workspace.containers().len().saturating_sub(1),
                                    );
                                }
                            },
                            OperationDirection::Right => match focused_workspace.layout() {
                                Layout::Default(layout) => {
                                    let target_index =
                                        layout.leftmost_index(focused_workspace.containers().len());
                                    focused_workspace.focus_container(target_index);
                                }
                                Layout::Custom(_) => {
                                    focused_workspace.focus_container(0);
                                }
                            },
                            _ => {}
                        };
                    }
                }
            }
            Some(idx) => {
                let workspace = self.focused_workspace_mut()?;
                workspace.focus_container(idx);
            }
        }

        if !cross_monitor_monocle_or_max {
            let ws = self.focused_workspace_mut()?;
            if ws.is_empty() {
                // This is to remove focus from the previous monitor
                let desktop_window = Window::from(WindowsApi::desktop_window()?);

                match WindowsApi::raise_and_focus_window(desktop_window.hwnd) {
                    Ok(()) => {}
                    Err(error) => {
                        tracing::warn!("{} {}:{}", error, file!(), line!());
                    }
                }
            } else if ws.layer() == &WorkspaceLayer::Floating && !ws.floating_windows().is_empty() {
                if let Some(window) = ws.focused_floating_window() {
                    window.focus(self.mouse_follows_focus)?;
                }
            } else {
                ws.set_layer(WorkspaceLayer::Tiling);
                if let Ok(focused_window) = self.focused_window() {
                    focused_window.focus(self.mouse_follows_focus)?;
                }
            }
        }

        Ok(())
    }

    #[tracing::instrument(skip(self))]
    pub fn move_floating_window_in_direction(
        &mut self,
        direction: OperationDirection,
    ) -> Result<()> {
        let mouse_follows_focus = self.mouse_follows_focus;

        let mut focused_monitor_work_area = self.focused_monitor_work_area()?;
        let border_offset = BORDER_OFFSET.load(Ordering::SeqCst);
        let border_width = BORDER_WIDTH.load(Ordering::SeqCst);
        focused_monitor_work_area.left += border_offset;
        focused_monitor_work_area.left += border_width;
        focused_monitor_work_area.top += border_offset;
        focused_monitor_work_area.top += border_width;
        focused_monitor_work_area.right -= border_offset * 2;
        focused_monitor_work_area.right -= border_width * 2;
        focused_monitor_work_area.bottom -= border_offset * 2;
        focused_monitor_work_area.bottom -= border_width * 2;

        let focused_workspace = self.focused_workspace()?;
        let delta = self.resize_delta;

        let focused_hwnd = WindowsApi::foreground_window()?;
        for window in focused_workspace.floating_windows().iter() {
            if window.hwnd == focused_hwnd {
                let mut rect = WindowsApi::window_rect(window.hwnd)?;
                match direction {
                    OperationDirection::Left => {
                        if rect.left - delta < focused_monitor_work_area.left {
                            rect.left = focused_monitor_work_area.left;
                        } else {
                            rect.left -= delta;
                        }
                    }
                    OperationDirection::Right => {
                        if rect.left + delta + rect.right
                            > focused_monitor_work_area.left + focused_monitor_work_area.right
                        {
                            rect.left = focused_monitor_work_area.left
                                + focused_monitor_work_area.right
                                - rect.right;
                        } else {
                            rect.left += delta;
                        }
                    }
                    OperationDirection::Up => {
                        if rect.top - delta < focused_monitor_work_area.top {
                            rect.top = focused_monitor_work_area.top;
                        } else {
                            rect.top -= delta;
                        }
                    }
                    OperationDirection::Down => {
                        if rect.top + delta + rect.bottom
                            > focused_monitor_work_area.top + focused_monitor_work_area.bottom
                        {
                            rect.top = focused_monitor_work_area.top
                                + focused_monitor_work_area.bottom
                                - rect.bottom;
                        } else {
                            rect.top += delta;
                        }
                    }
                }

                WindowsApi::position_window(window.hwnd, &rect, false, true)?;
                if mouse_follows_focus {
                    WindowsApi::center_cursor_in_rect(&rect)?;
                }

                break;
            }
        }

        Ok(())
    }

    #[tracing::instrument(skip(self))]
    pub fn move_container_in_direction(&mut self, direction: OperationDirection) -> Result<()> {
        self.handle_unmanaged_window_behaviour()?;

        let workspace = self.focused_workspace()?;
        let workspace_idx = self.focused_workspace_idx()?;

        // removing this messes up the monitor / container / window index somewhere
        // and results in the wrong window getting moved across the monitor boundary
        if workspace.is_focused_window_monocle_or_maximized()? {
            bail!("ignoring command while active window is in monocle mode or maximized");
        }

        tracing::info!("moving container");

        let origin_container_idx = workspace.focused_container_idx();
        let origin_monitor_idx = self.focused_monitor_idx();
        let target_container_idx = workspace.new_idx_for_direction(direction);

        // this is for when we are scrolling across workspaces like PaperWM
        if target_container_idx.is_none()
            && matches!(
                self.cross_boundary_behaviour,
                CrossBoundaryBehaviour::Workspace
            )
            && matches!(
                direction,
                OperationDirection::Left | OperationDirection::Right
            )
        {
            let workspace_count = if let Some(monitor) = self.focused_monitor() {
                monitor.workspaces().len()
            } else {
                1
            };

            let next_idx = match direction {
                OperationDirection::Left => match workspace_idx {
                    0 => workspace_count - 1,
                    n => n - 1,
                },
                OperationDirection::Right => match workspace_idx {
                    n if n == workspace_count - 1 => 0,
                    n => n + 1,
                },
                _ => workspace_idx,
            };

            // passing the direction here is how we handle whether to insert at the front
            // or the back of the container vecdeque in the target workspace
            self.move_container_to_workspace(next_idx, true, Some(direction))?;
            self.update_focused_workspace(self.mouse_follows_focus, true)?;

            return Ok(());
        }

        match target_container_idx {
            // If there is nowhere to move on the current workspace, try to move it onto the monitor
            // in that direction if there is one
            None => {
                // Don't do anything if the user has set the MoveBehaviour to NoOp
                if matches!(self.cross_monitor_move_behaviour, MoveBehaviour::NoOp) {
                    return Ok(());
                }

                let target_monitor_idx = self
                    .monitor_idx_in_direction(direction)
                    .ok_or_else(|| anyhow!("there is no container or monitor in this direction"))?;

                {
                    // actually move the container to target monitor using the direction
                    self.move_container_to_monitor(
                        target_monitor_idx,
                        None,
                        true,
                        Some(direction),
                    )?;

                    // focus the target monitor
                    self.focus_monitor(target_monitor_idx)?;

                    // unset monocle container on target workspace if there is one
                    let mut target_workspace_has_monocle = false;
                    if let Ok(target_workspace) = self.focused_workspace() {
                        if target_workspace.monocle_container().is_some() {
                            target_workspace_has_monocle = true;
                        }
                    }

                    if target_workspace_has_monocle {
                        self.toggle_monocle()?;
                    }

                    // get a mutable ref to the focused workspace on the target monitor
                    let target_workspace = self.focused_workspace_mut()?;

                    // if there is only one container on the target workspace after the insertion
                    // it means that there won't be one swapped back, so we have to decrement the
                    // focused position
                    if target_workspace.containers().len() == 1 {
                        let origin_workspace =
                            self.focused_workspace_for_monitor_idx_mut(origin_monitor_idx)?;

                        origin_workspace.focus_container(
                            origin_workspace.focused_container_idx().saturating_sub(1),
                        );
                    }
                }

                // if our MoveBehaviour is Swap, let's try to send back the window container
                // whose position which just took over
                if matches!(self.cross_monitor_move_behaviour, MoveBehaviour::Swap) {
                    {
                        let target_workspace = self.focused_workspace_mut()?;

                        // if the target workspace doesn't have more than one container, this means it
                        // was previously empty, by only doing the second part of the swap when there is
                        // more than one container, we can fall back to a "move" if there is nothing to
                        // swap with on the target monitor
                        if target_workspace.containers().len() > 1 {
                            // remove the container from the target monitor workspace
                            let target_container = target_workspace
                                // this is now focused_container_idx + 1 because we have inserted our origin container
                                .remove_container_by_idx(
                                    target_workspace.focused_container_idx() + 1,
                                )
                                .ok_or_else(|| {
                                    anyhow!("could not remove container at given target index")
                                })?;

                            let origin_workspace =
                                self.focused_workspace_for_monitor_idx_mut(origin_monitor_idx)?;

                            // insert the container from the target monitor workspace into the origin monitor workspace
                            // at the same position from which our origin container was removed
                            origin_workspace
                                .insert_container_at_idx(origin_container_idx, target_container);
                        }
                    }
                }

                // make sure to update the origin monitor workspace layout because it is no
                // longer focused so it won't get updated at the end of this fn
                let offset = self.work_area_offset;

                self.monitors_mut()
                    .get_mut(origin_monitor_idx)
                    .ok_or_else(|| anyhow!("there is no monitor at this index"))?
                    .update_focused_workspace(offset)?;

                let a = self
                    .focused_monitor()
                    .ok_or_else(|| anyhow!("there is no monitor focused monitor"))?
                    .id();
                let b = self
                    .monitors_mut()
                    .get_mut(origin_monitor_idx)
                    .ok_or_else(|| anyhow!("there is no monitor at this index"))?
                    .id();

                if !WindowsApi::monitors_have_same_dpi(a, b)? {
                    self.update_focused_workspace(self.mouse_follows_focus, true)?;
                }
            }
            Some(new_idx) => {
                let workspace = self.focused_workspace_mut()?;
                workspace.swap_containers(origin_container_idx, new_idx);
                workspace.focus_container(new_idx);
            }
        }

        self.update_focused_workspace(self.mouse_follows_focus, true)?;

        Ok(())
    }

    #[tracing::instrument(skip(self))]
    pub fn focus_floating_window_in_cycle_direction(
        &mut self,
        direction: CycleDirection,
    ) -> Result<()> {
        let mouse_follows_focus = self.mouse_follows_focus;
        let focused_workspace = self.focused_workspace()?;

        let mut target_idx = None;
        let len = focused_workspace.floating_windows().len();

        if len > 1 {
            let focused_hwnd = WindowsApi::foreground_window()?;
            for (idx, window) in focused_workspace.floating_windows().iter().enumerate() {
                if window.hwnd == focused_hwnd {
                    match direction {
                        CycleDirection::Previous => {
                            if idx == 0 {
                                target_idx = Some(len - 1)
                            } else {
                                target_idx = Some(idx - 1)
                            }
                        }
                        CycleDirection::Next => {
                            if idx == len - 1 {
                                target_idx = Some(0)
                            } else {
                                target_idx = Some(idx - 1)
                            }
                        }
                    }
                }
            }

            if target_idx.is_none() {
                target_idx = Some(0);
            }
        }

        if let Some(idx) = target_idx {
            if let Some(window) = focused_workspace.floating_windows().get(idx) {
                window.focus(mouse_follows_focus)?;
            }
        }

        Ok(())
    }

    #[tracing::instrument(skip(self))]
    pub fn focus_container_in_cycle_direction(&mut self, direction: CycleDirection) -> Result<()> {
        self.handle_unmanaged_window_behaviour()?;

        tracing::info!("focusing container");
        let mut maximize_next = false;
        let mut monocle_next = false;

        if self.focused_workspace_mut()?.maximized_window().is_some() {
            maximize_next = true;
            self.unmaximize_window()?;
        }

        if self.focused_workspace_mut()?.monocle_container().is_some() {
            monocle_next = true;
            self.monocle_off()?;
        }

        let workspace = self.focused_workspace_mut()?;

        let new_idx = workspace
            .new_idx_for_cycle_direction(direction)
            .ok_or_else(|| anyhow!("this is not a valid direction from the current position"))?;

        workspace.focus_container(new_idx);

        if maximize_next {
            self.toggle_maximize()?;
        } else if monocle_next {
            self.toggle_monocle()?;
        } else {
            self.focused_window_mut()?.focus(self.mouse_follows_focus)?;
        }

        Ok(())
    }

    #[tracing::instrument(skip(self))]
    pub fn move_container_in_cycle_direction(&mut self, direction: CycleDirection) -> Result<()> {
        self.handle_unmanaged_window_behaviour()?;

        let workspace = self.focused_workspace_mut()?;
        if workspace.is_focused_window_monocle_or_maximized()? {
            bail!("ignoring command while active window is in monocle mode or maximized");
        }

        tracing::info!("moving container");

        let current_idx = workspace.focused_container_idx();
        let new_idx = workspace
            .new_idx_for_cycle_direction(direction)
            .ok_or_else(|| anyhow!("this is not a valid direction from the current position"))?;

        workspace.swap_containers(current_idx, new_idx);
        workspace.focus_container(new_idx);
        self.update_focused_workspace(self.mouse_follows_focus, true)
    }

    #[tracing::instrument(skip(self))]
    pub fn cycle_container_window_in_direction(&mut self, direction: CycleDirection) -> Result<()> {
        self.handle_unmanaged_window_behaviour()?;

        tracing::info!("cycling container windows");

        let container =
            if let Some(container) = self.focused_workspace_mut()?.monocle_container_mut() {
                container
            } else {
                self.focused_container_mut()?
            };

        let len = NonZeroUsize::new(container.windows().len())
            .ok_or_else(|| anyhow!("there must be at least one window in a container"))?;

        if len.get() == 1 {
            bail!("there is only one window in this container");
        }

        let current_idx = container.focused_window_idx();
        let next_idx = direction.next_idx(current_idx, len);

        container.focus_window(next_idx);
        container.load_focused_window();

        if let Some(window) = container.focused_window() {
            window.focus(self.mouse_follows_focus)?;
        }

        self.update_focused_workspace(self.mouse_follows_focus, true)
    }

    #[tracing::instrument(skip(self))]
    pub fn cycle_container_window_index_in_direction(
        &mut self,
        direction: CycleDirection,
    ) -> Result<()> {
        self.handle_unmanaged_window_behaviour()?;

        tracing::info!("cycling container window index");

        let container =
            if let Some(container) = self.focused_workspace_mut()?.monocle_container_mut() {
                container
            } else {
                self.focused_container_mut()?
            };

        let len = NonZeroUsize::new(container.windows().len())
            .ok_or_else(|| anyhow!("there must be at least one window in a container"))?;

        if len.get() == 1 {
            bail!("there is only one window in this container");
        }

        let current_idx = container.focused_window_idx();
        let next_idx = direction.next_idx(current_idx, len);
        container.windows_mut().swap(current_idx, next_idx);

        container.focus_window(next_idx);
        container.load_focused_window();

        if let Some(window) = container.focused_window() {
            window.focus(self.mouse_follows_focus)?;
        }

        self.update_focused_workspace(self.mouse_follows_focus, true)
    }
    #[tracing::instrument(skip(self))]
    pub fn focus_container_window(&mut self, idx: usize) -> Result<()> {
        self.handle_unmanaged_window_behaviour()?;

        tracing::info!("focusing container window at index {idx}");

        let container =
            if let Some(container) = self.focused_workspace_mut()?.monocle_container_mut() {
                container
            } else {
                self.focused_container_mut()?
            };

        let len = NonZeroUsize::new(container.windows().len())
            .ok_or_else(|| anyhow!("there must be at least one window in a container"))?;

        if len.get() == 1 && idx != 0 {
            bail!("there is only one window in this container");
        }

        if container.windows().get(idx).is_none() {
            bail!("there is no window in this container at index {idx}");
        }

        container.focus_window(idx);
        container.load_focused_window();

        if let Some(window) = container.focused_window() {
            window.focus(self.mouse_follows_focus)?;
        }

        self.update_focused_workspace(self.mouse_follows_focus, true)
    }

    #[tracing::instrument(skip(self))]
    pub fn stack_all(&mut self) -> Result<()> {
        self.handle_unmanaged_window_behaviour()?;
        tracing::info!("stacking all windows on workspace");

        let workspace = self.focused_workspace_mut()?;

        let mut focused_hwnd = None;
        if let Some(container) = workspace.focused_container() {
            if let Some(window) = container.focused_window() {
                focused_hwnd = Some(window.hwnd);
            }
        }

        workspace.focus_container(workspace.containers().len().saturating_sub(1));
        while workspace.focused_container_idx() > 0 {
            workspace.move_window_to_container(0)?;
            workspace.focus_container(workspace.containers().len().saturating_sub(1));
        }

        if let Some(hwnd) = focused_hwnd {
            workspace.focus_container_by_window(hwnd)?;
        }

        self.update_focused_workspace(self.mouse_follows_focus, true)
    }

    #[tracing::instrument(skip(self))]
    pub fn unstack_all(&mut self) -> Result<()> {
        self.handle_unmanaged_window_behaviour()?;
        tracing::info!("unstacking all windows in container");

        let workspace = self.focused_workspace_mut()?;

        let mut focused_hwnd = None;
        if let Some(container) = workspace.focused_container() {
            if let Some(window) = container.focused_window() {
                focused_hwnd = Some(window.hwnd);
            }
        }

        let initial_focused_container_index = workspace.focused_container_idx();
        let mut focused_container = workspace.focused_container().cloned();

        while let Some(focused) = &focused_container {
            if focused.windows().len() > 1 {
                workspace.new_container_for_focused_window()?;
                workspace.focus_container(initial_focused_container_index);
                focused_container = workspace.focused_container().cloned();
            } else {
                focused_container = None;
            }
        }

        if let Some(hwnd) = focused_hwnd {
            workspace.focus_container_by_window(hwnd)?;
        }

        self.update_focused_workspace(self.mouse_follows_focus, true)
    }

    #[tracing::instrument(skip(self))]
    pub fn add_window_to_container(&mut self, direction: OperationDirection) -> Result<()> {
        self.handle_unmanaged_window_behaviour()?;

        tracing::info!("adding window to container");

        let workspace = self.focused_workspace_mut()?;
        let len = NonZeroUsize::new(workspace.containers_mut().len())
            .ok_or_else(|| anyhow!("there must be at least one container"))?;
        let current_container_idx = workspace.focused_container_idx();

        let is_valid = direction
            .destination(
                workspace.layout().as_boxed_direction().as_ref(),
                workspace.layout_flip(),
                workspace.focused_container_idx(),
                len,
            )
            .is_some();

        if is_valid {
            let new_idx = workspace.new_idx_for_direction(direction).ok_or_else(|| {
                anyhow!("this is not a valid direction from the current position")
            })?;

            let mut changed_focus = false;

            let adjusted_new_index = if new_idx > current_container_idx
                && !matches!(
                    workspace.layout(),
                    Layout::Default(DefaultLayout::Grid)
                        | Layout::Default(DefaultLayout::UltrawideVerticalStack)
                ) {
                workspace.focus_container(new_idx);
                changed_focus = true;
                new_idx.saturating_sub(1)
            } else {
                new_idx
            };

            let mut target_container_is_stack = false;

            if let Some(container) = workspace.containers().get(adjusted_new_index) {
                if container.windows().len() > 1 {
                    target_container_is_stack = true;
                }
            }

            if let Some(current) = workspace.focused_container() {
                if current.windows().len() > 1 && !target_container_is_stack {
                    workspace.focus_container(adjusted_new_index);
                    changed_focus = true;
                    workspace.move_window_to_container(current_container_idx)?;
                } else {
                    workspace.move_window_to_container(adjusted_new_index)?;
                }
            }

            if changed_focus {
                if let Some(container) = workspace.focused_container_mut() {
                    container.load_focused_window();
                    if let Some(window) = container.focused_window() {
                        window.focus(self.mouse_follows_focus)?;
                    }
                }
            }

            self.update_focused_workspace(self.mouse_follows_focus, false)?;
        }

        Ok(())
    }

    #[tracing::instrument(skip(self))]
    pub fn promote_container_to_front(&mut self) -> Result<()> {
        self.handle_unmanaged_window_behaviour()?;

        let workspace = self.focused_workspace_mut()?;

        if matches!(workspace.layout(), Layout::Default(DefaultLayout::Grid)) {
            tracing::debug!("ignoring promote command for grid layout");
            return Ok(());
        }

        tracing::info!("promoting container");

        workspace.promote_container()?;
        self.update_focused_workspace(self.mouse_follows_focus, true)
    }

    #[tracing::instrument(skip(self))]
    pub fn promote_focus_to_front(&mut self) -> Result<()> {
        self.handle_unmanaged_window_behaviour()?;

        let workspace = self.focused_workspace_mut()?;

        if matches!(workspace.layout(), Layout::Default(DefaultLayout::Grid)) {
            tracing::info!("ignoring promote focus command for grid layout");
            return Ok(());
        }

        tracing::info!("promoting focus");

        let target_idx = match workspace.layout() {
            Layout::Default(_) => 0,
            Layout::Custom(custom) => custom
                .first_container_idx(custom.primary_idx().map_or(0, |primary_idx| primary_idx)),
        };

        workspace.focus_container(target_idx);
        self.update_focused_workspace(self.mouse_follows_focus, true)
    }

    #[tracing::instrument(skip(self))]
    pub fn remove_window_from_container(&mut self) -> Result<()> {
        self.handle_unmanaged_window_behaviour()?;

        tracing::info!("removing window");

        if self.focused_container()?.windows().len() == 1 {
            bail!("a container must have at least one window");
        }

        let workspace = self.focused_workspace_mut()?;

        workspace.new_container_for_focused_window()?;
        self.update_focused_workspace(self.mouse_follows_focus, false)
    }

    #[tracing::instrument(skip(self))]
    pub fn toggle_tiling(&mut self) -> Result<()> {
        let workspace = self.focused_workspace_mut()?;
        workspace.set_tile(!*workspace.tile());
        self.update_focused_workspace(false, false)
    }

    #[tracing::instrument(skip(self))]
    pub fn toggle_float(&mut self, force_float: bool) -> Result<()> {
        let hwnd = WindowsApi::foreground_window()?;
        let workspace = self.focused_workspace_mut()?;
        if workspace.monocle_container().is_some() {
            tracing::warn!("ignoring toggle-float command while workspace has a monocle container");
            return Ok(());
        }

        let mut is_floating_window = false;

        for window in workspace.floating_windows() {
            if window.hwnd == hwnd {
                is_floating_window = true;
            }
        }

        if is_floating_window && !force_float {
            workspace.set_layer(WorkspaceLayer::Tiling);
            self.unfloat_window()?;
        } else {
            workspace.set_layer(WorkspaceLayer::Floating);
            self.float_window()?;
        }

        self.update_focused_workspace(is_floating_window, true)
    }

    #[tracing::instrument(skip(self))]
    pub fn toggle_lock(&mut self) -> Result<()> {
        let workspace = self.focused_workspace_mut()?;
        if let Some(container) = workspace.focused_container_mut() {
            // Toggle the locked flag
            container.set_locked(!container.locked());
        }
        Ok(())
    }

    #[tracing::instrument(skip(self))]
    pub fn float_window(&mut self) -> Result<()> {
        tracing::info!("floating window");

        let work_area = self.focused_monitor_work_area()?;

        let toggle_float_placement = self.window_management_behaviour.toggle_float_placement;

        let workspace = self.focused_workspace_mut()?;
        workspace.new_floating_window()?;

        let window = workspace
            .floating_windows_mut()
            .back_mut()
            .ok_or_else(|| anyhow!("there is no floating window"))?;

        if toggle_float_placement.should_center() {
            window.center(&work_area, toggle_float_placement.should_resize())?;
        }
        window.focus(self.mouse_follows_focus)?;

        Ok(())
    }

    #[tracing::instrument(skip(self))]
    pub fn unfloat_window(&mut self) -> Result<()> {
        tracing::info!("unfloating window");

        let workspace = self.focused_workspace_mut()?;
        workspace.new_container_for_floating_window()
    }

    #[tracing::instrument(skip(self))]
    pub fn toggle_monocle(&mut self) -> Result<()> {
        self.handle_unmanaged_window_behaviour()?;

        let workspace = self.focused_workspace()?;
        match workspace.monocle_container() {
            None => self.monocle_on()?,
            Some(_) => self.monocle_off()?,
        }

        self.update_focused_workspace(true, true)?;

        Ok(())
    }

    #[tracing::instrument(skip(self))]
    pub fn monocle_on(&mut self) -> Result<()> {
        tracing::info!("enabling monocle");

        let workspace = self.focused_workspace_mut()?;
        workspace.new_monocle_container()?;

        for container in workspace.containers_mut() {
            container.hide(None);
        }

        for window in workspace.floating_windows_mut() {
            window.hide();
        }

        Ok(())
    }

    #[tracing::instrument(skip(self))]
    pub fn monocle_off(&mut self) -> Result<()> {
        tracing::info!("disabling monocle");

        let workspace = self.focused_workspace_mut()?;

        for container in workspace.containers_mut() {
            container.restore();
        }

        for window in workspace.floating_windows_mut() {
            window.restore();
        }

        workspace.reintegrate_monocle_container()
    }

    #[tracing::instrument(skip(self))]
    pub fn toggle_maximize(&mut self) -> Result<()> {
        self.handle_unmanaged_window_behaviour()?;

        let workspace = self.focused_workspace_mut()?;

        match workspace.maximized_window() {
            None => self.maximize_window()?,
            Some(_) => self.unmaximize_window()?,
        }

        self.update_focused_workspace(true, false)
    }

    #[tracing::instrument(skip(self))]
    pub fn maximize_window(&mut self) -> Result<()> {
        tracing::info!("maximizing windowj");

        let workspace = self.focused_workspace_mut()?;
        workspace.new_maximized_window()
    }

    #[tracing::instrument(skip(self))]
    pub fn unmaximize_window(&mut self) -> Result<()> {
        tracing::info!("unmaximizing window");

        let workspace = self.focused_workspace_mut()?;
        workspace.reintegrate_maximized_window()
    }

    #[tracing::instrument(skip(self))]
    pub fn flip_layout(&mut self, layout_flip: Axis) -> Result<()> {
        let workspace = self.focused_workspace_mut()?;

        tracing::info!("flipping layout");

        #[allow(clippy::match_same_arms)]
        match workspace.layout_flip() {
            None => {
                workspace.set_layout_flip(Option::from(layout_flip));
            }
            Some(current_layout_flip) => {
                match current_layout_flip {
                    Axis::Horizontal => match layout_flip {
                        Axis::Horizontal => workspace.set_layout_flip(None),
                        Axis::Vertical => {
                            workspace.set_layout_flip(Option::from(Axis::HorizontalAndVertical))
                        }
                        Axis::HorizontalAndVertical => {
                            workspace.set_layout_flip(Option::from(Axis::HorizontalAndVertical))
                        }
                    },
                    Axis::Vertical => match layout_flip {
                        Axis::Horizontal => {
                            workspace.set_layout_flip(Option::from(Axis::HorizontalAndVertical))
                        }
                        Axis::Vertical => workspace.set_layout_flip(None),
                        Axis::HorizontalAndVertical => {
                            workspace.set_layout_flip(Option::from(Axis::HorizontalAndVertical))
                        }
                    },
                    Axis::HorizontalAndVertical => match layout_flip {
                        Axis::Horizontal => workspace.set_layout_flip(Option::from(Axis::Vertical)),
                        Axis::Vertical => workspace.set_layout_flip(Option::from(Axis::Horizontal)),
                        Axis::HorizontalAndVertical => workspace.set_layout_flip(None),
                    },
                };
            }
        }

        self.update_focused_workspace(false, false)
    }

    #[tracing::instrument(skip(self))]
    pub fn change_workspace_layout_default(&mut self, layout: DefaultLayout) -> Result<()> {
        tracing::info!("changing layout");

        let monitor_count = self.monitors().len();
        let workspace = self.focused_workspace_mut()?;

        if monitor_count > 1 && matches!(layout, DefaultLayout::Scrolling) {
            tracing::warn!(
                "scrolling layout is only supported for a single monitor; not changing layout"
            );
            return Ok(());
        }

        match workspace.layout() {
            Layout::Default(_) => {}
            Layout::Custom(layout) => {
                let primary_idx =
                    layout.first_container_idx(layout.primary_idx().ok_or_else(|| {
                        anyhow!("this custom layout does not have a primary column")
                    })?);

                if !workspace.containers().is_empty() && primary_idx < workspace.containers().len()
                {
                    workspace.swap_containers(0, primary_idx);
                }
            }
        }

        workspace.set_layout(Layout::Default(layout));
        self.update_focused_workspace(self.mouse_follows_focus, false)
    }

    #[tracing::instrument(skip(self))]
    pub fn cycle_layout(&mut self, direction: CycleDirection) -> Result<()> {
        tracing::info!("cycling layout");

        let workspace = self.focused_workspace_mut()?;
        let current_layout = workspace.layout();

        match current_layout {
            Layout::Default(current) => {
                let new_layout = match direction {
                    CycleDirection::Previous => current.cycle_previous(),
                    CycleDirection::Next => current.cycle_next(),
                };

                tracing::info!("next layout: {new_layout}");
                workspace.set_layout(Layout::Default(new_layout));
            }
            Layout::Custom(_) => {}
        }

        self.update_focused_workspace(self.mouse_follows_focus, false)
    }

    #[tracing::instrument(skip(self))]
    pub fn change_workspace_custom_layout<P>(&mut self, path: P) -> Result<()>
    where
        P: AsRef<Path> + std::fmt::Debug,
    {
        tracing::info!("changing layout");

        let layout = CustomLayout::from_path(path)?;
        let workspace = self.focused_workspace_mut()?;

        match workspace.layout() {
            Layout::Default(_) => {
                let primary_idx =
                    layout.first_container_idx(layout.primary_idx().ok_or_else(|| {
                        anyhow!("this custom layout does not have a primary column")
                    })?);

                if !workspace.containers().is_empty() && primary_idx < workspace.containers().len()
                {
                    workspace.swap_containers(0, primary_idx);
                }
            }
            Layout::Custom(_) => {}
        }

        workspace.set_layout(Layout::Custom(layout));
        workspace.set_layout_flip(None);
        self.update_focused_workspace(self.mouse_follows_focus, false)
    }

    #[tracing::instrument(skip(self))]
    pub fn adjust_workspace_padding(&mut self, sizing: Sizing, adjustment: i32) -> Result<()> {
        tracing::info!("adjusting workspace padding");

        let workspace = self.focused_workspace_mut()?;

        let padding = workspace
            .workspace_padding()
            .ok_or_else(|| anyhow!("there is no workspace padding"))?;

        workspace.set_workspace_padding(Option::from(sizing.adjust_by(padding, adjustment)));

        self.update_focused_workspace(false, false)
    }

    #[tracing::instrument(skip(self))]
    pub fn adjust_container_padding(&mut self, sizing: Sizing, adjustment: i32) -> Result<()> {
        tracing::info!("adjusting container padding");

        let workspace = self.focused_workspace_mut()?;

        let padding = workspace
            .container_padding()
            .ok_or_else(|| anyhow!("there is no container padding"))?;

        workspace.set_container_padding(Option::from(sizing.adjust_by(padding, adjustment)));

        self.update_focused_workspace(false, false)
    }

    #[tracing::instrument(skip(self))]
    pub fn set_workspace_tiling(
        &mut self,
        monitor_idx: usize,
        workspace_idx: usize,
        tile: bool,
    ) -> Result<()> {
        let monitor = self
            .monitors_mut()
            .get_mut(monitor_idx)
            .ok_or_else(|| anyhow!("there is no monitor"))?;

        let workspace = monitor
            .workspaces_mut()
            .get_mut(workspace_idx)
            .ok_or_else(|| anyhow!("there is no monitor"))?;

        workspace.set_tile(tile);

        self.update_focused_workspace(false, false)
    }

    #[tracing::instrument(skip(self))]
    pub fn add_workspace_layout_default_rule(
        &mut self,
        monitor_idx: usize,
        workspace_idx: usize,
        at_container_count: usize,
        layout: DefaultLayout,
    ) -> Result<()> {
        tracing::info!("setting workspace layout");

        let focused_monitor_idx = self.focused_monitor_idx();

        let monitor = self
            .monitors_mut()
            .get_mut(monitor_idx)
            .ok_or_else(|| anyhow!("there is no monitor"))?;

        let focused_workspace_idx = monitor.focused_workspace_idx();

        let workspace = monitor
            .workspaces_mut()
            .get_mut(workspace_idx)
            .ok_or_else(|| anyhow!("there is no monitor"))?;

        let rules: &mut Vec<(usize, Layout)> = workspace.layout_rules_mut();
        rules.retain(|pair| pair.0 != at_container_count);
        rules.push((at_container_count, Layout::Default(layout)));
        rules.sort_by(|a, b| a.0.cmp(&b.0));

        // If this is the focused workspace on a non-focused screen, let's update it
        if focused_monitor_idx != monitor_idx && focused_workspace_idx == workspace_idx {
            workspace.update()?;
            Ok(())
        } else {
            Ok(self.update_focused_workspace(false, false)?)
        }
    }

    #[tracing::instrument(skip(self))]
    pub fn add_workspace_layout_custom_rule<P>(
        &mut self,
        monitor_idx: usize,
        workspace_idx: usize,
        at_container_count: usize,
        path: P,
    ) -> Result<()>
    where
        P: AsRef<Path> + std::fmt::Debug,
    {
        tracing::info!("setting workspace layout");

        let focused_monitor_idx = self.focused_monitor_idx();

        let monitor = self
            .monitors_mut()
            .get_mut(monitor_idx)
            .ok_or_else(|| anyhow!("there is no monitor"))?;

        let focused_workspace_idx = monitor.focused_workspace_idx();

        let workspace = monitor
            .workspaces_mut()
            .get_mut(workspace_idx)
            .ok_or_else(|| anyhow!("there is no monitor"))?;

        let layout = CustomLayout::from_path(path)?;

        let rules: &mut Vec<(usize, Layout)> = workspace.layout_rules_mut();
        rules.retain(|pair| pair.0 != at_container_count);
        rules.push((at_container_count, Layout::Custom(layout)));
        rules.sort_by(|a, b| a.0.cmp(&b.0));

        // If this is the focused workspace on a non-focused screen, let's update it
        if focused_monitor_idx != monitor_idx && focused_workspace_idx == workspace_idx {
            workspace.update()?;
            Ok(())
        } else {
            Ok(self.update_focused_workspace(false, false)?)
        }
    }

    #[tracing::instrument(skip(self))]
    pub fn clear_workspace_layout_rules(
        &mut self,
        monitor_idx: usize,
        workspace_idx: usize,
    ) -> Result<()> {
        tracing::info!("setting workspace layout");

        let focused_monitor_idx = self.focused_monitor_idx();

        let monitor = self
            .monitors_mut()
            .get_mut(monitor_idx)
            .ok_or_else(|| anyhow!("there is no monitor"))?;

        let focused_workspace_idx = monitor.focused_workspace_idx();

        let workspace = monitor
            .workspaces_mut()
            .get_mut(workspace_idx)
            .ok_or_else(|| anyhow!("there is no monitor"))?;

        let rules: &mut Vec<(usize, Layout)> = workspace.layout_rules_mut();
        rules.clear();

        // If this is the focused workspace on a non-focused screen, let's update it
        if focused_monitor_idx != monitor_idx && focused_workspace_idx == workspace_idx {
            workspace.update()?;
            Ok(())
        } else {
            Ok(self.update_focused_workspace(false, false)?)
        }
    }

    #[tracing::instrument(skip(self))]
    pub fn set_workspace_layout_default(
        &mut self,
        monitor_idx: usize,
        workspace_idx: usize,
        layout: DefaultLayout,
    ) -> Result<()> {
        tracing::info!("setting workspace layout");

        let focused_monitor_idx = self.focused_monitor_idx();

        let monitor = self
            .monitors_mut()
            .get_mut(monitor_idx)
            .ok_or_else(|| anyhow!("there is no monitor"))?;

        let focused_workspace_idx = monitor.focused_workspace_idx();

        let workspace = monitor
            .workspaces_mut()
            .get_mut(workspace_idx)
            .ok_or_else(|| anyhow!("there is no monitor"))?;

        workspace.set_layout(Layout::Default(layout));

        // If this is the focused workspace on a non-focused screen, let's update it
        if focused_monitor_idx != monitor_idx && focused_workspace_idx == workspace_idx {
            workspace.update()?;
            Ok(())
        } else {
            Ok(self.update_focused_workspace(false, false)?)
        }
    }

    #[tracing::instrument(skip(self))]
    pub fn set_workspace_layout_custom<P>(
        &mut self,
        monitor_idx: usize,
        workspace_idx: usize,
        path: P,
    ) -> Result<()>
    where
        P: AsRef<Path> + std::fmt::Debug,
    {
        tracing::info!("setting workspace layout");
        let layout = CustomLayout::from_path(path)?;
        let focused_monitor_idx = self.focused_monitor_idx();

        let monitor = self
            .monitors_mut()
            .get_mut(monitor_idx)
            .ok_or_else(|| anyhow!("there is no monitor"))?;

        let focused_workspace_idx = monitor.focused_workspace_idx();

        let workspace = monitor
            .workspaces_mut()
            .get_mut(workspace_idx)
            .ok_or_else(|| anyhow!("there is no monitor"))?;

        workspace.set_layout(Layout::Custom(layout));
        workspace.set_layout_flip(None);

        // If this is the focused workspace on a non-focused screen, let's update it
        if focused_monitor_idx != monitor_idx && focused_workspace_idx == workspace_idx {
            workspace.update()?;
            Ok(())
        } else {
            Ok(self.update_focused_workspace(false, false)?)
        }
    }

    #[tracing::instrument(skip(self))]
    pub fn ensure_workspaces_for_monitor(
        &mut self,
        monitor_idx: usize,
        workspace_count: usize,
    ) -> Result<()> {
        tracing::info!("ensuring workspace count");

        let monitor = self
            .monitors_mut()
            .get_mut(monitor_idx)
            .ok_or_else(|| anyhow!("there is no monitor"))?;

        monitor.ensure_workspace_count(workspace_count);

        Ok(())
    }

    #[tracing::instrument(skip(self))]
    pub fn ensure_named_workspaces_for_monitor(
        &mut self,
        monitor_idx: usize,
        names: &Vec<String>,
    ) -> Result<()> {
        tracing::info!("ensuring workspace count");

        let monitor = self
            .monitors_mut()
            .get_mut(monitor_idx)
            .ok_or_else(|| anyhow!("there is no monitor"))?;

        monitor.ensure_workspace_count(names.len());

        for (workspace_idx, name) in names.iter().enumerate() {
            if let Some(workspace) = monitor.workspaces_mut().get_mut(workspace_idx) {
                workspace.set_name(Option::from(name.clone()));
            }
        }

        Ok(())
    }

    #[tracing::instrument(skip(self))]
    pub fn set_workspace_padding(
        &mut self,
        monitor_idx: usize,
        workspace_idx: usize,
        size: i32,
    ) -> Result<()> {
        tracing::info!("setting workspace padding");

        let monitor = self
            .monitors_mut()
            .get_mut(monitor_idx)
            .ok_or_else(|| anyhow!("there is no monitor"))?;

        let workspace = monitor
            .workspaces_mut()
            .get_mut(workspace_idx)
            .ok_or_else(|| anyhow!("there is no monitor"))?;

        workspace.set_workspace_padding(Option::from(size));

        self.update_focused_workspace(false, false)
    }

    #[tracing::instrument(skip(self))]
    pub fn set_workspace_name(
        &mut self,
        monitor_idx: usize,
        workspace_idx: usize,
        name: String,
    ) -> Result<()> {
        tracing::info!("setting workspace name");

        let monitor = self
            .monitors_mut()
            .get_mut(monitor_idx)
            .ok_or_else(|| anyhow!("there is no monitor"))?;

        let workspace = monitor
            .workspaces_mut()
            .get_mut(workspace_idx)
            .ok_or_else(|| anyhow!("there is no monitor"))?;

        workspace.set_name(Option::from(name.clone()));
        monitor.workspace_names_mut().insert(workspace_idx, name);

        Ok(())
    }

    #[tracing::instrument(skip(self))]
    pub fn set_container_padding(
        &mut self,
        monitor_idx: usize,
        workspace_idx: usize,
        size: i32,
    ) -> Result<()> {
        tracing::info!("setting container padding");

        let monitor = self
            .monitors_mut()
            .get_mut(monitor_idx)
            .ok_or_else(|| anyhow!("there is no monitor"))?;

        let workspace = monitor
            .workspaces_mut()
            .get_mut(workspace_idx)
            .ok_or_else(|| anyhow!("there is no monitor"))?;

        workspace.set_container_padding(Option::from(size));

        self.update_focused_workspace(false, false)
    }

    pub fn focused_monitor_size(&self) -> Result<Rect> {
        Ok(*self
            .focused_monitor()
            .ok_or_else(|| anyhow!("there is no monitor"))?
            .size())
    }

    pub fn focused_monitor_work_area(&self) -> Result<Rect> {
        Ok(*self
            .focused_monitor()
            .ok_or_else(|| anyhow!("there is no monitor"))?
            .work_area_size())
    }

    #[tracing::instrument(skip(self))]
    pub fn focus_monitor(&mut self, idx: usize) -> Result<()> {
        tracing::info!("focusing monitor");

        if self.monitors().get(idx).is_some() {
            self.monitors.focus(idx);
        } else {
            bail!("this is not a valid monitor index");
        }

        Ok(())
    }

    pub fn monitor_idx_from_window(&mut self, window: Window) -> Option<usize> {
        let hmonitor = WindowsApi::monitor_from_window(window.hwnd);

        for (i, monitor) in self.monitors().iter().enumerate() {
            if monitor.id() == hmonitor {
                return Option::from(i);
            }
        }

        // our hmonitor might be stale, so if we didn't return above, try querying via the latest
        // info taken from win32_display_data and update our hmonitor while we're at it
        if let Ok(latest) = WindowsApi::monitor(hmonitor) {
            for (i, monitor) in self.monitors_mut().iter_mut().enumerate() {
                if monitor.device_id() == latest.device_id() {
                    monitor.set_id(latest.id());
                    return Option::from(i);
                }
            }
        }

        None
    }

    pub fn monitor_idx_from_current_pos(&mut self) -> Option<usize> {
        let hmonitor = WindowsApi::monitor_from_point(WindowsApi::cursor_pos().ok()?);

        for (i, monitor) in self.monitors().iter().enumerate() {
            if monitor.id() == hmonitor {
                return Option::from(i);
            }
        }

        // our hmonitor might be stale, so if we didn't return above, try querying via the latest
        // info taken from win32_display_data and update our hmonitor while we're at it
        if let Ok(latest) = WindowsApi::monitor(hmonitor) {
            for (i, monitor) in self.monitors_mut().iter_mut().enumerate() {
                if monitor.device_id() == latest.device_id() {
                    monitor.set_id(latest.id());
                    return Option::from(i);
                }
            }
        }

        None
    }

    pub fn focused_workspace_idx(&self) -> Result<usize> {
        Ok(self
            .focused_monitor()
            .ok_or_else(|| anyhow!("there is no monitor"))?
            .focused_workspace_idx())
    }

    pub fn focused_workspace(&self) -> Result<&Workspace> {
        self.focused_monitor()
            .ok_or_else(|| anyhow!("there is no monitor"))?
            .focused_workspace()
            .ok_or_else(|| anyhow!("there is no workspace"))
    }

    pub fn focused_workspace_mut(&mut self) -> Result<&mut Workspace> {
        self.focused_monitor_mut()
            .ok_or_else(|| anyhow!("there is no monitor"))?
            .focused_workspace_mut()
            .ok_or_else(|| anyhow!("there is no workspace"))
    }

    pub fn focused_workspace_idx_for_monitor_idx(&self, idx: usize) -> Result<usize> {
        Ok(self
            .monitors()
            .get(idx)
            .ok_or_else(|| anyhow!("there is no monitor at this index"))?
            .focused_workspace_idx())
    }

    pub fn focused_workspace_for_monitor_idx(&self, idx: usize) -> Result<&Workspace> {
        self.monitors()
            .get(idx)
            .ok_or_else(|| anyhow!("there is no monitor at this index"))?
            .focused_workspace()
            .ok_or_else(|| anyhow!("there is no workspace"))
    }

    pub fn focused_workspace_for_monitor_idx_mut(&mut self, idx: usize) -> Result<&mut Workspace> {
        self.monitors_mut()
            .get_mut(idx)
            .ok_or_else(|| anyhow!("there is no monitor at this index"))?
            .focused_workspace_mut()
            .ok_or_else(|| anyhow!("there is no workspace"))
    }

    #[tracing::instrument(skip(self))]
    pub fn focus_workspace(&mut self, idx: usize) -> Result<()> {
        tracing::info!("focusing workspace");

        let mouse_follows_focus = self.mouse_follows_focus;
        let monitor = self
            .focused_monitor_mut()
            .ok_or_else(|| anyhow!("there is no workspace"))?;

        monitor.focus_workspace(idx)?;
        monitor.load_focused_workspace(mouse_follows_focus)?;

        self.update_focused_workspace(false, true)
    }

    #[tracing::instrument(skip(self))]
    pub fn monitor_workspace_index_by_name(&mut self, name: &str) -> Option<(usize, usize)> {
        tracing::info!("looking up workspace by name");

        for (monitor_idx, monitor) in self.monitors().iter().enumerate() {
            for (workspace_idx, workspace) in monitor.workspaces().iter().enumerate() {
                if let Some(workspace_name) = workspace.name() {
                    if workspace_name == name {
                        return Option::from((monitor_idx, workspace_idx));
                    }
                }
            }
        }

        None
    }

    #[tracing::instrument(skip(self))]
    pub fn new_workspace(&mut self) -> Result<()> {
        tracing::info!("adding new workspace");

        let mouse_follows_focus = self.mouse_follows_focus;
        let monitor = self
            .focused_monitor_mut()
            .ok_or_else(|| anyhow!("there is no workspace"))?;

        monitor.focus_workspace(monitor.new_workspace_idx())?;
        monitor.load_focused_workspace(mouse_follows_focus)?;

        self.update_focused_workspace(self.mouse_follows_focus, false)
    }

    pub fn focused_container(&self) -> Result<&Container> {
        self.focused_workspace()?
            .focused_container()
            .ok_or_else(|| anyhow!("there is no container"))
    }

    pub fn focused_container_idx(&self) -> Result<usize> {
        Ok(self.focused_workspace()?.focused_container_idx())
    }

    pub fn focused_container_mut(&mut self) -> Result<&mut Container> {
        self.focused_workspace_mut()?
            .focused_container_mut()
            .ok_or_else(|| anyhow!("there is no container"))
    }

    pub fn focused_window(&self) -> Result<&Window> {
        self.focused_container()?
            .focused_window()
            .ok_or_else(|| anyhow!("there is no window"))
    }

    fn focused_window_mut(&mut self) -> Result<&mut Window> {
        self.focused_container_mut()?
            .focused_window_mut()
            .ok_or_else(|| anyhow!("there is no window"))
    }

    /// Updates the list of `known_hwnds` and their monitor/workspace index pair
    ///
    /// [`known_hwnds`]: `Self.known_hwnds`
    pub fn update_known_hwnds(&mut self) {
        tracing::trace!("updating list of known hwnds");
        let mut known_hwnds = HashMap::new();
        for (m_idx, monitor) in self.monitors().iter().enumerate() {
            for (w_idx, workspace) in monitor.workspaces().iter().enumerate() {
                for container in workspace.containers() {
                    for window in container.windows() {
                        known_hwnds.insert(window.hwnd, (m_idx, w_idx));
                    }
                }

                for window in workspace.floating_windows() {
                    known_hwnds.insert(window.hwnd, (m_idx, w_idx));
                }

                if let Some(window) = workspace.maximized_window() {
                    known_hwnds.insert(window.hwnd, (m_idx, w_idx));
                }

                if let Some(container) = workspace.monocle_container() {
                    for window in container.windows() {
                        known_hwnds.insert(window.hwnd, (m_idx, w_idx));
                    }
                }
            }
        }

        if self.known_hwnds != known_hwnds {
            // Update reaper cache
            {
                let mut reaper_cache = crate::reaper::HWNDS_CACHE.lock();
                *reaper_cache = known_hwnds.clone();
            }

            // Save to file
            let hwnd_json = DATA_DIR.join("komorebi.hwnd.json");
            match OpenOptions::new()
                .write(true)
                .truncate(true)
                .create(true)
                .open(hwnd_json)
            {
                Ok(file) => {
                    if let Err(error) =
                        serde_json::to_writer_pretty(&file, &known_hwnds.keys().collect::<Vec<_>>())
                    {
                        tracing::error!("Failed to save list of known_hwnds on file: {}", error);
                    }
                }
                Err(error) => {
                    tracing::error!("Failed to save list of known_hwnds on file: {}", error);
                }
            }

            // Store new hwnds
            self.known_hwnds = known_hwnds;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::monitor;
    use crossbeam_channel::bounded;
    use crossbeam_channel::Sender;
    use std::path::PathBuf;
    use uuid::Uuid;

    struct TestContext {
        socket_path: Option<PathBuf>,
    }

    impl Drop for TestContext {
        fn drop(&mut self) {
            if let Some(socket_path) = &self.socket_path {
                // Clean up the socket file
                std::fs::remove_file(socket_path).unwrap();
            }
        }
    }

    fn setup_window_manager() -> (WindowManager, TestContext) {
        let (_sender, receiver): (Sender<WindowManagerEvent>, Receiver<WindowManagerEvent>) =
            bounded(1);

        // Temporary socket path for testing
        let socket_name = format!("komorebi-test-{}.sock", Uuid::new_v4());
        let socket_path = PathBuf::from(socket_name);

        // Create a new WindowManager instance
        let wm = WindowManager::new(receiver, Some(socket_path.clone()));

        // Window Manager should be created successfully
        assert!(wm.is_ok());

        (
            wm.unwrap(),
            TestContext {
                socket_path: Some(socket_path),
            },
        )
    }

    #[test]
    fn test_create_window_manager() {
        let (_wm, _test_context) = setup_window_manager();
    }

    #[test]
    fn test_focus_workspace() {
        let (mut wm, _test_context) = setup_window_manager();

        let m = monitor::new(
            0,
            Rect::default(),
            Rect::default(),
            "TestMonitor".to_string(),
            "TestDevice".to_string(),
            "TestDeviceID".to_string(),
            Some("TestMonitorID".to_string()),
        );

        // a new monitor should have a single workspace
        assert_eq!(m.workspaces().len(), 1);

        // the next index on the monitor should be the not-yet-created second workspace
        let new_workspace_index = m.new_workspace_idx();
        assert_eq!(new_workspace_index, 1);

        // add the monitor to the window manager
        wm.monitors_mut().push_back(m);

        {
            // focusing a workspace which doesn't yet exist should create it
            let monitor = wm.focused_monitor_mut().unwrap();
            monitor.focus_workspace(new_workspace_index).unwrap();
            assert_eq!(monitor.workspaces().len(), 2);
        }
        assert_eq!(wm.focused_workspace_idx().unwrap(), 1);

        {
            // focusing a workspace many indices ahead should create all workspaces
            // required along the way
            let monitor = wm.focused_monitor_mut().unwrap();
            monitor.focus_workspace(new_workspace_index + 2).unwrap();
            assert_eq!(monitor.workspaces().len(), 4);
        }
        assert_eq!(wm.focused_workspace_idx().unwrap(), 3);

        // we should be able to successfully focus an existing workspace too
        wm.focus_workspace(0).unwrap();
        assert_eq!(wm.focused_workspace_idx().unwrap(), 0);
    }

    #[test]
    fn test_remove_focused_workspace() {
        let (mut wm, _context) = setup_window_manager();

        let m = monitor::new(
            0,
            Rect::default(),
            Rect::default(),
            "TestMonitor".to_string(),
            "TestDevice".to_string(),
            "TestDeviceID".to_string(),
            Some("TestMonitorID".to_string()),
        );

        // a new monitor should have a single workspace
        assert_eq!(m.workspaces().len(), 1);

        // the next index on the monitor should be the not-yet-created second workspace
        let new_workspace_index = m.new_workspace_idx();
        assert_eq!(new_workspace_index, 1);

        // add the monitor to the window manager
        wm.monitors_mut().push_back(m);

        {
            // focus a workspace which doesn't yet exist should create it
            let monitor = wm.focused_monitor_mut().unwrap();
            monitor.focus_workspace(new_workspace_index + 1).unwrap();

            // Monitor focused workspace should be 2
            assert_eq!(monitor.focused_workspace_idx(), 2);

            // Should have 3 Workspaces
            assert_eq!(monitor.workspaces().len(), 3);
        }

        // Remove the focused workspace
        wm.remove_focused_workspace().unwrap();

        {
            let monitor = wm.focused_monitor_mut().unwrap();
            monitor.focus_workspace(new_workspace_index).unwrap();

            // Should be focused on workspace 1
            assert_eq!(monitor.focused_workspace_idx(), 1);

            // Should have 2 Workspaces
            assert_eq!(monitor.workspaces().len(), 2);
        }
    }

    #[test]
    fn test_set_workspace_name() {
        let (mut wm, _test_context) = setup_window_manager();

        let m = monitor::new(
            0,
            Rect::default(),
            Rect::default(),
            "TestMonitor".to_string(),
            "TestDevice".to_string(),
            "TestDeviceID".to_string(),
            Some("TestMonitorID".to_string()),
        );

        // a new monitor should have a single workspace
        assert_eq!(m.workspaces().len(), 1);

        // create a new workspace
        let new_workspace_index = m.new_workspace_idx();
        assert_eq!(new_workspace_index, 1);

        // add the monitor to the window manager
        wm.monitors_mut().push_back(m);

        {
            // focusing a workspace which doesn't yet exist should create it
            let monitor = wm.focused_monitor_mut().unwrap();
            monitor.focus_workspace(new_workspace_index).unwrap();
            assert_eq!(monitor.workspaces().len(), 2);
        }
        assert_eq!(wm.focused_workspace_idx().unwrap(), 1);

        // set the name of the first workspace
        wm.set_workspace_name(0, 0, "workspace1".to_string())
            .unwrap();

        // monitor_workspace_index_by_name should return the index of the workspace with the name "workspace1"
        let workspace_index = wm.monitor_workspace_index_by_name("workspace1").unwrap();

        // workspace index 0 should now have the name "workspace1"
        assert_eq!(workspace_index.1, 0);
    }

    #[test]
    fn test_switch_focus_monitors() {
        let (mut wm, _test_context) = setup_window_manager();

        {
            // Create a first monitor
            let m = monitor::new(
                0,
                Rect::default(),
                Rect::default(),
                "TestMonitor".to_string(),
                "TestDevice".to_string(),
                "TestDeviceID".to_string(),
                Some("TestMonitorID".to_string()),
            );

            // monitor should have a single workspace
            assert_eq!(m.workspaces().len(), 1);

            // add the monitor to the window manager
            wm.monitors_mut().push_back(m);
        }
        assert_eq!(wm.monitors().len(), 1);

        {
            // Create a second monitor
            let m = monitor::new(
                1,
                Rect::default(),
                Rect::default(),
                "TestMonitor2".to_string(),
                "TestDevice2".to_string(),
                "TestDeviceID2".to_string(),
                Some("TestMonitorID2".to_string()),
            );

            // monitor should have a single workspace
            assert_eq!(m.workspaces().len(), 1);

            // add the monitor to the window manager
            wm.monitors_mut().push_back(m);
        }
        assert_eq!(wm.monitors().len(), 2);

        {
            // Create a third monitor
            let m = monitor::new(
                2,
                Rect::default(),
                Rect::default(),
                "TestMonitor3".to_string(),
                "TestDevice3".to_string(),
                "TestDeviceID3".to_string(),
                Some("TestMonitorID3".to_string()),
            );

            // monitor should have a single workspace
            assert_eq!(m.workspaces().len(), 1);

            // add the monitor to the window manager
            wm.monitors_mut().push_back(m);
        }
        assert_eq!(wm.monitors().len(), 3);

        {
            // Set the first monitor as focused and check if it is focused
            wm.focus_monitor(0).unwrap();
            let current_monitor_idx = wm.monitors.focused_idx();
            assert_eq!(current_monitor_idx, 0);
        }

        {
            // Set the second monitor as focused and check if it is focused
            wm.focus_monitor(1).unwrap();
            let current_monitor_idx = wm.monitors.focused_idx();
            assert_eq!(current_monitor_idx, 1);
        }

        {
            // Set the third monitor as focused and check if it is focused
            wm.focus_monitor(2).unwrap();
            let current_monitor_idx = wm.monitors.focused_idx();
            assert_eq!(current_monitor_idx, 2);
        }

        // Switch back to the first monitor
        wm.focus_monitor(0).unwrap();
        let current_monitor_idx = wm.monitors.focused_idx();
        assert_eq!(current_monitor_idx, 0);
    }

    #[test]
    fn test_switch_focus_to_nonexistent_monitor() {
        let (mut wm, _test_context) = setup_window_manager();

        {
            // Create a first monitor
            let m = monitor::new(
                0,
                Rect::default(),
                Rect::default(),
                "TestMonitor".to_string(),
                "TestDevice".to_string(),
                "TestDeviceID".to_string(),
                Some("TestMonitorID".to_string()),
            );

            // monitor should have a single workspace
            assert_eq!(m.workspaces().len(), 1);

            // add the monitor to the window manager
            wm.monitors_mut().push_back(m);
        }

        // Should have 1 monitor and the monitor index should be 0
        assert_eq!(wm.monitors().len(), 1);
        assert_eq!(wm.focused_monitor_idx(), 0);

        // Should receive an error when trying to focus a non-existent monitor
        let result = wm.focus_monitor(1);
        assert!(
            result.is_err(),
            "Expected an error when focusing a non-existent monitor"
        );

        // Should still be focused on the first monitor
        assert_eq!(wm.focused_monitor_idx(), 0);
    }

    #[test]
    fn test_focused_monitor_size() {
        let (mut wm, _test_context) = setup_window_manager();

        {
            // Create a first monitor
            let m = monitor::new(
                0,
                Rect::default(),
                Rect::default(),
                "TestMonitor".to_string(),
                "TestDevice".to_string(),
                "TestDeviceID".to_string(),
                Some("TestMonitorID".to_string()),
            );

            // monitor should have a single workspace
            assert_eq!(m.workspaces().len(), 1);

            // add the monitor to the window manager
            wm.monitors_mut().push_back(m);
        }
        assert_eq!(wm.monitors().len(), 1);

        {
            // Set the first monitor as focused and check if it is focused
            wm.focus_monitor(0).unwrap();
            let current_monitor_size = wm.focused_monitor_size().unwrap();
            assert_eq!(current_monitor_size, Rect::default());
        }
    }

    #[test]
    fn test_focus_container_in_cycle_direction() {
        let (mut wm, _test_context) = setup_window_manager();

        // Create a monitor
        let mut m = monitor::new(
            0,
            Rect::default(),
            Rect::default(),
            "TestMonitor".to_string(),
            "TestDevice".to_string(),
            "TestDeviceID".to_string(),
            Some("TestMonitorID".to_string()),
        );

        let workspace = m.focused_workspace_mut().unwrap();
        workspace.set_layer(WorkspaceLayer::Tiling);

        for i in 0..4 {
            let mut container = Container::default();
            container.windows_mut().push_back(Window::from(i));
            workspace.add_container_to_back(container);
        }
        assert_eq!(workspace.containers().len(), 4);

        workspace.focus_container(0);

        // add the monitor to the window manager
        wm.monitors_mut().push_back(m);

        // container focus should be on the second container
        wm.focus_container_in_cycle_direction(CycleDirection::Next)
            .ok();
        assert_eq!(wm.focused_container_idx().unwrap(), 1);

        // container focus should be on the third container
        wm.focus_container_in_cycle_direction(CycleDirection::Next)
            .ok();
        assert_eq!(wm.focused_container_idx().unwrap(), 2);

        // container focus should be on the second container
        wm.focus_container_in_cycle_direction(CycleDirection::Previous)
            .ok();
        assert_eq!(wm.focused_container_idx().unwrap(), 1);

        // container focus should be on the first container
        wm.focus_container_in_cycle_direction(CycleDirection::Previous)
            .ok();
        assert_eq!(wm.focused_container_idx().unwrap(), 0);
    }

    #[test]
    fn test_transfer_window() {
        let (mut wm, _context) = setup_window_manager();

        {
            // Create a first monitor
            let mut m = monitor::new(
                0,
                Rect::default(),
                Rect::default(),
                "TestMonitor".to_string(),
                "TestDevice".to_string(),
                "TestDeviceID".to_string(),
                Some("TestMonitorID".to_string()),
            );

            // Create a container
            let workspace = m.focused_workspace_mut().unwrap();
            let mut container = Container::default();

            // Add a window to the container
            container.windows_mut().push_back(Window::from(0));
            workspace.add_container_to_back(container);

            // Should contain 1 container
            assert_eq!(workspace.containers().len(), 1);

            wm.monitors_mut().push_back(m);
        }

        {
            // Create a second monitor
            let mut m = monitor::new(
                1,
                Rect::default(),
                Rect::default(),
                "TestMonitor2".to_string(),
                "TestDevice2".to_string(),
                "TestDeviceID2".to_string(),
                Some("TestMonitorID2".to_string()),
            );

            // Create a container
            let workspace = m.focused_workspace_mut().unwrap();
            let mut container = Container::default();

            // Add a window to the container
            container.windows_mut().push_back(Window::from(1));
            workspace.add_container_to_back(container);

            // Should contain 1 container
            assert_eq!(workspace.containers().len(), 1);

            wm.monitors_mut().push_back(m);
        }

        // Should contain 2 monitors
        assert_eq!(wm.monitors().len(), 2);

        {
            // Monitor 0, Workspace 0, Window 0
            let origin = (0, 0, 0);

            // Monitor 1, Workspace 0, Window 0
            let target = (1, 0, 0);

            // Transfer the window from monitor 0 to monitor 1
            wm.transfer_window(origin, target).unwrap();

            // Monitor 1 should contain 0 containers
            let workspace = wm.focused_workspace_mut().unwrap();
            assert_eq!(workspace.containers().len(), 0);

            // Monitor 2 should contain 2 containers
            wm.focus_monitor(1).unwrap();
            let workspace = wm.focused_workspace_mut().unwrap();
            assert_eq!(workspace.containers().len(), 2);
        }

        {
            // Monitor 1, Workspace 0, Window 0
            let origin = (1, 0, 0);

            // Monitor 0, Workspace 0, Window 0
            let target = (0, 0, 0);

            // Transfer the window from monitor 1 back to monitor 0
            wm.transfer_window(origin, target).unwrap();

            // Monitor 2 should contain 1 containers
            let workspace = wm.focused_workspace_mut().unwrap();
            assert_eq!(workspace.containers().len(), 1);

            // Monitor 1 should contain 1 containers
            wm.focus_monitor(0).unwrap();
            let workspace = wm.focused_workspace_mut().unwrap();
            assert_eq!(workspace.containers().len(), 1);
        }
    }

    #[test]
    fn test_transfer_window_to_nonexistent_monitor() {
        // NOTE: transfer_window is primarily used when a window is being dragged by a mouse. The
        // transfer_window function does return an error when the target monitor doesn't exist but
        // there is a bug where the window isn't in the container after the window fails to
        // transfer. The test will test for the result of the transfer_window function but not if
        // the window is in the container after the transfer fails.

        let (mut wm, _context) = setup_window_manager();

        {
            // Create a first monitor
            let mut m = monitor::new(
                0,
                Rect::default(),
                Rect::default(),
                "TestMonitor".to_string(),
                "TestDevice".to_string(),
                "TestDeviceID".to_string(),
                Some("TestMonitorID".to_string()),
            );

            // Create a container
            let workspace = m.focused_workspace_mut().unwrap();
            let mut container = Container::default();

            // Add a window to the container
            container.windows_mut().push_back(Window::from(0));
            workspace.add_container_to_back(container);

            // Should contain 1 container
            assert_eq!(workspace.containers().len(), 1);

            wm.monitors_mut().push_back(m);
        }

        {
            // Monitor 0, Workspace 0, Window 0
            let origin = (0, 0, 0);

            // Monitor 1, Workspace 0, Window 0
            //
            let target = (1, 0, 0);

            // Attempt to transfer the window from monitor 0 to a non-existent monitor
            let result = wm.transfer_window(origin, target);

            // Result should be an error since the monitor doesn't exist
            assert!(
                result.is_err(),
                "Expected an error when transferring to a non-existent monitor"
            );

            assert_eq!(wm.focused_container_idx().unwrap(), 0);
            assert_eq!(wm.focused_workspace_idx().unwrap(), 0);
        }
    }

    #[test]
    fn test_transfer_container() {
        let (mut wm, _context) = setup_window_manager();

        {
            // Create a first monitor
            let mut m = monitor::new(
                0,
                Rect::default(),
                Rect::default(),
                "TestMonitor".to_string(),
                "TestDevice".to_string(),
                "TestDeviceID".to_string(),
                Some("TestMonitorID".to_string()),
            );

            // Create a container
            let workspace = m.focused_workspace_mut().unwrap();
            let mut container = Container::default();

            // Add a window to the container
            container.windows_mut().push_back(Window::from(0));
            workspace.add_container_to_back(container);

            // Should contain 1 container
            assert_eq!(workspace.containers().len(), 1);

            wm.monitors_mut().push_back(m);
        }

        {
            // Create a second monitor
            let mut m = monitor::new(
                1,
                Rect::default(),
                Rect::default(),
                "TestMonitor2".to_string(),
                "TestDevice2".to_string(),
                "TestDeviceID2".to_string(),
                Some("TestMonitorID2".to_string()),
            );

            // Create a container
            let workspace = m.focused_workspace_mut().unwrap();
            let mut container = Container::default();

            // Add a window to the container
            container.windows_mut().push_back(Window::from(1));
            workspace.add_container_to_back(container);

            // Should contain 1 container
            assert_eq!(workspace.containers().len(), 1);

            wm.monitors_mut().push_back(m);
        }

        // Should contain 2 monitors
        assert_eq!(wm.monitors().len(), 2);

        {
            // Monitor 0, Workspace 0, Window 0
            let origin = (0, 0, 0);

            // Monitor 1, Workspace 0, Window 0
            let target = (1, 0, 0);

            // Transfer the window from monitor 0 to monitor 1
            wm.transfer_container(origin, target).unwrap();

            // Monitor 1 should contain 0 containers
            let workspace = wm.focused_workspace_mut().unwrap();
            assert_eq!(workspace.containers().len(), 0);

            // Monitor 2 should contain 2 containers
            wm.focus_monitor(1).unwrap();
            let workspace = wm.focused_workspace_mut().unwrap();
            assert_eq!(workspace.containers().len(), 2);
        }

        {
            // Monitor 1, Workspace 0, Window 0
            let origin = (1, 0, 0);

            // Monitor 0, Workspace 0, Window 0
            let target = (0, 0, 0);

            // Transfer the window from monitor 1 back to monitor 0
            wm.transfer_container(origin, target).unwrap();

            // Monitor 2 should contain 1 containers
            let workspace = wm.focused_workspace_mut().unwrap();
            assert_eq!(workspace.containers().len(), 1);

            // Monitor 1 should contain 1 containers
            wm.focus_monitor(0).unwrap();
            let workspace = wm.focused_workspace_mut().unwrap();
            assert_eq!(workspace.containers().len(), 1);
        }
    }

    #[test]
    fn test_remove_window_from_container() {
        let (mut wm, _context) = setup_window_manager();

        {
            // Create a first monitor
            let mut m = monitor::new(
                0,
                Rect::default(),
                Rect::default(),
                "TestMonitor1".to_string(),
                "TestDevice1".to_string(),
                "TestDeviceID1".to_string(),
                Some("TestMonitorID1".to_string()),
            );

            // Create a container
            let mut container = Container::default();

            // Add three windows to the container
            for i in 0..3 {
                container.windows_mut().push_back(Window::from(i));
            }
            // Should have 3 windows in the container
            assert_eq!(container.windows().len(), 3);

            // Focus last window
            container.focus_window(2);

            // Should be focused on the 2nd window
            assert_eq!(container.focused_window_idx(), 2);

            // Add the container to a workspace
            let workspace = m.focused_workspace_mut().unwrap();
            workspace.add_container_to_back(container);

            // Add monitor to the window manager
            wm.monitors_mut().push_back(m);
        }

        // Remove the focused window from the container
        wm.remove_window_from_container().ok();

        {
            // Should have 2 containers in the workspace
            let workspace = wm.focused_workspace_mut().unwrap();
            assert_eq!(workspace.containers().len(), 2);

            // Should contain 1 window in the new container
            let container = workspace.focused_container_mut().unwrap();
            assert_eq!(container.windows().len(), 1);
        }

        {
            // Switch to the old container
            let workspace = wm.focused_workspace_mut().unwrap();
            workspace.focus_container(0);

            // Should contain 2 windows in the old container
            let container = workspace.focused_container_mut().unwrap();
            assert_eq!(container.windows().len(), 2);
        }
    }

    #[test]
    fn cycle_container_window_in_direction() {
        let (mut wm, _context) = setup_window_manager();

        {
            let mut m = monitor::new(
                0,
                Rect::default(),
                Rect::default(),
                "TestMonitor".to_string(),
                "TestDevice".to_string(),
                "TestDeviceID".to_string(),
                Some("TestMonitorID".to_string()),
            );

            let workspace = m.focused_workspace_mut().unwrap();

            {
                let mut container = Container::default();

                for i in 0..3 {
                    container.windows_mut().push_back(Window::from(i));
                }

                // Should have 3 windows in the container
                assert_eq!(container.windows().len(), 3);

                // Add container to workspace
                workspace.add_container_to_back(container);
            }

            // Add monitor to the window manager
            wm.monitors_mut().push_back(m);
        }

        // Cycle to the next window
        wm.cycle_container_window_in_direction(CycleDirection::Next)
            .ok();

        {
            // Should be on Window 1
            let workspace = wm.focused_workspace_mut().unwrap();
            let container = workspace.focused_container_mut().unwrap();
            assert_eq!(container.focused_window_idx(), 1);
        }

        // Cycle to the next window
        wm.cycle_container_window_in_direction(CycleDirection::Next)
            .ok();

        {
            // Should be on Window 2
            let workspace = wm.focused_workspace_mut().unwrap();
            let container = workspace.focused_container_mut().unwrap();
            assert_eq!(container.focused_window_idx(), 2);
        }

        // Cycle to the previous window
        wm.cycle_container_window_in_direction(CycleDirection::Previous)
            .ok();

        {
            // Should be on Window 1
            let workspace = wm.focused_workspace_mut().unwrap();
            let container = workspace.focused_container_mut().unwrap();
            assert_eq!(container.focused_window_idx(), 1);
        }
    }

    #[test]
    fn test_cycle_container_window_index_in_direction() {
        let (mut wm, _context) = setup_window_manager();

        {
            let mut m = monitor::new(
                0,
                Rect::default(),
                Rect::default(),
                "TestMonitor".to_string(),
                "TestDevice".to_string(),
                "TestDeviceID".to_string(),
                Some("TestMonitorID".to_string()),
            );

            let workspace = m.focused_workspace_mut().unwrap();

            {
                let mut container = Container::default();

                for i in 0..3 {
                    container.windows_mut().push_back(Window::from(i));
                }

                // Should have 3 windows in the container
                assert_eq!(container.windows().len(), 3);

                // Add container to workspace
                workspace.add_container_to_back(container);
            }

            // Add monitor to the window manager
            wm.monitors_mut().push_back(m);
        }

        // Cycle to the next window
        wm.cycle_container_window_index_in_direction(CycleDirection::Next)
            .ok();

        {
            // Should be on Window 1
            let workspace = wm.focused_workspace_mut().unwrap();
            let container = workspace.focused_container_mut().unwrap();
            assert_eq!(container.focused_window_idx(), 1);
        }

        // Cycle to the next window
        wm.cycle_container_window_index_in_direction(CycleDirection::Next)
            .ok();

        {
            // Should be on Window 2
            let workspace = wm.focused_workspace_mut().unwrap();
            let container = workspace.focused_container_mut().unwrap();
            assert_eq!(container.focused_window_idx(), 2);
        }

        // Cycle to the Previous window
        wm.cycle_container_window_index_in_direction(CycleDirection::Previous)
            .ok();

        {
            // Should be on Window 1
            let workspace = wm.focused_workspace_mut().unwrap();
            let container = workspace.focused_container_mut().unwrap();
            assert_eq!(container.focused_window_idx(), 1);
        }
    }

    #[test]
    fn test_swap_containers() {
        let (mut wm, _context) = setup_window_manager();

        {
            // Create a first monitor
            let mut m = monitor::new(
                0,
                Rect::default(),
                Rect::default(),
                "TestMonitor".to_string(),
                "TestDevice".to_string(),
                "TestDeviceID".to_string(),
                Some("TestMonitorID".to_string()),
            );

            // Create a container
            let mut container = Container::default();

            // Add three windows to the container
            for i in 0..3 {
                container.windows_mut().push_back(Window::from(i));
            }

            // Should have 3 windows in the container
            assert_eq!(container.windows().len(), 3);

            // Add the container to the workspace
            let workspace = m.focused_workspace_mut().unwrap();
            workspace.add_container_to_back(container);

            // Add monitor to the window manager
            wm.monitors_mut().push_back(m);
        }

        {
            // Create a second monitor
            let mut m = monitor::new(
                1,
                Rect::default(),
                Rect::default(),
                "TestMonitor2".to_string(),
                "TestDevice2".to_string(),
                "TestDeviceID2".to_string(),
                Some("TestMonitorID2".to_string()),
            );

            // Create a container
            let workspace = m.focused_workspace_mut().unwrap();
            let mut container = Container::default();

            // Add a window to the container
            container.windows_mut().push_back(Window::from(1));
            workspace.add_container_to_back(container);

            // Should contain 1 container
            assert_eq!(workspace.containers().len(), 1);

            wm.monitors_mut().push_back(m);
        }

        // Should contain 2 monitors
        assert_eq!(wm.monitors().len(), 2);

        // Monitor 0, Workspace 0, Window 0
        let origin = (0, 0, 0);

        // Monitor 1, Workspace 0, Window 0
        let target = (1, 0, 0);

        wm.swap_containers(origin, target).unwrap();

        {
            // Monitor 0 Workspace 0 container 0 should contain 1 container
            let workspace = wm.focused_workspace_mut().unwrap();
            let container = workspace.focused_container_mut().unwrap();
            assert_eq!(container.windows().len(), 1);
        }

        wm.focus_monitor(1).unwrap();

        {
            // Monitor 1 Workspace 0 container 0 should contain 3 containers
            let workspace = wm.focused_workspace_mut().unwrap();
            let container = workspace.focused_container_mut().unwrap();
            assert_eq!(container.windows().len(), 3);
        }
    }

    #[test]
    fn test_swap_container_with_nonexistent_container() {
        let (mut wm, _context) = setup_window_manager();

        {
            // Create a first monitor
            let mut m = monitor::new(
                0,
                Rect::default(),
                Rect::default(),
                "TestMonitor".to_string(),
                "TestDevice".to_string(),
                "TestDeviceID".to_string(),
                Some("TestMonitorID".to_string()),
            );

            // Create a container
            let mut container = Container::default();

            // Add three windows to the container
            for i in 0..3 {
                container.windows_mut().push_back(Window::from(i));
            }

            // Should have 3 windows in the container
            assert_eq!(container.windows().len(), 3);

            // Add the container to the workspace
            let workspace = m.focused_workspace_mut().unwrap();
            workspace.add_container_to_back(container);

            // Add monitor to the window manager
            wm.monitors_mut().push_back(m);
        }

        // Monitor 0, Workspace 0, Window 0
        let origin = (0, 0, 0);

        // Monitor 1, Workspace 0, Window 0
        let target = (0, 3, 0);

        // Should be focused on the first container
        assert_eq!(wm.focused_container_idx().unwrap(), 0);

        // Should return an error since there is only one container in the workspace
        let result = wm.swap_containers(origin, target);
        assert!(
            result.is_err(),
            "Expected an error when swapping with a non-existent container"
        );

        // Should still be focused on the first container
        assert_eq!(wm.focused_container_idx().unwrap(), 0);

        {
            // Should still have 1 container in the workspace
            let workspace = wm.focused_workspace_mut().unwrap();
            assert_eq!(workspace.containers().len(), 1);

            // Container should still have 3 windows
            let container = workspace.focused_container_mut().unwrap();
            assert_eq!(container.windows().len(), 3);
        }
    }

    #[test]
    fn test_swap_monitor_workspaces() {
        let (mut wm, _context) = setup_window_manager();

        {
            // Create a first monitor
            let mut m = monitor::new(
                0,
                Rect::default(),
                Rect::default(),
                "TestMonitor".to_string(),
                "TestDevice".to_string(),
                "TestDeviceID".to_string(),
                Some("TestMonitorID".to_string()),
            );

            // Create a container
            let mut container = Container::default();

            // Add three windows to the container
            for i in 0..3 {
                container.windows_mut().push_back(Window::from(i));
            }

            // Should have 3 windows in the container
            assert_eq!(container.windows().len(), 3);

            // Add the container to the workspace
            let workspace = m.focused_workspace_mut().unwrap();
            workspace.add_container_to_back(container);

            // Add monitor to the window manager
            wm.monitors_mut().push_back(m);
        }

        {
            // Create a second monitor
            let mut m = monitor::new(
                1,
                Rect::default(),
                Rect::default(),
                "TestMonitor2".to_string(),
                "TestDevice2".to_string(),
                "TestDeviceID2".to_string(),
                Some("TestMonitorID2".to_string()),
            );

            // Create a container
            let workspace = m.focused_workspace_mut().unwrap();
            let mut container = Container::default();

            // Add a window to the container
            container.windows_mut().push_back(Window::from(1));
            workspace.add_container_to_back(container);

            // Should contain 1 container
            assert_eq!(workspace.containers().len(), 1);

            // Add the monitor to the window manager
            wm.monitors_mut().push_back(m);
        }

        // Swap the workspaces between Monitor 0 and Monitor 1
        wm.swap_monitor_workspaces(0, 1).ok();

        {
            // The focused workspace container in Monitor 0 should contain 3 containers
            let workspace = wm.focused_workspace_mut().unwrap();
            let container = workspace.focused_container_mut().unwrap();
            assert_eq!(container.windows().len(), 1);
        }

        // Switch to Monitor 1
        wm.focus_monitor(1).unwrap();
        assert_eq!(wm.focused_monitor_idx(), 1);

        {
            // The focused workspace container in Monitor 1 should contain 3 containers
            let workspace = wm.focused_workspace_mut().unwrap();
            let container = workspace.focused_container_mut().unwrap();
            assert_eq!(container.windows().len(), 3);
        }
    }

    #[test]
    fn test_swap_workspace_with_nonexistent_monitor() {
        let (mut wm, _context) = setup_window_manager();

        {
            let mut m = monitor::new(
                0,
                Rect::default(),
                Rect::default(),
                "TestMonitor".to_string(),
                "TestDevice".to_string(),
                "TestDeviceID".to_string(),
                Some("TestMonitorID".to_string()),
            );

            // Add another workspace
            let new_workspace_index = m.new_workspace_idx();
            m.focus_workspace(new_workspace_index).unwrap();

            // Should have 2 workspaces
            assert_eq!(m.workspaces().len(), 2);

            // Add monitor to window manager
            wm.monitors_mut().push_back(m);
        }

        // Should be an error since Monitor 1 does not exist
        let result = wm.swap_monitor_workspaces(1, 0);
        assert!(
            result.is_err(),
            "Expected an error when swapping with a non-existent monitor"
        );

        {
            // Should still have 2 workspaces in Monitor 0
            let monitor = wm.monitors().front().unwrap();
            let workspaces = monitor.workspaces();
            assert_eq!(
                workspaces.len(),
                2,
                "Expected 2 workspaces after swap attempt"
            );
            assert_eq!(wm.focused_monitor_idx(), 0);
        }
    }

    #[test]
    fn test_move_workspace_to_monitor() {
        let (mut wm, _context) = setup_window_manager();

        {
            let mut m = monitor::new(
                0,
                Rect::default(),
                Rect::default(),
                "TestMonitor".to_string(),
                "TestDevice".to_string(),
                "TestDeviceID".to_string(),
                Some("TestMonitorID".to_string()),
            );

            // Add another workspace
            let new_workspace_index = m.new_workspace_idx();
            m.focus_workspace(new_workspace_index).unwrap();

            // Should have 2 workspaces
            assert_eq!(m.workspaces().len(), 2);

            // Add monitor to window manager
            wm.monitors_mut().push_back(m);
        }

        {
            let m = monitor::new(
                1,
                Rect::default(),
                Rect::default(),
                "TestMonitor2".to_string(),
                "TestDevice2".to_string(),
                "TestDeviceID2".to_string(),
                Some("TestMonitorID2".to_string()),
            );

            // Should contain 1 workspace
            assert_eq!(m.workspaces().len(), 1);

            // Add monitor to workspace
            wm.monitors_mut().push_back(m);
        }

        // Should contain 2 monitors
        assert_eq!(wm.monitors().len(), 2);

        // Move a workspace from Monitor 0 to Monitor 1
        wm.move_workspace_to_monitor(1).ok();

        {
            // Should be focused on Monitor 1
            assert_eq!(wm.focused_monitor_idx(), 1);

            // Should contain 2 workspaces
            let monitor = wm.focused_monitor_mut().unwrap();
            assert_eq!(monitor.workspaces().len(), 2);
        }

        {
            // Switch to Monitor 0
            wm.focus_monitor(0).unwrap();

            // Should contain 1 workspace
            let monitor = wm.focused_monitor_mut().unwrap();
            assert_eq!(monitor.workspaces().len(), 1);
        }
    }

    #[test]
    fn test_move_workspace_to_nonexistent_monitor() {
        let (mut wm, _context) = setup_window_manager();

        {
            let mut m = monitor::new(
                0,
                Rect::default(),
                Rect::default(),
                "TestMonitor".to_string(),
                "TestDevice".to_string(),
                "TestDeviceID".to_string(),
                Some("TestMonitorID".to_string()),
            );

            // Add another workspace
            let new_workspace_index = m.new_workspace_idx();
            m.focus_workspace(new_workspace_index).unwrap();

            // Should have 2 workspaces
            assert_eq!(m.workspaces().len(), 2);

            // Add monitor to window manager
            wm.monitors_mut().push_back(m);
        }

        // Attempt to move a workspace to a non-existent monitor
        let result = wm.move_workspace_to_monitor(1);

        // Should be an error since Monitor 1 does not exist
        assert!(
            result.is_err(),
            "Expected an error when moving to a non-existent monitor"
        );
    }

    #[test]
    fn test_toggle_tiling() {
        let (mut wm, _context) = setup_window_manager();

        {
            let mut m = monitor::new(
                0,
                Rect::default(),
                Rect::default(),
                "TestMonitor".to_string(),
                "TestDevice".to_string(),
                "TestDeviceID".to_string(),
                Some("TestMonitorID".to_string()),
            );

            // Set Workspace Layer to Tiling
            let workspace = m.focused_workspace_mut().unwrap();
            workspace.set_layer(WorkspaceLayer::Tiling);

            // Tiling state should be true
            assert!(*workspace.tile());

            // Add monitor to workspace
            wm.monitors_mut().push_back(m);
        }

        {
            // Tiling state should be false
            wm.toggle_tiling().unwrap();
            let workspace = wm.focused_workspace_mut().unwrap();
            assert!(!*workspace.tile());
        }

        {
            // Tiling state should be true
            wm.toggle_tiling().unwrap();
            let workspace = wm.focused_workspace_mut().unwrap();
            assert!(*workspace.tile());
        }
    }

    #[test]
    fn test_toggle_lock() {
        let (mut wm, _context) = setup_window_manager();

        {
            // Add monitor with default workspace to
            let mut m = monitor::new(
                0,
                Rect::default(),
                Rect::default(),
                "TestMonitor".to_string(),
                "TestDevice".to_string(),
                "TestDeviceID".to_string(),
                Some("TestMonitorID".to_string()),
            );

            let workspace = m.focused_workspace_mut().unwrap();

            // Create containers to add to the workspace
            for _ in 0..3 {
                let container = Container::default();
                workspace.add_container_to_back(container);
            }

            wm.monitors_mut().push_back(m);
        }

        {
            // Ensure container 2 is not locked
            let workspace = wm.focused_workspace_mut().unwrap();
            assert_eq!(workspace.focused_container_idx(), 2);
            assert!(!workspace.focused_container().unwrap().locked());
        }

        // Toggle lock on focused container
        wm.toggle_lock().unwrap();

        {
            // Ensure container 2 is locked
            let workspace = wm.focused_workspace_mut().unwrap();
            assert!(workspace.focused_container().unwrap().locked());
        }

        // Toggle lock on focused container
        wm.toggle_lock().unwrap();

        {
            // Ensure container 2 is not locked
            let workspace = wm.focused_workspace_mut().unwrap();
            assert!(!workspace.focused_container().unwrap().locked());
        }
    }

    #[test]
    fn test_float_window() {
        let (mut wm, _context) = setup_window_manager();

        {
            // Create a monitor
            let mut m = monitor::new(
                0,
                Rect::default(),
                Rect::default(),
                "TestMonitor".to_string(),
                "TestDevice".to_string(),
                "TestDeviceID".to_string(),
                Some("TestMonitorID".to_string()),
            );

            // Create a container
            let mut container = Container::default();

            // Add three windows to the container
            for i in 0..3 {
                container.windows_mut().push_back(Window::from(i));
            }

            // Should have 3 windows in the container
            assert_eq!(container.windows().len(), 3);

            // Add the container to the workspace
            let workspace = m.focused_workspace_mut().unwrap();
            workspace.add_container_to_back(container);

            // Add monitor to the window manager
            wm.monitors_mut().push_back(m);
        }

        // Add focused window to floating window list
        wm.float_window().ok();

        {
            let workspace = wm.focused_workspace().unwrap();
            let floating_windows = workspace.floating_windows();
            let container = workspace.focused_container().unwrap();

            // Hwnd 0 should be added to floating_windows
            assert_eq!(floating_windows[0].hwnd, 0);

            // Should have a length of 1
            assert_eq!(floating_windows.len(), 1);

            // Should have 2 windows in the container
            assert_eq!(container.windows().len(), 2);

            // Should be focused on window 1
            assert_eq!(container.focused_window(), Some(&Window { hwnd: 1 }));
        }

        // Add focused window to floating window list
        wm.float_window().ok();

        {
            let workspace = wm.focused_workspace().unwrap();
            let floating_windows = workspace.floating_windows();
            let container = workspace.focused_container().unwrap();

            // Hwnd 1 should be added to floating_windows
            assert_eq!(floating_windows[1].hwnd, 1);

            // Should have a length of 2
            assert_eq!(floating_windows.len(), 2);

            // Should have 1 window in the container
            assert_eq!(container.windows().len(), 1);

            // Should be focused on window 2
            assert_eq!(container.focused_window(), Some(&Window { hwnd: 2 }));
        }
    }

    #[test]
    fn test_maximize_and_unmaximize_window() {
        let (mut wm, _context) = setup_window_manager();

        {
            // Create a monitor
            let mut m = monitor::new(
                0,
                Rect::default(),
                Rect::default(),
                "TestMonitor".to_string(),
                "TestDevice".to_string(),
                "TestDeviceID".to_string(),
                Some("TestMonitorID".to_string()),
            );

            // Create a container
            let mut container = Container::default();

            // Add three windows to the container
            for i in 0..3 {
                container.windows_mut().push_back(Window::from(i));
            }

            // Should have 3 windows in the container
            assert_eq!(container.windows().len(), 3);

            // Add the container to the workspace
            let workspace = m.focused_workspace_mut().unwrap();
            workspace.add_container_to_back(container);

            // Add monitor to the window manager
            wm.monitors_mut().push_back(m);
        }

        {
            // No windows should be maximized
            let workspace = wm.focused_workspace().unwrap();
            let maximized_window = workspace.maximized_window();
            assert_eq!(*maximized_window, None);
        }

        // Maximize the focused window
        wm.maximize_window().ok();

        {
            // Window 0 should be maximized
            let workspace = wm.focused_workspace().unwrap();
            let maximized_window = workspace.maximized_window();
            assert_eq!(*maximized_window, Some(Window::from(0)));
        }

        wm.unmaximize_window().ok();

        {
            // No windows should be maximized
            let workspace = wm.focused_workspace().unwrap();
            let maximized_window = workspace.maximized_window();
            assert_eq!(*maximized_window, None);
        }

        // Focus container at index 1
        wm.focused_workspace_mut().unwrap().focus_container(1);

        {
            // Focus the window at index 1
            let container = wm.focused_container_mut().unwrap();
            container.focus_window(1);
        }

        // Maximize the focused window
        wm.maximize_window().ok();

        {
            // Window 2 should be maximized
            let workspace = wm.focused_workspace().unwrap();
            let maximized_window = workspace.maximized_window();
            assert_eq!(*maximized_window, Some(Window::from(2)));
        }

        wm.unmaximize_window().ok();

        {
            // No windows should be maximized
            let workspace = wm.focused_workspace().unwrap();
            let maximized_window = workspace.maximized_window();
            assert_eq!(*maximized_window, None);
        }
    }

    #[test]
    fn test_toggle_maximize() {
        let (mut wm, _context) = setup_window_manager();

        {
            // Create a monitor
            let mut m = monitor::new(
                0,
                Rect::default(),
                Rect::default(),
                "TestMonitor".to_string(),
                "TestDevice".to_string(),
                "TestDeviceID".to_string(),
                Some("TestMonitorID".to_string()),
            );

            // Create a container
            let mut container = Container::default();

            // Add three windows to the container
            for i in 0..3 {
                container.windows_mut().push_back(Window::from(i));
            }

            // Should have 3 windows in the container
            assert_eq!(container.windows().len(), 3);

            // Add the container to the workspace
            let workspace = m.focused_workspace_mut().unwrap();
            workspace.add_container_to_back(container);

            // Add monitor to the window manager
            wm.monitors_mut().push_back(m);
        }

        // Toggle maximize on
        wm.toggle_maximize().ok();

        {
            // Window 0 should be maximized
            let workspace = wm.focused_workspace().unwrap();
            let maximized_window = workspace.maximized_window();
            assert_eq!(*maximized_window, Some(Window::from(0)));
        }

        // Toggle maximize off
        wm.toggle_maximize().ok();

        {
            // No windows should be maximized
            let workspace = wm.focused_workspace().unwrap();
            let maximized_window = workspace.maximized_window();
            assert_eq!(*maximized_window, None);
        }
    }

    #[test]
    fn test_monocle_on_and_monocle_off() {
        let (mut wm, _context) = setup_window_manager();

        {
            // Create a monitor
            let mut m = monitor::new(
                0,
                Rect::default(),
                Rect::default(),
                "TestMonitor".to_string(),
                "TestDevice".to_string(),
                "TestDeviceID".to_string(),
                Some("TestMonitorID".to_string()),
            );

            // Create a container
            let mut container = Container::default();

            // Add a window to the container
            container.windows_mut().push_back(Window::from(1));

            // Should have 1 window in the container
            assert_eq!(container.windows().len(), 1);

            // Add the container to the workspace
            let workspace = m.focused_workspace_mut().unwrap();
            workspace.add_container_to_back(container);

            // Add monitor to the window manager
            wm.monitors_mut().push_back(m);
        }

        // Move container to monocle container
        wm.monocle_on().ok();

        {
            // Container should be a monocle container
            let monocle_container = wm
                .focused_workspace()
                .unwrap()
                .monocle_container()
                .as_ref()
                .unwrap();
            assert_eq!(monocle_container.windows().len(), 1);
            assert_eq!(monocle_container.windows()[0].hwnd, 1);
        }

        {
            // Should not have any containers
            let container = wm.focused_workspace().unwrap();
            assert_eq!(container.containers().len(), 0);
        }

        // Move monocle container to regular container
        wm.monocle_off().ok();

        {
            // Should have 1 container in the workspace
            let container = wm.focused_workspace().unwrap();
            assert_eq!(container.containers().len(), 1);
            assert_eq!(container.containers()[0].windows()[0].hwnd, 1);
        }

        {
            // No windows should be in the monocle container
            let monocle_container = wm.focused_workspace().unwrap().monocle_container();
            assert_eq!(*monocle_container, None);
        }
    }

    #[test]
    fn test_toggle_monocle() {
        let (mut wm, _context) = setup_window_manager();

        {
            // Create a monitor
            let mut m = monitor::new(
                0,
                Rect::default(),
                Rect::default(),
                "TestMonitor".to_string(),
                "TestDevice".to_string(),
                "TestDeviceID".to_string(),
                Some("TestMonitorID".to_string()),
            );

            // Create a container
            let mut container = Container::default();

            // Add a window to the container
            container.windows_mut().push_back(Window::from(1));

            // Should have 1 window in the container
            assert_eq!(container.windows().len(), 1);

            // Add the container to the workspace
            let workspace = m.focused_workspace_mut().unwrap();
            workspace.add_container_to_back(container);

            // Add monitor to the window manager
            wm.monitors_mut().push_back(m);
        }

        // Toggle monocle on
        wm.toggle_monocle().ok();

        {
            // Container should be a monocle container
            let monocle_container = wm
                .focused_workspace()
                .unwrap()
                .monocle_container()
                .as_ref()
                .unwrap();
            assert_eq!(monocle_container.windows().len(), 1);
            assert_eq!(monocle_container.windows()[0].hwnd, 1);
        }

        {
            // Should not have any containers
            let container = wm.focused_workspace().unwrap();
            assert_eq!(container.containers().len(), 0);
        }

        // Toggle monocle off
        wm.toggle_monocle().ok();

        {
            // Should have 1 container in the workspace
            let container = wm.focused_workspace().unwrap();
            assert_eq!(container.containers().len(), 1);
            assert_eq!(container.containers()[0].windows()[0].hwnd, 1);
        }

        {
            // No windows should be in the monocle container
            let monocle_container = wm.focused_workspace().unwrap().monocle_container();
            assert_eq!(*monocle_container, None);
        }
    }

    #[test]
    fn test_ensure_named_workspace_for_monitor() {
        let (mut wm, _context) = setup_window_manager();

        {
            // Create a monitor
            let m = monitor::new(
                0,
                Rect::default(),
                Rect::default(),
                "TestMonitor".to_string(),
                "TestDevice".to_string(),
                "TestDeviceID".to_string(),
                Some("TestMonitorID".to_string()),
            );

            // Add the monitor to the window manager
            wm.monitors_mut().push_back(m);
        }

        {
            // Create a monitor
            let m = monitor::new(
                1,
                Rect::default(),
                Rect::default(),
                "TestMonitor1".to_string(),
                "TestDevice1".to_string(),
                "TestDeviceID1".to_string(),
                Some("TestMonitorID1".to_string()),
            );

            // Add the monitor to the window manager
            wm.monitors_mut().push_back(m);
        }

        // Workspace names list
        let mut workspace_names = vec!["Workspace".to_string(), "Workspace1".to_string()];

        // Ensure workspaces for monitor 1
        wm.ensure_named_workspaces_for_monitor(1, &workspace_names)
            .ok();

        {
            // Monitor 1 should have 2 workspaces with names "Workspace" and "Workspace1"
            let monitor = wm.monitors().get(1).unwrap();
            let workspaces = monitor.workspaces();
            assert_eq!(workspaces.len(), workspace_names.len());
            for (i, workspace) in workspaces.iter().enumerate() {
                assert_eq!(workspace.name(), &Some(workspace_names[i].clone()));
            }
        }

        // Add more workspaces to list
        workspace_names.push("Workspace2".to_string());
        workspace_names.push("Workspace3".to_string());

        // Ensure workspaces for monitor 0
        wm.ensure_named_workspaces_for_monitor(0, &workspace_names)
            .ok();

        {
            // Monitor 0 should have 4 workspaces with names "Workspace", "Workspace1",
            // "Workspace2" and "Workspace3"
            let monitor = wm.monitors().front().unwrap();
            let workspaces = monitor.workspaces();
            assert_eq!(workspaces.len(), workspace_names.len());
            for (i, workspace) in workspaces.iter().enumerate() {
                assert_eq!(workspace.name(), &Some(workspace_names[i].clone()));
            }
        }
    }

    #[test]
    fn test_add_window_handle_to_move_based_on_workspace_rule() {
        let (wm, _context) = setup_window_manager();

        // Mock Data representing a window and its workspace/movement details
        let window_title = String::from("TestWindow");
        let hwnd = 12345;
        let origin_monitor_idx = 0;
        let origin_workspace_idx = 0;
        let target_monitor_idx = 2;
        let target_workspace_idx = 3;
        let floating = false;

        // Empty vector to hold workspace rule enforcement operations
        let mut to_move: Vec<EnforceWorkspaceRuleOp> = Vec::new();

        // Call the function to add a window movement operation based on workspace rules
        wm.add_window_handle_to_move_based_on_workspace_rule(
            &window_title,
            hwnd,
            origin_monitor_idx,
            origin_workspace_idx,
            target_monitor_idx,
            target_workspace_idx,
            floating,
            &mut to_move,
        );

        // Verify that the vector contains the expected operation with the correct values
        assert_eq!(to_move.len(), 1);
        let op = &to_move[0];
        assert_eq!(op.hwnd, hwnd); // 12345
        assert_eq!(op.origin_monitor_idx, origin_monitor_idx); // 0
        assert_eq!(op.origin_workspace_idx, origin_workspace_idx); // 0
        assert_eq!(op.target_monitor_idx, target_monitor_idx); // 2
        assert_eq!(op.target_workspace_idx, target_workspace_idx); // 3
        assert_eq!(op.floating, floating); // false
    }
}
