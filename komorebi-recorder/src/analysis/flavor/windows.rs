//! komorebi for Windows, as it describes itself.
//!
//! [`WindowsRules`] is the [`Rules`] of [`Flavor::Windows`]. It overrides all three methods that
//! have a default: [`Rules::verified`], [`Rules::classify_monitor`] and
//! [`Rules::window_is_unreadable`].
//!
//! The rules are written from komorebi for Windows' source and not yet checked against a recording
//! of it. That is what [`Rules::verified`] reports, and what the analysis tells whoever reads a
//! Windows session. What the rules rely on:
//!
//! - A window event serializes as `{"type": "FocusChange", "content": ["<WinEvent>", <window>]}`,
//!   or `{"type": "Manage", "content": <window>}` for the three that have no `WinEvent` (`Manage`,
//!   `Unmanage`, `Raise`). A window is `{hwnd, title, exe, class, rect}`, every field read live at
//!   the moment of serializing, so a window that was destroyed reads `could not get window title`
//!   and a rect of zeros.
//! - Monitor notifications are unit variants tagged the same way, `{"type": "SessionLocked"}`,
//!   and virtual desktop notifications are bare strings.
//! - A command is `{"type": "<Name>", "content": <argument>}` like a window event, told apart by its
//!   name.
//! - The log has no `processing event:` line. A handler's lines carry the span
//!   `process_event{event="FocusChange" winevent="SystemForeground" hwnd=131072}` (or
//!   `process_command{message=...}`), and an INFO `processed: <window>` (`processed` for a command)
//!   after the subscribers were notified marks its end.
//! - komorebi hides a window by cloaking it where it is, by default, so nothing in the `State`
//!   shows a window as hidden: [`Rules::hides_windows_by_position`] is `false` and
//!   `Rules::is_hidden_position` never matches.

use super::BlockHeader;
use super::Flavor;
use super::Rules;
use crate::analysis::blocks::Block;
use crate::analysis::blocks::BlockKind;
use crate::analysis::classify::Class;
use crate::analysis::events::EventInfo;
use crate::analysis::events::EventRecord;
use crate::analysis::events::Family;
use crate::analysis::logparse::Line;
use crate::analysis::state::Monitor;
use crate::analysis::state::Rect;
use serde_json::Value;

/// The [`Rules`] of komorebi for Windows: what its events, `State`, log and modules say.
///
/// A unit struct with no state. [`Flavor::rules`] returns a reference to one for
/// [`Flavor::Windows`]. Its [`verified`](Rules::verified) is `false`.
pub struct WindowsRules;

/// The events the OS reports about a window, and komorebi's own `Manage`, `Unmanage` and `Raise`.
const WINDOW_MANAGER_EVENTS: [&str; 14] = [
    "Destroy",
    "FocusChange",
    "Hide",
    "Cloak",
    "Minimize",
    "Show",
    "Uncloak",
    "MoveResizeStart",
    "MoveResizeEnd",
    "MouseCapture",
    "Manage",
    "Unmanage",
    "Raise",
    "TitleUpdate",
];

/// komorebi's notifications about the machine and its displays.
const MONITOR_NOTIFICATIONS: [&str; 7] = [
    "ResolutionScalingChanged",
    "WorkAreaChanged",
    "DisplayConnectionChange",
    "EnteringSuspendedState",
    "ResumingFromSuspendedState",
    "SessionLocked",
    "SessionUnlocked",
];

/// Sent as bare strings.
const VIRTUAL_DESKTOP_NOTIFICATIONS: [&str; 2] = [
    "EnteredAssociatedVirtualDesktop",
    "LeftAssociatedVirtualDesktop",
];

const GEOMETRY_EVENTS: [&str; 2] = ["MoveResizeStart", "MoveResizeEnd"];

/// What komorebi puts in place of a field it could not read.
const COULD_NOT_GET: &str = "could not get window";

/// A field of a window, unless komorebi could not read it.
fn field(window: &Value, key: &str) -> Option<String> {
    window
        .get(key)?
        .as_str()
        .filter(|text| !text.starts_with(COULD_NOT_GET))
        .map(str::to_string)
}

fn handle(window: &Value) -> Option<u32> {
    window
        .get("hwnd")?
        .as_i64()
        .and_then(|hwnd| u32::try_from(hwnd).ok())
}

/// A span field's value: tracing prints a string field in quotes and a number bare.
fn unquoted(value: &str) -> &str {
    value.trim().trim_matches('"')
}

impl Rules for WindowsRules {
    fn flavor(&self) -> Flavor {
        Flavor::Windows
    }

    fn verified(&self) -> bool {
        false
    }

