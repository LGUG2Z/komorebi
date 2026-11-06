use crate::DATA_DIR;
use crate::FLOATING_APPLICATIONS;
use crate::IGNORE_IDENTIFIERS;
use crate::MANAGE_IDENTIFIERS;
use crate::Notification;
use crate::NotificationEvent;
use crate::SESSION_FLOATING_APPLICATIONS;
use crate::SUBSCRIPTION_SOCKET_OPTIONS;
use crate::SUBSCRIPTION_SOCKETS;
use crate::WORKSPACE_MATCHING_RULES;
use crate::animation;
use crate::application::Application;
use crate::border_manager;
use crate::cf_array_as;
use crate::core::ApplicationIdentifier;
use crate::core::Axis;
use crate::core::CycleDirection;
use crate::core::Layout;
use crate::core::LayoutOptions;
use crate::core::OperationDirection;
use crate::core::Rect;
use crate::core::Sizing;
use crate::core::SocketMessage;
use crate::core::StateQuery;
use crate::core::WindowContainerBehaviour;
use crate::core::WindowKind;
use crate::core::config_generation::IdWithIdentifier;
use crate::core::config_generation::MatchingRule;
use crate::core::config_generation::MatchingStrategy;
use crate::core::config_generation::WorkspaceMatchingRule;
use crate::monitor::MonitorExt;
use crate::scrolling::WorkspaceScrolling;
use crate::workspace::WorkspaceExt;
use komorebi_config::matching::push_simple_rule;

use crate::core_graphics::CoreGraphicsApi;
use crate::current_space_id;
use crate::macos_api::MacosApi;
use crate::monitor::MonitorInformation;
use crate::notify_subscribers;
use crate::state::GlobalState;
use crate::state::State;
use crate::static_config::StaticConfig;
use crate::static_config::StaticConfigExt;
use crate::theme_manager;
use crate::window::AdhocWindow;
use crate::window::RuleDebug;
use crate::window::Window;
use crate::window::WindowInfo;
use crate::window_manager::WindowManager;
use crate::window_manager_event_listener;
use crate::workspace::WorkspaceLayer;
use crate::workspace::WorkspaceWindowLocation;
use color_eyre::eyre;
use color_eyre::eyre::Context;
use color_eyre::eyre::OptionExt;
use komorebi_themes::colour::Rgb;
use komorebi_transport::CommandTarget;
use komorebi_transport::NoPolicy;
use objc2_core_foundation::CFDictionary;
use parking_lot::Mutex;
use std::collections::HashMap;
use std::fs::File;
use std::fs::OpenOptions;
use std::num::NonZeroUsize;
use std::os::unix::net::UnixListener;
use std::os::unix::net::UnixStream;
use std::sync::Arc;
use std::sync::atomic::Ordering;
use std::time::Duration;
use version::build;

#[tracing::instrument]
pub fn listen_for_commands(wm: Arc<Mutex<WindowManager>>) {
    komorebi_transport::listen_for_commands::<WindowManager, _>(wm, NoPolicy);
}

/// What the generic command listener needs of the window manager
impl CommandTarget for WindowManager {
    type Listener = UnixListener;
    type Shared = Arc<Mutex<WindowManager>>;

    fn command_listener(shared: &Self::Shared) -> std::io::Result<UnixListener> {
        shared.lock().command_listener.try_clone()
    }

    fn try_lock_for<R>(
        shared: &Self::Shared,
        timeout: Duration,
        f: impl FnOnce(&mut Self) -> R,
    ) -> Option<R> {
        shared.try_lock_for(timeout).map(|mut wm| f(&mut wm))
    }

    fn is_paused(&self) -> bool {
        self.is_paused
    }

    fn process_command(
        &mut self,
        message: SocketMessage,
        reply: &mut UnixStream,
    ) -> eyre::Result<()> {
        WindowManager::process_command(self, message, reply)
    }
}

impl WindowManager {
    /// Switches the focused workspace between creating a container for a new window and adding it to the focused one
    pub fn toggle_workspace_window_container_behaviour(&mut self) -> eyre::Result<()> {
        let current_global_behaviour = self.window_management_behaviour.current_behaviour;
        if let Some(behaviour) = &mut self.focused_workspace_mut()?.window_container_behaviour {
            match behaviour {
                WindowContainerBehaviour::Create => *behaviour = WindowContainerBehaviour::Append,
                WindowContainerBehaviour::Append => *behaviour = WindowContainerBehaviour::Create,
            }
        } else {
            self.focused_workspace_mut()?.window_container_behaviour =
                Some(match current_global_behaviour {
                    WindowContainerBehaviour::Create => WindowContainerBehaviour::Append,
                    WindowContainerBehaviour::Append => WindowContainerBehaviour::Create,
                });
        };
        Ok(())
    }

    /// Sets the column and row ratios of the focused workspace's layout
    pub fn set_layout_ratios(
        &mut self,
        columns: &Option<Vec<f32>>,
        rows: &Option<Vec<f32>>,
    ) -> eyre::Result<()> {
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
        Ok(())
    }

