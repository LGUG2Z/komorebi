//! The first thing an investigator reads: what the user says is wrong, and what the recording shows
//! around it.
//!
//! The user's bug region and description are the ground truth of *what* went wrong and come first,
//! verbatim. Everything after them is evidence of *why*, drawn from the recording: the events in the
//! region (with komorebi's own echoes counted but not listed), what looked wrong and for how long,
//! the state as the bug began and as it ended and the difference, what happened to the machine
//! nearby, and the few events most worth tracing first, each with the reason it was chosen.
//!
//! [`Analysis::brief`] makes a [`BriefData`]: one [`BugBrief`] for each bug region the user marked,
//! each holding a [`RegionBrief`] of what the recording shows around it. A recording with no bug
//! marked gets the same digest over the whole recording instead ([`BriefData::whole_session`]).
//!
//! # What a region covers
//!
//! | Part | What it holds | Cut off at |
//! |---|---|---|
//! | the region | the bug region, widened to start half of [`PRE_ROLL_NS`] (five seconds) before it so that causes just before it are inside; the end is the bug's end, or the end of the recording for an open-ended region | |
//! | [`RegionEvents`] | the events received in the region, counted by class; komorebi's own echoes are counted and not listed | 40 events listed |
//! | [`RegionBrief::findings`] | what looked wrong or unusual in or just before the region, ranked as in `overview` | 10 findings |
//! | [`RegionBrief::suspects`] | the events most worth tracing first | 5 suspects |
//! | [`RegionBrief::state_at_start`], [`RegionBrief::state_at_end`] | komorebi's state as the bug began and as the region ended, with anything wrong in it | |
//! | [`RegionBrief::changes`] | what differs between those two states | 40 changes, with the total in [`RegionBrief::changes_total`] |
//! | [`RegionBrief::incidents`] | what happened to the machine (a display unplugged, a sleep) within [`PRE_ROLL_NS`] of the region | |
//!
//! # How suspects are chosen
//!
//! Each event received in the region is scored, and the five with the highest scores are
//! returned, highest first and earliest first among equals. Echoes and komorebi's own synthetic
//! events ([`Class::Echo`], [`Class::Manual`]) are never suspects. An event with none of the
//! reasons below is not a suspect, whatever its class:
//!
//! | Reason | Score |
//! |---|---|
//! | it left the state wrong: a violation is present after it that was not before | +100 |
//! | its handler logged an error (or, failing that, a warning) | +50 (+25) |
//! | its handler was slow: at least 250 ms from its header to the event being received | +30 |
//! | it was received within a second of the start of the bug | +20 |
//! | it changed the state in a way a person would notice | +15 |
//!
//! Each event also starts from a base that depends on its class: 40 for a command, a monitor change
//! or a lock; 20 for a lifecycle event; 10 for a focus change; 5 for anything else. The scores are
//! a ranking aid, not a measure: they say which event to trace first, not how likely it is to be
//! the cause.
//!
//! Warnings about annotation lines that could not be read are not part of a brief; the
//! `annotations` command shows them.

use super::Analysis;
use super::answers::ChangeView;
use super::answers::EventQuery;
use super::catalog::Skew;
use super::classify::Class;
use super::diff;
use super::invariants;
use super::invariants::Violation;
use super::overview::Finding;
use super::overview::IncidentView;
use super::overview::PRE_ROLL_NS;
use super::overview::SessionInfo;
use super::state::Focus;
use super::views::EventView;
use super::views::Moment;
use super::views::StateOrigin;
use crate::annotations::Annotation;
use crate::annotations::AnnotationView;
use crate::annotations::Kind;
use color_eyre::eyre;
use schemars::JsonSchema;
use serde::Serialize;
use std::collections::BTreeMap;

/// The `schema` of the `brief` command's output: the name that [`BriefData`] has in an envelope.
pub const SCHEMA_BRIEF: &str = "brief/1";

/// How many non-echo events a brief lists for a region.
const LISTED_EVENTS: usize = 40;
const LISTED_CHANGES: usize = 40;
const LISTED_FINDINGS: usize = 10;
const LISTED_SUSPECTS: usize = 5;

