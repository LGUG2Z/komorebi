use std::fs::File;
use std::io::BufRead;
use std::io::BufReader;
use std::io::ErrorKind;
use std::io::Write;
use std::path::PathBuf;
use std::process::Command;

use clap::AppSettings;
use clap::ArgEnum;
use clap::Clap;
use color_eyre::eyre::ContextCompat;
use color_eyre::Result;
use paste::paste;
use uds_windows::UnixListener;
use uds_windows::UnixStream;

use bindings::Windows::Win32::Foundation::HWND;
use bindings::Windows::Win32::UI::WindowsAndMessaging::ShowWindow;
use bindings::Windows::Win32::UI::WindowsAndMessaging::SHOW_WINDOW_CMD;
use bindings::Windows::Win32::UI::WindowsAndMessaging::SW_RESTORE;
use komorebi_core::ApplicationIdentifier;
use komorebi_core::CycleDirection;
use komorebi_core::Layout;
use komorebi_core::LayoutFlip;
use komorebi_core::OperationDirection;
use komorebi_core::Sizing;
use komorebi_core::SocketMessage;

#[derive(ArgEnum)]
enum BooleanState {
    Enable,
    Disable,
}

impl From<BooleanState> for bool {
    fn from(b: BooleanState) -> Self {
        match b {
            BooleanState::Enable => true,
            BooleanState::Disable => false,
        }
    }
}

macro_rules! gen_enum_subcommand_args {
    // SubCommand Pattern: Enum Type
    ( $( $name:ident: $element:ty ),+ ) => {
        $(
            paste! {
                #[derive(clap::Clap)]
                pub struct $name {
                    #[clap(arg_enum)]
                    [<$element:snake>]: $element
                }
            }
        )+
    };
}

gen_enum_subcommand_args! {
    Focus: OperationDirection,
    Move: OperationDirection,
    Stack: OperationDirection,
    CycleStack: CycleDirection,
    FlipLayout: LayoutFlip,
    WatchConfiguration: BooleanState,
    FocusFollowsMouse: BooleanState
}

macro_rules! gen_target_subcommand_args {
    // SubCommand Pattern
    ( $( $name:ident ),+ ) => {
        $(
            #[derive(clap::Clap)]
            pub struct $name {
                /// Target index (zero-indexed)
                target: usize,
            }
        )+
    };
}

gen_target_subcommand_args! {
    MoveToMonitor,
    MoveToWorkspace,
    FocusMonitor,
    FocusWorkspace
}