    /// Writes the answer to a state query to `reply`
    pub fn answer_query(
        &mut self,
        query: StateQuery,
        reply: &mut impl std::io::Write,
    ) -> eyre::Result<()> {
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
                let focused_monitor = self.focused_monitor().ok_or_eyre("there is no monitor")?;

                focused_monitor
                    .focused_workspace_name()
                    .unwrap_or_else(|| focused_monitor.focused_workspace_idx().to_string())
            }
            StateQuery::Version => build::VERSION.to_string(),
            StateQuery::FocusedWorkspaceLayout => {
                let focused_monitor = self.focused_monitor().ok_or_eyre("there is no monitor")?;

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
        Ok(())
    }

    /// Adds a rule that puts the windows matching `id` on a named workspace, if the workspace exists
    pub fn add_named_workspace_rule(
        &mut self,
        identifier: ApplicationIdentifier,
        id: &str,
        workspace: &str,
    ) -> eyre::Result<()> {
        if let Some((monitor_idx, workspace_idx)) = self.monitor_workspace_index_by_name(workspace)
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
        Ok(())
    }

    /// Adds a rule that puts a window matching `id` on a named workspace when it first appears, if the workspace exists
    pub fn add_initial_named_workspace_rule(
        &mut self,
        identifier: ApplicationIdentifier,
        id: &str,
        workspace: &str,
    ) -> eyre::Result<()> {
        if let Some((monitor_idx, workspace_idx)) = self.monitor_workspace_index_by_name(workspace)
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
        Ok(())
    }

    /// Sets the border colour of one kind of window
    pub fn set_border_colour(&mut self, kind: WindowKind, r: u32, g: u32, b: u32) {
        match kind {
            WindowKind::Single => {
                border_manager::FOCUSED.store(Rgb::new(r, g, b).into(), Ordering::SeqCst);
            }
            WindowKind::Stack => {
                border_manager::STACK.store(Rgb::new(r, g, b).into(), Ordering::SeqCst);
            }
            WindowKind::Monocle => {
                border_manager::MONOCLE.store(Rgb::new(r, g, b).into(), Ordering::SeqCst);
            }
            WindowKind::Unfocused => {
                border_manager::UNFOCUSED.store(Rgb::new(r, g, b).into(), Ordering::SeqCst);
            }
            WindowKind::UnfocusedLocked => {
                border_manager::UNFOCUSED_LOCKED.store(Rgb::new(r, g, b).into(), Ordering::SeqCst);
            }
            WindowKind::Floating => {
                border_manager::FLOATING.store(Rgb::new(r, g, b).into(), Ordering::SeqCst);
            }
        }
    }

    /// Focuses the next or previous workspace of the monitor that is focused, or of the monitor the pointer is on when its workspace is empty
    pub fn cycle_focus_workspace(&mut self, direction: CycleDirection) -> eyre::Result<()> {
        komorebi_core::cycle_focus_workspace(self, direction)
    }

    /// Focuses the next or previous empty workspace of the focused monitor, if it has one
    pub fn cycle_focus_empty_workspace(&mut self, direction: CycleDirection) -> eyre::Result<()> {
        komorebi_core::cycle_focus_empty_workspace(self, direction)
    }

    /// Removes the focused workspace when it is empty, unnamed and not the monitor's only one, and focuses the one before it
    pub fn close_workspace(&mut self) -> eyre::Result<()> {
        komorebi_core::close_workspace(self)
    }

    /// Moves the focused container to the workspace that was focused before this one, and follows it when `follow` is set
    pub fn move_container_to_last_workspace(&mut self, follow: bool) -> eyre::Result<()> {
        komorebi_core::move_container_to_last_workspace(self, follow)
    }

    /// Focuses the workspace that was focused before this one
    pub fn focus_last_workspace(&mut self) -> eyre::Result<()> {
        komorebi_core::focus_last_workspace(self)
    }

