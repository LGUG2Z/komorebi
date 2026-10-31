#![warn(clippy::all)]

use chrono::Utc;
use clap::CommandFactory;
use clap::Parser;
use clap::ValueEnum;
use color_eyre::eyre;
use fs_tail::TailedFile;
use komorebi_client::ApplicationIdentifier;
use komorebi_client::Axis;
use komorebi_client::CycleDirection;
use komorebi_client::DefaultLayout;
use komorebi_client::MoveBehaviour;
use komorebi_client::OperationBehaviour;
use komorebi_client::OperationDirection;
use komorebi_client::PanDirection;
use komorebi_client::PathExt;
use komorebi_client::Rect;
use komorebi_client::Sizing;
use komorebi_client::SocketMessage;
use komorebi_client::StateQuery;
use komorebi_client::WindowKind;
use komorebi_client::replace_env_in_path;
use komorebi_client::send_message;
use komorebi_client::send_query;
use komorebi_client::splash;
use komorebi_client::splash::ValidationFeedback;
use lazy_static::lazy_static;
use std::fs::File;
use std::fs::OpenOptions;
use std::io;
use std::io::BufRead;
use std::io::BufReader;
use std::io::Write;
use std::num::NonZeroUsize;
use std::path::PathBuf;
use std::process::Command;
use std::process::Stdio;
use std::sync::atomic::AtomicBool;
use std::sync::atomic::Ordering;
use std::time::Duration;
use sysinfo::ProcessesToUpdate;

mod pan;

