//! One window's life through a recording: when it appeared, where komorebi put it, when it was
//! parked off screen or moved, and which event did each.
//!
//! Most bugs are about one window ("my terminal ended up behind the dock"), and the state at any
//! moment is dozens of windows. [`Analysis::window_history`] follows the windows that match a query
//! through every recorded state and reports each moment something about them changed, with the
//! event it came with. The `window` command prints it.
//!
//! # What is reported
//!
//! The recorded states are read in event order, and an event makes an entry for a window if:
//!
//! - the window **appeared** in the state, or **went away** from it;
//! - it **moved**: its monitor, its workspace or its kind of place (container, monocle, maximized,
//!   floating) differs from the last state it was in. A change of container index or tab within
//!   the same kind of place is not a move;
//! - its workspace **came into view** or **went out of view**;
//! - it was **parked off screen** in komorebi's hiding position, or **brought back**;
//! - its **rect** is really somewhere else: more than 2 points off in left, top, width or height,
//!   while it was not parked or brought back by the same event; or
//! - the event is **about the window** (it names the window's id) and the window is in the
//!   state, even if nothing about the window changed.
//!
//! An event whose notification carried no state is skipped, since there is nothing to compare.
//! A window is followed only through the states in which it matches the query, so a window whose
//! title changes to something the query no longer matches is reported as having gone away.
//!
//! # Flavor
//!
//! Whether a window is parked is decided by the rules of the flavor that recorded the session
//! ([`Snapshot::is_hidden_position`]). komorebi for Mac parks a hidden window in a corner of its
//! monitor. komorebi for Windows cloaks it in place, so for a Windows session the history has no
//! "parked off screen" or "brought back" entries and every [`HistoryEntry::hidden`] is `false`.

use super::Analysis;
use super::classify::Class;
use super::envelope;
use super::state::Location;
use super::state::Place;
use super::state::Rect;
use super::state::Snapshot;
use super::state::Window;
use color_eyre::eyre;
use schemars::JsonSchema;
use serde::Serialize;
use std::collections::BTreeMap;

/// The schema name that identifies a [`HistoryData`] in an envelope (`window/1`); listed by the
/// `schema` command.
pub const SCHEMA_WINDOW: &str = "window/1";

/// A window rect that moved by no more than this is not reported as having moved.
const RECT_TOLERANCE: i64 = 2;

/// A window that a query matched, named as it was the first time it matched.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, JsonSchema)]
pub struct TrackedWindow {
    /// The window's id in komorebi's state: a window id on a Mac, a window handle on Windows.
    pub id: u64,
    /// The program the window belongs to, as komorebi names it.
    pub exe: String,
    /// The window's title in the first state it matched in. A title can change later.
    pub title: String,
}

/// One moment in a window's life: something about it changed, or an event was about it.
#[derive(Debug, Clone, PartialEq, Serialize, JsonSchema)]
pub struct HistoryEntry {
    /// The id of the window the entry is about.
    pub window_id: u64,
    /// The event whose notification carried this state.
    pub seq: u64,
    /// When the notification was received, in seconds of video time. An event is received when its
    /// handler has finished.
    pub video_s: f64,
    /// When the notification was received, in nanoseconds on the host's master monotonic clock.
    pub mach_ns: u64,
    /// A line saying what the event was, such as `MoveEnd pid 20934 AXWindowMoved`.
    pub event: String,
    /// The kind of event, as the analysis classified it.
    pub class: Class,
    /// Where the window was after this event; `None` if it was gone.
    pub location: Option<Location>,
    /// Where it really was on screen after this event.
    pub rect: Option<Rect>,
    /// Whether the window is parked in komorebi's hiding position (see
    /// [`Snapshot::is_hidden_position`]). Always `false` for a session recorded from komorebi for
    /// Windows, which hides a window without moving it.
    pub hidden: bool,
    /// How the window changed since the last state it was in, in words. Empty when the entry is
    /// only there because the event was about the window.
    pub changes: Vec<String>,
    /// This event was *about* the window (it names its id), not just a bystander.
    pub about_window: bool,
}

/// The life of the windows a query matched: the `data` of the `window` command.
#[derive(Debug, Clone, PartialEq, Serialize, JsonSchema)]
pub struct HistoryData {
    /// The query, as given.
    pub query: String,
    /// Every window the query matched in any recorded state, ordered by window id.
    pub windows: Vec<TrackedWindow>,
    /// The entries for events after the cursor, in event order and, within an event, in window id
    /// order. At most the `limit` asked for.
    pub entries: Vec<HistoryEntry>,
    /// How many entries there are after the cursor, including any not returned.
    pub matching: usize,
}

