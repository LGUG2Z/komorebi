//! komorebi for Mac, as it describes itself.
//!
//! [`MacRules`] is the [`Rules`] of [`Flavor::Macos`]. It keeps the defaults of
//! [`Rules::verified`] (`true`), [`Rules::classify_monitor`] and [`Rules::window_is_unreadable`].
//! Its tests read events shaped as komorebi for Mac sends them, and a `State` and a log that were
//! recorded from it. What the rules rely on:
//!
//! - A window event serializes as `{"type": "MoveEnd", "content": [<notification>, <pid>,
//!   <window id>]}`, where the notification is `{"type": "Accessibility", "content":
//!   "AXWindowMoved"}` for what the accessibility API reported, `{"type": "AppKitWorkspace",
//!   "content": "NS..."}` for what the workspace notification center did, or `{"type": "Manual",
//!   ...}` for komorebi making one up (its mouse listener reports a button release as a move or
//!   resize end this way). The process id is the only identification of the application.
//! - A monitor notification is a one-entry object, `{"DisplayConnectionChange": <display id>}`.
//!   A command with no argument may be a bare string or `{"type": "Retile"}`.
//! - A window in the `State` is `{"id": ..., "details": {"title": ..., "exe": ...}, "rect":
//!   ...}`, and its `rect` is read live from the accessibility API.
//! - The log has a header line for each handler, `processing event: <Name> for process <pid> with
//!   notification <notification>` or `processing command: <Name>`, and a command ends with
//!   `processed command: <Name>`. No line ends an event's handler: komorebi sends the notification
//!   after the handler body, so an event's receive time marks the end of its handler.
//! - komorebi hides a window by moving it to a corner of its monitor, leaving a one-point sliver
//!   inside the display.
//!
//! [`verified`]: Rules::verified

use super::BlockHeader;
use super::Flavor;
use super::Rules;
use crate::analysis::blocks::Block;
use crate::analysis::blocks::BlockKind;
use crate::analysis::catalog::Role;
use crate::analysis::classify::Class;
use crate::analysis::events::EventInfo;
use crate::analysis::events::EventRecord;
use crate::analysis::events::Family;
use crate::analysis::logparse::Line;
use crate::analysis::state::Monitor;
use crate::analysis::state::Rect;
use serde_json::Value;

/// The [`Rules`] of komorebi for Mac: what its events, `State`, log and modules say.
///
/// A unit struct with no state. [`Flavor::rules`] returns a reference to one for
/// [`Flavor::Macos`]. It can also be used directly, as the example on [`Rules`] does.
pub struct MacRules;

/// The names komorebi gives the events the OS reports.
const WINDOW_MANAGER_EVENTS: [&str; 14] = [
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
    "SpaceChange",
    "ScreenLock",
    "ScreenUnlock",
];

const GEOMETRY_EVENTS: [&str; 4] = ["MoveStart", "MoveEnd", "ResizeStart", "ResizeEnd"];

const EVENT_HEADER: &str = "processing event: ";
const COMMAND_HEADER: &str = "processing command: ";
const COMMAND_FINISHED: &str = "processed command: ";

/// `FocusChange for process 79469 with notification AXApplicationActivated`
fn parse_event_header(rest: &str) -> (String, Option<i32>, Option<String>) {
    let (name, tail) = rest.split_once(" for process ").unwrap_or((rest, ""));
    let (pid, notification) = tail.split_once(" with notification ").unwrap_or((tail, ""));
    (
        name.trim().to_string(),
        pid.trim().parse().ok(),
        Some(notification.trim().to_string()).filter(|text| !text.is_empty()),
    )
}

/// Whether a window-manager event was made up by komorebi itself rather than reported by macOS.
fn is_synthesized(event: &EventRecord) -> bool {
    event
        .info
        .notification
        .as_deref()
        .is_some_and(|notification| {
            !notification.starts_with("AX") && !notification.starts_with("NS")
        })
}

impl Rules for MacRules {
    fn flavor(&self) -> Flavor {
        Flavor::Macos
    }

    fn window_id(&self, window: &Value) -> Option<u64> {
        window.get("id")?.as_u64()
    }

    fn hides_windows_by_position(&self) -> bool {
        true
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

        // a monitor notification is a one-entry object: {"Resize": 69733248}
        if event.get("type").is_none()
            && let Some(object) = event.as_object()
            && object.len() == 1
            && let Some((name, value)) = object.iter().next()
        {
            return EventInfo {
                family: Family::Monitor,
                name: name.clone(),
                notification: None,
                pid: None,
                window_id: None,
                exe: None,
                argument: Some(crate::analysis::events::compact(value)),
            };
        }

        // a unit-variant command is a bare string
        if let Some(name) = event.as_str() {
            return EventInfo {
                family: Family::Command,
                name: name.to_string(),
                ..unknown(name)
            };
        }

        let Some(name) = event.get("type").and_then(Value::as_str) else {
            return unknown("unreadable");
        };
        let content = event.get("content");

        if WINDOW_MANAGER_EVENTS.contains(&name) {
            let parts = content.and_then(Value::as_array);
            let at = |index: usize| parts.and_then(|parts| parts.get(index));
            return EventInfo {
                family: Family::WindowManager,
                name: name.to_string(),
                notification: at(0)
                    .and_then(|notification| notification.get("content"))
                    .and_then(Value::as_str)
                    .map(str::to_string),
                pid: at(1)
                    .and_then(Value::as_i64)
                    .and_then(|pid| i32::try_from(pid).ok()),
                window_id: at(2)
                    .and_then(Value::as_u64)
                    .and_then(|id| u32::try_from(id).ok()),
                exe: None,
                argument: None,
            };
        }

        EventInfo {
            family: Family::Command,
            name: name.to_string(),
            notification: None,
            pid: None,
            window_id: None,
            exe: None,
            argument: content
                .filter(|value| !value.is_null())
                .map(crate::analysis::events::compact),
        }
    }

