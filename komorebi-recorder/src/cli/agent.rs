//! The commands an agent starts with: `brief`, `overview`, `search`, `guide` and `schema`.
//!
//! | Command | Asks | Answer, with `--json` |
//! |---|---|---|
//! | `brief` | [`Analysis::brief`] | `brief/1` |
//! | `overview` | [`Analysis::overview`] | `overview/1` |
//! | `search` | [`Analysis::search`] | `search/1` |
//! | `guide` | nothing: prints [`GUIDE`] | `guide/1` |
//! | `schema` | nothing: prints the JSON Schema of an output | the schema itself |
//!
//! With `--json` an answer is wrapped in the envelope every investigation command uses
//! (`{ok, schema, data, warnings, truncated, next_cursor, error}`), and a failure is reported in the
//! same envelope with an exit code that says what kind of failure it was (0 ok, 1 error, 2 usage, 3
//! not found, 4 unusable session, 5 over budget). A list that was cut short sets `truncated` and
//! gives a `next_cursor`, which `search` takes back as `--cursor`.
//!
//! Covered by `tests/brief.rs`, which runs these commands through the real binary.

use super::print_json;
use crate::analysis::Analysis;
use crate::analysis::brief::SCHEMA_BRIEF;
use crate::analysis::budget::Budget;
use crate::analysis::envelope;
use crate::analysis::envelope::Envelope;
use crate::analysis::overview::SCHEMA_OVERVIEW;
use crate::analysis::schemas;
use crate::analysis::search::SCHEMA_SEARCH;
use crate::analysis::search::SearchQuery;
use crate::report;
use clap::Args;
use color_eyre::eyre;
use serde::Serialize;
use std::path::PathBuf;
use std::time::Duration;

/// The agent playbook, also printed by `guide`: the text of `guide.md`, which is part of the binary.
pub const GUIDE: &str = include_str!("../guide.md");

/// The arguments of `brief`.
#[derive(Args, Debug)]
pub struct BriefArgs {
    /// Session directory
    pub session: PathBuf,
    /// Machine-readable JSON output
    #[arg(long)]
    pub json: bool,
}

/// The arguments of `overview`.
#[derive(Args, Debug)]
pub struct OverviewArgs {
    /// Session directory
    pub session: PathBuf,
    /// Most findings to list (0 for all)
    #[arg(long, default_value_t = 20)]
    pub limit: usize,
    /// Machine-readable JSON output
    #[arg(long)]
    pub json: bool,
}

/// The arguments of `search`.
#[derive(Args, Debug)]
pub struct SearchArgs {
    /// Session directory
    pub session: PathBuf,
    /// The text to look for
    pub text: String,
    /// Treat the text as a regular expression
    #[arg(long)]
    pub regex: bool,
    /// Match case
    #[arg(long)]
    pub case_sensitive: bool,
    /// Only after this moment
    #[arg(long, value_name = "TIME")]
    pub from: Option<String>,
    /// Only before this moment
    #[arg(long, value_name = "TIME")]
    pub to: Option<String>,
    /// Also search the macOS trace (large; narrow it with --from/--to)
    #[arg(long)]
    pub oslog: bool,
    /// Most hits to print (0 for all)
    #[arg(long, default_value_t = 50)]
    pub limit: usize,
    /// Continue after this many hits (from a truncated answer's `next_cursor`)
    #[arg(long, value_name = "N")]
    pub cursor: Option<usize>,
    /// Give up after this many milliseconds
    #[arg(long, value_name = "MS")]
    pub budget_ms: Option<u64>,
    /// Machine-readable JSON output
    #[arg(long)]
    pub json: bool,
}

/// The arguments of `guide`.
#[derive(Args, Debug)]
pub struct GuideArgs {
    /// Machine-readable JSON output
    #[arg(long)]
    pub json: bool,
}

/// The arguments of `schema`.
#[derive(Args, Debug)]
pub struct SchemaArgs {
    /// Which output's schema, e.g. `brief/1`; omit to list them
    pub name: Option<String>,
}

pub(super) fn brief(args: &BriefArgs) -> eyre::Result<()> {
    let analysis = Analysis::open(&args.session)?;
    let data = analysis.brief()?;

    if args.json {
        let mut warnings = analysis.warnings();
        warnings.extend(data.skew.warning.clone());
        return print_json(&Envelope::ok(SCHEMA_BRIEF, data).warnings(warnings));
    }

    print!("{}", report::brief_text(&data));
    Ok(())
}

pub(super) fn overview(args: &OverviewArgs) -> eyre::Result<()> {
    let analysis = Analysis::open(&args.session)?;
    let data = analysis.overview((args.limit != 0).then_some(args.limit))?;

    if args.json {
        return print_json(&Envelope::ok(SCHEMA_OVERVIEW, data).warnings(analysis.warnings()));
    }

    print!("{}", report::overview_text(&data));
    Ok(())
}

pub(super) fn search(args: &SearchArgs) -> eyre::Result<()> {
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

    let limit = (args.limit != 0).then_some(args.limit);
    let skip = args.cursor.unwrap_or(0);
    let data = analysis.search(
        &SearchQuery {
            pattern: args.text.clone(),
            regex: args.regex,
            case_sensitive: args.case_sensitive,
            from_mach_ns: moment(&args.from)?,
            to_mach_ns: moment(&args.to)?,
            oslog: args.oslog,
            skip,
            limit,
        },
        &budget,
    )?;

    let truncated = limit.is_some_and(|limit| data.matching > limit);
    let next_cursor = truncated.then(|| (skip + data.hits.len()).to_string());

    if args.json {
        let mut envelope = Envelope::ok(SCHEMA_SEARCH, data).warnings(analysis.warnings());
        if truncated {
            envelope = envelope.truncated(next_cursor);
        }
        return print_json(&envelope);
    }

    print!("{}", report::search_text(&data));
    if let Some(cursor) = next_cursor {
        println!("... more; continue with --cursor {cursor}");
    }
    Ok(())
}

#[derive(Serialize)]
struct GuideData {
    guide: &'static str,
}

pub(super) fn guide(args: &GuideArgs) -> eyre::Result<()> {
    if args.json {
        return print_json(&Envelope::ok("guide/1", GuideData { guide: GUIDE }));
    }

    print!("{GUIDE}");
    Ok(())
}

pub(super) fn schema(args: &SchemaArgs) -> eyre::Result<()> {
    let Some(name) = &args.name else {
        for entry in schemas::ALL {
            println!("{:<22} {}", entry.name, entry.about);
        }
        println!(
            "\nevery command's --json output is {{ok, schema, data, warnings, truncated, next_cursor, error}}; `schema` names which of these `data` is"
        );
        return Ok(());
    };

    let entry = schemas::find(name).ok_or_else(|| {
        envelope::not_found(format!(
            "there is no schema '{name}'; run `komorebi-recorder schema` to list them"
        ))
    })?;
    print_json(&(entry.schema)())
}
