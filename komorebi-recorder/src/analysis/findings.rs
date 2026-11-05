//! Every finding in one list: what the detectors found and what an agent concluded.
//!
//! `overview` ranks what the detectors found, and `annotations --kind finding` lists what an
//! agent wrote down. [`Analysis::merged_findings`] is both together, which is what the `findings`
//! command prints: an agent coming back to a session sees its own conclusions next to what the
//! detectors say, and can filter by kind and severity.
//!
//! # Stable ids
//!
//! A detector finding has no id of its own, so [`Finding::stable_id`] makes one: `f-` and six hex
//! digits of an FNV-1a hash of the finding's `kind`, its first reference (`event:12`, `log:345`,
//! `incident:2`) and the ids of the windows it is about (so that two windows wrong at the same event
//! are two ids); a finding with no reference uses the five-second bucket of video time it began in
//! instead of the reference. The hash is fixed (it does not change between runs, machines
//! or versions of the recorder), and it does not depend on the score, the summary or the bug
//! regions, so the same finding keeps its id when a bug region is added and the ranking changes.
//! It is the same for the same session however many times it is analysed. Two *different*
//! sessions share an id only where the same kind of finding is at the same reference, which for
//! numbered references is by coincidence of numbering: ids compare findings across analyses of one
//! session (before and after a fix to the detectors, say), and are not a fingerprint of a bug across
//! recordings. Agent findings keep the annotation id (`a-3f9c1e02`) they were given.

use super::Analysis;
use super::invariants::Severity;
use super::overview::Finding;
use crate::annotations::Kind;
use color_eyre::eyre;
use schemars::JsonSchema;
use serde::Serialize;

/// The `schema` of the `findings` command's output: the name [`FindingsData`] has in an envelope.
pub const SCHEMA_FINDINGS: &str = "findings/1";

/// The `kind` of a finding an agent wrote down with `annotate add --kind finding`.
pub const AGENT_KIND: &str = "agent-finding";

/// Who a finding is from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, JsonSchema)]
#[serde(rename_all = "kebab-case")]
pub enum FindingSource {
    /// Found by one of the detectors (see `overview`).
    Detector,
    /// Written down by an agent or a person with `annotate add --kind finding`.
    Agent,
}

/// One finding, from either source.
#[derive(Debug, Clone, PartialEq, Serialize, JsonSchema)]
pub struct FindingItem {
    /// `f-xxxxxx` for a detector finding (see [`Finding::stable_id`]), the annotation id for an
    /// agent's.
    pub id: String,
    /// Where it came from.
    pub source: FindingSource,
    /// The detector's kind (`invariant:<kind>`, `log-error`, ...), or `agent-finding`.
    pub kind: String,
    /// How serious the detector thinks it is. `None` for an agent's finding, which is a conclusion
    /// and has no severity.
    pub severity: Option<Severity>,
    /// The detector's ranking score (see `overview`); `None` for an agent's finding.
    pub score: Option<i64>,
    /// What was seen (a detector), or the conclusion (an agent).
    pub summary: String,
    /// When it began, in seconds of video time.
    pub start_video_s: f64,
    /// When it ended, in seconds of video time.
    pub end_video_s: f64,
    /// Where to look: `event:12`, `log:345`, `incident:2`, `code:<path>:<line>`, ...
    pub refs: Vec<String>,
    /// The bug regions it overlaps (a detector), or the one it explains (an agent).
    pub bugs: Vec<String>,
    /// Who wrote an agent's finding.
    pub author: Option<String>,
    /// A command that digs into it, if there is one.
    pub next: Option<String>,
}

/// What [`Analysis::merged_findings`] is asked for.
#[derive(Debug, Clone, Default)]
pub struct FindingsQuery {
    /// Only findings of this kind: the whole `kind`, or the part before the colon (`invariant`
    /// matches every `invariant:<kind>`).
    pub kind: Option<String>,
    /// Only detector findings of this severity. Agent findings have none, so a severity leaves them
    /// out.
    pub severity: Option<Severity>,
    /// The most findings to return; `None` for all of them.
    pub limit: Option<usize>,
}

