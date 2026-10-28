//! `findings`: what the detectors found and what an agent concluded, in one list.
//!
//! The answer is [`Analysis::merged_findings`] (`findings/1` with `--json`): the findings an agent
//! recorded with `annotate add --kind finding` first, then the detectors' findings ranked as
//! `overview` ranks them, each detector finding with the stable `f-xxxxxx` id
//! ([`crate::analysis::overview::Finding::stable_id`]). `--kind` and `--severity` narrow it and
//! `--limit` cuts it (20 by default; 0 for all); a cut list is marked `truncated`.
//!
//! Covered by `tests/findings.rs`, which runs the command through the real binary.

use super::print_json;
use crate::analysis::Analysis;
use crate::analysis::envelope::Envelope;
use crate::analysis::findings::FindingItem;
use crate::analysis::findings::FindingSource;
use crate::analysis::findings::FindingsData;
use crate::analysis::findings::FindingsQuery;
use crate::analysis::findings::SCHEMA_FINDINGS;
use crate::analysis::invariants::Severity;
use crate::timecode;
use clap::Args;
use clap::ValueEnum;
use color_eyre::eyre;
use std::fmt::Write;
use std::path::PathBuf;

/// A severity, as `--severity` spells it.
#[derive(Debug, Clone, Copy, ValueEnum)]
pub enum SeverityArg {
    /// The state is wrong in a way a person would see
    Error,
    /// Probably wrong, or an application's doing
    Warn,
    /// Worth knowing, not a fault in itself
    Info,
}

impl From<SeverityArg> for Severity {
    fn from(severity: SeverityArg) -> Self {
        match severity {
            SeverityArg::Error => Self::Error,
            SeverityArg::Warn => Self::Warn,
            SeverityArg::Info => Self::Info,
        }
    }
}

/// The arguments of `findings`.
#[derive(Args, Debug)]
pub struct FindingsArgs {
    /// Session directory
    pub session: PathBuf,
    /// Only this kind: `log-error`, `invariant` (every `invariant:<kind>`), `invariant:<kind>`,
    /// `agent-finding`, ...
    #[arg(long, value_name = "KIND")]
    pub kind: Option<String>,
    /// Only detector findings of this severity (an agent's findings have none, so they are left out)
    #[arg(long, value_enum)]
    pub severity: Option<SeverityArg>,
    /// Most findings to list (0 for all)
    #[arg(long, default_value_t = 20)]
    pub limit: usize,
    /// Machine-readable JSON output
    #[arg(long)]
    pub json: bool,
}

/// Renders the findings as text, one finding to a line and its next command under it.
#[must_use]
pub fn findings_text(data: &FindingsData) -> String {
    let mut out = String::new();
    let _ = writeln!(
        out,
        "{} finding(s): {} recorded by an agent, {} found by detectors",
        data.total, data.agent, data.detector
    );

    for item in &data.findings {
        out.push_str(&finding_line(item));
    }
    if data.findings.len() < data.total {
        let _ = writeln!(
            out,
            "... {} more; --limit 0 lists them all",
            data.total - data.findings.len()
        );
    }
    out
}

fn finding_line(item: &FindingItem) -> String {
    let mut out = String::new();
    let who = match (item.source, item.severity) {
        (FindingSource::Agent, _) => "agent".to_string(),
        (FindingSource::Detector, Some(severity)) => serde_json::to_value(severity)
            .ok()
            .and_then(|value| value.as_str().map(str::to_string))
            .unwrap_or_default(),
        (FindingSource::Detector, None) => "detector".to_string(),
    };
    let _ = writeln!(
        out,
        "  {} {who:<5} {} at {}: {}{}",
        item.id,
        item.kind,
        timecode::format(item.start_video_s),
        item.summary,
        if item.refs.is_empty() {
            String::new()
        } else {
            format!("  [{}]", item.refs.join(" "))
        }
    );
    if let Some(next) = &item.next {
        let _ = writeln!(out, "      next: {next}");
    }
    out
}

pub(super) fn findings(args: &FindingsArgs) -> eyre::Result<()> {
    let analysis = Analysis::open(&args.session)?;
    let data = analysis.merged_findings(&FindingsQuery {
        kind: args.kind.clone(),
        severity: args.severity.map(Severity::from),
        limit: super::limit(args.limit),
    })?;

    if args.json {
        let cut = data.findings.len() < data.total;
        let mut envelope = Envelope::ok(SCHEMA_FINDINGS, data).warnings(analysis.warnings());
        if cut {
            envelope = envelope.truncated(None);
        }
        return print_json(&envelope);
    }

    print!("{}", findings_text(&data));
    Ok(())
}
