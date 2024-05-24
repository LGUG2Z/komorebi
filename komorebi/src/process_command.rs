use std::collections::HashMap;
use std::fs::File;
use std::fs::OpenOptions;
use std::io::BufRead;
use std::io::BufReader;
use std::io::Read;
use std::net::TcpListener;
use std::net::TcpStream;
use std::num::NonZeroUsize;
use std::str::FromStr;
use std::sync::atomic::Ordering;
use std::sync::Arc;
use std::time::Duration;

use color_eyre::eyre::anyhow;
use color_eyre::Result;
use miow::pipe::connect;
use net2::TcpStreamExt;
use parking_lot::Mutex;
use schemars::gen::SchemaSettings;
use schemars::schema_for;
use uds_windows::UnixStream;

use komorebi_core::config_generation::ApplicationConfiguration;
use komorebi_core::config_generation::IdWithIdentifier;
use komorebi_core::config_generation::MatchingRule;
use komorebi_core::config_generation::MatchingStrategy;
use komorebi_core::ApplicationIdentifier;
use komorebi_core::Axis;
use komorebi_core::FocusFollowsMouseImplementation;
use komorebi_core::Layout;
use komorebi_core::MoveBehaviour;
use komorebi_core::OperationDirection;
use komorebi_core::Rect;
use komorebi_core::Sizing;
use komorebi_core::SocketMessage;
use komorebi_core::StateQuery;
use komorebi_core::WindowContainerBehaviour;
use komorebi_core::WindowKind;

use crate::border_manager;
use crate::border_manager::STYLE;
use crate::colour::Rgb;
use crate::current_virtual_desktop;
use crate::notify_subscribers;
use crate::stackbar_manager;
use crate::static_config::StaticConfig;
use crate::window::RuleDebug;
use crate::window::Window;
use crate::window_manager;
use crate::window_manager::WindowManager;
use crate::windows_api::WindowsApi;
use crate::GlobalState;
use crate::Notification;
use crate::NotificationEvent;
use crate::CUSTOM_FFM;
use crate::DATA_DIR;
use crate::DISPLAY_INDEX_PREFERENCES;
use crate::FLOAT_IDENTIFIERS;
use crate::HIDING_BEHAVIOUR;
use crate::INITIAL_CONFIGURATION_LOADED;
use crate::LAYERED_WHITELIST;
use crate::MANAGE_IDENTIFIERS;
use crate::MONITOR_INDEX_PREFERENCES;
use crate::NO_TITLEBAR;
use crate::OBJECT_NAME_CHANGE_ON_LAUNCH;
use crate::REMOVE_TITLEBARS;
use crate::SUBSCRIPTION_PIPES;
use crate::SUBSCRIPTION_SOCKETS;
use crate::TCP_CONNECTIONS;
use crate::TRAY_AND_MULTI_WINDOW_IDENTIFIERS;
use crate::WORKSPACE_RULES;
use stackbar_manager::STACKBAR_FOCUSED_TEXT_COLOUR;
use stackbar_manager::STACKBAR_LABEL;
use stackbar_manager::STACKBAR_MODE;
use stackbar_manager::STACKBAR_TAB_BACKGROUND_COLOUR;
use stackbar_manager::STACKBAR_TAB_HEIGHT;
use stackbar_manager::STACKBAR_TAB_WIDTH;
use stackbar_manager::STACKBAR_UNFOCUSED_TEXT_COLOUR;

#[tracing::instrument]
pub fn listen_for_commands(wm: Arc<Mutex<WindowManager>>) {
    let listener = wm
        .lock()
        .command_listener
        .try_clone()
        .expect("could not clone unix listener");

    std::thread::spawn(move || {
        tracing::info!("listening on komorebi.sock");
        for client in listener.incoming() {
            match client {
                Ok(stream) => match read_commands_uds(&wm, stream) {
                    Ok(()) => {}
                    Err(error) => tracing::error!("{}", error),
                },
                Err(error) => {
                    tracing::error!("{}", error);
                    break;
                }
            }
        }
    });
}

#[tracing::instrument]
pub fn listen_for_commands_tcp(wm: Arc<Mutex<WindowManager>>, port: usize) {
    let listener =
        TcpListener::bind(format!("0.0.0.0:{port}")).expect("could not start tcp server");

    std::thread::spawn(move || {
        tracing::info!("listening on 0.0.0.0:43663");
        for client in listener.incoming() {
            match client {
                Ok(mut stream) => {
                    stream
                        .set_keepalive(Some(Duration::from_secs(30)))
                        .expect("TCP keepalive should be set");

                    let addr = stream
                        .peer_addr()
                        .expect("incoming connection should have an address")
                        .to_string();

                    let mut connections = TCP_CONNECTIONS.lock();

                    connections.insert(
                        addr.clone(),
                        stream.try_clone().expect("stream should be cloneable"),
                    );

                    tracing::info!("listening for incoming tcp messages from {}", &addr);

                    match read_commands_tcp(&wm, &mut stream, &addr) {
                        Ok(()) => {}
                        Err(error) => tracing::error!("{}", error),
                    }
                }
                Err(error) => {
                    tracing::error!("{}", error);
                    break;
                }
            }
        }
    });
}

