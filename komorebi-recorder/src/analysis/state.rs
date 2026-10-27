//! komorebi's `State`, as a model that can be asked questions.
//!
//! Every subscriber notification carries the whole `State` serialized as JSON, and the recorder
//! stores each distinct one. The JSON is read tolerantly into a [`Snapshot`]: unknown fields are
//! ignored and missing ones default, so a komorebi newer than this recorder still reads.
//!
//! Two facts about it that matter when reading it:
//!
//! - A window's `rect` is **not** komorebi's bookkeeping. It is read live from the accessibility
//!   API at the moment the state is serialized, so it says where the window *really* was.
//!   Comparing it with where komorebi's layout says it should be (`layout_rects`) finds windows
//!   that are in the wrong place.
//! - komorebi's `Rect` is `{left, top, right, bottom}` where `right` is the **width** and `bottom`
//!   the **height**, in global display points. [`Rect`] uses honest names. komorebi for Windows
//!   writes its `Rect` the same way.
//!
//! The two flavors of komorebi do not write a window the same way. A Mac window is keyed by `id`
//! and keeps its title and program under `details`; a Windows window is keyed by its handle,
//! `hwnd`, with its title and program beside it. The flavor's [`Rules`] know which, and
//! [`Snapshot::parse`] asks them, so the rest of the analysis sees one model: a [`Snapshot`] of
//! [`Monitor`]s, [`Workspace`]s, [`Container`]s and [`Window`]s.
//!
//! This module reads and does not judge. Whether the state is consistent (a window in two places, a
//! layout that was not applied) is for the invariant checks of the analysis, which are built on
//! these types.
//!
//! # Example
//!
//! A one-window state as each flavor writes it, read into the same model:
//!
//! ```rust
//! use komorebi_recorder::analysis::flavor::Flavor;
//! use komorebi_recorder::analysis::state::{Place, Rect, Snapshot};
//! use serde_json::json;
//!
//! // a workspace with one container, containing the given window, on monitor 1
//! let state = |window: serde_json::Value| json!({
//!     "monitors": {"elements": [{
//!         "id": 1,
//!         "workspaces": {"elements": [{
//!             "name": "one",
//!             "containers": {"elements": [
//!                 {"id": "c", "windows": {"elements": [window], "focused": 0}}
//!             ], "focused": 0}
//!         }], "focused": 0}
//!     }], "focused": 0}
//! });
//!
//! let mac = Snapshot::parse(Flavor::Macos, &state(json!({
//!     "id": 59,
//!     "details": {"title": "Notes", "exe": "Notes"},
//!     "rect": {"left": 10, "top": 10, "right": 1686, "bottom": 1420}
//! })));
//! let (location, window) = mac.find(59).expect("the window");
//! assert_eq!(window.exe, "Notes");
//! // komorebi's `right` and `bottom` are a width and a height
//! assert_eq!(
//!     window.rect,
//!     Some(Rect { left: 10, top: 10, width: 1686, height: 1420 })
//! );
//! assert_eq!(location.place, Place::Container { index: 0, tab: 0 });
//! assert_eq!(location.describe(), "monitor 1 workspace one (container 0)");
//!
//! let windows = Snapshot::parse(Flavor::Windows, &state(json!({
//!     "hwnd": 131072, "title": "Notes", "exe": "notepad.exe",
//!     "rect": {"left": 0, "top": 0, "right": 100, "bottom": 100}
//! })));
//! assert_eq!(windows.find(131_072).expect("the window").1.exe, "notepad.exe");
//!
//! // only komorebi for Mac hides windows by position
//! assert!(mac.hides_windows_by_position());
//! assert!(!windows.hides_windows_by_position());
//! ```

use super::flavor::Flavor;
use super::flavor::Rules;
use schemars::JsonSchema;
use serde::Serialize;
use serde_json::Value;

