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
//! - **VisibleWindows**: what the compositor says about each window that can be seen on the
//!   workspace each monitor shows, by the id of the monitor, as JSON.
//! - **Query**: one value as text, the answer to a [`StateQuery`].
//! - **GlobalState**: the ignore and the manage rules and the data directory ([`GlobalState`]), as
//!   JSON.
//! - **SessionFloatRules**: the float rules **SessionFloatRule** added, as JSON.
//! - **DebugWindow**: what [`Window::should_manage`] read of a window and which rules matched it
//!   ([`RuleDebug`]), as JSON, or nothing for a window that is not there.
//! - **GenerateStaticConfig**: the window manager's settings as a `komorebi.json`
//!   ([`StaticConfig`]), as JSON.
//! - **StaticConfigSchema**, **ApplicationSpecificConfigurationSchema**, **NotificationSchema**
//!   and **SocketSchema**: the JSON Schema of `komorebi.json`, of a list of applications files,
//!   of a [`Notification`] and of a [`SocketMessage`]. They answer nothing when the daemon is
//!   built without the `schemars` feature.
//!
//! The others answer nothing:
//!
//! - **FocusWindow** and **CycleFocusWindow** give the focus to the container on one side of the
//!   focused one (or of the monitor on that side, at the edge of a monitor), or to the next or the
//!   previous one in the workspace
//!   ([`WindowManager::focus_container_in_direction`],
//!   [`WindowManager::focus_container_in_cycle_direction`]). On the floating layer they give it
//!   to a floating window instead ([`WindowManager::focus_floating_window_in_direction`],
//!   [`WindowManager::focus_floating_window_in_cycle_direction`]).
//! - **MoveWindow** and **CycleMoveWindow** swap the focused container with the one on one side
//!   of it (or take it to the monitor on that side, at the edge of a monitor, as
//!   **CrossMonitorMoveBehaviour** and **ToggleCrossMonitorMoveBehaviour** set), or with the next or
//!   the previous one, and tile the workspace again
//!   ([`WindowManager::move_container_in_direction`],
//!   [`WindowManager::move_container_in_cycle_direction`]). On the floating layer **MoveWindow**
//!   moves the focused floating window instead
//!   ([`WindowManager::move_floating_window_in_direction`]).
//! - **StackWindow** puts the focused window and the window in the container on one side of it in
//!   one container, a stack that shows the window that moved and hides the other, and
//!   **UnstackWindow** takes the focused window out of its stack again
//!   ([`WindowManager::add_window_to_container`],
//!   [`WindowManager::remove_window_from_container`]). **StackAll** puts every window of the
//!   workspace in one stack, and **UnstackAll** takes every window out of the focused one
//!   ([`WindowManager::stack_all`], [`WindowManager::unstack_all`]). **CycleStack** brings the next
//!   or the previous window of the focused stack to the front, **CycleStackIndex** moves the front
//!   window along the stack, and **FocusStackWindow** brings the window at an index to the front,
//!   on the monitor the pointer is on ([`WindowManager::cycle_container_window_in_direction`],
//!   [`WindowManager::cycle_container_window_index_in_direction`],
//!   [`WindowManager::focus_container_window`]).
//! - **ToggleWindowContainerBehaviour** and **ToggleWorkspaceWindowContainerBehaviour** set whether
//!   a window that opens gets a container of its own or joins the focused container, for every
//!   workspace or for the focused one, and **ToggleFloatOverride** and
//!   **ToggleWorkspaceFloatOverride** whether it floats
//!   ([`WindowManager::window_management_behaviour`]).
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
//! - **InitialWorkspaceRule**, **InitialNamedWorkspaceRule**, **WorkspaceRule** and
//!   **NamedWorkspaceRule** add a rule that sends the windows of an application to a workspace,
//!   given by its indices or by its name: when they open for the first time, or always;
//!   **ClearWorkspaceRules**, **ClearNamedWorkspaceRules** and **ClearAllWorkspaceRules** take
//!   rules away, and **EnforceWorkspaceRules** applies all of them to the windows that are open
//!   ([`WindowManager::enforce_workspace_rules`]).
//! - **WorkspaceLayoutRule** and **NamedWorkspaceLayoutRule** make a workspace change to a layout
//!   once it has so many containers, and **ClearWorkspaceLayoutRules** and
//!   **ClearNamedWorkspaceLayoutRules** take its rules away
//!   ([`WindowManager::add_workspace_layout_default_rule`],
//!   [`WindowManager::clear_workspace_layout_rules`]).
//! - **ReloadStaticConfiguration** applies a `komorebi.json` to the window manager
//!   ([`WindowManager::reload_static_configuration`]). The daemon sends itself this message when
//!   the file it was started with changes. **ReplaceConfiguration** makes the window manager
//!   again from a `komorebi.json`, which loads the monitors and the windows from the backend as
//!   the daemon does when it starts, and watches that file instead of the one it had. The old
//!   window manager is paused and brings back the windows it hid (the back of a stack), so that
//!   every window is in view for the new one to find; a file that is not a `komorebi.json` is
//!   logged and changes nothing.
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
//! - **ToggleWorkspaceLayer** switches the focused workspace between its tiling layer and its
//!   floating layer, which brings the windows of that layer to the front and gives the focus to
//!   one of them.
//! - **ToggleMonocle** gives the focused container the whole work area, with the others out of
//!   the layout and out of sight, or puts it back ([`WindowManager::toggle_monocle`]).
//!   **MonocleFocusBehaviour** sets what focusing in a direction does meanwhile, cycle the
//!   monocle or nothing, and **ToggleMonocleFocusBehaviour** switches between the two.
//! - **ToggleMaximize** has the compositor maximize the focused window, which leaves the layout,
//!   or restores it and puts it back in its tile ([`WindowManager::toggle_maximize`]). While a
//!   window is maximized, the messages about the tiles (**MoveWindow**, **CycleMoveWindow**, the
//!   stack messages, the resize, promote and preselect messages, **ToggleLock**, **ToggleFloat**,
//!   **ToggleMonocle** and the ones that send a container to another monitor or workspace) are
//!   ignored.
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
//!   or on the floating layer the focused floating window, and **QuickSave**, **QuickLoad**,
//!   **Save** and **Load** write the resize adjustments of the focused workspace's containers to
//!   a file and read them back.
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
//! - **Border** sets whether the compositor draws a border around each window, and **BorderWidth**
//!   and **BorderOffset** the room the layout leaves for it: each asks the backend for the look
//!   again ([`border_manager::send_notification`]), as does every message once it has been
//!   handled. **BorderColour** sets the color of the border of one kind of window
//!   ([`WindowKind`]), and every message, once handled, tells the backend the color of each
//!   window's border ([`border_manager::update_colours`]).
//! - **Animation** sets whether windows are animated going to their places, **AnimationDuration**
//!   for how long, and **AnimationFps** the frame rate that is stored and has nothing to map to
//!   (the compositor animates at the refresh rate of the screen); each is for every kind of
//!   animation or, given an [`AnimationPrefix`](crate::animation::AnimationPrefix), for one. The
//!   look asked of the backend after the message has the animation.
//! - **Retile** tiles the focused workspace of every monitor again, without the resize
//!   adjustments, and **RetileWithResizeDimensions** with them.
//! - **AddSubscriberSocket** and **AddSubscriberSocketWithOptions** add a subscriber by the name
//!   of its socket in the data directory, and **RemoveSubscriberSocket** removes one.
//! - **Stop** stops the daemon, after it has brought back the windows it hid, and
//!   **StopIgnoreRestore** stops it without ([`WindowManager::stop`]).
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
use crate::WORKSPACE_MATCHING_RULES;
use crate::animation::ANIMATION_DURATION_GLOBAL;
use crate::animation::ANIMATION_DURATION_PER_ANIMATION;
use crate::animation::ANIMATION_ENABLED_GLOBAL;
use crate::animation::ANIMATION_ENABLED_PER_ANIMATION;
use crate::animation::ANIMATION_FPS;
use crate::backend::backend;
use crate::border_manager;
use crate::core::ApplicationIdentifier;
use crate::core::Axis;
use crate::core::Layout;
use crate::core::LayoutOptions;
use crate::core::MonocleFocusBehaviour;
use crate::core::MoveBehaviour;
use crate::core::OperationDirection;
use crate::core::Rect;
use crate::core::ScrollingLayoutOptions;
use crate::core::SocketMessage;
use crate::core::StateQuery;
use crate::core::WindowContainerBehaviour;
use crate::core::WindowKind;
use crate::core::config_generation::IdWithIdentifier;
use crate::core::config_generation::MatchingRule;
use crate::core::config_generation::MatchingStrategy;
use crate::core::config_generation::WorkspaceMatchingRule;
use crate::monitor::MonitorInformation;
use crate::notify_subscribers;
use crate::state::GlobalState;
use crate::state::State;
use crate::static_config::StaticConfig;
use crate::window::RuleDebug;
use crate::window::Window;
use crate::window_manager::WindowManager;
use crate::window_manager_event_listener;
use crate::workspace::WorkspaceLayer;
use crate::workspace::WorkspaceWindowLocation;
use color_eyre::eyre;
use color_eyre::eyre::Context;
use color_eyre::eyre::OptionExt;
use parking_lot::Mutex;
use std::collections::HashMap;
use std::fs::File;
use std::fs::OpenOptions;
use std::io::BufRead;
use std::io::BufReader;
use std::num::NonZeroUsize;
use std::os::unix::net::UnixStream;
use std::str::FromStr;
use std::sync::Arc;
use std::sync::atomic::Ordering;
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