/// What about a window is worth noticing when it changes.
#[derive(Clone, PartialEq, Eq)]
struct Seen {
    monitor_id: u64,
    workspace: String,
    kind: &'static str,
    visible: bool,
    rect: Option<Rect>,
    hidden: bool,
    location: Location,
}

fn kind_of(place: &Place) -> &'static str {
    match place {
        Place::Container { .. } => "container",
        Place::Monocle { .. } => "monocle",
        Place::Maximized => "maximized",
        Place::Floating { .. } => "floating",
    }
}

fn seen(snapshot: &Snapshot, location: Location, window: &Window) -> Seen {
    let hidden = window.rect.as_ref().is_some_and(|rect| {
        snapshot
            .monitors
            .iter()
            .find(|monitor| monitor.id == location.monitor_id)
            .is_some_and(|monitor| snapshot.is_hidden_position(rect, monitor))
    });
    Seen {
        monitor_id: location.monitor_id,
        workspace: location.workspace.clone(),
        kind: kind_of(&location.place),
        visible: location.workspace_visible,
        rect: window.rect,
        hidden,
        location,
    }
}

fn matches(query: &str, window: &Window) -> bool {
    if let Ok(id) = query.parse::<u64>()
        && window.id == id
    {
        return true;
    }

    let needle = query.to_lowercase();
    window.exe.to_lowercase().contains(&needle) || window.title.to_lowercase().contains(&needle)
}

