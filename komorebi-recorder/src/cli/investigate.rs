//! `state`, `diff` and `events`: what komorebi believed, what changed, and what happened. Also
//! `code`, `trace` and `window`, which explain one log line, one event and one window.
//!
//! Each command opens the session as an [`Analysis`] ([`Analysis::open`]), asks it one question,
//! and prints the answer as text ([`crate::report`]) or, with `--json`, in the envelope its schema
//! names. A time argument is any form [`Analysis::resolve`] reads: `12.5`, `1:02.5`,
//! `wall:<RFC 3339>`, `event:<seq>`, `bug:<id>.end`, optionally followed by an offset such as `+3s`.
//!
//! | Command | Asks | Answer, with `--json` |
//! |---|---|---|
//! | `state` | [`Analysis::state_answer`] | `state/1` |
//! | `diff` | [`Analysis::diff_answer`] | `diff/1` |
//! | `events` | [`Analysis::events_answer`] | `events/1` |
//! | `code` | [`Analysis::code_for_lines`], [`Analysis::code_for_event`] or [`Analysis::code_search`] | `code/1` |
//! | `trace` | [`Analysis::trace`], and `Analysis::os_trace` with `--os` | `trace/1` |
//! | `window` | [`Analysis::window_history`] | `window/1` |
//!
//! `events` and `window` page with `--limit` and `--cursor`: when an answer is cut short it prints
//! (or, with `--json`, sets `next_cursor` to) the sequence number to continue after. A limit of 0
//! means no limit.
//!
//! Covered by `tests/investigate.rs`, `tests/trace.rs` and `tests/code.rs`, which run the commands
//! through the real binary.

use super::print_json;
use crate::analysis::Analysis;
use crate::analysis::answers::EventQuery;
use crate::analysis::answers::SCHEMA_DIFF;
use crate::analysis::answers::SCHEMA_EVENTS;
use crate::analysis::answers::SCHEMA_STATE;
use crate::analysis::answers::StateQuery;
use crate::analysis::classify::Class;
use crate::analysis::code::SCHEMA_CODE;
use crate::analysis::envelope;
use crate::analysis::envelope::Envelope;
use crate::report;
use clap::Args;
use clap::ValueEnum;
use color_eyre::eyre;
use std::path::PathBuf;

/// The arguments of `state`.
#[derive(Args, Debug)]
pub struct StateArgs {
    /// Session directory
    pub session: PathBuf,
    /// The moment: 12.5, 1:02.5, `wall:<RFC 3339>`, `event:<seq>`, `bug:<id>.end`, optionally +3s.
    /// `event:<seq>` is the state before that event ran, `event:<seq>.end` after it
    #[arg(long, value_name = "TIME")]
    pub at: String,
    /// Only windows with this id, or whose application or title contains this text
    #[arg(long, value_name = "ID|TEXT")]
    pub window: Option<String>,
    /// Only this monitor (display id)
    #[arg(long, value_name = "ID")]
    pub monitor: Option<u64>,
    /// Show workspaces that are not showing too
    #[arg(long)]
    pub all: bool,
    /// Machine-readable JSON output
    #[arg(long)]
    pub json: bool,
}

/// The arguments of `diff`.
#[derive(Args, Debug)]
pub struct DiffArgs {
    /// Session directory
    pub session: PathBuf,
    /// The earlier moment (see `state --at`)
    pub from: String,
    /// The later moment
    pub to: String,
    /// Machine-readable JSON output
    #[arg(long)]
    pub json: bool,
}

/// A class of event, as the `--class` option of `events` spells it: one variant for each [`Class`],
/// which says what each means.
///
/// The variants have no doc comments: clap would show each as a line of the option's help, and
/// that switches `events --help` to its long layout.
#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum ClassArg {
    Command,
    Drag,
    Focus,
    Lifecycle,
    Echo,
    Manual,
    Monitor,
    Lock,
    Other,
}

impl From<ClassArg> for Class {
    fn from(class: ClassArg) -> Self {
        match class {
            ClassArg::Command => Self::Command,
            ClassArg::Drag => Self::Drag,
            ClassArg::Focus => Self::Focus,
            ClassArg::Lifecycle => Self::Lifecycle,
            ClassArg::Echo => Self::Echo,
            ClassArg::Manual => Self::Manual,
            ClassArg::Monitor => Self::Monitor,
            ClassArg::Lock => Self::Lock,
            ClassArg::Other => Self::Other,
        }
    }
}

