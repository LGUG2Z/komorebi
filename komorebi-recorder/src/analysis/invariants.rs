//! Things that should never be true of komorebi's state, and what true state looks like.
//!
//! These are checks on one state taken on its own, so they can be run at any moment of a
//! recording. A violation is evidence, not a verdict: komorebi may be mid-way through a
//! transition, or an application may refuse a size komorebi asked for. Each says what it saw and
//! how sure it is by severity.
//!
//! There are two entry points, which look at different things:
//!
//! - [`check`] reads a [`Snapshot`] alone: the state komorebi reported, including where each
//!   window really was when it was serialized.
//! - `check_probed` compares a snapshot with what Windows reported about one window just after
//!   an event about it (`record --probe-windows`).
//!
//! `state`, `diff`, `trace` and `brief` call [`check`] on the states they show, and `overview`
//! calls both to find the states that were wrong and for how long.
//!
//! # The checks
//!
//! A violation's [`kind`](Violation::kind) is a stable name. Which flavor of komorebi a check
//! applies to is decided by how that komorebi hides a window: komorebi for Mac parks it in a corner
//! of its monitor, so a window's position shows whether it is hidden; komorebi for Windows cloaks
//! it where it is, so no position does (see [`Snapshot::hides_windows_by_position`]).
//!
//! | kind | severity | what it reports | komorebi for Mac | komorebi for Windows |
//! |---|---|---|---|---|
//! | `window-in-two-places` | error | one window id is in more than one place | [`check`] | [`check`] |
//! | `duplicate-monitor-id` | error | two monitors have the same id | [`check`] | [`check`] |
//! | `focus-out-of-range` | error | a monitor focuses a workspace, or a workspace a container, past the end of its list | [`check`] | [`check`] |
//! | `visible-window-hidden` | error | a window that should be on screen is hidden | [`check`]: parked in the hiding position | `check_probed`: Windows has it cloaked, minimized or not visible |
//! | `monitor-overlap` | warn | two monitors' areas overlap | [`check`] | [`check`] |
//! | `empty-container` | warn | a container holds no windows | [`check`] | [`check`] |
//! | `off-screen` | warn | a window on a showing workspace is where no monitor is | [`check`] | [`check`] |
//! | `layout-mismatch` | warn | a tiled window is not where the last layout put it | [`check`] | [`check`] |
//! | `hidden-window-visible` | warn | a window that should be hidden is on screen | [`check`]: not parked, on a workspace that is not showing | `check_probed`: Windows has it on screen |
//! | `unmanaged-window` | info | a window is on screen and komorebi does not have it | never | `check_probed` |
//!
//! So on a komorebi for Windows state [`check`] never reports `visible-window-hidden` or
//! `hidden-window-visible`, and on a komorebi for Mac state `check_probed` reports nothing: the
//! probe exists only on Windows. Without a probe, nothing can say whether a Windows window is
//! really hidden.

use super::state::Location;
use super::state::Monitor;
use super::state::Place;
use super::state::Rect;
use super::state::Snapshot;
use super::state::Window;
use crate::sources::winprobe::WindowFacts;
use schemars::JsonSchema;
use serde::Serialize;
use std::collections::BTreeMap;

/// How far a window's real rect may be from where the layout put it before it is reported. An
/// application with a minimum size (a terminal snapping to whole rows) is a few points off.
const LAYOUT_TOLERANCE: i64 = 4;

/// A window on a workspace that is not showing, with more than this share of itself on a display,
/// is leaking through.
const LEAK_SHARE_PERCENT: i64 = 25;

/// How serious a [`Violation`] is, and how sure it is that something is wrong.
///
/// The variants are ordered from most to least serious (`Error < Warn < Info`), which is the order
/// [`check`] sorts its result in.
///
/// # Example
///
/// ```rust
/// use komorebi_recorder::analysis::invariants::Severity;
///
/// assert!(Severity::Error < Severity::Warn);
/// assert!(Severity::Warn < Severity::Info);
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, JsonSchema)]
#[serde(rename_all = "kebab-case")]
pub enum Severity {
    /// The state is wrong in a way that a person would see or that komorebi's bookkeeping should
    /// not allow: a window in two places, a window that should be showing and is not.
    Error,
    /// The state is probably wrong, or is wrong in a way that may be an application's doing: a
    /// tiled window off from its layout, a window where no monitor is.
    Warn,
    /// Worth knowing and not a fault in itself: a window komorebi does not manage.
    Info,
}