/// A rectangle in komorebi's display coordinates, with names that say what the numbers are.
///
/// komorebi writes a rectangle as `{left, top, right, bottom}`, where `right` and `bottom` are a
/// width and a height. A `Rect` stores those two as [`Rect::width`] and [`Rect::height`], and
/// [`Rect::right_edge`] and [`Rect::bottom_edge`] give the edges.
///
/// A rect is not guaranteed to have a positive size: komorebi for Windows writes zeros for a window
/// it could not read. [`Rect::area`] counts a width or height below 0 as 0.
///
/// # Example
///
/// ```rust
/// use komorebi_recorder::analysis::state::Rect;
///
/// let a = Rect { left: 0, top: 0, width: 10, height: 10 };
/// let b = Rect { left: 5, top: 5, width: 10, height: 10 };
///
/// assert_eq!(a.right_edge(), 10);
/// assert_eq!(a.overlap_area(&b), 25);
/// assert_eq!(a.max_delta(&b), 5);
/// assert_eq!(a.describe(), "10x10 at (0, 0)");
///
/// // rects that only touch share no area
/// let c = Rect { left: 10, top: 0, width: 5, height: 5 };
/// assert_eq!(a.overlap_area(&c), 0);
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, JsonSchema)]
pub struct Rect {
    /// The x coordinate of the left edge.
    pub left: i64,
    /// The y coordinate of the top edge.
    pub top: i64,
    /// The width: komorebi's `right`.
    pub width: i64,
    /// The height: komorebi's `bottom`.
    pub height: i64,
}

impl Rect {
    /// Returns the x coordinate of the right edge: `left + width`.
    #[must_use]
    pub fn right_edge(&self) -> i64 {
        self.left + self.width
    }

    /// Returns the y coordinate of the bottom edge: `top + height`.
    #[must_use]
    pub fn bottom_edge(&self) -> i64 {
        self.top + self.height
    }

    /// Returns the area the two rectangles share, or 0 if they do not overlap (touching edges do
    /// not overlap).
    #[must_use]
    pub fn overlap_area(&self, other: &Rect) -> i64 {
        let width = self.right_edge().min(other.right_edge()) - self.left.max(other.left);
        let height = self.bottom_edge().min(other.bottom_edge()) - self.top.max(other.top);
        if width <= 0 || height <= 0 {
            0
        } else {
            width * height
        }
    }

    /// Returns the rectangle's area. A width or height below 0 counts as 0.
    #[must_use]
    pub fn area(&self) -> i64 {
        self.width.max(0) * self.height.max(0)
    }

    /// Returns the largest difference in any of the four numbers (`left`, `top`, `width`,
    /// `height`) between the two rectangles: 0 if they are equal.
    #[must_use]
    pub fn max_delta(&self, other: &Rect) -> i64 {
        [
            (self.left - other.left).abs(),
            (self.top - other.top).abs(),
            (self.width - other.width).abs(),
            (self.height - other.height).abs(),
        ]
        .into_iter()
        .max()
        .unwrap_or(0)
    }

    /// Returns the rectangle as text for a reader: `1686x1420 at (10, 10)`, the size and then the
    /// position of the top left corner.
    #[must_use]
    pub fn describe(&self) -> String {
        format!(
            "{}x{} at ({}, {})",
            self.width, self.height, self.left, self.top
        )
    }

    fn from_value(value: &Value) -> Option<Self> {
        let number = |key: &str| {
            let field = value.get(key)?;
            field
                .as_i64()
                .or_else(|| field.as_f64().map(|float| float.round() as i64))
        };

        Some(Self {
            left: number("left")?,
            top: number("top")?,
            // komorebi's `right` and `bottom` are a width and a height
            width: number("right")?,
            height: number("bottom")?,
        })
    }
}