// Thanks to @danielhenrymantilla for showing me how to use cfg_attr with an optional argument like
// this on the Rust Programming Language Community Discord Server
macro_rules! gen_workspace_subcommand_args {
    // Workspace Property: #[enum] Value Enum (if the value is an Enum)
    // Workspace Property: Value Type (if the value is anything else)
    ( $( $name:ident: $(#[enum] $(@$arg_enum:tt)?)? $value:ty ),+ ) => (
        paste! {
            $(
                #[derive(clap::Clap)]
                pub struct [<Workspace $name>] {
                    /// Monitor index (zero-indexed)
                    monitor: usize,

                    /// Workspace index on the specified monitor (zero-indexed)
                    workspace: usize,

                    $(#[clap(arg_enum)] $($arg_enum)?)?
                    #[cfg_attr(
                        all($(FALSE $($arg_enum)?)?),
                        doc = ""$name" of the workspace as a "$value""
                    )]
                    value: $value,
                }
            )+
        }
    )
}

gen_workspace_subcommand_args! {
    Name: String,
    Layout: #[enum] Layout,
    Tiling: #[enum] BooleanState
}

#[derive(Clap)]
struct Resize {
    #[clap(arg_enum)]
    edge: OperationDirection,
    #[clap(arg_enum)]
    sizing: Sizing,
}

#[derive(Clap)]
struct EnsureWorkspaces {
    /// Monitor index (zero-indexed)
    monitor: usize,
    /// Number of desired workspaces
    workspace_count: usize,
}

#[derive(Clap)]
struct Padding {
    /// Monitor index (zero-indexed)
    monitor: usize,
    /// Workspace index on the specified monitor (zero-indexed)
    workspace: usize,
    /// Pixels to pad with as an integer
    size: i32,
}

#[derive(Clap)]
struct PaddingAdjustment {
    #[clap(arg_enum)]
    sizing: Sizing,
    /// Pixels to adjust by as an integer
    adjustment: i32,
}

#[derive(Clap)]
struct ApplicationTarget {
    #[clap(arg_enum)]
    identifier: ApplicationIdentifier,
    /// Identifier as a string
    id: String,
}

#[derive(Clap)]
#[clap(version = "0.1.0", author = "Jade Iqbal <jadeiqbal@fastmail.com>")]
#[clap(setting = AppSettings::DeriveDisplayOrder)]
struct Opts {
    #[clap(subcommand)]
    subcmd: SubCommand,
}

#[derive(Clap)]
enum SubCommand {
    /// Start komorebi.exe as a background process
    Start,
    /// Stop the komorebi.exe process and restore all hidden windows
    Stop,
    /// Show a JSON representation of the current window manager state
    State,
    /// Change focus to the window in the specified direction
    #[clap(setting = AppSettings::ArgRequiredElseHelp)]
    Focus(Focus),
    /// Move the focused window in the specified direction
    #[clap(setting = AppSettings::ArgRequiredElseHelp)]
    Move(Move),
    /// Stack the focused window in the specified direction
    #[clap(setting = AppSettings::ArgRequiredElseHelp)]
    Stack(Stack),
    /// Resize the focused window in the specified direction
    #[clap(setting = AppSettings::ArgRequiredElseHelp)]
    Resize(Resize),
    /// Unstack the focused window
    Unstack,
    /// Cycle the focused stack in the specified cycle direction
    #[clap(setting = AppSettings::ArgRequiredElseHelp)]
    CycleStack(CycleStack),
    /// Move the focused window to the specified monitor
    #[clap(setting = AppSettings::ArgRequiredElseHelp)]
    MoveToMonitor(MoveToMonitor),
    /// Move the focused window to the specified workspace
    #[clap(setting = AppSettings::ArgRequiredElseHelp)]
    MoveToWorkspace(MoveToWorkspace),
    /// Focus the specified monitor
    #[clap(setting = AppSettings::ArgRequiredElseHelp)]
    FocusMonitor(FocusMonitor),
    /// Focus the specified workspace on the focused monitor
    #[clap(setting = AppSettings::ArgRequiredElseHelp)]
    FocusWorkspace(FocusWorkspace),
    /// Create and append a new workspace on the focused monitor
    NewWorkspace,
    /// Adjust container padding on the focused workspace
    #[clap(setting = AppSettings::ArgRequiredElseHelp)]
    AdjustContainerPadding(PaddingAdjustment),
    /// Adjust workspace padding on the focused workspace
    #[clap(setting = AppSettings::ArgRequiredElseHelp)]
    AdjustWorkspacePadding(PaddingAdjustment),
    /// Flip the layout on the focused workspace (BSP only)
    FlipLayout(FlipLayout),
    /// Promote the focused window to the top of the tree
    Promote,
    /// Force the retiling of all managed windows
    Retile,
    /// Create at least this many workspaces for the specified monitor
    #[clap(setting = AppSettings::ArgRequiredElseHelp)]
    EnsureWorkspaces(EnsureWorkspaces),
    /// Set the container padding for the specified workspace
    #[clap(setting = AppSettings::ArgRequiredElseHelp)]
    ContainerPadding(Padding),
    /// Set the workspace padding for the specified workspace
    #[clap(setting = AppSettings::ArgRequiredElseHelp)]
    WorkspacePadding(Padding),
    /// Set the layout for the specified workspace
    #[clap(setting = AppSettings::ArgRequiredElseHelp)]
    WorkspaceLayout(WorkspaceLayout),
    /// Enable or disable window tiling for the specified workspace
    #[clap(setting = AppSettings::ArgRequiredElseHelp)]
    WorkspaceTiling(WorkspaceTiling),
    /// Set the workspace name for the specified workspace
    #[clap(setting = AppSettings::ArgRequiredElseHelp)]
    WorkspaceName(WorkspaceName),
    /// Toggle the window manager on and off across all monitors
    TogglePause,
    /// Toggle window tiling on the focused workspace
    ToggleTiling,
    /// Toggle floating mode for the focused window
    ToggleFloat,
    /// Toggle monocle mode for the focused container
    ToggleMonocle,
    /// Restore all hidden windows (debugging command)
    RestoreWindows,
    /// Reload ~/komorebi.ahk (if it exists)
    ReloadConfiguration,
    /// Toggle the automatic reloading of ~/komorebi.ahk (if it exists)
    #[clap(setting = AppSettings::ArgRequiredElseHelp)]
    WatchConfiguration(WatchConfiguration),
    /// Add a rule to always float the specified application
    #[clap(setting = AppSettings::ArgRequiredElseHelp)]
    FloatRule(ApplicationTarget),
    /// Identify an application that closes to the system tray
    #[clap(setting = AppSettings::ArgRequiredElseHelp)]
    IdentifyTrayApplication(ApplicationTarget),
    /// Enable or disable focus follows mouse for the operating system
    FocusFollowsMouse(FocusFollowsMouse),
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
        SubCommand::Focus(arg) => {
            send_message(&*SocketMessage::FocusWindow(arg.operation_direction).as_bytes()?)?;
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
        SubCommand::Move(arg) => {
            send_message(&*SocketMessage::MoveWindow(arg.operation_direction).as_bytes()?)?;
        }
        SubCommand::MoveToMonitor(arg) => {
            send_message(&*SocketMessage::MoveContainerToMonitorNumber(arg.target).as_bytes()?)?;
        }
        SubCommand::MoveToWorkspace(arg) => {
            send_message(&*SocketMessage::MoveContainerToWorkspaceNumber(arg.target).as_bytes()?)?;
        }
        SubCommand::ContainerPadding(arg) => {
            send_message(
                &*SocketMessage::ContainerPadding(arg.monitor, arg.workspace, arg.size)
                    .as_bytes()?,
            )?;
        }
        SubCommand::WorkspacePadding(arg) => {
            send_message(
                &*SocketMessage::WorkspacePadding(arg.monitor, arg.workspace, arg.size)
                    .as_bytes()?,
            )?;
        }
        SubCommand::AdjustWorkspacePadding(arg) => {
            send_message(
                &*SocketMessage::AdjustWorkspacePadding(arg.sizing, arg.adjustment).as_bytes()?,
            )?;
        }
        SubCommand::AdjustContainerPadding(arg) => {
            send_message(
                &*SocketMessage::AdjustContainerPadding(arg.sizing, arg.adjustment).as_bytes()?,
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
        SubCommand::WorkspaceLayout(arg) => {
            send_message(
                &*SocketMessage::WorkspaceLayout(arg.monitor, arg.workspace, arg.value)
                    .as_bytes()?,
            )?;
        }
        SubCommand::WorkspaceTiling(arg) => {
            send_message(
                &*SocketMessage::WorkspaceTiling(arg.monitor, arg.workspace, arg.value.into())
                    .as_bytes()?,
            )?;
        }
        SubCommand::Start => {
            let mut buf: PathBuf;

            // The komorebi.ps1 shim will only exist in the Path if installed by Scoop
            let exec = if let Ok(output) = Command::new("where.exe").arg("komorebi.ps1").output() {
                let stdout = String::from_utf8(output.stdout)?;
                match stdout.trim() {
                    stdout if stdout.is_empty() => None,
                    stdout => {
                        buf = PathBuf::from(stdout);
                        buf.pop(); // %USERPROFILE%\scoop\shims
                        buf.pop(); // %USERPROFILE%\scoop
                        buf.push("apps\\komorebi\\current\\komorebi.exe"); //%USERPROFILE%\scoop\komorebi\current\komorebi.exe
                        Option::from(
                            buf.to_str()
                                .context("cannot create a string from the scoop komorebi path")?,
                        )
                    }
                }
            } else {
                None
            };

            let script = if let Some(exec) = exec {
                format!("Start-Process '{}' -WindowStyle hidden", exec)
            } else {
                String::from("Start-Process komorebi -WindowStyle hidden")
            };

            match powershell_script::run(&script, true) {
                Ok(output) => {
                    println!("{}", output);
                }
                Err(error) => {
                    println!("Error: {}", error);
                }
            }
        }
        SubCommand::Stop => {
            send_message(&*SocketMessage::Stop.as_bytes()?)?;
        }
        SubCommand::FloatRule(arg) => match arg.identifier {
            ApplicationIdentifier::Exe => {
                send_message(&*SocketMessage::FloatExe(arg.id).as_bytes()?)?;
            }
            ApplicationIdentifier::Class => {
                send_message(&*SocketMessage::FloatClass(arg.id).as_bytes()?)?;
            }
            ApplicationIdentifier::Title => {
                send_message(&*SocketMessage::FloatTitle(arg.id).as_bytes()?)?;
            }
        },
        SubCommand::Stack(arg) => {
            send_message(&*SocketMessage::StackWindow(arg.operation_direction).as_bytes()?)?;
        }
        SubCommand::Unstack => {
            send_message(&*SocketMessage::UnstackWindow.as_bytes()?)?;
        }
        SubCommand::CycleStack(arg) => {
            send_message(&*SocketMessage::CycleStack(arg.cycle_direction).as_bytes()?)?;
        }
        SubCommand::FlipLayout(arg) => {
            send_message(&*SocketMessage::FlipLayout(arg.layout_flip).as_bytes()?)?;
        }
        SubCommand::FocusMonitor(arg) => {
            send_message(&*SocketMessage::FocusMonitorNumber(arg.target).as_bytes()?)?;
        }
        SubCommand::FocusWorkspace(arg) => {
            send_message(&*SocketMessage::FocusWorkspaceNumber(arg.target).as_bytes()?)?;
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
        SubCommand::FocusFollowsMouse(arg) => {
            let enable = match arg.boolean_state {
                BooleanState::Enable => true,
                BooleanState::Disable => false,
            };

            send_message(&*SocketMessage::FocusFollowsMouse(enable).as_bytes()?)?;
        }
        SubCommand::ReloadConfiguration => {
            send_message(&*SocketMessage::ReloadConfiguration.as_bytes()?)?;
        }
        SubCommand::WatchConfiguration(arg) => {
            let enable = match arg.boolean_state {
                BooleanState::Enable => true,
                BooleanState::Disable => false,
            };
            send_message(&*SocketMessage::WatchConfiguration(enable).as_bytes()?)?;
        }
        SubCommand::IdentifyTrayApplication(target) => {
            send_message(
                &*SocketMessage::IdentifyTrayApplication(target.identifier, target.id)
                    .as_bytes()?,
            )?;
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
