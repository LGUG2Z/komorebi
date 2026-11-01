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
use crate::WorkspaceLayer;
use color_eyre::eyre;
use color_eyre::eyre::OptionExt;
use komorebi_config::behaviour::WindowContainerBehaviour;
use komorebi_layouts::CycleDirection;
use komorebi_layouts::OperationDirection;
use komorebi_protocol::message::SocketMessage;
use komorebi_protocol::types::MonocleFocusBehaviour;
use komorebi_protocol::types::MoveBehaviour;
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

    /// Whether the router may run `message` at all: an error refuses it before any arm runs. The
    /// default lets everything through (Windows, which gates inside 18 window manager functions
    /// instead, section 22.1 of docs/dev/core-model.md); KDE and macOS refuse every message while the
    /// focused window is not managed and `unmanaged_window_operation_behaviour` is `NoOp`.
    fn gate_message(&mut self, message: &SocketMessage) -> eyre::Result<()> {
        let _ = message;
        Ok(())
    }

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

/// What the commands on the focused window need from the window manager, beyond [`CommandHost`]:
/// the moves and focus changes by direction (each daemon's own text, they differ in the borders,
/// the animation guard and the window API) and the layout of the focused workspace.
pub trait WindowCommandHost: CommandHost {
    /// Moves the focused container in `direction` (tiling layer).
    fn move_container_in_direction(&mut self, direction: OperationDirection) -> eyre::Result<()>;

    /// Moves the focused floating window in `direction` (floating layer).
    fn move_floating_window_in_direction(
        &mut self,
        direction: OperationDirection,
    ) -> eyre::Result<()>;

    /// Focuses the container in `direction` (tiling layer).
    fn focus_container_in_direction(&mut self, direction: OperationDirection) -> eyre::Result<()>;

    /// Focuses the floating window in `direction` (floating layer).
    fn focus_floating_window_in_direction(
        &mut self,
        direction: OperationDirection,
    ) -> eyre::Result<()>;

    /// Focuses the next or previous container (tiling layer).
    fn focus_container_in_cycle_direction(&mut self, direction: CycleDirection)
    -> eyre::Result<()>;

    /// Focuses the next or previous floating window (floating layer).
    fn focus_floating_window_in_cycle_direction(
        &mut self,
        direction: CycleDirection,
    ) -> eyre::Result<()>;

    /// Preselects the place in `direction` of the focused container for the next window.
    fn preselect_container_in_direction(
        &mut self,
        direction: OperationDirection,
    ) -> eyre::Result<()>;

    /// Moves the focused container to the front of the layout.
    fn promote_container_to_front(&mut self) -> eyre::Result<()>;

    /// Lays the focused workspace out again (`Workspace::update`).
    fn update_focused_workspace_layout(&mut self) -> eyre::Result<()>;
}

fn focused_layer<H: CommandHost>(host: &H) -> eyre::Result<WorkspaceLayer> {
    Ok(host
        .focused_monitor()
        .and_then(|monitor| monitor.focused_workspace())
        .ok_or_eyre("there is no workspace")?
        .layer)
}

/// Moves the focused container (tiling layer) or floating window (floating layer) in `direction`
pub fn move_window<H: WindowCommandHost>(
    host: &mut H,
    direction: OperationDirection,
) -> eyre::Result<()> {
    match focused_layer(host)? {
        WorkspaceLayer::Tiling => host.move_container_in_direction(direction),
        WorkspaceLayer::Floating => host.move_floating_window_in_direction(direction),
    }
}

/// Focuses the container (tiling layer) or floating window (floating layer) in `direction`
pub fn focus_window<H: WindowCommandHost>(
    host: &mut H,
    direction: OperationDirection,
) -> eyre::Result<()> {
    match focused_layer(host)? {
        WorkspaceLayer::Tiling => host.focus_container_in_direction(direction),
        WorkspaceLayer::Floating => host.focus_floating_window_in_direction(direction),
    }
}

/// Focuses the next or previous container (tiling layer) or floating window (floating layer)
pub fn cycle_focus_window<H: WindowCommandHost>(
    host: &mut H,
    direction: CycleDirection,
) -> eyre::Result<()> {
    match focused_layer(host)? {
        WorkspaceLayer::Tiling => host.focus_container_in_cycle_direction(direction),
        WorkspaceLayer::Floating => host.focus_floating_window_in_cycle_direction(direction),
    }
}

/// Preselects the place in `direction` of the focused container for the next window, on the
/// tiling layer and only when the workspace has no preselect yet, and lays the workspace out
pub fn preselect_direction<H: WindowCommandHost>(
    host: &mut H,
    direction: OperationDirection,
) -> eyre::Result<()> {
    let workspace = host
        .focused_monitor()
        .and_then(|monitor| monitor.focused_workspace())
        .ok_or_eyre("there is no workspace")?;

    if workspace.preselected_container_idx.is_some() {
        tracing::warn!("ignoring command as this workspace already has a direction preselect set");
    } else if matches!(workspace.layer, WorkspaceLayer::Tiling) {
        host.preselect_container_in_direction(direction)?;
        host.update_focused_workspace_layout()?;
    }

    Ok(())
}