/// The arguments of `events`.
#[derive(Args, Debug)]
pub struct EventsArgs {
    /// Session directory
    pub session: PathBuf,
    /// Only events received at or after this moment
    #[arg(long, value_name = "TIME")]
    pub from: Option<String>,
    /// Only events received at or before this moment
    #[arg(long, value_name = "TIME")]
    pub to: Option<String>,
    /// Only these classes (repeatable)
    #[arg(long = "class", value_enum)]
    pub classes: Vec<ClassArg>,
    /// Leave out echoes of komorebi's own moves
    #[arg(long)]
    pub hide_echo: bool,
    /// Only events whose name contains this text, e.g. `MoveEnd` or `FocusWorkspace`
    #[arg(long)]
    pub name: Option<String>,
    /// Only events about this process id
    #[arg(long)]
    pub pid: Option<i32>,
    /// Only events about this window id
    #[arg(long)]
    pub window: Option<u32>,
    /// Most events to print (0 for all)
    #[arg(long, default_value_t = 50)]
    pub limit: usize,
    /// Continue after this event sequence number (from a truncated answer's `next_cursor`)
    #[arg(long, value_name = "SEQ")]
    pub cursor: Option<u64>,
    /// Say why each event was classified as it was
    #[arg(long)]
    pub verbose: bool,
    /// Machine-readable JSON output
    #[arg(long)]
    pub json: bool,
}

pub(super) fn state(args: &StateArgs) -> eyre::Result<()> {
    let analysis = Analysis::open(&args.session)?;
    let at = analysis.resolve(&args.at)?;
    let data = analysis.state_answer(
        at.mach_ns,
        &StateQuery {
            window: args.window.clone(),
            monitor: args.monitor,
            all: args.all,
        },
    )?;

    if args.json {
        return print_json(&Envelope::ok(SCHEMA_STATE, data).warnings(analysis.warnings()));
    }

    print!("{}", report::state_text(&data));
    Ok(())
}

pub(super) fn diff_command(args: &DiffArgs) -> eyre::Result<()> {
    let analysis = Analysis::open(&args.session)?;
    let from = analysis.resolve(&args.from)?;
    let to = analysis.resolve(&args.to)?;
    if to.mach_ns < from.mach_ns {
        return Err(envelope::usage(format!(
            "{} is before {}: give the earlier moment first",
            args.to, args.from
        )));
    }

    let data = analysis.diff_answer(from.mach_ns, to.mach_ns)?;
    if args.json {
        return print_json(&Envelope::ok(SCHEMA_DIFF, data).warnings(analysis.warnings()));
    }

    print!("{}", report::diff_text(&data));
    Ok(())
}

pub(super) fn events(args: &EventsArgs) -> eyre::Result<()> {
    let analysis = Analysis::open(&args.session)?;
    let moment = |text: &Option<String>| -> eyre::Result<Option<u64>> {
        text.as_deref()
            .map(|text| analysis.resolve(text).map(|resolved| resolved.mach_ns))
            .transpose()
    };

    let answer = analysis.events_answer(&EventQuery {
        from_mach_ns: moment(&args.from)?,
        to_mach_ns: moment(&args.to)?,
        classes: args.classes.iter().copied().map(Class::from).collect(),
        hide_echo: args.hide_echo,
        name: args.name.clone(),
        pid: args.pid,
        window: args.window,
        limit: (args.limit != 0).then_some(args.limit),
        after_seq: args.cursor,
    });

    if args.json {
        let mut envelope = Envelope::ok(SCHEMA_EVENTS, answer.data).warnings(analysis.warnings());
        if answer.next_cursor.is_some() {
            envelope = envelope.truncated(answer.next_cursor);
        }
        return print_json(&envelope);
    }

    print!("{}", report::events_text(&answer.data, args.verbose));
    if let Some(cursor) = &answer.next_cursor {
        println!("... more; continue with --cursor {cursor}");
    }
    Ok(())
}

/// The arguments of `code`.
#[derive(Args, Debug)]
pub struct CodeArgs {
    /// Session directory
    pub session: PathBuf,
    /// What to explain: `log:<line>` (a line of komorebi's log) or `event:<seq>` (everything the
    /// handler of that event logged)
    pub refs: Vec<String>,
    /// Instead, find statements in komorebi's source whose message contains this text
    #[arg(long, value_name = "TEXT")]
    pub grep: Option<String>,
    /// Lines of source to show either side of each statement
    #[arg(long, default_value_t = 3)]
    pub context: usize,
    /// A checkout of the recorded komorebi, to show source from [default: `$KOMOREBI_MAC_SRC` or
    /// `$KOMOREBI_WINDOWS_SRC` for the session's komorebi, `$KOMOREBI_SOURCE`, or the one this
    /// recorder was built from]
    #[arg(long = "komorebi-src", alias = "code-root", value_name = "DIR")]
    pub code_root: Option<PathBuf>,
    /// Machine-readable JSON output
    #[arg(long)]
    pub json: bool,
}

