use std::collections::HashMap;
use std::collections::VecDeque;
use std::sync::atomic::Ordering;

use color_eyre::Result;
use color_eyre::eyre::anyhow;
use color_eyre::eyre::bail;
use getset::CopyGetters;
use getset::Getters;
use getset::MutGetters;
use getset::Setters;
use schemars::JsonSchema;
use serde::Deserialize;
use serde::Serialize;

use crate::core::Rect;

use crate::DEFAULT_CONTAINER_PADDING;
use crate::DEFAULT_WORKSPACE_PADDING;
use crate::DefaultLayout;
use crate::Layout;
use crate::OperationDirection;
use crate::WindowsApi;
use crate::container::Container;
use crate::ring::Ring;
use crate::workspace::Workspace;

#[derive(
    Debug,
    Clone,
    Serialize,
    Deserialize,
    Getters,
    CopyGetters,
    MutGetters,
    Setters,
    JsonSchema,
    PartialEq,
)]
pub struct Monitor {
    #[getset(get_copy = "pub", set = "pub")]
    pub id: isize,
    #[getset(get = "pub", set = "pub")]
    pub name: String,
    #[getset(get = "pub", set = "pub")]
    pub device: String,
    #[getset(get = "pub", set = "pub")]
    pub device_id: String,
    #[getset(get = "pub", set = "pub")]
    pub serial_number_id: Option<String>,
    #[getset(get = "pub", set = "pub")]
    pub size: Rect,
    #[getset(get = "pub", set = "pub")]
    pub work_area_size: Rect,
    #[getset(get_copy = "pub", set = "pub")]
    pub work_area_offset: Option<Rect>,
    #[getset(get_copy = "pub", set = "pub")]
    pub window_based_work_area_offset: Option<Rect>,
    #[getset(get_copy = "pub", set = "pub")]
    pub window_based_work_area_offset_limit: isize,
    pub workspaces: Ring<Workspace>,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[getset(get_copy = "pub", set = "pub")]
    pub last_focused_workspace: Option<usize>,
    #[getset(get_mut = "pub")]
    pub workspace_names: HashMap<usize, String>,
    #[getset(get_copy = "pub", set = "pub")]
    pub container_padding: Option<i32>,
    #[getset(get_copy = "pub", set = "pub")]
    pub workspace_padding: Option<i32>,
}

impl_ring_elements!(Monitor, Workspace);

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
            id: monitor.id,
            name: monitor.name.clone(),
            device: monitor.device.clone(),
            device_id: monitor.device_id.clone(),
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
        id,
        name,
        device,
        device_id,
        serial_number_id,
        size,
        work_area_size,
        work_area_offset: None,
        window_based_work_area_offset: None,
        window_based_work_area_offset_limit: 1,
        workspaces,
        last_focused_workspace: None,
        workspace_names: HashMap::default(),
        container_padding: None,
        workspace_padding: None,
    }
}