    /// Looks for a window of the executable `exe` on every monitor and workspace, and focuses it
    pub fn eager_focus(&mut self, exe: &str) -> eyre::Result<()> {
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
                    if let Some(window) = &mut self.focused_workspace_mut()?.maximized_window {
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
        Ok(())
    }

    #[tracing::instrument(skip(self, reply, message))]
    pub fn process_command(
        &mut self,
        message: SocketMessage,
        mut reply: impl std::io::Write,
    ) -> eyre::Result<()> {
        if let Some(space_id) = &self.space_id
            && let Some(current_space_id) = current_space_id()
            && *space_id != current_space_id
        {
            tracing::trace!("ignoring events and commands while not on space {space_id}");
            return Ok(());
        }

        if matches!(message, SocketMessage::Theme(_)) {
            tracing::trace!("processing command: {message}");
        } else {
            tracing::info!("processing command: {message}");
        }

        #[allow(clippy::useless_asref)]
        // We don't have From implemented for &mut WindowManager
        let initial_state = State::from(self.as_ref());

        komorebi_core::CommandHost::gate_message(self, &message)?;

        match message {
            SocketMessage::Promote => self.promote_container_to_front()?,
            SocketMessage::PromoteSwap => self.promote_container_swap()?,
            SocketMessage::PromoteFocus => self.promote_focus_to_front()?,
            SocketMessage::PromoteWindow(direction) => {
                komorebi_core::promote_window(self, direction)?
            }
            SocketMessage::EagerFocus(ref exe) => self.eager_focus(exe)?,
            SocketMessage::FocusWindow(direction) => komorebi_core::focus_window(self, direction)?,
            SocketMessage::PreselectDirection(direction) => {
                komorebi_core::preselect_direction(self, direction)?
            }
            SocketMessage::CancelPreselect => komorebi_core::cancel_preselect(self)?,
            SocketMessage::MoveWindow(direction) => komorebi_core::move_window(self, direction)?,
            SocketMessage::CycleFocusWindow(direction) => {
                komorebi_core::cycle_focus_window(self, direction)?
            }
            SocketMessage::CycleMoveWindow(direction) => {
                self.move_container_in_cycle_direction(direction)?;
            }

            SocketMessage::StackWindow(direction) => {
                let _suspended_animations = animation::suspend();
                self.add_window_to_container(direction)?;
            }
            SocketMessage::UnstackWindow => {
                let _suspended_animations = animation::suspend();
                self.remove_window_from_container()?;
                border_manager::destroy_all_borders()?;
            }
            SocketMessage::StackAll => {
                let _suspended_animations = animation::suspend();
                self.stack_all()?;
            }
            SocketMessage::UnstackAll => {
                let _suspended_animations = animation::suspend();
                self.unstack_all(true)?;
                border_manager::destroy_all_borders()?;
            }
            SocketMessage::CycleStack(direction) => {
                let _suspended_animations = animation::suspend();
                self.cycle_container_window_in_direction(direction)?;
            }
            SocketMessage::CycleStackIndex(direction) => {
                let _suspended_animations = animation::suspend();
                self.cycle_container_window_index_in_direction(direction)?;
            }
            SocketMessage::FocusStackWindow(idx) => {
                // In case you are using this command on a bar on a monitor
                // different from the currently focused one, you'd want that
                // monitor to be focused so that the FocusStackWindow happens
                // on the monitor with the bar you just pressed.
                if let Some(monitor_idx) = self.monitor_idx_from_current_pos() {
                    self.focus_monitor(monitor_idx)?;
                }
                let _suspended_animations = animation::suspend();
                self.focus_container_window(idx)?;
            }
            SocketMessage::Minimize => self.minimize_foreground_window()?,
            SocketMessage::Close => self.close_foreground_window()?,
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
            ) => komorebi_core::set_container_locked(
                self,
                monitor_idx,
                workspace_idx,
                container_idx,
                true,
            )?,
            SocketMessage::UnlockMonitorWorkspaceContainer(
                monitor_idx,
                workspace_idx,
                container_idx,
            ) => komorebi_core::set_container_locked(
                self,
                monitor_idx,
                workspace_idx,
                container_idx,
                false,
            )?,
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
            SocketMessage::CycleFocusMonitor(direction) => {
                komorebi_core::cycle_focus_monitor(self, direction)?
            }
            SocketMessage::CycleFocusWorkspace(direction) => {
                self.cycle_focus_workspace(direction)?
            }
            SocketMessage::CycleFocusEmptyWorkspace(direction) => {
                self.cycle_focus_empty_workspace(direction)?
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
            SocketMessage::FocusLastWorkspace => self.focus_last_workspace()?,
            SocketMessage::FocusWorkspaceNumber(workspace_idx) => {
                komorebi_core::focus_workspace_number(self, workspace_idx)?
            }
            SocketMessage::FocusWorkspaceNumbers(workspace_idx) => {
                komorebi_core::focus_workspace_numbers(self, workspace_idx)?
            }
            SocketMessage::FocusMonitorWorkspaceNumber(monitor_idx, workspace_idx) => {
                komorebi_core::focus_monitor_workspace_number(self, monitor_idx, workspace_idx)?
            }
            SocketMessage::FocusNamedWorkspace(ref name) => {
                komorebi_core::focus_named_workspace(self, name)?
            }
            SocketMessage::CloseWorkspace => self.close_workspace()?,

            SocketMessage::MoveContainerToWorkspaceNumber(workspace_idx) => {
                self.move_container_to_workspace(workspace_idx, true, None)?;
            }
            SocketMessage::SendContainerToWorkspaceNumber(workspace_idx) => {
                self.move_container_to_workspace(workspace_idx, false, None)?;
            }
            SocketMessage::ToggleMonocle => self.toggle_monocle()?,
            SocketMessage::ToggleFloat => self.toggle_float(false)?,
            SocketMessage::ToggleWorkspaceLayer => self.toggle_workspace_layer()?,
            SocketMessage::ResizeWindowEdge(direction, sizing) => {
                // resizing is done by the key, over and over, and every press is laid out where it
                // goes and not by an animation that the next press would have to catch up with
                let _suspended_animations = animation::suspend();
                self.resize_window(direction, sizing, self.resize_delta, true)?;
            }
            SocketMessage::ResizeWindowAxis(axis, sizing) => {
                self.resize_window_axis(axis, sizing)?
            }
            SocketMessage::Retile => self.retile_all(false)?,
            SocketMessage::RetileWithResizeDimensions => self.retile_all(true)?,
            SocketMessage::ToggleWorkspaceWindowContainerBehaviour => {
                self.toggle_workspace_window_container_behaviour()?
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
                komorebi_core::toggle_window_container_behaviour(self)
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
            SocketMessage::ToggleCrossMonitorMoveBehaviour => {
                komorebi_core::toggle_cross_monitor_move_behaviour(self)
            }
            SocketMessage::ToggleTiling => {
                self.toggle_tiling()?;
            }
            SocketMessage::MoveContainerToLastWorkspace => {
                self.move_container_to_last_workspace(true)?
            }
            SocketMessage::SendContainerToLastWorkspace => {
                self.move_container_to_last_workspace(false)?;
            }
            SocketMessage::CycleMoveContainerToWorkspace(direction) => {
                komorebi_core::cycle_move_container_to_workspace(self, direction, true)?
            }
            SocketMessage::MoveContainerToMonitorNumber(monitor_idx) => {
                let direction = self.direction_from_monitor_idx(monitor_idx);
                self.move_container_to_monitor(monitor_idx, None, true, direction)?;
            }
            SocketMessage::SwapWorkspacesToMonitorNumber(monitor_idx) => {
                self.swap_focused_monitor(monitor_idx)?;
            }
            SocketMessage::CycleMoveContainerToMonitor(direction) => {
                komorebi_core::cycle_move_container_to_monitor(self, direction, true)?
            }
            SocketMessage::CycleSendContainerToWorkspace(direction) => {
                komorebi_core::cycle_move_container_to_workspace(self, direction, false)?
            }
            SocketMessage::SendContainerToMonitorNumber(monitor_idx) => {
                let direction = self.direction_from_monitor_idx(monitor_idx);
                self.move_container_to_monitor(monitor_idx, None, false, direction)?;
            }
            SocketMessage::CycleSendContainerToMonitor(direction) => {
                komorebi_core::cycle_move_container_to_monitor(self, direction, false)?
            }
            SocketMessage::SendContainerToMonitorWorkspaceNumber(monitor_idx, workspace_idx) => {
                komorebi_core::move_container_to_monitor_workspace_number(
                    self,
                    monitor_idx,
                    workspace_idx,
                    false,
                )?
            }
            SocketMessage::MoveContainerToMonitorWorkspaceNumber(monitor_idx, workspace_idx) => {
                komorebi_core::move_container_to_monitor_workspace_number(
                    self,
                    monitor_idx,
                    workspace_idx,
                    true,
                )?
            }
            SocketMessage::SendContainerToNamedWorkspace(ref workspace) => {
                komorebi_core::move_container_to_named_workspace(self, workspace, false)?
            }
            SocketMessage::MoveContainerToNamedWorkspace(ref workspace) => {
                komorebi_core::move_container_to_named_workspace(self, workspace, true)?
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
            SocketMessage::Query(query) => self.answer_query(query, &mut reply)?,
            SocketMessage::SessionFloatRule => self.add_session_float_rule()?,
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
                self.add_initial_named_workspace_rule(identifier, id, workspace)?
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
                self.add_named_workspace_rule(identifier, id, workspace)?
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
                self.add_ignore_rule(identifier, id)?
            }
            SocketMessage::ManageRule(identifier, ref id) => {
                let mut manage_identifiers = MANAGE_IDENTIFIERS.lock();

                push_simple_rule(&mut manage_identifiers, identifier, id);
            }
            SocketMessage::NewWorkspace => {
                self.new_workspace()?;
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
                self.focused_workspace_mut()?
                    .set_scrolling_columns(count.into());
                self.update_focused_workspace(false, false)?;
            }
            SocketMessage::ScrollingLayoutFullWidth => {
                self.focused_workspace_mut()?.toggle_scrolling_full_width();
                self.update_focused_workspace(false, false)?;
            }
            SocketMessage::CenterFocus => {
                self.focused_workspace_mut()?.center_scrolling_column();
                self.update_focused_workspace(false, false)?;
            }
            SocketMessage::Expand => {
                self.focused_workspace_mut()?.expand_scrolling_column();
                self.update_focused_workspace(false, false)?;
            }
            SocketMessage::Pan(direction) => self.pan(direction)?,
            SocketMessage::PanGesture(gesture) => self.pan_gesture(gesture)?,
            SocketMessage::Consume(direction) => {
                self.focused_workspace_mut()?.consume_scrolling(direction);
                self.update_focused_workspace(false, false)?;
            }
            SocketMessage::ToggleColumnStack => {
                self.focused_workspace_mut()?
                    .toggle_scrolling_column_stack()?;
                self.update_focused_workspace(false, false)?;
            }
            SocketMessage::Expel => {
                self.focused_workspace_mut()?.expel_scrolling();
                self.update_focused_workspace(false, false)?;
            }
            SocketMessage::LayoutRatios(ref columns, ref rows) => {
                self.set_layout_ratios(columns, rows)?
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
            SocketMessage::EnsureWorkspaces(monitor_idx, workspace_count) => {
                self.ensure_workspaces_for_monitor(monitor_idx, workspace_count)?;
            }
            SocketMessage::EnsureNamedWorkspaces(monitor_idx, ref names) => {
                self.ensure_named_workspaces_for_monitor(monitor_idx, names)?;
            }
            SocketMessage::MouseFollowsFocus(enable) => {
                self.mouse_follows_focus = enable;
            }
            SocketMessage::ToggleMouseFollowsFocus => {
                self.mouse_follows_focus = !self.mouse_follows_focus;
            }
            SocketMessage::CrossMonitorMoveBehaviour(behaviour) => {
                self.cross_monitor_move_behaviour = behaviour;
            }
            SocketMessage::ToggleMonocleFocusBehaviour => {
                komorebi_core::toggle_monocle_focus_behaviour(self)
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
                    let schema = komorebi_config::schema::view::<
                        Vec<crate::core::asc::ApplicationSpecificConfiguration>,
                    >(komorebi_config::schema::Platform::CURRENT);
                    let schema = schema.trim_end();

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
                    let schema = komorebi_config::schema::view::<
                        komorebi_config::static_config::StaticConfig,
                    >(komorebi_config::schema::Platform::CURRENT);
                    let schema = schema.trim_end();

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
            SocketMessage::DebugWindow(window_id) => self.debug_window(window_id, &mut reply)?,
            SocketMessage::DisplayIndexPreference(index_preference, ref display) => {
                crate::settings::settings().update(|settings| {
                    settings
                        .shared
                        .display_index_preferences
                        .insert(index_preference, display.clone());
                });
            }
            SocketMessage::ReplaceConfiguration(ref config) => {
                self.replace_configuration(config)?
            }
            SocketMessage::Border(enable) => {
                border_manager::BORDER_ENABLED.store(enable, Ordering::SeqCst);
                if !enable {
                    border_manager::destroy_all_borders()?;
                }
            }
            SocketMessage::BorderColour(kind, r, g, b) => self.set_border_colour(kind, r, g, b),
            SocketMessage::BorderWidth(width) => {
                border_manager::BORDER_WIDTH.store(width, Ordering::SeqCst);
                border_manager::destroy_all_borders()?;
            }
            SocketMessage::BorderOffset(offset) => {
                border_manager::BORDER_OFFSET.store(offset, Ordering::SeqCst);
                border_manager::destroy_all_borders()?;
            }
            SocketMessage::Theme(ref theme) => {
                theme_manager::send_notification(*theme.clone());
            }
            SocketMessage::Animation(enable, prefix) => {
                crate::settings::settings().update(|settings| {
                    let animation = &mut settings.shared.animation;
                    match prefix {
                        Some(prefix) => {
                            animation.enabled_per_animation.insert(prefix, enable);
                        }
                        None => {
                            animation.enabled = enable;
                            animation.enabled_per_animation.clear();
                        }
                    }
                });
            }
            SocketMessage::AnimationDuration(duration, prefix) => {
                crate::settings::settings().update(|settings| {
                    let animation = &mut settings.shared.animation;
                    match prefix {
                        Some(prefix) => {
                            animation.duration_per_animation.insert(prefix, duration);
                        }
                        None => {
                            animation.duration = duration;
                            animation.duration_per_animation.clear();
                        }
                    }
                });
            }
            SocketMessage::AnimationFps(fps) => {
                crate::settings::settings().update(|settings| settings.shared.animation.fps = fps);
            }
            SocketMessage::AnimationStyle(style, prefix) => {
                crate::settings::settings().update(|settings| {
                    let animation = &mut settings.shared.animation;
                    match prefix {
                        Some(prefix) => {
                            animation.style_per_animation.insert(prefix, style);
                        }
                        None => {
                            animation.style = style;
                            animation.style_per_animation.clear();
                        }
                    }
                });
            }
            // Messages of other platforms: answered, and nothing else happens
            SocketMessage::AddSubscriberPipe(..)
            | SocketMessage::AltFocusHack(..)
            | SocketMessage::ApplyState(..)
            | SocketMessage::BorderImplementation(..)
            | SocketMessage::BorderStyle(..)
            | SocketMessage::ChangeLayoutCustom(..)
            | SocketMessage::CompleteConfiguration
            | SocketMessage::FocusFollowsMouse(..)
            | SocketMessage::ForceFocus
            | SocketMessage::HotkeysState
            | SocketMessage::IdentifyBorderOverflowApplication(..)
            | SocketMessage::IdentifyLayeredApplication(..)
            | SocketMessage::IdentifyObjectNameChangeApplication(..)
            | SocketMessage::IdentifyTrayApplication(..)
            | SocketMessage::InvisibleBorders(..)
            | SocketMessage::MonitorIndexPreference(..)
            | SocketMessage::NamedWorkspaceLayoutCustom(..)
            | SocketMessage::NamedWorkspaceLayoutCustomRule(..)
            | SocketMessage::ReloadConfiguration
            | SocketMessage::ReloadHotkeys
            | SocketMessage::RemoveSubscriberPipe(..)
            | SocketMessage::RemoveTitleBar(..)
            | SocketMessage::StackbarBackgroundColour(..)
            | SocketMessage::StackbarFocusedTextColour(..)
            | SocketMessage::StackbarFontFamily(..)
            | SocketMessage::StackbarFontSize(..)
            | SocketMessage::StackbarHeight(..)
            | SocketMessage::StackbarLabel(..)
            | SocketMessage::StackbarMode(..)
            | SocketMessage::StackbarTabWidth(..)
            | SocketMessage::StackbarUnfocusedTextColour(..)
            | SocketMessage::ToggleFocusFollowsMouse(..)
            | SocketMessage::ToggleMaximize
            | SocketMessage::ToggleTitleBars
            | SocketMessage::ToggleTransparency
            | SocketMessage::Transparency(..)
            | SocketMessage::TransparencyAlpha(..)
            | SocketMessage::WatchConfiguration(..)
            | SocketMessage::WindowHidingBehaviour(..)
            | SocketMessage::WorkspaceLayoutCustom(..)
            | SocketMessage::WorkspaceLayoutCustomRule(..) => {
                reply.write_all(message.not_supported_reply().as_bytes())?;
                return Ok(());
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

        border_manager::send_notification(None, None, false);

        if matches!(message, SocketMessage::Theme(_)) {
            tracing::trace!("processed command: {message}");
        } else {
            tracing::info!("processed command: {message}");
        }

        Ok(())
    }
}

pub fn read_commands_uds(wm: &Arc<Mutex<WindowManager>>, stream: UnixStream) -> eyre::Result<()> {
    komorebi_transport::read_commands::<WindowManager, _>(wm, stream, &NoPolicy)
}

impl WindowManager {
    /// Switches the focused workspace between its tiling and floating layers, and focuses a window of the layer it switched to
    pub fn toggle_workspace_layer(&mut self) -> eyre::Result<()> {
        let mouse_follows_focus = self.mouse_follows_focus;
        let hiding_position = self
            .focused_monitor()
            .ok_or_eyre("there is no monitor")?
            .extra
            .window_hiding_position;
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
                    let rect = Rect::from(w.window_rect().unwrap_or_default());
                    rect.right * rect.bottom
                });
                window_idx_pairs.reverse();

                for (i, window) in window_idx_pairs {
                    if i == focused_idx {
                        to_focus = Some(window.clone());
                    } else {
                        window.restore()?;
                        window.raise()?;
                    }
                }

                if let Some(focused_window) = &mut to_focus {
                    // The focused window should be the last one raised to make sure it is
                    // on top
                    focused_window.restore()?;
                    focused_window.raise()?;
                }

                for container in workspace.containers() {
                    if let Some(_window) = container.focused_window() {
                        // TODO: figure out z order
                        // window.lower()?;
                    }
                }

                if let Some(monocle) = &workspace.monocle_container
                    && let Some(_window) = monocle.focused_window()
                {
                    // TODO: figure out z order
                    // window.lower()?;
                }
            }
            WorkspaceLayer::Floating => {
                workspace.layer = WorkspaceLayer::Tiling;
                tracing::info!("WorkspaceLayer is now Tiling");

                if let Some(monocle) = &workspace.monocle_container {
                    if let Some(window) = monocle.focused_window() {
                        to_focus = Some(window.clone());
                        window.raise()?;
                    }

                    for window in workspace.floating_windows_mut() {
                        window.hide(hiding_position)?;
                    }
                } else {
                    let focused_container_idx = workspace.focused_container_idx();
                    for (i, container) in workspace.containers_mut().iter_mut().enumerate() {
                        if let Some(window) = container.focused_window() {
                            if i == focused_container_idx {
                                to_focus = Some(window.clone());
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
                        let rect = Rect::from(w.window_rect().unwrap_or_default());
                        rect.right * rect.bottom
                    });

                    for window in window_idx_pairs {
                        // TODO: figure out z order
                        window.hide(hiding_position)?;
                        // window.lower()?;
                    }
                }
            }
        };

        if let Some(window) = to_focus {
            window.focus(mouse_follows_focus)?;
        }
        Ok(())
    }

    /// Resizes the focused window along an axis, by the edge that the axis says
    pub fn resize_window_axis(&mut self, axis: Axis, sizing: Sizing) -> eyre::Result<()> {
        let _suspended_animations = animation::suspend();
        match axis {
            Axis::Horizontal => {
                self.resize_window(OperationDirection::Left, sizing, self.resize_delta, false)?;
                self.resize_window(OperationDirection::Right, sizing, self.resize_delta, false)?;
            }
            Axis::Vertical => {
                self.resize_window(OperationDirection::Up, sizing, self.resize_delta, false)?;
                self.resize_window(OperationDirection::Down, sizing, self.resize_delta, false)?;
            }
            Axis::HorizontalAndVertical => {
                self.resize_window(OperationDirection::Left, sizing, self.resize_delta, false)?;
                self.resize_window(OperationDirection::Right, sizing, self.resize_delta, false)?;
                self.resize_window(OperationDirection::Up, sizing, self.resize_delta, false)?;
                self.resize_window(OperationDirection::Down, sizing, self.resize_delta, false)?;
            }
        }

        self.update_focused_workspace(false, false)?;
        Ok(())
    }

    /// Adds a float rule for the focused window (its executable, title and role), until the daemon stops
    pub fn add_session_float_rule(&mut self) -> eyre::Result<()> {
        let foreground_window =
            MacosApi::foreground_window().ok_or_eyre("there is no foreground window")?;
        let (exe, title, role) = (
            AdhocWindow::exe(&foreground_window),
            AdhocWindow::title(&foreground_window),
            AdhocWindow::class(&foreground_window),
        );

        if let (Some(exe), Some(title), Some(role)) = (exe, title, role) {
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
                    id: role,
                    matching_strategy: Option::from(MatchingStrategy::Equals),
                },
            ]);

            let mut floating_applications = FLOATING_APPLICATIONS.lock();
            floating_applications.push(rule.clone());
            let mut session_floating_applications = SESSION_FLOATING_APPLICATIONS.lock();
            session_floating_applications.push(rule.clone());

            self.toggle_float(true)?;
        }
        Ok(())
    }

