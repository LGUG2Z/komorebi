//! The answers to "what was the state", "what changed" and "what happened", as data.
//!
//! Built here, from an [`Analysis`], so that the commands print them and the viewer draws the
//! same thing. The data types are plain values that serialize: `komorebi-recorder` prints one as
//! text ([`crate::report`]) or, with `--json`, inside an envelope whose `schema` names the type.
//!
//! | Question | Asked with | Answered by | Data | Schema | Command |
//! |---|---|---|---|---|---|
//! | What was the state at a moment? | [`StateQuery`] | [`Analysis::state_answer`] | [`StateData`] | [`SCHEMA_STATE`] | `state` |
//! | What changed between two moments? | two times | [`Analysis::diff_answer`] | [`DiffData`] | [`SCHEMA_DIFF`] | `diff` |
//! | What happened? | [`EventQuery`] | [`Analysis::events_answer`] | [`EventsAnswer`], with [`EventsData`] | [`SCHEMA_EVENTS`] | `events` |
//!
//! The types at the end ([`AnnotationData`], [`AnnotationsData`], [`DeletedData`]) are what the
//! `annotate` and `annotations` commands print with `--json`.
//!
//! Times are nanoseconds on the host's master monotonic clock (`*_mach_ns`), which
//! [`Analysis::resolve`] gives for any time the user can type.
//!
//! # Example
//!
//! ```rust
//! use komorebi_recorder::analysis::Analysis;
//! use komorebi_recorder::analysis::answers::{EventQuery, StateQuery};
//! use komorebi_recorder::testkit::{SessionBuilder, mach_ns, state_with_windows};
//! use serde_json::json;
//!
//! let (session, _) = SessionBuilder::new("doc-answers")
//!     .event(2.0, json!({"type": "Retile"}), Some(state_with_windows(&[(7, "Terminal")])))
//!     .event(4.0, json!({"type": "Retile"}), Some(state_with_windows(&[(7, "Terminal"), (8, "Browser")])))
//!     .build();
//! let analysis = Analysis::open(&session.dir)?;
//!
//! // what was the state at 3 s: one window
//! let before = analysis.state_answer(mach_ns(3.0), &StateQuery::default())?;
//! assert_eq!(before.workspaces[0].windows, 1);
//! assert_eq!(before.origin.seq, Some(1));
//!
//! // what changed by 5 s: the browser appeared and the terminal made room
//! let diff = analysis.diff_answer(mach_ns(3.0), mach_ns(5.0))?;
//! assert!(diff.changes.iter().any(|change| change.text.contains("Browser") && change.text.contains("appeared")));
//!
//! // what happened: both events, or one at a time
//! let all = analysis.events_answer(&EventQuery::default());
//! assert_eq!((all.data.matching, all.data.total), (2, 2));
//! let first = analysis.events_answer(&EventQuery { limit: Some(1), ..EventQuery::default() });
//! assert_eq!(first.next_cursor.as_deref(), Some("1"));
//! # Ok::<(), Box<dyn std::error::Error>>(())
//! ```

use super::Analysis;
use super::classify::Class;
use super::diff;
use super::diff::Change;
use super::envelope;
use super::facade::StateAt;
use super::facade::StateSource;
use super::invariants;
use super::invariants::Violation;
use super::state::Focus;
use super::state::Location;
use super::state::Monitor;
use super::state::Place;
use super::state::Rect;
use super::state::Snapshot;
use super::state::Window;
use super::views::EventView;
use super::views::Moment;
use super::views::StateOrigin;
use crate::annotations::AnnotationView;
use color_eyre::eyre;
use schemars::JsonSchema;
use serde::Serialize;

/// The `schema` of the `state` command's output: the name that [`StateData`] has in an envelope.
pub const SCHEMA_STATE: &str = "state/1";
/// The `schema` of the `diff` command's output: the name that [`DiffData`] has in an envelope.
pub const SCHEMA_DIFF: &str = "diff/1";
/// The `schema` of the `events` command's output: the name that [`EventsData`] has in an envelope.
pub const SCHEMA_EVENTS: &str = "events/1";

