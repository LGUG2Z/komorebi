#![warn(clippy::all, clippy::nursery, clippy::pedantic)]
#![allow(clippy::missing_errors_doc)]

use std::fs::File;
use std::fs::OpenOptions;
use std::io::BufRead;
use std::io::BufReader;
use std::io::ErrorKind;
use std::io::Write;
use std::path::PathBuf;
use std::process::Command;

use clap::AppSettings;
use clap::ArgEnum;
use clap::Clap;
use color_eyre::eyre::anyhow;
use color_eyre::Result;
use fs_tail::TailedFile;
use heck::KebabCase;
use paste::paste;
use uds_windows::UnixListener;
use uds_windows::UnixStream;

use bindings::Windows::Win32::Foundation::HWND;
use bindings::Windows::Win32::UI::WindowsAndMessaging::ShowWindow;
use bindings::Windows::Win32::UI::WindowsAndMessaging::SHOW_WINDOW_CMD;
use bindings::Windows::Win32::UI::WindowsAndMessaging::SW_RESTORE;
use derive_ahk::AhkFunction;
use derive_ahk::AhkLibrary;
use komorebi_core::ApplicationIdentifier;
use komorebi_core::CycleDirection;
use komorebi_core::Flip;
use komorebi_core::FocusFollowsMouseImplementation;
use komorebi_core::Layout;
use komorebi_core::OperationDirection;
use komorebi_core::Rect;
use komorebi_core::Sizing;
use komorebi_core::SocketMessage;
use komorebi_core::StateQuery;

trait AhkLibrary {
    fn generate_ahk_library() -> String;
}