/// The `data` of the `findings` command.
#[derive(Debug, Clone, PartialEq, Serialize, JsonSchema)]
pub struct FindingsData {
    /// Agent findings first (earliest first), then detector findings, ranked as in `overview`.
    pub findings: Vec<FindingItem>,
    /// How many findings matched the filters, including any cut off by the limit.
    pub total: usize,
    /// How many of those were written by an agent.
    pub agent: usize,
    /// How many of those were found by a detector.
    pub detector: usize,
}

/// FNV-1a over `bytes`: fixed forever, unlike the standard library's hashers.
fn fnv1a(bytes: &[u8]) -> u64 {
    bytes.iter().fold(0xcbf2_9ce4_8422_2325, |hash, byte| {
        (hash ^ u64::from(*byte)).wrapping_mul(0x0000_0100_0000_01b3)
    })
}

impl Finding {
    /// Returns the finding's stable id, `f-` and six hex digits.
    ///
    /// It is made from the kind, the first reference (or, for a finding with none, the five-second
    /// bucket of video time it began in) and the windows it is about; see the [module](self) for what
    /// that does and does not promise.
    ///
    /// # Example
    ///
    /// ```rust
    /// use komorebi_recorder::analysis::Analysis;
    /// use komorebi_recorder::testkit::SessionBuilder;
    ///
    /// let (session, _) = SessionBuilder::new("doc-stable-id")
    ///     .log(3.0, "ERROR", "komorebi::window: could not do the thing")
    ///     .build();
    /// let found = Analysis::open(&session.dir)?.findings()?;
    ///
    /// let id = found[0].stable_id();
    /// assert!(id.starts_with("f-") && id.len() == 8);
    /// // the same finding, the same id
    /// assert_eq!(id, found[0].clone().stable_id());
    /// # Ok::<(), Box<dyn std::error::Error>>(())
    /// ```
    #[must_use]
    pub fn stable_id(&self) -> String {
        let place = self.refs.first().map_or_else(
            || format!("t{}", (self.start_video_s.max(0.0) / 5.0).floor() as u64),
            Clone::clone,
        );
        let mut windows = self.window_ids.clone();
        windows.sort_unstable();
        let hash = fnv1a(format!("{}\0{place}\0{windows:?}", self.kind).as_bytes());
        format!("f-{:06x}", hash & 0x00ff_ffff)
    }
}

