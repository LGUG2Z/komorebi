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
//! - `consume`, `expel` and `toggle-column-stack` put windows in the same column of the Scrolling
//!   layout and take them out, `scrolling-layout-full-width`, `center-focus` and `expand` make the
//!   focused column as wide as the screen, put it in the middle and fill the room that is left,
//!   and `pan` moves the view along the strip without moving the focus.
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
//! - `border` sets whether the compositor draws a border around each window, `border-colour` the
//!   color of the border of a kind of window, and `border-width` and `border-offset` the room the
//!   layout leaves for it.
//! - `focus-follows-mouse` and `toggle-focus-follows-mouse` set whether KWin gives the focus to the
//!   window under the pointer, as the pointer moves.
//! - `transparency` and `toggle-transparency` set whether the windows that do not have the focus
//!   are see-through, and `transparency-alpha` how opaque they are.
//! - `animation`, `animation-duration` and `animation-style` set whether windows are animated going
//!   to their places, for how long and with what easing, and `animation-fps` the frame rate that
//!   is stored and ignored.
//!
//! `ignore-rule` and `manage-rule` add a rule that keeps an application out of the layout or
//! always in it, `session-float-rule` floats the focused window and every window like it until the
//! daemon stops, `session-float-rules` shows those rules and `clear-session-float-rules` removes
//! them, and `global-state` shows the rules the daemon has. `visible-windows` shows the windows
//! that can be seen on the workspace each monitor shows.
//!
//! `initial-workspace-rule`, `initial-named-workspace-rule`, `workspace-rule` and
//! `named-workspace-rule` send the windows of an application to a workspace, when they open or
//! always, `clear-workspace-rules`, `clear-named-workspace-rules` and `clear-all-workspace-rules`
//! take those rules away, and `enforce-workspace-rules` applies them to the windows that are open.
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
//! The others work without the daemon: `quickstart` writes an example `komorebi.json`,
//! `applications.json` and `skhdrc` in the config home, `enable-autostart` and `disable-autostart` write and
//! remove the entry that has Plasma start komorebi at login, `start` runs `komorebi` in the
//! background, with the `komorebi.json` it is given with `--config`, `check` reads the
//! `komorebi.json` the daemon would and the key binding file, looks at the desktop, and reports
//! what is wrong, exiting with status 1 if something is an error, `configuration` shows the path
//! of the `komorebi.json` the daemon reads, `log` follows its log, `data-directory` shows where its
//! socket is, `completions` prints a completion script for a shell, and `static-config-schema`,
//! `application-specific-configuration-schema`, `notification-schema` and `socket-schema` print
//! the JSON Schema of `komorebi.json`, of an `applications.json`, of a notification and of a
//! message. The hidden `docgen` writes the help of every subcommand as Markdown, wrapped
//! at 100 columns whatever the width of the terminal, which is how `docs/cli` is made.
//!
//! `export-kde-shortcuts` and `hotkeys compile` read a key binding file (an skhdrc, a whkdrc or an
//! sxhkdrc) and write the bindings KDE can run by itself as shortcut files. `hotkeys state`,
//! `hotkeys reload` and `hotkeys release` look after the keys the daemon registers with KDE; the
//! [`hotkeys`] module has the details.

use chrono::Utc;
use clap::Parser;
use clap::Subcommand;
use color_eyre::eyre;
use fs_tail::TailedFile;
use komorebi_client::FocusFollowsMouseImplementation;
use komorebi_client::PanDirection;
use komorebi_client::PathExt;
use komorebi_client::SocketMessage;
use komorebi_client::WindowKind;
use komorebi_client::replace_env_in_path;
use komorebi_client::send_message;
use komorebi_client::send_query;
use komorebic_core::args::BooleanState;
use komorebic_core::args::OneEnum;
use komorebic_core::cli::Cmd;
use komorebic_core::cli::Meta;
use komorebic_core::cli::command;
use komorebic_core::docgen::DocFormat;
use komorebic_core::launcher;
use komorebic_core::launcher::Launcher;
use komorebic_core::messages::Messages;
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
use sysinfo::ProcessesToUpdate;

