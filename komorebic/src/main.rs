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
    Resize(Resize),
    Unstack,
    CycleStack(CycleDirection),
    MoveToMonitor(Target),
    MoveToWorkspace(Target),
    FocusMonitor(Target),
    FocusWorkspace(Target),
    NewWorkspace,
    Promote,
    EnsureWorkspaces(WorkspaceCountForMonitor),
    Retile,
    ContainerPadding(SizeForMonitorWorkspace),
    WorkspacePadding(SizeForMonitorWorkspace),
    WorkspaceLayout(LayoutForMonitorWorkspace),
    WorkspaceTiling(TilingForMonitorWorkspace),
    WorkspaceName(NameForMonitorWorkspace),
    ToggleTiling,
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
    FocusFollowsMouse(BooleanState),
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

fn on_or_off(s: &str) -> Result<bool, &'static str> {
    match s {
        "on" => Ok(true),
        "off" => Ok(false),
        _ => Err("expected `on` or `off`"),
    }
}

#[derive(Clap)]
struct TilingForMonitorWorkspace {
    monitor: usize,
    workspace: usize,
    #[clap(parse(try_from_str = on_or_off))]
    tile: bool,
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

#[derive(Clap)]
struct Resize {
    edge: OperationDirection,
    sizing: Sizing,
}

#[derive(Clap)]
enum BooleanState {
    Enable,
    Disable,
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
            send_message(&*SocketMessage::FocusWindow(direction).as_bytes()?)?;
        }
        SubCommand::Promote => {
            send_message(&*SocketMessage::Promote.as_bytes()?)?;
        }
        SubCommand::TogglePause => {
            send_message(&*SocketMessage::TogglePause.as_bytes()?)?;
        }
        SubCommand::Retile => {
            send_message(&*SocketMessage::Retile.as_bytes()?)?;
        }
        SubCommand::Move(direction) => {
            send_message(&*SocketMessage::MoveWindow(direction).as_bytes()?)?;
        }
        SubCommand::MoveToMonitor(display) => {
            send_message(
                &*SocketMessage::MoveContainerToMonitorNumber(display.number).as_bytes()?,
            )?;
        }
        SubCommand::MoveToWorkspace(workspace) => {
            send_message(
                &*SocketMessage::MoveContainerToWorkspaceNumber(workspace.number).as_bytes()?,
            )?;
        }
        SubCommand::ContainerPadding(gap) => {
            send_message(
                &*SocketMessage::ContainerPadding(gap.monitor, gap.workspace, gap.size)
                    .as_bytes()?,
            )?;
        }
        SubCommand::WorkspacePadding(gap) => {
            send_message(
                &*SocketMessage::WorkspacePadding(gap.monitor, gap.workspace, gap.size)
                    .as_bytes()?,
            )?;
        }
        SubCommand::AdjustWorkspacePadding(sizing_adjustment) => {
            send_message(
                &*SocketMessage::AdjustWorkspacePadding(
                    sizing_adjustment.sizing,
                    sizing_adjustment.adjustment,
                )
                .as_bytes()?,
            )?;
        }
        SubCommand::AdjustContainerPadding(sizing_adjustment) => {
            send_message(
                &*SocketMessage::AdjustContainerPadding(
                    sizing_adjustment.sizing,
                    sizing_adjustment.adjustment,
                )
                .as_bytes()?,
            )?;
        }
        SubCommand::ToggleTiling => {
            send_message(&*SocketMessage::ToggleTiling.as_bytes()?)?;
        }
        SubCommand::ToggleFloat => {
            send_message(&*SocketMessage::ToggleFloat.as_bytes()?)?;
        }
        SubCommand::ToggleMonocle => {
            send_message(&*SocketMessage::ToggleMonocle.as_bytes()?)?;
        }
        SubCommand::WorkspaceLayout(layout) => {
            send_message(
                &*SocketMessage::WorkspaceLayout(layout.monitor, layout.workspace, layout.layout)
                    .as_bytes()?,
            )?;
        }
        SubCommand::WorkspaceTiling(layout) => {
            send_message(
                &*SocketMessage::WorkspaceTiling(layout.monitor, layout.workspace, layout.tile)
                    .as_bytes()?,
            )?;
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
            send_message(&*SocketMessage::Stop.as_bytes()?)?;
        }
        SubCommand::FloatClass(target) => {
            send_message(&*SocketMessage::FloatClass(target.id).as_bytes()?)?;
        }
        SubCommand::FloatExe(target) => {
            send_message(&*SocketMessage::FloatExe(target.id).as_bytes()?)?;
        }
        SubCommand::FloatTitle(target) => {
            send_message(&*SocketMessage::FloatTitle(target.id).as_bytes()?)?;
        }
        SubCommand::Stack(direction) => {
            send_message(&*SocketMessage::StackWindow(direction).as_bytes()?)?;
        }
        SubCommand::Unstack => {
            send_message(&*SocketMessage::UnstackWindow.as_bytes()?)?;
        }
        SubCommand::CycleStack(direction) => {
            send_message(&*SocketMessage::CycleStack(direction).as_bytes()?)?;
        }
        SubCommand::FlipLayout(flip) => {
            send_message(&*SocketMessage::FlipLayout(flip).as_bytes()?)?;
        }
        SubCommand::FocusMonitor(target) => {
            send_message(&*SocketMessage::FocusMonitorNumber(target.number).as_bytes()?)?;
        }
        SubCommand::FocusWorkspace(target) => {
            send_message(&*SocketMessage::FocusWorkspaceNumber(target.number).as_bytes()?)?;
        }
        SubCommand::NewWorkspace => {
            send_message(&*SocketMessage::NewWorkspace.as_bytes()?)?;
        }
        SubCommand::WorkspaceName(name) => {
            send_message(
                &*SocketMessage::WorkspaceName(name.monitor, name.workspace, name.value)
                    .as_bytes()?,
            )?;
        }
        SubCommand::EnsureWorkspaces(workspaces) => {
            send_message(
                &*SocketMessage::EnsureWorkspaces(workspaces.monitor, workspaces.workspace_count)
                    .as_bytes()?,
            )?;
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

            send_message(&*SocketMessage::State.as_bytes()?)?;

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
        SubCommand::Resize(resize) => {
            send_message(&*SocketMessage::ResizeWindow(resize.edge, resize.sizing).as_bytes()?)?;
        }
        SubCommand::FocusFollowsMouse(enable) => {
            let enable = match enable {
                BooleanState::Enable => true,
                BooleanState::Disable => false,
            };

            send_message(&*SocketMessage::FocusFollowsMouse(enable).as_bytes()?)?;
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
