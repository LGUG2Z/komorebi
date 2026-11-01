//! The commands that only move the focus and the containers around the shared model.
//!
//! A daemon's `process_command` is a router over its `WindowManager`, which is per platform. What a
//! command does to the monitors, workspaces and containers does not depend on the platform, so the
//! commands whose work is only that are functions here, generic over a [`CommandHost`]: the few
//! operations of the window manager they build on (focus a monitor, show a workspace, move a
//! container). Each daemon implements the host with its own window manager, and the arm of its router
//! calls the function; a test implements it with a recording stub and checks the calls, which is how a
//! command's logic is tested without a Windows or macOS machine.

use crate::Monitor;
use crate::MonitorPlatform;
use color_eyre::eyre;
use color_eyre::eyre::OptionExt;
use komorebi_layouts::CycleDirection;
use komorebi_layouts::OperationDirection;
use std::collections::VecDeque;
use std::num::NonZeroUsize;

/// The window manager as the shared commands see it.
pub trait CommandHost {
    /// The platform whose monitors the host has.
    type P: MonitorPlatform;

    /// The monitors.
    fn monitors(&self) -> &VecDeque<Monitor<Self::P>>;

    /// The monitor that has the focus, if there is one.
    fn focused_monitor(&self) -> Option<&Monitor<Self::P>>;

    /// The monitor that has the focus, mutably.
    fn focused_monitor_mut(&mut self) -> Option<&mut Monitor<Self::P>>;

    /// The index of the monitor that has the focus.
    fn focused_monitor_idx(&self) -> usize;

    /// The index of the monitor the pointer is on, if the platform says where it is.
    fn monitor_idx_from_current_pos(&mut self) -> Option<usize>;

    /// Gives the focus to the monitor at `idx`.
    fn focus_monitor(&mut self, idx: usize) -> eyre::Result<()>;

    /// Shows workspace `idx` on the focused monitor and focuses it.
    fn focus_workspace(&mut self, idx: usize) -> eyre::Result<()>;

    /// Shows workspace `workspace_idx` on the monitor at `monitor_idx` (creating it if it is not
    /// there) without focusing that monitor.
    fn show_workspace_on(&mut self, monitor_idx: usize, workspace_idx: usize) -> eyre::Result<()>;

    /// Moves the focused container to workspace `idx` of the focused monitor.
    fn move_container_to_workspace(
        &mut self,
        idx: usize,
        follow: bool,
        direction: Option<OperationDirection>,
    ) -> eyre::Result<()>;
}

/// This is to ensure that even on an empty workspace on a secondary monitor, the secondary monitor
/// where the cursor is focused will be used as the target for the workspace switch op
fn focus_the_monitor_under_the_pointer<H: CommandHost>(host: &mut H) -> eyre::Result<()> {
    if let Some(monitor_idx) = host.monitor_idx_from_current_pos()
        && monitor_idx != host.focused_monitor_idx()
        && let Some(monitor) = host.monitors().get(monitor_idx)
        && let Some(workspace) = monitor.focused_workspace()
        && workspace.is_empty()
    {
        host.focus_monitor(monitor_idx)?;
    }

    Ok(())
}

/// Focuses the next or previous workspace of the monitor that is focused, or of the monitor the
/// pointer is on when its workspace is empty
pub fn cycle_focus_workspace<H: CommandHost>(
    host: &mut H,
    direction: CycleDirection,
) -> eyre::Result<()> {
    focus_the_monitor_under_the_pointer(host)?;

    let focused_monitor = host.focused_monitor().ok_or_eyre("there is no monitor")?;

    let focused_workspace_idx = focused_monitor.focused_workspace_idx();
    let workspaces = focused_monitor.workspaces().len();

    let workspace_idx = direction.next_idx(
        focused_workspace_idx,
        NonZeroUsize::new(workspaces).ok_or_eyre("there must be at least one workspace")?,
    );

    host.focus_workspace(workspace_idx)?;
    Ok(())
}

/// Focuses the next or previous empty workspace of the focused monitor, if it has one
pub fn cycle_focus_empty_workspace<H: CommandHost>(
    host: &mut H,
    direction: CycleDirection,
) -> eyre::Result<()> {
    focus_the_monitor_under_the_pointer(host)?;

    let focused_monitor = host.focused_monitor().ok_or_eyre("there is no monitor")?;

    let focused_workspace_idx = focused_monitor.focused_workspace_idx();
    let workspaces = focused_monitor.workspaces().len();

    let mut empty_workspaces = vec![];

    for (idx, w) in focused_monitor.workspaces().iter().enumerate() {
        if w.is_empty() {
            empty_workspaces.push(idx);
        }
    }

    if !empty_workspaces.is_empty() {
        let mut workspace_idx = direction.next_idx(
            focused_workspace_idx,
            NonZeroUsize::new(workspaces).ok_or_eyre("there must be at least one workspace")?,
        );

        while !empty_workspaces.contains(&workspace_idx) {
            workspace_idx = direction.next_idx(
                workspace_idx,
                NonZeroUsize::new(workspaces).ok_or_eyre("there must be at least one workspace")?,
            );
        }

        host.focus_workspace(workspace_idx)?;
    }
    Ok(())
}

