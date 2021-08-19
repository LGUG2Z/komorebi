use std::io::BufRead;
use std::io::BufReader;
use std::io::Write;
use std::str::FromStr;
use std::sync::Arc;
use std::thread;

use color_eyre::eyre::ContextCompat;
use color_eyre::Result;
use parking_lot::Mutex;
use uds_windows::UnixStream;

use komorebi_core::ApplicationIdentifier;
use komorebi_core::SocketMessage;

use crate::window_manager;
use crate::window_manager::WindowManager;
use crate::windows_api::WindowsApi;
use crate::FLOAT_CLASSES;
use crate::FLOAT_EXES;
use crate::FLOAT_TITLES;
use crate::MANAGE_IDENTIFIERS;
use crate::TRAY_AND_MULTI_WINDOW_CLASSES;
use crate::TRAY_AND_MULTI_WINDOW_EXES;
use crate::WORKSPACE_RULES;

#[tracing::instrument]
pub fn listen_for_commands(wm: Arc<Mutex<WindowManager>>) {
    let listener = wm
        .lock()
        .command_listener
        .try_clone()
        .expect("could not clone unix listener");

    thread::spawn(move || {
        tracing::info!("listening");
        for client in listener.incoming() {
            match client {
                Ok(stream) => match wm.lock().read_commands(stream) {
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

impl WindowManager {
    #[tracing::instrument(skip(self))]
    pub fn process_command(&mut self, message: SocketMessage) -> Result<()> {
        let virtual_desktop_id = winvd::helpers::get_current_desktop_number().ok();
        if let (Some(id), Some(virtual_desktop_id)) = (virtual_desktop_id, self.virtual_desktop_id)
        {
            if id != virtual_desktop_id {
                tracing::warn!(
                    "ignoring events while not on virtual desktop {:?}",
                    virtual_desktop_id
                );

                return Ok(());
            }
        }

        match message {
            SocketMessage::Promote => self.promote_container_to_front()?,
            SocketMessage::FocusWindow(direction) => {
                self.focus_container_in_direction(direction)?;
            }
            SocketMessage::MoveWindow(direction) => {
                self.move_container_in_direction(direction)?;
            }
            SocketMessage::StackWindow(direction) => self.add_window_to_container(direction)?,
            SocketMessage::UnstackWindow => self.remove_window_from_container()?,
            SocketMessage::CycleStack(direction) => {
                self.cycle_container_window_in_direction(direction)?;
            }
            SocketMessage::ToggleFloat => self.toggle_float()?,
            SocketMessage::ToggleMonocle => self.toggle_monocle()?,
            SocketMessage::ToggleMaximize => self.toggle_maximize()?,
            SocketMessage::ContainerPadding(monitor_idx, workspace_idx, size) => {
                self.set_container_padding(monitor_idx, workspace_idx, size)?;
            }
            SocketMessage::WorkspacePadding(monitor_idx, workspace_idx, size) => {
                self.set_workspace_padding(monitor_idx, workspace_idx, size)?;
            }
            SocketMessage::FloatClass(target) => {
                let mut float_classes = FLOAT_CLASSES.lock();
                if !float_classes.contains(&target) {
                    float_classes.push(target);
                }
            }
            SocketMessage::FloatExe(target) => {
                let mut float_exes = FLOAT_EXES.lock();
                if !float_exes.contains(&target) {
                    float_exes.push(target);
                }
            }
            SocketMessage::FloatTitle(target) => {
                let mut float_titles = FLOAT_TITLES.lock();
                if !float_titles.contains(&target) {
                    float_titles.push(target);
                }
            }
            SocketMessage::WorkspaceRule(identifier, id, monitor_idx, workspace_idx) => {
                match identifier {
                    ApplicationIdentifier::Exe | ApplicationIdentifier::Class => {
                        {
                            let mut workspace_rules = WORKSPACE_RULES.lock();
                            workspace_rules.insert(id, (monitor_idx, workspace_idx));
                        }

                        self.enforce_workspace_rules()?;
                    }
                    ApplicationIdentifier::Title => {}
                }
            }
            SocketMessage::ManageRule(identifier, id) => match identifier {
                ApplicationIdentifier::Exe | ApplicationIdentifier::Class => {
                    {
                        let mut manage_identifiers = MANAGE_IDENTIFIERS.lock();
                        if !manage_identifiers.contains(&id) {
                            manage_identifiers.push(id);
                        }
                    }

                    self.update_focused_workspace(false)?;
                }
                ApplicationIdentifier::Title => {}
            },
            SocketMessage::AdjustContainerPadding(sizing, adjustment) => {
                self.adjust_container_padding(sizing, adjustment)?;
            }
            SocketMessage::AdjustWorkspacePadding(sizing, adjustment) => {
                self.adjust_workspace_padding(sizing, adjustment)?;
            }
            SocketMessage::MoveContainerToWorkspaceNumber(workspace_idx) => {
                self.move_container_to_workspace(workspace_idx, true)?;
            }
            SocketMessage::MoveContainerToMonitorNumber(monitor_idx) => {
                self.move_container_to_monitor(monitor_idx, true)?;
            }
            SocketMessage::TogglePause => {
                tracing::info!("pausing");
                self.is_paused = !self.is_paused;
            }
            SocketMessage::ToggleTiling => {
                self.toggle_tiling()?;
            }
            SocketMessage::FocusMonitorNumber(monitor_idx) => {
                self.focus_monitor(monitor_idx)?;
                self.update_focused_workspace(true)?;
            }
            SocketMessage::Retile => {
                for monitor in self.monitors_mut() {
                    let work_area = *monitor.work_area_size();
                    let workspace = monitor
                        .focused_workspace_mut()
                        .context("there is no workspace")?;

                    // Reset any resize adjustments if we want to force a retile
                    for resize in workspace.resize_dimensions_mut() {
                        *resize = None;
                    }

                    workspace.update(&work_area)?;
                }
            }
            SocketMessage::FlipLayout(layout_flip) => self.flip_layout(layout_flip)?,
            SocketMessage::ChangeLayout(layout) => self.change_workspace_layout(layout)?,
            SocketMessage::WorkspaceTiling(monitor_idx, workspace_idx, tile) => {
                self.set_workspace_tiling(monitor_idx, workspace_idx, tile)?;
            }
            SocketMessage::WorkspaceLayout(monitor_idx, workspace_idx, layout) => {
                self.set_workspace_layout(monitor_idx, workspace_idx, layout)?;
            }
            SocketMessage::FocusWorkspaceNumber(workspace_idx) => {
                self.focus_workspace(workspace_idx)?;
            }
            SocketMessage::Stop => {
                tracing::info!(
                    "received stop command, restoring all hidden windows and terminating process"
                );
                self.restore_all_windows();
                std::process::exit(0)
            }
            SocketMessage::EnsureWorkspaces(monitor_idx, workspace_count) => {
                self.ensure_workspaces_for_monitor(monitor_idx, workspace_count)?;
            }
            SocketMessage::NewWorkspace => {
                self.new_workspace()?;
            }
            SocketMessage::WorkspaceName(monitor_idx, workspace_idx, name) => {
                self.set_workspace_name(monitor_idx, workspace_idx, name)?;
            }
            SocketMessage::State => {
                let state = serde_json::to_string_pretty(&window_manager::State::from(self))?;
                let mut socket = dirs::home_dir().context("there is no home directory")?;
                socket.push("komorebic.sock");
                let socket = socket.as_path();

                let mut stream = UnixStream::connect(&socket)?;
                stream.write_all(state.as_bytes())?;
            }
            SocketMessage::ResizeWindow(direction, sizing) => {
                self.resize_window(direction, sizing, Option::from(50))?;
            }
            SocketMessage::FocusFollowsMouse(enable) => {
                if enable {
                    WindowsApi::enable_focus_follows_mouse()?;
                } else {
                    WindowsApi::disable_focus_follows_mouse()?;
                }
            }
            SocketMessage::ReloadConfiguration => {
                Self::reload_configuration();
            }
            SocketMessage::WatchConfiguration(enable) => {
                self.watch_configuration(enable)?;
            }
            SocketMessage::IdentifyTrayApplication(identifier, id) => match identifier {
                ApplicationIdentifier::Exe => {
                    let mut exes = TRAY_AND_MULTI_WINDOW_EXES.lock();
                    if !exes.contains(&id) {
                        exes.push(id);
                    }
                }
                ApplicationIdentifier::Class => {
                    let mut classes = TRAY_AND_MULTI_WINDOW_CLASSES.lock();
                    if !classes.contains(&id) {
                        classes.push(id);
                    }
                }
                ApplicationIdentifier::Title => {}
            },
            SocketMessage::ManageFocusedWindow => {
                self.manage_focused_window()?;
            }
            SocketMessage::UnmanageFocusedWindow => {
                self.unmanage_focused_window()?;
            }
        }

        tracing::info!("processed");
        Ok(())
    }

    #[tracing::instrument(skip(self, stream))]
    pub fn read_commands(&mut self, stream: UnixStream) -> Result<()> {
        let stream = BufReader::new(stream);
        for line in stream.lines() {
            let message = SocketMessage::from_str(&line?)?;

            if self.is_paused {
                if let SocketMessage::TogglePause = message {
                    tracing::info!("resuming");
                    self.is_paused = !self.is_paused;
                    return Ok(());
                }

                tracing::trace!("ignoring while paused");
                return Ok(());
            }

            self.process_command(message)?;
        }

        Ok(())
    }
}
