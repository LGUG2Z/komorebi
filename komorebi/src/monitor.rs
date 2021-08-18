use std::collections::HashMap;
use std::collections::VecDeque;

use color_eyre::eyre::ContextCompat;
use color_eyre::Result;
use getset::CopyGetters;
use getset::Getters;
use getset::MutGetters;
use serde::Serialize;

use komorebi_core::Rect;

use crate::container::Container;
use crate::ring::Ring;
use crate::workspace::Workspace;

#[derive(Debug, Clone, Serialize, Getters, CopyGetters, MutGetters)]
pub struct Monitor {
    #[getset(get_copy = "pub")]
    id: isize,
    monitor_size: Rect,
    #[getset(get = "pub")]
    work_area_size: Rect,
    workspaces: Ring<Workspace>,
    #[serde(skip_serializing)]
    #[getset(get_mut = "pub")]
    workspace_names: HashMap<usize, String>,
}

impl_ring_elements!(Monitor, Workspace);

pub fn new(id: isize, monitor_size: Rect, work_area_size: Rect) -> Monitor {
    Monitor {
        id,
        monitor_size,
        work_area_size,
        workspaces: Ring::default(),
        workspace_names: HashMap::default(),
    }
}

impl Monitor {
    pub fn load_focused_workspace(&mut self) -> Result<()> {
        let focused_idx = self.focused_workspace_idx();
        for (i, workspace) in self.workspaces_mut().iter_mut().enumerate() {
            if i == focused_idx {
                workspace.restore()?;
            } else {
                workspace.hide();
            }
        }

        Ok(())
    }

    pub fn add_container(&mut self, container: Container) -> Result<()> {
        let workspace = self
            .focused_workspace_mut()
            .context("there is no workspace")?;

        workspace.add_container(container);

        Ok(())
    }

    pub fn ensure_workspace_count(&mut self, ensure_count: usize) {
        if self.workspaces().len() < ensure_count {
            self.workspaces_mut()
                .resize(ensure_count, Workspace::default());
        }
    }

    #[tracing::instrument(skip(self))]
    pub fn move_container_to_workspace(
        &mut self,
        target_workspace_idx: usize,
        follow: bool,
    ) -> Result<()> {
        let workspace = self
            .focused_workspace_mut()
            .context("there is no workspace")?;

        if workspace.maximized_window().is_some() {
            return Err(eyre::anyhow!(
                "cannot move native maximized window to another monitor or workspace"
            ));
        }

        let container = workspace
            .remove_focused_container()
            .context("there is no container")?;

        let workspaces = self.workspaces_mut();

        let target_workspace = match workspaces.get_mut(target_workspace_idx) {
            None => {
                workspaces.resize(target_workspace_idx + 1, Workspace::default());
                workspaces.get_mut(target_workspace_idx).unwrap()
            }
            Some(workspace) => workspace,
        };

        target_workspace.add_container(container);

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
                    .context("there is no workspace")?
                    .set_name(name);
            }
        }

        Ok(())
    }

    pub fn new_workspace_idx(&self) -> usize {
        self.workspaces().len()
    }

    pub fn update_focused_workspace(&mut self) -> Result<()> {
        let work_area = *self.work_area_size();

        self.focused_workspace_mut()
            .context("there is no workspace")?
            .update(&work_area)?;

        Ok(())
    }
}
