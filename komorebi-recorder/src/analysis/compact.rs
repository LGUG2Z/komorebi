//! `brief --compact`: the brief cut down to what fits in a small context window.
//!
//! The full [`BriefData`] carries every event, change and state of each bug region, which is what an
//! investigation wants after it has started and more than an agent wants to read first. A
//! [`CompactBrief`] is what to read before that, in about 40 lines: the user's words, one line on the
//! state of the recording, the three highest-ranked findings (each with the references to look at),
//! and the exact commands to run next. [`BriefData::compact`] makes one from a brief that has already
//! been made, so it shows the same findings and the same commands the full brief does.
//!
//! Limits, so that the size is predictable: at most [`TOP_FINDINGS`] findings, at most
//! [`MAX_TEXT`] characters of each bug's description, [`MAX_SUMMARY`] of each finding's summary, at most
//! [`MAX_NEXT`] commands, and every bug region is listed (a recording rarely has more than one or
//! two). Anything cut off is counted, and the full `brief` has it.

use super::brief::BriefData;
use super::invariants::Severity;
use crate::timecode;
use schemars::JsonSchema;
use serde::Serialize;
use std::fmt::Write;

/// The `schema` of `brief --compact --json`: the name [`CompactBrief`] has in an envelope.
pub const SCHEMA_BRIEF_COMPACT: &str = "brief-compact/1";

/// How many findings a compact brief lists.
pub const TOP_FINDINGS: usize = 3;
/// How many characters of a description or a summary a compact brief keeps.
pub const MAX_TEXT: usize = 300;
/// How many characters of a finding's summary a compact brief keeps.
pub const MAX_SUMMARY: usize = 160;
/// How many commands a compact brief suggests.
pub const MAX_NEXT: usize = 6;

/// A bug region the user marked, in the user's words.
#[derive(Debug, Clone, PartialEq, Serialize, JsonSchema)]
pub struct CompactRegion {
    /// The annotation id, e.g. `a-3f9c1e02`.
    pub id: String,
    /// What the user said is wrong, cut to [`MAX_TEXT`] characters.
    pub text: String,
    /// What should have happened instead, cut the same way.
    pub expected: Option<String>,
    /// Where the region starts, in seconds of video time.
    pub start_video_s: f64,
    /// Where it ends, in seconds of video time (the end of the recording if open-ended).
    pub end_video_s: f64,
}

/// One of the highest-ranked findings.
#[derive(Debug, Clone, PartialEq, Serialize, JsonSchema)]
pub struct CompactFinding {
    /// The stable id, `f-xxxxxx` (see [`super::findings`]).
    pub id: String,
    /// How serious it is.
    pub severity: Severity,
    /// The detector's kind.
    pub kind: String,
    /// What was seen, cut to [`MAX_SUMMARY`] characters.
    pub summary: String,
    /// When it began, in seconds of video time.
    pub at_video_s: f64,
    /// Where to look: `event:12`, `log:345`, ...
    pub refs: Vec<String>,
}

/// The brief in a few lines: the `data` of `brief --compact --json`.
#[derive(Debug, Clone, PartialEq, Serialize, JsonSchema)]
pub struct CompactBrief {
    /// The session directory.
    pub session: String,
    /// The state of the recording in one line: how long, what, and komorebi's state at the end.
    pub state: String,
    /// The bug regions the user marked; empty if none was.
    pub regions: Vec<CompactRegion>,
    /// How many findings there are in or near the regions (or in the whole recording if no region
    /// was marked), of which the first [`TOP_FINDINGS`] are in `top_findings`.
    pub findings_total: usize,
    /// The highest-ranked findings.
    pub top_findings: Vec<CompactFinding>,
    /// How many findings an agent has already recorded (`findings` lists them).
    pub recorded_findings: usize,
    /// Commands to run next, exactly as they can be pasted.
    pub next: Vec<String>,
}

fn cut(text: &str, most: usize) -> String {
    let flat = text.split_whitespace().collect::<Vec<_>>().join(" ");
    if flat.chars().count() <= most {
        return flat;
    }
    let mut cut: String = flat.chars().take(most).collect();
    cut.push_str("...");
    cut
}