    fn is_geometry_event(&self, name: &str) -> bool {
        GEOMETRY_EVENTS.contains(&name)
    }

    fn classify_window_manager(&self, event: &EventRecord) -> (Class, &'static str) {
        if event
            .info
            .notification
            .as_deref()
            .is_some_and(|notification| notification.starts_with("ShowOn"))
        {
            return (Class::Manual, "komorebi's own synthetic show event");
        }

        match event.info.name.as_str() {
            "FocusChange" => (Class::Focus, "focus moved"),
            "Show" | "Destroy" | "Minimize" | "Restore" | "Manage" | "Unmanage" => {
                (Class::Lifecycle, "a window or application changed")
            }
            // komorebi's own mouse listener reports a button release as a move or resize end
            // whose notification is not one of macOS's (AX..., NS...)
            name if GEOMETRY_EVENTS.contains(&name) && is_synthesized(event) => (
                Class::Manual,
                "komorebi's mouse listener saw a button release after a click or drag; nothing was reported moved",
            ),
            name if GEOMETRY_EVENTS.contains(&name) => (
                Class::Drag,
                "a window was moved or resized by something other than komorebi",
            ),
            "ScreenLock" | "ScreenUnlock" | "SpaceChange" => {
                (Class::Lock, "the screen locked or the space changed")
            }
            _ => (Class::Other, "not a known kind of event"),
        }
    }

    /// Whether `rect` is where komorebi parks a window it has hidden on `monitor`: with a
    /// one-point sliver still inside the display, so the window stays "on screen" to macOS. The
    /// vertical position is not checked, because macOS pulls a window up to keep its title bar
    /// reachable.
    fn is_hidden_position(&self, rect: &Rect, monitor: &Monitor) -> bool {
        let tolerance = 2;
        let bottom_right = monitor.hiding_position == "BottomRight";
        if bottom_right {
            (rect.left - (monitor.size.left + monitor.size.width - 1)).abs() <= tolerance
        } else {
            (rect.right_edge() - (monitor.size.left + 1)).abs() <= tolerance
        }
    }

    fn block_header(&self, line: &Line, _open: Option<&Block>) -> Option<BlockHeader> {
        match line.root_span() {
            Some("process_event") if line.message.starts_with(EVENT_HEADER) => {
                let (name, pid, notification) =
                    parse_event_header(&line.message[EVENT_HEADER.len()..]);
                Some(BlockHeader {
                    kind: BlockKind::Event,
                    name,
                    pid,
                    window_id: None,
                    notification,
                })
            }
            Some("process_command") if line.message.starts_with(COMMAND_HEADER) => {
                Some(BlockHeader {
                    kind: BlockKind::Command,
                    name: line.message[COMMAND_HEADER.len()..].trim().to_string(),
                    pid: None,
                    window_id: None,
                    notification: None,
                })
            }
            _ => None,
        }
    }

    fn is_block_end(&self, line: &Line, kind: BlockKind) -> bool {
        kind == BlockKind::Command && line.message.starts_with(COMMAND_FINISHED)
    }

    fn role(&self, target: &str) -> Role {
        crate::analysis::catalog::role_in(ROLES, target)
    }

    fn slow_handler_cause(&self) -> &'static str {
        "slow accessibility calls into other applications, or waiting for the lock another handler held"
    }
}