    /// Adds a rule that ignores the windows matching `id`, and takes the ones it matches out of the workspaces
    pub fn add_ignore_rule(
        &mut self,
        identifier: ApplicationIdentifier,
        id: &str,
    ) -> eyre::Result<()> {
        let mut ignore_identifiers = IGNORE_IDENTIFIERS.lock();

        push_simple_rule(&mut ignore_identifiers, identifier, id);

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
                            if window.role().unwrap_or_default() == *id
                                || window.subrole().unwrap_or_default() == *id
                            {
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
        Ok(())
    }

    /// Replaces the running configuration with the one in the file
    pub fn replace_configuration(&mut self, config: &std::path::Path) -> eyre::Result<()> {
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
                &self.run_loop.0,
            )?;

            // Initialize the new wm
            wm.init()?;

            wm.restore_all_windows(true)?;

            // This is equivalent to StaticConfig::postload for this use case
            StaticConfig::reload(config, &mut wm)?;

            // Set self to the new wm instance
            *self = wm;
        }
        Ok(())
    }
}

impl WindowManager {
    /// Minimizes the window that has the focus
    pub fn minimize_foreground_window(&mut self) -> eyre::Result<()> {
        let foreground_window =
            MacosApi::foreground_window().ok_or_eyre("there is no foreground window")?;
        AdhocWindow::minimize(&foreground_window)?;
        Ok(())
    }

