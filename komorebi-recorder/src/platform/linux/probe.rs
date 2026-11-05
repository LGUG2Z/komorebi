//! What KWin says about a window, at the moment komorebi for KDE reports an event about it
//! (`record --probe-windows`).
//!
//! komorebi for KDE hides a window by moving it to one more virtual desktop, `komorebi-park`, that
//! nobody looks at: the window keeps its place and its frame, so komorebi's `State` cannot say
//! whether a window is really hidden any more than it can on Windows. KWin can, and it is asked
//! here, as `inspect` of Windows asks Windows: `org.kde.KWin.getWindowInfo` answers for a window by
//! the UUID KWin has for it, with the desktops it is on, whether it is minimized, and its frame.
//!
//! # Finding the window
//!
//! komorebi's id for a window is a number it counts out as it meets windows, and it does not say
//! the UUID. So the window of an event is found by what komorebi says about it, which is KWin's:
//! the class, the title, and the frame. KWin's own windows runner (`/WindowsRunner`, which KRunner
//! asks) lists the windows that match a word, and each is asked for its info. A window is taken
//! when its class and title are the ones given and its frame is the one given too; one that is
//! the only window with that class and title is taken whatever its frame, which it may have
//! moved from since komorebi read it. A window that is not told from another by any of these is not
//! guessed at: it has no facts, as a window Windows could not read has none. Once found, a window
//! is remembered by its id, and asked about by its UUID from then on.
//!
//! # What the facts say
//!
//! | fact | KWin |
//! |---|---|
//! | `exists` | KWin answers for the UUID |
//! | `minimized` | `minimized` |
//! | `cloaked` | the only desktop it is on is `komorebi-park`: komorebi's way of putting a window away |
//! | `visible` | the window is on the desktop being shown, or on every desktop: komorebi shows each of its workspaces as a virtual desktop, so a window of a workspace that is not showing is not visible. (With a desktop for each output, which Plasma can do, the one desktop asked for is the active output's) |
//! | `rect` | the frame, as `[left, top, right, bottom]` |

use crate::sources::winprobe::WindowFacts;
use serde_json::Value;
use std::collections::HashMap;
use std::collections::HashSet;
use std::sync::Mutex;
use std::sync::OnceLock;
use zbus::blocking::Connection;
use zbus::zvariant::OwnedValue;

/// The name of the desktop komorebi puts windows it hides on.
const PARK: &str = "komorebi-park";

/// What komorebi says a window is, and so what to look for.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Wanted {
    /// The window's class, which is its application id.
    pub class: String,
    /// The window's title.
    pub title: String,
    /// The frame komorebi has for it, as `[left, top, width, height]`.
    pub frame: [i64; 4],
}

/// What KWin says of one window: the parts of `getWindowInfo` that matter here.
#[derive(Debug, Clone, PartialEq)]
pub struct Info {
    /// KWin's UUID for the window.
    pub uuid: String,
    /// The class of the window.
    pub class: String,
    /// The title of the window.
    pub caption: String,
    /// Whether the window is minimized.
    pub minimized: bool,
    /// The ids of the desktops the window is on.
    pub desktops: Vec<String>,
    /// The frame, as `[left, top, width, height]`.
    pub frame: [i64; 4],
}

impl Info {
    fn same_kind(&self, wanted: &Wanted) -> bool {
        self.class == wanted.class && self.caption == wanted.title
    }
}

/// Returns the window of `candidates` that is the one `wanted` describes: the one of that class and
/// title that has that frame, or the only one of that class and title. `None` if there is none, or
/// if several are and none has the frame.
#[must_use]
pub fn find<'a>(candidates: &'a [Info], wanted: &Wanted) -> Option<&'a Info> {
    let kind: Vec<&Info> = candidates
        .iter()
        .filter(|info| info.same_kind(wanted))
        .collect();
    let exactly: Vec<&Info> = kind
        .iter()
        .copied()
        .filter(|info| info.frame == wanted.frame)
        .collect();
    match (exactly.as_slice(), kind.as_slice()) {
        ([one], _) | ([], [one]) => Some(one),
        _ => None,
    }
}

