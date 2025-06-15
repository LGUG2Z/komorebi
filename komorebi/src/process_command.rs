use color_eyre::eyre::anyhow;
use color_eyre::eyre::OptionExt;
use color_eyre::Result;
use komorebi_themes::colour::Rgb;
use miow::pipe::connect;
use net2::TcpStreamExt;
use parking_lot::Mutex;
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
use uds_windows::UnixStream;

use crate::animation::ANIMATION_DURATION_GLOBAL;
use crate::animation::ANIMATION_DURATION_PER_ANIMATION;
use crate::animation::ANIMATION_ENABLED_GLOBAL;
use crate::animation::ANIMATION_ENABLED_PER_ANIMATION;
use crate::animation::ANIMATION_FPS;
use crate::animation::ANIMATION_STYLE_GLOBAL;
use crate::animation::ANIMATION_STYLE_PER_ANIMATION;
use crate::border_manager;
use crate::border_manager::IMPLEMENTATION;
use crate::border_manager::STYLE;
use crate::build;
use crate::config_generation::WorkspaceMatchingRule;
use crate::core::config_generation::IdWithIdentifier;
use crate::core::config_generation::MatchingRule;
use crate::core::config_generation::MatchingStrategy;
use crate::core::ApplicationIdentifier;
use crate::core::Axis;
use crate::core::BorderImplementation;
use crate::core::FocusFollowsMouseImplementation;
use crate::core::Layout;
use crate::core::MoveBehaviour;
use crate::core::OperationDirection;
use crate::core::Rect;
use crate::core::Sizing;
use crate::core::SocketMessage;
use crate::core::StateQuery;
use crate::core::WindowContainerBehaviour;
use crate::core::WindowKind;
use crate::current_virtual_desktop;
use crate::default_layout::LayoutOptions;
use crate::default_layout::ScrollingLayoutOptions;
use crate::monitor::MonitorInformation;
use crate::notify_subscribers;
use crate::stackbar_manager;
use crate::stackbar_manager::STACKBAR_FONT_FAMILY;
use crate::stackbar_manager::STACKBAR_FONT_SIZE;
use crate::static_config::StaticConfig;
use crate::theme_manager;
use crate::transparency_manager;
use crate::window::RuleDebug;
use crate::window::Window;
use crate::window_manager;
use crate::window_manager::WindowManager;
use crate::windows_api::WindowsApi;
use crate::winevent_listener;
use crate::workspace::WorkspaceLayer;
use crate::workspace::WorkspaceWindowLocation;
use crate::GlobalState;
use crate::Notification;
use crate::NotificationEvent;
use crate::State;
use crate::CUSTOM_FFM;
use crate::DATA_DIR;
use crate::DISPLAY_INDEX_PREFERENCES;
use crate::FLOATING_APPLICATIONS;
use crate::HIDING_BEHAVIOUR;
use crate::IGNORE_IDENTIFIERS;
use crate::INITIAL_CONFIGURATION_LOADED;
use crate::LAYERED_WHITELIST;
use crate::MANAGE_IDENTIFIERS;
use crate::MONITOR_INDEX_PREFERENCES;
use crate::NO_TITLEBAR;
use crate::OBJECT_NAME_CHANGE_ON_LAUNCH;
use crate::REMOVE_TITLEBARS;
use crate::SESSION_FLOATING_APPLICATIONS;
use crate::SUBSCRIPTION_PIPES;
use crate::SUBSCRIPTION_SOCKETS;
use crate::SUBSCRIPTION_SOCKET_OPTIONS;
use crate::TCP_CONNECTIONS;
use crate::TRAY_AND_MULTI_WINDOW_IDENTIFIERS;
use crate::WINDOWS_11;
use crate::WORKSPACE_MATCHING_RULES;
use stackbar_manager::STACKBAR_FOCUSED_TEXT_COLOUR;
use stackbar_manager::STACKBAR_LABEL;
use stackbar_manager::STACKBAR_MODE;
use stackbar_manager::STACKBAR_TAB_BACKGROUND_COLOUR;
use stackbar_manager::STACKBAR_TAB_HEIGHT;
use stackbar_manager::STACKBAR_TAB_WIDTH;
use stackbar_manager::STACKBAR_UNFOCUSED_TEXT_COLOUR;

