//! The key map: what each key of the viewer does.
//!
//! Kept apart from the window so the whole map can be read, listed in the help and tested. A key
//! becomes an [`Action`] through [`action`]; the window decides how to carry it out. Nothing here
//! needs a window or a [`Context`](eframe::egui::Context): the keys and modifiers are plain egui
//! values.
//!
//! The keys follow a video editor: `Space` plays and pauses, `J`, `K` and `L` shuttle (see
//! [`shuttle`]), the arrow keys step by a frame or, with `Shift`, by a second. [`BINDINGS`] is the
//! list the help window shows, kept next to the map so the two are edited together.
//!
//! The tab keys (`Cmd`/`Ctrl` with `T`, `W` and the digits, `Ctrl` `Tab`) are in the map, and are
//! carried out by the window rather than by a session. Otherwise three things are not in the map.
//! Keys with `Command`, `Ctrl` or `Alt` held belong to the
//! system and give no action; the window handles `Command`-`Z` (bring back a deleted annotation)
//! itself. While a text box wants the keyboard, the window reads no keys at all. And the symbol
//! keys are matched by what they type, not by which physical key they are, since that varies with
//! the keyboard layout.

use eframe::egui::Key;
use eframe::egui::Modifiers;

/// An action a key asks the window to carry out.
///
/// [`action`] makes one from a key press, and
/// [`Open::apply`](super::Open::apply) carries it out. The variants are the things the viewer's
/// keyboard can do, so adding one is how a new key gets an effect.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Action {
    /// Play if paused, pause if playing (`Space`).
    TogglePlay,
    /// Pause where the playhead is (`K`).
    Pause,
    /// Play forwards, faster each time it is pressed while already playing forwards (`L`).
    PlayForwards,
    /// Play backwards, faster each time (`J`).
    PlayBackwards,
    /// Step one frame, and pause: `-1` for back and `1` for forward (`←` and `→`).
    StepFrame(i32),
    /// Jump this many seconds, negative for back (`Shift` with `←` and `→`: `-1` and `1`).
    Skip(i32),
    /// Go to the start of the recording (`Home`).
    ToStart,
    /// Go to the end of the recording (`End`).
    ToEnd,
    /// Go to the event before the playhead and select it (`,`).
    PreviousEvent,
    /// Go to the event after the playhead and select it (`.`).
    NextEvent,
    /// Go to the start of the bug region before the playhead (`{`).
    PreviousBug,
    /// Go to the start of the bug region after the playhead (`}`).
    NextBug,
    /// Mark the start of a selection on the timeline at the playhead (`[`).
    SetIn,
    /// Mark the end of a selection on the timeline at the playhead (`]`).
    SetOut,
    /// Clear the selection and stop looping. [`action`] never returns it: no key is bound to it,
    /// and [`Escape`](Action::Escape) is what `Esc` gives.
    ClearSelection,
    /// Open the composer for a bug region, from the selection if there is one (`B`).
    MarkBug,
    /// Loop the selection, or stop looping (`\`).
    ToggleLoop,
    /// Show or hide komorebi's state over the video (`O`).
    ToggleOverlay,
    /// Zoom the timeline in around the playhead (`+` or `=`).
    ZoomIn,
    /// Zoom the timeline out around the playhead (`-` or `_`).
    ZoomOut,
    /// Show the whole recording on the timeline (`0`).
    FitTimeline,
    /// Make the display with this index among the displays that have video (counting from `0`, so
    /// `1` to `9` give `0` to `8`) the one that stepping acts on.
    FocusDisplay(usize),
    /// Show or hide the list of keys (`?`).
    Help,
    /// Close the list of keys if it is open, else close the form for a bug region if it is open,
    /// else clear the selection and stop looping (`Esc`).
    Escape,
    /// Open a new start-screen tab (`Cmd`/`Ctrl` `T`). Carried out by the window, not by an open
    /// session ([`ViewerApp::apply_tab_action`](super::ViewerApp::apply_tab_action)).
    NewTab,
    /// Close the current tab (`Cmd`/`Ctrl` `W`).
    CloseTab,
    /// Go to the next tab, from the last to the first (`Ctrl` `Tab`, and `Cmd` `Shift` `]`).
    NextTab,
    /// Go to the previous tab, from the first to the last (`Ctrl` `Shift` `Tab`, and `Cmd` `Shift`
    /// `[`).
    PreviousTab,
    /// Go to this tab, counting from `0` (`Cmd`/`Ctrl` with `1` to `8`). One beyond the tabs there
    /// are does nothing.
    GoToTab(usize),
    /// Go to the last tab (`Cmd`/`Ctrl` `9`).
    LastTab,
}

