//! The subcommands of komorebic for Windows that are not a message to the daemon in every `komorebic`.

use crate::args::*;
use clap::Parser;
use clap::Subcommand;
use komorebi_layouts::*;
use komorebi_protocol::pathext::replace_env_in_path;
use komorebi_protocol::types::*;
use std::path::PathBuf;

#[derive(Parser)]
pub struct WorkspaceCustomLayout {
    /// Monitor index (zero-indexed)
    pub monitor: usize,

    /// Workspace index on the specified monitor (zero-indexed)
    pub workspace: usize,

    /// JSON or YAML file from which the custom layout definition should be loaded
    #[clap(value_parser = replace_env_in_path)]
    pub path: PathBuf,
}

#[derive(Parser)]
pub struct NamedWorkspaceCustomLayout {
    /// Target workspace name
    pub workspace: String,

    /// JSON or YAML file from which the custom layout definition should be loaded
    #[clap(value_parser = replace_env_in_path)]
    pub path: PathBuf,
}

#[derive(Parser)]
pub struct WorkspaceCustomLayoutRule {
    /// Monitor index (zero-indexed)
    pub monitor: usize,

    /// Workspace index on the specified monitor (zero-indexed)
    pub workspace: usize,

    /// The number of window containers on-screen required to trigger this layout rule
    pub at_container_count: usize,

    /// JSON or YAML file from which the custom layout definition should be loaded
    #[clap(value_parser = replace_env_in_path)]
    pub path: PathBuf,
}

#[derive(Parser)]
pub struct NamedWorkspaceCustomLayoutRule {
    /// Target workspace name
    pub workspace: String,

    /// The number of window containers on-screen required to trigger this layout rule
    pub at_container_count: usize,

    /// JSON or YAML file from which the custom layout definition should be loaded
    #[clap(value_parser = replace_env_in_path)]
    pub path: PathBuf,
}

#[derive(Parser)]
pub struct InvisibleBorders {
    /// Size of the left invisible border
    pub left: i32,
    /// Size of the top invisible border (usually 0)
    pub top: i32,
    /// Size of the right invisible border (usually left * 2)
    pub right: i32,
    /// Size of the bottom invisible border (usually the same as left)
    pub bottom: i32,
}

#[derive(Parser)]
pub struct MonitorIndexPreference {
    /// Preferred monitor index (zero-indexed)
    pub index_preference: usize,
    /// Left value of the monitor's size Rect
    pub left: i32,
    /// Top value of the monitor's size Rect
    pub top: i32,
    /// Right value of the monitor's size Rect
    pub right: i32,
    /// Bottom value of the monitor's size Rect
    pub bottom: i32,
}

#[derive(Parser)]
pub struct ToggleFocusFollowsMouse {
    #[clap(value_enum, short, long, default_value = "windows")]
    pub implementation: FocusFollowsMouseImplementation,
}

#[derive(Parser)]
pub struct FocusFollowsMouse {
    #[clap(value_enum, short, long, default_value = "windows")]
    pub implementation: FocusFollowsMouseImplementation,
    #[clap(value_enum)]
    pub boolean_state: BooleanState,
}

#[derive(Parser)]
pub struct Transparency {
    #[clap(value_enum)]
    pub boolean_state: BooleanState,
}

#[derive(Parser)]
pub struct TransparencyAlpha {
    /// Alpha
    pub alpha: u8,
}

#[derive(Parser)]
pub struct BorderStyle {
    /// Desired border style
    #[clap(value_enum)]
    pub style: komorebi_protocol::types::BorderStyle,
}

#[derive(Parser)]
pub struct BorderImplementation {
    /// Desired border implementation
    #[clap(value_enum)]
    pub style: komorebi_protocol::types::BorderImplementation,
}

#[derive(Parser)]
pub struct StackbarMode {
    /// Desired stackbar mode
    #[clap(value_enum)]
    pub mode: komorebi_protocol::types::StackbarMode,
}

#[derive(Parser)]
pub struct Docgen {
    /// Output directory for generated documentation files
    #[clap(short, long)]
    pub output: Option<PathBuf>,
}