/// komorebi's state at one moment of a region, in a line and with anything wrong in it.
#[derive(Debug, Clone, PartialEq, Serialize, JsonSchema)]
pub struct StateBrief {
    /// The moment the state is of.
    pub at: Moment,
    /// Which notification the state is from, and how old it was at `at`.
    pub origin: Option<StateOrigin>,
    /// How big the state is, in words: the number of monitors and windows.
    pub summary: String,
    /// What had the keyboard focus.
    pub focus: Option<Focus>,
    /// What looks wrong in the state.
    pub violations: Vec<Violation>,
}

/// An event worth tracing first, with why.
#[derive(Debug, Clone, PartialEq, Serialize, JsonSchema)]
pub struct Suspect {
    /// The event.
    pub event: EventView,
    /// A ranking score: higher is more worth tracing. It means something only compared with the
    /// other suspects of the same region.
    pub score: i64,
    /// Why this event is worth tracing.
    pub reasons: Vec<String>,
    /// The command that traces it: `komorebi-recorder trace <session> event:<seq>`.
    pub next: String,
}

/// The events received in a region, counted and the first of them listed.
#[derive(Debug, Clone, PartialEq, Serialize, JsonSchema)]
pub struct RegionEvents {
    /// How many events there were, echoes included.
    pub total: usize,
    /// How many events there were of each class, keyed by the class name in lowercase.
    pub by_class: BTreeMap<String, usize>,
    /// Echoes of komorebi's own moves: counted above, not listed.
    pub echoes_hidden: usize,
    /// The events that are not echoes, in the order received, up to a limit of 40.
    pub listed: Vec<EventView>,
    /// Whether there were more events that are not echoes than were listed. The `events` command,
    /// with `--from` and `--to`, pages through the rest.
    pub truncated: bool,
}

/// What the recording shows around one stretch of time: the events, what looked wrong, the events
/// to trace first, the state at either end and what changed between them.
#[derive(Debug, Clone, PartialEq, Serialize, JsonSchema)]
pub struct RegionBrief {
    /// The start of the region. For a bug region this is a few seconds before it starts, so causes
    /// just before it are included; for the whole recording, the start of the video.
    pub from: Moment,
    /// The end of the region.
    pub to: Moment,
    /// The events received between `from` and `to`.
    pub events: RegionEvents,
    /// What looked wrong or unusual in or just before the region, most relevant first, up to a
    /// limit of 10.
    pub findings: Vec<Finding>,
    /// The events most worth tracing first, best first, up to a limit of 5.
    pub suspects: Vec<Suspect>,
    /// komorebi's state as the region began; `None` if no state had been recorded by then.
    pub state_at_start: Option<StateBrief>,
    /// komorebi's state as the region ended; `None` if no state had been recorded by then.
    pub state_at_end: Option<StateBrief>,
    /// What differs between the state at the start and at the end, up to a limit of 40.
    pub changes: Vec<ChangeView>,
    /// How many differences there are in all, including any not listed in `changes`.
    pub changes_total: usize,
    /// What happened to the machine (a display unplugged, a sleep) in or near the region.
    pub incidents: Vec<IncidentView>,
}

/// One bug region the user marked, and what the recording shows around it.
#[derive(Debug, Clone, PartialEq, Serialize, JsonSchema)]
pub struct BugBrief {
    /// What the user said: the region, the description and what they expected.
    pub bug: AnnotationView,
    /// What the recording shows around the region.
    pub region: RegionBrief,
    /// Commands that dig further.
    pub commands: Vec<String>,
}

/// The first thing an investigator reads, made by [`Analysis::brief`].
#[derive(Debug, Clone, PartialEq, Serialize, JsonSchema)]
pub struct BriefData {
    /// The facts about the recording an investigator wants first.
    pub session: SessionInfo,
    /// Whether komorebi's source is the version that was recorded.
    pub skew: Skew,
    /// One brief for each bug region the user marked, in the order of their start. Empty if none
    /// was marked.
    pub bugs: Vec<BugBrief>,
    /// With no bug marked: the same digest over the whole recording.
    pub whole_session: Option<RegionBrief>,
    /// Conclusions an earlier investigation recorded.
    pub findings_recorded: Vec<AnnotationView>,
    /// The notes people or agents wrote.
    pub notes: Vec<AnnotationView>,
    /// What to do next, as commands to run and questions to ask.
    pub next: Vec<String>,
}

impl Analysis {
    fn state_brief(&self, mach_ns: u64) -> Option<StateBrief> {
        let found = self.state_at(mach_ns).ok()?;
        let data = self
            .state_answer(mach_ns, &super::answers::StateQuery::default())
            .ok()?;
        Some(StateBrief {
            at: data.at,
            origin: Some(data.origin),
            summary: super::views::headline(&found.snapshot),
            focus: data.focus,
            violations: data.violations,
        })
    }

