//! [`Container`], the windows that share one tile of a workspace's layout.
//!
//! The container is [`komorebi_core::Container`], shared with the other komorebi daemons, over the
//! macOS [`Window`]. What it cannot share is how a window is hidden: macOS needs the monitor's
//! `WindowHidingPosition` (and for a whole container the bounds of its display) with every hide,
//! and a hide can fail. Those four operations are [`ContainerExt`], under names the shared
//! container does not use (`add_window_at`, `load_focused_window_at`, `hide_on`,
//! `restore_focused_window`), because an inherent method of the shared type would hide an
//! extension method of the same name.

use crate::accessibility::error::AccessibilityError;
use crate::core::WindowHidingPosition;
use crate::window::Window;
use color_eyre::eyre;
use komorebi_core::ContainerWindow;
use objc2_core_foundation::CGRect;

/// The windows that share one tile of a workspace's layout
pub type Container = komorebi_core::Container<Window>;

/// Where the macOS daemon hides a window: the corner of the monitor and, when it is known, the
/// bounds of the display of the monitor (a window that hangs over the edge of a display is not
/// always on the display that a lookup by its position finds).
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct MacosHideSpot {
    /// The corner the window is parked in
    pub position: WindowHidingPosition,
    /// The bounds of the display, or `None` to look the display up by the window's position
    pub screen: Option<CGRect>,
}

impl ContainerWindow for Window {
    type Id = u32;
    type HideSpot = MacosHideSpot;

    fn id(&self) -> u32 {
        self.id
    }

    fn exe_name(&self) -> Option<String> {
        self.exe()
    }

    fn hide_window(&self, spot: MacosHideSpot) -> eyre::Result<()> {
        // hiding moves the window through Accessibility, which a unit test does not have
        if cfg!(test) {
            tracing::info!(
                "not hiding window {} to {} during test execution",
                self.id,
                spot.position
            );

            return Ok(());
        }

        Ok(self.hide_on(spot.position, spot.screen)?)
    }

    fn restore_window(&self) -> eyre::Result<()> {
        if cfg!(test) {
            tracing::info!("not restoring window {} during test execution", self.id);

            return Ok(());
        }

        Ok(self.restore()?)
    }

    fn hide_stacked(&self, spot: MacosHideSpot) -> eyre::Result<()> {
        self.hide_window(spot)
    }

    fn restore_stacked(&self) -> eyre::Result<()> {
        self.restore_window()
    }
}

/// What a macOS container does that the shared one cannot: hide and restore its windows with the
/// monitor's hiding position, and say when that failed.
pub trait ContainerExt {
    /// Puts a window at the back of the container and gives it the front, hiding every other
    /// window of the container at `hiding_position`
    fn add_window_at(
        &mut self,
        window: &Window,
        hiding_position: WindowHidingPosition,
    ) -> eyre::Result<()>;

    /// Hides the unfocused windows of the container and restores the focused one. This function
    /// is used to make sure we update the window that should be shown on a stack. If the container
    /// isn't a stack this function won't change anything.
    fn load_focused_window_at(
        &mut self,
        hiding_position: WindowHidingPosition,
    ) -> Result<(), AccessibilityError>;

    /// Hide the windows of the container, other than `omit`, on `screen`, the bounds of the display
    /// of the monitor that has the container, or on whichever display they are found on if that is
    /// not given
    fn hide_on(
        &mut self,
        hiding_position: WindowHidingPosition,
        omit: Option<u32>,
        screen: Option<CGRect>,
    ) -> eyre::Result<()>;

    /// Restores the window in front, which is the one that is shown when the container is a stack
    fn restore_focused_window(&mut self) -> eyre::Result<()>;
}

impl ContainerExt for Container {
    fn add_window_at(
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

    fn load_focused_window_at(
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

    fn hide_on(
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

    fn restore_focused_window(&mut self) -> eyre::Result<()> {
        if let Some(window) = self.focused_window_mut() {
            window.restore()?;
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use komorebi_collections::Lockable;
    use serde_json;

    #[test]
    fn test_contains_window() {
        let mut container = Container::default();

        for i in 0..3 {
            container
                .add_window_at(&Window::from(i), WindowHidingPosition::BottomLeft)
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
                .add_window_at(&Window::from(i), WindowHidingPosition::BottomLeft)
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
                .add_window_at(&Window::from(i), WindowHidingPosition::BottomLeft)
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
            .add_window_at(&Window::from(1), WindowHidingPosition::BottomLeft)
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
                .add_window_at(&Window::from(i), WindowHidingPosition::BottomLeft)
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
                .add_window_at(&Window::from(i), WindowHidingPosition::BottomLeft)
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

#[cfg(test)]
mod state_pin {
    use super::*;

    /// `komorebic state` prints a container as `{id, locked, windows: {elements, focused}}`; the
    /// shared container must keep writing exactly that for macOS.
    #[test]
    fn a_container_is_written_as_id_locked_and_windows() {
        let container = Container {
            id: "c1".to_string(),
            locked: true,
            ..Default::default()
        };

        assert_eq!(
            serde_json::to_string(&container).unwrap(),
            r#"{"id":"c1","locked":true,"windows":{"elements":[],"focused":0}}"#
        );
    }

    /// the old macOS container needed `locked` to be present; the shared one does not
    #[test]
    fn a_container_without_locked_is_read() {
        let container: Container =
            serde_json::from_str(r#"{"id":"c1","windows":{"elements":[],"focused":0}}"#).unwrap();

        assert!(!container.locked);
    }
}
