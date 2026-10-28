//! The daemon's model of the compositor: the windows, monitors and virtual desktops KWin reports,
//! kept up to date by the events the script in KWin sends, and the window events the window manager
//! gets from them.
//!
//! This is pure: JSON in, [`Output`]s out, no D-Bus and no threads, so everything the bridge
//! decides can be tested without a compositor. The script (`bridge.js`) holds no logic: it reports
//! what KWin tells it, and every decision about what a report means is made here. The events are
//! JSON objects with a `type`, described on [`Table::handle`].
//!
//! Every virtual desktop is a workspace: workspace `i` is KWin's desktop `i`, and each output
//! shows one desktop of its own, because komorebi turns on KWin's per-output virtual desktops. The
//! commands that show a workspace and put a window on one are made here too
//! ([`Table::switch_commands`], [`Table::window_workspace_commands`]), so that the table knows what
//! komorebi asked for and does not report it back as somebody else's doing.
//!
//! # Example
//!
//! The script's first report is a snapshot of the desktop. A window that opens after it is an
//! `added` event, which the window manager gets as a [`WindowManagerEvent::Show`]:
//!
//! ```rust
//! use std::time::Instant;
//!
//! use komorebi::kwin::state::{Output, Table};
//! use komorebi::window_manager_event::WindowManagerEvent;
//! use serde_json::json;
//!
//! let mut table = Table::new();
//! let now = Instant::now();
//!
//! table.handle(
//!     &json!({
//!         "type": "snapshot",
//!         "windows": [],
//!         "screens": [{
//!             "name": "DP-1", "manufacturer": "ACME", "model": "M1", "serial": "S1",
//!             "geo": { "x": 0, "y": 0, "w": 1920, "h": 1080 },
//!             "area": { "x": 0, "y": 0, "w": 1920, "h": 1050 },
//!         }],
//!         "active": null,
//!         "cursor": { "x": 960, "y": 540 },
//!     }),
//!     now,
//! );
//!
//! let out = table.handle(
//!     &json!({
//!         "type": "added", "id": "{6b9c}", "pid": 0, "class": "foot",
//!         "caption": "a terminal", "normal": true, "minimized": false,
//!         "geo": { "x": 0, "y": 0, "w": 800, "h": 600 },
//!     }),
//!     now,
//! );
//!
//! // KWin names the window by a UUID; komorebi gives it a number of its own
//! assert!(matches!(&out[..], [Output::Event(WindowManagerEvent::Show(_, window))] if window.id == 1));
//! assert_eq!(table.windows()[0].title, "a terminal");
//! assert_eq!(table.monitors()[0].work_area.bottom, 1050);
//! ```

use crate::backend::MonitorInfo;
use crate::backend::WindowInfo;
use crate::core::Rect;
use crate::window::Window;
use crate::window_manager_event::KWinNotification;
use crate::window_manager_event::SystemNotification;
use crate::window_manager_event::WindowManagerEvent;
use serde_json::Value;
use serde_json::json;
use std::collections::HashMap;
use std::time::Duration;
use std::time::Instant;

/// How long a closed window can still be asked about, so that the `Destroy` event that names it
/// can be logged and serialized
const TOMBSTONE: Duration = Duration::from_secs(60);

struct Entry {
    uuid: String,
    info: WindowInfo,
    /// What KWin says: a normal window, as opposed to a dialog, splash, panel, popup...
    kwin_normal: bool,
    desktops: Vec<String>,
    closed_at: Option<Instant>,
}

/// The work a handled event leaves for the rest of the daemon.
#[derive(Debug)]
pub enum Output {
    /// A window event for the window manager.
    Event(WindowManagerEvent),
    /// The answer to a cursor query.
    Cursor {
        /// The query this answers, as the daemon numbered it.
        token: u64,
        /// Where the pointer is, across.
        x: i32,
        /// Where the pointer is, down.
        y: i32,
    },
}

/// The windows and monitors of the desktop, as the script last reported them.
///
/// A window is known by the UUID KWin gives it, and the table gives it a `u32` the first time it
/// sees it ([`WindowInfo::id`]), which is how every other part of komorebi names it. An id is never
/// given to another window, also after the window has closed. A closed window stays in the table,
/// with [`WindowInfo::alive`] false, for a minute, so that the event that names it can still be
/// logged.
#[derive(Default)]
pub struct Table {
    next_id: u32,
    by_uuid: HashMap<String, u32>,
    windows: HashMap<u32, Entry>,
    monitors: Vec<MonitorInfo>,
    foreground: Option<u32>,
    cursor: Option<(i32, i32)>,
    /// `(id, name)` of every virtual desktop, in KWin's order
    desktops: Vec<(String, String)>,
    /// The desktop each output shows, by output name. They are independent (komorebi turns on
    /// KWin's per-output virtual desktops), which is what lets every monitor have a workspace of
    /// its own in front.
    current: HashMap<String, String>,
}

fn rect(v: &Value) -> Rect {
    let n = |k: &str| v[k].as_i64().unwrap_or(0) as i32;
    Rect {
        left: n("x"),
        top: n("y"),
        right: n("w"),
        bottom: n("h"),
    }
}

fn strings(v: &Value) -> Vec<String> {
    v.as_array()
        .map(|a| {
            a.iter()
                .filter_map(|s| s.as_str().map(String::from))
                .collect()
        })
        .unwrap_or_default()
}

/// FNV-1a: a monitor's id has to be the same on every run, so no `RandomState`
fn stable_id(s: &str) -> u32 {
    let mut hash: u32 = 0x811c_9dc5;
    for b in s.bytes() {
        hash ^= u32::from(b);
        hash = hash.wrapping_mul(0x0100_0193);
    }
    hash
}

