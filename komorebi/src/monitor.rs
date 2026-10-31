use crate::workspace::WorkspaceExt;
use std::collections::HashMap;
use std::sync::atomic::Ordering;

use color_eyre::eyre;
use color_eyre::eyre::OptionExt;
use color_eyre::eyre::bail;
use serde::Deserialize;
use serde::Serialize;

use crate::border_manager::BORDER_ENABLED;
use crate::border_manager::BORDER_OFFSET;
use crate::border_manager::BORDER_WIDTH;
use crate::core::Rect;
use komorebi_backend::MonitorId;
use komorebi_core::ContainerWindow;

use crate::DefaultLayout;
use crate::Layout;
use crate::OperationDirection;
use crate::WindowsApi;
use crate::container::Container;
use crate::ring::Ring;
use crate::workspace::WindowsPlatform;
use crate::workspace::Workspace;
use crate::workspace::WorkspaceGlobals;
use crate::workspace::WorkspaceLayer;
use serde::Deserializer;
use serde::Serializer;

/// `Monitor::id` is a `MonitorId` in Rust and the same plain JSON number as the `isize` handle it
/// was before: `komorebic state` and the saved state files do not change.
mod hmonitor_json {
    use komorebi_backend::MonitorId;
    use serde::Deserialize;
    use serde::Deserializer;
    use serde::Serializer;

    pub fn serialize<S: Serializer>(id: &MonitorId, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_i64(id.to_hmonitor() as i64)
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(deserializer: D) -> Result<MonitorId, D::Error> {
        Ok(MonitorId::from_hmonitor(isize::deserialize(deserializer)?))
    }
}

/// The fields only a Windows monitor has, written flat into the monitor's JSON
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
pub struct WindowsMonitorExtra {
    /// The name of the display (`DISPLAY1`)
    pub name: String,
    /// The device id of the display
    pub device_id: String,
    /// The latest known name of each workspace, by index. A flattened map is read through a buffer
    /// that keeps its keys as strings, so they are parsed here.
    #[serde(deserialize_with = "workspace_names_from_json")]
    pub workspace_names: HashMap<usize, String>,
}

fn workspace_names_from_json<'de, D: Deserializer<'de>>(
    deserializer: D,
) -> Result<HashMap<usize, String>, D::Error> {
    HashMap::<String, String>::deserialize(deserializer)?
        .into_iter()
        .map(|(key, name)| {
            key.parse()
                .map(|key| (key, name))
                .map_err(serde::de::Error::custom)
        })
        .collect()
}

/// The monitor over the Windows [`Window`], which is `komorebi_core::Monitor` with this platform's
/// methods added by [`MonitorExt`]
pub type Monitor = komorebi_core::Monitor<WindowsPlatform>;

impl komorebi_core::MonitorPlatform for WindowsPlatform {
    type Id = MonitorId;
    type SerialNumber = Option<String>;
    type Extra = WindowsMonitorExtra;
    #[cfg(feature = "schemars")]
    type IdSchema = isize;

    fn serialize_id<S: Serializer>(id: &MonitorId, serializer: S) -> Result<S::Ok, S::Error> {
        hmonitor_json::serialize(id, serializer)
    }

    fn deserialize_id<'de, D: Deserializer<'de>>(deserializer: D) -> Result<MonitorId, D::Error> {
        hmonitor_json::deserialize(deserializer)
    }

