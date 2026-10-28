//! How the daemon answers the messages on its socket: [`listen_for_commands`] and
//! [`WindowManager::process_command`].
//!
//! A client connects to `komorebi.sock` in [`struct@DATA_DIR`](crate::DATA_DIR) and writes one
//! [`SocketMessage`] as JSON per line. Each connection is read on a separate thread, and each
//! message is handled with the window manager locked. A message that asks a question is answered
//! on the same connection, and the answer ends when the daemon closes it:
//!
//! - **State**: the window manager's [`State`], as JSON.
//! - **MonitorInformation**: the id, the name and the bounds of each monitor, as JSON.
//! - **Query**: one value as text, the answer to a [`StateQuery`].
//! - **GlobalState**: the ignore and the manage rules and the data directory ([`GlobalState`]), as
//!   JSON.
//! - **SessionFloatRules**: the float rules **SessionFloatRule** added, as JSON.
//! - **DebugWindow**: what [`Window::should_manage`] read of a window and which rules matched it
//!   ([`RuleDebug`]), as JSON, or nothing for a window that is not there.
//!
//! The others answer nothing:
//!
//! - **FocusWindow** and **CycleFocusWindow** give the focus to the container on one side of the
//!   focused one (or of the monitor on that side, at the edge of a monitor), or to the next or the
//!   previous one in the workspace
//!   ([`WindowManager::focus_container_in_direction`],
//!   [`WindowManager::focus_container_in_cycle_direction`]).
//! - **MoveWindow** and **CycleMoveWindow** swap the focused container with the one on one side
//!   of it (or take it to the monitor on that side, at the edge of a monitor, as
//!   **CrossMonitorMoveBehaviour** and **ToggleCrossMonitorMoveBehaviour** set), or with the next or
//!   the previous one, and tile the workspace again
//!   ([`WindowManager::move_container_in_direction`],
//!   [`WindowManager::move_container_in_cycle_direction`]).
//! - **Promote** moves the focused container to the largest tile, **PromoteSwap** swaps it with
//!   the container there (and back), **PromoteFocus** gives that container the focus, and
//!   **PromoteWindow** promotes the container on one side of the focused one
//!   ([`WindowManager::promote_container_to_front`], [`WindowManager::promote_container_swap`],
//!   [`WindowManager::promote_focus_to_front`]).
//! - **ChangeLayout**, **CycleLayout** and **FlipLayout** give the focused workspace another
//!   layout, or mirror it, and **ToggleTiling** turns its tiling off and on
//!   ([`WindowManager::change_workspace_layout_default`], [`WindowManager::cycle_layout`],
//!   [`WindowManager::flip_layout`], [`WindowManager::toggle_tiling`]).
//! - **ScrollingLayoutColumns** sets how many columns the scrolling layout shows on the focused
//!   workspace, and **LayoutRatios** the share of the work area its first columns and rows take,
//!   and both tile the workspace again.
//! - **PreselectDirection** reserves the place on one side of the focused container for the next
//!   window ([`WindowManager::preselect_container_in_direction`]), unless a place is reserved
//!   already, and **CancelPreselect** gives the reserved place up.
//! - **MouseFollowsFocus** and **ToggleMouseFollowsFocus** set whether the pointer is moved to
//!   the window that is given the focus.
//! - **ToggleLock** locks the focused container at its index, or unlocks it
//!   ([`WindowManager::toggle_lock`]), and **LockMonitorWorkspaceContainer** and
//!   **UnlockMonitorWorkspaceContainer** do the same for a container given by the index of its
//!   monitor, its workspace and itself.
//! - **ToggleFloat** takes the focused window out of the layout into the floating windows of the
//!   workspace, which komorebi does not move, or puts a floating window back
//!   ([`WindowManager::toggle_float`]).
//! - **WorkspacePadding** and **ContainerPadding** set the paddings of a workspace given by the
//!   index of its monitor and its own, **NamedWorkspacePadding** and
//!   **NamedWorkspaceContainerPadding** those of the workspace with a name
//!   ([`WindowManager::monitor_workspace_index_by_name`]; a name no workspace has changes
//!   nothing), **FocusedWorkspacePadding** and **FocusedWorkspaceContainerPadding** those of the
//!   focused workspace, and **AdjustWorkspacePadding** and **AdjustContainerPadding** make the
//!   focused workspace's larger or smaller ([`WindowManager::set_workspace_padding`],
//!   [`WindowManager::set_container_padding`], [`WindowManager::adjust_workspace_padding`],
//!   [`WindowManager::adjust_container_padding`]).
//! - **WorkAreaOffset**, **MonitorWorkAreaOffset** and **WorkspaceWorkAreaOffset** set the work
//!   area offset of every monitor, of one monitor or of one workspace, and tile every monitor again
//!   (a monitor or a workspace that is not there is ignored), and
//!   **ToggleWindowBasedWorkAreaOffset** sets whether the focused workspace takes its monitor's
//!   window-based offset.
//! - **Minimize** and **Close** ask the compositor to minimize or close the window that has the
//!   focus. The window leaves the layout when the compositor reports that it did.
//! - **UnmanageFocusedWindow** takes the window that has the focus out of the layout and leaves it
//!   alone until it closes, and **ManageFocusedWindow** takes it back
//!   ([`WindowManager::unmanage_focused_window`], [`WindowManager::manage_focused_window`]).
//! - **UnmanagedWindowOperationBehaviour** sets whether a message is carried out while the window
//!   that has the focus is not tiled ([`WindowManager::handle_unmanaged_window_behaviour`]). With
//!   `NoOp`, every message is refused then, this one included.
//! - **IgnoreRule** adds a rule that keeps the windows of an application out of the layout, and
//!   takes the ones it names exactly out of the workspaces in front; **ManageRule** adds a rule
//!   that has them tiled whatever the ignore rules say. **SessionFloatRule** floats the window
//!   that has the focus and adds a rule that floats the windows that open with its executable,
//!   title and class, and **ClearSessionFloatRules** removes those rules.
//! - **TogglePause** pauses the window manager, or resumes it: while it is paused, it handles no
//!   event, and of the messages only **TogglePause**, **State**, **GlobalState** and **Stop**
//!   ([`read_commands_uds`]).
//! - **ResizeWindowEdge** and **ResizeWindowAxis** resize the focused container by the
//!   [`WindowManager::resize_delta`] that **ResizeDelta** sets ([`WindowManager::resize_window`]),
//!   and **QuickSave**, **QuickLoad**, **Save** and **Load** write the resize adjustments of the
//!   focused workspace's containers to a file and read them back.
//! - **FocusWorkspaceNumber** shows a workspace of the focused monitor, by its index
//!   ([`WindowManager::focus_workspace`]): the compositor is asked to show its desktop on that
//!   monitor. **MoveContainerToWorkspaceNumber** moves the focused container to a workspace and
//!   shows it, and **SendContainerToWorkspaceNumber** moves it there and stays
//!   ([`WindowManager::move_container_to_workspace`]). A workspace that does not exist yet is
//!   made, with the ones before it.
//! - **CycleFocusWorkspace**, **CycleMoveContainerToWorkspace** and
//!   **CycleSendContainerToWorkspace** do the same for the next or the previous workspace, and
//!   **FocusLastWorkspace**, **MoveContainerToLastWorkspace** and
//!   **SendContainerToLastWorkspace** for the workspace that was in front before.
//!   **CycleFocusEmptyWorkspace** shows the next or the previous workspace with no window, and
//!   **FocusWorkspaceNumbers** shows a workspace on every monitor.
//! - **WorkspaceName** names a workspace and **EnsureNamedWorkspaces** gives a monitor named
//!   workspaces; **FocusNamedWorkspace** shows the workspace with a name, on its monitor
//!   ([`WindowManager::monitor_workspace_index_by_name`]), and **NamedWorkspaceLayout** and
//!   **NamedWorkspaceTiling** set its layout and its tiling, which **WorkspaceLayout** and
//!   **WorkspaceTiling** do by index. **EnsureWorkspaces** gives a monitor workspaces up to a
//!   number, **NewWorkspace** adds one and shows it, and **CloseWorkspace** removes the workspace
//!   in front when it has no window and no name and is not the monitor's last.
//! - **FocusMonitorNumber**, **CycleFocusMonitor** and **FocusMonitorAtCursor** focus another
//!   monitor ([`WindowManager::focus_monitor`]), and **FocusMonitorWorkspaceNumber** a workspace
//!   of it. **MoveContainerToMonitorNumber**, **SendContainerToMonitorNumber**, their cycle and
//!   monitor-workspace forms, **MoveContainerToNamedWorkspace** and
//!   **SendContainerToNamedWorkspace** take the focused container to another monitor
//!   ([`WindowManager::move_container_to_monitor`]); **MoveWorkspaceToMonitorNumber**,
//!   **CycleMoveWorkspaceToMonitor** and **SwapWorkspacesToMonitorNumber** move workspaces
//!   between monitors. **DisplayIndexPreference** gives a monitor an index, and **EagerFocus**
//!   focuses the first window of an executable on any workspace of any monitor.
//! - **Retile** tiles the focused workspace of every monitor again, without the resize
//!   adjustments, and **RetileWithResizeDimensions** with them.
//! - **AddSubscriberSocket** and **AddSubscriberSocketWithOptions** add a subscriber by the name
//!   of its socket in the data directory, and **RemoveSubscriberSocket** removes one.
//! - **Stop** stops the daemon ([`WindowManager::stop`]).
//!
//! Once a message has been handled, every subscriber is sent a notification of it
//! ([`notify_subscribers`]).