fn monitor(v: &Value) -> MonitorInfo {
    let name = v["name"].as_str().unwrap_or_default().to_string();
    let serial = v["serial"].as_str().unwrap_or_default();

    // an output's name follows the port it is plugged into; its serial follows the monitor
    let serial_number_id = if serial.is_empty() {
        name.clone()
    } else {
        format!(
            "{}-{}-{serial}",
            v["manufacturer"].as_str().unwrap_or_default(),
            v["model"].as_str().unwrap_or_default()
        )
    };

    MonitorInfo {
        id: stable_id(&serial_number_id),
        name,
        serial_number_id,
        size: rect(&v["geo"]),
        work_area: rect(&v["area"]),
    }
}

fn kwin(n: KWinNotification) -> SystemNotification {
    SystemNotification::KWin(n)
}

impl Table {
    /// Returns an empty table. The first window it sees gets the id 1.
    pub fn new() -> Self {
        Self {
            next_id: 1,
            ..Self::default()
        }
    }

    // ---- queries ---------------------------------------------------------------------------

    /// Returns what is known about the window, or `None` if the table has never seen it or it
    /// closed more than a minute ago.
    pub fn window(&self, id: u32) -> Option<WindowInfo> {
        self.windows.get(&id).map(|e| e.info.clone())
    }

    /// Returns the windows komorebi may manage that are open right now, ordered by id.
    pub fn windows(&self) -> Vec<WindowInfo> {
        let mut windows: Vec<_> = self
            .windows
            .values()
            .filter(|e| e.info.alive && self.manageable(e))
            .map(|e| e.info.clone())
            .collect();
        windows.sort_by_key(|w| w.id);
        windows
    }

    /// Returns the monitors, in the order KWin lists its outputs.
    pub fn monitors(&self) -> Vec<MonitorInfo> {
        self.monitors.clone()
    }

    /// Returns the id of the window KWin last reported active, if it is still open.
    pub fn foreground(&self) -> Option<u32> {
        self.foreground
            .filter(|id| self.windows.get(id).is_some_and(|e| e.info.alive))
    }

    /// Returns where the pointer was the last time the script said.
    pub fn cursor(&self) -> Option<(i32, i32)> {
        self.cursor
    }

    /// Returns the UUID KWin knows the window by, which is how a command for the script names
    /// it, or `None` if the table does not know the window.
    pub fn uuid(&self, id: u32) -> Option<String> {
        self.windows.get(&id).map(|e| e.uuid.clone())
    }

    /// Whether komorebi should have anything to do with this window: a normal window, as opposed
    /// to a dialog, a panel, a popup. Every desktop is a workspace of komorebi's.
    fn manageable(&self, entry: &Entry) -> bool {
        entry.kwin_normal
    }

    // ---- workspaces ------------------------------------------------------------------------

    /// The ids of the desktops that are workspaces, in order: all of them
    fn workspace_desktops(&self) -> Vec<&str> {
        self.desktops.iter().map(|(id, _)| id.as_str()).collect()
    }

    /// Which workspace a desktop is
    fn workspace_index(&self, desktop: &str) -> Option<usize> {
        self.workspace_desktops().iter().position(|d| *d == desktop)
    }

    /// Returns the workspace a window is on: the first of its desktops. A window on every desktop
    /// (KWin gives it no desktop) is on none.
    pub fn window_workspace(&self, id: u32) -> Option<usize> {
        let entry = self.windows.get(&id)?;
        let desktop = entry.desktops.first()?;
        self.workspace_index(desktop)
    }

    /// Returns the workspace each monitor shows, as `(monitor id, workspace index)`
    pub fn current_workspaces(&self) -> Vec<(u32, usize)> {
        self.monitors
            .iter()
            .filter_map(|m| {
                let desktop = self.current.get(&m.name)?;
                Some((m.id, self.workspace_index(desktop)?))
            })
            .collect()
    }

    /// Returns how many workspace desktops there are
    pub fn workspace_count(&self) -> usize {
        self.workspace_desktops().len()
    }

    /// Replaces the list of desktops with what KWin says it is now, for after komorebi created
    /// some. The list is `(id, name)` pairs, in KWin's order.
    pub fn set_desktops(&mut self, desktops: Vec<(String, String)>) {
        self.desktops = desktops;
    }

    /// Returns the commands that show workspace `idx` on a monitor, and records that the monitor
    /// shows it, so that KWin's report of the switch is not taken for somebody else's. A monitor
    /// the table does not know gets no command.
    ///
    /// # Errors
    ///
    /// [`NeedsDesktops`] when there are not that many desktops yet: make them, then ask again.
    pub fn switch_commands(
        &mut self,
        monitor_id: u32,
        idx: usize,
    ) -> Result<Vec<Value>, NeedsDesktops> {
        let desktop = self
            .workspace_desktops()
            .get(idx)
            .map(|d| d.to_string())
            .ok_or(NeedsDesktops(idx + 1))?;

        let Some(monitor) = self.monitors.iter().find(|m| m.id == monitor_id) else {
            return Ok(vec![]);
        };

        let output = monitor.name.clone();
        self.current.insert(output.clone(), desktop.clone());
        Ok(vec![
            json!({ "op": "output_desktop", "output": output, "v": desktop }),
        ])
    }

    /// Returns the commands that put a window on workspace `idx`, and records the window there,
    /// so that the next query sees it at once and KWin's report of the move raises nothing. A
    /// window the table does not know gets no command.
    ///
    /// # Errors
    ///
    /// [`NeedsDesktops`] when there are not that many desktops yet: make them, then ask again.
    pub fn window_workspace_commands(
        &mut self,
        id: u32,
        idx: usize,
    ) -> Result<Vec<Value>, NeedsDesktops> {
        let desktop = self
            .workspace_desktops()
            .get(idx)
            .map(|d| d.to_string())
            .ok_or(NeedsDesktops(idx + 1))?;

        let Some(entry) = self.windows.get_mut(&id) else {
            return Ok(vec![]);
        };

        entry.desktops = vec![desktop.clone()];
        Ok(vec![
            json!({ "op": "desktop", "id": entry.uuid, "v": desktop }),
        ])
    }

