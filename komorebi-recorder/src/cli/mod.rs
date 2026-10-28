//! The command line: the table of commands ([`Command`]) and the commands that need no module of
//! their own.
//!
//! Each family of commands is implemented next to its arguments in a module of this one, and every
//! command is run through [`execute`], which turns its outcome into the process's exit status.
//! `--json` on a command also decides how a failure is reported (see [`Command::wants_json`]).

mod agent;
mod annotate;
mod doctor;
mod findings;
mod investigate;
pub mod outputs;
mod logq;
mod query;
mod record;
pub mod refspec;
pub mod until_exit;
mod video;
mod viewer_command;

use crate::analysis::envelope::Envelope;
use crate::analysis::envelope::unusable;
use crate::analysis::envelope::with_hint;
use crate::session::Session;
use clap::Args;
use clap::Parser;
use clap::Subcommand;
use color_eyre::eyre;
use serde::Serialize;
use std::path::PathBuf;

pub use agent::BriefArgs;
pub use agent::GuideArgs;
pub use agent::OverviewArgs;
pub use agent::SchemaArgs;
pub use agent::SearchArgs;
pub use annotate::AnnotateArgs;
pub use annotate::AnnotationsArgs;
pub use doctor::DoctorArgs;
pub use findings::FindingsArgs;
pub use investigate::CodeArgs;
pub use investigate::DiffArgs;
pub use investigate::EventsArgs;
pub use investigate::StateArgs;
pub use investigate::TraceArgs;
pub use investigate::WindowArgs;
pub use logq::LogqArgs;
pub use query::AtArgs;
pub use query::FilterArgs;
pub use query::FramesArgs;
pub use query::LocateArgs;
pub use query::RangeArgs;
pub use query::SummaryArgs;
pub use record::DisplaysArgs;
pub use record::RecordArgs;
pub use video::BlankArgs;
pub use video::ChangesArgs;
pub use video::CompareArgs;
pub use video::FramediffArgs;
pub use video::MotionArgs;
pub use video::SheetArgs;
pub use viewer_command::ViewArgs;

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
    Displays(DisplaysArgs),
    /// What this machine can do for a recording, and probes that measure what one leans on
    Doctor(DoctorArgs),
    /// Everything recorded around a video timecode: komorebi log, events and OS trace
    At(AtArgs),
    /// Everything recorded between two video timecodes
    Range(RangeArgs),
    /// A per-bucket digest of the whole recording
    Summary(SummaryArgs),
    /// Reverse lookup: a UTC time, log line or event number to a video timecode
    Locate(LocateArgs),
    /// Extract PNG frames at video timecodes (needs ffmpeg)
    Frames(FramesArgs),
    /// Start here when investigating: what the user marked as the bug, and what the recording shows
    /// around it
    Brief(BriefArgs),
    /// The whole recording at a glance: what stands out, ranked, and when things happened
    Overview(OverviewArgs),
    /// Where a word appears: komorebi's log, events, incidents, annotations, optionally the OS trace
    Search(SearchArgs),
    /// Query komorebi's log by field: level, target, message, span fields (hwnd, event, ...), with counts and group-by
    Logq(LogqArgs),
    /// How to investigate a recording, for an agent
    Guide(GuideArgs),
    /// The JSON Schema of a command's `--json` output
    Schema(SchemaArgs),
    /// The state komorebi was in at a moment: monitors, workspaces, windows, focus, and anything wrong
    State(StateArgs),
    /// What changed between two moments, in terms a person would use
    Diff(DiffArgs),
    /// komorebi's events, classified, with the handler that ran for each
    Events(EventsArgs),
    /// One window's life through the recording: where it was, when it moved or was hidden
    Window(WindowArgs),
    /// One event end to end: what its handler logged, what changed, and who changed it
    Trace(TraceArgs),
    /// When and where on each display the picture changed
    Changes(ChangesArgs),
    /// When, and how hard, a display or a region of it was moving
    Motion(MotionArgs),
    /// How two frames differ: changed share, brightness delta, box of change, optional PNG
    Framediff(FramediffArgs),
    /// When a region was blank (one colour, such as white) or frozen
    Blank(BlankArgs),
    /// Two recordings side by side: frames, motion, findings, annotations
    Compare(CompareArgs),
    /// A contact sheet image of frames, with the times, events and bug region marked
    Sheet(SheetArgs),
    /// Open a recording in a window: play and scrub the video with every event on a timeline
    View(ViewArgs),
    /// Which line of komorebi's source wrote a line of its log, with the code around it
    Code(CodeArgs),
    /// What the detectors found and what an agent concluded, in one list, with stable ids
    Findings(FindingsArgs),
    /// Add, edit or delete a session's bug regions, notes and an agent's findings
    Annotate(AnnotateArgs),
    /// List a session's bug regions, notes and findings
    Annotations(AnnotationsArgs),
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
    /// Machine-readable JSON output
    #[arg(long)]
    pub json: bool,
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
    /// Machine-readable JSON output (a failed self-test is a failed envelope)
    #[arg(long)]
    pub json: bool,
}