/// One thing wrong with a state, with the windows and place it is about.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, JsonSchema)]
pub struct Violation {
    /// Stable name of the check, e.g. `visible-window-hidden`. The kinds are listed in the
    /// [module](self) documentation.
    pub kind: String,
    /// How serious it is.
    pub severity: Severity,
    /// A sentence saying what was seen, naming the windows and where they are. Two violations are
    /// equal only if their messages are, so a problem whose numbers change reads as a different
    /// violation.
    pub message: String,
    /// The ids of the windows it is about; empty for a problem with monitors or workspaces.
    pub window_ids: Vec<u64>,
    /// The display id of the monitor it is about, if it is about one.
    pub monitor_id: Option<u64>,
    /// The name of the workspace it is about, if it is about one.
    pub workspace: Option<String>,
}

fn violation(
    kind: &str,
    severity: Severity,
    message: String,
    window_ids: Vec<u64>,
    location: Option<&Location>,
) -> Violation {
    Violation {
        kind: kind.to_string(),
        severity,
        message,
        window_ids,
        monitor_id: location.map(|location| location.monitor_id),
        workspace: location.map(|location| location.workspace.clone()),
    }
}

/// `found` with the monitor and workspace it is about filled in.
fn about(mut found: Violation, monitor_id: u64, workspace: Option<&str>) -> Violation {
    found.monitor_id = Some(monitor_id);
    found.workspace = workspace.map(str::to_string);
    found
}

fn name(window: &Window) -> String {
    format!("{} \"{}\" (window {})", window.exe, window.title, window.id)
}

/// Whether komorebi's state says a window should be on screen: `Some(true)` for the window in front
/// on a workspace that is showing, `Some(false)` for one that should be hidden (another workspace,
/// a tab behind the front one, something else filling the workspace), `None` where the state does
/// not say (a floating window).
fn should_be_shown(snapshot: &Snapshot, location: &Location) -> Option<bool> {
    if !location.workspace_visible {
        return Some(false);
    }

    let workspace = snapshot
        .monitors
        .iter()
        .find(|monitor| monitor.id == location.monitor_id)?
        .workspaces
        .iter()
        .find(|workspace| workspace.name == location.workspace)?;

    match location.place {
        Place::Container { index, tab } => {
            if workspace.monocle.is_some() || workspace.maximized.is_some() {
                // one window fills the workspace and the tiled ones are put away
                return Some(false);
            }
            Some(workspace.containers.get(index)?.focused == tab)
        }
        Place::Monocle { tab } => Some(workspace.monocle.as_ref()?.focused == tab),
        Place::Maximized => Some(true),
        Place::Floating { .. } => None,
    }
}

