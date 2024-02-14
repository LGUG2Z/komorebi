use std::collections::VecDeque;

use getset::Getters;
use nanoid::nanoid;
use schemars::JsonSchema;
use serde::Serialize;

use crate::ring::Ring;
use crate::window::Window;

#[derive(Debug, Clone, Serialize, Getters, JsonSchema)]
pub struct Container {
    #[serde(skip_serializing)]
    #[getset(get = "pub")]
    id: String,
    windows: Ring<Window>,
}

impl_ring_elements!(Container, Window);

impl Default for Container {
    fn default() -> Self {
        Self {
            id: nanoid!(),
            windows: Ring::default(),
        }
    }
}

impl PartialEq for Container {
    fn eq(&self, other: &Self) -> bool {
        self.id == other.id
    }
}

impl Container {
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

        if idx != 0 {
            self.focus_window(idx - 1);
        };

        window
    }

    pub fn remove_focused_window(&mut self) -> Option<Window> {
        let focused_idx = self.focused_window_idx();
        self.remove_window_by_idx(focused_idx)
    }

    pub fn add_window(&mut self, window: Window) {
        self.windows_mut().push_back(window);
        self.focus_window(self.windows().len() - 1);
    }

    #[tracing::instrument(skip(self))]
    pub fn focus_window(&mut self, idx: usize) {
        tracing::info!("focusing window");
        self.windows.focus(idx);
    }

    pub fn categories(&self) -> Vec<String> {
        let mut categories = Vec::new();
        for window in self.windows() {
            if let Some(category) = window.category() {
                print!("\n\ncategory {}\n\n", category);
                categories.push(category);
            }
        }

        categories
    }
}
