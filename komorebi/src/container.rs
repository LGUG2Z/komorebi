use std::collections::VecDeque;
use std::num::NonZeroUsize;
use color_eyre::eyre::anyhow;
use color_eyre::Result;
use getset::{Getters, MutGetters, Setters, CopyGetters};
use nanoid::nanoid;
use schemars::JsonSchema;
use serde::Deserialize;
use serde::Serialize;

use crate::core::{Axis, DefaultLayout, Layout, Rect};

use crate::ring::Ring;
use crate::window::Window;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Getters, CopyGetters, MutGetters, Setters, JsonSchema)]
pub struct Container {
    #[getset(get = "pub")]
    id: String,
    windows: Ring<Window>,
    #[getset(get = "pub", get_mut = "pub", set = "pub")]
    monocle_window: Option<Window>,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[getset(get_copy = "pub", set = "pub")]
    monocle_window_restore_idx: Option<usize>,
    #[getset(get = "pub", get_mut = "pub", set = "pub")]
    layout: Layout,
    #[getset(get = "pub", get_mut = "pub", set = "pub")]
    layout_rules: Vec<(usize, Layout)>,
    #[getset(get_copy = "pub", set = "pub")]
    layout_flip: Option<Axis>,
    #[getset(get = "pub", set = "pub")]
    latest_layout: Vec<Rect>,
    #[getset(get = "pub", get_mut = "pub", set = "pub")]
    resize_dimensions: Vec<Option<Rect>>,
    #[getset(get = "pub", set = "pub")]
    tile: bool,
}

impl_ring_elements!(Container, Window);

impl Default for Container {
    fn default() -> Self {
        Self {
            id: nanoid!(),
            windows: Ring::default(),
            monocle_window: None,
            monocle_window_restore_idx: None,
            layout: Layout::Default(DefaultLayout::BSP),
            layout_rules: vec![],
            layout_flip: None,
            latest_layout: vec![],
            resize_dimensions: vec![],
            tile: true,
        }
    }
}

impl Container {
    pub fn new_monocle_window(&mut self) -> Result<()> {
        let focused_idx = self.focused_window_idx();
        let window = self.remove_focused_window()
            .ok_or_else(|| anyhow!("there is no window"))?;

        if self.windows().is_empty() {
            self.windows_mut().remove(focused_idx);
        } else {
            self.load_focused_window();
        }

        self.set_monocle_window(Option::from(window));
        self.set_monocle_window_restore_idx(Option::from(focused_idx));

        Ok(())
    }

    pub fn reintegrate_monocle_window(&mut self) -> Result<()> {
        let restore_idx = self.monocle_window_restore_idx()
            .ok_or_else(|| anyhow!("there is no monocle restore index"))?;

        let window = self.monocle_window()
            .as_ref()
            .ok_or_else(|| anyhow!("there is no monocle window"))?;

        let window = *window;
        if restore_idx >= self.windows().len() {
            self.windows_mut().push_back(window);
            self.focus_window(self.windows().len().saturating_sub(1));
        } else {
            self.windows_mut().insert(restore_idx, window);
            self.focus_window(restore_idx);
        }

        self.load_focused_window();
        self.set_monocle_window(None);
        self.set_monocle_window_restore_idx(None);

        Ok(())
    }

    pub fn hide(&self, omit: Option<isize>) {
        if let Some(window) = self.monocle_window() {
            let mut should_hide = omit.is_none();

            if !should_hide {
                if let Some(omit) = omit {
                    if omit != window.hwnd {
                        should_hide = true
                    }
                }
            }

            if should_hide {
                window.hide();
            }
        }

        for window in self.windows().iter().rev() {
            let mut should_hide = omit.is_none();

            if !should_hide {
                if let Some(omit) = omit {
                    if omit != window.hwnd {
                        should_hide = true
                    }
                }
            }

            if should_hide {
                window.hide();
            }
        }
    }

    pub fn restore(&self) {
        if let Some(window) = self.monocle_window() {
            window.restore();
            return;
        }

        if let Some(window) = self.focused_window() {
            window.restore();
        }
    }

    pub fn load_focused_window(&mut self) {
        let focused_idx = self.focused_window_idx();
        for (i, window) in self.windows_mut().iter_mut().enumerate() {
            if i == focused_idx {
                window.restore();
            } else {
                window.hide();
            }
        }
    }

    pub fn hwnd_from_exe(&self, exe: &str) -> Option<isize> {
        for window in self.windows() {
            if let Ok(window_exe) = window.exe() {
                if exe == window_exe {
                    return Option::from(window.hwnd);
                }
            }
        }

        None
    }

    pub fn idx_from_exe(&self, exe: &str) -> Option<usize> {
        for (idx, window) in self.windows().iter().enumerate() {
            if let Ok(window_exe) = window.exe() {
                if exe == window_exe {
                    return Option::from(idx);
                }
            }
        }

        None
    }

