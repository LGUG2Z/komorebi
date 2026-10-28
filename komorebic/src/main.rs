//! `komorebic`, the command line interface of komorebi for KDE Plasma.
//!
//! A subcommand that asks something of the daemon is one message on its socket: an order, such as
//! `stop` or `retile`, is written and `komorebic` exits; a question, such as `state` or `query`,
//! is written and the answer is printed. The messages are [`komorebi_client::SocketMessage`]s,
//! sent with [`komorebi_client::send_message`] and [`komorebi_client::send_query`].
//!
//! The orders that change what the daemon tiles:
//!
//! - `focus` and `cycle-focus` give the focus to another window of the focused workspace, and
//!   `mouse-follows-focus` and `toggle-mouse-follows-focus` set whether the pointer goes with it.
//! - `move` and `cycle-move` move the focused window to another tile.
//! - `change-layout`, `cycle-layout` and `flip-layout` change the layout of the focused
//!   workspace, `scrolling-layout-columns` and `layout-ratios` tune it, `toggle-tiling` turns its
//!   tiling off and on, and `preselect-direction` and `cancel-preselect` reserve the place of the
//!   next window and give it up.
//! - `stack` puts the focused window in one container with the window on one side of it, so that
//!   they share a tile and only one is in sight, `unstack` takes it out again, and `stack-all` and
//!   `unstack-all` do it for the whole workspace and for the focused stack. `cycle-stack` brings
//!   the next or the previous window of the focused stack to the front, `cycle-stack-index` moves
//!   the front window along the stack, and `focus-stack-window` brings the window at an index to
//!   the front. `toggle-window-container-behaviour` and
//!   `toggle-workspace-window-container-behaviour` make a window that opens join the focused
//!   container instead of getting a tile, and `toggle-float-override` and
//!   `toggle-workspace-float-override` make it float.
//! - `promote`, `promote-swap`, `promote-focus` and `promote-window` bring a window to the largest
//!   tile, or the focus to it.
//! - `toggle-float` takes the focused window out of the layout, or puts it back,
//!   `toggle-workspace-layer` switches the focused workspace between its tiled windows and its
//!   floating ones, and `toggle-lock` locks the focused container at its index, or unlocks it.
//! - `toggle-monocle` gives the focused container the whole work area, or puts it back, and
//!   `monocle-focus-behaviour` and `toggle-monocle-focus-behaviour` set what focusing in a
//!   direction does meanwhile. `toggle-maximize` has KWin maximize the focused window, which
//!   leaves the layout, and restores it.
//! - `focused-workspace-padding`, `focused-workspace-container-padding`, `workspace-padding`,
//!   `container-padding` and the two `adjust-*-padding` set the padding around the tiles and
//!   between them, and `global-work-area-offset`, `monitor-work-area-offset`,
//!   `workspace-work-area-offset` and `toggle-window-based-work-area-offset` keep parts of the work
//!   area free of tiles.
//! - `minimize` and `close` minimize and close the focused window, `unmanage` takes it out of
//!   the layout until `manage` takes it back, `unmanaged-window-operation-behaviour` sets whether
//!   the daemon carries out an order while the focused window is not one it tiles, and
//!   `toggle-pause` pauses the daemon and resumes it.
//! - `resize-edge` and `resize-axis` resize the focused window by the `resize-delta`, and
//!   `quick-save-resize`, `quick-load-resize`, `save-resize` and `load-resize` save the sizes of
//!   the tiles of the focused workspace and load them back.
//! - `focus-workspace` shows another workspace of the focused monitor, `move-to-workspace` moves
//!   the focused window to a workspace and shows it, and `send-to-workspace` moves it there and
//!   stays. A workspace is KWin's virtual desktop of the same number, counted from 0.
//!   `cycle-workspace`, `cycle-empty-workspace`, `cycle-move-to-workspace` and
//!   `cycle-send-to-workspace` go to the next or the previous workspace, `focus-last-workspace`,
//!   `move-to-last-workspace` and `send-to-last-workspace` to the one that was in front before,
//!   `focus-workspaces` shows a workspace on every monitor, and `new-workspace` and
//!   `close-workspace` add a workspace and remove an empty one.
//! - `focus-monitor`, `cycle-monitor` and `focus-monitor-at-cursor` focus another monitor, and
//!   `focus-monitor-workspace` a workspace of another monitor; `move-to-monitor`,
//!   `send-to-monitor`, their `cycle-` forms, `move-to-monitor-workspace`,
//!   `send-to-monitor-workspace`, `move-to-named-workspace` and `send-to-named-workspace` take the
//!   focused window to another monitor; `move-workspace-to-monitor`,
//!   `cycle-move-workspace-to-monitor` and `swap-workspaces-with-monitor` take workspaces there;
//!   `cross-monitor-move-behaviour` and `toggle-cross-monitor-move-behaviour` set what `move` does
//!   at the edge of a monitor, and `display-index-preference` gives a monitor an index.
//!   `eager-focus` focuses the first window of an executable, on any workspace of any monitor.
//! - `workspace-name` names a workspace, which `focus-named-workspace`, `named-workspace-layout`,
//!   `named-workspace-tiling`, `named-workspace-padding` and `named-workspace-container-padding`
//!   find by its name; `workspace-layout` and `workspace-tiling` set the layout and the tiling of
//!   a workspace by its index, and `ensure-workspaces` and `ensure-named-workspaces` give a
//!   monitor workspaces up to a number, or with names.
//!
//! `ignore-rule` and `manage-rule` add a rule that keeps an application out of the layout or
//! always in it, `session-float-rule` floats the focused window and every window like it until the
//! daemon stops, `session-float-rules` shows those rules and `clear-session-float-rules` removes
//! them, and `global-state` shows the rules the daemon has.
//!
//! `workspace-layout-rule` and `named-workspace-layout-rule` make a workspace change layout once
//! it has so many windows, and `clear-workspace-layout-rules` and
//! `clear-named-workspace-layout-rules` take those rules away. `generate-static-config` prints the
//! settings of the daemon as a `komorebi.json`, and `replace-configuration` makes the daemon
//! start over with another one.
//!
//! `subscribe-socket` and `unsubscribe-socket` add and remove a subscriber: a program that listens
//! on a socket in the daemon's data directory, to which the daemon sends a notification after each
//! event and each message it handles.
//!
//! The others work without the daemon: `quickstart` writes an example `komorebi.json` and
//! `applications.json` in the config home, `enable-autostart` and `disable-autostart` write and
//! remove the entry that has Plasma start komorebi at login, `start` runs `komorebi` in the
//! background, with the
//! `komorebi.json` it is given with `--config`, `configuration` shows the path of the
//! `komorebi.json` the daemon reads, `log` follows its log, `data-directory` shows where its
//! socket is, `completions` prints a completion script for a shell, and `static-config-schema`,
//! `application-specific-configuration-schema`, `notification-schema` and `socket-schema` print
//! the JSON Schema of `komorebi.json`, of an `applications.json`, of a notification and of a
//! message. The hidden `docgen` writes the help of every subcommand as Markdown, wrapped
//! at 100 columns whatever the width of the terminal, which is how `docs/cli` is made.

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
use komorebi_client::PathExt;
use komorebi_client::Rect;
use komorebi_client::Sizing;
use komorebi_client::SocketMessage;
use komorebi_client::StateQuery;
use komorebi_client::replace_env_in_path;
use komorebi_client::send_message;
use komorebi_client::send_query;
use lazy_static::lazy_static;
use std::fs::File;
use std::io::BufRead;
use std::io::Write;
use std::num::NonZeroUsize;
use std::path::PathBuf;
use std::process::Command;
use std::process::Stdio;
use std::sync::atomic::AtomicBool;
use std::sync::atomic::Ordering;
use std::time::Duration;
use sysinfo::ProcessesToUpdate;

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
    // WatchConfiguration: BooleanState,
    MouseFollowsFocus: BooleanState,
    Query: StateQuery,
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
    /// Desired monocle focus behavior
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
    /// Arguments to pass to komorebi, after a `--`: `komorebic start -- --log-level debug`
    #[clap(last = true)]
    komorebi_args: Vec<String>,
}

