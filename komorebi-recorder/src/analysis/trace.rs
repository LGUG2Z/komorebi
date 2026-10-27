//! One event, end to end: what ran, what it logged, what it changed and who changed it.
//!
//! komorebi sends the state *after* each notification, so what an event changed is the difference
//! between its state and the previous event's. That difference is not all the handler's doing:
//! background threads (the reaper, the workspace reconciliator) change state with no notification
//! of their own, and their changes show up in whichever event comes next. So every change is given
//! an **attribution** with the evidence for it and how sure it is, and a change nothing explains is
//! said to be unexplained rather than blamed on the handler.
//!
//! [`Analysis::trace`] builds a [`TraceData`] for one event: what its handler logged (each line
//! tied to the statement of komorebi's source that wrote it), what other threads logged in the
//! interval, what changed with each change attributed, and which of the state's violations
//! (see [`super::invariants`]) the event introduced or cleared. It is what the `trace` command
//! prints, and what the viewer's inspector shows for an event.
//!
//! # How a change is attributed
//!
//! The rules are tried in this order for each change, and the first that applies decides:
//!
//! | the change | the evidence | `actor` | `confidence` |
//! |---|---|---|---|
//! | a window went away | `komorebi::reaper` logged `reaping window: <that window's id>` in the interval | `reaper` | `high` |
//! | a window moved, a workspace came into focus, containers were reordered, or a rect really changed | `komorebi::workspace_reconciliator` logged a line containing `reconciliat` in the interval | `workspace-reconciliator` | `low` if the handler could also have made the change (below), else `medium` |
//! | one the event's handler is expected to make | none needed | `handler` | `high` if the handler's lines were found in the log, else `medium` |
//! | anything else | none: no log line accounts for it | `unexplained` | `low` |
//!
//! A handler is expected to make any change if the event is a command, a monitor change or could
//! not be read, and for a window-manager event it depends on the kind: a `FocusChange` is expected
//! to change focus, a workspace's focus, move a window, change a rect or reorder containers; a
//! `MoveStart`, `MoveEnd`, `ResizeStart` or `ResizeEnd` to change a rect, move a window, reorder
//! containers or change focus; every other window-manager event (windows shown, hidden or
//! closed reshuffle everything around them) to make any change.
//!
//! The attribution is a judgement from the log, and gives what it rests on in `evidence` and
//! `reason`. `unexplained` is a finding in itself: komorebi changed state with no notification
//! and nothing in the log accounts for it, or its log level does not show the cause.
//!
//! # Flavor
//!
//! The first two rules look for log lines of komorebi for Mac: its reaper logs `reaping window:
//! <id>` and it has a workspace reconciliator. komorebi for Windows does neither (its source has no
//! workspace reconciliator and no such reaper line), so for a Windows session a change is
//! attributed to the `handler` or is `unexplained`. Everything else here is the same for both:
//! the roles of the log lines ([`Rules::role`](super::flavor::Rules::role)), the explanation of a
//! slow handler ([`Rules::slow_handler_cause`](super::flavor::Rules::slow_handler_cause)) and
//! where a window is hidden are the rules of the flavor that recorded the session.

use super::Analysis;
use super::answers::ChangeView;
use super::catalog;
use super::catalog::Confidence;
use super::diff;
use super::diff::Change;
use super::envelope;
use super::invariants;
use super::invariants::Violation;
use super::logparse::Line;
use super::timespec::Edge;
use super::timespec::Lookup;
use super::views::EventView;
use color_eyre::eyre;
use schemars::JsonSchema;
use serde::Serialize;

/// The schema name that identifies a [`TraceData`] in an envelope (`trace/1`); listed by the
/// `schema` command.
pub const SCHEMA_TRACE: &str = "trace/1";

/// Where in komorebi's source a log line came from: the statement that wrote it, reduced to what a
/// reader of a trace needs (see `Source` in [`super::catalog`] for the whole match).
#[derive(Debug, Clone, PartialEq, Serialize, JsonSchema)]
pub struct SourceRef {
    /// `file:line` of the statement in komorebi's source, as an editor opens it.
    pub location: String,
    /// The function the statement is in, with its type if it is in an `impl`:
    /// `WindowManager::process_event`.
    pub function: String,
    /// How much the text of the line pins the statement down. An `ambiguous` or `generic` match
    /// is not a place to pin a cause on.
    pub confidence: Confidence,
    /// The file differs between the recorded komorebi and the indexed source, so the statement
    /// may not be the one that ran.
    pub file_changed: bool,
}

