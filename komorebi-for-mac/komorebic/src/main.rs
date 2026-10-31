#![warn(clippy::all)]

use chrono::Utc;
use clap::Parser;
use clap::Subcommand;
use color_eyre::eyre;
use fs_tail::TailedFile;
use komorebi_client::OperationBehaviour;
use komorebi_client::OperationDirection;
use komorebi_client::PanDirection;
use komorebi_client::PathExt;
use komorebi_client::SocketMessage;
use komorebi_client::WindowKind;
use komorebi_client::replace_env_in_path;
use komorebi_client::send_message;
use komorebi_client::send_query;
use komorebi_client::splash;
use komorebi_client::splash::ValidationFeedback;
use komorebic_core::args::BooleanState;
use komorebic_core::args::OneEnum;
use komorebic_core::cli::Cmd;
use komorebic_core::cli::Meta;
use komorebic_core::cli::command;
use komorebic_core::docgen::DocFormat;
use komorebic_core::messages::Messages;
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

#[derive(Parser)]
struct MonocleFocusBehaviour {
    /// Desired monocle focus behaviour
    #[clap(value_enum)]
    behaviour: komorebi_client::MonocleFocusBehaviour,
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

#[derive(Subcommand)]
enum MacosCommands {
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

fn print_query(message: &SocketMessage) {
    match send_query(message) {
        Ok(response) => println!("{response}"),
        Err(error) => panic!("{}", error),
    }
}

/// What the top of the help and `--version` say.
const META: Meta = Meta {
    name: "komorebic",
    about: env!("CARGO_PKG_DESCRIPTION"),
    author: env!("CARGO_PKG_AUTHORS"),
    version: version::LONG_VERSION,
};

/// Write a message to the daemon's socket, or, if the daemon answers it, print the answer.
fn send_or_query(messages: Messages) -> eyre::Result<()> {
    let query = messages.is_query();
    let message = SocketMessage::from(messages);
    if query {
        print_query(&message);
    } else {
        send_message(&message)?;
    }
    Ok(())
}

fn main() -> eyre::Result<()> {
    let opts = komorebic_core::cli::parse::<MacosCommands>(&META);

    match opts.cmd {
        Cmd::Messages(messages) => send_or_query(messages)?,
        Cmd::Platform(platform) => match platform {
            MacosCommands::Docgen(args) => {
                let output_dir = args.output.unwrap_or_else(|| PathBuf::from("docs/cli"));
                let pages = komorebic_core::docgen::write(
                    command::<MacosCommands>(&META),
                    &DocFormat::PLAIN,
                    &output_dir,
                )?;
                for page in pages {
                    println!("{}", page.display());
                }
            }
            MacosCommands::Splash(args) => {
                splash::show(args.mdm_server)?;
            }
            MacosCommands::Completions(args) => {
                let mut cli = command::<MacosCommands>(&META);
                clap_complete::generate(args.shell, &mut cli, "komorebic", &mut std::io::stdout());
            }
            MacosCommands::License(args) => {
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
            MacosCommands::Permissions => {
                let mut current_exe = std::env::current_exe().expect("unable to get exec path");
                current_exe.pop();
                let komorebi_exe = current_exe.join("komorebi");

                let term_program = std::env::var("TERM_PROGRAM")
                    .unwrap_or_else(|_| String::from("this terminal emulator"));

                println!(
                    "\nPlease grant Accessibility permissions to \"{}\" and \"{term_program}\"",
                    komorebi_exe.display()
                );
                println!(
                    "Accessibility permissions are required for komorebi to to manage windows"
                );
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
            MacosCommands::Quickstart => {
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

                let local_appdata_dir =
                    dirs::data_local_dir().expect("could not find localdata dir");
                let data_dir = local_appdata_dir.join("komorebi");
                std::fs::create_dir_all(&*HOME_DIR)?;
                std::fs::create_dir_all(data_dir)?;

                let mut komorebi_json =
                    include_str!("../../docs/komorebi.example.json").to_string();
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
                println!(
                    "Accessibility permissions are required for komorebi to to manage windows"
                );
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
            MacosCommands::EnableAutostart(args) => {
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
            MacosCommands::DisableAutostart => {
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
            MacosCommands::Start(args) => {
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
                println!(
                    "* Leave a tip https://ko-fi.com/lgug2z - An alternative to GitHub Sponsors"
                );
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
            MacosCommands::Stop(args) => {
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
            MacosCommands::Configuration => {
                let static_config = HOME_DIR.join("komorebi.json");

                if static_config.exists() {
                    println!("{}", static_config.display());
                }
            }
            MacosCommands::DataDirectory => {
                let dir = &*DATA_DIR;
                if dir.exists() {
                    println!("{}", dir.display());
                }
            }
            MacosCommands::Log => {
                let timestamp = Utc::now().format("%Y-%m-%d").to_string();
                let color_log = DATA_DIR.join(format!("komorebi.log.{timestamp}"));
                let file = TailedFile::new(File::open(color_log)?);
                let locked = file.lock();
                #[allow(clippy::significant_drop_in_scrutinee, clippy::lines_filter_map_ok)]
                for line in locked.lines().flatten() {
                    println!("{line}");
                }
            }
            MacosCommands::Retile => {
                send_message(&SocketMessage::Retile)?;
            }
            MacosCommands::PromoteFocus => {
                send_message(&SocketMessage::PromoteFocus)?;
            }
            MacosCommands::ToggleWindowContainerBehaviour => {
                send_message(&SocketMessage::ToggleWindowContainerBehaviour)?;
            }
            MacosCommands::ToggleWorkspaceWindowContainerBehaviour => {
                send_message(&SocketMessage::ToggleWorkspaceWindowContainerBehaviour)?;
            }
            MacosCommands::ToggleMonocleFocusBehaviour => {
                send_message(&SocketMessage::ToggleMonocleFocusBehaviour)?;
            }
            MacosCommands::FetchAppSpecificConfiguration => {
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
            MacosCommands::EagerFocus(args) => {
                send_message(&SocketMessage::EagerFocus(args.exe))?;
            }
            MacosCommands::ScrollingLayoutColumns(args) => {
                send_message(&SocketMessage::ScrollingLayoutColumns(args.count))?;
            }
            MacosCommands::ScrollingLayoutFullWidth => {
                send_message(&SocketMessage::ScrollingLayoutFullWidth)?;
            }
            MacosCommands::CenterFocus => {
                send_message(&SocketMessage::CenterFocus)?;
            }
            MacosCommands::Consume(args) => {
                send_message(&SocketMessage::Consume(args.value))?;
            }
            MacosCommands::Expel => {
                send_message(&SocketMessage::Expel)?;
            }
            MacosCommands::ToggleColumnStack => {
                send_message(&SocketMessage::ToggleColumnStack)?;
            }
            MacosCommands::Pan(args) => {
                // a flag that has a default counts as given, so what only goes with --stream
                // cannot be left to the parser to say
                if !args.stream && (args.sensitivity.is_some() || args.reversed || args.paging) {
                    command::<MacosCommands>(&META)
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
            MacosCommands::LayoutRatios(args) => {
                if args.columns.is_none() && args.rows.is_none() {
                    println!(
                        "No ratios provided, nothing to change. Use --columns or --rows to specify ratios."
                    );
                } else {
                    send_message(&SocketMessage::LayoutRatios(args.columns, args.rows))?;
                }
            }
            MacosCommands::MonocleFocusBehaviour(args) => {
                send_message(&SocketMessage::MonocleFocusBehaviour(args.behaviour))?;
            }
            MacosCommands::UnmanagedWindowOperationBehaviour(args) => {
                send_message(&SocketMessage::UnmanagedWindowOperationBehaviour(
                    args.value,
                ))?;
            }
            MacosCommands::ApplicationSpecificConfigurationSchema => {
                #[cfg(feature = "schemars")]
                {
                    let asc =
                        schemars::schema_for!(komorebi_client::ApplicationSpecificConfiguration);
                    let schema = serde_json::to_string_pretty(&asc)?;
                    println!("{schema}");
                }
            }
            MacosCommands::NotificationSchema => {
                #[cfg(feature = "schemars")]
                {
                    let notification = schemars::schema_for!(komorebi_client::Notification);
                    let schema = serde_json::to_string_pretty(&notification)?;
                    println!("{schema}");
                }
            }
            MacosCommands::SocketSchema => {
                #[cfg(feature = "schemars")]
                {
                    let socket_message = schemars::schema_for!(SocketMessage);
                    let schema = serde_json::to_string_pretty(&socket_message)?;
                    println!("{schema}");
                }
            }
            MacosCommands::StaticConfigSchema => {
                #[cfg(feature = "schemars")]
                {
                    let socket_message = schemars::schema_for!(komorebi_client::StaticConfig);
                    let schema = serde_json::to_string_pretty(&socket_message)?;
                    println!("{schema}");
                }
            }
            MacosCommands::GenerateStaticConfig => {
                print_query(&SocketMessage::GenerateStaticConfig);
            }
            MacosCommands::BorderColour(args) => {
                send_message(&SocketMessage::BorderColour(
                    args.window_kind,
                    args.r,
                    args.g,
                    args.b,
                ))?;
            }
            MacosCommands::Animation(args) => {
                send_message(&SocketMessage::Animation(
                    args.boolean_state.into(),
                    args.animation_type,
                ))?;
            }
            MacosCommands::AnimationDuration(args) => {
                send_message(&SocketMessage::AnimationDuration(
                    args.duration,
                    args.animation_type,
                ))?;
            }
            MacosCommands::AnimationStyle(args) => {
                send_message(&SocketMessage::AnimationStyle(
                    args.style,
                    args.animation_type,
                ))?;
            }
        },
    }

    Ok(())
}
