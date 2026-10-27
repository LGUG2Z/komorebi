//! What kind of thing each event was, and which ones are noise.
//!
//! A recording of komorebi at work is mostly events nobody asked for: when komorebi moves a window
//! the OS reports the move back, so a single command is followed by a burst of `MoveEnd`
//! notifications for the windows it just placed. Reading those as if the user had dragged windows
//! around sends an investigation the wrong way, so events are classified, with the reason.
//!
//! The classification is a judgement made from the event, its neighbours and whether the state
//! changed, and the `reason` explains it. It never hides an event; it labels it.
//!
//! Two parties make it. Which class a single event has depends on komorebi's vocabulary, so the
//! [`Rules`] of the flavor that recorded the session answer that:
//! [`Rules::classify_window_manager`] for what the OS reported and [`Rules::classify_monitor`] for
//! a display or machine notification. What only the neighbours can tell, an echo, and whether a
//! button release was a real drag, is the same for both flavors and is decided here, by
//! [`classify`].

use super::events::EventRecord;
use super::events::Family;
use super::flavor::Rules;
use schemars::JsonSchema;
use serde::Serialize;

/// The kind of thing an event was.
///
/// Serialized in kebab case (`command`, `drag`, ..., `other`). A class tells a reader what to make
/// of an event in a recording; the [`Classification`] that carries it gives the reason.
///
/// Most are answers of the flavor's [`Rules`]. [`classify`] itself gives [`Class::Command`] to every
/// command that is not a query, and [`Class::Other`] to a query or an unreadable notification. It
/// derives [`Class::Echo`] from an event and its neighbours, and a [`Rules`] implementor never
/// answers that one.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, JsonSchema)]
#[serde(rename_all = "kebab-case")]
pub enum Class {
    /// Sent to komorebi by `komorebic` or a hotkey daemon.
    Command,
    /// The OS reported a window being moved or resized, not caused by komorebi.
    Drag,
    /// Which application or window has focus changed.
    Focus,
    /// A window or application appeared, went away, was minimized or restored. On Windows, also a
    /// window hidden or shown, which is often komorebi cloaking the windows of a workspace it left.
    Lifecycle,
    /// The OS reporting back a move or resize komorebi itself had just made.
    Echo,
    /// komorebi's own synthetic events: a show it made up, a button release its mouse listener
    /// reported as the end of a move or resize that moved nothing, or (on Windows) a window it
    /// raised.
    Manual,
    /// A display was connected, disconnected or resized.
    Monitor,
    /// Screen lock, unlock or a space change. On Windows, also a sleep or wake and a virtual
    /// desktop left or returned to.
    Lock,
    /// None of the above: a query or a subscription (usually this recorder talking to komorebi), an
    /// event no rule recognizes, or a notification that could not be read.
    Other,
}

/// A [`Class`] and the reason for it.
///
/// The reason is a sentence for a reader, written by whoever gave the class: a [`Rules`] method,
/// or [`classify`] for an echo, a promoted drag, a command and a query. It names the evidence
/// where there is some (an echo names the event it followed and how long after).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, JsonSchema)]
pub struct Classification {
    /// The kind of event it was.
    pub class: Class,
    /// Why it was given that class.
    pub reason: String,
}

/// How long after komorebi acts the OS may still be reporting the windows it moved.
const ECHO_WINDOW_NS: u64 = 1_500_000_000;

/// Commands that ask komorebi something or register a listener rather than change anything.
fn is_bookkeeping(name: &str) -> bool {
    matches!(
        name,
        "State"
            | "GlobalState"
            | "DebugWindow"
            | "AddSubscriberSocket"
            | "AddSubscriberSocketWithOptions"
            | "RemoveSubscriberSocket"
    ) || name.starts_with("Query")
}

fn class_of(event: &EventRecord, rules: &dyn Rules) -> Classification {
    let (class, reason) = match event.info.family {
        Family::Command if is_bookkeeping(&event.info.name) => (
            Class::Other,
            "a query or subscription, not a change: usually this recorder talking to komorebi",
        ),
        Family::Command => (Class::Command, "a command sent to komorebi"),
        Family::Monitor => rules.classify_monitor(event),
        Family::Unknown => (Class::Other, "the notification could not be read"),
        Family::WindowManager => rules.classify_window_manager(event),
    };

    Classification {
        class,
        reason: reason.to_string(),
    }
}