use crate::DATA_DIR;
use crate::DISPLAY_INDEX_PREFERENCES;
use crate::FLOATING_APPLICATIONS;
use crate::IGNORE_IDENTIFIERS;
use crate::MANAGE_IDENTIFIERS;
use crate::Notification;
use crate::NotificationEvent;
use crate::SESSION_FLOATING_APPLICATIONS;
use crate::SUBSCRIPTION_SOCKET_OPTIONS;
use crate::SUBSCRIPTION_SOCKETS;
use crate::backend::backend;
use crate::core::ApplicationIdentifier;
use crate::core::Axis;
use crate::core::Layout;
use crate::core::LayoutOptions;
use crate::core::MoveBehaviour;
use crate::core::OperationDirection;
use crate::core::Rect;
use crate::core::ScrollingLayoutOptions;
use crate::core::SocketMessage;
use crate::core::StateQuery;
use crate::core::config_generation::IdWithIdentifier;
use crate::core::config_generation::MatchingRule;
use crate::core::config_generation::MatchingStrategy;
use crate::monitor::MonitorInformation;
use crate::notify_subscribers;
use crate::state::GlobalState;
use crate::state::State;
use crate::window::RuleDebug;
use crate::window::Window;
use crate::window_manager::WindowManager;
use crate::workspace::WorkspaceWindowLocation;
use color_eyre::eyre;
use color_eyre::eyre::Context;
use color_eyre::eyre::OptionExt;
use parking_lot::Mutex;
use std::fs::File;
use std::fs::OpenOptions;
use std::io::BufRead;
use std::io::BufReader;
use std::num::NonZeroUsize;
use std::os::unix::net::UnixStream;
use std::str::FromStr;
use std::sync::Arc;
use std::time::Duration;
use version::build;

/// Starts the thread that accepts connections on the window manager's
/// [`command_listener`](WindowManager::command_listener), and reads each connection on a separate
/// thread ([`read_commands_uds`])
///
/// When accepting fails, the error is logged and the listening thread is started again.
#[tracing::instrument]
pub fn listen_for_commands(wm: Arc<Mutex<WindowManager>>) {
    std::thread::spawn(move || {
        loop {
            let wm = wm.clone();

            let _ = std::thread::spawn(move || {
                let listener = wm
                    .lock()
                    .command_listener
                    .try_clone()
                    .expect("could not clone unix listener");

                tracing::info!("listening on komorebi.sock");
                for client in listener.incoming() {
                    match client {
                        Ok(stream) => {
                            let wm_clone = wm.clone();
                            std::thread::spawn(move || {
                                match read_commands_uds(&wm_clone, stream) {
                                    Ok(()) => {}
                                    Err(error) => {
                                        tracing::error!("{error}")
                                    }
                                }
                            });
                        }
                        Err(error) => {
                            tracing::error!("failed to get unix stream {}", error);
                            break;
                        }
                    }
                }
            })
            .join();

            tracing::error!("restarting failed thread");
        }
    });
}

