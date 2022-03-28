use std::collections::VecDeque;
use std::io::ErrorKind;
use std::num::NonZeroUsize;
use std::path::PathBuf;
use std::sync::Arc;
use std::thread;

use color_eyre::eyre::anyhow;
use color_eyre::Result;
use crossbeam_channel::Receiver;
use hotwatch::notify::DebouncedEvent;
use hotwatch::Hotwatch;
use parking_lot::Mutex;
use schemars::JsonSchema;
use serde::Serialize;
use uds_windows::UnixListener;

use komorebi_core::custom_layout::CustomLayout;
use komorebi_core::Arrangement;
use komorebi_core::Axis;
use komorebi_core::CycleDirection;
use komorebi_core::DefaultLayout;
use komorebi_core::FocusFollowsMouseImplementation;
use komorebi_core::Layout;
use komorebi_core::OperationDirection;
use komorebi_core::Rect;
use komorebi_core::Sizing;
use komorebi_core::WindowContainerBehaviour;

use crate::container::Container;
use crate::current_virtual_desktop;
use crate::load_configuration;
use crate::monitor::Monitor;
use crate::ring::Ring;
use crate::window::Window;
use crate::window_manager_event::WindowManagerEvent;
use crate::windows_api::WindowsApi;
use crate::winevent_listener::WINEVENT_CALLBACK_CHANNEL;
use crate::workspace::Workspace;
use crate::BORDER_OVERFLOW_IDENTIFIERS;
use crate::FLOAT_IDENTIFIERS;
use crate::HOME_DIR;
use crate::LAYERED_EXE_WHITELIST;
use crate::MANAGE_IDENTIFIERS;
use crate::OBJECT_NAME_CHANGE_ON_LAUNCH;
use crate::TRAY_AND_MULTI_WINDOW_IDENTIFIERS;
use crate::WORKSPACE_RULES;