/// The desktops of KWin that matter to a window: the one komorebi parks windows on, if it has made it,
/// and the one being shown.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Desktops {
    /// The id of the parking desktop.
    pub park: Option<String>,
    /// The id of the desktop that is showing.
    pub current: Option<String>,
}

/// What a window's facts are, given what KWin says and the desktops...
#[must_use]
pub fn facts_of(hwnd: u32, info: &Info, desktops: &Desktops) -> WindowFacts {
    let [left, top, width, height] = info.frame;
    let clamp = |value: i64| i32::try_from(value).unwrap_or(0);
    WindowFacts {
        hwnd,
        at_ns: 0,
        exists: true,
        visible: desktops
            .current
            .as_ref()
            .is_none_or(|current| info.desktops.is_empty() || info.desktops.contains(current)),
        cloaked: desktops
            .park
            .as_ref()
            .is_some_and(|park| info.desktops.len() == 1 && info.desktops[0] == *park),
        minimized: info.minimized,
        rect: Some([
            clamp(left),
            clamp(top),
            clamp(left + width),
            clamp(top + height),
        ]),
        komorebi_says: None,
    }
}

/// The facts of a window KWin has no more.
fn gone(hwnd: u32) -> WindowFacts {
    WindowFacts {
        hwnd,
        at_ns: 0,
        exists: false,
        visible: false,
        cloaked: false,
        minimized: false,
        rect: None,
        komorebi_says: None,
    }
}

/// What the event's window says it is: the `[notification, window]` of the event's content.
fn wanted_of(window: &Value) -> Option<Wanted> {
    let details = window.get("details")?;
    let rect = window.get("rect")?;
    let number = |key: &str| {
        rect.get(key)
            .and_then(Value::as_f64)
            .map(|n| n.round() as i64)
    };
    Some(Wanted {
        class: details.get("class")?.as_str()?.to_string(),
        title: details.get("title")?.as_str()?.to_string(),
        frame: [
            number("left")?,
            number("top")?,
            number("right")?,
            number("bottom")?,
        ],
    })
}

fn bus() -> Option<&'static Connection> {
    static BUS: OnceLock<Option<Connection>> = OnceLock::new();
    BUS.get_or_init(|| Connection::session().ok()).as_ref()
}

fn string_of(map: &HashMap<String, OwnedValue>, key: &str) -> Option<String> {
    String::try_from(map.get(key)?.try_clone().ok()?).ok()
}

fn number_of(map: &HashMap<String, OwnedValue>, key: &str) -> Option<i64> {
    let value = f64::try_from(map.get(key)?.try_clone().ok()?).ok()?;
    #[allow(clippy::cast_possible_truncation)]
    Some(value.round() as i64)
}

/// Asks KWin about the window with this UUID. `None` when it has none.
fn info_of(bus: &Connection, uuid: &str) -> Option<Info> {
    let map: HashMap<String, OwnedValue> = bus
        .call_method(
            Some("org.kde.KWin"),
            "/KWin",
            Some("org.kde.KWin"),
            "getWindowInfo",
            &(uuid,),
        )
        .ok()?
        .body()
        .deserialize()
        .ok()?;
    Some(Info {
        uuid: string_of(&map, "uuid")?,
        class: string_of(&map, "resourceClass")?,
        caption: string_of(&map, "caption")?,
        minimized: bool::try_from(map.get("minimized")?.try_clone().ok()?).ok()?,
        desktops: Vec::<String>::try_from(map.get("desktops")?.try_clone().ok()?).ok()?,
        frame: [
            number_of(&map, "x")?,
            number_of(&map, "y")?,
            number_of(&map, "width")?,
            number_of(&map, "height")?,
        ],
    })
}

