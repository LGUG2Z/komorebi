//! [`Container`], the windows that share one tile of a workspace's layout.
//!
//! A container is a ring of windows. The layout gives each container a tile, and every window of
//! the container is put on that tile. A new window gets a container of its own
//! ([`Workspace::new_container_for_window`](crate::workspace::Workspace::new_container_for_window)).
//!
//! A container with more than one window is a stack: the focused window is the one in front, and
//! the others are hidden ([`Window::hide`]) until they are given the tile
//! ([`Container::load_focused_window`]).
//!
//! A container can be locked ([`Lockable`]): a locked container keeps its index in the workspace
//! while others are inserted, removed or swapped around it.

use crate::lockable_sequence::Lockable;
use crate::ring::Ring;
use crate::window::Window;
use nanoid::nanoid;
use serde::Deserialize;
use serde::Serialize;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
pub struct Container {
    pub id: String,
    pub locked: bool,
    pub windows: Ring<Window>,
}

impl_ring_elements!(Container, Window);

impl Default for Container {
    fn default() -> Self {
        Self {
            id: nanoid!(),
            locked: false,
            windows: Ring::default(),
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
    /// Returns the empty container that holds the place a preselection reserved
    /// ([`Workspace::preselect_container_idx`](crate::workspace::Workspace::preselect_container_idx))
    pub fn preselect() -> Self {
        Self {
            id: "PRESELECT".to_string(),
            locked: false,
            windows: Default::default(),
        }
    }

    /// Returns true if and only if this is the container of a preselection
    pub fn is_preselect(&self) -> bool {
        self.id == "PRESELECT"
    }

    /// Hides every window of the container but `omit`, or every window when it is `None`
    ///
    pub fn hide(&self, omit: Option<u32>) {
        for window in self.windows().iter().rev() {
            let mut should_hide = omit.is_none();

            if !should_hide
                && let Some(omit) = omit
                && omit != window.id
            {
                should_hide = true
            }

            if should_hide {
                window.hide();
            }
        }
    }

    /// Restores the focused window of the container, which is the one that is in front when the
    /// container is a stack
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
                window.restore();
            } else {
                window.hide();
            }
        }
    }

    /// The index of the first window of the executable `exe` in this container
    pub fn idx_from_exe(&self, exe: &str) -> Option<usize> {
        for (idx, window) in self.windows().iter().enumerate() {
            if let Some(window_exe) = window.exe()
                && exe == window_exe
            {
                return Option::from(idx);
            }
        }

        None
    }

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

    pub fn remove_window_by_idx(&mut self, idx: usize) -> Option<Window> {
        let window = self.windows_mut().remove(idx);
        self.focus_window(idx.saturating_sub(1));
        window
    }

    pub fn remove_focused_window(&mut self) -> Option<Window> {
        let focused_idx = self.focused_window_idx();
        self.remove_window_by_idx(focused_idx)
    }

    /// Puts a window at the back of the container and gives it the front, hiding every other
    /// window of the container ([`Window::hide`])
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
    use crate::backend::fake::FakeBackend;
    use crate::backend::with_backend;
    use serde_json;
    use std::sync::Arc;

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

    #[test]
    fn the_json_of_a_container_is_its_id_whether_it_is_locked_and_the_ring_of_windows() {
        let mut container = Container {
            id: "c1".to_string(),
            locked: true,
            windows: Default::default(),
        };

        assert_eq!(
            serde_json::to_string(&container).unwrap(),
            r#"{"id":"c1","locked":true,"windows":{"elements":[],"focused":0}}"#
        );

        container.add_window(Window::from(5));
        container.add_window(Window::from(9));
        container.focus_window(0);

        let json = serde_json::to_string(&container).unwrap();
        assert!(json.starts_with(r#"{"id":"c1","locked":true,"windows":{"elements":[{"id":5,"#));
        assert!(json.contains(r#"},{"id":9,"#));
        assert!(json.ends_with(r#"}],"focused":0}}"#));
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

    /// A container of the windows 701.. on a fake desktop, with the window at `focus` in front
    fn stack_of(
        fake: &Arc<FakeBackend>,
        ids: std::ops::RangeInclusive<u32>,
        focus: usize,
    ) -> Container {
        let mut container = Container::default();
        for id in ids {
            fake.open_window(id, "foot", "foot", &format!("window {id}"));
            container.add_window(Window::from(id));
        }
        container.focus_window(focus);
        container
    }

    #[test]
    fn a_window_added_to_a_container_is_in_front_and_the_others_are_hidden() {
        let fake = Arc::new(FakeBackend::with_one_monitor());

        with_backend(fake.clone(), || {
            let container = stack_of(&fake, 701..=703, 2);

            assert_eq!(container.focused_window().map(|w| w.id), Some(703));
            assert!(fake.is_hidden(701) && fake.is_hidden(702));
            assert!(!fake.is_hidden(703));
        });
    }

    #[test]
    fn loading_the_focused_window_shows_it_and_hides_the_rest() {
        let fake = Arc::new(FakeBackend::with_one_monitor());

        with_backend(fake.clone(), || {
            let mut container = stack_of(&fake, 704..=706, 0);

            container.load_focused_window();

            assert!(!fake.is_hidden(704));
            assert!(fake.is_hidden(705) && fake.is_hidden(706));
        });
    }

    #[test]
    fn hiding_a_container_leaves_out_the_window_it_is_told_to_and_restoring_shows_the_focused_one()
    {
        let fake = Arc::new(FakeBackend::with_one_monitor());

        with_backend(fake.clone(), || {
            let container = stack_of(&fake, 707..=709, 1);

            container.hide(Some(709));
            assert!(fake.is_hidden(707) && fake.is_hidden(708) && !fake.is_hidden(709));

            container.hide(None);
            assert!(fake.is_hidden(709));

            container.restore();
            assert!(fake.is_hidden(707) && !fake.is_hidden(708) && fake.is_hidden(709));
        });
    }
}