    /// Closes the window that has the focus
    pub fn close_foreground_window(&mut self) -> eyre::Result<()> {
        let foreground_window =
            MacosApi::foreground_window().ok_or_eyre("there is no foreground window")?;
        AdhocWindow::close(&foreground_window)?;
        Ok(())
    }

    /// Answers with why the window of this id is, or is not, managed
    pub fn debug_window(
        &self,
        window_id: i64,
        reply: &mut impl std::io::Write,
    ) -> eyre::Result<()> {
        if let Some(window_list_info) = CoreGraphicsApi::window_list_info() {
            for raw_window_info in cf_array_as::<CFDictionary>(&window_list_info) {
                let raw_info = WindowInfo::new(raw_window_info);
                let application = Application::new(raw_info.owner_pid)?;
                if let Some(elements) = application.window_elements() {
                    for element in elements {
                        if let Ok(wid) = crate::ax_registry::identify(&element)
                            && i64::from(wid) == window_id
                            && let Ok(window) = Window::new(element, application.clone())
                        {
                            let mut rule_debug = RuleDebug::default();
                            let _ = window.should_manage(None, &mut rule_debug);
                            let schema = serde_json::to_string_pretty(&rule_debug)?;

                            reply.write_all(schema.as_bytes())?;
                        }
                    }
                }
            }
        }
        Ok(())
    }
}