    fn describe_event(&self, event: &Value) -> EventInfo {
        let unknown = |name: &str| EventInfo {
            family: Family::Unknown,
            name: name.to_string(),
            notification: None,
            pid: None,
            window_id: None,
            exe: None,
            argument: None,
        };

        // virtual desktop notifications are bare strings
        if let Some(name) = event.as_str() {
            return EventInfo {
                family: if VIRTUAL_DESKTOP_NOTIFICATIONS.contains(&name) {
                    Family::Monitor
                } else {
                    Family::Unknown
                },
                ..unknown(name)
            };
        }

        let Some(name) = event.get("type").and_then(Value::as_str) else {
            return unknown("unreadable");
        };
        let content = event.get("content");

        if MONITOR_NOTIFICATIONS.contains(&name) {
            return EventInfo {
                family: Family::Monitor,
                ..unknown(name)
            };
        }

        if WINDOW_MANAGER_EVENTS.contains(&name) {
            // `[<WinEvent>, <window>]`, or just `<window>` for the events with no WinEvent
            let (notification, window) = match content {
                Some(Value::Array(parts)) => (
                    parts.first().and_then(Value::as_str).map(str::to_string),
                    parts.get(1),
                ),
                other => (None, other),
            };

            return EventInfo {
                family: Family::WindowManager,
                name: name.to_string(),
                notification,
                pid: None,
                window_id: window.and_then(handle),
                exe: window.and_then(|window| field(window, "exe")),
                argument: None,
            };
        }

        EventInfo {
            family: Family::Command,
            name: name.to_string(),
            argument: content
                .filter(|value| !value.is_null())
                .map(crate::analysis::events::compact),
            ..unknown(name)
        }
    }

    fn is_geometry_event(&self, name: &str) -> bool {
        GEOMETRY_EVENTS.contains(&name)
    }

    fn classify_window_manager(&self, event: &EventRecord) -> (Class, &'static str) {
        match event.info.name.as_str() {
            "FocusChange" => (Class::Focus, "focus moved"),
            "Show" | "Destroy" | "Minimize" | "Manage" | "Unmanage" => (
                Class::Lifecycle,
                "a window appeared, went away, was minimized or restored, or komorebi began or stopped managing it",
            ),
            // switching workspace hides the windows of the one left by cloaking them, and the OS
            // reports that back
            "Hide" | "Cloak" | "Uncloak" => (
                Class::Lifecycle,
                "a window was hidden or shown: komorebi cloaks the windows of a workspace it leaves, so this is often its own doing reported back",
            ),
            name if GEOMETRY_EVENTS.contains(&name) => (
                Class::Drag,
                "a window was moved or resized with the mouse (or by something other than komorebi)",
            ),
            "MouseCapture" => (
                Class::Other,
                "a window captured or released the mouse: part of a drag, or a menu",
            ),
            "Raise" => (Class::Manual, "komorebi raised a window itself"),
            "TitleUpdate" => (Class::Other, "a window changed its title"),
            _ => (Class::Other, "not a known kind of event"),
        }
    }

    fn classify_monitor(&self, event: &EventRecord) -> (Class, &'static str) {
        match event.info.name.as_str() {
            "SessionLocked" | "SessionUnlocked" => {
                (Class::Lock, "the session was locked or unlocked")
            }
            "EnteringSuspendedState" | "ResumingFromSuspendedState" => {
                (Class::Lock, "the machine went to sleep or woke")
            }
            "EnteredAssociatedVirtualDesktop" | "LeftAssociatedVirtualDesktop" => (
                Class::Lock,
                "the virtual desktop komorebi is managing was left or returned to",
            ),
            _ => (Class::Monitor, "a display changed"),
        }
    }

    fn window_id(&self, window: &Value) -> Option<u64> {
        window
            .get("hwnd")?
            .as_i64()
            .and_then(|hwnd| u64::try_from(hwnd).ok())
    }

    fn window_is_unreadable(&self, window: &Value) -> bool {
        window
            .get("title")
            .and_then(Value::as_str)
            .is_some_and(|title| title.starts_with(COULD_NOT_GET))
    }

    fn hides_windows_by_position(&self) -> bool {
        false
    }

    fn is_hidden_position(&self, _rect: &Rect, _monitor: &Monitor) -> bool {
        // a cloaked window is exactly where it was; nothing in the state says it is hidden
        false
    }

    fn block_header(&self, line: &Line, open: Option<&Block>) -> Option<BlockHeader> {
        let kind = match line.root_span()? {
            "process_event" => BlockKind::Event,
            "process_command" => BlockKind::Command,
            _ => return None,
        };

        let root = line.spans.first()?;
        let get = |key: &str| {
            root.fields
                .iter()
                .find(|(name, _)| name == key)
                .map(|(_, value)| unquoted(value).to_string())
        };

        let header = match kind {
            BlockKind::Event => BlockHeader {
                kind,
                name: get("event").unwrap_or_default(),
                pid: None,
                window_id: get("hwnd").and_then(|hwnd| hwnd.parse().ok()),
                notification: get("winevent").filter(|winevent| winevent != "None"),
            },
            BlockKind::Command => BlockHeader {
                kind,
                // `message=FocusWindow(Left)`: the command's name is what is before any argument
                name: get("message")
                    .map(|message| {
                        message
                            .split(['(', ' ', '{'])
                            .next()
                            .unwrap_or_default()
                            .to_string()
                    })
                    .unwrap_or_default(),
                pid: None,
                window_id: None,
                notification: None,
            },
        };

        // the handler that is open carries on while its span says the same thing and it has not
        // logged that it finished; a different span, or the end, starts a new one
        let continues = open.is_some_and(|block| {
            block.kind == header.kind
                && block.name == header.name
                && block.window_id == header.window_id
                && block.notification == header.notification
        });
        (!continues).then_some(header)
    }