impl Action {
    /// Whether this action is about the window's tabs rather than the session in the current one.
    #[must_use]
    pub fn is_tab_action(self) -> bool {
        matches!(
            self,
            Self::NewTab
                | Self::CloseTab
                | Self::NextTab
                | Self::PreviousTab
                | Self::GoToTab(_)
                | Self::LastTab
        )
    }
}

/// Every binding, for the help screen: the keys and what they do, as text to show a person.
///
/// Each entry is `(keys, what they do)`, for example `("Space", "play / pause")`. The text is
/// written by hand, not generated from [`action`], and nothing checks the two against each other
/// beyond the unit test `every_binding_in_the_help_is_real`, which looks for a few of the keys.
///
/// # Example
///
/// ```rust
/// use komorebi_recorder::viewer::keymap::BINDINGS;
///
/// let (keys, does) = BINDINGS[0];
/// assert_eq!((keys, does), ("Space", "play / pause"));
/// assert!(BINDINGS.iter().any(|(keys, _)| keys.starts_with("J")));
/// ```
pub const BINDINGS: &[(&str, &str)] = &[
    ("Space", "play / pause"),
    (
        "J  K  L",
        "play backwards / pause / play forwards (again = faster)",
    ),
    ("←  →", "back / forward one frame"),
    ("Shift ←  →", "back / forward one second"),
    ("Home  End", "to the start / end"),
    (",  .", "previous / next event"),
    ("{  }", "previous / next bug region"),
    ("[  ]", "set the start / end of a selection"),
    ("B", "mark a bug region (from the selection)"),
    ("\\", "loop the selection"),
    ("O", "show komorebi's state over the video"),
    ("+  -", "zoom the timeline"),
    ("0", "fit the whole recording"),
    ("1 – 9", "step by that display"),
    ("Esc", "clear the selection / close a panel"),
    ("?", "this help"),
    ("Cmd/Ctrl T", "a new tab"),
    ("Cmd/Ctrl W", "close the tab"),
    (
        "Ctrl Tab  Ctrl Shift Tab",
        "next / previous tab (on a Mac also Cmd Shift ]  [)",
    ),
    ("Cmd/Ctrl 1 – 9", "go to that tab (9 is the last)"),
];

