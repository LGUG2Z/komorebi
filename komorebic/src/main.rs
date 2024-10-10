#![warn(clippy::all)]
#![allow(clippy::missing_errors_doc, clippy::doc_markdown)]

use chrono::Utc;
use std::fs::File;
use std::fs::OpenOptions;
use std::io::BufRead;
use std::io::BufReader;
use std::io::Write;
use std::path::Path;
use std::path::PathBuf;
use std::process::Command;
use std::sync::atomic::AtomicBool;
use std::sync::atomic::Ordering;
use std::time::Duration;

use clap::CommandFactory;
use clap::Parser;
use clap::ValueEnum;
use color_eyre::eyre::anyhow;
use color_eyre::eyre::bail;
use color_eyre::Result;
use dirs::data_local_dir;
use fs_tail::TailedFile;
use komorebi_client::resolve_home_path;
use komorebi_client::send_message;
use komorebi_client::send_query;
use komorebi_client::ApplicationConfiguration;
use komorebi_client::Notification;
use lazy_static::lazy_static;
use miette::NamedSource;
use miette::Report;
use miette::SourceOffset;
use miette::SourceSpan;
use paste::paste;
use schemars::gen::SchemaSettings;
use schemars::schema_for;
use sysinfo::ProcessesToUpdate;
use which::which;
use windows::Win32::Foundation::HWND;
use windows::Win32::UI::WindowsAndMessaging::ShowWindow;
use windows::Win32::UI::WindowsAndMessaging::SHOW_WINDOW_CMD;
use windows::Win32::UI::WindowsAndMessaging::SW_RESTORE;

use komorebi_client::ApplicationConfigurationGenerator;
use komorebi_client::ApplicationIdentifier;
use komorebi_client::Axis;
use komorebi_client::CycleDirection;
use komorebi_client::DefaultLayout;
use komorebi_client::FocusFollowsMouseImplementation;
use komorebi_client::HidingBehaviour;
use komorebi_client::MoveBehaviour;
use komorebi_client::OperationBehaviour;
use komorebi_client::OperationDirection;
use komorebi_client::Rect;
use komorebi_client::Sizing;
use komorebi_client::SocketMessage;
use komorebi_client::StateQuery;
use komorebi_client::StaticConfig;
use komorebi_client::WindowKind;

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
                        "$Env:KOMOREBI_CONFIG_HOME is set to '{home_path}', which is not a valid directory",
                    );
                }
            },
        )
    };
    static ref DATA_DIR: PathBuf = dirs::data_local_dir()
        .expect("there is no local data directory")
        .join("komorebi");
    static ref WHKD_CONFIG_DIR: PathBuf = {
        std::env::var("WHKD_CONFIG_HOME").map_or_else(
            |_| {
                dirs::home_dir()
                    .expect("there is no home directory")
                    .join(".config")
            },
            |home_path| {
                let whkd_config_home = PathBuf::from(&home_path);

                assert!(
                    whkd_config_home.as_path().is_dir(),
                    "$Env:WHKD_CONFIG_HOME is set to '{}', which is not a valid directory",
                    whkd_config_home.to_string_lossy()
                );

                whkd_config_home
            },
        )
    };
}

shadow_rs::shadow!(build);

#[derive(thiserror::Error, Debug, miette::Diagnostic)]
#[error("{message}")]
#[diagnostic(code(komorebi::configuration), help("try fixing this syntax error"))]
struct ConfigurationError {
    message: String,
    #[source_code]
    src: NamedSource<String>,
    #[label("This bit here")]
    bad_bit: SourceSpan,
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
                #[derive(clap::Parser)]
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
    CycleMoveWorkspaceToMonitor: CycleDirection,
    Stack: OperationDirection,
    CycleStack: CycleDirection,
    FlipLayout: Axis,
    ChangeLayout: DefaultLayout,
    CycleLayout: CycleDirection,
    WatchConfiguration: BooleanState,
    MouseFollowsFocus: BooleanState,
    Query: StateQuery,
    WindowHidingBehaviour: HidingBehaviour,
    CrossMonitorMoveBehaviour: MoveBehaviour,
    UnmanagedWindowOperationBehaviour: OperationBehaviour,
    PromoteWindow: OperationDirection,
}