/// Returns the [`Classification`] of every event: one per event, in the same order.
///
/// `events` must be in receipt order, which is how `events::load` returns
/// them. Each event is first classified alone: a command is [`Class::Command`], except a query or
/// subscription (`State`, `GlobalState`, `DebugWindow`, `AddSubscriberSocket`,
/// `AddSubscriberSocketWithOptions`, `RemoveSubscriberSocket`, or a name that starts with
/// `Query`), which is [`Class::Other`]; an event that could not be read is [`Class::Other`]; a
/// window-manager event or a monitor notification is whatever
/// [`Rules::classify_window_manager`] or [`Rules::classify_monitor`] returns. Then the neighbours
/// are looked at, and two labels change:
///
/// - A [`Class::Manual`] event that [`Rules::is_geometry_event`] names, after which the state
///   changed (it has a different state id from the event before, and both have one), is a real
///   drag: [`Class::Drag`].
/// - A [`Class::Drag`] after which the state did not change (the same state id as the event
///   before) is an echo, [`Class::Echo`], if komorebi acted shortly before: looking back from it
///   for the nearest earlier event that is a command, focus, lifecycle, monitor or lock event,
///   that event was received at most 1.5 s earlier. An event with no state id is never called an
///   echo.
///
/// When the pattern does not hold (the move changed the state, nothing happened in the second and
/// a half before it, or the state is not known) the event stays a drag. It is a judgement, not a
/// proof, and the reason gives what it was based on.
///
/// # Example
///
/// A move that changes nothing right after a command is the OS reporting the command back; the same
/// move later, with the layout changed, is a real one:
///
/// ```rust
/// use komorebi_recorder::analysis::classify::{classify, Class};
/// use komorebi_recorder::analysis::events::EventRecord;
/// use komorebi_recorder::analysis::flavor::Flavor;
/// use serde_json::json;
///
/// let rules = Flavor::Macos.rules();
/// let event = |seq: u64, at_ms: u64, state_id: u64, value: serde_json::Value| EventRecord {
///     seq,
///     recv_mach_ns: at_ms * 1_000_000,
///     recv_wall_us: 0,
///     state_id: Some(state_id),
///     info: rules.describe_event(&value),
///     parse_error: None,
/// };
///
/// let moved = json!({
///     "type": "MoveEnd",
///     "content": [{"type": "Accessibility", "content": "AXWindowMoved"}, 10, 7]
/// });
/// let events = vec![
///     event(1, 0, 1, json!({"type": "FocusWorkspaceNumber", "content": 1})),
///     event(2, 80, 1, moved.clone()),     // state unchanged, 80 ms after the command
///     event(3, 10_000, 2, moved),         // much later, and the state changed
/// ];
///
/// let found = classify(&events, rules);
/// let classes: Vec<Class> = found.iter().map(|found| found.class).collect();
/// assert_eq!(classes, [Class::Command, Class::Echo, Class::Drag]);
/// assert!(found[1].reason.contains("FocusWorkspaceNumber"));
/// ```
#[must_use]
pub fn classify(events: &[EventRecord], rules: &dyn Rules) -> Vec<Classification> {
    let mut classes: Vec<Classification> =
        events.iter().map(|event| class_of(event, rules)).collect();

    for index in 0..events.len() {
        let event = &events[index];

        // a release that did change the layout was a real drag
        if classes[index].class == Class::Manual
            && rules.is_geometry_event(&event.info.name)
            && index > 0
            && event.state_id.is_some()
            && events[index - 1].state_id.is_some()
            && event.state_id != events[index - 1].state_id
        {
            classes[index] = Classification {
                class: Class::Drag,
                reason: "the end of a mouse drag or resize, and the layout changed because of it"
                    .to_string(),
            };
            continue;
        }

        if classes[index].class != Class::Drag {
            continue;
        }

        // a move or resize that changed nothing, soon after komorebi acted, is komorebi's own
        let unchanged =
            index > 0 && event.state_id.is_some() && event.state_id == events[index - 1].state_id;
        if !unchanged {
            continue;
        }

        let acted = (0..index).rev().find_map(|earlier| {
            let gap = event
                .recv_mach_ns
                .saturating_sub(events[earlier].recv_mach_ns);
            if gap > ECHO_WINDOW_NS {
                return Some(None);
            }

            matches!(
                classes[earlier].class,
                Class::Command | Class::Focus | Class::Lifecycle | Class::Monitor | Class::Lock
            )
            .then(|| Some((earlier, gap)))
        });

        if let Some(Some((earlier, gap))) = acted {
            classes[index] = Classification {
                class: Class::Echo,
                reason: format!(
                    "state unchanged, {:.0} ms after komorebi handled {} (#{}): the OS reporting back windows komorebi moved",
                    gap as f64 / 1e6,
                    events[earlier].info.summary(),
                    events[earlier].seq
                ),
            };
        }
    }

    classes
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::analysis::flavor::Flavor;
    use serde_json::json;

    fn event(seq: u64, at_ms: u64, state_id: Option<u64>, value: serde_json::Value) -> EventRecord {
        EventRecord {
            seq,
            recv_mach_ns: at_ms * 1_000_000,
            recv_wall_us: 0,
            state_id,
            info: Flavor::Macos.rules().describe_event(&value),
            parse_error: None,
        }
    }

    fn command(seq: u64, at_ms: u64, state: u64) -> EventRecord {
        event(
            seq,
            at_ms,
            Some(state),
            json!({"type": "FocusWorkspaceNumber", "content": 1}),
        )
    }

    fn move_end(seq: u64, at_ms: u64, state: u64) -> EventRecord {
        event(
            seq,
            at_ms,
            Some(state),
            json!({"type": "MoveEnd", "content": [{"type": "Accessibility", "content": "AXWindowMoved"}, 10, 7]}),
        )
    }

    fn classes(events: &[EventRecord]) -> Vec<Class> {
        classify(events, Flavor::Macos.rules())
            .iter()
            .map(|c| c.class)
            .collect()
    }

    #[test]
    fn each_family_and_name_has_a_class() {
        let events = vec![
            command(1, 0, 1),
            event(2, 10, Some(2), json!({"DisplayConnectionChange": 1})),
            event(
                3,
                20,
                Some(3),
                json!({"type": "FocusChange", "content": [{"type": "Accessibility", "content": "AXApplicationActivated"}, 1, null]}),
            ),
            event(
                4,
                30,
                Some(4),
                json!({"type": "Show", "content": [{"type": "Accessibility", "content": "AXWindowCreated"}, 1]}),
            ),
            event(
                5,
                40,
                Some(5),
                json!({"type": "Show", "content": [{"type": "Manual", "content": "ShowOnInputEvent"}, 1]}),
            ),
            event(
                6,
                50,
                Some(6),
                json!({"type": "ScreenLock", "content": [{"type": "Accessibility", "content": "x"}, 1]}),
            ),
        ];

        assert_eq!(
            classes(&events),
            [
                Class::Command,
                Class::Monitor,
                Class::Focus,
                Class::Lifecycle,
                Class::Manual,
                Class::Lock
            ]
        );
    }

    fn synthesized_move_end(seq: u64, at_ms: u64, state: u64) -> EventRecord {
        event(
            seq,
            at_ms,
            Some(state),
            json!({"type": "MoveEnd", "content": [{"type": "Manual", "content": "MoveEnd"}, 10, 7]}),
        )
    }

    #[test]
    fn a_button_release_komorebi_made_up_is_not_an_echo_of_a_move() {
        // seen on a real recording: a click, then komorebi's mouse listener reports MoveEnd and
        // ResizeEnd with the notification "MoveEnd" rather than one of macOS's
        let events = vec![command(1, 0, 1), synthesized_move_end(2, 100, 1)];
        let found = classify(&events, Flavor::Macos.rules());
        assert_eq!(found[1].class, Class::Manual);
        assert!(
            found[1].reason.contains("mouse listener"),
            "{}",
            found[1].reason
        );

        // but one after which the layout really changed is a drag that ended
        let dragged = vec![command(1, 0, 1), synthesized_move_end(2, 100, 2)];
        assert_eq!(
            classify(&dragged, Flavor::Macos.rules())[1].class,
            Class::Drag
        );
    }

    #[test]
    fn queries_and_subscriptions_are_not_commands() {
        let events = vec![
            event(
                1,
                0,
                Some(1),
                json!({"type": "AddSubscriberSocketWithOptions", "content": ["x", {}]}),
            ),
            event(2, 10, Some(1), json!({"type": "State"})),
            event(3, 20, Some(1), json!({"type": "QueryFocusedWorkspaceName"})),
        ];
        assert_eq!(classes(&events), [Class::Other, Class::Other, Class::Other]);
    }

    #[test]
    fn a_move_that_changed_the_state_is_a_real_move() {
        let events = vec![command(1, 0, 1), move_end(2, 200, 2)];
        assert_eq!(classes(&events), [Class::Command, Class::Drag]);
    }

    #[test]
    fn moves_that_change_nothing_right_after_a_command_are_echoes_of_it() {
        // the burst seen after FocusWorkspaceNumber in a real log: one MoveEnd per window
        let events = vec![
            command(1, 0, 1),
            move_end(2, 80, 1),
            move_end(3, 120, 1),
            move_end(4, 160, 1),
        ];

        let found = classify(&events, Flavor::Macos.rules());
        assert_eq!(found[1].class, Class::Echo);
        assert_eq!(found[3].class, Class::Echo);
        assert!(
            found[1].reason.contains("FocusWorkspaceNumber"),
            "{}",
            found[1].reason
        );
        assert!(found[1].reason.contains("#1"));
    }

    #[test]
    fn an_unchanged_move_long_after_anything_is_not_called_an_echo() {
        let events = vec![command(1, 0, 1), move_end(2, 10_000, 1)];
        assert_eq!(classes(&events), [Class::Command, Class::Drag]);
    }

    #[test]
    fn an_unchanged_move_with_no_state_is_not_called_an_echo() {
        let events = vec![command(1, 0, 1), move_end(2, 100, 1)];
        let mut without = events.clone();
        without[1].state_id = None;
        assert_eq!(classes(&without)[1], Class::Drag);
        assert_eq!(classes(&events)[1], Class::Echo);
    }
}
