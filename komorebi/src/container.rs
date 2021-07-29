use std::collections::VecDeque;

use nanoid::nanoid;

use crate::ring::Ring;
use crate::window::Window;

#[derive(Debug, Clone)]
pub struct Container {
    id: String,
    windows: Ring<Window>,
}

impl Default for Container {
    fn default() -> Self {
        Self {
            id: nanoid!(),
            windows: Ring::default(),
        }
    }
}

impl PartialEq for &Container {
    fn eq(&self, other: &Self) -> bool {
        self.id == other.id
    }
}

impl Container {
    pub fn hide(&mut self) {
        for window in self.windows_mut() {
            window.hide();
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
        self.windows_mut().remove(idx)
    }

    pub fn remove_focused_window(&mut self) -> Option<Window> {
        let focused_idx = self.focused_window_idx();
        let window = self.remove_window_by_idx(focused_idx);

        if focused_idx != 0 {
            self.focus_window(focused_idx - 1);
        }

        window
    }

    pub fn add_window(&mut self, window: Window) {
        self.windows_mut().push_back(window);
        self.focus_window(self.windows().len() - 1);
    }

    pub fn focused_window(&self) -> Option<&Window> {
        self.windows.focused()
    }

    pub const fn focused_window_idx(&self) -> usize {
        self.windows.focused_idx()
    }

    pub fn focused_window_mut(&mut self) -> Option<&mut Window> {
        self.windows.focused_mut()
    }

    pub fn focus_window(&mut self, idx: usize) {
        tracing::info!("focusing window at index: {}", idx);
        self.windows.focus(idx);
    }

    pub const fn windows(&self) -> &VecDeque<Window> {
        self.windows.elements()
    }

    pub fn windows_mut(&mut self) -> &mut VecDeque<Window> {
        self.windows.elements_mut()
    }

    pub fn visible_window_mut(&mut self) -> Option<&mut Window> {
        self.focused_window_mut()
    }

    pub const fn id(&self) -> &String {
        &self.id
    }
}