/// Returns the action for a key pressed with `modifiers`, if it is bound.
///
/// `text` is what the key typed, if anything: the symbol keys (`,` `.` `{` `}` `[` `]` `\` `+` `=`
/// `-` `_` `?`) are read from it, since where they are on the keyboard varies with the layout. When
/// `text` is one of them it decides the action whatever `key` is. Otherwise the action comes from
/// `key`: the letters `J`, `K`, `L`, `B` and `O`, `Space`, the arrow keys (with `Shift` for a
/// second at a time), `Home`, `End`, `Esc` and the digits (`0` fits the timeline, `1` to `9` pick a
/// display).
///
/// The tab keys hold `Command` (`Cmd` on a Mac, `Ctrl` elsewhere): `T` opens a tab, `W` closes
/// one, `1` to `9` go to a tab. `Ctrl` `Tab` (with `Shift`, backwards) and, on a Mac, `Command`
/// `Shift` `]` and `[` move between tabs. Anything else pressed with `Command`, `Ctrl` or `Alt`
/// gives `None`, as does any key that is not bound. [`Action::ClearSelection`] is never returned.
///
/// # Example
///
/// ```rust
/// use eframe::egui::{Key, Modifiers};
/// use komorebi_recorder::viewer::keymap::{action, Action};
///
/// assert_eq!(action(Key::Space, Modifiers::NONE, None), Some(Action::TogglePlay));
/// assert_eq!(action(Key::ArrowRight, Modifiers::NONE, None), Some(Action::StepFrame(1)));
/// assert_eq!(action(Key::ArrowRight, Modifiers::SHIFT, None), Some(Action::Skip(1)));
/// assert_eq!(action(Key::Num2, Modifiers::NONE, None), Some(Action::FocusDisplay(1)));
///
/// // symbols are read from what they type, not from where they are on the keyboard
/// assert_eq!(
///     action(Key::Comma, Modifiers::NONE, Some(",")),
///     Some(Action::PreviousEvent),
/// );
/// assert_eq!(action(Key::Equals, Modifiers::NONE, Some("=")), Some(Action::ZoomIn));
///
/// // the command key is left to the system, and an unbound key does nothing
/// assert_eq!(action(Key::B, Modifiers::COMMAND, None), None);
/// assert_eq!(action(Key::T, Modifiers::COMMAND, None), Some(Action::NewTab));
/// assert_eq!(action(Key::Num9, Modifiers::COMMAND, None), Some(Action::LastTab));
/// assert_eq!(action(Key::Q, Modifiers::NONE, None), None);
/// ```
#[must_use]
pub fn action(key: Key, modifiers: Modifiers, text: Option<&str>) -> Option<Action> {
    if let Some(tab) = tab_action(key, modifiers) {
        return Some(tab);
    }
    if modifiers.command || modifiers.ctrl || modifiers.alt {
        return None;
    }

    // symbols by what they type, since where they are on the keyboard differs
    match text {
        Some(",") => return Some(Action::PreviousEvent),
        Some(".") => return Some(Action::NextEvent),
        Some("{") => return Some(Action::PreviousBug),
        Some("}") => return Some(Action::NextBug),
        Some("[") => return Some(Action::SetIn),
        Some("]") => return Some(Action::SetOut),
        Some("\\") => return Some(Action::ToggleLoop),
        Some("+" | "=") => return Some(Action::ZoomIn),
        Some("-" | "_") => return Some(Action::ZoomOut),
        Some("?") => return Some(Action::Help),
        _ => {}
    }

    Some(match key {
        Key::Space => Action::TogglePlay,
        Key::J => Action::PlayBackwards,
        Key::K => Action::Pause,
        Key::L => Action::PlayForwards,
        Key::ArrowLeft if modifiers.shift => Action::Skip(-1),
        Key::ArrowRight if modifiers.shift => Action::Skip(1),
        Key::ArrowLeft => Action::StepFrame(-1),
        Key::ArrowRight => Action::StepFrame(1),
        Key::Home => Action::ToStart,
        Key::End => Action::ToEnd,
        Key::B => Action::MarkBug,
        Key::O => Action::ToggleOverlay,
        Key::Num0 => Action::FitTimeline,
        Key::Num1 => Action::FocusDisplay(0),
        Key::Num2 => Action::FocusDisplay(1),
        Key::Num3 => Action::FocusDisplay(2),
        Key::Num4 => Action::FocusDisplay(3),
        Key::Num5 => Action::FocusDisplay(4),
        Key::Num6 => Action::FocusDisplay(5),
        Key::Num7 => Action::FocusDisplay(6),
        Key::Num8 => Action::FocusDisplay(7),
        Key::Num9 => Action::FocusDisplay(8),
        Key::Escape => Action::Escape,
        _ => return None,
    })
}

fn tab_action(key: Key, modifiers: Modifiers) -> Option<Action> {
    if modifiers.alt {
        return None;
    }
    if modifiers.ctrl && key == Key::Tab {
        return Some(if modifiers.shift {
            Action::PreviousTab
        } else {
            Action::NextTab
        });
    }
    if !modifiers.command {
        return None;
    }
    if modifiers.shift {
        return match key {
            Key::CloseBracket | Key::CloseCurlyBracket => Some(Action::NextTab),
            Key::OpenBracket | Key::OpenCurlyBracket => Some(Action::PreviousTab),
            _ => None,
        };
    }
    Some(match key {
        Key::T => Action::NewTab,
        Key::W => Action::CloseTab,
        Key::Num1 => Action::GoToTab(0),
        Key::Num2 => Action::GoToTab(1),
        Key::Num3 => Action::GoToTab(2),
        Key::Num4 => Action::GoToTab(3),
        Key::Num5 => Action::GoToTab(4),
        Key::Num6 => Action::GoToTab(5),
        Key::Num7 => Action::GoToTab(6),
        Key::Num8 => Action::GoToTab(7),
        Key::Num9 => Action::LastTab,
        _ => return None,
    })
}