impl WindowManager {
    /// Handles one message, writes the answer to a question to `reply`, and sends the subscribers
    /// a notification of the message
    ///
    /// A message that fails is not notified.
    ///
    /// # Errors
    ///
    /// - The unmanaged window operation behavior is `NoOp` and the window that has the focus is not
    ///   tiled, for one of the reasons [`WindowManager::handle_unmanaged_window_behaviour`] gives.
    /// - A query about the focused monitor, workspace or container when there is none.
    /// - The focus cannot be moved, for one of the reasons
    ///   [`WindowManager::focus_container_in_direction`] and
    ///   [`WindowManager::focus_container_in_cycle_direction`] give.
    /// - The focused container cannot be moved, for one of the reasons
    ///   [`WindowManager::move_container_in_direction`] and
    ///   [`WindowManager::move_container_in_cycle_direction`] give.
    /// - There is no focused workspace to change the layout or the tiling of, to preselect a
    ///   place in, or to resize or promote a container of, or it cannot be tiled again.
    /// - The monitor or the workspace of `LockMonitorWorkspaceContainer` or
    ///   `UnlockMonitorWorkspaceContainer` is not there (a container that is not there is ignored).
    /// - The focused window cannot be floated or put back, for one of the reasons
    ///   [`WindowManager::toggle_float`] gives.
    /// - The monitor a monitor command names is not there, or the compositor reports no window
    ///   with the focus to take to it ([`WindowManager::move_container_to_monitor`]).
    /// - The monitor or the workspace a message names by its index is not there
    ///   (`WorkspaceName`, `WorkspaceLayout`, `WorkspaceTiling`, `EnsureWorkspaces`,
    ///   `EnsureNamedWorkspaces`). A name no workspace has is ignored.
    /// - The focused monitor has no workspace or no container to move to another workspace, or
    ///   the compositor reports no window with the focus (`MoveContainerToWorkspaceNumber`,
    ///   `SendContainerToWorkspaceNumber`), or the focused window of the workspace that is shown
    ///   cannot be focused.
    /// - The monitor or the workspace of `WorkspacePadding` or `ContainerPadding` is not there, or
    ///   the focused workspace has no padding of its own for `AdjustWorkspacePadding` or
    ///   `AdjustContainerPadding` to change.
    /// - `Minimize`, `Close`, `ManageFocusedWindow`, `UnmanageFocusedWindow` or `SessionFloatRule`
    ///   when the compositor reports no focused window, or the backend refuses to minimize, close or
    ///   float it.
    /// - `IgnoreRule` cannot take a window out of a workspace, or tile the workspace again.
    /// - The file of `QuickSave` or `Save` cannot be written, or the file of `QuickLoad` or
    ///   `Load` cannot be read or is not a list of resize adjustments.
    /// - The window manager fails to retile, for one of the reasons
    ///   [`WindowManager::retile_all`] gives.
    /// - The answer cannot be written to `reply`.
    /// - `Stop` fails, for the reason [`WindowManager::stop`] gives.
    /// - The notification cannot be sent, for one of the reasons [`notify_subscribers`] gives.
    #[tracing::instrument(skip(self, reply, message))]
    pub fn process_command(
        &mut self,
        message: SocketMessage,
        mut reply: impl std::io::Write,
    ) -> eyre::Result<()> {
        tracing::info!("processing command: {message}");

        #[allow(clippy::useless_asref)]
        // We don't have From implemented for &mut WindowManager
        let initial_state = State::from(self.as_ref());

        self.handle_unmanaged_window_behaviour()?;

        match message {
            SocketMessage::Promote => self.promote_container_to_front()?,
            SocketMessage::PromoteSwap => self.promote_container_swap()?,
            SocketMessage::PromoteFocus => self.promote_focus_to_front()?,
            SocketMessage::PromoteWindow(direction) => {
                self.focus_container_in_direction(direction)?;
                self.promote_container_to_front()?
            }
            SocketMessage::EagerFocus(ref exe) => {
                let focused_monitor_idx = self.focused_monitor_idx();
                let mouse_follows_focus = self.mouse_follows_focus;

                let mut window_location = None;
                let mut monitor_to_focus = None;
                let mut needs_workspace_loading = false;

                'search: for (monitor_idx, monitor) in self.monitors_mut().iter_mut().enumerate() {
                    for (workspace_idx, workspace) in monitor.workspaces().iter().enumerate() {
                        if let Some(location) = workspace.location_from_exe(exe) {
                            window_location = Some(location);

                            if monitor_idx != focused_monitor_idx {
                                monitor_to_focus = Some(monitor_idx);
                            }

                            // Focus workspace if it is not already the focused one, without
                            // loading it so that we don't give focus to the wrong window, we will
                            // load it later after focusing the wanted window
                            let focused_ws_idx = monitor.focused_workspace_idx();
                            if focused_ws_idx != workspace_idx {
                                monitor.last_focused_workspace = Option::from(focused_ws_idx);
                                monitor.focus_workspace(workspace_idx)?;
                                needs_workspace_loading = true;
                            }

                            break 'search;
                        }
                    }
                }

                if let Some(monitor_idx) = monitor_to_focus {
                    self.focus_monitor(monitor_idx)?;
                }

                if let Some(location) = window_location {
                    match location {
                        WorkspaceWindowLocation::Container(container_idx, window_idx) => {
                            let focused_container_idx = self.focused_container_idx()?;
                            if container_idx != focused_container_idx {
                                self.focused_workspace_mut()?.focus_container(container_idx);
                            }

                            self.focus_container_window(window_idx)?;
                        }
                        WorkspaceWindowLocation::Floating(window_idx) => {
                            if let Some(window) = self
                                .focused_workspace_mut()?
                                .floating_windows_mut()
                                .get_mut(window_idx)
                            {
                                window.focus(mouse_follows_focus)?;
                            }
                        }
                    }

                    if needs_workspace_loading {
                        let mouse_follows_focus = self.mouse_follows_focus;
                        if let Some(monitor) = self.focused_monitor_mut() {
                            monitor.load_focused_workspace(mouse_follows_focus)?;
                        }
                    }
                }
            }
            SocketMessage::FocusWindow(direction) => {
                self.focus_container_in_direction(direction)?;
            }
            SocketMessage::PreselectDirection(direction) => {
                let focused_workspace = self.focused_workspace()?;
                let mut update = false;

                if focused_workspace.preselected_container_idx.is_some() {
                    tracing::warn!(
                        "ignoring command as this workspace already has a direction preselect set"
                    );
                } else {
                    self.preselect_container_in_direction(direction)?;
                    update = true;
                }

                if update {
                    self.focused_workspace_mut()?.update()?;
                }
            }
            SocketMessage::CancelPreselect => {
                let focused_workspace = self.focused_workspace_mut()?;
                focused_workspace.cancel_preselect();
                focused_workspace.update()?;
            }
            SocketMessage::MoveWindow(direction) => {
                self.move_container_in_direction(direction)?;
            }
            SocketMessage::CycleFocusWindow(direction) => {
                self.focus_container_in_cycle_direction(direction)?;
            }
            SocketMessage::CycleMoveWindow(direction) => {
                self.move_container_in_cycle_direction(direction)?;
            }
            SocketMessage::CycleFocusMonitor(direction) => {
                let monitor_idx = direction.next_idx(
                    self.focused_monitor_idx(),
                    NonZeroUsize::new(self.monitors().len())
                        .ok_or_eyre("there must be at least one monitor")?,
                );

                self.focus_monitor(monitor_idx)?;
                self.update_focused_workspace(self.mouse_follows_focus, true)?;
            }
            SocketMessage::CycleFocusWorkspace(direction) => {
                // This is to ensure that even on an empty workspace on a secondary monitor, the
                // secondary monitor where the cursor is focused will be used as the target for
                // the workspace switch op
                if let Some(monitor_idx) = self.monitor_idx_from_current_pos()
                    && monitor_idx != self.focused_monitor_idx()
                    && let Some(monitor) = self.monitors().get(monitor_idx)
                    && let Some(workspace) = monitor.focused_workspace()
                    && workspace.is_empty()
                {
                    self.focus_monitor(monitor_idx)?;
                }

                let focused_monitor = self.focused_monitor().ok_or_eyre("there is no monitor")?;

                let focused_workspace_idx = focused_monitor.focused_workspace_idx();
                let workspaces = focused_monitor.workspaces().len();

                let workspace_idx = direction.next_idx(
                    focused_workspace_idx,
                    NonZeroUsize::new(workspaces)
                        .ok_or_eyre("there must be at least one workspace")?,
                );

                self.focus_workspace(workspace_idx)?;
            }
            SocketMessage::CycleFocusEmptyWorkspace(direction) => {
                let focused_monitor = self.focused_monitor().ok_or_eyre("there is no monitor")?;

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
                        NonZeroUsize::new(workspaces)
                            .ok_or_eyre("there must be at least one workspace")?,
                    );

                    while !empty_workspaces.contains(&workspace_idx) {
                        workspace_idx = direction.next_idx(
                            workspace_idx,
                            NonZeroUsize::new(workspaces)
                                .ok_or_eyre("there must be at least one workspace")?,
                        );
                    }

                    self.focus_workspace(workspace_idx)?;
                }
            }
            SocketMessage::FocusMonitorNumber(monitor_idx) => {
                self.focus_monitor(monitor_idx)?;
                self.update_focused_workspace(self.mouse_follows_focus, true)?;
            }
            SocketMessage::FocusMonitorAtCursor => {
                if let Some(monitor_idx) = self.monitor_idx_from_current_pos() {
                    self.focus_monitor(monitor_idx)?;
                }
            }
            SocketMessage::FocusLastWorkspace => {
                let idx = self
                    .focused_monitor()
                    .ok_or_eyre("there is no monitor")?
                    .focused_workspace_idx();

                if let Some(monitor) = self.focused_monitor_mut()
                    && let Some(last_focused_workspace) = monitor.last_focused_workspace
                {
                    self.focus_workspace(last_focused_workspace)?;
                }

                self.focused_monitor_mut()
                    .ok_or_eyre("there is no monitor")?
                    .last_focused_workspace = Option::from(idx);
            }
            SocketMessage::FocusWorkspaceNumber(workspace_idx) => {
                if self.focused_workspace_idx().unwrap_or_default() != workspace_idx {
                    self.focus_workspace(workspace_idx)?;
                }
            }
            SocketMessage::FocusWorkspaceNumbers(workspace_idx) => {
                let focused_monitor_idx = self.focused_monitor_idx();

                for (i, monitor) in self.monitors_mut().iter_mut().enumerate() {
                    if i != focused_monitor_idx {
                        monitor.focus_workspace(workspace_idx)?;
                        monitor.load_focused_workspace(false)?;
                    }
                }

                self.focus_workspace(workspace_idx)?;
            }
            SocketMessage::FocusMonitorWorkspaceNumber(monitor_idx, workspace_idx) => {
                let focused_monitor_idx = self.focused_monitor_idx();
                let focused_workspace_idx = self.focused_workspace_idx().unwrap_or_default();

                let focused_pair = (focused_monitor_idx, focused_workspace_idx);

                if focused_pair != (monitor_idx, workspace_idx) {
                    self.focus_monitor(monitor_idx)?;
                    self.focus_workspace(workspace_idx)?;
                }
            }
            SocketMessage::FocusNamedWorkspace(ref name) => {
                if let Some((monitor_idx, workspace_idx)) =
                    self.monitor_workspace_index_by_name(name)
                {
                    self.focus_monitor(monitor_idx)?;
                    self.focus_workspace(workspace_idx)?;
                }
            }
            SocketMessage::CloseWorkspace => {
                let mut can_close = false;

                if let Some(monitor) = self.focused_monitor_mut() {
                    let focused_workspace_idx = monitor.focused_workspace_idx();
                    let next_focused_workspace_idx = focused_workspace_idx.saturating_sub(1);

                    if let Some(workspace) = monitor.focused_workspace()
                        && monitor.workspaces().len() > 1
                        && workspace.containers().is_empty()
                        && workspace.floating_windows().is_empty()
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
                        self.focus_workspace(next_focused_workspace_idx)?;
                    }
                }
            }

            SocketMessage::MoveContainerToWorkspaceNumber(workspace_idx) => {
                self.move_container_to_workspace(workspace_idx, true, None)?;
            }
            SocketMessage::SendContainerToWorkspaceNumber(workspace_idx) => {
                self.move_container_to_workspace(workspace_idx, false, None)?;
            }
            SocketMessage::MoveContainerToLastWorkspace => {
                // This is to ensure that even on an empty workspace on a secondary monitor, the
                // secondary monitor where the cursor is focused will be used as the target for
                // the workspace switch op
                if let Some(monitor_idx) = self.monitor_idx_from_current_pos()
                    && monitor_idx != self.focused_monitor_idx()
                    && let Some(monitor) = self.monitors().get(monitor_idx)
                    && let Some(workspace) = monitor.focused_workspace()
                    && workspace.is_empty()
                {
                    self.focus_monitor(monitor_idx)?;
                }

                let idx = self
                    .focused_monitor()
                    .ok_or_eyre("there is no monitor")?
                    .focused_workspace_idx();

                if let Some(monitor) = self.focused_monitor_mut()
                    && let Some(last_focused_workspace) = monitor.last_focused_workspace
                {
                    self.move_container_to_workspace(last_focused_workspace, true, None)?;
                }

                self.focused_monitor_mut()
                    .ok_or_eyre("there is no monitor")?
                    .last_focused_workspace = Option::from(idx);
            }
            SocketMessage::SendContainerToLastWorkspace => {
                // This is to ensure that even on an empty workspace on a secondary monitor, the
                // secondary monitor where the cursor is focused will be used as the target for
                // the workspace switch op
                if let Some(monitor_idx) = self.monitor_idx_from_current_pos()
                    && monitor_idx != self.focused_monitor_idx()
                    && let Some(monitor) = self.monitors().get(monitor_idx)
                    && let Some(workspace) = monitor.focused_workspace()
                    && workspace.is_empty()
                {
                    self.focus_monitor(monitor_idx)?;
                }

                let idx = self
                    .focused_monitor()
                    .ok_or_eyre("there is no monitor")?
                    .focused_workspace_idx();

                if let Some(monitor) = self.focused_monitor_mut()
                    && let Some(last_focused_workspace) = monitor.last_focused_workspace
                {
                    self.move_container_to_workspace(last_focused_workspace, false, None)?;
                }
                self.focused_monitor_mut()
                    .ok_or_eyre("there is no monitor")?
                    .last_focused_workspace = Option::from(idx);
            }
            SocketMessage::CycleMoveContainerToWorkspace(direction) => {
                let focused_monitor = self.focused_monitor().ok_or_eyre("there is no monitor")?;

                let focused_workspace_idx = focused_monitor.focused_workspace_idx();
                let workspaces = focused_monitor.workspaces().len();

                let workspace_idx = direction.next_idx(
                    focused_workspace_idx,
                    NonZeroUsize::new(workspaces)
                        .ok_or_eyre("there must be at least one workspace")?,
                );

                self.move_container_to_workspace(workspace_idx, true, None)?;
            }
            SocketMessage::MoveContainerToMonitorNumber(monitor_idx) => {
                let direction = self.direction_from_monitor_idx(monitor_idx);
                self.move_container_to_monitor(monitor_idx, None, true, direction)?;
            }
            SocketMessage::SwapWorkspacesToMonitorNumber(monitor_idx) => {
                self.swap_focused_monitor(monitor_idx)?;
            }
            SocketMessage::CycleMoveContainerToMonitor(direction) => {
                let monitor_idx = direction.next_idx(
                    self.focused_monitor_idx(),
                    NonZeroUsize::new(self.monitors().len())
                        .ok_or_eyre("there must be at least one monitor")?,
                );

                let direction = self.direction_from_monitor_idx(monitor_idx);
                self.move_container_to_monitor(monitor_idx, None, true, direction)?;
            }
            SocketMessage::CycleSendContainerToWorkspace(direction) => {
                let focused_monitor = self.focused_monitor().ok_or_eyre("there is no monitor")?;

                let focused_workspace_idx = focused_monitor.focused_workspace_idx();
                let workspaces = focused_monitor.workspaces().len();

                let workspace_idx = direction.next_idx(
                    focused_workspace_idx,
                    NonZeroUsize::new(workspaces)
                        .ok_or_eyre("there must be at least one workspace")?,
                );

                self.move_container_to_workspace(workspace_idx, false, None)?;
            }
            SocketMessage::SendContainerToMonitorNumber(monitor_idx) => {
                let direction = self.direction_from_monitor_idx(monitor_idx);
                self.move_container_to_monitor(monitor_idx, None, false, direction)?;
            }
            SocketMessage::CycleSendContainerToMonitor(direction) => {
                let monitor_idx = direction.next_idx(
                    self.focused_monitor_idx(),
                    NonZeroUsize::new(self.monitors().len())
                        .ok_or_eyre("there must be at least one monitor")?,
                );

                let direction = self.direction_from_monitor_idx(monitor_idx);
                self.move_container_to_monitor(monitor_idx, None, false, direction)?;
            }
            SocketMessage::SendContainerToMonitorWorkspaceNumber(monitor_idx, workspace_idx) => {
                let direction = self.direction_from_monitor_idx(monitor_idx);
                self.move_container_to_monitor(
                    monitor_idx,
                    Option::from(workspace_idx),
                    false,
                    direction,
                )?;
            }
            SocketMessage::MoveContainerToMonitorWorkspaceNumber(monitor_idx, workspace_idx) => {
                let direction = self.direction_from_monitor_idx(monitor_idx);
                self.move_container_to_monitor(
                    monitor_idx,
                    Option::from(workspace_idx),
                    true,
                    direction,
                )?;
            }
            SocketMessage::SendContainerToNamedWorkspace(ref workspace) => {
                if let Some((monitor_idx, workspace_idx)) =
                    self.monitor_workspace_index_by_name(workspace)
                {
                    let direction = self.direction_from_monitor_idx(monitor_idx);
                    self.move_container_to_monitor(
                        monitor_idx,
                        Option::from(workspace_idx),
                        false,
                        direction,
                    )?;
                }
            }
            SocketMessage::MoveContainerToNamedWorkspace(ref workspace) => {
                if let Some((monitor_idx, workspace_idx)) =
                    self.monitor_workspace_index_by_name(workspace)
                {
                    let direction = self.direction_from_monitor_idx(monitor_idx);
                    self.move_container_to_monitor(
                        monitor_idx,
                        Option::from(workspace_idx),
                        true,
                        direction,
                    )?;
                }
            }

            SocketMessage::MoveWorkspaceToMonitorNumber(monitor_idx) => {
                self.move_workspace_to_monitor(monitor_idx)?;
            }
            SocketMessage::CycleMoveWorkspaceToMonitor(direction) => {
                let monitor_idx = direction.next_idx(
                    self.focused_monitor_idx(),
                    NonZeroUsize::new(self.monitors().len())
                        .ok_or_eyre("there must be at least one monitor")?,
                );

                self.move_workspace_to_monitor(monitor_idx)?;
            }
            SocketMessage::ToggleCrossMonitorMoveBehaviour => {
                match self.cross_monitor_move_behaviour {
                    MoveBehaviour::Swap => {
                        self.cross_monitor_move_behaviour = MoveBehaviour::Insert;
                    }
                    MoveBehaviour::Insert => {
                        self.cross_monitor_move_behaviour = MoveBehaviour::Swap;
                    }
                    _ => {}
                }
            }
            SocketMessage::CrossMonitorMoveBehaviour(behaviour) => {
                self.cross_monitor_move_behaviour = behaviour;
            }
            SocketMessage::DisplayIndexPreference(index_preference, ref display) => {
                let mut display_index_preferences = DISPLAY_INDEX_PREFERENCES.write();
                display_index_preferences.insert(index_preference, display.clone());
            }
            SocketMessage::NewWorkspace => {
                self.new_workspace()?;
            }
            SocketMessage::WorkspaceTiling(monitor_idx, workspace_idx, tile) => {
                self.set_workspace_tiling(monitor_idx, workspace_idx, tile)?;
            }
            SocketMessage::WorkspaceLayout(monitor_idx, workspace_idx, layout) => {
                self.set_workspace_layout_default(monitor_idx, workspace_idx, layout)?;
            }
            SocketMessage::NamedWorkspaceTiling(ref workspace, tile) => {
                if let Some((monitor_idx, workspace_idx)) =
                    self.monitor_workspace_index_by_name(workspace)
                {
                    self.set_workspace_tiling(monitor_idx, workspace_idx, tile)?;
                }
            }
            SocketMessage::WorkspaceName(monitor_idx, workspace_idx, ref name) => {
                self.set_workspace_name(monitor_idx, workspace_idx, name.to_string())?;
            }
            SocketMessage::NamedWorkspaceLayout(ref workspace, layout) => {
                if let Some((monitor_idx, workspace_idx)) =
                    self.monitor_workspace_index_by_name(workspace)
                {
                    self.set_workspace_layout_default(monitor_idx, workspace_idx, layout)?;
                }
            }
            SocketMessage::EnsureWorkspaces(monitor_idx, workspace_count) => {
                self.ensure_workspaces_for_monitor(monitor_idx, workspace_count)?;
            }
            SocketMessage::EnsureNamedWorkspaces(monitor_idx, ref names) => {
                self.ensure_named_workspaces_for_monitor(monitor_idx, names)?;
            }
            SocketMessage::ToggleFloat => self.toggle_float(false)?,
            SocketMessage::ResizeWindowEdge(direction, sizing) => {
                self.resize_window(direction, sizing, self.resize_delta, true)?;
            }
            SocketMessage::ResizeWindowAxis(axis, sizing) => {
                match axis {
                    Axis::Horizontal => {
                        self.resize_window(
                            OperationDirection::Left,
                            sizing,
                            self.resize_delta,
                            false,
                        )?;
                        self.resize_window(
                            OperationDirection::Right,
                            sizing,
                            self.resize_delta,
                            false,
                        )?;
                    }
                    Axis::Vertical => {
                        self.resize_window(
                            OperationDirection::Up,
                            sizing,
                            self.resize_delta,
                            false,
                        )?;
                        self.resize_window(
                            OperationDirection::Down,
                            sizing,
                            self.resize_delta,
                            false,
                        )?;
                    }
                    Axis::HorizontalAndVertical => {
                        self.resize_window(
                            OperationDirection::Left,
                            sizing,
                            self.resize_delta,
                            false,
                        )?;
                        self.resize_window(
                            OperationDirection::Right,
                            sizing,
                            self.resize_delta,
                            false,
                        )?;
                        self.resize_window(
                            OperationDirection::Up,
                            sizing,
                            self.resize_delta,
                            false,
                        )?;
                        self.resize_window(
                            OperationDirection::Down,
                            sizing,
                            self.resize_delta,
                            false,
                        )?;
                    }
                }

                self.update_focused_workspace(false, false)?;
            }
            SocketMessage::Minimize => {
                let foreground_window = backend()
                    .foreground_window()
                    .ok_or_eyre("there is no foreground window")?;
                Window::from(foreground_window).minimize()?;
            }
            SocketMessage::Close => {
                let foreground_window = backend()
                    .foreground_window()
                    .ok_or_eyre("there is no foreground window")?;
                Window::from(foreground_window).close()?;
            }
            SocketMessage::ManageFocusedWindow => {
                self.manage_focused_window()?;
            }
            SocketMessage::UnmanageFocusedWindow => {
                self.unmanage_focused_window()?;
            }
            SocketMessage::LockMonitorWorkspaceContainer(
                monitor_idx,
                workspace_idx,
                container_idx,
            ) => {
                let monitor = self
                    .monitors_mut()
                    .get_mut(monitor_idx)
                    .ok_or_eyre("no monitor at the given index")?;

                let workspace = monitor
                    .workspaces_mut()
                    .get_mut(workspace_idx)
                    .ok_or_eyre("no workspace at the given index")?;

                if let Some(container) = workspace.containers_mut().get_mut(container_idx) {
                    container.locked = true;
                }
            }
            SocketMessage::UnlockMonitorWorkspaceContainer(
                monitor_idx,
                workspace_idx,
                container_idx,
            ) => {
                let monitor = self
                    .monitors_mut()
                    .get_mut(monitor_idx)
                    .ok_or_eyre("no monitor at the given index")?;

                let workspace = monitor
                    .workspaces_mut()
                    .get_mut(workspace_idx)
                    .ok_or_eyre("no workspace at the given index")?;

                if let Some(container) = workspace.containers_mut().get_mut(container_idx) {
                    container.locked = false;
                }
            }
            SocketMessage::FlipLayout(layout_flip) => self.flip_layout(layout_flip)?,
            SocketMessage::ChangeLayout(layout) => self.change_workspace_layout_default(layout)?,
            SocketMessage::CycleLayout(direction) => self.cycle_layout(direction)?,
            SocketMessage::TogglePause => {
                if self.is_paused {
                    tracing::info!("resuming");
                } else {
                    tracing::info!("pausing");
                }

                self.is_paused = !self.is_paused;
            }
            SocketMessage::Retile => self.retile_all(false)?,
            SocketMessage::RetileWithResizeDimensions => self.retile_all(true)?,
            SocketMessage::ToggleLock => self.toggle_lock()?,
            SocketMessage::ToggleWindowBasedWorkAreaOffset => {
                let workspace = self.focused_workspace_mut()?;
                workspace.apply_window_based_work_area_offset =
                    !workspace.apply_window_based_work_area_offset;

                self.retile_all(true)?;
            }
            SocketMessage::ToggleTiling => {
                self.toggle_tiling()?;
            }
            SocketMessage::State => {
                let state = match serde_json::to_string_pretty(&State::from(&*self)) {
                    Ok(state) => state,
                    Err(error) => error.to_string(),
                };

                tracing::info!("replying to state");

                reply.write_all(state.as_bytes())?;

                tracing::info!("replying to state done");
            }
            SocketMessage::GlobalState => {
                let state = match serde_json::to_string_pretty(&GlobalState::default()) {
                    Ok(state) => state,
                    Err(error) => error.to_string(),
                };

                tracing::info!("replying to global state");

                reply.write_all(state.as_bytes())?;

                tracing::info!("replying to global state done");
            }
            SocketMessage::MonitorInformation => {
                let mut monitors = vec![];
                for monitor in self.monitors() {
                    monitors.push(MonitorInformation::from(monitor));
                }

                let monitors_state = serde_json::to_string_pretty(&monitors)
                    .unwrap_or_else(|error| error.to_string());

                reply.write_all(monitors_state.as_bytes())?;
            }
            SocketMessage::Query(query) => {
                let response = match query {
                    StateQuery::FocusedMonitorIndex => self.focused_monitor_idx().to_string(),
                    StateQuery::FocusedWorkspaceIndex => self
                        .focused_monitor()
                        .ok_or_eyre("there is no monitor")?
                        .focused_workspace_idx()
                        .to_string(),
                    StateQuery::FocusedContainerIndex => self
                        .focused_workspace()?
                        .focused_container_idx()
                        .to_string(),
                    StateQuery::FocusedWindowIndex => {
                        self.focused_container()?.focused_window_idx().to_string()
                    }
                    StateQuery::FocusedWorkspaceName => {
                        let focused_monitor =
                            self.focused_monitor().ok_or_eyre("there is no monitor")?;

                        focused_monitor
                            .focused_workspace_name()
                            .unwrap_or_else(|| focused_monitor.focused_workspace_idx().to_string())
                    }
                    StateQuery::Version => build::VERSION.to_string(),
                    StateQuery::FocusedWorkspaceLayout => {
                        let focused_monitor =
                            self.focused_monitor().ok_or_eyre("there is no monitor")?;

                        focused_monitor.focused_workspace_layout().map_or_else(
                            || "None".to_string(),
                            |layout| match layout {
                                Layout::Default(default_layout) => default_layout.to_string(),
                            },
                        )
                    }
                    StateQuery::FocusedContainerKind => {
                        match self.focused_workspace()?.focused_container() {
                            None => "None".to_string(),
                            Some(container) => {
                                if container.windows().len() > 1 {
                                    "Stack".to_string()
                                } else {
                                    "Single".to_string()
                                }
                            }
                        }
                    }
                };

                reply.write_all(response.as_bytes())?;
            }
            SocketMessage::SessionFloatRule => {
                let foreground_window = Window::from(
                    backend()
                        .foreground_window()
                        .ok_or_eyre("there is no foreground window")?,
                );
                let (exe, title, class) = (
                    foreground_window.exe(),
                    foreground_window.title(),
                    foreground_window.class(),
                );

                if let (Some(exe), Some(title), Some(class)) = (exe, title, class) {
                    let rule = MatchingRule::Composite(vec![
                        IdWithIdentifier {
                            kind: ApplicationIdentifier::Exe,
                            id: exe,
                            matching_strategy: Option::from(MatchingStrategy::Equals),
                        },
                        IdWithIdentifier {
                            kind: ApplicationIdentifier::Title,
                            id: title,
                            matching_strategy: Option::from(MatchingStrategy::Equals),
                        },
                        IdWithIdentifier {
                            kind: ApplicationIdentifier::Class,
                            id: class,
                            matching_strategy: Option::from(MatchingStrategy::Equals),
                        },
                    ]);

                    let mut floating_applications = FLOATING_APPLICATIONS.lock();
                    floating_applications.push(rule.clone());
                    let mut session_floating_applications = SESSION_FLOATING_APPLICATIONS.lock();
                    session_floating_applications.push(rule.clone());

                    self.toggle_float(true)?;
                }
            }
            SocketMessage::SessionFloatRules => {
                let session_floating_applications = SESSION_FLOATING_APPLICATIONS.lock();
                let rules = serde_json::to_string_pretty(&*session_floating_applications)
                    .unwrap_or_else(|error| error.to_string());

                reply.write_all(rules.as_bytes())?;
            }
            SocketMessage::ClearSessionFloatRules => {
                let mut floating_applications = FLOATING_APPLICATIONS.lock();
                let mut session_floating_applications = SESSION_FLOATING_APPLICATIONS.lock();
                floating_applications.retain(|r| !session_floating_applications.contains(r));
                session_floating_applications.clear()
            }
            SocketMessage::IgnoreRule(identifier, ref id) => {
                let mut ignore_identifiers = IGNORE_IDENTIFIERS.lock();

                let mut should_push = true;
                for i in &*ignore_identifiers {
                    if let MatchingRule::Simple(i) = i
                        && i.id.eq(id)
                    {
                        should_push = false;
                    }
                }

                if should_push {
                    ignore_identifiers.push(MatchingRule::Simple(IdWithIdentifier {
                        kind: identifier,
                        id: id.clone(),
                        matching_strategy: Option::from(MatchingStrategy::Legacy),
                    }));
                }

                let offset = self.work_area_offset;

                let mut window_ids_to_purge = vec![];
                for (i, monitor) in self.monitors().iter().enumerate() {
                    for container in monitor
                        .focused_workspace()
                        .ok_or_eyre("there is no workspace")?
                        .containers()
                    {
                        for window in container.windows() {
                            match identifier {
                                ApplicationIdentifier::Path => {
                                    if window.path().unwrap_or_default().to_string_lossy() == *id {
                                        window_ids_to_purge.push((i, window.id));
                                    }
                                }
                                ApplicationIdentifier::Exe => {
                                    if window.exe().unwrap_or_default() == *id {
                                        window_ids_to_purge.push((i, window.id));
                                    }
                                }
                                ApplicationIdentifier::Class => {
                                    if window.class().unwrap_or_default() == *id {
                                        window_ids_to_purge.push((i, window.id));
                                    }
                                }
                                ApplicationIdentifier::Title => {
                                    if window.title().unwrap_or_default() == *id {
                                        window_ids_to_purge.push((i, window.id));
                                    }
                                }
                            }
                        }
                    }
                }

                for (monitor_idx, id) in window_ids_to_purge {
                    let monitor = self
                        .monitors_mut()
                        .get_mut(monitor_idx)
                        .ok_or_eyre("there is no monitor")?;

                    monitor
                        .focused_workspace_mut()
                        .ok_or_eyre("there is no focused workspace")?
                        .remove_window(id)?;

                    monitor.update_focused_workspace(offset)?;
                }
            }
            SocketMessage::ManageRule(identifier, ref id) => {
                let mut manage_identifiers = MANAGE_IDENTIFIERS.lock();

                let mut should_push = true;
                for m in &*manage_identifiers {
                    if let MatchingRule::Simple(m) = m
                        && m.id.eq(id)
                    {
                        should_push = false;
                    }
                }

                if should_push {
                    manage_identifiers.push(MatchingRule::Simple(IdWithIdentifier {
                        kind: identifier,
                        id: id.clone(),
                        matching_strategy: Option::from(MatchingStrategy::Legacy),
                    }));
                }
            }
            SocketMessage::ContainerPadding(monitor_idx, workspace_idx, size) => {
                self.set_container_padding(monitor_idx, workspace_idx, size)?;
            }
            SocketMessage::NamedWorkspaceContainerPadding(ref workspace, size) => {
                if let Some((monitor_idx, workspace_idx)) =
                    self.monitor_workspace_index_by_name(workspace)
                {
                    self.set_container_padding(monitor_idx, workspace_idx, size)?;
                }
            }
            SocketMessage::WorkspacePadding(monitor_idx, workspace_idx, size) => {
                self.set_workspace_padding(monitor_idx, workspace_idx, size)?;
            }
            SocketMessage::NamedWorkspacePadding(ref workspace, size) => {
                if let Some((monitor_idx, workspace_idx)) =
                    self.monitor_workspace_index_by_name(workspace)
                {
                    self.set_workspace_padding(monitor_idx, workspace_idx, size)?;
                }
            }
            SocketMessage::FocusedWorkspaceContainerPadding(adjustment) => {
                let focused_monitor_idx = self.focused_monitor_idx();

                let focused_monitor = self.focused_monitor().ok_or_eyre("there is no monitor")?;

                let focused_workspace_idx = focused_monitor.focused_workspace_idx();

                self.set_container_padding(focused_monitor_idx, focused_workspace_idx, adjustment)?;
            }
            SocketMessage::FocusedWorkspacePadding(adjustment) => {
                let focused_monitor_idx = self.focused_monitor_idx();

                let focused_monitor = self.focused_monitor().ok_or_eyre("there is no monitor")?;

                let focused_workspace_idx = focused_monitor.focused_workspace_idx();

                self.set_workspace_padding(focused_monitor_idx, focused_workspace_idx, adjustment)?;
            }
            SocketMessage::WorkAreaOffset(rect) => {
                self.work_area_offset = Option::from(rect);
                self.retile_all(false)?;
            }
            SocketMessage::MonitorWorkAreaOffset(monitor_idx, rect) => {
                if let Some(monitor) = self.monitors_mut().get_mut(monitor_idx) {
                    monitor.work_area_offset = Option::from(rect);
                    self.retile_all(false)?;
                }
            }
            SocketMessage::WorkspaceWorkAreaOffset(monitor_idx, workspace_idx, rect) => {
                if let Some(monitor) = self.monitors_mut().get_mut(monitor_idx)
                    && let Some(workspace) = monitor.workspaces_mut().get_mut(workspace_idx)
                {
                    workspace.work_area_offset = Option::from(rect);
                    self.retile_all(false)?
                }
            }
            SocketMessage::ResizeDelta(delta) => {
                self.resize_delta = delta;
            }
            SocketMessage::AdjustContainerPadding(sizing, adjustment) => {
                self.adjust_container_padding(sizing, adjustment)?;
            }
            SocketMessage::AdjustWorkspacePadding(sizing, adjustment) => {
                self.adjust_workspace_padding(sizing, adjustment)?;
            }
            SocketMessage::ScrollingLayoutColumns(count) => {
                let focused_workspace = self.focused_workspace_mut()?;

                let options = match focused_workspace.layout_options {
                    Some(mut opts) => {
                        if let Some(scrolling) = &mut opts.scrolling {
                            scrolling.columns = count.into();
                        }

                        opts
                    }
                    None => LayoutOptions {
                        scrolling: Some(ScrollingLayoutOptions {
                            columns: count.into(),
                            center_focused_column: Default::default(),
                        }),
                        grid: None,
                        column_ratios: None,
                        row_ratios: None,
                    },
                };

                focused_workspace.layout_options = Some(options);
                self.update_focused_workspace(false, false)?;
            }
            SocketMessage::LayoutRatios(ref columns, ref rows) => {
                use crate::core::validate_ratios;

                let focused_workspace = self.focused_workspace_mut()?;

                let mut options = focused_workspace.layout_options.unwrap_or(LayoutOptions {
                    scrolling: None,
                    grid: None,
                    column_ratios: None,
                    row_ratios: None,
                });

                if let Some(cols) = columns {
                    options.column_ratios = Some(validate_ratios(cols));
                }

                if let Some(rws) = rows {
                    options.row_ratios = Some(validate_ratios(rws));
                }

                focused_workspace.layout_options = Some(options);
                self.update_focused_workspace(false, false)?;
            }
            SocketMessage::MouseFollowsFocus(enable) => {
                self.mouse_follows_focus = enable;
            }
            SocketMessage::ToggleMouseFollowsFocus => {
                self.mouse_follows_focus = !self.mouse_follows_focus;
            }
            SocketMessage::UnmanagedWindowOperationBehaviour(behaviour) => {
                self.unmanaged_window_operation_behaviour = behaviour;
            }
            SocketMessage::AddSubscriberSocket(ref socket) => {
                let mut sockets = SUBSCRIPTION_SOCKETS.lock();
                let socket_path = DATA_DIR.join(socket);
                sockets.insert(socket.clone(), socket_path);
            }
            SocketMessage::AddSubscriberSocketWithOptions(ref socket, options) => {
                let mut sockets = SUBSCRIPTION_SOCKETS.lock();
                let socket_path = DATA_DIR.join(socket);
                sockets.insert(socket.clone(), socket_path);

                let mut socket_options = SUBSCRIPTION_SOCKET_OPTIONS.lock();
                socket_options.insert(socket.clone(), options);
            }
            SocketMessage::RemoveSubscriberSocket(ref socket) => {
                let mut sockets = SUBSCRIPTION_SOCKETS.lock();
                sockets.remove(socket);
            }
            SocketMessage::Stop => {
                self.stop()?;
            }
            SocketMessage::QuickSave => {
                let workspace = self.focused_workspace()?;
                let resize = &workspace.resize_dimensions;

                let quicksave_json = std::env::temp_dir().join("komorebi.quicksave.json");

                let file = OpenOptions::new()
                    .write(true)
                    .truncate(true)
                    .create(true)
                    .open(quicksave_json)?;

                serde_json::to_writer_pretty(&file, &resize)?;
            }
            SocketMessage::QuickLoad => {
                let workspace = self.focused_workspace_mut()?;

                let quicksave_json = std::env::temp_dir().join("komorebi.quicksave.json");

                let file = File::open(&quicksave_json).wrap_err(format!(
                    "no quicksave found at {}",
                    quicksave_json.display()
                ))?;

                let resize: Vec<Option<Rect>> = serde_json::from_reader(file)?;

                workspace.resize_dimensions = resize;
                self.update_focused_workspace(false, false)?;
            }
            SocketMessage::Save(ref path) => {
                let workspace = self.focused_workspace_mut()?;
                let resize = &workspace.resize_dimensions;

                let file = OpenOptions::new()
                    .write(true)
                    .truncate(true)
                    .create(true)
                    .open(path)?;

                serde_json::to_writer_pretty(&file, &resize)?;
            }
            SocketMessage::Load(ref path) => {
                let workspace = self.focused_workspace_mut()?;

                let file =
                    File::open(path).wrap_err(format!("no file found at {}", path.display()))?;

                let resize: Vec<Option<Rect>> = serde_json::from_reader(file)?;

                workspace.resize_dimensions = resize;
                self.update_focused_workspace(false, false)?;
            }
            SocketMessage::DebugWindow(window_id) => {
                let window = Window::from(window_id);
                if window.is_valid() {
                    let mut rule_debug = RuleDebug::default();
                    let _ = window.should_manage(None, &mut rule_debug);
                    let schema = serde_json::to_string_pretty(&rule_debug)?;

                    reply.write_all(schema.as_bytes())?;
                }
            }
        }

        self.update_known_window_ids();

        notify_subscribers(
            Notification {
                event: NotificationEvent::Socket(message.clone()),
                state: self.as_ref().into(),
            },
            initial_state.has_been_modified(self.as_ref()),
        )?;

        tracing::info!("processed command: {message}");

        Ok(())
    }
}