/// The window manager as the shared commands of `komorebi_core::command` see it
impl komorebi_core::CommandHost for WindowManager {
    type P = crate::workspace::MacosPlatform;

    fn gate_message(
        &mut self,
        _message: &komorebi_protocol::message::SocketMessage,
    ) -> eyre::Result<()> {
        self.handle_unmanaged_window_behaviour()
    }

    fn monitors(&self) -> &std::collections::VecDeque<crate::monitor::Monitor> {
        WindowManager::monitors(self)
    }

    fn focused_monitor(&self) -> Option<&crate::monitor::Monitor> {
        WindowManager::focused_monitor(self)
    }

    fn focused_monitor_mut(&mut self) -> Option<&mut crate::monitor::Monitor> {
        WindowManager::focused_monitor_mut(self)
    }

    fn focused_monitor_idx(&self) -> usize {
        WindowManager::focused_monitor_idx(self)
    }

    fn monitor_idx_from_current_pos(&mut self) -> Option<usize> {
        WindowManager::monitor_idx_from_current_pos(self)
    }

    fn focus_monitor(&mut self, idx: usize) -> eyre::Result<()> {
        WindowManager::focus_monitor(self, idx)
    }

    fn focus_workspace(&mut self, idx: usize) -> eyre::Result<()> {
        WindowManager::focus_workspace(self, idx)
    }