/// The speeds `J` and `L` step through, as multiples of real time. The last is the fastest
/// [`shuttle`] gives, which is the fastest the playhead goes
/// ([`MAX_RATE`](crate::playback::transport::MAX_RATE)).
pub const SHUTTLE_RATES: [f64; 4] = [1.0, 2.0, 4.0, 8.0];

/// Returns the rate to play at after a shuttle key (`J` or `L`).
///
/// The first press plays at normal speed, each further press in the same direction takes the next
/// of [`SHUTTLE_RATES`], and pressing the other direction starts over at normal speed.
///
/// `current` is the signed rate now (negative for backwards), `playing` is whether the playhead
/// is moving, and `forwards` is the key pressed (`L` is forwards). The result is signed:
/// negative for backwards. From a stop, or when turning round, it is `1.0` or `-1.0`; a current
/// speed already at the last of the rates stays there; and a speed between two of them (the
/// speed list in the window offers `0.25` and `0.5`) goes to the next one above it.
///
/// # Example
///
/// ```rust
/// use komorebi_recorder::viewer::keymap::shuttle;
///
/// // from a stop, the first press plays at normal speed
/// assert_eq!(shuttle(1.0, false, true), 1.0);
/// // each press in the same direction goes faster, up to the limit
/// assert_eq!(shuttle(1.0, true, true), 2.0);
/// assert_eq!(shuttle(4.0, true, true), 8.0);
/// assert_eq!(shuttle(8.0, true, true), 8.0);
/// // backwards is the same, with a sign
/// assert_eq!(shuttle(-1.0, true, false), -2.0);
/// // turning round starts over at normal speed
/// assert_eq!(shuttle(4.0, true, false), -1.0);
/// assert_eq!(shuttle(-4.0, true, true), 1.0);
/// // between two of the rates, it goes to the next one above
/// assert_eq!(shuttle(0.25, true, true), 1.0);
/// ```
#[must_use]
pub fn shuttle(current: f64, playing: bool, forwards: bool) -> f64 {
    let direction = if forwards { 1.0 } else { -1.0 };
    if !playing || current.signum() != direction {
        return direction;
    }

    let faster = SHUTTLE_RATES
        .iter()
        .copied()
        .find(|rate| *rate > current.abs() + 1e-9)
        .unwrap_or(SHUTTLE_RATES[SHUTTLE_RATES.len() - 1]);
    direction * faster
}

#[cfg(test)]
mod tests {
    use super::*;

    fn plain() -> Modifiers {
        Modifiers::NONE
    }

    #[test]
    fn the_transport_keys_do_what_video_players_do() {
        assert_eq!(action(Key::Space, plain(), None), Some(Action::TogglePlay));
        assert_eq!(action(Key::J, plain(), None), Some(Action::PlayBackwards));
        assert_eq!(action(Key::K, plain(), None), Some(Action::Pause));
        assert_eq!(action(Key::L, plain(), None), Some(Action::PlayForwards));
        assert_eq!(
            action(Key::ArrowLeft, plain(), None),
            Some(Action::StepFrame(-1))
        );
        assert_eq!(
            action(Key::ArrowRight, plain(), None),
            Some(Action::StepFrame(1))
        );
        assert_eq!(
            action(Key::ArrowRight, Modifiers::SHIFT, None),
            Some(Action::Skip(1))
        );
        assert_eq!(action(Key::Home, plain(), None), Some(Action::ToStart));
    }

    #[test]
    fn symbols_are_read_from_what_they_type_not_where_they_are() {
        assert_eq!(
            action(Key::Comma, plain(), Some(",")),
            Some(Action::PreviousEvent)
        );
        assert_eq!(
            action(Key::Period, plain(), Some(".")),
            Some(Action::NextEvent)
        );
        assert_eq!(
            action(Key::OpenBracket, plain(), Some("[")),
            Some(Action::SetIn)
        );
        assert_eq!(
            action(Key::CloseBracket, plain(), Some("]")),
            Some(Action::SetOut)
        );
        assert_eq!(
            action(Key::Questionmark, plain(), Some("?")),
            Some(Action::Help)
        );
        assert_eq!(
            action(Key::Equals, plain(), Some("=")),
            Some(Action::ZoomIn)
        );
        assert_eq!(
            action(Key::Minus, plain(), Some("-")),
            Some(Action::ZoomOut)
        );
    }