    /// KWin went away. Its windows go with it, and nothing will report them closing.
    ///
    /// Returns a [`WindowManagerEvent::Destroy`] for every window the window manager may manage,
    /// as if each had been closed, and forgets which window has the focus. The windows stay in
    /// the table as closed ones, so their ids are not given out again. A KWin that comes back
    /// reports its desktop in a new snapshot.
    pub fn compositor_lost(&mut self, now: Instant) -> Vec<Output> {
        let ids: Vec<u32> = self
            .windows
            .iter()
            .filter(|(_, e)| e.info.alive)
            .map(|(id, _)| *id)
            .collect();

        let mut out = vec![];
        for id in ids {
            let uuid = self.windows[&id].uuid.clone();
            out.extend(self.on_removed(&json!({ "id": uuid }), now));
        }

        self.foreground = None;
        out
    }

    /// Records the frame komorebi has just asked KWin to give a window, so that the window manager
    /// sees it at once. The `geometry` event that says what the client did replaces it.
    pub fn note_requested_rect(&mut self, id: u32, rect: Rect) {
        if let Some(entry) = self.windows.get_mut(&id) {
            entry.info.rect = rect;
        }
    }

    // ---- events ----------------------------------------------------------------------------

    fn id_for(&mut self, uuid: &str) -> u32 {
        if let Some(id) = self.by_uuid.get(uuid) {
            return *id;
        }
        let id = self.next_id.max(1);
        self.next_id = id + 1;
        self.by_uuid.insert(uuid.to_string(), id);
        id
    }

    fn entry_from(&mut self, w: &Value) -> (u32, Entry) {
        let uuid = w["id"].as_str().unwrap_or_default().to_string();
        let id = self.id_for(&uuid);
        let class = w["class"].as_str().unwrap_or_default().to_string();
        let pid = w["pid"].as_i64().unwrap_or_default() as i32;
        let (exe, path) = process(pid, &class);

        let info = WindowInfo {
            id,
            pid,
            title: w["caption"].as_str().unwrap_or_default().to_string(),
            class,
            exe,
            path,
            rect: rect(&w["geo"]),
            minimized: w["minimized"].as_bool().unwrap_or(false),
            normal: false,
            alive: true,
        };

        let entry = Entry {
            uuid,
            info,
            kwin_normal: w["normal"].as_bool().unwrap_or(false),
            desktops: strings(&w["desktops"]),
            closed_at: None,
        };

        (id, entry)
    }

    /// `WindowInfo::normal` is "may be managed", which is kept in step after every change rather
    /// than recomputed by the consumer
    fn sync_normal(&mut self, id: u32) {
        let manageable = self.windows.get(&id).is_some_and(|e| self.manageable(e));
        if let Some(entry) = self.windows.get_mut(&id) {
            entry.info.normal = manageable;
        }
    }

    fn prune(&mut self, now: Instant) {
        let expired: Vec<u32> = self
            .windows
            .iter()
            .filter(|(_, e)| {
                e.closed_at
                    .is_some_and(|t| now.duration_since(t) > TOMBSTONE)
            })
            .map(|(id, _)| *id)
            .collect();

        for id in expired {
            if let Some(entry) = self.windows.remove(&id) {
                self.by_uuid.remove(&entry.uuid);
            }
        }
    }

    /// Updates the table with one event from the script, and returns what the rest of the daemon
    /// has to do about it. `now` is when the event arrived; a closed window is forgotten a minute
    /// after it closed.
    ///
    /// The event's `type` is one of:
    ///
    /// - `snapshot`: everything on the desktop (`windows`, `screens`, `desktops`,
    ///   `current_desktops`, `active`, `cursor`), sent once, when the script starts. It fills the
    ///   table and raises no event: the window manager reads the desktop from the table when it
    ///   starts.
    /// - `added`, `removed`: a window opened or closed. For a window komorebi may manage, they are
    ///   a [`WindowManagerEvent::Show`] and a [`WindowManagerEvent::Destroy`].
    /// - `activated`: KWin gave a window the focus ([`WindowManagerEvent::FocusChange`]), or,
    ///   with no `id`, took it away from every window.
    /// - `minimized`: a window was minimized or brought back ([`WindowManagerEvent::Minimize`],
    ///   [`WindowManagerEvent::Restore`]).
    /// - `caption`: a window's title changed ([`WindowManagerEvent::TitleUpdate`]).
    /// - `geometry`: a window's frame changed. It raises no event.
    /// - `desktops`: a window is on other desktops. When it is now on another workspace than the
    ///   table had it on, this is a [`WindowManagerEvent::WorkspaceMove`]; komorebi's own move
    ///   ([`Table::window_workspace_commands`]) is on it already, and raises nothing.
    /// - `current_desktops`: the desktop each output shows. An output that shows another desktop
    ///   than the table had it showing is a [`WindowManagerEvent::WorkspaceChange`]; komorebi's own
    ///   switch ([`Table::switch_commands`]) raises nothing.
    /// - `desktop_list`: the virtual desktops, in order. It raises no event.
    /// - `cursor`: the answer to a cursor query ([`Output::Cursor`]).
    /// - `failed`: commands the script could not carry out. They are logged.
    ///
    /// Anything else is logged at debug level and ignored. A dialog, a panel or a popup is in the
    /// table, and can have the focus, but it raises no window event.
    pub fn handle(&mut self, event: &Value, now: Instant) -> Vec<Output> {
        self.prune(now);

        match event["type"].as_str() {
            Some("snapshot") => self.on_snapshot(event),
            Some("added") => self.on_added(event),
            Some("removed") => self.on_removed(event, now),
            Some("activated") => self.on_activated(event),
            Some("minimized") => self.on_minimized(event),
            Some("caption") => self.on_caption(event),
            Some("geometry") => {
                if let Some(entry) = self.entry_mut(event) {
                    entry.info.rect = rect(&event["geo"]);
                }
                vec![]
            }
            Some("desktops") => self.on_desktops(event),
            Some("current_desktops") => self.on_current_desktops(event),
            Some("desktop_list") => {
                self.desktops = desktop_list(&event["desktops"]);
                vec![]
            }
            Some("cursor") => {
                let (x, y) = (
                    event["x"].as_i64().unwrap_or(0) as i32,
                    event["y"].as_i64().unwrap_or(0) as i32,
                );
                self.cursor = Some((x, y));
                vec![Output::Cursor {
                    token: event["token"].as_u64().unwrap_or(0),
                    x,
                    y,
                }]
            }
            Some("failed") => {
                tracing::warn!("the bridge could not do: {}", event["cmds"]);
                vec![]
            }
            other => {
                tracing::debug!("ignoring unknown bridge event {other:?}");
                vec![]
            }
        }
    }

