//! The model every file is read into: a keymap of modes of bindings, and the diagnostics about it.

use crate::Format;
use crate::qt::Combo;
use std::fmt;

/// The name of the mode a keymap starts in.
pub const DEFAULT_MODE: &str = "default";

/// The key bindings read from one file.
///
/// A keymap is a list of [modes](Mode), each with its bindings, and the settings that apply to
/// every mode: the applications in which no binding is active ([`Keymap::blacklist`]), the binding
/// that turns every other binding off and on ([`Keymap::pause`]), and the templates the file
/// defined ([`Keymap::defines`]).
///
/// All three formats read into this one structure, so a binding means the same thing whatever
/// file it came from: `alt - h : komorebic focus left` in an skhdrc and `alt + h : komorebic focus
/// left` in a whkdrc are equal [`Binding`]s, apart from where each was written
/// ([`Binding::span`] and [`Binding::source`]); the keymaps differ only in [`Keymap::format`].
///
/// # Example
///
/// ```rust
/// use komorebi_hotkeys::{Binding, Body, DEFAULT_MODE, Edge, Format, Keymap, Run, Span};
/// use komorebi_hotkeys::qt::{Combo, Mods, key_to_qt};
///
/// let mut keymap = Keymap::new(Format::Skhd);
/// keymap.modes[0].bindings.push(Binding {
///     chord: vec![Combo::new(Mods::ALT, key_to_qt(Format::Skhd, "h").unwrap())],
///     edge: Edge::Press,
///     body: Body::Run(Run::Command("komorebic focus left".into())),
///     span: Span::default(),
///     source: "alt - h : komorebic focus left".into(),
/// });
///
/// let default = keymap.mode(DEFAULT_MODE).unwrap();
/// assert_eq!(default.bindings[0].chord[0].to_string(), "Alt+H");
/// assert!(!keymap.declares_modes());
/// ```
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Keymap {
    /// The format of the file the keymap was read from.
    pub format: Format,
    /// The modes, the [default mode](DEFAULT_MODE) first.
    pub modes: Vec<Mode>,
    /// The applications in which no binding is active, as the file names them.
    pub blacklist: Vec<String>,
    /// The binding that turns every other binding off, and on again.
    pub pause: Option<Pause>,
    /// The names the file defined. Their uses are already expanded in the bindings; they are kept
    /// to be listed.
    pub defines: Vec<Define>,
}

impl Keymap {
    /// Returns a keymap with an empty default mode and nothing else.
    pub fn new(format: Format) -> Keymap {
        Keymap {
            format,
            modes: vec![Mode::new(DEFAULT_MODE)],
            blacklist: Vec::new(),
            pause: None,
            defines: Vec::new(),
        }
    }

    /// Returns the mode with this name.
    pub fn mode(&self, name: &str) -> Option<&Mode> {
        self.modes.iter().find(|mode| mode.name == name)
    }

    /// Returns true if and only if the keymap has a mode other than the default one.
    pub fn declares_modes(&self) -> bool {
        self.modes.iter().any(|mode| mode.name != DEFAULT_MODE)
    }
}

/// A set of bindings that are active together.
///
/// Only one mode is active at a time. A keymap starts in the [default mode](DEFAULT_MODE), and a
/// binding whose body is [`Run::Mode`] switches to another.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Mode {
    /// The mode's name.
    pub name: String,
    /// Whether the mode swallows every key that none of its bindings is for (skhd's `@`).
    pub capture: bool,
    /// A command to run each time the mode is entered.
    pub on_enter: Option<String>,
    /// The bindings, in the order the file has them.
    pub bindings: Vec<Binding>,
}

impl Mode {
    /// Returns an empty mode with this name, which does not capture and runs nothing when it is
    /// entered.
    pub fn new(name: impl Into<String>) -> Mode {
        Mode {
            name: name.into(),
            capture: false,
            on_enter: None,
            bindings: Vec::new(),
        }
    }
}

/// One binding: the keys, when it fires, and what it does.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Binding {
    /// The combos to press, one after the other. A plain binding has one; a chord chain (sxhkd's
    /// `alt + w ; h`) has more.
    pub chord: Vec<Combo>,
    /// Whether the binding fires when the last combo is pressed or when it is released.
    pub edge: Edge,
    /// What the binding does.
    pub body: Body,
    /// Where the binding was written.
    pub span: Span,
    /// The binding as it was written, on one line, for logs and for the comment of a generated
    /// shortcut file.
    pub source: String,
}

