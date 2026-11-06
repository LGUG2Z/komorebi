//! [`Container`], the windows that share one tile of a workspace's layout.
//!
//! The container is [`komorebi_core::Container`], shared with the other komorebi daemons, over the
//! macOS [`Window`]. How a window is hidden is [`MacosHideSpot`]: the monitor's `WindowHidingPosition`
//! and, for the containers of a workspace that is being hidden, the bounds of its display. The
//! workspace gives it with `Workspace::hide_spot` and every container call takes it, as on the other
//! platforms (where it is `()`).

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

impl komorebi_core::WorkspaceWindow for Window {
    fn title(&self) -> Option<String> {
        Window::title(self)
    }

    fn exe(&self) -> Option<String> {
        Window::exe(self)
    }

    /// macOS has no window class: the subrole, or the role without one (see
    /// [`crate::window::class_of`]), which is what the state shows as the class
    fn class(&self) -> Option<String> {
        Window::class(self)
    }

    fn path(&self) -> Option<String> {
        Window::path(self).map(|path| path.to_string_lossy().into_owned())
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
                .add_window(Window::from(i), Default::default())
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
                .add_window(Window::from(i), Default::default())
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
                .add_window(Window::from(i), Default::default())
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
            .add_window(Window::from(1), Default::default())
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
                .add_window(Window::from(i), Default::default())
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
                .add_window(Window::from(i), Default::default())
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
