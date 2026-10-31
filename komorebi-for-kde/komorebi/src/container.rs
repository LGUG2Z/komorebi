//! [`Container`], the windows that share one tile of a workspace's layout.
//!
//! The container itself is [`komorebi_core::Container`], shared with the other komorebi daemons;
//! here it is the one over KDE's [`Window`], a handle that is the compositor's id.
//!
//! A container with more than one window is a stack: the focused window is the one in front, and
//! the others are hidden ([`Window::hide`]) until they are given the tile
//! ([`Container::load_focused_window`](komorebi_core::Container::load_focused_window)).

use crate::window::Window;
use color_eyre::eyre;
use komorebi_core::ContainerWindow;
use komorebi_core::WorkspaceWindow;

/// The windows that share one tile of a workspace's layout
pub type Container = komorebi_core::Container<Window>;

impl ContainerWindow for Window {
    type Id = u32;
    type HideSpot = ();

    fn id(&self) -> u32 {
        self.id
    }

    fn exe_name(&self) -> Option<String> {
        self.exe()
    }

    fn hide_window(&self, _spot: ()) -> eyre::Result<()> {
        self.hide();
        Ok(())
    }

    fn restore_window(&self) -> eyre::Result<()> {
        self.restore();
        Ok(())
    }

    fn hide_stacked(&self, _spot: ()) -> eyre::Result<()> {
        self.hide();
        Ok(())
    }

    fn restore_stacked(&self) -> eyre::Result<()> {
        self.restore();
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::backend::fake::FakeBackend;
    use crate::backend::with_backend;
    use komorebi_core::Lockable;
    use serde_json;
    use std::sync::Arc;

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

        let _ = container.add_window(Window::from(5), ());
        let _ = container.add_window(Window::from(9), ());
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
            let _ = container.add_window(Window::from(id), ());
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

            let _ = container.load_focused_window(());

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

            let _ = container.hide(Some(709), ());
            assert!(fake.is_hidden(707) && fake.is_hidden(708) && !fake.is_hidden(709));

            let _ = container.hide(None, ());
            assert!(fake.is_hidden(709));

            let _ = container.restore();
            assert!(fake.is_hidden(707) && !fake.is_hidden(708) && fake.is_hidden(709));
        });
    }
}
