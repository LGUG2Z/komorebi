//! [`Monitor`], a connected output and the ring of workspaces it can show.
//!
//! A monitor has the bounds and the work area the backend reports for it
//! ([`MonitorInfo`](crate::backend::MonitorInfo)), and shows one of its workspaces, the focused
//! one. Before the focused workspace is tiled it is given the monitor's work area, the monitor's
//! padding (or [`DEFAULT_CONTAINER_PADDING`] and [`DEFAULT_WORKSPACE_PADDING`]) and the work area
//! offset ([`WorkspaceGlobals`]).

use crate::DEFAULT_CONTAINER_PADDING;
use crate::DEFAULT_WORKSPACE_PADDING;
use crate::container::Container;
use crate::core::Layout;
use crate::core::Rect;
use crate::ring::Ring;
use crate::workspace::Workspace;
use crate::workspace::WorkspaceGlobals;
use color_eyre::eyre;
use color_eyre::eyre::OptionExt;
use serde::Deserialize;
use serde::Serialize;
use std::collections::VecDeque;
use std::sync::atomic::Ordering;

/// A monitor's identity and bounds, as the daemon answers
/// [`SocketMessage::MonitorInformation`](crate::core::SocketMessage::MonitorInformation) with them
#[derive(Debug, Serialize, Deserialize)]
pub struct MonitorInformation {
    pub id: u32,
    pub device: String,
    pub serial_number_id: String,
    pub size: Rect,
}

impl From<&Monitor> for MonitorInformation {
    fn from(value: &Monitor) -> Self {
        Self {
            id: value.id,
            device: value.device.clone(),
            serial_number_id: value.serial_number_id.clone(),
            size: value.size,
        }
    }
}

impl_ring_elements!(Monitor, Workspace);

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
pub struct Monitor {
    pub id: u32,
    pub device: String,
    pub serial_number_id: String,
    pub workspaces: Ring<Workspace>,
    pub size: Rect,
    pub work_area_offset: Option<Rect>,
    pub work_area_size: Rect,
    pub window_based_work_area_offset: Option<Rect>,
    pub window_based_work_area_offset_limit: isize,
    pub container_padding: Option<i32>,
    pub workspace_padding: Option<i32>,
}

/// Returns a monitor with one empty workspace, the way [`Monitor::new`] makes one, from owned
/// names
pub fn new(
    id: u32,
    size: Rect,
    work_area_size: Rect,
    device: String,
    serial_number_id: String,
) -> Monitor {
    let mut workspaces = Ring::default();
    workspaces.elements_mut().push_back(Workspace::default());

    Monitor {
        id,
        device,
        serial_number_id,
        size,
        work_area_size,
        work_area_offset: None,
        window_based_work_area_offset: None,
        window_based_work_area_offset_limit: 1,
        workspaces,
        container_padding: None,
        workspace_padding: None,
    }
}

impl Monitor {
    #[allow(clippy::field_reassign_with_default)]
    pub fn new(
        id: u32,
        size: Rect,
        work_area_size: Rect,
        device: &str,
        serial_number_id: &str,
    ) -> Self {
        let mut workspaces = Ring::default();
        workspaces.elements_mut().push_back(Workspace::default());

        Self {
            id,
            device: device.to_string(),
            serial_number_id: serial_number_id.to_string(),
            workspaces,
            size,
            work_area_offset: None,
            work_area_size,
            window_based_work_area_offset: None,
            window_based_work_area_offset_limit: 1,
            container_padding: None,
            workspace_padding: None,
        }
    }