/// Commands that are about the tiles of the layout (moving, stacking, resizing, promoting,
/// locking, floating, the monocle, and sending containers to other monitors and workspaces), which
/// a maximized window has left
fn is_layout_command(message: &SocketMessage) -> bool {
    matches!(
        message,
        SocketMessage::MoveWindow(_)
            | SocketMessage::CycleMoveWindow(_)
            | SocketMessage::StackWindow(_)
            | SocketMessage::UnstackWindow
            | SocketMessage::CycleStack(_)
            | SocketMessage::CycleStackIndex(_)
            | SocketMessage::FocusStackWindow(_)
            | SocketMessage::StackAll
            | SocketMessage::UnstackAll
            | SocketMessage::ResizeWindowEdge(..)
            | SocketMessage::ResizeWindowAxis(..)
            | SocketMessage::PreselectDirection(_)
            | SocketMessage::Promote
            | SocketMessage::PromoteSwap
            | SocketMessage::PromoteFocus
            | SocketMessage::PromoteWindow(_)
            | SocketMessage::ToggleLock
            | SocketMessage::ToggleFloat
            | SocketMessage::ToggleMonocle
            | SocketMessage::MoveContainerToLastWorkspace
            | SocketMessage::SendContainerToLastWorkspace
            | SocketMessage::MoveContainerToMonitorNumber(_)
            | SocketMessage::CycleMoveContainerToMonitor(_)
            | SocketMessage::MoveContainerToWorkspaceNumber(_)
            | SocketMessage::MoveContainerToNamedWorkspace(_)
            | SocketMessage::CycleMoveContainerToWorkspace(_)
            | SocketMessage::SendContainerToMonitorNumber(_)
            | SocketMessage::CycleSendContainerToMonitor(_)
            | SocketMessage::SendContainerToWorkspaceNumber(_)
            | SocketMessage::CycleSendContainerToWorkspace(_)
            | SocketMessage::SendContainerToMonitorWorkspaceNumber(..)
            | SocketMessage::MoveContainerToMonitorWorkspaceNumber(..)
            | SocketMessage::SendContainerToNamedWorkspace(_)
    )
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
    /// - The monocle cannot be turned on or off, for one of the reasons
    ///   [`WindowManager::toggle_monocle`] gives.
    /// - The focused window cannot be maximized or restored, for one of the reasons
    ///   [`WindowManager::toggle_maximize`] gives.
    /// - The focused window cannot be stacked, unstacked or cycled, for one of the reasons
    ///   [`WindowManager::add_window_to_container`], [`WindowManager::remove_window_from_container`],
    ///   [`WindowManager::stack_all`], [`WindowManager::unstack_all`],
    ///   [`WindowManager::cycle_container_window_in_direction`],
    ///   [`WindowManager::cycle_container_window_index_in_direction`] and
    ///   [`WindowManager::focus_container_window`] give, or the focused
    ///   workspace is not there for a behavior to be set on (`ToggleWorkspaceWindowContainerBehaviour`,
    ///   `ToggleWorkspaceFloatOverride`).
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
    /// - `Stop` or `StopIgnoreRestore` fails, for the reason [`WindowManager::stop`] gives.
    /// - The notification cannot be sent, for one of the reasons [`notify_subscribers`] gives.
    #[tracing::instrument(skip(self, reply, message))]
    pub fn process_command(
        &mut self,
        message: SocketMessage,
        mut reply: impl std::io::Write,
    ) -> eyre::Result<()> {
        tracing::info!("processing command: {message}");

        // a window that is maximized (by `toggle-maximize`) is not in the layout, and what is
        // asked of the layout is not asked of it: nothing happens until it is restored, by
        // `toggle-maximize`
        if is_layout_command(&message)
            && self
                .focused_workspace()
                .is_ok_and(|workspace| workspace.maximized_window.is_some())
        {
            tracing::info!("ignoring {message}: the window in front is maximized");
            return Ok(());
        }

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
                        WorkspaceWindowLocation::Monocle(window_idx) => {
                            self.focus_container_window(window_idx)?;
                        }
                        WorkspaceWindowLocation::Maximized => {
                            if let Some(window) =
                                &mut self.focused_workspace_mut()?.maximized_window
                            {
                                window.focus(mouse_follows_focus)?;
                            }
                        }
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
                let focused_workspace = self.focused_workspace()?;
                match focused_workspace.layer {
                    WorkspaceLayer::Tiling => {
                        self.focus_container_in_direction(direction)?;
                    }
                    WorkspaceLayer::Floating => {
                        self.focus_floating_window_in_direction(direction)?;
                    }
                }
            }
            SocketMessage::PreselectDirection(direction) => {
                let focused_workspace = self.focused_workspace()?;
                let mut update = false;

                if focused_workspace.preselected_container_idx.is_some() {
                    tracing::warn!(
                        "ignoring command as this workspace already has a direction preselect set"
                    );
                } else if matches!(focused_workspace.layer, WorkspaceLayer::Tiling) {
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
                let focused_workspace = self.focused_workspace()?;
                match focused_workspace.layer {
                    WorkspaceLayer::Tiling => {
                        self.move_container_in_direction(direction)?;
                    }
                    WorkspaceLayer::Floating => {
                        self.move_floating_window_in_direction(direction)?;
                    }
                }
            }
            SocketMessage::CycleFocusWindow(direction) => {
                let focused_workspace = self.focused_workspace()?;
                match focused_workspace.layer {
                    WorkspaceLayer::Tiling => {
                        self.focus_container_in_cycle_direction(direction)?;
                    }
                    WorkspaceLayer::Floating => {
                        self.focus_floating_window_in_cycle_direction(direction)?;
                    }
                }
            }
            SocketMessage::CycleMoveWindow(direction) => {
                self.move_container_in_cycle_direction(direction)?;
            }
            SocketMessage::StackWindow(direction) => {
                let restore = ANIMATION_ENABLED_GLOBAL.swap(false, Ordering::SeqCst);
                self.add_window_to_container(direction)?;
                ANIMATION_ENABLED_GLOBAL.store(restore, Ordering::SeqCst);
            }
            SocketMessage::UnstackWindow => {
                let restore = ANIMATION_ENABLED_GLOBAL.swap(false, Ordering::SeqCst);
                self.remove_window_from_container()?;
                border_manager::destroy_all_borders()?;
                ANIMATION_ENABLED_GLOBAL.store(restore, Ordering::SeqCst);
            }
            SocketMessage::StackAll => {
                let restore = ANIMATION_ENABLED_GLOBAL.swap(false, Ordering::SeqCst);
                self.stack_all()?;
                ANIMATION_ENABLED_GLOBAL.store(restore, Ordering::SeqCst);
            }
            SocketMessage::UnstackAll => {
                let restore = ANIMATION_ENABLED_GLOBAL.swap(false, Ordering::SeqCst);
                self.unstack_all(true)?;
                border_manager::destroy_all_borders()?;
                ANIMATION_ENABLED_GLOBAL.store(restore, Ordering::SeqCst);
            }
            SocketMessage::CycleStack(direction) => {
                let restore = ANIMATION_ENABLED_GLOBAL.swap(false, Ordering::SeqCst);
                self.cycle_container_window_in_direction(direction)?;
                ANIMATION_ENABLED_GLOBAL.store(restore, Ordering::SeqCst);
            }
            SocketMessage::CycleStackIndex(direction) => {
                let restore = ANIMATION_ENABLED_GLOBAL.swap(false, Ordering::SeqCst);
                self.cycle_container_window_index_in_direction(direction)?;
                ANIMATION_ENABLED_GLOBAL.store(restore, Ordering::SeqCst);
            }
            SocketMessage::FocusStackWindow(idx) => {
                // In case you are using this command on a bar on a monitor
                // different from the currently focused one, you'd want that
                // monitor to be focused so that the FocusStackWindow happens
                // on the monitor with the bar you just pressed.
                if let Some(monitor_idx) = self.monitor_idx_from_current_pos() {
                    self.focus_monitor(monitor_idx)?;
                }
                let restore = ANIMATION_ENABLED_GLOBAL.swap(false, Ordering::SeqCst);
                self.focus_container_window(idx)?;
                ANIMATION_ENABLED_GLOBAL.store(restore, Ordering::SeqCst);
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
            SocketMessage::ReloadStaticConfiguration(ref pathbuf) => {
                self.reload_static_configuration(pathbuf)?;
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
            SocketMessage::ReplaceConfiguration(ref config) => {
                // Check that this is a valid static config file first
                if StaticConfig::read(config).is_ok() {
                    // Clear workspace rules; these will need to be replaced
                    WORKSPACE_MATCHING_RULES.lock().clear();
                    // Pause so that restored windows come to the foreground from all workspaces
                    self.is_paused = true;
                    // Bring all windows to the foreground
                    self.restore_all_windows(false)?;

                    // Create a new wm from the config path
                    let mut wm = StaticConfig::preload(
                        config,
                        window_manager_event_listener::event_rx(),
                        self.command_listener.try_clone().ok(),
                    )?;

                    // Initialize the new wm
                    wm.init()?;

                    wm.restore_all_windows(true)?;

                    // This is equivalent to StaticConfig::postload for this use case
                    StaticConfig::reload(config, &mut wm)?;

                    // Set self to the new wm instance
                    *self = wm;
                } else {
                    tracing::warn!(
                        "not replacing the configuration: {} is not a komorebi.json",
                        config.display()
                    );
                }
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
            SocketMessage::WorkspaceLayoutRule(
                monitor_idx,
                workspace_idx,
                at_container_count,
                layout,
            ) => {
                self.add_workspace_layout_default_rule(
                    monitor_idx,
                    workspace_idx,
                    at_container_count,
                    layout,
                )?;
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
            SocketMessage::ClearWorkspaceLayoutRules(monitor_idx, workspace_idx) => {
                self.clear_workspace_layout_rules(monitor_idx, workspace_idx)?;
            }
            SocketMessage::EnsureWorkspaces(monitor_idx, workspace_count) => {
                self.ensure_workspaces_for_monitor(monitor_idx, workspace_count)?;
            }
            SocketMessage::EnsureNamedWorkspaces(monitor_idx, ref names) => {
                self.ensure_named_workspaces_for_monitor(monitor_idx, names)?;
            }
            SocketMessage::ToggleMonocle => self.toggle_monocle()?,
            SocketMessage::ToggleMaximize => self.toggle_maximize()?,
            SocketMessage::ToggleFloat => self.toggle_float(false)?,
            SocketMessage::ToggleWorkspaceLayer => {
                let mouse_follows_focus = self.mouse_follows_focus;
                let workspace = self.focused_workspace_mut()?;

                let mut to_focus = None;
                match workspace.layer {
                    WorkspaceLayer::Tiling => {
                        workspace.layer = WorkspaceLayer::Floating;
                        tracing::info!("WorkspaceLayer is now Floating");

                        let focused_idx = workspace.focused_floating_window_idx();
                        let mut window_idx_pairs = workspace
                            .floating_windows_mut()
                            .make_contiguous()
                            .iter_mut()
                            .enumerate()
                            .collect::<Vec<_>>();

                        // Sort by window area
                        window_idx_pairs.sort_by_key(|(_, w)| {
                            let rect = w.rect().unwrap_or_default();
                            rect.right * rect.bottom
                        });
                        window_idx_pairs.reverse();

                        for (i, window) in window_idx_pairs {
                            if i == focused_idx {
                                to_focus = Some(*window);
                            } else {
                                window.restore();
                                window.raise()?;
                            }
                        }

                        if let Some(focused_window) = &mut to_focus {
                            // The focused window should be the last one raised to make sure it is
                            // on top
                            focused_window.restore();
                            focused_window.raise()?;
                        }
                    }
                    WorkspaceLayer::Floating => {
                        workspace.layer = WorkspaceLayer::Tiling;
                        tracing::info!("WorkspaceLayer is now Tiling");

                        if let Some(monocle) = &workspace.monocle_container {
                            if let Some(window) = monocle.focused_window() {
                                to_focus = Some(*window);
                                window.raise()?;
                            }

                            for window in workspace.floating_windows_mut() {
                                window.hide();
                            }
                        } else {
                            let focused_container_idx = workspace.focused_container_idx();
                            for (i, container) in workspace.containers_mut().iter_mut().enumerate()
                            {
                                if let Some(window) = container.focused_window() {
                                    if i == focused_container_idx {
                                        to_focus = Some(*window);
                                    }

                                    window.raise()?;
                                }
                            }

                            let mut window_idx_pairs = workspace
                                .floating_windows_mut()
                                .make_contiguous()
                                .iter_mut()
                                .collect::<Vec<_>>();

                            // Sort by window area
                            window_idx_pairs.sort_by_key(|w| {
                                let rect = w.rect().unwrap_or_default();
                                rect.right * rect.bottom
                            });

                            for window in window_idx_pairs {
                                window.hide();
                            }
                        }
                    }
                };

                if let Some(window) = to_focus {
                    window.focus(mouse_follows_focus)?;
                }
            }
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
            SocketMessage::ToggleWorkspaceWindowContainerBehaviour => {
                let current_global_behaviour = self.window_management_behaviour.current_behaviour;
                if let Some(behaviour) =
                    &mut self.focused_workspace_mut()?.window_container_behaviour
                {
                    match behaviour {
                        WindowContainerBehaviour::Create => {
                            *behaviour = WindowContainerBehaviour::Append
                        }
                        WindowContainerBehaviour::Append => {
                            *behaviour = WindowContainerBehaviour::Create
                        }
                    }
                } else {
                    self.focused_workspace_mut()?.window_container_behaviour =
                        Some(match current_global_behaviour {
                            WindowContainerBehaviour::Create => WindowContainerBehaviour::Append,
                            WindowContainerBehaviour::Append => WindowContainerBehaviour::Create,
                        });
                };
            }
            SocketMessage::ToggleWorkspaceFloatOverride => {
                let current_global_override = self.window_management_behaviour.float_override;
                if let Some(float_override) = &mut self.focused_workspace_mut()?.float_override {
                    *float_override = !*float_override;
                } else {
                    self.focused_workspace_mut()?.float_override = Some(!current_global_override);
                };
            }
            SocketMessage::ToggleLock => self.toggle_lock()?,
            SocketMessage::ToggleWindowContainerBehaviour => {
                match self.window_management_behaviour.current_behaviour {
                    WindowContainerBehaviour::Create => {
                        self.window_management_behaviour.current_behaviour =
                            WindowContainerBehaviour::Append;
                    }
                    WindowContainerBehaviour::Append => {
                        self.window_management_behaviour.current_behaviour =
                            WindowContainerBehaviour::Create;
                    }
                }
            }
            SocketMessage::ToggleFloatOverride => {
                self.window_management_behaviour.float_override =
                    !self.window_management_behaviour.float_override;
            }
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
            SocketMessage::VisibleWindows => {
                let mut monitor_visible_windows = HashMap::new();

                for monitor in self.monitors() {
                    if let Some(ws) = monitor.focused_workspace() {
                        monitor_visible_windows
                            .insert(monitor.id, ws.visible_window_details().clone());
                    }
                }

                let visible_windows_state = serde_json::to_string_pretty(&monitor_visible_windows)
                    .unwrap_or_else(|error| error.to_string());

                reply.write_all(visible_windows_state.as_bytes())?;
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
            SocketMessage::InitialWorkspaceRule(identifier, ref id, monitor_idx, workspace_idx) => {
                let mut workspace_rules = WORKSPACE_MATCHING_RULES.lock();
                let workspace_matching_rule = WorkspaceMatchingRule {
                    monitor_index: monitor_idx,
                    workspace_index: workspace_idx,
                    matching_rule: MatchingRule::Simple(IdWithIdentifier {
                        kind: identifier,
                        id: id.to_string(),
                        matching_strategy: Some(MatchingStrategy::Legacy),
                    }),
                    initial_only: true,
                };

                if !workspace_rules.contains(&workspace_matching_rule) {
                    workspace_rules.push(workspace_matching_rule);
                }
            }
            SocketMessage::InitialNamedWorkspaceRule(identifier, ref id, ref workspace) => {
                if let Some((monitor_idx, workspace_idx)) =
                    self.monitor_workspace_index_by_name(workspace)
                {
                    let mut workspace_rules = WORKSPACE_MATCHING_RULES.lock();
                    let workspace_matching_rule = WorkspaceMatchingRule {
                        monitor_index: monitor_idx,
                        workspace_index: workspace_idx,
                        matching_rule: MatchingRule::Simple(IdWithIdentifier {
                            kind: identifier,
                            id: id.to_string(),
                            matching_strategy: Some(MatchingStrategy::Legacy),
                        }),
                        initial_only: true,
                    };

                    if !workspace_rules.contains(&workspace_matching_rule) {
                        workspace_rules.push(workspace_matching_rule);
                    }
                }
            }
            SocketMessage::WorkspaceRule(identifier, ref id, monitor_idx, workspace_idx) => {
                let mut workspace_rules = WORKSPACE_MATCHING_RULES.lock();
                let workspace_matching_rule = WorkspaceMatchingRule {
                    monitor_index: monitor_idx,
                    workspace_index: workspace_idx,
                    matching_rule: MatchingRule::Simple(IdWithIdentifier {
                        kind: identifier,
                        id: id.to_string(),
                        matching_strategy: Some(MatchingStrategy::Legacy),
                    }),
                    initial_only: false,
                };

                if !workspace_rules.contains(&workspace_matching_rule) {
                    workspace_rules.push(workspace_matching_rule);
                }
            }
            SocketMessage::NamedWorkspaceRule(identifier, ref id, ref workspace) => {
                if let Some((monitor_idx, workspace_idx)) =
                    self.monitor_workspace_index_by_name(workspace)
                {
                    let mut workspace_rules = WORKSPACE_MATCHING_RULES.lock();
                    let workspace_matching_rule = WorkspaceMatchingRule {
                        monitor_index: monitor_idx,
                        workspace_index: workspace_idx,
                        matching_rule: MatchingRule::Simple(IdWithIdentifier {
                            kind: identifier,
                            id: id.to_string(),
                            matching_strategy: Some(MatchingStrategy::Legacy),
                        }),
                        initial_only: false,
                    };

                    if !workspace_rules.contains(&workspace_matching_rule) {
                        workspace_rules.push(workspace_matching_rule);
                    }
                }
            }
            SocketMessage::ClearWorkspaceRules(monitor_idx, workspace_idx) => {
                let mut workspace_rules = WORKSPACE_MATCHING_RULES.lock();

                workspace_rules.retain(|r| {
                    r.monitor_index != monitor_idx && r.workspace_index != workspace_idx
                });
            }
            SocketMessage::ClearNamedWorkspaceRules(ref workspace) => {
                if let Some((monitor_idx, workspace_idx)) =
                    self.monitor_workspace_index_by_name(workspace)
                {
                    let mut workspace_rules = WORKSPACE_MATCHING_RULES.lock();
                    workspace_rules.retain(|r| {
                        r.monitor_index != monitor_idx && r.workspace_index != workspace_idx
                    });
                }
            }
            SocketMessage::ClearAllWorkspaceRules => {
                let mut workspace_rules = WORKSPACE_MATCHING_RULES.lock();
                workspace_rules.clear();
            }
            SocketMessage::EnforceWorkspaceRules => {
                {
                    let mut already_moved = self.already_moved_window_handles.lock();
                    already_moved.clear();
                }
                self.enforce_workspace_rules()?;
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
            SocketMessage::NamedWorkspaceLayoutRule(ref workspace, at_container_count, layout) => {
                if let Some((monitor_idx, workspace_idx)) =
                    self.monitor_workspace_index_by_name(workspace)
                {
                    self.add_workspace_layout_default_rule(
                        monitor_idx,
                        workspace_idx,
                        at_container_count,
                        layout,
                    )?;
                }
            }
            SocketMessage::ClearNamedWorkspaceLayoutRules(ref workspace) => {
                if let Some((monitor_idx, workspace_idx)) =
                    self.monitor_workspace_index_by_name(workspace)
                {
                    self.clear_workspace_layout_rules(monitor_idx, workspace_idx)?;
                }
            }
            SocketMessage::MouseFollowsFocus(enable) => {
                self.mouse_follows_focus = enable;
            }
            SocketMessage::ToggleMouseFollowsFocus => {
                self.mouse_follows_focus = !self.mouse_follows_focus;
            }
            SocketMessage::ToggleMonocleFocusBehaviour => {
                self.monocle_focus_behaviour = match self.monocle_focus_behaviour {
                    MonocleFocusBehaviour::Cycle => MonocleFocusBehaviour::NoOp,
                    MonocleFocusBehaviour::NoOp => MonocleFocusBehaviour::Cycle,
                };
            }
            SocketMessage::MonocleFocusBehaviour(behaviour) => {
                self.monocle_focus_behaviour = behaviour;
            }
            SocketMessage::UnmanagedWindowOperationBehaviour(behaviour) => {
                self.unmanaged_window_operation_behaviour = behaviour;
            }
            SocketMessage::ApplicationSpecificConfigurationSchema => {
                #[cfg(feature = "schemars")]
                {
                    let asc = schemars::schema_for!(
                        Vec<crate::core::asc::ApplicationSpecificConfiguration>
                    );
                    let schema = serde_json::to_string_pretty(&asc)?;

                    reply.write_all(schema.as_bytes())?;
                }
            }
            SocketMessage::NotificationSchema => {
                #[cfg(feature = "schemars")]
                {
                    let notification = schemars::schema_for!(Notification);
                    let schema = serde_json::to_string_pretty(&notification)?;

                    reply.write_all(schema.as_bytes())?;
                }
            }
            SocketMessage::SocketSchema => {
                #[cfg(feature = "schemars")]
                {
                    let socket_message = schemars::schema_for!(SocketMessage);
                    let schema = serde_json::to_string_pretty(&socket_message)?;

                    reply.write_all(schema.as_bytes())?;
                }
            }
            SocketMessage::StaticConfigSchema => {
                #[cfg(feature = "schemars")]
                {
                    let static_config = schemars::schema_for!(StaticConfig);
                    let schema = serde_json::to_string_pretty(&static_config)?;

                    reply.write_all(schema.as_bytes())?;
                }
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
                self.stop(false)?;
            }
            SocketMessage::StopIgnoreRestore => {
                self.stop(true)?;
            }
            SocketMessage::GenerateStaticConfig => {
                let config = serde_json::to_string_pretty(&StaticConfig::from(&*self))?;

                reply.write_all(config.as_bytes())?;
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
            SocketMessage::Border(enable) => {
                border_manager::BORDER_ENABLED.store(enable, Ordering::SeqCst);
                if !enable {
                    border_manager::destroy_all_borders()?;
                }
            }
            SocketMessage::BorderColour(kind, r, g, b) => match kind {
                WindowKind::Single => {
                    border_manager::FOCUSED.store(border_manager::pack(r, g, b), Ordering::SeqCst);
                }
                WindowKind::Stack => {
                    border_manager::STACK.store(border_manager::pack(r, g, b), Ordering::SeqCst);
                }
                WindowKind::Monocle => {
                    border_manager::MONOCLE.store(border_manager::pack(r, g, b), Ordering::SeqCst);
                }
                WindowKind::Unfocused => {
                    border_manager::UNFOCUSED
                        .store(border_manager::pack(r, g, b), Ordering::SeqCst);
                }
                WindowKind::UnfocusedLocked => {
                    border_manager::UNFOCUSED_LOCKED
                        .store(border_manager::pack(r, g, b), Ordering::SeqCst);
                }
                WindowKind::Floating => {
                    border_manager::FLOATING.store(border_manager::pack(r, g, b), Ordering::SeqCst);
                }
            },
            SocketMessage::BorderWidth(width) => {
                border_manager::BORDER_WIDTH.store(width, Ordering::SeqCst);
                border_manager::destroy_all_borders()?;
            }
            SocketMessage::BorderOffset(offset) => {
                border_manager::BORDER_OFFSET.store(offset, Ordering::SeqCst);
                border_manager::destroy_all_borders()?;
            }
            SocketMessage::Animation(enable, prefix) => match prefix {
                Some(prefix) => {
                    ANIMATION_ENABLED_PER_ANIMATION
                        .lock()
                        .insert(prefix, enable);
                }
                None => {
                    ANIMATION_ENABLED_GLOBAL.store(enable, Ordering::SeqCst);
                    ANIMATION_ENABLED_PER_ANIMATION.lock().clear();
                }
            },
            SocketMessage::AnimationDuration(duration, prefix) => match prefix {
                Some(prefix) => {
                    ANIMATION_DURATION_PER_ANIMATION
                        .lock()
                        .insert(prefix, duration);
                }
                None => {
                    ANIMATION_DURATION_GLOBAL.store(duration, Ordering::SeqCst);
                    ANIMATION_DURATION_PER_ANIMATION.lock().clear();
                }
            },
            SocketMessage::AnimationFps(fps) => {
                ANIMATION_FPS.store(fps, Ordering::SeqCst);
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

        border_manager::send_notification();
        border_manager::update_colours(self);

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