#[derive(Parser)]
#[allow(clippy::struct_excessive_bools)]
pub struct Start {
    /// Allow the use of komorebi's custom focus-follows-mouse implementation
    #[clap(hide = true)]
    #[clap(short, long = "ffm")]
    pub ffm: bool,
    /// Path to a static configuration JSON file
    #[clap(short, long)]
    #[clap(value_parser = replace_env_in_path)]
    pub config: Option<PathBuf>,
    /// Wait for `komorebic complete-configuration` to be sent before processing events
    #[clap(short, long)]
    pub await_configuration: bool,
    /// Start a TCP server on the given port to allow the direct sending of SocketMessages
    #[clap(short, long)]
    pub tcp_port: Option<usize>,
    /// Start whkd in a background process
    #[clap(long)]
    pub whkd: bool,
    /// Start autohotkey configuration file
    #[clap(hide = true)]
    #[clap(long)]
    pub ahk: bool,
    /// Start komorebi-bar in a background process
    #[clap(long)]
    pub bar: bool,
    /// Start masir in a background process for focus-follows-mouse
    #[clap(long)]
    pub masir: bool,
    /// Do not attempt to auto-apply a dumped state temp file from a previously running instance of komorebi
    #[clap(long)]
    pub clean_state: bool,
}

#[derive(Parser)]
pub struct Stop {
    /// Stop whkd if it is running as a background process
    #[clap(long)]
    pub whkd: bool,
    /// Stop ahk if it is running as a background process
    #[clap(hide = true)]
    #[clap(long)]
    pub ahk: bool,
    /// Stop komorebi-bar if it is running as a background process
    #[clap(long)]
    pub bar: bool,
    /// Stop masir if it is running as a background process
    #[clap(long)]
    pub masir: bool,
    /// Do not restore windows after stopping komorebi
    #[clap(long, hide = true)]
    pub ignore_restore: bool,
}

#[derive(Parser)]
pub struct Kill {
    /// Kill whkd if it is running as a background process
    #[clap(long)]
    pub whkd: bool,
    /// Kill ahk if it is running as a background process
    #[clap(hide = true)]
    #[clap(long)]
    pub ahk: bool,
    /// Kill komorebi-bar if it is running as a background process
    #[clap(long)]
    pub bar: bool,
    /// Kill masir if it is running as a background process
    #[clap(long)]
    pub masir: bool,
}

#[derive(Parser)]
pub struct LoadCustomLayout {
    /// JSON or YAML file from which the custom layout definition should be loaded
    #[clap(value_parser = replace_env_in_path)]
    pub path: PathBuf,
}

#[derive(Parser)]
pub struct SubscribePipe {
    /// Name of the pipe to send event notifications to (without "\\.\pipe\" prepended)
    pub named_pipe: String,
}

#[derive(Parser)]
pub struct UnsubscribePipe {
    /// Name of the pipe to stop sending event notifications to (without "\\.\pipe\" prepended)
    pub named_pipe: String,
}

#[derive(Parser)]
pub struct AhkAppSpecificConfiguration {
    /// YAML file from which the application-specific configurations should be loaded
    #[clap(value_parser = replace_env_in_path)]
    pub path: PathBuf,
    /// Optional YAML file of overrides to apply over the first file
    #[clap(value_parser = replace_env_in_path)]
    pub override_path: Option<PathBuf>,
}

#[derive(Parser)]
pub struct PwshAppSpecificConfiguration {
    /// YAML file from which the application-specific configurations should be loaded
    #[clap(value_parser = replace_env_in_path)]
    pub path: PathBuf,
    /// Optional YAML file of overrides to apply over the first file
    #[clap(value_parser = replace_env_in_path)]
    pub override_path: Option<PathBuf>,
}

#[derive(Parser)]
pub struct FormatAppSpecificConfiguration {
    /// YAML file from which the application-specific configurations should be loaded
    #[clap(value_parser = replace_env_in_path)]
    pub path: PathBuf,
}

#[derive(Parser)]
pub struct ConvertAppSpecificConfiguration {
    /// YAML file from which the application-specific configurations should be loaded
    #[clap(value_parser = replace_env_in_path)]
    pub path: PathBuf,
}

#[derive(Parser)]
pub struct AltFocusHack {
    #[clap(value_enum)]
    pub boolean_state: BooleanState,
}

#[derive(Parser)]
pub struct EnableAutostart {
    /// Path to a static configuration JSON file
    #[clap(action, short, long)]
    #[clap(value_parser = replace_env_in_path)]
    pub config: Option<PathBuf>,
    /// Enable komorebi's custom focus-follows-mouse implementation
    #[clap(hide = true)]
    #[clap(short, long = "ffm")]
    pub ffm: bool,
    /// Enable autostart of whkd
    #[clap(long)]
    pub whkd: bool,
    /// Enable autostart of ahk
    #[clap(hide = true)]
    #[clap(long)]
    pub ahk: bool,
    /// Enable autostart of komorebi-bar
    #[clap(long)]
    pub bar: bool,
    /// Enable autostart of masir
    #[clap(long)]
    pub masir: bool,
}