impl WindowManager {
    // TODO(raggi): wrap reply in a newtype that can decorate a human friendly
    // name for the peer, such as getting the pid of the komorebic process for
    // the UDS or the IP:port for TCP.
    #[tracing::instrument(skip(self, reply))]
    pub fn process_command(
        &mut self,
        message: SocketMessage,
        mut reply: impl std::io::Write,
    ) -> Result<()> {
        if let Some(virtual_desktop_id) = &self.virtual_desktop_id {
            if let Some(id) = current_virtual_desktop() {
                if id != *virtual_desktop_id {
                    tracing::info!(
                        "ignoring events and commands while not on virtual desktop {:?}",
                        virtual_desktop_id
                    );
                    return Ok(());
                }
            }
        }

        match message {
            SocketMessage::CycleFocusWorkspace(_) | SocketMessage::FocusWorkspaceNumber(_) => {
                if let Some(monitor) = self.focused_monitor_mut() {
                    let idx = monitor.focused_workspace_idx();
                    monitor.set_last_focused_workspace(Option::from(idx));
                }
            }
            SocketMessage::FocusMonitorWorkspaceNumber(target_monitor_idx, _) => {
                let idx = self.focused_workspace_idx_for_monitor_idx(target_monitor_idx)?;
                if let Some(monitor) = self.monitors_mut().get_mut(target_monitor_idx) {
                    monitor.set_last_focused_workspace(Option::from(idx));
                }
            }

            _ => {}
        };

        match message {
            SocketMessage::Promote => self.promote_container_to_front()?,
            SocketMessage::PromoteFocus => self.promote_focus_to_front()?,
            SocketMessage::PromoteWindow(direction) => {
                self.focus_container_in_direction(direction)?;
                self.promote_container_to_front()?
            }
            SocketMessage::FocusWindow(direction) => {
                self.focus_container_in_direction(direction)?;
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
            SocketMessage::StackWindow(direction) => self.add_window_to_container(direction)?,
            SocketMessage::UnstackWindow => self.remove_window_from_container()?,
            SocketMessage::StackAll => self.stack_all()?,
            SocketMessage::UnstackAll => self.unstack_all()?,
            SocketMessage::CycleStack(direction) => {
                self.cycle_container_window_in_direction(direction)?;
                self.focused_window()?.focus(self.mouse_follows_focus)?;
            }
            SocketMessage::ForceFocus => {
                let focused_window = self.focused_window()?;
                let focused_window_rect = WindowsApi::window_rect(focused_window.hwnd())?;
                WindowsApi::center_cursor_in_rect(&focused_window_rect)?;
                WindowsApi::left_click();
            }
            SocketMessage::Close => {
                Window {
                    hwnd: WindowsApi::foreground_window()?,
                }
                .close()?;
            }
            SocketMessage::Minimize => {
                Window {
                    hwnd: WindowsApi::foreground_window()?,
                }
                .minimize();
            }
            SocketMessage::ToggleFloat => self.toggle_float()?,
            SocketMessage::ToggleMonocle => self.toggle_monocle()?,
            SocketMessage::ToggleMaximize => self.toggle_maximize()?,
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
            SocketMessage::InitialWorkspaceRule(_, ref id, monitor_idx, workspace_idx) => {
                self.handle_initial_workspace_rules(id, monitor_idx, workspace_idx)?;
            }
            SocketMessage::InitialNamedWorkspaceRule(_, ref id, ref workspace) => {
                if let Some((monitor_idx, workspace_idx)) =
                    self.monitor_workspace_index_by_name(workspace)
                {
                    self.handle_initial_workspace_rules(id, monitor_idx, workspace_idx)?;
                }
            }
            SocketMessage::WorkspaceRule(_, ref id, monitor_idx, workspace_idx) => {
                self.handle_definitive_workspace_rules(id, monitor_idx, workspace_idx)?;
            }
            SocketMessage::NamedWorkspaceRule(_, ref id, ref workspace) => {
                if let Some((monitor_idx, workspace_idx)) =
                    self.monitor_workspace_index_by_name(workspace)
                {
                    self.handle_definitive_workspace_rules(id, monitor_idx, workspace_idx)?;
                }
            }
            SocketMessage::ManageRule(identifier, ref id) => {
                let mut manage_identifiers = MANAGE_IDENTIFIERS.lock();

                let mut should_push = true;
                for m in &*manage_identifiers {
                    if let MatchingRule::Simple(m) = m {
                        if m.id.eq(id) {
                            should_push = false;
                        }
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
            SocketMessage::FloatRule(identifier, ref id) => {
                let mut float_identifiers = FLOAT_IDENTIFIERS.lock();

                let mut should_push = true;
                for f in &*float_identifiers {
                    if let MatchingRule::Simple(f) = f {
                        if f.id.eq(id) {
                            should_push = false;
                        }
                    }
                }

                if should_push {
                    float_identifiers.push(MatchingRule::Simple(IdWithIdentifier {
                        kind: identifier,
                        id: id.clone(),
                        matching_strategy: Option::from(MatchingStrategy::Legacy),
                    }));
                }

                let offset = self.work_area_offset;

                let mut hwnds_to_purge = vec![];
                for (i, monitor) in self.monitors().iter().enumerate() {
                    for container in monitor
                        .focused_workspace()
                        .ok_or_else(|| anyhow!("there is no workspace"))?
                        .containers()
                    {
                        for window in container.windows() {
                            match identifier {
                                ApplicationIdentifier::Path => {
                                    if window.path()? == *id {
                                        hwnds_to_purge.push((i, window.hwnd));
                                    }
                                }
                                ApplicationIdentifier::Exe => {
                                    if window.exe()? == *id {
                                        hwnds_to_purge.push((i, window.hwnd));
                                    }
                                }
                                ApplicationIdentifier::Class => {
                                    if window.class()? == *id {
                                        hwnds_to_purge.push((i, window.hwnd));
                                    }
                                }
                                ApplicationIdentifier::Title => {
                                    if window.title()? == *id {
                                        hwnds_to_purge.push((i, window.hwnd));
                                    }
                                }
                            }
                        }
                    }
                }

                for (monitor_idx, hwnd) in hwnds_to_purge {
                    let monitor = self
                        .monitors_mut()
                        .get_mut(monitor_idx)
                        .ok_or_else(|| anyhow!("there is no monitor"))?;

                    monitor
                        .focused_workspace_mut()
                        .ok_or_else(|| anyhow!("there is no focused workspace"))?
                        .remove_window(hwnd)?;

                    monitor.update_focused_workspace(offset)?;
                }
            }
            SocketMessage::FocusedWorkspaceContainerPadding(adjustment) => {
                let focused_monitor_idx = self.focused_monitor_idx();

                let focused_monitor = self
                    .focused_monitor()
                    .ok_or_else(|| anyhow!("there is no monitor"))?;

                let focused_workspace_idx = focused_monitor.focused_workspace_idx();

                self.set_container_padding(focused_monitor_idx, focused_workspace_idx, adjustment)?;
            }
            SocketMessage::FocusedWorkspacePadding(adjustment) => {
                let focused_monitor_idx = self.focused_monitor_idx();

                let focused_monitor = self
                    .focused_monitor()
                    .ok_or_else(|| anyhow!("there is no monitor"))?;

                let focused_workspace_idx = focused_monitor.focused_workspace_idx();

                self.set_workspace_padding(focused_monitor_idx, focused_workspace_idx, adjustment)?;
            }
            SocketMessage::AdjustContainerPadding(sizing, adjustment) => {
                self.adjust_container_padding(sizing, adjustment)?;
            }
            SocketMessage::AdjustWorkspacePadding(sizing, adjustment) => {
                self.adjust_workspace_padding(sizing, adjustment)?;
            }
            SocketMessage::MoveContainerToWorkspaceNumber(workspace_idx) => {
                self.move_container_to_workspace(workspace_idx, true)?;
            }
            SocketMessage::CycleMoveContainerToWorkspace(direction) => {
                let focused_monitor = self
                    .focused_monitor()
                    .ok_or_else(|| anyhow!("there is no monitor"))?;

                let focused_workspace_idx = focused_monitor.focused_workspace_idx();
                let workspaces = focused_monitor.workspaces().len();

                let workspace_idx = direction.next_idx(
                    focused_workspace_idx,
                    NonZeroUsize::new(workspaces)
                        .ok_or_else(|| anyhow!("there must be at least one workspace"))?,
                );

                self.move_container_to_workspace(workspace_idx, true)?;
            }
            SocketMessage::MoveContainerToMonitorNumber(monitor_idx) => {
                self.move_container_to_monitor(monitor_idx, None, true)?;
            }
            SocketMessage::SwapWorkspacesToMonitorNumber(monitor_idx) => {
                self.swap_focused_monitor(monitor_idx)?;
            }
            SocketMessage::CycleMoveContainerToMonitor(direction) => {
                let monitor_idx = direction.next_idx(
                    self.focused_monitor_idx(),
                    NonZeroUsize::new(self.monitors().len())
                        .ok_or_else(|| anyhow!("there must be at least one monitor"))?,
                );

                self.move_container_to_monitor(monitor_idx, None, true)?;
            }
            SocketMessage::SendContainerToWorkspaceNumber(workspace_idx) => {
                self.move_container_to_workspace(workspace_idx, false)?;
            }
            SocketMessage::CycleSendContainerToWorkspace(direction) => {
                let focused_monitor = self
                    .focused_monitor()
                    .ok_or_else(|| anyhow!("there is no monitor"))?;

                let focused_workspace_idx = focused_monitor.focused_workspace_idx();
                let workspaces = focused_monitor.workspaces().len();

                let workspace_idx = direction.next_idx(
                    focused_workspace_idx,
                    NonZeroUsize::new(workspaces)
                        .ok_or_else(|| anyhow!("there must be at least one workspace"))?,
                );

                self.move_container_to_workspace(workspace_idx, false)?;
            }
            SocketMessage::SendContainerToMonitorNumber(monitor_idx) => {
                self.move_container_to_monitor(monitor_idx, None, false)?;
            }
            SocketMessage::CycleSendContainerToMonitor(direction) => {
                let monitor_idx = direction.next_idx(
                    self.focused_monitor_idx(),
                    NonZeroUsize::new(self.monitors().len())
                        .ok_or_else(|| anyhow!("there must be at least one monitor"))?,
                );

                self.move_container_to_monitor(monitor_idx, None, false)?;
            }
            SocketMessage::SendContainerToMonitorWorkspaceNumber(monitor_idx, workspace_idx) => {
                self.move_container_to_monitor(monitor_idx, Option::from(workspace_idx), false)?;
            }
            SocketMessage::MoveContainerToMonitorWorkspaceNumber(monitor_idx, workspace_idx) => {
                self.move_container_to_monitor(monitor_idx, Option::from(workspace_idx), true)?;
            }
            SocketMessage::SendContainerToNamedWorkspace(ref workspace) => {
                if let Some((monitor_idx, workspace_idx)) =
                    self.monitor_workspace_index_by_name(workspace)
                {
                    self.move_container_to_monitor(
                        monitor_idx,
                        Option::from(workspace_idx),
                        false,
                    )?;
                }
            }
            SocketMessage::MoveContainerToNamedWorkspace(ref workspace) => {
                if let Some((monitor_idx, workspace_idx)) =
                    self.monitor_workspace_index_by_name(workspace)
                {
                    self.move_container_to_monitor(monitor_idx, Option::from(workspace_idx), true)?;
                }
            }

            SocketMessage::MoveWorkspaceToMonitorNumber(monitor_idx) => {
                self.move_workspace_to_monitor(monitor_idx)?;
            }
            SocketMessage::CycleMoveWorkspaceToMonitor(direction) => {
                let monitor_idx = direction.next_idx(
                    self.focused_monitor_idx(),
                    NonZeroUsize::new(self.monitors().len())
                        .ok_or_else(|| anyhow!("there must be at least one monitor"))?,
                );

                self.move_workspace_to_monitor(monitor_idx)?;
            }
            SocketMessage::TogglePause => {
                if self.is_paused {
                    tracing::info!("resuming");
                } else {
                    tracing::info!("pausing");
                }

                self.is_paused = !self.is_paused;
                self.retile_all(true)?;
            }
            SocketMessage::ToggleTiling => {
                self.toggle_tiling()?;
            }
            SocketMessage::CycleFocusMonitor(direction) => {
                let monitor_idx = direction.next_idx(
                    self.focused_monitor_idx(),
                    NonZeroUsize::new(self.monitors().len())
                        .ok_or_else(|| anyhow!("there must be at least one monitor"))?,
                );

                self.focus_monitor(monitor_idx)?;
                self.update_focused_workspace(self.mouse_follows_focus, true)?;
            }
            SocketMessage::FocusMonitorNumber(monitor_idx) => {
                self.focus_monitor(monitor_idx)?;
                self.update_focused_workspace(self.mouse_follows_focus, true)?;
            }
            SocketMessage::Retile => {
                border_manager::destroy_all_borders()?;
                self.retile_all(false)?
            }
            SocketMessage::FlipLayout(layout_flip) => self.flip_layout(layout_flip)?,
            SocketMessage::ChangeLayout(layout) => self.change_workspace_layout_default(layout)?,
            SocketMessage::CycleLayout(direction) => self.cycle_layout(direction)?,
            SocketMessage::ChangeLayoutCustom(ref path) => {
                self.change_workspace_custom_layout(path)?;
            }
            SocketMessage::WorkspaceLayoutCustom(monitor_idx, workspace_idx, ref path) => {
                self.set_workspace_layout_custom(monitor_idx, workspace_idx, path)?;
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
            SocketMessage::WorkspaceLayoutCustomRule(
                monitor_idx,
                workspace_idx,
                at_container_count,
                ref path,
            ) => {
                self.add_workspace_layout_custom_rule(
                    monitor_idx,
                    workspace_idx,
                    at_container_count,
                    path,
                )?;
            }
            SocketMessage::ClearWorkspaceLayoutRules(monitor_idx, workspace_idx) => {
                self.clear_workspace_layout_rules(monitor_idx, workspace_idx)?;
            }
            SocketMessage::NamedWorkspaceLayoutCustom(ref workspace, ref path) => {
                if let Some((monitor_idx, workspace_idx)) =
                    self.monitor_workspace_index_by_name(workspace)
                {
                    self.set_workspace_layout_custom(monitor_idx, workspace_idx, path)?;
                }
            }
            SocketMessage::NamedWorkspaceTiling(ref workspace, tile) => {
                if let Some((monitor_idx, workspace_idx)) =
                    self.monitor_workspace_index_by_name(workspace)
                {
                    self.set_workspace_tiling(monitor_idx, workspace_idx, tile)?;
                }
            }
            SocketMessage::NamedWorkspaceLayout(ref workspace, layout) => {
                if let Some((monitor_idx, workspace_idx)) =
                    self.monitor_workspace_index_by_name(workspace)
                {
                    self.set_workspace_layout_default(monitor_idx, workspace_idx, layout)?;
                }
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
            SocketMessage::NamedWorkspaceLayoutCustomRule(
                ref workspace,
                at_container_count,
                ref path,
            ) => {
                if let Some((monitor_idx, workspace_idx)) =
                    self.monitor_workspace_index_by_name(workspace)
                {
                    self.add_workspace_layout_custom_rule(
                        monitor_idx,
                        workspace_idx,
                        at_container_count,
                        path,
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
            SocketMessage::CycleFocusWorkspace(direction) => {
                // This is to ensure that even on an empty workspace on a secondary monitor, the
                // secondary monitor where the cursor is focused will be used as the target for
                // the workspace switch op
                if let Some(monitor_idx) = self.monitor_idx_from_current_pos() {
                    self.focus_monitor(monitor_idx)?;
                }

                let focused_monitor = self
                    .focused_monitor()
                    .ok_or_else(|| anyhow!("there is no monitor"))?;

                let focused_workspace_idx = focused_monitor.focused_workspace_idx();
                let workspaces = focused_monitor.workspaces().len();

                let workspace_idx = direction.next_idx(
                    focused_workspace_idx,
                    NonZeroUsize::new(workspaces)
                        .ok_or_else(|| anyhow!("there must be at least one workspace"))?,
                );

                self.focus_workspace(workspace_idx)?;
            }
            SocketMessage::FocusLastWorkspace => {
                // This is to ensure that even on an empty workspace on a secondary monitor, the
                // secondary monitor where the cursor is focused will be used as the target for
                // the workspace switch op
                if let Some(monitor_idx) = self.monitor_idx_from_current_pos() {
                    self.focus_monitor(monitor_idx)?;
                }

                let idx = self
                    .focused_monitor()
                    .ok_or_else(|| anyhow!("there is no monitor"))?
                    .focused_workspace_idx();

                if let Some(monitor) = self.focused_monitor_mut() {
                    if let Some(last_focused_workspace) = monitor.last_focused_workspace() {
                        self.focus_workspace(last_focused_workspace)?;
                    }
                }

                self.focused_monitor_mut()
                    .ok_or_else(|| anyhow!("there is no monitor"))?
                    .set_last_focused_workspace(Option::from(idx));
            }
            SocketMessage::FocusWorkspaceNumber(workspace_idx) => {
                // This is to ensure that even on an empty workspace on a secondary monitor, the
                // secondary monitor where the cursor is focused will be used as the target for
                // the workspace switch op
                if let Some(monitor_idx) = self.monitor_idx_from_current_pos() {
                    self.focus_monitor(monitor_idx)?;
                }

                if self.focused_workspace_idx().unwrap_or_default() != workspace_idx {
                    self.focus_workspace(workspace_idx)?;
                }
            }
            SocketMessage::FocusWorkspaceNumbers(workspace_idx) => {
                // This is to ensure that even on an empty workspace on a secondary monitor, the
                // secondary monitor where the cursor is focused will be used as the target for
                // the workspace switch op
                if let Some(monitor_idx) = self.monitor_idx_from_current_pos() {
                    self.focus_monitor(monitor_idx)?;
                }

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
            SocketMessage::Stop => {
                tracing::info!(
                    "received stop command, restoring all hidden windows and terminating process"
                );
                self.restore_all_windows()?;

                if WindowsApi::focus_follows_mouse()? {
                    WindowsApi::disable_focus_follows_mouse()?;
                }

                std::process::exit(0)
            }
            SocketMessage::MonitorIndexPreference(index_preference, left, top, right, bottom) => {
                let mut monitor_index_preferences = MONITOR_INDEX_PREFERENCES.lock();
                monitor_index_preferences.insert(
                    index_preference,
                    Rect {
                        left,
                        top,
                        right,
                        bottom,
                    },
                );
            }
            SocketMessage::DisplayIndexPreference(index_preference, ref display) => {
                let mut display_index_preferences = DISPLAY_INDEX_PREFERENCES.lock();
                display_index_preferences.insert(index_preference, display.clone());
            }
            SocketMessage::EnsureWorkspaces(monitor_idx, workspace_count) => {
                self.ensure_workspaces_for_monitor(monitor_idx, workspace_count)?;
            }
            SocketMessage::EnsureNamedWorkspaces(monitor_idx, ref names) => {
                self.ensure_named_workspaces_for_monitor(monitor_idx, names)?;
            }
            SocketMessage::NewWorkspace => {
                self.new_workspace()?;
            }
            SocketMessage::WorkspaceName(monitor_idx, workspace_idx, ref name) => {
                self.set_workspace_name(monitor_idx, workspace_idx, name.to_string())?;
            }
            SocketMessage::State => {
                let state = match serde_json::to_string_pretty(&window_manager::State::from(&*self))
                {
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
                        monitor_visible_windows.insert(
                            monitor.device_id().clone(),
                            ws.visible_window_details().clone(),
                        );
                    }
                }

                let visible_windows_state =
                    match serde_json::to_string_pretty(&monitor_visible_windows) {
                        Ok(state) => state,
                        Err(error) => error.to_string(),
                    };

                reply.write_all(visible_windows_state.as_bytes())?;
            }

            SocketMessage::Query(query) => {
                let response = match query {
                    StateQuery::FocusedMonitorIndex => self.focused_monitor_idx(),
                    StateQuery::FocusedWorkspaceIndex => self
                        .focused_monitor()
                        .ok_or_else(|| anyhow!("there is no monitor"))?
                        .focused_workspace_idx(),
                    StateQuery::FocusedContainerIndex => {
                        self.focused_workspace()?.focused_container_idx()
                    }
                    StateQuery::FocusedWindowIndex => {
                        self.focused_container()?.focused_window_idx()
                    }
                }
                .to_string();

                reply.write_all(response.as_bytes())?;
            }
            SocketMessage::ResizeWindowEdge(direction, sizing) => {
                self.resize_window(direction, sizing, self.resize_delta, true)?;
            }
            SocketMessage::ResizeWindowAxis(axis, sizing) => {
                // If the user has a custom layout, allow for the resizing of the primary column
                // with this signal
                let workspace = self.focused_workspace_mut()?;
                let container_len = workspace.containers().len();
                let no_layout_rules = workspace.layout_rules().is_empty();

                if let Layout::Custom(ref mut custom) = workspace.layout_mut() {
                    if matches!(axis, Axis::Horizontal) {
                        #[allow(clippy::cast_precision_loss)]
                        let percentage = custom
                            .primary_width_percentage()
                            .unwrap_or(100.0 / (custom.len() as f32));

                        if no_layout_rules {
                            match sizing {
                                Sizing::Increase => {
                                    custom.set_primary_width_percentage(percentage + 5.0);
                                }
                                Sizing::Decrease => {
                                    custom.set_primary_width_percentage(percentage - 5.0);
                                }
                            }
                        } else {
                            for rule in workspace.layout_rules_mut() {
                                if container_len >= rule.0 {
                                    if let Layout::Custom(ref mut custom) = rule.1 {
                                        match sizing {
                                            Sizing::Increase => {
                                                custom
                                                    .set_primary_width_percentage(percentage + 5.0);
                                            }
                                            Sizing::Decrease => {
                                                custom
                                                    .set_primary_width_percentage(percentage - 5.0);
                                            }
                                        }
                                    }
                                }
                            }
                        }
                    }
                    // Otherwise proceed with the resizing logic for individual window containers in the
                    // assumed BSP layout
                } else {
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
                }

                self.update_focused_workspace(false, false)?;
            }
            SocketMessage::FocusFollowsMouse(mut implementation, enable) => {
                if !CUSTOM_FFM.load(Ordering::SeqCst) {
                    tracing::warn!(
                        "komorebi was not started with the --ffm flag, so the komorebi implementation of focus follows mouse cannot be enabled; defaulting to windows implementation"
                    );
                    implementation = FocusFollowsMouseImplementation::Windows;
                }

                match implementation {
                    FocusFollowsMouseImplementation::Komorebi => {
                        if WindowsApi::focus_follows_mouse()? {
                            tracing::warn!(
                                "the komorebi implementation of focus follows mouse cannot be enabled while the windows implementation is enabled"
                            );
                        } else if enable {
                            self.focus_follows_mouse = Option::from(implementation);
                        } else {
                            self.focus_follows_mouse = None;
                            self.has_pending_raise_op = false;
                        }
                    }
                    FocusFollowsMouseImplementation::Windows => {
                        if matches!(
                            self.focus_follows_mouse,
                            Some(FocusFollowsMouseImplementation::Komorebi)
                        ) {
                            tracing::warn!(
                                "the windows implementation of focus follows mouse cannot be enabled while the komorebi implementation is enabled"
                            );
                        } else if enable {
                            WindowsApi::enable_focus_follows_mouse()?;
                            self.focus_follows_mouse =
                                Option::from(FocusFollowsMouseImplementation::Windows);
                        } else {
                            WindowsApi::disable_focus_follows_mouse()?;
                            self.focus_follows_mouse = None;
                        }
                    }
                }
            }
            SocketMessage::ToggleFocusFollowsMouse(mut implementation) => {
                if !CUSTOM_FFM.load(Ordering::SeqCst) {
                    tracing::warn!(
                        "komorebi was not started with the --ffm flag, so the komorebi implementation of focus follows mouse cannot be toggled; defaulting to windows implementation"
                    );
                    implementation = FocusFollowsMouseImplementation::Windows;
                }

                match implementation {
                    FocusFollowsMouseImplementation::Komorebi => {
                        if WindowsApi::focus_follows_mouse()? {
                            tracing::warn!(
                                "the komorebi implementation of focus follows mouse cannot be toggled while the windows implementation is enabled"
                            );
                        } else {
                            match self.focus_follows_mouse {
                                None => {
                                    self.focus_follows_mouse = Option::from(implementation);
                                    self.has_pending_raise_op = false;
                                }
                                Some(FocusFollowsMouseImplementation::Komorebi) => {
                                    self.focus_follows_mouse = None;
                                }
                                Some(FocusFollowsMouseImplementation::Windows) => {
                                    tracing::warn!("ignoring command that could mix different focus follows mouse implementations");
                                }
                            }
                        }
                    }
                    FocusFollowsMouseImplementation::Windows => {
                        if matches!(
                            self.focus_follows_mouse,
                            Some(FocusFollowsMouseImplementation::Komorebi)
                        ) {
                            tracing::warn!(
                                "the windows implementation of focus follows mouse cannot be toggled while the komorebi implementation is enabled"
                            );
                        } else {
                            match self.focus_follows_mouse {
                                None => {
                                    WindowsApi::enable_focus_follows_mouse()?;
                                    self.focus_follows_mouse = Option::from(implementation);
                                }
                                Some(FocusFollowsMouseImplementation::Windows) => {
                                    WindowsApi::disable_focus_follows_mouse()?;
                                    self.focus_follows_mouse = None;
                                }
                                Some(FocusFollowsMouseImplementation::Komorebi) => {
                                    tracing::warn!("ignoring command that could mix different focus follows mouse implementations");
                                }
                            }
                        }
                    }
                }
            }
            SocketMessage::ReloadConfiguration => {
                Self::reload_configuration();
            }
            SocketMessage::ReloadStaticConfiguration(ref pathbuf) => {
                self.reload_static_configuration(pathbuf)?;
            }
            SocketMessage::CompleteConfiguration => {
                if !INITIAL_CONFIGURATION_LOADED.load(Ordering::SeqCst) {
                    INITIAL_CONFIGURATION_LOADED.store(true, Ordering::SeqCst);
                    self.update_focused_workspace(false, false)?;
                }
            }
            SocketMessage::WatchConfiguration(enable) => {
                self.watch_configuration(enable)?;
            }
            SocketMessage::IdentifyObjectNameChangeApplication(identifier, ref id) => {
                let mut identifiers = OBJECT_NAME_CHANGE_ON_LAUNCH.lock();

                let mut should_push = true;
                for i in &*identifiers {
                    if let MatchingRule::Simple(i) = i {
                        if i.id.eq(id) {
                            should_push = false;
                        }
                    }
                }

                if should_push {
                    identifiers.push(MatchingRule::Simple(IdWithIdentifier {
                        kind: identifier,
                        id: id.clone(),
                        matching_strategy: Option::from(MatchingStrategy::Legacy),
                    }));
                }
            }
            SocketMessage::IdentifyTrayApplication(identifier, ref id) => {
                let mut identifiers = TRAY_AND_MULTI_WINDOW_IDENTIFIERS.lock();
                let mut should_push = true;
                for i in &*identifiers {
                    if let MatchingRule::Simple(i) = i {
                        if i.id.eq(id) {
                            should_push = false;
                        }
                    }
                }

                if should_push {
                    identifiers.push(MatchingRule::Simple(IdWithIdentifier {
                        kind: identifier,
                        id: id.clone(),
                        matching_strategy: Option::from(MatchingStrategy::Legacy),
                    }));
                }
            }
            SocketMessage::IdentifyLayeredApplication(identifier, ref id) => {
                let mut identifiers = LAYERED_WHITELIST.lock();

                let mut should_push = true;
                for i in &*identifiers {
                    if let MatchingRule::Simple(i) = i {
                        if i.id.eq(id) {
                            should_push = false;
                        }
                    }
                }

                if should_push {
                    identifiers.push(MatchingRule::Simple(IdWithIdentifier {
                        kind: identifier,
                        id: id.clone(),
                        matching_strategy: Option::from(MatchingStrategy::Legacy),
                    }));
                }
            }
            SocketMessage::ManageFocusedWindow => {
                self.manage_focused_window()?;
            }
            SocketMessage::UnmanageFocusedWindow => {
                self.unmanage_focused_window()?;
            }
            SocketMessage::InvisibleBorders(_rect) => {}
            SocketMessage::WorkAreaOffset(rect) => {
                self.work_area_offset = Option::from(rect);
                self.retile_all(false)?;
            }
            SocketMessage::MonitorWorkAreaOffset(monitor_idx, rect) => {
                if let Some(monitor) = self.monitors_mut().get_mut(monitor_idx) {
                    monitor.set_work_area_offset(Option::from(rect));
                    self.retile_all(false)?;
                }
            }
            SocketMessage::QuickSave => {
                let workspace = self.focused_workspace()?;
                let resize = workspace.resize_dimensions();

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

                let file = File::open(&quicksave_json)
                    .map_err(|_| anyhow!("no quicksave found at {}", quicksave_json.display()))?;

                let resize: Vec<Option<Rect>> = serde_json::from_reader(file)?;

                workspace.set_resize_dimensions(resize);
                self.update_focused_workspace(false, false)?;
            }
            SocketMessage::Save(ref path) => {
                let workspace = self.focused_workspace_mut()?;
                let resize = workspace.resize_dimensions();

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
                    File::open(path).map_err(|_| anyhow!("no file found at {}", path.display()))?;

                let resize: Vec<Option<Rect>> = serde_json::from_reader(file)?;

                workspace.set_resize_dimensions(resize);
                self.update_focused_workspace(false, false)?;
            }
            SocketMessage::AddSubscriberSocket(ref socket) => {
                let mut sockets = SUBSCRIPTION_SOCKETS.lock();
                let socket_path = DATA_DIR.join(socket);
                sockets.insert(socket.clone(), socket_path);
            }
            SocketMessage::RemoveSubscriberSocket(ref socket) => {
                let mut sockets = SUBSCRIPTION_SOCKETS.lock();
                sockets.remove(socket);
            }
            SocketMessage::AddSubscriberPipe(ref subscriber) => {
                let mut pipes = SUBSCRIPTION_PIPES.lock();
                let pipe_path = format!(r"\\.\pipe\{subscriber}");
                let pipe = connect(&pipe_path).map_err(|_| {
                    anyhow!("the named pipe '{}' has not yet been created; please create it before running this command", pipe_path)
                })?;

                pipes.insert(subscriber.clone(), pipe);
            }
            SocketMessage::RemoveSubscriberPipe(ref subscriber) => {
                let mut pipes = SUBSCRIPTION_PIPES.lock();
                pipes.remove(subscriber);
            }
            SocketMessage::MouseFollowsFocus(enable) => {
                self.mouse_follows_focus = enable;
            }
            SocketMessage::ToggleMouseFollowsFocus => {
                self.mouse_follows_focus = !self.mouse_follows_focus;
            }
            SocketMessage::ResizeDelta(delta) => {
                self.resize_delta = delta;
            }
            SocketMessage::ToggleWindowContainerBehaviour => {
                match self.window_container_behaviour {
                    WindowContainerBehaviour::Create => {
                        self.window_container_behaviour = WindowContainerBehaviour::Append;
                    }
                    WindowContainerBehaviour::Append => {
                        self.window_container_behaviour = WindowContainerBehaviour::Create;
                    }
                }
            }
            SocketMessage::WindowHidingBehaviour(behaviour) => {
                let mut hiding_behaviour = HIDING_BEHAVIOUR.lock();
                *hiding_behaviour = behaviour;
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
            SocketMessage::UnmanagedWindowOperationBehaviour(behaviour) => {
                self.unmanaged_window_operation_behaviour = behaviour;
            }
            SocketMessage::Border(enable) => {
                border_manager::BORDER_ENABLED.store(enable, Ordering::SeqCst);
            }
            SocketMessage::BorderColour(kind, r, g, b) => match kind {
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
            },
            SocketMessage::BorderStyle(style) => {
                let mut border_style = STYLE.lock();
                *border_style = style;
            }
            SocketMessage::BorderWidth(width) => {
                border_manager::BORDER_WIDTH.store(width, Ordering::SeqCst);
            }
            SocketMessage::BorderOffset(offset) => {
                border_manager::BORDER_OFFSET.store(offset, Ordering::SeqCst);
            }
            SocketMessage::StackbarMode(mode) => {
                STACKBAR_MODE.store(mode);
            }
            SocketMessage::StackbarLabel(label) => {
                STACKBAR_LABEL.store(label);
            }
            SocketMessage::StackbarFocusedTextColour(r, g, b) => {
                let rgb = Rgb::new(r, g, b);
                STACKBAR_FOCUSED_TEXT_COLOUR.store(rgb.into(), Ordering::SeqCst);
            }
            SocketMessage::StackbarUnfocusedTextColour(r, g, b) => {
                let rgb = Rgb::new(r, g, b);
                STACKBAR_UNFOCUSED_TEXT_COLOUR.store(rgb.into(), Ordering::SeqCst);
            }
            SocketMessage::StackbarBackgroundColour(r, g, b) => {
                let rgb = Rgb::new(r, g, b);
                STACKBAR_TAB_BACKGROUND_COLOUR.store(rgb.into(), Ordering::SeqCst);
            }
            SocketMessage::StackbarHeight(height) => {
                STACKBAR_TAB_HEIGHT.store(height, Ordering::SeqCst);
            }
            SocketMessage::StackbarTabWidth(width) => {
                STACKBAR_TAB_WIDTH.store(width, Ordering::SeqCst);
            }
            SocketMessage::ApplicationSpecificConfigurationSchema => {
                let asc = schema_for!(Vec<ApplicationConfiguration>);
                let schema = serde_json::to_string_pretty(&asc)?;

                reply.write_all(schema.as_bytes())?;
            }
            SocketMessage::NotificationSchema => {
                let notification = schema_for!(Notification);
                let schema = serde_json::to_string_pretty(&notification)?;

                reply.write_all(schema.as_bytes())?;
            }
            SocketMessage::SocketSchema => {
                let socket_message = schema_for!(SocketMessage);
                let schema = serde_json::to_string_pretty(&socket_message)?;

                reply.write_all(schema.as_bytes())?;
            }
            SocketMessage::StaticConfigSchema => {
                let settings = SchemaSettings::default().with(|s| {
                    s.option_nullable = false;
                    s.option_add_null_type = false;
                    s.inline_subschemas = true;
                });

                let gen = settings.into_generator();
                let socket_message = gen.into_root_schema_for::<StaticConfig>();
                let schema = serde_json::to_string_pretty(&socket_message)?;

                reply.write_all(schema.as_bytes())?;
            }
            SocketMessage::GenerateStaticConfig => {
                let config = serde_json::to_string_pretty(&StaticConfig::from(&*self))?;

                reply.write_all(config.as_bytes())?;
            }
            SocketMessage::RemoveTitleBar(_, ref id) => {
                let mut identifiers = NO_TITLEBAR.lock();
                if !identifiers.contains(id) {
                    identifiers.push(id.clone());
                }
            }
            SocketMessage::ToggleTitleBars => {
                let current = REMOVE_TITLEBARS.load(Ordering::SeqCst);
                REMOVE_TITLEBARS.store(!current, Ordering::SeqCst);
                self.update_focused_workspace(false, false)?;
            }
            SocketMessage::DebugWindow(hwnd) => {
                let window = Window { hwnd };
                let mut rule_debug = RuleDebug::default();
                let _ = window.should_manage(None, &mut rule_debug);
                let schema = serde_json::to_string_pretty(&rule_debug)?;

                reply.write_all(schema.as_bytes())?;
            }
            // Deprecated commands
            SocketMessage::AltFocusHack(_)
            | SocketMessage::IdentifyBorderOverflowApplication(_, _) => {}
        };

        let notification = Notification {
            event: NotificationEvent::Socket(message.clone()),
            state: self.as_ref().into(),
        };

        notify_subscribers(&serde_json::to_string(&notification)?)?;
        border_manager::event_tx().send(border_manager::Notification)?;
        stackbar_manager::event_tx().send(stackbar_manager::Notification)?;

        tracing::info!("processed");
        Ok(())
    }

    #[tracing::instrument(skip(self), level = "debug")]
    fn handle_initial_workspace_rules(
        &mut self,
        id: &String,
        monitor_idx: usize,
        workspace_idx: usize,
    ) -> Result<()> {
        self.handle_workspace_rules(id, monitor_idx, workspace_idx, true)?;

        Ok(())
    }

    #[tracing::instrument(skip(self), level = "debug")]
    fn handle_definitive_workspace_rules(
        &mut self,
        id: &String,
        monitor_idx: usize,
        workspace_idx: usize,
    ) -> Result<()> {
        self.handle_workspace_rules(id, monitor_idx, workspace_idx, false)?;

        Ok(())
    }

    #[tracing::instrument(skip(self), level = "debug")]
    pub fn handle_workspace_rules(
        &mut self,
        id: &String,
        monitor_idx: usize,
        workspace_idx: usize,
        initial_workspace_rule: bool,
    ) -> Result<()> {
        {
            let mut workspace_rules = WORKSPACE_RULES.lock();
            workspace_rules.insert(
                id.to_string(),
                (monitor_idx, workspace_idx, initial_workspace_rule),
            );
        }

        self.enforce_workspace_rules()?;

        Ok(())
    }
}

pub fn read_commands_uds(wm: &Arc<Mutex<WindowManager>>, mut stream: UnixStream) -> Result<()> {
    let reader = BufReader::new(stream.try_clone()?);
    // TODO(raggi): while this processes more than one command, if there are
    // replies there is no clearly defined protocol for framing yet - it's
    // perhaps whole-json objects for now, but termination is signalled by
    // socket shutdown.
    for line in reader.lines() {
        let message = SocketMessage::from_str(&line?)?;

        let mut wm = wm.lock();

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

    Ok(())
}

pub fn read_commands_tcp(
    wm: &Arc<Mutex<WindowManager>>,
    stream: &mut TcpStream,
    addr: &str,
) -> Result<()> {
    let mut reader = BufReader::new(stream.try_clone()?);

    loop {
        let mut buf = vec![0; 1024];
        match reader.read(&mut buf) {
            Err(..) => {
                tracing::warn!("removing disconnected tcp client: {addr}");
                let mut connections = TCP_CONNECTIONS.lock();
                connections.remove(addr);
                break;
            }
            Ok(size) => {
                let Ok(message) = SocketMessage::from_str(&String::from_utf8_lossy(&buf[..size]))
                else {
                    tracing::warn!("client sent an invalid message, disconnecting: {addr}");
                    let mut connections = TCP_CONNECTIONS.lock();
                    connections.remove(addr);
                    break;
                };

                let mut wm = wm.lock();

                if wm.is_paused {
                    return match message {
                        SocketMessage::TogglePause
                        | SocketMessage::State
                        | SocketMessage::GlobalState
                        | SocketMessage::Stop => Ok(wm.process_command(message, stream)?),
                        _ => {
                            tracing::trace!("ignoring while paused");
                            Ok(())
                        }
                    };
                }

                wm.process_command(message.clone(), &mut *stream)?;
            }
        }
    }

    Ok(())
}