impl Analysis {
    /// Returns the history of the windows matching `query`, for events after `after_seq`, at most
    /// `limit` entries.
    ///
    /// `query` is a window id or text. A window matches if its id is the number `query` spells, or
    /// if `query`, compared case-insensitively, is part of the window's program or title. A number
    /// therefore matches windows whose title contains it as well as the window with that id.
    ///
    /// `after_seq` is a cursor: only entries for events whose `seq` is greater are counted and
    /// returned, so a caller pages by passing the last `seq` it has. With `limit` set, entries
    /// stop being returned after that many, but [`HistoryData::matching`] still counts all of them.
    /// All the windows the query matched are in [`HistoryData::windows`] whatever the cursor.
    /// What an entry means is described in the [module](self) documentation.
    ///
    /// # Errors
    ///
    /// Returns an error if:
    ///
    /// - no window matching `query` appears in any recorded state (the not-found exit kind). An
    ///   empty `entries`, because the cursor is past the last change, is not an error;
    /// - a state that an event refers to cannot be read: the file is missing (not found) or is not
    ///   valid JSON.
    ///
    /// # Example
    ///
    /// ```rust
    /// use komorebi_recorder::analysis::Analysis;
    /// use komorebi_recorder::testkit::SessionBuilder;
    /// use komorebi_recorder::testkit::state_with_windows;
    /// use serde_json::json;
    ///
    /// // Ghostty fills the screen, then Safari is opened beside it and Ghostty is squeezed.
    /// let (session, _) = SessionBuilder::new("doc-window-history")
    ///     .event(2.0, json!({"type": "Retile"}), Some(state_with_windows(&[(1, "Ghostty")])))
    ///     .event(
    ///         3.0,
    ///         json!({"type": "Retile"}),
    ///         Some(state_with_windows(&[(1, "Ghostty"), (2, "Safari")])),
    ///     )
    ///     .build();
    /// let analysis = Analysis::open(&session.dir)?;
    ///
    /// let history = analysis.window_history("ghostty", None, None)?;
    /// assert_eq!(history.windows.len(), 1);
    /// assert_eq!(history.windows[0].id, 1);
    /// assert_eq!(history.entries.len(), 2);
    /// assert_eq!(
    ///     history.entries[0].changes,
    ///     ["appeared on monitor 1 workspace one (container 0)"]
    /// );
    /// assert_eq!(
    ///     history.entries[1].changes,
    ///     ["really at 500x800 at (0, 0) instead of 1000x800 at (0, 0)"]
    /// );
    ///
    /// // paging: only what came after event 1
    /// let rest = analysis.window_history("ghostty", Some(1), None)?;
    /// assert_eq!(rest.entries.len(), 1);
    /// assert_eq!(rest.matching, 1);
    ///
    /// // a query nothing matches is an error, not an empty history
    /// assert!(analysis.window_history("no such window", None, None).is_err());
    /// # Ok::<(), Box<dyn std::error::Error>>(())
    /// ```
    ///
    /// Also covered by `tests/trace.rs::a_window_is_followed_through_every_state` and
    /// `tests/trace.rs::window_history_pages_with_a_cursor`.
    pub fn window_history(
        &self,
        query: &str,
        after_seq: Option<u64>,
        limit: Option<usize>,
    ) -> eyre::Result<HistoryData> {
        let mut tracked: BTreeMap<u64, TrackedWindow> = BTreeMap::new();
        let mut last: BTreeMap<u64, Option<Seen>> = BTreeMap::new();
        let mut entries: Vec<HistoryEntry> = Vec::new();
        let mut matching = 0_usize;

        let mut previous_state_id: Option<u64> = None;
        let mut previous_snapshot: Option<std::sync::Arc<Snapshot>> = None;

        for (index, event) in self.events().iter().enumerate() {
            let Some(state_id) = event.state_id else {
                continue;
            };

            // an unchanged state cannot have changed a window, but the event may be about it
            let snapshot = if previous_state_id == Some(state_id) {
                previous_snapshot.clone().ok_or_else(|| {
                    eyre::Report::msg("internal error: repeated state without a snapshot")
                })?
            } else {
                self.snapshot(state_id)?
            };
            previous_state_id = Some(state_id);
            previous_snapshot = Some(snapshot.clone());

            let class = self.pairing().classes[index].class;
            let here: BTreeMap<u64, (Location, &Window)> = snapshot
                .windows()
                .into_iter()
                .filter(|(_, window)| matches(query, window))
                .fold(BTreeMap::new(), |mut found, (location, window)| {
                    found.entry(window.id).or_insert((location, window));
                    found
                });

            for (id, (_, window)) in &here {
                tracked.entry(*id).or_insert_with(|| TrackedWindow {
                    id: *id,
                    exe: window.exe.clone(),
                    title: window.title.clone(),
                });
                last.entry(*id).or_insert(None);
            }

            let about = |id: u64| {
                event
                    .info
                    .window_id
                    .is_some_and(|window| u64::from(window) == id)
            };
            let ids: Vec<u64> = last.keys().copied().collect();
            for id in ids {
                let now = here
                    .get(&id)
                    .map(|(location, window)| seen(&snapshot, location.clone(), window));
                let before = last.get(&id).cloned().flatten();

                let mut changes = Vec::new();
                match (&before, &now) {
                    (None, Some(now)) => {
                        changes.push(format!("appeared on {}", now.location.describe()))
                    }
                    (Some(_), None) => changes.push("went away".to_string()),
                    (Some(was), Some(now)) => {
                        if (was.monitor_id, &was.workspace, was.kind)
                            != (now.monitor_id, &now.workspace, now.kind)
                        {
                            changes.push(format!(
                                "moved from {} to {}",
                                was.location.describe(),
                                now.location.describe()
                            ));
                        }
                        if was.visible != now.visible {
                            changes.push(
                                if now.visible {
                                    "its workspace came into view"
                                } else {
                                    "its workspace went out of view"
                                }
                                .to_string(),
                            );
                        }
                        if was.hidden != now.hidden {
                            changes.push(
                                if now.hidden {
                                    "parked off screen in komorebi's hiding position"
                                } else {
                                    "brought back on screen"
                                }
                                .to_string(),
                            );
                        }
                        if let (Some(from), Some(to)) = (&was.rect, &now.rect)
                            && from.max_delta(to) > RECT_TOLERANCE
                            && was.hidden == now.hidden
                        {
                            changes.push(format!(
                                "really at {} instead of {}",
                                to.describe(),
                                from.describe()
                            ));
                        }
                    }
                    (None, None) => {}
                }

                let relevant = !changes.is_empty() || (about(id) && now.is_some());
                if relevant && after_seq.is_none_or(|after| event.seq > after) {
                    matching += 1;
                    if limit.is_none_or(|limit| entries.len() < limit) {
                        entries.push(HistoryEntry {
                            window_id: id,
                            seq: event.seq,
                            video_s: self.video_s(event.recv_mach_ns),
                            mach_ns: event.recv_mach_ns,
                            event: event.info.summary(),
                            class,
                            location: now.as_ref().map(|seen| seen.location.clone()),
                            rect: now.as_ref().and_then(|seen| seen.rect),
                            hidden: now.as_ref().is_some_and(|seen| seen.hidden),
                            changes,
                            about_window: about(id),
                        });
                    }
                }
                last.insert(id, now);
            }
        }

        if tracked.is_empty() {
            return Err(envelope::not_found(format!(
                "no window matching '{query}' appears in any recorded state"
            )));
        }

        Ok(HistoryData {
            query: query.to_string(),
            windows: tracked.into_values().collect(),
            entries,
            matching,
        })
    }
}
