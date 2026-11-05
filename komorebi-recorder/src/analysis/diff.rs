//! The differences between two komorebi states, described the way a person would.
//!
//! A raw JSON diff of komorebi's `State` is mostly noise: a window that moved from the second
//! container to the first is a dozen changed paths. [`diff`] compares two [`Snapshot`]s and reports
//! [`Change`]s instead: a window appeared, went away, moved to another workspace or was really
//! somewhere else on screen; focus moved; a layout or a workspace changed; a monitor came or went.
//! It is a pure function of the two states: nothing is read from a session, so any two snapshots
//! can be compared, from any recording. The `diff` command, `trace` and the viewer all call it.
//!
//! # What counts as a change
//!
//! - **Windows are followed by id.** A window has moved only if its monitor, its workspace or the
//!   container it is in changes, or it goes from one kind of place to another (tiled, monocle,
//!   maximized, floating). Containers are compared by their id, not their index, so a container
//!   that slides from index 1 to index 0 because the one before it closed is not a move, and
//!   neither is switching to another tab of the same container. If one id appears in more than one
//!   place in a state (see `window-in-two-places` in `crate::analysis::invariants`), the first place
//!   found is the one used.
//! - **A rect must move by more than 2 points** in any of left, top, width and height to be a
//!   [`Change::WindowRect`]: the accessibility API rounds, and some applications snap to whole
//!   points. A window whose rect is missing from either state is never reported as having moved
//!   on screen.
//! - **A reorder is one change.** If the same containers are in a different order, the result is a
//!   single [`Change::ContainerOrder`] rather than a series of moves.
//! - **Monitors are matched by id** and workspaces by name, within a monitor present in both
//!   states. A workspace that exists in only one of the states has no layout or order to compare.
//!
//! # Flavor
//!
//! komorebi for Mac parks a hidden window in a corner of its monitor, so a window's rect shows
//! whether it is hidden. komorebi for Windows cloaks a window where it is, so it does not. The
//! `hidden_before` and `hidden_after` flags of [`Change::WindowRect`] are therefore decided by the
//! rules of the flavor that wrote the state ([`Snapshot::is_hidden_position`]), and are always
//! `false` for a komorebi for Windows state. Nothing else here depends on the flavor.

use super::state::Focus;
use super::state::Location;
use super::state::Monitor;
use super::state::Rect;
use super::state::Snapshot;
use super::state::Window;
use schemars::JsonSchema;
use serde::Serialize;
use std::collections::BTreeMap;

/// A window rect that moved by no more than this is treated as not having moved: the accessibility
/// API rounds, and some applications snap to whole points.
const RECT_TOLERANCE: i64 = 2;

/// A window, named so that a reader can tell which one a [`Change`] is about.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, JsonSchema)]
pub struct WindowRef {
    /// The window's id in komorebi's state: a window id on a Mac, a window handle on Windows.
    pub id: u64,
    /// The window's title when the state was serialized.
    pub title: String,
    /// The program the window belongs to, as komorebi names it.
    pub exe: String,
}

impl WindowRef {
    fn of(window: &Window) -> Self {
        Self {
            id: window.id,
            title: window.title.clone(),
            exe: window.exe.clone(),
        }
    }

    /// Returns the window as `exe "title" (window id)`.
    #[must_use]
    pub fn describe(&self) -> String {
        format!("{} \"{}\" (window {})", self.exe, self.title, self.id)
    }
}

