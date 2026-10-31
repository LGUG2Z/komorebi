//! The subcommands of komorebic for Macos that are not a message to the daemon in every `komorebic`.

use clap::Parser;
use clap::Subcommand;
use komorebi_layouts::*;
use komorebi_protocol::pathext::replace_env_in_path;
use std::path::PathBuf;

#[derive(Parser)]
#[allow(clippy::struct_excessive_bools)]
pub struct Start {
    /// Path to a static configuration JSON file
    #[clap(short, long)]
    #[clap(value_parser = replace_env_in_path)]
    pub config: Option<PathBuf>,
    /// Start komorebi-bar in a background process
    #[clap(long)]
    pub bar: bool,
    // /// Do not attempt to auto-apply a dumped state temp file from a previously running instance of komorebi
    // #[clap(long)]
    // clean_state: bool,
}

#[derive(Parser)]
pub struct Stop {
    /// Do not restore windows after stopping komorebi
    #[clap(long, hide = true)]
    pub ignore_restore: bool,
    /// Stop komorebi-bar if it is running as a background process
    #[clap(long)]
    pub bar: bool,
}

#[derive(Parser)]
pub struct EnableAutostart {
    /// Path to a static configuration JSON file
    #[clap(action, short, long)]
    #[clap(value_parser = replace_env_in_path)]
    pub config: Option<PathBuf>,
    /// Enable autostart of komorebi-bar
    #[clap(long)]
    pub bar: bool,
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
    /// Read the gestures of fingers on a touchpad from standard input, in the gesture stream that
    /// whkd writes for a swipe of three or four fingers, and pan with them
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
pub struct Docgen {
    /// Output directory for generated documentation files
    #[clap(short, long)]
    pub output: Option<PathBuf>,
}

#[derive(Parser)]
pub struct Completions {
    #[clap(value_enum)]
    pub shell: clap_complete::Shell,
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
pub enum MacosCommands {
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
    // /// Launch the komorebi-gui debugging tool
    // Gui,
    // /// Toggle the komorebi-shortcuts helper
    // ToggleShortcuts,
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
    // /// Forcibly focus the window at the cursor with a left mouse click
    // ForceFocus,
    // /// Set the invisible border dimensions around each window
    // #[clap(arg_required_else_help = true)]
    // InvisibleBorders(InvisibleBorders),
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
    // /// Set the monitor index preference for a monitor identified using its size
    // #[clap(arg_required_else_help = true)]
    // MonitorIndexPreference(MonitorIndexPreference),
    // /// Set a custom layout for the specified workspace
    // #[clap(hide = true)]
    // #[clap(arg_required_else_help = true)]
    // WorkspaceCustomLayout(WorkspaceCustomLayout),
    // /// Set a custom layout for the specified workspace
    // #[clap(hide = true)]
    // #[clap(arg_required_else_help = true)]
    // NamedWorkspaceCustomLayout(NamedWorkspaceCustomLayout),
    // /// Add a dynamic custom layout for the specified workspace
    // #[clap(hide = true)]
    // #[clap(arg_required_else_help = true)]
    // WorkspaceCustomLayoutRule(WorkspaceCustomLayoutRule),
    // /// Add a dynamic custom layout for the specified workspace
    // #[clap(hide = true)]
    // #[clap(arg_required_else_help = true)]
    // NamedWorkspaceCustomLayoutRule(NamedWorkspaceCustomLayoutRule),
    // /// Toggle native maximization for the focused window
    // ToggleMaximize,
    // /// Restore all hidden windows (debugging command)
    // RestoreWindows,
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
    // /// Enable or disable focus follows mouse for the operating system
    // #[clap(hide = true)]
    // #[clap(arg_required_else_help = true)]
    // FocusFollowsMouse(FocusFollowsMouse),
    // /// Toggle focus follows mouse for the operating system
    // #[clap(hide = true)]
    // #[clap(arg_required_else_help = true)]
    // ToggleFocusFollowsMouse(ToggleFocusFollowsMouse),
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