/// The UUIDs of the windows KWin's windows runner lists for a query.
fn runner_uuids(bus: &Connection, query: &str) -> Vec<String> {
    type Match = (
        String,
        String,
        String,
        i32,
        f64,
        HashMap<String, OwnedValue>,
    );
    let matches: Vec<Match> = bus
        .call_method(
            Some("org.kde.KWin"),
            "/WindowsRunner",
            Some("org.kde.krunner1"),
            "Match",
            &(query,),
        )
        .ok()
        .and_then(|reply| reply.body().deserialize().ok())
        .unwrap_or_default();
    matches
        .into_iter()
        .filter_map(|(id, ..)| {
            // an id is the action, an underscore, and the UUID
            id.split_once('_').map(|(_, uuid)| uuid.to_string())
        })
        .collect()
}

/// The desktops there are now.
fn desktops_of(bus: &Connection) -> Desktops {
    Desktops {
        park: park_id(bus),
        current: current_id(bus),
    }
}

/// The id of the desktop that is showing.
fn current_id(bus: &Connection) -> Option<String> {
    let reply = bus
        .call_method(
            Some("org.kde.KWin"),
            "/VirtualDesktopManager",
            Some("org.freedesktop.DBus.Properties"),
            "Get",
            &("org.kde.KWin.VirtualDesktopManager", "current"),
        )
        .ok()?;
    let value: OwnedValue = reply.body().deserialize().ok()?;
    value.try_into().ok()
}

/// The id of the parking desktop, if komorebi has made it.
fn park_id(bus: &Connection) -> Option<String> {
    let reply = bus
        .call_method(
            Some("org.kde.KWin"),
            "/VirtualDesktopManager",
            Some("org.freedesktop.DBus.Properties"),
            "Get",
            &("org.kde.KWin.VirtualDesktopManager", "desktops"),
        )
        .ok()?;
    let value: OwnedValue = reply.body().deserialize().ok()?;
    let desktops: Vec<(u32, String, String)> = value.try_into().ok()?;
    desktops
        .into_iter()
        .find(|(_, _, name)| name == PARK)
        .map(|(_, id, _)| id)
}

/// The window of each komorebi id that was found, by UUID.
fn bound() -> &'static Mutex<HashMap<u32, String>> {
    static BOUND: OnceLock<Mutex<HashMap<u32, String>>> = OnceLock::new();
    BOUND.get_or_init(Mutex::default)
}

