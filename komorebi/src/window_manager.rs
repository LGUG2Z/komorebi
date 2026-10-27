//! [`WindowManager`], everything komorebi tiles, and the operations on it.
//!
//! The window manager keeps the monitors it loaded from the backend
//! ([`WindowManager::init`]), the windows the user minimized, and an index of which monitor and
//! workspace each window it manages is on. It changes when an event comes in
//! ([`WindowManager::process_event`]), and every change ends with the focused workspace tiled
//! again ([`WindowManager::update_focused_workspace`]).

use crate::LibraryError;
use crate::container::Container;
use crate::core::Rect;
use crate::desktop;
use crate::monitor::Monitor;
use crate::ring::Ring;
use crate::window::Window;
use crate::window_manager_event::WindowManagerEvent;
use crate::workspace::Workspace;
use color_eyre::eyre;
use color_eyre::eyre::OptionExt;
use crossbeam_channel::Receiver;
use std::collections::HashMap;

#[derive(Debug)]
pub struct WindowManager {
    pub monitors: Ring<Monitor>,
    pub mouse_follows_focus: bool,
    pub work_area_offset: Option<Rect>,
    /// The events the backend has reported and the window manager has yet to handle, from
    /// [`crate::window_manager_event_listener`]
    pub incoming_events: Receiver<WindowManagerEvent>,
    pub minimized_windows: HashMap<u32, Window>,
    /// Maps each known window id to the (monitor, workspace) index pair managing it
    pub known_window_ids: HashMap<u32, (usize, usize)>,
}

impl_ring_elements!(WindowManager, Monitor);

impl WindowManager {
    /// Returns a window manager with no monitors, which handles the events that arrive on
    /// `incoming`: [`WindowManager::init`] loads the monitors from the backend
    pub fn new(incoming: Receiver<WindowManagerEvent>) -> eyre::Result<Self> {
        Ok(Self {
            monitors: Ring::default(),
            mouse_follows_focus: true,
            work_area_offset: None,
            incoming_events: incoming,
            minimized_windows: HashMap::new(),
            known_window_ids: Default::default(),
        })
    }

    /// Loads the monitors and the windows that are already open from the backend
    /// ([`desktop`])
    #[tracing::instrument(skip(self))]
    pub fn init(&mut self) -> Result<(), LibraryError> {
        tracing::info!("initializing");
        desktop::load_monitor_information(self)?;
        desktop::load_workspace_information(self)?;
        // the windows that were open already are in containers now; an event about one of them
        // (the first thing to happen may be that it closes) has to find it
        self.update_known_window_ids();
        Ok(())
    }

    #[tracing::instrument(skip(self))]
    pub fn update_focused_workspace(
        &mut self,
        follow_focus: bool,
        trigger_focus: bool,
    ) -> eyre::Result<()> {
        let offset = self.work_area_offset;
        let mouse_follows_focus = self.mouse_follows_focus;

        self.focused_monitor_mut()
            .ok_or_eyre("there is no monitor")?
            .update_focused_workspace(offset)?;

        if follow_focus
            && trigger_focus
            && let Ok(window) = self.focused_window_mut()
        {
            window.focus(mouse_follows_focus)?;
        }

        Ok(())
    }

    pub fn focused_workspace_idx(&self) -> eyre::Result<usize> {
        Ok(self
            .focused_monitor()
            .ok_or_eyre("there is no monitor")?
            .focused_workspace_idx())
    }

    pub fn focused_workspace(&self) -> eyre::Result<&Workspace> {
        self.focused_monitor()
            .ok_or_eyre("there is no monitor")?
            .focused_workspace()
            .ok_or_eyre("there is no workspace")
    }

    pub fn focused_workspace_mut(&mut self) -> eyre::Result<&mut Workspace> {
        self.focused_monitor_mut()
            .ok_or_eyre("there is no monitor")?
            .focused_workspace_mut()
            .ok_or_eyre("there is no workspace")
    }

    pub fn focused_container(&self) -> eyre::Result<&Container> {
        self.focused_workspace()?
            .focused_container()
            .ok_or_eyre("there is no container")
    }

    pub fn focused_container_idx(&self) -> eyre::Result<usize> {
        Ok(self.focused_workspace()?.focused_container_idx())
    }

    pub fn focused_container_mut(&mut self) -> eyre::Result<&mut Container> {
        self.focused_workspace_mut()?
            .focused_container_mut()
            .ok_or_eyre("there is no container")
    }

    pub fn focused_window(&self) -> eyre::Result<&Window> {
        self.focused_container()?
            .focused_window()
            .ok_or_eyre("there is no window")
    }

    fn focused_window_mut(&mut self) -> eyre::Result<&mut Window> {
        self.focused_container_mut()?
            .focused_window_mut()
            .ok_or_eyre("there is no window")
    }

    /// Takes a window the user minimized out of the focused workspace, and keeps it until it is
    /// restored
    pub fn extract_minimized_window(&mut self, window_id: u32) -> eyre::Result<()> {
        let workspace = self.focused_workspace_mut()?;
        let window = workspace.remove_window(window_id)?;
        self.minimized_windows.insert(window_id, window);

        Ok(())
    }

    /// Drop every window that no longer exists from the focused workspace and retile
    pub fn reap_invalid_windows(&mut self) -> eyre::Result<()> {
        let reaped_count = self.focused_workspace_mut()?.reap_invalid_windows()?;

        if reaped_count > 0 {
            tracing::debug!("reaped {reaped_count} invalid window(s)");
        }

        self.update_focused_workspace(false, false)
    }

