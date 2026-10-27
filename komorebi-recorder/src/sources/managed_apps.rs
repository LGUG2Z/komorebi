//! Finding out which processes komorebi is managing, so each one can get its own OS trace.
//!
//! komorebi's `State` JSON deliberately carries no process id: a `Window` serializes only its
//! `id`, `rect` and `details { title, exe, role, subrole, icon_path }`. So the recorder walks the
//! state generically for window ids and resolves id -> owning pid itself with the OS window list
//! (see [`crate::platform::apps`]; on macOS `CGWindowListCopyWindowInfo(OptionAll, ...)`, where
//! `OptionAll` matters because komorebi's own wrapper only lists on-screen windows, which would
//! miss everything on other workspaces and spaces).
//!
//! The steps, each a pure function except the window list:
//!
//! 1. [`collect_windows`] reads the windows out of a `State`.
//! 2. [`window_owners`] asks the OS which process owns each window id.
//! 3. [`resolve`] joins the two into [`ManagedApp`]s.
//! 4. An [`AppTracker`] folds what is found over the whole recording and reports each process once,
//!    so the recorder starts one OS-trace stream per process.
//!
//! [`event_pid`] short-cuts this for a new process: a window manager event names the process id,
//! so the recorder need not wait for the next `State`. [`gui_app_pids`] finds every application
//! with a window, for the mode that traces all of them rather than only the managed ones.
//!
//! # Host and flavor
//!
//! The shapes read here are komorebi for Mac's: a window is an object with a numeric `id` and a
//! `details` object, and a window manager event carries the process id as `content[1]`. A
//! komorebi for Windows `State` holds windows as `{hwnd, title, exe, class, rect}` and its events
//! carry a window object instead of a process id, so on a Windows session [`collect_windows`] finds
//! no windows and [`event_pid`] returns `None`. Per-process traces exist only on macOS (the Windows
//! OS trace is one stream for every window, see `wintrace`), so nothing needs the result there.

use crate::clock;
use serde::Deserialize;
use serde::Serialize;
use serde_json::Value;
use std::collections::BTreeMap;
use std::collections::BTreeSet;
use std::collections::HashMap;
use std::collections::btree_map::Entry;

/// A window found in a komorebi `State`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StateWindow {
    /// The window's `id` in the `State`: the same number the window server uses for it on a Mac.
    pub id: u32,
    /// `details.exe`: the application's name as komorebi reports it (`Slack`). `None` if the
    /// window has none.
    pub exe: Option<String>,
    /// `details.title`. `None` if the window has none.
    pub title: Option<String>,
}

/// Collects every window in a `State` without depending on its exact nesting.
///
/// Any object with a numeric `id` and a `details` object counts. Tiled containers, the monocle
/// container, a maximized window and floating windows are all found, and so are layouts that do
/// not exist yet.
///
/// Each window is returned once, in ascending order of `id`; a window that appears twice keeps the
/// first reading. An object whose `id` is not an integer that fits in `u32`, or that has no
/// `details` object, is not a window.
///
/// # Example
///
/// ```rust
/// use komorebi_recorder::sources::managed_apps::collect_windows;
///
/// let state: serde_json::Value = serde_json::from_str(r#"{
///     "layout": {"deeply": {"nested": [
///         {"id": 1, "details": {"exe": "A", "title": "one"}},
///         {"monocle_container": {"windows": {"elements": [{"id": 2, "details": {"exe": "B"}}]}}},
///         {"maximized_window": {"id": 3, "details": {"exe": "C"}}},
///         {"id": "not-a-window-id", "details": {"exe": "D"}},
///         {"id": 4, "no_details": true}
///     ]}},
///     "floating_windows": {"elements": [{"id": 5, "details": {"exe": "E"}}]}
/// }"#)?;
///
/// let windows = collect_windows(&state);
/// let ids: Vec<u32> = windows.iter().map(|window| window.id).collect();
/// assert_eq!(ids, [1, 2, 3, 5]);
/// assert_eq!(windows[0].exe.as_deref(), Some("A"));
/// assert_eq!(windows[0].title.as_deref(), Some("one"));
/// # Ok::<(), Box<dyn std::error::Error>>(())
/// ```
#[must_use]
pub fn collect_windows(state: &Value) -> Vec<StateWindow> {
    fn walk(value: &Value, out: &mut BTreeMap<u32, StateWindow>) {
        match value {
            Value::Object(map) => {
                if let (Some(id), Some(Value::Object(details))) =
                    (map.get("id").and_then(Value::as_u64), map.get("details"))
                    && let Ok(id) = u32::try_from(id)
                {
                    let text =
                        |key: &str| details.get(key).and_then(Value::as_str).map(str::to_string);

                    out.entry(id).or_insert_with(|| StateWindow {
                        id,
                        exe: text("exe"),
                        title: text("title"),
                    });
                }

                for child in map.values() {
                    walk(child, out);
                }
            }
            Value::Array(items) => {
                for item in items {
                    walk(item, out);
                }
            }
            _ => {}
        }
    }

    let mut windows = BTreeMap::new();
    walk(state, &mut windows);
    windows.into_values().collect()
}

