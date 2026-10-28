//! Keeping komorebi's idea of where each window is in step with where the compositor has it, when
//! monitors come and go.
//!
//! komorebi decides which monitor and workspace a window belongs to, and the compositor shows it
//! there. When an output is unplugged the compositor takes the windows on it to the ones that are
//! left. The window is then on one monitor and komorebi has it on another, and what komorebi has is
//! wrong: its tile is on a screen that is gone, and the window sits wherever it landed, untiled.
//!
//! [`WindowManager::evacuate_monitor`] puts this right. It moves what was on a monitor that is gone
//! to the monitors that are left. The monitor reconciliator calls it for each monitor that went
//! ([`reconcile_display_change`]).
//!
//! A window is on the monitor whose area has the middle of the window's frame.
//!
//! # What is guaranteed, and by what
//!
//! | Rule | Enforced by | Proof |
//! |---|---|---|
//! | The windows of a monitor that is gone are tiled on the monitors that are left | [`WindowManager::evacuate_monitor`], called by the monitor reconciliator | `tests/hotplug.rs::the_windows_of_a_monitor_that_goes_are_tiled_on_the_one_that_is_left` |
//! | A stack moves as a stack | [`WindowManager::evacuate_monitor`] moves whole containers | `tests/hotplug.rs::a_stack_goes_with_the_monitor_as_a_stack` |
//! | A monocle or a maximized window of a monitor that is gone is not lost | [`WindowManager::evacuate_monitor`] takes both out of the workspace and puts each in a container | `tests/hotplug.rs::a_monocle_of_a_monitor_that_goes_comes_back_as_a_container_with_the_windows_it_hid`, `tests/hotplug.rs::a_maximized_window_of_a_monitor_that_goes_is_not_lost` |
//!
//! [`reconcile_display_change`]: crate::monitor_reconciliator::reconcile_display_change

#![deny(missing_docs)]

use crate::core::Rect;
use crate::window::Window;
use crate::window_manager::WindowManager;
use color_eyre::eyre;
use color_eyre::eyre::OptionExt;

/// Returns true if and only if the point is in the area, the left and top edges included and the
/// right and bottom ones not (a [`Rect`]'s `right` and `bottom` are its width and height).
fn contains(area: &Rect, (x, y): (i32, i32)) -> bool {
    x >= area.left && x < area.left + area.right && y >= area.top && y < area.top + area.bottom
}

/// Returns the middle of a rect.
fn middle(rect: &Rect) -> (i32, i32) {
    (rect.left + rect.right / 2, rect.top + rect.bottom / 2)
}

impl WindowManager {
    /// Returns the index of the monitor whose area has the middle of this rect, if any.
    fn monitor_idx_holding(&self, rect: &Rect) -> Option<usize> {
        let point = middle(rect);
        self.monitors()
            .iter()
            .position(|monitor| contains(&monitor.size, point))
    }

    /// Moves what was on the monitor with this serial number id to the monitors that are left,
    /// because the compositor has taken its windows there and they have nowhere else to be tiled.
    ///
    /// Each container and floating window goes to the workspace of the same number on the monitor
    /// that now has it, or on the first monitor that is left when that is not clear yet. A monocle
    /// and a maximized window come back as ordinary containers. The monitor itself stays in the
    /// list, empty, for the caller to remove. Nothing is done when there is no such monitor, or no
    /// other.
    ///
    /// # Errors
    ///
    /// Returns an error if a monitor or workspace it moves windows between is not there, or if a
    /// maximized window cannot be put in a container.
    ///
    /// Covered by `tests/hotplug.rs::a_stack_goes_with_the_monitor_as_a_stack` and
    /// `tests/hotplug.rs::the_windows_of_a_monitor_that_goes_are_tiled_on_the_one_that_is_left`.
    pub fn evacuate_monitor(&mut self, serial_number_id: &str) -> eyre::Result<()> {
        let Some(gone) = self
            .monitors()
            .iter()
            .position(|m| m.serial_number_id == serial_number_id)
        else {
            return Ok(());
        };

        let remaining: Vec<usize> = (0..self.monitors().len()).filter(|i| *i != gone).collect();
        let Some(first) = remaining.first().copied() else {
            return Ok(());
        };

        // where the middle of a window is now, if it is on a monitor that is left
        let destination = |wm: &WindowManager, window: &Window| {
            window
                .rect()
                .ok()
                .and_then(|rect| wm.monitor_idx_holding(&rect))
                .filter(|idx| remaining.contains(idx))
                .unwrap_or(first)
        };

        let workspace_count = self.monitors()[gone].workspaces().len();

        for w_idx in 0..workspace_count {
            // take everything out of the workspace first, then place it
            let (containers, floating, monocle, maximized) = {
                let workspace = self
                    .monitors_mut()
                    .get_mut(gone)
                    .and_then(|m| m.workspaces_mut().get_mut(w_idx))
                    .ok_or_eyre("there is no such workspace")?;

                let containers: Vec<_> = workspace.containers_mut().drain(..).collect();
                workspace.resize_dimensions.clear();
                let floating: Vec<_> = workspace.floating_windows_mut().drain(..).collect();
                let monocle = workspace.monocle_container.take();
                workspace.monocle_container_restore_idx = None;
                let maximized = workspace.maximized_window.take();
                workspace.maximized_window_restore_idx = None;

                (containers, floating, monocle, maximized)
            };

            let place = |wm: &mut WindowManager,
                         window: Option<Window>,
                         f: Box<dyn FnOnce(&mut crate::workspace::Workspace)>|
             -> eyre::Result<()> {
                let to = window.map_or(first, |w| destination(wm, &w));
                let monitor = wm
                    .monitors_mut()
                    .get_mut(to)
                    .ok_or_eyre("there is no such monitor")?;
                monitor.ensure_workspace_count(w_idx + 1);
                let workspace = monitor
                    .workspaces_mut()
                    .get_mut(w_idx)
                    .ok_or_eyre("there is no such workspace")?;
                f(workspace);
                Ok(())
            };

            for container in containers {
                let window = container.focused_window().copied();
                place(
                    self,
                    window,
                    Box::new(move |workspace| workspace.containers_mut().push_back(container)),
                )?;
            }

            for window in floating {
                place(
                    self,
                    Some(window),
                    Box::new(move |workspace| workspace.floating_windows_mut().push_back(window)),
                )?;
            }

            // a monocle, which shows one window of its container, comes as an ordinary container
            if let Some(mut container) = monocle {
                let window = container.focused_window().copied();
                let _ = container.restore();
                place(
                    self,
                    window,
                    Box::new(move |workspace| workspace.containers_mut().push_back(container)),
                )?;
            }

            // a maximized window is a window of an ordinary container on the monitor it goes to
            if let Some(window) = maximized {
                let mut container = crate::container::Container::default();
                container.add_window(&window)?;
                place(
                    self,
                    Some(window),
                    Box::new(move |workspace| workspace.containers_mut().push_back(container)),
                )?;
            }
        }

        Ok(())
    }
}
