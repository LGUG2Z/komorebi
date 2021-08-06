use std::collections::VecDeque;

use color_eyre::eyre::ContextCompat;
use color_eyre::Result;
use serde::Serialize;

use komorebi_core::Layout;
use komorebi_core::LayoutFlip;
use komorebi_core::OperationDirection;
use komorebi_core::Rect;

use crate::container::Container;
use crate::ring::Ring;
use crate::window::Window;
use crate::windows_api::WindowsApi;

#[derive(Debug, Clone, Serialize)]
pub struct Workspace {
    name: Option<String>,
    containers: Ring<Container>,
    monocle_container: Option<Container>,
    #[serde(skip_serializing)]
    monocle_restore_idx: Option<usize>,
    floating_windows: Vec<Window>,
    layout: Layout,
    layout_flip: Option<LayoutFlip>,
    workspace_padding: Option<i32>,
    container_padding: Option<i32>,
    #[serde(skip_serializing)]
    latest_layout: Vec<Rect>,
}

impl Default for Workspace {
    fn default() -> Self {
        Self {
            name: None,
            containers: Ring::default(),
            monocle_container: None,
            monocle_restore_idx: None,
            floating_windows: Vec::default(),
            layout: Layout::BSP,
            layout_flip: None,
            workspace_padding: Option::from(10),
            container_padding: Option::from(10),
            latest_layout: vec![],
        }
    }
}

impl Workspace {
    pub fn hide(&mut self) {
        for container in self.containers_mut() {
            for window in container.windows_mut() {
                window.hide();
            }
        }
    }

    pub fn restore(&mut self) -> Result<()> {
        let idx = self.focused_container_idx();
        let mut to_focus = None;
        for (i, container) in self.containers_mut().iter_mut().enumerate() {
            if let Some(window) = container.visible_window_mut() {
                window.restore();

                if idx == i {
                    to_focus = Option::from(window);
                }
            }
        }

        // Do this here to make sure that an error doesn't stop the restoration of other windows
        if let Some(window) = to_focus {
            window.focus()?;
        }

        Ok(())
    }

    pub fn update(&mut self, work_area: &Rect) -> Result<()> {
        let mut adjusted_work_area = *work_area;
        adjusted_work_area.add_padding(self.workspace_padding());

        if let Some(container) = self.monocle_container_mut() {
            if let Some(window) = container.focused_window_mut() {
                window.set_position(&adjusted_work_area, true)?;
            }
        } else {
            let layouts = self.layout().calculate(
                &adjusted_work_area,
                self.containers().len(),
                self.container_padding(),
                self.layout_flip(),
            );

            let windows = self.visible_windows_mut();
            for (i, window) in windows.into_iter().enumerate() {
                if let (Some(window), Some(layout)) = (window, layouts.get(i)) {
                    window.set_position(layout, false)?;
                }
            }

            self.set_latest_layout(layouts);
        }

        Ok(())
    }

    pub fn reap_orphans(&mut self) -> Result<(usize, usize)> {
        let mut hwnds = vec![];
        let mut floating_hwnds = vec![];

        for window in self.visible_windows_mut().into_iter().flatten() {
            if !window.is_window() {
                hwnds.push(window.hwnd);
            }
        }

        for window in self.floating_windows() {
            if !window.is_window() {
                floating_hwnds.push(window.hwnd);
            }
        }

        for hwnd in &hwnds {
            tracing::debug!("reaping hwnd: {}", hwnd);
            self.remove_window(*hwnd)?;
        }

        for hwnd in &floating_hwnds {
            tracing::debug!("reaping floating hwnd: {}", hwnd);
            self.floating_windows_mut()
                .retain(|w| !floating_hwnds.contains(&w.hwnd));
        }

        let mut container_ids = vec![];
        for container in self.containers() {
            if container.windows().is_empty() {
                container_ids.push(container.id().clone());
            }
        }

        self.containers_mut()
            .retain(|c| !container_ids.contains(c.id()));

        Ok((hwnds.len() + floating_hwnds.len(), container_ids.len()))
    }

    pub fn focus_container_by_window(&mut self, hwnd: isize) -> Result<()> {
        let container_idx = self
            .container_idx_for_window(hwnd)
            .context("there is no container/window")?;

        let container = self
            .containers_mut()
            .get_mut(container_idx)
            .context("there is no container")?;

        let window_idx = container
            .idx_for_window(hwnd)
            .context("there is no window")?;

        container.focus_window(window_idx);
        self.focus_container(container_idx);

        Ok(())
    }

    pub fn container_idx_from_current_point(&self) -> Option<usize> {
        let mut idx = None;

        let point = WindowsApi::cursor_pos().ok()?;

        for (i, _container) in self.containers().iter().enumerate() {
            if let Some(rect) = self.latest_layout().get(i) {
                if rect.contains_point((point.x, point.y)) {
                    idx = Option::from(i);
                }
            }
        }

        idx
    }