/// A window that appears in komorebi's state, as the state describes it.
///
/// Fields that the state does not have (a komorebi that does not write them, or one newer than this
/// recorder) are empty text, not an error.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, JsonSchema)]
pub struct Window {
    /// The window's id in the state: a window id on a Mac, a handle on Windows
    /// ([`Rules::window_id`]).
    pub id: u64,
    /// The window's title. Empty if the state has none. For a window komorebi for Windows could not
    /// read, it is komorebi's error text (`could not get window title`), not a title.
    pub title: String,
    /// The program: an application name on a Mac, an executable file name on Windows. Empty if the
    /// state has none, and komorebi's error text for a window it could not read.
    pub exe: String,
    /// The window's class, which komorebi for Windows reads from the window (`Chrome_WidgetWin_1`).
    /// Empty on a Mac, which has none, and when the state has none or komorebi could not read the
    /// window (its error text is not a class).
    pub class: String,
    /// Where the window really was when the state was serialized (a live read from the OS, not
    /// komorebi's bookkeeping). `None` if the state has no rect for it, or if komorebi could not read
    /// the window ([`Rules::window_is_unreadable`]), so that a rect of zeros is not mistaken for a
    /// window parked at a corner.
    pub rect: Option<Rect>,
}

/// A container: one tile of a workspace, holding one window or a stack of tabbed ones.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, JsonSchema)]
pub struct Container {
    /// komorebi's identifier for the container.
    pub id: String,
    /// The windows in the container, one tile's worth: more than one when they are tabbed or
    /// stacked.
    pub windows: Vec<Window>,
    /// Which of `windows` is in front: an index into `windows`. 0 if the state does not say.
    pub focused: usize,
    /// Whether the state has the container as locked (`locked`); `false` if it does not say.
    pub locked: bool,
}

impl Container {
    /// Returns the window in front, or `None` if `focused` is not an index of `windows` (an empty
    /// container, for one).
    #[must_use]
    pub fn focused_window(&self) -> Option<&Window> {
        self.windows.get(self.focused)
    }
}

/// A workspace of a monitor: its tiled containers, and its monocle, maximized and floating windows.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, JsonSchema)]
pub struct Workspace {
    /// The workspace's name; empty if it has none.
    pub name: String,
    /// The tiled containers, in komorebi's order.
    pub containers: Vec<Container>,
    /// Which of `containers` has focus: an index into `containers`. 0 if the state does not say.
    pub focused: usize,
    /// A container filling the workspace on its own.
    pub monocle: Option<Container>,
    /// The window maximized on the workspace, if there is one.
    pub maximized: Option<Window>,
    /// The windows floating above the tiles.
    pub floating: Vec<Window>,
    /// Which of `floating` has focus: an index into `floating`. 0 if the state does not say.
    pub floating_focused: usize,
    /// `Tiling` or `Floating`: which layer has the focus.
    pub layer: String,
    /// The layout's short name: `Columns`, `BSP`, ... Empty if the state has none.
    pub layout: String,
    /// Where komorebi's last layout pass put each container, by container index.
    pub layout_rects: Vec<Rect>,
    /// Whether tiling is on for the workspace. `true` if the state does not say.
    pub tiling: bool,
}

/// A monitor and its workspaces.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, JsonSchema)]
pub struct Monitor {
    /// The display id: the same number as the `--display` of the recording.
    pub id: u64,
    /// The display's name, as komorebi has it (`Built-in Retina Display`).
    pub device: String,
    /// The monitor's whole area.
    pub size: Rect,
    /// The monitor's work area, from `work_area_size`.
    pub work_area: Rect,
    /// The monitor's workspaces, in komorebi's order.
    pub workspaces: Vec<Workspace>,
    /// Which of `workspaces` is showing: an index into `workspaces`. 0 if the state does not say.
    pub focused: usize,
    /// `BottomLeft` or `BottomRight`: the corner komorebi hides windows in. Empty if the state does
    /// not say.
    pub hiding_position: String,
}

impl Monitor {
    /// Returns the workspace that is showing, or `None` if `focused` is not an index of
    /// `workspaces`.
    #[must_use]
    pub fn focused_workspace(&self) -> Option<&Workspace> {
        self.workspaces.get(self.focused)
    }
}