/// A line of komorebi's log, with where it came from in the source.
#[derive(Debug, Clone, PartialEq, Serialize, JsonSchema)]
pub struct TraceLine {
    /// The line of the session's `komorebi.log`, counting from 1.
    pub line: u64,
    /// When the line was logged, in seconds of video time; `None` if it has no time or it cannot
    /// be placed on the session's clock.
    pub video_s: Option<f64>,
    /// The level in capitals (`INFO`); `None` for a line that is not a log record.
    pub level: Option<String>,
    /// The span chain, e.g. `process_event:focus_window`. Empty for a line from a thread that
    /// is not in a handler.
    pub spans: String,
    /// The module the line was logged from, such as `komorebi::reaper`.
    pub target: Option<String>,
    /// The message, without ANSI escapes.
    pub message: String,
    /// The statement that wrote it, if one matched.
    pub source: Option<SourceRef>,
    /// The part of komorebi that logged it changes state without sending a notification, so its
    /// lines are candidates for a change with no event before it.
    pub changes_state_silently: bool,
}

/// Who made a change, and what that is based on.
///
/// This is a judgement from the log, not a fact: see the [module](self) documentation for the
/// rules.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, JsonSchema)]
pub struct Attribution {
    /// `handler`, `reaper`, `workspace-reconciliator` or `unexplained`.
    pub actor: String,
    /// `high`, `medium` or `low`.
    pub confidence: String,
    /// The line numbers of `komorebi.log` that support it: the log lines of the reaper or the
    /// reconciliator, or the handler's header. Empty for `unexplained`, and for a handler whose
    /// lines were not found.
    pub evidence: Vec<u64>,
    /// The attribution in a sentence.
    pub reason: String,
}

/// A change an event made, with who made it.
///
/// In JSON it is the fields of the change (a `ChangeView`: `text` and the change's fields,
/// including its `kind`) with an `attribution` beside them.
#[derive(Debug, Clone, PartialEq, Serialize, JsonSchema)]
pub struct TracedChange {
    /// The change, with a line of text that describes it.
    #[serde(flatten)]
    pub change: ChangeView,
    /// Who made it.
    pub attribution: Attribution,
}

/// One event end to end: the `data` of the `trace` command.
#[derive(Debug, Clone, PartialEq, Serialize, JsonSchema)]
pub struct TraceData {
    /// The event, with its class and the handler that was paired with it.
    pub event: EventView,
    /// The lines the handler logged, in order; empty if the log has no block for this event.
    pub handler: Vec<TraceLine>,
    /// Lines from other threads logged between the previous notification and this one: what else
    /// was going on. For the first event that is everything logged before it. Lines that belong to
    /// any handler, and continuation lines such as panic backtraces, are not included.
    pub ambient: Vec<TraceLine>,
    /// The changes the event made, each with its attribution: the difference between the state when
    /// its handler began (the state the previous event left) and the state it carried. Empty, with
    /// a note, if the state before or after is unknown.
    pub changes: Vec<TracedChange>,
    /// Violations (see [`super::invariants`]) that are in the state after the event and were not in
    /// the state before it. A violation is the same one only if its message is the same.
    pub violations_appeared: Vec<Violation>,
    /// Violations that were in the state before the event and are not in the state after it.
    pub violations_resolved: Vec<Violation>,
    /// Things worth knowing that are not data: a slow handler, errors, a missing log.
    pub notes: Vec<String>,
    /// The OS trace around this event, what the rest of the system did (an `OsTraceData`, from
    /// `Analysis::os_trace`): only when asked for (`trace --os`), as the OS trace can be large.
    /// [`Analysis::trace`] always leaves it `None`, and the caller fills it in.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub os_trace: Option<super::os_trace::OsTraceData>,
    /// Windows' account of the event's window just after it, checked against komorebi's state:
    /// only for a recording made with `--probe-windows`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub window_probe: Option<ProbeView>,
}

/// A window as Windows reported it, and the problems that makes in komorebi's state.
///
/// Only a session recorded with `record --probe-windows` has these, and only for events about a
/// window. See [`check_probed`](super::invariants::check_probed) for what is compared.
#[derive(Debug, Clone, PartialEq, Serialize, JsonSchema)]
pub struct ProbeView {
    /// Windows' account of the window (a `WindowFacts` from `crate::sources::winprobe`).
    pub facts: crate::sources::winprobe::WindowFacts,
    /// How long after the event it was read, in milliseconds: the longer, the less it shows. `0` if
    /// it was read before the event was received.
    pub read_after_ms: f64,
    /// The problems the facts show in komorebi's state, if any: empty when they agree.
    pub violations: Vec<Violation>,
}