    pub fn contains_window(&self, hwnd: isize) -> bool {
        for x in self.containers() {
            if x.contains_window(hwnd) {
                return true;
            }
        }

        false
    }

    pub fn promote_container(&mut self) -> Result<()> {
        let container = self
            .remove_focused_container()
            .context("there is no container")?;
        self.containers_mut().push_front(container);
        self.focus_container(0);

        Ok(())
    }

    pub fn add_container(&mut self, container: Container) {
        self.containers_mut().push_back(container);
        self.focus_container(self.containers().len() - 1);
    }

    fn remove_container_by_idx(&mut self, idx: usize) -> Option<Container> {
        self.containers_mut().remove(idx)
    }

    fn container_idx_for_window(&mut self, hwnd: isize) -> Option<usize> {
        let mut idx = None;
        for (i, x) in self.containers().iter().enumerate() {
            if x.contains_window(hwnd) {
                idx = Option::from(i);
            }
        }

        idx
    }

    pub fn remove_window(&mut self, hwnd: isize) -> Result<()> {
        if self.floating_windows().iter().any(|w| w.hwnd == hwnd) {
            self.floating_windows_mut().retain(|w| w.hwnd != hwnd);
            return Ok(());
        }

        let container_idx = self
            .container_idx_for_window(hwnd)
            .context("there is no window")?;

        let container = self
            .containers_mut()
            .get_mut(container_idx)
            .context("there is no container")?;

        let window_idx = container
            .windows()
            .iter()
            .position(|window| window.hwnd == hwnd)
            .context("there is no window")?;

        container
            .remove_window_by_idx(window_idx)
            .context("there is no window")?;

        if container.windows().is_empty() {
            self.containers_mut()
                .remove(container_idx)
                .context("there is no container")?;
        }

        if container_idx != 0 {
            self.focus_container(container_idx - 1);
        }

        Ok(())
    }

    pub fn remove_focused_container(&mut self) -> Option<Container> {
        let focused_idx = self.focused_container_idx();
        let container = self.remove_container_by_idx(focused_idx);

        if focused_idx != 0 {
            self.focus_container(focused_idx - 1);
        }

        container
    }

    pub fn new_idx_for_direction(&self, direction: OperationDirection) -> Option<usize> {
        if direction.is_valid(
            self.layout,
            self.focused_container_idx(),
            self.containers().len(),
        ) {
            Option::from(direction.new_idx(self.layout, self.containers.focused_idx()))
        } else {
            None
        }
    }

    pub fn move_window_to_container(&mut self, target_container_idx: usize) -> Result<()> {
        let focused_idx = self.focused_container_idx();

        let container = self
            .focused_container_mut()
            .context("there is no container")?;

        let window = container
            .remove_focused_window()
            .context("there is no window")?;

        // This is a little messy
        let adjusted_target_container_index = if container.windows().is_empty() {
            self.containers_mut().remove(focused_idx);
            if focused_idx < target_container_idx {
                target_container_idx - 1
            } else {
                target_container_idx
            }
        } else {
            container.load_focused_window();
            target_container_idx
        };

        let target_container = self
            .containers_mut()
            .get_mut(adjusted_target_container_index)
            .context("there is no container")?;

        target_container.add_window(window);

        self.focus_container(adjusted_target_container_index);
        self.focused_container_mut()
            .context("there is no container")?
            .load_focused_window();

        Ok(())
    }

    pub fn new_container_for_focused_window(&mut self) -> Result<()> {
        let focused_container_idx = self.focused_container_idx();

        let container = self
            .focused_container_mut()
            .context("there is no container")?;

        let window = container
            .remove_focused_window()
            .context("there is no window")?;

        if container.windows().is_empty() {
            self.containers_mut().remove(focused_container_idx);
        } else {
            container.load_focused_window();
        }

        self.new_container_for_window(window);

        let mut container = Container::default();
        container.add_window(window);
        Ok(())
    }

    pub fn new_container_for_floating_window(&mut self) -> Result<()> {
        let focused_idx = self.focused_container_idx();
        let window = self
            .remove_focused_floating_window()
            .context("there is no floating window")?;

        let mut container = Container::default();
        container.add_window(window);
        self.containers_mut().insert(focused_idx, container);

        Ok(())
    }

    pub fn new_container_for_window(&mut self, window: Window) {
        let focused_idx = self.focused_container_idx();
        let len = self.containers().len();

        let mut container = Container::default();
        container.add_window(window);

        if focused_idx == len - 1 {
            self.containers_mut().resize(len, Container::default());
        }

        self.containers_mut().insert(focused_idx + 1, container);
        self.focus_container(focused_idx + 1);
    }

