#![warn(clippy::all)]
#![windows_subsystem = "windows"]

use komorebi_client::SocketMessage;
use std::io::Write;
use std::ptr;
use std::sync::Arc;
use std::thread;
use std::time::Duration;
use uds_windows::UnixStream;
use windows::Win32::Foundation::CloseHandle;
use windows::Win32::Foundation::ERROR_PIPE_CONNECTED;
use windows::Win32::Foundation::HANDLE;
use windows::Win32::Security::InitializeSecurityDescriptor;
use windows::Win32::Security::SetSecurityDescriptorDacl;
use windows::Win32::Security::PSECURITY_DESCRIPTOR;
use windows::Win32::Security::SECURITY_ATTRIBUTES;
use windows::Win32::Security::SECURITY_DESCRIPTOR;
use windows::Win32::Storage::FileSystem::ReadFile;
use windows::Win32::Storage::FileSystem::PIPE_ACCESS_DUPLEX;
use windows::Win32::System::Pipes::ConnectNamedPipe;
use windows::Win32::System::Pipes::CreateNamedPipeA;
use windows::Win32::System::Pipes::DisconnectNamedPipe;
use windows::Win32::System::Pipes::PIPE_READMODE_BYTE;
use windows::Win32::System::Pipes::PIPE_TYPE_BYTE;
use windows::Win32::System::Pipes::PIPE_WAIT;

const PIPE_NAME: &[u8] = b"\\\\.\\pipe\\komorebi-command\0";
const BUFFER_SIZE: usize = 512;
const NUM_PIPE_INSTANCES: usize = 4;

fn main() {
    let data_dir = dirs::data_local_dir()
        .expect("Unable to locate local data directory")
        .join("komorebi");

    let komorebi_socket = Arc::new(data_dir.join("komorebi.sock"));
    let mut handles = Vec::with_capacity(NUM_PIPE_INSTANCES);

    for _ in 0..NUM_PIPE_INSTANCES {
        let socket_path = Arc::clone(&komorebi_socket);
        let handle = thread::spawn(move || {
            pipe_listener_thread(socket_path);
        });
        handles.push(handle);
    }

    for handle in handles {
        let _ = handle.join();
    }
}

fn pipe_listener_thread(komorebi_socket: Arc<std::path::PathBuf>) {
    let mut sd = SECURITY_DESCRIPTOR::default();
    let mut sa = SECURITY_ATTRIBUTES {
        nLength: std::mem::size_of::<SECURITY_ATTRIBUTES>() as u32,
        lpSecurityDescriptor: ptr::null_mut(),
        bInheritHandle: false.into(),
    };

    unsafe {
        let sd_ptr = PSECURITY_DESCRIPTOR(&mut sd as *mut _ as *mut _);
        if InitializeSecurityDescriptor(sd_ptr, 1).is_ok() {
            if SetSecurityDescriptorDacl(sd_ptr, true, None, false).is_ok() {
                sa.lpSecurityDescriptor = sd_ptr.0;
            }
        }
    }

    let mut buffer = [0u8; BUFFER_SIZE];

    loop {
        let pipe_handle = unsafe {
            CreateNamedPipeA(
                windows::core::PCSTR::from_raw(PIPE_NAME.as_ptr()),
                PIPE_ACCESS_DUPLEX,
                PIPE_TYPE_BYTE | PIPE_READMODE_BYTE | PIPE_WAIT,
                255,
                BUFFER_SIZE as u32,
                BUFFER_SIZE as u32,
                0,
                Some(&sa),
            )
        };

        let pipe_handle = match pipe_handle {
            Ok(handle) => handle,
            Err(_) => {
                thread::sleep(Duration::from_millis(100));
                continue;
            }
        };

        let connected = unsafe { ConnectNamedPipe(pipe_handle, None) };

        if let Err(e) = connected {
            let error_code = e.code().0 as u32;
            if error_code != ERROR_PIPE_CONNECTED.0 {
                unsafe {
                    let _ = CloseHandle(pipe_handle);
                }
                continue;
            }
        }

        handle_client(pipe_handle, &mut buffer, &komorebi_socket);

        unsafe {
            let _ = DisconnectNamedPipe(pipe_handle);
            let _ = CloseHandle(pipe_handle);
        }
    }
}