fn spans_of(line: &Line) -> String {
    line.spans
        .iter()
        .map(|span| span.name.as_str())
        .collect::<Vec<_>>()
        .join(":")
}

/// Whether a change is the kind of thing this event's handler is expected to do.
fn expected_of_handler(event: &EventView, change: &Change) -> bool {
    use crate::analysis::events::Family;
    match event.info.family {
        // commands, monitor changes and anything unreadable can do anything
        Family::Command | Family::Monitor | Family::Unknown => true,
        Family::WindowManager => match event.info.name.as_str() {
            "FocusChange" => matches!(
                change,
                Change::Focus { .. }
                    | Change::WorkspaceFocus { .. }
                    | Change::WindowMoved { .. }
                    | Change::WindowRect { .. }
                    | Change::ContainerOrder { .. }
            ),
            "MoveStart" | "MoveEnd" | "ResizeStart" | "ResizeEnd" => matches!(
                change,
                Change::WindowRect { .. }
                    | Change::WindowMoved { .. }
                    | Change::ContainerOrder { .. }
                    | Change::Focus { .. }
            ),
            // showing, hiding and closing windows reshuffles everything around them
            _ => true,
        },
    }
}

impl Analysis {
    fn trace_line(&self, line: &Line, skew: &catalog::Skew) -> TraceLine {
        let source = match (&line.target, line.level) {
            (Some(target), Some(level)) => self.catalog().find(target, level, &line.message),
            _ => None,
        };
        let role = line
            .target
            .as_deref()
            .map(|target| self.flavor().rules().role(target));
        TraceLine {
            line: line.number,
            video_s: line
                .wall_us
                .and_then(|wall_us| self.clock().wall_to_mach(wall_us))
                .map(|mach_ns| self.video_s(mach_ns)),
            level: line.level.map(|level| format!("{level:?}").to_uppercase()),
            spans: spans_of(line),
            target: line.target.clone(),
            message: line.message.clone(),
            source: source.map(|source| SourceRef {
                location: source.statement.location(),
                function: source.statement.qualified_function(),
                confidence: source.confidence,
                file_changed: skew.file_changed(&source.statement.file),
            }),
            changes_state_silently: role.is_some_and(|role| role.changes_state_silently),
        }
    }

