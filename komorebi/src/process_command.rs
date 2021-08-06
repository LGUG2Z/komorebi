use std::io::BufRead;
use std::io::BufReader;
use std::io::Write;
use std::str::FromStr;
use std::sync::Arc;
use std::sync::Mutex;
use std::thread;

use color_eyre::eyre::ContextCompat;
use color_eyre::Result;
use uds_windows::UnixStream;

use komorebi_core::SocketMessage;

use crate::window_manager::WindowManager;
use crate::FLOAT_CLASSES;
use crate::FLOAT_EXES;
use crate::FLOAT_TITLES;

#[tracing::instrument]
pub fn listen_for_commands(wm: Arc<Mutex<WindowManager>>) {
    let listener = wm
        .lock()
        .unwrap()
        .command_listener
        .try_clone()
        .expect("could not clone unix listener");

    thread::spawn(move || {
        tracing::info!("listening");
        for client in listener.incoming() {
            match client {
                Ok(stream) => match wm.lock().unwrap().read_commands(stream) {
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
            SocketMessage::ContainerPadding(monitor_idx, workspace_idx, size) => {
                self.set_container_padding(monitor_idx, workspace_idx, size)?;
            }
            SocketMessage::WorkspacePadding(monitor_idx, workspace_idx, size) => {
                self.set_workspace_padding(monitor_idx, workspace_idx, size)?;
            }
            SocketMessage::FloatClass(target) => {
                let mut float_classes = FLOAT_CLASSES.lock().unwrap();
                if !float_classes.contains(&target) {
                    float_classes.push(target);
                }
            }
            SocketMessage::FloatExe(target) => {
                let mut float_exes = FLOAT_EXES.lock().unwrap();
                if !float_exes.contains(&target) {
                    float_exes.push(target);
                }
            }
            SocketMessage::FloatTitle(target) => {
                let mut float_titles = FLOAT_TITLES.lock().unwrap();
                if !float_titles.contains(&target) {
                    float_titles.push(target);
                }
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
            SocketMessage::MoveContainerToMonitorNumber(monitor_idx) => {
                self.move_container_to_monitor(monitor_idx, true)?;
            }
            SocketMessage::TogglePause => {
                tracing::info!("pausing");
                self.is_paused = !self.is_paused;
            }
            SocketMessage::FocusMonitorNumber(monitor_idx) => {
                self.focus_monitor(monitor_idx)?;
                self.update_focused_workspace(true)?;
            }
            SocketMessage::Retile => {
                for monitor in self.monitors_mut() {
                    let work_area = *monitor.work_area_size();
                    monitor
                        .focused_workspace_mut()
                        .context("there is no workspace")?
                        .update(&work_area)?;
                }
            }
            SocketMessage::FlipLayout(layout_flip) => self.flip_layout(layout_flip)?,
            SocketMessage::ChangeLayout(layout) => self.change_workspace_layout(layout)?,
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
            SocketMessage::WorkspaceName(monitor_idx, workspace_idx, name) => {
                self.set_workspace_name(monitor_idx, workspace_idx, name)?;
            }
            SocketMessage::State => {
                let state = serde_json::to_string_pretty(self)?;
                let mut socket = dirs::home_dir().context("there is no home directory")?;
                socket.push("komorebic.sock");
                let socket = socket.as_path();

                let mut stream = UnixStream::connect(&socket)?;
                stream.write_all(state.as_bytes())?;
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