/// A window asked about and everything known about where it is.
#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct WindowMatch {
    /// The window, as the state records it. Its `rect` is where it really was when the state was
    /// written, not where komorebi's layout put it.
    pub window: Window,
    /// Which monitor, workspace and place in the workspace it is in.
    pub location: Location,
    /// Where the layout says this container should be, when it is a tiled window.
    ///
    /// `None` for a window that is not in a container of the workspace's layout (monocle, maximized
    /// or floating, or a workspace with a monocle container) or whose container has no recorded
    /// layout rect.
    pub layout_rect: Option<Rect>,
    /// Parked off screen the way komorebi hides windows.
    ///
    /// This is whether the window's real rect is where komorebi parks a window it has hidden on that
    /// monitor, which depends on the komorebi that recorded the state: komorebi for Windows hides a
    /// window by cloaking it in place, so its position never says.
    pub hidden_position: bool,
}

/// One workspace of one monitor, in a state answer.
#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct WorkspaceSummary {
    /// The monitor (display id) the workspace is on.
    pub monitor_id: u64,
    /// The workspace's name.
    pub name: String,
    /// Whether it is the workspace showing on its monitor.
    pub showing: bool,
    /// How many windows it holds, of every kind: tiled, monocle, maximized and floating.
    pub windows: usize,
}

/// The answer to "what was komorebi's state at this moment", made by [`Analysis::state_answer`].
#[derive(Debug, Serialize, JsonSchema)]
pub struct StateData {
    /// The moment asked about, on every clock.
    pub at: Moment,
    /// Which notification the state is from, and how old it is at `at`.
    pub origin: StateOrigin,
    /// What has the keyboard focus.
    pub focus: Option<Focus>,
    /// Whether komorebi was paused.
    pub paused: bool,
    /// The monitors, with only the workspaces asked for (the showing ones by default).
    pub monitors: Vec<Monitor>,
    /// Every workspace of every monitor, however the monitors and workspaces were filtered.
    pub workspaces: Vec<WorkspaceSummary>,
    /// The windows that matched the window asked about (`--window`); empty if none was asked about.
    pub matches: Vec<WindowMatch>,
    /// What looks wrong in the state, narrowed to the monitor and windows asked about.
    pub violations: Vec<Violation>,
}

/// One difference between two states: the sentence a person would say, and the change it says it
/// of.
#[derive(Debug, Clone, PartialEq, Serialize, JsonSchema)]
pub struct ChangeView {
    /// The change in words.
    pub text: String,
    /// The change itself. Its fields are flattened into the same JSON object as `text`.
    #[serde(flatten)]
    pub change: Change,
}

/// The answer to "what changed between these two moments", made by [`Analysis::diff_answer`].
#[derive(Debug, Serialize, JsonSchema)]
pub struct DiffData {
    /// The earlier moment, as given.
    pub from: Moment,
    /// Which notification the state at `from` is from.
    pub from_origin: StateOrigin,
    /// The later moment, as given.
    pub to: Moment,
    /// Which notification the state at `to` is from.
    pub to_origin: StateOrigin,
    /// What differs between the two states. Empty if nothing a person would notice does, which is
    /// also the case when both moments fall in the same state.
    pub changes: Vec<ChangeView>,
    /// Violations present at `to` and not at `from`.
    pub violations_appeared: Vec<Violation>,
    /// Violations present at `from` and not at `to`.
    pub violations_resolved: Vec<Violation>,
}

/// The answer to "what happened", made by [`Analysis::events_answer`]: the events that matched, in
/// order, with the counts to tell whether the list is whole.
#[derive(Debug, Serialize, JsonSchema)]
pub struct EventsData {
    /// The events returned, in the order they were received.
    pub events: Vec<EventView>,
    /// How many events matched the filters, including any not returned because of the limit
    /// (`--limit`) or because they come before the cursor (`--cursor`).
    pub matching: usize,
    /// How many events the session has.
    pub total: usize,
    /// The name komorebi knew this recorder by. When komorebi logs at DEBUG it names it for each
    /// notification pushed, which makes pairing events with handlers exact.
    pub subscriber: Option<String>,
    /// Set when some of the returned events show the received time as their `komorebi_time`
    /// because the log has no `pushed notification` line for them: says so and why. Absent when
    /// every event has its pushed time.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub komorebi_time_note: Option<String>,
}

