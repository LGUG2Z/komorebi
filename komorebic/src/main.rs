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
use std::sync::atomic::AtomicBool;
use std::sync::atomic::Ordering;
use std::time::Duration;

use clap::Parser;
use clap::ValueEnum;
use color_eyre::eyre::anyhow;
use color_eyre::Result;
use fs_tail::TailedFile;
use heck::ToKebabCase;
use lazy_static::lazy_static;
use paste::paste;
use sysinfo::SystemExt;
use uds_windows::UnixListener;
use uds_windows::UnixStream;
use windows::Win32::Foundation::HWND;
use windows::Win32::UI::WindowsAndMessaging::ShowWindow;
use windows::Win32::UI::WindowsAndMessaging::SHOW_WINDOW_CMD;
use windows::Win32::UI::WindowsAndMessaging::SW_RESTORE;

use derive_ahk::AhkFunction;
use derive_ahk::AhkLibrary;
use komorebi_core::config_generation::ApplicationConfigurationGenerator;
use komorebi_core::ApplicationIdentifier;
use komorebi_core::Axis;
use komorebi_core::CycleDirection;
use komorebi_core::DefaultLayout;
use komorebi_core::FocusFollowsMouseImplementation;
use komorebi_core::HidingBehaviour;
use komorebi_core::MoveBehaviour;
use komorebi_core::OperationBehaviour;
use komorebi_core::OperationDirection;
use komorebi_core::Rect;
use komorebi_core::Sizing;
use komorebi_core::SocketMessage;
use komorebi_core::StateQuery;
use komorebi_core::WindowKind;

lazy_static! {
    static ref HAS_CUSTOM_CONFIG_HOME: AtomicBool = AtomicBool::new(false);
    static ref HOME_DIR: PathBuf = {
        std::env::var("KOMOREBI_CONFIG_HOME").map_or_else(
            |_| dirs::home_dir().expect("there is no home directory"),
            |home_path| {
                let home = PathBuf::from(&home_path);

                if home.as_path().is_dir() {
                    HAS_CUSTOM_CONFIG_HOME.store(true, Ordering::SeqCst);
                    home
                } else {
                    panic!(
                        "$Env:KOMOREBI_CONFIG_HOME is set to '{}', which is not a valid directory",
                        home_path
                    );
                }
            },
        )
    };
    static ref DATA_DIR: PathBuf = dirs::data_local_dir()
        .expect("there is no local data directory")
        .join("komorebi");
}

trait AhkLibrary {
    fn generate_ahk_library() -> String;
}

trait AhkFunction {
    fn generate_ahk_function() -> String;
}

#[derive(Copy, Clone, ValueEnum)]
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
                #[derive(clap::Parser, derive_ahk::AhkFunction)]
                pub struct $name {
                    #[clap(value_enum)]
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
    CycleMoveToWorkspace: CycleDirection,
    CycleSendToWorkspace: CycleDirection,
    CycleSendToMonitor: CycleDirection,
    CycleMoveToMonitor: CycleDirection,
    CycleMonitor: CycleDirection,
    CycleWorkspace: CycleDirection,
    Stack: OperationDirection,
    CycleStack: CycleDirection,
    FlipLayout: Axis,
    ChangeLayout: DefaultLayout,
    WatchConfiguration: BooleanState,
    MouseFollowsFocus: BooleanState,
    Query: StateQuery,
    WindowHidingBehaviour: HidingBehaviour,
    CrossMonitorMoveBehaviour: MoveBehaviour,
    UnmanagedWindowOperationBehaviour: OperationBehaviour,
}