#[derive(Debug)]
pub struct WindowManager {
    pub monitors: Ring<Monitor>,
    pub incoming_events: Arc<Mutex<Receiver<WindowManagerEvent>>>,
    pub command_listener: UnixListener,
    pub is_paused: bool,
    pub invisible_borders: Rect,
    pub work_area_offset: Option<Rect>,
    pub resize_delta: i32,
    pub window_container_behaviour: WindowContainerBehaviour,
    pub focus_follows_mouse: Option<FocusFollowsMouseImplementation>,
    pub mouse_follows_focus: bool,
    pub hotwatch: Hotwatch,
    pub virtual_desktop_id: Option<Vec<u8>>,
    pub has_pending_raise_op: bool,
    pub pending_move_op: Option<(usize, usize, usize)>,
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct State {
    pub monitors: Ring<Monitor>,
    pub is_paused: bool,
    pub invisible_borders: Rect,
    pub resize_delta: i32,
    pub new_window_behaviour: WindowContainerBehaviour,
    pub work_area_offset: Option<Rect>,
    pub focus_follows_mouse: Option<FocusFollowsMouseImplementation>,
    pub mouse_follows_focus: bool,
    pub has_pending_raise_op: bool,
    pub float_identifiers: Vec<String>,
    pub manage_identifiers: Vec<String>,
    pub layered_exe_whitelist: Vec<String>,
    pub tray_and_multi_window_identifiers: Vec<String>,
    pub border_overflow_identifiers: Vec<String>,
    pub name_change_on_launch_identifiers: Vec<String>,
}

impl AsRef<Self> for WindowManager {
    fn as_ref(&self) -> &Self {
        self
    }
}

impl From<&WindowManager> for State {
    fn from(wm: &WindowManager) -> Self {
        Self {
            monitors: wm.monitors.clone(),
            is_paused: wm.is_paused,
            invisible_borders: wm.invisible_borders,
            work_area_offset: wm.work_area_offset,
            resize_delta: wm.resize_delta,
            new_window_behaviour: wm.window_container_behaviour,
            focus_follows_mouse: wm.focus_follows_mouse.clone(),
            mouse_follows_focus: wm.mouse_follows_focus,
            has_pending_raise_op: wm.has_pending_raise_op,
            float_identifiers: FLOAT_IDENTIFIERS.lock().clone(),
            manage_identifiers: MANAGE_IDENTIFIERS.lock().clone(),
            layered_exe_whitelist: LAYERED_EXE_WHITELIST.lock().clone(),
            tray_and_multi_window_identifiers: TRAY_AND_MULTI_WINDOW_IDENTIFIERS.lock().clone(),
            border_overflow_identifiers: BORDER_OVERFLOW_IDENTIFIERS.lock().clone(),
            name_change_on_launch_identifiers: OBJECT_NAME_CHANGE_ON_LAUNCH.lock().clone(),
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
    pub fn new(incoming: Arc<Mutex<Receiver<WindowManagerEvent>>>) -> Result<Self> {
        let home = HOME_DIR.clone();
        let mut socket = home;
        socket.push("komorebi.sock");
        let socket = socket.as_path();

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

        Ok(Self {
            monitors: Ring::default(),
            incoming_events: incoming,
            command_listener: listener,
            is_paused: false,
            invisible_borders: Rect {
                left: 7,
                top: 0,
                right: 14,
                bottom: 7,
            },
            virtual_desktop_id: current_virtual_desktop(),
            work_area_offset: None,
            window_container_behaviour: WindowContainerBehaviour::Create,
            resize_delta: 50,
            focus_follows_mouse: None,
            mouse_follows_focus: true,
            hotwatch: Hotwatch::new()?,
            has_pending_raise_op: false,
            pending_move_op: None,
        })
    }

    #[tracing::instrument(skip(self))]
    pub fn init(&mut self) -> Result<()> {
        tracing::info!("initialising");
        WindowsApi::load_monitor_information(&mut self.monitors)?;
        WindowsApi::load_workspace_information(&mut self.monitors)?;
        self.update_focused_workspace(false)
    }

    #[tracing::instrument]
    pub fn reload_configuration() {
        tracing::info!("reloading configuration");
        thread::spawn(|| load_configuration().expect("could not load configuration"));
    }

    #[tracing::instrument(skip(self))]
    pub fn watch_configuration(&mut self, enable: bool) -> Result<()> {
        let home = HOME_DIR.clone();

        let mut config_v1 = home.clone();
        config_v1.push("komorebi.ahk");

        let mut config_v2 = home;
        config_v2.push("komorebi.ahk2");

        if config_v1.exists() {
            self.configure_watcher(enable, config_v1)?;
        } else if config_v2.exists() {
            self.configure_watcher(enable, config_v2)?;
        }

        Ok(())
    }

    fn configure_watcher(&mut self, enable: bool, config: PathBuf) -> Result<()> {
        if config.exists() {
            if enable {
                tracing::info!(
                    "watching configuration for changes: {}",
                    config
                        .as_os_str()
                        .to_str()
                        .ok_or_else(|| anyhow!("cannot convert path to string"))?
                );
                // Always make absolutely sure that there isn't an already existing watch, because
                // hotwatch allows multiple watches to be registered for the same path
                match self.hotwatch.unwatch(config.clone()) {
                    Ok(_) => {}
                    Err(error) => match error {
                        hotwatch::Error::Notify(error) => match error {
                            hotwatch::notify::Error::WatchNotFound => {}
                            error => return Err(error.into()),
                        },
                        error @ hotwatch::Error::Io(_) => return Err(error.into()),
                    },
                }

                self.hotwatch.watch(config, |event| match event {
                    // Editing in Notepad sends a NoticeWrite while editing in (Neo)Vim sends
                    // a NoticeRemove, presumably because of the use of swap files?
                    DebouncedEvent::NoticeWrite(_) | DebouncedEvent::NoticeRemove(_) => {
                        thread::spawn(|| {
                            load_configuration().expect("could not load configuration");
                        });
                    }
                    _ => {}
                })?;
            } else {
                tracing::info!(
                    "no longer watching configuration for changes: {}",
                    config
                        .as_os_str()
                        .to_str()
                        .ok_or_else(|| anyhow!("cannot convert path to string"))?
                );

                self.hotwatch.unwatch(config)?;
            };
        }

        Ok(())
    }

    #[tracing::instrument(skip(self))]
    pub fn reconcile_monitors(&mut self) -> Result<()> {
        let valid_hmonitors = WindowsApi::valid_hmonitors()?;
        let mut invalid = vec![];

        for monitor in self.monitors_mut() {
            if !valid_hmonitors.contains(&monitor.id()) {
                let mut mark_as_invalid = true;

                // If an invalid hmonitor has at least one window in the window manager state,
                // we can attempt to update its hmonitor id in-place so that it doesn't get reaped
                if let Some(workspace) = monitor.focused_workspace() {
                    if let Some(container) = workspace.focused_container() {
                        if let Some(window) = container.focused_window() {
                            let actual_hmonitor = WindowsApi::monitor_from_window(window.hwnd());
                            if actual_hmonitor != monitor.id() {
                                monitor.set_id(actual_hmonitor);
                                mark_as_invalid = false;
                            }
                        }
                    }
                }

                if mark_as_invalid {
                    invalid.push(monitor.id());
                }
            }
        }

        // Remove any invalid monitors from our state
        self.monitors_mut().retain(|m| !invalid.contains(&m.id()));

        let invisible_borders = self.invisible_borders;
        let offset = self.work_area_offset;

        for monitor in self.monitors_mut() {
            let mut should_update = false;
            let reference = WindowsApi::monitor(monitor.id())?;
            // TODO: If this is different, force a redraw

            if reference.work_area_size() != monitor.work_area_size() {
                monitor.set_work_area_size(Rect {
                    left: reference.work_area_size().left,
                    top: reference.work_area_size().top,
                    right: reference.work_area_size().right,
                    bottom: reference.work_area_size().bottom,
                });

                should_update = true;
            }

            if reference.size() != monitor.size() {
                monitor.set_size(Rect {
                    left: reference.size().left,
                    top: reference.size().top,
                    right: reference.size().right,
                    bottom: reference.size().bottom,
                });

                should_update = true;
            }

            if should_update {
                monitor.update_focused_workspace(offset, &invisible_borders)?;
            }
        }

        // Check for and add any new monitors that may have been plugged in
        WindowsApi::load_monitor_information(&mut self.monitors)?;

        Ok(())
    }

    #[tracing::instrument(skip(self))]
    pub fn enforce_workspace_rules(&mut self) -> Result<()> {
        let mut to_move = vec![];

        let focused_monitor_idx = self.focused_monitor_idx();
        let focused_workspace_idx = self
            .monitors()
            .get(focused_monitor_idx)
            .ok_or_else(|| anyhow!("there is no monitor with that index"))?
            .focused_workspace_idx();

        let workspace_rules = WORKSPACE_RULES.lock();
        // Go through all the monitors and workspaces
        for (i, monitor) in self.monitors().iter().enumerate() {
            for (j, workspace) in monitor.workspaces().iter().enumerate() {
                // And all the visible windows (at the top of a container)
                for window in workspace.visible_windows().into_iter().flatten() {
                    // If the executable names or titles of any of those windows are in our rules map
                    if let Some((monitor_idx, workspace_idx)) = workspace_rules.get(&window.exe()?)
                    {
                        tracing::info!(
                            "{} should be on monitor {}, workspace {}",
                            window.title()?,
                            *monitor_idx,
                            *workspace_idx
                        );

                        // Create an operation outline and save it for later in the fn
                        to_move.push(EnforceWorkspaceRuleOp {
                            hwnd: window.hwnd,
                            origin_monitor_idx: i,
                            origin_workspace_idx: j,
                            target_monitor_idx: *monitor_idx,
                            target_workspace_idx: *workspace_idx,
                        });
                    } else if let Some((monitor_idx, workspace_idx)) =
                        workspace_rules.get(&window.title()?)
                    {
                        tracing::info!(
                            "{} should be on monitor {}, workspace {}",
                            window.title()?,
                            *monitor_idx,
                            *workspace_idx
                        );

                        to_move.push(EnforceWorkspaceRuleOp {
                            hwnd: window.hwnd,
                            origin_monitor_idx: i,
                            origin_workspace_idx: j,
                            target_monitor_idx: *monitor_idx,
                            target_workspace_idx: *workspace_idx,
                        });
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
            let origin_workspace = self
                .monitors_mut()
                .get_mut(op.origin_monitor_idx)
                .ok_or_else(|| anyhow!("there is no monitor with that index"))?
                .workspaces_mut()
                .get_mut(op.origin_workspace_idx)
                .ok_or_else(|| anyhow!("there is no workspace with that index"))?;

            // Hide the window we are about to remove if it is on the currently focused workspace
            if op.is_origin(focused_monitor_idx, focused_workspace_idx) {
                Window { hwnd: op.hwnd }.hide();
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

            target_workspace.new_container_for_window(Window { hwnd: op.hwnd });
        }

        // Only re-tile the focused workspace if we need to
        if should_update_focused_workspace {
            self.update_focused_workspace(false)?;
        }

        Ok(())
    }

    #[tracing::instrument(skip(self))]
    pub fn retile_all(&mut self, preserve_resize_dimensions: bool) -> Result<()> {
        let invisible_borders = self.invisible_borders;
        let offset = self.work_area_offset;

        for monitor in self.monitors_mut() {
            let work_area = *monitor.work_area_size();
            let workspace = monitor
                .focused_workspace_mut()
                .ok_or_else(|| anyhow!("there is no workspace"))?;

            // Reset any resize adjustments if we want to force a retile
            if !preserve_resize_dimensions {
                for resize in workspace.resize_dimensions_mut() {
                    *resize = None;
                }
            }

            workspace.update(&work_area, offset, &invisible_borders)?;
        }

        Ok(())
    }

    #[tracing::instrument(skip(self))]
    pub fn manage_focused_window(&mut self) -> Result<()> {
        let hwnd = WindowsApi::foreground_window()?;
        let event = WindowManagerEvent::Manage(Window { hwnd });
        Ok(WINEVENT_CALLBACK_CHANNEL.lock().0.send(event)?)
    }

    #[tracing::instrument(skip(self))]
    pub fn unmanage_focused_window(&mut self) -> Result<()> {
        let hwnd = WindowsApi::foreground_window()?;
        let event = WindowManagerEvent::Unmanage(Window { hwnd });
        Ok(WINEVENT_CALLBACK_CHANNEL.lock().0.send(event)?)
    }

    #[tracing::instrument(skip(self))]
    pub fn raise_window_at_cursor_pos(&mut self) -> Result<()> {
        let mut hwnd = WindowsApi::window_at_cursor_pos()?;

        if self.has_pending_raise_op
            || self.focused_window()?.hwnd == hwnd
            // Sometimes we need this check, because the focus may have been given by a click
            // to a non-window such as the taskbar or system tray, and komorebi doesn't know that
            // the focused window of the workspace is not actually focused by the OS at that point
            || WindowsApi::foreground_window()? == hwnd
        {
            Ok(())
        } else {
            let mut known_hwnd = false;
            for monitor in self.monitors() {
                for workspace in monitor.workspaces() {
                    if workspace.contains_window(hwnd) {
                        known_hwnd = true;
                    }
                }
            }

            // TODO: Not sure if this needs to be made configurable just yet...
            let overlay_classes = [
                // Chromium/Electron
                "Chrome_RenderWidgetHostHWND".to_string(),
                // Explorer
                "DirectUIHWND".to_string(),
                "SysTreeView32".to_string(),
                "ToolbarWindow32".to_string(),
                "NetUIHWND".to_string(),
            ];

            if !known_hwnd {
                let class = Window { hwnd }.class()?;
                // Some applications (Electron/Chromium-based, explorer) have (invisible?) overlays
                // windows that we need to look beyond to find the actual window to raise
                if overlay_classes.contains(&class) {
                    for monitor in self.monitors() {
                        for workspace in monitor.workspaces() {
                            if let Some(exe_hwnd) = workspace.hwnd_from_exe(&Window { hwnd }.exe()?)
                            {
                                hwnd = exe_hwnd;
                                known_hwnd = true;
                            }
                        }
                    }
                }
            }

            if known_hwnd {
                let event = WindowManagerEvent::Raise(Window { hwnd });
                self.has_pending_raise_op = true;
                Ok(WINEVENT_CALLBACK_CHANNEL.lock().0.send(event)?)
            } else {
                tracing::debug!("not raising unknown window: {}", Window { hwnd });
                Ok(())
            }
        }
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

        Ok(())
    }

    #[tracing::instrument(skip(self))]
    pub fn update_focused_workspace(&mut self, follow_focus: bool) -> Result<()> {
        tracing::info!("updating");

        let invisible_borders = self.invisible_borders;
        let offset = self.work_area_offset;

        self.focused_monitor_mut()
            .ok_or_else(|| anyhow!("there is no monitor"))?
            .update_focused_workspace(offset, &invisible_borders)?;

        if follow_focus {
            if let Some(window) = self.focused_workspace()?.maximized_window() {
                window.focus(self.mouse_follows_focus)?;
            } else if let Some(container) = self.focused_workspace()?.monocle_container() {
                if let Some(window) = container.focused_window() {
                    window.focus(self.mouse_follows_focus)?;
                }
            } else if let Ok(window) = self.focused_window_mut() {
                window.focus(self.mouse_follows_focus)?;
            } else {
                let desktop_window = Window {
                    hwnd: WindowsApi::desktop_window()?,
                };

                // Calling this directly instead of the window.focus() wrapper because trying to
                // attach to the thread of the desktop window always seems to result in "Access is
                // denied (os error 5)"
                match WindowsApi::set_foreground_window(desktop_window.hwnd()) {
                    Ok(_) => {}
                    Err(error) => {
                        tracing::warn!("{} {}:{}", error, file!(), line!());
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
        let work_area = self.focused_monitor_work_area()?;
        let workspace = self.focused_workspace_mut()?;

        match workspace.layout() {
            Layout::Default(layout) => {
                tracing::info!("resizing window");
                let len = NonZeroUsize::new(workspace.containers().len())
                    .ok_or_else(|| anyhow!("there must be at least one container"))?;
                let focused_idx = workspace.focused_container_idx();
                let focused_idx_resize = workspace
                    .resize_dimensions()
                    .get(focused_idx)
                    .ok_or_else(|| anyhow!("there is no resize adjustment for this container"))?;

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
                        &work_area,
                        len,
                        workspace.container_padding(),
                        workspace.layout_flip(),
                        &[],
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
                        self.update_focused_workspace(false)
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
        Ok(())
    }

    #[tracing::instrument(skip(self))]
    pub fn restore_all_windows(&mut self) {
        tracing::info!("restoring all hidden windows");

        for monitor in self.monitors_mut() {
            for workspace in monitor.workspaces_mut() {
                for containers in workspace.containers_mut() {
                    for window in containers.windows_mut() {
                        window.restore();
                    }
                }
            }
        }
    }

    #[tracing::instrument(skip(self))]
    pub fn move_container_to_monitor(
        &mut self,
        monitor_idx: usize,
        workspace_idx: Option<usize>,
        follow: bool,
    ) -> Result<()> {
        tracing::info!("moving container");

        let invisible_borders = self.invisible_borders;
        let offset = self.work_area_offset;
        let mouse_follows_focus = self.mouse_follows_focus;

        let monitor = self
            .focused_monitor_mut()
            .ok_or_else(|| anyhow!("there is no monitor"))?;
        let workspace = monitor
            .focused_workspace_mut()
            .ok_or_else(|| anyhow!("there is no workspace"))?;

        if workspace.maximized_window().is_some() {
            return Err(anyhow!(
                "cannot move native maximized window to another monitor or workspace"
            ));
        }

        let container = workspace
            .remove_focused_container()
            .ok_or_else(|| anyhow!("there is no container"))?;

        let target_monitor = self
            .monitors_mut()
            .get_mut(monitor_idx)
            .ok_or_else(|| anyhow!("there is no monitor"))?;

        target_monitor.add_container(container, workspace_idx)?;
        target_monitor.load_focused_workspace(mouse_follows_focus)?;
        target_monitor.update_focused_workspace(offset, &invisible_borders)?;

        if follow {
            self.focus_monitor(monitor_idx)?;
        }

        self.update_focused_workspace(self.mouse_follows_focus)
    }

    #[tracing::instrument(skip(self))]
    pub fn move_container_to_workspace(&mut self, idx: usize, follow: bool) -> Result<()> {
        tracing::info!("moving container");

        let mouse_follows_focus = self.mouse_follows_focus;
        let monitor = self
            .focused_monitor_mut()
            .ok_or_else(|| anyhow!("there is no monitor"))?;

        monitor.move_container_to_workspace(idx, follow)?;
        monitor.load_focused_workspace(mouse_follows_focus)?;

        self.update_focused_workspace(mouse_follows_focus)
    }
    pub fn remove_focused_workspace(&mut self) -> Option<Workspace> {
        let focused_monitor: &mut Monitor = self.focused_monitor_mut()?;
        let focused_workspace_idx = focused_monitor.focused_workspace_idx();
        focused_monitor.remove_workspace_by_idx(focused_workspace_idx)
    }

    #[tracing::instrument(skip(self))]
    pub fn move_workspace_to_monitor(&mut self, idx: usize) -> Result<()> {
        tracing::info!("moving workspace");
        let mouse_follows_focus = self.mouse_follows_focus;
        let workspace = self
            .remove_focused_workspace()
            .ok_or_else(|| anyhow!("there is no workspace"))?;

        {
            let target_monitor: &mut Monitor = self
                .monitors_mut()
                .get_mut(idx)
                .ok_or_else(|| anyhow!("there is no monitor"))?;

            target_monitor.workspaces_mut().push_back(workspace);
            target_monitor.focus_workspace(target_monitor.workspaces().len() - 1)?;
            target_monitor.load_focused_workspace(mouse_follows_focus)?;
        }

        self.focus_monitor(idx)?;
        self.update_focused_workspace(mouse_follows_focus)
    }

    #[tracing::instrument(skip(self))]
    pub fn focus_container_in_direction(&mut self, direction: OperationDirection) -> Result<()> {
        tracing::info!("focusing container");
        let workspace = self.focused_workspace_mut()?;

        let new_idx = workspace
            .new_idx_for_direction(direction)
            .ok_or_else(|| anyhow!("this is not a valid direction from the current position"))?;

        workspace.focus_container(new_idx);
        self.focused_window_mut()?.focus(self.mouse_follows_focus)?;

        Ok(())
    }

    #[tracing::instrument(skip(self))]
    pub fn move_container_in_direction(&mut self, direction: OperationDirection) -> Result<()> {
        tracing::info!("moving container");

        let workspace = self.focused_workspace_mut()?;

        let current_idx = workspace.focused_container_idx();
        let new_idx = workspace
            .new_idx_for_direction(direction)
            .ok_or_else(|| anyhow!("this is not a valid direction from the current position"))?;

        workspace.swap_containers(current_idx, new_idx);
        workspace.focus_container(new_idx);
        self.update_focused_workspace(self.mouse_follows_focus)
    }

    #[tracing::instrument(skip(self))]
    pub fn focus_container_in_cycle_direction(&mut self, direction: CycleDirection) -> Result<()> {
        tracing::info!("focusing container");
        let workspace = self.focused_workspace_mut()?;

        let new_idx = workspace
            .new_idx_for_cycle_direction(direction)
            .ok_or_else(|| anyhow!("this is not a valid direction from the current position"))?;

        workspace.focus_container(new_idx);
        self.focused_window_mut()?.focus(self.mouse_follows_focus)?;

        Ok(())
    }

    #[tracing::instrument(skip(self))]
    pub fn move_container_in_cycle_direction(&mut self, direction: CycleDirection) -> Result<()> {
        tracing::info!("moving container");

        let workspace = self.focused_workspace_mut()?;

        let current_idx = workspace.focused_container_idx();
        let new_idx = workspace
            .new_idx_for_cycle_direction(direction)
            .ok_or_else(|| anyhow!("this is not a valid direction from the current position"))?;

        workspace.swap_containers(current_idx, new_idx);
        workspace.focus_container(new_idx);
        self.update_focused_workspace(self.mouse_follows_focus)
    }

    #[tracing::instrument(skip(self))]
    pub fn cycle_container_window_in_direction(&mut self, direction: CycleDirection) -> Result<()> {
        tracing::info!("cycling container windows");

        let container = self.focused_container_mut()?;

        let len = NonZeroUsize::new(container.windows().len())
            .ok_or_else(|| anyhow!("there must be at least one window in a container"))?;

        if len.get() == 1 {
            return Err(anyhow!("there is only one window in this container"));
        }

        let current_idx = container.focused_window_idx();
        let next_idx = direction.next_idx(current_idx, len);

        container.focus_window(next_idx);
        container.load_focused_window();

        self.update_focused_workspace(self.mouse_follows_focus)
    }

    #[tracing::instrument(skip(self))]
    pub fn add_window_to_container(&mut self, direction: OperationDirection) -> Result<()> {
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

            let adjusted_new_index = if new_idx > current_container_idx {
                new_idx - 1
            } else {
                new_idx
            };

            workspace.move_window_to_container(adjusted_new_index)?;
            self.update_focused_workspace(self.mouse_follows_focus)?;
        }

        Ok(())
    }

    #[tracing::instrument(skip(self))]
    pub fn promote_container_to_front(&mut self) -> Result<()> {
        tracing::info!("promoting container");

        let workspace = self.focused_workspace_mut()?;
        workspace.promote_container()?;
        self.update_focused_workspace(self.mouse_follows_focus)
    }

    #[tracing::instrument(skip(self))]
    pub fn remove_window_from_container(&mut self) -> Result<()> {
        tracing::info!("removing window");

        if self.focused_container()?.windows().len() == 1 {
            return Err(anyhow!("a container must have at least one window"));
        }

        let workspace = self.focused_workspace_mut()?;

        workspace.new_container_for_focused_window()?;
        self.update_focused_workspace(self.mouse_follows_focus)
    }

    #[tracing::instrument(skip(self))]
    pub fn toggle_tiling(&mut self) -> Result<()> {
        let workspace = self.focused_workspace_mut()?;
        workspace.set_tile(!*workspace.tile());
        self.update_focused_workspace(false)
    }

    #[tracing::instrument(skip(self))]
    pub fn toggle_float(&mut self) -> Result<()> {
        let hwnd = WindowsApi::foreground_window()?;
        let workspace = self.focused_workspace_mut()?;

        let mut is_floating_window = false;

        for window in workspace.floating_windows() {
            if window.hwnd == hwnd {
                is_floating_window = true;
            }
        }

        if is_floating_window {
            self.unfloat_window()?;
        } else {
            self.float_window()?;
        }

        self.update_focused_workspace(is_floating_window)
    }

    #[tracing::instrument(skip(self))]
    pub fn float_window(&mut self) -> Result<()> {
        tracing::info!("floating window");

        let work_area = self.focused_monitor_work_area()?;
        let invisible_borders = self.invisible_borders;

        let workspace = self.focused_workspace_mut()?;
        workspace.new_floating_window()?;

        let window = workspace
            .floating_windows_mut()
            .last_mut()
            .ok_or_else(|| anyhow!("there is no floating window"))?;

        window.center(&work_area, &invisible_borders)?;
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
        let workspace = self.focused_workspace_mut()?;

        match workspace.monocle_container() {
            None => self.monocle_on()?,
            Some(_) => self.monocle_off()?,
        }

        self.update_focused_workspace(false)
    }

    #[tracing::instrument(skip(self))]
    pub fn monocle_on(&mut self) -> Result<()> {
        tracing::info!("enabling monocle");

        let workspace = self.focused_workspace_mut()?;
        workspace.new_monocle_container()
    }

    #[tracing::instrument(skip(self))]
    pub fn monocle_off(&mut self) -> Result<()> {
        tracing::info!("disabling monocle");

        let workspace = self.focused_workspace_mut()?;
        workspace.reintegrate_monocle_container()
    }

    #[tracing::instrument(skip(self))]
    pub fn toggle_maximize(&mut self) -> Result<()> {
        let workspace = self.focused_workspace_mut()?;

        match workspace.maximized_window() {
            None => self.maximize_window()?,
            Some(_) => self.unmaximize_window()?,
        }

        self.update_focused_workspace(false)
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
        tracing::info!("flipping layout");

        let workspace = self.focused_workspace_mut()?;

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

        self.update_focused_workspace(false)
    }

    #[tracing::instrument(skip(self))]
    pub fn change_workspace_layout_default(&mut self, layout: DefaultLayout) -> Result<()> {
        tracing::info!("changing layout");

        let workspace = self.focused_workspace_mut()?;

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
        self.update_focused_workspace(self.mouse_follows_focus)
    }

    #[tracing::instrument(skip(self))]
    pub fn change_workspace_custom_layout(&mut self, path: PathBuf) -> Result<()> {
        tracing::info!("changing layout");

        let layout = CustomLayout::from_path_buf(path)?;
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
        self.update_focused_workspace(self.mouse_follows_focus)
    }

    #[tracing::instrument(skip(self))]
    pub fn adjust_workspace_padding(&mut self, sizing: Sizing, adjustment: i32) -> Result<()> {
        tracing::info!("adjusting workspace padding");

        let workspace = self.focused_workspace_mut()?;

        let padding = workspace
            .workspace_padding()
            .ok_or_else(|| anyhow!("there is no workspace padding"))?;

        workspace.set_workspace_padding(Option::from(sizing.adjust_by(padding, adjustment)));

        self.update_focused_workspace(false)
    }

    #[tracing::instrument(skip(self))]
    pub fn adjust_container_padding(&mut self, sizing: Sizing, adjustment: i32) -> Result<()> {
        tracing::info!("adjusting container padding");

        let workspace = self.focused_workspace_mut()?;

        let padding = workspace
            .container_padding()
            .ok_or_else(|| anyhow!("there is no container padding"))?;

        workspace.set_container_padding(Option::from(sizing.adjust_by(padding, adjustment)));

        self.update_focused_workspace(false)
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

        self.update_focused_workspace(false)
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

        let invisible_borders = self.invisible_borders;
        let offset = self.work_area_offset;
        let focused_monitor_idx = self.focused_monitor_idx();

        let monitor = self
            .monitors_mut()
            .get_mut(monitor_idx)
            .ok_or_else(|| anyhow!("there is no monitor"))?;

        let work_area = *monitor.work_area_size();
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
            workspace.update(&work_area, offset, &invisible_borders)?;
            Ok(())
        } else {
            Ok(self.update_focused_workspace(false)?)
        }
    }

    #[tracing::instrument(skip(self))]
    pub fn add_workspace_layout_custom_rule(
        &mut self,
        monitor_idx: usize,
        workspace_idx: usize,
        at_container_count: usize,
        path: PathBuf,
    ) -> Result<()> {
        tracing::info!("setting workspace layout");

        let invisible_borders = self.invisible_borders;
        let offset = self.work_area_offset;
        let focused_monitor_idx = self.focused_monitor_idx();

        let monitor = self
            .monitors_mut()
            .get_mut(monitor_idx)
            .ok_or_else(|| anyhow!("there is no monitor"))?;

        let work_area = *monitor.work_area_size();
        let focused_workspace_idx = monitor.focused_workspace_idx();

        let workspace = monitor
            .workspaces_mut()
            .get_mut(workspace_idx)
            .ok_or_else(|| anyhow!("there is no monitor"))?;

        let layout = CustomLayout::from_path_buf(path)?;

        let rules: &mut Vec<(usize, Layout)> = workspace.layout_rules_mut();
        rules.retain(|pair| pair.0 != at_container_count);
        rules.push((at_container_count, Layout::Custom(layout)));
        rules.sort_by(|a, b| a.0.cmp(&b.0));

        // If this is the focused workspace on a non-focused screen, let's update it
        if focused_monitor_idx != monitor_idx && focused_workspace_idx == workspace_idx {
            workspace.update(&work_area, offset, &invisible_borders)?;
            Ok(())
        } else {
            Ok(self.update_focused_workspace(false)?)
        }
    }

    #[tracing::instrument(skip(self))]
    pub fn clear_workspace_layout_rules(
        &mut self,
        monitor_idx: usize,
        workspace_idx: usize,
    ) -> Result<()> {
        tracing::info!("setting workspace layout");

        let invisible_borders = self.invisible_borders;
        let offset = self.work_area_offset;
        let focused_monitor_idx = self.focused_monitor_idx();

        let monitor = self
            .monitors_mut()
            .get_mut(monitor_idx)
            .ok_or_else(|| anyhow!("there is no monitor"))?;

        let work_area = *monitor.work_area_size();
        let focused_workspace_idx = monitor.focused_workspace_idx();

        let workspace = monitor
            .workspaces_mut()
            .get_mut(workspace_idx)
            .ok_or_else(|| anyhow!("there is no monitor"))?;

        let rules: &mut Vec<(usize, Layout)> = workspace.layout_rules_mut();
        rules.clear();

        // If this is the focused workspace on a non-focused screen, let's update it
        if focused_monitor_idx != monitor_idx && focused_workspace_idx == workspace_idx {
            workspace.update(&work_area, offset, &invisible_borders)?;
            Ok(())
        } else {
            Ok(self.update_focused_workspace(false)?)
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

        let invisible_borders = self.invisible_borders;
        let offset = self.work_area_offset;
        let focused_monitor_idx = self.focused_monitor_idx();

        let monitor = self
            .monitors_mut()
            .get_mut(monitor_idx)
            .ok_or_else(|| anyhow!("there is no monitor"))?;

        let work_area = *monitor.work_area_size();
        let focused_workspace_idx = monitor.focused_workspace_idx();

        let workspace = monitor
            .workspaces_mut()
            .get_mut(workspace_idx)
            .ok_or_else(|| anyhow!("there is no monitor"))?;

        workspace.set_layout(Layout::Default(layout));

        // If this is the focused workspace on a non-focused screen, let's update it
        if focused_monitor_idx != monitor_idx && focused_workspace_idx == workspace_idx {
            workspace.update(&work_area, offset, &invisible_borders)?;
            Ok(())
        } else {
            Ok(self.update_focused_workspace(false)?)
        }
    }

    #[tracing::instrument(skip(self))]
    pub fn set_workspace_layout_custom(
        &mut self,
        monitor_idx: usize,
        workspace_idx: usize,
        path: PathBuf,
    ) -> Result<()> {
        tracing::info!("setting workspace layout");
        let layout = CustomLayout::from_path_buf(path)?;
        let invisible_borders = self.invisible_borders;
        let offset = self.work_area_offset;
        let focused_monitor_idx = self.focused_monitor_idx();

        let monitor = self
            .monitors_mut()
            .get_mut(monitor_idx)
            .ok_or_else(|| anyhow!("there is no monitor"))?;

        let work_area = *monitor.work_area_size();
        let focused_workspace_idx = monitor.focused_workspace_idx();

        let workspace = monitor
            .workspaces_mut()
            .get_mut(workspace_idx)
            .ok_or_else(|| anyhow!("there is no monitor"))?;

        workspace.set_layout(Layout::Custom(layout));

        // If this is the focused workspace on a non-focused screen, let's update it
        if focused_monitor_idx != monitor_idx && focused_workspace_idx == workspace_idx {
            workspace.update(&work_area, offset, &invisible_borders)?;
            Ok(())
        } else {
            Ok(self.update_focused_workspace(false)?)
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

        self.update_focused_workspace(false)
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

        self.update_focused_workspace(false)
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
            return Err(anyhow!("this is not a valid monitor index"));
        }

        Ok(())
    }

    pub fn monitor_idx_from_window(&mut self, window: Window) -> Option<usize> {
        let hmonitor = WindowsApi::monitor_from_window(window.hwnd());

        for (i, monitor) in self.monitors().iter().enumerate() {
            if monitor.id() == hmonitor {
                return Option::from(i);
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

        None
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

    #[tracing::instrument(skip(self))]
    pub fn focus_workspace(&mut self, idx: usize) -> Result<()> {
        tracing::info!("focusing workspace");

        let mouse_follows_focus = self.mouse_follows_focus;
        let monitor = self
            .focused_monitor_mut()
            .ok_or_else(|| anyhow!("there is no workspace"))?;

        monitor.focus_workspace(idx)?;
        monitor.load_focused_workspace(mouse_follows_focus)?;

        self.update_focused_workspace(mouse_follows_focus)
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

        self.update_focused_workspace(self.mouse_follows_focus)
    }

    pub fn focused_container(&self) -> Result<&Container> {
        self.focused_workspace()?
            .focused_container()
            .ok_or_else(|| anyhow!("there is no container"))
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
}