    fn is_block_end(&self, line: &Line, kind: BlockKind) -> bool {
        match kind {
            // INFO `processed: (hwnd: ...)`, or TRACE for the noisiest events
            BlockKind::Event => line.message.starts_with("processed: "),
            BlockKind::Command => line.message == "processed",
        }
    }

    fn slow_handler_cause(&self) -> &'static str {
        "slow Win32 calls into other programs' windows (positioning one that is busy or hung, or an animation), or waiting for the lock another handler held"
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn describe(event: &Value) -> EventInfo {
        WindowsRules.describe_event(event)
    }

    fn window(hwnd: i64, exe: &str) -> Value {
        json!({
            "hwnd": hwnd, "title": "a title", "exe": exe, "class": "Chrome_WidgetWin_1",
            "rect": {"left": 0, "top": 0, "right": 100, "bottom": 100}
        })
    }

    #[test]
    fn a_window_event_names_its_win_event_handle_and_program() {
        let info = describe(&json!({
            "type": "FocusChange",
            "content": ["SystemForeground", window(131_072, "chrome.exe")]
        }));

        assert_eq!(info.family, Family::WindowManager);
        assert_eq!(info.name, "FocusChange");
        assert_eq!(info.notification.as_deref(), Some("SystemForeground"));
        assert_eq!(info.window_id, Some(131_072));
        assert_eq!(info.exe.as_deref(), Some("chrome.exe"));
        assert_eq!(info.pid, None);
        assert_eq!(
            info.summary(),
            "FocusChange chrome.exe window 131072 SystemForeground"
        );
    }

    #[test]
    fn manage_unmanage_and_raise_carry_a_window_and_no_win_event() {
        for name in ["Manage", "Unmanage", "Raise"] {
            let info = describe(&json!({"type": name, "content": window(7, "code.exe")}));
            assert_eq!(info.family, Family::WindowManager, "{name}");
            assert_eq!(info.notification, None);
            assert_eq!(info.window_id, Some(7));
            assert_eq!(info.exe.as_deref(), Some("code.exe"));
        }
    }

    #[test]
    fn a_window_komorebi_could_not_read_has_a_handle_and_no_program() {
        let info = describe(&json!({
            "type": "Destroy",
            "content": ["ObjectDestroy", {
                "hwnd": 9, "title": "could not get window title",
                "exe": "could not get window exe", "class": "could not get window class",
                "rect": {"left": 0, "top": 0, "right": 0, "bottom": 0}
            }]
        }));

        assert_eq!(info.window_id, Some(9));
        assert_eq!(info.exe, None, "an error message is not a program");
    }

    #[test]
    fn notifications_about_the_machine_are_monitor_events_and_not_commands() {
        for name in MONITOR_NOTIFICATIONS {
            let info = describe(&json!({"type": name}));
            assert_eq!(info.family, Family::Monitor, "{name}");
            assert_eq!(info.name, name);
        }

        for name in VIRTUAL_DESKTOP_NOTIFICATIONS {
            let info = describe(&json!(name));
            assert_eq!(info.family, Family::Monitor, "{name}");
        }

        // a bare string that is not one of those is not something to guess at
        assert_eq!(describe(&json!("Retile")).family, Family::Unknown);
    }

    #[test]
    fn commands_carry_their_argument() {
        let info = describe(&json!({"type": "FocusWorkspaceNumber", "content": 2}));
        assert_eq!(info.family, Family::Command);
        assert_eq!(info.argument.as_deref(), Some("2"));
        assert_eq!(describe(&json!({"type": "Retile"})).family, Family::Command);
    }

    #[test]
    fn nonsense_is_unknown_not_a_panic() {
        assert_eq!(describe(&json!(12)).family, Family::Unknown);
        assert_eq!(describe(&json!({"type": 3})).family, Family::Unknown);
        assert_eq!(describe(&json!({"a": 1})).family, Family::Unknown);
    }

    #[test]
    fn handles_are_window_ids_and_unreadable_windows_are_noticed() {
        assert_eq!(
            WindowsRules.window_id(&window(131_072, "a.exe")),
            Some(131_072)
        );
        assert_eq!(
            WindowsRules.window_id(&json!({"id": 5})),
            None,
            "a Mac window"
        );
        assert!(WindowsRules.window_is_unreadable(&json!({"title": "could not get window title"})));
        assert!(!WindowsRules.window_is_unreadable(&window(1, "a.exe")));
        assert!(!WindowsRules.hides_windows_by_position());
    }
}
