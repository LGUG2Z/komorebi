//! [`Container`], the windows that share one tile of a workspace's layout.
//!
//! A container is a ring of windows. The layout gives each container a tile, and every window of
//! the container is put on that tile. A new window gets a container of its own
//! ([`Workspace::new_container_for_window`](crate::workspace::Workspace::new_container_for_window)).
//!
//! A container can be locked ([`Lockable`]): a locked container keeps its index in the workspace
//! while others are inserted, removed or swapped around it.

use crate::lockable_sequence::Lockable;
use crate::ring::Ring;
use crate::window::Window;
use color_eyre::eyre;
use nanoid::nanoid;
use serde::Deserialize;
use serde::Serialize;

impl_ring_elements!(Container, Window);

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
pub struct Container {
    pub id: String,
    pub locked: bool,
    pub windows: Ring<Window>,
}

impl Default for Container {
    fn default() -> Self {
        Self {
            id: nanoid!(),
            locked: false,
            windows: Default::default(),
        }
    }
}

impl Lockable for Container {
    fn locked(&self) -> bool {
        self.locked
    }

    fn set_locked(&mut self, locked: bool) -> &mut Self {
        self.locked = locked;
        self
    }
}

impl Container {
    pub fn contains_window(&self, window_id: u32) -> bool {
        for window in self.windows() {
            if window.id == window_id {
                return true;
            }
        }

        false
    }

    pub fn idx_for_window(&self, window_id: u32) -> Option<usize> {
        for (i, window) in self.windows().iter().enumerate() {
            if window.id == window_id {
                return Option::from(i);
            }
        }

        None
    }

    pub fn remove_focused_window(&mut self) -> Option<Window> {
        let focused_idx = self.focused_window_idx();
        self.remove_window_by_idx(focused_idx)
    }

    pub fn add_window(&mut self, window: &Window) -> eyre::Result<()> {
        self.windows_mut().push_back(*window);
        self.focus_window(self.windows().len().saturating_sub(1));

        Ok(())
    }

    #[tracing::instrument(skip(self))]
    pub fn focus_window(&mut self, idx: usize) {
        tracing::info!("focusing window");
        self.windows.focus(idx);
    }

    pub fn remove_window_by_idx(&mut self, idx: usize) -> Option<Window> {
        let window = self.windows_mut().remove(idx);
        self.focus_window(idx.saturating_sub(1));
        window
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json;

    #[test]
    fn test_contains_window() {
        let mut container = Container::default();

        for i in 0..3 {
            container.add_window(&Window::from(i)).unwrap();
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
            container.add_window(&Window::from(i)).unwrap();
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
            container.add_window(&Window::from(i)).unwrap();
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

        container.add_window(&Window::from(1)).unwrap();

        assert_eq!(container.windows().len(), 1);
        assert_eq!(container.focused_window_idx(), 0);
        assert!(container.contains_window(1));
    }

    #[test]
    fn test_focus_window() {
        let mut container = Container::default();

        for i in 0..3 {
            container.add_window(&Window::from(i)).unwrap();
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
            container.add_window(&Window::from(i)).unwrap();
        }

        // Should return the index of the window
        assert_eq!(container.idx_for_window(1), Some(1));

        // Should return None since window 4 doesn't exist
        assert_eq!(container.idx_for_window(4), None);
    }

    #[test]
    fn serializes_and_deserializes() {
        let mut container = Container::default();
        container.set_locked(true);

        let serialized = serde_json::to_string(&container).expect("Should serialize");
        let deserialized: Container =
            serde_json::from_str(&serialized).expect("Should deserialize");

        assert!(deserialized.locked);
        assert_eq!(deserialized.id, container.id);
    }
}