    pub fn contains_window(&self, hwnd: isize) -> bool {
        for window in self.windows() {
            if window.hwnd == hwnd {
                return true;
            }
        }

        false
    }

    pub fn idx_for_window(&self, hwnd: isize) -> Option<usize> {
        let mut idx = None;
        for (i, window) in self.windows().iter().enumerate() {
            if window.hwnd == hwnd {
                idx = Option::from(i);
            }
        }

        idx
    }

    pub fn remove_window_by_idx(&mut self, idx: usize) -> Option<Window> {
        let window = self.windows_mut().remove(idx);
        self.focus_window(idx.saturating_sub(1));
        window
    }

    pub fn remove_focused_window(&mut self) -> Option<Window> {
        let focused_idx = self.focused_window_idx();
        self.remove_window_by_idx(focused_idx)
    }

    pub fn add_window(&mut self, window: Window) {
        self.windows_mut().push_back(window);
        self.focus_window(self.windows().len().saturating_sub(1));
        let focused_window_idx = self.focused_window_idx();

        for (i, window) in self.windows().iter().enumerate() {
            if i != focused_window_idx {
                window.hide();
            }
        }
    }

    fn enforce_resize_constraints(&mut self) {
        match self.layout {
            Layout::Default(DefaultLayout::BSP) => self.enforce_resize_constraints_for_bsp(),
            Layout::Default(DefaultLayout::Columns) => self.enforce_resize_for_columns(),
            Layout::Default(DefaultLayout::Rows) => self.enforce_resize_for_rows(),
            Layout::Default(DefaultLayout::VerticalStack) => {
                self.enforce_resize_for_vertical_stack();
            }
            Layout::Default(DefaultLayout::RightMainVerticalStack) => {
                self.enforce_resize_for_right_vertical_stack();
            }
            Layout::Default(DefaultLayout::HorizontalStack) => {
                self.enforce_resize_for_horizontal_stack();
            }
            Layout::Default(DefaultLayout::UltrawideVerticalStack) => {
                self.enforce_resize_for_ultrawide();
            }
            _ => self.enforce_no_resize(),
        }
    }

    fn enforce_resize_constraints_for_bsp(&mut self) {
        for (i, rect) in self.resize_dimensions_mut().iter_mut().enumerate() {
            if let Some(rect) = rect {
                // Even windows can't be resized to the bottom
                if i % 2 == 0 {
                    rect.bottom = 0;
                    // Odd windows can't be resized to the right
                } else {
                    rect.right = 0;
                }
            }
        }

        // The first window can never be resized to the left or the top
        if let Some(Some(first)) = self.resize_dimensions_mut().first_mut() {
            first.top = 0;
            first.left = 0;
        }

        // The last window can never be resized to the bottom or the right
        if let Some(Some(last)) = self.resize_dimensions_mut().last_mut() {
            last.bottom = 0;
            last.right = 0;
        }
    }

    fn enforce_resize_for_columns(&mut self) {
        let resize_dimensions = self.resize_dimensions_mut();
        match resize_dimensions.len() {
            0 | 1 => self.enforce_no_resize(),
            _ => {
                let len = resize_dimensions.len();
                for (i, rect) in resize_dimensions.iter_mut().enumerate() {
                    if let Some(rect) = rect {
                        rect.top = 0;
                        rect.bottom = 0;

                        if i == 0 {
                            rect.left = 0;
                        }
                        if i == len - 1 {
                            rect.right = 0;
                        }
                    }
                }
            }
        }
    }

    fn enforce_resize_for_rows(&mut self) {
        let resize_dimensions = self.resize_dimensions_mut();
        match resize_dimensions.len() {
            0 | 1 => self.enforce_no_resize(),
            _ => {
                let len = resize_dimensions.len();
                for (i, rect) in resize_dimensions.iter_mut().enumerate() {
                    if let Some(rect) = rect {
                        rect.left = 0;
                        rect.right = 0;

                        if i == 0 {
                            rect.top = 0;
                        }
                        if i == len - 1 {
                            rect.bottom = 0;
                        }
                    }
                }
            }
        }
    }

    fn enforce_resize_for_vertical_stack(&mut self) {
        let resize_dimensions = self.resize_dimensions_mut();
        match resize_dimensions.len() {
            0 | 1 => self.enforce_no_resize(),
            _ => {
                if let Some(mut left) = resize_dimensions[0] {
                    left.top = 0;
                    left.bottom = 0;
                    left.left = 0;
                }

                let stack_size = resize_dimensions[1..].len();
                for (i, rect) in resize_dimensions[1..].iter_mut().enumerate() {
                    if let Some(rect) = rect {
                        rect.right = 0;

                        if i == 0 {
                            rect.top = 0;
                        } else if i == stack_size - 1 {
                            rect.bottom = 0;
                        }
                    }
                }
            }
        }
    }

