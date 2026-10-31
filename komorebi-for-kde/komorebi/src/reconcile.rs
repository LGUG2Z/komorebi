//! Keeping komorebi's idea of where each window is in step with where the compositor has it, when
//! monitors come and go and when something else moves windows from one monitor to another.
//!
//! komorebi decides which monitor and workspace a window belongs to, and the compositor shows it
//! there. Two things move a window from under komorebi: the user (a shortcut that sends a window to
//! the next screen) and the compositor (when an output is unplugged it takes the windows on it to
//! the ones that are left; when it is plugged back it may take them back). Either way the window
//! is on one monitor and komorebi has it on another, and what komorebi has is wrong: its tile is
//! on a screen that is not the window's, and the window sits wherever it landed, untiled.
//!
//! Three methods of [`WindowManager`] put this right:
//!
//! - [`WindowManager::follow_windows_to_their_monitors`] moves every container and floating window
//!   to the monitor it is on. The monitor reconciliator calls it a moment after the monitors
//!   change.
//! - [`WindowManager::follow_window_to_its_monitor`] does the same when the compositor reports that
//!   a window moved to another output.
//! - [`WindowManager::evacuate_monitor`] moves what was on a monitor that is gone to the monitors
//!   that are left.
//!
//! A window is on the monitor whose area has the middle of the window's frame.
//!
//! # What is guaranteed, and by what
//!
//! | Rule | Enforced by | Proof |
//! |---|---|---|
//! | A window moved to another output is tiled on that monitor, on the workspace of the same number | [`WindowManager::follow_windows_to_their_monitors`] | `tests/follow_output.rs::a_window_sent_to_another_screen_by_a_shortcut_is_tiled_there` |
//! | The windows of a monitor that is gone are tiled on the monitors that are left | [`WindowManager::evacuate_monitor`], called by the monitor reconciliator | `tests/follow_output.rs::the_windows_of_a_monitor_that_goes_are_tiled_on_the_one_that_is_left_and_come_back_with_it` |
//! | A stack moves as a stack | [`WindowManager::follow_windows_to_their_monitors`] and [`WindowManager::evacuate_monitor`] move whole containers | `tests/hotplug.rs::a_stack_goes_with_the_monitor_as_a_stack` |
//! | Nothing is moved while the user drags a window: the drag decides where that window goes | [`WindowManager::follow_windows_to_their_monitors`] returns at once | `tests/follow_output.rs::nothing_is_followed_while_the_user_drags_a_window` |
//! | A monocle of a monitor that is gone comes back as a container, and a maximized window stays maximized | [`WindowManager::evacuate_monitor`] takes both out of the workspace: the monocle is put in a container, the maximized window becomes the maximized window of the workspace it goes to ([`Workspace::adopt_maximized_window`]) | `tests/hotplug.rs::a_monocle_of_a_monitor_that_goes_comes_back_as_a_container_with_the_windows_it_hid`, `tests/hotplug.rs::a_maximized_window_of_a_monitor_that_goes_is_not_lost`, `tests/hotplug.rs::a_maximized_window_of_a_monitor_that_goes_stays_maximized`, `tests/hotplug.rs::a_maximized_window_that_comes_to_a_workspace_with_one_takes_its_place` |
//!
//! [`reconcile_display_change`]: crate::monitor_reconciliator::reconcile_display_change
//! [`Workspace::adopt_maximized_window`]: crate::workspace::Workspace::adopt_maximized_window

#![deny(missing_docs)]