    fn entry_mut(&mut self, event: &Value) -> Option<&mut Entry> {
        let uuid = event["id"].as_str()?;
        let id = *self.by_uuid.get(uuid)?;
        self.windows.get_mut(&id)
    }

    fn id_of(&self, event: &Value) -> Option<u32> {
        self.by_uuid.get(event["id"].as_str()?).copied()
    }

    fn on_snapshot(&mut self, event: &Value) -> Vec<Output> {
        // desktops first: which workspace a window is on depends on them
        self.desktops = desktop_list(&event["desktops"]);
        self.current = current_map(&event["current_desktops"]);

        // rebuild the windows, keeping the ids of those we already knew
        let old = std::mem::take(&mut self.windows);
        for w in event["windows"].as_array().cloned().unwrap_or_default() {
            let (id, entry) = self.entry_from(&w);
            self.windows.insert(id, entry);
            self.sync_normal(id);
        }
        // closed windows still within their grace period
        for (id, entry) in old {
            if entry.closed_at.is_some() {
                self.windows.entry(id).or_insert(entry);
            }
        }

        self.foreground = event["active"]
            .as_str()
            .and_then(|u| self.by_uuid.get(u))
            .copied();

        if let (Some(x), Some(y)) = (event["cursor"]["x"].as_i64(), event["cursor"]["y"].as_i64()) {
            self.cursor = Some((x as i32, y as i32));
        }

        self.set_monitors(&event["screens"]);
        vec![]
    }

    fn set_monitors(&mut self, screens: &Value) {
        self.monitors = screens
            .as_array()
            .map(|a| a.iter().map(monitor).collect())
            .unwrap_or_default();
    }

    fn on_added(&mut self, event: &Value) -> Vec<Output> {
        let (id, entry) = self.entry_from(event);
        if self.windows.get(&id).is_some_and(|e| e.info.alive) {
            // already announced (the script announces on activation as well as on creation)
            return vec![];
        }

        self.windows.insert(id, entry);
        self.sync_normal(id);

        if self.windows[&id].info.normal {
            vec![Output::Event(WindowManagerEvent::Show(
                kwin(KWinNotification::WindowAdded),
                Window::from(id),
            ))]
        } else {
            vec![]
        }
    }

    fn on_removed(&mut self, event: &Value, now: Instant) -> Vec<Output> {
        let Some(id) = self.id_of(event) else {
            return vec![];
        };

        let manageable = self.windows.get(&id).is_some_and(|e| e.info.normal);

        if let Some(entry) = self.windows.get_mut(&id) {
            entry.info.alive = false;
            entry.info.normal = false;
            entry.closed_at = Some(now);
        }

        if self.foreground == Some(id) {
            self.foreground = None;
        }

        if manageable {
            vec![Output::Event(WindowManagerEvent::Destroy(
                kwin(KWinNotification::WindowRemoved),
                Window::from(id),
            ))]
        } else {
            vec![]
        }
    }

    fn on_activated(&mut self, event: &Value) -> Vec<Output> {
        let Some(uuid) = event["id"].as_str() else {
            self.foreground = None;
            return vec![];
        };

        let Some(id) = self.by_uuid.get(uuid).copied() else {
            return vec![];
        };

        self.foreground = Some(id);
        tracing::debug!(
            "KWin activated window {id} ({})",
            self.windows.get(&id).map_or("?", |e| e.info.title.as_str())
        );

        if self.windows.get(&id).is_some_and(|e| e.info.normal) {
            vec![Output::Event(WindowManagerEvent::FocusChange(
                kwin(KWinNotification::WindowActivated),
                Window::from(id),
            ))]
        } else {
            vec![]
        }
    }

    fn on_minimized(&mut self, event: &Value) -> Vec<Output> {
        let minimized = event["v"].as_bool().unwrap_or(false);
        let Some(id) = self.id_of(event) else {
            return vec![];
        };

        let Some(entry) = self.windows.get_mut(&id) else {
            return vec![];
        };
        entry.info.minimized = minimized;

        if !entry.info.normal {
            return vec![];
        }

        let window = Window::from(id);
        vec![Output::Event(if minimized {
            WindowManagerEvent::Minimize(kwin(KWinNotification::WindowMinimized), window)
        } else {
            WindowManagerEvent::Restore(kwin(KWinNotification::WindowRestored), window)
        })]
    }

    fn on_caption(&mut self, event: &Value) -> Vec<Output> {
        let Some(id) = self.id_of(event) else {
            return vec![];
        };

        let Some(entry) = self.windows.get_mut(&id) else {
            return vec![];
        };
        entry.info.title = event["caption"].as_str().unwrap_or_default().to_string();

        if entry.info.normal {
            vec![Output::Event(WindowManagerEvent::TitleUpdate(
                kwin(KWinNotification::CaptionChanged),
                Window::from(id),
            ))]
        } else {
            vec![]
        }
    }

