use crate::border::Border;
use crate::current_virtual_desktop;
use crate::monitor::Monitor;
use crate::ring::Ring;
use crate::window_manager::WindowManager;
use crate::window_manager_event::WindowManagerEvent;
use crate::workspace::Workspace;
use crate::ALT_FOCUS_HACK;
use crate::BORDER_COLOUR_CURRENT;
use crate::BORDER_COLOUR_MONOCLE;
use crate::BORDER_COLOUR_SINGLE;
use crate::BORDER_COLOUR_STACK;
use crate::BORDER_ENABLED;
use crate::BORDER_HWND;
use crate::BORDER_OFFSET;
use crate::BORDER_OVERFLOW_IDENTIFIERS;
use crate::BORDER_WIDTH;
use crate::DATA_DIR;
use crate::DEFAULT_CONTAINER_PADDING;
use crate::DEFAULT_WORKSPACE_PADDING;
use crate::FLOAT_IDENTIFIERS;
use crate::HIDING_BEHAVIOUR;
use crate::LAYERED_WHITELIST;
use crate::MANAGE_IDENTIFIERS;
use crate::MONITOR_INDEX_PREFERENCES;
use crate::OBJECT_NAME_CHANGE_ON_LAUNCH;
use crate::TRAY_AND_MULTI_WINDOW_IDENTIFIERS;
use crate::WORKSPACE_RULES;
use color_eyre::Result;
use crossbeam_channel::Receiver;
use dirs::home_dir;
use hotwatch::notify::DebouncedEvent;
use hotwatch::Hotwatch;
use komorebi_core::config_generation::ApplicationConfigurationGenerator;
use komorebi_core::config_generation::ApplicationOptions;
use komorebi_core::config_generation::IdWithIdentifier;
use komorebi_core::ApplicationIdentifier;
use komorebi_core::DefaultLayout;
use komorebi_core::FocusFollowsMouseImplementation;
use komorebi_core::HidingBehaviour;
use komorebi_core::Layout;
use komorebi_core::MoveBehaviour;
use komorebi_core::OperationBehaviour;
use komorebi_core::Rect;
use komorebi_core::SocketMessage;
use komorebi_core::WindowContainerBehaviour;
use parking_lot::Mutex;
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
pub struct Rgb {
    /// Red
    pub r: u32,
    /// Green
    pub g: u32,
    /// Blue
    pub b: u32,
}

impl From<u32> for Rgb {
    fn from(value: u32) -> Self {
        Self {
            r: value & 0xff,
            g: value >> 8 & 0xff,
            b: value >> 16 & 0xff,
        }
    }
}

#[derive(Debug, Serialize, Deserialize, JsonSchema)]
pub struct ActiveWindowBorderColours {
    /// Border colour when the container contains a single window
    pub single: Rgb,
    /// Border colour when the container contains multiple windows
    pub stack: Rgb,
    /// Border colour when the container is in monocle mode
    pub monocle: Rgb,
}

#[derive(Debug, Serialize, Deserialize, JsonSchema)]
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
        }
    }
}

#[derive(Debug, Serialize, Deserialize, JsonSchema)]
pub struct MonitorConfig {
    /// Workspace configurations
    pub workspaces: Vec<WorkspaceConfig>,
    /// Monitor-specific work area offset (default: None)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub work_area_offset: Option<Rect>,
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
        }
    }
}