/// Where a window is in the tree: on which monitor and workspace, and in which place on it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, JsonSchema)]
pub struct Location {
    /// The [`Monitor::id`] of the monitor.
    pub monitor_id: u64,
    /// The name of the workspace.
    pub workspace: String,
    /// Where on the workspace.
    pub place: Place,
    /// Whether that workspace is the one showing on its monitor.
    pub workspace_visible: bool,
}

/// Where on a workspace a window is.
///
/// Serialized as an object with the kebab-case variant name in `kind`: `{"kind": "container",
/// "index": 2, "tab": 0}`, `{"kind": "maximized"}`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, JsonSchema)]
#[serde(tag = "kind", rename_all = "kebab-case")]
pub enum Place {
    /// In a tiled container.
    Container {
        /// Which of the workspace's containers: an index into [`Workspace::containers`].
        index: usize,
        /// Which of the container's windows: an index into [`Container::windows`], 0 for the
        /// first.
        tab: usize,
    },
    /// In the container that fills the workspace.
    Monocle {
        /// Which of the container's windows.
        tab: usize,
    },
    /// The window maximized on the workspace.
    Maximized,
    /// Floating above the tiles.
    Floating {
        /// Which of the workspace's floating windows: an index into [`Workspace::floating`].
        index: usize,
    },
}

impl Place {
    /// Returns the place as text for a reader: `container 2`, `container 2 tab 1` (the tab only
    /// when it is not the first), `monocle`, `maximized` or `floating 0`.
    ///
    /// # Example
    ///
    /// ```rust
    /// use komorebi_recorder::analysis::state::Place;
    ///
    /// assert_eq!(Place::Container { index: 2, tab: 0 }.describe(), "container 2");
    /// assert_eq!(Place::Container { index: 2, tab: 1 }.describe(), "container 2 tab 1");
    /// assert_eq!(Place::Maximized.describe(), "maximized");
    /// ```
    #[must_use]
    pub fn describe(&self) -> String {
        match self {
            Self::Container { index, tab } if *tab > 0 => format!("container {index} tab {tab}"),
            Self::Container { index, .. } => format!("container {index}"),
            Self::Monocle { .. } => "monocle".to_string(),
            Self::Maximized => "maximized".to_string(),
            Self::Floating { index } => format!("floating {index}"),
        }
    }
}

impl Location {
    /// Returns the location as text for a reader: `monitor 3 workspace one (container 0)`.
    #[must_use]
    pub fn describe(&self) -> String {
        format!(
            "monitor {} workspace {} ({})",
            self.monitor_id,
            self.workspace,
            self.place.describe()
        )
    }
}

/// The monitor, workspace and window that have the keyboard focus, as komorebi's state has them.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, JsonSchema)]
pub struct Focus {
    /// The [`Monitor::id`] of the focused monitor.
    pub monitor_id: u64,
    /// The name of its focused workspace.
    pub workspace: String,
    /// The focused window's [`Window::id`]; `None` if the workspace has no window to focus.
    pub window_id: Option<u64>,
    /// Where the focused window is on the workspace; `None` when there is none.
    pub place: Option<Place>,
}

/// komorebi's whole `State` at one moment, read into a model.
///
/// A snapshot is built with [`Snapshot::parse`], which never fails: a state with fields missing or
/// unknown reads with what is there, and what is not is empty or a default. It records which
/// [`Flavor`] wrote it, because what a window's position says depends on that (see
/// [`Snapshot::hides_windows_by_position`]); the flavor is not part of the serialized form.
///
/// See the [module documentation](self) for an example.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, JsonSchema)]
pub struct Snapshot {
    /// Which komorebi wrote this state, and so how to read the parts of it that mean different
    /// things in each. Not part of the state itself.
    #[serde(skip)]
    #[schemars(skip)]
    pub flavor: Flavor,
    /// The monitors, in komorebi's order.
    pub monitors: Vec<Monitor>,
    /// Which of `monitors` has focus: an index into `monitors`. 0 if the state does not say.
    pub focused_monitor: usize,
    /// Whether komorebi was paused (`is_paused`); `false` if the state does not say.
    pub paused: bool,
}

