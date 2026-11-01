use crate::monitor::MonitorExt;
use crate::scrolling::WorkspaceScrolling;
use crate::workspace::WorkspaceExt;
use color_eyre::eyre;
use color_eyre::eyre::OptionExt;
use color_eyre::eyre::WrapErr;
use komorebi_themes::colour::Rgb;
use komorebi_transport::CommandTarget;
use komorebi_transport::ConnectionPolicy;
use komorebi_transport::allowed_while_paused;
use miow::pipe::connect;
use parking_lot::Mutex;
use std::collections::HashMap;
use std::fs::File;
use std::fs::OpenOptions;
use std::io::BufReader;
use std::io::Read;
use std::net::TcpListener;
use std::net::TcpStream;
use std::num::NonZeroUsize;
use std::str::FromStr;
use std::sync::Arc;
use std::sync::atomic::Ordering;
use std::time::Duration;
use uds_windows::UnixListener;
use uds_windows::UnixStream;

use crate::CUSTOM_FFM;
use crate::DATA_DIR;
use crate::FLOATING_APPLICATIONS;
use crate::HIDING_BEHAVIOUR;
use crate::IGNORE_IDENTIFIERS;
use crate::INITIAL_CONFIGURATION_LOADED;
use crate::LAYERED_WHITELIST;
use crate::MANAGE_IDENTIFIERS;
use crate::MONITOR_INDEX_PREFERENCES;
use crate::NO_TITLEBAR;
use crate::Notification;
use crate::NotificationEvent;
use crate::OBJECT_NAME_CHANGE_ON_LAUNCH;
use crate::REMOVE_TITLEBARS;
use crate::SESSION_FLOATING_APPLICATIONS;
use crate::SUBSCRIPTION_PIPES;
use crate::SUBSCRIPTION_SOCKET_OPTIONS;
use crate::SUBSCRIPTION_SOCKETS;
use crate::TCP_CONNECTIONS;
use crate::TRAY_AND_MULTI_WINDOW_IDENTIFIERS;
use crate::WINDOWS_11;
use crate::WORKSPACE_MATCHING_RULES;
use crate::border_manager;
use crate::border_manager::IMPLEMENTATION;
use crate::border_manager::STYLE;
use crate::build;
use crate::config_generation::WorkspaceMatchingRule;
use crate::core::ApplicationIdentifier;
use crate::core::Axis;
use crate::core::BorderImplementation;
use crate::core::CycleDirection;
use crate::core::FocusFollowsMouseImplementation;
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
use crate::current_virtual_desktop;
use crate::live_resize;
use crate::monitor::MonitorInformation;
use crate::notify_subscribers;
use crate::stackbar_manager;
use crate::stackbar_manager::STACKBAR_FONT_FAMILY;
use crate::stackbar_manager::STACKBAR_FONT_SIZE;
use crate::state::GlobalState;
use crate::state::State;
use crate::static_config::StaticConfig;
use crate::static_config::StaticConfigExt;
use crate::theme_manager;
use crate::transparency_manager;
use crate::window::RuleDebug;
use crate::window::Window;
use crate::window_manager::WindowManager;
use crate::windows_api::WindowsApi;
use crate::windows_backend::backend;
use crate::winevent_listener;
use crate::workspace::WorkspaceLayer;
use crate::workspace::WorkspaceWindowLocation;
use komorebi_backend::WindowId;
use komorebi_core::ContainerWindow;
use stackbar_manager::STACKBAR_FOCUSED_TEXT_COLOUR;
use stackbar_manager::STACKBAR_LABEL;
use stackbar_manager::STACKBAR_MODE;
use stackbar_manager::STACKBAR_TAB_BACKGROUND_COLOUR;
use stackbar_manager::STACKBAR_TAB_HEIGHT;
use stackbar_manager::STACKBAR_TAB_WIDTH;
use stackbar_manager::STACKBAR_UNFOCUSED_TEXT_COLOUR;

#[tracing::instrument]
pub fn listen_for_commands(wm: Arc<Mutex<WindowManager>>) {
    komorebi_transport::listen_for_commands::<WindowManager, _>(wm, WindowsConnectionPolicy);
}

