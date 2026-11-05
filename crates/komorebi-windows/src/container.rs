//! [`Container`], the windows that share one tile of a workspace's layout.
//!
//! The container itself is [`komorebi_core::Container`], shared with the other komorebi daemons;
//! here it is the one over the Windows [`Window`], a handle that is an `HWND`.

use crate::window::Window;
use crate::windows_backend::backend;
use color_eyre::eyre;
use komorebi_backend::WindowId;
use komorebi_core::ContainerWindow;
use komorebi_core::WorkspaceWindow;

/// The windows that share one tile of a workspace's layout
pub type Container = komorebi_core::Container<Window>;

impl ContainerWindow for Window {
    type Id = isize;
    type HideSpot = ();

    fn id(&self) -> isize {
        self.hwnd
    }

    fn exe_name(&self) -> Option<String> {
        Window::exe(*self)
    }

    fn hide_window(&self, _spot: ()) -> eyre::Result<()> {
        let _ = backend().hide(WindowId::from_hwnd(self.hwnd));
        Ok(())
    }

    fn restore_window(&self) -> eyre::Result<()> {
        let _ = backend().restore(WindowId::from_hwnd(self.hwnd));
        Ok(())
    }

    fn hide_stacked(&self, _spot: ()) -> eyre::Result<()> {
        self.hide_with_border(false);
        Ok(())
    }

    fn restore_stacked(&self) -> eyre::Result<()> {
        self.restore_with_border(false);
        Ok(())
    }
}

impl WorkspaceWindow for Window {
    fn title(&self) -> Option<String> {
        Window::title(*self)
    }

    fn exe(&self) -> Option<String> {
        Window::exe(*self)
    }

    fn class(&self) -> Option<String> {
        Window::class(*self)
    }

    fn path(&self) -> Option<String> {
        Window::path(*self)
    }
}

/// What only the Windows container has
pub trait ContainerExt {
    /// The handle of the first window of the executable `exe` in this container
    fn hwnd_from_exe(&self, exe: &str) -> Option<isize>;
}

impl ContainerExt for Container {
    fn hwnd_from_exe(&self, exe: &str) -> Option<isize> {
        for window in self.windows() {
            if let Some(window_exe) = window.exe()
                && exe == window_exe
            {
                return Option::from(window.hwnd);
            }
        }

        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Lockable;
    use serde_json;

    #[test]
    fn deserializes_with_missing_locked_field_defaults_to_false() {
        let json = r#"{
            "id": "test-1",
            "windows": { "elements": [], "focused": 0 }
        }"#;
        let container: Container = serde_json::from_str(json).expect("Should deserialize");

        assert!(!container.locked);
        assert_eq!(container.id, "test-1");
        assert!(container.windows().is_empty());

        let json = r#"{
            "id": "test-2",
            "windows": { "elements": [ { "hwnd": 5 }, { "hwnd": 9 } ], "focused": 1 }
        }"#;
        let container: Container = serde_json::from_str(json).unwrap();
        assert_eq!(container.id, "test-2");
        assert!(!container.locked);
        assert_eq!(container.windows(), &[Window::from(5), Window::from(9)]);
        assert_eq!(container.focused_window_idx(), 1);
    }

    #[test]
    fn the_json_of_a_container_is_its_id_whether_it_is_locked_and_the_ring_of_windows() {
        let container = Container {
            id: "c1".to_string(),
            locked: true,
            windows: Default::default(),
        };

        assert_eq!(
            serde_json::to_string(&container).unwrap(),
            r#"{"id":"c1","locked":true,"windows":{"elements":[],"focused":0}}"#
        );
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