    /// Score each event in `from..=to` by how likely it is to be where things went wrong.
    fn suspects(&self, from_ns: u64, to_ns: u64, bug_start_ns: u64) -> Vec<Suspect> {
        let events = self.events();
        let pairing = self.pairing();
        let mut ranked = Vec::new();

        for (index, event) in events.iter().enumerate() {
            if event.recv_mach_ns < from_ns || event.recv_mach_ns > to_ns {
                continue;
            }

            let class = pairing.classes[index].class;
            if matches!(class, Class::Echo | Class::Manual) {
                continue;
            }

            let mut score = 0_i64;
            let mut reasons = Vec::new();

            score += match class {
                Class::Command | Class::Monitor | Class::Lock => 40,
                Class::Lifecycle => 20,
                Class::Focus => 10,
                _ => 5,
            };

            // did it leave the state wrong?
            if let (Some(this), Some(before)) = (
                event.state_id,
                index
                    .checked_sub(1)
                    .and_then(|before| events[before].state_id),
            ) && this != before
                && let (Ok(after), Ok(previous)) = (self.snapshot(this), self.snapshot(before))
            {
                let (now, was) = (invariants::check(&after), invariants::check(&previous));
                let appeared: Vec<&Violation> = now.iter().filter(|v| !was.contains(v)).collect();
                if !appeared.is_empty() {
                    score += 100;
                    reasons.push(format!(
                        "left the state wrong: {}",
                        appeared
                            .iter()
                            .map(|violation| violation.kind.as_str())
                            .collect::<Vec<_>>()
                            .join(", ")
                    ));
                }
                if diff::diff(&previous, &after).is_empty() {
                    // the state was rewritten but nothing a person would notice changed
                } else {
                    score += 15;
                    reasons.push("changed the state".to_string());
                }
            }

            if let Some(handler) = &pairing
                .pairs
                .get(index)
                .and_then(|pair| pair.block)
                .and_then(|id| self.log().blocks.get(id))
            {
                if handler.errors > 0 {
                    score += 50;
                    reasons.push(format!("its handler logged {} error(s)", handler.errors));
                } else if handler.warnings > 0 {
                    score += 25;
                    reasons.push(format!(
                        "its handler logged {} warning(s)",
                        handler.warnings
                    ));
                }
            }
            if pairing.pairs[index]
                .handler_ms
                .is_some_and(|ms| ms >= 250.0)
            {
                score += 30;
                reasons.push("its handler was slow".to_string());
            }

            // what happens as the bug begins matters most
            let near_start = event.recv_mach_ns.abs_diff(bug_start_ns) <= 1_000_000_000;
            if near_start {
                score += 20;
                reasons.push("right at the start of the region".to_string());
            }

            if reasons.is_empty() {
                continue;
            }

            if let Some(view) = self.event_view(index) {
                ranked.push(Suspect {
                    next: format!(
                        "komorebi-recorder trace {} event:{}",
                        self.session.dir.display(),
                        event.seq
                    ),
                    event: view,
                    score,
                    reasons,
                });
            }
        }

        ranked.sort_by(|a, b| b.score.cmp(&a.score).then(a.event.seq.cmp(&b.event.seq)));
        ranked.truncate(LISTED_SUSPECTS);
        ranked
    }