/// One difference between two states, as [`diff`] reports it.
///
/// In JSON a change is an object with a `kind` field naming the variant in kebab case
/// ([`Change::kind`]) and the variant's fields beside it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, JsonSchema)]
#[serde(tag = "kind", rename_all = "kebab-case")]
pub enum Change {
    /// A monitor is in the second state and was not in the first.
    MonitorAdded {
        /// The display id of the monitor ([`Monitor::id`]).
        monitor_id: u64,
        /// The monitor's device name.
        device: String,
        /// The monitor's area, in global display points.
        size: Rect,
    },
    /// A monitor was in the first state and is not in the second.
    MonitorRemoved {
        /// The display id of the monitor ([`Monitor::id`]).
        monitor_id: u64,
        /// The monitor's device name.
        device: String,
    },
    /// A monitor is in both states but its area differs (by any amount: there is no tolerance).
    MonitorResized {
        /// The display id of the monitor ([`Monitor::id`]).
        monitor_id: u64,
        /// The area in the first state, in global display points.
        from: Rect,
        /// The area in the second state, in global display points.
        to: Rect,
    },
    /// The workspace showing on a monitor changed.
    WorkspaceFocus {
        /// The display id of the monitor.
        monitor_id: u64,
        /// The name of the workspace that was showing; empty if the monitor had none.
        from: String,
        /// The name of the workspace that is showing; empty if the monitor has none.
        to: String,
    },
    /// The keyboard focus moved (see [`Snapshot::focus`] for what has it).
    Focus {
        /// The focus before: a monitor, workspace and window; `None` if nothing had it.
        from: Option<Focus>,
        /// The focus after, in the same terms; `None` if nothing has it.
        to: Option<Focus>,
    },
    /// A workspace's layout changed.
    Layout {
        /// The display id of the monitor the workspace is on.
        monitor_id: u64,
        /// The name of the workspace.
        workspace: String,
        /// The layout's name before, e.g. `Columns` or `BSP`.
        from: String,
        /// The layout's name after.
        to: String,
    },
    /// The containers of a workspace are in a different order (window ids by container).
    ContainerOrder {
        /// The display id of the monitor the workspace is on.
        monitor_id: u64,
        /// The name of the workspace.
        workspace: String,
        /// The id of the first window of each container, in container order, before.
        from: Vec<u64>,
        /// The same list after: the same ids in another order.
        to: Vec<u64>,
    },
    /// A window is in the second state and was not in the first.
    WindowAdded {
        /// The window.
        window: WindowRef,
        /// Where it is in the second state.
        at: Location,
    },
    /// A window was in the first state and is not in the second.
    WindowRemoved {
        /// The window.
        window: WindowRef,
        /// Where it was in the first state.
        from: Location,
    },
    /// Moved to another container, another kind of place, another workspace or another monitor.
    WindowMoved {
        /// The window.
        window: WindowRef,
        /// Where it was in the first state.
        from: Location,
        /// Where it is in the second state.
        to: Location,
    },
    /// Really somewhere else on screen.
    WindowRect {
        /// The window.
        window: WindowRef,
        /// Where the window was in the first state, as read live from the OS.
        from: Rect,
        /// Where the window is in the second state, as read live from the OS.
        to: Rect,
        /// Whether `from` is where komorebi parks a hidden window. Always `false` for komorebi
        /// for Windows, which hides a window without moving it.
        hidden_before: bool,
        /// Whether `to` is where komorebi parks a hidden window. Always `false` for komorebi for
        /// Windows.
        hidden_after: bool,
    },
    /// komorebi was paused or resumed.
    Paused {
        /// Whether komorebi is paused in the second state.
        paused: bool,
    },
}