/// Removes the focused workspace when it is empty, unnamed and not the monitor's only one, and
/// focuses the one before it
pub fn close_workspace<H: CommandHost>(host: &mut H) -> eyre::Result<()> {
    focus_the_monitor_under_the_pointer(host)?;

    let mut can_close = false;

    if let Some(monitor) = host.focused_monitor_mut() {
        let focused_workspace_idx = monitor.focused_workspace_idx();
        let next_focused_workspace_idx = focused_workspace_idx.saturating_sub(1);

        if let Some(workspace) = monitor.focused_workspace()
            && monitor.workspaces().len() > 1
            && workspace.containers().is_empty()
            && workspace.floating_windows().is_empty()
            && workspace.monocle_container.is_none()
            && workspace.maximized_window.is_none()
            && workspace.name.is_none()
        {
            can_close = true;
        }

        if can_close
            && monitor
                .workspaces_mut()
                .remove(focused_workspace_idx)
                .is_some()
        {
            host.focus_workspace(next_focused_workspace_idx)?;
        }
    }
    Ok(())
}

/// Focuses the workspace that was focused before this one
pub fn focus_last_workspace<H: CommandHost>(host: &mut H) -> eyre::Result<()> {
    focus_the_monitor_under_the_pointer(host)?;

    let idx = host
        .focused_monitor()
        .ok_or_eyre("there is no monitor")?
        .focused_workspace_idx();

    if let Some(monitor) = host.focused_monitor_mut()
        && let Some(last_focused_workspace) = monitor.last_focused_workspace
    {
        host.focus_workspace(last_focused_workspace)?;
    }

    host.focused_monitor_mut()
        .ok_or_eyre("there is no monitor")?
        .last_focused_workspace = Option::from(idx);
    Ok(())
}

/// Moves the focused container to the workspace that was focused before this one, and follows it
/// when `follow` is set
pub fn move_container_to_last_workspace<H: CommandHost>(
    host: &mut H,
    follow: bool,
) -> eyre::Result<()> {
    focus_the_monitor_under_the_pointer(host)?;

    let idx = host
        .focused_monitor()
        .ok_or_eyre("there is no monitor")?
        .focused_workspace_idx();

    if let Some(monitor) = host.focused_monitor_mut()
        && let Some(last_focused_workspace) = monitor.last_focused_workspace
    {
        host.move_container_to_workspace(last_focused_workspace, follow, None)?;
    }

    host.focused_monitor_mut()
        .ok_or_eyre("there is no monitor")?
        .last_focused_workspace = Option::from(idx);
    Ok(())
}

/// What the commands that move across monitors need besides [`CommandHost`].
pub trait MonitorCommandHost: CommandHost {
    /// Whether the pointer follows the focus.
    fn mouse_follows_focus(&self) -> bool;

    /// Lays the focused workspace out again and, with `trigger_focus`, focuses its window.
    fn update_focused_workspace(
        &mut self,
        follow_focus: bool,
        trigger_focus: bool,
    ) -> eyre::Result<()>;

    /// The direction in which one goes from the focused monitor to the one at `idx`, if they are
    /// next to each other.
    fn direction_from_monitor_idx(&self, idx: usize) -> Option<OperationDirection>;

    /// Moves the focused container to the monitor at `idx` (to workspace `workspace_idx` of it, or
    /// the one it shows), and follows it when `follow` is set.
    fn move_container_to_monitor(
        &mut self,
        idx: usize,
        workspace_idx: Option<usize>,
        follow: bool,
        direction: Option<OperationDirection>,
    ) -> eyre::Result<()>;

    /// The index of the workspace shown on the focused monitor.
    fn focused_workspace_idx(&self) -> eyre::Result<usize>;
}

/// Moves the focused container to the next or previous workspace of the focused monitor, and follows
/// it when `follow` is set
pub fn cycle_move_container_to_workspace<H: CommandHost>(
    host: &mut H,
    direction: CycleDirection,
    follow: bool,
) -> eyre::Result<()> {
    let focused_monitor = host.focused_monitor().ok_or_eyre("there is no monitor")?;

    let focused_workspace_idx = focused_monitor.focused_workspace_idx();
    let workspaces = focused_monitor.workspaces().len();

    let workspace_idx = direction.next_idx(
        focused_workspace_idx,
        NonZeroUsize::new(workspaces).ok_or_eyre("there must be at least one workspace")?,
    );

    host.move_container_to_workspace(workspace_idx, follow, None)?;
    Ok(())
}