#[derive(Debug, Serialize, Deserialize, JsonSchema)]
pub struct StaticConfig {
    /// Dimensions of Windows' own invisible borders; don't set these yourself unless you are told to
    #[serde(skip_serializing_if = "Option::is_none")]
    pub invisible_borders: Option<Rect>,
    /// Delta to resize windows by (default 50)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub resize_delta: Option<i32>,
    /// Determine what happens when a new window is opened (default: Create)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub window_container_behaviour: Option<WindowContainerBehaviour>,
    /// Determine what happens when a window is moved across a monitor boundary (default: Swap)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cross_monitor_move_behaviour: Option<MoveBehaviour>,
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
    /// Width of the active window border (default: 20)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub border_width: Option<i32>,
    /// Offset of the active window border (default: None)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub border_offset: Option<Rect>,
    /// Display an active window border (default: false)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub active_window_border: Option<bool>,
    /// Active window border colours for different container types
    #[serde(skip_serializing_if = "Option::is_none")]
    pub active_window_border_colours: Option<ActiveWindowBorderColours>,
    /// Global default workspace padding (default: 10)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub default_workspace_padding: Option<i32>,
    /// Global default container padding (default: 10)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub default_container_padding: Option<i32>,
    /// Monitor and workspace configurations
    #[serde(skip_serializing_if = "Option::is_none")]
    pub monitors: Option<Vec<MonitorConfig>>,
    /// Always send the ALT key when using focus commands (default: false)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub alt_focus_hack: Option<bool>,
    /// Which Windows signal to use when hiding windows (default: minimize)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub window_hiding_behaviour: Option<HidingBehaviour>,
    /// Global work area (space used for tiling) offset (default: None)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub global_work_area_offset: Option<Rect>,
    /// Individual window floating rules
    #[serde(skip_serializing_if = "Option::is_none")]
    pub float_rules: Option<Vec<IdWithIdentifier>>,
    /// Individual window force-manage rules
    #[serde(skip_serializing_if = "Option::is_none")]
    pub manage_rules: Option<Vec<IdWithIdentifier>>,
    /// Identify border overflow applications
    #[serde(skip_serializing_if = "Option::is_none")]
    pub border_overflow_applications: Option<Vec<IdWithIdentifier>>,
    /// Identify tray and multi-window applications
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tray_and_multi_window_applications: Option<Vec<IdWithIdentifier>>,
    /// Identify applications that have the WS_EX_LAYERED extended window style
    #[serde(skip_serializing_if = "Option::is_none")]
    pub layered_applications: Option<Vec<IdWithIdentifier>>,
    /// Identify applications that send EVENT_OBJECT_NAMECHANGE on launch (very rare)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub object_name_change_applications: Option<Vec<IdWithIdentifier>>,
    /// Set monitor index preferences
    #[serde(skip_serializing_if = "Option::is_none")]
    pub monitor_index_preferences: Option<HashMap<usize, Rect>>,
}

impl From<&WindowManager> for StaticConfig {
    #[allow(clippy::too_many_lines)]
    fn from(value: &WindowManager) -> Self {
        let default_invisible_borders = Rect {
            left: 7,
            top: 0,
            right: 14,
            bottom: 7,
        };

        let mut monitors = vec![];
        for m in value.monitors() {
            monitors.push(MonitorConfig::from(m));
        }

        let mut to_remove = vec![];

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
            }
        }

        for (m_idx, w_idx, id) in to_remove {
            if let Some(monitor) = monitors.get_mut(m_idx) {
                if let Some(workspace) = monitor.workspaces.get_mut(w_idx) {
                    if let Some(rules) = &mut workspace.workspace_rules {
                        rules.retain(|r| r.id != id);
                    }

                    if let Some(rules) = &mut workspace.initial_workspace_rules {
                        rules.retain(|r| r.id != id);
                    }
                }
            }
        }

        let border_colours = if BORDER_COLOUR_SINGLE.load(Ordering::SeqCst) == 0 {
            None
        } else {
            Option::from(ActiveWindowBorderColours {
                single: Rgb::from(BORDER_COLOUR_SINGLE.load(Ordering::SeqCst)),
                stack: Rgb::from(if BORDER_COLOUR_STACK.load(Ordering::SeqCst) == 0 {
                    BORDER_COLOUR_SINGLE.load(Ordering::SeqCst)
                } else {
                    BORDER_COLOUR_STACK.load(Ordering::SeqCst)
                }),
                monocle: Rgb::from(if BORDER_COLOUR_MONOCLE.load(Ordering::SeqCst) == 0 {
                    BORDER_COLOUR_SINGLE.load(Ordering::SeqCst)
                } else {
                    BORDER_COLOUR_MONOCLE.load(Ordering::SeqCst)
                }),
            })
        };

        Self {
            invisible_borders: if value.invisible_borders == default_invisible_borders {
                None
            } else {
                Option::from(value.invisible_borders)
            },
            resize_delta: Option::from(value.resize_delta),
            window_container_behaviour: Option::from(value.window_container_behaviour),
            cross_monitor_move_behaviour: Option::from(value.cross_monitor_move_behaviour),
            unmanaged_window_operation_behaviour: Option::from(
                value.unmanaged_window_operation_behaviour,
            ),
            focus_follows_mouse: value.focus_follows_mouse,
            mouse_follows_focus: Option::from(value.mouse_follows_focus),
            app_specific_configuration_path: None,
            border_width: Option::from(BORDER_WIDTH.load(Ordering::SeqCst)),
            border_offset: *BORDER_OFFSET.lock(),
            active_window_border: Option::from(BORDER_ENABLED.load(Ordering::SeqCst)),
            active_window_border_colours: border_colours,
            default_workspace_padding: Option::from(
                DEFAULT_WORKSPACE_PADDING.load(Ordering::SeqCst),
            ),
            default_container_padding: Option::from(
                DEFAULT_CONTAINER_PADDING.load(Ordering::SeqCst),
            ),
            monitors: Option::from(monitors),
            alt_focus_hack: Option::from(ALT_FOCUS_HACK.load(Ordering::SeqCst)),
            window_hiding_behaviour: Option::from(*HIDING_BEHAVIOUR.lock()),
            global_work_area_offset: value.work_area_offset,
            float_rules: None,
            manage_rules: None,
            border_overflow_applications: None,
            tray_and_multi_window_applications: None,
            layered_applications: None,
            object_name_change_applications: None,
            monitor_index_preferences: Option::from(MONITOR_INDEX_PREFERENCES.lock().clone()),
        }
    }
}