/// Which windows, monitors and workspaces a state answer is about.
///
/// The default asks about nothing in particular: every monitor, the workspaces that are showing,
/// and no window.
#[derive(Debug, Clone, Default)]
pub struct StateQuery {
    /// A window id, or text found in a window's application or title (compared without regard to
    /// case). The workspaces holding a match are included in the answer even if they are not
    /// showing. If nothing matches, [`Analysis::state_answer`] fails.
    pub window: Option<String>,
    /// Only this monitor (display id) in [`StateData::monitors`] and [`StateData::violations`].
    pub monitor: Option<u64>,
    /// Include workspaces that are not showing.
    pub all: bool,
}

/// Which events an events answer includes.
///
/// The default includes every event. A filter is only applied if it is set, and an event must pass
/// all of the filters that are.
#[derive(Debug, Clone, Default)]
pub struct EventQuery {
    /// Only events received at or after this time (nanoseconds on the master clock).
    pub from_mach_ns: Option<u64>,
    /// Only events received at or before this time.
    pub to_mach_ns: Option<u64>,
    /// Only events of these classes; empty means any class.
    pub classes: Vec<Class>,
    /// Leave out echoes of komorebi's own moves ([`Class::Echo`]).
    pub hide_echo: bool,
    /// Only events whose name contains this text, compared without regard to case.
    pub name: Option<String>,
    /// Only events about this process id.
    pub pid: Option<i32>,
    /// Only events about this window id.
    pub window: Option<u32>,
    /// Most events to return; `None` for all.
    pub limit: Option<usize>,
    /// Only events after this sequence number.
    pub after_seq: Option<u64>,
}

/// An events answer and where to continue if it was cut short.
pub struct EventsAnswer {
    /// The events and their counts.
    pub data: EventsData,
    /// Set if [`EventQuery::limit`] cut the answer short: the sequence number of the last event
    /// returned, to pass back as [`EventQuery::after_seq`] (the `--cursor` of the `events` command)
    /// for the rest. `None` if the answer is whole.
    pub next_cursor: Option<String>,
}

fn matches_window(text: &str, window: &Window) -> bool {
    if let Ok(id) = text.parse::<u64>()
        && window.id == id
    {
        return true;
    }

    let needle = text.to_lowercase();
    window.exe.to_lowercase().contains(&needle) || window.title.to_lowercase().contains(&needle)
}

impl Analysis {
    fn origin(&self, at: &StateAt, asked_mach_ns: u64) -> StateOrigin {
        match at.source {
            StateSource::Event { seq, recv_mach_ns } => StateOrigin {
                source: "event".to_string(),
                seq: Some(seq),
                state_id: at.state_id,
                age_ms: Some(asked_mach_ns.saturating_sub(recv_mach_ns) as f64 / 1e6),
            },
            StateSource::Start => StateOrigin {
                source: "start".to_string(),
                seq: None,
                state_id: None,
                age_ms: Some(
                    asked_mach_ns.saturating_sub(self.manifest.start.mach_ns) as f64 / 1e6,
                ),
            },
        }
    }