/// Focuses the next or previous monitor
pub fn cycle_focus_monitor<H: MonitorCommandHost>(
    host: &mut H,
    direction: CycleDirection,
) -> eyre::Result<()> {
    let monitor_idx = direction.next_idx(
        host.focused_monitor_idx(),
        NonZeroUsize::new(host.monitors().len())
            .ok_or_eyre("there must be at least one monitor")?,
    );

    host.focus_monitor(monitor_idx)?;
    let mouse_follows_focus = host.mouse_follows_focus();
    host.update_focused_workspace(mouse_follows_focus, true)?;
    Ok(())
}

/// Moves the focused container to the next or previous monitor, and follows it when `follow` is set
pub fn cycle_move_container_to_monitor<H: MonitorCommandHost>(
    host: &mut H,
    direction: CycleDirection,
    follow: bool,
) -> eyre::Result<()> {
    let monitor_idx = direction.next_idx(
        host.focused_monitor_idx(),
        NonZeroUsize::new(host.monitors().len())
            .ok_or_eyre("there must be at least one monitor")?,
    );

    let direction = host.direction_from_monitor_idx(monitor_idx);
    host.move_container_to_monitor(monitor_idx, None, follow, direction)?;
    Ok(())
}

/// Shows workspace `workspace_idx` of monitor `monitor_idx`, unless that is what is in front
pub fn focus_monitor_workspace_number<H: MonitorCommandHost>(
    host: &mut H,
    monitor_idx: usize,
    workspace_idx: usize,
) -> eyre::Result<()> {
    let focused_monitor_idx = host.focused_monitor_idx();
    let focused_workspace_idx = host.focused_workspace_idx().unwrap_or_default();
    let focused_pair = (focused_monitor_idx, focused_workspace_idx);

    if focused_pair != (monitor_idx, workspace_idx) {
        host.focus_monitor(monitor_idx)?;
        host.focus_workspace(workspace_idx)?;
    }
    Ok(())
}

/// Moves the focused container to workspace `workspace_idx` of monitor `monitor_idx`, and follows it
/// when `follow` is set
pub fn move_container_to_monitor_workspace_number<H: MonitorCommandHost>(
    host: &mut H,
    monitor_idx: usize,
    workspace_idx: usize,
    follow: bool,
) -> eyre::Result<()> {
    let direction = host.direction_from_monitor_idx(monitor_idx);
    host.move_container_to_monitor(monitor_idx, Option::from(workspace_idx), follow, direction)?;
    Ok(())
}

/// What the commands that reach into a monitor's workspaces by index need besides [`CommandHost`].
pub trait ContainerCommandHost: CommandHost {
    /// The monitors, mutably.
    fn monitors_mut(&mut self) -> &mut VecDeque<Monitor<Self::P>>;
}

/// Locks or unlocks container `container_idx` of workspace `workspace_idx` of monitor
/// `monitor_idx`, so that it keeps its index (or not) while others are inserted, removed or swapped.
/// A container index that is not there is ignored; a monitor or workspace that is not there is an
/// error.
pub fn set_container_locked<H: ContainerCommandHost>(
    host: &mut H,
    monitor_idx: usize,
    workspace_idx: usize,
    container_idx: usize,
    locked: bool,
) -> eyre::Result<()> {
    let monitor = host
        .monitors_mut()
        .get_mut(monitor_idx)
        .ok_or_eyre("no monitor at the given index")?;

    let workspace = monitor
        .workspaces_mut()
        .get_mut(workspace_idx)
        .ok_or_eyre("no workspace at the given index")?;

    if let Some(container) = workspace.containers_mut().get_mut(container_idx) {
        container.locked = locked;
    }

    Ok(())
}

/// Shows workspace `workspace_idx` on the focused monitor, unless it is the one in front; the
/// monitor the pointer is on is focused first when its workspace is empty
pub fn focus_workspace_number<H: MonitorCommandHost>(
    host: &mut H,
    workspace_idx: usize,
) -> eyre::Result<()> {
    focus_the_monitor_under_the_pointer(host)?;

    if host.focused_workspace_idx().unwrap_or_default() != workspace_idx {
        host.focus_workspace(workspace_idx)?;
    }
    Ok(())
}

/// Shows workspace `workspace_idx` on every monitor, the focused one last; the monitor the pointer
/// is on is focused first when its workspace is empty
pub fn focus_workspace_numbers<H: CommandHost>(
    host: &mut H,
    workspace_idx: usize,
) -> eyre::Result<()> {
    focus_the_monitor_under_the_pointer(host)?;

    let focused_monitor_idx = host.focused_monitor_idx();

    for idx in 0..host.monitors().len() {
        if idx != focused_monitor_idx {
            host.show_workspace_on(idx, workspace_idx)?;
        }
    }

    host.focus_workspace(workspace_idx)?;
    Ok(())
}