mod hotkeys;

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

#[derive(Parser)]
struct MonocleFocusBehaviour {
    /// Desired monocle focus behavior
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
struct Pan {
    /// The way that the view goes
    #[clap(value_enum)]
    direction: PanDirection,
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
struct TransparencyAlpha {
    /// Alpha
    alpha: u8,
}

#[derive(Parser)]
struct FocusFollowsMouse {
    /// On KDE both are KWin's own focus policy
    #[clap(value_enum, short, long, default_value = "windows")]
    implementation: FocusFollowsMouseImplementation,
    #[clap(value_enum)]
    boolean_state: BooleanState,
}

#[derive(Parser)]
struct ToggleFocusFollowsMouse {
    /// On KDE both are KWin's own focus policy
    #[clap(value_enum, short, long, default_value = "windows")]
    implementation: FocusFollowsMouseImplementation,
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
struct Check {
    /// Path to a static configuration JSON file
    #[clap(action, short, long)]
    #[clap(value_parser = replace_env_in_path)]
    komorebi_config: Option<PathBuf>,
    /// Look at the configuration and the key bindings only, not at the desktop
    #[clap(long)]
    configuration_only: bool,
}

#[derive(Parser)]
struct Completions {
    #[clap(value_enum)]
    shell: clap_complete::Shell,
}

#[derive(Subcommand)]
enum KdeCommands {
    #[clap(hide = true)]
    Docgen(Docgen),
    /// Generate komorebic CLI completions for the target shell
    #[clap(arg_required_else_help = true)]
    Completions(Completions),
    /// Gather example configurations for a new-user quickstart
    Quickstart,
    /// Turn an skhdrc into KDE global shortcuts (they work over RDP, unlike evdev hotkey daemons)
    ExportKdeShortcuts(hotkeys::ExportKdeShortcuts),
    /// Key bindings: KDE global shortcuts from a key binding file, and the keys of the daemon
    #[clap(arg_required_else_help = true)]
    Hotkeys(hotkeys::Hotkeys),
    /// Start komorebi as a background process
    Start(Start),
    /// Stop the komorebi process
    Stop(Stop),
    /// Check komorebi configuration and the desktop it runs on for common errors
    Check(Check),
    /// Show the path to komorebi.json
    #[clap(alias = "config")]
    Configuration,
    /// Show the path to komorebi's data directory, which has its socket
    #[clap(alias = "datadir")]
    DataDirectory,
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
    /// Move the view of the Scrolling layout a column to the left or the right, which does
    /// nothing in any other layout
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
    /// Toggle the compositor's maximization for the focused window
    ToggleMaximize,
    // /// Set the border style
    // #[clap(arg_required_else_help = true)]
    // BorderStyle(BorderStyle),
    // /// Set the border implementation
    // #[clap(arg_required_else_help = true)]
    // BorderImplementation(BorderImplementation),
    /// Enable or disable transparency for unfocused windows
    #[clap(arg_required_else_help = true)]
    Transparency(OneEnum<BooleanState>),
    /// Set the alpha value for unfocused window transparency
    #[clap(arg_required_else_help = true)]
    TransparencyAlpha(TransparencyAlpha),
    /// Toggle transparency for unfocused windows
    ToggleTransparency,
    /// Enable or disable focus follows mouse (KWin's own focus policy)
    #[clap(arg_required_else_help = true)]
    FocusFollowsMouse(FocusFollowsMouse),
    /// Toggle focus follows mouse (KWin's own focus policy)
    ToggleFocusFollowsMouse(ToggleFocusFollowsMouse),
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

/// Starting `komorebi` for `komorebic start`: a `Command` for the daemon, found among the
/// processes by its exact name.
struct KdeLauncher<'a> {
    args: &'a Start,
    system: sysinfo::System,
}

impl Launcher for KdeLauncher<'_> {
    fn daemon_name(&self) -> &str {
        "komorebi"
    }

