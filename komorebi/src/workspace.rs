//! [`Workspace`], a ring of containers tiled by a layout.
//!
//! A workspace keeps its containers in order. [`Workspace::update`] asks the layout for one tile
//! per container in the work area it was given, less its padding, and puts every window of each
//! container on its tile with [`Window::set_position`]. The work area and the padding a workspace
//! falls back to come from the monitor it is on ([`WorkspaceGlobals`]).

use crate::container::Container;
use crate::core::CycleDirection;
use crate::core::DefaultLayout;
use crate::core::Layout;
use crate::core::OperationDirection;
use crate::core::Rect;
use crate::lockable_sequence::LockableSequence;
use crate::ring::Ring;
use crate::window::Window;
use color_eyre::eyre;
use color_eyre::eyre::OptionExt;
use serde::Deserialize;
use serde::Serialize;
use std::collections::HashMap;
use std::num::NonZeroUsize;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
pub struct Workspace {
    pub containers: Ring<Container>,
    pub layout: Layout,
    pub workspace_padding: Option<i32>,
    pub container_padding: Option<i32>,
    #[serde(skip)]
    pub globals: WorkspaceGlobals,
}

impl_ring_elements!(Workspace, Container);

impl Default for Workspace {
    fn default() -> Self {
        Self {
            containers: Default::default(),
            workspace_padding: None,
            container_padding: None,
            layout: Layout::Default(DefaultLayout::BSP),
            globals: Default::default(),
        }
    }
}

#[derive(Debug, Default, Copy, Clone, Serialize, Deserialize, PartialEq)]
/// The work area a monitor gives the workspace it shows, and the padding and the offset the
/// workspace falls back to when it sets none of its own
pub struct WorkspaceGlobals {
    pub container_padding: Option<i32>,
    pub workspace_padding: Option<i32>,
    pub work_area: Rect,
    pub work_area_offset: Option<Rect>,
}

impl Workspace {
    #[tracing::instrument(skip(self))]
    pub fn focus_container(&mut self, idx: usize) {
        tracing::info!("focusing container");

        self.containers.focus(idx);
    }

    pub fn swap_containers(&mut self, i: usize, j: usize) {
        self.containers.elements_mut().swap_respecting_locks(i, j);
        self.focus_container(j);
    }

    /// Returns the index of the container whose tile is on the `direction` side of the focused
    /// one in the workspace's layout, or `None` when there is none on that side
    pub fn new_idx_for_direction(&self, direction: OperationDirection) -> Option<usize> {
        let len = NonZeroUsize::new(self.containers().len())?;

        direction.destination(
            self.layout.as_boxed_direction().as_ref(),
            None,
            self.focused_container_idx(),
            len,
            None,
        )
    }

    /// Returns the index of the container after (or before) the focused one, wrapping around at
    /// the ends, or `None` when the workspace has no containers
    pub fn new_idx_for_cycle_direction(&self, direction: CycleDirection) -> Option<usize> {
        Option::from(direction.next_idx(
            self.focused_container_idx(),
            NonZeroUsize::new(self.containers().len())?,
        ))
    }

    pub fn container_idx_for_window(&self, window_id: u32) -> Option<usize> {
        let mut idx = None;
        for (i, x) in self.containers().iter().enumerate() {
            if x.contains_window(window_id) {
                idx = Option::from(i);
            }
        }

        idx
    }

    pub fn focus_container_by_window(&mut self, window_id: u32) -> eyre::Result<()> {
        let container_idx = self
            .container_idx_for_window(window_id)
            .ok_or_eyre("there is no container/window")?;

        let container = self
            .containers_mut()
            .get_mut(container_idx)
            .ok_or_eyre("there is no container")?;

        let window_idx = container
            .idx_for_window(window_id)
            .ok_or_eyre("there is no window")?;

        container.focus_window(window_idx);

        self.focus_container(container_idx);

        Ok(())
    }