/// Reads the messages on one connection, one per line, and hands each to
/// [`WindowManager::process_command`] with the connection to answer on
///
/// A message that arrives while the window manager has been locked for a second is logged and
/// dropped. While the window manager is paused, the first message that is not `TogglePause`,
/// `State`, `GlobalState` or `Stop` is dropped, and the first that is one of them is handled;
/// either way, the lines after it are not read.
///
/// # Errors
///
/// - The connection cannot be read, or cloned to be read and written at once.
/// - A line is not a [`SocketMessage`]: the lines after it are not read.
/// - [`WindowManager::process_command`] fails: the lines after it are not read.
pub fn read_commands_uds(
    wm: &Arc<Mutex<WindowManager>>,
    mut stream: UnixStream,
) -> eyre::Result<()> {
    let reader = BufReader::new(stream.try_clone()?);
    // TODO(raggi): while this processes more than one command, if there are
    // replies there is no clearly defined protocol for framing yet - it's
    // perhaps whole-json objects for now, but termination is signalled by
    // socket shutdown.
    for line in reader.lines() {
        let message = SocketMessage::from_str(&line?)?;

        match wm.try_lock_for(Duration::from_secs(1)) {
            None => {
                tracing::warn!(
                    "could not acquire window manager lock, not processing message: {message}"
                );
            }
            Some(mut wm) => {
                if wm.is_paused {
                    return match message {
                        SocketMessage::TogglePause
                        | SocketMessage::State
                        | SocketMessage::GlobalState
                        | SocketMessage::Stop => Ok(wm.process_command(message, &mut stream)?),
                        _ => {
                            tracing::trace!("ignoring while paused");
                            Ok(())
                        }
                    };
                }

                wm.process_command(message.clone(), &mut stream)?;
            }
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::backend::fake::FakeBackend;
    use crate::backend::with_backend;
    use crate::core::Rect;
    use crate::monitor;
    use crate::window_manager_event::WindowManagerEvent;
    use crossbeam_channel::Receiver;
    use crossbeam_channel::Sender;
    use crossbeam_channel::bounded;
    use std::io::Read;
    use std::io::Write;
    use std::net::Shutdown;
    use std::path::PathBuf;
    use uuid::Uuid;

    /// A window manager over `fake`, listening on a socket for this test alone in /tmp, which is
    /// short enough for a unix socket path on macOS. The socket is removed when the test is done.
    struct Daemon {
        fake: Arc<FakeBackend>,
        wm: Arc<Mutex<WindowManager>>,
        socket: PathBuf,
    }

    impl Drop for Daemon {
        fn drop(&mut self) {
            let _ = std::fs::remove_file(&self.socket);
        }
    }

    impl Daemon {
        fn new(fake: FakeBackend) -> Self {
            let fake = Arc::new(fake);
            let socket = PathBuf::from(format!("/tmp/komorebi-test-{}.sock", Uuid::new_v4()));

            let (_sender, events) = crossbeam_channel::bounded(1);

            let wm = with_backend(fake.clone(), || {
                let mut wm = WindowManager::new(events, Some(socket.clone())).unwrap();
                wm.init().unwrap();
                wm
            });

            Self {
                fake,
                wm: Arc::new(Mutex::new(wm)),
                socket,
            }
        }

        /// Writes `lines` on a new connection, has the window manager read the connection the
        /// way the daemon does, and returns what it wrote back
        fn send(&self, lines: &str) -> eyre::Result<String> {
            let mut client = UnixStream::connect(&self.socket)?;
            client.write_all(lines.as_bytes())?;
            client.shutdown(Shutdown::Write)?;

            // the client reads on another thread, so that an answer bigger than the
            // socket's buffer does not block the window manager writing it
            let reading = std::thread::spawn(move || {
                let mut reply = String::new();
                client.read_to_string(&mut reply).map(|_| reply)
            });

            let (stream, _) = self.wm.lock().command_listener.accept()?;
            let handled = with_backend(self.fake.clone(), || read_commands_uds(&self.wm, stream));

            let reply = reading.join().expect("the client does not panic")?;
            handled.map(|()| reply)
        }

        fn ask(&self, message: SocketMessage) -> eyre::Result<String> {
            self.send(&serde_json::to_string(&message)?)
        }
    }

    fn rect(left: i32, top: i32, right: i32, bottom: i32) -> Rect {
        Rect {
            left,
            top,
            right,
            bottom,
        }
    }

    fn two_windows(first: u32) -> FakeBackend {
        let fake = FakeBackend::with_one_monitor();
        fake.open_window(first, "foot", "foot", "first");
        fake.open_window(first + 1, "foot", "foot", "second");
        fake
    }

    #[test]
    fn the_state_has_every_window_of_every_workspace() -> eyre::Result<()> {
        let daemon = Daemon::new(two_windows(701));

        let state: serde_json::Value = serde_json::from_str(&daemon.ask(SocketMessage::State)?)?;

        let windows = &state["monitors"]["elements"][0]["workspaces"]["elements"][0]["containers"]
            ["elements"];
        assert_eq!(windows[0]["windows"]["elements"][0]["id"], 701);
        assert_eq!(windows[1]["windows"]["elements"][0]["id"], 702);
        assert_eq!(
            windows[1]["windows"]["elements"][0]["details"]["title"],
            "second"
        );
        assert_eq!(state["mouse_follows_focus"], true);
        Ok(())
    }

    #[test]
    fn monitor_information_names_each_monitor_and_its_bounds() -> eyre::Result<()> {
        let fake = FakeBackend::with_one_monitor();
        fake.add_monitor(
            2,
            "Virtual-1",
            rect(1920, 0, 1920, 1080),
            rect(1920, 0, 1920, 1080),
        );
        let daemon = Daemon::new(fake);

        let monitors: serde_json::Value =
            serde_json::from_str(&daemon.ask(SocketMessage::MonitorInformation)?)?;

        assert_eq!(monitors.as_array().map(Vec::len), Some(2));
        assert_eq!(monitors[1]["id"], 2);
        assert_eq!(monitors[1]["device"], "Virtual-1");
        assert_eq!(monitors[1]["size"]["left"], 1920);
        assert_eq!(monitors[0]["size"]["right"], 1920);
        Ok(())
    }

    #[test]
    fn a_query_is_answered_with_one_value() -> eyre::Result<()> {
        let daemon = Daemon::new(two_windows(711));
        let query = |query| daemon.ask(SocketMessage::Query(query));

        assert_eq!(query(StateQuery::FocusedMonitorIndex)?, "0");
        assert_eq!(query(StateQuery::FocusedWorkspaceIndex)?, "0");
        assert_eq!(query(StateQuery::FocusedContainerIndex)?, "0");
        assert_eq!(query(StateQuery::FocusedWindowIndex)?, "0");
        // a workspace with no name is named by its index
        assert_eq!(query(StateQuery::FocusedWorkspaceName)?, "0");
        assert_eq!(query(StateQuery::FocusedWorkspaceLayout)?, "BSP");
        assert_eq!(query(StateQuery::FocusedContainerKind)?, "Single");
        // the version is shadow-rs's description of the build, the package version first
        let version = format!("pkg_version:{}", env!("CARGO_PKG_VERSION"));
        assert!(query(StateQuery::Version)?.contains(&version));
        Ok(())
    }

    #[test]
    fn a_query_about_a_container_on_an_empty_workspace_is_an_error() {
        let daemon = Daemon::new(FakeBackend::with_one_monitor());

        let answer = daemon.ask(SocketMessage::Query(StateQuery::FocusedWindowIndex));

        assert_eq!(
            answer.map_err(|error| error.to_string()),
            Err(String::from("there is no container"))
        );
    }

    #[test]
    fn retile_puts_a_window_moved_by_hand_back_on_its_tile() -> eyre::Result<()> {
        let daemon = Daemon::new(two_windows(721));
        daemon.ask(SocketMessage::Retile)?;
        let tile = daemon.fake.rect_of(721);

        daemon
            .fake
            .move_window_by_user(721, rect(300, 300, 400, 400));
        assert_ne!(daemon.fake.rect_of(721), tile);

        assert_eq!(daemon.ask(SocketMessage::Retile)?, "");
        assert_eq!(daemon.fake.rect_of(721), tile);
        Ok(())
    }

    #[test]
    fn the_messages_on_one_connection_are_handled_in_order() -> eyre::Result<()> {
        let daemon = Daemon::new(two_windows(731));
        daemon
            .fake
            .move_window_by_user(731, rect(300, 300, 400, 400));

        let reply = daemon.send(
            "{\"type\":\"Retile\"}\n{\"type\":\"Query\",\"content\":\"FocusedContainerKind\"}\n",
        )?;

        assert_eq!(reply, "Single");
        assert_ne!(daemon.fake.rect_of(731), Some(rect(300, 300, 400, 400)));
        Ok(())
    }

    #[test]
    fn while_paused_only_toggle_pause_state_and_stop_are_handled() -> eyre::Result<()> {
        let daemon = Daemon::new(two_windows(741));
        daemon.ask(SocketMessage::Retile)?;
        let tile = daemon.fake.rect_of(741);
        daemon.ask(SocketMessage::TogglePause)?;
        daemon
            .fake
            .move_window_by_user(741, rect(300, 300, 400, 400));

        assert_eq!(daemon.ask(SocketMessage::Retile)?, "");
        assert_eq!(daemon.fake.rect_of(741), Some(rect(300, 300, 400, 400)));
        // the message that is dropped is the last one read on its connection
        assert_eq!(
            daemon.send("{\"type\":\"Retile\"}\n{\"type\":\"State\"}\n")?,
            ""
        );
        let state: serde_json::Value = serde_json::from_str(&daemon.ask(SocketMessage::State)?)?;
        assert_eq!(state["is_paused"], true);

        daemon.ask(SocketMessage::TogglePause)?;
        daemon.ask(SocketMessage::Retile)?;
        assert_eq!(daemon.fake.rect_of(741), tile);
        Ok(())
    }

    #[test]
    fn a_line_that_is_not_a_message_is_an_error() {
        let daemon = Daemon::new(FakeBackend::with_one_monitor());

        let reply = daemon.send("{\"type\":\"Nonsense\"}\n");

        assert!(reply.is_err());
    }

    fn send_socket_message(socket: &PathBuf, message: SocketMessage) {
        let mut stream = UnixStream::connect(socket).unwrap();
        stream
            .set_write_timeout(Some(Duration::from_secs(1)))
            .unwrap();
        stream
            .write_all(serde_json::to_string(&message).unwrap().as_bytes())
            .unwrap();
    }

    #[test]
    fn test_receive_socket_message() {
        let (_sender, receiver): (Sender<WindowManagerEvent>, Receiver<WindowManagerEvent>) =
            bounded(1);
        // /tmp: short enough for a unix socket path on macOS, and writable wherever the tests run
        let socket_name = format!("/tmp/komorebi-test-{}.sock", Uuid::new_v4());
        let socket_path = PathBuf::from(&socket_name);
        let mut wm = WindowManager::new(receiver, Some(socket_path.clone())).unwrap();
        let m = monitor::new(
            0,
            Rect::default(),
            Rect::default(),
            "TestMonitor".to_string(),
            "TestDeviceID".to_string(),
        );

        wm.monitors_mut().push_back(m);

        // send a message
        send_socket_message(&socket_path, SocketMessage::FocusWorkspaceNumber(5));

        let (stream, _) = wm.command_listener.accept().unwrap();
        let reader = BufReader::new(stream.try_clone().unwrap());
        let next = reader.lines().next();

        // read and deserialize the message
        let message_string = next.unwrap().unwrap();
        let message = SocketMessage::from_str(&message_string).unwrap();
        assert!(matches!(message, SocketMessage::FocusWorkspaceNumber(5)));

        // process the message
        wm.process_command(message, stream).unwrap();

        // check the updated window manager state
        assert_eq!(wm.focused_workspace_idx().unwrap(), 5);

        std::fs::remove_file(socket_path).unwrap();
    }
}