    fn known_workspace_name(monitor: &Monitor, idx: usize) -> Option<String> {
        monitor.extra.workspace_names.get(&idx).cloned()
    }
}
#[derive(Serialize)]
pub struct MonitorInformation {
    pub id: isize,
    pub name: String,
    pub device: String,
    pub device_id: String,
    pub serial_number_id: Option<String>,
    pub size: Rect,
}

impl From<&Monitor> for MonitorInformation {
    fn from(monitor: &Monitor) -> Self {
        Self {
            id: monitor.id.to_hmonitor(),
            name: monitor.extra.name.clone(),
            device: monitor.device.clone(),
            device_id: monitor.extra.device_id.clone(),
            serial_number_id: monitor.serial_number_id.clone(),
            size: monitor.size,
        }
    }
}

pub fn new(
    id: isize,
    size: Rect,
    work_area_size: Rect,
    name: String,
    device: String,
    device_id: String,
    serial_number_id: Option<String>,
) -> Monitor {
    let mut workspaces = Ring::default();
    workspaces.elements_mut().push_back(Workspace::default());

    Monitor {
        id: MonitorId::from_hmonitor(id),
        device,
        serial_number_id,
        size,
        work_area_size,
        work_area_offset: None,
        window_based_work_area_offset: None,
        window_based_work_area_offset_limit: 1,
        workspaces,
        last_focused_workspace: None,
        extra: WindowsMonitorExtra {
            name,
            device_id,
            workspace_names: HashMap::default(),
        },
        container_padding: None,
        workspace_padding: None,
        wallpaper: None,
        floating_layer_behaviour: None,
    }
}

pub trait MonitorExt {
    fn new(
        id: isize,
        size: Rect,
        work_area_size: Rect,
        name: String,
        device: String,
        device_id: String,
        serial_number_id: Option<String>,
    ) -> Self;

    fn placeholder() -> Self;

    fn load_focused_workspace(&mut self, mouse_follows_focus: bool) -> eyre::Result<()>;

    /// Updates the `globals` field of all workspaces
    fn update_workspaces_globals(&mut self, offset: Option<Rect>);

    /// Updates the `globals` field of workspace with index `workspace_idx`
    fn update_workspace_globals(&mut self, workspace_idx: usize, offset: Option<Rect>);

    /// Adds a container to this `Monitor` using the move direction to calculate if the container
    /// should be added in front of all containers, in the back or in place of the focused
    /// container, moving the rest along. The move direction should be from the origin monitor
    /// towards the target monitor or from the origin workspace towards the target workspace.
    fn add_container_with_direction(
        &mut self,
        container: Container,
        workspace_idx: Option<usize>,
        direction: OperationDirection,
    ) -> eyre::Result<()>;

    fn move_container_to_workspace(
        &mut self,
        target_workspace_idx: usize,
        follow: bool,
        direction: Option<OperationDirection>,
    ) -> eyre::Result<()>;

    fn update_focused_workspace(&mut self, offset: Option<Rect>) -> eyre::Result<()>;
}

impl MonitorExt for Monitor {
    fn new(
        id: isize,
        size: Rect,
        work_area_size: Rect,
        name: String,
        device: String,
        device_id: String,
        serial_number_id: Option<String>,
    ) -> Self {
        new(
            id,
            size,
            work_area_size,
            name,
            device,
            device_id,
            serial_number_id,
        )
    }

    fn placeholder() -> Self {
        Self {
            id: MonitorId(0),
            device: "".to_string(),
            serial_number_id: None,
            size: Default::default(),
            work_area_size: Default::default(),
            work_area_offset: None,
            window_based_work_area_offset: None,
            window_based_work_area_offset_limit: 0,
            workspaces: Default::default(),
            last_focused_workspace: None,
            extra: WindowsMonitorExtra {
                name: "PLACEHOLDER".to_string(),
                device_id: "".to_string(),
                workspace_names: Default::default(),
            },
            container_padding: None,
            workspace_padding: None,
            wallpaper: None,
            floating_layer_behaviour: None,
        }
    }

    fn load_focused_workspace(&mut self, mouse_follows_focus: bool) -> eyre::Result<()> {
        let focused_idx = self.focused_workspace_idx();
        let hmonitor = self.id.to_hmonitor();
        let monitor_wp = self.wallpaper.clone();
        for (i, workspace) in self.workspaces_mut().iter_mut().enumerate() {
            if i == focused_idx {
                workspace.restore(mouse_follows_focus, hmonitor, &monitor_wp)?;
            } else {
                workspace.hide(None);
            }
        }

        Ok(())
    }

