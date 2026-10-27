//! `komorebic`, the command line interface of komorebi for KDE Plasma.
//!
//! A subcommand that asks something of the daemon is one message on its socket: an order, such as
//! `stop` or `retile`, is written and `komorebic` exits; a question, such as `state` or `query`,
//! is written and the answer is printed. The messages are [`komorebi_client::SocketMessage`]s,
//! sent with [`komorebi_client::send_message`] and [`komorebi_client::send_query`].
//!
//! `focus` and `cycle-focus` give the focus to another window of the focused workspace, and
//! `mouse-follows-focus` and `toggle-mouse-follows-focus` set whether the pointer goes with it.
//! `move` and `cycle-move` move the focused window to another tile. `change-layout`,
//! `cycle-layout` and `flip-layout` change the layout of the focused workspace, `toggle-tiling`
//! turns its tiling off and on, and `preselect-direction` and `cancel-preselect` reserve the
//! place of the next window and give it up.
//!
//! `subscribe-socket` and `unsubscribe-socket` add and remove a subscriber: a program that listens
//! on a socket in the daemon's data directory, to which the daemon sends a notification after each
//! event and each message it handles.
//!
//! The others work without the daemon: `start` runs `komorebi` in the background, `log` follows
//! its log, `data-directory` shows where its socket is, and `completions` prints a completion
//! script for a shell. The hidden `docgen` writes the help of every subcommand as Markdown, wrapped
//! at 100 columns whatever the width of the terminal, which is how `docs/cli` is made.

use chrono::Utc;
use clap::CommandFactory;
use clap::Parser;
use clap::ValueEnum;
use color_eyre::eyre;
use fs_tail::TailedFile;
use komorebi_client::Axis;
use komorebi_client::CycleDirection;
use komorebi_client::DefaultLayout;
use komorebi_client::OperationDirection;
use komorebi_client::SocketMessage;
use komorebi_client::StateQuery;
use komorebi_client::send_message;
use komorebi_client::send_query;
use std::fs::File;
use std::io::BufRead;
use std::io::Write;
use std::path::PathBuf;
use std::process::Command;
use std::process::Stdio;
use std::time::Duration;
use sysinfo::ProcessesToUpdate;

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
    FlipLayout: Axis,
    ChangeLayout: DefaultLayout,
    CycleLayout: CycleDirection,
    // WatchConfiguration: BooleanState,
    MouseFollowsFocus: BooleanState,
    Query: StateQuery,
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
#[allow(clippy::struct_excessive_bools)]
struct Start {
    /// Arguments to pass to komorebi, after a `--`: `komorebic start -- --log-level debug`
    #[clap(last = true)]
    komorebi_args: Vec<String>,
}

#[derive(Parser)]
struct Stop {}

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
    /// Start komorebi as a background process
    Start(Start),
    /// Stop the komorebi process
    Stop(Stop),
    /// Show the path to komorebi's data directory, which has its socket
    #[clap(alias = "datadir")]
    DataDirectory,
    /// Show a JSON representation of the current window manager state
    State,
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
    /// Change focus to the window in the specified cycle direction
    #[clap(arg_required_else_help = true)]
    CycleFocus(CycleFocus),
    /// Move the focused window in the specified cycle direction
    #[clap(arg_required_else_help = true)]
    CycleMove(CycleMove),
    /// Set the layout on the focused workspace
    #[clap(arg_required_else_help = true)]
    ChangeLayout(ChangeLayout),
    /// Cycle between available layouts
    #[clap(arg_required_else_help = true)]
    CycleLayout(CycleLayout),
    // /// Load a custom layout from file for the focused workspace
    // #[clap(hide = true)]
    // #[clap(arg_required_else_help = true)]
    // LoadCustomLayout(LoadCustomLayout),
    /// Flip the layout on the focused workspace
    #[clap(arg_required_else_help = true)]
    FlipLayout(FlipLayout),
    /// Force the retiling of all managed windows
    Retile,
    /// Toggle window tiling on the focused workspace
    ToggleTiling,
    /// Enable or disable mouse follows focus on all workspaces
    #[clap(arg_required_else_help = true)]
    MouseFollowsFocus(MouseFollowsFocus),
    /// Toggle mouse follows focus on all workspaces
    ToggleMouseFollowsFocus,
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
        SubCommand::Start(args) => {
            let mut command = &mut Command::new("komorebi");

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
                if let Ok(output) = Command::new("komorebi").args(&args.komorebi_args).output() {
                    println!("{}", String::from_utf8(output.stderr)?);
                }
            }
        }
        SubCommand::Stop(_) => {
            send_message(&SocketMessage::Stop)?;
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
        SubCommand::ChangeLayout(args) => {
            send_message(&SocketMessage::ChangeLayout(args.default_layout))?;
        }
        SubCommand::CycleLayout(args) => {
            send_message(&SocketMessage::CycleLayout(args.cycle_direction))?;
        }
        SubCommand::FlipLayout(args) => {
            send_message(&SocketMessage::FlipLayout(args.axis))?;
        }
        SubCommand::Retile => {
            send_message(&SocketMessage::Retile)?;
        }
        SubCommand::ToggleTiling => {
            send_message(&SocketMessage::ToggleTiling)?;
        }
        SubCommand::State => {
            print_query(&SocketMessage::State);
        }
        SubCommand::Query(args) => {
            print_query(&SocketMessage::Query(args.state_query));
        }
        SubCommand::MonitorInformation => {
            print_query(&SocketMessage::MonitorInformation);
        }
        SubCommand::ToggleMouseFollowsFocus => {
            send_message(&SocketMessage::ToggleMouseFollowsFocus)?;
        }
        SubCommand::MouseFollowsFocus(args) => {
            send_message(&SocketMessage::MouseFollowsFocus(args.boolean_state.into()))?;
        }
        SubCommand::SubscribeSocket(args) => {
            send_message(&SocketMessage::AddSubscriberSocket(args.socket))?;
        }
        SubCommand::UnsubscribeSocket(args) => {
            send_message(&SocketMessage::RemoveSubscriberSocket(args.socket))?;
        }
    }

    Ok(())
}