/// Checks what Windows reported about one window against komorebi's state after the event about it.
///
/// Only komorebi for Windows has this to check: it hides a window by cloaking it where it is, so
/// `State` cannot show whether a window really is hidden. `facts` is what the probe read just
/// after the event (see `WindowFacts` in `crate::sources::winprobe`), `snapshot` the state
/// komorebi sent with it, and `exe` the program the event named, used only to name a window
/// komorebi does not have. The result is empty if:
///
/// - the window no longer exists (`facts.exists` is `false`), since a destroyed window is nobody's
///   business;
/// - komorebi is paused, since it is not managing windows;
/// - `snapshot` is from komorebi for Mac, whose hidden windows are found by [`check`].
///
/// Otherwise the window, found in the state by its handle, is judged:
///
/// - `visible-window-hidden` (error): komorebi has the window in front on a workspace that is
///   showing, and Windows has it cloaked, minimized or not visible;
/// - `hidden-window-visible` (warn): komorebi has it put away (on a workspace that is not
///   showing, behind the front tab of its container, or behind a monocle or maximized window), and
///   Windows has it shown on a monitor;
/// - `unmanaged-window` (info): komorebi does not have it at all, and Windows has it shown. It is
///   not a fault in itself, since most dialogs and tooltips are rightly left alone, so it is
///   information, with komorebi's own explanation when the probe asked for one. Only the first
///   non-blank line of that explanation is quoted.
///
/// A window in a floating place is not judged either way, since the state does not say whether a
/// floating window should be showing.
///
/// A window komorebi is mid-way through moving can be caught in between: like every violation this
/// is evidence, and one that does not last is reported as the transition it probably was (see
/// `overview`). The function does not fail or panic.
///
/// # Example
///
/// ```rust
/// use komorebi_recorder::analysis::flavor::Flavor;
/// use komorebi_recorder::analysis::invariants::Severity;
/// use komorebi_recorder::analysis::invariants::check_probed;
/// use komorebi_recorder::analysis::state::Snapshot;
/// use komorebi_recorder::sources::winprobe::WindowFacts;
/// use komorebi_recorder::testkit::windows_state;
/// use komorebi_recorder::testkit::windows_window;
///
/// // komorebi has Chrome in front on the workspace that is showing ...
/// let state = windows_state(
///     65_537,
///     vec![vec![windows_window(131_072, "chrome.exe", Some([0, 0, 960, 1040]))]],
/// );
/// let snapshot = Snapshot::parse(Flavor::Windows, &state);
///
/// // ... and Windows says it is cloaked
/// let facts = WindowFacts {
///     hwnd: 131_072,
///     at_ns: 0,
///     exists: true,
///     visible: true,
///     cloaked: true,
///     minimized: false,
///     rect: Some([0, 0, 960, 1040]),
///     komorebi_says: None,
/// };
/// let found = check_probed(&snapshot, &facts, Some("chrome.exe"));
/// assert_eq!(found.len(), 1);
/// assert_eq!(found[0].kind, "visible-window-hidden");
/// assert_eq!(found[0].severity, Severity::Error);
/// assert!(found[0].message.contains("cloaked"));
///
/// // a visible window komorebi does not have is information
/// let stray = WindowFacts { hwnd: 999, cloaked: false, ..facts.clone() };
/// let found = check_probed(&snapshot, &stray, Some("dialog.exe"));
/// assert_eq!(found[0].kind, "unmanaged-window");
/// assert_eq!(found[0].severity, Severity::Info);
///
/// // a state from komorebi for Mac is not judged by what Windows says
/// let mac = Snapshot::parse(Flavor::Macos, &state);
/// assert!(check_probed(&mac, &facts, None).is_empty());
/// ```
///
/// Covered by the unit tests of `analysis::probed` and by
/// `tests/windows_flavor.rs::a_window_komorebi_shows_that_windows_cloaked_is_a_finding_in_the_overview`.
#[must_use]
pub fn check_probed(snapshot: &Snapshot, facts: &WindowFacts, exe: Option<&str>) -> Vec<Violation> {
    if !facts.exists || snapshot.paused || snapshot.hides_windows_by_position() {
        return Vec::new();
    }

    let id = u64::from(facts.hwnd);
    let named = |window: Option<&Window>| match (window, exe) {
        (Some(window), _) => name(window),
        (None, Some(exe)) => format!("{exe} window {id}"),
        (None, None) => format!("window {id}"),
    };

    let found = snapshot
        .windows()
        .into_iter()
        .find(|(_, window)| window.id == id);
    let Some((location, window)) = found else {
        if !facts.shown() {
            return Vec::new();
        }

        let why = facts
            .komorebi_says
            .as_deref()
            .and_then(|says| says.lines().map(str::trim).find(|line| !line.is_empty()))
            .map_or_else(String::new, |line| format!("; komorebi says: {line}"));
        return vec![violation(
            "unmanaged-window",
            Severity::Info,
            format!(
                "{} is on screen but komorebi does not manage it{why}",
                named(None)
            ),
            vec![id],
            None,
        )];
    };

    match should_be_shown(snapshot, &location) {
        Some(true) => facts
            .hidden_how()
            .map(|how| {
                violation(
                    "visible-window-hidden",
                    Severity::Error,
                    format!(
                        "{} is in front on {}, which is showing, but Windows has it {how}",
                        named(Some(window)),
                        location.describe()
                    ),
                    vec![id],
                    Some(&location),
                )
            })
            .into_iter()
            .collect(),
        Some(false) if facts.shown() => {
            let on_screen = facts.rect.is_none_or(|[left, top, right, bottom]| {
                let rect = Rect {
                    left: i64::from(left),
                    top: i64::from(top),
                    width: i64::from(right - left),
                    height: i64::from(bottom - top),
                };
                snapshot
                    .monitors
                    .iter()
                    .any(|monitor| monitor.size.overlap_area(&rect) > 0)
            });
            if !on_screen {
                return Vec::new();
            }

            vec![violation(
                "hidden-window-visible",
                Severity::Warn,
                format!(
                    "{} is on {}, which is not showing its windows, but Windows has it on screen",
                    named(Some(window)),
                    location.describe()
                ),
                vec![id],
                Some(&location),
            )]
        }
        _ => Vec::new(),
    }
}