pub(super) fn code(args: &CodeArgs) -> eyre::Result<()> {
    let analysis = Analysis::open(&args.session)?;
    let root = args
        .code_root
        .clone()
        .or_else(|| crate::annotations::default_code_root(analysis.flavor()));

    let data = if let Some(text) = &args.grep {
        analysis.code_search(text)
    } else {
        if args.refs.is_empty() {
            return Err(envelope::usage(
                "say what to explain: log:<line>, event:<seq>, or --grep <text>",
            ));
        }

        let mut combined: Option<crate::analysis::code::CodeData> = None;
        for text in &args.refs {
            let item: crate::analysis::ids::ItemRef = text.parse()?;
            let part = match item {
                crate::analysis::ids::ItemRef::Log(line) => {
                    analysis.code_for_lines(text, &[line], args.context, root.as_deref())?
                }
                crate::analysis::ids::ItemRef::Event(seq) => {
                    analysis.code_for_event(seq, args.context, root.as_deref())?
                }
                _ => {
                    return Err(envelope::usage(format!(
                        "'{text}': only log:<line> and event:<seq> have source code"
                    )));
                }
            };
            match &mut combined {
                Some(all) => {
                    all.about = format!("{} {text}", all.about);
                    all.lines.extend(part.lines);
                }
                None => combined = Some(part),
            }
        }
        combined.ok_or_else(|| envelope::usage("nothing to explain"))?
    };

    if args.json {
        let mut warnings = analysis.warnings();
        warnings.extend(data.skew.warning.clone());
        return print_json(&Envelope::ok(SCHEMA_CODE, data).warnings(warnings));
    }

    print!("{}", report::code_text(&data));
    Ok(())
}

/// The arguments of `trace`.
#[derive(Args, Debug)]
pub struct TraceArgs {
    /// Session directory
    pub session: PathBuf,
    /// The event: `event:<seq>` or just the number
    pub event: String,
    /// Machine-readable JSON output
    #[arg(long)]
    pub json: bool,
}

pub(super) fn trace(args: &TraceArgs) -> eyre::Result<()> {
    let analysis = Analysis::open(&args.session)?;
    let item: crate::analysis::ids::ItemRef = if args.event.chars().all(|c| c.is_ascii_digit()) {
        format!("event:{}", args.event).parse()?
    } else {
        args.event.parse()?
    };
    let crate::analysis::ids::ItemRef::Event(seq) = item else {
        return Err(envelope::usage(format!(
            "'{}': trace explains an event, e.g. event:12",
            args.event
        )));
    };

    let data = analysis.trace(seq)?;
    if args.json {
        let mut warnings = analysis.warnings();
        let skew = analysis.skew();
        warnings.extend(skew.warning);
        return print_json(
            &Envelope::ok(crate::analysis::trace::SCHEMA_TRACE, data).warnings(warnings),
        );
    }

    let skew = analysis.skew();
    if let Some(warning) = skew.warning {
        eprintln!("WARNING: {warning}");
    }
    print!("{}", report::trace_text(&data));
    Ok(())
}

/// The arguments of `window`.
#[derive(Args, Debug)]
pub struct WindowArgs {
    /// Session directory
    pub session: PathBuf,
    /// A window id, or text in the application name or title
    pub query: String,
    /// Most entries to print (0 for all)
    #[arg(long, default_value_t = 100)]
    pub limit: usize,
    /// Continue after this event sequence number
    #[arg(long, value_name = "SEQ")]
    pub cursor: Option<u64>,
    /// Machine-readable JSON output
    #[arg(long)]
    pub json: bool,
}

pub(super) fn window(args: &WindowArgs) -> eyre::Result<()> {
    let analysis = Analysis::open(&args.session)?;
    let limit = (args.limit != 0).then_some(args.limit);
    let data = analysis.window_history(&args.query, args.cursor, limit)?;
    let truncated = data.matching > data.entries.len();
    let next_cursor = truncated
        .then(|| data.entries.last().map(|entry| entry.seq.to_string()))
        .flatten();

    if args.json {
        let mut envelope = Envelope::ok(crate::analysis::history::SCHEMA_WINDOW, data)
            .warnings(analysis.warnings());
        if next_cursor.is_some() {
            envelope = envelope.truncated(next_cursor);
        }
        return print_json(&envelope);
    }

    print!("{}", report::history_text(&data));
    if let Some(cursor) = next_cursor {
        println!("continue with --cursor {cursor}");
    }
    Ok(())
}