    fn enforce_resize_for_right_vertical_stack(&mut self) {
        let resize_dimensions = self.resize_dimensions_mut();
        match resize_dimensions.len() {
            0 | 1 => self.enforce_no_resize(),
            _ => {
                if let Some(mut left) = resize_dimensions[1] {
                    left.top = 0;
                    left.bottom = 0;
                    left.right = 0;
                }

                let stack_size = resize_dimensions[1..].len();
                for (i, rect) in resize_dimensions[1..].iter_mut().enumerate() {
                    if let Some(rect) = rect {
                        rect.left = 0;

                        if i == 0 {
                            rect.top = 0;
                        } else if i == stack_size - 1 {
                            rect.bottom = 0;
                        }
                    }
                }
            }
        }
    }

    fn enforce_resize_for_horizontal_stack(&mut self) {
        let resize_dimensions = self.resize_dimensions_mut();
        match resize_dimensions.len() {
            0 | 1 => self.enforce_no_resize(),
            _ => {
                if let Some(mut left) = resize_dimensions[0] {
                    left.top = 0;
                    left.left = 0;
                    left.right = 0;
                }

                let stack_size = resize_dimensions[1..].len();
                for (i, rect) in resize_dimensions[1..].iter_mut().enumerate() {
                    if let Some(rect) = rect {
                        rect.bottom = 0;

                        if i == 0 {
                            rect.left = 0;
                        }
                        if i == stack_size - 1 {
                            rect.right = 0;
                        }
                    }
                }
            }
        }
    }

    fn enforce_resize_for_ultrawide(&mut self) {
        let resize_dimensions = self.resize_dimensions_mut();
        match resize_dimensions.len() {
            0 | 1 => self.enforce_no_resize(),
            2 => {
                if let Some(mut right) = resize_dimensions[0] {
                    right.top = 0;
                    right.bottom = 0;
                    right.right = 0;
                }

                if let Some(mut left) = resize_dimensions[1] {
                    left.top = 0;
                    left.bottom = 0;
                    left.left = 0;
                }
            }
            _ => {
                if let Some(mut right) = resize_dimensions[0] {
                    right.top = 0;
                    right.bottom = 0;
                }

                if let Some(mut left) = resize_dimensions[1] {
                    left.top = 0;
                    left.bottom = 0;
                    left.left = 0;
                }

                let stack_size = resize_dimensions[2..].len();
                for (i, rect) in resize_dimensions[2..].iter_mut().enumerate() {
                    if let Some(rect) = rect {
                        rect.right = 0;

                        if i == 0 {
                            rect.top = 0;
                        } else if i == stack_size - 1 {
                            rect.bottom = 0;
                        }
                    }
                }
            }
        }
    }

    fn enforce_no_resize(&mut self) {
        for rect in self.resize_dimensions_mut().iter_mut().flatten() {
            rect.left = 0;
            rect.right = 0;
            rect.top = 0;
            rect.bottom = 0;
        }
    }

    pub fn update(&mut self, container_rect: &Rect) -> Result<()> {
        // Handle monocle window first - it takes precedence
        if let Some(window) = self.monocle_window_mut() {
            window.set_position(container_rect, true)?;
            return Ok(());
        }

        // If no windows or not tiling, nothing to do
        if !*self.tile() || self.windows().is_empty() {
            return Ok(());
        }

        self.enforce_resize_constraints();

        // Check layout rules and update layout if needed
        if !self.layout_rules().is_empty() {
            let mut updated_layout = None;

            for rule in self.layout_rules() {
                if self.windows().len() >= rule.0 {
                    updated_layout = Option::from(rule.1.clone());
                }
            }

            if let Some(updated_layout) = updated_layout {
                if !matches!(updated_layout, Layout::Default(DefaultLayout::BSP)) {
                    self.set_layout_flip(None);
                }

                self.set_layout(updated_layout);
            }
        }

        // Calculate layouts for all windows
        let mut layouts = self.layout().as_boxed_arrangement().calculate(
            container_rect,
            NonZeroUsize::new(self.windows().len())
                .ok_or_else(|| anyhow!("there must be at least one window to calculate a container layout"))?,
            None, // containers don't have internal padding
            self.layout_flip(),
            self.resize_dimensions(),
        );

        // Apply layouts to windows
        for (i, window) in self.windows_mut().iter_mut().enumerate() {
            if let Some(layout) = layouts.get(i) {
                // Only set position for focused window, hide others
                if self.focused_window_idx() == i {
                    window.set_position(layout, true)?;
                } else {
                    window.hide();
                }
            }
        }

        // Store latest layout for future reference
        self.set_latest_layout(layouts);

        // Ensure resize_dimensions length matches window count
        let window_count = self.windows().len();
        self.resize_dimensions_mut().resize(window_count, None);

        Ok(())
    }

    #[tracing::instrument(skip(self))]
    pub fn focus_window(&mut self, idx: usize) {
        tracing::info!("focusing window");
        self.windows.focus(idx);
    }
}