    fn show_workspace_on(&mut self, monitor_idx: usize, workspace_idx: usize) -> eyre::Result<()> {
        let monitor = self
            .monitors_mut()
            .get_mut(monitor_idx)
            .ok_or_eyre("there is no monitor at this index")?;
        monitor.focus_workspace(workspace_idx)?;
        monitor.load_focused_workspace(false)?;
        Ok(())
    }

    fn move_container_to_workspace(
        &mut self,
        idx: usize,
        follow: bool,
        direction: Option<OperationDirection>,
    ) -> eyre::Result<()> {
        WindowManager::move_container_to_workspace(self, idx, follow, direction)
    }
}

impl komorebi_core::MonitorCommandHost for WindowManager {
    fn mouse_follows_focus(&self) -> bool {
        self.mouse_follows_focus
    }

    fn update_focused_workspace(
        &mut self,
        follow_focus: bool,
        trigger_focus: bool,
    ) -> eyre::Result<()> {
        WindowManager::update_focused_workspace(self, follow_focus, trigger_focus)
    }

    fn direction_from_monitor_idx(&self, idx: usize) -> Option<OperationDirection> {
        WindowManager::direction_from_monitor_idx(self, idx)
    }

    fn move_container_to_monitor(
        &mut self,
        idx: usize,
        workspace_idx: Option<usize>,
        follow: bool,
        direction: Option<OperationDirection>,
    ) -> eyre::Result<()> {
        WindowManager::move_container_to_monitor(self, idx, workspace_idx, follow, direction)
    }