#[derive(Parser)]
pub struct Check {
    /// Path to a static configuration JSON file
    #[clap(action, short, long)]
    #[clap(value_parser = replace_env_in_path)]
    pub komorebi_config: Option<PathBuf>,
}

#[derive(Parser)]
pub struct Pan {
    /// The way that the view goes
    #[clap(
        value_enum,
        required_unless_present = "stream",
        conflicts_with = "stream"
    )]
    pub direction: Option<PanDirection>,
    /// Read the gestures of fingers on a touchpad from standard input, as whkd writes them for
    /// a swipe of three or four fingers, and pan with them
    #[clap(long)]
    pub stream: bool,
    /// What the distances of a gesture are multiplied by, which is 1 if it is left out, with
    /// --stream
    #[clap(long)]
    pub sensitivity: Option<f64>,
    /// Go the other way to the fingers, with --stream
    #[clap(long)]
    pub reversed: bool,
    /// Take a step for every two centimetres that the fingers go, and not move the view with
    /// them, with --stream
    #[clap(long)]
    pub paging: bool,
}

#[derive(Parser)]
pub struct LayoutRatios {
    /// Column width ratios (space-separated values between 0.1 and 0.9)
    #[clap(short, long, num_args = 1..)]
    pub columns: Option<Vec<f32>>,
    /// Row height ratios (space-separated values between 0.1 and 0.9)
    #[clap(short, long, num_args = 1..)]
    pub rows: Option<Vec<f32>>,
}

#[derive(Parser)]
pub struct License {
    /// Email address associated with an Individual Commercial Use License
    pub email: String,
}

#[derive(Parser)]
pub struct Splash {
    pub mdm_server: Option<String>,
}