#[derive(Parser)]
struct Stop {
    /// Do not restore windows after stopping komorebi
    #[clap(long, hide = true)]
    ignore_restore: bool,
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
}

#[derive(Parser)]
struct EagerFocus {
    /// Case-sensitive exe identifier
    exe: String,
}

#[derive(Parser)]
struct ScrollingLayoutColumns {
    /// Desired number of visible columns
    count: NonZeroUsize,
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
#[clap(author, about, version = version::LONG_VERSION)]
struct Opts {
    #[clap(subcommand)]
    subcmd: SubCommand,
}

#[derive(Parser)]
enum SubCommand {
    #[clap(hide = true)]
    Docgen(Docgen),
    /// Generate komorebic CLI completions for the target shell
    #[clap(arg_required_else_help = true)]
    Completions(Completions),
    /// Gather example configurations for a new-user quickstart
    Quickstart,
    /// Start komorebi as a background process
    Start(Start),
    /// Stop the komorebi process
    Stop(Stop),
    /// Show the path to komorebi.json
    #[clap(alias = "config")]
    Configuration,
    /// Show the path to komorebi's data directory, which has its socket
    #[clap(alias = "datadir")]
    DataDirectory,
    /// Show a JSON representation of the current window manager state
    State,
    /// Show a JSON representation of the current global state
    GlobalState,
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
    /// Set the number of visible columns for the Scrolling layout on the focused workspace
    #[clap(arg_required_else_help = true)]
    ScrollingLayoutColumns(ScrollingLayoutColumns),
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
    /// Toggle the behavior for new windows (stacking or dynamic tiling)
    ToggleWindowContainerBehaviour,
    /// Enable or disable float override, which makes it so every new window opens in floating mode
    ToggleFloatOverride,
    /// Toggle the behavior for new windows (stacking or dynamic tiling) for currently focused
    /// workspace. If there was no behavior set for the workspace previously it takes the opposite
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
    /// Toggle the compositor's maximization for the focused window
    ToggleMaximize,
    /// Toggle a lock for the focused container, ensuring it will not be displaced by any new windows
    ToggleLock,
    /// Force komorebi to manage the focused window
    Manage,
    /// Unmanage a window that was forcibly managed
    Unmanage,
    /// Set the behaviour when moving windows across monitor boundaries
    #[clap(arg_required_else_help = true)]
    CrossMonitorMoveBehaviour(CrossMonitorMoveBehaviour),
    /// Toggle the behaviour when moving windows across monitor boundaries
    ToggleCrossMonitorMoveBehaviour,
    /// Set the behavior when focusing in a direction while a monocle container is active
    #[clap(arg_required_else_help = true)]
    MonocleFocusBehaviour(MonocleFocusBehaviour),
    /// Toggle the behavior when focusing in a direction while a monocle container is active
    ToggleMonocleFocusBehaviour,
    /// Set the operation behavior when the focused window is not managed
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
    /// Enable or disable mouse follows focus on all workspaces
    #[clap(arg_required_else_help = true)]
    MouseFollowsFocus(MouseFollowsFocus),
    /// Toggle mouse follows focus on all workspaces
    ToggleMouseFollowsFocus,
    /// Replace the configuration of a running instance of komorebi from a static configuration file
    #[clap(arg_required_else_help = true)]
    ReplaceConfiguration(ReplaceConfiguration),
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
    /// Writes an XDG autostart entry so komorebi starts with the Plasma session
    EnableAutostart(EnableAutostart),
    /// Removes the XDG autostart entry
    DisableAutostart,
}

/// `~/.config/autostart/komorebi.desktop`, which Plasma runs at login
fn autostart_entry() -> PathBuf {
    dirs::config_dir()
        .expect("there is no config directory")
        .join("autostart")
        .join("komorebi.desktop")
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
                    // the same pages whatever the width of the terminal docgen runs in
                    let help_text = cmd.clone().term_width(100).render_long_help().to_string();
                    let outpath = output_dir.join(format!("{name}.md"));
                    let markdown = format!("```text\n{help_text}\n```\n");
                    std::fs::write(&outpath, markdown)?;
                    println!("{}", outpath.display());
                }
            }
        }
        SubCommand::Completions(args) => {
            let mut cli = Opts::command();
            clap_complete::generate(args.shell, &mut cli, "komorebic", &mut std::io::stdout());
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

            std::fs::create_dir_all(&*HOME_DIR)?;
            std::fs::create_dir_all(&*komorebi_client::DATA_DIR)?;

            let komorebi_json = include_str!("../../docs/komorebi.example.json").to_string();
            let komorebi_path = HOME_DIR.join("komorebi.json");
            let applications_path = HOME_DIR.join("applications.json");

            let mut written_files = Vec::new();

            write_file_with_prompt(&komorebi_path, &komorebi_json, &mut written_files)?;

            let applications_json = include_str!("../applications.json");
            write_file_with_prompt(&applications_path, applications_json, &mut written_files)?;
            if written_files.is_empty() {
                println!("\nNo files were written.")
            } else {
                println!(
                    "\nThe following example files were written:\n{}",
                    written_files.join("\n")
                );
            }

            println!("\nYou can now run: komorebic start");
        }
        SubCommand::EnableAutostart(args) => {
            let exec = match args.config {
                None => String::from("komorebi"),
                Some(config) => format!("komorebi --config \"{}\"", config.display()),
            };

            let target = autostart_entry();
            if let Some(dir) = target.parent() {
                std::fs::create_dir_all(dir)?;
            }

            std::fs::write(
                &target,
                format!(
                    "[Desktop Entry]\nType=Application\nName=komorebi\nComment=A tiling window manager for KDE Plasma\nExec={exec}\nX-KDE-autostart-after=plasma-workspace\nTerminal=false\n"
                ),
            )?;

            println!("Wrote {}", target.display());
        }
        SubCommand::DisableAutostart => {
            let target = autostart_entry();
            if target.exists() {
                std::fs::remove_file(&target)?;
                println!("Deleted {}", target.display());
            } else {
                println!("There is no autostart entry at {}", target.display());
            }
        }
        SubCommand::Start(args) => {
            let mut command = &mut Command::new("komorebi");

            if let Some(config) = &args.config {
                command = command.args(["--config", &config.display().to_string()])
            };

            command = command
                .args(&args.komorebi_args)
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
                std::io::stdout().flush()?;
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
                let mut command = Command::new("komorebi");

                if let Some(config) = &args.config {
                    command.args(["--config", &config.display().to_string()]);
                }

                if let Ok(output) = command.args(&args.komorebi_args).output() {
                    println!("{}", String::from_utf8(output.stderr)?);
                }
            }
        }
        SubCommand::Stop(args) => {
            if args.ignore_restore {
                send_message(&SocketMessage::StopIgnoreRestore)?;
            } else {
                send_message(&SocketMessage::Stop)?;
            }
        }
        SubCommand::Configuration => {
            let static_config = HOME_DIR.join("komorebi.json");

            if static_config.exists() {
                println!("{}", static_config.display());
            }
        }
        SubCommand::DataDirectory => {
            let dir = &*komorebi_client::DATA_DIR;
            if dir.exists() {
                println!("{}", dir.display());
            }
        }
        SubCommand::Log => {
            let timestamp = Utc::now().format("%Y-%m-%d").to_string();
            let color_log = komorebi_client::LOG_DIR.join(format!("komorebi.log.{timestamp}"));
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
        SubCommand::ToggleMonocle => {
            send_message(&SocketMessage::ToggleMonocle)?;
        }
        SubCommand::ToggleMaximize => {
            send_message(&SocketMessage::ToggleMaximize)?;
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
        SubCommand::State => {
            print_query(&SocketMessage::State);
        }
        SubCommand::GlobalState => {
            print_query(&SocketMessage::GlobalState);
        }
        SubCommand::Query(args) => {
            print_query(&SocketMessage::Query(args.state_query));
        }
        SubCommand::MonitorInformation => {
            print_query(&SocketMessage::MonitorInformation);
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
        SubCommand::CrossMonitorMoveBehaviour(args) => {
            send_message(&SocketMessage::CrossMonitorMoveBehaviour(
                args.move_behaviour,
            ))?;
        }
        SubCommand::DisplayIndexPreference(args) => {
            send_message(&SocketMessage::DisplayIndexPreference(
                args.index_preference,
                args.display,
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
        SubCommand::ReplaceConfiguration(args) => {
            send_message(&SocketMessage::ReplaceConfiguration(args.path))?;
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
    }

    Ok(())
}