    fn on_desktops(&mut self, event: &Value) -> Vec<Output> {
        let Some(id) = self.id_of(event) else {
            return vec![];
        };

        let before = self.window_workspace(id);
        let desktops = strings(&event["desktops"]);

        if let Some(entry) = self.windows.get_mut(&id) {
            entry.desktops = desktops;
        }

        // A window that is on another workspace's desktop than it was: komorebi's own doing coming
        // back (the window manager already has it there) or somebody else's, which it will adopt.
        let after = self
            .windows
            .get(&id)
            .filter(|e| e.info.normal && e.info.alive)
            .and_then(|e| e.desktops.first())
            .and_then(|d| self.workspace_index(d));

        match after {
            Some(idx) if before != Some(idx) => {
                vec![Output::Event(WindowManagerEvent::WorkspaceMove(
                    kwin(KWinNotification::WindowMovedToDesktop),
                    Window::from(id),
                    idx,
                ))]
            }
            _ => vec![],
        }
    }

    /// Some output now shows another desktop: komorebi's request, or a pager, a shortcut, an app
    /// raising a window (KWin switches that output to the window's desktop)
    fn on_current_desktops(&mut self, event: &Value) -> Vec<Output> {
        let now = current_map(&event["current"]);
        let mut out = vec![];

        let mut changes: Vec<(String, String)> = now
            .iter()
            .filter(|(output, desktop)| self.current.get(*output) != Some(*desktop))
            .map(|(o, d)| (o.clone(), d.clone()))
            .collect();
        changes.sort();

        for (output, desktop) in changes {
            self.current.insert(output.clone(), desktop.clone());

            let (Some(idx), Some(monitor)) = (
                self.workspace_index(&desktop),
                self.monitors.iter().find(|m| m.name == output),
            ) else {
                continue;
            };

            out.push(Output::Event(WindowManagerEvent::WorkspaceChange(
                kwin(KWinNotification::WorkspaceChanged),
                monitor.id,
                idx,
            )));
        }

        out
    }
}

fn desktop_list(v: &Value) -> Vec<(String, String)> {
    v.as_array()
        .map(|a| {
            a.iter()
                .filter_map(|d| {
                    Some((
                        d["id"].as_str()?.to_string(),
                        d["name"].as_str()?.to_string(),
                    ))
                })
                .collect()
        })
        .unwrap_or_default()
}

fn current_map(v: &Value) -> HashMap<String, String> {
    v.as_array()
        .map(|a| {
            a.iter()
                .filter_map(|d| {
                    Some((
                        d["output"].as_str()?.to_string(),
                        d["id"].as_str()?.to_string(),
                    ))
                })
                .collect()
        })
        .unwrap_or_default()
}

/// The error of a request for a workspace that has no desktop yet: at least this many desktops
/// are needed.
///
/// [`Table::switch_commands`] and [`Table::window_workspace_commands`] return it; the KWin backend
/// makes the desktops and asks again.
#[derive(Debug)]
pub struct NeedsDesktops(pub usize);

