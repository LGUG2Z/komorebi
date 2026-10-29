//! komorebi for KDE, as it describes itself.
//!
//! [`KdeRules`] is the [`Rules`] of [`Flavor::Kde`]. It keeps the defaults of
//! [`Rules::window_is_unreadable`] and answers `false` to [`Rules::verified`]: it was written from
//! komorebi for KDE's source, and has not been checked against a recording of it. What the rules
//! rely on:
//!
//! - A window event serializes as `{"type": "Show", "content": [{"type": "KWin", "content":
//!   "WindowAdded"}, <window>]}`, where the first part is the compositor signal the bridge script
//!   heard (`{"type": "Manual", ...}` is komorebi making one up) and the window is `{"id": ...,
//!   "rect": ..., "details": {"title": ..., "exe": ..., "class": ..., "path": ...}}`. `Manage` and
//!   `Unmanage` carry the window alone, and a `WorkspaceChange` carries a monitor id and a
//!   workspace index in the place of the window. There is no process id.
//! - A monitor notification is a one-entry object (`{"Resize": 0}`,
//!   `{"DisplayConnectionChange": 0}`), or the bare string `"Wake"`.
//! - A window in the `State` is known by its `id`, a number komorebi gave it (not KWin's UUID).
//! - The log has a header line for each handler, `processing event: <Name> for window <id> with
//!   notification <notification>` inside a `handle_event` span, and `processing command: <Name>`
//!   inside a `process_command` span, where a command ends with `processed command: <Name>`. An
//!   event's handler is timed by a debug line, `handled <Name> in <time>`.
//! - komorebi hides a window by moving it to the virtual desktop it calls `komorebi-park`, and
//!   does not move it, so a hidden window is not told from a visible one by where it is.

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

/// The [`Rules`] of komorebi for KDE: what its events, `State`, log and modules say.
///
/// A unit struct with no state. [`Flavor::rules`] returns a reference to one for
/// [`Flavor::Kde`].
pub struct KdeRules;

/// The names komorebi for KDE gives the events that are about a window, or a workspace.
const WINDOW_MANAGER_EVENTS: [&str; 18] = [
    "FocusChange",
    "Show",
    "Destroy",
    "Minimize",
    "Restore",
    "TitleUpdate",
    "FullscreenChange",
    "OutputChange",
    "MaximizeChange",
    "Manage",
    "Unmanage",
    "MoveStart",
    "MoveEnd",
    "ResizeStart",
    "ResizeEnd",
    "ResizeStep",
    "WorkspaceChange",
    "WorkspaceMove",
];

/// The names of the notifications about the monitors.
const MONITOR_NOTIFICATIONS: [&str; 3] = ["Resize", "DisplayConnectionChange", "Wake"];

const GEOMETRY_EVENTS: [&str; 4] = ["MoveStart", "MoveEnd", "ResizeStart", "ResizeEnd"];

const EVENT_HEADER: &str = "processing event: ";
const COMMAND_HEADER: &str = "processing command: ";
const COMMAND_FINISHED: &str = "processed command: ";
const EVENT_FINISHED: &str = "handled ";

/// `Show for window 12 with notification WindowAdded`
fn parse_event_header(rest: &str) -> (String, Option<u32>, Option<String>) {
    let (name, tail) = rest.split_once(" for window ").unwrap_or((rest, ""));
    let (id, notification) = tail.split_once(" with notification ").unwrap_or((tail, ""));
    (
        name.trim().to_string(),
        id.trim().parse().ok(),
        Some(notification.trim().to_string()).filter(|text| !text.is_empty()),
    )
}

impl Rules for KdeRules {
    fn flavor(&self) -> Flavor {
        Flavor::Kde
    }

    fn verified(&self) -> bool {
        false
    }

    fn classify_monitor(&self, event: &EventRecord) -> (Class, &'static str) {
        if event.info.name == "Wake" {
            (
                Class::Lock,
                "the machine woke up or the screen was unlocked",
            )
        } else {
            (Class::Monitor, "a display changed")
        }
    }

    fn window_id(&self, window: &Value) -> Option<u64> {
        window.get("id")?.as_u64()
    }