/// The process that owns a window, as reported by the OS.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WindowOwner {
    /// The owning process id. [`gui_app_pids`] and [`resolve`] ignore a pid that is not positive.
    pub pid: i32,
    /// The owning application's name as the OS reports it: the window server's owner name on a
    /// Mac, the program's file name without `.exe` on Windows. Empty if the OS gave none.
    pub name: String,
    /// The window's layer: `0` for an ordinary application window, higher for things like the menu
    /// bar and the Dock. Always `0` on Windows, which reports none.
    pub layer: i64,
}

/// Returns every window the OS knows about (all spaces or virtual desktops, visible or not),
/// keyed by window id.
///
/// This asks the OS each time it is called, through [`crate::platform::apps::window_owners`]; see
/// there for how the hosts differ. The map is empty if the OS cannot be asked.
///
/// This depends on the machine it runs on, so it has no doctest. It is covered by
/// `window_server_listing_works` in this file's unit tests, which skips itself where no window
/// server is reachable.
#[must_use]
pub fn window_owners() -> HashMap<u32, WindowOwner> {
    crate::platform::apps::window_owners()
}

/// Returns every process that owns at least one ordinary (layer 0) window, i.e. the apps a user
/// can see.
///
/// Each is returned with the owner name of one of its windows. Processes with a pid that is not
/// positive are left out.
///
/// # Example
///
/// ```rust
/// use komorebi_recorder::sources::managed_apps::{WindowOwner, gui_app_pids};
/// use std::collections::HashMap;
///
/// let owners = HashMap::from([
///     (1, WindowOwner { pid: 10, name: "App".into(), layer: 0 }),
///     (2, WindowOwner { pid: 20, name: "Dock".into(), layer: 20 }),
/// ]);
///
/// let apps = gui_app_pids(&owners);
/// assert_eq!(apps.len(), 1);
/// assert_eq!(apps.get(&10).map(String::as_str), Some("App"));
/// ```
#[must_use]
pub fn gui_app_pids(owners: &HashMap<u32, WindowOwner>) -> BTreeMap<i32, String> {
    owners
        .values()
        .filter(|owner| owner.layer == 0 && owner.pid > 0)
        .map(|owner| (owner.pid, owner.name.clone()))
        .collect()
}

/// How a window's owning process was found. Serialized in the manifest as `window_id` or
/// `owner_name`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Resolution {
    /// The window id was found in the window server's list.
    WindowId,
    /// The window id was not listed, but exactly one process has a matching owner name.
    OwnerName,
}

/// A process komorebi manages windows of.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ManagedApp {
    /// The process id, always positive.
    pub pid: i32,
    /// The process's name: the owner name the OS gave it.
    pub name: String,
    /// The ids of this process's windows in the `State`.
    pub window_ids: BTreeSet<u32>,
    /// How the process was found, for the first of its windows that was resolved.
    pub resolution: Resolution,
}

