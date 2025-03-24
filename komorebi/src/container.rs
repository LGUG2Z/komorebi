use std::collections::VecDeque;

use getset::Getters;
use nanoid::nanoid;
use serde::Deserialize;
use serde::Serialize;

use crate::ring::Ring;
use crate::window::Window;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Getters)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
pub struct Container {
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

impl Container {
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
        if let Some(window) = self.focused_window() {
            window.restore();
        }
    }

    /// Hides the unfocused windows of the container and restores the focused one. This function
    /// is used to make sure we update the window that should be shown on a stack. If the container
    /// isn't a stack this function won't change anything.
    pub fn load_focused_window(&mut self) {
        let focused_idx = self.focused_window_idx();

        for (i, window) in self.windows_mut().iter_mut().enumerate() {
            if i == focused_idx {
                window.restore_with_border(false);
            } else {
                window.hide_with_border(false);
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
        for (i, window) in self.windows().iter().enumerate() {
            if window.hwnd == hwnd {
                return Option::from(i);
            }
        }

        None
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

    #[tracing::instrument(skip(self))]
    pub fn focus_window(&mut self, idx: usize) {
        tracing::info!("focusing window");
        self.windows.focus(idx);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_contains_window() {
        let mut container = Container::default();

        for i in 0..3 {
            container.add_window(Window::from(i));
        }

        // Should return true for existing windows
        assert!(container.contains_window(1));
        assert_eq!(container.idx_for_window(1), Some(1));

        // Should return false since window 4 doesn't exist
        assert!(!container.contains_window(4));
        assert_eq!(container.idx_for_window(4), None);
    }

    #[test]
    fn test_remove_window_by_idx() {
        let mut container = Container::default();

        for i in 0..3 {
            container.add_window(Window::from(i));
        }

        // Remove window 1
        container.remove_window_by_idx(1);

        // Should only have 2 windows left
        assert_eq!(container.windows().len(), 2);

        // Should return false since window 1 was removed
        assert!(!container.contains_window(1));
    }

    #[test]
    fn test_remove_focused_window() {
        let mut container = Container::default();

        for i in 0..3 {
            container.add_window(Window::from(i));
        }

        // Should be focused on the last created window
        assert_eq!(container.focused_window_idx(), 2);

        // Remove the focused window
        container.remove_focused_window();

        // Should be focused on the window before the removed one
        assert_eq!(container.focused_window_idx(), 1);

        // Should only have 2 windows left
        assert_eq!(container.windows().len(), 2);
    }

    #[test]
    fn test_add_window() {
        let mut container = Container::default();

        container.add_window(Window::from(1));

        assert_eq!(container.windows().len(), 1);
        assert_eq!(container.focused_window_idx(), 0);
        assert!(container.contains_window(1));
    }

    #[test]
    fn test_focus_window() {
        let mut container = Container::default();

        for i in 0..3 {
            container.add_window(Window::from(i));
        }

        // Should focus on the last created window
        assert_eq!(container.focused_window_idx(), 2);

        // focus on the window at index 1
        container.focus_window(1);

        // Should be focused on window 1
        assert_eq!(container.focused_window_idx(), 1);

        // focus on the window at index 0
        container.focus_window(0);

        // Should be focused on window 0
        assert_eq!(container.focused_window_idx(), 0);
    }

    #[test]
    fn test_idx_for_window() {
        let mut container = Container::default();

        for i in 0..3 {
            container.add_window(Window::from(i));
        }

        // Should return the index of the window
        assert_eq!(container.idx_for_window(1), Some(1));

        // Should return None since window 4 doesn't exist
        assert_eq!(container.idx_for_window(4), None);
    }
}
