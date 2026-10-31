//! [`Container`], the windows that share one tile of a workspace's layout.
//!
//! A container is a ring of windows. The layout gives each container a tile, and every window of
//! the container is put on that tile. A new window gets a container of its own.
//!
//! A container with more than one window is a stack: the focused window is the one in front, and
//! the others are hidden ([`ContainerWindow::hide_window`]) until they are given the tile
//! ([`Container::load_focused_window`]).
//!
//! A container can be locked ([`Lockable`]): a locked container keeps its index in the workspace
//! while others are inserted, removed or swapped around it.

use komorebi_collections::Lockable;
use komorebi_collections::ring::Ring;
use nanoid::nanoid;
use color_eyre::eyre;
use serde::Deserialize;
use serde::Serialize;
use std::collections::VecDeque;

/// What a container needs of a platform's window.
///
/// The window is a handle that is cheap to clone (Windows: an `HWND`; KDE: the compositor's id; macOS: an
/// Accessibility element). Showing and
/// hiding it is the platform's business, so the container calls these four operations and nothing
/// else. They say whether they worked (`Ok` always, on KDE and Windows, where a window that cannot
/// be hidden is left where it is), and hiding takes the [`ContainerWindow::HideSpot`].
pub trait ContainerWindow: Clone + PartialEq + std::fmt::Debug {
    /// What names a window to the rest of the daemon: `isize` on Windows, `u32` on KDE and macOS.
    type Id: Copy + PartialEq;

    /// Where a window is put when it is hidden, for a platform that hides by moving the window
    /// somewhere: on macOS the corner of the monitor and the bounds of its display. `()` where
    /// hiding takes no argument (KDE and Windows). The workspace asks the platform for it with
    /// [`WorkspacePlatform::hide_spot`](crate::WorkspacePlatform::hide_spot).
    type HideSpot: Copy + Default;

    /// The window's id.
    fn id(&self) -> Self::Id;

    /// The file name of the executable that owns the window, if it can be told.
    fn exe_name(&self) -> Option<String>;

    /// Takes the window out of sight, as a window that is not in front of its stack is.
    fn hide_window(&self, spot: Self::HideSpot) -> eyre::Result<()>;

    /// Brings the window back into sight.
    fn restore_window(&self) -> eyre::Result<()>;

    /// Hides the window as [`Container::load_focused_window`] does, which on Windows leaves the
    /// window's border alone; elsewhere it is [`ContainerWindow::hide_window`].
    fn hide_stacked(&self, spot: Self::HideSpot) -> eyre::Result<()>;

    /// Restores the window as [`Container::load_focused_window`] does, which on Windows leaves the
    /// window's border alone; elsewhere it is [`ContainerWindow::restore_window`].
    fn restore_stacked(&self) -> eyre::Result<()>;
}

/// What a workspace asks of a platform window, besides what a container asks.
///
/// The four lookups answer from what the platform knows about the window and are `None` when it
/// cannot say (the window is gone, or the platform has no source for the field).
pub trait WorkspaceWindow: ContainerWindow {
    /// The window's title.
    fn title(&self) -> Option<String>;

    /// The file name of the executable that owns the window.
    fn exe(&self) -> Option<String>;

    /// The window class (the `app_id` on KDE, the Win32 class on Windows).
    fn class(&self) -> Option<String>;

    /// The full path of the executable that owns the window.
    fn path(&self) -> Option<String>;
}

/// A ring of windows that share one tile of a workspace's layout.
///
/// The serialised form is `{"id", "locked", "windows": {"elements", "focused"}}`, which is what
/// `komorebic state` prints for a container whatever the platform; `locked` may be missing when it
/// is read.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[cfg_attr(feature = "schemars", schemars(rename = "Container"))]
pub struct Container<W> {
    /// The container's id, `PRESELECT` for the place a preselection reserved.
    pub id: String,
    /// Whether the container keeps its index while others are inserted, removed or swapped.
    #[serde(default)]
    pub locked: bool,
    /// The windows, with the one in front focused.
    pub windows: Ring<W>,
}