/// The outcome of resolving a state's windows to processes.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Resolved {
    /// The processes found, one entry each, in ascending order of process id.
    pub apps: Vec<ManagedApp>,
    /// Windows in the state that could not be tied to any process.
    pub unresolved: Vec<StateWindow>,
}

/// Resolves state windows to owning processes.
///
/// A window is resolved by its id when `owners` has it. Otherwise, if it has an `exe`, it is
/// resolved by name, but only when exactly one process in `owners` has that owner name: an
/// ambiguous or unknown name leaves the window unresolved. Windows of one process are gathered
/// into one [`ManagedApp`].
///
/// # Example
///
/// ```rust
/// use komorebi_recorder::sources::managed_apps::{Resolution, StateWindow, WindowOwner, resolve};
/// use std::collections::HashMap;
///
/// let owner = |pid, name: &str| WindowOwner { pid, name: name.into(), layer: 0 };
/// let owners = HashMap::from([
///     (1, owner(100, "Ghostty")),
///     (2, owner(200, "Dup")),
///     (3, owner(300, "Dup")),
/// ]);
/// let window = |id, exe: &str| StateWindow { id, exe: Some(exe.into()), title: None };
///
/// let resolved = resolve(
///     &[window(1, "Ghostty"), window(99, "Ghostty"), window(98, "Dup"), window(97, "Nope")],
///     &owners,
/// );
///
/// // window 1 is listed; 99 is not, but only one process is called Ghostty
/// assert_eq!(resolved.apps.len(), 1);
/// assert_eq!(resolved.apps[0].pid, 100);
/// assert_eq!(resolved.apps[0].resolution, Resolution::WindowId);
/// assert_eq!(resolved.apps[0].window_ids.iter().copied().collect::<Vec<_>>(), [1, 99]);
///
/// // two processes are called Dup, and nobody is called Nope
/// let unresolved: Vec<u32> = resolved.unresolved.iter().map(|window| window.id).collect();
/// assert_eq!(unresolved, [98, 97]);
/// ```
#[must_use]
pub fn resolve(windows: &[StateWindow], owners: &HashMap<u32, WindowOwner>) -> Resolved {
    let mut by_name: HashMap<&str, BTreeSet<i32>> = HashMap::new();
    for owner in owners.values() {
        by_name
            .entry(owner.name.as_str())
            .or_default()
            .insert(owner.pid);
    }

    let mut apps: BTreeMap<i32, ManagedApp> = BTreeMap::new();
    let mut unresolved = Vec::new();

    for window in windows {
        let found = owners
            .get(&window.id)
            .map(|owner| (owner.pid, owner.name.clone(), Resolution::WindowId))
            .or_else(|| {
                let exe = window.exe.as_deref()?;
                let pids = by_name.get(exe)?;
                // only trust a name match when it is unambiguous
                (pids.len() == 1).then(|| {
                    (
                        *pids.iter().next().unwrap_or(&0),
                        exe.to_string(),
                        Resolution::OwnerName,
                    )
                })
            });

        match found {
            Some((pid, name, resolution)) if pid > 0 => {
                apps.entry(pid)
                    .or_insert_with(|| ManagedApp {
                        pid,
                        name,
                        window_ids: BTreeSet::new(),
                        resolution,
                    })
                    .window_ids
                    .insert(window.id);
            }
            _ => unresolved.push(window.clone()),
        }
    }

    Resolved {
        apps: apps.into_values().collect(),
        unresolved,
    }
}

const WINDOW_MANAGER_EVENTS: &[&str] = &[
    "FocusChange",
    "Show",
    "Destroy",
    "Minimize",
    "Restore",
    "Manage",
    "Unmanage",
    "MoveStart",
    "MoveEnd",
    "ResizeStart",
    "ResizeEnd",
];