/// When a binding fires.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Edge {
    /// When the last combo of the chord is pressed.
    Press,
    /// When the last combo of the chord is released (sxhkd's `@`).
    Release,
}

/// What a binding does.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Body {
    /// The same thing in every application.
    Run(Run),
    /// A different thing depending on the application that has the focus.
    ///
    /// The scope that names the focused application applies, the first one if several do; when
    /// none does, the [`App::Other`] scope applies, wherever it is in the list. When there is no
    /// such scope, or its [`Then`] is [`Then::Ignore`], the binding does nothing and the key
    /// reaches the application.
    Scoped(Vec<Scope>),
}

/// An action.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Run {
    /// Runs a command.
    Command(String),
    /// Switches to the mode with this name, and runs its [`Mode::on_enter`] command.
    Mode(String),
}

/// What a scoped binding does in some applications.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Scope {
    /// The applications the scope is for.
    pub app: App,
    /// What the binding does in them.
    pub then: Then,
}

/// The applications a [`Scope`] applies to.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum App {
    /// The application with this name, as the file writes it.
    Named(String),
    /// Every application no other scope of the binding names (skhd's `*`, whkd's `Default`).
    Other,
}

/// What a scoped binding does in the applications of its scope.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Then {
    /// It runs this action.
    Run(Run),
    /// Nothing: the key reaches the application (skhd's `~`, whkd's `Ignore`).
    Ignore,
}

/// The binding that turns every other binding off, and on again (whkd's `.pause`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Pause {
    /// The keys.
    pub combo: Combo,
    /// A command to run each time the bindings are turned off or on (whkd's `.pause_hook`).
    pub hook: Option<String>,
    /// Where the binding was written.
    pub span: Span,
}

/// A name the file defined (skhd's `.define`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Define {
    /// The name.
    pub name: String,
    /// What the name stands for.
    pub value: DefineValue,
    /// Where it was defined.
    pub span: Span,
}

/// What a [`Define`] stands for.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum DefineValue {
    /// A command, with placeholders `{{1}}`, `{{2}}`... for its arguments.
    Command(String),
    /// A group of applications.
    Apps(Vec<String>),
}

/// A range of the text a keymap was read from.
///
/// The default value, all zeros, is a span that points nowhere, for a binding that was not read
/// from a file.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct Span {
    /// The byte offset of the first byte.
    pub start: usize,
    /// The byte offset one past the last byte.
    pub end: usize,
    /// The line of the first byte, counted from 1.
    pub line: usize,
}

/// Something about a file that its user should know.
///
/// A diagnostic is either about text that could not be read ([`Severity::Error`]), or about a
/// binding komorebi realizes differently from the file's own daemon, or not at all
/// ([`Severity::Warning`]), or a remark ([`Severity::Note`]). [`Diagnostic::support`] says how
/// faithfully the construct it is about is realized.
///
/// # Example
///
/// ```rust
/// use komorebi_hotkeys::{Diagnostic, Severity, Span, Support};
///
/// let diagnostic = Diagnostic {
///     span: Span { start: 40, end: 47, line: 3 },
///     severity: Severity::Warning,
///     support: Support::Approximated,
///     msg: "`lalt` is Alt: KDE does not tell left and right apart".into(),
/// };
/// assert_eq!(
///     diagnostic.to_string(),
///     "line 3: warning: `lalt` is Alt: KDE does not tell left and right apart",
/// );
/// ```
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Diagnostic {
    /// The text the diagnostic is about.
    pub span: Span,
    /// How much it matters.
    pub severity: Severity,
    /// How faithfully the construct is realized.
    pub support: Support,
    /// The message, which starts in lower case and does not end with a period.
    pub msg: String,
}

/// Writes `line N: severity: message`.
impl fmt::Display for Diagnostic {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "line {}: {}: {}",
            self.span.line, self.severity, self.msg
        )
    }
}

/// How much a [`Diagnostic`] matters.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Severity {
    /// A remark: nothing is lost.
    Note,
    /// The text is valid in its format, but komorebi does not realize it the way the format's own
    /// daemon does.
    Warning,
    /// The text is not valid in its format, and is skipped.
    Error,
}

/// Writes `note`, `warning` or `error`.
impl fmt::Display for Severity {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Severity::Note => "note",
            Severity::Warning => "warning",
            Severity::Error => "error",
        })
    }
}

/// How faithfully komorebi realizes the construct a [`Diagnostic`] is about.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Support {
    /// As the file's own daemon would.
    Exact,
    /// In a way that differs from the file's own daemon, which the message describes.
    Approximated,
    /// Not at all: the construct is left out.
    Unsupported,
}