impl StaticConfig {
    #[allow(clippy::cognitive_complexity, clippy::too_many_lines)]
    fn apply_globals(&self) -> Result<()> {
        if let Some(monitor_index_preferences) = &self.monitor_index_preferences {
            let mut preferences = MONITOR_INDEX_PREFERENCES.lock();
            *preferences = monitor_index_preferences.clone();
        }

        if let Some(behaviour) = self.window_hiding_behaviour {
            let mut window_hiding_behaviour = HIDING_BEHAVIOUR.lock();
            *window_hiding_behaviour = behaviour;
        }

        if let Some(hack) = self.alt_focus_hack {
            ALT_FOCUS_HACK.store(hack, Ordering::SeqCst);
        }

        if let Some(container) = self.default_container_padding {
            DEFAULT_CONTAINER_PADDING.store(container, Ordering::SeqCst);
        }

        if let Some(workspace) = self.default_workspace_padding {
            DEFAULT_WORKSPACE_PADDING.store(workspace, Ordering::SeqCst);
        }

        if let Some(width) = self.border_width {
            BORDER_WIDTH.store(width, Ordering::SeqCst);
        }

        if let Some(offset) = self.border_offset {
            let mut border_offset = BORDER_OFFSET.lock();
            *border_offset = Some(offset);
        }

        if let Some(colours) = &self.active_window_border_colours {
            BORDER_COLOUR_SINGLE.store(
                colours.single.r | (colours.single.g << 8) | (colours.single.b << 16),
                Ordering::SeqCst,
            );
            BORDER_COLOUR_CURRENT.store(
                colours.single.r | (colours.single.g << 8) | (colours.single.b << 16),
                Ordering::SeqCst,
            );
            BORDER_COLOUR_STACK.store(
                colours.stack.r | (colours.stack.g << 8) | (colours.stack.b << 16),
                Ordering::SeqCst,
            );
            BORDER_COLOUR_MONOCLE.store(
                colours.monocle.r | (colours.monocle.g << 8) | (colours.monocle.b << 16),
                Ordering::SeqCst,
            );
        }

        let mut float_identifiers = FLOAT_IDENTIFIERS.lock();
        let mut manage_identifiers = MANAGE_IDENTIFIERS.lock();
        let mut tray_and_multi_window_identifiers = TRAY_AND_MULTI_WINDOW_IDENTIFIERS.lock();
        let mut border_overflow_identifiers = BORDER_OVERFLOW_IDENTIFIERS.lock();
        let mut object_name_change_identifiers = OBJECT_NAME_CHANGE_ON_LAUNCH.lock();
        let mut layered_identifiers = LAYERED_WHITELIST.lock();

        if let Some(float) = &self.float_rules {
            for identifier in float {
                if !float_identifiers.contains(&identifier.id) {
                    float_identifiers.push(identifier.id.clone());
                }
            }
        }

        if let Some(float) = &self.manage_rules {
            for identifier in float {
                if !manage_identifiers.contains(&identifier.id) {
                    manage_identifiers.push(identifier.id.clone());
                }
            }
        }

        if let Some(identifiers) = &self.object_name_change_applications {
            for identifier in identifiers {
                if !object_name_change_identifiers.contains(&identifier.id) {
                    object_name_change_identifiers.push(identifier.id.clone());
                }
            }
        }

        if let Some(identifiers) = &self.layered_applications {
            for identifier in identifiers {
                if !layered_identifiers.contains(&identifier.id) {
                    layered_identifiers.push(identifier.id.clone());
                }
            }
        }

        if let Some(identifiers) = &self.border_overflow_applications {
            for identifier in identifiers {
                if !border_overflow_identifiers.contains(&identifier.id) {
                    border_overflow_identifiers.push(identifier.id.clone());
                }
            }
        }

        if let Some(identifiers) = &self.tray_and_multi_window_applications {
            for identifier in identifiers {
                if !tray_and_multi_window_identifiers.contains(&identifier.id) {
                    tray_and_multi_window_identifiers.push(identifier.id.clone());
                }
            }
        }

        if let Some(path) = &self.app_specific_configuration_path {
            let stringified = path.to_string_lossy();
            let stringified = stringified.replace(
                "$Env:USERPROFILE",
                &home_dir().expect("no home dir").to_string_lossy(),
            );

            let content = std::fs::read_to_string(stringified)?;
            let asc = ApplicationConfigurationGenerator::load(&content)?;

            for entry in asc {
                if let Some(float) = entry.float_identifiers {
                    for f in float {
                        if !float_identifiers.contains(&f.id) {
                            float_identifiers.push(f.id.clone());
                        }
                    }
                }
                if let Some(options) = entry.options {
                    for o in options {
                        match o {
                            ApplicationOptions::ObjectNameChange => {
                                if !object_name_change_identifiers.contains(&entry.identifier.id) {
                                    object_name_change_identifiers
                                        .push(entry.identifier.id.clone());
                                }
                            }
                            ApplicationOptions::Layered => {
                                if !layered_identifiers.contains(&entry.identifier.id) {
                                    layered_identifiers.push(entry.identifier.id.clone());
                                }
                            }
                            ApplicationOptions::BorderOverflow => {
                                if !border_overflow_identifiers.contains(&entry.identifier.id) {
                                    border_overflow_identifiers.push(entry.identifier.id.clone());
                                }
                            }
                            ApplicationOptions::TrayAndMultiWindow => {
                                if !tray_and_multi_window_identifiers.contains(&entry.identifier.id)
                                {
                                    tray_and_multi_window_identifiers
                                        .push(entry.identifier.id.clone());
                                }
                            }
                            ApplicationOptions::Force => {
                                if !manage_identifiers.contains(&entry.identifier.id) {
                                    manage_identifiers.push(entry.identifier.id.clone());
                                }
                            }
                        }
                    }
                }
            }
        }

        Ok(())
    }