    fn focused_workspace_idx(&self) -> eyre::Result<usize> {
        WindowManager::focused_workspace_idx(self)
    }
}

impl komorebi_core::ContainerCommandHost for WindowManager {
    fn monitors_mut(&mut self) -> &mut std::collections::VecDeque<crate::monitor::Monitor> {
        WindowManager::monitors_mut(self)
    }
}

/// The window manager as the window commands of `komorebi_core::command` see it
impl komorebi_core::WindowCommandHost for WindowManager {
    fn move_container_in_direction(&mut self, direction: OperationDirection) -> eyre::Result<()> {
        WindowManager::move_container_in_direction(self, direction)
    }

    fn move_floating_window_in_direction(
        &mut self,
        direction: OperationDirection,
    ) -> eyre::Result<()> {
        WindowManager::move_floating_window_in_direction(self, direction)
    }

    fn focus_container_in_direction(&mut self, direction: OperationDirection) -> eyre::Result<()> {
        WindowManager::focus_container_in_direction(self, direction)
    }

    fn focus_floating_window_in_direction(
        &mut self,
        direction: OperationDirection,
    ) -> eyre::Result<()> {
        WindowManager::focus_floating_window_in_direction(self, direction)
    }

    fn focus_container_in_cycle_direction(
        &mut self,
        direction: CycleDirection,
    ) -> eyre::Result<()> {
        WindowManager::focus_container_in_cycle_direction(self, direction)
    }

    fn focus_floating_window_in_cycle_direction(
        &mut self,
        direction: CycleDirection,
    ) -> eyre::Result<()> {
        WindowManager::focus_floating_window_in_cycle_direction(self, direction)
    }

    fn preselect_container_in_direction(
        &mut self,
        direction: OperationDirection,
    ) -> eyre::Result<()> {
        WindowManager::preselect_container_in_direction(self, direction)
    }

    fn promote_container_to_front(&mut self) -> eyre::Result<()> {
        WindowManager::promote_container_to_front(self)
    }

    fn update_focused_workspace_layout(&mut self) -> eyre::Result<()> {
        self.focused_workspace_mut()?.update()
    }
}

/// The window manager as the behaviour toggles of `komorebi_core::command` see it
impl komorebi_core::BehaviourCommandHost for WindowManager {
    fn window_container_behaviour(&self) -> komorebi_config::behaviour::WindowContainerBehaviour {
        self.window_management_behaviour.current_behaviour
    }

    fn set_window_container_behaviour(
        &mut self,
        behaviour: komorebi_config::behaviour::WindowContainerBehaviour,
    ) {
        self.window_management_behaviour.current_behaviour = behaviour;
    }

    fn cross_monitor_move_behaviour(&self) -> komorebi_protocol::types::MoveBehaviour {
        self.cross_monitor_move_behaviour
    }

    fn set_cross_monitor_move_behaviour(
        &mut self,
        behaviour: komorebi_protocol::types::MoveBehaviour,
    ) {
        self.cross_monitor_move_behaviour = behaviour;
    }

    fn monocle_focus_behaviour(&self) -> komorebi_protocol::types::MonocleFocusBehaviour {
        self.monocle_focus_behaviour
    }

    fn set_monocle_focus_behaviour(
        &mut self,
        behaviour: komorebi_protocol::types::MonocleFocusBehaviour,
    ) {
        self.monocle_focus_behaviour = behaviour;
    }
}

#[cfg(test)]
mod tests {
    use crate::SocketMessage;
    use crate::WindowManagerEvent;
    use crate::core::Rect;
    use crate::monitor;
    use crate::window_manager::WindowManager;
    use crossbeam_channel::Receiver;
    use crossbeam_channel::Sender;
    use crossbeam_channel::bounded;
    use objc2_core_foundation::CFRunLoop;
    use std::io::BufRead;
    use std::io::BufReader;
    use std::io::Write;
    use std::os::unix::net::UnixStream;
    use std::path::PathBuf;
    use std::str::FromStr;
    use std::time::Duration;
    use uuid::Uuid;

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
        let socket_name = format!("komorebi-test-{}.sock", Uuid::new_v4());
        let socket_path = PathBuf::from(&socket_name);
        let mut wm = WindowManager::new(
            &CFRunLoop::main().unwrap(),
            receiver,
            Some(socket_path.clone()),
        )
        .unwrap();
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