    pub fn new_floating_window(&mut self) -> Result<()> {
        let focused_idx = self.focused_container_idx();

        let container = self
            .focused_container_mut()
            .context("there is no container")?;

        let window = container
            .remove_focused_window()
            .context("there is no window")?;

        if container.windows().is_empty() {
            self.containers_mut().remove(focused_idx);
        } else {
            container.load_focused_window();
        }

        self.floating_windows_mut().push(window);

        Ok(())
    }

    pub fn new_monocle_container(&mut self) -> Result<()> {
        let focused_idx = self.focused_container_idx();
        let container = self
            .containers_mut()
            .remove(focused_idx)
            .context("there is not container")?;

        self.monocle_container = Option::from(container);
        self.monocle_restore_idx = Option::from(focused_idx);

        if focused_idx != 0 {
            self.focus_container(focused_idx - 1);
        }

        self.monocle_container_mut()
            .context("there is no monocle container")?
            .load_focused_window();

        Ok(())
    }

    pub fn reintegrate_monocle_container(&mut self) -> Result<()> {
        let restore_idx = self
            .monocle_restore_idx()
            .context("there is no monocle restore index")?;

        let container = self
            .monocle_container_mut()
            .context("there is no monocle container")?;

        let container = container.clone();
        if restore_idx > self.containers().len() - 1 {
            self.containers_mut()
                .resize(restore_idx, Container::default());
        }

        self.containers_mut().insert(restore_idx, container);
        self.focus_container(restore_idx);
        self.focused_container_mut()
            .context("there is no container")?
            .load_focused_window();

        self.monocle_container = None;

        Ok(())
    }

    pub const fn monocle_container(&self) -> Option<&Container> {
        self.monocle_container.as_ref()
    }

    pub fn monocle_container_mut(&mut self) -> Option<&mut Container> {
        self.monocle_container.as_mut()
    }

    pub const fn monocle_restore_idx(&self) -> Option<usize> {
        self.monocle_restore_idx
    }

    pub fn focused_container(&self) -> Option<&Container> {
        self.containers.focused()
    }

    pub const fn focused_container_idx(&self) -> usize {
        self.containers.focused_idx()
    }

    pub fn focused_container_mut(&mut self) -> Option<&mut Container> {
        self.containers.focused_mut()
    }

    #[tracing::instrument(skip(self))]
    pub fn focus_container(&mut self, idx: usize) {
        tracing::info!("focusing container");

        self.containers.focus(idx);
    }

    pub const fn containers(&self) -> &VecDeque<Container> {
        self.containers.elements()
    }

    pub fn containers_mut(&mut self) -> &mut VecDeque<Container> {
        self.containers.elements_mut()
    }

    pub fn swap_containers(&mut self, i: usize, j: usize) {
        self.containers.swap(i, j);
        self.focus_container(j);
    }

    pub fn remove_focused_floating_window(&mut self) -> Option<Window> {
        let hwnd = WindowsApi::top_visible_window().ok()?;

        let mut idx = None;
        for (i, window) in self.floating_windows.iter().enumerate() {
            if hwnd == window.hwnd {
                idx = Option::from(i);
            }
        }

        match idx {
            None => None,
            Some(idx) => {
                if self.floating_windows.get(idx).is_some() {
                    Option::from(self.floating_windows_mut().remove(idx))
                } else {
                    None
                }
            }
        }
    }

    pub const fn floating_windows(&self) -> &Vec<Window> {
        &self.floating_windows
    }

    pub fn floating_windows_mut(&mut self) -> &mut Vec<Window> {
        self.floating_windows.as_mut()
    }

    pub fn visible_windows_mut(&mut self) -> Vec<Option<&mut Window>> {
        let mut vec = vec![];
        for container in self.containers_mut() {
            vec.push(container.visible_window_mut());
        }

        vec
    }

    pub const fn layout(&self) -> Layout {
        self.layout
    }

    pub const fn layout_flip(&self) -> Option<LayoutFlip> {
        self.layout_flip
    }

    pub const fn workspace_padding(&self) -> Option<i32> {
        self.workspace_padding
    }

    pub const fn container_padding(&self) -> Option<i32> {
        self.container_padding
    }

    pub const fn latest_layout(&self) -> &Vec<Rect> {
        &self.latest_layout
    }

    pub fn set_name(&mut self, name: Option<String>) {
        self.name = name;
    }

    pub fn set_layout(&mut self, layout: Layout) {
        self.layout = layout;
    }

    pub fn set_layout_flip(&mut self, layout_flip: Option<LayoutFlip>) {
        self.layout_flip = layout_flip;
    }

    pub fn set_workspace_padding(&mut self, padding: Option<i32>) {
        self.workspace_padding = padding;
    }

    pub fn set_container_padding(&mut self, padding: Option<i32>) {
        self.container_padding = padding;
    }

    pub fn set_latest_layout(&mut self, layout: Vec<Rect>) {
        self.latest_layout = layout;
    }
}