    fn spawn(&mut self) -> eyre::Result<()> {
        let mut command = Command::new("komorebi");

        if let Some(config) = &self.args.config {
            command.args(["--config", &config.display().to_string()]);
        }

        command
            .args(&self.args.komorebi_args)
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .stdin(Stdio::null())
            .spawn()?;

        Ok(())
    }

    fn is_running(&mut self) -> eyre::Result<bool> {
        self.system.refresh_processes(ProcessesToUpdate::All, true);

        Ok(self
            .system
            .processes_by_exact_name("komorebi".as_ref())
            .next()
            .is_some())
    }

    fn diagnose(&mut self) -> eyre::Result<()> {
        let mut command = Command::new("komorebi");

        if let Some(config) = &self.args.config {
            command.args(["--config", &config.display().to_string()]);
        }

        if let Ok(output) = command.args(&self.args.komorebi_args).output() {
            println!("{}", String::from_utf8(output.stderr)?);
        }

        Ok(())
    }
}

/// What the top of the help and `--version` say.
const META: Meta = Meta {
    name: "komorebic",
    about: "The command-line interface for Komorebi, a tiling window manager for KDE Plasma",
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

pub fn main() -> eyre::Result<()> {
    let opts = komorebic_core::cli::parse::<KdeCommands>(&META);

    match opts.cmd {
        Cmd::Messages(messages) => send_or_query(messages)?,
        Cmd::Platform(platform) => match platform {
            KdeCommands::Docgen(args) => {
                let output_dir = args.output.unwrap_or_else(|| PathBuf::from("docs/cli"));
                let pages = komorebic_core::docgen::write(
                    command::<KdeCommands>(&META),
                    &DocFormat::KDE,
                    &output_dir,
                )?;
                for page in pages {
                    println!("{}", page.display());
                }
            }
            KdeCommands::Completions(args) => {
                let mut cli = command::<KdeCommands>(&META);
                clap_complete::generate(args.shell, &mut cli, "komorebic", &mut std::io::stdout());
            }
            KdeCommands::ExportKdeShortcuts(args) => hotkeys::export_kde_shortcuts(args)?,
            KdeCommands::Hotkeys(args) => hotkeys::hotkeys(args)?,
            KdeCommands::Quickstart => {
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
                let skhdrc_path = HOME_DIR.join("skhdrc");

                let mut written_files = Vec::new();

                write_file_with_prompt(&komorebi_path, &komorebi_json, &mut written_files)?;

                let applications_json = include_str!("../applications.json");
                write_file_with_prompt(&applications_path, applications_json, &mut written_files)?;

                let skhdrc = include_str!("../../docs/skhdrc.sample");
                write_file_with_prompt(&skhdrc_path, skhdrc, &mut written_files)?;
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
            KdeCommands::EnableAutostart(args) => {
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
            KdeCommands::DisableAutostart => {
                let target = autostart_entry();
                if target.exists() {
                    std::fs::remove_file(&target)?;
                    println!("Deleted {}", target.display());
                } else {
                    println!("There is no autostart entry at {}", target.display());
                }
            }
            KdeCommands::Check(args) => {
                use komorebi_client::check::Finding;
                use komorebi_client::check::Report;

                let mut report = Report::default();

                // where komorebi looks
                let mut place = vec![];
                if std::env::var_os("KOMOREBI_CONFIG_HOME").is_some() {
                    place.push(Finding::note(format!(
                        "KOMOREBI_CONFIG_HOME is set: looking in {}",
                        HOME_DIR.display()
                    )));
                } else {
                    place.push(Finding::note(format!(
                        "KOMOREBI_CONFIG_HOME is not set: looking in {}",
                        HOME_DIR.display()
                    )));
                }

                let static_config = match args.komorebi_config {
                    Some(path) => {
                        place.push(Finding::note(format!(
                            "using the configuration file given with --komorebi-config: {}",
                            path.display()
                        )));
                        path
                    }
                    None => HOME_DIR.join("komorebi.json"),
                };

                let (findings, wants) = komorebi_client::check::check_configuration(&static_config);
                place.extend(findings);
                report.push("Configuration", place);

                report.push("Key bindings", hotkeys::check(&HOME_DIR));

                if !args.configuration_only {
                    report.push(
                        "The desktop",
                        komorebi_client::check::check_environment(&wants),
                    );
                }

                print!("{}", report.render());

                if report.has_errors() {
                    std::process::exit(1);
                }
            }
            KdeCommands::Start(args) => {
                launcher::start(&mut KdeLauncher {
                    args: &args,
                    system: sysinfo::System::new_all()?,
                })?;
            }
            KdeCommands::Stop(args) => {
                send_message(&launcher::stop_message(args.ignore_restore))?;
            }
            KdeCommands::Configuration => {
                let static_config = HOME_DIR.join("komorebi.json");

                if static_config.exists() {
                    println!("{}", static_config.display());
                }
            }
            KdeCommands::DataDirectory => {
                let dir = &*komorebi_client::DATA_DIR;
                if dir.exists() {
                    println!("{}", dir.display());
                }
            }
            KdeCommands::Log => {
                let timestamp = Utc::now().format("%Y-%m-%d").to_string();
                let color_log = komorebi_client::LOG_DIR.join(format!("komorebi.log.{timestamp}"));
                let file = TailedFile::new(File::open(color_log)?);
                let locked = file.lock();
                #[allow(clippy::significant_drop_in_scrutinee, clippy::lines_filter_map_ok)]
                for line in locked.lines().flatten() {
                    println!("{line}");
                }
            }
            KdeCommands::ToggleMaximize => {
                send_message(&SocketMessage::ToggleMaximize)?;
            }
            KdeCommands::Pan(args) => {
                send_message(&SocketMessage::Pan(args.direction))?;
            }
            KdeCommands::LayoutRatios(args) => {
                if args.columns.is_none() && args.rows.is_none() {
                    println!(
                        "No ratios provided, nothing to change. Use --columns or --rows to specify ratios."
                    );
                } else {
                    send_message(&SocketMessage::LayoutRatios(args.columns, args.rows))?;
                }
            }
            KdeCommands::ApplicationSpecificConfigurationSchema => {
                #[cfg(feature = "schemars")]
                {
                    let schema = komorebi_config::schema::view::<
                        komorebi_client::ApplicationSpecificConfiguration,
                    >(komorebi_config::schema::Platform::CURRENT);
                    print!("{schema}");
                }
            }
            KdeCommands::NotificationSchema => {
                #[cfg(feature = "schemars")]
                {
                    let notification = schemars::schema_for!(komorebi_client::Notification);
                    let schema = serde_json::to_string_pretty(&notification)?;
                    println!("{schema}");
                }
            }
            KdeCommands::SocketSchema => {
                #[cfg(feature = "schemars")]
                {
                    let socket_message = schemars::schema_for!(SocketMessage);
                    let schema = serde_json::to_string_pretty(&socket_message)?;
                    println!("{schema}");
                }
            }
            KdeCommands::StaticConfigSchema => {
                #[cfg(feature = "schemars")]
                {
                    let schema = komorebi_config::schema::view::<
                        komorebi_config::static_config::StaticConfig,
                    >(komorebi_config::schema::Platform::CURRENT);
                    print!("{schema}");
                }
            }
            KdeCommands::GenerateStaticConfig => {
                print_query(&SocketMessage::GenerateStaticConfig);
            }
            KdeCommands::Transparency(args) => {
                send_message(&SocketMessage::Transparency(args.value.into()))?;
            }
            KdeCommands::TransparencyAlpha(args) => {
                send_message(&SocketMessage::TransparencyAlpha(args.alpha))?;
            }
            KdeCommands::ToggleTransparency => {
                send_message(&SocketMessage::ToggleTransparency)?;
            }
            KdeCommands::FocusFollowsMouse(args) => {
                send_message(&SocketMessage::FocusFollowsMouse(
                    args.implementation,
                    args.boolean_state.into(),
                ))?;
            }
            KdeCommands::ToggleFocusFollowsMouse(args) => {
                send_message(&SocketMessage::ToggleFocusFollowsMouse(args.implementation))?;
            }
        },
    }

    Ok(())
}
