//! [`Monitor`], a connected output and the ring of workspaces it can show.
//!
//! A monitor has the bounds and the work area the backend reports for it
//! ([`MonitorInfo`](crate::backend::MonitorInfo)), and shows one of its workspaces, the focused
//! one. Before the focused workspace is tiled it is given the monitor's work area, the monitor's
//! padding (or [`DEFAULT_CONTAINER_PADDING`] and [`DEFAULT_WORKSPACE_PADDING`]), the work area
//! offset and, while the compositor draws borders, the room they take ([`WorkspaceGlobals`]).
//!
//! Workspace `i` of a monitor is the compositor's virtual desktop `i` on that monitor. Showing
//! another workspace ([`Monitor::load_focused_workspace`]) puts every window on the desktop of the
//! workspace it is in, asks the compositor to show the focused workspace's desktop, which shows its
//! windows and hides the others, and gives the focus to the workspace's focused window.

use crate::workspace::WorkspaceExt;
use crate::DEFAULT_CONTAINER_PADDING;
use crate::DEFAULT_WORKSPACE_PADDING;
use crate::backend::backend;
use crate::border_manager::BORDER_DRAWN;
use crate::border_manager::BORDER_OFFSET;
use crate::border_manager::BORDER_WIDTH;
use crate::container::Container;
use crate::core::DefaultLayout;
use crate::core::Layout;
use crate::core::OperationDirection;
use crate::core::Rect;
use crate::ring::Ring;
use crate::workspace::Workspace;
use crate::workspace::KdePlatform;
use crate::workspace::WorkspaceGlobals;
use crate::workspace::WorkspaceLayer;
use color_eyre::eyre;
use color_eyre::eyre::OptionExt;
use komorebi_backend::MonitorId;
use komorebi_backend::WindowId;
use serde::Deserialize;
use serde::Serialize;
use std::sync::atomic::Ordering;

/// The monitor over KDE's [`Window`], which is `komorebi_core::Monitor` with this platform's methods
/// added by [`MonitorExt`]
pub type Monitor = komorebi_core::Monitor<KdePlatform>;

impl komorebi_core::MonitorPlatform for KdePlatform {
    type Id = u32;
    type SerialNumber = String;
    type Extra = komorebi_core::NoExtra;
    #[cfg(feature = "schemars")]
    type IdSchema = u32;

    fn serialize_id<S: serde::Serializer>(id: &u32, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_u32(*id)
    }

    fn deserialize_id<'de, D: serde::Deserializer<'de>>(deserializer: D) -> Result<u32, D::Error> {
        u32::deserialize(deserializer)
    }
}

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
            extra: Default::default(),
        id,
        device,
        serial_number_id,
        size,
        work_area_size,
        work_area_offset: None,
        window_based_work_area_offset: None,
        window_based_work_area_offset_limit: 1,
        workspaces,
        last_focused_workspace: None,
        container_padding: None,
        workspace_padding: None,
        wallpaper: None,
        floating_layer_behaviour: None,
    }
}


pub trait MonitorExt {
    fn new(
        id: u32,
        size: Rect,
        work_area_size: Rect,
        device: &str,
        serial_number_id: &str,
    ) -> Self;

    /// A stand-in for a monitor the user gave an index to (`DisplayIndexPreference`) that is not
    /// connected, so that the monitors after it keep their indices while it is not there
    fn placeholder() -> Self;

    /// Tiles the focused workspace in this monitor's work area, less `offset` if the monitor has
    /// no offset of its own
    fn update_focused_workspace(&mut self, offset: Option<Rect>) -> eyre::Result<()>;

    /// Updates the `globals` field of workspace with index `workspace_idx`
    fn update_workspace_globals(&mut self, workspace_idx: usize, offset: Option<Rect>);

    /// Make this monitor show its focused workspace.
    ///
    /// The workspaces are the compositor's virtual desktops, so this is a request to switch the
    /// desktop this monitor shows, and the compositor shows the windows on it and hides the rest.
    /// What is left for komorebi is to tile the windows that came into view.
    fn load_focused_workspace(&mut self, mouse_follows_focus: bool) -> eyre::Result<()>;