/// Returns the process id carried by a window manager event notification (`content[1]`), if the
/// event is one. Used to notice a new process with no polling delay.
///
/// `event` is the `event` value of a notification (see
/// [`EventRow::event`](crate::sources::komorebi_events::EventRow::event)). The result is `None`
/// unless the event's `type` is one of `FocusChange`, `Show`, `Destroy`, `Minimize`, `Restore`,
/// `Manage`, `Unmanage`, `MoveStart`, `MoveEnd`, `ResizeStart` and `ResizeEnd` and `content[1]` is
/// a positive integer that fits in `i32`: commands and monitor events, and the events a Mac sends
/// with no real process (a negative id), have none.
///
/// # Example
///
/// ```rust
/// use komorebi_recorder::sources::managed_apps::event_pid;
/// use serde_json::json;
///
/// let focus = json!({"type": "FocusChange", "content": [{"type": "Accessibility", "content": "AXMainWindowChanged"}, 20934, 3395]});
/// assert_eq!(event_pid(&focus), Some(20934));
///
/// // a command is not a window manager event
/// assert_eq!(event_pid(&json!({"type": "FocusWindow", "content": "Left"})), None);
/// // a negative process id is not a process
/// assert_eq!(event_pid(&json!({"type": "Show", "content": [{"type": "AppKitWorkspace", "content": "x"}, -1]})), None);
/// ```
#[must_use]
pub fn event_pid(event: &Value) -> Option<i32> {
    let kind = event.get("type")?.as_str()?;
    if !WINDOW_MANAGER_EVENTS.contains(&kind) {
        return None;
    }

    let pid = event.get("content")?.as_array()?.get(1)?.as_i64()?;
    i32::try_from(pid).ok().filter(|pid| *pid > 0)
}

/// A managed process as tracked across a whole recording, as the manifest (`session.json`, key
/// `apps`) records it.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct AppRecord {
    /// The process id.
    pub pid: i32,
    /// The process's name (`Brave Beta`), as the recorder learned it. It also names the process's
    /// stream file.
    pub exe: String,
    /// The ids of the process's windows in komorebi's `State`, in the order they were first seen.
    pub window_ids: Vec<u32>,
    /// When the recorder first noticed this process, on the host's master monotonic clock in
    /// nanoseconds; its OS trace starts no earlier than this.
    pub first_seen_mach_ns: u64,
    /// How the process came to be traced: `state` (found in a `State`), `event` (named by a window
    /// manager event), `all-gui` (every application, `--oslog-apps all-gui`) or `explicit`
    /// (named by `--trace-process`).
    pub origin: String,
    /// How the process was resolved from a window of the `State`. `None` for a process that was
    /// not found through one of its windows.
    pub resolution: Option<Resolution>,
    /// The process's OS-trace file relative to the session directory
    /// (`oslog/apps/<exe>-<pid>.ndjson`). `None` if no stream was started for it.
    pub stream_file: Option<String>,
    /// Unified-log events captured for this process.
    #[serde(default)]
    pub events: u64,
    /// Bytes of unified-log records captured for this process.
    #[serde(default)]
    pub bytes: u64,
}

/// Tracks the distinct managed processes seen over a recording, and the windows that could not be
/// tied to a process.
///
/// # Example
///
/// ```rust
/// use komorebi_recorder::sources::managed_apps::{AppTracker, StateWindow, WindowOwner, resolve};
/// use std::collections::HashMap;
///
/// let owners = HashMap::from([
///     (1, WindowOwner { pid: 100, name: "A".into(), layer: 0 }),
///     (2, WindowOwner { pid: 100, name: "A".into(), layer: 0 }),
/// ]);
/// let window = |id| StateWindow { id, exe: None, title: None };
///
/// let mut tracker = AppTracker::default();
///
/// // the first time a process is seen, it is returned (the caller starts its stream) ...
/// let fresh = tracker.observe(resolve(&[window(1)], &owners), "state");
/// assert_eq!(fresh.len(), 1);
///
/// // ... and never again, though a new window of it is merged in
/// let fresh = tracker.observe(resolve(&[window(1), window(2)], &owners), "state");
/// assert!(fresh.is_empty());
/// assert_eq!(tracker.records()[0].window_ids, [1, 2]);
///
/// // a process named directly is tracked once too
/// assert!(tracker.observe_pid(300, "C".into(), "event").is_some());
/// assert!(tracker.observe_pid(300, "C".into(), "event").is_none());
/// ```
#[derive(Debug, Default)]
pub struct AppTracker {
    apps: BTreeMap<i32, AppRecord>,
    unresolved: BTreeMap<u32, StateWindow>,
}