/// Checks one state and returns every violation found, most serious first.
///
/// The result is sorted by severity and then by kind, so errors come before warnings, and is empty
/// for a state with nothing wrong. Which checks apply to which flavor is in the [module](self)
/// documentation: on a komorebi for Windows state the position-based checks of hidden windows do
/// not run, because that state cannot show whether a window is hidden (use `check_probed`
/// for that).
///
/// What is checked:
///
/// - the same window in more than one place;
/// - two monitors with one id, or whose areas overlap;
/// - a focus index past the end of what it indexes, for monitors and for workspaces;
/// - a container with no windows;
/// - for every window that has a rect, on a workspace that is showing: parked in komorebi's hiding
///   position (`visible-window-hidden`, in any kind of place), else on no monitor
///   (`off-screen`);
/// - for every window on a workspace that is not showing, where windows are hidden by position:
///   not parked, and with at least 25% of its area on its monitor (`hidden-window-visible`);
/// - for the focused workspace of each monitor, if it is tiling and not monocle, maximized or on
///   the floating layer: a focused window more than 4 points from where the last layout put its
///   container, in any of left, top, width and height (`layout-mismatch`). The few points of slack
///   are for an application with a minimum size, such as a terminal snapping to whole rows.
///
/// A window with no rect, or whose monitor is not in the state, is skipped by the position checks.
/// The function does not fail or panic.
///
/// # Example
///
/// ```rust
/// use komorebi_recorder::analysis::flavor::Flavor;
/// use komorebi_recorder::analysis::invariants::Severity;
/// use komorebi_recorder::analysis::invariants::check;
/// use komorebi_recorder::analysis::state::Snapshot;
/// use komorebi_recorder::testkit::container;
/// use komorebi_recorder::testkit::monitor;
/// use komorebi_recorder::testkit::state;
/// use komorebi_recorder::testkit::window;
/// use komorebi_recorder::testkit::workspace;
///
/// // One window fills the only workspace of a 1000x800 monitor, but it is parked in the sliver at
/// // the bottom left, where komorebi for Mac puts a window it has hidden.
/// let layout = [[0, 0, 1000, 800]];
/// let parked = window(1, "Ghostty", Some([-999, 790, 1000, 800]));
/// let screen = monitor(
///     1,
///     [0, 0, 1000, 800],
///     vec![workspace("one", vec![container("a", vec![parked])], 0, &layout)],
///     0,
/// );
/// let snapshot = Snapshot::parse(Flavor::Macos, &state(vec![screen], 0));
///
/// let found = check(&snapshot);
/// assert_eq!(found.len(), 1);
/// assert_eq!(found[0].kind, "visible-window-hidden");
/// assert_eq!(found[0].severity, Severity::Error);
/// assert_eq!(found[0].window_ids, [1]);
/// assert_eq!(found[0].monitor_id, Some(1));
/// ```
///
/// Every kind that [`check`] reports has a unit test in this module, named after it
/// (`a_window_on_the_showing_workspace_parked_off_screen_is_an_error`,
/// `a_tiled_window_that_is_not_where_the_layout_put_it_is_a_mismatch`, ...).
#[must_use]
pub fn check(snapshot: &Snapshot) -> Vec<Violation> {
    let mut found = Vec::new();
    let windows = snapshot.windows();

    // the same window in two places
    let mut places: BTreeMap<u64, Vec<&Location>> = BTreeMap::new();
    for (location, window) in &windows {
        places.entry(window.id).or_default().push(location);
    }
    for (id, locations) in &places {
        if locations.len() > 1 {
            let described: Vec<String> = locations
                .iter()
                .map(|location| location.describe())
                .collect();
            found.push(violation(
                "window-in-two-places",
                Severity::Error,
                format!(
                    "window {id} is in {} places: {}",
                    locations.len(),
                    described.join("; ")
                ),
                vec![*id],
                locations.first().copied(),
            ));
        }
    }

    // monitors that overlap or share an id
    for (index, monitor) in snapshot.monitors.iter().enumerate() {
        for other in &snapshot.monitors[index + 1..] {
            if monitor.id == other.id {
                found.push(violation(
                    "duplicate-monitor-id",
                    Severity::Error,
                    format!("two monitors have the id {}", monitor.id),
                    vec![],
                    None,
                ));
            } else if monitor.size.overlap_area(&other.size) > 0 {
                found.push(violation(
                    "monitor-overlap",
                    Severity::Warn,
                    format!(
                        "monitor {} ({}) and monitor {} ({}) overlap",
                        monitor.id,
                        monitor.size.describe(),
                        other.id,
                        other.size.describe()
                    ),
                    vec![],
                    None,
                ));
            }
        }

        // a focus that points past the end of what it focuses
        if monitor.focused >= monitor.workspaces.len() && !monitor.workspaces.is_empty() {
            found.push(about(
                violation(
                    "focus-out-of-range",
                    Severity::Error,
                    format!(
                        "monitor {} focuses workspace {} of {}",
                        monitor.id,
                        monitor.focused,
                        monitor.workspaces.len()
                    ),
                    vec![],
                    None,
                ),
                monitor.id,
                None,
            ));
        }
        for workspace in &monitor.workspaces {
            if workspace.focused >= workspace.containers.len() && !workspace.containers.is_empty() {
                found.push(about(
                    violation(
                        "focus-out-of-range",
                        Severity::Error,
                        format!(
                            "monitor {} workspace {} focuses container {} of {}",
                            monitor.id,
                            workspace.name,
                            workspace.focused,
                            workspace.containers.len()
                        ),
                        vec![],
                        None,
                    ),
                    monitor.id,
                    Some(&workspace.name),
                ));
            }
            for container in &workspace.containers {
                if container.windows.is_empty() {
                    found.push(about(
                        violation(
                            "empty-container",
                            Severity::Warn,
                            format!(
                                "monitor {} workspace {} has an empty container {}",
                                monitor.id, workspace.name, container.id
                            ),
                            vec![],
                            None,
                        ),
                        monitor.id,
                        Some(&workspace.name),
                    ));
                }
            }
        }
    }

    let monitor_of = |id: u64| snapshot.monitors.iter().find(|monitor| monitor.id == id);

    for (location, window) in &windows {
        let (Some(rect), Some(monitor)) = (&window.rect, monitor_of(location.monitor_id)) else {
            continue;
        };
        let hidden = snapshot.is_hidden_position(rect, monitor);

        if location.workspace_visible {
            // on a workspace that is showing, so it should be on screen
            if hidden {
                found.push(violation(
                    "visible-window-hidden",
                    Severity::Error,
                    format!(
                        "{} is on {} which is showing, but it is parked off screen at {}",
                        name(window),
                        location.describe(),
                        rect.describe()
                    ),
                    vec![window.id],
                    Some(location),
                ));
            } else if snapshot
                .monitors
                .iter()
                .all(|monitor| monitor.size.overlap_area(rect) == 0)
            {
                found.push(violation(
                    "off-screen",
                    Severity::Warn,
                    format!(
                        "{} is on {} but at {} which is on no display",
                        name(window),
                        location.describe(),
                        rect.describe()
                    ),
                    vec![window.id],
                    Some(location),
                ));
            }
        } else if snapshot.hides_windows_by_position()
            && !hidden
            && rect.area() > 0
            && leaks(rect, monitor)
        {
            found.push(violation(
                "hidden-window-visible",
                Severity::Warn,
                format!(
                    "{} is on {} which is not showing, but it is on screen at {}",
                    name(window),
                    location.describe(),
                    rect.describe()
                ),
                vec![window.id],
                Some(location),
            ));
        }
    }

    // tiled windows that are not where the layout put them
    for monitor in &snapshot.monitors {
        let Some(workspace) = monitor.focused_workspace() else {
            continue;
        };
        if !workspace.tiling
            || workspace.layer == "Floating"
            || workspace.monocle.is_some()
            || workspace.maximized.is_some()
        {
            continue;
        }

        for (index, container) in workspace.containers.iter().enumerate() {
            let (Some(expected), Some(window)) = (
                workspace.layout_rects.get(index),
                container.focused_window(),
            ) else {
                continue;
            };
            let Some(actual) = &window.rect else {
                continue;
            };
            if snapshot.is_hidden_position(actual, monitor) {
                // reported as visible-window-hidden
                continue;
            }

            let delta = expected.max_delta(actual);
            if delta > LAYOUT_TOLERANCE {
                let location = Location {
                    monitor_id: monitor.id,
                    workspace: workspace.name.clone(),
                    place: Place::Container {
                        index,
                        tab: container.focused,
                    },
                    workspace_visible: true,
                };
                found.push(violation(
                    "layout-mismatch",
                    Severity::Warn,
                    format!(
                        "{} is at {} but the layout put container {index} at {} ({delta} points off)",
                        name(window),
                        actual.describe(),
                        expected.describe()
                    ),
                    vec![window.id],
                    Some(&location),
                ));
            }
        }
    }

    found.sort_by(|a, b| (a.severity, &a.kind).cmp(&(b.severity, &b.kind)));
    found
}

