use std::fs::File;
use std::io::BufRead;
use std::io::BufReader;
use std::io::ErrorKind;
use std::io::Write;

use clap::Clap;
use color_eyre::eyre::ContextCompat;
use color_eyre::Result;
use uds_windows::UnixListener;
use uds_windows::UnixStream;

use bindings::Windows::Win32::Foundation::HWND;
use bindings::Windows::Win32::UI::WindowsAndMessaging::ShowWindow;
use bindings::Windows::Win32::UI::WindowsAndMessaging::SHOW_WINDOW_CMD;
use bindings::Windows::Win32::UI::WindowsAndMessaging::SW_RESTORE;
use komorebi_core::CycleDirection;
use komorebi_core::Layout;
use komorebi_core::LayoutFlip;
use komorebi_core::OperationDirection;
use komorebi_core::Sizing;
use komorebi_core::SocketMessage;

#[derive(Clap)]
#[clap(version = "1.0", author = "Jade Iqbal <jadeiqbal@fastmail.com>")]
struct Opts {
    #[clap(subcommand)]
    subcmd: SubCommand,
}

#[derive(Clap)]
enum SubCommand {
    Focus(OperationDirection),
    Move(OperationDirection),
    Stack(OperationDirection),
    Unstack,
    CycleStack(CycleDirection),
    MoveToMonitor(Target),
    MoveToWorkspace(Target),
    FocusMonitor(Target),
    FocusWorkspace(Target),
    Promote,
    EnsureWorkspaces(WorkspaceCountForMonitor),
    Retile,
    ContainerPadding(SizeForMonitorWorkspace),
    WorkspacePadding(SizeForMonitorWorkspace),
    WorkspaceLayout(LayoutForMonitorWorkspace),
    WorkspaceName(NameForMonitorWorkspace),
    ToggleFloat,
    TogglePause,
    ToggleMonocle,
    RestoreWindows,
    State,
    Start,
    Stop,
    FloatClass(FloatTarget),
    FloatExe(FloatTarget),
    FloatTitle(FloatTarget),
    AdjustContainerPadding(SizingAdjustment),
    AdjustWorkspacePadding(SizingAdjustment),
    FlipLayout(LayoutFlip),
}

#[derive(Clap)]
struct WorkspaceCountForMonitor {
    monitor: usize,
    workspace_count: usize,
}

#[derive(Clap)]
struct SizeForMonitorWorkspace {
    monitor: usize,
    workspace: usize,
    size: i32,
}

#[derive(Clap)]
struct NameForMonitorWorkspace {
    monitor: usize,
    workspace: usize,
    value: String,
}

#[derive(Clap)]
struct LayoutForMonitorWorkspace {
    monitor: usize,
    workspace: usize,
    layout: Layout,
}

#[derive(Clap)]
struct Target {
    number: usize,
}

#[derive(Clap)]
struct SizingAdjustment {
    sizing: Sizing,
    adjustment: i32,
}

#[derive(Clap)]
struct FloatTarget {
    id: String,
}

pub fn send_message(bytes: &[u8]) -> Result<()> {
    let mut socket = dirs::home_dir().context("there is no home directory")?;
    socket.push("komorebi.sock");
    let socket = socket.as_path();

    let mut stream = UnixStream::connect(&socket)?;
    Ok(stream.write_all(&*bytes)?)
}