trait AhkFunction {
    fn generate_ahk_function() -> String;
}

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
    ( $( $name:ident: $element:ty ),+ $(,)? ) => {
        $(
            paste! {
                #[derive(clap::Clap, derive_ahk::AhkFunction)]
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
    CycleFocus: CycleDirection,
    CycleMove: CycleDirection,
    CycleMonitor: CycleDirection,
    CycleWorkspace: CycleDirection,
    Stack: OperationDirection,
    CycleStack: CycleDirection,
    FlipLayout: Flip,
    ChangeLayout: Layout,
    WatchConfiguration: BooleanState,
    Query: StateQuery,
}

macro_rules! gen_target_subcommand_args {
    // SubCommand Pattern
    ( $( $name:ident ),+ $(,)? ) => {
        $(
            #[derive(clap::Clap, derive_ahk::AhkFunction)]
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
    SendToMonitor,
    SendToWorkspace,
    FocusMonitor,
    FocusWorkspace,
}

// Thanks to @danielhenrymantilla for showing me how to use cfg_attr with an optional argument like
// this on the Rust Programming Language Community Discord Server
macro_rules! gen_workspace_subcommand_args {
    // Workspace Property: #[enum] Value Enum (if the value is an Enum)
    // Workspace Property: Value Type (if the value is anything else)
    ( $( $name:ident: $(#[enum] $(@$arg_enum:tt)?)? $value:ty ),+ $(,)? ) => (
        paste! {
            $(
                #[derive(clap::Clap, derive_ahk::AhkFunction)]
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
    Tiling: #[enum] BooleanState,
}

#[derive(Clap, AhkFunction)]
struct Resize {
    #[clap(arg_enum)]
    edge: OperationDirection,
    #[clap(arg_enum)]
    sizing: Sizing,
}

#[derive(Clap, AhkFunction)]
struct InvisibleBorders {
    /// Size of the left invisible border
    left: i32,
    /// Size of the top invisible border (usually 0)
    top: i32,
    /// Size of the right invisible border (usually left * 2)
    right: i32,
    /// Size of the bottom invisible border (usually the same as left)
    bottom: i32,
}

#[derive(Clap, AhkFunction)]
struct WorkAreaOffset {
    /// Size of the left work area offset (set right to left * 2 to maintain right padding)
    left: i32,
    /// Size of the top work area offset (set bottom to the same value to maintain bottom padding)
    top: i32,
    /// Size of the right work area offset
    right: i32,
    /// Size of the bottom work area offset
    bottom: i32,
}

#[derive(Clap, AhkFunction)]
struct EnsureWorkspaces {
    /// Monitor index (zero-indexed)
    monitor: usize,
    /// Number of desired workspaces
    workspace_count: usize,
}

macro_rules! gen_padding_subcommand_args {
    // SubCommand Pattern
    ( $( $name:ident ),+ $(,)? ) => {
        $(
            #[derive(clap::Clap, derive_ahk::AhkFunction)]
            pub struct $name {
                /// Monitor index (zero-indexed)
                monitor: usize,
                /// Workspace index on the specified monitor (zero-indexed)
                workspace: usize,
                /// Pixels to pad with as an integer
                size: i32,
            }
        )+
    };
}

gen_padding_subcommand_args! {
    ContainerPadding,
    WorkspacePadding,
}

macro_rules! gen_padding_adjustment_subcommand_args {
    // SubCommand Pattern
    ( $( $name:ident ),+ $(,)? ) => {
        $(
            #[derive(clap::Clap, derive_ahk::AhkFunction)]
            pub struct $name {
                #[clap(arg_enum)]
                sizing: Sizing,
                /// Pixels to adjust by as an integer
                adjustment: i32,
            }
        )+
    };
}

gen_padding_adjustment_subcommand_args! {
    AdjustContainerPadding,
    AdjustWorkspacePadding,
}

macro_rules! gen_application_target_subcommand_args {
    // SubCommand Pattern
    ( $( $name:ident ),+ $(,)? ) => {
        $(
            #[derive(clap::Clap, derive_ahk::AhkFunction)]
            pub struct $name {
                #[clap(arg_enum)]
                identifier: ApplicationIdentifier,
                /// Identifier as a string
                id: String,
            }
        )+
    };
}

gen_application_target_subcommand_args! {
    FloatRule,
    ManageRule,
    IdentifyTrayApplication,
    IdentifyBorderOverflow,
}

#[derive(Clap, AhkFunction)]
struct WorkspaceRule {
    #[clap(arg_enum)]
    identifier: ApplicationIdentifier,
    /// Identifier as a string
    id: String,
    /// Monitor index (zero-indexed)
    monitor: usize,
    /// Workspace index on the specified monitor (zero-indexed)
    workspace: usize,
}

#[derive(Clap, AhkFunction)]
struct ToggleFocusFollowsMouse {
    #[clap(arg_enum, short, long, default_value = "windows")]
    implementation: FocusFollowsMouseImplementation,
}

#[derive(Clap, AhkFunction)]
struct FocusFollowsMouse {
    #[clap(arg_enum, short, long, default_value = "windows")]
    implementation: FocusFollowsMouseImplementation,
    #[clap(arg_enum)]
    boolean_state: BooleanState,
}

#[derive(Clap, AhkFunction)]
struct Start {
    /// Allow the use of komorebi's custom focus-follows-mouse implementation
    #[clap(long)]
    ffm: bool,
}

#[derive(Clap, AhkFunction)]
struct Save {
    /// File to which the resize layout dimensions should be saved
    path: String,
}

#[derive(Clap, AhkFunction)]
struct Load {
    /// File from which the resize layout dimensions should be loaded
    path: String,
}

#[derive(Clap)]
#[clap(author, about, version, setting = AppSettings::DeriveDisplayOrder)]
struct Opts {
    #[clap(subcommand)]
    subcmd: SubCommand,
}

#[derive(Clap, AhkLibrary)]
enum SubCommand {
    /// Start komorebi.exe as a background process
    Start(Start),
    /// Stop the komorebi.exe process and restore all hidden windows
    Stop,
    /// Show a JSON representation of the current window manager state
    State,
    /// Query the current window manager state
    #[clap(setting = AppSettings::ArgRequiredElseHelp)]
    Query(Query),
    /// Tail komorebi.exe's process logs (cancel with Ctrl-C)
    Log,
    /// Quicksave the current resize layout dimensions
    QuickSave,
    /// Load the last quicksaved resize layout dimensions
    QuickLoad,
    /// Save the current resize layout dimensions to a file
    #[clap(setting = AppSettings::ArgRequiredElseHelp)]
    Save(Save),
    /// Load the resize layout dimensions from a file
    #[clap(setting = AppSettings::ArgRequiredElseHelp)]
    Load(Load),
    /// Change focus to the window in the specified direction
    #[clap(setting = AppSettings::ArgRequiredElseHelp)]
    Focus(Focus),
    /// Move the focused window in the specified direction
    #[clap(setting = AppSettings::ArgRequiredElseHelp)]
    Move(Move),
    /// Change focus to the window in the specified cycle direction
    #[clap(setting = AppSettings::ArgRequiredElseHelp)]
    CycleFocus(CycleFocus),
    /// Move the focused window in the specified cycle direction
    #[clap(setting = AppSettings::ArgRequiredElseHelp)]
    CycleMove(CycleMove),
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
    /// Send the focused window to the specified monitor
    #[clap(setting = AppSettings::ArgRequiredElseHelp)]
    SendToMonitor(SendToMonitor),
    /// Send the focused window to the specified workspace
    #[clap(setting = AppSettings::ArgRequiredElseHelp)]
    SendToWorkspace(SendToWorkspace),
    /// Focus the specified monitor
    #[clap(setting = AppSettings::ArgRequiredElseHelp)]
    FocusMonitor(FocusMonitor),
    /// Focus the specified workspace on the focused monitor
    #[clap(setting = AppSettings::ArgRequiredElseHelp)]
    FocusWorkspace(FocusWorkspace),
    /// Focus the monitor in the given cycle direction
    #[clap(setting = AppSettings::ArgRequiredElseHelp)]
    CycleMonitor(CycleMonitor),
    /// Focus the workspace in the given cycle direction
    #[clap(setting = AppSettings::ArgRequiredElseHelp)]
    CycleWorkspace(CycleWorkspace),
    /// Create and append a new workspace on the focused monitor
    NewWorkspace,
    /// Set the invisible border dimensions around each window
    #[clap(setting = AppSettings::ArgRequiredElseHelp)]
    InvisibleBorders(InvisibleBorders),
    /// Set offsets to exclude parts of the work area from tiling
    #[clap(setting = AppSettings::ArgRequiredElseHelp)]
    WorkAreaOffset(WorkAreaOffset),
    /// Adjust container padding on the focused workspace
    #[clap(setting = AppSettings::ArgRequiredElseHelp)]
    AdjustContainerPadding(AdjustContainerPadding),
    /// Adjust workspace padding on the focused workspace
    #[clap(setting = AppSettings::ArgRequiredElseHelp)]
    AdjustWorkspacePadding(AdjustWorkspacePadding),
    /// Set the layout on the focused workspace
    #[clap(setting = AppSettings::ArgRequiredElseHelp)]
    ChangeLayout(ChangeLayout),
    /// Flip the layout on the focused workspace (BSP only)
    #[clap(setting = AppSettings::ArgRequiredElseHelp)]
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
    ContainerPadding(ContainerPadding),
    /// Set the workspace padding for the specified workspace
    #[clap(setting = AppSettings::ArgRequiredElseHelp)]
    WorkspacePadding(WorkspacePadding),
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
    /// Toggle native maximization for the focused window
    ToggleMaximize,
    /// Restore all hidden windows (debugging command)
    RestoreWindows,
    /// Force komorebi to manage the focused window
    Manage,
    /// Unmanage a window that was forcibly managed
    Unmanage,
    /// Reload ~/komorebi.ahk (if it exists)
    ReloadConfiguration,
    /// Enable or disable watching of ~/komorebi.ahk (if it exists)
    #[clap(setting = AppSettings::ArgRequiredElseHelp)]
    WatchConfiguration(WatchConfiguration),
    /// Add a rule to always float the specified application
    #[clap(setting = AppSettings::ArgRequiredElseHelp)]
    FloatRule(FloatRule),
    /// Add a rule to always manage the specified application
    #[clap(setting = AppSettings::ArgRequiredElseHelp)]
    ManageRule(ManageRule),
    /// Add a rule to associate an application with a workspace
    #[clap(setting = AppSettings::ArgRequiredElseHelp)]
    WorkspaceRule(WorkspaceRule),
    /// Identify an application that closes to the system tray
    #[clap(setting = AppSettings::ArgRequiredElseHelp)]
    IdentifyTrayApplication(IdentifyTrayApplication),
    /// Identify an application that has overflowing borders
    #[clap(setting = AppSettings::ArgRequiredElseHelp)]
    IdentifyBorderOverflow(IdentifyBorderOverflow),
    /// Enable or disable focus follows mouse for the operating system
    #[clap(setting = AppSettings::ArgRequiredElseHelp)]
    FocusFollowsMouse(FocusFollowsMouse),
    /// Toggle focus follows mouse for the operating system
    #[clap(setting = AppSettings::ArgRequiredElseHelp)]
    ToggleFocusFollowsMouse(ToggleFocusFollowsMouse),
    /// Generate a library of AutoHotKey helper functions
    AhkLibrary,
}

pub fn send_message(bytes: &[u8]) -> Result<()> {
    let mut socket = dirs::home_dir().ok_or_else(|| anyhow!("there is no home directory"))?;
    socket.push("komorebi.sock");
    let socket = socket.as_path();

    let mut stream = UnixStream::connect(&socket)?;
    Ok(stream.write_all(&*bytes)?)
}

#[allow(clippy::too_many_lines)]
fn main() -> Result<()> {
    let opts: Opts = Opts::parse();

    match opts.subcmd {
        SubCommand::AhkLibrary => {
            let mut library =
                dirs::home_dir().ok_or_else(|| anyhow!("there is no home directory"))?;
            library.push("komorebic.lib.ahk");
            let mut file = OpenOptions::new()
                .write(true)
                .create(true)
                .truncate(true)
                .open(library.clone())?;

            file.write_all(SubCommand::generate_ahk_library().as_bytes())?;

            println!(
                "\nAHK helper library for komorebic written to {}",
                library.to_str().ok_or_else(|| anyhow!(
                    "could not find the path to the generated ahk lib file"
                ))?
            );

            println!(
                "\nYou can include the library at the top of your ~/komorebi.ahk config with this line:"
            );

            println!("\n#Include %A_ScriptDir%\\komorebic.lib.ahk");
        }
        SubCommand::Log => {
            let mut color_log = std::env::temp_dir();
            color_log.push("komorebi.log");
            let file = TailedFile::new(File::open(color_log)?);
            let locked = file.lock();
            for line in locked.lines() {
                println!("{}", line?);
            }
        }
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
        SubCommand::CycleFocus(arg) => {
            send_message(&*SocketMessage::CycleFocusWindow(arg.cycle_direction).as_bytes()?)?;
        }
        SubCommand::CycleMove(arg) => {
            send_message(&*SocketMessage::CycleMoveWindow(arg.cycle_direction).as_bytes()?)?;
        }
        SubCommand::MoveToMonitor(arg) => {
            send_message(&*SocketMessage::MoveContainerToMonitorNumber(arg.target).as_bytes()?)?;
        }
        SubCommand::MoveToWorkspace(arg) => {
            send_message(&*SocketMessage::MoveContainerToWorkspaceNumber(arg.target).as_bytes()?)?;
        }
        SubCommand::SendToMonitor(arg) => {
            send_message(&*SocketMessage::SendContainerToMonitorNumber(arg.target).as_bytes()?)?;
        }
        SubCommand::SendToWorkspace(arg) => {
            send_message(&*SocketMessage::SendContainerToWorkspaceNumber(arg.target).as_bytes()?)?;
        }
        SubCommand::InvisibleBorders(arg) => {
            send_message(
                &*SocketMessage::InvisibleBorders(Rect {
                    left: arg.left,
                    top: arg.top,
                    right: arg.right,
                    bottom: arg.bottom,
                })
                .as_bytes()?,
            )?;
        }
        SubCommand::WorkAreaOffset(arg) => {
            send_message(
                &*SocketMessage::WorkAreaOffset(Rect {
                    left: arg.left,
                    top: arg.top,
                    right: arg.right,
                    bottom: arg.bottom,
                })
                .as_bytes()?,
            )?;
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
        SubCommand::ToggleFocusFollowsMouse(arg) => {
            send_message(&*SocketMessage::ToggleFocusFollowsMouse(arg.implementation).as_bytes()?)?;
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
        SubCommand::ToggleMaximize => {
            send_message(&*SocketMessage::ToggleMaximize.as_bytes()?)?;
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
        SubCommand::Start(arg) => {
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
                        Option::from(buf.to_str().ok_or_else(|| {
                            anyhow!("cannot create a string from the scoop komorebi path")
                        })?)
                    }
                }
            } else {
                None
            };

            let script = exec.map_or_else(
                || {
                    if arg.ffm {
                        String::from(
                            "Start-Process komorebi.exe -ArgumentList '--ffm' -WindowStyle hidden",
                        )
                    } else {
                        String::from("Start-Process komorebi.exe -WindowStyle hidden")
                    }
                },
                |exec| {
                    if arg.ffm {
                        format!(
                            "Start-Process '{}' -ArgumentList '--ffm' -WindowStyle hidden",
                            exec
                        )
                    } else {
                        format!("Start-Process '{}' -WindowStyle hidden", exec)
                    }
                },
            );

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
        SubCommand::FloatRule(arg) => {
            send_message(&*SocketMessage::FloatRule(arg.identifier, arg.id).as_bytes()?)?;
        }
        SubCommand::ManageRule(arg) => {
            send_message(&*SocketMessage::ManageRule(arg.identifier, arg.id).as_bytes()?)?;
        }
        SubCommand::WorkspaceRule(arg) => {
            send_message(
                &*SocketMessage::WorkspaceRule(arg.identifier, arg.id, arg.monitor, arg.workspace)
                    .as_bytes()?,
            )?;
        }
        SubCommand::Stack(arg) => {
            send_message(&*SocketMessage::StackWindow(arg.operation_direction).as_bytes()?)?;
        }
        SubCommand::Unstack => {
            send_message(&*SocketMessage::UnstackWindow.as_bytes()?)?;
        }
        SubCommand::CycleStack(arg) => {
            send_message(&*SocketMessage::CycleStack(arg.cycle_direction).as_bytes()?)?;
        }
        SubCommand::ChangeLayout(arg) => {
            send_message(&*SocketMessage::ChangeLayout(arg.layout).as_bytes()?)?;
        }
        SubCommand::FlipLayout(arg) => {
            send_message(&*SocketMessage::FlipLayout(arg.flip).as_bytes()?)?;
        }
        SubCommand::FocusMonitor(arg) => {
            send_message(&*SocketMessage::FocusMonitorNumber(arg.target).as_bytes()?)?;
        }
        SubCommand::FocusWorkspace(arg) => {
            send_message(&*SocketMessage::FocusWorkspaceNumber(arg.target).as_bytes()?)?;
        }
        SubCommand::CycleMonitor(arg) => {
            send_message(&*SocketMessage::CycleFocusMonitor(arg.cycle_direction).as_bytes()?)?;
        }
        SubCommand::CycleWorkspace(arg) => {
            send_message(&*SocketMessage::CycleFocusWorkspace(arg.cycle_direction).as_bytes()?)?;
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
            let home = dirs::home_dir().ok_or_else(|| anyhow!("there is no home directory"))?;
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
                        println!("{}", line?);
                    }

                    return Ok(());
                }
                Err(error) => {
                    panic!("{}", error);
                }
            }
        }
        SubCommand::Query(arg) => {
            let home = dirs::home_dir().ok_or_else(|| anyhow!("there is no home directory"))?;
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

            send_message(&*SocketMessage::Query(arg.state_query).as_bytes()?)?;

            let listener = UnixListener::bind(&socket)?;
            match listener.accept() {
                Ok(incoming) => {
                    let stream = BufReader::new(incoming.0);
                    for line in stream.lines() {
                        println!("{}", line?);
                    }

                    return Ok(());
                }
                Err(error) => {
                    panic!("{}", error);
                }
            }
        }
        SubCommand::RestoreWindows => {
            let mut hwnd_json =
                dirs::home_dir().ok_or_else(|| anyhow!("there is no home directory"))?;
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

            send_message(
                &*SocketMessage::FocusFollowsMouse(arg.implementation, enable).as_bytes()?,
            )?;
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
        SubCommand::IdentifyBorderOverflow(target) => {
            send_message(
                &*SocketMessage::IdentifyBorderOverflow(target.identifier, target.id).as_bytes()?,
            )?;
        }
        SubCommand::Manage => {
            send_message(&*SocketMessage::ManageFocusedWindow.as_bytes()?)?;
        }
        SubCommand::Unmanage => {
            send_message(&*SocketMessage::UnmanageFocusedWindow.as_bytes()?)?;
        }
        SubCommand::QuickSave => {
            send_message(&*SocketMessage::QuickSave.as_bytes()?)?;
        }
        SubCommand::QuickLoad => {
            send_message(&*SocketMessage::QuickLoad.as_bytes()?)?;
        }
        SubCommand::Save(arg) => {
            send_message(&*SocketMessage::Save(resolve_windows_path(&arg.path)?).as_bytes()?)?;
        }
        SubCommand::Load(arg) => {
            send_message(&*SocketMessage::Load(resolve_windows_path(&arg.path)?).as_bytes()?)?;
        }
    }

    Ok(())
}

fn resolve_windows_path(raw_path: &str) -> Result<PathBuf> {
    let path = if raw_path.starts_with('~') {
        raw_path.replacen(
            "~",
            &dirs::home_dir()
                .ok_or_else(|| anyhow!("there is no home directory"))?
                .display()
                .to_string(),
            1,
        )
    } else {
        raw_path.to_string()
    };

    let full_path = PathBuf::from(path);

    let parent = full_path
        .parent()
        .ok_or_else(|| anyhow!("cannot parse directory"))?;

    let file = full_path
        .components()
        .last()
        .ok_or_else(|| anyhow!("cannot parse filename"))?;

    let mut canonicalized = std::fs::canonicalize(parent)?;
    canonicalized.push(file);

    Ok(canonicalized)
}

fn show_window(hwnd: HWND, command: SHOW_WINDOW_CMD) {
    // BOOL is returned but does not signify whether or not the operation was succesful
    // https://docs.microsoft.com/en-us/windows/win32/api/winuser/nf-winuser-showwindow
    unsafe { ShowWindow(hwnd, command) };
}

fn restore_window(hwnd: HWND) {
    show_window(hwnd, SW_RESTORE);
}