#[inline]
fn handle_client(
    pipe_handle: HANDLE,
    buffer: &mut [u8; BUFFER_SIZE],
    komorebi_socket: &std::path::Path,
) {
    let mut bytes_read = 0u32;

    let read_result = unsafe { ReadFile(pipe_handle, Some(buffer), Some(&mut bytes_read), None) };

    if read_result.is_err() || bytes_read == 0 {
        return;
    }

    let command_str = unsafe { std::str::from_utf8_unchecked(&buffer[..bytes_read as usize]) };
    let trimmed = command_str.trim().trim_start_matches('\u{FEFF}');

    if let Some(message) = parse_command(trimmed) {
        if let Ok(json) = serde_json::to_string(&message) {
            if let Ok(mut stream) = UnixStream::connect(komorebi_socket) {
                let _ = stream.set_write_timeout(Some(Duration::from_millis(100)));
                let _ = stream.write_all(json.as_bytes());
            }
        }
    }
}

#[inline]
fn parse_command(cmd: &str) -> Option<SocketMessage> {
    use komorebi_client::*;
    use std::path::PathBuf;

    let mut parts = cmd.splitn(3, ' ');
    let command = parts.next()?;
    let arg1 = parts.next();
    let arg2 = parts.next();

    match command {
        // No-arg commands
        "quick-save-resize" => Some(SocketMessage::QuickSave),
        "quick-load-resize" => Some(SocketMessage::QuickLoad),
        "minimize" => Some(SocketMessage::Minimize),
        "close" => Some(SocketMessage::Close),
        "force-focus" => Some(SocketMessage::ForceFocus),
        "unstack" => Some(SocketMessage::UnstackWindow),
        "stack-all" => Some(SocketMessage::StackAll),
        "unstack-all" => Some(SocketMessage::UnstackAll),
        "send-to-last-workspace" => Some(SocketMessage::SendContainerToLastWorkspace),
        "move-to-last-workspace" => Some(SocketMessage::MoveContainerToLastWorkspace),
        "focus-monitor-at-cursor" => Some(SocketMessage::FocusMonitorAtCursor),
        "focus-last-workspace" => Some(SocketMessage::FocusLastWorkspace),
        "close-workspace" => Some(SocketMessage::CloseWorkspace),
        "new-workspace" => Some(SocketMessage::NewWorkspace),
        "promote" => Some(SocketMessage::Promote),
        "promote-focus" => Some(SocketMessage::PromoteFocus),
        "retile" => Some(SocketMessage::Retile),
        "toggle-pause" => Some(SocketMessage::TogglePause),
        "toggle-tiling" => Some(SocketMessage::ToggleTiling),
        "toggle-float" => Some(SocketMessage::ToggleFloat),
        "toggle-monocle" => Some(SocketMessage::ToggleMonocle),
        "toggle-maximize" => Some(SocketMessage::ToggleMaximize),
        "toggle-lock" => Some(SocketMessage::ToggleLock),
        "manage" => Some(SocketMessage::ManageFocusedWindow),
        "unmanage" => Some(SocketMessage::UnmanageFocusedWindow),
        "reload-configuration" => Some(SocketMessage::ReloadConfiguration),

        // PathBuf commands
        "save-resize" => Some(SocketMessage::Save(PathBuf::from(arg1?))),
        "load-resize" => Some(SocketMessage::Load(PathBuf::from(arg1?))),

        // OperationDirection commands
        "focus" => Some(SocketMessage::FocusWindow(arg1?.parse().ok()?)),
        "move" => Some(SocketMessage::MoveWindow(arg1?.parse().ok()?)),
        "stack" => Some(SocketMessage::StackWindow(arg1?.parse().ok()?)),
        "promote-window" => Some(SocketMessage::PromoteWindow(arg1?.parse().ok()?)),

        // CycleDirection commands
        "cycle-focus" => Some(SocketMessage::CycleFocusWindow(arg1?.parse().ok()?)),
        "cycle-move" => Some(SocketMessage::CycleMoveWindow(arg1?.parse().ok()?)),
        "cycle-stack" => Some(SocketMessage::CycleStack(arg1?.parse().ok()?)),
        "cycle-stack-index" => Some(SocketMessage::CycleStackIndex(arg1?.parse().ok()?)),
        "cycle-move-to-monitor" => Some(SocketMessage::CycleMoveContainerToMonitor(
            arg1?.parse().ok()?,
        )),
        "cycle-move-to-workspace" => Some(SocketMessage::CycleMoveContainerToWorkspace(
            arg1?.parse().ok()?,
        )),
        "cycle-send-to-monitor" => Some(SocketMessage::CycleSendContainerToMonitor(
            arg1?.parse().ok()?,
        )),
        "cycle-send-to-workspace" => Some(SocketMessage::CycleSendContainerToWorkspace(
            arg1?.parse().ok()?,
        )),
        "cycle-monitor" => Some(SocketMessage::CycleFocusMonitor(arg1?.parse().ok()?)),
        "cycle-workspace" => Some(SocketMessage::CycleFocusWorkspace(arg1?.parse().ok()?)),
        "cycle-empty-workspace" => {
            Some(SocketMessage::CycleFocusEmptyWorkspace(arg1?.parse().ok()?))
        }
        "cycle-move-workspace-to-monitor" => Some(SocketMessage::CycleMoveWorkspaceToMonitor(
            arg1?.parse().ok()?,
        )),
        "cycle-layout" => Some(SocketMessage::CycleLayout(arg1?.parse().ok()?)),

        // String commands
        "eager-focus" => Some(SocketMessage::EagerFocus(arg1?.to_string())),
        "move-to-named-workspace" => Some(SocketMessage::MoveContainerToNamedWorkspace(
            arg1?.to_string(),
        )),
        "send-to-named-workspace" => Some(SocketMessage::SendContainerToNamedWorkspace(
            arg1?.to_string(),
        )),
        "focus-named-workspace" => Some(SocketMessage::FocusNamedWorkspace(arg1?.to_string())),

        // usize commands
        "focus-stack-window" => Some(SocketMessage::FocusStackWindow(arg1?.parse().ok()?)),
        "move-to-monitor" => Some(SocketMessage::MoveContainerToMonitorNumber(
            arg1?.parse().ok()?,
        )),
        "move-to-workspace" => Some(SocketMessage::MoveContainerToWorkspaceNumber(
            arg1?.parse().ok()?,
        )),
        "send-to-monitor" => Some(SocketMessage::SendContainerToMonitorNumber(
            arg1?.parse().ok()?,
        )),
        "send-to-workspace" => Some(SocketMessage::SendContainerToWorkspaceNumber(
            arg1?.parse().ok()?,
        )),
        "focus-monitor" => Some(SocketMessage::FocusMonitorNumber(arg1?.parse().ok()?)),
        "focus-workspace" => Some(SocketMessage::FocusWorkspaceNumber(arg1?.parse().ok()?)),
        "focus-workspaces" => Some(SocketMessage::FocusWorkspaceNumbers(arg1?.parse().ok()?)),
        "move-workspace-to-monitor" => Some(SocketMessage::MoveWorkspaceToMonitorNumber(
            arg1?.parse().ok()?,
        )),
        "swap-workspaces-with-monitor" => Some(SocketMessage::SwapWorkspacesToMonitorNumber(
            arg1?.parse().ok()?,
        )),

        // i32 commands
        "resize-delta" => Some(SocketMessage::ResizeDelta(arg1?.parse().ok()?)),

        // DefaultLayout commands
        "change-layout" => Some(SocketMessage::ChangeLayout(arg1?.parse().ok()?)),

        // Axis commands
        "flip-layout" => Some(SocketMessage::FlipLayout(arg1?.parse().ok()?)),

        // Two-argument commands (usize, usize)
        "send-to-monitor-workspace" => Some(SocketMessage::SendContainerToMonitorWorkspaceNumber(
            arg1?.parse().ok()?,
            arg2?.parse().ok()?,
        )),
        "move-to-monitor-workspace" => Some(SocketMessage::MoveContainerToMonitorWorkspaceNumber(
            arg1?.parse().ok()?,
            arg2?.parse().ok()?,
        )),
        "focus-monitor-workspace" => Some(SocketMessage::FocusMonitorWorkspaceNumber(
            arg1?.parse().ok()?,
            arg2?.parse().ok()?,
        )),

        // Two-argument commands (OperationDirection, Sizing)
        "resize-edge" => Some(SocketMessage::ResizeWindowEdge(
            arg1?.parse().ok()?,
            arg2?.parse().ok()?,
        )),

        // Two-argument commands (Axis, Sizing)
        "resize-axis" => Some(SocketMessage::ResizeWindowAxis(
            arg1?.parse().ok()?,
            arg2?.parse().ok()?,
        )),

        _ => None,
    }
}