use crate::workspace::WorkspaceExt;
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

    /// Returns true if and only if a move or a resize by the user is going on.
    fn is_dragging(&self) -> bool {
        self.pending_move_op.is_some() || self.pending_resize_op.is_some()
    }

    /// Puts every container and floating window komorebi has on the monitor it is on, and returns
    /// true if and only if anything moved.
    ///
    /// A container is on the monitor of its focused window. Whole containers move, to the workspace
    /// of the same number, which is created if the monitor has fewer, at the end of its tiles. The
    /// monitors that windows moved between are tiled again. Nothing moves while the user is
    /// dragging a window.
    ///
    /// # Errors
    ///
    /// Returns an error if a monitor, workspace, container or window that was found misplaced is
    /// not there to move, or if a monitor that changed cannot be tiled again.
    ///
    /// Covered by `tests/follow_output.rs::a_window_sent_to_another_screen_by_a_shortcut_is_tiled_there`,
    /// `tests/follow_output.rs::nothing_is_followed_while_the_user_drags_a_window` and the tests of
    /// [`WindowManager::follow_window_to_its_monitor`].
    pub fn follow_windows_to_their_monitors(&mut self) -> eyre::Result<bool> {
        if self.is_dragging() {
            return Ok(false);
        }

        let mut touched: Vec<usize> = vec![];

        // each pass moves one container or window, and the indexes of the rest shift with it
        for _ in 0..10_000 {
            let Some(misplaced) = self.find_misplaced() else {
                break;
            };

            let Misplaced {
                monitor: from_monitor,
                workspace: workspace_idx,
                slot,
                to_monitor,
            } = misplaced;

            tracing::info!(
                "window(s) of workspace {workspace_idx} are on monitor {to_monitor}, not on {from_monitor}: following them"
            );

            let from = self
                .monitors_mut()
                .get_mut(from_monitor)
                .and_then(|m| m.workspaces_mut().get_mut(workspace_idx))
                .ok_or_eyre("there is no such workspace")?;

            match slot {
                Slot::Container(idx) => {
                    let container = from
                        .remove_container(idx)
                        .ok_or_eyre("there is no such container")?;

                    let to = self
                        .monitors_mut()
                        .get_mut(to_monitor)
                        .ok_or_eyre("there is no such monitor")?;
                    to.ensure_workspace_count(workspace_idx + 1);
                    to.workspaces_mut()
                        .get_mut(workspace_idx)
                        .ok_or_eyre("there is no such workspace")?
                        .containers_mut()
                        .push_back(container);
                }
                Slot::Floating(id) => {
                    let window = from
                        .remove_floating_window(id)
                        .ok_or_eyre("there is no such window")?;

                    let to = self
                        .monitors_mut()
                        .get_mut(to_monitor)
                        .ok_or_eyre("there is no such monitor")?;
                    to.ensure_workspace_count(workspace_idx + 1);
                    to.workspaces_mut()
                        .get_mut(workspace_idx)
                        .ok_or_eyre("there is no such workspace")?
                        .floating_windows_mut()
                        .push_back(window);
                }
            }

            for idx in [from_monitor, to_monitor] {
                if !touched.contains(&idx) {
                    touched.push(idx);
                }
            }
        }

        if touched.is_empty() {
            return Ok(false);
        }

        self.update_known_window_ids();

        let offset = self.work_area_offset;
        for idx in touched {
            if let Some(monitor) = self.monitors_mut().get_mut(idx) {
                monitor.update_focused_workspace(offset)?;
            }
        }

        Ok(true)
    }

    /// Follows a window the compositor reports moved to another output, and returns true if and
    /// only if anything moved.
    ///
    /// Every window is looked at, not only this one: when one has moved, others may have too. A
    /// move komorebi made itself is already in its state, and finds nothing to do.
    ///
    /// # Errors
    ///
    /// The same as [`WindowManager::follow_windows_to_their_monitors`].
    ///
    /// Covered by `tests/follow_output.rs::a_window_sent_to_another_screen_by_a_shortcut_is_tiled_there`.
    pub fn follow_window_to_its_monitor(&mut self, window: Window) -> eyre::Result<bool> {
        let moved = self.follow_windows_to_their_monitors()?;
        if moved {
            tracing::debug!("window {} was on another monitor", window.id);
        }
        Ok(moved)
    }

    /// Returns the first container or floating window that is on another monitor than the one
    /// komorebi has it on.
    fn find_misplaced(&self) -> Option<Misplaced> {
        for (m_idx, monitor) in self.monitors().iter().enumerate() {
            for (w_idx, workspace) in monitor.workspaces().iter().enumerate() {
                for (c_idx, container) in workspace.containers().iter().enumerate() {
                    let Some(window) = container.focused_window() else {
                        continue;
                    };
                    if let Some(to) = window
                        .rect()
                        .ok()
                        .and_then(|rect| self.monitor_idx_holding(&rect))
                        .filter(|to| *to != m_idx)
                    {
                        return Some(Misplaced {
                            monitor: m_idx,
                            workspace: w_idx,
                            slot: Slot::Container(c_idx),
                            to_monitor: to,
                        });
                    }
                }

                for window in workspace.floating_windows() {
                    if let Some(to) = window
                        .rect()
                        .ok()
                        .and_then(|rect| self.monitor_idx_holding(&rect))
                        .filter(|to| *to != m_idx)
                    {
                        return Some(Misplaced {
                            monitor: m_idx,
                            workspace: w_idx,
                            slot: Slot::Floating(window.id),
                            to_monitor: to,
                        });
                    }
                }
            }
        }

        None
    }

    /// Moves what was on the monitor with this serial number id to the monitors that are left,
    /// because the compositor has taken its windows there and they have nowhere else to be tiled.
    ///
    /// Each container and floating window goes to the workspace of the same number on the monitor
    /// that now has it, or on the first monitor that is left when that is not clear yet. A monocle
    /// comes back as an ordinary container, with the windows it hid shown again. A maximized window
    /// stays maximized, as the maximized window of the workspace it goes to; the one that workspace
    /// had goes back to its tile. The monitor itself stays in the list, empty, for the caller to
    /// remove. Nothing is done when there is no such monitor, or no other.
    ///
    /// # Errors
    ///
    /// Returns an error if a monitor or workspace it moves windows between is not there, or if a
    /// maximized window cannot be put in a container. A maximized window that cannot be kept
    /// maximized is logged, and stays a tile.
    ///
    /// Covered by `tests/hotplug.rs::a_stack_goes_with_the_monitor_as_a_stack`,
    /// `tests/hotplug.rs::the_windows_of_a_monitor_that_goes_are_tiled_on_the_one_that_is_left`,
    /// `tests/hotplug.rs::a_maximized_window_of_a_monitor_that_goes_stays_maximized`,
    /// `tests/hotplug.rs::a_maximized_window_that_comes_to_a_workspace_with_one_takes_its_place` and
    /// `tests/follow_output.rs::the_windows_of_a_monitor_that_goes_are_tiled_on_the_one_that_is_left_and_come_back_with_it`.
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
            if let Some(container) = monocle {
                let window = container.focused_window().copied();
                let _ = container.restore();
                place(
                    self,
                    window,
                    Box::new(move |workspace| workspace.containers_mut().push_back(container)),
                )?;
            }

            // already maximized, and the compositor keeps it so on the monitor it has gone to
            if let Some(window) = maximized {
                let id = window.id;
                let mut container = crate::container::Container::default();
                container.add_window(window);
                place(
                    self,
                    Some(window),
                    Box::new(move |workspace| {
                        workspace.containers_mut().push_back(container);
                        if let Err(error) = workspace.adopt_maximized_window(id) {
                            tracing::warn!("could not keep window {id} maximized: {error}");
                        }
                    }),
                )?;
            }
        }

        Ok(())
    }
}

/// Where a misplaced container or window is in its workspace.
enum Slot {
    /// A container, by its index in the workspace.
    Container(usize),
    /// A floating window, by its id.
    Floating(u32),
}

/// A container or floating window that is on another monitor than the one komorebi has it on.
struct Misplaced {
    /// The index of the monitor komorebi has it on.
    monitor: usize,
    /// The index of its workspace, on both monitors.
    workspace: usize,
    /// Where it is in the workspace.
    slot: Slot,
    /// The index of the monitor it is on.
    to_monitor: usize,
}
