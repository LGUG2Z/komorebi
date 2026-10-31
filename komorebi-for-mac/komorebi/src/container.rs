use crate::accessibility::error::AccessibilityError;
use crate::core::WindowHidingPosition;
use crate::lockable_sequence::Lockable;
use crate::ring::Ring;
use crate::window::Window;
use color_eyre::eyre;
use nanoid::nanoid;
use objc2_core_foundation::CGRect;
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
    pub fn preselect() -> Self {
        Self {
            id: "PRESELECT".to_string(),
            locked: false,
            windows: Default::default(),
        }
    }

    pub fn is_preselect(&self) -> bool {
        self.id == "PRESELECT"
    }

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

    pub fn remove_focused_window(&mut self) -> Option<Window> {
        let focused_idx = self.focused_window_idx();
        self.remove_window_by_idx(focused_idx)
    }

    pub fn add_window(
        &mut self,
        window: &Window,
        hiding_position: WindowHidingPosition,
    ) -> eyre::Result<()> {
        self.windows_mut().push_back(window.clone());
        self.focus_window(self.windows().len().saturating_sub(1));

        if !cfg!(test) {
            let focused_window_idx = self.focused_window_idx();

            for (i, window) in self.windows_mut().iter_mut().enumerate() {
                if i != focused_window_idx {
                    window.hide(hiding_position)?;
                }
            }
        } else {
            tracing::info!("not hiding windows to {hiding_position} during test execution")
        }

        Ok(())
    }

    #[tracing::instrument(skip(self))]
    pub fn focus_window(&mut self, idx: usize) {
        tracing::info!("focusing window");
        self.windows.focus(idx);
    }

    /// Hides the unfocused windows of the container and restores the focused one. This function
    /// is used to make sure we update the window that should be shown on a stack. If the container
    /// isn't a stack this function won't change anything.
    pub fn load_focused_window(
        &mut self,
        hiding_position: WindowHidingPosition,
    ) -> Result<(), AccessibilityError> {
        let focused_idx = self.focused_window_idx();

        if !cfg!(test) {
            for (i, window) in self.windows_mut().iter_mut().enumerate() {
                if i == focused_idx {
                    window.restore()?;
                } else {
                    window.hide(hiding_position)?;
                }
            }
        } else {
            tracing::info!(
                "not hiding/restoring windows to/from {hiding_position} during test execution"
            )
        }

        Ok(())
    }

    pub fn remove_window_by_idx(&mut self, idx: usize) -> Option<Window> {
        let window = self.windows_mut().remove(idx);
        self.focus_window(idx.saturating_sub(1));
        window
    }

    /// Hide the windows of the container, other than `omit`, on `screen`, the bounds of the display
    /// of the monitor that has the container, or on whichever display they are found on if that is
    /// not given
    pub fn hide(
        &mut self,
        hiding_position: WindowHidingPosition,
        omit: Option<u32>,
        screen: Option<CGRect>,
    ) -> eyre::Result<()> {
        for window in self.windows_mut().iter_mut().rev() {
            let mut should_hide = omit.is_none();

            if !should_hide
                && let Some(omit) = omit
                && omit != window.id
            {
                should_hide = true
            }

            // a window that has gone away must not leave the ones after it in view
            if should_hide && let Err(error) = window.hide_on(hiding_position, screen) {
                tracing::warn!("failed to hide window {}: {error}", window.id);
            }
        }

        Ok(())
    }

    pub fn restore(&mut self) -> eyre::Result<()> {
        if let Some(window) = self.focused_window_mut() {
            window.restore()?;
        }

        Ok(())
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
            container
                .add_window(&Window::from(i), WindowHidingPosition::BottomLeft)
                .unwrap();
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
            container
                .add_window(&Window::from(i), WindowHidingPosition::BottomLeft)
                .unwrap();
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
            container
                .add_window(&Window::from(i), WindowHidingPosition::BottomLeft)
                .unwrap();
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

        container
            .add_window(&Window::from(1), WindowHidingPosition::BottomLeft)
            .unwrap();

        assert_eq!(container.windows().len(), 1);
        assert_eq!(container.focused_window_idx(), 0);
        assert!(container.contains_window(1));
    }

    #[test]
    fn test_focus_window() {
        let mut container = Container::default();

        for i in 0..3 {
            container
                .add_window(&Window::from(i), WindowHidingPosition::BottomLeft)
                .unwrap();
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
            container
                .add_window(&Window::from(i), WindowHidingPosition::BottomLeft)
                .unwrap();
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