    fn region_brief(
        &self,
        from_ns: u64,
        to_ns: u64,
        bug_start_ns: u64,
        findings: &[Finding],
    ) -> eyre::Result<RegionBrief> {
        let answer = self.events_answer(&EventQuery {
            from_mach_ns: Some(from_ns),
            to_mach_ns: Some(to_ns),
            ..EventQuery::default()
        });
        let mut by_class: BTreeMap<String, usize> = BTreeMap::new();
        for view in &answer.data.events {
            *by_class
                .entry(format!("{:?}", view.class).to_lowercase())
                .or_default() += 1;
        }
        let echoes = answer
            .data
            .events
            .iter()
            .filter(|view| view.class == Class::Echo)
            .count();
        let real: Vec<EventView> = answer
            .data
            .events
            .into_iter()
            .filter(|view| view.class != Class::Echo)
            .collect();
        let truncated = real.len() > LISTED_EVENTS;

        let start = self.state_brief(from_ns.max(bug_start_ns.min(to_ns)));
        let end = self.state_brief(to_ns);
        let changes = match (self.state_at(bug_start_ns.min(to_ns)), self.state_at(to_ns)) {
            (Ok(before), Ok(after)) => diff::diff(&before.snapshot, &after.snapshot),
            _ => Vec::new(),
        };
        let changes_total = changes.len();

        let incidents: Vec<IncidentView> = self
            .incidents()
            .into_iter()
            .filter(|incident| {
                let at = self.manifest.mach_at_video_seconds(incident.video_s);
                at + PRE_ROLL_NS >= from_ns && at <= to_ns + PRE_ROLL_NS
            })
            .collect();

        Ok(RegionBrief {
            from: self.moment(from_ns),
            to: self.moment(to_ns),
            events: RegionEvents {
                total: real.len() + echoes,
                by_class,
                echoes_hidden: echoes,
                listed: real.into_iter().take(LISTED_EVENTS).collect(),
                truncated,
            },
            findings: findings
                .iter()
                .filter(|finding| finding.end_mach_ns >= from_ns && finding.start_mach_ns <= to_ns)
                .take(LISTED_FINDINGS)
                .cloned()
                .collect(),
            suspects: self.suspects(from_ns, to_ns, bug_start_ns),
            state_at_start: start,
            state_at_end: end,
            changes: changes
                .into_iter()
                .take(LISTED_CHANGES)
                .map(|change| ChangeView {
                    text: change.describe(),
                    change,
                })
                .collect(),
            changes_total,
            incidents,
        })
    }