fn array<'a>(value: &'a Value, key: &str) -> &'a [Value] {
    value
        .get(key)
        .and_then(|ring| ring.get("elements"))
        .and_then(Value::as_array)
        .map_or(&[], Vec::as_slice)
}

fn focused(value: &Value, key: &str) -> usize {
    value
        .get(key)
        .and_then(|ring| ring.get("focused"))
        .and_then(Value::as_u64)
        .map_or(0, |index| index as usize)
}

fn text(value: &Value, key: &str) -> String {
    value
        .get(key)
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_string()
}

fn window(rules: &dyn Rules, value: &Value) -> Option<Window> {
    let id = rules.window_id(value)?;
    // a mac window keeps its title and program under `details`, a Windows one beside the rest
    let details = value.get("details").unwrap_or(value);
    // what komorebi could not read it does not guess: its rect is a zero and its title an error
    // message, which would read as a window parked at the corner of the screen
    let unreadable = rules.window_is_unreadable(value);
    Some(Window {
        id,
        title: text(details, "title"),
        exe: text(details, "exe"),
        class: if unreadable {
            String::new()
        } else {
            text(details, "class")
        },
        rect: value
            .get("rect")
            .and_then(Rect::from_value)
            .filter(|_| !unreadable),
    })
}

fn container(rules: &dyn Rules, value: &Value) -> Container {
    Container {
        id: text(value, "id"),
        windows: array(value, "windows")
            .iter()
            .filter_map(|value| window(rules, value))
            .collect(),
        focused: focused(value, "windows"),
        locked: value
            .get("locked")
            .and_then(Value::as_bool)
            .unwrap_or(false),
    }
}

/// `{"Default": "Columns"}` or `"BSP"` or `{"Custom": ...}` as a short name.
fn layout_name(value: Option<&Value>) -> String {
    match value {
        Some(Value::String(name)) => name.clone(),
        Some(Value::Object(object)) => {
            object
                .iter()
                .next()
                .map_or_else(String::new, |(key, inner)| match inner.as_str() {
                    Some(name) => name.to_string(),
                    None => key.clone(),
                })
        }
        _ => String::new(),
    }
}

fn workspace(rules: &dyn Rules, value: &Value) -> Workspace {
    Workspace {
        name: text(value, "name"),
        containers: array(value, "containers")
            .iter()
            .map(|value| container(rules, value))
            .collect(),
        focused: focused(value, "containers"),
        monocle: value
            .get("monocle_container")
            .filter(|monocle| monocle.is_object())
            .map(|monocle| container(rules, monocle)),
        maximized: value
            .get("maximized_window")
            .and_then(|value| window(rules, value)),
        floating: array(value, "floating_windows")
            .iter()
            .filter_map(|value| window(rules, value))
            .collect(),
        floating_focused: focused(value, "floating_windows"),
        layer: text(value, "layer"),
        layout: layout_name(value.get("layout")),
        layout_rects: value
            .get("latest_layout")
            .and_then(Value::as_array)
            .map(|rects| rects.iter().filter_map(Rect::from_value).collect())
            .unwrap_or_default(),
        tiling: value.get("tile").and_then(Value::as_bool).unwrap_or(true),
    }
}

fn monitor(rules: &dyn Rules, value: &Value) -> Monitor {
    let rect = |key: &str| {
        value.get(key).and_then(Rect::from_value).unwrap_or(Rect {
            left: 0,
            top: 0,
            width: 0,
            height: 0,
        })
    };

    Monitor {
        id: value
            .get("id")
            .and_then(|id| id.as_u64().or_else(|| id.as_i64().map(|id| id as u64)))
            .unwrap_or(0),
        device: text(value, "device"),
        size: rect("size"),
        work_area: rect("work_area_size"),
        workspaces: array(value, "workspaces")
            .iter()
            .map(|value| workspace(rules, value))
            .collect(),
        focused: focused(value, "workspaces"),
        hiding_position: text(value, "window_hiding_position"),
    }
}