impl AppTracker {
    /// Folds a resolution into the tracker, returning the records for processes seen for the
    /// first time (the caller spawns a stream for each).
    ///
    /// A process already tracked only gains any new window ids. Windows that could not be
    /// resolved are remembered until a later resolution ties them to a process. `origin` is stored
    /// as the [`AppRecord::origin`] of each new record.
    pub fn observe(&mut self, resolved: Resolved, origin: &str) -> Vec<AppRecord> {
        let mut fresh = Vec::new();

        for app in resolved.apps {
            match self.apps.entry(app.pid) {
                Entry::Vacant(slot) => {
                    let record = AppRecord {
                        pid: app.pid,
                        exe: app.name,
                        window_ids: app.window_ids.into_iter().collect(),
                        first_seen_mach_ns: clock::mono_now_ns(),
                        origin: origin.to_string(),
                        resolution: Some(app.resolution),
                        ..AppRecord::default()
                    };
                    fresh.push(record.clone());
                    slot.insert(record);
                }
                Entry::Occupied(mut slot) => {
                    let record = slot.get_mut();
                    for id in app.window_ids {
                        if !record.window_ids.contains(&id) {
                            record.window_ids.push(id);
                        }
                    }
                }
            }
        }

        for window in resolved.unresolved {
            self.unresolved.insert(window.id, window);
        }

        // a window that resolved later is no longer unresolved
        let known: BTreeSet<u32> = self
            .apps
            .values()
            .flat_map(|app| app.window_ids.iter().copied())
            .collect();
        self.unresolved.retain(|id, _| !known.contains(id));

        fresh
    }

    /// Tracks a process the recorder was told about directly (an event pid, a `--trace-process`
    /// name, `all-gui`).
    ///
    /// Returns the new record, or `None` if the process was already tracked. The record has no
    /// windows and no [`Resolution`].
    pub fn observe_pid(&mut self, pid: i32, name: String, origin: &str) -> Option<AppRecord> {
        if self.apps.contains_key(&pid) {
            return None;
        }

        let record = AppRecord {
            pid,
            exe: name,
            window_ids: Vec::new(),
            first_seen_mach_ns: clock::mono_now_ns(),
            origin: origin.to_string(),
            resolution: None,
            ..AppRecord::default()
        };
        self.apps.insert(pid, record.clone());
        Some(record)
    }

    /// Records the path of the OS-trace file started for process `pid`, relative to the session
    /// directory. Does nothing if the process is not tracked.
    pub fn set_stream_file(&mut self, pid: i32, file: &str) {
        if let Some(app) = self.apps.get_mut(&pid) {
            app.stream_file = Some(file.to_string());
        }
    }

    /// Returns `true` if and only if process `pid` is already tracked.
    #[must_use]
    pub fn contains(&self, pid: i32) -> bool {
        self.apps.contains_key(&pid)
    }

    /// Returns a copy of every tracked process's record, in ascending order of process id.
    #[must_use]
    pub fn records(&self) -> Vec<AppRecord> {
        self.apps.values().cloned().collect()
    }

