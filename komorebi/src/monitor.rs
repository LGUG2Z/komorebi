use std::collections::HashMap;
use std::collections::VecDeque;

use color_eyre::eyre::ContextCompat;
use color_eyre::Result;

use komorebi_core::Rect;

use crate::container::Container;
use crate::ring::Ring;
use crate::workspace::Workspace;

#[derive(Debug, Clone)]
pub struct Monitor {
    id: isize,
    monitor_size: Rect,
    work_area_size: Rect,
    workspaces: Ring<Workspace>,
    workspace_names: HashMap<usize, String>,
}

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

    pub fn move_container_to_workspace(
        &mut self,
        target_workspace_idx: usize,
        follow: bool,
    ) -> Result<()> {
        let container = self
            .focused_workspace_mut()
            .context("there is no workspace")?
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

    pub fn focused_workspace(&self) -> Option<&Workspace> {
        self.workspaces.focused()
    }

    pub const fn focused_workspace_idx(&self) -> usize {
        self.workspaces.focused_idx()
    }

    pub fn focused_workspace_mut(&mut self) -> Option<&mut Workspace> {
        self.workspaces.focused_mut()
    }

    pub fn focus_workspace(&mut self, idx: usize) -> Result<()> {
        {
            let workspaces = self.workspaces_mut();

            tracing::info!("focusing workspace at index: {}", idx);
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

    pub fn update_focused_workspace(&mut self) -> Result<()> {
        tracing::info!("updating workspace: {}", self.focused_workspace_idx());
        let work_area = *self.work_area_size();

        self.focused_workspace_mut()
            .context("there is no workspace")?
            .update(&work_area)?;

        Ok(())
    }

    pub const fn workspaces(&self) -> &VecDeque<Workspace> {
        self.workspaces.elements()
    }

    pub fn workspaces_mut(&mut self) -> &mut VecDeque<Workspace> {
        self.workspaces.elements_mut()
    }

    pub fn workspace_names_mut(&mut self) -> &mut HashMap<usize, String> {
        &mut self.workspace_names
    }

    pub const fn id(&self) -> isize {
        self.id
    }

    pub const fn work_area_size(&self) -> &Rect {
        &self.work_area_size
    }
}