    /// Returns the state at `mach_ns`, filtered to what `query` asks about, with what looks wrong
    /// in it.
    ///
    /// The state is the one [`Analysis::state_at`] finds, so it is as of the latest notification
    /// before `mach_ns`; [`StateData::origin`] names which one and how old it is.
    ///
    /// # Errors
    ///
    /// Returns the errors of [`Analysis::state_at`] (no state was recorded before `mach_ns`), and a
    /// not-found error if [`StateQuery::window`] is set and no window in the state matches it.
    pub fn state_answer(&self, mach_ns: u64, query: &StateQuery) -> eyre::Result<StateData> {
        let found = self.state_at(mach_ns)?;
        let snapshot: &Snapshot = &found.snapshot;

        let matches: Vec<WindowMatch> = match &query.window {
            Some(text) => snapshot
                .windows()
                .into_iter()
                .filter(|(_, window)| matches_window(text, window))
                .map(|(location, window)| {
                    let monitor = snapshot
                        .monitors
                        .iter()
                        .find(|monitor| monitor.id == location.monitor_id);
                    let layout_rect = monitor
                        .and_then(|monitor| {
                            monitor
                                .workspaces
                                .iter()
                                .find(|workspace| workspace.name == location.workspace)
                        })
                        .and_then(|workspace| match location.place {
                            Place::Container { index, .. } if workspace.monocle.is_none() => {
                                workspace.layout_rects.get(index).copied()
                            }
                            _ => None,
                        });
                    let hidden_position = window
                        .rect
                        .as_ref()
                        .zip(monitor)
                        .is_some_and(|(rect, monitor)| snapshot.is_hidden_position(rect, monitor));
                    WindowMatch {
                        window: window.clone(),
                        location,
                        layout_rect,
                        hidden_position,
                    }
                })
                .collect(),
            None => Vec::new(),
        };
        if query.window.is_some() && matches.is_empty() {
            return Err(envelope::not_found(format!(
                "no window matches '{}' in the state at {}",
                query.window.as_deref().unwrap_or_default(),
                self.moment(mach_ns).timecode
            )));
        }

        let all_windows = snapshot.windows();
        let workspaces: Vec<WorkspaceSummary> = snapshot
            .monitors
            .iter()
            .flat_map(|monitor| {
                let all_windows = &all_windows;
                monitor
                    .workspaces
                    .iter()
                    .enumerate()
                    .map(move |(index, workspace)| WorkspaceSummary {
                        monitor_id: monitor.id,
                        name: workspace.name.clone(),
                        showing: index == monitor.focused,
                        windows: all_windows
                            .iter()
                            .filter(|(location, _)| {
                                location.monitor_id == monitor.id
                                    && location.workspace == workspace.name
                            })
                            .count(),
                    })
            })
            .collect();

        let wanted = |monitor: &Monitor, index: usize, name: &str| {
            query.all
                || index == monitor.focused
                || matches.iter().any(|found| {
                    found.location.monitor_id == monitor.id && found.location.workspace == name
                })
        };
        let monitors: Vec<Monitor> = snapshot
            .monitors
            .iter()
            .filter(|monitor| query.monitor.is_none_or(|id| id == monitor.id))
            .map(|monitor| {
                let mut kept = monitor.clone();
                kept.workspaces = monitor
                    .workspaces
                    .iter()
                    .enumerate()
                    .filter(|(index, workspace)| wanted(monitor, *index, &workspace.name))
                    .map(|(_, workspace)| workspace.clone())
                    .collect();
                kept
            })
            .collect();

        let mut violations = invariants::check(snapshot);
        if let Some(id) = query.monitor {
            violations.retain(|violation| violation.monitor_id.is_none_or(|monitor| monitor == id));
        }
        if !matches.is_empty() {
            violations.retain(|violation| {
                violation.window_ids.is_empty()
                    || violation
                        .window_ids
                        .iter()
                        .any(|id| matches.iter().any(|found| found.window.id == *id))
            });
        }

        Ok(StateData {
            at: self.moment(mach_ns),
            origin: self.origin(&found, mach_ns),
            focus: snapshot.focus(),
            paused: snapshot.paused,
            monitors,
            workspaces,
            matches,
            violations,
        })
    }