    /// Returns the windows seen so far that no process has been tied to, in ascending order of
    /// window id.
    #[must_use]
    pub fn unresolved(&self) -> Vec<StateWindow> {
        self.unresolved.values().cloned().collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A real komorebi `State` (window titles sanitised), captured while reproducing a bug.
    const STATE_REPRO: &str = include_str!("../../tests/fixtures/state.json");

    fn state() -> Value {
        serde_json::from_str(STATE_REPRO).expect("the repro state parses")
    }

    #[test]
    fn walks_every_window_in_a_real_state() {
        let windows = collect_windows(&state());
        let ids: Vec<u32> = windows.iter().map(|window| window.id).collect();

        // spot-check ids from different workspaces
        for id in [59, 61, 8310, 107, 3395, 6293, 8550] {
            assert!(ids.contains(&id), "window {id} missing from {ids:?}");
        }

        let slack = windows
            .iter()
            .find(|window| window.id == 59)
            .expect("slack");
        assert_eq!(slack.exe.as_deref(), Some("Slack"));
        assert!(windows.len() >= 17, "found {}", windows.len());

        // ids are unique
        let unique: BTreeSet<u32> = ids.iter().copied().collect();
        assert_eq!(unique.len(), ids.len());
    }

    #[test]
    fn finds_windows_wherever_they_are_nested() {
        let state: Value = serde_json::from_str(
            r#"{
                "layout": {"deeply": {"nested": [
                    {"id": 1, "details": {"exe": "A", "title": "one"}},
                    {"monocle_container": {"windows": {"elements": [{"id": 2, "details": {"exe": "B"}}]}}},
                    {"maximized_window": {"id": 3, "details": {"exe": "C"}}},
                    {"id": "not-a-window-id", "details": {"exe": "D"}},
                    {"id": 4, "no_details": true}
                ]}},
                "floating_windows": {"elements": [{"id": 5, "details": {"exe": "E"}}]}
            }"#,
        )
        .expect("json");

        let ids: Vec<u32> = collect_windows(&state)
            .iter()
            .map(|window| window.id)
            .collect();
        assert_eq!(ids, vec![1, 2, 3, 5]);
    }

    #[test]
    fn resolves_windows_by_id_across_spaces() {
        let owners = HashMap::from([
            (
                59,
                WindowOwner {
                    pid: 100,
                    name: "Slack".into(),
                    layer: 0,
                },
            ),
            (
                61,
                WindowOwner {
                    pid: 200,
                    name: "Brave".into(),
                    layer: 0,
                },
            ),
            (
                8310,
                WindowOwner {
                    pid: 200,
                    name: "Brave".into(),
                    layer: 0,
                },
            ),
        ]);

        let windows: Vec<StateWindow> = [59, 61, 8310]
            .into_iter()
            .map(|id| StateWindow {
                id,
                exe: None,
                title: None,
            })
            .collect();

        let resolved = resolve(&windows, &owners);
        assert!(resolved.unresolved.is_empty());
        assert_eq!(resolved.apps.len(), 2);

        let brave = resolved
            .apps
            .iter()
            .find(|app| app.pid == 200)
            .expect("brave");
        assert_eq!(brave.name, "Brave");
        assert_eq!(brave.window_ids, BTreeSet::from([61, 8310]));
    }

    #[test]
    fn falls_back_to_an_unambiguous_owner_name() {
        let owners = HashMap::from([
            (
                1,
                WindowOwner {
                    pid: 100,
                    name: "Ghostty".into(),
                    layer: 0,
                },
            ),
            (
                2,
                WindowOwner {
                    pid: 200,
                    name: "Dup".into(),
                    layer: 0,
                },
            ),
            (
                3,
                WindowOwner {
                    pid: 300,
                    name: "Dup".into(),
                    layer: 0,
                },
            ),
        ]);

        let windows = vec![
            StateWindow {
                id: 99,
                exe: Some("Ghostty".into()),
                title: None,
            },
            StateWindow {
                id: 98,
                exe: Some("Dup".into()),
                title: None,
            },
            StateWindow {
                id: 97,
                exe: Some("Nope".into()),
                title: None,
            },
        ];

        let resolved = resolve(&windows, &owners);
        assert_eq!(resolved.apps.len(), 1);
        assert_eq!(resolved.apps[0].pid, 100);
        assert_eq!(resolved.apps[0].resolution, Resolution::OwnerName);
        assert_eq!(
            resolved.unresolved.len(),
            2,
            "ambiguous and unknown names stay unresolved"
        );
    }