impl<W> Container<W> {
    /// The windows of the container.
    pub const fn windows(&self) -> &VecDeque<W> {
        self.windows.elements()
    }

    /// The windows of the container, mutably.
    pub fn windows_mut(&mut self) -> &mut VecDeque<W> {
        self.windows.elements_mut()
    }

    /// The window in front, if there is one.
    pub fn focused_window(&self) -> Option<&W> {
        self.windows.focused()
    }

    /// The index of the window in front.
    pub const fn focused_window_idx(&self) -> usize {
        self.windows.focused_idx()
    }

    /// The window in front, mutably.
    pub fn focused_window_mut(&mut self) -> Option<&mut W> {
        self.windows.focused_mut()
    }
}

impl<W> Default for Container<W> {
    fn default() -> Self {
        Self {
            id: nanoid!(),
            locked: false,
            windows: Ring::default(),
        }
    }
}

impl<W> Lockable for Container<W> {
    fn locked(&self) -> bool {
        self.locked
    }

    fn set_locked(&mut self, locked: bool) -> &mut Self {
        self.locked = locked;
        self
    }
}

impl<W: ContainerWindow> Container<W> {
    /// Returns the empty container that holds the place a preselection reserved.
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

    /// Hides every window of the container but `omit`, or every window when it is `None`, at `spot`.
    /// A window that cannot be hidden is logged and the others are still hidden: a window that has
    /// gone away must not leave the ones after it in view.
    pub fn hide(&self, omit: Option<W::Id>, spot: W::HideSpot) -> eyre::Result<()> {
        for window in self.windows().iter().rev() {
            let mut should_hide = omit.is_none();

            if !should_hide
                && let Some(omit) = omit
                && omit != window.id()
            {
                should_hide = true
            }

            if should_hide && let Err(error) = window.hide_window(spot) {
                tracing::warn!("failed to hide window {:?}: {error}", window);
            }
        }

        Ok(())
    }

    /// Restores the focused window of the container, which is the one that is in front when the
    /// container is a stack
    pub fn restore(&self) -> eyre::Result<()> {
        if let Some(window) = self.focused_window() {
            window.restore_window()?;
        }

        Ok(())
    }

    /// Hides the unfocused windows of the container and restores the focused one. This function
    /// is used to make sure we update the window that should be shown on a stack. If the container
    /// isn't a stack this function won't change anything.
    pub fn load_focused_window(&mut self, spot: W::HideSpot) -> eyre::Result<()> {
        let focused_idx = self.focused_window_idx();

        for (i, window) in self.windows_mut().iter_mut().enumerate() {
            if i == focused_idx {
                window.restore_stacked()?;
            } else {
                window.hide_stacked(spot)?;
            }
        }

        Ok(())
    }

    /// The index of the first window of the executable `exe` in this container
    pub fn idx_from_exe(&self, exe: &str) -> Option<usize> {
        for (idx, window) in self.windows().iter().enumerate() {
            if let Some(window_exe) = window.exe_name()
                && exe == window_exe
            {
                return Option::from(idx);
            }
        }

        None
    }

    /// Returns true if and only if the container has the window
    pub fn contains_window(&self, window_id: W::Id) -> bool {
        for window in self.windows() {
            if window.id() == window_id {
                return true;
            }
        }

        false
    }

    /// The index of the window in the container
    pub fn idx_for_window(&self, window_id: W::Id) -> Option<usize> {
        for (i, window) in self.windows().iter().enumerate() {
            if window.id() == window_id {
                return Option::from(i);
            }
        }

        None
    }

    /// Takes a window out of the container and focuses the one before it
    pub fn remove_window_by_idx(&mut self, idx: usize) -> Option<W> {
        let window = self.windows_mut().remove(idx);
        self.focus_window(idx.saturating_sub(1));
        window
    }

    /// Takes the window in front out of the container
    pub fn remove_focused_window(&mut self) -> Option<W> {
        let focused_idx = self.focused_window_idx();
        self.remove_window_by_idx(focused_idx)
    }