/// Forgets the preselect of the focused workspace and lays it out
pub fn cancel_preselect<H: WindowCommandHost>(host: &mut H) -> eyre::Result<()> {
    host.focused_monitor_mut()
        .and_then(|monitor| monitor.focused_workspace_mut())
        .ok_or_eyre("there is no workspace")?
        .cancel_preselect();
    host.update_focused_workspace_layout()
}

/// Focuses the container in `direction`, then moves it to the front of the layout
pub fn promote_window<H: WindowCommandHost>(
    host: &mut H,
    direction: OperationDirection,
) -> eyre::Result<()> {
    host.focus_container_in_direction(direction)?;
    host.promote_container_to_front()
}

/// The index of the workspace called `name` and the monitor that has it, searching the monitors in
/// order
pub fn monitor_workspace_index_by_name<H: CommandHost>(
    host: &H,
    name: &str,
) -> Option<(usize, usize)> {
    host.monitors()
        .iter()
        .enumerate()
        .find_map(|(monitor_idx, monitor)| {
            monitor
                .workspaces()
                .iter()
                .position(|workspace| workspace.name.as_deref() == Some(name))
                .map(|workspace_idx| (monitor_idx, workspace_idx))
        })
}

/// Focuses the workspace called `name`, on the monitor that has it
pub fn focus_named_workspace<H: CommandHost>(host: &mut H, name: &str) -> eyre::Result<()> {
    if let Some((monitor_idx, workspace_idx)) = monitor_workspace_index_by_name(host, name) {
        host.focus_monitor(monitor_idx)?;
        host.focus_workspace(workspace_idx)?;
    }
    Ok(())
}

/// Moves the focused container to the workspace called `name`, on whichever monitor has it, and
/// follows it when `follow` is set
pub fn move_container_to_named_workspace<H: MonitorCommandHost>(
    host: &mut H,
    name: &str,
    follow: bool,
) -> eyre::Result<()> {
    if let Some((monitor_idx, workspace_idx)) = monitor_workspace_index_by_name(host, name) {
        let direction = host.direction_from_monitor_idx(monitor_idx);
        host.move_container_to_monitor(monitor_idx, Some(workspace_idx), follow, direction)?;
    }
    Ok(())
}

/// The settings the behaviour toggles flip.
pub trait BehaviourCommandHost {
    /// Whether a new window gets a container of its own or joins the focused one.
    fn window_container_behaviour(&self) -> WindowContainerBehaviour;
    /// Sets [`BehaviourCommandHost::window_container_behaviour`].
    fn set_window_container_behaviour(&mut self, behaviour: WindowContainerBehaviour);
    /// What a cross-monitor move does with the container it meets.
    fn cross_monitor_move_behaviour(&self) -> MoveBehaviour;
    /// Sets [`BehaviourCommandHost::cross_monitor_move_behaviour`].
    fn set_cross_monitor_move_behaviour(&mut self, behaviour: MoveBehaviour);
    /// What focusing does while a monocle container is in front.
    fn monocle_focus_behaviour(&self) -> MonocleFocusBehaviour;
    /// Sets [`BehaviourCommandHost::monocle_focus_behaviour`].
    fn set_monocle_focus_behaviour(&mut self, behaviour: MonocleFocusBehaviour);
}

/// Flips between creating a container for a new window and appending it to the focused one
pub fn toggle_window_container_behaviour<H: BehaviourCommandHost>(host: &mut H) {
    host.set_window_container_behaviour(match host.window_container_behaviour() {
        WindowContainerBehaviour::Create => WindowContainerBehaviour::Append,
        WindowContainerBehaviour::Append => WindowContainerBehaviour::Create,
    });
}

/// Flips a cross-monitor move between swapping and inserting; `NoOp` stays as it is
pub fn toggle_cross_monitor_move_behaviour<H: BehaviourCommandHost>(host: &mut H) {
    match host.cross_monitor_move_behaviour() {
        MoveBehaviour::Swap => host.set_cross_monitor_move_behaviour(MoveBehaviour::Insert),
        MoveBehaviour::Insert => host.set_cross_monitor_move_behaviour(MoveBehaviour::Swap),
        _ => {}
    }
}

/// Flips the focus behaviour of a monocle container between cycling and doing nothing
pub fn toggle_monocle_focus_behaviour<H: BehaviourCommandHost>(host: &mut H) {
    host.set_monocle_focus_behaviour(match host.monocle_focus_behaviour() {
        MonocleFocusBehaviour::Cycle => MonocleFocusBehaviour::NoOp,
        MonocleFocusBehaviour::NoOp => MonocleFocusBehaviour::Cycle,
    });
}