/// The executable of a process, by name and by path.
///
/// Rules match on these (`Exe` is the basename, `Path` the full path), as they do on Windows.
/// `/proc/<pid>/exe` is a symlink to the executable; for a window whose process cannot be read
/// (another user's, a sandbox) the class is the best identity there is.
fn process(pid: i32, class: &str) -> (String, String) {
    #[cfg(target_os = "linux")]
    if pid > 0
        && let Ok(path) = std::fs::read_link(format!("/proc/{pid}/exe"))
    {
        let exe = path
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_default();

        return (exe, path.to_string_lossy().to_string());
    }

    let _ = pid;
    (class.to_string(), String::new())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn win(id: &str, desktops: &[&str]) -> Value {
        json!({
            "id": id, "pid": 0, "class": "foot", "caption": "terminal", "normal": true,
            "minimized": false, "fullscreen": false,
            "geo": { "x": 10, "y": 20, "w": 800, "h": 600 },
            "desktops": desktops,
        })
    }

    fn snapshot(windows: Vec<Value>) -> Value {
        json!({
            "type": "snapshot",
            "windows": windows,
            "screens": [{
                "name": "DP-1", "manufacturer": "ACME", "model": "M1", "serial": "S1",
                "geo": { "x": 0, "y": 0, "w": 1920, "h": 1080 },
                "area": { "x": 0, "y": 0, "w": 1920, "h": 1050 },
            }],
            "desktops": [{ "id": "d1", "name": "Desktop 1" }, { "id": "d2", "name": "Desktop 2" }],
            "current_desktops": [{ "output": "DP-1", "id": "d1" }],
            "active": null,
            "cursor": { "x": 5, "y": 6 },
        })
    }

    fn table(windows: Vec<Value>) -> Table {
        let mut t = Table::new();
        t.handle(&snapshot(windows), Instant::now());
        t
    }

    fn events(out: &[Output]) -> Vec<String> {
        out.iter()
            .filter_map(|o| match o {
                Output::Event(e) => Some(format!("{e} {}", e.window_id().unwrap_or(0))),
                _ => None,
            })
            .collect()
    }

    /// The events of a session as the bridge sends them: `bridge.test.js` plays the session
    /// against a fake KWin and checks that the bridge sends exactly these
    #[test]
    fn the_bridges_events_for_a_session_are_the_window_events_komorebi_expects() {
        let session: Vec<Value> =
            serde_json::from_str(include_str!("fixtures/events.json")).unwrap();
        let mut t = Table::new();
        let now = Instant::now();

        let mut trace = vec![];
        for event in &session {
            let out = t.handle(event, now);
            let said: Vec<String> = out
                .iter()
                .map(|output| match output {
                    Output::Event(WindowManagerEvent::WorkspaceMove(_, window, idx)) => {
                        format!("WorkspaceMove {} to {idx}", window.id)
                    }
                    Output::Event(WindowManagerEvent::WorkspaceChange(_, _, idx)) => {
                        format!("WorkspaceChange to {idx}")
                    }
                    Output::Event(e) => format!("{e} {}", e.window_id().unwrap_or(0)),
                    Output::Cursor { token, x, y } => format!("Cursor {token} at {x},{y}"),
                })
                .collect();
            trace.push(format!(
                "{} -> {}",
                event["type"].as_str().unwrap(),
                said.join(", ")
            ));

            // what komorebi asks for next (fixtures/commands.json, batch 3): the editor on the
            // second workspace, and the output showing it
            if event["seq"] == 15 {
                let monitor = t.monitors()[0].id;
                t.window_workspace_commands(1, 1).unwrap();
                t.switch_commands(monitor, 1).unwrap();
            }

            if event["type"] == "added" && event["caption"] == "a terminal" {
                let w = t.window(2).unwrap();
                assert_eq!(
                    (w.pid, w.class.as_str(), w.title.as_str()),
                    (4343, "foot", "a terminal")
                );
                assert_eq!(
                    w.rect,
                    Rect {
                        left: 480,
                        top: 270,
                        right: 960,
                        bottom: 540
                    }
                );
                assert!(w.normal && !w.minimized);
            }
        }

        assert_eq!(
            trace,
            [
                "snapshot -> ",
                "added -> Show 2",
                "activated -> FocusChange 2",
                "added -> ",
                "minimized -> Minimize 2",
                "minimized -> Restore 2",
                "geometry -> ",
                "cursor -> Cursor 1 at 1200,701",
                "removed -> Destroy 2",
                // the window manager's commands, and the terminal's, which had closed
                "geometry -> ",
                "activated -> FocusChange 1",
                "minimized -> Minimize 1",
                "minimized -> Restore 1",
                "removed -> ",
                "failed -> ",
                // komorebi's own move and switch, coming back
                "desktops -> ",
                "current_desktops -> ",
                // somebody else's: a new desktop, the editor moved back, the first one shown
                "desktop_list -> ",
                "desktops -> WorkspaceMove 1 to 0",
                "current_desktops -> WorkspaceChange to 0",
            ]
        );

        // the editor of the snapshot is left; its dialog, which komorebi does not manage, closed
        let windows = t.windows();
        assert_eq!(windows.len(), 1);
        assert_eq!(windows[0].title, "notes.txt — Kate");
        assert_eq!(windows[0].rect.right, 960, "the frame komorebi asked for");
        assert_eq!(t.window(3).map(|w| w.alive), Some(false));
        assert_eq!(t.window(2).unwrap().rect.bottom, 532, "the geometry event");
        assert_eq!(t.foreground(), Some(1));
        assert_eq!(t.cursor(), Some((1200, 701)));
        assert_eq!(t.workspace_count(), 3);
        assert_eq!(t.window_workspace(1), Some(0));
        assert_eq!(t.current_workspaces(), [(t.monitors()[0].id, 0)]);

        let monitor = &t.monitors()[0];
        assert_eq!(
            (monitor.name.as_str(), monitor.serial_number_id.as_str()),
            ("DP-1", "ACME-M1-S1")
        );
        assert_eq!(
            monitor.work_area,
            Rect {
                left: 0,
                top: 0,
                right: 1920,
                bottom: 1050
            }
        );
    }

    #[test]
    fn the_snapshot_fills_the_table_without_raising_events() {
        let mut t = Table::new();
        let out = t.handle(
            &snapshot(vec![win("a", &["d1"]), win("b", &["d1"])]),
            Instant::now(),
        );

        assert!(events(&out).is_empty());
        let windows = t.windows();
        assert_eq!(windows.len(), 2);
        assert_eq!(
            windows[0].rect,
            Rect {
                left: 10,
                top: 20,
                right: 800,
                bottom: 600
            }
        );
        assert_eq!(t.cursor(), Some((5, 6)));
        assert_eq!(t.current_workspaces(), vec![(t.monitors()[0].id, 0)]);
    }

    #[test]
    fn monitors_get_a_stable_identity_from_the_serial_not_the_port() {
        let t = table(vec![]);
        let m = &t.monitors()[0];
        assert_eq!(m.serial_number_id, "ACME-M1-S1");
        assert_eq!(m.id, stable_id("ACME-M1-S1"));
        assert_eq!(m.work_area.bottom, 1050);

        let mut t2 = Table::new();
        let mut s = snapshot(vec![]);
        s["screens"][0]["name"] = json!("HDMI-A-2");
        t2.handle(&s, Instant::now());
        assert_eq!(
            t2.monitors()[0].id,
            m.id,
            "same monitor on another port keeps its id"
        );
    }

    #[test]
    fn a_monitor_without_a_serial_is_identified_by_its_name() {
        let mut t = Table::new();
        let mut s = snapshot(vec![]);
        s["screens"][0]["serial"] = json!("");
        t.handle(&s, Instant::now());
        assert_eq!(t.monitors()[0].serial_number_id, "DP-1");
    }

    #[test]
    fn ids_are_assigned_once_per_window_and_survive_a_new_snapshot() {
        let mut t = table(vec![win("a", &["d1"]), win("b", &["d1"])]);
        let before: Vec<u32> = t.windows().iter().map(|w| w.id).collect();

        t.handle(
            &snapshot(vec![
                win("b", &["d1"]),
                win("a", &["d1"]),
                win("c", &["d1"]),
            ]),
            Instant::now(),
        );
        let after: Vec<u32> = t.windows().iter().map(|w| w.id).collect();

        assert_eq!(&after[..2], &before[..]);
        assert_eq!(after.len(), 3);
    }

    #[test]
    fn a_new_window_raises_show_and_then_focus_in_that_order() {
        let mut t = table(vec![]);
        let now = Instant::now();

        let shown = t.handle(
            &json!({ "type": "added", "id": "w1", "pid": 0, "class": "foot",
            "caption": "x", "normal": true, "geo": {"x":0,"y":0,"w":1,"h":1}, "desktops": ["d1"] }),
            now,
        );
        let focused = t.handle(&json!({ "type": "activated", "id": "w1" }), now);

        assert_eq!(events(&shown), vec!["Show 1"]);
        assert_eq!(events(&focused), vec!["FocusChange 1"]);
        assert_eq!(t.foreground(), Some(1));
    }

    #[test]
    fn announcing_the_same_window_twice_shows_it_once() {
        let mut t = table(vec![]);
        let added = json!({ "type": "added", "id": "w1", "pid": 0, "class": "foot", "caption": "x",
            "normal": true, "geo": {"x":0,"y":0,"w":1,"h":1}, "desktops": ["d1"] });

        assert_eq!(events(&t.handle(&added, Instant::now())).len(), 1);
        assert!(events(&t.handle(&added, Instant::now())).is_empty());
    }

    #[test]
    fn dialogs_panels_and_popups_raise_nothing() {
        let mut t = table(vec![]);
        let now = Instant::now();
        let mut dialog = win("dlg", &["d1"]);
        dialog["type"] = json!("added");
        dialog["normal"] = json!(false);

        assert!(t.handle(&dialog, now).is_empty());
        assert!(
            t.handle(&json!({ "type": "activated", "id": "dlg" }), now)
                .is_empty()
        );
        assert!(t.windows().is_empty());
        // ... though they are still known, and still have a foreground
        assert_eq!(t.foreground(), Some(1));
    }

    #[test]
    fn a_closed_window_raises_destroy_and_can_still_be_asked_about() {
        let mut t = table(vec![win("a", &["d1"])]);
        let now = Instant::now();
        t.handle(&json!({ "type": "activated", "id": "a" }), now);

        let out = t.handle(&json!({ "type": "removed", "id": "a" }), now);
        assert_eq!(events(&out), vec!["Destroy 1"]);
        assert_eq!(t.foreground(), None);

        let info = t.window(1).expect("a tombstone is kept");
        assert!(!info.alive);
        assert_eq!(info.title, "terminal");
        assert!(t.windows().is_empty());
    }

    #[test]
    fn tombstones_are_dropped_after_a_while_and_their_ids_are_not_reused() {
        let mut t = table(vec![win("a", &["d1"])]);
        let now = Instant::now();
        t.handle(&json!({ "type": "removed", "id": "a" }), now);

        t.handle(
            &json!({ "type": "geometry", "id": "nobody", "geo": {"x":0,"y":0,"w":0,"h":0} }),
            now + Duration::from_secs(61),
        );
        assert!(t.window(1).is_none());

        let out = t.handle(
            &json!({ "type": "added", "id": "b", "pid": 0, "class": "foot", "caption": "x",
            "normal": true, "geo": {"x":0,"y":0,"w":1,"h":1}, "desktops": ["d1"] }),
            now,
        );
        assert_eq!(events(&out), vec!["Show 2"]);
    }

    #[test]
    fn minimize_and_restore_map_to_their_events() {
        let mut t = table(vec![win("a", &["d1"])]);
        let now = Instant::now();

        assert_eq!(
            events(&t.handle(&json!({ "type": "minimized", "id": "a", "v": true }), now)),
            vec!["Minimize 1"]
        );
        assert!(t.window(1).unwrap().minimized);
        assert_eq!(
            events(&t.handle(&json!({ "type": "minimized", "id": "a", "v": false }), now)),
            vec!["Restore 1"]
        );
    }

    #[test]
    fn geometry_updates_the_rect_silently() {
        let mut t = table(vec![win("a", &["d1"])]);
        let out = t.handle(
            &json!({ "type": "geometry", "id": "a", "geo": {"x":1,"y":2,"w":3,"h":4} }),
            Instant::now(),
        );

        assert!(out.is_empty());
        assert_eq!(
            t.window(1).unwrap().rect,
            Rect {
                left: 1,
                top: 2,
                right: 3,
                bottom: 4
            }
        );
    }

    #[test]
    fn a_title_change_raises_a_title_update() {
        let mut t = table(vec![win("a", &["d1"])]);
        let out = t.handle(
            &json!({ "type": "caption", "id": "a", "caption": "renamed" }),
            Instant::now(),
        );

        assert_eq!(events(&out), vec!["TitleUpdate 1"]);
        assert_eq!(t.window(1).unwrap().title, "renamed");
    }

    #[test]
    fn every_desktop_is_a_workspace_and_a_window_is_on_the_one_it_is_on() {
        let t = table(vec![
            win("first", &["d1"]),
            win("second", &["d2"]),
            win("everywhere", &[]),
        ]);

        assert_eq!(
            t.windows().len(),
            3,
            "windows on any desktop are komorebi's"
        );
        assert_eq!(t.window_workspace(1), Some(0));
        assert_eq!(t.window_workspace(2), Some(1));
        assert_eq!(
            t.window_workspace(3),
            None,
            "a window on every desktop is on none"
        );
        assert_eq!(t.workspace_count(), 2);
    }

    #[test]
    fn moving_a_window_to_another_desktop_is_reported_once_as_a_workspace_move() {
        let mut t = table(vec![win("a", &["d1"])]);
        let now = Instant::now();

        let out = t.handle(
            &json!({ "type": "desktops", "id": "a", "desktops": ["d2"] }),
            now,
        );
        assert!(
            matches!(
                &out[..],
                [Output::Event(WindowManagerEvent::WorkspaceMove(_, _, 1))]
            ),
            "{out:?}"
        );
        assert_eq!(t.window_workspace(1), Some(1));
        assert!(t.window(1).unwrap().normal);

        // the same again is nothing new
        let out = t.handle(
            &json!({ "type": "desktops", "id": "a", "desktops": ["d2"] }),
            now,
        );
        assert!(out.is_empty());
    }

    #[test]
    fn komorebis_own_move_coming_back_from_kwin_raises_nothing() {
        let mut t = table(vec![win("a", &["d1"])]);

        // komorebi put it on workspace 1; kwin says so
        t.window_workspace_commands(1, 1).unwrap();
        let out = t.handle(
            &json!({ "type": "desktops", "id": "a", "desktops": ["d2"] }),
            Instant::now(),
        );

        assert!(out.is_empty(), "{out:?}");
    }

    #[test]
    fn a_window_is_put_on_a_workspace_by_putting_it_on_its_desktop() {
        let mut t = table(vec![win("a", &["d1"])]);

        assert_eq!(
            t.window_workspace_commands(1, 1).unwrap(),
            vec![json!({ "op": "desktop", "id": "a", "v": "d2" })]
        );
        assert_eq!(
            t.window_workspace(1),
            Some(1),
            "visible to the next query at once"
        );
    }

    #[test]
    fn showing_a_workspace_switches_that_output_only() {
        let mut t = table(vec![]);
        let monitor = t.monitors()[0].id;

        assert_eq!(
            t.switch_commands(monitor, 1).unwrap(),
            vec![json!({ "op": "output_desktop", "output": "DP-1", "v": "d2" })]
        );
        assert_eq!(t.current_workspaces(), vec![(monitor, 1)]);
    }

    #[test]
    fn a_workspace_without_a_desktop_asks_for_one() {
        let mut t = table(vec![win("a", &["d1"])]);
        let monitor = t.monitors()[0].id;

        assert!(matches!(
            t.switch_commands(monitor, 4),
            Err(NeedsDesktops(5))
        ));
        assert!(matches!(
            t.window_workspace_commands(1, 2),
            Err(NeedsDesktops(3))
        ));
    }

    #[test]
    fn another_party_switching_a_monitor_is_a_workspace_change() {
        let mut t = table(vec![]);
        let monitor = t.monitors()[0].id;

        let out = t.handle(
            &json!({ "type": "current_desktops", "current": [{ "output": "DP-1", "id": "d2" }] }),
            Instant::now(),
        );
        assert!(
            matches!(&out[..], [Output::Event(WindowManagerEvent::WorkspaceChange(_, m, 1))] if *m == monitor),
            "{out:?}"
        );

        // the same again says nothing
        let out = t.handle(
            &json!({ "type": "current_desktops", "current": [{ "output": "DP-1", "id": "d2" }] }),
            Instant::now(),
        );
        assert!(out.is_empty());
    }

    #[test]
    fn komorebi_switching_a_monitor_comes_back_as_a_change_to_what_it_already_knows() {
        let mut t = table(vec![]);
        let monitor = t.monitors()[0].id;
        t.switch_commands(monitor, 1).unwrap();

        // the table already believes it, so KWin's report of it raises nothing a second time
        let out = t.handle(
            &json!({ "type": "current_desktops", "current": [{ "output": "DP-1", "id": "d2" }] }),
            Instant::now(),
        );
        assert!(out.is_empty(), "{out:?}");
    }

    #[test]
    fn a_new_list_of_desktops_is_read_and_raises_nothing() {
        let mut t = table(vec![win("a", &["d3"])]);
        assert_eq!(t.window_workspace(1), None, "d3 is not a desktop yet");

        let out = t.handle(
            &json!({ "type": "desktop_list", "desktops": [
                { "id": "d1", "name": "Desktop 1" },
                { "id": "d2", "name": "Desktop 2" },
                { "id": "d3", "name": "mail" },
            ] }),
            Instant::now(),
        );

        assert!(out.is_empty(), "{out:?}");
        assert_eq!(t.workspace_count(), 3);
        assert_eq!(t.window_workspace(1), Some(2));
    }

    #[test]
    fn a_cursor_query_answer_is_passed_on_and_remembered() {
        let mut t = table(vec![]);
        let out = t.handle(
            &json!({ "type": "cursor", "token": 7, "x": 300, "y": 400 }),
            Instant::now(),
        );

        assert!(matches!(
            &out[..],
            [Output::Cursor {
                token: 7,
                x: 300,
                y: 400
            }]
        ));
        assert_eq!(t.cursor(), Some((300, 400)));
    }

    #[test]
    fn a_requested_rect_is_visible_at_once_and_replaced_by_the_real_one() {
        let mut t = table(vec![win("a", &["d1"])]);
        t.note_requested_rect(
            1,
            Rect {
                left: 0,
                top: 0,
                right: 960,
                bottom: 1050,
            },
        );
        assert_eq!(t.window(1).unwrap().rect.right, 960);

        t.handle(
            &json!({ "type": "geometry", "id": "a", "geo": {"x":0,"y":0,"w":956,"h":1048} }),
            Instant::now(),
        );
        assert_eq!(
            t.window(1).unwrap().rect.right,
            956,
            "the client rounded it to its cell grid"
        );
    }

    #[test]
    fn when_kwin_goes_away_every_window_it_had_is_destroyed() {
        let mut t = table(vec![win("a", &["d1"]), win("b", &["d1"])]);
        t.handle(&json!({ "type": "activated", "id": "a" }), Instant::now());

        let mut destroyed = events(&t.compositor_lost(Instant::now()));
        destroyed.sort();
        assert_eq!(destroyed, vec!["Destroy 1", "Destroy 2"]);
        assert_eq!(t.foreground(), None);
        assert!(t.windows().is_empty());
        assert!(
            t.compositor_lost(Instant::now()).is_empty(),
            "a window is destroyed once"
        );

        // the windows of the KWin that comes back are new ones, with ids of their own
        t.handle(&snapshot(vec![win("c", &["d1"])]), Instant::now());
        let ids: Vec<u32> = t.windows().iter().map(|w| w.id).collect();
        assert_eq!(ids, vec![3]);
    }
}