/// What the Windows daemon does to a connection that the others do not: a client that stalls is
/// given up on after a second, except while the fingers of a pan gesture are down
#[derive(Clone, Copy, Debug)]
struct WindowsConnectionPolicy;

impl ConnectionPolicy<UnixStream> for WindowsConnectionPolicy {
    fn on_accept(&self, stream: &UnixStream) {
        // when connections come in: a request that waits in the backlog while the process is
        // stalled is seen here late, next to the heartbeat
        {
            static LAST_ACCEPT: parking_lot::Mutex<Option<std::time::Instant>> =
                parking_lot::Mutex::new(None);
            let now = std::time::Instant::now();
            let since = LAST_ACCEPT
                .lock()
                .replace(now)
                .map(|last| now.duration_since(last));
            tracing::debug!(
                "accepted a command connection ({} ms since the previous one)",
                since.map_or(-1, |d| d.as_millis() as i64)
            );
        }

        match stream.set_read_timeout(Some(Duration::from_secs(1))) {
            Ok(()) => {}
            Err(error) => tracing::error!("{}", error),
        }
    }

    fn lock_holder(&self) -> Option<String> {
        crate::lock_holder::describe()
    }

    fn on_message(&self, message: &SocketMessage, stream: &UnixStream) {
        // the fingers of a gesture can be still for longer than a command is waited for, and
        // what they do next is told on this connection: it is closed when they have lifted
        if matches!(
            message,
            SocketMessage::PanGesture(crate::core::PanGesture::Begin)
        ) {
            let _ = stream.set_read_timeout(Some(Duration::from_secs(30)));
        }

        // looked for here, before the window manager is locked for the message: a process scan
        // under the lock held every other thread for seconds (see `replace_configuration`)
        // and kept for this connection thread only, which handles the message next: every message
        // read replaces it, so a message that was dropped (paused, lock not acquired) leaves
        // nothing for a later one, and other connections have their own
        BAR_RUNNING.set(
            matches!(message, SocketMessage::ReplaceConfiguration(_)).then(komorebi_bar_is_running),
        );
    }
}

thread_local! {
    /// Whether komorebi-bar.exe was running when this thread read a `ReplaceConfiguration`, for the
    /// handler of that message to take: `None` when the last message read was another one
    static BAR_RUNNING: std::cell::Cell<Option<bool>> = const { std::cell::Cell::new(None) };
}

/// The thread that restarts the komorebi-bar processes after a `ReplaceConfiguration`
static BAR_RESTART: Mutex<Option<std::thread::JoinHandle<()>>> = Mutex::new(None);

/// Waits for the restart of the bars that a `ReplaceConfiguration` started, if one is running:
/// the daemon calls it before it exits, so that a bar is not started after `komorebic stop` has
/// stopped the bars
pub fn finish_bar_restart() {
    let handle = BAR_RESTART.lock().take();
    if let Some(handle) = handle {
        let _ = handle.join();
    }
}