    /// Traces event `seq`: what its handler logged, what else was logged, what it changed and who
    /// made each change.
    ///
    /// The changes are the difference between the state at the moment the handler began and the
    /// state the event carried, so anything background threads did since the previous
    /// notification is in them too, attributed by [the rules](self#how-a-change-is-attributed).
    /// The handler's lines are those komorebi's log has for the event, if any. The notes say
    /// what is missing or worth knowing: no handler lines in the log, a handler that took 100 ms or
    /// more from its header to the notification (with the flavor's likely cause), errors and
    /// warnings the handler logged, and, for an event the analysis classified as an echo of
    /// komorebi's own move, why.
    ///
    /// `TraceData::os_trace` is `None` in the result: ask `Analysis::os_trace` for it and set
    /// it, as `trace --os` does.
    ///
    /// # Errors
    ///
    /// Returns an error if:
    ///
    /// - there is no event numbered `seq` (the not-found exit kind);
    /// - the event carried a state, it is not the first event, and no state was recorded before it
    ///   (not found);
    /// - a state file is missing (not found) or is not valid JSON.
    ///
    /// An event that carried no state, and the first event of a session that has no starting
    /// state, are not errors: they have no `changes` and a note saying why.
    ///
    /// # Example
    ///
    /// ```rust
    /// use komorebi_recorder::analysis::Analysis;
    /// use komorebi_recorder::testkit::SessionBuilder;
    /// use komorebi_recorder::testkit::state_with_windows;
    /// use serde_json::json;
    ///
    /// let notification = |name: &str, what: &str| {
    ///     json!({"type": name, "content": [{"type": "Accessibility", "content": what}, 7, 1]})
    /// };
    ///
    /// // Two windows; then Brave is closed (the reaper notices, and sends no notification of its
    /// // own) and the next focus change reports the result.
    /// let (session, _) = SessionBuilder::new("doc-trace")
    ///     .event(
    ///         1.0,
    ///         notification("Show", "AXWindowCreated"),
    ///         Some(state_with_windows(&[(1, "Ghostty"), (2, "Brave")])),
    ///     )
    ///     .event(
    ///         4.0,
    ///         notification("FocusChange", "AXApplicationActivated"),
    ///         Some(state_with_windows(&[(1, "Ghostty")])),
    ///     )
    ///     .log(3.5, "INFO", "komorebi::reaper: reaping window: 2")
    ///     .log(
    ///         3.9,
    ///         "INFO",
    ///         "process_event: komorebi::process_event: processing event: FocusChange for process 7 with notification AXApplicationActivated",
    ///     )
    ///     .build();
    /// let analysis = Analysis::open(&session.dir)?;
    ///
    /// let trace = analysis.trace(2)?;
    /// assert_eq!(trace.handler.len(), 1);
    /// // the reaper's line is from another thread, so it is ambient
    /// assert_eq!(trace.ambient[0].message, "reaping window: 2");
    ///
    /// // window 2 vanished with no notification of its own, and the reaper's line says why
    /// let removed = trace
    ///     .changes
    ///     .iter()
    ///     .find(|traced| traced.change.change.kind() == "window-removed")
    ///     .expect("a window was removed");
    /// assert_eq!(removed.attribution.actor, "reaper");
    /// assert_eq!(removed.attribution.confidence, "high");
    /// assert_eq!(removed.attribution.evidence, [1]);
    ///
    /// // an event that does not exist is an error
    /// assert!(analysis.trace(99).is_err());
    /// # Ok::<(), Box<dyn std::error::Error>>(())
    /// ```
    ///
    /// Also covered by
    /// `tests/trace.rs::a_change_with_no_event_of_its_own_is_blamed_on_the_reaper_by_its_log_line`
    /// and `tests/trace.rs::an_event_with_no_log_says_so_instead_of_inventing_a_handler`, and, for
    /// a Windows session, by
    /// `tests/windows_flavor.rs::the_state_is_read_by_window_handle_and_a_trace_says_what_moved`.
    pub fn trace(&self, seq: u64) -> eyre::Result<TraceData> {
        let index = self
            .event_index(seq)
            .ok_or_else(|| envelope::not_found(format!("there is no event #{seq}")))?;
        let view = self
            .event_view(index)
            .ok_or_else(|| envelope::not_found(format!("there is no event #{seq}")))?;
        let event = &self.events()[index];
        let log = self.log();
        let pair = &self.pairing().pairs[index];
        let block = pair.block.and_then(|id| log.blocks.get(id));

        let skew = self.skew();
        let handler: Vec<TraceLine> = block
            .map(|block| {
                block
                    .lines
                    .iter()
                    .filter_map(|number| log.lines.get(usize::try_from(number - 1).ok()?))
                    .map(|line| self.trace_line(line, &skew))
                    .collect()
            })
            .unwrap_or_default();

        // lines from other threads between the previous notification and this one
        let previous_recv = index
            .checked_sub(1)
            .map_or(0, |before| self.events()[before].recv_mach_ns);
        let handler_lines: std::collections::HashSet<u64> = log
            .blocks
            .iter()
            .flat_map(|block| block.lines.iter().copied())
            .collect();
        let ambient_lines: Vec<&Line> = log
            .lines
            .iter()
            .filter(|line| !line.continuation && !handler_lines.contains(&line.number))
            .filter(|line| {
                line.wall_us
                    .and_then(|wall_us| self.clock().wall_to_mach(wall_us))
                    .is_some_and(|mach_ns| mach_ns > previous_recv && mach_ns <= event.recv_mach_ns)
            })
            .collect();

        // what changed, and who did it
        let start = self.event_mach_ns(seq, Edge::Start)?;
        let (changes, appeared, resolved, mut notes) =
            if index == 0 && self.state_at(start).is_err() {
                (
                    Vec::new(),
                    Vec::new(),
                    Vec::new(),
                    vec![
                        "no state was recorded before this event, so what it changed is unknown"
                            .to_string(),
                    ],
                )
            } else if event.state_id.is_none() {
                (
                    Vec::new(),
                    Vec::new(),
                    Vec::new(),
                    vec!["this notification carried no state".to_string()],
                )
            } else {
                let before = self.state_at(start)?;
                let after = self.state_at(event.recv_mach_ns)?;
                let raw = diff::diff(&before.snapshot, &after.snapshot);
                let (was, now) = (
                    invariants::check(&before.snapshot),
                    invariants::check(&after.snapshot),
                );
                let traced = raw
                    .into_iter()
                    .map(|change| {
                        let attribution = self.attribute(&view, &change, &ambient_lines);
                        TracedChange {
                            change: ChangeView {
                                text: change.describe(),
                                change,
                            },
                            attribution,
                        }
                    })
                    .collect();
                (
                    traced,
                    now.iter().filter(|v| !was.contains(v)).cloned().collect(),
                    was.iter().filter(|v| !now.contains(v)).cloned().collect(),
                    Vec::new(),
                )
            };

        if let Some(block) = block {
            if let Some(ms) = pair.handler_ms
                && ms >= 100.0
            {
                notes.push(format!(
                    "the handler took at least {ms:.0} ms from its header to the notification: {}",
                    self.flavor().rules().slow_handler_cause()
                ));
            }
            if block.errors + block.warnings > 0 {
                notes.push(format!(
                    "the handler logged {} error(s) and {} warning(s)",
                    block.errors, block.warnings
                ));
            }
        } else {
            notes.push(
                "no handler lines for this event were found in komorebi's log: it may not have \
                 been recorded, or komorebi may log this kind of event only at a more verbose level"
                    .to_string(),
            );
        }
        if view.class == super::classify::Class::Echo {
            notes.push(view.reason.clone());
        }

        Ok(TraceData {
            event: view,
            handler,
            ambient: ambient_lines
                .iter()
                .map(|line| self.trace_line(line, &skew))
                .collect(),
            changes,
            violations_appeared: appeared,
            violations_resolved: resolved,
            notes,
            os_trace: None,
            window_probe: self.window_probe(index),
        })
    }