    /// Put every window of every workspace of this monitor on that workspace's desktop, unless it
    /// already is. komorebi decides which workspace a window belongs to (a window sent to another
    /// workspace, a new window, a rule); this is how the compositor learns of it.
    fn assign_windows_to_workspaces(&self);

    /// Moves the focused container of the focused workspace, or the focused window when it is a
    /// floating one, to the back of workspace `target_workspace_idx`, creating the workspace (and
    /// the ones before it) if it does not exist, and focuses that workspace when `follow` is set.
    ///
    /// With a `direction`, the container goes where a container moved across a boundary in that
    /// direction goes ([`Monitor::add_container_with_direction`]). A container that moves to a
    /// workspace with a monocle container first turns the monocle off there. The target workspace
    /// is on the floating layer after a floating window moved to it, and on the tiling layer after
    /// a container did.
    fn move_container_to_workspace(
        &mut self,
        target_workspace_idx: usize,
        follow: bool,
        direction: Option<OperationDirection>,
    ) -> eyre::Result<()>;

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

    fn update_workspaces_globals(&mut self, offset: Option<Rect>);

}

impl MonitorExt for Monitor {
    #[allow(clippy::field_reassign_with_default)]
    fn new(
        id: u32,
        size: Rect,
        work_area_size: Rect,
        device: &str,
        serial_number_id: &str,
    ) -> Self {
        let mut workspaces = Ring::default();
        workspaces.elements_mut().push_back(Workspace::default());

        Self {
            extra: Default::default(),
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
            last_focused_workspace: None,
            wallpaper: None,
            floating_layer_behaviour: None,
        }
    }

    fn placeholder() -> Self {
        Self {
            extra: Default::default(),
            // temp
            id: 2051316,
            device: "".to_string(),
            serial_number_id: "PLACEHOLDER".to_string(),
            size: Default::default(),
            work_area_size: Default::default(),
            work_area_offset: None,
            window_based_work_area_offset: None,
            window_based_work_area_offset_limit: 0,
            workspaces: Default::default(),
            last_focused_workspace: None,
            container_padding: None,
            workspace_padding: None,
            wallpaper: None,
            floating_layer_behaviour: None,
        }
    }

    #[tracing::instrument(skip_all)]
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