    #[test]
    fn the_command_key_is_left_to_the_system() {
        assert_eq!(action(Key::B, Modifiers::COMMAND, None), None);
        assert_eq!(action(Key::Space, Modifiers::CTRL, None), None);
        assert_eq!(action(Key::Q, plain(), None), None);
    }

    #[test]
    fn the_number_keys_pick_a_display() {
        assert_eq!(
            action(Key::Num1, plain(), None),
            Some(Action::FocusDisplay(0))
        );
        assert_eq!(
            action(Key::Num9, plain(), None),
            Some(Action::FocusDisplay(8))
        );
        assert_eq!(action(Key::Num0, plain(), None), Some(Action::FitTimeline));
    }

    #[test]
    fn shuttling_speeds_up_in_a_direction_and_starts_over_in_the_other() {
        assert_eq!(shuttle(1.0, false, true), 1.0);
        assert_eq!(shuttle(1.0, true, true), 2.0);
        assert_eq!(shuttle(2.0, true, true), 4.0);
        assert_eq!(shuttle(4.0, true, true), 8.0);
        assert_eq!(shuttle(8.0, true, true), 8.0, "no faster than the limit");

        assert_eq!(shuttle(-1.0, true, false), -2.0);
        assert_eq!(
            shuttle(4.0, true, false),
            -1.0,
            "turning round starts at normal speed"
        );
        assert_eq!(shuttle(-4.0, true, true), 1.0);
        assert_eq!(
            shuttle(-4.0, false, false),
            -1.0,
            "from a stop it starts at normal speed"
        );
    }

    #[test]
    fn the_tab_keys_hold_command_and_do_not_clash_with_the_plain_ones() {
        assert_eq!(
            action(Key::T, Modifiers::COMMAND, None),
            Some(Action::NewTab)
        );
        assert_eq!(
            action(Key::W, Modifiers::COMMAND, None),
            Some(Action::CloseTab)
        );
        assert_eq!(
            action(Key::Tab, Modifiers::CTRL, None),
            Some(Action::NextTab)
        );
        assert_eq!(
            action(
                Key::Tab,
                Modifiers {
                    ctrl: true,
                    shift: true,
                    ..Modifiers::NONE
                },
                None
            ),
            Some(Action::PreviousTab)
        );
        let command_shift = Modifiers {
            command: true,
            shift: true,
            ..Modifiers::NONE
        };
        assert_eq!(
            action(Key::CloseBracket, command_shift, None),
            Some(Action::NextTab)
        );
        assert_eq!(
            action(Key::OpenCurlyBracket, command_shift, None),
            Some(Action::PreviousTab)
        );
        assert_eq!(
            action(Key::Num1, Modifiers::COMMAND, None),
            Some(Action::GoToTab(0))
        );
        assert_eq!(
            action(Key::Num9, Modifiers::COMMAND, None),
            Some(Action::LastTab)
        );
        // the plain digit still picks a display, the plain letters still do what they did
        assert_eq!(
            action(Key::Num1, plain(), None),
            Some(Action::FocusDisplay(0))
        );
        assert_eq!(action(Key::T, plain(), None), None);
        assert_eq!(action(Key::W, plain(), None), None);
        assert_eq!(action(Key::Tab, plain(), None), None);
        // Alt makes them something else again
        assert_eq!(
            action(
                Key::T,
                Modifiers {
                    command: true,
                    alt: true,
                    ..Modifiers::NONE
                },
                None
            ),
            None
        );
    }

    #[test]
    fn every_binding_in_the_help_is_real() {
        // the help lists a key for each of these; if one stops working the list must change
        let listed: String = BINDINGS
            .iter()
            .map(|(keys, _)| *keys)
            .collect::<Vec<_>>()
            .join(" ");
        for key in ["Space", "J", "Home", "B", "Esc", "Cmd/Ctrl T", "Ctrl Tab"] {
            assert!(listed.contains(key), "{key}");
        }
    }
}
