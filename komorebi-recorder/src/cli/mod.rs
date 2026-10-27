//! The command line: the table of commands ([`Command`]) and the commands that need no module of
//! their own.
//!
//! Each family of commands is implemented next to its arguments in a module of this one, and every
//! command is run through [`execute`], which turns its outcome into the process's exit status.
//! `--json` on a command also decides how a failure is reported (see [`Command::wants_json`]).

mod record;

use crate::session::Session;
use clap::Args;
use clap::Parser;
use clap::Subcommand;
use color_eyre::eyre;
use std::path::PathBuf;

pub use record::RecordArgs;

/// The command line of `komorebi-recorder`.
#[derive(Parser)]
#[command(
    author,
    version,
    about = "Record the screen, komorebi's logs and events, and the OS trace in lockstep",
    long_about = "Record the screen together with komorebi's log, komorebi's subscriber events and \
                  the OS trace (the unified log on macOS, WinEvents and the Event Log on Windows), \
                  all on one clock, into a session directory. \
                  Then ask what was happening at any second of the video, or where in the video any \
                  log line or event is.\n\n\
                  Run with no subcommand to open the recorder window."
)]
pub struct Opts {
    /// The command to run.
    #[command(subcommand)]
    pub command: Option<Command>,
}

/// What `komorebi-recorder` was asked to do.
#[derive(Subcommand)]
pub enum Command {
    /// Open the recorder window with Record / Stop buttons (the default)
    Gui(RecordArgs),
    /// Record without a window; Ctrl-C (or --duration) stops and finalizes the session
    Record(RecordArgs),
    /// List the displays that can be recorded
    Displays,
    /// Rebuild a session's timeline from its raw files, recovering it first if the recorder died
    Reindex(ReindexArgs),
    /// Check the video-to-clock alignment against a rendered on-screen clock
    Selftest(SelftestArgs),
    /// The clock window used by `selftest` (internal)
    #[command(hide = true)]
    SelftestClock,
}

/// The arguments of `reindex`.
#[derive(Args, Debug)]
pub struct ReindexArgs {
    /// Session directory
    pub session: PathBuf,
}

/// The arguments of `selftest`.
#[derive(Args, Debug)]
pub struct SelftestArgs {
    /// Seconds to record
    #[arg(long, default_value_t = 5.0)]
    pub seconds: f64,
    /// Largest allowed disagreement between the video and the clocks, in milliseconds
    #[arg(long, default_value_t = 100.0)]
    pub tolerance_ms: f64,
    /// Session directory for the self-test recording
    #[arg(long)]
    pub out: Option<PathBuf>,
}

impl Command {
    /// Whether this command was asked for JSON, which also decides how a failure is reported.
    #[must_use]
    pub fn wants_json(&self) -> bool {
        let _ = self;
        false
    }
}

/// Runs `command` and turns its outcome into the process exit status.
///
/// The status is 0, or the code that says what kind of failure it was (see
/// [`crate::analysis::envelope::ExitKind`]). With `--json` a failure is also printed as an envelope
/// so that a caller parsing stdout still gets an answer.
pub fn execute(command: Command) -> std::process::ExitCode {
    let json = command.wants_json();
    match run(command) {
        Ok(()) => std::process::ExitCode::SUCCESS,
        Err(error) => {
            let kind = crate::analysis::envelope::exit_kind(&error);
            if json {
                let envelope = crate::analysis::envelope::Envelope::failed(&error);
                if let Ok(text) = serde_json::to_string_pretty(&envelope) {
                    println!("{text}");
                }
            }
            eprintln!("Error: {error:?}");
            std::process::ExitCode::from(kind.code())
        }
    }
}

/// Runs `command`.
///
/// # Errors
///
/// Returns whatever error the command fails with; [`execute`] turns it into an exit status.
pub fn run(command: Command) -> eyre::Result<()> {
    match command {
        Command::Gui(args) => crate::app::run(args),
        Command::Record(args) => record::record(&args),
        Command::Displays => record::displays(),
        Command::Reindex(args) => reindex(&args),
        Command::Selftest(args) => crate::selftest::run(&args),
        Command::SelftestClock => crate::selftest::run_clock(),
    }
}

/// What both binaries (`komorebi-recorder` and, on Windows, the no-console
/// `komorebi-recorder-gui`) do: read the command line and run it.
/// No command opens the recorder window.
///
/// # Errors
///
/// Returns an error if the error report handler cannot be installed. A failing command is not an
/// error here: it is the exit status that is returned.
pub fn main() -> eyre::Result<std::process::ExitCode> {
    use clap::Parser;

    color_eyre::install()?;
    crate::platform::init();

    let opts = Opts::parse();
    Ok(execute(
        opts.command
            .unwrap_or_else(|| Command::Gui(RecordArgs::default())),
    ))
}

fn reindex(args: &ReindexArgs) -> eyre::Result<()> {
    let session = Session::open(&args.session)?;
    let reindexed = crate::recovery::reindex_session(&session)?;

    for note in &reindexed.recovered {
        println!("recovered: {note}");
    }

    println!("indexed {} records", reindexed.stats.entries);
    for (source, count) in &reindexed.stats.by_source {
        println!("  {source}: {count}");
    }

    Ok(())
}
