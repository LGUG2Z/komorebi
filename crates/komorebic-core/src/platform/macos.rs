//! The subcommands of komorebic for Macos that are not a message to the daemon in every `komorebic`.

use crate::args::*;
use clap::Parser;
use clap::Subcommand;
use komorebi_layouts::*;
use komorebi_protocol::animation::prefix::AnimationPrefix;
use komorebi_protocol::pathext::replace_env_in_path;
use komorebi_protocol::types::*;
use std::num::NonZeroUsize;
use std::path::PathBuf;

#[derive(Parser)]
pub struct MonocleFocusBehaviour {
    /// Desired monocle focus behaviour
    #[clap(value_enum)]
    pub behaviour: komorebi_protocol::types::MonocleFocusBehaviour,
}

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
pub struct EagerFocus {
    /// Case-sensitive exe identifier
    pub exe: String,
}

#[derive(Parser)]
pub struct ScrollingLayoutColumns {
    /// Number of columns that fit across the screen
    pub count: NonZeroUsize,
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
pub struct BorderColour {
    #[clap(value_enum, short, long, default_value = "single")]
    pub window_kind: WindowKind,
    /// Red
    pub r: u32,
    /// Green
    pub g: u32,
    /// Blue
    pub b: u32,
}

#[derive(Parser)]
pub struct Animation {
    #[clap(value_enum)]
    pub boolean_state: BooleanState,
    /// Animation type to apply the state to. If not specified, sets global state
    #[clap(value_enum, short, long)]
    pub animation_type: Option<AnimationPrefix>,
}

#[derive(Parser)]
pub struct AnimationDuration {
    /// Desired animation durations in ms
    pub duration: u64,
    /// Animation type to apply the duration to. If not specified, sets global duration
    #[clap(value_enum, short, long)]
    pub animation_type: Option<AnimationPrefix>,
}

#[derive(Parser)]
pub struct AnimationStyle {
    /// Desired ease function for animation
    #[clap(value_enum, short, long, default_value = "linear")]
    pub style: komorebi_protocol::animation::AnimationStyle,
    /// Animation type to apply the style to. If not specified, sets global style
    #[clap(value_enum, short, long)]
    pub animation_type: Option<AnimationPrefix>,
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
    /// Focus the first managed window matching the given executable
    #[clap(arg_required_else_help = true)]
    EagerFocus(EagerFocus),
    // /// Set the invisible border dimensions around each window
    // #[clap(arg_required_else_help = true)]
    // InvisibleBorders(InvisibleBorders),
    /// Set how many columns fit across the Scrolling layout on the focused workspace
    #[clap(arg_required_else_help = true)]
    ScrollingLayoutColumns(ScrollingLayoutColumns),
    /// Make the focused column of the Scrolling layout as wide as the screen, or put it back
    ScrollingLayoutFullWidth,
    /// Scroll the Scrolling layout so that the focused column is in the middle of the screen, once
    ///
    /// Only the Scrolling layout scrolls, so this does nothing in the other layouts.
    CenterFocus,
    /// Take the window on one side into the focused column
    ///
    /// Left takes the last window of the column on the left to the top of the focused column, and
    /// right takes the first window of the column on the right to the bottom of it. Up and down do
    /// nothing. Only the Scrolling layout has columns of several windows, so this does nothing in
    /// the other layouts.
    #[clap(arg_required_else_help = true)]
    Consume(OneEnum<OperationDirection>),
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
    /// Promote the user focus to the largest window container
    PromoteFocus,
    /// Force the retiling of all managed windows
    ///
    /// Every column of a workspace in the Scrolling layout goes back to the width that
    /// scrolling-layout-columns gives, and every window in a column of several goes back to an
    /// equal share of its height.
    Retile,
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
    /// Toggle the behaviour for new windows (stacking or dynamic tiling)
    ToggleWindowContainerBehaviour,
    /// Toggle the behaviour for new windows (stacking or dynamic tiling) for currently focused
    /// workspace. If there was no behaviour set for the workspace previously it takes the opposite
    /// of the global value.
    ToggleWorkspaceWindowContainerBehaviour,
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
    /// Set the behaviour when focusing in a direction while a monocle container is active
    #[clap(arg_required_else_help = true)]
    MonocleFocusBehaviour(MonocleFocusBehaviour),
    /// Toggle the behaviour when focusing in a direction while a monocle container is active
    ToggleMonocleFocusBehaviour,
    /// Set the operation behaviour when the focused window is not managed
    #[clap(arg_required_else_help = true)]
    UnmanagedWindowOperationBehaviour(OneEnum<OperationBehaviour>),
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
    /// Set the colour for a window border kind
    #[clap(arg_required_else_help = true)]
    #[clap(alias = "active-window-border-colour")]
    BorderColour(BorderColour),
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