/// Stops the running komorebi-bar processes and starts them again from the configuration. Runs on
/// a thread of its own: PowerShell takes seconds, which must not be spent with the window manager
/// locked.
fn restart_bars(bar_configurations: Option<Vec<std::path::PathBuf>>) {
    let script = r"
Stop-Process -Name:komorebi-bar -ErrorAction SilentlyContinue
    ";
    match powershell_script::run(script) {
        Ok(_) => {
            println!("{script}");

            // start new bar(s)
            if let Some(display_bar_configurations) = bar_configurations {
                for config_file_path in &display_bar_configurations {
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

/// Whether a komorebi-bar.exe process is running. Only the names of the processes are read:
/// `System::new_all` also opens the performance counters and reads the command line, the
/// environment and the owner of every process, which is the slow part.
fn komorebi_bar_is_running() -> bool {
    let mut system = sysinfo::System::new();
    system.refresh_processes_specifics(
        sysinfo::ProcessesToUpdate::All,
        true,
        sysinfo::ProcessRefreshKind::nothing(),
    );

    system
        .processes_by_name("komorebi-bar.exe".as_ref())
        .next()
        .is_some()
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

#[tracing::instrument]
pub fn listen_for_commands_tcp(wm: Arc<Mutex<WindowManager>>, port: usize) {
    let listener =
        TcpListener::bind(format!("0.0.0.0:{port}")).expect("could not start tcp server");

    std::thread::spawn(move || {
        tracing::info!("listening on 0.0.0.0:43663");
        for client in listener.incoming() {
            match client {
                Ok(mut stream) => {
                    net2::TcpStreamExt::set_keepalive(&stream, Some(Duration::from_secs(30)))
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
    ) -> eyre::Result<()> {
        let _held = crate::lock_holder::held(format!("command {message}"));
        let command_started = std::time::Instant::now();
        // when this command is over, an event answer computed before it is not used (see
        // `COMMAND_GENERATION`); the bump comes after the command has changed what it changes
        struct BumpGeneration;
        impl Drop for BumpGeneration {
            fn drop(&mut self) {
                crate::COMMAND_GENERATION.fetch_add(1, Ordering::SeqCst);
            }
        }
        let _bump_generation = BumpGeneration;
        if let Some(virtual_desktop_id) = &self.virtual_desktop_id
            && let Some(id) = current_virtual_desktop()
            && id != *virtual_desktop_id
        {
            tracing::info!(
                "ignoring events and commands while not on virtual desktop {:?}",
                virtual_desktop_id
            );
            return Ok(());
        }

        #[allow(clippy::useless_asref)]
        // We don't have From implemented for &mut WindowManager
        let initial_state = State::from(self.as_ref());

        let mut force_update_borders = false;
        match message {
            SocketMessage::Promote => self.promote_container_to_front()?,
            SocketMessage::PromoteSwap => self.promote_container_swap()?,
            SocketMessage::PromoteFocus => self.promote_focus_to_front()?,
            SocketMessage::PromoteWindow(direction) => {
                komorebi_core::promote_window(self, direction)?
            }
            SocketMessage::EagerFocus(ref exe) => {
                self.eager_focus(exe)?;
            }
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
                let focused_window_rect =
                    backend().frame(WindowId::from_hwnd(focused_window.hwnd))?;
                WindowsApi::center_cursor_in_rect(&focused_window_rect)?;
                WindowsApi::left_click();
            }
            SocketMessage::Close => {
                backend().close(WindowId::from_hwnd(WindowsApi::foreground_window()?))?;
            }
            SocketMessage::Minimize => {
                let _ = backend().minimize(WindowId::from_hwnd(WindowsApi::foreground_window()?));
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

                push_simple_rule(&mut manage_identifiers, identifier, id);
            }
            SocketMessage::SessionFloatRule => {
                self.add_session_float_rule()?;
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
                self.add_ignore_rule(identifier, id)?;
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
            SocketMessage::AdjustContainerPadding(sizing, adjustment) => {
                self.adjust_container_padding(sizing, adjustment)?;
            }
            SocketMessage::AdjustWorkspacePadding(sizing, adjustment) => {
                self.adjust_workspace_padding(sizing, adjustment)?;
            }
            SocketMessage::MoveContainerToLastWorkspace => {
                self.move_container_to_last_workspace(true)?;
            }
            SocketMessage::SendContainerToLastWorkspace => {
                self.move_container_to_last_workspace(false)?;
            }
            SocketMessage::MoveContainerToWorkspaceNumber(workspace_idx) => {
                self.move_container_to_workspace(workspace_idx, true, None)?;
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
            SocketMessage::SendContainerToWorkspaceNumber(workspace_idx) => {
                self.move_container_to_workspace(workspace_idx, false, None)?;
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
                komorebi_core::cycle_focus_monitor(self, direction)?
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
            SocketMessage::Pan(direction) => self.pan(direction)?,
            SocketMessage::PanGesture(gesture) => self.pan_gesture(gesture)?,
            SocketMessage::ChangeLayout(layout) => self.change_workspace_layout_default(layout)?,
            SocketMessage::CycleLayout(direction) => self.cycle_layout(direction)?,
            SocketMessage::LayoutRatios(ref columns, ref rows) => {
                self.set_layout_ratios(columns, rows)?;
            }
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
                self.cycle_focus_workspace(direction)?;
            }
            SocketMessage::CycleFocusEmptyWorkspace(direction) => {
                self.cycle_focus_empty_workspace(direction)?;
            }
            SocketMessage::CloseWorkspace => {
                self.close_workspace()?;
            }
            SocketMessage::FocusLastWorkspace => {
                self.focus_last_workspace()?;
            }
            SocketMessage::FocusWorkspaceNumber(workspace_idx) => {
                komorebi_core::focus_workspace_number(self, workspace_idx)?
            }
            SocketMessage::FocusWorkspaceNumbers(workspace_idx) => {
                self.focus_workspace_numbers(workspace_idx)?;
            }
            SocketMessage::FocusMonitorWorkspaceNumber(monitor_idx, workspace_idx) => {
                komorebi_core::focus_monitor_workspace_number(self, monitor_idx, workspace_idx)?
            }
            SocketMessage::FocusNamedWorkspace(ref name) => {
                komorebi_core::focus_named_workspace(self, name)?
            }
            SocketMessage::ToggleWorkspaceLayer => {
                self.toggle_workspace_layer()?;
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
                crate::settings::settings().update(|settings| {
                    settings
                        .shared
                        .display_index_preferences
                        .insert(index_preference, display.clone());
                });
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
                        monitor_visible_windows.insert(
                            monitor.extra.device_id.clone(),
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
                self.answer_query(query, &mut reply)?;
            }
            SocketMessage::ResizeWindowEdge(direction, sizing) => {
                let _in_place = live_resize::InPlace::new();
                self.resize_window(direction, sizing, self.resize_delta, true)?;
            }
            SocketMessage::ResizeWindowAxis(axis, sizing) => {
                self.resize_window_axis(axis, sizing)?;
            }
            SocketMessage::FocusFollowsMouse(implementation, enable) => {
                self.focus_follows_mouse(implementation, enable)?;
            }
            SocketMessage::ToggleFocusFollowsMouse(implementation) => {
                self.toggle_focus_follows_mouse(implementation)?;
            }
            SocketMessage::ReloadConfiguration => {
                Self::reload_configuration();
                force_update_borders = true;
            }
            SocketMessage::ReplaceConfiguration(ref config) => {
                force_update_borders = self.replace_configuration(config)?;
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

                push_simple_rule(&mut identifiers, identifier, id);
            }
            SocketMessage::IdentifyTrayApplication(identifier, ref id) => {
                let mut identifiers = TRAY_AND_MULTI_WINDOW_IDENTIFIERS.lock();
                push_simple_rule(&mut identifiers, identifier, id);
            }
            SocketMessage::IdentifyLayeredApplication(identifier, ref id) => {
                let mut identifiers = LAYERED_WHITELIST.lock();

                push_simple_rule(&mut identifiers, identifier, id);
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
            SocketMessage::ToggleWindowBasedWorkAreaOffset => {
                let workspace = self.focused_workspace_mut()?;
                workspace.apply_window_based_work_area_offset =
                    !workspace.apply_window_based_work_area_offset;

                self.retile_all(true)?;
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
                let pipe = connect(&pipe_path).wrap_err(
                    format!("the named pipe '{}' has not yet been created; please create it before running this command", pipe_path)
                )?;

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
                komorebi_core::toggle_window_container_behaviour(self)
            }
            SocketMessage::ToggleFloatOverride => {
                self.window_management_behaviour.float_override =
                    !self.window_management_behaviour.float_override;
            }
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
            SocketMessage::WindowHidingBehaviour(behaviour) => {
                let mut hiding_behaviour = HIDING_BEHAVIOUR.lock();
                *hiding_behaviour = behaviour;
            }
            SocketMessage::ToggleCrossMonitorMoveBehaviour => {
                komorebi_core::toggle_cross_monitor_move_behaviour(self)
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
                self.set_border_colour(kind, r, g, b);
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
            SocketMessage::ToggleTransparency => {
                crate::settings::settings().update(|settings| {
                    settings.shared.transparency.enabled = !settings.shared.transparency.enabled;
                });
            }
            SocketMessage::Transparency(enable) => {
                crate::settings::settings()
                    .update(|settings| settings.shared.transparency.enabled = enable);
            }
            SocketMessage::TransparencyAlpha(alpha) => {
                crate::settings::settings()
                    .update(|settings| settings.shared.transparency.alpha = alpha);
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
                    let socket_message = schemars::schema_for!(SocketMessage);
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

                push_simple_rule(&mut identifiers, identifier, id);
            }
            SocketMessage::ToggleTitleBars => {
                let current = REMOVE_TITLEBARS.load(Ordering::SeqCst);
                REMOVE_TITLEBARS.store(!current, Ordering::SeqCst);
                self.update_focused_workspace(false, false)?;
            }
            SocketMessage::DebugWindow(hwnd) => {
                let window = Window::from(hwnd as isize);
                let mut rule_debug = RuleDebug::default();
                let _ = window.should_manage(None, &mut rule_debug);
                let schema = serde_json::to_string_pretty(&rule_debug)?;

                reply.write_all(schema.as_bytes())?;
            }
            SocketMessage::Theme(ref theme) => {
                theme_manager::send_notification(*theme.clone());
            }
            SocketMessage::ApplyState(ref blob) => {
                let state: State = serde_json::from_value(blob.0.clone())?;
                self.apply_state(state);
            }
            // Messages of other platforms: answered, and nothing else happens
            SocketMessage::ReloadHotkeys | SocketMessage::HotkeysState => {
                reply.write_all(message.not_supported_reply().as_bytes())?;
                return Ok(());
            }
            // Deprecated commands
            SocketMessage::AltFocusHack(_)
            | SocketMessage::IdentifyBorderOverflowApplication(_, _) => {}
        };

        // Update list of known_hwnds and their monitor/workspace index pair
        let after_command = std::time::Instant::now();
        self.update_known_hwnds();
        let after_hwnds = std::time::Instant::now();

        notify_subscribers(
            Notification {
                event: NotificationEvent::Socket(message.clone()),
                state: self.as_ref().into(),
            },
            initial_state.has_been_modified(self.as_ref()),
        )?;
        tracing::debug!(
            "processed `{message}`: command {} ms, known hwnds {} ms, notify subscribers {} ms",
            after_command.duration_since(command_started).as_millis(),
            after_hwnds.duration_since(after_command).as_millis(),
            after_hwnds.elapsed().as_millis()
        );

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

pub fn read_commands_uds(wm: &Arc<Mutex<WindowManager>>, stream: UnixStream) -> eyre::Result<()> {
    komorebi_transport::read_commands::<WindowManager, _>(wm, stream, &WindowsConnectionPolicy)
}

pub fn read_commands_tcp(
    wm: &Arc<Mutex<WindowManager>>,
    stream: &mut TcpStream,
    addr: &str,
) -> eyre::Result<()> {
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
                    return if allowed_while_paused(&message) {
                        Ok(wm.process_command(message, stream)?)
                    } else {
                        tracing::trace!("ignoring while paused");
                        Ok(())
                    };
                }

                wm.process_command(message.clone(), &mut *stream)?;
            }
        }
    }

    Ok(())
}

/// Add a simple socket-created rule unless one with the same kind and id is already present;
/// a rule of another kind with the same id is a different rule
fn push_simple_rule(rules: &mut Vec<MatchingRule>, kind: ApplicationIdentifier, id: &str) {
    let exists = rules
        .iter()
        .any(|r| matches!(r, MatchingRule::Simple(r) if r.kind == kind && r.id.eq(id)));

    if !exists {
        rules.push(MatchingRule::Simple(IdWithIdentifier {
            kind,
            id: id.to_string(),
            matching_strategy: Option::from(MatchingStrategy::Legacy),
        }));
    }
}

// extracted-arms-impl
impl WindowManager {
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

    pub fn replace_configuration(&mut self, config: &std::path::PathBuf) -> eyre::Result<bool> {
        let mut force_update_borders = false;
        let mut last = std::time::Instant::now();
        let mut lap = |step: &str| {
            let now = std::time::Instant::now();
            tracing::info!(
                "replace-configuration: {step} took {} ms",
                now.duration_since(last).as_millis()
            );
            last = now;
        };
        // Check that this is a valid static config file first
        if StaticConfig::read(config).is_ok() {
            // Clear workspace rules; these will need to be replaced
            WORKSPACE_MATCHING_RULES.lock().clear();
            // Pause so that restored windows come to the foreground from all workspaces
            self.is_paused = true;
            // Bring all windows to the foreground
            self.restore_all_windows(false)?;
            lap("restore all windows");

            // Create a new wm from the config path
            let mut wm = StaticConfig::preload(
                config,
                winevent_listener::event_rx(),
                self.command_listener.try_clone().ok(),
            )?;
            lap("preload");

            // Initialize the new wm
            wm.init()?;
            lap("init");

            wm.restore_all_windows(true)?;
            lap("restore all windows of the new wm");

            // This is equivalent to StaticConfig::postload for this use case
            StaticConfig::reload(config, &mut wm)?;
            lap("reload");

            // Set self to the new wm instance
            *self = wm;
            lap("swap");

            // check if there are any bars
            let has_bar = BAR_RUNNING.take().unwrap_or_else(komorebi_bar_is_running);
            lap("process scan for komorebi-bar");

            // stop bar(s)
            if has_bar {
                // an error reading the bar section is returned, as it was; only the PowerShell
                // process work (seconds) is not done with the window manager locked
                let bar_configurations = StaticConfig::read(config)?.bar_configurations;
                finish_bar_restart();
                *BAR_RESTART.lock() = Some(std::thread::spawn(move || {
                    restart_bars(bar_configurations);
                }));
            }

            lap("bar restart");
            force_update_borders = true;
        }
        Ok(force_update_borders)
    }

    pub fn answer_query(
        &mut self,
        query: StateQuery,
        mut reply: impl std::io::Write,
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
            StateQuery::Version => build::RUST_VERSION.to_string(),
            StateQuery::FocusedWorkspaceLayout => {
                let focused_monitor = self.focused_monitor().ok_or_eyre("there is no monitor")?;

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
        Ok(())
    }
    pub fn focus_follows_mouse(
        &mut self,
        mut implementation: FocusFollowsMouseImplementation,
        enable: bool,
    ) -> eyre::Result<()> {
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
        Ok(())
    }

    pub fn toggle_focus_follows_mouse(
        &mut self,
        mut implementation: FocusFollowsMouseImplementation,
    ) -> eyre::Result<()> {
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
                            tracing::warn!(
                                "ignoring command that could mix different focus follows mouse implementations"
                            );
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
                            tracing::warn!(
                                "ignoring command that could mix different focus follows mouse implementations"
                            );
                        }
                    }
                }
            }
        }
        Ok(())
    }

    pub fn resize_window_axis(&mut self, axis: Axis, sizing: Sizing) -> eyre::Result<()> {
        let _in_place = live_resize::InPlace::new();

        // If the user has a custom layout, allow for the resizing of the primary column
        // with this signal
        let workspace = self.focused_workspace_mut()?;
        let container_len = workspace.containers().len();
        let no_layout_rules = workspace.layout_rules.is_empty();

        if let Layout::Custom(custom) = &mut workspace.layout {
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
                    for rule in &mut workspace.layout_rules {
                        if container_len >= rule.0
                            && let Layout::Custom(ref mut custom) = rule.1
                        {
                            match sizing {
                                Sizing::Increase => {
                                    custom.set_primary_width_percentage(percentage + 5.0);
                                }
                                Sizing::Decrease => {
                                    custom.set_primary_width_percentage(percentage - 5.0);
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
                    self.resize_window(OperationDirection::Left, sizing, self.resize_delta, false)?;
                    self.resize_window(
                        OperationDirection::Right,
                        sizing,
                        self.resize_delta,
                        false,
                    )?;
                }
                Axis::Vertical => {
                    self.resize_window(OperationDirection::Up, sizing, self.resize_delta, false)?;
                    self.resize_window(OperationDirection::Down, sizing, self.resize_delta, false)?;
                }
                Axis::HorizontalAndVertical => {
                    self.resize_window(OperationDirection::Left, sizing, self.resize_delta, false)?;
                    self.resize_window(
                        OperationDirection::Right,
                        sizing,
                        self.resize_delta,
                        false,
                    )?;
                    self.resize_window(OperationDirection::Up, sizing, self.resize_delta, false)?;
                    self.resize_window(OperationDirection::Down, sizing, self.resize_delta, false)?;
                }
            }
        }

        self.update_focused_workspace(false, false)?;
        Ok(())
    }

    pub fn eager_focus(&mut self, exe: &str) -> eyre::Result<()> {
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
        Ok(())
    }
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

    pub fn add_session_float_rule(&mut self) -> eyre::Result<()> {
        let foreground_window = WindowsApi::foreground_window()?;
        let window = Window::from(foreground_window);
        if let (Some(exe), Some(title), Some(class)) =
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
        Ok(())
    }

    pub fn add_ignore_rule(
        &mut self,
        identifier: ApplicationIdentifier,
        id: &str,
    ) -> eyre::Result<()> {
        let mut ignore_identifiers = IGNORE_IDENTIFIERS.lock();

        push_simple_rule(&mut ignore_identifiers, identifier, id);

        let offset = self.work_area_offset;

        let mut hwnds_to_purge = vec![];
        for (i, monitor) in self.monitors().iter().enumerate() {
            for container in monitor
                .focused_workspace()
                .ok_or_eyre("there is no workspace")?
                .containers()
            {
                for window in container.windows() {
                    match identifier {
                        ApplicationIdentifier::Path => {
                            if window.path().ok_or_eyre("the window has no path")? == *id {
                                hwnds_to_purge.push((i, window.hwnd));
                            }
                        }
                        ApplicationIdentifier::Exe => {
                            if window.exe().ok_or_eyre("the window has no exe")? == *id {
                                hwnds_to_purge.push((i, window.hwnd));
                            }
                        }
                        ApplicationIdentifier::Class => {
                            if window.class().ok_or_eyre("the window has no class")? == *id {
                                hwnds_to_purge.push((i, window.hwnd));
                            }
                        }
                        ApplicationIdentifier::Title => {
                            if window.title().ok_or_eyre("the window has no title")? == *id {
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
                .ok_or_eyre("there is no monitor")?;

            monitor
                .focused_workspace_mut()
                .ok_or_eyre("there is no focused workspace")?
                .remove_window(hwnd)?;

            monitor.update_focused_workspace(offset)?;
        }
        Ok(())
    }
    pub fn move_container_to_last_workspace(&mut self, follow: bool) -> eyre::Result<()> {
        komorebi_core::move_container_to_last_workspace(self, follow)
    }

    pub fn focus_workspace_numbers(&mut self, workspace_idx: usize) -> eyre::Result<()> {
        komorebi_core::focus_workspace_numbers(self, workspace_idx)
    }

    pub fn focus_last_workspace(&mut self) -> eyre::Result<()> {
        komorebi_core::focus_last_workspace(self)
    }

    pub fn close_workspace(&mut self) -> eyre::Result<()> {
        komorebi_core::close_workspace(self)
    }

    pub fn cycle_focus_empty_workspace(&mut self, direction: CycleDirection) -> eyre::Result<()> {
        komorebi_core::cycle_focus_empty_workspace(self, direction)
    }

    pub fn cycle_focus_workspace(&mut self, direction: CycleDirection) -> eyre::Result<()> {
        komorebi_core::cycle_focus_workspace(self, direction)
    }

    pub fn toggle_workspace_layer(&mut self) -> eyre::Result<()> {
        let mouse_follows_focus = self.mouse_follows_focus;
        let workspace = self.focused_workspace_mut()?;

        let mut to_focus = None;
        match workspace.layer {
            WorkspaceLayer::Tiling => {
                workspace.layer = WorkspaceLayer::Floating;

                let focused_idx = workspace.focused_floating_window_idx();
                let mut window_idx_pairs = workspace
                    .floating_windows_mut()
                    .make_contiguous()
                    .iter()
                    .enumerate()
                    .collect::<Vec<_>>();

                // Sort by window area
                window_idx_pairs.sort_by_key(|(_, w)| {
                    let rect = backend()
                        .frame(WindowId::from_hwnd(w.hwnd))
                        .unwrap_or_default();
                    rect.right * rect.bottom
                });
                window_idx_pairs.reverse();

                for (i, window) in window_idx_pairs {
                    if i == focused_idx {
                        to_focus = Some(*window);
                    } else {
                        let _ = window.restore_window();
                        backend().raise(WindowId::from_hwnd(window.hwnd))?;
                    }
                }

                if let Some(focused_window) = &to_focus {
                    // The focused window should be the last one raised to make sure it is
                    // on top
                    let _ = focused_window.restore_window();
                    backend().raise(WindowId::from_hwnd(focused_window.hwnd))?;
                }

                for container in workspace.containers() {
                    if let Some(window) = container.focused_window() {
                        window.lower()?;
                    }
                }

                if let Some(monocle) = &workspace.monocle_container
                    && let Some(window) = monocle.focused_window()
                {
                    window.lower()?;
                }
            }
            WorkspaceLayer::Floating => {
                workspace.layer = WorkspaceLayer::Tiling;

                if let Some(monocle) = &workspace.monocle_container {
                    if let Some(window) = monocle.focused_window() {
                        to_focus = Some(*window);
                        backend().raise(WindowId::from_hwnd(window.hwnd))?;
                    }
                    for window in workspace.floating_windows() {
                        let _ = window.hide_window(());
                    }
                } else {
                    let focused_container_idx = workspace.focused_container_idx();
                    for (i, container) in workspace.containers_mut().iter_mut().enumerate() {
                        if let Some(window) = container.focused_window() {
                            if i == focused_container_idx {
                                to_focus = Some(*window);
                            }
                            backend().raise(WindowId::from_hwnd(window.hwnd))?;
                        }
                    }

                    let mut window_idx_pairs = workspace
                        .floating_windows_mut()
                        .make_contiguous()
                        .iter()
                        .collect::<Vec<_>>();

                    // Sort by window area
                    window_idx_pairs.sort_by_key(|w| {
                        let rect = backend()
                            .frame(WindowId::from_hwnd(w.hwnd))
                            .unwrap_or_default();
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
        Ok(())
    }
}

/// The window manager as the shared commands of `komorebi_core::command` see it
impl komorebi_core::CommandHost for WindowManager {
    type P = crate::workspace::WindowsPlatform;

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
    use super::push_simple_rule;
    use crate::Rect;
    use crate::SocketMessage;
    use crate::WindowManagerEvent;
    use crate::core::ApplicationIdentifier;
    use crate::core::config_generation::IdWithIdentifier;
    use crate::core::config_generation::MatchingRule;
    use crate::monitor;
    use crate::window_manager::WindowManager;
    use crossbeam_channel::Receiver;
    use crossbeam_channel::Sender;
    use crossbeam_channel::bounded;
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

    #[test]
    fn push_simple_rule_keeps_rules_of_another_kind_with_the_same_id() {
        let mut rules = vec![];

        push_simple_rule(&mut rules, ApplicationIdentifier::Class, "foo");
        push_simple_rule(&mut rules, ApplicationIdentifier::Exe, "foo");
        assert_eq!(rules.len(), 2);

        push_simple_rule(&mut rules, ApplicationIdentifier::Exe, "foo");
        push_simple_rule(&mut rules, ApplicationIdentifier::Class, "foo");
        assert_eq!(rules.len(), 2);

        rules.clear();
        rules.push(MatchingRule::Composite(vec![IdWithIdentifier {
            kind: ApplicationIdentifier::Exe,
            id: "foo".to_string(),
            matching_strategy: None,
        }]));
        push_simple_rule(&mut rules, ApplicationIdentifier::Exe, "foo");
        assert_eq!(rules.len(), 2);
    }
}