/// Print `value` as pretty JSON.
pub(crate) fn print_json(value: &impl Serialize) -> eyre::Result<()> {
    println!("{}", serde_json::to_string_pretty(value)?);
    Ok(())
}

pub(crate) fn limit(value: usize) -> Option<usize> {
    (value != 0).then_some(value)
}

impl Command {
    /// Whether this command was asked for JSON, which also decides how a failure is reported.
    #[must_use]
    pub fn wants_json(&self) -> bool {
        match self {
            Self::At(args) => args.filter.json,
            Self::Range(args) => args.filter.json,
            Self::Summary(args) => args.json,
            Self::Locate(args) => args.json,
            Self::Frames(args) => args.json,
            Self::Brief(args) => args.json,
            Self::Overview(args) => args.json,
            Self::Search(args) => args.json,
            Self::Logq(args) => args.json,
            Self::Guide(args) => args.json,
            Self::State(args) => args.json,
            Self::Diff(args) => args.json,
            Self::Events(args) => args.json,
            Self::Code(args) => args.json,
            Self::Trace(args) => args.json,
            Self::Window(args) => args.json,
            Self::Changes(args) => args.json,
            Self::Sheet(args) => args.json,
            Self::Motion(args) => args.json,
            Self::Framediff(args) => args.json,
            Self::Blank(args) => args.json,
            Self::Compare(args) => args.json,
            Self::Annotate(args) => args.json(),
            Self::Annotations(args) => args.json,
            Self::Doctor(args) => args.json,
            Self::Record(args) => args.json,
            Self::Displays(args) => args.json,
            Self::Reindex(args) => args.json,
            Self::Selftest(args) => args.json,
            Self::Findings(args) => args.json,
            _ => false,
        }
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
        Command::Displays(args) => record::displays(&args),
        Command::Doctor(args) => doctor::doctor(&args),
        Command::At(args) => query::at(&args),
        Command::Range(args) => query::range(&args),
        Command::Summary(args) => query::summary(&args),
        Command::Locate(args) => query::locate(&args),
        Command::Frames(args) => query::frames_command(&args),
        Command::Brief(args) => agent::brief(&args),
        Command::Overview(args) => agent::overview(&args),
        Command::Search(args) => agent::search(&args),
        Command::Logq(args) => logq::logq(&args),
        Command::Guide(args) => agent::guide(&args),
        Command::Schema(args) => agent::schema(&args),
        Command::State(args) => investigate::state(&args),
        Command::Diff(args) => investigate::diff_command(&args),
        Command::Events(args) => investigate::events(&args),
        Command::Code(args) => investigate::code(&args),
        Command::Trace(args) => investigate::trace(&args),
        Command::Window(args) => investigate::window(&args),
        Command::Changes(args) => video::changes(&args),
        Command::View(args) => crate::viewer::run(args.session),
        Command::Sheet(args) => video::sheet(&args),
        Command::Motion(args) => video::motion(&args),
        Command::Framediff(args) => video::framediff(&args),
        Command::Blank(args) => video::blank(&args),
        Command::Compare(args) => video::compare(&args),
        Command::Findings(args) => findings::findings(&args),
        Command::Annotate(args) => annotate::annotate(&args),
        Command::Annotations(args) => annotate::list(&args),
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

pub(crate) fn load(session: &std::path::Path) -> eyre::Result<(Session, crate::session::Manifest)> {
    let session = Session::open(session)?;
    let manifest = session.manifest()?;

    if manifest.status == crate::session::STATUS_RECORDING {
        let dir = session.dir.display();
        return Err(match crate::lock::recorder(&session.dir) {
            crate::lock::Recorder::Running(_) => unusable(format!(
                "{dir} is still being recorded; stop the recording first"
            )),
            crate::lock::Recorder::NotRunning | crate::lock::Recorder::Unknown => with_hint(
                unusable(format!(
                    "{dir} was left unfinished: its recorder was killed or crashed"
                )),
                format!("recover it with `komorebi-recorder reindex {dir}`"),
            ),
        });
    }

    Ok((session, manifest))
}

fn reindex(args: &ReindexArgs) -> eyre::Result<()> {
    let session = Session::open(&args.session)?;
    let reindexed = crate::recovery::reindex_session(&session)?;

    if args.json {
        return print_json(&Envelope::ok(
            outputs::SCHEMA_REINDEX,
            outputs::ReindexData {
                session: session.dir.display().to_string(),
                recovered: reindexed.recovered,
                entries: reindexed.stats.entries,
                by_source: reindexed.stats.by_source,
            },
        ));
    }

    for note in &reindexed.recovered {
        println!("recovered: {note}");
    }

    println!("indexed {} records", reindexed.stats.entries);
    for (source, count) in &reindexed.stats.by_source {
        println!("  {source}: {count}");
    }

    Ok(())
}
