//! `logq`: structured queries over a session's `komorebi.log` (see [`crate::analysis::logq`]).
//!
//! With `--json` the answer is the `logq/1` envelope. A page cut short by `--limit` sets
//! `truncated` and a `next_cursor`, which `--cursor` takes back. `--count` prints only the number
//! of matching lines.
//!
//! Covered by `tests/logq.rs`, which runs the command through the real binary.

use super::print_json;
use crate::analysis::Analysis;
use crate::analysis::budget::Budget;
use crate::analysis::envelope::Envelope;
use crate::analysis::logq::LogqQuery;
use crate::analysis::logq::SCHEMA_LOGQ;
use crate::analysis::logq::logq_text;
use clap::Args;
use color_eyre::eyre;
use std::path::PathBuf;
use std::time::Duration;

/// The arguments of `logq`.
#[derive(Args, Debug)]
pub struct LogqArgs {
    /// Session directory
    pub session: PathBuf,
    /// Predicates joined by `and`/`or`: `level=ERROR and message~"ghost"`, `hwnd=0x77179C`,
    /// `command=MoveWindow`. Operators: = != ~ !~. Fields: level, target, message, line, spans,
    /// command, event, or any span field (hwnd, winevent, direction, ...)
    pub query: String,
    /// Only after this moment (video time, `log:N`, `event:N`, ...)
    #[arg(long, value_name = "TIME")]
    pub from: Option<String>,
    /// Only before this moment
    #[arg(long, value_name = "TIME")]
    pub to: Option<String>,
    /// Most hits (or groups) to print (0 for all)
    #[arg(long, default_value_t = 50)]
    pub limit: usize,
    /// Continue after this many hits (from a truncated answer's `next_cursor`)
    #[arg(long, value_name = "N")]
    pub cursor: Option<usize>,
    /// Print only how many lines match
    #[arg(long, conflicts_with = "group_by")]
    pub count: bool,
    /// Count the matching lines by each distinct value of this field, most common first
    #[arg(long, value_name = "FIELD")]
    pub group_by: Option<String>,
    /// Show each hit's span chain in text output (cut to 100 characters)
    #[arg(long)]
    pub spans: bool,
    /// Give up after this many milliseconds
    #[arg(long, value_name = "MS")]
    pub budget_ms: Option<u64>,
    /// Machine-readable JSON output
    #[arg(long)]
    pub json: bool,
}

pub(super) fn logq(args: &LogqArgs) -> eyre::Result<()> {
    let analysis = Analysis::open(&args.session)?;
    let moment = |text: &Option<String>| -> eyre::Result<Option<u64>> {
        text.as_deref()
            .map(|text| analysis.resolve(text).map(|resolved| resolved.mach_ns))
            .transpose()
    };

    let mut budget = Budget::unlimited().reading_at_most(512 * 1024 * 1024);
    if let Some(ms) = args.budget_ms {
        budget = budget.within(Duration::from_millis(ms));
    }

    let limit = super::limit(args.limit);
    let skip = args.cursor.unwrap_or(0);
    let data = analysis.logq(
        &LogqQuery {
            query: args.query.clone(),
            from_mach_ns: moment(&args.from)?,
            to_mach_ns: moment(&args.to)?,
            group_by: args.group_by.clone(),
            skip,
            limit,
            count_only: args.count,
        },
        &budget,
    )?;

    let (returned, total) = if data.group_by.is_some() {
        (data.groups.len(), data.distinct)
    } else {
        (data.hits.len(), data.matching)
    };
    let truncated = !args.count && limit.is_some() && skip + returned < total;
    let next_cursor = truncated.then(|| (skip + returned).to_string());

    if args.json {
        let mut warnings = analysis.warnings();
        warnings.extend(data.notes.clone());
        let mut envelope = Envelope::ok(SCHEMA_LOGQ, data).warnings(warnings);
        if truncated {
            envelope = envelope.truncated(next_cursor);
        }
        return print_json(&envelope);
    }

    print!("{}", logq_text(&data, args.count, args.spans));
    for note in &data.notes {
        eprintln!("note: {note}");
    }
    if let Some(cursor) = next_cursor {
        eprintln!(
            "... {} of {total} shown; continue with --cursor {cursor}",
            returned
        );
    }
    Ok(())
}