    /// Tiles the focused workspace of every monitor
    #[tracing::instrument(skip(self))]
    pub fn retile_all(&mut self) -> eyre::Result<()> {
        let offset = self.work_area_offset;

        for monitor in self.monitors_mut() {
            let offset = if monitor.work_area_offset.is_some() {
                monitor.work_area_offset
            } else {
                offset
            };

            let focused_workspace_idx = monitor.focused_workspace_idx();
            monitor.update_workspace_globals(focused_workspace_idx, offset);

            let workspace = monitor
                .focused_workspace_mut()
                .ok_or_eyre("there is no workspace")?;

            workspace.update()?;
        }

        Ok(())
    }

    pub fn update_known_window_ids(&mut self) {
        tracing::trace!("updating list of known window_ids");
        let mut known_window_ids = HashMap::new();
        for (m_idx, monitor) in self.monitors().iter().enumerate() {
            for (w_idx, workspace) in monitor.workspaces().iter().enumerate() {
                for container in workspace.containers() {
                    for window in container.windows() {
                        known_window_ids.insert(window.id, (m_idx, w_idx));
                    }
                }
            }
        }

        if self.known_window_ids != known_window_ids {
            // Store new window_ids
            self.known_window_ids = known_window_ids;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::backend::fake::FakeBackend;
    use crate::backend::with_backend;
    use std::sync::Arc;

    fn setup_window_manager() -> WindowManager {
        let (_sender, receiver) = crossbeam_channel::bounded(1);

        // Create a new WindowManager instance
        let wm = WindowManager::new(receiver);

        // Window Manager should be created successfully
        assert!(wm.is_ok());

        wm.unwrap()
    }

    #[test]
    fn test_create_window_manager() {
        let _wm = setup_window_manager();
    }

    fn rect(left: i32, top: i32, right: i32, bottom: i32) -> Rect {
        Rect {
            left,
            top,
            right,
            bottom,
        }
    }

    /// The fake's 1920x1080 monitor and a second one to its right, with the windows `ids` open on
    /// the first and `id + 1` on the second for each of them
    fn two_monitors(ids: &[u32]) -> Arc<FakeBackend> {
        let fake = Arc::new(FakeBackend::with_one_monitor());
        fake.add_monitor(
            2,
            "Virtual-1",
            rect(1920, 0, 1920, 1080),
            rect(1920, 0, 1920, 1080),
        );

        for id in ids {
            fake.open_window(*id, "foot", "foot", "left");
            fake.open_window(id + 1, "foot", "foot", "right");
            fake.move_window_by_user(id + 1, rect(2000, 100, 800, 600));
        }

        fake
    }

    #[test]
    fn init_puts_each_open_window_on_the_monitor_it_is_on() -> eyre::Result<()> {
        let fake = two_monitors(&[601]);
        let mut wm = setup_window_manager();

        with_backend(fake, || wm.init())?;

        assert_eq!(wm.monitors().len(), 2);
        assert_eq!(wm.known_window_ids.get(&601), Some(&(0, 0)));
        assert_eq!(wm.known_window_ids.get(&602), Some(&(1, 0)));
        assert_eq!(wm.focused_workspace_idx()?, 0);
        assert_eq!(wm.focused_container_idx()?, 0);
        assert_eq!(wm.focused_container()?.windows().len(), 1);
        assert_eq!(wm.focused_window()?.id, 601);
        Ok(())
    }

    #[test]
    fn retile_all_tiles_the_focused_workspace_of_every_monitor() -> eyre::Result<()> {
        let fake = two_monitors(&[611]);
        let mut wm = setup_window_manager();

        with_backend(fake.clone(), || {
            wm.init()?;
            wm.retile_all()
        })?;

        // the default workspace and container padding are 5 each, and the second monitor has no
        // panel
        assert_eq!(fake.rect_of(611), Some(rect(10, 10, 1900, 1030)));
        assert_eq!(fake.rect_of(612), Some(rect(1930, 10, 1900, 1060)));
        Ok(())
    }

    #[test]
    fn reap_invalid_windows_drops_a_closed_window_and_retiles() -> eyre::Result<()> {
        let fake = Arc::new(FakeBackend::with_one_monitor());
        fake.open_window(621, "foot", "foot", "stays");
        fake.open_window(622, "foot", "foot", "closes");
        let mut wm = setup_window_manager();

        with_backend(fake.clone(), || {
            wm.init()?;
            fake.close_window_now(622);
            wm.reap_invalid_windows()
        })?;

        assert!(!wm.focused_workspace()?.contains_window(622));
        assert_eq!(fake.rect_of(621), Some(rect(10, 10, 1900, 1030)));
        Ok(())
    }

    #[test]
    fn a_minimized_window_is_kept_out_of_the_workspace() -> eyre::Result<()> {
        let fake = Arc::new(FakeBackend::with_one_monitor());
        fake.open_window(631, "foot", "foot", "minimized");
        let mut wm = setup_window_manager();

        with_backend(fake, || {
            wm.init()?;
            wm.extract_minimized_window(631)
        })?;

        assert!(wm.focused_workspace()?.is_empty());
        assert_eq!(wm.minimized_windows.get(&631), Some(&Window::from(631)));
        Ok(())
    }
}