    /// Puts the window in a container of its own, after the focused container (at the start of an
    /// empty workspace), and focuses that container
    pub fn new_container_for_window(&mut self, window: &Window) -> eyre::Result<()> {
        let next_idx = if self.containers().is_empty() {
            0
        } else {
            self.focused_container_idx() + 1
        };

        let mut container = Container::default();
        container.add_window(window)?;

        self.insert_container_at_idx(next_idx, container);

        Ok(())
    }

    // this fn respects locked container indexes - we should use it for pretty much everything
    // except monocle and maximize toggles
    pub fn insert_container_at_idx(&mut self, idx: usize, container: Container) -> usize {
        let insertion_idx = self
            .containers_mut()
            .insert_respecting_locks(idx, container);

        self.focus_container(insertion_idx);

        insertion_idx
    }

    pub fn prune_duplicate_windows(&mut self) -> eyre::Result<()> {
        let mut tracker = HashMap::new();
        let mut to_prune_from_containers = vec![];

        for container in self.containers() {
            for window in container.windows() {
                let entry = tracker.entry(window.id).or_insert(0);
                *entry += 1;
            }
        }

        for (window_id, count) in &tracker {
            if *count > 1 {
                to_prune_from_containers.push(*window_id);
            }
        }

        for id in to_prune_from_containers {
            self.remove_window(id)?;
            tracker.remove(&id);
        }

        Ok(())
    }

    /// Remove every window that no longer exists
    #[tracing::instrument(skip(self))]
    pub fn reap_invalid_windows(&mut self) -> eyre::Result<usize> {
        let mut invalid_window_ids = vec![];

        for container in self.containers() {
            for window in container.windows() {
                if !window.is_valid() {
                    invalid_window_ids.push(window.id);
                }
            }
        }

        for window_id in &invalid_window_ids {
            let _ = self.remove_window(*window_id);
        }

        tracing::debug!("reaping invalid windows {invalid_window_ids:?}");

        Ok(invalid_window_ids.len())
    }

    pub fn remove_window(&mut self, window_id: u32) -> eyre::Result<Window> {
        let container_idx = self
            .container_idx_for_window(window_id)
            .ok_or_eyre("there is no window")?;

        let container = self
            .containers_mut()
            .get_mut(container_idx)
            .ok_or_eyre("there is no container")?;

        let window_idx = container
            .windows()
            .iter()
            .position(|window| window.id == window_id)
            .ok_or_eyre("there is no window")?;

        let window = container
            .remove_window_by_idx(window_idx)
            .ok_or_eyre("there is no window")?;

        if container.windows().is_empty() {
            self.remove_container_by_idx(container_idx);
            self.focus_previous_container();
        }

        Ok(window)
    }

    // this fn respects locked container indexes - we should use it for pretty much everything
    // except monocle and maximize toggles
    pub fn remove_container_by_idx(&mut self, idx: usize) -> Option<Container> {
        self.containers_mut().remove_respecting_locks(idx)
    }

    pub fn focus_previous_container(&mut self) {
        let focused_idx = self.focused_container_idx();
        self.focus_container(focused_idx.saturating_sub(1));
    }

    pub fn is_empty(&self) -> bool {
        self.containers().is_empty()
    }

    pub fn contains_window(&self, window_id: u32) -> bool {
        for container in self.containers() {
            if container.contains_window(window_id) {
                return true;
            }
        }

        false
    }

    pub fn remove_focused_container(&mut self) -> Option<Container> {
        let focused_idx = self.focused_container_idx();
        let container = self.remove_container_by_idx(focused_idx);
        self.focus_previous_container();

        container
    }

    pub fn remove_container(&mut self, idx: usize) -> Option<Container> {
        let container = self.remove_container_by_idx(idx);
        self.focus_previous_container();

        container
    }

    pub fn add_container_to_back(&mut self, container: Container) {
        self.containers_mut().push_back(container);
        self.focus_last_container();
    }

    pub fn add_container_to_front(&mut self, container: Container) {
        self.containers_mut().push_front(container);
        self.focus_first_container();
    }

