//! `assert`: scripted pass/fail checks over a recording, for an agent's own scenario tests.
//!
//! Each `--check` is a kind, its arguments and a comparison (see
//! [`crate::analysis::assertions`] for the grammar and the kinds). The answer is a line per check
//! and a summary; with `--json` it is the `assert/1` envelope. The exit code is the result: 0 when
//! every check passes, 6 when one does not hold, and 2, 3 or 4 when the checks are malformed or
//! cannot be measured. Read-only: it never writes to the session.
//!
//! Covered by `tests/assert.rs`, which runs the command through the real binary.

use super::print_json;
use crate::analysis::Analysis;
use crate::analysis::assertions;
use crate::analysis::assertions::SCHEMA_ASSERT;
use crate::analysis::envelope::Envelope;
use crate::analysis::envelope::ExitKind;
use crate::analysis::envelope::reported;
use clap::Args;
use color_eyre::eyre;
use std::path::PathBuf;

/// The arguments of `assert`.
#[derive(Args, Debug)]
pub struct AssertArgs {
    /// Session directory
    pub session: PathBuf,
    /// A check, `<kind> <args...> <metric><op><number>` (repeatable): `findings kind=layout-mismatch
    /// count<=0`, `log "level=ERROR" count==0`, `motion region=0,0,800,600 segments<=2`,
    /// `blank region=0,0,800,600 color=white ms<=100`, `annotations kind=note count>=1`. Operators:
    /// == != <= >= < >. Exits 0 if all pass, 6 if one fails
    #[arg(long = "check", value_name = "CHECK", required = true)]
    pub checks: Vec<String>,
    /// Machine-readable JSON output
    #[arg(long)]
    pub json: bool,
}

pub(super) fn assert(args: &AssertArgs) -> eyre::Result<()> {
    // read every check before opening the session: a typo is a usage error whatever the session is
    let parsed = assertions::parse_all(&args.checks)?;
    let analysis = Analysis::open(&args.session)?;
    let data = assertions::run_parsed(&analysis, &parsed);

    let kind = data.exit_kind();
    let summary = data.summary();
    if args.json {
        let mut envelope = Envelope::ok(SCHEMA_ASSERT, data).warnings(analysis.warnings());
        if kind != ExitKind::Ok {
            envelope = envelope.with_error(kind, summary.clone());
        }
        print_json(&envelope)?;
    } else {
        for warning in analysis.warnings() {
            eprintln!("note: {warning}");
        }
        print!("{}", data.to_text());
    }

    match kind {
        ExitKind::Ok => Ok(()),
        kind => Err(reported(kind, summary)),
    }
}
