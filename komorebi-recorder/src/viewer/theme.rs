//! Colors: what each kind of thing looks like wherever it is drawn.
//!
//! The constants are the palette of the window (dark, with one accent), and the functions map the
//! analysis types the viewer shows ([`Class`], [`Severity`], [`Level`]) to a color, so that an
//! event class has the same color on the timeline and in its chip in the inspector. Nothing here
//! needs a window: a [`Color32`] is a plain value.

use crate::analysis::classify::Class;
use crate::analysis::invariants::Severity;
use crate::analysis::logparse::Level;
use eframe::egui::Color32;

/// The fill of the window and of its panels.
pub const BACKGROUND: Color32 = Color32::from_rgb(22, 22, 27);
/// The fill of a lane of the timeline (every other lane), and of a card in the inspector.
pub const LANE: Color32 = Color32::from_rgb(30, 30, 37);
/// The fill of the lanes of the timeline that alternate with [`LANE`].
pub const LANE_ALT: Color32 = Color32::from_rgb(34, 34, 42);
/// Secondary text: captions, the names of lanes and the labels of the ruler.
pub const TEXT_DIM: Color32 = Color32::from_rgb(140, 140, 152);
/// The playhead on the timeline.
pub const PLAYHEAD: Color32 = Color32::from_rgb(255, 214, 90);
/// The color of what is wrong and of what the user marked as the bug: bug regions, errors, windows
/// that break an invariant in the state overlay.
pub const BUG: Color32 = Color32::from_rgb(230, 70, 60);
/// The range picked on the timeline.
pub const SELECTION: Color32 = Color32::from_rgb(90, 150, 240);
/// The stretches of a display's video in its lane of the timeline.
pub const VIDEO: Color32 = Color32::from_rgb(70, 110, 160);
/// The accent: the outline of the display the keys act on, the focused window in the state
/// overlay, and komorebi's own handler and process.
pub const ACCENT: Color32 = Color32::from_rgb(110, 170, 255);
/// Good news and confirmations: nothing looks wrong, the note after an action.
pub const OK: Color32 = Color32::from_rgb(110, 200, 140);

/// Returns the color of an event of `class`, on the timeline and in its chip in the inspector.
///
/// [`Class::Echo`], [`Class::Manual`] and [`Class::Other`] share one dim color, and so do
/// [`Class::Monitor`] and [`Class::Lock`].
///
/// # Example
///
/// ```rust
/// use komorebi_recorder::analysis::classify::Class;
/// use komorebi_recorder::viewer::theme;
///
/// assert_eq!(theme::class(Class::Echo), theme::class(Class::Other));
/// assert_eq!(theme::class(Class::Monitor), theme::class(Class::Lock));
/// assert_ne!(theme::class(Class::Command), theme::class(Class::Focus));
/// ```
#[must_use]
pub fn class(class: Class) -> Color32 {
    match class {
        Class::Command => Color32::from_rgb(110, 180, 255),
        Class::Focus => Color32::from_rgb(170, 170, 200),
        Class::Lifecycle => Color32::from_rgb(120, 205, 145),
        Class::Drag => Color32::from_rgb(245, 170, 90),
        Class::Echo | Class::Manual | Class::Other => Color32::from_rgb(88, 88, 100),
        Class::Monitor | Class::Lock => Color32::from_rgb(205, 130, 240),
    }
}

/// Returns how much an event of `class` matters when many share a column of pixels: the loudest
/// wins.
///
/// The weights, from loudest to quietest, are [`Class::Monitor`] and [`Class::Lock`] (`6`),
/// [`Class::Command`], [`Class::Lifecycle`], [`Class::Drag`], [`Class::Focus`] and, quietest,
/// [`Class::Echo`], [`Class::Manual`] and [`Class::Other`] (`1`). A weight is also what a dense
/// lane of the timeline uses to pick the color of a column: see
/// [`bin`](super::timeline::bin).
///
/// # Example
///
/// ```rust
/// use komorebi_recorder::analysis::classify::Class;
/// use komorebi_recorder::viewer::theme::class_weight;
///
/// assert_eq!(class_weight(Class::Monitor), 6);
/// assert!(class_weight(Class::Monitor) > class_weight(Class::Command));
/// assert!(class_weight(Class::Command) > class_weight(Class::Lifecycle));
/// assert!(class_weight(Class::Lifecycle) > class_weight(Class::Drag));
/// assert!(class_weight(Class::Drag) > class_weight(Class::Focus));
/// assert!(class_weight(Class::Focus) > class_weight(Class::Echo));
/// assert_eq!(class_weight(Class::Echo), class_weight(Class::Other));
/// ```
#[must_use]
pub fn class_weight(class: Class) -> u8 {
    match class {
        Class::Monitor | Class::Lock => 6,
        Class::Command => 5,
        Class::Lifecycle => 4,
        Class::Drag => 3,
        Class::Focus => 2,
        Class::Echo | Class::Manual | Class::Other => 1,
    }
}

/// Returns the color of a finding or a violation of `severity`: red for an error, amber for a
/// warning, gray for information.
///
/// # Example
///
/// ```rust
/// use komorebi_recorder::analysis::invariants::Severity;
/// use komorebi_recorder::viewer::theme;
///
/// assert_ne!(theme::severity(Severity::Error), theme::severity(Severity::Warn));
/// assert_ne!(theme::severity(Severity::Warn), theme::severity(Severity::Info));
/// ```
#[must_use]
pub fn severity(severity: Severity) -> Color32 {
    match severity {
        Severity::Error => Color32::from_rgb(235, 85, 75),
        Severity::Warn => Color32::from_rgb(235, 185, 60),
        Severity::Info => Color32::from_rgb(130, 130, 150),
    }
}

/// Returns the color of a line of komorebi's log at `level`.
///
/// An error and a warning are drawn in the colors [`severity`] gives [`Severity::Error`] and
/// [`Severity::Warn`]; every other level is [`TEXT_DIM`].
///
/// # Example
///
/// ```rust
/// use komorebi_recorder::analysis::invariants::Severity;
/// use komorebi_recorder::analysis::logparse::Level;
/// use komorebi_recorder::viewer::theme::{self, TEXT_DIM};
///
/// assert_eq!(theme::level(Level::Error), theme::severity(Severity::Error));
/// assert_eq!(theme::level(Level::Warn), theme::severity(Severity::Warn));
/// assert_eq!(theme::level(Level::Debug), TEXT_DIM);
/// ```
#[must_use]
pub fn level(level: Level) -> Color32 {
    match level {
        Level::Error => Color32::from_rgb(235, 85, 75),
        Level::Warn => Color32::from_rgb(235, 185, 60),
        _ => TEXT_DIM,
    }
}

/// The color of what happened to the machine: an incident on its lane of the timeline, and the
/// system's records in the list of what surrounded an event.
pub const INCIDENT: Color32 = Color32::from_rgb(170, 160, 255);