/// The modules of komorebi for Mac and what each does, for [`Rules::role`].
///
/// Each entry is `(module, summary, changes_state_silently)`: the module path that appears as a log
/// line's target, a sentence about what the part does, and whether it changes komorebi's state
/// without sending a subscriber notification (the reaper and the workspace reconciliator do). A
/// target is matched to the entry with the longest module that is a prefix of it, so
/// `komorebi::accessibility::error` finds `komorebi::accessibility`.
pub const ROLES: &[(&str, &str, bool)] = &[
    (
        "komorebi::process_event",
        "handles events from the OS one at a time under the window-manager lock; the subscriber notification is sent after the handler body",
        false,
    ),
    (
        "komorebi::process_command",
        "handles commands from komorebic or a hotkey daemon one at a time under the lock; notifies subscribers before it logs 'processed command'",
        false,
    ),
    (
        "komorebi::window_manager",
        "window-manager operations, called from the handlers",
        false,
    ),
    (
        "komorebi::workspace",
        "operations on a workspace's containers: layout, focus, adding and removing",
        false,
    ),
    (
        "komorebi::container",
        "operations on a container and its tabbed windows",
        false,
    ),
    (
        "komorebi::monitor",
        "operations on a monitor and its workspaces",
        false,
    ),
    (
        "komorebi::window",
        "operations on one window, through the accessibility API",
        false,
    ),
    (
        "komorebi::macos_api",
        "calls into macOS; an AXError comes from the target application or the OS, not from komorebi",
        false,
    ),
    (
        "komorebi::accessibility",
        "calls into the macOS accessibility API; an AXError comes from the target application or the OS",
        false,
    ),
    (
        "komorebi::application",
        "per-application accessibility observers",
        false,
    ),
    (
        "komorebi::border_manager",
        "draws window borders on its own thread; cosmetic, never changes the layout state",
        false,
    ),
    (
        "komorebi::reaper",
        "background thread that removes windows that no longer exist; it takes the lock and changes state without sending a notification",
        true,
    ),
    (
        "komorebi::workspace_reconciliator",
        "background thread that moves windows between workspaces after a space change; it takes the lock and changes state without sending a notification",
        true,
    ),
    (
        "komorebi::monitor_reconciliator",
        "background thread that reacts to displays connecting, disconnecting and resizing; it sends a monitor notification when done",
        false,
    ),
    (
        "komorebi::display_reconfiguration_listener",
        "macOS callback for display changes, feeding the monitor reconciliator",
        false,
    ),
    (
        "komorebi::static_config",
        "loads and applies komorebi.json",
        false,
    ),
    (
        "komorebi::theme_manager",
        "applies border and bar themes",
        false,
    ),
    (
        "komorebi::input_event_listener",
        "watches the mouse and keyboard for the hacks that need them",
        false,
    ),
    (
        "komorebi::window_manager_event_listener",
        "receives OS notifications and queues them for the event handler",
        false,
    ),
    (
        "komorebi::notification_center_listener",
        "receives macOS workspace notifications (app activation, space change, screen lock)",
        false,
    ),
];

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn describe(event: &Value) -> EventInfo {
        MacRules.describe_event(event)
    }

    #[test]
    fn a_window_manager_event_has_its_process_window_and_notification() {
        let info = describe(&json!({
            "type": "MoveEnd",
            "content": [{"type": "Accessibility", "content": "AXWindowMoved"}, 20934, 4711]
        }));

        assert_eq!(info.family, Family::WindowManager);
        assert_eq!(info.name, "MoveEnd");
        assert_eq!(info.notification.as_deref(), Some("AXWindowMoved"));
        assert_eq!(info.pid, Some(20934));
        assert_eq!(info.window_id, Some(4711));
        assert_eq!(
            info.summary(),
            "MoveEnd pid 20934 window 4711 AXWindowMoved"
        );
    }

    #[test]
    fn a_focus_change_may_have_no_window() {
        let info = describe(&json!({
            "type": "FocusChange",
            "content": [{"type": "AppKitWorkspace", "content": "NSWorkspaceDidActivateApplicationNotification"}, 79469, null]
        }));

        assert_eq!(info.pid, Some(79469));
        assert_eq!(info.window_id, None);
        assert_eq!(
            info.notification.as_deref(),
            Some("NSWorkspaceDidActivateApplicationNotification")
        );
    }

    #[test]
    fn commands_carry_their_argument() {
        let info = describe(&json!({"type": "FocusWorkspaceNumber", "content": 2}));
        assert_eq!(info.family, Family::Command);
        assert_eq!(info.name, "FocusWorkspaceNumber");
        assert_eq!(info.argument.as_deref(), Some("2"));

        let bare = describe(&json!({"type": "Retile"}));
        assert_eq!(bare.family, Family::Command);
        assert_eq!(bare.argument, None);

        assert_eq!(describe(&json!("Retile")).family, Family::Command);
    }

    #[test]
    fn monitor_notifications_are_one_entry_objects() {
        let info = describe(&json!({"DisplayConnectionChange": 1}));
        assert_eq!(info.family, Family::Monitor);
        assert_eq!(info.name, "DisplayConnectionChange");
        assert_eq!(info.argument.as_deref(), Some("1"));
    }

    #[test]
    fn nonsense_is_unknown_not_a_panic() {
        assert_eq!(describe(&json!(12)).family, Family::Unknown);
        assert_eq!(describe(&json!({"a": 1, "b": 2})).family, Family::Unknown);
        assert_eq!(describe(&json!({"type": 3})).family, Family::Unknown);
    }

    #[test]
    fn roles_are_found_by_the_longest_module_prefix() {
        let role = |target: &str| MacRules.role(target);
        assert!(role("komorebi::reaper").changes_state_silently);
        assert!(role("komorebi::workspace_reconciliator").changes_state_silently);
        assert!(!role("komorebi::process_event").changes_state_silently);
        assert!(
            role("komorebi::accessibility::error")
                .summary
                .contains("accessibility")
        );
        assert_eq!(role("komorebi::brand_new").summary, "komorebi internals");
    }
}