    fn update_workspaces_globals(&mut self, offset: Option<Rect>) {
        let container_padding = self
            .container_padding
            .or(Some(crate::settings::container_padding()));
        let workspace_padding = self
            .workspace_padding
            .or(Some(crate::settings::workspace_padding()));
        let (border_width, border_offset) = {
            let border_enabled = BORDER_ENABLED.load(Ordering::SeqCst);
            if border_enabled {
                let border_width = BORDER_WIDTH.load(Ordering::SeqCst);
                let border_offset = BORDER_OFFSET.load(Ordering::SeqCst);
                (border_width, border_offset)
            } else {
                (0, 0)
            }
        };
        let work_area = self.work_area_size;
        let work_area_offset = self.work_area_offset.or(offset);
        let window_based_work_area_offset = self.window_based_work_area_offset;
        let window_based_work_area_offset_limit = self.window_based_work_area_offset_limit;
        let floating_layer_behaviour = self.floating_layer_behaviour;

        for workspace in self.workspaces_mut() {
            workspace.globals = WorkspaceGlobals {
                container_padding,
                workspace_padding,
                border_width,
                border_offset,
                work_area,
                work_area_offset,
                window_based_work_area_offset,
                window_based_work_area_offset_limit,
                floating_layer_behaviour,
            }
        }
    }

    fn update_workspace_globals(&mut self, workspace_idx: usize, offset: Option<Rect>) {
        let container_padding = self
            .container_padding
            .or(Some(crate::settings::container_padding()));
        let workspace_padding = self
            .workspace_padding
            .or(Some(crate::settings::workspace_padding()));
        let (border_width, border_offset) = {
            let border_enabled = BORDER_ENABLED.load(Ordering::SeqCst);
            if border_enabled {
                let border_width = BORDER_WIDTH.load(Ordering::SeqCst);
                let border_offset = BORDER_OFFSET.load(Ordering::SeqCst);
                (border_width, border_offset)
            } else {
                (0, 0)
            }
        };
        let work_area = self.work_area_size;
        let work_area_offset = self.work_area_offset.or(offset);
        let window_based_work_area_offset = self.window_based_work_area_offset;
        let window_based_work_area_offset_limit = self.window_based_work_area_offset_limit;
        let floating_layer_behaviour = self.floating_layer_behaviour;

        if let Some(workspace) = self.workspaces_mut().get_mut(workspace_idx) {
            workspace.globals = WorkspaceGlobals {
                container_padding,
                workspace_padding,
                border_width,
                border_offset,
                work_area,
                work_area_offset,
                window_based_work_area_offset,
                window_based_work_area_offset_limit,
                floating_layer_behaviour,
            }
        }
    }

