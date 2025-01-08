use color_eyre::eyre::anyhow;
use color_eyre::Result;
use getset::CopyGetters;
use getset::Getters;
use getset::MutGetters;
use getset::Setters;
use nanoid::nanoid;
use schemars::JsonSchema;
use serde::Deserialize;
use serde::Serialize;
use std::collections::VecDeque;
use std::num::NonZeroUsize;

use crate::core::Axis;
use crate::core::DefaultLayout;
use crate::core::Layout;
use crate::core::Rect;

use crate::ring::Ring;
use crate::window::Window;
use crate::windows_api::WindowsApi;

#[derive(
    Debug,
    Clone,
    PartialEq,
    Serialize,
    Deserialize,
    Getters,
    CopyGetters,
    MutGetters,
    Setters,
    JsonSchema,
)]
pub struct Container {
    #[getset(get = "pub")]
    id: String,
    windows: Ring<Window>,
    #[getset(get_copy = "pub", set = "pub")]
    monocle: bool,
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
            monocle: false,
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
    pub fn toggle_monocle(&mut self) -> Result<()> {
        self.set_monocle(!self.monocle());
        Ok(())
    }

    pub fn hide(&self, omit: Option<isize>) {
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
        if self.monocle() {
            // In monocle mode, only restore the focused window
            if let Some(window) = self.focused_window() {
                window.restore();
            }
        } else {
            // In regular layout mode, restore all windows
            for window in self.windows() {
                window.restore();
            }
        }
    }

    pub fn load_focused_window(&mut self) {
        if self.monocle() {
            // In monocle mode, only show focused window
            let focused_idx = self.focused_window_idx();
            for (i, window) in self.windows_mut().iter_mut().enumerate() {
                if i == focused_idx {
                    window.restore();
                } else {
                    window.hide();
                }
            }
        } else {
            // When not in monocle mode, restore all windows
            for window in self.windows_mut() {
                window.restore();
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
        match self.layout() {
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

    pub fn update(
        &mut self,
        container_rect: &Rect,
        should_have_stackbar: bool,
        tab_height: &i32,
    ) -> Result<()> {
        // If no windows, nothing to do
        if self.windows().is_empty() {
            return Ok(());
        }

        // Handle monocle mode
        if self.monocle() {
            for window in self.windows() {
                window.set_position(container_rect, true)?;
            }
            return Ok(());
        }

        // Handle non-tiling mode
        if !*self.tile() {
            for window in self.windows() {
                if let Ok(current_pos) = WindowsApi::window_rect(window.hwnd) {
                    let mut new_pos = current_pos;
                    let mut needs_adjustment = false;

                    // Calculate container dimensions (right and bottom are width/height)
                    let container_width = container_rect.right;
                    let container_height = container_rect.bottom;

                    // Set minimum sizes to half of container dimensions
                    let min_width = container_width / 2;
                    let min_height = container_height / 2;

                    // Calculate window dimensions (right and bottom are width/height)
                    let window_width = new_pos.right;
                    let window_height = new_pos.bottom;

                    // Adjust right edge if needed
                    if (new_pos.left + window_width) > (container_rect.left + container_width) {
                        let new_width = (container_rect.left + container_width) - new_pos.left;
                        
                        // Always adjust the right edge to container boundary
                        new_pos.right = container_width - (new_pos.left - container_rect.left);
                        needs_adjustment = true;

                        // Special handling if new width would be smaller than minimum
                        if new_width < min_width {
                            // Move left edge to center while keeping right edge at container boundary
                            new_pos.left = container_rect.left + container_width - min_width;
                            new_pos.right = min_width;
                        }
                    }

                    // Adjust left edge if needed
                    if new_pos.left < container_rect.left {
                        let new_width = window_width + (new_pos.left - container_rect.left);
                        
                        // Always adjust the left edge to container boundary
                        new_pos.left = container_rect.left;
                        new_pos.right = new_width;
                        needs_adjustment = true;

                        // Special handling if new width would be smaller than minimum
                        if new_width < min_width {
                            // Move right edge to center while keeping left edge at container boundary
                            new_pos.right = min_width;
                            new_pos.left = container_rect.left;
                        }
                    }

                    // Adjust bottom edge if needed
                    if (new_pos.top + window_height) > (container_rect.top + container_height) {
                        let new_height = (container_rect.top + container_height) - new_pos.top;
                        
                        // Always adjust the bottom edge to container boundary
                        new_pos.bottom = container_height - (new_pos.top - container_rect.top);
                        needs_adjustment = true;

                        // Special handling if new height would be smaller than minimum
                        if new_height < min_height {
                            // Move top edge to center while keeping bottom edge at container boundary
                            new_pos.top = container_rect.top + container_height - min_height;
                            new_pos.bottom = min_height;
                        }
                    }

                    // Adjust top edge if needed
                    if new_pos.top < container_rect.top {
                        let new_height = window_height + (new_pos.top - container_rect.top);
                        
                        // Always adjust the top edge to container boundary
                        new_pos.top = container_rect.top;
                        new_pos.bottom = new_height;
                        needs_adjustment = true;

                        // Special handling if new height would be smaller than minimum
                        if new_height < min_height {
                            // Move bottom edge to center while keeping top edge at container boundary
                            new_pos.bottom = min_height;
                            new_pos.top = container_rect.top;
                        }
                    }

                    if needs_adjustment {
                        window.set_position(&new_pos, false)?;
                    }
                }
                window.restore();
            }
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
            NonZeroUsize::new(self.windows().len()).ok_or_else(|| {
                anyhow!("there must be at least one window to calculate a container layout")
            })?,
            None, // containers don't have internal padding
            self.layout_flip(),
            self.resize_dimensions(),
        );

        // Apply layouts to windows
        for (i, window) in self.windows().iter().enumerate() {
            if let Some(layout) = layouts.get_mut(i) {
                if should_have_stackbar && !self.monocle() {
                    if let Some(focused_window) = self.focused_window() {
                        if focused_window.hwnd == window.hwnd {
                            layout.top += *tab_height;
                            layout.bottom -= *tab_height;
                        }
                    }
                }
                window.set_position(layout, false)?;
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