impl Monitor {
    pub fn new(
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

    pub fn placeholder() -> Self {
        Self {
            id: 0,
            name: "PLACEHOLDER".to_string(),
            device: "".to_string(),
            device_id: "".to_string(),
            serial_number_id: None,
            size: Default::default(),
            work_area_size: Default::default(),
            work_area_offset: None,
            window_based_work_area_offset: None,
            window_based_work_area_offset_limit: 0,
            workspaces: Default::default(),
            last_focused_workspace: None,
            workspace_names: Default::default(),
            container_padding: None,
            workspace_padding: None,
        }
    }

    pub fn focused_workspace_name(&self) -> Option<String> {
        self.focused_workspace()
            .map(|w| w.name().clone())
            .unwrap_or(None)
    }

    pub fn load_focused_workspace(&mut self, mouse_follows_focus: bool) -> Result<()> {
        let focused_idx = self.focused_workspace_idx();
        for (i, workspace) in self.workspaces_mut().iter_mut().enumerate() {
            if i == focused_idx {
                workspace.restore(mouse_follows_focus)?;
            } else {
                workspace.hide(None);
            }
        }

        Ok(())
    }

    /// Updates the `globals` field of all workspaces
    pub fn update_workspaces_globals(&mut self, offset: Option<Rect>) {
        let container_padding = self
            .container_padding()
            .or(Some(DEFAULT_CONTAINER_PADDING.load(Ordering::SeqCst)));
        let workspace_padding = self
            .workspace_padding()
            .or(Some(DEFAULT_WORKSPACE_PADDING.load(Ordering::SeqCst)));
        let work_area = *self.work_area_size();
        let offset = self.work_area_offset.or(offset);
        let window_based_work_area_offset = self.window_based_work_area_offset();
        let limit = self.window_based_work_area_offset_limit();

        for workspace in self.workspaces_mut() {
            workspace.globals_mut().container_padding = container_padding;
            workspace.globals_mut().workspace_padding = workspace_padding;
            workspace.globals_mut().work_area = work_area;
            workspace.globals_mut().work_area_offset = offset;
            workspace.globals_mut().window_based_work_area_offset = window_based_work_area_offset;
            workspace.globals_mut().window_based_work_area_offset_limit = limit;
        }
    }

    /// Updates the `globals` field of workspace with index `workspace_idx`
    pub fn update_workspace_globals(&mut self, workspace_idx: usize, offset: Option<Rect>) {
        let container_padding = self
            .container_padding()
            .or(Some(DEFAULT_CONTAINER_PADDING.load(Ordering::SeqCst)));
        let workspace_padding = self
            .workspace_padding()
            .or(Some(DEFAULT_WORKSPACE_PADDING.load(Ordering::SeqCst)));
        let work_area = *self.work_area_size();
        let offset = self.work_area_offset.or(offset);
        let window_based_work_area_offset = self.window_based_work_area_offset();
        let limit = self.window_based_work_area_offset_limit();

        if let Some(workspace) = self.workspaces_mut().get_mut(workspace_idx) {
            workspace.globals_mut().container_padding = container_padding;
            workspace.globals_mut().workspace_padding = workspace_padding;
            workspace.globals_mut().work_area = work_area;
            workspace.globals_mut().work_area_offset = offset;
            workspace.globals_mut().window_based_work_area_offset = window_based_work_area_offset;
            workspace.globals_mut().window_based_work_area_offset_limit = limit;
        }
    }

    pub fn add_container(
        &mut self,
        container: Container,
        workspace_idx: Option<usize>,
    ) -> Result<()> {
        let workspace = if let Some(idx) = workspace_idx {
            self.workspaces_mut()
                .get_mut(idx)
                .ok_or_else(|| anyhow!("there is no workspace at index {}", idx))?
        } else {
            self.focused_workspace_mut()
                .ok_or_else(|| anyhow!("there is no workspace"))?
        };

        workspace.add_container_to_back(container);

        Ok(())
    }

    /// Adds a container to this `Monitor` using the move direction to calculate if the container
    /// should be added in front of all containers, in the back or in place of the focused
    /// container, moving the rest along. The move direction should be from the origin monitor
    /// towards the target monitor or from the origin workspace towards the target workspace.
    pub fn add_container_with_direction(
        &mut self,
        container: Container,
        workspace_idx: Option<usize>,
        direction: OperationDirection,
    ) -> Result<()> {
        let workspace = if let Some(idx) = workspace_idx {
            self.workspaces_mut()
                .get_mut(idx)
                .ok_or_else(|| anyhow!("there is no workspace at index {}", idx))?
        } else {
            self.focused_workspace_mut()
                .ok_or_else(|| anyhow!("there is no workspace"))?
        };

        match direction {
            OperationDirection::Left => {
                // insert the container into the workspace on the monitor at the back (or rightmost position)
                // if we are moving across a boundary to the left (back = right side of the target)
                match workspace.layout() {
                    Layout::Default(layout) => match layout {
                        DefaultLayout::RightMainVerticalStack => {
                            workspace.add_container_to_front(container);
                        }
                        DefaultLayout::UltrawideVerticalStack => {
                            if workspace.containers().len() == 1 {
                                workspace.insert_container_at_idx(0, container);
                            } else {
                                workspace.add_container_to_back(container);
                            }
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
                match workspace.layout() {
                    Layout::Default(layout) => {
                        let target_index = layout.leftmost_index(workspace.containers().len());

                        match layout {
                            DefaultLayout::RightMainVerticalStack
                            | DefaultLayout::UltrawideVerticalStack => {
                                if workspace.containers().len() == 1 {
                                    workspace.add_container_to_back(container);
                                } else {
                                    workspace.insert_container_at_idx(target_index, container);
                                }
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

    pub fn ensure_workspace_count(&mut self, ensure_count: usize) {
        if self.workspaces().len() < ensure_count {
            self.workspaces_mut()
                .resize(ensure_count, Workspace::default());
        }
    }

    pub fn remove_workspaces(&mut self) -> VecDeque<Workspace> {
        self.workspaces_mut().drain(..).collect()
    }

    #[tracing::instrument(skip(self))]
    pub fn move_container_to_workspace(
        &mut self,
        target_workspace_idx: usize,
        follow: bool,
        direction: Option<OperationDirection>,
    ) -> Result<()> {
        let workspace = self
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

        if let Some(idx) = floating_window_index {
            let window = workspace.floating_windows_mut().remove(idx);

            let workspaces = self.workspaces_mut();
            #[allow(clippy::option_if_let_else)]
            let target_workspace = match workspaces.get_mut(target_workspace_idx) {
                None => {
                    workspaces.resize(target_workspace_idx + 1, Workspace::default());
                    workspaces.get_mut(target_workspace_idx).unwrap()
                }
                Some(workspace) => workspace,
            };

            target_workspace.floating_windows_mut().push(window);
        } else {
            let container = workspace
                .remove_focused_container()
                .ok_or_else(|| anyhow!("there is no container"))?;

            let workspaces = self.workspaces_mut();

            #[allow(clippy::option_if_let_else)]
            let target_workspace = match workspaces.get_mut(target_workspace_idx) {
                None => {
                    workspaces.resize(target_workspace_idx + 1, Workspace::default());
                    workspaces.get_mut(target_workspace_idx).unwrap()
                }
                Some(workspace) => workspace,
            };

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

    #[tracing::instrument(skip(self))]
    pub fn focus_workspace(&mut self, idx: usize) -> Result<()> {
        tracing::info!("focusing workspace");

        {
            let workspaces = self.workspaces_mut();

            if workspaces.get(idx).is_none() {
                workspaces.resize(idx + 1, Workspace::default());
            }

            self.workspaces.focus(idx);
        }

        // Always set the latest known name when creating the workspace for the first time
        {
            let name = { self.workspace_names.get(&idx).cloned() };
            if name.is_some() {
                self.workspaces_mut()
                    .get_mut(idx)
                    .ok_or_else(|| anyhow!("there is no workspace"))?
                    .set_name(name);
            }
        }

        Ok(())
    }

    pub fn new_workspace_idx(&self) -> usize {
        self.workspaces().len()
    }

    pub fn update_focused_workspace(&mut self, offset: Option<Rect>) -> Result<()> {
        let offset = if self.work_area_offset().is_some() {
            self.work_area_offset()
        } else {
            offset
        };

        let focused_workspace_idx = self.focused_workspace_idx();
        self.update_workspace_globals(focused_workspace_idx, offset);
        self.focused_workspace_mut()
            .ok_or_else(|| anyhow!("there is no workspace"))?
            .update()?;

        Ok(())
    }
}