impl Change {
    /// Returns the kind name of the change, the same as the `kind` field of its JSON: for example
    /// `window-added`.
    #[must_use]
    pub fn kind(&self) -> &'static str {
        match self {
            Self::MonitorAdded { .. } => "monitor-added",
            Self::MonitorRemoved { .. } => "monitor-removed",
            Self::MonitorResized { .. } => "monitor-resized",
            Self::WorkspaceFocus { .. } => "workspace-focus",
            Self::Focus { .. } => "focus",
            Self::Layout { .. } => "layout",
            Self::ContainerOrder { .. } => "container-order",
            Self::WindowAdded { .. } => "window-added",
            Self::WindowRemoved { .. } => "window-removed",
            Self::WindowMoved { .. } => "window-moved",
            Self::WindowRect { .. } => "window-rect",
            Self::Paused { .. } => "paused",
        }
    }

    /// Returns one line a person can read, such as `Safari "window 2" appeared on monitor 1
    /// workspace one (container 1)`.
    #[must_use]
    pub fn describe(&self) -> String {
        let focus = |focus: &Option<Focus>| {
            focus.as_ref().map_or_else(
                || "nothing".to_string(),
                |focus| {
                    format!(
                        "window {} on monitor {} workspace {}",
                        focus
                            .window_id
                            .map_or_else(|| "(none)".to_string(), |id| id.to_string()),
                        focus.monitor_id,
                        focus.workspace
                    )
                },
            )
        };

        match self {
            Self::MonitorAdded {
                monitor_id,
                device,
                size,
            } => {
                format!(
                    "monitor {monitor_id} \"{device}\" appeared, {}",
                    size.describe()
                )
            }
            Self::MonitorRemoved { monitor_id, device } => {
                format!("monitor {monitor_id} \"{device}\" went away")
            }
            Self::MonitorResized {
                monitor_id,
                from,
                to,
            } => format!(
                "monitor {monitor_id} changed from {} to {}",
                from.describe(),
                to.describe()
            ),
            Self::WorkspaceFocus {
                monitor_id,
                from,
                to,
            } => {
                format!("monitor {monitor_id} switched from workspace {from} to {to}")
            }
            Self::Focus { from, to } => {
                format!("focus moved from {} to {}", focus(from), focus(to))
            }
            Self::Layout {
                monitor_id,
                workspace,
                from,
                to,
            } => format!(
                "monitor {monitor_id} workspace {workspace} layout changed from {from} to {to}"
            ),
            Self::ContainerOrder {
                monitor_id,
                workspace,
                from,
                to,
            } => format!(
                "monitor {monitor_id} workspace {workspace} containers reordered from {from:?} to {to:?}"
            ),
            Self::WindowAdded { window, at } => {
                format!("{} appeared on {}", window.describe(), at.describe())
            }
            Self::WindowRemoved { window, from } => {
                format!("{} went away from {}", window.describe(), from.describe())
            }
            Self::WindowMoved { window, from, to } => format!(
                "{} moved from {} to {}",
                window.describe(),
                from.describe(),
                to.describe()
            ),
            Self::WindowRect {
                window,
                from,
                to,
                hidden_before,
                hidden_after,
            } => {
                let note = match (hidden_before, hidden_after) {
                    (false, true) => " (now hidden)",
                    (true, false) => " (now shown)",
                    _ => "",
                };
                format!(
                    "{} is really at {} instead of {}{note}",
                    window.describe(),
                    to.describe(),
                    from.describe()
                )
            }
            Self::Paused { paused } => {
                format!("komorebi {}", if *paused { "paused" } else { "resumed" })
            }
        }
    }
}

/// Where a window sits, coarsely: it only counts as having moved if this changes. Container
/// *indexes* shift whenever a container before it comes or goes, so containers are compared by id.
fn anchor(snapshot: &Snapshot) -> BTreeMap<u64, (u64, String, String, Location, Window)> {
    let mut anchors = BTreeMap::new();
    for monitor in &snapshot.monitors {
        for (index, workspace) in monitor.workspaces.iter().enumerate() {
            let visible = index == monitor.focused;
            let mut put = |window: &Window, slot: String, place: super::state::Place| {
                anchors.entry(window.id).or_insert_with(|| {
                    (
                        monitor.id,
                        workspace.name.clone(),
                        slot,
                        Location {
                            monitor_id: monitor.id,
                            workspace: workspace.name.clone(),
                            place,
                            workspace_visible: visible,
                        },
                        window.clone(),
                    )
                });
            };

            for (container_index, container) in workspace.containers.iter().enumerate() {
                for (tab, window) in container.windows.iter().enumerate() {
                    put(
                        window,
                        format!("container {}", container.id),
                        super::state::Place::Container {
                            index: container_index,
                            tab,
                        },
                    );
                }
            }
            if let Some(monocle) = &workspace.monocle {
                for (tab, window) in monocle.windows.iter().enumerate() {
                    put(
                        window,
                        "monocle".to_string(),
                        super::state::Place::Monocle { tab },
                    );
                }
            }
            if let Some(window) = &workspace.maximized {
                put(
                    window,
                    "maximized".to_string(),
                    super::state::Place::Maximized,
                );
            }
            for (index, window) in workspace.floating.iter().enumerate() {
                put(
                    window,
                    "floating".to_string(),
                    super::state::Place::Floating { index },
                );
            }
        }
    }

    anchors
}

fn is_hidden(snapshot: &Snapshot, window: &Window) -> bool {
    window.rect.as_ref().is_some_and(|rect| {
        snapshot
            .monitor_under(rect)
            .or_else(|| snapshot.monitors.first())
            .is_some_and(|monitor| snapshot.is_hidden_position(rect, monitor))
    })
}

fn monitor_by_id(snapshot: &Snapshot, id: u64) -> Option<&Monitor> {
    snapshot.monitors.iter().find(|monitor| monitor.id == id)
}