#[tracing::instrument]
pub fn listen_for_commands(wm: Arc<Mutex<WindowManager>>) {
    std::thread::spawn(move || loop {
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
                            match stream.set_read_timeout(Some(Duration::from_secs(1))) {
                                Ok(()) => {}
                                Err(error) => tracing::error!("{}", error),
                            }
                            match read_commands_uds(&wm_clone, stream) {
                                Ok(()) => {}
                                Err(error) => tracing::error!("{}", error),
                            }
                        });
                    }
                    Err(error) => {
                        tracing::error!("{}", error);
                        break;
                    }
                }
            }
        })
        .join();

        tracing::error!("restarting failed thread");
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

        #[allow(clippy::useless_asref)]
        // We don't have From implemented for &mut WindowManager
        let initial_state = State::from(self.as_ref());

        match message {
            SocketMessage::CycleFocusEmptyWorkspace(_)
            | SocketMessage::CycleFocusWorkspace(_)
            | SocketMessage::FocusWorkspaceNumber(_) => {
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

        let mut force_update_borders = false;
        match message {
            SocketMessage::Promote => self.promote_container_to_front()?,
            SocketMessage::PromoteFocus => self.promote_focus_to_front()?,
            SocketMessage::PromoteWindow(direction) => {
                self.focus_container_in_direction(direction)?;
                self.promote_container_to_front()?
            }
            SocketMessage::EagerFocus(ref exe) => {
                let focused_monitor_idx = self.focused_monitor_idx();

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
                                monitor.set_last_focused_workspace(Option::from(focused_ws_idx));
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
                                self.focused_workspace_mut()?.maximized_window_mut()
                            {
                                window.focus(self.mouse_follows_focus)?;
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
                                window.focus(self.mouse_follows_focus)?;
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
                match focused_workspace.layer() {
                    WorkspaceLayer::Tiling => {
                        self.focus_container_in_direction(direction)?;
                    }
                    WorkspaceLayer::Floating => {
                        self.focus_floating_window_in_direction(direction)?;
                    }
                }
            }
            SocketMessage::MoveWindow(direction) => {
                let focused_workspace = self.focused_workspace()?;
                match focused_workspace.layer() {
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
                match focused_workspace.layer() {
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
            SocketMessage::StackWindow(direction) => self.add_window_to_container(direction)?,
            SocketMessage::UnstackWindow => self.remove_window_from_container()?,
            SocketMessage::StackAll => self.stack_all()?,
            SocketMessage::UnstackAll => self.unstack_all(true)?,
            SocketMessage::CycleStack(direction) => {
                self.cycle_container_window_in_direction(direction)?;
            }
            SocketMessage::CycleStackIndex(direction) => {
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
                self.focus_container_window(idx)?;
            }
            SocketMessage::ForceFocus => {
                let focused_window = self.focused_window()?;
                let focused_window_rect = WindowsApi::window_rect(focused_window.hwnd)?;
                WindowsApi::center_cursor_in_rect(&focused_window_rect)?;
                WindowsApi::left_click();
            }
            SocketMessage::Close => {
                Window::from(WindowsApi::foreground_window()?).close()?;
            }
            SocketMessage::Minimize => {
                Window::from(WindowsApi::foreground_window()?).minimize();
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
                    container.set_locked(true);
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
                    container.set_locked(false);
                }
            }
            SocketMessage::ToggleLock => self.toggle_lock()?,
            SocketMessage::ToggleFloat => self.toggle_float(false)?,
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
            SocketMessage::SessionFloatRule => {
                let foreground_window = WindowsApi::foreground_window()?;
                let window = Window::from(foreground_window);
                if let (Ok(exe), Ok(title), Ok(class)) =
                    (window.exe(), window.title(), window.class())
                {
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
                let rules = match serde_json::to_string_pretty(&*session_floating_applications) {
                    Ok(rules) => rules,
                    Err(error) => error.to_string(),
                };

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
                    if let MatchingRule::Simple(i) = i {
                        if i.id.eq(id) {
                            should_push = false;
                        }
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
            SocketMessage::MoveContainerToLastWorkspace => {
                // This is to ensure that even on an empty workspace on a secondary monitor, the
                // secondary monitor where the cursor is focused will be used as the target for
                // the workspace switch op
                if let Some(monitor_idx) = self.monitor_idx_from_current_pos() {
                    if monitor_idx != self.focused_monitor_idx() {
                        if let Some(monitor) = self.monitors().get(monitor_idx) {
                            if let Some(workspace) = monitor.focused_workspace() {
                                if workspace.is_empty() {
                                    self.focus_monitor(monitor_idx)?;
                                }
                            }
                        }
                    }
                }

                let idx = self
                    .focused_monitor()
                    .ok_or_else(|| anyhow!("there is no monitor"))?
                    .focused_workspace_idx();

                if let Some(monitor) = self.focused_monitor_mut() {
                    if let Some(last_focused_workspace) = monitor.last_focused_workspace() {
                        self.move_container_to_workspace(last_focused_workspace, true, None)?;
                    }
                }

                self.focused_monitor_mut()
                    .ok_or_else(|| anyhow!("there is no monitor"))?
                    .set_last_focused_workspace(Option::from(idx));
            }
            SocketMessage::SendContainerToLastWorkspace => {
                // This is to ensure that even on an empty workspace on a secondary monitor, the
                // secondary monitor where the cursor is focused will be used as the target for
                // the workspace switch op
                if let Some(monitor_idx) = self.monitor_idx_from_current_pos() {
                    if monitor_idx != self.focused_monitor_idx() {
                        if let Some(monitor) = self.monitors().get(monitor_idx) {
                            if let Some(workspace) = monitor.focused_workspace() {
                                if workspace.is_empty() {
                                    self.focus_monitor(monitor_idx)?;
                                }
                            }
                        }
                    }
                }

                let idx = self
                    .focused_monitor()
                    .ok_or_else(|| anyhow!("there is no monitor"))?
                    .focused_workspace_idx();

                if let Some(monitor) = self.focused_monitor_mut() {
                    if let Some(last_focused_workspace) = monitor.last_focused_workspace() {
                        self.move_container_to_workspace(last_focused_workspace, false, None)?;
                    }
                }
                self.focused_monitor_mut()
                    .ok_or_else(|| anyhow!("there is no monitor"))?
                    .set_last_focused_workspace(Option::from(idx));
            }
            SocketMessage::MoveContainerToWorkspaceNumber(workspace_idx) => {
                self.move_container_to_workspace(workspace_idx, true, None)?;
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
                        .ok_or_else(|| anyhow!("there must be at least one monitor"))?,
                );

                let direction = self.direction_from_monitor_idx(monitor_idx);
                self.move_container_to_monitor(monitor_idx, None, true, direction)?;
            }
            SocketMessage::SendContainerToWorkspaceNumber(workspace_idx) => {
                self.move_container_to_workspace(workspace_idx, false, None)?;
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
                        .ok_or_else(|| anyhow!("there must be at least one monitor"))?,
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
            SocketMessage::FocusMonitorAtCursor => {
                if let Some(monitor_idx) = self.monitor_idx_from_current_pos() {
                    self.focus_monitor(monitor_idx)?;
                }
            }
            SocketMessage::Retile => {
                border_manager::destroy_all_borders()?;
                force_update_borders = true;
                self.retile_all(false)?
            }
            SocketMessage::RetileWithResizeDimensions => {
                border_manager::destroy_all_borders()?;
                force_update_borders = true;
                self.retile_all(true)?
            }
            SocketMessage::FlipLayout(layout_flip) => self.flip_layout(layout_flip)?,
            SocketMessage::ScrollingLayoutColumns(count) => {
                let focused_workspace = self.focused_workspace_mut()?;

                let options = match focused_workspace.layout_options() {
                    Some(mut opts) => {
                        if let Some(scrolling) = &mut opts.scrolling {
                            scrolling.columns = count.into();
                        }

                        opts
                    }
                    None => LayoutOptions {
                        scrolling: Some(ScrollingLayoutOptions {
                            columns: count.into(),
                        }),
                    },
                };

                focused_workspace.set_layout_options(Some(options));
                self.update_focused_workspace(false, false)?;
            }
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
                    if monitor_idx != self.focused_monitor_idx() {
                        if let Some(monitor) = self.monitors().get(monitor_idx) {
                            if let Some(workspace) = monitor.focused_workspace() {
                                if workspace.is_empty() {
                                    self.focus_monitor(monitor_idx)?;
                                }
                            }
                        }
                    }
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
            SocketMessage::CycleFocusEmptyWorkspace(direction) => {
                // This is to ensure that even on an empty workspace on a secondary monitor, the
                // secondary monitor where the cursor is focused will be used as the target for
                // the workspace switch op
                if let Some(monitor_idx) = self.monitor_idx_from_current_pos() {
                    if monitor_idx != self.focused_monitor_idx() {
                        if let Some(monitor) = self.monitors().get(monitor_idx) {
                            if let Some(workspace) = monitor.focused_workspace() {
                                if workspace.is_empty() {
                                    self.focus_monitor(monitor_idx)?;
                                }
                            }
                        }
                    }
                }

                let focused_monitor = self
                    .focused_monitor()
                    .ok_or_else(|| anyhow!("there is no monitor"))?;

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
                            .ok_or_else(|| anyhow!("there must be at least one workspace"))?,
                    );

                    while !empty_workspaces.contains(&workspace_idx) {
                        workspace_idx = direction.next_idx(
                            workspace_idx,
                            NonZeroUsize::new(workspaces)
                                .ok_or_else(|| anyhow!("there must be at least one workspace"))?,
                        );
                    }

                    self.focus_workspace(workspace_idx)?;
                }
            }
            SocketMessage::CloseWorkspace => {
                // This is to ensure that even on an empty workspace on a secondary monitor, the
                // secondary monitor where the cursor is focused will be used as the target for
                // the workspace switch op
                if let Some(monitor_idx) = self.monitor_idx_from_current_pos() {
                    if monitor_idx != self.focused_monitor_idx() {
                        if let Some(monitor) = self.monitors().get(monitor_idx) {
                            if let Some(workspace) = monitor.focused_workspace() {
                                if workspace.is_empty() {
                                    self.focus_monitor(monitor_idx)?;
                                }
                            }
                        }
                    }
                }

                let mut can_close = false;

                if let Some(monitor) = self.focused_monitor_mut() {
                    let focused_workspace_idx = monitor.focused_workspace_idx();
                    let next_focused_workspace_idx = focused_workspace_idx.saturating_sub(1);

                    if let Some(workspace) = monitor.focused_workspace() {
                        if monitor.workspaces().len() > 1
                            && workspace.containers().is_empty()
                            && workspace.floating_windows().is_empty()
                            && workspace.monocle_container().is_none()
                            && workspace.maximized_window().is_none()
                            && workspace.name().is_none()
                        {
                            can_close = true;
                        }
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
            SocketMessage::FocusLastWorkspace => {
                // This is to ensure that even on an empty workspace on a secondary monitor, the
                // secondary monitor where the cursor is focused will be used as the target for
                // the workspace switch op
                if let Some(monitor_idx) = self.monitor_idx_from_current_pos() {
                    if monitor_idx != self.focused_monitor_idx() {
                        if let Some(monitor) = self.monitors().get(monitor_idx) {
                            if let Some(workspace) = monitor.focused_workspace() {
                                if workspace.is_empty() {
                                    self.focus_monitor(monitor_idx)?;
                                }
                            }
                        }
                    }
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
                    if monitor_idx != self.focused_monitor_idx() {
                        if let Some(monitor) = self.monitors().get(monitor_idx) {
                            if let Some(workspace) = monitor.focused_workspace() {
                                if workspace.is_empty() {
                                    self.focus_monitor(monitor_idx)?;
                                }
                            }
                        }
                    }
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
                    if monitor_idx != self.focused_monitor_idx() {
                        if let Some(monitor) = self.monitors().get(monitor_idx) {
                            if let Some(workspace) = monitor.focused_workspace() {
                                if workspace.is_empty() {
                                    self.focus_monitor(monitor_idx)?;
                                }
                            }
                        }
                    }
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
            SocketMessage::ToggleWorkspaceLayer => {
                let mouse_follows_focus = self.mouse_follows_focus;
                let workspace = self.focused_workspace_mut()?;

                let mut to_focus = None;
                match workspace.layer() {
                    WorkspaceLayer::Tiling => {
                        workspace.set_layer(WorkspaceLayer::Floating);

                        let focused_idx = workspace.focused_floating_window_idx();
                        let mut window_idx_pairs = workspace
                            .floating_windows_mut()
                            .make_contiguous()
                            .iter()
                            .enumerate()
                            .collect::<Vec<_>>();

                        // Sort by window area
                        window_idx_pairs.sort_by_key(|(_, w)| {
                            let rect = WindowsApi::window_rect(w.hwnd).unwrap_or_default();
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

                        if let Some(focused_window) = &to_focus {
                            // The focused window should be the last one raised to make sure it is
                            // on top
                            focused_window.restore();
                            focused_window.raise()?;
                        }

                        for container in workspace.containers() {
                            if let Some(window) = container.focused_window() {
                                window.lower()?;
                            }
                        }

                        if let Some(monocle) = workspace.monocle_container() {
                            if let Some(window) = monocle.focused_window() {
                                window.lower()?;
                            }
                        }
                    }
                    WorkspaceLayer::Floating => {
                        workspace.set_layer(WorkspaceLayer::Tiling);

                        if let Some(monocle) = workspace.monocle_container() {
                            if let Some(window) = monocle.focused_window() {
                                to_focus = Some(*window);
                                window.raise()?;
                            }
                            for window in workspace.floating_windows() {
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
                                .iter()
                                .collect::<Vec<_>>();

                            // Sort by window area
                            window_idx_pairs.sort_by_key(|w| {
                                let rect = WindowsApi::window_rect(w.hwnd).unwrap_or_default();
                                rect.right * rect.bottom
                            });

                            for window in window_idx_pairs {
                                window.lower()?;
                            }
                        }
                    }
                };
                if let Some(window) = to_focus {
                    window.focus(mouse_follows_focus)?;
                }
            }
            SocketMessage::Stop => {
                self.stop(false)?;
            }
            SocketMessage::StopIgnoreRestore => {
                self.stop(true)?;
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
                let mut display_index_preferences = DISPLAY_INDEX_PREFERENCES.write();
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
                        .ok_or_else(|| anyhow!("there is no monitor"))?
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
                        let focused_monitor = self
                            .focused_monitor()
                            .ok_or_else(|| anyhow!("there is no monitor"))?;

                        focused_monitor
                            .focused_workspace_name()
                            .unwrap_or_else(|| focused_monitor.focused_workspace_idx().to_string())
                    }
                    StateQuery::Version => build::RUST_VERSION.to_string(),
                    StateQuery::FocusedWorkspaceLayout => {
                        let focused_monitor = self
                            .focused_monitor()
                            .ok_or_else(|| anyhow!("there is no monitor"))?;

                        focused_monitor.focused_workspace_layout().map_or_else(
                            || "None".to_string(),
                            |layout| match layout {
                                Layout::Default(default_layout) => default_layout.to_string(),
                                Layout::Custom(_) => "Custom".to_string(),
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
                force_update_borders = true;
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
                        winevent_listener::event_rx(),
                        self.command_listener.try_clone().ok(),
                    )?;

                    // Initialize the new wm
                    wm.init()?;

                    wm.restore_all_windows(true)?;

                    // This is equivalent to StaticConfig::postload for this use case
                    StaticConfig::reload(config, &mut wm)?;

                    // Set self to the new wm instance
                    *self = wm;

                    // check if there are any bars
                    let mut system = sysinfo::System::new_all();
                    system.refresh_processes(sysinfo::ProcessesToUpdate::All, true);

                    let has_bar = system
                        .processes_by_name("komorebi-bar.exe".as_ref())
                        .next()
                        .is_some();

                    // stop bar(s)
                    if has_bar {
                        let script = r"
Stop-Process -Name:komorebi-bar -ErrorAction SilentlyContinue
                ";
                        match powershell_script::run(script) {
                            Ok(_) => {
                                println!("{script}");

                                // start new bar(s)
                                let mut config = StaticConfig::read(config)?;
                                if let Some(display_bar_configurations) =
                                    &mut config.bar_configurations
                                {
                                    for config_file_path in &mut *display_bar_configurations {
                                        let script = r#"Start-Process "komorebi-bar" '"--config" "CONFIGFILE"' -WindowStyle hidden"#
                                            .replace("CONFIGFILE", &config_file_path.to_string_lossy());

                                        match powershell_script::run(&script) {
                                            Ok(_) => {
                                                println!("{script}");
                                            }
                                            Err(error) => {
                                                println!("Error: {error}");
                                            }
                                        }
                                    }
                                } else {
                                    let script = r"
if (!(Get-Process komorebi-bar -ErrorAction SilentlyContinue))
{
  Start-Process komorebi-bar -WindowStyle hidden
}
                ";
                                    match powershell_script::run(script) {
                                        Ok(_) => {
                                            println!("{script}");
                                        }
                                        Err(error) => {
                                            println!("Error: {error}");
                                        }
                                    }
                                }
                            }
                            Err(error) => {
                                println!("Error: {error}");
                            }
                        }
                    }

                    force_update_borders = true;
                }
            }
            SocketMessage::ReloadStaticConfiguration(ref pathbuf) => {
                self.reload_static_configuration(pathbuf)?;
                force_update_borders = true;
            }
            SocketMessage::CompleteConfiguration => {
                if !INITIAL_CONFIGURATION_LOADED.load(Ordering::SeqCst) {
                    INITIAL_CONFIGURATION_LOADED.store(true, Ordering::SeqCst);
                    self.update_focused_workspace(false, false)?;
                    force_update_borders = true;
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
            SocketMessage::ToggleWindowBasedWorkAreaOffset => {
                let workspace = self.focused_workspace_mut()?;
                workspace.set_apply_window_based_work_area_offset(
                    !workspace.apply_window_based_work_area_offset(),
                );

                self.retile_all(true)?;
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
            SocketMessage::ToggleWorkspaceWindowContainerBehaviour => {
                let current_global_behaviour = self.window_management_behaviour.current_behaviour;
                if let Some(behaviour) = self
                    .focused_workspace_mut()?
                    .window_container_behaviour_mut()
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
                    self.focused_workspace_mut()?
                        .set_window_container_behaviour(Some(match current_global_behaviour {
                            WindowContainerBehaviour::Create => WindowContainerBehaviour::Append,
                            WindowContainerBehaviour::Append => WindowContainerBehaviour::Create,
                        }));
                };
            }
            SocketMessage::ToggleWorkspaceFloatOverride => {
                let current_global_override = self.window_management_behaviour.float_override;
                if let Some(float_override) = self.focused_workspace_mut()?.float_override_mut() {
                    *float_override = !*float_override;
                } else {
                    self.focused_workspace_mut()?
                        .set_float_override(Some(!current_global_override));
                };
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
                if !enable {
                    match IMPLEMENTATION.load() {
                        BorderImplementation::Komorebi => {
                            border_manager::destroy_all_borders()?;
                        }
                        BorderImplementation::Windows => {
                            self.remove_all_accents()?;
                        }
                    }
                } else if matches!(IMPLEMENTATION.load(), BorderImplementation::Komorebi) {
                    force_update_borders = true;
                }
            }
            SocketMessage::BorderImplementation(implementation) => {
                if !*WINDOWS_11 && matches!(implementation, BorderImplementation::Windows) {
                    tracing::error!(
                        "BorderImplementation::Windows is only supported on Windows 11 and above"
                    );
                } else {
                    IMPLEMENTATION.store(implementation);
                    match IMPLEMENTATION.load() {
                        BorderImplementation::Komorebi => {
                            self.remove_all_accents()?;
                            force_update_borders = true;
                        }
                        BorderImplementation::Windows => {
                            border_manager::destroy_all_borders()?;
                        }
                    }
                }
            }
            SocketMessage::BorderColour(kind, r, g, b) => {
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
                        border_manager::UNFOCUSED_LOCKED
                            .store(Rgb::new(r, g, b).into(), Ordering::SeqCst);
                    }
                    WindowKind::Floating => {
                        border_manager::FLOATING.store(Rgb::new(r, g, b).into(), Ordering::SeqCst);
                    }
                }
                force_update_borders = true;
            }
            SocketMessage::BorderStyle(style) => {
                STYLE.store(style);
                force_update_borders = true;
            }
            SocketMessage::BorderWidth(width) => {
                border_manager::BORDER_WIDTH.store(width, Ordering::SeqCst);
                force_update_borders = true;
            }
            SocketMessage::BorderOffset(offset) => {
                border_manager::BORDER_OFFSET.store(offset, Ordering::SeqCst);
                force_update_borders = true;
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
            SocketMessage::AnimationStyle(style, prefix) => match prefix {
                Some(prefix) => {
                    ANIMATION_STYLE_PER_ANIMATION.lock().insert(prefix, style);
                }
                None => {
                    let mut animation_style = ANIMATION_STYLE_GLOBAL.lock();
                    *animation_style = style;
                    ANIMATION_STYLE_PER_ANIMATION.lock().clear();
                }
            },
            SocketMessage::ToggleTransparency => {
                let current = transparency_manager::TRANSPARENCY_ENABLED.load(Ordering::SeqCst);
                transparency_manager::TRANSPARENCY_ENABLED.store(!current, Ordering::SeqCst);
            }
            SocketMessage::Transparency(enable) => {
                transparency_manager::TRANSPARENCY_ENABLED.store(enable, Ordering::SeqCst);
            }
            SocketMessage::TransparencyAlpha(alpha) => {
                transparency_manager::TRANSPARENCY_ALPHA.store(alpha, Ordering::SeqCst);
            }
            SocketMessage::StackbarMode(mode) => {
                STACKBAR_MODE.store(mode);
                self.retile_all(true)?;
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
            SocketMessage::StackbarFontSize(size) => {
                STACKBAR_FONT_SIZE.store(size, Ordering::SeqCst);
            }
            #[allow(clippy::assigning_clones)]
            SocketMessage::StackbarFontFamily(ref font_family) => {
                *STACKBAR_FONT_FAMILY.lock() = font_family.clone();
            }
            SocketMessage::ApplicationSpecificConfigurationSchema => {
                #[cfg(feature = "schemars")]
                {
                    let asc = schemars::schema_for!(
                        Vec<crate::core::config_generation::ApplicationConfiguration>
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
                    let settings = schemars::gen::SchemaSettings::default().with(|s| {
                        s.option_nullable = false;
                        s.option_add_null_type = false;
                        s.inline_subschemas = true;
                    });

                    let gen = settings.into_generator();
                    let socket_message = gen.into_root_schema_for::<StaticConfig>();
                    let schema = serde_json::to_string_pretty(&socket_message)?;

                    reply.write_all(schema.as_bytes())?;
                }
            }
            SocketMessage::GenerateStaticConfig => {
                let config = serde_json::to_string_pretty(&StaticConfig::from(&*self))?;

                reply.write_all(config.as_bytes())?;
            }
            SocketMessage::RemoveTitleBar(identifier, ref id) => {
                let mut identifiers = NO_TITLEBAR.lock();

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
            SocketMessage::ToggleTitleBars => {
                let current = REMOVE_TITLEBARS.load(Ordering::SeqCst);
                REMOVE_TITLEBARS.store(!current, Ordering::SeqCst);
                self.update_focused_workspace(false, false)?;
            }
            SocketMessage::DebugWindow(hwnd) => {
                let window = Window::from(hwnd);
                let mut rule_debug = RuleDebug::default();
                let _ = window.should_manage(None, &mut rule_debug);
                let schema = serde_json::to_string_pretty(&rule_debug)?;

                reply.write_all(schema.as_bytes())?;
            }
            SocketMessage::Theme(ref theme) => {
                theme_manager::send_notification(*theme.clone());
            }
            // Deprecated commands
            SocketMessage::AltFocusHack(_)
            | SocketMessage::IdentifyBorderOverflowApplication(_, _) => {}
        };

        // Update list of known_hwnds and their monitor/workspace index pair
        self.update_known_hwnds();

        notify_subscribers(
            Notification {
                event: NotificationEvent::Socket(message.clone()),
                state: self.as_ref().into(),
            },
            initial_state.has_been_modified(self.as_ref()),
        )?;

        if force_update_borders {
            border_manager::send_force_update();
        } else {
            border_manager::send_notification(None);
        }
        transparency_manager::send_notification();
        stackbar_manager::send_notification();

        tracing::info!("processed");
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

#[cfg(test)]
mod tests {
    use crate::monitor;
    use crate::window_manager::WindowManager;
    use crate::Rect;
    use crate::SocketMessage;
    use crate::WindowManagerEvent;
    use crossbeam_channel::bounded;
    use crossbeam_channel::Receiver;
    use crossbeam_channel::Sender;
    use std::io::BufRead;
    use std::io::BufReader;
    use std::io::Write;
    use std::path::PathBuf;
    use std::str::FromStr;
    use std::time::Duration;
    use uds_windows::UnixStream;
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
        let mut wm = WindowManager::new(receiver, Some(socket_path.clone())).unwrap();
        let m = monitor::new(
            0,
            Rect::default(),
            Rect::default(),
            "TestMonitor".to_string(),
            "TestDevice".to_string(),
            "TestDeviceID".to_string(),
            Some("TestMonitorID".to_string()),
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