    /// Returns the brief of the session: one entry per bug region the user marked, or, if none, a
    /// digest of the whole recording.
    ///
    /// See the [module docs](self) for what a region covers and how suspects are chosen. A bug's
    /// region, description and expectation are returned as written, and the rest is evidence drawn
    /// from the recording. A part of the brief that cannot be made (the state at a moment before any
    /// state was recorded) is `None` or empty, and does not fail the brief.
    ///
    /// # Errors
    ///
    /// Returns an error if `annotations.jsonl` exists but cannot be read, or if one of the states
    /// the recording refers to is missing or is not valid JSON (the search for what looked wrong
    /// reads them all).
    ///
    /// # Example
    ///
    /// A user marked a region after a display went away. A focus change at 5.2 s left a window
    /// parked off screen, and that is the first thing to trace:
    ///
    /// ```rust
    /// use komorebi_recorder::analysis::Analysis;
    /// use komorebi_recorder::annotations::{Kind, New, add};
    /// use komorebi_recorder::testkit::{
    ///     SessionBuilder, container, mach_ns, monitor, state, window, workspace,
    /// };
    /// use serde_json::json;
    ///
    /// let layout = [[0, 0, 500, 800], [500, 0, 500, 800]];
    /// // two tiled windows; the second is where the layout says, or parked off screen
    /// let screen = |second: [i64; 4]| {
    ///     state(
    ///         vec![monitor(
    ///             1,
    ///             [0, 0, 1000, 800],
    ///             vec![workspace(
    ///                 "one",
    ///                 vec![
    ///                     container("a", vec![window(1, "Ghostty", Some(layout[0]))]),
    ///                     container("b", vec![window(2, "Brave", Some(second))]),
    ///                 ],
    ///                 1,
    ///                 &layout,
    ///             )],
    ///             0,
    ///         )],
    ///         0,
    ///     )
    /// };
    /// let focus = |what: &str| {
    ///     json!({"type": "FocusChange", "content": [{"type": "Accessibility", "content": what}, 11, 2]})
    /// };
    ///
    /// let (session, _) = SessionBuilder::new("doc-brief")
    ///     .event(2.0, json!({"type": "Retile"}), Some(screen(layout[1])))
    ///     .event(5.2, focus("AXApplicationActivated"), Some(screen([-499, 790, 500, 800])))
    ///     .incident(4.9, "display-disconnected", "display 3 went away")
    ///     .build();
    /// add(&session.dir, New {
    ///     kind: Some(Kind::Bug),
    ///     start_mach_ns: mach_ns(4.5),
    ///     end_mach_ns: Some(mach_ns(7.0)),
    ///     text: "my Brave window is gone from the screen".to_string(),
    ///     ..New::default()
    /// })?;
    ///
    /// let brief = Analysis::open(&session.dir)?.brief()?;
    ///
    /// // the user's words come first, verbatim
    /// assert_eq!(brief.bugs[0].bug.text, "my Brave window is gone from the screen");
    ///
    /// // the focus change left the state wrong, so it is the first suspect, and says why
    /// let suspect = &brief.bugs[0].region.suspects[0];
    /// assert_eq!(suspect.event.seq, 2);
    /// assert!(suspect.reasons[0].starts_with("left the state wrong"));
    ///
    /// // as the bug began the state was fine; as it ended it was not
    /// let region = &brief.bugs[0].region;
    /// assert!(region.state_at_start.as_ref().is_some_and(|s| s.violations.is_empty()));
    /// assert!(region.state_at_end.as_ref().is_some_and(|s| !s.violations.is_empty()));
    /// assert_eq!(region.incidents[0].kind, "display-disconnected");
    /// # Ok::<(), Box<dyn std::error::Error>>(())
    /// ```
    ///
    /// Covered, through the command, by the tests in `tests/brief.rs`, such as
    /// `the_brief_leads_with_the_users_words_and_points_at_the_event_that_broke_it`.
    pub fn brief(&self) -> eyre::Result<BriefData> {
        let loaded = crate::annotations::load(&self.session.dir)?;
        let bugs: Vec<&Annotation> = loaded.bugs().collect();
        let all = self.findings()?;
        let dir = self.session.dir.display().to_string();

        let mut briefs = Vec::new();
        for bug in &bugs {
            let end = bug.end_or_session_end(&self.manifest);
            let from = bug.start_mach_ns.saturating_sub(PRE_ROLL_NS / 2);
            let ranked = self.rank(all.clone(), &[bug]);
            let region = self.region_brief(from, end, bug.start_mach_ns, &ranked)?;

            let mut commands = vec![
                format!(
                    "komorebi-recorder events {dir} --from bug:{id} --to bug:{id}.end --hide-echo",
                    id = bug.id
                ),
                format!(
                    "komorebi-recorder diff {dir} bug:{id} bug:{id}.end",
                    id = bug.id
                ),
            ];
            if let Some(suspect) = region.suspects.first() {
                commands.insert(0, suspect.next.clone());
                commands.push(format!(
                    "komorebi-recorder code {dir} event:{}",
                    suspect.event.seq
                ));
            }
            if !self.manifest.displays.is_empty() {
                commands.push(format!("komorebi-recorder sheet {dir} --bug {}", bug.id));
                commands.push(format!("komorebi-recorder changes {dir} --bug {}", bug.id));
            }
            for window in region
                .findings
                .iter()
                .flat_map(|finding| finding.window_ids.iter())
                .take(1)
            {
                commands.push(format!("komorebi-recorder window {dir} {window}"));
            }
            commands.push(format!(
                "komorebi-recorder annotate {dir} add --kind finding --about {} --text '<cause>' --ref event:<n> --ref code:<path>:<line>",
                bug.id
            ));

            briefs.push(BugBrief {
                bug: bug.view(&self.manifest),
                region,
                commands,
            });
        }

        let whole_session = if bugs.is_empty() {
            let end = crate::analysis::timespec::session_end_ns(&self.manifest);
            let start = self.manifest.video_t0();
            let ranked = self.rank(all, &[]);
            Some(self.region_brief(start, end, start, &ranked)?)
        } else {
            None
        };

        let mut next = Vec::new();
        if bugs.is_empty() {
            next.push(format!(
                "no bug region was marked: ask the user when it happened and what it looked like, \
                 then `komorebi-recorder annotate {dir} add --kind bug --start <t> --end <t> --text '<what is wrong>'`"
            ));
        }
        next.push(format!("komorebi-recorder overview {dir}"));
        next.push("komorebi-recorder guide".to_string());

        let recorded: Vec<AnnotationView> = loaded
            .annotations
            .iter()
            .filter(|annotation| annotation.kind == Kind::Finding)
            .map(|annotation| annotation.view(&self.manifest))
            .collect();
        let notes: Vec<AnnotationView> = loaded
            .annotations
            .iter()
            .filter(|annotation| annotation.kind == Kind::Note)
            .map(|annotation| annotation.view(&self.manifest))
            .collect();

        Ok(BriefData {
            session: self.session_info(),
            skew: self.skew(),
            bugs: briefs,
            whole_session,
            findings_recorded: recorded,
            notes,
            next,
        })
    }
}