macro_rules! gen_target_subcommand_args {
    // SubCommand Pattern
    ( $( $name:ident ),+ $(,)? ) => {
        $(
            #[derive(clap::Parser, derive_ahk::AhkFunction)]
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
    MoveWorkspaceToMonitor,
}

macro_rules! gen_named_target_subcommand_args {
    // SubCommand Pattern
    ( $( $name:ident ),+ $(,)? ) => {
        $(
            #[derive(clap::Parser, derive_ahk::AhkFunction)]
            pub struct $name {
                /// Target workspace name
                workspace: String,
            }
        )+
    };
}

gen_named_target_subcommand_args! {
    MoveToNamedWorkspace,
    SendToNamedWorkspace,
    FocusNamedWorkspace,
    ClearNamedWorkspaceLayoutRules
}

// Thanks to @danielhenrymantilla for showing me how to use cfg_attr with an optional argument like
// this on the Rust Programming Language Community Discord Server
macro_rules! gen_workspace_subcommand_args {
    // Workspace Property: #[enum] Value Enum (if the value is an Enum)
    // Workspace Property: Value Type (if the value is anything else)
    ( $( $name:ident: $(#[enum] $(@$value_enum:tt)?)? $value:ty ),+ $(,)? ) => (
        paste! {
            $(
                #[derive(clap::Parser, derive_ahk::AhkFunction)]
                pub struct [<Workspace $name>] {
                    /// Monitor index (zero-indexed)
                    monitor: usize,

                    /// Workspace index on the specified monitor (zero-indexed)
                    workspace: usize,

                    $(#[clap(value_enum)] $($value_enum)?)?
                    #[cfg_attr(
                        all($(FALSE $($value_enum)?)?),
                        doc = ""$name " of the workspace as a "$value ""
                    )]
                    value: $value,
                }
            )+
        }
    )
}

gen_workspace_subcommand_args! {
    Name: String,
    Layout: #[enum] DefaultLayout,
    Tiling: #[enum] BooleanState,
}

macro_rules! gen_named_workspace_subcommand_args {
    // Workspace Property: #[enum] Value Enum (if the value is an Enum)
    // Workspace Property: Value Type (if the value is anything else)
    ( $( $name:ident: $(#[enum] $(@$value_enum:tt)?)? $value:ty ),+ $(,)? ) => (
        paste! {
            $(
                #[derive(clap::Parser, derive_ahk::AhkFunction)]
                pub struct [<NamedWorkspace $name>] {
                    /// Target workspace name
                    workspace: String,

                    $(#[clap(value_enum)] $($value_enum)?)?
                    #[cfg_attr(
                        all($(FALSE $($value_enum)?)?),
                        doc = ""$name " of the workspace as a "$value ""
                    )]
                    value: $value,
                }
            )+
        }
    )
}

gen_named_workspace_subcommand_args! {
    Layout: #[enum] DefaultLayout,
    Tiling: #[enum] BooleanState,
}

#[derive(Parser, AhkFunction)]
pub struct ClearWorkspaceLayoutRules {
    /// Monitor index (zero-indexed)
    monitor: usize,

    /// Workspace index on the specified monitor (zero-indexed)
    workspace: usize,
}

#[derive(Parser, AhkFunction)]
pub struct WorkspaceCustomLayout {
    /// Monitor index (zero-indexed)
    monitor: usize,

    /// Workspace index on the specified monitor (zero-indexed)
    workspace: usize,

    /// JSON or YAML file from which the custom layout definition should be loaded
    path: String,
}

#[derive(Parser, AhkFunction)]
pub struct NamedWorkspaceCustomLayout {
    /// Target workspace name
    workspace: String,

    /// JSON or YAML file from which the custom layout definition should be loaded
    path: String,
}

#[derive(Parser, AhkFunction)]
pub struct WorkspaceLayoutRule {
    /// Monitor index (zero-indexed)
    monitor: usize,

    /// Workspace index on the specified monitor (zero-indexed)
    workspace: usize,

    /// The number of window containers on-screen required to trigger this layout rule
    at_container_count: usize,

    #[clap(value_enum)]
    layout: DefaultLayout,
}

#[derive(Parser, AhkFunction)]
pub struct NamedWorkspaceLayoutRule {
    /// Target workspace name
    workspace: String,

    /// The number of window containers on-screen required to trigger this layout rule
    at_container_count: usize,

    #[clap(value_enum)]
    layout: DefaultLayout,
}

#[derive(Parser, AhkFunction)]
pub struct WorkspaceCustomLayoutRule {
    /// Monitor index (zero-indexed)
    monitor: usize,

    /// Workspace index on the specified monitor (zero-indexed)
    workspace: usize,

    /// The number of window containers on-screen required to trigger this layout rule
    at_container_count: usize,

    /// JSON or YAML file from which the custom layout definition should be loaded
    path: String,
}

#[derive(Parser, AhkFunction)]
pub struct NamedWorkspaceCustomLayoutRule {
    /// Target workspace name
    workspace: String,

    /// The number of window containers on-screen required to trigger this layout rule
    at_container_count: usize,

    /// JSON or YAML file from which the custom layout definition should be loaded
    path: String,
}

#[derive(Parser, AhkFunction)]
struct Resize {
    #[clap(value_enum)]
    edge: OperationDirection,
    #[clap(value_enum)]
    sizing: Sizing,
}

#[derive(Parser, AhkFunction)]
struct ResizeAxis {
    #[clap(value_enum)]
    axis: Axis,
    #[clap(value_enum)]
    sizing: Sizing,
}

#[derive(Parser, AhkFunction)]
struct ResizeDelta {
    /// The delta of pixels by which to increase or decrease window dimensions when resizing
    pixels: i32,
}

#[derive(Parser, AhkFunction)]
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

#[derive(Parser, AhkFunction)]
struct GlobalWorkAreaOffset {
    /// Size of the left work area offset (set right to left * 2 to maintain right padding)
    left: i32,
    /// Size of the top work area offset (set bottom to the same value to maintain bottom padding)
    top: i32,
    /// Size of the right work area offset
    right: i32,
    /// Size of the bottom work area offset
    bottom: i32,
}

#[derive(Parser, AhkFunction)]
struct MonitorWorkAreaOffset {
    /// Monitor index (zero-indexed)
    monitor: usize,
    /// Size of the left work area offset (set right to left * 2 to maintain right padding)
    left: i32,
    /// Size of the top work area offset (set bottom to the same value to maintain bottom padding)
    top: i32,
    /// Size of the right work area offset
    right: i32,
    /// Size of the bottom work area offset
    bottom: i32,
}

#[derive(Parser, AhkFunction)]
struct MonitorIndexPreference {
    /// Preferred monitor index (zero-indexed)
    index_preference: usize,
    /// Left value of the monitor's size Rect
    left: i32,
    /// Top value of the monitor's size Rect
    top: i32,
    /// Right value of the monitor's size Rect
    right: i32,
    /// Bottom value of the monitor's size Rect
    bottom: i32,
}

#[derive(Parser, AhkFunction)]
struct EnsureWorkspaces {
    /// Monitor index (zero-indexed)
    monitor: usize,
    /// Number of desired workspaces
    workspace_count: usize,
}

#[derive(Parser, AhkFunction)]
struct EnsureNamedWorkspaces {
    /// Monitor index (zero-indexed)
    monitor: usize,
    /// Names of desired workspaces
    names: Vec<String>,
}

#[derive(Parser, AhkFunction)]
struct FocusMonitorWorkspace {
    /// Target monitor index (zero-indexed)
    target_monitor: usize,
    /// Workspace index on the target monitor (zero-indexed)
    target_workspace: usize,
}

#[derive(Parser, AhkFunction)]
pub struct SendToMonitorWorkspace {
    /// Target monitor index (zero-indexed)
    target_monitor: usize,
    /// Workspace index on the target monitor (zero-indexed)
    target_workspace: usize,
}

macro_rules! gen_padding_subcommand_args {
    // SubCommand Pattern
    ( $( $name:ident ),+ $(,)? ) => {
        $(
            #[derive(clap::Parser, derive_ahk::AhkFunction)]
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

macro_rules! gen_named_padding_subcommand_args {
    // SubCommand Pattern
    ( $( $name:ident ),+ $(,)? ) => {
        $(
            #[derive(clap::Parser, derive_ahk::AhkFunction)]
            pub struct $name {
                /// Target workspace name
                workspace: String,

                /// Pixels to pad with as an integer
                size: i32,
            }
        )+
    };
}

gen_named_padding_subcommand_args! {
    NamedWorkspaceContainerPadding,
    NamedWorkspacePadding,
}

macro_rules! gen_padding_adjustment_subcommand_args {
    // SubCommand Pattern
    ( $( $name:ident ),+ $(,)? ) => {
        $(
            #[derive(clap::Parser, derive_ahk::AhkFunction)]
            pub struct $name {
                #[clap(value_enum)]
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
            #[derive(clap::Parser, derive_ahk::AhkFunction)]
            pub struct $name {
                #[clap(value_enum)]
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
    IdentifyLayeredApplication,
    IdentifyObjectNameChangeApplication,
    IdentifyBorderOverflowApplication,
}

#[derive(Parser, AhkFunction)]
struct InitialWorkspaceRule {
    #[clap(value_enum)]
    identifier: ApplicationIdentifier,
    /// Identifier as a string
    id: String,
    /// Monitor index (zero-indexed)
    monitor: usize,
    /// Workspace index on the specified monitor (zero-indexed)
    workspace: usize,
}

#[derive(Parser, AhkFunction)]
struct InitialNamedWorkspaceRule {
    #[clap(value_enum)]
    identifier: ApplicationIdentifier,
    /// Identifier as a string
    id: String,
    /// Name of a workspace
    workspace: String,
}

#[derive(Parser, AhkFunction)]
struct WorkspaceRule {
    #[clap(value_enum)]
    identifier: ApplicationIdentifier,
    /// Identifier as a string
    id: String,
    /// Monitor index (zero-indexed)
    monitor: usize,
    /// Workspace index on the specified monitor (zero-indexed)
    workspace: usize,
}

#[derive(Parser, AhkFunction)]
struct NamedWorkspaceRule {
    #[clap(value_enum)]
    identifier: ApplicationIdentifier,
    /// Identifier as a string
    id: String,
    /// Name of a workspace
    workspace: String,
}

#[derive(Parser, AhkFunction)]
struct ToggleFocusFollowsMouse {
    #[clap(value_enum, short, long, default_value = "windows")]
    implementation: FocusFollowsMouseImplementation,
}

#[derive(Parser, AhkFunction)]
struct FocusFollowsMouse {
    #[clap(value_enum, short, long, default_value = "windows")]
    implementation: FocusFollowsMouseImplementation,
    #[clap(value_enum)]
    boolean_state: BooleanState,
}

#[derive(Parser, AhkFunction)]
struct ActiveWindowBorder {
    #[clap(value_enum)]
    boolean_state: BooleanState,
}

#[derive(Parser, AhkFunction)]
struct ActiveWindowBorderColour {
    #[clap(value_enum, short, long, default_value = "single")]
    window_kind: WindowKind,
    /// Red
    r: u32,
    /// Green
    g: u32,
    /// Blue
    b: u32,
}

#[derive(Parser, AhkFunction)]
struct ActiveWindowBorderWidth {
    /// Desired width of the active window border
    width: i32,
}

#[derive(Parser, AhkFunction)]
struct ActiveWindowBorderOffset {
    /// Desired offset of the active window border
    offset: i32,
}

#[derive(Parser, AhkFunction)]
struct Start {
    /// Allow the use of komorebi's custom focus-follows-mouse implementation
    #[clap(action, short, long = "ffm")]
    ffm: bool,
    /// Wait for 'komorebic complete-configuration' to be sent before processing events
    #[clap(action, short, long)]
    await_configuration: bool,
    /// Start a TCP server on the given port to allow the direct sending of SocketMessages
    #[clap(action, short, long)]
    tcp_port: Option<usize>,
}

#[derive(Parser, AhkFunction)]
struct SaveResize {
    /// File to which the resize layout dimensions should be saved
    path: String,
}

#[derive(Parser, AhkFunction)]
struct LoadResize {
    /// File from which the resize layout dimensions should be loaded
    path: String,
}

#[derive(Parser, AhkFunction)]
struct LoadCustomLayout {
    /// JSON or YAML file from which the custom layout definition should be loaded
    path: String,
}

#[derive(Parser, AhkFunction)]
struct Subscribe {
    /// Name of the pipe to send event notifications to (without "\\.\pipe\" prepended)
    named_pipe: String,
}

#[derive(Parser, AhkFunction)]
struct Unsubscribe {
    /// Name of the pipe to stop sending event notifications to (without "\\.\pipe\" prepended)
    named_pipe: String,
}

#[derive(Parser, AhkFunction)]
struct AhkAppSpecificConfiguration {
    /// YAML file from which the application-specific configurations should be loaded
    path: String,
    /// Optional YAML file of overrides to apply over the first file
    override_path: Option<String>,
}

#[derive(Parser, AhkFunction)]
struct PwshAppSpecificConfiguration {
    /// YAML file from which the application-specific configurations should be loaded
    path: String,
    /// Optional YAML file of overrides to apply over the first file
    override_path: Option<String>,
}

#[derive(Parser, AhkFunction)]
struct FormatAppSpecificConfiguration {
    /// YAML file from which the application-specific configurations should be loaded
    path: String,
}

#[derive(Parser, AhkFunction)]
struct AltFocusHack {
    #[clap(value_enum)]
    boolean_state: BooleanState,
}

#[derive(Parser)]
#[clap(author, about, version)]
struct Opts {
    #[clap(subcommand)]
    subcmd: SubCommand,
}

#[derive(Parser, AhkLibrary)]
enum SubCommand {
    /// Start komorebi.exe as a background process
    Start(Start),
    /// Stop the komorebi.exe process and restore all hidden windows
    Stop,
    /// Output various important komorebi-related environment values
    Check,
    /// Show a JSON representation of the current window manager state
    State,
    /// Query the current window manager state
    #[clap(arg_required_else_help = true)]
    Query(Query),
    /// Subscribe to komorebi events
    #[clap(arg_required_else_help = true)]
    Subscribe(Subscribe),
    /// Unsubscribe from komorebi events
    #[clap(arg_required_else_help = true)]
    Unsubscribe(Unsubscribe),
    /// Tail komorebi.exe's process logs (cancel with Ctrl-C)
    Log,
    /// Quicksave the current resize layout dimensions
    #[clap(alias = "quick-save")]
    QuickSaveResize,
    /// Load the last quicksaved resize layout dimensions
    #[clap(alias = "quick-load")]
    QuickLoadResize,
    /// Save the current resize layout dimensions to a file
    #[clap(arg_required_else_help = true)]
    #[clap(alias = "save")]
    SaveResize(SaveResize),
    /// Load the resize layout dimensions from a file
    #[clap(arg_required_else_help = true)]
    #[clap(alias = "load")]
    LoadResize(LoadResize),
    /// Change focus to the window in the specified direction
    #[clap(arg_required_else_help = true)]
    Focus(Focus),
    /// Move the focused window in the specified direction
    #[clap(arg_required_else_help = true)]
    Move(Move),
    /// Minimize the focused window
    Minimize,
    /// Close the focused window
    Close,
    /// Forcibly focus the window at the cursor with a left mouse click
    ForceFocus,
    /// Change focus to the window in the specified cycle direction
    #[clap(arg_required_else_help = true)]
    CycleFocus(CycleFocus),
    /// Move the focused window in the specified cycle direction
    #[clap(arg_required_else_help = true)]
    CycleMove(CycleMove),
    /// Stack the focused window in the specified direction
    #[clap(arg_required_else_help = true)]
    Stack(Stack),
    /// Resize the focused window in the specified direction
    #[clap(arg_required_else_help = true)]
    #[clap(alias = "resize")]
    ResizeEdge(Resize),
    /// Resize the focused window or primary column along the specified axis
    #[clap(arg_required_else_help = true)]
    ResizeAxis(ResizeAxis),
    /// Unstack the focused window
    Unstack,
    /// Cycle the focused stack in the specified cycle direction
    #[clap(arg_required_else_help = true)]
    CycleStack(CycleStack),
    /// Move the focused window to the specified monitor
    #[clap(arg_required_else_help = true)]
    MoveToMonitor(MoveToMonitor),
    /// Move the focused window to the monitor in the given cycle direction
    #[clap(arg_required_else_help = true)]
    CycleMoveToMonitor(CycleMoveToMonitor),
    /// Move the focused window to the specified workspace
    #[clap(arg_required_else_help = true)]
    MoveToWorkspace(MoveToWorkspace),
    /// Move the focused window to the specified workspace
    #[clap(arg_required_else_help = true)]
    MoveToNamedWorkspace(MoveToNamedWorkspace),
    /// Move the focused window to the workspace in the given cycle direction
    #[clap(arg_required_else_help = true)]
    CycleMoveToWorkspace(CycleMoveToWorkspace),
    /// Send the focused window to the specified monitor
    #[clap(arg_required_else_help = true)]
    SendToMonitor(SendToMonitor),
    /// Send the focused window to the monitor in the given cycle direction
    #[clap(arg_required_else_help = true)]
    CycleSendToMonitor(CycleSendToMonitor),
    /// Send the focused window to the specified workspace
    #[clap(arg_required_else_help = true)]
    SendToWorkspace(SendToWorkspace),
    /// Send the focused window to the specified workspace
    #[clap(arg_required_else_help = true)]
    SendToNamedWorkspace(SendToNamedWorkspace),
    /// Send the focused window to the workspace in the given cycle direction
    #[clap(arg_required_else_help = true)]
    CycleSendToWorkspace(CycleSendToWorkspace),
    /// Send the focused window to the specified monitor workspace
    #[clap(arg_required_else_help = true)]
    SendToMonitorWorkspace(SendToMonitorWorkspace),
    /// Focus the specified monitor
    #[clap(arg_required_else_help = true)]
    FocusMonitor(FocusMonitor),
    /// Focus the specified workspace on the focused monitor
    #[clap(arg_required_else_help = true)]
    FocusWorkspace(FocusWorkspace),
    /// Focus the specified workspace on the target monitor
    #[clap(arg_required_else_help = true)]
    FocusMonitorWorkspace(FocusMonitorWorkspace),
    /// Focus the specified workspace
    #[clap(arg_required_else_help = true)]
    FocusNamedWorkspace(FocusNamedWorkspace),
    /// Focus the monitor in the given cycle direction
    #[clap(arg_required_else_help = true)]
    CycleMonitor(CycleMonitor),
    /// Focus the workspace in the given cycle direction
    #[clap(arg_required_else_help = true)]
    CycleWorkspace(CycleWorkspace),
    /// Move the focused workspace to the specified monitor
    #[clap(arg_required_else_help = true)]
    MoveWorkspaceToMonitor(MoveWorkspaceToMonitor),
    /// Create and append a new workspace on the focused monitor
    NewWorkspace,
    /// Set the resize delta (used by resize-edge and resize-axis)
    #[clap(arg_required_else_help = true)]
    ResizeDelta(ResizeDelta),
    /// Set the invisible border dimensions around each window
    #[clap(arg_required_else_help = true)]
    InvisibleBorders(InvisibleBorders),
    /// Set offsets to exclude parts of the work area from tiling
    #[clap(arg_required_else_help = true)]
    GlobalWorkAreaOffset(GlobalWorkAreaOffset),
    /// Set offsets for a monitor to exclude parts of the work area from tiling
    #[clap(arg_required_else_help = true)]
    MonitorWorkAreaOffset(MonitorWorkAreaOffset),
    /// Adjust container padding on the focused workspace
    #[clap(arg_required_else_help = true)]
    AdjustContainerPadding(AdjustContainerPadding),
    /// Adjust workspace padding on the focused workspace
    #[clap(arg_required_else_help = true)]
    AdjustWorkspacePadding(AdjustWorkspacePadding),
    /// Set the layout on the focused workspace
    #[clap(arg_required_else_help = true)]
    ChangeLayout(ChangeLayout),
    /// Load a custom layout from file for the focused workspace
    #[clap(arg_required_else_help = true)]
    LoadCustomLayout(LoadCustomLayout),
    /// Flip the layout on the focused workspace (BSP only)
    #[clap(arg_required_else_help = true)]
    FlipLayout(FlipLayout),
    /// Promote the focused window to the top of the tree
    Promote,
    /// Promote the user focus to the top of the tree
    PromoteFocus,
    /// Force the retiling of all managed windows
    Retile,
    /// Set the monitor index preference for a monitor identified using its size
    #[clap(arg_required_else_help = true)]
    MonitorIndexPreference(MonitorIndexPreference),
    /// Create at least this many workspaces for the specified monitor
    #[clap(arg_required_else_help = true)]
    EnsureWorkspaces(EnsureWorkspaces),
    /// Create these many named workspaces for the specified monitor
    #[clap(arg_required_else_help = true)]
    EnsureNamedWorkspaces(EnsureNamedWorkspaces),
    /// Set the container padding for the specified workspace
    #[clap(arg_required_else_help = true)]
    ContainerPadding(ContainerPadding),
    /// Set the container padding for the specified workspace
    #[clap(arg_required_else_help = true)]
    NamedWorkspaceContainerPadding(NamedWorkspaceContainerPadding),
    /// Set the workspace padding for the specified workspace
    #[clap(arg_required_else_help = true)]
    WorkspacePadding(WorkspacePadding),
    /// Set the workspace padding for the specified workspace
    #[clap(arg_required_else_help = true)]
    NamedWorkspacePadding(NamedWorkspacePadding),
    /// Set the layout for the specified workspace
    #[clap(arg_required_else_help = true)]
    WorkspaceLayout(WorkspaceLayout),
    /// Set the layout for the specified workspace
    #[clap(arg_required_else_help = true)]
    NamedWorkspaceLayout(NamedWorkspaceLayout),
    /// Set a custom layout for the specified workspace
    #[clap(arg_required_else_help = true)]
    WorkspaceCustomLayout(WorkspaceCustomLayout),
    /// Set a custom layout for the specified workspace
    #[clap(arg_required_else_help = true)]
    NamedWorkspaceCustomLayout(NamedWorkspaceCustomLayout),
    /// Add a dynamic layout rule for the specified workspace
    #[clap(arg_required_else_help = true)]
    WorkspaceLayoutRule(WorkspaceLayoutRule),
    /// Add a dynamic layout rule for the specified workspace
    #[clap(arg_required_else_help = true)]
    NamedWorkspaceLayoutRule(NamedWorkspaceLayoutRule),
    /// Add a dynamic custom layout for the specified workspace
    #[clap(arg_required_else_help = true)]
    WorkspaceCustomLayoutRule(WorkspaceCustomLayoutRule),
    /// Add a dynamic custom layout for the specified workspace
    #[clap(arg_required_else_help = true)]
    NamedWorkspaceCustomLayoutRule(NamedWorkspaceCustomLayoutRule),
    /// Clear all dynamic layout rules for the specified workspace
    #[clap(arg_required_else_help = true)]
    ClearWorkspaceLayoutRules(ClearWorkspaceLayoutRules),
    /// Clear all dynamic layout rules for the specified workspace
    #[clap(arg_required_else_help = true)]
    ClearNamedWorkspaceLayoutRules(ClearNamedWorkspaceLayoutRules),
    /// Enable or disable window tiling for the specified workspace
    #[clap(arg_required_else_help = true)]
    WorkspaceTiling(WorkspaceTiling),
    /// Enable or disable window tiling for the specified workspace
    #[clap(arg_required_else_help = true)]
    NamedWorkspaceTiling(NamedWorkspaceTiling),
    /// Set the workspace name for the specified workspace
    #[clap(arg_required_else_help = true)]
    WorkspaceName(WorkspaceName),
    /// Toggle the behaviour for new windows (stacking or dynamic tiling)
    ToggleWindowContainerBehaviour,
    /// Toggle window tiling on the focused workspace
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
    #[clap(arg_required_else_help = true)]
    WatchConfiguration(WatchConfiguration),
    /// Signal that the final configuration option has been sent
    CompleteConfiguration,
    /// Enable or disable a hack simulating ALT key presses to ensure focus changes succeed
    #[clap(arg_required_else_help = true)]
    AltFocusHack(AltFocusHack),
    /// Set the window behaviour when switching workspaces / cycling stacks
    #[clap(arg_required_else_help = true)]
    WindowHidingBehaviour(WindowHidingBehaviour),
    /// Set the behaviour when moving windows across monitor boundaries
    #[clap(arg_required_else_help = true)]
    CrossMonitorMoveBehaviour(CrossMonitorMoveBehaviour),
    /// Toggle the behaviour when moving windows across monitor boundaries
    ToggleCrossMonitorMoveBehaviour,
    /// Set the operation behaviour when the focused window is not managed
    #[clap(arg_required_else_help = true)]
    UnmanagedWindowOperationBehaviour(UnmanagedWindowOperationBehaviour),
    /// Add a rule to always float the specified application
    #[clap(arg_required_else_help = true)]
    FloatRule(FloatRule),
    /// Add a rule to always manage the specified application
    #[clap(arg_required_else_help = true)]
    ManageRule(ManageRule),
    /// Add a rule to associate an application with a workspace on first show
    #[clap(arg_required_else_help = true)]
    InitialWorkspaceRule(InitialWorkspaceRule),
    /// Add a rule to associate an application with a named workspace on first show
    #[clap(arg_required_else_help = true)]
    InitialNamedWorkspaceRule(InitialNamedWorkspaceRule),
    /// Add a rule to associate an application with a workspace
    #[clap(arg_required_else_help = true)]
    WorkspaceRule(WorkspaceRule),
    /// Add a rule to associate an application with a named workspace
    #[clap(arg_required_else_help = true)]
    NamedWorkspaceRule(NamedWorkspaceRule),
    /// Identify an application that sends EVENT_OBJECT_NAMECHANGE on launch
    #[clap(arg_required_else_help = true)]
    IdentifyObjectNameChangeApplication(IdentifyObjectNameChangeApplication),
    /// Identify an application that closes to the system tray
    #[clap(arg_required_else_help = true)]
    IdentifyTrayApplication(IdentifyTrayApplication),
    /// Identify an application that has WS_EX_LAYERED, but should still be managed
    #[clap(arg_required_else_help = true)]
    IdentifyLayeredApplication(IdentifyLayeredApplication),
    /// Identify an application that has overflowing borders
    #[clap(arg_required_else_help = true)]
    #[clap(alias = "identify-border-overflow")]
    IdentifyBorderOverflowApplication(IdentifyBorderOverflowApplication),
    /// Enable or disable the active window border
    #[clap(arg_required_else_help = true)]
    ActiveWindowBorder(ActiveWindowBorder),
    /// Set the colour for the active window border
    #[clap(arg_required_else_help = true)]
    ActiveWindowBorderColour(ActiveWindowBorderColour),
    /// Set the width for the active window border
    #[clap(arg_required_else_help = true)]
    ActiveWindowBorderWidth(ActiveWindowBorderWidth),
    /// Set the offset for the active window border
    #[clap(arg_required_else_help = true)]
    ActiveWindowBorderOffset(ActiveWindowBorderOffset),
    /// Enable or disable focus follows mouse for the operating system
    #[clap(arg_required_else_help = true)]
    FocusFollowsMouse(FocusFollowsMouse),
    /// Toggle focus follows mouse for the operating system
    #[clap(arg_required_else_help = true)]
    ToggleFocusFollowsMouse(ToggleFocusFollowsMouse),
    /// Enable or disable mouse follows focus on all workspaces
    #[clap(arg_required_else_help = true)]
    MouseFollowsFocus(MouseFollowsFocus),
    /// Toggle mouse follows focus on all workspaces
    ToggleMouseFollowsFocus,
    /// Generate a library of AutoHotKey helper functions
    AhkLibrary,
    /// Generate common app-specific configurations and fixes to use in komorebi.ahk
    #[clap(arg_required_else_help = true)]
    #[clap(alias = "ahk-asc")]
    AhkAppSpecificConfiguration(AhkAppSpecificConfiguration),
    /// Generate common app-specific configurations and fixes in a PowerShell script
    #[clap(arg_required_else_help = true)]
    #[clap(alias = "pwsh-asc")]
    PwshAppSpecificConfiguration(PwshAppSpecificConfiguration),
    /// Format a YAML file for use with the 'ahk-app-specific-configuration' command
    #[clap(arg_required_else_help = true)]
    #[clap(alias = "fmt-asc")]
    FormatAppSpecificConfiguration(FormatAppSpecificConfiguration),
    /// Generate a JSON Schema of subscription notifications
    NotificationSchema,
    /// Generate a JSON Schema of socket messages
    SocketSchema,
}

pub fn send_message(bytes: &[u8]) -> Result<()> {
    let socket = DATA_DIR.join("komorebi.sock");
    let mut stream = UnixStream::connect(&socket)?;
    Ok(stream.write_all(bytes)?)
}

#[allow(clippy::too_many_lines)]
fn main() -> Result<()> {
    let opts: Opts = Opts::parse();

    match opts.subcmd {
        SubCommand::Check => {
            let home = HOME_DIR.clone();
            let home_lossy_string = home.to_string_lossy();
            if HAS_CUSTOM_CONFIG_HOME.load(Ordering::SeqCst) {
                println!("KOMOREBI_CONFIG_HOME detected: {home_lossy_string}\n");
            } else {
                println!(
                    "No KOMOREBI_CONFIG_HOME detected, defaulting to {}\n",
                    dirs::home_dir()
                        .expect("could not find home dir")
                        .to_string_lossy()
                );
            }

            println!("Looking for configuration files in {home_lossy_string}\n");

            let mut config_pwsh = home.clone();
            config_pwsh.push("komorebi.ps1");

            let mut config_ahk = home.clone();
            config_ahk.push("komorebi.ahk");

            if config_pwsh.exists() {
                println!("Found komorebi.ps1; this file will be autoloaded by komorebi\n");
                let mut config_whkd = dirs::home_dir().expect("could not find home dir");
                config_whkd.push(".config");
                config_whkd.push("whkdrc");
                if config_whkd.exists() {
                    println!("Found ~/.config/whkdrc; key bindings will be loaded from here when whkd is started\n");
                } else {
                    println!("No ~/.config/whkdrc found; you may not be able to control komorebi with your keyboard\n");
                }
            } else if config_ahk.exists() {
                println!("Found komorebi.ahk; this file will be autoloaded by komorebi\n");
            } else {
                println!("No komorebi configuration found in {home_lossy_string}\n");
                println!("If running 'komorebic start --await-configuration', you will manually have to call the following command to begin tiling: komorebic complete-configuration\n");
            }
        }
        SubCommand::AhkLibrary => {
            let mut library = HOME_DIR.clone();
            library.push("komorebic.lib.ahk");
            let mut file = OpenOptions::new()
                .write(true)
                .create(true)
                .truncate(true)
                .open(library.clone())?;

            let output: String = SubCommand::generate_ahk_library();
            let fixed_output = output.replace("%id%", "\"%id%\"");

            file.write_all(fixed_output.as_bytes())?;

            println!(
                "\nAHKv1 helper library for komorebic written to {}",
                library.to_str().ok_or_else(|| anyhow!(
                    "could not find the path to the generated ahk lib file"
                ))?
            );

            println!("\nYou can convert this file to AHKv2 syntax using https://github.com/mmikeww/AHK-v2-script-converter");

            println!(
                "\nYou can include the converted library at the top of your komorebi.ahk config with this line:"
            );

            println!("\n#Include komorebic.lib.ahk");
        }
        SubCommand::Log => {
            let mut color_log = std::env::temp_dir();
            color_log.push("komorebi.log");
            let file = TailedFile::new(File::open(color_log)?);
            let locked = file.lock();
            #[allow(clippy::significant_drop_in_scrutinee)]
            for line in locked.lines().flatten() {
                println!("{}", line);
            }
        }
        SubCommand::Focus(arg) => {
            send_message(&SocketMessage::FocusWindow(arg.operation_direction).as_bytes()?)?;
        }
        SubCommand::ForceFocus => {
            send_message(&SocketMessage::ForceFocus.as_bytes()?)?;
        }
        SubCommand::Close => {
            send_message(&SocketMessage::Close.as_bytes()?)?;
        }
        SubCommand::Minimize => {
            send_message(&SocketMessage::Minimize.as_bytes()?)?;
        }
        SubCommand::Promote => {
            send_message(&SocketMessage::Promote.as_bytes()?)?;
        }
        SubCommand::PromoteFocus => {
            send_message(&SocketMessage::PromoteFocus.as_bytes()?)?;
        }
        SubCommand::TogglePause => {
            send_message(&SocketMessage::TogglePause.as_bytes()?)?;
        }
        SubCommand::Retile => {
            send_message(&SocketMessage::Retile.as_bytes()?)?;
        }
        SubCommand::Move(arg) => {
            send_message(&SocketMessage::MoveWindow(arg.operation_direction).as_bytes()?)?;
        }
        SubCommand::CycleFocus(arg) => {
            send_message(&SocketMessage::CycleFocusWindow(arg.cycle_direction).as_bytes()?)?;
        }
        SubCommand::CycleMove(arg) => {
            send_message(&SocketMessage::CycleMoveWindow(arg.cycle_direction).as_bytes()?)?;
        }
        SubCommand::MoveToMonitor(arg) => {
            send_message(&SocketMessage::MoveContainerToMonitorNumber(arg.target).as_bytes()?)?;
        }
        SubCommand::CycleMoveToMonitor(arg) => {
            send_message(
                &SocketMessage::CycleMoveContainerToMonitor(arg.cycle_direction).as_bytes()?,
            )?;
        }
        SubCommand::MoveToWorkspace(arg) => {
            send_message(&SocketMessage::MoveContainerToWorkspaceNumber(arg.target).as_bytes()?)?;
        }
        SubCommand::MoveToNamedWorkspace(arg) => {
            send_message(&SocketMessage::MoveContainerToNamedWorkspace(arg.workspace).as_bytes()?)?;
        }
        SubCommand::CycleMoveToWorkspace(arg) => {
            send_message(
                &SocketMessage::CycleMoveContainerToWorkspace(arg.cycle_direction).as_bytes()?,
            )?;
        }
        SubCommand::SendToMonitor(arg) => {
            send_message(&SocketMessage::SendContainerToMonitorNumber(arg.target).as_bytes()?)?;
        }
        SubCommand::CycleSendToMonitor(arg) => {
            send_message(
                &SocketMessage::CycleSendContainerToMonitor(arg.cycle_direction).as_bytes()?,
            )?;
        }
        SubCommand::SendToWorkspace(arg) => {
            send_message(&SocketMessage::SendContainerToWorkspaceNumber(arg.target).as_bytes()?)?;
        }
        SubCommand::SendToNamedWorkspace(arg) => {
            send_message(&SocketMessage::SendContainerToNamedWorkspace(arg.workspace).as_bytes()?)?;
        }
        SubCommand::CycleSendToWorkspace(arg) => {
            send_message(
                &SocketMessage::CycleSendContainerToWorkspace(arg.cycle_direction).as_bytes()?,
            )?;
        }
        SubCommand::SendToMonitorWorkspace(arg) => {
            send_message(
                &SocketMessage::SendContainerToMonitorWorkspaceNumber(
                    arg.target_monitor,
                    arg.target_workspace,
                )
                .as_bytes()?,
            )?;
        }
        SubCommand::MoveWorkspaceToMonitor(arg) => {
            send_message(&SocketMessage::MoveWorkspaceToMonitorNumber(arg.target).as_bytes()?)?;
        }
        SubCommand::InvisibleBorders(arg) => {
            send_message(
                &SocketMessage::InvisibleBorders(Rect {
                    left: arg.left,
                    top: arg.top,
                    right: arg.right,
                    bottom: arg.bottom,
                })
                .as_bytes()?,
            )?;
        }
        SubCommand::MonitorWorkAreaOffset(arg) => {
            send_message(
                &SocketMessage::MonitorWorkAreaOffset(
                    arg.monitor,
                    Rect {
                        left: arg.left,
                        top: arg.top,
                        right: arg.right,
                        bottom: arg.bottom,
                    },
                )
                .as_bytes()?,
            )?;
        }
        SubCommand::GlobalWorkAreaOffset(arg) => {
            send_message(
                &SocketMessage::WorkAreaOffset(Rect {
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
                &SocketMessage::ContainerPadding(arg.monitor, arg.workspace, arg.size)
                    .as_bytes()?,
            )?;
        }
        SubCommand::NamedWorkspaceContainerPadding(arg) => {
            send_message(
                &SocketMessage::NamedWorkspaceContainerPadding(arg.workspace, arg.size)
                    .as_bytes()?,
            )?;
        }
        SubCommand::WorkspacePadding(arg) => {
            send_message(
                &SocketMessage::WorkspacePadding(arg.monitor, arg.workspace, arg.size)
                    .as_bytes()?,
            )?;
        }
        SubCommand::NamedWorkspacePadding(arg) => {
            send_message(
                &SocketMessage::NamedWorkspacePadding(arg.workspace, arg.size).as_bytes()?,
            )?;
        }
        SubCommand::AdjustWorkspacePadding(arg) => {
            send_message(
                &SocketMessage::AdjustWorkspacePadding(arg.sizing, arg.adjustment).as_bytes()?,
            )?;
        }
        SubCommand::AdjustContainerPadding(arg) => {
            send_message(
                &SocketMessage::AdjustContainerPadding(arg.sizing, arg.adjustment).as_bytes()?,
            )?;
        }
        SubCommand::ToggleFocusFollowsMouse(arg) => {
            send_message(&SocketMessage::ToggleFocusFollowsMouse(arg.implementation).as_bytes()?)?;
        }
        SubCommand::ToggleTiling => {
            send_message(&SocketMessage::ToggleTiling.as_bytes()?)?;
        }
        SubCommand::ToggleFloat => {
            send_message(&SocketMessage::ToggleFloat.as_bytes()?)?;
        }
        SubCommand::ToggleMonocle => {
            send_message(&SocketMessage::ToggleMonocle.as_bytes()?)?;
        }
        SubCommand::ToggleMaximize => {
            send_message(&SocketMessage::ToggleMaximize.as_bytes()?)?;
        }
        SubCommand::WorkspaceLayout(arg) => {
            send_message(
                &SocketMessage::WorkspaceLayout(arg.monitor, arg.workspace, arg.value)
                    .as_bytes()?,
            )?;
        }
        SubCommand::NamedWorkspaceLayout(arg) => {
            send_message(
                &SocketMessage::NamedWorkspaceLayout(arg.workspace, arg.value).as_bytes()?,
            )?;
        }
        SubCommand::WorkspaceCustomLayout(arg) => {
            send_message(
                &SocketMessage::WorkspaceLayoutCustom(
                    arg.monitor,
                    arg.workspace,
                    resolve_windows_path(&arg.path)?,
                )
                .as_bytes()?,
            )?;
        }
        SubCommand::NamedWorkspaceCustomLayout(arg) => {
            send_message(
                &SocketMessage::NamedWorkspaceLayoutCustom(
                    arg.workspace,
                    resolve_windows_path(&arg.path)?,
                )
                .as_bytes()?,
            )?;
        }
        SubCommand::WorkspaceLayoutRule(arg) => {
            send_message(
                &SocketMessage::WorkspaceLayoutRule(
                    arg.monitor,
                    arg.workspace,
                    arg.at_container_count,
                    arg.layout,
                )
                .as_bytes()?,
            )?;
        }
        SubCommand::NamedWorkspaceLayoutRule(arg) => {
            send_message(
                &SocketMessage::NamedWorkspaceLayoutRule(
                    arg.workspace,
                    arg.at_container_count,
                    arg.layout,
                )
                .as_bytes()?,
            )?;
        }
        SubCommand::WorkspaceCustomLayoutRule(arg) => {
            send_message(
                &SocketMessage::WorkspaceLayoutCustomRule(
                    arg.monitor,
                    arg.workspace,
                    arg.at_container_count,
                    resolve_windows_path(&arg.path)?,
                )
                .as_bytes()?,
            )?;
        }
        SubCommand::NamedWorkspaceCustomLayoutRule(arg) => {
            send_message(
                &SocketMessage::NamedWorkspaceLayoutCustomRule(
                    arg.workspace,
                    arg.at_container_count,
                    resolve_windows_path(&arg.path)?,
                )
                .as_bytes()?,
            )?;
        }
        SubCommand::ClearWorkspaceLayoutRules(arg) => {
            send_message(
                &SocketMessage::ClearWorkspaceLayoutRules(arg.monitor, arg.workspace).as_bytes()?,
            )?;
        }
        SubCommand::ClearNamedWorkspaceLayoutRules(arg) => {
            send_message(
                &SocketMessage::ClearNamedWorkspaceLayoutRules(arg.workspace).as_bytes()?,
            )?;
        }
        SubCommand::WorkspaceTiling(arg) => {
            send_message(
                &SocketMessage::WorkspaceTiling(arg.monitor, arg.workspace, arg.value.into())
                    .as_bytes()?,
            )?;
        }
        SubCommand::NamedWorkspaceTiling(arg) => {
            send_message(
                &SocketMessage::NamedWorkspaceTiling(arg.workspace, arg.value.into()).as_bytes()?,
            )?;
        }
        SubCommand::Start(arg) => {
            let mut buf: PathBuf;

            // The komorebi.ps1 shim will only exist in the Path if installed by Scoop
            let exec = if let Ok(output) = Command::new("where.exe").arg("komorebi.ps1").output() {
                let stdout = String::from_utf8(output.stdout)?;
                match stdout.trim() {
                    stdout if stdout.is_empty() => None,
                    // It's possible that a komorebi.ps1 config will be in %USERPROFILE% - ignore this
                    stdout if !stdout.contains("scoop") => None,
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
                    if arg.ffm | arg.await_configuration | arg.tcp_port.is_some() {
                        format!(
                            "Start-Process komorebi.exe -ArgumentList {} -WindowStyle hidden",
                            arg.tcp_port.map_or_else(
                                || if arg.ffm && arg.await_configuration {
                                    "'--ffm','--await-configuration'".to_string()
                                } else if arg.ffm {
                                    "'--ffm'".to_string()
                                } else {
                                    "'--await-configuration'".to_string()
                                },
                                |port| if arg.ffm {
                                    format!("'--ffm','--tcp-port={}'", port)
                                } else if arg.await_configuration {
                                    format!("'--await-configuration','--tcp-port={}'", port)
                                } else if arg.ffm && arg.await_configuration {
                                    format!("'--ffm','--await-configuration','--tcp-port={}'", port)
                                } else {
                                    format!("'--tcp-port={}'", port)
                                }
                            )
                        )
                    } else {
                        String::from("Start-Process komorebi.exe -WindowStyle hidden")
                    }
                },
                |exec| {
                    if arg.ffm | arg.await_configuration {
                        format!(
                            "Start-Process '{}' -ArgumentList {} -WindowStyle hidden",
                            exec,
                            arg.tcp_port.map_or_else(
                                || if arg.ffm && arg.await_configuration {
                                    "'--ffm','--await-configuration'".to_string()
                                } else if arg.ffm {
                                    "'--ffm'".to_string()
                                } else {
                                    "'--await-configuration'".to_string()
                                },
                                |port| if arg.ffm {
                                    format!("'--ffm','--tcp-port={}'", port)
                                } else if arg.await_configuration {
                                    format!("'--await-configuration','--tcp-port={}'", port)
                                } else if arg.ffm && arg.await_configuration {
                                    format!("'--ffm','--await-configuration','--tcp-port={}'", port)
                                } else {
                                    format!("'--tcp-port={}'", port)
                                }
                            )
                        )
                    } else {
                        format!("Start-Process '{}' -WindowStyle hidden", exec)
                    }
                },
            );

            let mut running = false;

            while !running {
                match powershell_script::run(&script) {
                    Ok(_) => {
                        println!("{}", script);
                    }
                    Err(error) => {
                        println!("Error: {}", error);
                    }
                }

                print!("Waiting for komorebi.exe to start...");
                std::thread::sleep(Duration::from_secs(3));

                let mut system = sysinfo::System::new_all();
                system.refresh_processes();

                if system.processes_by_name("komorebi.exe").next().is_some() {
                    println!("Started!");
                    running = true;
                } else {
                    println!("komorebi.exe did not start... Trying again");
                }
            }
        }
        SubCommand::Stop => {
            send_message(&SocketMessage::Stop.as_bytes()?)?;
        }
        SubCommand::FloatRule(arg) => {
            send_message(&SocketMessage::FloatRule(arg.identifier, arg.id).as_bytes()?)?;
        }
        SubCommand::ManageRule(arg) => {
            send_message(&SocketMessage::ManageRule(arg.identifier, arg.id).as_bytes()?)?;
        }
        SubCommand::InitialWorkspaceRule(arg) => {
            send_message(
                &SocketMessage::InitialWorkspaceRule(
                    arg.identifier,
                    arg.id,
                    arg.monitor,
                    arg.workspace,
                )
                .as_bytes()?,
            )?;
        }
        SubCommand::InitialNamedWorkspaceRule(arg) => {
            send_message(
                &SocketMessage::InitialNamedWorkspaceRule(arg.identifier, arg.id, arg.workspace)
                    .as_bytes()?,
            )?;
        }
        SubCommand::WorkspaceRule(arg) => {
            send_message(
                &SocketMessage::WorkspaceRule(arg.identifier, arg.id, arg.monitor, arg.workspace)
                    .as_bytes()?,
            )?;
        }
        SubCommand::NamedWorkspaceRule(arg) => {
            send_message(
                &SocketMessage::NamedWorkspaceRule(arg.identifier, arg.id, arg.workspace)
                    .as_bytes()?,
            )?;
        }
        SubCommand::Stack(arg) => {
            send_message(&SocketMessage::StackWindow(arg.operation_direction).as_bytes()?)?;
        }
        SubCommand::Unstack => {
            send_message(&SocketMessage::UnstackWindow.as_bytes()?)?;
        }
        SubCommand::CycleStack(arg) => {
            send_message(&SocketMessage::CycleStack(arg.cycle_direction).as_bytes()?)?;
        }
        SubCommand::ChangeLayout(arg) => {
            send_message(&SocketMessage::ChangeLayout(arg.default_layout).as_bytes()?)?;
        }
        SubCommand::LoadCustomLayout(arg) => {
            send_message(
                &SocketMessage::ChangeLayoutCustom(resolve_windows_path(&arg.path)?).as_bytes()?,
            )?;
        }
        SubCommand::FlipLayout(arg) => {
            send_message(&SocketMessage::FlipLayout(arg.axis).as_bytes()?)?;
        }
        SubCommand::FocusMonitor(arg) => {
            send_message(&SocketMessage::FocusMonitorNumber(arg.target).as_bytes()?)?;
        }
        SubCommand::FocusWorkspace(arg) => {
            send_message(&SocketMessage::FocusWorkspaceNumber(arg.target).as_bytes()?)?;
        }
        SubCommand::FocusMonitorWorkspace(arg) => {
            send_message(
                &SocketMessage::FocusMonitorWorkspaceNumber(
                    arg.target_monitor,
                    arg.target_workspace,
                )
                .as_bytes()?,
            )?;
        }
        SubCommand::FocusNamedWorkspace(arg) => {
            send_message(&SocketMessage::FocusNamedWorkspace(arg.workspace).as_bytes()?)?;
        }
        SubCommand::CycleMonitor(arg) => {
            send_message(&SocketMessage::CycleFocusMonitor(arg.cycle_direction).as_bytes()?)?;
        }
        SubCommand::CycleWorkspace(arg) => {
            send_message(&SocketMessage::CycleFocusWorkspace(arg.cycle_direction).as_bytes()?)?;
        }
        SubCommand::NewWorkspace => {
            send_message(&SocketMessage::NewWorkspace.as_bytes()?)?;
        }
        SubCommand::WorkspaceName(name) => {
            send_message(
                &SocketMessage::WorkspaceName(name.monitor, name.workspace, name.value)
                    .as_bytes()?,
            )?;
        }
        SubCommand::MonitorIndexPreference(arg) => {
            send_message(
                &SocketMessage::MonitorIndexPreference(
                    arg.index_preference,
                    arg.left,
                    arg.top,
                    arg.right,
                    arg.bottom,
                )
                .as_bytes()?,
            )?;
        }
        SubCommand::EnsureWorkspaces(workspaces) => {
            send_message(
                &SocketMessage::EnsureWorkspaces(workspaces.monitor, workspaces.workspace_count)
                    .as_bytes()?,
            )?;
        }
        SubCommand::EnsureNamedWorkspaces(arg) => {
            send_message(
                &SocketMessage::EnsureNamedWorkspaces(arg.monitor, arg.names).as_bytes()?,
            )?;
        }
        SubCommand::State => {
            let home = DATA_DIR.clone();
            let mut socket = home;
            socket.push("komorebic.sock");
            let socket = socket.as_path();

            match std::fs::remove_file(socket) {
                Ok(_) => {}
                Err(error) => match error.kind() {
                    // Doing this because ::exists() doesn't work reliably on Windows via IntelliJ
                    ErrorKind::NotFound => {}
                    _ => {
                        return Err(error.into());
                    }
                },
            };

            let listener = UnixListener::bind(socket)?;

            send_message(&SocketMessage::State.as_bytes()?)?;

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
            let home = DATA_DIR.clone();
            let mut socket = home;
            socket.push("komorebic.sock");
            let socket = socket.as_path();

            match std::fs::remove_file(socket) {
                Ok(_) => {}
                Err(error) => match error.kind() {
                    // Doing this because ::exists() doesn't work reliably on Windows via IntelliJ
                    ErrorKind::NotFound => {}
                    _ => {
                        return Err(error.into());
                    }
                },
            };

            let listener = UnixListener::bind(socket)?;

            send_message(&SocketMessage::Query(arg.state_query).as_bytes()?)?;

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
            let hwnd_json = DATA_DIR.join("komorebi.hwnd.json");

            let file = File::open(hwnd_json)?;
            let reader = BufReader::new(file);
            let hwnds: Vec<isize> = serde_json::from_reader(reader)?;

            for hwnd in hwnds {
                restore_window(HWND(hwnd));
            }
        }
        SubCommand::ResizeEdge(resize) => {
            send_message(&SocketMessage::ResizeWindowEdge(resize.edge, resize.sizing).as_bytes()?)?;
        }
        SubCommand::ResizeAxis(arg) => {
            send_message(&SocketMessage::ResizeWindowAxis(arg.axis, arg.sizing).as_bytes()?)?;
        }
        SubCommand::FocusFollowsMouse(arg) => {
            send_message(
                &SocketMessage::FocusFollowsMouse(arg.implementation, arg.boolean_state.into())
                    .as_bytes()?,
            )?;
        }
        SubCommand::ReloadConfiguration => {
            send_message(&SocketMessage::ReloadConfiguration.as_bytes()?)?;
        }
        SubCommand::WatchConfiguration(arg) => {
            send_message(&SocketMessage::WatchConfiguration(arg.boolean_state.into()).as_bytes()?)?;
        }
        SubCommand::CompleteConfiguration => {
            send_message(&SocketMessage::CompleteConfiguration.as_bytes()?)?;
        }
        SubCommand::AltFocusHack(arg) => {
            send_message(&SocketMessage::AltFocusHack(arg.boolean_state.into()).as_bytes()?)?;
        }
        SubCommand::IdentifyObjectNameChangeApplication(target) => {
            send_message(
                &SocketMessage::IdentifyObjectNameChangeApplication(target.identifier, target.id)
                    .as_bytes()?,
            )?;
        }
        SubCommand::IdentifyTrayApplication(target) => {
            send_message(
                &SocketMessage::IdentifyTrayApplication(target.identifier, target.id).as_bytes()?,
            )?;
        }
        SubCommand::IdentifyLayeredApplication(target) => {
            send_message(
                &SocketMessage::IdentifyLayeredApplication(target.identifier, target.id)
                    .as_bytes()?,
            )?;
        }
        SubCommand::IdentifyBorderOverflowApplication(target) => {
            send_message(
                &SocketMessage::IdentifyBorderOverflowApplication(target.identifier, target.id)
                    .as_bytes()?,
            )?;
        }
        SubCommand::Manage => {
            send_message(&SocketMessage::ManageFocusedWindow.as_bytes()?)?;
        }
        SubCommand::Unmanage => {
            send_message(&SocketMessage::UnmanageFocusedWindow.as_bytes()?)?;
        }
        SubCommand::QuickSaveResize => {
            send_message(&SocketMessage::QuickSave.as_bytes()?)?;
        }
        SubCommand::QuickLoadResize => {
            send_message(&SocketMessage::QuickLoad.as_bytes()?)?;
        }
        SubCommand::SaveResize(arg) => {
            send_message(&SocketMessage::Save(resolve_windows_path(&arg.path)?).as_bytes()?)?;
        }
        SubCommand::LoadResize(arg) => {
            send_message(&SocketMessage::Load(resolve_windows_path(&arg.path)?).as_bytes()?)?;
        }
        SubCommand::Subscribe(arg) => {
            send_message(&SocketMessage::AddSubscriber(arg.named_pipe).as_bytes()?)?;
        }
        SubCommand::Unsubscribe(arg) => {
            send_message(&SocketMessage::RemoveSubscriber(arg.named_pipe).as_bytes()?)?;
        }
        SubCommand::ToggleMouseFollowsFocus => {
            send_message(&SocketMessage::ToggleMouseFollowsFocus.as_bytes()?)?;
        }
        SubCommand::MouseFollowsFocus(arg) => {
            send_message(&SocketMessage::MouseFollowsFocus(arg.boolean_state.into()).as_bytes()?)?;
        }
        SubCommand::ActiveWindowBorder(arg) => {
            send_message(&SocketMessage::ActiveWindowBorder(arg.boolean_state.into()).as_bytes()?)?;
        }
        SubCommand::ActiveWindowBorderColour(arg) => {
            send_message(
                &SocketMessage::ActiveWindowBorderColour(arg.window_kind, arg.r, arg.g, arg.b)
                    .as_bytes()?,
            )?;
        }
        SubCommand::ActiveWindowBorderWidth(arg) => {
            send_message(&SocketMessage::ActiveWindowBorderWidth(arg.width).as_bytes()?)?;
        }
        SubCommand::ActiveWindowBorderOffset(arg) => {
            send_message(&SocketMessage::ActiveWindowBorderOffset(arg.offset).as_bytes()?)?;
        }
        SubCommand::ResizeDelta(arg) => {
            send_message(&SocketMessage::ResizeDelta(arg.pixels).as_bytes()?)?;
        }
        SubCommand::ToggleWindowContainerBehaviour => {
            send_message(&SocketMessage::ToggleWindowContainerBehaviour.as_bytes()?)?;
        }
        SubCommand::WindowHidingBehaviour(arg) => {
            send_message(&SocketMessage::WindowHidingBehaviour(arg.hiding_behaviour).as_bytes()?)?;
        }
        SubCommand::CrossMonitorMoveBehaviour(arg) => {
            send_message(
                &SocketMessage::CrossMonitorMoveBehaviour(arg.move_behaviour).as_bytes()?,
            )?;
        }
        SubCommand::ToggleCrossMonitorMoveBehaviour => {
            send_message(&SocketMessage::ToggleCrossMonitorMoveBehaviour.as_bytes()?)?;
        }
        SubCommand::UnmanagedWindowOperationBehaviour(arg) => {
            send_message(
                &SocketMessage::UnmanagedWindowOperationBehaviour(arg.operation_behaviour)
                    .as_bytes()?,
            )?;
        }
        SubCommand::AhkAppSpecificConfiguration(arg) => {
            let content = std::fs::read_to_string(resolve_windows_path(&arg.path)?)?;
            let lines = if let Some(override_path) = arg.override_path {
                let override_content =
                    std::fs::read_to_string(resolve_windows_path(&override_path)?)?;

                ApplicationConfigurationGenerator::generate_ahk(
                    &content,
                    Option::from(override_content.as_str()),
                )?
            } else {
                ApplicationConfigurationGenerator::generate_ahk(&content, None)?
            };

            let mut generated_config = HOME_DIR.clone();
            generated_config.push("komorebi.generated.ahk");
            let mut file = OpenOptions::new()
                .write(true)
                .create(true)
                .truncate(true)
                .open(generated_config.clone())?;

            file.write_all(lines.join("\n").as_bytes())?;

            println!(
                "\nApplication-specific generated configuration written to {}",
                generated_config.to_str().ok_or_else(|| anyhow!(
                    "could not find the path to the generated configuration file"
                ))?
            );

            println!(
                "\nYou can include the generated configuration at the top of your komorebi.ahk config with this line:"
            );

            println!("\n#Include %A_ScriptDir%\\komorebi.generated.ahk");
        }
        SubCommand::PwshAppSpecificConfiguration(arg) => {
            let content = std::fs::read_to_string(resolve_windows_path(&arg.path)?)?;
            let lines = if let Some(override_path) = arg.override_path {
                let override_content =
                    std::fs::read_to_string(resolve_windows_path(&override_path)?)?;

                ApplicationConfigurationGenerator::generate_pwsh(
                    &content,
                    Option::from(override_content.as_str()),
                )?
            } else {
                ApplicationConfigurationGenerator::generate_pwsh(&content, None)?
            };

            let mut generated_config = HOME_DIR.clone();
            generated_config.push("komorebi.generated.ps1");
            let mut file = OpenOptions::new()
                .write(true)
                .create(true)
                .truncate(true)
                .open(generated_config.clone())?;

            file.write_all(lines.join("\n").as_bytes())?;

            println!(
                "\nApplication-specific generated configuration written to {}",
                generated_config.to_str().ok_or_else(|| anyhow!(
                    "could not find the path to the generated configuration file"
                ))?
            );
        }
        SubCommand::FormatAppSpecificConfiguration(arg) => {
            let file_path = resolve_windows_path(&arg.path)?;
            let content = std::fs::read_to_string(&file_path)?;
            let formatted_content = ApplicationConfigurationGenerator::format(&content)?;

            let mut file = OpenOptions::new()
                .write(true)
                .create(true)
                .truncate(true)
                .open(file_path)?;

            file.write_all(formatted_content.as_bytes())?;

            println!("File successfully formatted for PRs to https://github.com/LGUG2Z/komorebi-application-specific-configuration");
        }
        SubCommand::NotificationSchema => {
            let home = DATA_DIR.clone();
            let mut socket = home;
            socket.push("komorebic.sock");
            let socket = socket.as_path();

            match std::fs::remove_file(socket) {
                Ok(_) => {}
                Err(error) => match error.kind() {
                    // Doing this because ::exists() doesn't work reliably on Windows via IntelliJ
                    ErrorKind::NotFound => {}
                    _ => {
                        return Err(error.into());
                    }
                },
            };

            send_message(&SocketMessage::NotificationSchema.as_bytes()?)?;

            let listener = UnixListener::bind(socket)?;
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
        SubCommand::SocketSchema => {
            let home = DATA_DIR.clone();
            let mut socket = home;
            socket.push("komorebic.sock");
            let socket = socket.as_path();

            match std::fs::remove_file(socket) {
                Ok(_) => {}
                Err(error) => match error.kind() {
                    // Doing this because ::exists() doesn't work reliably on Windows via IntelliJ
                    ErrorKind::NotFound => {}
                    _ => {
                        return Err(error.into());
                    }
                },
            };

            send_message(&SocketMessage::SocketSchema.as_bytes()?)?;

            let listener = UnixListener::bind(socket)?;
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
    }

    Ok(())
}

fn resolve_windows_path(raw_path: &str) -> Result<PathBuf> {
    let path = if raw_path.starts_with('~') {
        raw_path.replacen(
            '~',
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

    Ok(if parent.is_dir() {
        let file = full_path
            .components()
            .last()
            .ok_or_else(|| anyhow!("cannot parse filename"))?;

        let mut canonicalized = std::fs::canonicalize(parent)?;
        canonicalized.push(file);
        canonicalized
    } else {
        full_path
    })
}

fn show_window(hwnd: HWND, command: SHOW_WINDOW_CMD) {
    // BOOL is returned but does not signify whether or not the operation was succesful
    // https://docs.microsoft.com/en-us/windows/win32/api/winuser/nf-winuser-showwindow
    unsafe { ShowWindow(hwnd, command) };
}

fn restore_window(hwnd: HWND) {
    show_window(hwnd, SW_RESTORE);
}