    fn focus_last_container(&mut self) {
        self.focus_container(self.containers().len().saturating_sub(1));
    }

    fn focus_first_container(&mut self) {
        self.focus_container(0);
    }
}

impl Workspace {
    /// Tiles the workspace: asks the layout for a tile per container in the work area, less the
    /// offset and the workspace padding, and puts every window of each container on its tile.
    ///
    /// Containers that have no windows left are dropped first, and so is a window that is in more
    /// than one container. A tile is clamped to the work area, so it cannot bleed onto a
    /// neighboring monitor.
    pub fn update(&mut self) -> eyre::Result<()> {
        // make sure we are never holding on to empty containers
        self.prune_duplicate_windows()?;
        let focused_container_idx = self.focused_container_idx();
        self.containers_mut().retain(|c| !c.windows().is_empty());

        // make sure we are never focused on a no longer existent final empty container
        if focused_container_idx >= self.containers().len() {
            self.focus_container(focused_container_idx.saturating_sub(1));
        }

        let container_padding = self
            .container_padding
            .or(self.globals.container_padding)
            .unwrap_or_default();
        let workspace_padding = self
            .workspace_padding
            .or(self.globals.workspace_padding)
            .unwrap_or_default();
        let work_area = self.globals.work_area;
        let work_area_offset = self.globals.work_area_offset;

        let mut adjusted_work_area = work_area_offset.map_or_else(
            || work_area,
            |offset| {
                let mut with_offset = work_area;
                with_offset.left += offset.left;
                with_offset.top += offset.top;
                with_offset.right -= offset.right;
                with_offset.bottom -= offset.bottom;

                with_offset
            },
        );

        adjusted_work_area.add_padding(workspace_padding);

        // the backend batches the commands this produces and sends them together
        let result = (|| -> eyre::Result<()> {
            if !self.containers().is_empty() {
                let mut layouts = self.layout.as_boxed_arrangement().calculate(
                    &adjusted_work_area,
                    NonZeroUsize::new(self.containers().len()).ok_or_eyre(
                        "there must be at least one container to calculate a workspace layout",
                    )?,
                    Some(container_padding),
                    None,
                    &[],
                    self.focused_container_idx(),
                    None,
                    &[],
                );

                let containers = self.containers_mut();

                for (i, container) in containers.iter_mut().enumerate() {
                    if let Some(layout) = layouts.get_mut(i) {
                        // Every tile is clamped to the work area so it cannot bleed onto an
                        // adjacent monitor.

                        // horizontal
                        if layout.left < adjusted_work_area.left {
                            layout.right -= adjusted_work_area.left - layout.left;
                            layout.left = adjusted_work_area.left;
                        }
                        let max_right_edge = adjusted_work_area.left + adjusted_work_area.right;
                        let current_right_edge = layout.left + layout.right;
                        if current_right_edge > max_right_edge {
                            layout.right = max_right_edge - layout.left;
                        }

                        // vertical
                        if layout.top < adjusted_work_area.top {
                            layout.bottom -= adjusted_work_area.top - layout.top;
                            layout.top = adjusted_work_area.top;
                        }
                        let max_bottom_edge = adjusted_work_area.top + adjusted_work_area.bottom;
                        let current_bottom_edge = layout.top + layout.bottom;
                        if current_bottom_edge > max_bottom_edge {
                            layout.bottom = max_bottom_edge - layout.top;
                        }

                        for window in container.windows_mut() {
                            if let Err(error) = window.set_position(layout) {
                                tracing::warn!("failed to set window position: {error}")
                            }
                        }
                    }
                }
            }
            Ok(())
        })();

        result?;

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::backend::fake::FakeBackend;
    use crate::backend::with_backend;
    use crate::container::Container;
    use crate::window::Window;
    use std::collections::HashMap;
    use std::sync::Arc;

    #[test]
    fn test_locked_containers_with_new_window() {
        let mut ws = Workspace::default();

        let mut state = HashMap::new();

        // add 4 containers
        for i in 0..4 {
            let mut container = Container::default();
            if i == 3 {
                container.locked = true; // set index 3 locked
            }
            state.insert(i, container.id.to_string());
            ws.add_container_to_back(container);
        }
        assert_eq!(ws.containers().len(), 4);

        // focus container at index 2
        ws.focus_container(2);

        // simulate a new window being launched on this workspace
        ws.new_container_for_window(&Window::from(123)).unwrap();

        // new length should be 5, with the focus on the new window at index 4
        assert_eq!(ws.containers().len(), 5);
        assert_eq!(ws.focused_container_idx(), 4);
        assert_eq!(
            ws.focused_container().unwrap().focused_window().unwrap().id,
            123
        );

        // when inserting a new container at index 0, index 3's container should not change
        ws.focus_container(0);
        ws.new_container_for_window(&Window::from(234)).unwrap();
        assert_eq!(
            ws.containers()[3].id.to_string(),
            state.get(&3).unwrap().to_string()
        );
    }

    #[test]
    fn test_locked_containers_remove_window() {
        let mut ws = Workspace::default();

        // add 4 containers
        for i in 0..4 {
            let mut container = Container::default();
            container.windows_mut().push_back(Window::from(i));
            if i == 1 {
                container.locked = true;
            }
            ws.add_container_to_back(container);
        }
        assert_eq!(ws.containers().len(), 4);

        ws.remove_window(0).unwrap();
        assert_eq!(ws.containers()[0].focused_window().unwrap().id, 2);
        // index 1 should still be the same
        assert_eq!(ws.containers()[1].focused_window().unwrap().id, 1);
        assert_eq!(ws.containers()[2].focused_window().unwrap().id, 3);
    }

    #[test]
    fn test_contains_window() {
        // Create default workspace
        let mut workspace = Workspace::default();

        // Add a window to the container
        let mut container = Container::default();
        container.windows_mut().push_back(Window::from(0));

        // Add container
        workspace.add_container_to_back(container);

        // Should be true
        assert!(workspace.contains_window(0));

        // Should be false
        assert!(!workspace.is_empty())
    }

    #[test]
    fn test_add_container_to_back() {
        let mut workspace = Workspace::default();

        {
            // Container with 3 windows
            let mut container = Container::default();
            for i in 0..3 {
                container.windows_mut().push_back(Window::from(i));
            }
            workspace.add_container_to_back(container);
        }

        {
            // Container with 1 window
            let mut container = Container::default();
            container.windows_mut().push_back(Window::from(1));
            workspace.add_container_to_back(container);
        }
        // Should have 2 containers
        assert_eq!(workspace.containers().len(), 2);

        // Get focused container. Should be the index of the last container added
        let container = workspace.focused_container_mut().unwrap();

        // Should be focused on the container with 1 window
        assert_eq!(container.windows().len(), 1);
    }

    #[test]
    fn test_add_container_to_front() {
        let mut workspace = Workspace::default();

        {
            // Container with 1 window
            let mut container = Container::default();
            container.windows_mut().push_back(Window::from(1));
            workspace.add_container_to_front(container);
        }

        {
            // Container with 3 windows
            let mut container = Container::default();
            for i in 0..3 {
                container.windows_mut().push_back(Window::from(i));
            }
            workspace.add_container_to_front(container);
        }
        // Should have 2 containers
        assert_eq!(workspace.containers().len(), 2);

        // Get focused container. Should be the index of the last container added
        let container = workspace.focused_container_mut().unwrap();

        // Should be focused on the container with 3 windows
        assert_eq!(container.windows().len(), 3);
    }

    #[test]
    fn test_remove_non_existent_window() {
        let mut workspace = Workspace::default();

        {
            // Add a container with one window
            let mut container = Container::default();
            container.windows_mut().push_back(Window::from(1));
            workspace.add_container_to_back(container);
        }

        // Attempt to remove a non-existent window
        let result = workspace.remove_window(2);

        // Should return an error
        assert!(
            result.is_err(),
            "Expected an error when removing a non-existent window"
        );

        // Get focused container. Should be the index of the last container added
        let container = workspace.focused_container_mut().unwrap();

        // Should still have 1 window
        assert_eq!(container.windows().len(), 1);
    }

    #[test]
    fn test_remove_focused_container() {
        let mut workspace = Workspace::default();

        {
            // Container with 1 window
            let mut container = Container::default();
            container.windows_mut().push_back(Window::from(1));
            workspace.add_container_to_back(container);
        }

        {
            // Container with 1 window
            let mut container = Container::default();
            container.windows_mut().push_back(Window::from(1));
            workspace.add_container_to_back(container);
        }
        // Should have 2 containers
        assert_eq!(workspace.containers().len(), 2);

        // Should be focused on the container at index 1
        assert_eq!(workspace.focused_container_idx(), 1);

        // Store the container at index 1 before removal
        let container_to_remove = workspace.containers().get(1).cloned();
        workspace.remove_focused_container();

        // Should only have 1 container
        assert_eq!(workspace.containers().len(), 1);

        // Should be focused on the container at index 0
        assert_eq!(workspace.focused_container_idx(), 0);

        // Ensure the container at index 1 before removal is no longer present
        assert!(container_to_remove.is_some());
        assert!(
            !workspace
                .containers()
                .contains(&container_to_remove.unwrap())
        );
    }

    #[test]
    fn test_insert_container_at_idx() {
        let mut workspace = Workspace::default();

        for i in 0..4 {
            let mut container = Container::default();
            container.windows_mut().push_back(Window::from(i));
            workspace.add_container_to_back(container);
        }

        // Should have 4 containers
        assert_eq!(workspace.containers().len(), 4);

        // Should be focused on the last container
        assert_eq!(workspace.focused_container_idx(), 3);

        // Insert a container at index 4
        workspace.insert_container_at_idx(4, Container::default());

        // Should have 5 containers
        assert_eq!(workspace.containers().len(), 5);

        // Should be focused on the newly inserted container
        assert_eq!(workspace.focused_container_idx(), 4);
    }

    #[test]
    fn test_remove_container_by_idx() {
        let mut workspace = Workspace::default();

        for i in 0..3 {
            let mut container = Container::default();
            container.windows_mut().push_back(Window::from(i));
            workspace.add_container_to_back(container);
        }

        // Should have 3 containers
        assert_eq!(workspace.containers().len(), 3);

        // Should be focused on the last container
        assert_eq!(workspace.focused_container_idx(), 2);

        // Store the container at index 1 before removal
        let container_to_remove = workspace.containers().get(1).cloned();

        // Remove the container at index 1
        workspace.remove_container_by_idx(1);

        // Should have 2 containers
        assert_eq!(workspace.containers().len(), 2);

        // Ensure the container at index 1 before removal is no longer present
        assert!(container_to_remove.is_some());
        assert!(
            !workspace
                .containers()
                .contains(&container_to_remove.unwrap())
        );
    }

    #[test]
    fn test_remove_container() {
        let mut workspace = Workspace::default();

        for i in 0..3 {
            let mut container = Container::default();
            container.windows_mut().push_back(Window::from(i));
            workspace.add_container_to_back(container);
        }

        // Should have 3 containers
        assert_eq!(workspace.containers().len(), 3);

        // Should be focused on the last container
        assert_eq!(workspace.focused_container_idx(), 2);

        // Store the container at index 2 before removal
        let container_to_remove = workspace.containers().get(2).cloned();

        // Remove the container at index 2
        workspace.remove_container(2);

        // Should be focused on the previous container which is index 1
        assert_eq!(workspace.focused_container_idx(), 1);

        // Should have 2 containers
        assert_eq!(workspace.containers().len(), 2);

        // Ensure the container at index 1 before removal is no longer present
        assert!(container_to_remove.is_some());
        assert!(
            !workspace
                .containers()
                .contains(&container_to_remove.unwrap())
        );
    }

    #[test]
    fn test_focus_container() {
        let mut workspace = Workspace::default();

        for i in 0..3 {
            let mut container = Container::default();
            container.windows_mut().push_back(Window::from(i));
            workspace.add_container_to_back(container);
        }

        // Should have 3 containers
        assert_eq!(workspace.containers().len(), 3);

        // Should be focused on the last container
        assert_eq!(workspace.focused_container_idx(), 2);

        // Focus on container 1
        workspace.focus_container(1);
        assert_eq!(workspace.focused_container_idx(), 1);

        // Focus on container 0
        workspace.focus_container(0);
        assert_eq!(workspace.focused_container_idx(), 0);

        // Focus on container 2
        workspace.focus_container(2);
        assert_eq!(workspace.focused_container_idx(), 2);
    }

    #[test]
    fn test_focus_previous_container() {
        let mut workspace = Workspace::default();

        for i in 0..3 {
            let mut container = Container::default();
            container.windows_mut().push_back(Window::from(i));
            workspace.add_container_to_back(container);
        }

        // Should have 3 containers
        assert_eq!(workspace.containers().len(), 3);

        // Should be focused on the last container
        assert_eq!(workspace.focused_container_idx(), 2);

        // Focus on the previous container
        workspace.focus_previous_container();

        // Should be focused on container 1
        assert_eq!(workspace.focused_container_idx(), 1);
    }

    #[test]
    fn test_focus_last_container() {
        let mut workspace = Workspace::default();

        for i in 0..3 {
            let mut container = Container::default();
            container.windows_mut().push_back(Window::from(i));
            workspace.add_container_to_back(container);
        }

        // Should have 3 containers
        assert_eq!(workspace.containers().len(), 3);

        // Change focus to the first container for the test
        workspace.focus_container(0);
        assert_eq!(workspace.focused_container_idx(), 0);

        // Focus on the last container
        workspace.focus_last_container();

        // Should be focused on container 1
        assert_eq!(workspace.focused_container_idx(), 2);
    }

    #[test]
    fn test_focus_first_container() {
        let mut workspace = Workspace::default();

        for i in 0..3 {
            let mut container = Container::default();
            container.windows_mut().push_back(Window::from(i));
            workspace.add_container_to_back(container);
        }

        // Should have 3 containers
        assert_eq!(workspace.containers().len(), 3);

        // Should be focused on the last container
        assert_eq!(workspace.focused_container_idx(), 2);

        // Focus on the first container
        workspace.focus_first_container();

        // Should be focused on container 1
        assert_eq!(workspace.focused_container_idx(), 0);
    }

    #[test]
    fn test_swap_containers() {
        let mut workspace = Workspace::default();

        {
            let mut container = Container::default();
            for i in 0..3 {
                container.windows_mut().push_back(Window::from(i));
            }
            workspace.add_container_to_back(container);
        }

        {
            let mut container = Container::default();
            container.windows_mut().push_back(Window::from(1));
            workspace.add_container_to_back(container);
        }

        // Should have 2 containers
        assert_eq!(workspace.containers().len(), 2);

        {
            // Should be focused on container 1
            assert_eq!(workspace.focused_container_idx(), 1);

            // Should have 1 window
            let container = workspace.focused_container_mut().unwrap();
            assert_eq!(container.windows().len(), 1);
        }

        // Swap containers 0 and 1
        workspace.swap_containers(0, 1);

        {
            // Should be focused on container 0
            assert_eq!(workspace.focused_container_idx(), 1);

            let container = workspace.focused_container_mut().unwrap();
            assert_eq!(container.windows().len(), 3);
        }
    }

    #[test]
    fn test_new_container_for_window() {
        let mut workspace = Workspace::default();

        {
            let mut container = Container::default();
            container.windows_mut().push_back(Window::from(1));
            workspace.add_container_to_back(container);
        }

        // Add new window to container
        workspace
            .new_container_for_window(&Window::from(2))
            .unwrap();

        // Container 0 should have 1 window
        let container = workspace.focused_container_mut().unwrap();
        assert_eq!(container.windows().len(), 1);

        // Should return true that window 2 exists
        assert!(workspace.contains_window(2));
    }

    #[test]
    fn test_remove_window() {
        let mut workspace = Workspace::default();

        {
            // Container with 1 window
            let mut container = Container::default();
            for i in 0..3 {
                container.windows_mut().push_back(Window::from(i));
            }
            workspace.add_container_to_back(container);
        }

        // Remove window 1
        workspace.remove_window(1).ok();

        // Should have 2 windows
        let container = workspace.focused_container_mut().unwrap();
        assert_eq!(container.windows().len(), 2);

        // Check that window 1 is removed
        assert!(!workspace.contains_window(1));
    }

    #[test]
    fn test_focus_container_by_window() {
        let mut workspace = Workspace::default();

        {
            // Container with 3 windows
            let mut container = Container::default();
            for i in 0..3 {
                container.windows_mut().push_back(Window::from(i));
            }
            workspace.add_container_to_back(container);
        }

        {
            // Container with 1 window
            let mut container = Container::default();
            container.windows_mut().push_back(Window::from(4));
            workspace.add_container_to_back(container);
        }

        // Focus container by window
        workspace.focus_container_by_window(1).unwrap();

        // Should be focused on workspace 0
        assert_eq!(workspace.focused_container_idx(), 0);

        // Should be focused on window 1 and hwnd should be 1
        let focused_container = workspace.focused_container_mut().unwrap();
        assert_eq!(focused_container.focused_window().unwrap().id, 1);
        assert_eq!(focused_container.focused_window_idx(), 1);
    }

    fn tile(left: i32, top: i32, right: i32, bottom: i32) -> Rect {
        Rect {
            left,
            top,
            right,
            bottom,
        }
    }

    /// A workspace on the fake's 1920x1080 monitor, whose work area is 1920x1050, with a container
    /// for each of `ids`
    fn workspace_with(ids: &[u32]) -> (Arc<FakeBackend>, Workspace) {
        let fake = Arc::new(FakeBackend::with_one_monitor());
        let mut workspace = Workspace::default();
        workspace.globals.work_area = tile(0, 0, 1920, 1050);

        for id in ids {
            fake.open_window(*id, "foot", "foot", &format!("terminal {id}"));
            workspace
                .new_container_for_window(&Window::from(*id))
                .unwrap();
        }

        (fake, workspace)
    }

    #[test]
    fn update_puts_each_window_on_its_tile() -> eyre::Result<()> {
        let (fake, mut workspace) = workspace_with(&[401, 402]);

        with_backend(fake.clone(), || workspace.update())?;

        assert_eq!(fake.rect_of(401), Some(tile(0, 0, 960, 1050)));
        assert_eq!(fake.rect_of(402), Some(tile(960, 0, 960, 1050)));
        Ok(())
    }

    #[test]
    fn update_leaves_the_padding_and_the_offset_free() -> eyre::Result<()> {
        let (fake, mut workspace) = workspace_with(&[411]);
        workspace.workspace_padding = Some(10);
        workspace.container_padding = Some(5);
        workspace.globals.work_area_offset = Some(tile(0, 40, 0, 40));

        with_backend(fake.clone(), || workspace.update())?;

        // the offset moves the top edge down by 40, the workspace padding takes 10 off every edge,
        // and the container padding 5 more
        assert_eq!(fake.rect_of(411), Some(tile(15, 55, 1890, 980)));
        Ok(())
    }

    #[test]
    fn update_drops_a_container_that_has_no_windows_left() -> eyre::Result<()> {
        let (fake, mut workspace) = workspace_with(&[421]);
        workspace.add_container_to_back(Container::default());

        with_backend(fake.clone(), || workspace.update())?;

        assert_eq!(workspace.containers().len(), 1);
        assert_eq!(workspace.focused_container_idx(), 0);
        assert_eq!(fake.rect_of(421), Some(tile(0, 0, 1920, 1050)));
        Ok(())
    }
}