/// Returns what changed from the state `before` to the state `after`, empty if nothing did.
///
/// The changes come in a fixed order, which tests and the text output rely on:
///
/// 1. [`Change::Paused`];
/// 2. [`Change::MonitorRemoved`] and [`Change::MonitorResized`], by monitor of `before`, then
///    [`Change::MonitorAdded`];
/// 3. for each monitor in both states, [`Change::WorkspaceFocus`], then for each of its workspaces
///    [`Change::Layout`] and [`Change::ContainerOrder`];
/// 4. [`Change::Focus`];
/// 5. for each window of `after`, in order of window id, [`Change::WindowAdded`], or else
///    [`Change::WindowMoved`] followed by [`Change::WindowRect`];
/// 6. [`Change::WindowRemoved`], for each window of `before` that is not in `after`, in order of
///    window id.
///
/// What counts as a change is described in the [module](self) documentation. The comparison cannot
/// fail and does not panic.
///
/// # Example
///
/// A second window is opened beside the first, so the first is squeezed to half the screen:
///
/// ```rust
/// use komorebi_recorder::analysis::diff::Change;
/// use komorebi_recorder::analysis::diff::diff;
/// use komorebi_recorder::analysis::flavor::Flavor;
/// use komorebi_recorder::analysis::state::Snapshot;
/// use komorebi_recorder::testkit::state_with_windows;
///
/// let before = Snapshot::parse(Flavor::Macos, &state_with_windows(&[(1, "Ghostty")]));
/// let after = Snapshot::parse(
///     Flavor::Macos,
///     &state_with_windows(&[(1, "Ghostty"), (2, "Safari")]),
/// );
///
/// let changes = diff(&before, &after);
/// let kinds: Vec<_> = changes.iter().map(Change::kind).collect();
/// assert_eq!(kinds, ["window-rect", "window-added"]);
/// assert_eq!(
///     changes[0].describe(),
///     "Ghostty \"window 1\" (window 1) is really at 500x800 at (0, 0) instead of 1000x800 at (0, 0)"
/// );
/// assert_eq!(
///     changes[1].describe(),
///     "Safari \"window 2\" (window 2) appeared on monitor 1 workspace one (container 1)"
/// );
///
/// // the same state twice has nothing to report
/// assert!(diff(&after, &after.clone()).is_empty());
/// ```
///
/// Covered in detail by the unit tests of this module, among them
/// `container_indexes_shifting_is_not_a_move`, `reordered_containers_are_one_change` and
/// `rect_jitter_within_the_tolerance_is_not_a_change`.
#[must_use]
pub fn diff(before: &Snapshot, after: &Snapshot) -> Vec<Change> {
    let mut changes = Vec::new();

    if before.paused != after.paused {
        changes.push(Change::Paused {
            paused: after.paused,
        });
    }

    for monitor in &before.monitors {
        match monitor_by_id(after, monitor.id) {
            None => changes.push(Change::MonitorRemoved {
                monitor_id: monitor.id,
                device: monitor.device.clone(),
            }),
            Some(now) if now.size != monitor.size => changes.push(Change::MonitorResized {
                monitor_id: monitor.id,
                from: monitor.size,
                to: now.size,
            }),
            Some(_) => {}
        }
    }
    for monitor in &after.monitors {
        if monitor_by_id(before, monitor.id).is_none() {
            changes.push(Change::MonitorAdded {
                monitor_id: monitor.id,
                device: monitor.device.clone(),
                size: monitor.size,
            });
        }
    }

    for monitor in &after.monitors {
        let Some(was) = monitor_by_id(before, monitor.id) else {
            continue;
        };

        let name = |monitor: &Monitor| {
            monitor
                .focused_workspace()
                .map_or_else(String::new, |workspace| workspace.name.clone())
        };
        if name(was) != name(monitor) {
            changes.push(Change::WorkspaceFocus {
                monitor_id: monitor.id,
                from: name(was),
                to: name(monitor),
            });
        }

        for workspace in &monitor.workspaces {
            let Some(old) = was.workspaces.iter().find(|old| old.name == workspace.name) else {
                continue;
            };

            if old.layout != workspace.layout {
                changes.push(Change::Layout {
                    monitor_id: monitor.id,
                    workspace: workspace.name.clone(),
                    from: old.layout.clone(),
                    to: workspace.layout.clone(),
                });
            }

            let order = |workspace: &super::state::Workspace| -> Vec<u64> {
                workspace
                    .containers
                    .iter()
                    .filter_map(|container| container.windows.first().map(|window| window.id))
                    .collect()
            };
            let (old_order, new_order) = (order(old), order(workspace));
            let mut old_sorted = old_order.clone();
            let mut new_sorted = new_order.clone();
            old_sorted.sort_unstable();
            new_sorted.sort_unstable();
            // the same containers, in another order: a reorder rather than adds and removes
            if old_order != new_order && old_sorted == new_sorted {
                changes.push(Change::ContainerOrder {
                    monitor_id: monitor.id,
                    workspace: workspace.name.clone(),
                    from: old_order,
                    to: new_order,
                });
            }
        }
    }

    let (focus_before, focus_after) = (before.focus(), after.focus());
    if focus_before != focus_after {
        changes.push(Change::Focus {
            from: focus_before,
            to: focus_after,
        });
    }

    let (was, now) = (anchor(before), anchor(after));
    for (id, (monitor, workspace, slot, location, window)) in &now {
        match was.get(id) {
            None => changes.push(Change::WindowAdded {
                window: WindowRef::of(window),
                at: location.clone(),
            }),
            Some((old_monitor, old_workspace, old_slot, old_location, old_window)) => {
                if (old_monitor, old_workspace, old_slot) != (monitor, workspace, slot) {
                    changes.push(Change::WindowMoved {
                        window: WindowRef::of(window),
                        from: old_location.clone(),
                        to: location.clone(),
                    });
                }

                if let (Some(from), Some(to)) = (&old_window.rect, &window.rect)
                    && from.max_delta(to) > RECT_TOLERANCE
                {
                    changes.push(Change::WindowRect {
                        window: WindowRef::of(window),
                        from: *from,
                        to: *to,
                        hidden_before: is_hidden(before, old_window),
                        hidden_after: is_hidden(after, window),
                    });
                }
            }
        }
    }
    for (id, (_, _, _, location, window)) in &was {
        if !now.contains_key(id) {
            changes.push(Change::WindowRemoved {
                window: WindowRef::of(window),
                from: location.clone(),
            });
        }
    }

    changes
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::Value;
    use serde_json::json;

    fn window(id: u64, rect: Option<[i64; 4]>) -> Value {
        let mut value = json!({"id": id, "details": {"title": format!("w{id}"), "exe": "app"}});
        if let Some([left, top, width, height]) = rect {
            value["rect"] = json!({"left": left, "top": top, "right": width, "bottom": height});
        }
        value
    }

    fn container(id: &str, windows: Vec<Value>) -> Value {
        json!({"id": id, "windows": {"elements": windows, "focused": 0}})
    }

    fn state(workspaces: Vec<(&str, Vec<Value>, usize)>, focused_workspace: usize) -> Snapshot {
        let workspaces: Vec<Value> = workspaces
            .into_iter()
            .map(|(name, containers, focused)| {
                json!({"name": name, "layer": "Tiling", "layout": {"Default": "Columns"},
                    "containers": {"elements": containers, "focused": focused}})
            })
            .collect();
        Snapshot::from_value(&json!({
            "monitors": {"elements": [{
                "id": 1, "device": "d",
                "size": {"left": 0, "top": 0, "right": 1000, "bottom": 800},
                "window_hiding_position": "BottomLeft",
                "workspaces": {"elements": workspaces, "focused": focused_workspace},
            }], "focused": 0}
        }))
    }

    fn kinds(changes: &[Change]) -> Vec<&'static str> {
        changes.iter().map(Change::kind).collect()
    }

    #[test]
    fn identical_states_have_no_changes() {
        let a = state(
            vec![(
                "one",
                vec![container("a", vec![window(1, Some([0, 0, 500, 800]))])],
                0,
            )],
            0,
        );
        assert!(diff(&a, &a.clone()).is_empty());
    }

    #[test]
    fn a_window_appearing_and_focus_moving_are_described_as_such() {
        let before = state(
            vec![("one", vec![container("a", vec![window(1, None)])], 0)],
            0,
        );
        let after = state(
            vec![(
                "one",
                vec![
                    container("a", vec![window(1, None)]),
                    container("b", vec![window(2, None)]),
                ],
                1,
            )],
            0,
        );

        let changes = diff(&before, &after);
        assert_eq!(kinds(&changes), ["focus", "window-added"]);
        let Change::WindowAdded { window, at } = &changes[1] else {
            panic!("{changes:?}")
        };
        assert_eq!(window.id, 2);
        assert_eq!(at.workspace, "one");
        assert!(changes[1].describe().contains("w2"));
    }

    #[test]
    fn a_window_leaving_a_workspace_is_a_move_not_an_add_and_remove() {
        let before = state(
            vec![
                ("one", vec![container("a", vec![window(1, None)])], 0),
                ("two", vec![], 0),
            ],
            0,
        );
        let after = state(
            vec![
                ("one", vec![], 0),
                ("two", vec![container("a", vec![window(1, None)])], 0),
            ],
            0,
        );

        let changes = diff(&before, &after);
        assert!(kinds(&changes).contains(&"window-moved"), "{changes:?}");
        assert!(!kinds(&changes).contains(&"window-added"));
        assert!(!kinds(&changes).contains(&"window-removed"));
    }

    #[test]
    fn container_indexes_shifting_is_not_a_move() {
        let before = state(
            vec![(
                "one",
                vec![
                    container("a", vec![window(1, None)]),
                    container("b", vec![window(2, None)]),
                ],
                0,
            )],
            0,
        );
        // container "a" is closed: "b" moves from index 1 to 0 but is not moved
        let after = state(
            vec![("one", vec![container("b", vec![window(2, None)])], 0)],
            0,
        );

        assert_eq!(kinds(&diff(&before, &after)), ["focus", "window-removed"]);
    }

    #[test]
    fn reordered_containers_are_one_change() {
        let a = container("a", vec![window(1, None)]);
        let b = container("b", vec![window(2, None)]);
        let before = state(vec![("one", vec![a.clone(), b.clone()], 0)], 0);
        let after = state(vec![("one", vec![b, a], 0)], 0);

        let changes = diff(&before, &after);
        assert!(kinds(&changes).contains(&"container-order"), "{changes:?}");
        assert!(!kinds(&changes).contains(&"window-moved"));
    }

    #[test]
    fn a_window_really_moving_on_screen_is_reported_with_hiding() {
        let on = window(1, Some([0, 0, 500, 800]));
        // hidden at the bottom-left sliver: right edge one point inside the monitor
        let hidden = window(1, Some([-499, 790, 500, 800]));
        let before = state(vec![("one", vec![container("a", vec![on])], 0)], 0);
        let after = state(vec![("one", vec![container("a", vec![hidden])], 0)], 0);

        let changes = diff(&before, &after);
        let Change::WindowRect {
            hidden_before,
            hidden_after,
            ..
        } = &changes[0]
        else {
            panic!("{changes:?}")
        };
        assert!(!hidden_before && *hidden_after);
        assert!(changes[0].describe().contains("now hidden"));
    }

    #[test]
    fn rect_jitter_within_the_tolerance_is_not_a_change() {
        let before = state(
            vec![(
                "one",
                vec![container("a", vec![window(1, Some([0, 0, 500, 800]))])],
                0,
            )],
            0,
        );
        let after = state(
            vec![(
                "one",
                vec![container("a", vec![window(1, Some([1, 0, 499, 800]))])],
                0,
            )],
            0,
        );
        assert!(diff(&before, &after).is_empty());
    }

    #[test]
    fn workspace_switches_and_monitor_changes_are_reported() {
        let before = state(vec![("one", vec![], 0), ("two", vec![], 0)], 0);
        let after = state(vec![("one", vec![], 0), ("two", vec![], 0)], 1);
        let changes = diff(&before, &after);
        assert!(matches!(
            &changes[0],
            Change::WorkspaceFocus { from, to, .. } if from == "one" && to == "two"
        ));

        let docked = Snapshot::from_value(&json!({
            "monitors": {"elements": [
                {"id": 1, "device": "a", "size": {"left": 0, "top": 0, "right": 10, "bottom": 10}},
                {"id": 2, "device": "b", "size": {"left": 10, "top": 0, "right": 10, "bottom": 10}},
            ], "focused": 0}
        }));
        let undocked = Snapshot::from_value(&json!({
            "monitors": {"elements": [
                {"id": 1, "device": "a", "size": {"left": 0, "top": 0, "right": 20, "bottom": 10}},
            ], "focused": 0}
        }));
        let changes = diff(&docked, &undocked);
        assert_eq!(kinds(&changes), ["monitor-resized", "monitor-removed"]);
        assert_eq!(
            kinds(&diff(&undocked, &docked)),
            ["monitor-resized", "monitor-added"]
        );
    }

    #[test]
    fn pausing_is_reported() {
        let mut paused = state(vec![], 0);
        paused.paused = true;
        let changes = diff(&state(vec![], 0), &paused);
        assert_eq!(changes, [Change::Paused { paused: true }]);
        assert_eq!(changes[0].describe(), "komorebi paused");
    }
}