impl Snapshot {
    /// Returns the model of a `State` as the komorebi of `flavor` serialized it.
    ///
    /// This never fails. The JSON is read tolerantly: unknown fields are ignored, and a missing one
    /// is an empty list, empty text, 0 or `false` (`tiling` is `true`). A window the flavor's
    /// [`Rules::window_id`] does not recognize as one is left out. Numbers in a rect may be floats;
    /// they are rounded.
    ///
    /// `state` is what komorebi sent in a notification, or what the recorder stored in
    /// `states/<id>.json`. See the [module documentation](self) for an example.
    #[must_use]
    pub fn parse(flavor: Flavor, state: &Value) -> Self {
        Self {
            flavor,
            monitors: array(state, "monitors")
                .iter()
                .map(|value| monitor(flavor.rules(), value))
                .collect(),
            focused_monitor: focused(state, "monitors"),
            paused: state
                .get("is_paused")
                .and_then(Value::as_bool)
                .unwrap_or(false),
        }
    }

    /// A macOS state, for the tests, whose states all are.
    #[cfg(test)]
    #[must_use]
    pub fn from_value(state: &Value) -> Self {
        Self::parse(Flavor::Macos, state)
    }

    /// Returns every window with where it is, in order: by monitor, then workspace, then the
    /// tiled containers (each tab in turn), the monocle container, the maximized window and the
    /// floating windows.
    ///
    /// A window that the state lists in two places appears twice.
    #[must_use]
    pub fn windows(&self) -> Vec<(Location, &Window)> {
        let mut found = Vec::new();
        for monitor in &self.monitors {
            for (index, workspace) in monitor.workspaces.iter().enumerate() {
                let visible = index == monitor.focused;
                let at = |place: Place| Location {
                    monitor_id: monitor.id,
                    workspace: workspace.name.clone(),
                    place,
                    workspace_visible: visible,
                };

                for (container_index, container) in workspace.containers.iter().enumerate() {
                    for (tab, window) in container.windows.iter().enumerate() {
                        found.push((
                            at(Place::Container {
                                index: container_index,
                                tab,
                            }),
                            window,
                        ));
                    }
                }
                if let Some(monocle) = &workspace.monocle {
                    for (tab, window) in monocle.windows.iter().enumerate() {
                        found.push((at(Place::Monocle { tab }), window));
                    }
                }
                if let Some(window) = &workspace.maximized {
                    found.push((at(Place::Maximized), window));
                }
                for (floating_index, window) in workspace.floating.iter().enumerate() {
                    found.push((
                        at(Place::Floating {
                            index: floating_index,
                        }),
                        window,
                    ));
                }
            }
        }

        found
    }

    /// Returns the first place a window with this id is, in the order of [`Snapshot::windows`], or
    /// `None` if there is none.
    #[must_use]
    pub fn find(&self, window_id: u64) -> Option<(Location, &Window)> {
        self.windows()
            .into_iter()
            .find(|(_, window)| window.id == window_id)
    }

    /// Returns what has focus: the focused monitor's focused workspace's focused window, where a
    /// floating layer beats the tiled containers and a monocle container beats the rest.
    ///
    /// The layer wins only when the workspace's `layer` is `Floating` and it has floating windows.
    /// A `focused_monitor` that is not an index of `monitors` falls back to the first monitor. The
    /// result is `None` if there is no monitor or the monitor has no focused workspace; a focused
    /// workspace with nothing to focus is a [`Focus`] with no `window_id` and no `place`.
    #[must_use]
    pub fn focus(&self) -> Option<Focus> {
        let monitor = self
            .monitors
            .get(self.focused_monitor)
            .or_else(|| self.monitors.first())?;
        let workspace = monitor.focused_workspace()?;

        let (window, place) = if workspace.layer == "Floating" && !workspace.floating.is_empty() {
            let index = workspace
                .floating_focused
                .min(workspace.floating.len().saturating_sub(1));
            (
                workspace.floating.get(index),
                Some(Place::Floating { index }),
            )
        } else if let Some(monocle) = &workspace.monocle {
            (
                monocle.focused_window(),
                Some(Place::Monocle {
                    tab: monocle.focused,
                }),
            )
        } else if let Some(container) = workspace.containers.get(workspace.focused) {
            (
                container.focused_window(),
                Some(Place::Container {
                    index: workspace.focused,
                    tab: container.focused,
                }),
            )
        } else {
            (None, None)
        };

        Some(Focus {
            monitor_id: monitor.id,
            workspace: workspace.name.clone(),
            window_id: window.map(|window| window.id),
            place,
        })
    }