    /// Tiles the focused workspace in this monitor's work area, less `offset` if the monitor has
    /// no offset of its own
    #[tracing::instrument(skip_all)]
    pub fn update_focused_workspace(&mut self, offset: Option<Rect>) -> eyre::Result<()> {
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

    /// Updates the `globals` field of workspace with index `workspace_idx`
    pub fn update_workspace_globals(&mut self, workspace_idx: usize, offset: Option<Rect>) {
        let container_padding = self
            .container_padding
            .or(Some(DEFAULT_CONTAINER_PADDING.load(Ordering::Relaxed)));
        let workspace_padding = self
            .workspace_padding
            .or(Some(DEFAULT_WORKSPACE_PADDING.load(Ordering::Relaxed)));
        let work_area = self.work_area_size;
        let work_area_offset = self.work_area_offset.or(offset);
        let window_based_work_area_offset = self.window_based_work_area_offset;
        let window_based_work_area_offset_limit = self.window_based_work_area_offset_limit;

        if let Some(workspace) = self.workspaces_mut().get_mut(workspace_idx) {
            workspace.globals = WorkspaceGlobals {
                container_padding,
                workspace_padding,
                work_area,
                work_area_offset,
                window_based_work_area_offset,
                window_based_work_area_offset_limit,
            }
        }
    }

    #[tracing::instrument(skip(self))]
    pub fn focus_workspace(&mut self, idx: usize) -> eyre::Result<()> {
        tracing::info!("focusing workspace");

        {
            let workspaces = self.workspaces_mut();

            if workspaces.get(idx).is_none() {
                workspaces.resize(idx + 1, Workspace::default());
            }
            self.workspaces.focus(idx);
        }

        Ok(())
    }

    pub fn new_workspace_idx(&self) -> usize {
        self.workspaces().len()
    }

    pub fn remove_workspace_by_idx(&mut self, idx: usize) -> Option<Workspace> {
        if idx < self.workspaces().len() {
            return self.workspaces_mut().remove(idx);
        }

        if idx == 0 {
            self.workspaces_mut().push_back(Workspace::default());
        } else {
            self.focus_workspace(idx.saturating_sub(1)).ok()?;
        };

        None
    }

    pub fn add_container(
        &mut self,
        container: Container,
        workspace_idx: Option<usize>,
    ) -> eyre::Result<()> {
        let workspace = if let Some(idx) = workspace_idx {
            self.workspaces_mut()
                .get_mut(idx)
                .ok_or_eyre(format!("there is no workspace at index {}", idx))?
        } else {
            self.focused_workspace_mut()
                .ok_or_eyre("there is no workspace")?
        };

        workspace.add_container_to_back(container);

        Ok(())
    }

    pub fn remove_workspaces(&mut self) -> VecDeque<Workspace> {
        std::mem::take(self.workspaces_mut())
    }

    pub fn ensure_workspace_count(&mut self, ensure_count: usize) {
        if self.workspaces().len() < ensure_count {
            self.workspaces_mut()
                .resize(ensure_count, Workspace::default());
        }
    }

    /// The layout of the focused workspace, if the monitor has one and its tiling is on
    pub fn focused_workspace_layout(&self) -> Option<Layout> {
        self.focused_workspace().and_then(|workspace| {
            if workspace.tile {
                Some(workspace.layout.clone())
            } else {
                None
            }
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::backend::fake::FakeBackend;
    use crate::backend::with_backend;
    use crate::window::Window;
    use std::sync::Arc;

    #[test]
    fn test_add_container() {
        let mut m = Monitor::new(
            0,
            Rect::default(),
            Rect::default(),
            "TestDevice",
            "TestDeviceId",
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
            "TestDevice",
            "TestDeviceId",
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
            "TestDevice",
            "TestDeviceId",
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
            "TestDevice",
            "TestDeviceId",
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
            "TestDevice",
            "TestDeviceId",
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
            "TestDevice",
            "TestDeviceId",
        );

        let new_workspace_index = m.new_workspace_idx();

        // Should be the last workspace index: 1
        assert_eq!(new_workspace_index, 1);
    }

    #[test]
    fn test_ensure_workspace_count_workspace_contains_two_workspaces() {
        let mut m = Monitor::new(
            0,
            Rect::default(),
            Rect::default(),
            "TestDevice",
            "TestDeviceId",
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
            "TestDevice",
            "TestDeviceId",
        );

        // Ensure the monitor has at least 5 workspaces
        m.ensure_workspace_count(5);

        // Monitor should have 5 workspaces
        assert_eq!(m.workspaces().len(), 5, "Monitor should have 5 workspaces");

        // Try to call the ensure workspace count again to ensure it doesn't change
        m.ensure_workspace_count(3);
        assert_eq!(m.workspaces().len(), 5, "Monitor should have 5 workspaces");
    }

    fn rect(left: i32, top: i32, right: i32, bottom: i32) -> Rect {
        Rect {
            left,
            top,
            right,
            bottom,
        }
    }

    /// The fake's monitor, as a [`Monitor`], with one window tiled on its focused workspace
    fn monitor_with_window(id: u32) -> (Arc<FakeBackend>, Monitor) {
        let fake = Arc::new(FakeBackend::with_one_monitor());
        fake.open_window(id, "foot", "foot", "a terminal");

        let mut m = Monitor::new(
            1,
            rect(0, 0, 1920, 1080),
            rect(0, 0, 1920, 1050),
            "Virtual-0",
            "Virtual-0",
        );
        m.focused_workspace_mut()
            .unwrap()
            .new_container_for_window(&Window::from(id))
            .unwrap();

        (fake, m)
    }

    #[test]
    fn the_focused_workspace_is_tiled_in_the_work_area_with_the_default_padding() -> eyre::Result<()>
    {
        let (fake, mut m) = monitor_with_window(501);

        with_backend(fake.clone(), || m.update_focused_workspace(None))?;

        // the work area leaves the panel alone; the default workspace padding and the default
        // container padding are 5 each
        assert_eq!(fake.rect_of(501), Some(rect(10, 10, 1900, 1030)));
        Ok(())
    }

    #[test]
    fn the_monitors_own_padding_and_offset_win_over_what_it_is_given() -> eyre::Result<()> {
        let (fake, mut m) = monitor_with_window(511);
        m.container_padding = Some(0);
        m.workspace_padding = Some(0);
        m.work_area_offset = Some(rect(100, 0, 100, 0));

        with_backend(fake.clone(), || {
            m.update_focused_workspace(Some(rect(0, 200, 0, 200)))
        })?;

        assert_eq!(fake.rect_of(511), Some(rect(100, 0, 1820, 1050)));
        Ok(())
    }
}