impl BriefData {
    /// Returns the compact form of this brief; see the [module](super::compact) for its limits.
    ///
    /// The findings are those of the bug regions' briefs (or of the whole recording when no region
    /// was marked), each once, highest score first. The commands are those the first bug region's
    /// brief suggests, then `findings` and the full `brief`.
    ///
    /// # Example
    ///
    /// ```rust
    /// use komorebi_recorder::analysis::Analysis;
    /// use komorebi_recorder::testkit::SessionBuilder;
    ///
    /// let (session, _) = SessionBuilder::new("doc-compact")
    ///     .log(3.0, "ERROR", "komorebi::window: could not do the thing")
    ///     .build();
    /// let compact = Analysis::open(&session.dir)?.brief()?.compact();
    ///
    /// assert!(compact.regions.is_empty());
    /// assert_eq!(compact.top_findings[0].kind, "log-error");
    /// assert!(compact.top_findings[0].id.starts_with("f-"));
    /// # Ok::<(), Box<dyn std::error::Error>>(())
    /// ```
    #[must_use]
    pub fn compact(&self) -> CompactBrief {
        let dir = &self.session.dir;

        let mut findings: Vec<&super::overview::Finding> = self
            .bugs
            .iter()
            .map(|bug| &bug.region)
            .chain(self.whole_session.as_ref())
            .flat_map(|region| region.findings.iter())
            .collect();
        findings.sort_by(|a, b| {
            b.score
                .cmp(&a.score)
                .then(a.start_mach_ns.cmp(&b.start_mach_ns))
        });
        let mut seen = std::collections::BTreeSet::new();
        findings.retain(|finding| seen.insert(finding.stable_id()));

        let region = self
            .bugs
            .first()
            .map(|bug| &bug.region)
            .or(self.whole_session.as_ref());
        let at_end = region.and_then(|region| region.state_at_end.as_ref());
        let session = &self.session;
        let state = format!(
            "{:.0} s of video, {} display(s), {} events, {} log lines, {} incidents; sync {}; {}",
            session.duration_s,
            session.displays.len(),
            session.events,
            session.log_lines,
            session.incidents,
            session.sync_quality,
            at_end.map_or_else(
                || "no komorebi state recorded".to_string(),
                |state| {
                    let wrong = state.violations.len();
                    if wrong == 0 {
                        format!("at the end: {}, nothing wrong", state.summary)
                    } else {
                        format!("at the end: {}, {wrong} thing(s) wrong", state.summary)
                    }
                }
            ),
        );

        let mut next: Vec<String> = match self.bugs.first() {
            Some(bug) => bug.commands.iter().take(MAX_NEXT - 2).cloned().collect(),
            None => self
                .next
                .iter()
                .filter(|command| command.starts_with("no bug region"))
                .cloned()
                .collect(),
        };
        next.push(format!("komorebi-recorder findings {dir}"));
        next.push(format!("komorebi-recorder brief {dir}"));

        CompactBrief {
            session: dir.clone(),
            state,
            regions: self
                .bugs
                .iter()
                .map(|bug| CompactRegion {
                    id: bug.bug.id.clone(),
                    text: cut(&bug.bug.text, MAX_TEXT),
                    expected: bug.bug.expected.as_deref().map(|text| cut(text, MAX_TEXT)),
                    start_video_s: bug.bug.start_video_s,
                    end_video_s: bug.bug.end_video_s,
                })
                .collect(),
            findings_total: findings.len(),
            top_findings: findings
                .iter()
                .take(TOP_FINDINGS)
                .map(|finding| CompactFinding {
                    id: finding.stable_id(),
                    severity: finding.severity,
                    kind: finding.kind.clone(),
                    summary: cut(&finding.summary, MAX_SUMMARY),
                    at_video_s: finding.start_video_s,
                    refs: finding.refs.clone(),
                })
                .collect(),
            recorded_findings: self.findings_recorded.len(),
            next,
        }
    }
}

impl CompactBrief {
    /// Returns the brief as plain text, in about 40 lines.
    ///
    /// # Example
    ///
    /// ```rust
    /// use komorebi_recorder::analysis::Analysis;
    /// use komorebi_recorder::testkit::SessionBuilder;
    ///
    /// let (session, _) = SessionBuilder::new("doc-compact-text").build();
    /// let text = Analysis::open(&session.dir)?.brief()?.compact().text();
    /// assert!(text.lines().count() < 40);
    /// assert!(text.contains("komorebi-recorder findings"));
    /// # Ok::<(), Box<dyn std::error::Error>>(())
    /// ```
    #[must_use]
    pub fn text(&self) -> String {
        let mut out = String::new();

        if self.regions.is_empty() {
            let _ = writeln!(out, "NO BUG REGION WAS MARKED");
        }
        for region in &self.regions {
            let _ = writeln!(
                out,
                "BUG {} ({} -> {}), as the user said:\n  {}",
                region.id,
                timecode::format(region.start_video_s),
                timecode::format(region.end_video_s),
                region.text
            );
            if let Some(expected) = &region.expected {
                let _ = writeln!(out, "  expected: {expected}");
            }
        }

        let _ = writeln!(out, "\nrecording {}\n  {}", self.session, self.state);

        let _ = writeln!(
            out,
            "\nfindings ({} shown of {}; {} recorded by an agent):",
            self.top_findings.len(),
            self.findings_total,
            self.recorded_findings
        );
        if self.top_findings.is_empty() {
            let _ = writeln!(out, "  none: no detector fired");
        }
        for finding in &self.top_findings {
            let severity = serde_json::to_value(finding.severity)
                .ok()
                .and_then(|value| value.as_str().map(str::to_string))
                .unwrap_or_default();
            let _ = writeln!(
                out,
                "  {} {severity} {} at {}: {}  [{}]",
                finding.id,
                finding.kind,
                timecode::format(finding.at_video_s),
                finding.summary,
                finding.refs.join(" ")
            );
        }

        let _ = writeln!(out, "\nnext:");
        for command in &self.next {
            let _ = writeln!(out, "  {command}");
        }
        out
    }
}
