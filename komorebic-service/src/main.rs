#![warn(clippy::all)]
#![windows_subsystem = "windows"]

use color_eyre::Result;
use komorebi_client::send_message;
use komorebi_client::SocketMessage;
use std::ptr;
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
const BUFFER_SIZE: usize = 4096;

fn main() -> Result<()> {
    color_eyre::install()?;

    // Setup file logging
    let data_dir = dirs::data_local_dir()
        .expect("Unable to locate local data directory")
        .join("komorebi");
    std::fs::create_dir_all(&data_dir)?;

    let log_file = data_dir.join("komorebic-service.log");
    let file = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(log_file)?;

    tracing_subscriber::fmt()
        .with_writer(file)
        .with_ansi(false)
        .init();

    tracing::info!("komorebic-service starting");
    tracing::info!("Listening on: \\\\.\\pipe\\komorebi-command");

    loop {
        // Create security descriptor that allows everyone to access the pipe
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

        // Create a new named pipe instance
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
            Err(e) => {
                tracing::error!("Failed to create named pipe: {}", e);
                std::thread::sleep(std::time::Duration::from_secs(1));
                continue;
            }
        };

        // Wait for a client to connect
        let connected = unsafe { ConnectNamedPipe(pipe_handle, None) };

        if let Err(e) = connected {
            let error_code = e.code().0 as u32;
            if error_code != ERROR_PIPE_CONNECTED.0 {
                tracing::error!("Failed to connect to client: {}", e);
                unsafe {
                    let _ = CloseHandle(pipe_handle);
                }
                continue;
            }
        }

        // Handle the client request
        if let Err(e) = handle_client(pipe_handle) {
            tracing::error!("Error handling client: {}", e);
        }

        // Disconnect and close the pipe
        unsafe {
            let _ = DisconnectNamedPipe(pipe_handle);
            let _ = CloseHandle(pipe_handle);
        }
    }
}

fn handle_client(pipe_handle: HANDLE) -> Result<()> {
    let mut buffer = vec![0u8; BUFFER_SIZE];
    let mut bytes_read = 0u32;

    let read_result =
        unsafe { ReadFile(pipe_handle, Some(&mut buffer), Some(&mut bytes_read), None) };

    if let Err(e) = read_result {
        return Err(color_eyre::eyre::eyre!("Failed to read from pipe: {}", e));
    }

    if bytes_read == 0 {
        return Err(color_eyre::eyre::eyre!("Read 0 bytes from pipe"));
    }

    // Convert bytes to string and strip BOM if present
    let command_str = String::from_utf8_lossy(&buffer[..bytes_read as usize]);
    let trimmed = command_str.trim().trim_start_matches('\u{FEFF}');

    tracing::info!("Received: '{}'", trimmed);

    // Parse and execute the command
    let message = parse_command(trimmed)?;
    send_message(&message)?;

    tracing::info!("Sent to komorebi: {:?}", message);
    Ok(())
}

/// Parse a simple command string into a SocketMessage
/// Format: "Command" or "Command arg"
fn parse_command(cmd: &str) -> Result<SocketMessage> {
    let parts: Vec<&str> = cmd.splitn(2, ' ').collect();
    let command = parts[0];
    let arg = parts.get(1).map(|s| s.trim());

    let message = match command {
        // Focus commands
        "Focus" => {
            let direction = arg.ok_or_else(|| {
                color_eyre::eyre::eyre!("Focus requires direction (Left/Right/Up/Down)")
            })?;
            SocketMessage::FocusWindow(direction.parse()?)
        }
        "FocusNamedWorkspace" => {
            let workspace = arg.ok_or_else(|| {
                color_eyre::eyre::eyre!("FocusNamedWorkspace requires workspace name")
            })?;
            SocketMessage::FocusNamedWorkspace(workspace.to_string())
        }
        "FocusLastWorkspace" => SocketMessage::FocusLastWorkspace,

        // Add more commands here following the same pattern:
        // "CommandName" => SocketMessage::CommandName(args...),
        _ => {
            return Err(color_eyre::eyre::eyre!("Unknown command: {}", command));
        }
    };

    Ok(message)
}