    /// Returns whether a window's position says if it is hidden: komorebi for Mac parks a hidden
    /// window in a corner, and komorebi for Windows by default cloaks it where it is, so there a
    /// hidden window is not told from a visible one by where it is.
    ///
    /// This is [`Rules::hides_windows_by_position`] of the snapshot's flavor. Checks that depend on
    /// where hidden windows are skip a snapshot for which it is `false`.
    #[must_use]
    pub fn hides_windows_by_position(&self) -> bool {
        self.flavor.rules().hides_windows_by_position()
    }

    /// Returns whether `rect` is where komorebi parks a window it has hidden on `monitor`. What that
    /// looks like is up to the flavor of komorebi that recorded the state: it is
    /// [`Rules::is_hidden_position`] of the snapshot's flavor.
    #[must_use]
    pub fn is_hidden_position(&self, rect: &Rect, monitor: &Monitor) -> bool {
        self.flavor.rules().is_hidden_position(rect, monitor)
    }

    /// Returns the monitor whose area holds most of `rect`, or `None` if `rect` overlaps none. On a
    /// tie, the monitor later in `monitors` is returned.
    #[must_use]
    pub fn monitor_under(&self, rect: &Rect) -> Option<&Monitor> {
        self.monitors
            .iter()
            .map(|monitor| (monitor.size.overlap_area(rect), monitor))
            .filter(|(area, _)| *area > 0)
            .max_by_key(|(area, _)| *area)
            .map(|(_, monitor)| monitor)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture() -> Snapshot {
        let state: Value =
            serde_json::from_str(include_str!("../../tests/fixtures/state.json")).expect("fixture");
        Snapshot::from_value(&state)
    }

    #[test]
    fn a_real_state_reads_into_monitors_workspaces_and_windows() {
        let snapshot = fixture();

        assert_eq!(snapshot.monitors.len(), 2);
        assert_eq!(snapshot.monitors[0].id, 1);
        assert_eq!(snapshot.monitors[0].device, "Built-in Retina Display");
        assert_eq!(snapshot.monitors[1].workspaces.len(), 10);
        assert!(!snapshot.paused);

        let windows = snapshot.windows();
        assert!(windows.len() > 15);
        let (location, slack) = snapshot.find(59).expect("window 59");
        assert_eq!(slack.exe, "Slack");
        assert_eq!(slack.title, "window 59");
        assert_eq!(location.monitor_id, 3);
        assert_eq!(location.workspace, "one");
        assert_eq!(location.place, Place::Container { index: 0, tab: 0 });
    }

    #[test]
    fn komorebis_right_and_bottom_are_read_as_width_and_height() {
        let snapshot = fixture();
        let layout = &snapshot.monitors[1].workspaces[0].layout_rects;
        // {"left": 10, "top": 10, "right": 1686, "bottom": 1420}
        assert_eq!(
            layout[0],
            Rect {
                left: 10,
                top: 10,
                width: 1686,
                height: 1420
            }
        );
        assert_eq!(layout[0].right_edge(), 1696);
    }

    #[test]
    fn layouts_have_short_names() {
        assert_eq!(fixture().monitors[1].workspaces[0].layout, "Columns");
        assert_eq!(layout_name(Some(&serde_json::json!("BSP"))), "BSP");
        assert_eq!(layout_name(None), "");
    }

    #[test]
    fn hidden_windows_sit_on_the_sliver_and_visible_ones_do_not() {
        let snapshot = fixture();
        let monitor = &snapshot.monitors[1];
        let (_, hidden) = snapshot
            .find(59)
            .expect("a window on a workspace that is not showing");
        assert!(snapshot.is_hidden_position(hidden.rect.as_ref().expect("rect"), monitor));

        let visible = Rect {
            left: 10,
            top: 10,
            width: 1686,
            height: 1420,
        };
        assert!(!snapshot.is_hidden_position(&visible, monitor));
    }

    #[test]
    fn focus_follows_the_focused_monitor_workspace_and_container() {
        let focus = fixture().focus().expect("focus");
        // the second monitor (display 3) is focused, on workspace "ten", at its third container
        assert_eq!(focus.monitor_id, 3);
        assert_eq!(focus.workspace, "ten");
        assert_eq!(focus.window_id, Some(8550));
        assert_eq!(focus.place, Some(Place::Container { index: 2, tab: 0 }));
    }

    fn workspace_state(extra: Value) -> Snapshot {
        let mut workspace = serde_json::json!({
            "name": "w", "layer": "Tiling",
            "containers": {"elements": [
                {"id": "a", "windows": {"elements": [{"id": 1, "rect": null}], "focused": 0}},
                {"id": "b", "windows": {"elements": [{"id": 2}], "focused": 0}}
            ], "focused": 1},
            "floating_windows": {"elements": [{"id": 3}], "focused": 0},
        });
        for (key, value) in extra.as_object().expect("object") {
            workspace[key] = value.clone();
        }
        Snapshot::from_value(&serde_json::json!({
            "monitors": {"elements": [{"id": 1, "workspaces": {"elements": [workspace], "focused": 0}}], "focused": 0}
        }))
    }

    #[test]
    fn a_monocle_container_beats_the_containers_and_a_floating_layer_beats_both() {
        let plain = workspace_state(serde_json::json!({}));
        assert_eq!(plain.focus().expect("focus").window_id, Some(2));

        let monocle = workspace_state(serde_json::json!({
            "monocle_container": {"id": "m", "windows": {"elements": [{"id": 9}], "focused": 0}}
        }));
        assert_eq!(monocle.focus().expect("focus").window_id, Some(9));

        let floating = workspace_state(serde_json::json!({
            "layer": "Floating",
            "monocle_container": {"id": "m", "windows": {"elements": [{"id": 9}], "focused": 0}}
        }));
        assert_eq!(floating.focus().expect("focus").window_id, Some(3));
    }

    #[test]
    fn missing_and_unknown_fields_do_not_break_reading() {
        let snapshot = Snapshot::from_value(&serde_json::json!({"brand_new": 1}));
        assert!(snapshot.monitors.is_empty());
        assert!(snapshot.focus().is_none());

        let odd = Snapshot::from_value(&serde_json::json!({
            "monitors": {"elements": [{"id": 1, "future_field": [1, 2], "workspaces": {"elements": [{"name": "x", "monocle_container": null}]}}]}
        }));
        assert_eq!(odd.monitors[0].workspaces[0].name, "x");
        assert!(odd.monitors[0].workspaces[0].monocle.is_none());
    }

    #[test]
    fn a_window_without_a_rect_has_none_and_overlap_is_computed() {
        let snapshot = workspace_state(serde_json::json!({}));
        assert!(snapshot.find(1).expect("window").1.rect.is_none());

        let a = Rect {
            left: 0,
            top: 0,
            width: 10,
            height: 10,
        };
        let b = Rect {
            left: 5,
            top: 5,
            width: 10,
            height: 10,
        };
        assert_eq!(a.overlap_area(&b), 25);
        assert_eq!(
            a.overlap_area(&Rect {
                left: 20,
                top: 0,
                width: 5,
                height: 5
            }),
            0
        );
        assert_eq!(a.max_delta(&b), 5);
    }
}