impl Analysis {
    /// Returns the detectors' findings and the agent's, together, filtered by `query`.
    ///
    /// The detector findings are those of [`Analysis::findings`], ranked against the bug regions by
    /// [`Analysis::rank`]; the agent's are the `finding` annotations. Agent findings come first.
    ///
    /// # Errors
    ///
    /// Returns an error if the annotations cannot be read, or a recorded state cannot (see
    /// [`Analysis::findings`]).
    ///
    /// # Example
    ///
    /// ```rust
    /// use komorebi_recorder::analysis::Analysis;
    /// use komorebi_recorder::analysis::findings::FindingsQuery;
    /// use komorebi_recorder::testkit::SessionBuilder;
    ///
    /// let (session, _) = SessionBuilder::new("doc-merged-findings")
    ///     .log(3.0, "ERROR", "komorebi::window: could not do the thing")
    ///     .build();
    /// let data = Analysis::open(&session.dir)?.merged_findings(&FindingsQuery::default())?;
    /// assert_eq!(data.detector, 1);
    /// assert_eq!(data.findings[0].kind, "log-error");
    /// # Ok::<(), Box<dyn std::error::Error>>(())
    /// ```
    pub fn merged_findings(&self, query: &FindingsQuery) -> eyre::Result<FindingsData> {
        let bugs = self.bugs()?;
        let bug_refs: Vec<&crate::annotations::Annotation> = bugs.iter().collect();
        let ranked = self.rank(self.findings()?, &bug_refs);
        let loaded = crate::annotations::load(&self.session.dir)?;

        let mut items: Vec<FindingItem> = loaded
            .annotations
            .iter()
            .filter(|annotation| annotation.kind == Kind::Finding)
            .map(|annotation| {
                let view = annotation.view(&self.manifest);
                FindingItem {
                    id: view.id,
                    source: FindingSource::Agent,
                    kind: AGENT_KIND.to_string(),
                    severity: None,
                    score: None,
                    summary: view.text,
                    start_video_s: view.start_video_s,
                    end_video_s: view.end_video_s,
                    refs: view.refs.iter().map(ToString::to_string).collect(),
                    bugs: view.about.into_iter().collect(),
                    author: Some(view.author),
                    next: None,
                }
            })
            .collect();
        items.sort_by(|a, b| a.start_video_s.total_cmp(&b.start_video_s));

        items.extend(ranked.into_iter().map(|finding| FindingItem {
            id: finding.stable_id(),
            source: FindingSource::Detector,
            kind: finding.kind,
            severity: Some(finding.severity),
            score: Some(finding.score),
            summary: finding.summary,
            start_video_s: finding.start_video_s,
            end_video_s: finding.end_video_s,
            refs: finding.refs,
            bugs: finding.bugs,
            author: None,
            next: finding.next,
        }));

        items.retain(|item| {
            query.kind.as_deref().is_none_or(|kind| {
                item.kind == kind
                    || item
                        .kind
                        .strip_prefix(kind)
                        .is_some_and(|rest| rest.starts_with(':'))
            }) && query
                .severity
                .is_none_or(|severity| item.severity == Some(severity))
        });

        let total = items.len();
        let agent = items
            .iter()
            .filter(|item| item.source == FindingSource::Agent)
            .count();
        items.truncate(query.limit.unwrap_or(usize::MAX));

        Ok(FindingsData {
            findings: items,
            total,
            agent,
            detector: total - agent,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::analysis::invariants::Severity;

    fn finding(kind: &str, refs: &[&str], start_video_s: f64) -> Finding {
        Finding {
            kind: kind.to_string(),
            severity: Severity::Warn,
            score: 0,
            summary: String::new(),
            start_video_s,
            end_video_s: start_video_s,
            start_mach_ns: 0,
            end_mach_ns: 0,
            refs: refs.iter().map(ToString::to_string).collect(),
            window_ids: Vec::new(),
            persisted_to_end: false,
            bugs: Vec::new(),
            next: None,
        }
    }

    #[test]
    fn the_id_is_fixed_and_follows_the_kind_and_first_reference() {
        let a = finding("log-error", &["log:12"], 3.0);
        // pinned: the hash must not change between versions
        assert_eq!(
            a.stable_id(),
            finding("log-error", &["log:12"], 9.0).stable_id()
        );
        assert_ne!(
            a.stable_id(),
            finding("log-error", &["log:13"], 3.0).stable_id()
        );
        assert_ne!(
            a.stable_id(),
            finding("log-warning", &["log:12"], 3.0).stable_id()
        );
        // the score and the summary do not matter
        let mut ranked = a.clone();
        ranked.score = 1300;
        ranked.summary = "different".to_string();
        assert_eq!(a.stable_id(), ranked.stable_id());
        assert_eq!(a.stable_id().len(), 8);
        assert_eq!(fnv1a(b"a"), 0xaf63_dc4c_8601_ec8c);
    }

    #[test]
    fn two_windows_wrong_at_the_same_event_are_two_ids() {
        let mut one = finding("invariant:layout-mismatch", &["event:4"], 3.0);
        one.window_ids = vec![7];
        let mut other = one.clone();
        other.window_ids = vec![9];
        assert_ne!(one.stable_id(), other.stable_id());

        // the order the windows are listed in does not matter
        let mut both = one.clone();
        both.window_ids = vec![9, 7];
        let mut reversed = one.clone();
        reversed.window_ids = vec![7, 9];
        assert_eq!(both.stable_id(), reversed.stable_id());
    }

    #[test]
    fn a_finding_with_no_reference_uses_a_five_second_bucket() {
        let early = finding("event-storm", &[], 1.0);
        assert_eq!(
            early.stable_id(),
            finding("event-storm", &[], 4.9).stable_id()
        );
        assert_ne!(
            early.stable_id(),
            finding("event-storm", &[], 5.1).stable_id()
        );
    }
}