    fn add_container_with_direction(
        &mut self,
        container: Container,
        workspace_idx: Option<usize>,
        direction: OperationDirection,
    ) -> eyre::Result<()> {
        let workspace = if let Some(idx) = workspace_idx {
            self.workspaces_mut()
                .get_mut(idx)
                .ok_or_eyre(format!("there is no workspace at index {idx}"))?
        } else {
            self.focused_workspace_mut()
                .ok_or_eyre("there is no workspace")?
        };

        match direction {
            OperationDirection::Left => {
                // insert the container into the workspace on the monitor at the back (or rightmost position)
                // if we are moving across a boundary to the left (back = right side of the target)
                match workspace.layout {
                    Layout::Default(layout) => match layout {
                        DefaultLayout::RightMainVerticalStack => {
                            workspace.add_container_to_front(container);
                        }
                        DefaultLayout::UltrawideVerticalStack
                            if workspace.containers().len() == 1 =>
                        {
                            workspace.insert_container_at_idx(0, container);
                        }
                        _ => {
                            workspace.add_container_to_back(container);
                        }
                    },
                    Layout::Custom(_) => {
                        workspace.add_container_to_back(container);
                    }
                }
            }
            OperationDirection::Right => {
                // insert the container into the workspace on the monitor at the front (or leftmost position)
                // if we are moving across a boundary to the right (front = left side of the target)
                match workspace.layout {
                    Layout::Default(layout) => {
                        let target_index = layout.leftmost_index(workspace.containers().len());

                        match layout {
                            DefaultLayout::RightMainVerticalStack
                            | DefaultLayout::UltrawideVerticalStack
                                if workspace.containers().len() == 1 =>
                            {
                                workspace.add_container_to_back(container);
                            }
                            _ => {
                                workspace.insert_container_at_idx(target_index, container);
                            }
                        }
                    }
                    Layout::Custom(_) => {
                        workspace.add_container_to_front(container);
                    }
                }
            }
            OperationDirection::Up | OperationDirection::Down => {
                // insert the container into the workspace on the monitor at the position
                // where the currently focused container on that workspace is
                workspace.insert_container_at_idx(workspace.focused_container_idx(), container);
            }
        };

        Ok(())
    }

    #[tracing::instrument(skip(self))]
    fn move_container_to_workspace(
        &mut self,
        target_workspace_idx: usize,
        follow: bool,
        direction: Option<OperationDirection>,
    ) -> eyre::Result<()> {
        let workspace = self
            .focused_workspace_mut()
            .ok_or_eyre("there is no workspace")?;

        if workspace.maximized_window.is_some() {
            bail!("cannot move native maximized window to another monitor or workspace");
        }

        let foreground_hwnd = WindowsApi::foreground_window()?;
        let floating_window_index = workspace
            .floating_windows()
            .iter()
            .position(|w| w.hwnd == foreground_hwnd);

        if let Some(idx) = floating_window_index {
            if let Some(window) = workspace.floating_windows_mut().remove(idx) {
                let workspaces = self.workspaces_mut();
                #[allow(clippy::option_if_let_else)]
                let target_workspace = match workspaces.get_mut(target_workspace_idx) {
                    None => {
                        workspaces.resize(target_workspace_idx + 1, Workspace::default());
                        workspaces.get_mut(target_workspace_idx).unwrap()
                    }
                    Some(workspace) => workspace,
                };

                target_workspace.floating_windows_mut().push_back(window);
                target_workspace.layer = WorkspaceLayer::Floating;
            }
        } else {
            let container = workspace
                .remove_focused_container()
                .ok_or_eyre("there is no container")?;

            let workspaces = self.workspaces_mut();

            #[allow(clippy::option_if_let_else)]
            let target_workspace = match workspaces.get_mut(target_workspace_idx) {
                None => {
                    workspaces.resize(target_workspace_idx + 1, Workspace::default());
                    workspaces.get_mut(target_workspace_idx).unwrap()
                }
                Some(workspace) => workspace,
            };

            if target_workspace.monocle_container.is_some() {
                for container in target_workspace.containers_mut() {
                    container.restore();
                }

                for window in target_workspace.floating_windows_mut() {
                    window.restore_window();
                }

                target_workspace.reintegrate_monocle_container()?;
            }

            target_workspace.layer = WorkspaceLayer::Tiling;

            if let Some(direction) = direction {
                self.add_container_with_direction(
                    container,
                    Some(target_workspace_idx),
                    direction,
                )?;
            } else {
                target_workspace.add_container_to_back(container);
            }
        }

        if follow {
            self.focus_workspace(target_workspace_idx)?;
        }

        Ok(())
    }