    /// What Windows said about event `index`'s window, if it was asked, checked against the state
    /// komorebi reported with the event.
    fn window_probe(&self, index: usize) -> Option<ProbeView> {
        let event = self.events().get(index)?;
        let facts = event.probe.clone()?;
        let violations = event
            .state_id
            .and_then(|state_id| self.snapshot(state_id).ok())
            .map(|snapshot| invariants::check_probed(&snapshot, &facts, event.info.exe.as_deref()))
            .unwrap_or_default();

        Some(ProbeView {
            read_after_ms: facts.at_ns.saturating_sub(event.recv_mach_ns) as f64 / 1e6,
            facts,
            violations,
        })
    }

    /// Say who made `change`, from what the log shows happened around it.
    fn attribute(&self, event: &EventView, change: &Change, ambient: &[&Line]) -> Attribution {
        let lines_of = |target: &str, needle: &str| -> Vec<u64> {
            ambient
                .iter()
                .filter(|line| {
                    line.target.as_deref() == Some(target) && line.message.contains(needle)
                })
                .map(|line| line.number)
                .collect()
        };

        // a window removed that the reaper logged reaping, by id
        if let Change::WindowRemoved { window, .. } = change {
            let evidence = lines_of(
                "komorebi::reaper",
                &format!("reaping window: {}", window.id),
            );
            if !evidence.is_empty() {
                return Attribution {
                    actor: "reaper".to_string(),
                    confidence: "high".to_string(),
                    evidence,
                    reason: format!(
                        "the reaper logged reaping window {}: it removes windows that no longer \
                         exist, without sending a notification",
                        window.id
                    ),
                };
            }
        }

        let reconciling = lines_of("komorebi::workspace_reconciliator", "reconciliat");
        let moves = matches!(
            change,
            Change::WindowMoved { .. }
                | Change::WorkspaceFocus { .. }
                | Change::ContainerOrder { .. }
                | Change::WindowRect { .. }
        );
        let by_handler = expected_of_handler(event, change);

        if moves && !reconciling.is_empty() {
            return Attribution {
                actor: "workspace-reconciliator".to_string(),
                confidence: if by_handler { "low" } else { "medium" }.to_string(),
                evidence: reconciling,
                reason: if by_handler {
                    "the workspace reconciliator ran in this interval and could have done this, \
                     but so could the handler"
                        .to_string()
                } else {
                    "the workspace reconciliator ran in this interval, and this is not what this \
                     event's handler does"
                        .to_string()
                },
            };
        }

        if by_handler {
            let quality = event.handler.as_ref().map(|handler| handler.quality);
            return Attribution {
                actor: "handler".to_string(),
                confidence: if quality.is_some() { "high" } else { "medium" }.to_string(),
                evidence: event
                    .handler
                    .as_ref()
                    .map(|handler| vec![handler.header_line])
                    .unwrap_or_default(),
                reason: format!(
                    "{} is the kind of change {} makes",
                    change.kind(),
                    event.info.name
                ),
            };
        }

        Attribution {
            actor: "unexplained".to_string(),
            confidence: "low".to_string(),
            evidence: Vec::new(),
            reason: format!(
                "{} is not something {} does, and no background activity in the log accounts for \
                 it: komorebi changed state with no notification, or the log is at a level that \
                 does not show it",
                change.kind(),
                event.info.name
            ),
        }
    }
}