    #[test]
    fn extracts_pids_from_window_manager_events() {
        let event: Value = serde_json::from_str(
            r#"{"type":"FocusChange","content":[{"type":"Accessibility","content":"AXMainWindowChanged"},20934,3395]}"#,
        )
        .expect("json");
        assert_eq!(event_pid(&event), Some(20934));

        let socket: Value =
            serde_json::from_str(r#"{"type":"FocusWindow","content":"Left"}"#).expect("json");
        assert_eq!(event_pid(&socket), None);

        let space: Value = serde_json::from_str(
            r#"{"type":"Show","content":[{"type":"AppKitWorkspace","content":"x"},-1]}"#,
        )
        .expect("json");
        assert_eq!(event_pid(&space), None, "negative pids are not processes");
    }

    #[test]
    fn tracker_reports_each_process_once() {
        let owners = HashMap::from([
            (
                1,
                WindowOwner {
                    pid: 100,
                    name: "A".into(),
                    layer: 0,
                },
            ),
            (
                2,
                WindowOwner {
                    pid: 100,
                    name: "A".into(),
                    layer: 0,
                },
            ),
            (
                3,
                WindowOwner {
                    pid: 200,
                    name: "B".into(),
                    layer: 0,
                },
            ),
        ]);
        let window = |id| StateWindow {
            id,
            exe: None,
            title: None,
        };

        let mut tracker = AppTracker::default();
        let fresh = tracker.observe(resolve(&[window(1), window(3)], &owners), "state");
        assert_eq!(fresh.len(), 2);

        // the same state again discovers nothing new, but a new window of a known app is merged
        let fresh = tracker.observe(
            resolve(&[window(1), window(2), window(3)], &owners),
            "state",
        );
        assert!(fresh.is_empty());
        let records = tracker.records();
        assert_eq!(
            records
                .iter()
                .find(|app| app.pid == 100)
                .expect("a")
                .window_ids,
            vec![1, 2]
        );

        assert!(tracker.observe_pid(100, "A".into(), "event").is_none());
        assert!(tracker.observe_pid(300, "C".into(), "event").is_some());
        assert!(tracker.contains(300));
    }

    #[test]
    fn unresolved_windows_are_reported_until_they_resolve() {
        let mut tracker = AppTracker::default();
        let window = StateWindow {
            id: 7,
            exe: Some("Ghost".into()),
            title: None,
        };
        tracker.observe(
            resolve(std::slice::from_ref(&window), &HashMap::new()),
            "state",
        );
        assert_eq!(tracker.unresolved().len(), 1);

        let owners = HashMap::from([(
            7,
            WindowOwner {
                pid: 5,
                name: "Ghost".into(),
                layer: 0,
            },
        )]);
        tracker.observe(resolve(std::slice::from_ref(&window), &owners), "state");
        assert!(tracker.unresolved().is_empty());
    }

    #[test]
    fn gui_apps_are_layer_zero_owners() {
        let owners = HashMap::from([
            (
                1,
                WindowOwner {
                    pid: 10,
                    name: "App".into(),
                    layer: 0,
                },
            ),
            (
                2,
                WindowOwner {
                    pid: 20,
                    name: "Dock".into(),
                    layer: 20,
                },
            ),
        ]);
        let gui = gui_app_pids(&owners);
        assert_eq!(gui.len(), 1);
        assert_eq!(gui.get(&10).map(String::as_str), Some("App"));
    }

    /// Against the real window server: the call must work and see windows. Sandboxed builds may
    /// have no window server to ask, in which case there is nothing to assert.
    #[test]
    fn window_server_listing_works() {
        let owners = window_owners();
        if owners.is_empty() {
            eprintln!("skipping: no window server is reachable from this environment");
            return;
        }

        assert!(owners.values().all(|owner| owner.pid > 0));
    }
}