/// Whether enough of `rect` is on `monitor` for the window to be seen.
fn leaks(rect: &Rect, monitor: &Monitor) -> bool {
    let overlap = monitor.size.overlap_area(rect);
    rect.area() > 0 && overlap * 100 / rect.area() >= LEAK_SHARE_PERCENT
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testkit::container;
    use crate::testkit::monitor;
    use crate::testkit::state;
    use crate::testkit::window;
    use crate::testkit::workspace;

    fn snapshot(monitors: Vec<serde_json::Value>) -> Snapshot {
        Snapshot::from_value(&state(monitors, 0))
    }

    fn kinds(violations: &[Violation]) -> Vec<&str> {
        violations
            .iter()
            .map(|violation| violation.kind.as_str())
            .collect()
    }

    const SCREEN: [i64; 4] = [0, 0, 1000, 800];

    #[test]
    fn a_tidy_state_has_no_violations() {
        let layout = [[0, 0, 500, 800], [500, 0, 500, 800]];
        let state = snapshot(vec![monitor(
            1,
            SCREEN,
            vec![workspace(
                "one",
                vec![
                    container("a", vec![window(1, "x", Some(layout[0]))]),
                    container("b", vec![window(2, "y", Some(layout[1]))]),
                ],
                0,
                &layout,
            )],
            0,
        )]);

        assert_eq!(check(&state), []);
    }

    #[test]
    fn a_window_on_the_showing_workspace_parked_off_screen_is_an_error() {
        let layout = [[0, 0, 1000, 800]];
        // right edge one point inside the monitor: the bottom-left hiding sliver
        let state = snapshot(vec![monitor(
            1,
            SCREEN,
            vec![workspace(
                "one",
                vec![container(
                    "a",
                    vec![window(1, "x", Some([-999, 790, 1000, 800]))],
                )],
                0,
                &layout,
            )],
            0,
        )]);

        let found = check(&state);
        assert_eq!(kinds(&found), ["visible-window-hidden"]);
        assert_eq!(found[0].severity, Severity::Error);
        assert_eq!(found[0].window_ids, [1]);
        assert_eq!(found[0].monitor_id, Some(1));
    }

    #[test]
    fn a_window_on_a_workspace_that_is_not_showing_but_on_screen_is_leaking() {
        let state = snapshot(vec![monitor(
            1,
            SCREEN,
            vec![
                workspace("one", vec![], 0, &[]),
                workspace(
                    "two",
                    vec![container(
                        "a",
                        vec![window(1, "x", Some([100, 100, 500, 500]))],
                    )],
                    0,
                    &[],
                ),
            ],
            0,
        )]);

        let found = check(&state);
        assert_eq!(kinds(&found), ["hidden-window-visible"]);
        assert_eq!(found[0].workspace.as_deref(), Some("two"));
    }

    #[test]
    fn a_hidden_window_on_a_hidden_workspace_is_fine() {
        let state = snapshot(vec![monitor(
            1,
            SCREEN,
            vec![
                workspace("one", vec![], 0, &[]),
                workspace(
                    "two",
                    vec![container(
                        "a",
                        vec![window(1, "x", Some([-499, 790, 500, 800]))],
                    )],
                    0,
                    &[],
                ),
            ],
            0,
        )]);

        assert_eq!(check(&state), []);
    }

    #[test]
    fn a_tiled_window_that_is_not_where_the_layout_put_it_is_a_mismatch() {
        let layout = [[0, 0, 500, 800], [500, 0, 500, 800]];
        let state = snapshot(vec![monitor(
            1,
            SCREEN,
            vec![workspace(
                "one",
                vec![
                    container("a", vec![window(1, "x", Some(layout[0]))]),
                    // 120 points narrower than asked
                    container("b", vec![window(2, "y", Some([500, 0, 380, 800]))]),
                ],
                0,
                &layout,
            )],
            0,
        )]);

        let found = check(&state);
        assert_eq!(kinds(&found), ["layout-mismatch"]);
        assert_eq!(found[0].window_ids, [2]);
        assert!(
            found[0].message.contains("120 points off"),
            "{}",
            found[0].message
        );

        // a few points of slack is not a mismatch
        let close = snapshot(vec![monitor(
            1,
            SCREEN,
            vec![workspace(
                "one",
                vec![container(
                    "b",
                    vec![window(2, "y", Some([502, 0, 497, 800]))],
                )],
                0,
                &[[500, 0, 500, 800]],
            )],
            0,
        )]);
        assert_eq!(check(&close), []);
    }

    #[test]
    fn a_visible_window_on_no_display_is_off_screen() {
        let state = snapshot(vec![monitor(
            1,
            SCREEN,
            vec![workspace(
                "one",
                vec![container(
                    "a",
                    vec![window(1, "x", Some([5000, 5000, 500, 500]))],
                )],
                0,
                &[],
            )],
            0,
        )]);

        assert_eq!(kinds(&check(&state)), ["off-screen"]);
    }

    #[test]
    fn the_same_window_in_two_containers_is_an_error() {
        let state = snapshot(vec![monitor(
            1,
            SCREEN,
            vec![workspace(
                "one",
                vec![
                    container("a", vec![window(1, "x", None)]),
                    container("b", vec![window(1, "x", None)]),
                ],
                0,
                &[],
            )],
            0,
        )]);

        let found = check(&state);
        assert_eq!(kinds(&found), ["window-in-two-places"]);
        assert!(found[0].message.contains("2 places"));
    }

    #[test]
    fn overlapping_monitors_and_broken_focus_are_reported() {
        let overlapping = snapshot(vec![
            monitor(1, [0, 0, 1000, 800], vec![], 0),
            monitor(2, [500, 0, 1000, 800], vec![], 0),
        ]);
        assert_eq!(kinds(&check(&overlapping)), ["monitor-overlap"]);

        let duplicate = snapshot(vec![
            monitor(1, [0, 0, 100, 100], vec![], 0),
            monitor(1, [100, 0, 100, 100], vec![], 0),
        ]);
        assert_eq!(kinds(&check(&duplicate)), ["duplicate-monitor-id"]);

        let broken = snapshot(vec![monitor(
            1,
            SCREEN,
            vec![workspace(
                "one",
                vec![container("a", vec![window(1, "x", None)])],
                5,
                &[],
            )],
            3,
        )]);
        assert_eq!(
            kinds(&check(&broken)),
            ["focus-out-of-range", "focus-out-of-range"]
        );
    }

    #[test]
    fn errors_sort_before_warnings() {
        let state = snapshot(vec![monitor(
            1,
            SCREEN,
            vec![workspace(
                "one",
                vec![
                    container("a", vec![]),
                    container("b", vec![window(1, "x", Some([-999, 790, 1000, 800]))]),
                ],
                0,
                &[],
            )],
            0,
        )]);

        let found = check(&state);
        assert_eq!(kinds(&found), ["visible-window-hidden", "empty-container"]);
    }

    #[test]
    fn a_real_state_is_checked_without_inventing_problems() {
        let value: serde_json::Value =
            serde_json::from_str(include_str!("../../tests/fixtures/state.json")).expect("fixture");
        let found = check(&Snapshot::from_value(&value));
        eprintln!("{found:#?}");

        // every window on a workspace that is not showing is parked, and the showing ones sit
        // where the layout put them
        assert!(
            found
                .iter()
                .all(|violation| violation.severity != Severity::Error),
            "{found:#?}"
        );
    }
}