    fn update_focused_workspace(&mut self, offset: Option<Rect>) -> eyre::Result<()> {
        let offset = if self.work_area_offset.is_some() {
            self.work_area_offset
        } else {
            offset
        };

        let focused_workspace_idx = self.focused_workspace_idx();
        self.update_workspace_globals(focused_workspace_idx, offset);
        self.focused_workspace_mut()
            .ok_or_eyre("there is no workspace")?
            .update()?;

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_add_container() {
        let mut m = Monitor::new(
            0,
            Rect::default(),
            Rect::default(),
            "TestMonitor".to_string(),
            "TestDevice".to_string(),
            "TestDeviceID".to_string(),
            Some("TestMonitorID".to_string()),
        );

        // Add container to the default workspace
        m.add_container(Container::default(), Some(0)).unwrap();

        // Should contain a container in the current focused workspace
        let workspace = m.focused_workspace_mut().unwrap();
        assert_eq!(workspace.containers().len(), 1);
    }

    #[test]
    fn test_remove_workspace_by_idx() {
        let mut m = Monitor::new(
            0,
            Rect::default(),
            Rect::default(),
            "TestMonitor".to_string(),
            "TestDevice".to_string(),
            "TestDeviceID".to_string(),
            Some("TestMonitorID".to_string()),
        );

        let new_workspace_index = m.new_workspace_idx();
        assert_eq!(new_workspace_index, 1);

        // Create workspace 2
        m.focus_workspace(new_workspace_index).unwrap();

        // Should have 2 workspaces
        assert_eq!(m.workspaces().len(), 2);

        // Create workspace 3
        m.focus_workspace(new_workspace_index + 1).unwrap();

        // Should have 3 workspaces
        assert_eq!(m.workspaces().len(), 3);

        // Remove workspace 1
        m.remove_workspace_by_idx(1);

        // Should have only 2 workspaces
        assert_eq!(m.workspaces().len(), 2);
    }

    #[test]
    fn test_remove_workspaces() {
        let mut m = Monitor::new(
            0,
            Rect::default(),
            Rect::default(),
            "TestMonitor".to_string(),
            "TestDevice".to_string(),
            "TestDeviceID".to_string(),
            Some("TestMonitorID".to_string()),
        );

        let new_workspace_index = m.new_workspace_idx();
        assert_eq!(new_workspace_index, 1);

        // Create workspace 2
        m.focus_workspace(new_workspace_index).unwrap();

        // Should have 2 workspaces
        assert_eq!(m.workspaces().len(), 2);

        // Create workspace 3
        m.focus_workspace(new_workspace_index + 1).unwrap();

        // Should have 3 workspaces
        assert_eq!(m.workspaces().len(), 3);

        // Remove all workspaces
        m.remove_workspaces();

        // All workspaces should be removed
        assert_eq!(m.workspaces().len(), 0);
    }

    #[test]
    fn test_remove_nonexistent_workspace() {
        let mut m = Monitor::new(
            0,
            Rect::default(),
            Rect::default(),
            "TestMonitor".to_string(),
            "TestDevice".to_string(),
            "TestDeviceID".to_string(),
            Some("TestMonitorID".to_string()),
        );

        // Try to remove a workspace that doesn't exist
        let removed_workspace = m.remove_workspace_by_idx(1);

        // Should return None since there is no workspace at index 1
        assert!(removed_workspace.is_none());
    }

    #[test]
    fn test_focus_workspace() {
        let mut m = Monitor::new(
            0,
            Rect::default(),
            Rect::default(),
            "TestMonitor".to_string(),
            "TestDevice".to_string(),
            "TestDeviceID".to_string(),
            Some("TestMonitorID".to_string()),
        );

        let new_workspace_index = m.new_workspace_idx();
        assert_eq!(new_workspace_index, 1);

        // Focus workspace 2
        m.focus_workspace(new_workspace_index).unwrap();

        // Should have 2 workspaces
        assert_eq!(m.workspaces().len(), 2);

        // Should be focused on workspace 2
        assert_eq!(m.focused_workspace_idx(), 1);
    }

    #[test]
    fn test_new_workspace_idx() {
        let m = Monitor::new(
            0,
            Rect::default(),
            Rect::default(),
            "TestMonitor".to_string(),
            "TestDevice".to_string(),
            "TestDeviceID".to_string(),
            Some("TestMonitorID".to_string()),
        );

        let new_workspace_index = m.new_workspace_idx();

        // Should be the last workspace index: 1
        assert_eq!(new_workspace_index, 1);
    }

    #[test]
    fn test_move_container_to_workspace() {
        let mut m = Monitor::new(
            0,
            Rect::default(),
            Rect::default(),
            "TestMonitor".to_string(),
            "TestDevice".to_string(),
            "TestDeviceID".to_string(),
            Some("TestMonitorID".to_string()),
        );

        let new_workspace_index = m.new_workspace_idx();
        assert_eq!(new_workspace_index, 1);

        {
            // Create workspace 1 and add 3 containers
            let workspace = m.focused_workspace_mut().unwrap();
            for _ in 0..3 {
                let container = Container::default();
                workspace.add_container_to_back(container);
            }

            // Should have 3 containers in workspace 1
            assert_eq!(m.focused_workspace().unwrap().containers().len(), 3);
        }

        // Create and focus workspace 2
        m.focus_workspace(new_workspace_index).unwrap();

        // Focus workspace 1
        m.focus_workspace(0).unwrap();

        // Move container to workspace 2
        m.move_container_to_workspace(1, true, None).unwrap();

        // Should be focused on workspace 2
        assert_eq!(m.focused_workspace_idx(), 1);

        // Workspace 2 should have 1 container now
        assert_eq!(m.focused_workspace().unwrap().containers().len(), 1);

        // Move to workspace 1
        m.focus_workspace(0).unwrap();

        // Workspace 1 should have 2 containers
        assert_eq!(m.focused_workspace().unwrap().containers().len(), 2);

        // Move a another container from workspace 1 to workspace 2 without following
        m.move_container_to_workspace(1, false, None).unwrap();

        // Should have 1 container
        assert_eq!(m.focused_workspace().unwrap().containers().len(), 1);

        // Should still be focused on workspace 1
        assert_eq!(m.focused_workspace_idx(), 0);

        // Switch to workspace 2
        m.focus_workspace(1).unwrap();

        // Workspace 2 should now have 2 containers
        assert_eq!(m.focused_workspace().unwrap().containers().len(), 2);
    }

    #[test]
    fn test_move_container_to_nonexistent_workspace() {
        let mut m = Monitor::new(
            0,
            Rect::default(),
            Rect::default(),
            "TestMonitor".to_string(),
            "TestDevice".to_string(),
            "TestDeviceID".to_string(),
            Some("TestMonitorID".to_string()),
        );

        {
            // Create workspace 1 and add 3 containers
            let workspace = m.focused_workspace_mut().unwrap();
            for _ in 0..3 {
                let container = Container::default();
                workspace.add_container_to_back(container);
            }

            // Should have 3 containers in workspace 1
            assert_eq!(m.focused_workspace().unwrap().containers().len(), 3);
        }

        // Should only have 1 workspace
        assert_eq!(m.workspaces().len(), 1);

        // Try to move a container to a workspace that doesn't exist
        m.move_container_to_workspace(8, true, None).unwrap();

        // Should have 9 workspaces now
        assert_eq!(m.workspaces().len(), 9);

        // Should be focused on workspace 8
        assert_eq!(m.focused_workspace_idx(), 8);

        // Should have 1 container in workspace 8
        assert_eq!(m.focused_workspace().unwrap().containers().len(), 1);
    }

    #[test]
    fn test_ensure_workspace_count_workspace_contains_two_workspaces() {
        let mut m = Monitor::new(
            0,
            Rect::default(),
            Rect::default(),
            "TestMonitor".to_string(),
            "TestDevice".to_string(),
            "TestDeviceID".to_string(),
            Some("TestMonitorID".to_string()),
        );

        // Create and focus another workspace
        let new_workspace_index = m.new_workspace_idx();
        m.focus_workspace(new_workspace_index).unwrap();

        // Should have 2 workspaces now
        assert_eq!(m.workspaces().len(), 2, "Monitor should have 2 workspaces");

        // Ensure the monitor has at least 5 workspaces
        m.ensure_workspace_count(5);

        // Monitor should have 5 workspaces
        assert_eq!(m.workspaces().len(), 5, "Monitor should have 5 workspaces");
    }

    #[test]
    fn test_ensure_workspace_count_only_default_workspace() {
        let mut m = Monitor::new(
            0,
            Rect::default(),
            Rect::default(),
            "TestMonitor".to_string(),
            "TestDevice".to_string(),
            "TestDeviceID".to_string(),
            Some("TestMonitorID".to_string()),
        );

        // Ensure the monitor has at least 5 workspaces
        m.ensure_workspace_count(5);

        // Monitor should have 5 workspaces
        assert_eq!(m.workspaces().len(), 5, "Monitor should have 5 workspaces");

        // Try to call the ensure workspace count again to ensure it doesn't change
        m.ensure_workspace_count(3);
        assert_eq!(m.workspaces().len(), 5, "Monitor should have 5 workspaces");
    }

    /// The monitor object of a real `komorebic state` capture (e2e/lib/tests/fixtures/kfw-tiled.json,
    /// workspaces emptied): the JSON a Monitor reads and writes. Pins the representation of `id` (a
    /// plain number) and `serial_number_id` (null or a string) so that changing their Rust types
    /// cannot change the state JSON.
    const MONITOR_JSON: &str = r#"{
        "id": 65537,
        "name": "DISPLAY1",
        "device": "Default_Monitor",
        "device_id": "Default_Monitor-4&183790ca&0&UID0",
        "serial_number_id": null,
        "size": { "left": 0, "top": 0, "right": 1280, "bottom": 800 },
        "work_area_size": { "left": 0, "top": 0, "right": 1280, "bottom": 752 },
        "work_area_offset": null,
        "window_based_work_area_offset": null,
        "window_based_work_area_offset_limit": 1,
        "workspaces": { "elements": [], "focused": 0 },
        "workspace_names": {},
        "container_padding": null,
        "workspace_padding": null,
        "wallpaper": null,
        "floating_layer_behaviour": null
    }"#;