lazy_static! {
    static ref HAS_CUSTOM_CONFIG_HOME: AtomicBool = AtomicBool::new(false);
    static ref HOME_DIR: PathBuf = {
        std::env::var("KOMOREBI_CONFIG_HOME").map_or_else(
            |_| dirs::home_dir().expect("there is no home directory").join(".config").join("komorebi"),
            |home_path| {
                let home = home_path.replace_env();
                if home.as_path().is_dir() {
                    HAS_CUSTOM_CONFIG_HOME.store(true, Ordering::SeqCst);
                    home
                } else {
                    panic!(
                        "$KOMOREBI_CONFIG_HOME is set to \"{home_path}\", which is not a valid directory",
                    );
                }
            },
        )
    };
    static ref DATA_DIR: PathBuf = dirs::data_local_dir()
        .expect("there is no local data directory")
        .join("komorebi");
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
            pastey::paste! {
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
    PreselectDirection: OperationDirection,
    CycleFocus: CycleDirection,
    CycleMove: CycleDirection,
    CycleMoveToWorkspace: CycleDirection,
    CycleSendToWorkspace: CycleDirection,
    CycleSendToMonitor: CycleDirection,
    CycleMoveToMonitor: CycleDirection,
    CycleMonitor: CycleDirection,
    CycleWorkspace: CycleDirection,
    CycleEmptyWorkspace: CycleDirection,
    CycleMoveWorkspaceToMonitor: CycleDirection,
    Stack: OperationDirection,
    CycleStack: CycleDirection,
    CycleStackIndex: CycleDirection,
    FlipLayout: Axis,
    ChangeLayout: DefaultLayout,
    CycleLayout: CycleDirection,
    Consume: OperationDirection,
    // WatchConfiguration: BooleanState,
    MouseFollowsFocus: BooleanState,
    Query: StateQuery,
    // WindowHidingBehaviour: HidingBehaviour,
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

#[derive(Parser)]
struct MonocleFocusBehaviour {
    /// Desired monocle focus behaviour
    #[clap(value_enum)]
    behaviour: komorebi_client::MonocleFocusBehaviour,
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
        pastey::paste! {
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
        pastey::paste! {
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
    // IdentifyTrayApplication,
    // IdentifyLayeredApplication,
    // IdentifyObjectNameChangeApplication,
    // IdentifyBorderOverflowApplication,
    // RemoveTitleBar,
}

#[derive(Parser)]
pub struct ClearWorkspaceLayoutRules {
    /// Monitor index (zero-indexed)
    monitor: usize,

    /// Workspace index on the specified monitor (zero-indexed)
    workspace: usize,
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
struct WorkspaceWorkAreaOffset {
    /// Monitor index (zero-indexed)
    monitor: usize,
    /// Workspace index (zero-indexed)
    workspace: usize,
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
#[allow(clippy::struct_excessive_bools)]
struct Start {
    /// Path to a static configuration JSON file
    #[clap(short, long)]
    #[clap(value_parser = replace_env_in_path)]
    config: Option<PathBuf>,
    /// Start komorebi-bar in a background process
    #[clap(long)]
    bar: bool,
    // /// Do not attempt to auto-apply a dumped state temp file from a previously running instance of komorebi
    // #[clap(long)]
    // clean_state: bool,
}

#[derive(Parser)]
struct Stop {
    /// Do not restore windows after stopping komorebi
    #[clap(long, hide = true)]
    ignore_restore: bool,
    /// Stop komorebi-bar if it is running as a background process
    #[clap(long)]
    bar: bool,
}

#[derive(Parser)]
struct SaveResize {
    /// File to which the resize layout dimensions should be saved
    #[clap(value_parser = replace_env_in_path)]
    path: PathBuf,
}

#[derive(Parser)]
struct LoadResize {
    /// File from which the resize layout dimensions should be loaded
    #[clap(value_parser = replace_env_in_path)]
    path: PathBuf,
}

#[derive(Parser)]
struct ReplaceConfiguration {
    /// Static configuration JSON file from which the configuration should be loaded
    #[clap(value_parser = replace_env_in_path)]
    path: PathBuf,
}

#[derive(Parser)]
struct EnableAutostart {
    /// Path to a static configuration JSON file
    #[clap(action, short, long)]
    #[clap(value_parser = replace_env_in_path)]
    config: Option<PathBuf>,
    /// Enable autostart of komorebi-bar
    #[clap(long)]
    bar: bool,
}

#[derive(Parser)]
struct EagerFocus {
    /// Case-sensitive exe identifier
    exe: String,
}

#[derive(Parser)]
struct ScrollingLayoutColumns {
    /// Number of columns that fit across the screen
    count: NonZeroUsize,
}

#[derive(Parser)]
struct Pan {
    /// The way that the view goes
    #[clap(
        value_enum,
        required_unless_present = "stream",
        conflicts_with = "stream"
    )]
    direction: Option<PanDirection>,
    /// Read the gestures of fingers on a touchpad from standard input, in the gesture stream that
    /// whkd writes for a swipe of three or four fingers, and pan with them
    #[clap(long)]
    stream: bool,
    /// What the distances of a gesture are multiplied by, which is 1 if it is left out, with
    /// --stream
    #[clap(long)]
    sensitivity: Option<f64>,
    /// Go the other way to the fingers, with --stream
    #[clap(long)]
    reversed: bool,
    /// Take a step for every two centimetres that the fingers go, and not move the view with
    /// them, with --stream
    #[clap(long)]
    paging: bool,
}

#[derive(Parser)]
struct LayoutRatios {
    /// Column width ratios (space-separated values between 0.1 and 0.9)
    #[clap(short, long, num_args = 1..)]
    columns: Option<Vec<f32>>,
    /// Row height ratios (space-separated values between 0.1 and 0.9)
    #[clap(short, long, num_args = 1..)]
    rows: Option<Vec<f32>>,
}

#[derive(Parser)]
struct Border {
    #[clap(value_enum)]
    boolean_state: BooleanState,
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
struct Animation {
    #[clap(value_enum)]
    boolean_state: BooleanState,
    /// Animation type to apply the state to. If not specified, sets global state
    #[clap(value_enum, short, long)]
    animation_type: Option<komorebi_client::AnimationPrefix>,
}

#[derive(Parser)]
struct AnimationDuration {
    /// Desired animation durations in ms
    duration: u64,
    /// Animation type to apply the duration to. If not specified, sets global duration
    #[clap(value_enum, short, long)]
    animation_type: Option<komorebi_client::AnimationPrefix>,
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
    /// Animation type to apply the style to. If not specified, sets global style
    #[clap(value_enum, short, long)]
    animation_type: Option<komorebi_client::AnimationPrefix>,
}

#[derive(Parser)]
struct Docgen {
    /// Output directory for generated documentation files
    #[clap(short, long)]
    output: Option<PathBuf>,
}

#[derive(Parser)]
struct Completions {
    #[clap(value_enum)]
    shell: clap_complete::Shell,
}

#[derive(Parser)]
struct License {
    /// Email address associated with an Individual Commercial Use License
    email: String,
}

#[derive(Parser)]
struct Splash {
    mdm_server: Option<String>,
}

#[derive(Parser)]
#[clap(author, about, version = version::LONG_VERSION)]
struct Opts {
    #[clap(subcommand)]
    subcmd: SubCommand,
}

#[derive(Parser)]
enum SubCommand {
    #[clap(hide = true)]
    Docgen(Docgen),
    #[clap(hide = true)]
    Splash(Splash),
    /// Generate komorebic CLI completions for the target shell
    #[clap(arg_required_else_help = true)]
    Completions(Completions),
    /// Show the System Settings permissions dialogs required by komorebi
    Permissions,
    /// Gather example configurations for a new-user quickstart
    Quickstart,
    /// Specify an email associated with an Individual Commercial Use License
    #[clap(arg_required_else_help = true)]
    License(License),
    /// Start komorebi as a background process
    Start(Start),
    /// Stop the komorebi process and restore all hidden windows
    Stop(Stop),
    // /// Kill background processes started by komorebic
    // Kill(Kill),
    // /// Check komorebi configuration and related files for common errors
    // Check(Check),
    /// Show the path to komorebi.json
    #[clap(alias = "config")]
    Configuration,
    // /// Show the path to komorebi.bar.json
    // #[clap(alias = "bar-config")]
    // #[clap(alias = "bconfig")]
    // BarConfiguration,
    // /// Show the path to whkdrc
    // #[clap(alias = "whkd")]
    // Whkdrc,
    /// Show the path to komorebi's data directory in $HOME/Library/Application Support
    #[clap(alias = "datadir")]
    DataDirectory,
    /// Show a JSON representation of the current window manager state
    State,
    /// Show a JSON representation of the current global state
    GlobalState,
    // /// Launch the komorebi-gui debugging tool
    // Gui,
    // /// Toggle the komorebi-shortcuts helper
    // ToggleShortcuts,
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
    // /// Subscribe to komorebi events using a Named Pipe
    // #[clap(arg_required_else_help = true)]
    // #[clap(alias = "subscribe")]
    // SubscribePipe(SubscribePipe),
    // /// Unsubscribe from komorebi events
    // #[clap(arg_required_else_help = true)]
    // #[clap(alias = "unsubscribe")]
    // UnsubscribePipe(UnsubscribePipe),
    /// Tail komorebi's process logs (cancel with Ctrl-C)
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
    /// Preselect the specified direction for the next window to be spawned on supported layouts
    #[clap(arg_required_else_help = true)]
    PreselectDirection(PreselectDirection),
    /// Cancel a workspace preselect set by the preselect-direction command, if one exists
    CancelPreselect,
    /// Minimize the focused window
    Minimize,
    /// Close the focused window
    Close,
    // /// Forcibly focus the window at the cursor with a left mouse click
    // ForceFocus,
    /// Change focus to the window in the specified cycle direction
    #[clap(arg_required_else_help = true)]
    CycleFocus(CycleFocus),
    /// Move the focused window in the specified cycle direction
    #[clap(arg_required_else_help = true)]
    CycleMove(CycleMove),
    /// Focus the first managed window matching the given executable
    #[clap(arg_required_else_help = true)]
    EagerFocus(EagerFocus),
    /// Stack the focused window in the specified direction
    #[clap(arg_required_else_help = true)]
    Stack(Stack),
    /// Unstack the focused window
    Unstack,
    /// Cycle the focused stack in the specified cycle direction
    #[clap(arg_required_else_help = true)]
    CycleStack(CycleStack),
    /// Cycle the index of the focused window in the focused stack in the specified cycle direction
    #[clap(arg_required_else_help = true)]
    CycleStackIndex(CycleStackIndex),
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
    /// Send the focused window to the last focused monitor workspace
    SendToLastWorkspace,
    /// Move the focused window to the last focused monitor workspace
    MoveToLastWorkspace,
    /// Focus the specified monitor
    #[clap(arg_required_else_help = true)]
    FocusMonitor(FocusMonitor),
    /// Focus the monitor at the current cursor location
    FocusMonitorAtCursor,
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
    /// Close the focused workspace (must be empty and unnamed)
    CloseWorkspace,
    /// Focus the monitor in the given cycle direction
    #[clap(arg_required_else_help = true)]
    CycleMonitor(CycleMonitor),
    /// Focus the workspace in the given cycle direction
    #[clap(arg_required_else_help = true)]
    CycleWorkspace(CycleWorkspace),
    /// Focus the next empty workspace in the given cycle direction (if one exists)
    #[clap(arg_required_else_help = true)]
    CycleEmptyWorkspace(CycleWorkspace),
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
    // /// Set the invisible border dimensions around each window
    // #[clap(arg_required_else_help = true)]
    // InvisibleBorders(InvisibleBorders),
    /// Set offsets to exclude parts of the work area from tiling
    #[clap(arg_required_else_help = true)]
    GlobalWorkAreaOffset(GlobalWorkAreaOffset),
    /// Set offsets for a monitor to exclude parts of the work area from tiling
    #[clap(arg_required_else_help = true)]
    MonitorWorkAreaOffset(MonitorWorkAreaOffset),
    /// Set offsets for a workspace to exclude parts of the work area from tiling
    #[clap(arg_required_else_help = true)]
    WorkspaceWorkAreaOffset(WorkspaceWorkAreaOffset),
    /// Toggle application of the window-based work area offset for the focused workspace
    ToggleWindowBasedWorkAreaOffset,
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
    /// Set how many columns fit across the Scrolling layout on the focused workspace
    #[clap(arg_required_else_help = true)]
    ScrollingLayoutColumns(ScrollingLayoutColumns),
    /// Make the focused column of the Scrolling layout as wide as the screen, or put it back
    ScrollingLayoutFullWidth,
    /// Scroll the Scrolling layout so that the focused column is in the middle of the screen, once
    ///
    /// Only the Scrolling layout scrolls, so this does nothing in the other layouts.
    CenterFocus,
    /// Make the focused column of the Scrolling layout fill the room that the columns in view
    /// leave on the screen, which does nothing in any other layout
    Expand,
    /// Take the window on one side into the focused column
    ///
    /// Left takes the last window of the column on the left to the top of the focused column, and
    /// right takes the first window of the column on the right to the bottom of it. Up and down do
    /// nothing. Only the Scrolling layout has columns of several windows, so this does nothing in
    /// the other layouts.
    #[clap(arg_required_else_help = true)]
    Consume(Consume),
    /// Take the focused window out of its column and into a column of its own
    ///
    /// Only the Scrolling layout has columns of several windows, so this does nothing in the other
    /// layouts.
    Expel,
    /// Stack the windows of the focused column, or spread a stack out into a column
    ///
    /// A column of several windows becomes one stack of all of them, which takes the height of the
    /// column and shows one window at a time, and the usual stack commands work on it. A stack
    /// becomes a column with a window in each slot, in the order of its tabs. Only the Scrolling
    /// layout has columns of several windows, so this does nothing in the other layouts.
    ToggleColumnStack,
    /// Move the view of the Scrolling layout a column to the left or the right, or with the
    /// fingers on a touchpad
    ///
    /// The strip of columns moves by the width of a new column and comes to rest with a column
    /// against an edge of the screen. Focus stays on the column that has it if that is still fully
    /// in view, and otherwise goes to the nearest column that is. Only the Scrolling layout has a
    /// strip to move, so this does nothing in the other layouts.
    ///
    /// With --stream the strip follows the fingers as they go, 10 centimetres of travel being the
    /// width of the screen, until they lift, and then comes to rest in the column that the speed
    /// that they lifted at takes it to. Standard input is the gesture stream that whkd writes on
    /// Windows, one message to a line: `begin`, `move dx=<cm> t=<ms>`, `end` and `cancel`. A
    /// gesture that is not finished when standard input closes is given up, and the strip goes back
    /// to where it was.
    #[clap(arg_required_else_help = true)]
    Pan(Pan),
    /// Set the layout column and row ratios for the focused workspace
    LayoutRatios(LayoutRatios),
    // /// Load a custom layout from file for the focused workspace
    // #[clap(hide = true)]
    // #[clap(arg_required_else_help = true)]
    // LoadCustomLayout(LoadCustomLayout),
    /// Flip the layout on the focused workspace
    #[clap(arg_required_else_help = true)]
    FlipLayout(FlipLayout),
    /// Promote the focused window to the largest tile via container removal and re-insertion
    Promote,
    /// Promote the focused window to the largest tile by swapping container indices with the largest tile
    PromoteSwap,
    /// Promote the user focus to the largest window container
    PromoteFocus,
    /// Promote the window in the specified direction
    PromoteWindow(PromoteWindow),
    /// Force the retiling of all managed windows
    ///
    /// Every column of a workspace in the Scrolling layout goes back to the width that
    /// scrolling-layout-columns gives, and every window in a column of several goes back to an
    /// equal share of its height.
    Retile,
    // /// Set the monitor index preference for a monitor identified using its size
    // #[clap(arg_required_else_help = true)]
    // MonitorIndexPreference(MonitorIndexPreference),
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
    // /// Set a custom layout for the specified workspace
    // #[clap(hide = true)]
    // #[clap(arg_required_else_help = true)]
    // WorkspaceCustomLayout(WorkspaceCustomLayout),
    // /// Set a custom layout for the specified workspace
    // #[clap(hide = true)]
    // #[clap(arg_required_else_help = true)]
    // NamedWorkspaceCustomLayout(NamedWorkspaceCustomLayout),
    /// Add a dynamic layout rule for the specified workspace
    #[clap(arg_required_else_help = true)]
    WorkspaceLayoutRule(WorkspaceLayoutRule),
    /// Add a dynamic layout rule for the specified workspace
    #[clap(arg_required_else_help = true)]
    NamedWorkspaceLayoutRule(NamedWorkspaceLayoutRule),
    // /// Add a dynamic custom layout for the specified workspace
    // #[clap(hide = true)]
    // #[clap(arg_required_else_help = true)]
    // WorkspaceCustomLayoutRule(WorkspaceCustomLayoutRule),
    // /// Add a dynamic custom layout for the specified workspace
    // #[clap(hide = true)]
    // #[clap(arg_required_else_help = true)]
    // NamedWorkspaceCustomLayoutRule(NamedWorkspaceCustomLayoutRule),
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
    /// Toggle between the Tiling and Floating layers on the focused workspace
    ToggleWorkspaceLayer,
    /// Toggle the paused state for all window tiling
    TogglePause,
    /// Toggle window tiling on the focused workspace
    ToggleTiling,
    /// Toggle floating mode for the focused window
    ToggleFloat,
    /// Toggle monocle mode for the focused container
    ToggleMonocle,
    // /// Toggle native maximization for the focused window
    // ToggleMaximize,
    /// Toggle a lock for the focused container, ensuring it will not be displaced by any new windows
    ToggleLock,
    // /// Restore all hidden windows (debugging command)
    // RestoreWindows,
    /// Force komorebi to manage the focused window
    Manage,
    /// Unmanage a window that was forcibly managed
    Unmanage,
    /// Replace the configuration of a running instance of komorebi from a static configuration file
    #[clap(arg_required_else_help = true)]
    ReplaceConfiguration(ReplaceConfiguration),
    // /// Reload legacy komorebi.ahk or komorebi.ps1 configurations (if they exist)
    // ReloadConfiguration,
    // /// Enable or disable watching of legacy komorebi.ahk or komorebi.ps1 configurations (if they exist)
    // #[clap(arg_required_else_help = true)]
    // WatchConfiguration(WatchConfiguration),
    // /// For legacy komorebi.ahk or komorebi.ps1 configurations, signal that the final configuration option has been sent
    // CompleteConfiguration,
    // /// DEPRECATED since v0.1.22
    // #[clap(arg_required_else_help = true)]
    // #[clap(hide = true)]
    // AltFocusHack(AltFocusHack),
    // /// Set the window behaviour when switching workspaces / cycling stacks
    // #[clap(arg_required_else_help = true)]
    // WindowHidingBehaviour(WindowHidingBehaviour),
    /// Set the behaviour when moving windows across monitor boundaries
    #[clap(arg_required_else_help = true)]
    CrossMonitorMoveBehaviour(CrossMonitorMoveBehaviour),
    /// Toggle the behaviour when moving windows across monitor boundaries
    ToggleCrossMonitorMoveBehaviour,
    /// Set the behaviour when focusing in a direction while a monocle container is active
    #[clap(arg_required_else_help = true)]
    MonocleFocusBehaviour(MonocleFocusBehaviour),
    /// Toggle the behaviour when focusing in a direction while a monocle container is active
    ToggleMonocleFocusBehaviour,
    /// Set the operation behaviour when the focused window is not managed
    #[clap(arg_required_else_help = true)]
    UnmanagedWindowOperationBehaviour(UnmanagedWindowOperationBehaviour),
    /// Add a rule to float the foreground window for the rest of this session
    SessionFloatRule,
    /// Show all session float rules
    SessionFloatRules,
    /// Clear all session float rules
    ClearSessionFloatRules,
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
    /// Enforce all workspace rules, including initial workspace rules that have already been applied
    EnforceWorkspaceRules,
    // /// Identify an application that sends EVENT_OBJECT_NAMECHANGE on launch
    // #[clap(arg_required_else_help = true)]
    // IdentifyObjectNameChangeApplication(IdentifyObjectNameChangeApplication),
    // /// Identify an application that closes to the system tray
    // #[clap(arg_required_else_help = true)]
    // IdentifyTrayApplication(IdentifyTrayApplication),
    // /// Identify an application that has WS_EX_LAYERED, but should still be managed
    // #[clap(arg_required_else_help = true)]
    // IdentifyLayeredApplication(IdentifyLayeredApplication),
    // /// Whitelist an application for title bar removal
    // #[clap(arg_required_else_help = true)]
    // RemoveTitleBar(RemoveTitleBar),
    // /// Toggle title bars for whitelisted applications
    // ToggleTitleBars,
    // /// Identify an application that has overflowing borders
    // #[clap(hide = true)]
    // #[clap(alias = "identify-border-overflow")]
    // IdentifyBorderOverflowApplication(IdentifyBorderOverflowApplication),
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
    // /// Set the border style
    // #[clap(arg_required_else_help = true)]
    // BorderStyle(BorderStyle),
    // /// Set the border implementation
    // #[clap(arg_required_else_help = true)]
    // BorderImplementation(BorderImplementation),
    // /// Set the stackbar mode
    // #[clap(arg_required_else_help = true)]
    // StackbarMode(StackbarMode),
    // /// Enable or disable transparency for unfocused windows
    // #[clap(arg_required_else_help = true)]
    // Transparency(Transparency),
    // /// Set the alpha value for unfocused window transparency
    // #[clap(arg_required_else_help = true)]
    // TransparencyAlpha(TransparencyAlpha),
    // /// Toggle transparency for unfocused windows
    // ToggleTransparency,
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
    // /// Enable or disable focus follows mouse for the operating system
    // #[clap(hide = true)]
    // #[clap(arg_required_else_help = true)]
    // FocusFollowsMouse(FocusFollowsMouse),
    // /// Toggle focus follows mouse for the operating system
    // #[clap(hide = true)]
    // #[clap(arg_required_else_help = true)]
    // ToggleFocusFollowsMouse(ToggleFocusFollowsMouse),
    /// Enable or disable mouse follows focus on all workspaces
    #[clap(arg_required_else_help = true)]
    MouseFollowsFocus(MouseFollowsFocus),
    /// Toggle mouse follows focus on all workspaces
    ToggleMouseFollowsFocus,
    // /// Generate common app-specific configurations and fixes to use in komorebi.ahk
    // #[clap(arg_required_else_help = true)]
    // #[clap(alias = "ahk-asc")]
    // AhkAppSpecificConfiguration(AhkAppSpecificConfiguration),
    // /// Generate common app-specific configurations and fixes in a PowerShell script
    // #[clap(arg_required_else_help = true)]
    // #[clap(alias = "pwsh-asc")]
    // PwshAppSpecificConfiguration(PwshAppSpecificConfiguration),
    // /// Convert a v1 ASC YAML file to a v2 ASC JSON file
    // #[clap(arg_required_else_help = true)]
    // #[clap(alias = "convert-asc")]
    // ConvertAppSpecificConfiguration(ConvertAppSpecificConfiguration),
    // /// Format a YAML file for use with the 'app-specific-configuration' command
    // #[clap(arg_required_else_help = true)]
    // #[clap(alias = "fmt-asc")]
    // #[clap(hide = true)]
    // FormatAppSpecificConfiguration(FormatAppSpecificConfiguration),
    /// Fetch the latest version of applications.json from komorebi-application-specific-configuration
    #[clap(alias = "fetch-asc")]
    FetchAppSpecificConfiguration,
    /// Generate a JSON Schema for applications.json
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
    /// Generates and loads the com.lgug2z.komorebi.plist file in $HOME/Library/LaunchAgents
    EnableAutostart(EnableAutostart),
    /// Unloads and deletes the com.lgug2z.komorebi.plist file in $HOME/Library/LaunchAgents
    DisableAutostart,
}

fn print_query(message: &SocketMessage) {
    match send_query(message) {
        Ok(response) => println!("{response}"),
        Err(error) => panic!("{}", error),
    }
}

fn main() -> eyre::Result<()> {
    let opts: Opts = Opts::parse();

    match opts.subcmd {
        SubCommand::Docgen(args) => {
            let mut cli = Opts::command();
            let subcommands = cli.get_subcommands_mut();

            let output_dir = args.output.unwrap_or_else(|| PathBuf::from("docs/cli"));
            std::fs::create_dir_all(&output_dir)?;

            let ignore = [
                "docgen",
                "splash",
                "alt-focus-hack",
                "identify-border-overflow-application",
                "load-custom-layout",
                "workspace-custom-layout",
                "named-workspace-custom-layout",
                "workspace-custom-layout-rule",
                "named-workspace-custom-layout-rule",
                "focus-follows-mouse",
                "toggle-focus-follows-mouse",
                "format-app-specific-configuration",
            ];

            for cmd in subcommands {
                let name = cmd.get_name().to_string();
                if !ignore.contains(&name.as_str()) {
                    let help_text = cmd.render_long_help().to_string();
                    let outpath = output_dir.join(format!("{name}.md"));
                    let markdown = format!("```\n{help_text}\n```");
                    std::fs::write(&outpath, markdown)?;
                    println!("{}", outpath.display());
                }
            }
        }
        SubCommand::Splash(args) => {
            splash::show(args.mdm_server)?;
        }
        SubCommand::Completions(args) => {
            let mut cli = Opts::command();
            clap_complete::generate(args.shell, &mut cli, "komorebic", &mut std::io::stdout());
        }
        SubCommand::License(args) => {
            let _ = std::fs::remove_file(DATA_DIR.join("icul.validation"));
            std::fs::write(DATA_DIR.join("icul"), &args.email)?;
            match splash::should()? {
                ValidationFeedback::Successful(icul_validation) => {
                    println!("Individual commercial use license validation successful");
                    println!(
                        "Local validation file saved to {}",
                        icul_validation.display()
                    );
                    println!("\n{}", std::fs::read_to_string(&icul_validation)?);
                }
                ValidationFeedback::Unsuccessful(invalid_payload) => {
                    println!(
                        "No active individual commercial use license found for {}",
                        args.email
                    );
                    println!("\n{invalid_payload}");
                    println!(
                        "\nYou can purchase an individual commercial use license at https://lgug2z.com/software/komorebi"
                    );
                }
                ValidationFeedback::NoEmail => {}
                ValidationFeedback::NoConnectivity => {
                    println!(
                        "Could not make a connection to validate an individual commercial use license for {}",
                        args.email
                    );
                }
            }
        }
        SubCommand::Permissions => {
            let mut current_exe = std::env::current_exe().expect("unable to get exec path");
            current_exe.pop();
            let komorebi_exe = current_exe.join("komorebi");

            let term_program = std::env::var("TERM_PROGRAM")
                .unwrap_or_else(|_| String::from("this terminal emulator"));

            println!(
                "\nPlease grant Accessibility permissions to \"{}\" and \"{term_program}\"",
                komorebi_exe.display()
            );
            println!("Accessibility permissions are required for komorebi to to manage windows");
            Command::new("open")
                .arg(
                    "x-apple.systempreferences:com.apple.preference.security?Privacy_Accessibility",
                )
                .spawn()?;

            println!("\nHit return when done...");
            std::io::stdout().flush()?;
            let mut input = String::new();
            std::io::stdin().read_line(&mut input)?;

            println!(
                "Please grant Screen Recording permissions to \"{}\" and \"{term_program}\"",
                komorebi_exe.display()
            );
            println!(
                "Screen Recording permissions are required to for komorebi to read window titles"
            );
            Command::new("open")
                .arg(
                    "x-apple.systempreferences:com.apple.preference.security?Privacy_ScreenCapture",
                )
                .spawn()?;

            println!("\nHit return when done...");
            std::io::stdout().flush()?;
            let mut input = String::new();
            std::io::stdin().read_line(&mut input)?;
        }
        SubCommand::Quickstart => {
            fn write_file_with_prompt(
                path: &PathBuf,
                content: &str,
                created_files: &mut Vec<String>,
            ) -> eyre::Result<()> {
                if path.exists() {
                    print!(
                        "{} will be overwritten, do you want to continue? (y/N): ",
                        path.display()
                    );
                    std::io::stdout().flush()?;
                    let mut input = String::new();
                    std::io::stdin().read_line(&mut input)?;
                    let trimmed = input.trim().to_lowercase();
                    if trimmed == "y" || trimmed == "yes" {
                        std::fs::write(path, content)?;
                        created_files.push(path.display().to_string());
                    } else {
                        println!("Skipping {}", path.display());
                    }
                } else {
                    std::fs::write(path, content)?;
                    created_files.push(path.display().to_string());
                }
                Ok(())
            }

            let local_appdata_dir = dirs::data_local_dir().expect("could not find localdata dir");
            let data_dir = local_appdata_dir.join("komorebi");
            std::fs::create_dir_all(&*HOME_DIR)?;
            std::fs::create_dir_all(data_dir)?;

            let mut komorebi_json = include_str!("../../docs/komorebi.example.json").to_string();
            let komorebi_bar_json =
                include_str!("../../docs/komorebi.bar.example.json").to_string();

            if std::env::var("KOMOREBI_CONFIG_HOME").is_ok() {
                komorebi_json =
                    komorebi_json.replace("Env:USERPROFILE", "Env:KOMOREBI_CONFIG_HOME");
            }

            let komorebi_path = HOME_DIR.join("komorebi.json");
            let bar_path = HOME_DIR.join("komorebi.bar.json");
            let applications_path = HOME_DIR.join("applications.json");
            let skhdrc_path = HOME_DIR.join("skhdrc");

            let mut written_files = Vec::new();

            write_file_with_prompt(&komorebi_path, &komorebi_json, &mut written_files)?;
            write_file_with_prompt(&bar_path, &komorebi_bar_json, &mut written_files)?;

            let applications_json = include_str!("../applications.json");
            write_file_with_prompt(&applications_path, applications_json, &mut written_files)?;

            let whkdrc = include_str!("../../docs/skhdrc.sample");
            write_file_with_prompt(&skhdrc_path, whkdrc, &mut written_files)?;
            if written_files.is_empty() {
                println!("\nNo files were written.")
            } else {
                println!(
                    "\nThe following example files were written:\n{}",
                    written_files.join("\n")
                );
            }

            let mut current_exe = std::env::current_exe().expect("unable to get exec path");
            current_exe.pop();
            let komorebi_exe = current_exe.join("komorebi");

            let term_program = std::env::var("TERM_PROGRAM")
                .unwrap_or_else(|_| String::from("this terminal emulator"));

            println!(
                "\nPlease grant Accessibility permissions to \"{}\" and \"{term_program}\"",
                komorebi_exe.display()
            );
            println!("Accessibility permissions are required for komorebi to to manage windows");
            Command::new("open")
                .arg(
                    "x-apple.systempreferences:com.apple.preference.security?Privacy_Accessibility",
                )
                .spawn()?;

            println!("\nHit return when done...");
            std::io::stdout().flush()?;
            let mut input = String::new();
            std::io::stdin().read_line(&mut input)?;

            println!(
                "Please grant Screen Recording permissions to \"{}\" and \"{term_program}\"",
                komorebi_exe.display()
            );
            println!(
                "Screen Recording permissions are required to for komorebi to read window titles"
            );
            Command::new("open")
                .arg(
                    "x-apple.systempreferences:com.apple.preference.security?Privacy_ScreenCapture",
                )
                .spawn()?;

            println!("\nHit return when done...");
            std::io::stdout().flush()?;
            let mut input = String::new();
            std::io::stdin().read_line(&mut input)?;

            if let Ok((mdm, server)) = splash::mdm_enrollment()
                && mdm
            {
                if let Some(server) = server {
                    println!(
                        "It looks like you are using a corporate device enrolled in mobile device management ({server})\n"
                    );
                } else {
                    println!(
                        "It looks like you are using a corporate device enrolled in mobile device management\n"
                    );
                }
                println!("The Komorebi License does not permit any kind of commercial use\n");
                println!(
                    "A dedicated Individual Commercial Use License is available if you wish to use this software at work\n"
                );
                println!(
                    "You are strongly encouraged to make your employer pay for your license, either directly or via reimbursement\n"
                );
                println!(
                    "If you already have a license, you can run \"komorebic license <email>\" with the email address your license is associated with\n"
                );
            }

            println!("You can now run: komorebic start");
            println!(
                "\nIf you have skhd hotkey daemon installed, you can also run: skhd --config {}",
                skhdrc_path.display()
            );
        }
        SubCommand::EnableAutostart(args) => {
            let mut current_exe = std::env::current_exe().expect("unable to get exec path");
            current_exe.pop();
            let komorebic_exe = current_exe.join("komorebi");

            let plist = match args.config {
                None => {
                    format!(
                        r#"
           <?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple Computer//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
        <key>EnvironmentVariables</key>
        <dict>
                <key>PATH</key>
                <string>$HOME/.nix-profile/bin:/etc/profiles/per-user/$USER/bin:/run/current-system/sw/bin:/nix/var/nix/profiles/default/bin:/usr/local/bin:/usr/bin:/bin:/usr/sbin:/sbin</string>
        </dict>
        <key>RunAtLoad</key>
        <true/>
        <key>KeepAlive</key>
        <true/>
        <key>Label</key>
        <string>com.lgug2z.komorebi</string>
        <key>ProcessType</key>
        <string>Interactive</string>
        <key>ProgramArguments</key>
        <array>
                <string>{}</string>
        </array>
        <key>StandardOutPath</key>
        <string>/tmp/komorebi.out.log</string>
        <key>StandardErrorPath</key>
        <string>/tmp/komorebi.err.log</string>
</dict>
</plist>"#,
                        komorebic_exe.to_string_lossy()
                    )
                }
                Some(config) => {
                    format!(
                        r#"
           <?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple Computer//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
        <key>EnvironmentVariables</key>
        <dict>
                <key>PATH</key>
                <string>$HOME/.nix-profile/bin:/etc/profiles/per-user/$USER/bin:/run/current-system/sw/bin:/nix/var/nix/profiles/default/bin:/usr/local/bin:/usr/bin:/bin:/usr/sbin:/sbin</string>
        </dict>
        <key>RunAtLoad</key>
        <true/>
        <key>KeepAlive</key>
        <true/>
        <key>Label</key>
        <string>com.lgug2z.komorebi</string>
        <key>ProcessType</key>
        <string>Interactive</string>
        <key>ProgramArguments</key>
        <array>
                <string>{}</string>
                <string>--config</string>
                <string>{}</string>
        </array>
        <key>StandardOutPath</key>
        <string>/tmp/komorebi.out.log</string>
        <key>StandardErrorPath</key>
        <string>/tmp/komorebi.err.log</string>
</dict>
</plist>"#,
                        komorebic_exe.to_string_lossy(),
                        config.to_string_lossy()
                    )
                }
            };

            let target = dirs::home_dir()
                .expect("no $HOME dir")
                .join("Library")
                .join("LaunchAgents")
                .join("com.lgug2z.komorebi.plist");

            std::fs::write(&target, &plist)?;

            println!("Created com.lgug2z.komorebi.plist at {}", target.display());
            Command::new("launchctl")
                .args(["load", &*target.to_string_lossy()])
                .output()?;
            println!("Ran 'launchctl load {}'", target.display());

            if args.bar {
                let komorebi_bar_exe = current_exe.join("komorebi-bar");
                let komorebic_exe_for_bar = current_exe.join("komorebic");

                // Create a launcher script that waits for komorebi to be healthy
                let launcher_script = format!(
                    r#"#!/bin/sh

# Wait for komorebi to be healthy
max_attempts=30
attempt=0

while [ $attempt -lt $max_attempts ]; do
    if {} state >/dev/null 2>&1; then
        # komorebi is healthy, start the bar
        exec {}
    fi
    attempt=$((attempt + 1))
    sleep 1
done

# If we get here, komorebi failed to start
echo "komorebi-bar: Timeout waiting for komorebi to become healthy" >&2
exit 1
"#,
                    komorebic_exe_for_bar.to_string_lossy(),
                    komorebi_bar_exe.to_string_lossy()
                );

                let launcher_path = DATA_DIR.join("komorebi-bar-launcher.sh");
                std::fs::write(&launcher_path, &launcher_script)?;

                // Make the script executable
                #[cfg(unix)]
                {
                    use std::os::unix::fs::PermissionsExt;
                    let mut perms = std::fs::metadata(&launcher_path)?.permissions();
                    perms.set_mode(0o755);
                    std::fs::set_permissions(&launcher_path, perms)?;
                }

                let bar_plist = format!(
                    r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple Computer//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
        <key>EnvironmentVariables</key>
        <dict>
                <key>PATH</key>
                <string>$HOME/.nix-profile/bin:/etc/profiles/per-user/$USER/bin:/run/current-system/sw/bin:/nix/var/nix/profiles/default/bin:/usr/local/bin:/usr/bin:/bin:/usr/sbin:/sbin</string>
        </dict>
        <key>RunAtLoad</key>
        <true/>
        <key>KeepAlive</key>
        <dict>
                <key>SuccessfulExit</key>
                <false/>
        </dict>
        <key>Label</key>
        <string>com.lgug2z.komorebi-bar</string>
        <key>ProcessType</key>
        <string>Interactive</string>
        <key>ProgramArguments</key>
        <array>
                <string>{}</string>
        </array>
        <key>StandardOutPath</key>
        <string>/tmp/komorebi-bar.out.log</string>
        <key>StandardErrorPath</key>
        <string>/tmp/komorebi-bar.err.log</string>
</dict>
</plist>"#,
                    launcher_path.to_string_lossy()
                );

                let bar_target = dirs::home_dir()
                    .expect("no $HOME dir")
                    .join("Library")
                    .join("LaunchAgents")
                    .join("com.lgug2z.komorebi-bar.plist");

                std::fs::write(&bar_target, &bar_plist)?;
                println!(
                    "Created com.lgug2z.komorebi-bar.plist at {}",
                    bar_target.display()
                );

                Command::new("launchctl")
                    .args(["load", &*bar_target.to_string_lossy()])
                    .output()?;
                println!("Ran 'launchctl load {}'", bar_target.display());
            }
        }
        SubCommand::DisableAutostart => {
            let target = dirs::home_dir()
                .expect("no $HOME dir")
                .join("Library")
                .join("LaunchAgents")
                .join("com.lgug2z.komorebi.plist");

            Command::new("launchctl")
                .args(["unload", &*target.to_string_lossy()])
                .output()?;
            println!("Ran 'launchctl unload {}'", target.display());
            std::fs::remove_file(&target)?;
            println!("Deleted {}", target.display());

            // Also remove bar plist if it exists
            let bar_target = dirs::home_dir()
                .expect("no $HOME dir")
                .join("Library")
                .join("LaunchAgents")
                .join("com.lgug2z.komorebi-bar.plist");

            if bar_target.exists() {
                Command::new("launchctl")
                    .args(["unload", &*bar_target.to_string_lossy()])
                    .output()?;
                println!("Ran 'launchctl unload {}'", bar_target.display());
                std::fs::remove_file(&bar_target)?;
                println!("Deleted {}", bar_target.display());
            }

            // Clean up launcher script if it exists
            let launcher_path = DATA_DIR.join("komorebi-bar-launcher.sh");
            if launcher_path.exists() {
                std::fs::remove_file(&launcher_path)?;
                println!("Deleted {}", launcher_path.display());
            }
        }
        SubCommand::Start(args) => {
            let mut command = &mut Command::new("komorebi");

            if let Some(config) = &args.config {
                command =
                    command.args(["--config", &format!("'--config=\"{}\"'", config.display())])
            };

            command = command
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .stdin(Stdio::null());

            let mut system = sysinfo::System::new_all();
            system.refresh_processes(ProcessesToUpdate::All, true);

            let mut attempts = 0;
            let mut running = system
                .processes_by_exact_name("komorebi".as_ref())
                .next()
                .is_some();

            while !running && attempts <= 2 {
                command.spawn()?;

                print!("Waiting for komorebi to start... ");
                std::thread::sleep(Duration::from_secs(3));

                system.refresh_processes(ProcessesToUpdate::All, true);

                if system
                    .processes_by_exact_name("komorebi".as_ref())
                    .next()
                    .is_some()
                {
                    println!("Started!");
                    running = true;
                } else {
                    println!("komorebi did not start... Trying again");
                    attempts += 1;
                }
            }

            if !running {
                println!("\nRunning komorebi directly for detailed error output\n");
                if let Some(config) = &args.config {
                    if let Ok(output) = Command::new("komorebi")
                        .arg(format!("'--config=\"{}\"'", config.display()))
                        .output()
                    {
                        println!("{}", String::from_utf8(output.stderr)?);
                    }
                } else if let Ok(output) = Command::new("komorebi").output() {
                    println!("{}", String::from_utf8(output.stderr)?);
                }

                return Ok(());
            }

            if args.bar {
                let mut command = &mut Command::new("komorebi-bar");

                command = command
                    .stdout(Stdio::null())
                    .stderr(Stdio::null())
                    .stdin(Stdio::null());

                command.spawn()?;
            }

            println!("\nThank you for using komorebi!\n");
            println!("# Commercial Use License");
            if let Ok((mdm, server)) = splash::mdm_enrollment()
                && mdm
            {
                if let Some(server) = server {
                    println!(
                        "* It looks like you are using a corporate device enrolled in mobile device management ({server})"
                    );
                } else {
                    println!(
                        "* It looks like you are using a corporate device enrolled in mobile device management"
                    );
                }
            }
            println!(
                "* View licensing options https://lgug2z.com/software/komorebi - A commercial use license is required to use komorebi at work"
            );
            println!("\n# Personal Use Sponsorship");
            println!(
                "* Become a sponsor https://github.com/sponsors/LGUG2Z - $10/month makes a big difference"
            );
            println!("* Leave a tip https://ko-fi.com/lgug2z - An alternative to GitHub Sponsors");
            println!("\n# Community");
            println!(
                "* Join the Discord https://discord.gg/mGkn66PHkx - Chat, ask questions, share your desktops"
            );
            println!(
                "* Subscribe to https://youtube.com/@LGUG2Z - Development videos, feature previews and release overviews"
            );
            println!(
                "* Explore the Awesome Komorebi list https://github.com/LGUG2Z/awesome-komorebi - Projects in the komorebi ecosystem"
            );
            println!("\n# Documentation");
            println!(
                "* Read the docs https://lgug2z.github.io/komorebi - Quickly search through all komorebic commands"
            );
        }
        SubCommand::Stop(args) => {
            if args.ignore_restore {
                send_message(&SocketMessage::StopIgnoreRestore)?;
            } else {
                send_message(&SocketMessage::Stop)?;
            }

            if args.bar {
                Command::new("pkill").arg("komorebi-bar").spawn()?;
            }

            // TODO: see if we need a force quit sometimes like we do on Windows
            // let mut system = sysinfo::System::new_all();
            // system.refresh_processes(ProcessesToUpdate::All, true);
            //
            // let running = system.processes_by_exact_name("komorebi".as_ref()).next();
            //
            // if let Some(process) = running {
            //     process.kill_with(Signal::Interrupt);
            // }
        }
        SubCommand::Configuration => {
            let static_config = HOME_DIR.join("komorebi.json");

            if static_config.exists() {
                println!("{}", static_config.display());
            }
        }
        SubCommand::DataDirectory => {
            let dir = &*DATA_DIR;
            if dir.exists() {
                println!("{}", dir.display());
            }
        }
        SubCommand::Log => {
            let timestamp = Utc::now().format("%Y-%m-%d").to_string();
            let color_log = DATA_DIR.join(format!("komorebi.log.{timestamp}"));
            let file = TailedFile::new(File::open(color_log)?);
            let locked = file.lock();
            #[allow(clippy::significant_drop_in_scrutinee, clippy::lines_filter_map_ok)]
            for line in locked.lines().flatten() {
                println!("{line}");
            }
        }
        SubCommand::Focus(args) => {
            send_message(&SocketMessage::FocusWindow(args.operation_direction))?;
        }
        SubCommand::Move(args) => {
            send_message(&SocketMessage::MoveWindow(args.operation_direction))?;
        }
        SubCommand::PreselectDirection(args) => {
            send_message(&SocketMessage::PreselectDirection(args.operation_direction))?;
        }
        SubCommand::CancelPreselect => {
            send_message(&SocketMessage::CancelPreselect)?;
        }
        SubCommand::CycleFocus(args) => {
            send_message(&SocketMessage::CycleFocusWindow(args.cycle_direction))?;
        }
        SubCommand::CycleMove(args) => {
            send_message(&SocketMessage::CycleMoveWindow(args.cycle_direction))?;
        }
        SubCommand::TogglePause => {
            send_message(&SocketMessage::TogglePause)?;
        }
        SubCommand::ChangeLayout(args) => {
            send_message(&SocketMessage::ChangeLayout(args.default_layout))?;
        }
        SubCommand::CycleLayout(args) => {
            send_message(&SocketMessage::CycleLayout(args.cycle_direction))?;
        }
        SubCommand::FlipLayout(args) => {
            send_message(&SocketMessage::FlipLayout(args.axis))?;
        }
        SubCommand::Stack(args) => {
            send_message(&SocketMessage::StackWindow(args.operation_direction))?;
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
        SubCommand::FocusStackWindow(args) => {
            send_message(&SocketMessage::FocusStackWindow(args.target))?;
        }
        SubCommand::CycleStack(args) => {
            send_message(&SocketMessage::CycleStack(args.cycle_direction))?;
        }
        SubCommand::CycleStackIndex(args) => {
            send_message(&SocketMessage::CycleStackIndex(args.cycle_direction))?;
        }
        SubCommand::FocusWorkspace(args) => {
            send_message(&SocketMessage::FocusWorkspaceNumber(args.target))?;
        }
        SubCommand::ToggleMonocle => {
            send_message(&SocketMessage::ToggleMonocle)?;
        }
        SubCommand::ToggleFloat => {
            send_message(&SocketMessage::ToggleFloat)?;
        }
        SubCommand::ToggleWorkspaceLayer => {
            send_message(&SocketMessage::ToggleWorkspaceLayer)?;
        }
        SubCommand::ResizeEdge(args) => {
            send_message(&SocketMessage::ResizeWindowEdge(args.edge, args.sizing))?;
        }
        SubCommand::ResizeAxis(args) => {
            send_message(&SocketMessage::ResizeWindowAxis(args.axis, args.sizing))?;
        }
        SubCommand::Retile => {
            send_message(&SocketMessage::Retile)?;
        }
        SubCommand::Close => {
            send_message(&SocketMessage::Close)?;
        }
        SubCommand::Minimize => {
            send_message(&SocketMessage::Minimize)?;
        }
        SubCommand::Manage => {
            send_message(&SocketMessage::ManageFocusedWindow)?;
        }
        SubCommand::Unmanage => {
            send_message(&SocketMessage::UnmanageFocusedWindow)?;
        }
        SubCommand::Promote => {
            send_message(&SocketMessage::Promote)?;
        }
        SubCommand::PromoteSwap => {
            send_message(&SocketMessage::PromoteSwap)?;
        }
        SubCommand::PromoteFocus => {
            send_message(&SocketMessage::PromoteFocus)?;
        }
        SubCommand::PromoteWindow(args) => {
            send_message(&SocketMessage::PromoteWindow(args.operation_direction))?;
        }
        SubCommand::ToggleWindowBasedWorkAreaOffset => {
            send_message(&SocketMessage::ToggleWindowBasedWorkAreaOffset)?;
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
        SubCommand::ToggleTiling => {
            send_message(&SocketMessage::ToggleTiling)?;
        }
        SubCommand::ToggleLock => {
            send_message(&SocketMessage::ToggleLock)?;
        }
        SubCommand::ToggleCrossMonitorMoveBehaviour => {
            send_message(&SocketMessage::ToggleCrossMonitorMoveBehaviour)?;
        }
        SubCommand::ToggleMonocleFocusBehaviour => {
            send_message(&SocketMessage::ToggleMonocleFocusBehaviour)?;
        }
        SubCommand::FocusMonitor(args) => {
            send_message(&SocketMessage::FocusMonitorNumber(args.target))?;
        }
        SubCommand::FocusMonitorAtCursor => {
            send_message(&SocketMessage::FocusMonitorAtCursor)?;
        }
        SubCommand::FocusLastWorkspace => {
            send_message(&SocketMessage::FocusLastWorkspace)?;
        }
        SubCommand::FocusWorkspaces(args) => {
            send_message(&SocketMessage::FocusWorkspaceNumbers(args.target))?;
        }
        SubCommand::FocusMonitorWorkspace(args) => {
            send_message(&SocketMessage::FocusMonitorWorkspaceNumber(
                args.target_monitor,
                args.target_workspace,
            ))?;
        }
        SubCommand::FocusNamedWorkspace(args) => {
            send_message(&SocketMessage::FocusNamedWorkspace(args.workspace))?;
        }
        SubCommand::CloseWorkspace => {
            send_message(&SocketMessage::CloseWorkspace)?;
        }
        SubCommand::CycleMonitor(args) => {
            send_message(&SocketMessage::CycleFocusMonitor(args.cycle_direction))?;
        }
        SubCommand::CycleWorkspace(args) => {
            send_message(&SocketMessage::CycleFocusWorkspace(args.cycle_direction))?;
        }
        SubCommand::CycleEmptyWorkspace(args) => {
            send_message(&SocketMessage::CycleFocusEmptyWorkspace(
                args.cycle_direction,
            ))?;
        }
        SubCommand::MoveToMonitor(args) => {
            send_message(&SocketMessage::MoveContainerToMonitorNumber(args.target))?;
        }
        SubCommand::CycleMoveToMonitor(args) => {
            send_message(&SocketMessage::CycleMoveContainerToMonitor(
                args.cycle_direction,
            ))?;
        }
        SubCommand::MoveToWorkspace(args) => {
            send_message(&SocketMessage::MoveContainerToWorkspaceNumber(args.target))?;
        }
        SubCommand::MoveToNamedWorkspace(args) => {
            send_message(&SocketMessage::MoveContainerToNamedWorkspace(
                args.workspace,
            ))?;
        }
        SubCommand::CycleMoveToWorkspace(args) => {
            send_message(&SocketMessage::CycleMoveContainerToWorkspace(
                args.cycle_direction,
            ))?;
        }
        SubCommand::SendToMonitor(args) => {
            send_message(&SocketMessage::SendContainerToMonitorNumber(args.target))?;
        }
        SubCommand::CycleSendToMonitor(args) => {
            send_message(&SocketMessage::CycleSendContainerToMonitor(
                args.cycle_direction,
            ))?;
        }
        SubCommand::SendToWorkspace(args) => {
            send_message(&SocketMessage::SendContainerToWorkspaceNumber(args.target))?;
        }
        SubCommand::SendToNamedWorkspace(args) => {
            send_message(&SocketMessage::SendContainerToNamedWorkspace(
                args.workspace,
            ))?;
        }
        SubCommand::CycleSendToWorkspace(args) => {
            send_message(&SocketMessage::CycleSendContainerToWorkspace(
                args.cycle_direction,
            ))?;
        }
        SubCommand::SendToMonitorWorkspace(args) => {
            send_message(&SocketMessage::SendContainerToMonitorWorkspaceNumber(
                args.target_monitor,
                args.target_workspace,
            ))?;
        }
        SubCommand::MoveToMonitorWorkspace(args) => {
            send_message(&SocketMessage::MoveContainerToMonitorWorkspaceNumber(
                args.target_monitor,
                args.target_workspace,
            ))?;
        }
        SubCommand::MoveWorkspaceToMonitor(args) => {
            send_message(&SocketMessage::MoveWorkspaceToMonitorNumber(args.target))?;
        }
        SubCommand::CycleMoveWorkspaceToMonitor(args) => {
            send_message(&SocketMessage::CycleMoveWorkspaceToMonitor(
                args.cycle_direction,
            ))?;
        }
        SubCommand::MoveToLastWorkspace => {
            send_message(&SocketMessage::MoveContainerToLastWorkspace)?;
        }
        SubCommand::SendToLastWorkspace => {
            send_message(&SocketMessage::SendContainerToLastWorkspace)?;
        }
        SubCommand::SwapWorkspacesWithMonitor(args) => {
            send_message(&SocketMessage::SwapWorkspacesToMonitorNumber(args.target))?;
        }
        SubCommand::State => {
            print_query(&SocketMessage::State);
        }
        SubCommand::GlobalState => {
            print_query(&SocketMessage::GlobalState);
        }
        SubCommand::Query(args) => {
            print_query(&SocketMessage::Query(args.state_query));
        }
        SubCommand::VisibleWindows => {
            print_query(&SocketMessage::VisibleWindows);
        }
        SubCommand::MonitorInformation => {
            print_query(&SocketMessage::MonitorInformation);
        }
        SubCommand::FetchAppSpecificConfiguration => {
            let content = reqwest::blocking::get("https://raw.githubusercontent.com/LGUG2Z/komorebi-application-specific-configuration/master/applications.mac.json")?
                .text()?;

            let output_file = HOME_DIR.join("applications.json");

            let mut file = OpenOptions::new()
                .write(true)
                .create(true)
                .truncate(true)
                .open(&output_file)?;

            file.write_all(content.as_bytes())?;

            println!(
                "Latest version of applications.mac.json from https://github.com/LGUG2Z/komorebi-application-specific-configuration downloaded\n"
            );
            println!(
                "You can add this to your komorebi.json static configuration file like this: \n\n\"app_specific_configuration_path\": \"{}\"",
                output_file.display().to_string().replace("\\", "/")
            );
        }
        SubCommand::SessionFloatRule => {
            send_message(&SocketMessage::SessionFloatRule)?;
        }
        SubCommand::SessionFloatRules => {
            print_query(&SocketMessage::SessionFloatRules);
        }
        SubCommand::ClearSessionFloatRules => {
            send_message(&SocketMessage::ClearSessionFloatRules)?;
        }
        SubCommand::IgnoreRule(args) => {
            send_message(&SocketMessage::IgnoreRule(args.identifier, args.id))?;
        }
        SubCommand::ManageRule(args) => {
            send_message(&SocketMessage::ManageRule(args.identifier, args.id))?;
        }
        SubCommand::InitialWorkspaceRule(args) => {
            send_message(&SocketMessage::InitialWorkspaceRule(
                args.identifier,
                args.id,
                args.monitor,
                args.workspace,
            ))?;
        }
        SubCommand::InitialNamedWorkspaceRule(args) => {
            send_message(&SocketMessage::InitialNamedWorkspaceRule(
                args.identifier,
                args.id,
                args.workspace,
            ))?;
        }
        SubCommand::WorkspaceRule(args) => {
            send_message(&SocketMessage::WorkspaceRule(
                args.identifier,
                args.id,
                args.monitor,
                args.workspace,
            ))?;
        }
        SubCommand::NamedWorkspaceRule(args) => {
            send_message(&SocketMessage::NamedWorkspaceRule(
                args.identifier,
                args.id,
                args.workspace,
            ))?;
        }
        SubCommand::ClearWorkspaceRules(args) => {
            send_message(&SocketMessage::ClearWorkspaceRules(
                args.monitor,
                args.workspace,
            ))?;
        }
        SubCommand::ClearNamedWorkspaceRules(args) => {
            send_message(&SocketMessage::ClearNamedWorkspaceRules(args.workspace))?;
        }
        SubCommand::ClearAllWorkspaceRules => {
            send_message(&SocketMessage::ClearAllWorkspaceRules)?;
        }
        SubCommand::EnforceWorkspaceRules => {
            send_message(&SocketMessage::EnforceWorkspaceRules)?;
        }
        SubCommand::EagerFocus(args) => {
            send_message(&SocketMessage::EagerFocus(args.exe))?;
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
        SubCommand::ResizeDelta(args) => {
            send_message(&SocketMessage::ResizeDelta(args.pixels))?;
        }
        SubCommand::MonitorWorkAreaOffset(args) => {
            send_message(&SocketMessage::MonitorWorkAreaOffset(
                args.monitor,
                Rect {
                    left: args.left,
                    top: args.top,
                    right: args.right,
                    bottom: args.bottom,
                },
            ))?;
        }
        SubCommand::GlobalWorkAreaOffset(args) => {
            send_message(&SocketMessage::WorkAreaOffset(Rect {
                left: args.left,
                top: args.top,
                right: args.right,
                bottom: args.bottom,
            }))?;
        }

        SubCommand::WorkspaceWorkAreaOffset(args) => {
            send_message(&SocketMessage::WorkspaceWorkAreaOffset(
                args.monitor,
                args.workspace,
                Rect {
                    left: args.left,
                    top: args.top,
                    right: args.right,
                    bottom: args.bottom,
                },
            ))?;
        }
        SubCommand::ContainerPadding(args) => {
            send_message(&SocketMessage::ContainerPadding(
                args.monitor,
                args.workspace,
                args.size,
            ))?;
        }
        SubCommand::NamedWorkspaceContainerPadding(args) => {
            send_message(&SocketMessage::NamedWorkspaceContainerPadding(
                args.workspace,
                args.size,
            ))?;
        }
        SubCommand::WorkspacePadding(args) => {
            send_message(&SocketMessage::WorkspacePadding(
                args.monitor,
                args.workspace,
                args.size,
            ))?;
        }
        SubCommand::NamedWorkspacePadding(args) => {
            send_message(&SocketMessage::NamedWorkspacePadding(
                args.workspace,
                args.size,
            ))?;
        }
        SubCommand::FocusedWorkspacePadding(args) => {
            send_message(&SocketMessage::FocusedWorkspacePadding(args.size))?;
        }
        SubCommand::FocusedWorkspaceContainerPadding(args) => {
            send_message(&SocketMessage::FocusedWorkspaceContainerPadding(args.size))?;
        }
        SubCommand::AdjustWorkspacePadding(args) => {
            send_message(&SocketMessage::AdjustWorkspacePadding(
                args.sizing,
                args.adjustment,
            ))?;
        }
        SubCommand::AdjustContainerPadding(args) => {
            send_message(&SocketMessage::AdjustContainerPadding(
                args.sizing,
                args.adjustment,
            ))?;
        }
        SubCommand::WorkspaceLayout(args) => {
            send_message(&SocketMessage::WorkspaceLayout(
                args.monitor,
                args.workspace,
                args.value,
            ))?;
        }
        SubCommand::NamedWorkspaceLayout(args) => {
            send_message(&SocketMessage::NamedWorkspaceLayout(
                args.workspace,
                args.value,
            ))?;
        }

        SubCommand::WorkspaceTiling(args) => {
            send_message(&SocketMessage::WorkspaceTiling(
                args.monitor,
                args.workspace,
                args.value.into(),
            ))?;
        }
        SubCommand::NamedWorkspaceTiling(args) => {
            send_message(&SocketMessage::NamedWorkspaceTiling(
                args.workspace,
                args.value.into(),
            ))?;
        }
        SubCommand::ScrollingLayoutColumns(args) => {
            send_message(&SocketMessage::ScrollingLayoutColumns(args.count))?;
        }
        SubCommand::ScrollingLayoutFullWidth => {
            send_message(&SocketMessage::ScrollingLayoutFullWidth)?;
        }
        SubCommand::CenterFocus => {
            send_message(&SocketMessage::CenterFocus)?;
        }
        SubCommand::Expand => {
            send_message(&SocketMessage::Expand)?;
        }
        SubCommand::Consume(args) => {
            send_message(&SocketMessage::Consume(args.operation_direction))?;
        }
        SubCommand::Expel => {
            send_message(&SocketMessage::Expel)?;
        }
        SubCommand::ToggleColumnStack => {
            send_message(&SocketMessage::ToggleColumnStack)?;
        }
        SubCommand::Pan(args) => {
            // a flag that has a default counts as given, so what only goes with --stream
            // cannot be left to the parser to say
            if !args.stream && (args.sensitivity.is_some() || args.reversed || args.paging) {
                Opts::command()
                    .error(
                        clap::error::ErrorKind::ArgumentConflict,
                        "--sensitivity, --reversed and --paging can only be used with --stream",
                    )
                    .exit();
            }

            if args.stream {
                pan::run(
                    BufReader::new(io::stdin()),
                    pan::Options {
                        sensitivity: args.sensitivity.unwrap_or(1.0),
                        reversed: args.reversed,
                        paging: args.paging,
                    },
                    &mut pan::Socket::new(komorebi_client::Connection::open),
                )?;
            } else if let Some(direction) = args.direction {
                send_message(&SocketMessage::Pan(direction))?;
            }
        }
        SubCommand::LayoutRatios(args) => {
            if args.columns.is_none() && args.rows.is_none() {
                println!(
                    "No ratios provided, nothing to change. Use --columns or --rows to specify ratios."
                );
            } else {
                send_message(&SocketMessage::LayoutRatios(args.columns, args.rows))?;
            }
        }
        SubCommand::ClearWorkspaceLayoutRules(args) => {
            send_message(&SocketMessage::ClearWorkspaceLayoutRules(
                args.monitor,
                args.workspace,
            ))?;
        }
        SubCommand::ClearNamedWorkspaceLayoutRules(args) => {
            send_message(&SocketMessage::ClearNamedWorkspaceLayoutRules(
                args.workspace,
            ))?;
        }
        SubCommand::EnsureWorkspaces(workspaces) => {
            send_message(&SocketMessage::EnsureWorkspaces(
                workspaces.monitor,
                workspaces.workspace_count,
            ))?;
        }
        SubCommand::EnsureNamedWorkspaces(args) => {
            send_message(&SocketMessage::EnsureNamedWorkspaces(
                args.monitor,
                args.names,
            ))?;
        }
        SubCommand::WorkspaceLayoutRule(args) => {
            send_message(&SocketMessage::WorkspaceLayoutRule(
                args.monitor,
                args.workspace,
                args.at_container_count,
                args.layout,
            ))?;
        }
        SubCommand::NamedWorkspaceLayoutRule(args) => {
            send_message(&SocketMessage::NamedWorkspaceLayoutRule(
                args.workspace,
                args.at_container_count,
                args.layout,
            ))?;
        }
        SubCommand::CrossMonitorMoveBehaviour(args) => {
            send_message(&SocketMessage::CrossMonitorMoveBehaviour(
                args.move_behaviour,
            ))?;
        }
        SubCommand::MonocleFocusBehaviour(args) => {
            send_message(&SocketMessage::MonocleFocusBehaviour(args.behaviour))?;
        }
        SubCommand::UnmanagedWindowOperationBehaviour(args) => {
            send_message(&SocketMessage::UnmanagedWindowOperationBehaviour(
                args.operation_behaviour,
            ))?;
        }
        SubCommand::ToggleMouseFollowsFocus => {
            send_message(&SocketMessage::ToggleMouseFollowsFocus)?;
        }
        SubCommand::MouseFollowsFocus(args) => {
            send_message(&SocketMessage::MouseFollowsFocus(args.boolean_state.into()))?;
        }
        SubCommand::ApplicationSpecificConfigurationSchema => {
            #[cfg(feature = "schemars")]
            {
                let asc = schemars::schema_for!(komorebi_client::ApplicationSpecificConfiguration);
                let schema = serde_json::to_string_pretty(&asc)?;
                println!("{schema}");
            }
        }
        SubCommand::NotificationSchema => {
            #[cfg(feature = "schemars")]
            {
                let notification = schemars::schema_for!(komorebi_client::Notification);
                let schema = serde_json::to_string_pretty(&notification)?;
                println!("{schema}");
            }
        }
        SubCommand::SocketSchema => {
            #[cfg(feature = "schemars")]
            {
                let socket_message = schemars::schema_for!(SocketMessage);
                let schema = serde_json::to_string_pretty(&socket_message)?;
                println!("{schema}");
            }
        }
        SubCommand::StaticConfigSchema => {
            #[cfg(feature = "schemars")]
            {
                let socket_message = schemars::schema_for!(komorebi_client::StaticConfig);
                let schema = serde_json::to_string_pretty(&socket_message)?;
                println!("{schema}");
            }
        }
        SubCommand::SubscribeSocket(args) => {
            send_message(&SocketMessage::AddSubscriberSocket(args.socket))?;
        }
        SubCommand::UnsubscribeSocket(args) => {
            send_message(&SocketMessage::RemoveSubscriberSocket(args.socket))?;
        }
        SubCommand::GenerateStaticConfig => {
            print_query(&SocketMessage::GenerateStaticConfig);
        }
        SubCommand::QuickSaveResize => {
            send_message(&SocketMessage::QuickSave)?;
        }
        SubCommand::QuickLoadResize => {
            send_message(&SocketMessage::QuickLoad)?;
        }
        SubCommand::SaveResize(args) => {
            send_message(&SocketMessage::Save(args.path))?;
        }
        SubCommand::LoadResize(args) => {
            send_message(&SocketMessage::Load(args.path))?;
        }
        SubCommand::DisplayIndexPreference(args) => {
            send_message(&SocketMessage::DisplayIndexPreference(
                args.index_preference,
                args.display,
            ))?;
        }
        SubCommand::ReplaceConfiguration(args) => {
            send_message(&SocketMessage::ReplaceConfiguration(args.path))?;
        }
        SubCommand::Border(args) => {
            send_message(&SocketMessage::Border(args.boolean_state.into()))?;
        }
        SubCommand::BorderColour(args) => {
            send_message(&SocketMessage::BorderColour(
                args.window_kind,
                args.r,
                args.g,
                args.b,
            ))?;
        }
        SubCommand::BorderWidth(args) => {
            send_message(&SocketMessage::BorderWidth(args.width))?;
        }
        SubCommand::BorderOffset(args) => {
            send_message(&SocketMessage::BorderOffset(args.offset))?;
        }
        SubCommand::Animation(args) => {
            send_message(&SocketMessage::Animation(
                args.boolean_state.into(),
                args.animation_type,
            ))?;
        }
        SubCommand::AnimationDuration(args) => {
            send_message(&SocketMessage::AnimationDuration(
                args.duration,
                args.animation_type,
            ))?;
        }
        SubCommand::AnimationFps(args) => {
            send_message(&SocketMessage::AnimationFps(args.fps))?;
        }
        SubCommand::AnimationStyle(args) => {
            send_message(&SocketMessage::AnimationStyle(
                args.style,
                args.animation_type,
            ))?;
        }
    }

    Ok(())
}