fn main() -> Result<()> {
    let opts: Opts = Opts::parse();

    match opts.subcmd {
        SubCommand::Focus(direction) => {
            let bytes = SocketMessage::FocusWindow(direction).as_bytes()?;
            send_message(&*bytes)?;
        }
        SubCommand::Promote => {
            let bytes = SocketMessage::Promote.as_bytes()?;
            send_message(&*bytes)?;
        }
        SubCommand::TogglePause => {
            let bytes = SocketMessage::TogglePause.as_bytes()?;
            send_message(&*bytes)?;
        }
        SubCommand::Retile => {
            let bytes = SocketMessage::Retile.as_bytes()?;
            send_message(&*bytes)?;
        }
        SubCommand::Move(direction) => {
            let bytes = SocketMessage::MoveWindow(direction).as_bytes()?;
            send_message(&*bytes)?;
        }
        SubCommand::MoveToMonitor(display) => {
            let bytes = SocketMessage::MoveContainerToMonitorNumber(display.number)
                .as_bytes()
                .unwrap();
            send_message(&*bytes)?;
        }
        SubCommand::MoveToWorkspace(workspace) => {
            let bytes = SocketMessage::MoveContainerToWorkspaceNumber(workspace.number)
                .as_bytes()
                .unwrap();
            send_message(&*bytes)?;
        }
        SubCommand::ContainerPadding(gap) => {
            let bytes = SocketMessage::ContainerPadding(gap.monitor, gap.workspace, gap.size)
                .as_bytes()
                .unwrap();
            send_message(&*bytes)?;
        }
        SubCommand::WorkspacePadding(gap) => {
            let bytes = SocketMessage::WorkspacePadding(gap.monitor, gap.workspace, gap.size)
                .as_bytes()
                .unwrap();
            send_message(&*bytes)?;
        }
        SubCommand::AdjustWorkspacePadding(sizing_adjustment) => {
            let bytes = SocketMessage::AdjustWorkspacePadding(
                sizing_adjustment.sizing,
                sizing_adjustment.adjustment,
            )
            .as_bytes()
            .unwrap();
            send_message(&*bytes)?;
        }
        SubCommand::AdjustContainerPadding(sizing_adjustment) => {
            let bytes = SocketMessage::AdjustContainerPadding(
                sizing_adjustment.sizing,
                sizing_adjustment.adjustment,
            )
            .as_bytes()
            .unwrap();
            send_message(&*bytes)?;
        }
        SubCommand::ToggleFloat => {
            let bytes = SocketMessage::ToggleFloat.as_bytes()?;
            send_message(&*bytes)?;
        }
        SubCommand::ToggleMonocle => {
            let bytes = SocketMessage::ToggleMonocle.as_bytes()?;
            send_message(&*bytes)?;
        }
        SubCommand::WorkspaceLayout(layout) => {
            let bytes =
                SocketMessage::WorkspaceLayout(layout.monitor, layout.workspace, layout.layout)
                    .as_bytes()
                    .unwrap();
            send_message(&*bytes)?;
        }
        SubCommand::Start => {
            let script = r#"Start-Process komorebi -WindowStyle hidden"#;
            match powershell_script::run(script, true) {
                Ok(output) => {
                    println!("{}", output);
                }
                Err(e) => {
                    println!("Error: {}", e);
                }
            }
        }
        SubCommand::Stop => {
            let bytes = SocketMessage::Stop.as_bytes()?;
            send_message(&*bytes)?;
        }
        SubCommand::FloatClass(target) => {
            let bytes = SocketMessage::FloatClass(target.id).as_bytes()?;
            send_message(&*bytes)?;
        }
        SubCommand::FloatExe(target) => {
            let bytes = SocketMessage::FloatExe(target.id).as_bytes()?;
            send_message(&*bytes)?;
        }
        SubCommand::FloatTitle(target) => {
            let bytes = SocketMessage::FloatTitle(target.id).as_bytes()?;
            send_message(&*bytes)?;
        }
        SubCommand::Stack(direction) => {
            let bytes = SocketMessage::StackWindow(direction).as_bytes()?;
            send_message(&*bytes)?;
        }
        SubCommand::Unstack => {
            let bytes = SocketMessage::UnstackWindow.as_bytes()?;
            send_message(&*bytes)?;
        }
        SubCommand::CycleStack(direction) => {
            let bytes = SocketMessage::CycleStack(direction).as_bytes()?;
            send_message(&*bytes)?;
        }
        SubCommand::FlipLayout(flip) => {
            let bytes = SocketMessage::FlipLayout(flip).as_bytes()?;
            send_message(&*bytes)?;
        }
        SubCommand::FocusMonitor(target) => {
            let bytes = SocketMessage::FocusMonitorNumber(target.number)
                .as_bytes()
                .unwrap();
            send_message(&*bytes)?;
        }
        SubCommand::FocusWorkspace(target) => {
            let bytes = SocketMessage::FocusWorkspaceNumber(target.number)
                .as_bytes()
                .unwrap();
            send_message(&*bytes)?;
        }
        SubCommand::WorkspaceName(name) => {
            let bytes = SocketMessage::WorkspaceName(name.monitor, name.workspace, name.value)
                .as_bytes()
                .unwrap();
            send_message(&*bytes)?;
        }
        SubCommand::EnsureWorkspaces(workspaces) => {
            let bytes =
                SocketMessage::EnsureWorkspaces(workspaces.monitor, workspaces.workspace_count)
                    .as_bytes()
                    .unwrap();
            send_message(&*bytes)?;
        }
        SubCommand::State => {
            let home = dirs::home_dir().context("there is no home directory")?;
            let mut socket = home;
            socket.push("komorebic.sock");
            let socket = socket.as_path();

            match std::fs::remove_file(&socket) {
                Ok(_) => {}
                Err(error) => match error.kind() {
                    // Doing this because ::exists() doesn't work reliably on Windows via IntelliJ
                    ErrorKind::NotFound => {}
                    _ => {
                        return Err(error.into());
                    }
                },
            };

            let bytes = SocketMessage::State.as_bytes().unwrap();
            send_message(&*bytes)?;

            let listener = UnixListener::bind(&socket)?;
            match listener.accept() {
                Ok(incoming) => {
                    let stream = BufReader::new(incoming.0);
                    for line in stream.lines() {
                        println!("{}", line?)
                    }

                    return Ok(());
                }
                Err(error) => {
                    panic!("{}", error)
                }
            }
        }
        SubCommand::RestoreWindows => {
            let mut hwnd_json = dirs::home_dir().context("there is no home directory")?;
            hwnd_json.push("komorebi.hwnd.json");

            let file = File::open(hwnd_json)?;
            let reader = BufReader::new(file);
            let hwnds: Vec<isize> = serde_json::from_reader(reader)?;

            for hwnd in hwnds {
                restore_window(HWND(hwnd));
            }
        }
    }

    Ok(())
}

fn show_window(hwnd: HWND, command: SHOW_WINDOW_CMD) {
    // BOOL is returned but does not signify whether or not the operation was succesful
    // https://docs.microsoft.com/en-us/windows/win32/api/winuser/nf-winuser-showwindow
    unsafe { ShowWindow(hwnd, command) };
}

fn restore_window(hwnd: HWND) {
    show_window(hwnd, SW_RESTORE);
}
