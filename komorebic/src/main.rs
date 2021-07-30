use std::io::Write;

use clap::Clap;
use color_eyre::Result;
use uds_windows::UnixStream;

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

pub fn send_message(bytes: &[u8]) {
    let mut socket = dirs::home_dir().unwrap();
    socket.push("komorebi.sock");
    let socket = socket.as_path();

    let mut stream = match UnixStream::connect(&socket) {
        Err(_) => panic!("server is not running"),
        Ok(stream) => stream,
    };

    if stream.write_all(&*bytes).is_err() {
        panic!("couldn't send message")
    }
}

fn main() -> Result<()> {
    let opts: Opts = Opts::parse();

    match opts.subcmd {
        SubCommand::Focus(direction) => {
            let bytes = SocketMessage::FocusWindow(direction).as_bytes()?;
            send_message(&*bytes);
        }
        SubCommand::Promote => {
            let bytes = SocketMessage::Promote.as_bytes()?;
            send_message(&*bytes);
        }
        SubCommand::TogglePause => {
            let bytes = SocketMessage::TogglePause.as_bytes()?;
            send_message(&*bytes);
        }
        SubCommand::Retile => {
            let bytes = SocketMessage::Retile.as_bytes()?;
            send_message(&*bytes);
        }
        SubCommand::Move(direction) => {
            let bytes = SocketMessage::MoveWindow(direction).as_bytes()?;
            send_message(&*bytes);
        }
        SubCommand::MoveToMonitor(display) => {
            let bytes = SocketMessage::MoveContainerToMonitorNumber(display.number)
                .as_bytes()
                .unwrap();
            send_message(&*bytes);
        }
        SubCommand::MoveToWorkspace(workspace) => {
            let bytes = SocketMessage::MoveContainerToWorkspaceNumber(workspace.number)
                .as_bytes()
                .unwrap();
            send_message(&*bytes);
        }
        SubCommand::ContainerPadding(gap) => {
            let bytes = SocketMessage::ContainerPadding(gap.monitor, gap.workspace, gap.size)
                .as_bytes()
                .unwrap();
            send_message(&*bytes);
        }
        SubCommand::WorkspacePadding(gap) => {
            let bytes = SocketMessage::WorkspacePadding(gap.monitor, gap.workspace, gap.size)
                .as_bytes()
                .unwrap();
            send_message(&*bytes);
        }
        SubCommand::AdjustWorkspacePadding(sizing_adjustment) => {
            let bytes = SocketMessage::AdjustWorkspacePadding(
                sizing_adjustment.sizing,
                sizing_adjustment.adjustment,
            )
            .as_bytes()
            .unwrap();
            send_message(&*bytes);
        }
        SubCommand::AdjustContainerPadding(sizing_adjustment) => {
            let bytes = SocketMessage::AdjustContainerPadding(
                sizing_adjustment.sizing,
                sizing_adjustment.adjustment,
            )
            .as_bytes()
            .unwrap();
            send_message(&*bytes);
        }
        SubCommand::ToggleFloat => {
            let bytes = SocketMessage::ToggleFloat.as_bytes()?;
            send_message(&*bytes);
        }
        SubCommand::ToggleMonocle => {
            let bytes = SocketMessage::ToggleMonocle.as_bytes()?;
            send_message(&*bytes);
        }
        SubCommand::WorkspaceLayout(layout) => {
            let bytes =
                SocketMessage::WorkspaceLayout(layout.monitor, layout.workspace, layout.layout)
                    .as_bytes()
                    .unwrap();
            send_message(&*bytes);
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
            send_message(&*bytes);
        }
        SubCommand::FloatClass(target) => {
            let bytes = SocketMessage::FloatClass(target.id).as_bytes()?;
            send_message(&*bytes);
        }
        SubCommand::FloatExe(target) => {
            let bytes = SocketMessage::FloatExe(target.id).as_bytes()?;
            send_message(&*bytes);
        }
        SubCommand::FloatTitle(target) => {
            let bytes = SocketMessage::FloatTitle(target.id).as_bytes()?;
            send_message(&*bytes);
        }
        SubCommand::Stack(direction) => {
            let bytes = SocketMessage::StackWindow(direction).as_bytes()?;
            send_message(&*bytes);
        }
        SubCommand::Unstack => {
            let bytes = SocketMessage::UnstackWindow.as_bytes()?;
            send_message(&*bytes);
        }
        SubCommand::CycleStack(direction) => {
            let bytes = SocketMessage::CycleStack(direction).as_bytes()?;
            send_message(&*bytes);
        }
        SubCommand::FlipLayout(flip) => {
            let bytes = SocketMessage::FlipLayout(flip).as_bytes()?;
            send_message(&*bytes);
        }
        SubCommand::FocusMonitor(target) => {
            let bytes = SocketMessage::FocusMonitorNumber(target.number)
                .as_bytes()
                .unwrap();
            send_message(&*bytes);
        }
        SubCommand::FocusWorkspace(target) => {
            let bytes = SocketMessage::FocusWorkspaceNumber(target.number)
                .as_bytes()
                .unwrap();
            send_message(&*bytes);
        }
        SubCommand::WorkspaceName(name) => {
            let bytes = SocketMessage::WorkspaceName(name.monitor, name.workspace, name.value)
                .as_bytes()
                .unwrap();
            send_message(&*bytes);
        }
        SubCommand::EnsureWorkspaces(workspaces) => {
            let bytes =
                SocketMessage::EnsureWorkspaces(workspaces.monitor, workspaces.workspace_count)
                    .as_bytes()
                    .unwrap();
            send_message(&*bytes);
        }
    }

    Ok(())
}