/// Returns what KWin says about the window `hwnd` of an event, whose window the event carries as
/// `window` (komorebi's own description of it), or `None` if KWin cannot be asked or the window
/// cannot be told from the others.
#[must_use]
pub fn inspect(hwnd: u32, window: &Value) -> Option<WindowFacts> {
    let bus = bus()?;
    let known = bound().lock().ok()?.get(&hwnd).cloned();
    if let Some(uuid) = known {
        return Some(match info_of(bus, &uuid) {
            Some(info) => facts_of(hwnd, &info, &desktops_of(bus)),
            None => {
                bound().lock().ok()?.remove(&hwnd);
                gone(hwnd)
            }
        });
    }

    let wanted = wanted_of(window)?;
    let mut seen = HashSet::new();
    let mut candidates = Vec::new();
    for query in [wanted.class.as_str(), wanted.title.as_str()] {
        for uuid in runner_uuids(bus, query) {
            if seen.insert(uuid.clone()) {
                candidates.extend(info_of(bus, &uuid));
            }
        }
        if let Some(info) = find(&candidates, &wanted) {
            bound().lock().ok()?.insert(hwnd, info.uuid.clone());
            return Some(facts_of(hwnd, info, &desktops_of(bus)));
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn info(uuid: &str, caption: &str, frame: [i64; 4], desktops: &[&str]) -> Info {
        Info {
            uuid: uuid.to_string(),
            class: "org.kde.konsole".to_string(),
            caption: caption.to_string(),
            minimized: false,
            desktops: desktops.iter().map(ToString::to_string).collect(),
            frame,
        }
    }

    fn wanted(title: &str, frame: [i64; 4]) -> Wanted {
        Wanted {
            class: "org.kde.konsole".to_string(),
            title: title.to_string(),
            frame,
        }
    }

    #[test]
    fn a_window_is_found_by_its_frame_among_ones_of_the_same_title() {
        let windows = [
            info("a", "~ : bash", [0, 0, 500, 400], &["1"]),
            info("b", "~ : bash", [500, 0, 500, 400], &["1"]),
        ];
        let found = find(&windows, &wanted("~ : bash", [500, 0, 500, 400])).expect("found");
        assert_eq!(found.uuid, "b");
    }

    #[test]
    fn the_only_window_of_a_title_is_found_where_it_has_moved_to() {
        let windows = [
            info("a", "repo", [0, 0, 500, 400], &["1"]),
            info("b", "~ : bash", [500, 0, 500, 400], &["1"]),
        ];
        let found = find(&windows, &wanted("repo", [10, 10, 400, 300])).expect("found");
        assert_eq!(found.uuid, "a");
    }

    #[test]
    fn windows_that_cannot_be_told_apart_are_not_guessed_at() {
        let windows = [
            info("a", "~ : bash", [0, 0, 500, 400], &["1"]),
            info("b", "~ : bash", [500, 0, 500, 400], &["1"]),
        ];
        assert_eq!(find(&windows, &wanted("~ : bash", [9, 9, 9, 9])), None);
        assert_eq!(find(&windows, &wanted("other", [0, 0, 500, 400])), None);
        assert_eq!(find(&[], &wanted("x", [0, 0, 1, 1])), None);
    }

    fn desktops(park: Option<&str>, current: Option<&str>) -> Desktops {
        Desktops {
            park: park.map(ToString::to_string),
            current: current.map(ToString::to_string),
        }
    }

    #[test]
    fn a_window_on_the_parking_desktop_alone_is_cloaked() {
        let parked = info("a", "t", [10, 20, 300, 200], &["park"]);
        let facts = facts_of(7, &parked, &desktops(Some("park"), Some("1")));
        assert!(facts.cloaked && !facts.shown());
        assert_eq!(facts.hidden_how(), Some("cloaked"));
        assert_eq!(facts.rect, Some([10, 20, 310, 220]));

        // with no parking desktop made, nothing is parked
        assert!(!facts_of(7, &parked, &desktops(None, None)).cloaked);
    }

    #[test]
    fn a_window_of_a_desktop_that_is_not_showing_is_not_visible() {
        let window = info("a", "t", [0, 0, 1, 1], &["1"]);
        assert!(facts_of(7, &window, &desktops(None, Some("1"))).shown());
        let away = facts_of(7, &window, &desktops(None, Some("2")));
        assert_eq!(away.hidden_how(), Some("not visible"));
        // a window on every desktop is on the one that shows, and so is one when KWin cannot say
        let everywhere = info("a", "t", [0, 0, 1, 1], &[]);
        assert!(facts_of(7, &everywhere, &desktops(None, Some("2"))).shown());
        assert!(facts_of(7, &window, &desktops(None, None)).shown());
    }

    #[test]
    fn a_minimized_window_is_minimized_and_not_cloaked() {
        let mut window = info("a", "t", [0, 0, 1, 1], &["1"]);
        window.minimized = true;
        let facts = facts_of(7, &window, &desktops(Some("park"), Some("1")));
        assert_eq!(facts.hidden_how(), Some("minimized"));
    }

    #[test]
    fn what_komorebi_says_of_a_window_is_what_is_looked_for() {
        let window = json!({
            "id": 4,
            "rect": {"left": 41, "top": 41, "right": 1213, "bottom": 952},
            "details": {"title": "repo : bash", "exe": "konsole", "class": "org.kde.konsole", "path": "/x"}
        });
        assert_eq!(
            wanted_of(&window),
            Some(wanted("repo : bash", [41, 41, 1213, 952]))
        );
        assert_eq!(wanted_of(&json!({"id": 4})), None);
    }
}