#[derive(Subcommand)]
pub enum WindowsCommands {
    #[clap(hide = true)]
    Docgen(Docgen),
    #[clap(hide = true)]
    Splash(Splash),
    /// Gather example configurations for a new-user quickstart
    Quickstart,
    /// Specify an email associated with an Individual Commercial Use License
    #[clap(arg_required_else_help = true)]
    License(License),
    /// Start komorebi.exe as a background process
    Start(Start),
    /// Stop the komorebi.exe process and restore all hidden windows
    Stop(Stop),
    /// Kill background processes started by komorebic
    Kill(Kill),
    /// Check komorebi configuration and related files for common errors
    Check(Check),
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
    /// Show the path to komorebi's data directory in %LOCALAPPDATA%
    #[clap(alias = "datadir")]
    DataDirectory,
    /// Launch the komorebi-gui debugging tool
    Gui,
    /// Toggle the komorebi-shortcuts helper
    ToggleShortcuts,
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
    /// Forcibly focus the window at the cursor with a left mouse click
    ForceFocus,
    /// Set the invisible border dimensions around each window
    #[clap(arg_required_else_help = true)]
    InvisibleBorders(InvisibleBorders),
    /// Move the view of the Scrolling layout a column to the left or the right, or with the
    /// fingers on a touchpad, which does nothing in any other layout
    #[clap(arg_required_else_help = true)]
    Pan(Pan),
    /// Set the layout column and row ratios for the focused workspace
    LayoutRatios(LayoutRatios),
    /// Load a custom layout from file for the focused workspace
    #[clap(hide = true)]
    #[clap(arg_required_else_help = true)]
    LoadCustomLayout(LoadCustomLayout),
    /// Set the monitor index preference for a monitor identified using its size
    #[clap(arg_required_else_help = true)]
    MonitorIndexPreference(MonitorIndexPreference),
    /// Set a custom layout for the specified workspace
    #[clap(hide = true)]
    #[clap(arg_required_else_help = true)]
    WorkspaceCustomLayout(WorkspaceCustomLayout),
    /// Set a custom layout for the specified workspace
    #[clap(hide = true)]
    #[clap(arg_required_else_help = true)]
    NamedWorkspaceCustomLayout(NamedWorkspaceCustomLayout),
    /// Add a dynamic custom layout for the specified workspace
    #[clap(hide = true)]
    #[clap(arg_required_else_help = true)]
    WorkspaceCustomLayoutRule(WorkspaceCustomLayoutRule),
    /// Add a dynamic custom layout for the specified workspace
    #[clap(hide = true)]
    #[clap(arg_required_else_help = true)]
    NamedWorkspaceCustomLayoutRule(NamedWorkspaceCustomLayoutRule),
    /// Toggle native maximization for the focused window
    ToggleMaximize,
    /// Restore all hidden windows (debugging command)
    RestoreWindows,
    /// Reload legacy komorebi.ahk or komorebi.ps1 configurations (if they exist)
    ReloadConfiguration,
    /// Enable or disable watching of legacy komorebi.ahk or komorebi.ps1 configurations (if they exist)
    #[clap(arg_required_else_help = true)]
    WatchConfiguration(OneEnum<BooleanState>),
    /// For legacy komorebi.ahk or komorebi.ps1 configurations, signal that the final configuration option has been sent
    CompleteConfiguration,
    /// DEPRECATED since v0.1.22
    #[deprecated(note = "No longer required")]
    #[clap(arg_required_else_help = true)]
    #[clap(hide = true)]
    AltFocusHack(AltFocusHack),
    /// Set the window behaviour when switching workspaces / cycling stacks
    #[clap(arg_required_else_help = true)]
    WindowHidingBehaviour(OneEnum<HidingBehaviour>),
    /// Identify an application that sends EVENT_OBJECT_NAMECHANGE on launch
    #[clap(arg_required_else_help = true)]
    IdentifyObjectNameChangeApplication(ApplicationTarget),
    /// Identify an application that closes to the system tray
    #[clap(arg_required_else_help = true)]
    IdentifyTrayApplication(ApplicationTarget),
    /// Identify an application that has WS_EX_LAYERED, but should still be managed
    #[clap(arg_required_else_help = true)]
    IdentifyLayeredApplication(ApplicationTarget),
    /// Whitelist an application for title bar removal
    #[clap(arg_required_else_help = true)]
    RemoveTitleBar(ApplicationTarget),
    /// Toggle title bars for whitelisted applications
    ToggleTitleBars,
    /// Identify an application that has overflowing borders
    #[clap(hide = true)]
    #[clap(alias = "identify-border-overflow")]
    IdentifyBorderOverflowApplication(ApplicationTarget),
    /// Set the border style
    #[clap(arg_required_else_help = true)]
    BorderStyle(BorderStyle),
    /// Set the border implementation
    #[clap(arg_required_else_help = true)]
    BorderImplementation(BorderImplementation),
    /// Set the stackbar mode
    #[clap(arg_required_else_help = true)]
    StackbarMode(StackbarMode),
    /// Enable or disable transparency for unfocused windows
    #[clap(arg_required_else_help = true)]
    Transparency(Transparency),
    /// Set the alpha value for unfocused window transparency
    #[clap(arg_required_else_help = true)]
    TransparencyAlpha(TransparencyAlpha),
    /// Toggle transparency for unfocused windows
    ToggleTransparency,
    /// Enable or disable focus follows mouse for the operating system
    #[clap(hide = true)]
    #[clap(arg_required_else_help = true)]
    FocusFollowsMouse(FocusFollowsMouse),
    /// Toggle focus follows mouse for the operating system
    #[clap(hide = true)]
    #[clap(arg_required_else_help = true)]
    ToggleFocusFollowsMouse(ToggleFocusFollowsMouse),
    /// Generate common app-specific configurations and fixes to use in komorebi.ahk
    #[clap(arg_required_else_help = true)]
    #[clap(alias = "ahk-asc")]
    AhkAppSpecificConfiguration(AhkAppSpecificConfiguration),
    /// Generate common app-specific configurations and fixes in a PowerShell script
    #[clap(arg_required_else_help = true)]
    #[clap(alias = "pwsh-asc")]
    PwshAppSpecificConfiguration(PwshAppSpecificConfiguration),
    /// Convert a v1 ASC YAML file to a v2 ASC JSON file
    #[clap(arg_required_else_help = true)]
    #[clap(alias = "convert-asc")]
    ConvertAppSpecificConfiguration(ConvertAppSpecificConfiguration),
    /// Format a YAML file for use with the `app-specific-configuration` command
    #[clap(arg_required_else_help = true)]
    #[clap(alias = "fmt-asc")]
    #[clap(hide = true)]
    FormatAppSpecificConfiguration(FormatAppSpecificConfiguration),
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
    /// Generates the komorebi.lnk shortcut in shell:startup to autostart komorebi
    EnableAutostart(EnableAutostart),
    /// Deletes the komorebi.lnk shortcut in shell:startup to disable autostart
    DisableAutostart,
}