    /// Puts a window at the back of the container and gives it the front, hiding every other
    /// window of the container ([`ContainerWindow::hide_window`])
    pub fn add_window(&mut self, window: W, spot: W::HideSpot) -> eyre::Result<()> {
        self.windows_mut().push_back(window);
        self.focus_window(self.windows().len().saturating_sub(1));

        let focused_window_idx = self.focused_window_idx();

        for (i, window) in self.windows().iter().enumerate() {
            if i != focused_window_idx {
                window.hide_window(spot)?;
            }
        }

        Ok(())
    }

    /// Gives the front to the window at `idx`
    #[tracing::instrument(skip(self))]
    pub fn focus_window(&mut self, idx: usize) {
        tracing::info!("focusing window");
        self.windows.focus(idx);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;

    #[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
    struct Stub(u32);

    thread_local! {
        static LOG: RefCell<Vec<String>> = const { RefCell::new(Vec::new()) };
    }

    fn log() -> Vec<String> {
        LOG.with(|log| std::mem::take(&mut *log.borrow_mut()))
    }

    /// the spot that the tests ask windows to be hidden at
    const SPOT: u8 = 7;

    impl ContainerWindow for Stub {
        type Id = u32;
        type HideSpot = u8;

        fn id(&self) -> u32 {
            self.0
        }

        fn exe_name(&self) -> Option<String> {
            Some(format!("exe{}", self.0))
        }

        fn hide_window(&self, spot: u8) -> eyre::Result<()> {
            LOG.with(|log| log.borrow_mut().push(format!("hide {} at {spot}", self.0)));
            Ok(())
        }

        fn restore_window(&self) -> eyre::Result<()> {
            LOG.with(|log| log.borrow_mut().push(format!("restore {}", self.0)));
            Ok(())
        }

        fn hide_stacked(&self, spot: u8) -> eyre::Result<()> {
            LOG.with(|log| log.borrow_mut().push(format!("hide-stacked {} at {spot}", self.0)));
            Ok(())
        }

        fn restore_stacked(&self) -> eyre::Result<()> {
            LOG.with(|log| log.borrow_mut().push(format!("restore-stacked {}", self.0)));
            Ok(())
        }
    }

    impl WorkspaceWindow for Stub {
        fn title(&self) -> Option<String> {
            Some(format!("title{}", self.0))
        }

        fn exe(&self) -> Option<String> {
            self.exe_name()
        }

        fn class(&self) -> Option<String> {
            Some("class".to_string())
        }

        fn path(&self) -> Option<String> {
            None
        }
    }

    #[test]
    fn a_workspace_window_answers_the_four_lookups() {
        let window = Stub(3);

        assert_eq!(window.title().as_deref(), Some("title3"));
        assert_eq!(WorkspaceWindow::exe(&window).as_deref(), Some("exe3"));
        assert_eq!(window.class().as_deref(), Some("class"));
        assert_eq!(window.path(), None);
    }

    fn container_of(ids: &[u32]) -> Container<Stub> {
        let mut container = Container::default();
        for id in ids {
            container.add_window(Stub(*id), SPOT).unwrap();
        }
        log();
        container
    }

    #[test]
    fn test_contains_window() {
        let container = container_of(&[0, 1, 2]);

        assert!(container.contains_window(1));
        assert_eq!(container.idx_for_window(1), Some(1));
        assert!(!container.contains_window(4));
        assert_eq!(container.idx_for_window(4), None);
    }

    #[test]
    fn test_remove_window_by_idx() {
        let mut container = container_of(&[0, 1, 2]);

        container.remove_window_by_idx(1);

        assert_eq!(container.windows().len(), 2);
        assert!(!container.contains_window(1));
    }

    #[test]
    fn test_remove_focused_window() {
        let mut container = container_of(&[0, 1, 2]);

        assert_eq!(container.focused_window_idx(), 2);
        container.remove_focused_window();
        assert_eq!(container.focused_window_idx(), 1);
        assert_eq!(container.windows().len(), 2);
    }

    #[test]
    fn test_add_window() {
        let container = container_of(&[1]);

        assert_eq!(container.windows().len(), 1);
        assert_eq!(container.focused_window_idx(), 0);
        assert!(container.contains_window(1));
    }

    #[test]
    fn test_focus_window() {
        let mut container = container_of(&[0, 1, 2]);

        assert_eq!(container.focused_window_idx(), 2);
        container.focus_window(1);
        assert_eq!(container.focused_window_idx(), 1);
        container.focus_window(0);
        assert_eq!(container.focused_window_idx(), 0);
    }

    #[test]
    fn a_window_added_is_in_front_and_the_others_are_hidden() {
        let mut container = Container::default();
        container.add_window(Stub(1), SPOT).unwrap();
        container.add_window(Stub(2), SPOT).unwrap();

        assert_eq!(container.focused_window(), Some(&Stub(2)));
        assert_eq!(log(), ["hide 1 at 7"]);
    }

    #[test]
    fn loading_the_focused_window_restores_it_and_hides_the_rest_the_stacked_way() {
        let mut container = container_of(&[1, 2, 3]);
        container.focus_window(0);

        container.load_focused_window(SPOT).unwrap();

        assert_eq!(
            log(),
            ["restore-stacked 1", "hide-stacked 2 at 7", "hide-stacked 3 at 7"]
        );
    }

    #[test]
    fn hiding_leaves_out_the_window_it_is_told_to_and_restoring_shows_the_focused_one() {
        let mut container = container_of(&[7, 8, 9]);
        container.focus_window(1);

        container.hide(Some(9), SPOT).unwrap();
        assert_eq!(log(), ["hide 8 at 7", "hide 7 at 7"]);

        container.hide(None, SPOT).unwrap();
        assert_eq!(log(), ["hide 9 at 7", "hide 8 at 7", "hide 7 at 7"]);

        container.restore().unwrap();
        assert_eq!(log(), ["restore 8"]);
    }

    #[test]
    fn the_first_window_of_an_executable_is_found_by_its_index() {
        let container = container_of(&[4, 5]);

        assert_eq!(container.idx_from_exe("exe5"), Some(1));
        assert_eq!(container.idx_from_exe("nothing"), None);
    }

    #[test]
    fn the_json_of_a_container_is_its_id_whether_it_is_locked_and_the_ring_of_windows() {
        let mut container: Container<Stub> = Container {
            id: "c1".to_string(),
            locked: true,
            windows: Ring::default(),
        };

        assert_eq!(
            serde_json::to_string(&container).unwrap(),
            r#"{"id":"c1","locked":true,"windows":{"elements":[],"focused":0}}"#
        );

        container.add_window(Stub(5), SPOT).unwrap();
        container.add_window(Stub(9), SPOT).unwrap();
        container.focus_window(0);

        assert_eq!(
            serde_json::to_string(&container).unwrap(),
            r#"{"id":"c1","locked":true,"windows":{"elements":[5,9],"focused":0}}"#
        );
    }

    #[test]
    fn locked_may_be_missing_when_a_container_is_read() {
        let container: Container<Stub> =
            serde_json::from_str(r#"{"id":"test-1","windows":{"elements":[],"focused":0}}"#)
                .unwrap();

        assert!(!container.locked);
        assert_eq!(container.id, "test-1");
        assert!(container.windows().is_empty());
    }

    #[test]
    fn a_container_is_a_preselect_by_its_id() {
        assert!(Container::<Stub>::preselect().is_preselect());
        assert!(!Container::<Stub>::default().is_preselect());
    }

    #[cfg(feature = "schemars")]
    impl schemars::JsonSchema for Stub {
        fn schema_name() -> std::borrow::Cow<'static, str> {
            "Stub".into()
        }

        fn json_schema(_: &mut schemars::SchemaGenerator) -> schemars::Schema {
            schemars::json_schema!({ "type": "integer" })
        }
    }

    #[cfg(feature = "schemars")]
    #[test]
    fn the_schema_of_a_container_is_called_container_whatever_the_window() {
        use schemars::JsonSchema;

        assert_eq!(Container::<Stub>::schema_name(), "Container");
    }
}