    fn update_workspace_globals(&mut self, workspace_idx: usize, offset: Option<Rect>) {
        let container_padding = self
            .container_padding
            .or(Some(DEFAULT_CONTAINER_PADDING.load(Ordering::Relaxed)));
        let workspace_padding = self
            .workspace_padding
            .or(Some(DEFAULT_WORKSPACE_PADDING.load(Ordering::Relaxed)));
        // a border the compositor draws outside the window takes room that the window leaves
        let (border_width, border_offset) = if BORDER_DRAWN.load(Ordering::SeqCst) {
            (
                BORDER_WIDTH.load(Ordering::SeqCst),
                BORDER_OFFSET.load(Ordering::SeqCst),
            )
        } else {
            (0, 0)
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

    fn load_focused_workspace(&mut self, mouse_follows_focus: bool) -> eyre::Result<()> {
        let focused_idx = self.focused_workspace_idx();
        let monitor_id = self.id;
        let monitor_wp = self.wallpaper.clone();

        // a window moved to this workspace has to be on its desktop before the desktop is shown
        // and a window of it focused, or focusing it would take the monitor back to the desktop it
        // was on
        self.assign_windows_to_workspaces();

        if let Err(error) = backend().switch_workspace(MonitorId::from_u32(monitor_id), focused_idx)
        {
            tracing::warn!("could not show workspace {focused_idx}: {error}");
        }

        if let Some(workspace) = self.workspaces_mut().get_mut(focused_idx) {
            workspace.restore(mouse_follows_focus, monitor_id, &monitor_wp)?;
        }

        Ok(())
    }

    fn assign_windows_to_workspaces(&self) {
        for (idx, workspace) in self.workspaces().iter().enumerate() {
            let windows = workspace
                .containers()
                .iter()
                .flat_map(|c| c.windows().iter())
                .chain(workspace.floating_windows().iter())
                .chain(
                    workspace
                        .monocle_container
                        .iter()
                        .flat_map(|c| c.windows().iter()),
                )
                .chain(workspace.maximized_window.iter());

            for window in windows {
                if backend()
                    .window_workspace(WindowId::from_u32(window.id))
                    .ok()
                    .flatten()
                    != Some(idx)
                    && let Err(error) =
                        backend().set_window_workspace(WindowId::from_u32(window.id), idx)
                {
                    tracing::debug!(
                        "could not put window {} on workspace {idx}: {error}",
                        window.id
                    );
                }
            }
        }
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

        let foreground_hwnd = if !cfg!(test) {
            backend()
                .foreground_window()
                .and_then(WindowId::to_u32)
                .ok_or_eyre("no foreground window")?
        } else {
            0
        };

        let floating_window_index = workspace
            .floating_windows()
            .iter()
            .position(|w| w.id == foreground_hwnd);

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
                    let _ = container.restore();
                }

                for window in target_workspace.floating_windows_mut() {
                    window.restore();
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

    fn add_container_with_direction(
        &mut self,
        container: Container,
        workspace_idx: Option<usize>,
        direction: OperationDirection,
    ) -> eyre::Result<()> {
        let workspace = if let Some(idx) = workspace_idx {
            self.workspaces_mut()
                .get_mut(idx)
                .ok_or_eyre(format!("there is no workspace at index {}", idx))?
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

    fn update_workspaces_globals(&mut self, offset: Option<Rect>) {
        let container_padding = self
            .container_padding
            .or(Some(DEFAULT_CONTAINER_PADDING.load(Ordering::SeqCst)));
        let workspace_padding = self
            .workspace_padding
            .or(Some(DEFAULT_WORKSPACE_PADDING.load(Ordering::SeqCst)));
        let (border_width, border_offset) = (0, 0);
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
    fn test_move_container_to_workspace() {
        let mut m = Monitor::new(
            0,
            Rect::default(),
            Rect::default(),
            "TestDevice",
            "TestDeviceId",
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
            "TestDevice",
            "TestDeviceId",
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
        let _ = m.focused_workspace_mut()
            .unwrap()
            .new_container_for_window(Window::from(id));

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

    /// The monitor object of a real `komorebic state` capture (e2e/lib/tests/fixtures/kfde-tiled.json,
    /// workspaces emptied): the JSON a Monitor reads and writes. Pins the representation of `id` (a
    /// plain number) and `serial_number_id` (a string) so that moving the type cannot change the
    /// state JSON.
    const MONITOR_JSON: &str = r#"{
        "id": 1230296105,
        "device": "Virtual-0",
        "serial_number_id": "Virtual-0",
        "workspaces": { "elements": [], "focused": 0 },
        "size": { "left": 0, "top": 0, "right": 1920, "bottom": 1080 },
        "work_area_offset": null,
        "work_area_size": { "left": 0, "top": 0, "right": 1920, "bottom": 1050 },
        "window_based_work_area_offset": null,
        "window_based_work_area_offset_limit": 1,
        "container_padding": null,
        "workspace_padding": null,
        "last_focused_workspace": 0,
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
    fn monitor_information_has_the_four_documented_fields() {
        let monitor: Monitor = serde_json::from_str(MONITOR_JSON).unwrap();

        assert_eq!(
            serde_json::to_value(MonitorInformation::from(&monitor)).unwrap(),
            serde_json::json!({
                "id": 1230296105,
                "device": "Virtual-0",
                "serial_number_id": "Virtual-0",
                "size": { "left": 0, "top": 0, "right": 1920, "bottom": 1080 }
            })
        );
    }
}