macro_rules! gen_target_subcommand_args {
    // SubCommand Pattern
    ( $( $name:ident ),+ $(,)? ) => {
        $(
            #[derive(clap::Parser)]
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
    FocusWorkspaces,
    MoveWorkspaceToMonitor,
    SwapWorkspacesWithMonitor,
    FocusStackWindow,
}

macro_rules! gen_named_target_subcommand_args {
    // SubCommand Pattern
    ( $( $name:ident ),+ $(,)? ) => {
        $(
            #[derive(clap::Parser)]
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
                #[derive(clap::Parser)]
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
                #[derive(clap::Parser)]
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

#[derive(Parser)]
pub struct ClearWorkspaceLayoutRules {
    /// Monitor index (zero-indexed)
    monitor: usize,

    /// Workspace index on the specified monitor (zero-indexed)
    workspace: usize,
}

#[derive(Parser)]
pub struct WorkspaceCustomLayout {
    /// Monitor index (zero-indexed)
    monitor: usize,

    /// Workspace index on the specified monitor (zero-indexed)
    workspace: usize,

    /// JSON or YAML file from which the custom layout definition should be loaded
    path: PathBuf,
}

#[derive(Parser)]
pub struct NamedWorkspaceCustomLayout {
    /// Target workspace name
    workspace: String,

    /// JSON or YAML file from which the custom layout definition should be loaded
    path: PathBuf,
}

#[derive(Parser)]
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

#[derive(Parser)]
pub struct NamedWorkspaceLayoutRule {
    /// Target workspace name
    workspace: String,

    /// The number of window containers on-screen required to trigger this layout rule
    at_container_count: usize,

    #[clap(value_enum)]
    layout: DefaultLayout,
}

#[derive(Parser)]
pub struct WorkspaceCustomLayoutRule {
    /// Monitor index (zero-indexed)
    monitor: usize,

    /// Workspace index on the specified monitor (zero-indexed)
    workspace: usize,

    /// The number of window containers on-screen required to trigger this layout rule
    at_container_count: usize,

    /// JSON or YAML file from which the custom layout definition should be loaded
    path: PathBuf,
}

#[derive(Parser)]
pub struct NamedWorkspaceCustomLayoutRule {
    /// Target workspace name
    workspace: String,

    /// The number of window containers on-screen required to trigger this layout rule
    at_container_count: usize,

    /// JSON or YAML file from which the custom layout definition should be loaded
    path: PathBuf,
}

#[derive(Parser)]
struct Resize {
    #[clap(value_enum)]
    edge: OperationDirection,
    #[clap(value_enum)]
    sizing: Sizing,
}

#[derive(Parser)]
struct ResizeAxis {
    #[clap(value_enum)]
    axis: Axis,
    #[clap(value_enum)]
    sizing: Sizing,
}

#[derive(Parser)]
struct ResizeDelta {
    /// The delta of pixels by which to increase or decrease window dimensions when resizing
    pixels: i32,
}

#[derive(Parser)]
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

#[derive(Parser)]
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

#[derive(Parser)]
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

#[derive(Parser)]
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

#[derive(Parser)]
struct DisplayIndexPreference {
    /// Preferred monitor index (zero-indexed)
    index_preference: usize,
    /// Display name as identified in komorebic state
    display: String,
}

#[derive(Parser)]
struct EnsureWorkspaces {
    /// Monitor index (zero-indexed)
    monitor: usize,
    /// Number of desired workspaces
    workspace_count: usize,
}

#[derive(Parser)]
struct EnsureNamedWorkspaces {
    /// Monitor index (zero-indexed)
    monitor: usize,
    /// Names of desired workspaces
    names: Vec<String>,
}

#[derive(Parser)]
struct FocusMonitorWorkspace {
    /// Target monitor index (zero-indexed)
    target_monitor: usize,
    /// Workspace index on the target monitor (zero-indexed)
    target_workspace: usize,
}

#[derive(Parser)]
pub struct SendToMonitorWorkspace {
    /// Target monitor index (zero-indexed)
    target_monitor: usize,
    /// Workspace index on the target monitor (zero-indexed)
    target_workspace: usize,
}

#[derive(Parser)]
pub struct MoveToMonitorWorkspace {
    /// Target monitor index (zero-indexed)
    target_monitor: usize,
    /// Workspace index on the target monitor (zero-indexed)
    target_workspace: usize,
}

macro_rules! gen_focused_workspace_padding_subcommand_args {
    // SubCommand Pattern
    ( $( $name:ident ),+ $(,)? ) => {
        $(
            #[derive(clap::Parser)]
            pub struct $name {
                /// Pixels size to set as an integer
                size: i32,
            }
        )+
    };
}

gen_focused_workspace_padding_subcommand_args! {
    FocusedWorkspaceContainerPadding,
    FocusedWorkspacePadding,
}

macro_rules! gen_padding_subcommand_args {
    // SubCommand Pattern
    ( $( $name:ident ),+ $(,)? ) => {
        $(
            #[derive(clap::Parser)]
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
            #[derive(clap::Parser)]
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
            #[derive(clap::Parser)]
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
            #[derive(clap::Parser)]
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
    IgnoreRule,
    ManageRule,
    IdentifyTrayApplication,
    IdentifyLayeredApplication,
    IdentifyObjectNameChangeApplication,
    IdentifyBorderOverflowApplication,
    RemoveTitleBar,
}

#[derive(Parser)]
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

#[derive(Parser)]
struct InitialNamedWorkspaceRule {
    #[clap(value_enum)]
    identifier: ApplicationIdentifier,
    /// Identifier as a string
    id: String,
    /// Name of a workspace
    workspace: String,
}

#[derive(Parser)]
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

#[derive(Parser)]
struct NamedWorkspaceRule {
    #[clap(value_enum)]
    identifier: ApplicationIdentifier,
    /// Identifier as a string
    id: String,
    /// Name of a workspace
    workspace: String,
}

#[derive(Parser)]
struct ClearWorkspaceRules {
    /// Monitor index (zero-indexed)
    monitor: usize,
    /// Workspace index on the specified monitor (zero-indexed)
    workspace: usize,
}

#[derive(Parser)]
struct ClearNamedWorkspaceRules {
    /// Name of a workspace
    workspace: String,
}

#[derive(Parser)]
struct ToggleFocusFollowsMouse {
    #[clap(value_enum, short, long, default_value = "windows")]
    implementation: FocusFollowsMouseImplementation,
}

#[derive(Parser)]
struct FocusFollowsMouse {
    #[clap(value_enum, short, long, default_value = "windows")]
    implementation: FocusFollowsMouseImplementation,
    #[clap(value_enum)]
    boolean_state: BooleanState,
}

#[derive(Parser)]
struct Border {
    #[clap(value_enum)]
    boolean_state: BooleanState,
}

#[derive(Parser)]
struct Transparency {
    #[clap(value_enum)]
    boolean_state: BooleanState,
}

#[derive(Parser)]
struct TransparencyAlpha {
    /// Alpha
    alpha: u8,
}

#[derive(Parser)]
struct BorderColour {
    #[clap(value_enum, short, long, default_value = "single")]
    window_kind: WindowKind,
    /// Red
    r: u32,
    /// Green
    g: u32,
    /// Blue
    b: u32,
}

#[derive(Parser)]
struct BorderWidth {
    /// Desired width of the window border
    width: i32,
}

#[derive(Parser)]
struct BorderOffset {
    /// Desired offset of the window border
    offset: i32,
}
#[derive(Parser)]
struct BorderStyle {
    /// Desired border style
    #[clap(value_enum)]
    style: komorebi_client::BorderStyle,
}

#[derive(Parser)]
struct BorderImplementation {
    /// Desired border implementation
    #[clap(value_enum)]
    style: komorebi_client::BorderImplementation,
}

#[derive(Parser)]
struct Animation {
    #[clap(value_enum)]
    boolean_state: BooleanState,
}

#[derive(Parser)]
struct AnimationDuration {
    /// Desired animation durations in ms
    duration: u64,
}

#[derive(Parser)]
struct AnimationFps {
    /// Desired animation frames per second
    fps: u64,
}

#[derive(Parser)]
struct AnimationStyle {
    /// Desired ease function for animation
    #[clap(value_enum, short, long, default_value = "linear")]
    style: komorebi_client::AnimationStyle,
}

#[derive(Parser)]
#[allow(clippy::struct_excessive_bools)]
struct Start {
    /// Allow the use of komorebi's custom focus-follows-mouse implementation
    #[clap(short, long = "ffm")]
    ffm: bool,
    /// Path to a static configuration JSON file
    #[clap(short, long)]
    config: Option<PathBuf>,
    /// Wait for 'komorebic complete-configuration' to be sent before processing events
    #[clap(short, long)]
    await_configuration: bool,
    /// Start a TCP server on the given port to allow the direct sending of SocketMessages
    #[clap(short, long)]
    tcp_port: Option<usize>,
    /// Start whkd in a background process
    #[clap(long)]
    whkd: bool,
    /// Start autohotkey configuration file
    #[clap(long)]
    ahk: bool,
    /// Start komorebi-bar in a background process
    #[clap(long)]
    bar: bool,
}

#[derive(Parser)]
struct Stop {
    /// Stop whkd if it is running as a background process
    #[clap(long)]
    whkd: bool,
    /// Stop komorebi-bar if it is running as a background process
    #[clap(long)]
    bar: bool,
}

#[derive(Parser)]
struct SaveResize {
    /// File to which the resize layout dimensions should be saved
    path: PathBuf,
}

#[derive(Parser)]
struct LoadResize {
    /// File from which the resize layout dimensions should be loaded
    path: PathBuf,
}

#[derive(Parser)]
struct LoadCustomLayout {
    /// JSON or YAML file from which the custom layout definition should be loaded
    path: PathBuf,
}

#[derive(Parser)]
struct SubscribeSocket {
    /// Name of the socket to send event notifications to
    socket: String,
}

#[derive(Parser)]
struct UnsubscribeSocket {
    /// Name of the socket to stop sending event notifications to
    socket: String,
}

#[derive(Parser)]
struct SubscribePipe {
    /// Name of the pipe to send event notifications to (without "\\.\pipe\" prepended)
    named_pipe: String,
}

#[derive(Parser)]
struct UnsubscribePipe {
    /// Name of the pipe to stop sending event notifications to (without "\\.\pipe\" prepended)
    named_pipe: String,
}

#[derive(Parser)]
struct AhkAppSpecificConfiguration {
    /// YAML file from which the application-specific configurations should be loaded
    path: PathBuf,
    /// Optional YAML file of overrides to apply over the first file
    override_path: Option<PathBuf>,
}

#[derive(Parser)]
struct PwshAppSpecificConfiguration {
    /// YAML file from which the application-specific configurations should be loaded
    path: PathBuf,
    /// Optional YAML file of overrides to apply over the first file
    override_path: Option<PathBuf>,
}

#[derive(Parser)]
struct FormatAppSpecificConfiguration {
    /// YAML file from which the application-specific configurations should be loaded
    path: PathBuf,
}

#[derive(Parser)]
struct AltFocusHack {
    #[clap(value_enum)]
    boolean_state: BooleanState,
}

#[derive(Parser)]
struct EnableAutostart {
    /// Path to a static configuration JSON file
    #[clap(action, short, long)]
    config: Option<PathBuf>,
    /// Enable komorebi's custom focus-follows-mouse implementation
    #[clap(short, long = "ffm")]
    ffm: bool,
    /// Enable autostart of whkd
    #[clap(long)]
    whkd: bool,
    /// Enable autostart of ahk
    #[clap(long)]
    ahk: bool,
    /// Enable autostart of komorebi-bar
    #[clap(long)]
    bar: bool,
}

#[derive(Parser)]
struct ReplaceConfiguration {
    /// Static configuration JSON file from which the configuration should be loaded
    path: PathBuf,
}

#[derive(Parser)]
#[clap(author, about, version = build::CLAP_LONG_VERSION)]
struct Opts {
    #[clap(subcommand)]
    subcmd: SubCommand,
}

#[derive(Parser)]
enum SubCommand {
    #[clap(hide = true)]
    Docgen,
    /// Gather example configurations for a new-user quickstart
    Quickstart,
    /// Start komorebi.exe as a background process
    Start(Start),
    /// Stop the komorebi.exe process and restore all hidden windows
    Stop(Stop),
    /// Check komorebi configuration and related files for common errors
    Check,
    /// Show the path to komorebi.json
    #[clap(alias = "config")]
    Configuration,
    /// Show the path to komorebi.bar.json
    #[clap(alias = "bar-config")]
    #[clap(alias = "bconfig")]
    BarConfiguration,
    /// Show the path to whkdrc
    #[clap(alias = "whkd")]
    Whkdrc,
    /// Show a JSON representation of the current window manager state
    State,
    /// Show a JSON representation of the current global state
    GlobalState,
    /// Launch the komorebi-gui debugging tool
    Gui,
    /// Show a JSON representation of visible windows
    VisibleWindows,
    /// Show information about connected monitors
    #[clap(alias = "monitor-info")]
    MonitorInformation,
    /// Query the current window manager state
    #[clap(arg_required_else_help = true)]
    Query(Query),
    /// Subscribe to komorebi events using a Unix Domain Socket
    #[clap(arg_required_else_help = true)]
    SubscribeSocket(SubscribeSocket),
    /// Unsubscribe from komorebi events
    #[clap(arg_required_else_help = true)]
    UnsubscribeSocket(UnsubscribeSocket),
    /// Subscribe to komorebi events using a Named Pipe
    #[clap(arg_required_else_help = true)]
    #[clap(alias = "subscribe")]
    SubscribePipe(SubscribePipe),
    /// Unsubscribe from komorebi events
    #[clap(arg_required_else_help = true)]
    #[clap(alias = "unsubscribe")]
    UnsubscribePipe(UnsubscribePipe),
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
    /// Unstack the focused window
    Unstack,
    /// Cycle the focused stack in the specified cycle direction
    #[clap(arg_required_else_help = true)]
    CycleStack(CycleStack),
    /// Focus the specified window index in the focused stack
    #[clap(arg_required_else_help = true)]
    FocusStackWindow(FocusStackWindow),
    /// Stack all windows on the focused workspace
    StackAll,
    /// Unstack all windows in the focused container
    UnstackAll,
    /// Resize the focused window in the specified direction
    #[clap(arg_required_else_help = true)]
    #[clap(alias = "resize")]
    ResizeEdge(Resize),
    /// Resize the focused window or primary column along the specified axis
    #[clap(arg_required_else_help = true)]
    ResizeAxis(ResizeAxis),
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
    /// Move the focused window to the specified monitor workspace
    #[clap(arg_required_else_help = true)]
    MoveToMonitorWorkspace(MoveToMonitorWorkspace),
    /// Focus the specified monitor
    #[clap(arg_required_else_help = true)]
    FocusMonitor(FocusMonitor),
    /// Focus the last focused workspace on the focused monitor
    FocusLastWorkspace,
    /// Focus the specified workspace on the focused monitor
    #[clap(arg_required_else_help = true)]
    FocusWorkspace(FocusWorkspace),
    /// Focus the specified workspace on all monitors
    #[clap(arg_required_else_help = true)]
    FocusWorkspaces(FocusWorkspaces),
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
    /// Move the focused workspace monitor in the given cycle direction
    #[clap(arg_required_else_help = true)]
    CycleMoveWorkspaceToMonitor(CycleMoveWorkspaceToMonitor),
    /// Swap focused monitor workspaces with specified monitor
    #[clap(arg_required_else_help = true)]
    SwapWorkspacesWithMonitor(SwapWorkspacesWithMonitor),
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
    /// Set container padding on the focused workspace
    #[clap(arg_required_else_help = true)]
    FocusedWorkspaceContainerPadding(FocusedWorkspaceContainerPadding),
    /// Set workspace padding on the focused workspace
    #[clap(arg_required_else_help = true)]
    FocusedWorkspacePadding(FocusedWorkspacePadding),
    /// Adjust container padding on the focused workspace
    #[clap(arg_required_else_help = true)]
    AdjustContainerPadding(AdjustContainerPadding),
    /// Adjust workspace padding on the focused workspace
    #[clap(arg_required_else_help = true)]
    AdjustWorkspacePadding(AdjustWorkspacePadding),
    /// Set the layout on the focused workspace
    #[clap(arg_required_else_help = true)]
    ChangeLayout(ChangeLayout),
    /// Cycle between available layouts
    #[clap(arg_required_else_help = true)]
    CycleLayout(CycleLayout),
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
    /// Promote the window in the specified direction
    PromoteWindow(PromoteWindow),
    /// Force the retiling of all managed windows
    Retile,
    /// Set the monitor index preference for a monitor identified using its size
    #[clap(arg_required_else_help = true)]
    MonitorIndexPreference(MonitorIndexPreference),
    /// Set the display index preference for a monitor identified using its display name
    #[clap(arg_required_else_help = true)]
    DisplayIndexPreference(DisplayIndexPreference),
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
    /// Enable or disable float override, which makes it so every new window opens in floating mode
    ToggleFloatOverride,
    /// Toggle the behaviour for new windows (stacking or dynamic tiling) for currently focused
    /// workspace. If there was no behaviour set for the workspace previously it takes the opposite
    /// of the global value.
    ToggleWorkspaceWindowContainerBehaviour,
    /// Enable or disable float override, which makes it so every new window opens in floating
    /// mode, for the currently focused workspace. If there was no override value set for the
    /// workspace previously it takes the opposite of the global value.
    ToggleWorkspaceFloatOverride,
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
    /// Replace the configuration of a running instance of komorebi from a static configuration file
    #[clap(arg_required_else_help = true)]
    ReplaceConfiguration(ReplaceConfiguration),
    /// Reload legacy komorebi.ahk or komorebi.ps1 configurations (if they exist)
    ReloadConfiguration,
    /// Enable or disable watching of legacy komorebi.ahk or komorebi.ps1 configurations (if they exist)
    #[clap(arg_required_else_help = true)]
    WatchConfiguration(WatchConfiguration),
    /// For legacy komorebi.ahk or komorebi.ps1 configurations, signal that the final configuration option has been sent
    CompleteConfiguration,
    /// DEPRECATED since v0.1.22
    #[clap(arg_required_else_help = true)]
    #[clap(hide = true)]
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
    /// Add a rule to ignore the specified application
    #[clap(arg_required_else_help = true)]
    #[clap(alias = "float-rule")]
    IgnoreRule(IgnoreRule),
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
    /// Remove all application association rules for a workspace by monitor and workspace index
    #[clap(arg_required_else_help = true)]
    ClearWorkspaceRules(ClearWorkspaceRules),
    /// Remove all application association rules for a named workspace
    #[clap(arg_required_else_help = true)]
    ClearNamedWorkspaceRules(ClearNamedWorkspaceRules),
    /// Remove all application association rules for all workspaces
    ClearAllWorkspaceRules,
    /// Identify an application that sends EVENT_OBJECT_NAMECHANGE on launch
    #[clap(arg_required_else_help = true)]
    IdentifyObjectNameChangeApplication(IdentifyObjectNameChangeApplication),
    /// Identify an application that closes to the system tray
    #[clap(arg_required_else_help = true)]
    IdentifyTrayApplication(IdentifyTrayApplication),
    /// Identify an application that has WS_EX_LAYERED, but should still be managed
    #[clap(arg_required_else_help = true)]
    IdentifyLayeredApplication(IdentifyLayeredApplication),
    /// Whitelist an application for title bar removal
    #[clap(arg_required_else_help = true)]
    RemoveTitleBar(RemoveTitleBar),
    /// Toggle title bars for whitelisted applications
    ToggleTitleBars,
    /// Identify an application that has overflowing borders
    #[clap(hide = true)]
    #[clap(alias = "identify-border-overflow")]
    IdentifyBorderOverflowApplication(IdentifyBorderOverflowApplication),
    /// Enable or disable borders
    #[clap(arg_required_else_help = true)]
    #[clap(alias = "active-window-border")]
    Border(Border),
    /// Set the colour for a window border kind
    #[clap(arg_required_else_help = true)]
    #[clap(alias = "active-window-border-colour")]
    BorderColour(BorderColour),
    /// Set the border width
    #[clap(arg_required_else_help = true)]
    #[clap(alias = "active-window-border-width")]
    BorderWidth(BorderWidth),
    /// Set the border offset
    #[clap(arg_required_else_help = true)]
    #[clap(alias = "active-window-border-offset")]
    BorderOffset(BorderOffset),
    /// Set the border style
    #[clap(arg_required_else_help = true)]
    BorderStyle(BorderStyle),
    /// Set the border implementation
    #[clap(arg_required_else_help = true)]
    BorderImplementation(BorderImplementation),
    /// Enable or disable transparency for unfocused windows
    #[clap(arg_required_else_help = true)]
    Transparency(Transparency),
    /// Set the alpha value for unfocused window transparency
    #[clap(arg_required_else_help = true)]
    TransparencyAlpha(TransparencyAlpha),
    /// Toggle transparency for unfocused windows
    ToggleTransparency,
    /// Enable or disable movement animations
    #[clap(arg_required_else_help = true)]
    Animation(Animation),
    /// Set the duration for movement animations in ms
    #[clap(arg_required_else_help = true)]
    AnimationDuration(AnimationDuration),
    /// Set the frames per second for movement animations
    #[clap(arg_required_else_help = true)]
    AnimationFps(AnimationFps),
    /// Set the ease function for movement animations
    #[clap(arg_required_else_help = true)]
    AnimationStyle(AnimationStyle),
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
    /// Fetch the latest version of applications.yaml from komorebi-application-specific-configuration
    #[clap(alias = "fetch-asc")]
    FetchAppSpecificConfiguration,
    /// Generate a JSON Schema for applications.yaml
    #[clap(alias = "asc-schema")]
    ApplicationSpecificConfigurationSchema,
    /// Generate a JSON Schema of subscription notifications
    NotificationSchema,
    /// Generate a JSON Schema of socket messages
    SocketSchema,
    /// Generate a JSON Schema of the static configuration file
    StaticConfigSchema,
    /// Generates a static configuration JSON file based on the current window manager state
    GenerateStaticConfig,
    /// Generates the komorebi.lnk shortcut in shell:startup to autostart komorebi
    EnableAutostart(EnableAutostart),
    /// Deletes the komorebi.lnk shortcut in shell:startup to disable autostart
    DisableAutostart,
}

// print_query is a helper that queries komorebi and prints the response.
// panics on error.
fn print_query(message: &SocketMessage) {
    match send_query(message) {
        Ok(response) => println!("{response}"),
        Err(error) => panic!("{}", error),
    }
}

fn startup_dir() -> Result<PathBuf> {
    let startup = dirs::home_dir()
        .expect("unable to obtain user's home folder")
        .join("AppData")
        .join("Roaming")
        .join("Microsoft")
        .join("Windows")
        .join("Start Menu")
        .join("Programs")
        .join("Startup");

    if !startup.is_dir() {
        std::fs::create_dir_all(&startup)?;
    }

    Ok(startup)
}

#[allow(clippy::too_many_lines, clippy::cognitive_complexity)]
fn main() -> Result<()> {
    let opts: Opts = Opts::parse();

    match opts.subcmd {
        SubCommand::Docgen => {
            let mut cli = Opts::command();
            let subcommands = cli.get_subcommands_mut();
            std::fs::create_dir_all("docs/cli")?;

            let ignore = [
                "docgen",
                "alt-focus-hack",
                "identify-border-overflow-application",
            ];

            for cmd in subcommands {
                let name = cmd.get_name().to_string();
                if !ignore.contains(&name.as_str()) {
                    let help_text = cmd.render_long_help().to_string();
                    let outpath = format!("docs/cli/{name}.md");
                    let markdown = format!("# {name}\n\n```\n{help_text}\n```");
                    std::fs::write(outpath, markdown)?;
                    println!("    - cli/{name}.md");
                }
            }
        }
        SubCommand::Quickstart => {
            let local_appdata_dir = data_local_dir().expect("could not find localdata dir");
            let data_dir = local_appdata_dir.join("komorebi");
            std::fs::create_dir_all(&*WHKD_CONFIG_DIR)?;
            std::fs::create_dir_all(&*HOME_DIR)?;
            std::fs::create_dir_all(data_dir)?;

            let mut komorebi_json = include_str!("../../docs/komorebi.example.json").to_string();
            let komorebi_bar_json =
                include_str!("../../docs/komorebi.bar.example.json").to_string();

            if std::env::var("KOMOREBI_CONFIG_HOME").is_ok() {
                komorebi_json =
                    komorebi_json.replace("Env:USERPROFILE", "Env:KOMOREBI_CONFIG_HOME");
            }

            std::fs::write(HOME_DIR.join("komorebi.json"), komorebi_json)?;
            std::fs::write(HOME_DIR.join("komorebi.bar.json"), komorebi_bar_json)?;

            let applications_yaml = include_str!("../applications.yaml");
            std::fs::write(HOME_DIR.join("applications.yaml"), applications_yaml)?;

            let whkdrc = include_str!("../../docs/whkdrc.sample");
            std::fs::write(WHKD_CONFIG_DIR.join("whkdrc"), whkdrc)?;

            println!("Example komorebi.json, komorebi.bar.json, whkdrc and latest applications.yaml files created");
            println!("You can now run komorebic start --whkd --bar");
        }
        SubCommand::EnableAutostart(args) => {
            let mut current_exe = std::env::current_exe().expect("unable to get exec path");
            current_exe.pop();
            let komorebic_exe = current_exe.join("komorebic-no-console.exe");
            let komorebic_exe = dunce::simplified(&komorebic_exe);

            let startup_dir = startup_dir()?;
            let shortcut_file = startup_dir.join("komorebi.lnk");
            let shortcut_file = dunce::simplified(&shortcut_file);

            let mut arguments = String::from("start");

            if let Some(config) = args.config {
                arguments.push_str(" --config ");
                arguments.push_str(&config.to_string_lossy());
            }

            if args.ffm {
                arguments.push_str(" --ffm");
            }

            if args.bar {
                arguments.push_str(" --bar");
            }

            if args.whkd {
                arguments.push_str(" --whkd");
            } else if args.ahk {
                arguments.push_str(" --ahk");
            }

            Command::new("powershell")
                .arg("-c")
                .arg("$WshShell = New-Object -comObject WScript.Shell; $Shortcut = $WshShell.CreateShortcut($env:SHORTCUT_PATH); $Shortcut.TargetPath = $env:TARGET_PATH; $Shortcut.Arguments = $env:TARGET_ARGS; $Shortcut.Save()")
                .env("SHORTCUT_PATH", shortcut_file.as_os_str())
                .env("TARGET_PATH", komorebic_exe.as_os_str())
                .env("TARGET_ARGS", arguments)
                .output()?;

            println!("NOTE: If your komorebi.json file contains a reference to $Env:KOMOREBI_CONFIG_HOME,");
            println!("you need to add this to System Properties > Environment Variables > User Variables");
            println!("in order for the autostart command to work properly");
        }
        SubCommand::DisableAutostart => {
            let startup_dir = startup_dir()?;
            let shortcut_file = startup_dir.join("komorebi.lnk");

            if shortcut_file.is_file() {
                std::fs::remove_file(shortcut_file)?;
            }
        }
        SubCommand::Check => {
            let home_display = HOME_DIR.display();
            if HAS_CUSTOM_CONFIG_HOME.load(Ordering::SeqCst) {
                println!("KOMOREBI_CONFIG_HOME detected: {home_display}\n");
            } else {
                println!(
                    "No KOMOREBI_CONFIG_HOME detected, defaulting to {}\n",
                    dirs::home_dir()
                        .expect("could not find home dir")
                        .to_string_lossy()
                );
            }

            println!("Looking for configuration files in {home_display}\n");

            let static_config = HOME_DIR.join("komorebi.json");
            let config_pwsh = HOME_DIR.join("komorebi.ps1");
            let config_ahk = HOME_DIR.join("komorebi.ahk");
            let config_whkd = WHKD_CONFIG_DIR.join("whkdrc");

            if static_config.exists() {
                let config_source = std::fs::read_to_string(&static_config)?;
                let lines: Vec<_> = config_source.lines().collect();
                let parsed_config = serde_json::from_str::<serde_json::Value>(&config_source);
                if let Err(serde_error) = &parsed_config {
                    let line = lines[serde_error.line() - 2];

                    let offset = SourceOffset::from_location(
                        config_source.clone(),
                        serde_error.line() - 1,
                        line.len(),
                    );

                    let error_string = serde_error.to_string();
                    let msgs: Vec<_> = error_string.split(" at ").collect();

                    let diagnostic = ConfigurationError {
                        message: msgs[0].to_string(),
                        src: NamedSource::new("komorebi.json", config_source.clone()),
                        bad_bit: SourceSpan::new(offset, 2),
                    };

                    println!("{:?}", Report::new(diagnostic));
                }

                println!("Found komorebi.json; this file can be passed to the start command with the --config flag\n");

                if let Ok(config) = StaticConfig::read(&static_config) {
                    match config.app_specific_configuration_path {
                        None => {
                            println!("Application specific configuration file path has not been set. Try running 'komorebic fetch-asc'\n");
                        }
                        Some(path) => {
                            if !Path::exists(Path::new(&path)) {
                                println!("Application specific configuration file path '{}' does not exist. Try running 'komorebic fetch-asc'\n", path.display());
                            }
                        }
                    }
                }

                // Check that this file adheres to the schema static config schema as the last step,
                // so that more basic errors above can be shown to the error before schema-specific
                // errors
                let _ = serde_json::from_str::<StaticConfig>(&config_source)?;

                if config_whkd.exists() {
                    println!("Found {}; key bindings will be loaded from here when whkd is started, and you can start it automatically using the --whkd flag\n", config_whkd.to_string_lossy());
                } else {
                    println!("No ~/.config/whkdrc found; you may not be able to control komorebi with your keyboard\n");
                }
            } else if config_pwsh.exists() {
                println!("Found komorebi.ps1; this file will be autoloaded by komorebi\n");
                if config_whkd.exists() {
                    println!(
                        "Found {}; key bindings will be loaded from here when whkd is started\n",
                        config_whkd.to_string_lossy()
                    );
                } else {
                    println!("No ~/.config/whkdrc found; you may not be able to control komorebi with your keyboard\n");
                }
            } else if config_ahk.exists() {
                println!("Found komorebi.ahk; this file will be autoloaded by komorebi\n");
            } else {
                println!("No komorebi configuration found in {home_display}\n");
                println!("If running 'komorebic start --await-configuration', you will manually have to call the following command to begin tiling: komorebic complete-configuration\n");
            }
        }
        SubCommand::Configuration => {
            let static_config = HOME_DIR.join("komorebi.json");

            if static_config.exists() {
                println!("{}", static_config.display());
            }
        }
        SubCommand::BarConfiguration => {
            let static_config = HOME_DIR.join("komorebi.bar.json");

            if static_config.exists() {
                println!("{}", static_config.display());
            }
        }
        SubCommand::Whkdrc => {
            let whkdrc = WHKD_CONFIG_DIR.join("whkdrc");

            if whkdrc.exists() {
                println!("{}", whkdrc.display());
            }
        }
        SubCommand::Log => {
            let timestamp = Utc::now().format("%Y-%m-%d").to_string();
            let color_log = std::env::temp_dir().join(format!("komorebi.log.{timestamp}"));
            let file = TailedFile::new(File::open(color_log)?);
            let locked = file.lock();
            #[allow(clippy::significant_drop_in_scrutinee, clippy::lines_filter_map_ok)]
            for line in locked.lines().flatten() {
                println!("{line}");
            }
        }
        SubCommand::Focus(arg) => {
            send_message(&SocketMessage::FocusWindow(arg.operation_direction))?;
        }
        SubCommand::ForceFocus => {
            send_message(&SocketMessage::ForceFocus)?;
        }
        SubCommand::Close => {
            send_message(&SocketMessage::Close)?;
        }
        SubCommand::Minimize => {
            send_message(&SocketMessage::Minimize)?;
        }
        SubCommand::Promote => {
            send_message(&SocketMessage::Promote)?;
        }
        SubCommand::PromoteFocus => {
            send_message(&SocketMessage::PromoteFocus)?;
        }
        SubCommand::PromoteWindow(arg) => {
            send_message(&SocketMessage::PromoteWindow(arg.operation_direction))?;
        }
        SubCommand::TogglePause => {
            send_message(&SocketMessage::TogglePause)?;
        }
        SubCommand::Retile => {
            send_message(&SocketMessage::Retile)?;
        }
        SubCommand::Move(arg) => {
            send_message(&SocketMessage::MoveWindow(arg.operation_direction))?;
        }
        SubCommand::CycleFocus(arg) => {
            send_message(&SocketMessage::CycleFocusWindow(arg.cycle_direction))?;
        }
        SubCommand::CycleMove(arg) => {
            send_message(&SocketMessage::CycleMoveWindow(arg.cycle_direction))?;
        }
        SubCommand::MoveToMonitor(arg) => {
            send_message(&SocketMessage::MoveContainerToMonitorNumber(arg.target))?;
        }
        SubCommand::CycleMoveToMonitor(arg) => {
            send_message(&SocketMessage::CycleMoveContainerToMonitor(
                arg.cycle_direction,
            ))?;
        }
        SubCommand::MoveToWorkspace(arg) => {
            send_message(&SocketMessage::MoveContainerToWorkspaceNumber(arg.target))?;
        }
        SubCommand::MoveToNamedWorkspace(arg) => {
            send_message(&SocketMessage::MoveContainerToNamedWorkspace(arg.workspace))?;
        }
        SubCommand::CycleMoveToWorkspace(arg) => {
            send_message(&SocketMessage::CycleMoveContainerToWorkspace(
                arg.cycle_direction,
            ))?;
        }
        SubCommand::SendToMonitor(arg) => {
            send_message(&SocketMessage::SendContainerToMonitorNumber(arg.target))?;
        }
        SubCommand::CycleSendToMonitor(arg) => {
            send_message(&SocketMessage::CycleSendContainerToMonitor(
                arg.cycle_direction,
            ))?;
        }
        SubCommand::SendToWorkspace(arg) => {
            send_message(&SocketMessage::SendContainerToWorkspaceNumber(arg.target))?;
        }
        SubCommand::SendToNamedWorkspace(arg) => {
            send_message(&SocketMessage::SendContainerToNamedWorkspace(arg.workspace))?;
        }
        SubCommand::CycleSendToWorkspace(arg) => {
            send_message(&SocketMessage::CycleSendContainerToWorkspace(
                arg.cycle_direction,
            ))?;
        }
        SubCommand::SendToMonitorWorkspace(arg) => {
            send_message(&SocketMessage::SendContainerToMonitorWorkspaceNumber(
                arg.target_monitor,
                arg.target_workspace,
            ))?;
        }
        SubCommand::MoveToMonitorWorkspace(arg) => {
            send_message(&SocketMessage::MoveContainerToMonitorWorkspaceNumber(
                arg.target_monitor,
                arg.target_workspace,
            ))?;
        }
        SubCommand::MoveWorkspaceToMonitor(arg) => {
            send_message(&SocketMessage::MoveWorkspaceToMonitorNumber(arg.target))?;
        }
        SubCommand::CycleMoveWorkspaceToMonitor(arg) => {
            send_message(&SocketMessage::CycleMoveWorkspaceToMonitor(
                arg.cycle_direction,
            ))?;
        }
        SubCommand::SwapWorkspacesWithMonitor(arg) => {
            send_message(&SocketMessage::SwapWorkspacesToMonitorNumber(arg.target))?;
        }
        SubCommand::InvisibleBorders(arg) => {
            send_message(&SocketMessage::InvisibleBorders(Rect {
                left: arg.left,
                top: arg.top,
                right: arg.right,
                bottom: arg.bottom,
            }))?;
        }
        SubCommand::MonitorWorkAreaOffset(arg) => {
            send_message(&SocketMessage::MonitorWorkAreaOffset(
                arg.monitor,
                Rect {
                    left: arg.left,
                    top: arg.top,
                    right: arg.right,
                    bottom: arg.bottom,
                },
            ))?;
        }
        SubCommand::GlobalWorkAreaOffset(arg) => {
            send_message(&SocketMessage::WorkAreaOffset(Rect {
                left: arg.left,
                top: arg.top,
                right: arg.right,
                bottom: arg.bottom,
            }))?;
        }
        SubCommand::ContainerPadding(arg) => {
            send_message(&SocketMessage::ContainerPadding(
                arg.monitor,
                arg.workspace,
                arg.size,
            ))?;
        }
        SubCommand::NamedWorkspaceContainerPadding(arg) => {
            send_message(&SocketMessage::NamedWorkspaceContainerPadding(
                arg.workspace,
                arg.size,
            ))?;
        }
        SubCommand::WorkspacePadding(arg) => {
            send_message(&SocketMessage::WorkspacePadding(
                arg.monitor,
                arg.workspace,
                arg.size,
            ))?;
        }
        SubCommand::NamedWorkspacePadding(arg) => {
            send_message(&SocketMessage::NamedWorkspacePadding(
                arg.workspace,
                arg.size,
            ))?;
        }
        SubCommand::FocusedWorkspacePadding(arg) => {
            send_message(&SocketMessage::FocusedWorkspacePadding(arg.size))?;
        }
        SubCommand::FocusedWorkspaceContainerPadding(arg) => {
            send_message(&SocketMessage::FocusedWorkspaceContainerPadding(arg.size))?;
        }
        SubCommand::AdjustWorkspacePadding(arg) => {
            send_message(&SocketMessage::AdjustWorkspacePadding(
                arg.sizing,
                arg.adjustment,
            ))?;
        }
        SubCommand::AdjustContainerPadding(arg) => {
            send_message(&SocketMessage::AdjustContainerPadding(
                arg.sizing,
                arg.adjustment,
            ))?;
        }
        SubCommand::ToggleFocusFollowsMouse(arg) => {
            send_message(&SocketMessage::ToggleFocusFollowsMouse(arg.implementation))?;
        }
        SubCommand::ToggleTiling => {
            send_message(&SocketMessage::ToggleTiling)?;
        }
        SubCommand::ToggleFloat => {
            send_message(&SocketMessage::ToggleFloat)?;
        }
        SubCommand::ToggleMonocle => {
            send_message(&SocketMessage::ToggleMonocle)?;
        }
        SubCommand::ToggleMaximize => {
            send_message(&SocketMessage::ToggleMaximize)?;
        }
        SubCommand::WorkspaceLayout(arg) => {
            send_message(&SocketMessage::WorkspaceLayout(
                arg.monitor,
                arg.workspace,
                arg.value,
            ))?;
        }
        SubCommand::NamedWorkspaceLayout(arg) => {
            send_message(&SocketMessage::NamedWorkspaceLayout(
                arg.workspace,
                arg.value,
            ))?;
        }
        SubCommand::WorkspaceCustomLayout(arg) => {
            send_message(&SocketMessage::WorkspaceLayoutCustom(
                arg.monitor,
                arg.workspace,
                resolve_home_path(arg.path)?,
            ))?;
        }
        SubCommand::NamedWorkspaceCustomLayout(arg) => {
            send_message(&SocketMessage::NamedWorkspaceLayoutCustom(
                arg.workspace,
                resolve_home_path(arg.path)?,
            ))?;
        }
        SubCommand::WorkspaceLayoutRule(arg) => {
            send_message(&SocketMessage::WorkspaceLayoutRule(
                arg.monitor,
                arg.workspace,
                arg.at_container_count,
                arg.layout,
            ))?;
        }
        SubCommand::NamedWorkspaceLayoutRule(arg) => {
            send_message(&SocketMessage::NamedWorkspaceLayoutRule(
                arg.workspace,
                arg.at_container_count,
                arg.layout,
            ))?;
        }
        SubCommand::WorkspaceCustomLayoutRule(arg) => {
            send_message(&SocketMessage::WorkspaceLayoutCustomRule(
                arg.monitor,
                arg.workspace,
                arg.at_container_count,
                resolve_home_path(arg.path)?,
            ))?;
        }
        SubCommand::NamedWorkspaceCustomLayoutRule(arg) => {
            send_message(&SocketMessage::NamedWorkspaceLayoutCustomRule(
                arg.workspace,
                arg.at_container_count,
                resolve_home_path(arg.path)?,
            ))?;
        }
        SubCommand::ClearWorkspaceLayoutRules(arg) => {
            send_message(&SocketMessage::ClearWorkspaceLayoutRules(
                arg.monitor,
                arg.workspace,
            ))?;
        }
        SubCommand::ClearNamedWorkspaceLayoutRules(arg) => {
            send_message(&SocketMessage::ClearNamedWorkspaceLayoutRules(
                arg.workspace,
            ))?;
        }
        SubCommand::WorkspaceTiling(arg) => {
            send_message(&SocketMessage::WorkspaceTiling(
                arg.monitor,
                arg.workspace,
                arg.value.into(),
            ))?;
        }
        SubCommand::NamedWorkspaceTiling(arg) => {
            send_message(&SocketMessage::NamedWorkspaceTiling(
                arg.workspace,
                arg.value.into(),
            ))?;
        }
        SubCommand::Start(arg) => {
            let mut ahk: String = String::from("autohotkey.exe");

            if let Ok(komorebi_ahk_exe) = std::env::var("KOMOREBI_AHK_EXE") {
                if which(&komorebi_ahk_exe).is_ok() {
                    ahk = komorebi_ahk_exe;
                }
            }

            if arg.whkd && which("whkd").is_err() {
                bail!("could not find whkd, please make sure it is installed before using the --whkd flag");
            }

            if arg.ahk && which(&ahk).is_err() {
                bail!("could not find autohotkey, please make sure it is installed before using the --ahk flag");
            }

            let mut buf: PathBuf;

            // The komorebi.ps1 shim will only exist in the Path if installed by Scoop
            let exec = if let Ok(output) = Command::new("where.exe").arg("komorebi.ps1").output() {
                let stdout = String::from_utf8(output.stdout)?;
                match stdout.trim() {
                    "" => None,
                    // It's possible that a komorebi.ps1 config will be in %USERPROFILE% - ignore this
                    stdout if !stdout.contains("scoop") => None,
                    stdout => {
                        buf = PathBuf::from(stdout);
                        buf.pop(); // %USERPROFILE%\scoop\shims
                        buf.pop(); // %USERPROFILE%\scoop
                        buf.push("apps\\komorebi\\current\\komorebi.exe"); //%USERPROFILE%\scoop\komorebi\current\komorebi.exe
                        Some(buf.to_str().ok_or_else(|| {
                            anyhow!("cannot create a string from the scoop komorebi path")
                        })?)
                    }
                }
            } else {
                None
            };

            let mut flags = vec![];
            if let Some(config) = &arg.config {
                let path = resolve_home_path(config)?;
                if !path.is_file() {
                    bail!("could not find file: {}", path.display());
                }

                // we don't need to replace UNC prefix here as `resolve_home_path` already did
                flags.push(format!("'--config=\"{}\"'", path.display()));
            }

            if arg.ffm {
                flags.push("'--ffm'".to_string());
            }

            if arg.await_configuration {
                flags.push("'--await-configuration'".to_string());
            }

            if let Some(port) = arg.tcp_port {
                flags.push(format!("'--tcp-port={port}'"));
            }

            let script = if flags.is_empty() {
                format!(
                    "Start-Process '{}' -WindowStyle hidden",
                    exec.unwrap_or("komorebi.exe")
                )
            } else {
                let argument_list = flags.join(",");
                format!(
                    "Start-Process '{}' -ArgumentList {argument_list} -WindowStyle hidden",
                    exec.unwrap_or("komorebi.exe")
                )
            };

            let mut attempts = 0;
            let mut running = false;

            while !running && attempts <= 2 {
                match powershell_script::run(&script) {
                    Ok(_) => {
                        println!("{script}");
                    }
                    Err(error) => {
                        println!("Error: {error}");
                    }
                }

                print!("Waiting for komorebi.exe to start...");
                std::thread::sleep(Duration::from_secs(3));

                let mut system = sysinfo::System::new_all();
                system.refresh_processes(ProcessesToUpdate::All);

                if system
                    .processes_by_name("komorebi.exe".as_ref())
                    .next()
                    .is_some()
                {
                    println!("Started!");
                    running = true;
                } else {
                    println!("komorebi.exe did not start... Trying again");
                    attempts += 1;
                }
            }

            if !running {
                println!("\nRunning komorebi.exe directly for detailed error output\n");
                if let Some(config) = arg.config {
                    let path = resolve_home_path(config)?;
                    if let Ok(output) = Command::new("komorebi.exe")
                        .arg(format!("'--config=\"{}\"'", path.display()))
                        .output()
                    {
                        println!("{}", String::from_utf8(output.stderr)?);
                    }
                } else if let Ok(output) = Command::new("komorebi.exe").output() {
                    println!("{}", String::from_utf8(output.stderr)?);
                }

                return Ok(());
            }

            if arg.whkd {
                let script = r"
if (!(Get-Process whkd -ErrorAction SilentlyContinue))
{
  Start-Process whkd -WindowStyle hidden
}
                ";
                match powershell_script::run(script) {
                    Ok(_) => {
                        println!("{script}");
                    }
                    Err(error) => {
                        println!("Error: {error}");
                    }
                }
            }

            if arg.ahk {
                let config_ahk = HOME_DIR.join("komorebi.ahk");
                let config_ahk = dunce::simplified(&config_ahk);

                let script = format!(
                    r#"
  Start-Process '{ahk}' '{config}' -WindowStyle hidden
                "#,
                    config = config_ahk.display()
                );

                match powershell_script::run(&script) {
                    Ok(_) => {
                        println!("{script}");
                    }
                    Err(error) => {
                        println!("Error: {error}");
                    }
                }
            }

            let static_config = arg.config.clone().map_or_else(
                || {
                    let komorebi_json = HOME_DIR.join("komorebi.json");
                    if komorebi_json.is_file() {
                        Option::from(komorebi_json)
                    } else {
                        None
                    }
                },
                Option::from,
            );

            if arg.bar {
                if let Some(config) = &static_config {
                    let mut config = StaticConfig::read(config)?;
                    if let Some(display_bar_configurations) = &mut config.bar_configurations {
                        for config_file_path in &mut *display_bar_configurations {
                            let script = r"Start-Process 'komorebi-bar' '--config CONFIGFILE' -WindowStyle hidden"
                            .replace("CONFIGFILE", &config_file_path.to_string_lossy());

                            match powershell_script::run(&script) {
                                Ok(_) => {
                                    println!("{script}");
                                }
                                Err(error) => {
                                    println!("Error: {error}");
                                }
                            }
                        }
                    } else {
                        let script = r"
if (!(Get-Process komorebi-bar -ErrorAction SilentlyContinue))
{
  Start-Process komorebi-bar -WindowStyle hidden
}
                ";
                        match powershell_script::run(script) {
                            Ok(_) => {
                                println!("{script}");
                            }
                            Err(error) => {
                                println!("Error: {error}");
                            }
                        }
                    }
                }
            }

            println!("\nThank you for using komorebi!\n");
            println!("* Become a sponsor https://github.com/sponsors/LGUG2Z - Even $1/month makes a big difference");
            println!(
                "* Subscribe to https://youtube.com/@LGUG2Z - Live dev videos and feature previews"
            );
            println!("* Join the Discord https://discord.gg/mGkn66PHkx - Chat, ask questions, share your desktops");
            println!("* Read the docs https://lgug2z.github.io/komorebi - Quickly search through all komorebic commands");

            let bar_config = arg.config.map_or_else(
                || {
                    let bar_json = HOME_DIR.join("komorebi.bar.json");
                    if bar_json.is_file() {
                        Option::from(bar_json)
                    } else {
                        None
                    }
                },
                Option::from,
            );

            if let Some(config) = &static_config {
                let path = resolve_home_path(config)?;
                let raw = std::fs::read_to_string(path)?;
                StaticConfig::aliases(&raw);
                StaticConfig::deprecated(&raw);
            }

            if bar_config.is_some() {
                let output = Command::new("komorebi-bar.exe").arg("--aliases").output()?;
                let stdout = String::from_utf8(output.stdout)?;
                println!("{stdout}");
            }
        }
        SubCommand::Stop(arg) => {
            if arg.whkd {
                let script = r"
Stop-Process -Name:whkd -ErrorAction SilentlyContinue
                ";
                match powershell_script::run(script) {
                    Ok(_) => {
                        println!("{script}");
                    }
                    Err(error) => {
                        println!("Error: {error}");
                    }
                }
            }

            if arg.bar {
                let script = r"
Stop-Process -Name:komorebi-bar -ErrorAction SilentlyContinue
                ";
                match powershell_script::run(script) {
                    Ok(_) => {
                        println!("{script}");
                    }
                    Err(error) => {
                        println!("Error: {error}");
                    }
                }
            }

            send_message(&SocketMessage::Stop)?;
            let mut system = sysinfo::System::new_all();
            system.refresh_processes(ProcessesToUpdate::All);

            if system.processes_by_name("komorebi.exe".as_ref()).count() >= 1 {
                println!("komorebi is still running, attempting to force-quit");

                let script = r"
Stop-Process -Name:komorebi -ErrorAction SilentlyContinue
                ";
                match powershell_script::run(script) {
                    Ok(_) => {
                        println!("{script}");

                        let hwnd_json = DATA_DIR.join("komorebi.hwnd.json");

                        let file = File::open(hwnd_json)?;
                        let reader = BufReader::new(file);
                        let hwnds: Vec<isize> = serde_json::from_reader(reader)?;

                        for hwnd in hwnds {
                            restore_window(hwnd);
                        }
                    }
                    Err(error) => {
                        println!("Error: {error}");
                    }
                }
            }
        }
        SubCommand::IgnoreRule(arg) => {
            send_message(&SocketMessage::IgnoreRule(arg.identifier, arg.id))?;
        }
        SubCommand::ManageRule(arg) => {
            send_message(&SocketMessage::ManageRule(arg.identifier, arg.id))?;
        }
        SubCommand::InitialWorkspaceRule(arg) => {
            send_message(&SocketMessage::InitialWorkspaceRule(
                arg.identifier,
                arg.id,
                arg.monitor,
                arg.workspace,
            ))?;
        }
        SubCommand::InitialNamedWorkspaceRule(arg) => {
            send_message(&SocketMessage::InitialNamedWorkspaceRule(
                arg.identifier,
                arg.id,
                arg.workspace,
            ))?;
        }
        SubCommand::WorkspaceRule(arg) => {
            send_message(&SocketMessage::WorkspaceRule(
                arg.identifier,
                arg.id,
                arg.monitor,
                arg.workspace,
            ))?;
        }
        SubCommand::NamedWorkspaceRule(arg) => {
            send_message(&SocketMessage::NamedWorkspaceRule(
                arg.identifier,
                arg.id,
                arg.workspace,
            ))?;
        }
        SubCommand::ClearWorkspaceRules(arg) => {
            send_message(&SocketMessage::ClearWorkspaceRules(
                arg.monitor,
                arg.workspace,
            ))?;
        }
        SubCommand::ClearNamedWorkspaceRules(arg) => {
            send_message(&SocketMessage::ClearNamedWorkspaceRules(arg.workspace))?;
        }
        SubCommand::ClearAllWorkspaceRules => {
            send_message(&SocketMessage::ClearAllWorkspaceRules)?;
        }
        SubCommand::Stack(arg) => {
            send_message(&SocketMessage::StackWindow(arg.operation_direction))?;
        }
        SubCommand::StackAll => {
            send_message(&SocketMessage::StackAll)?;
        }
        SubCommand::Unstack => {
            send_message(&SocketMessage::UnstackWindow)?;
        }
        SubCommand::UnstackAll => {
            send_message(&SocketMessage::UnstackAll)?;
        }
        SubCommand::FocusStackWindow(arg) => {
            send_message(&SocketMessage::FocusStackWindow(arg.target))?;
        }
        SubCommand::CycleStack(arg) => {
            send_message(&SocketMessage::CycleStack(arg.cycle_direction))?;
        }
        SubCommand::ChangeLayout(arg) => {
            send_message(&SocketMessage::ChangeLayout(arg.default_layout))?;
        }
        SubCommand::CycleLayout(arg) => {
            send_message(&SocketMessage::CycleLayout(arg.cycle_direction))?;
        }
        SubCommand::LoadCustomLayout(arg) => {
            send_message(&SocketMessage::ChangeLayoutCustom(resolve_home_path(
                arg.path,
            )?))?;
        }
        SubCommand::FlipLayout(arg) => {
            send_message(&SocketMessage::FlipLayout(arg.axis))?;
        }
        SubCommand::FocusMonitor(arg) => {
            send_message(&SocketMessage::FocusMonitorNumber(arg.target))?;
        }
        SubCommand::FocusLastWorkspace => {
            send_message(&SocketMessage::FocusLastWorkspace)?;
        }
        SubCommand::FocusWorkspace(arg) => {
            send_message(&SocketMessage::FocusWorkspaceNumber(arg.target))?;
        }
        SubCommand::FocusWorkspaces(arg) => {
            send_message(&SocketMessage::FocusWorkspaceNumbers(arg.target))?;
        }
        SubCommand::FocusMonitorWorkspace(arg) => {
            send_message(&SocketMessage::FocusMonitorWorkspaceNumber(
                arg.target_monitor,
                arg.target_workspace,
            ))?;
        }
        SubCommand::FocusNamedWorkspace(arg) => {
            send_message(&SocketMessage::FocusNamedWorkspace(arg.workspace))?;
        }
        SubCommand::CycleMonitor(arg) => {
            send_message(&SocketMessage::CycleFocusMonitor(arg.cycle_direction))?;
        }
        SubCommand::CycleWorkspace(arg) => {
            send_message(&SocketMessage::CycleFocusWorkspace(arg.cycle_direction))?;
        }
        SubCommand::NewWorkspace => {
            send_message(&SocketMessage::NewWorkspace)?;
        }
        SubCommand::WorkspaceName(name) => {
            send_message(&SocketMessage::WorkspaceName(
                name.monitor,
                name.workspace,
                name.value,
            ))?;
        }
        SubCommand::MonitorIndexPreference(arg) => {
            send_message(&SocketMessage::MonitorIndexPreference(
                arg.index_preference,
                arg.left,
                arg.top,
                arg.right,
                arg.bottom,
            ))?;
        }
        SubCommand::DisplayIndexPreference(arg) => {
            send_message(&SocketMessage::DisplayIndexPreference(
                arg.index_preference,
                arg.display,
            ))?;
        }
        SubCommand::EnsureWorkspaces(workspaces) => {
            send_message(&SocketMessage::EnsureWorkspaces(
                workspaces.monitor,
                workspaces.workspace_count,
            ))?;
        }
        SubCommand::EnsureNamedWorkspaces(arg) => {
            send_message(&SocketMessage::EnsureNamedWorkspaces(
                arg.monitor,
                arg.names,
            ))?;
        }
        SubCommand::State => {
            print_query(&SocketMessage::State);
        }
        SubCommand::GlobalState => {
            print_query(&SocketMessage::GlobalState);
        }
        SubCommand::Gui => {
            Command::new("komorebi-gui").spawn()?;
        }
        SubCommand::VisibleWindows => {
            print_query(&SocketMessage::VisibleWindows);
        }
        SubCommand::MonitorInformation => {
            print_query(&SocketMessage::MonitorInformation);
        }
        SubCommand::Query(arg) => {
            print_query(&SocketMessage::Query(arg.state_query));
        }
        SubCommand::RestoreWindows => {
            let hwnd_json = DATA_DIR.join("komorebi.hwnd.json");

            let file = File::open(hwnd_json)?;
            let reader = BufReader::new(file);
            let hwnds: Vec<isize> = serde_json::from_reader(reader)?;

            for hwnd in hwnds {
                restore_window(hwnd);
            }
        }
        SubCommand::ResizeEdge(resize) => {
            send_message(&SocketMessage::ResizeWindowEdge(resize.edge, resize.sizing))?;
        }
        SubCommand::ResizeAxis(arg) => {
            send_message(&SocketMessage::ResizeWindowAxis(arg.axis, arg.sizing))?;
        }
        SubCommand::FocusFollowsMouse(arg) => {
            send_message(&SocketMessage::FocusFollowsMouse(
                arg.implementation,
                arg.boolean_state.into(),
            ))?;
        }
        SubCommand::ReplaceConfiguration(arg) => {
            send_message(&SocketMessage::ReplaceConfiguration(arg.path))?;
        }
        SubCommand::ReloadConfiguration => {
            send_message(&SocketMessage::ReloadConfiguration)?;
        }
        SubCommand::WatchConfiguration(arg) => {
            send_message(&SocketMessage::WatchConfiguration(arg.boolean_state.into()))?;
        }
        SubCommand::CompleteConfiguration => {
            send_message(&SocketMessage::CompleteConfiguration)?;
        }
        SubCommand::IdentifyObjectNameChangeApplication(target) => {
            send_message(&SocketMessage::IdentifyObjectNameChangeApplication(
                target.identifier,
                target.id,
            ))?;
        }
        SubCommand::IdentifyTrayApplication(target) => {
            send_message(&SocketMessage::IdentifyTrayApplication(
                target.identifier,
                target.id,
            ))?;
        }
        SubCommand::IdentifyLayeredApplication(target) => {
            send_message(&SocketMessage::IdentifyLayeredApplication(
                target.identifier,
                target.id,
            ))?;
        }
        SubCommand::RemoveTitleBar(target) => {
            match target.identifier {
                ApplicationIdentifier::Exe => {}
                _ => {
                    bail!("this command requires applications to be identified by their exe");
                }
            }

            send_message(&SocketMessage::RemoveTitleBar(target.identifier, target.id))?;
        }
        SubCommand::ToggleTitleBars => {
            send_message(&SocketMessage::ToggleTitleBars)?;
        }
        SubCommand::Manage => {
            send_message(&SocketMessage::ManageFocusedWindow)?;
        }
        SubCommand::Unmanage => {
            send_message(&SocketMessage::UnmanageFocusedWindow)?;
        }
        SubCommand::QuickSaveResize => {
            send_message(&SocketMessage::QuickSave)?;
        }
        SubCommand::QuickLoadResize => {
            send_message(&SocketMessage::QuickLoad)?;
        }
        SubCommand::SaveResize(arg) => {
            send_message(&SocketMessage::Save(resolve_home_path(arg.path)?))?;
        }
        SubCommand::LoadResize(arg) => {
            send_message(&SocketMessage::Load(resolve_home_path(arg.path)?))?;
        }
        SubCommand::SubscribeSocket(arg) => {
            send_message(&SocketMessage::AddSubscriberSocket(arg.socket))?;
        }
        SubCommand::UnsubscribeSocket(arg) => {
            send_message(&SocketMessage::RemoveSubscriberSocket(arg.socket))?;
        }
        SubCommand::SubscribePipe(arg) => {
            send_message(&SocketMessage::AddSubscriberPipe(arg.named_pipe))?;
        }
        SubCommand::UnsubscribePipe(arg) => {
            send_message(&SocketMessage::RemoveSubscriberPipe(arg.named_pipe))?;
        }
        SubCommand::ToggleMouseFollowsFocus => {
            send_message(&SocketMessage::ToggleMouseFollowsFocus)?;
        }
        SubCommand::MouseFollowsFocus(arg) => {
            send_message(&SocketMessage::MouseFollowsFocus(arg.boolean_state.into()))?;
        }
        SubCommand::Border(arg) => {
            send_message(&SocketMessage::Border(arg.boolean_state.into()))?;
        }
        SubCommand::BorderColour(arg) => {
            send_message(&SocketMessage::BorderColour(
                arg.window_kind,
                arg.r,
                arg.g,
                arg.b,
            ))?;
        }
        SubCommand::BorderWidth(arg) => {
            send_message(&SocketMessage::BorderWidth(arg.width))?;
        }
        SubCommand::BorderOffset(arg) => {
            send_message(&SocketMessage::BorderOffset(arg.offset))?;
        }
        SubCommand::BorderStyle(arg) => {
            send_message(&SocketMessage::BorderStyle(arg.style))?;
        }
        SubCommand::BorderImplementation(arg) => {
            send_message(&SocketMessage::BorderImplementation(arg.style))?;
        }
        SubCommand::Transparency(arg) => {
            send_message(&SocketMessage::Transparency(arg.boolean_state.into()))?;
        }
        SubCommand::TransparencyAlpha(arg) => {
            send_message(&SocketMessage::TransparencyAlpha(arg.alpha))?;
        }
        SubCommand::ToggleTransparency => {
            send_message(&SocketMessage::ToggleTransparency)?;
        }
        SubCommand::Animation(arg) => {
            send_message(&SocketMessage::Animation(arg.boolean_state.into()))?;
        }
        SubCommand::AnimationDuration(arg) => {
            send_message(&SocketMessage::AnimationDuration(arg.duration))?;
        }
        SubCommand::AnimationFps(arg) => {
            send_message(&SocketMessage::AnimationFps(arg.fps))?;
        }
        SubCommand::AnimationStyle(arg) => {
            send_message(&SocketMessage::AnimationStyle(arg.style))?;
        }

        SubCommand::ResizeDelta(arg) => {
            send_message(&SocketMessage::ResizeDelta(arg.pixels))?;
        }
        SubCommand::ToggleWindowContainerBehaviour => {
            send_message(&SocketMessage::ToggleWindowContainerBehaviour)?;
        }
        SubCommand::ToggleFloatOverride => {
            send_message(&SocketMessage::ToggleFloatOverride)?;
        }
        SubCommand::ToggleWorkspaceWindowContainerBehaviour => {
            send_message(&SocketMessage::ToggleWorkspaceWindowContainerBehaviour)?;
        }
        SubCommand::ToggleWorkspaceFloatOverride => {
            send_message(&SocketMessage::ToggleWorkspaceFloatOverride)?;
        }
        SubCommand::WindowHidingBehaviour(arg) => {
            send_message(&SocketMessage::WindowHidingBehaviour(arg.hiding_behaviour))?;
        }
        SubCommand::CrossMonitorMoveBehaviour(arg) => {
            send_message(&SocketMessage::CrossMonitorMoveBehaviour(
                arg.move_behaviour,
            ))?;
        }
        SubCommand::ToggleCrossMonitorMoveBehaviour => {
            send_message(&SocketMessage::ToggleCrossMonitorMoveBehaviour)?;
        }
        SubCommand::UnmanagedWindowOperationBehaviour(arg) => {
            send_message(&SocketMessage::UnmanagedWindowOperationBehaviour(
                arg.operation_behaviour,
            ))?;
        }
        SubCommand::AhkAppSpecificConfiguration(arg) => {
            let content = std::fs::read_to_string(resolve_home_path(arg.path)?)?;
            let lines = if let Some(override_path) = arg.override_path {
                let override_content = std::fs::read_to_string(resolve_home_path(override_path)?)?;

                ApplicationConfigurationGenerator::generate_ahk(
                    &content,
                    Option::from(override_content.as_str()),
                )?
            } else {
                ApplicationConfigurationGenerator::generate_ahk(&content, None)?
            };

            let generated_config = HOME_DIR.join("komorebi.generated.ahk");
            let mut file = OpenOptions::new()
                .write(true)
                .create(true)
                .truncate(true)
                .open(&generated_config)?;

            file.write_all(lines.join("\n").as_bytes())?;

            println!(
                "\nApplication-specific generated configuration written to {}",
                generated_config.display()
            );
        }
        SubCommand::PwshAppSpecificConfiguration(arg) => {
            let content = std::fs::read_to_string(resolve_home_path(arg.path)?)?;
            let lines = if let Some(override_path) = arg.override_path {
                let override_content = std::fs::read_to_string(resolve_home_path(override_path)?)?;

                ApplicationConfigurationGenerator::generate_pwsh(
                    &content,
                    Option::from(override_content.as_str()),
                )?
            } else {
                ApplicationConfigurationGenerator::generate_pwsh(&content, None)?
            };

            let generated_config = HOME_DIR.join("komorebi.generated.ps1");
            let mut file = OpenOptions::new()
                .write(true)
                .create(true)
                .truncate(true)
                .open(&generated_config)?;

            file.write_all(lines.join("\n").as_bytes())?;

            println!(
                "\nApplication-specific generated configuration written to {}",
                generated_config.display()
            );
        }
        SubCommand::FormatAppSpecificConfiguration(arg) => {
            let file_path = resolve_home_path(arg.path)?;
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
        SubCommand::FetchAppSpecificConfiguration => {
            let content = reqwest::blocking::get("https://raw.githubusercontent.com/LGUG2Z/komorebi-application-specific-configuration/master/applications.yaml")?
                .text()?;

            let output_file = HOME_DIR.join("applications.yaml");

            let mut file = OpenOptions::new()
                .write(true)
                .create(true)
                .truncate(true)
                .open(&output_file)?;

            file.write_all(content.as_bytes())?;

            println!("Latest version of applications.yaml from https://github.com/LGUG2Z/komorebi-application-specific-configuration downloaded\n");
            println!(
               "You can add this to your komorebi.json static configuration file like this: \n\n\"app_specific_configuration_path\": \"{}\"",
               output_file.display()
            );
        }
        SubCommand::ApplicationSpecificConfigurationSchema => {
            let asc = schema_for!(Vec<ApplicationConfiguration>);
            let schema = serde_json::to_string_pretty(&asc)?;
            println!("{schema}");
        }
        SubCommand::NotificationSchema => {
            let notification = schema_for!(Notification);
            let schema = serde_json::to_string_pretty(&notification)?;
            println!("{schema}");
        }
        SubCommand::SocketSchema => {
            let socket_message = schema_for!(SocketMessage);
            let schema = serde_json::to_string_pretty(&socket_message)?;
            println!("{schema}");
        }
        SubCommand::StaticConfigSchema => {
            let settings = SchemaSettings::default().with(|s| {
                s.option_nullable = false;
                s.option_add_null_type = false;
                s.inline_subschemas = true;
            });

            let gen = settings.into_generator();
            let socket_message = gen.into_root_schema_for::<StaticConfig>();
            let schema = serde_json::to_string_pretty(&socket_message)?;
            println!("{schema}");
        }
        SubCommand::GenerateStaticConfig => {
            print_query(&SocketMessage::GenerateStaticConfig);
        }
        // Deprecated
        SubCommand::AltFocusHack(_) | SubCommand::IdentifyBorderOverflowApplication(_) => {
            println!("Command deprecated - this is now automatically handled by komorebi! 🎉");
        }
    }

    Ok(())
}

fn show_window(hwnd: HWND, command: SHOW_WINDOW_CMD) {
    // BOOL is returned but does not signify whether or not the operation was succesful
    // https://docs.microsoft.com/en-us/windows/win32/api/winuser/nf-winuser-showwindow
    // TODO: error handling
    unsafe {
        let _ = ShowWindow(hwnd, command);
    };
}

fn remove_transparency(hwnd: isize) {
    let _ = komorebi_client::Window::from(hwnd).opaque();
}

fn restore_window(hwnd: isize) {
    show_window(HWND(hwnd as *mut core::ffi::c_void), SW_RESTORE);
    remove_transparency(hwnd);
}