    fn hides_windows_by_position(&self) -> bool {
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

        // a monitor notification is a one-entry object: {"Resize": 0}
        if event.get("type").is_none()
            && let Some(object) = event.as_object()
            && object.len() == 1
            && let Some((name, value)) = object.iter().next()
        {
            return EventInfo {
                family: Family::Monitor,
                name: name.clone(),
                argument: Some(crate::analysis::events::compact(value)),
                ..unknown(name)
            };
        }

        // a unit-variant command is a bare string, and so is `Wake`, the one monitor notification
        // with nothing to say but itself
        if let Some(name) = event.as_str() {
            let family = if MONITOR_NOTIFICATIONS.contains(&name) {
                Family::Monitor
            } else {
                Family::Command
            };
            return EventInfo {
                family,
                ..unknown(name)
            };
        }

        let Some(name) = event.get("type").and_then(Value::as_str) else {
            return unknown("unreadable");
        };
        let content = event.get("content");

        if WINDOW_MANAGER_EVENTS.contains(&name) {
            // `Manage` and `Unmanage` carry the window alone; the others an array that starts
            // with the notification, and then has the window or the monitor
            let parts = content.and_then(Value::as_array);
            let at = |index: usize| parts.and_then(|parts| parts.get(index));
            let window = match parts {
                Some(_) => at(1).filter(|part| part.is_object()),
                None => content.filter(|part| part.is_object()),
            };
            return EventInfo {
                family: Family::WindowManager,
                name: name.to_string(),
                notification: at(0)
                    .and_then(|notification| notification.get("content"))
                    .and_then(Value::as_str)
                    .map(str::to_string),
                pid: None,
                window_id: window
                    .and_then(|window| window.get("id"))
                    .and_then(Value::as_u64)
                    .and_then(|id| u32::try_from(id).ok()),
                exe: window
                    .and_then(|window| window.get("details"))
                    .and_then(|details| details.get("exe"))
                    .and_then(Value::as_str)
                    .map(str::to_string),
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
            "Show" | "Destroy" | "Minimize" | "Restore" | "Manage" | "Unmanage" | "TitleUpdate"
            | "FullscreenChange" | "MaximizeChange" | "OutputChange" => {
                (Class::Lifecycle, "a window or its state changed")
            }
            name if GEOMETRY_EVENTS.contains(&name) => (
                Class::Drag,
                "the user moved or resized a window with the pointer",
            ),
            "ResizeStep" => (
                Class::Drag,
                "the user is part way through resizing a window",
            ),
            "WorkspaceChange" | "WorkspaceMove" => (Class::Lock, "a virtual desktop changed"),
            _ => (Class::Other, "not a known kind of event"),
        }
    }

    /// Never: komorebi for KDE hides a window by putting it on another virtual desktop, and leaves
    /// where it is alone.
    fn is_hidden_position(&self, _rect: &Rect, _monitor: &Monitor) -> bool {
        false
    }

    fn block_header(&self, line: &Line, _open: Option<&Block>) -> Option<BlockHeader> {
        match line.root_span() {
            Some("handle_event") if line.message.starts_with(EVENT_HEADER) => {
                let (name, window_id, notification) =
                    parse_event_header(&line.message[EVENT_HEADER.len()..]);
                Some(BlockHeader {
                    kind: BlockKind::Event,
                    name,
                    pid: None,
                    window_id,
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
        match kind {
            BlockKind::Command => line.message.starts_with(COMMAND_FINISHED),
            BlockKind::Event => {
                line.target.as_deref() == Some("komorebi::process_event")
                    && line.message.starts_with(EVENT_FINISHED)
            }
        }
    }

    fn role(&self, target: &str) -> Role {
        crate::analysis::catalog::role_in(ROLES, target)
    }

    fn slow_handler_cause(&self) -> &'static str {
        "slow calls to KWin over D-Bus, or waiting for the lock another handler held"
    }
}

/// The modules of komorebi for KDE and what each does, for [`Rules::role`].
///
/// Each entry is `(module, summary, changes_state_silently)`, as in the Mac and Windows tables. A
/// target is matched to the entry with the longest module that is a prefix of it.
pub const ROLES: &[(&str, &str, bool)] = &[
    (
        "komorebi::process_event",
        "handles events from the compositor one at a time under the window-manager lock; a notification is sent to subscribers after a handled event about a managed window",
        false,
    ),
    (
        "komorebi::process_command",
        "handles commands from komorebic or a hotkey one at a time under the lock",
        false,
    ),
    (
        "komorebi::window_manager_event_listener",
        "receives the bridge script's events and queues them for the event handler",
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
        "operations on a container and its stacked windows",
        false,
    ),
    (
        "komorebi::monitor_reconciliator",
        "reacts to outputs connecting, disconnecting and resizing, and to a wake",
        false,
    ),
    (
        "komorebi::monitor",
        "operations on a monitor and its workspaces",
        false,
    ),
    (
        "komorebi::window",
        "operations on one window, through the backend",
        false,
    ),
    (
        "komorebi::reconcile",
        "brings komorebi's state and KWin's windows back into agreement",
        false,
    ),
    (
        "komorebi::kwin",
        "talks to the KWin bridge script over D-Bus; a failure here is KWin's or the script's, not the layout's",
        false,
    ),
    (
        "komorebi::backend",
        "what komorebi needs from a compositor, behind one trait",
        false,
    ),
    (
        "komorebi::desktop",
        "virtual desktops, including the parking desktop that hidden windows are put on",
        false,
    ),
    (
        "komorebi::border_manager",
        "draws window borders through a KWin effect; cosmetic, never changes the layout state",
        false,
    ),
    (
        "komorebi::animation",
        "animates window moves through a KWin effect",
        false,
    ),
    (
        "komorebi::transparency_manager",
        "sets how see-through unfocused windows are",
        false,
    ),
    (
        "komorebi::static_config",
        "loads and applies komorebi.json",
        false,
    ),
    ("komorebi::theme_manager", "applies border themes", false),
];

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn describe(event: &Value) -> EventInfo {
        KdeRules.describe_event(event)
    }

    #[test]
    fn a_window_event_has_its_window_program_and_notification() {
        let info = describe(&json!({
            "type": "Show",
            "content": [
                {"type": "KWin", "content": "WindowAdded"},
                {"id": 12, "rect": {"left": 0, "top": 0, "right": 10, "bottom": 10},
                 "details": {"title": "t", "exe": "konsole", "class": "org.kde.konsole", "path": "/usr/bin/konsole"}}
            ]
        }));

        assert_eq!(info.family, Family::WindowManager);
        assert_eq!(info.name, "Show");
        assert_eq!(info.notification.as_deref(), Some("WindowAdded"));
        assert_eq!(info.pid, None);
        assert_eq!(info.window_id, Some(12));
        assert_eq!(info.exe.as_deref(), Some("konsole"));
        assert_eq!(info.summary(), "Show konsole window 12 WindowAdded");
    }

    #[test]
    fn manage_carries_the_window_alone_and_a_workspace_change_has_none() {
        let manage = describe(&json!({
            "type": "Manage",
            "content": {"id": 7, "details": {"exe": "dolphin"}}
        }));
        assert_eq!(manage.family, Family::WindowManager);
        assert_eq!(manage.window_id, Some(7));
        assert_eq!(manage.exe.as_deref(), Some("dolphin"));

        let change = describe(&json!({
            "type": "WorkspaceChange",
            "content": [{"type": "KWin", "content": "WorkspaceChanged"}, 1, 2]
        }));
        assert_eq!(change.family, Family::WindowManager);
        assert_eq!(change.notification.as_deref(), Some("WorkspaceChanged"));
        assert_eq!(change.window_id, None);
    }

    #[test]
    fn commands_carry_their_argument() {
        let info = describe(&json!({"type": "FocusWorkspaceNumber", "content": 2}));
        assert_eq!(info.family, Family::Command);
        assert_eq!(info.argument.as_deref(), Some("2"));
        assert_eq!(describe(&json!("Retile")).family, Family::Command);
    }

    #[test]
    fn monitor_notifications_are_objects_and_a_bare_wake() {
        let info = describe(&json!({"DisplayConnectionChange": 0}));
        assert_eq!(info.family, Family::Monitor);
        assert_eq!(info.name, "DisplayConnectionChange");

        let wake = describe(&json!("Wake"));
        assert_eq!(wake.family, Family::Monitor);
        assert_eq!(wake.name, "Wake");
    }

    #[test]
    fn nonsense_is_unknown_not_a_panic() {
        assert_eq!(describe(&json!(12)).family, Family::Unknown);
        assert_eq!(describe(&json!({"a": 1, "b": 2})).family, Family::Unknown);
        assert_eq!(describe(&json!({"type": 3})).family, Family::Unknown);
    }

    #[test]
    fn a_header_line_is_read_into_its_parts() {
        assert_eq!(
            parse_event_header("Show for window 12 with notification WindowAdded"),
            (
                "Show".to_string(),
                Some(12),
                Some("WindowAdded".to_string())
            )
        );
    }

    #[test]
    fn windows_are_not_hidden_by_position() {
        assert!(!KdeRules.hides_windows_by_position());
        assert!(!KdeRules.verified());
    }

    #[test]
    fn roles_are_found_by_the_longest_module_prefix() {
        let role = |target: &str| KdeRules.role(target);
        assert!(role("komorebi::kwin::bridge").summary.contains("KWin"));
        assert!(
            role("komorebi::monitor_reconciliator")
                .summary
                .contains("outputs")
        );
        assert_eq!(role("komorebi::brand_new").summary, "komorebi internals");
    }
}