    #[test]
    fn a_monitor_serializes_to_the_json_of_the_state_capture() {
        let expected: serde_json::Value = serde_json::from_str(MONITOR_JSON).unwrap();
        let monitor: Monitor = serde_json::from_value(expected.clone()).unwrap();

        assert_eq!(serde_json::to_value(&monitor).unwrap(), expected);
    }

    #[test]
    fn a_monitor_id_and_serial_are_plain_json_values() {
        for id in [0, 1, 65537, isize::MAX, -1, isize::MIN] {
            let mut value: serde_json::Value = serde_json::from_str(MONITOR_JSON).unwrap();
            value["id"] = serde_json::json!(id);
            value["serial_number_id"] = serde_json::json!("ABC123");

            let monitor: Monitor = serde_json::from_value(value.clone()).unwrap();
            assert_eq!(serde_json::to_value(&monitor).unwrap(), value, "id {id}");
        }
    }

    #[test]
    fn monitor_information_has_the_documented_fields() {
        let monitor: Monitor = serde_json::from_str(MONITOR_JSON).unwrap();
        let value = serde_json::to_value(MonitorInformation::from(&monitor)).unwrap();

        assert_eq!(
            value,
            serde_json::json!({
                "id": 65537,
                "name": "DISPLAY1",
                "device": "Default_Monitor",
                "device_id": "Default_Monitor-4&183790ca&0&UID0",
                "serial_number_id": null,
                "size": { "left": 0, "top": 0, "right": 1280, "bottom": 800 }
            })
        );
    }
}