    /// Returns what changed between the state at `from_mach_ns` and the state at `to_mach_ns`, in
    /// terms a person would use, and which violations appeared or went away.
    ///
    /// Both states are found as [`Analysis::state_at`] does. The order of the two times is not
    /// checked: with `to_mach_ns` before `from_mach_ns` the answer describes the change backwards.
    /// The `diff` command refuses that.
    ///
    /// # Errors
    ///
    /// Returns the errors of [`Analysis::state_at`] for either moment.
    pub fn diff_answer(&self, from_mach_ns: u64, to_mach_ns: u64) -> eyre::Result<DiffData> {
        let before = self.state_at(from_mach_ns)?;
        let after = self.state_at(to_mach_ns)?;
        let changes = diff::diff(&before.snapshot, &after.snapshot);

        let (was, now) = (
            invariants::check(&before.snapshot),
            invariants::check(&after.snapshot),
        );
        Ok(DiffData {
            from: self.moment(from_mach_ns),
            from_origin: self.origin(&before, from_mach_ns),
            to: self.moment(to_mach_ns),
            to_origin: self.origin(&after, to_mach_ns),
            changes: changes
                .into_iter()
                .map(|change| ChangeView {
                    text: change.describe(),
                    change,
                })
                .collect(),
            violations_appeared: now.iter().filter(|v| !was.contains(v)).cloned().collect(),
            violations_resolved: was.iter().filter(|v| !now.contains(v)).cloned().collect(),
        })
    }

    /// Returns the events matching `query`, in the order they were received.
    ///
    /// This does not fail: an empty answer means nothing matched. At most [`EventQuery::limit`]
    /// events are returned, starting after [`EventQuery::after_seq`]; [`EventsData::matching`]
    /// counts the matches before that, and [`EventsAnswer::next_cursor`] gives where to continue.
    #[must_use]
    pub fn events_answer(&self, query: &EventQuery) -> EventsAnswer {
        let all = self.events();
        let needle = query.name.as_deref().map(str::to_lowercase);
        let wanted: Vec<usize> = (0..all.len())
            .filter(|index| {
                let event = &all[*index];
                let class = self.pairing().classes[*index].class;
                query
                    .from_mach_ns
                    .is_none_or(|from| event.recv_mach_ns >= from)
                    && query.to_mach_ns.is_none_or(|to| event.recv_mach_ns <= to)
                    && (query.classes.is_empty() || query.classes.contains(&class))
                    && !(query.hide_echo && class == Class::Echo)
                    && needle
                        .as_ref()
                        .is_none_or(|needle| event.info.name.to_lowercase().contains(needle))
                    && query.pid.is_none_or(|pid| event.info.pid == Some(pid))
                    && query
                        .window
                        .is_none_or(|window| event.info.window_id == Some(window))
            })
            .collect();

        let remaining: Vec<usize> = wanted
            .iter()
            .copied()
            .filter(|index| query.after_seq.is_none_or(|after| all[*index].seq > after))
            .collect();
        let count = query
            .limit
            .map_or(remaining.len(), |limit| limit.min(remaining.len()));
        let events: Vec<EventView> = remaining[..count]
            .iter()
            .filter_map(|index| self.event_view(*index))
            .collect();
        let komorebi_time_note = super::views::komorebi_time_note(&events);
        let next_cursor = (count < remaining.len())
            .then(|| events.last().map(|view| view.seq.to_string()))
            .flatten();

        EventsAnswer {
            data: EventsData {
                events,
                matching: wanted.len(),
                total: all.len(),
                subscriber: self.pairing().subscriber.clone(),
                komorebi_time_note,
            },
            next_cursor,
        }
    }
}

/// `annotate add` / `edit` answer with the annotation as it now reads.
#[derive(Serialize, JsonSchema)]
pub struct AnnotationData {
    /// The annotation, read back from the file after the write.
    pub annotation: AnnotationView,
}

/// The answer of `annotate delete`.
#[derive(Serialize, JsonSchema)]
pub struct DeletedData {
    /// The id of the annotation that was removed.
    pub deleted: String,
}

/// The answer of `annotations`: the session's annotations, optionally of one kind.
#[derive(Serialize, JsonSchema)]
pub struct AnnotationsData {
    /// The annotations, ordered by start and then by id.
    pub annotations: Vec<AnnotationView>,
}