    #[allow(clippy::too_many_lines)]
    pub fn preload(
        path: &PathBuf,
        incoming: Arc<Mutex<Receiver<WindowManagerEvent>>>,
    ) -> Result<WindowManager> {
        let content = std::fs::read_to_string(path)?;
        let value: Self = serde_json::from_str(&content)?;
        value.apply_globals()?;

        let socket = DATA_DIR.join("komorebi.sock");

        match std::fs::remove_file(&socket) {
            Ok(_) => {}
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
            monitor_cache: HashMap::new(),
            incoming_events: incoming,
            command_listener: listener,
            is_paused: false,
            invisible_borders: value.invisible_borders.unwrap_or(Rect {
                left: 7,
                top: 0,
                right: 14,
                bottom: 7,
            }),
            virtual_desktop_id: current_virtual_desktop(),
            work_area_offset: value.global_work_area_offset,
            window_container_behaviour: value
                .window_container_behaviour
                .unwrap_or(WindowContainerBehaviour::Create),
            cross_monitor_move_behaviour: value
                .cross_monitor_move_behaviour
                .unwrap_or(MoveBehaviour::Swap),
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

        let bytes = SocketMessage::ReloadStaticConfiguration(path.clone()).as_bytes()?;

        wm.hotwatch.watch(path, move |event| match event {
            // Editing in Notepad sends a NoticeWrite while editing in (Neo)Vim sends
            // a NoticeRemove, presumably because of the use of swap files?
            DebouncedEvent::NoticeWrite(_) | DebouncedEvent::NoticeRemove(_) => {
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
                if let Some(m) = wm.monitors_mut().get_mut(i) {
                    m.ensure_workspace_count(monitor.workspaces.len());
                    m.set_work_area_offset(monitor.work_area_offset);

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

        if value.active_window_border == Some(true) {
            if BORDER_HWND.load(Ordering::SeqCst) == 0 {
                Border::create("komorebi-border-window")?;
            }

            BORDER_ENABLED.store(true, Ordering::SeqCst);
            wm.show_border()?;
        }

        Ok(())
    }

    pub fn reload(path: &PathBuf, wm: &mut WindowManager) -> Result<()> {
        let content = std::fs::read_to_string(path)?;
        let value: Self = serde_json::from_str(&content)?;

        value.apply_globals()?;

        if let Some(monitors) = value.monitors {
            for (i, monitor) in monitors.iter().enumerate() {
                if let Some(m) = wm.monitors_mut().get_mut(i) {
                    m.ensure_workspace_count(monitor.workspaces.len());
                    m.set_work_area_offset(monitor.work_area_offset);

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

        if value.active_window_border == Some(true) {
            if BORDER_HWND.load(Ordering::SeqCst) == 0 {
                Border::create("komorebi-border-window")?;
            }

            BORDER_ENABLED.store(true, Ordering::SeqCst);
            wm.show_border()?;
        } else {
            BORDER_ENABLED.store(false, Ordering::SeqCst);
            wm.hide_border()?;
        }

        if let Some(val) = value.invisible_borders {
            wm.invisible_borders = val;
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
        wm.focus_follows_mouse = value.focus_follows_mouse;

        Ok(())
    }
}
