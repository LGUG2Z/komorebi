//! One model for the key bindings of skhd, whkd and sxhkd files, with the keys as KDE stores them.
//!
//! skhd (macOS), whkd (Windows) and sxhkd (X11) are hotkey daemons: each reads a file of key
//! bindings and runs a command when a combination of keys is pressed. komorebi users keep their
//! bindings to `komorebic` in such a file. On KDE Plasma under Wayland no client sees the keys of
//! another, and the program that runs a command on a key is KDE's global shortcut daemon. The
//! three files say much the same things in three syntaxes, so this crate has one model for all of
//! them, [`Keymap`], and the keys as KDE stores them, [`qt`].
//!
//! # The model
//!
//! - A [`Keymap`] is the content of one file: its [modes](Mode), and the settings that apply to
//!   every mode (a blacklist of applications, a pause binding, the names the file defined).
//! - A [`Mode`] is a set of [bindings](Binding) that are active together. A keymap starts in the
//!   [default mode](DEFAULT_MODE).
//! - A [`Binding`] is a chord (one or more [combos](qt::Combo) pressed one after the other), an
//!   [edge](Edge) (press or release) and a [body](Body): either an action ([`Run`]: a command or
//!   a mode switch), or a different action per application ([`Scope`]s).
//! - A [`Diagnostic`] is something about the file its user should know, with the [`Span`] of the
//!   text it is about, its [`Severity`] and how faithfully the construct is realized
//!   ([`Support`]).
//!
//! # Keys
//!
//! Each format names keys in its own vocabulary: skhd writes `0x21` or `return`, whkd `oem_4` or
//! `return`, sxhkd `bracketleft` or `Return`. [`qt::key_to_qt`] turns a name into a Qt key code,
//! and [`qt::Combo`] holds a key with its modifiers in the form KWin matches, so that the same key
//! is the same value whatever file named it:
//!
//! ```rust
//! use komorebi_hotkeys::Format;
//! use komorebi_hotkeys::qt::{Combo, Mods, key_to_qt};
//!
//! let skhd = Combo::new(Mods::ALT | Mods::SHIFT, key_to_qt(Format::Skhd, "0x21").unwrap());
//! let whkd = Combo::new(Mods::ALT | Mods::SHIFT, key_to_qt(Format::Whkd, "oem_4").unwrap());
//! assert_eq!(skhd, whkd);
//! // Shift+[ types {, and KWin matches the character
//! assert_eq!(skhd.to_string(), "Alt+{");
//! ```
//!
//! Key names are read as on a US keyboard layout; [the `qt` module](qt#a-us-layout-only) says
//! what that means for other layouts.
//!
//! # Reading a file
//!
//! [`parse`] reads the text of a file in one of the three [formats](Format) into a [`Parsed`]:
//! the [`Keymap`] and the [diagnostics](Diagnostic) about the file. Reading never fails. What is
//! not valid syntax is skipped with an error, and what is valid but cannot be realized on KDE,
//! or only differently, is reported with a warning that says so. Each format has a module that
//! documents what it reads and what it does not:
//!
//! - [`skhd`]: an skhdrc, with modes, per-application bindings, `.blacklist` and skhd.zig's
//!   `.define`.
//! - [`whkd`]: a whkdrc, with per-application bindings and `.pause`.
//! - [`sxhkd`]: an sxhkdrc, with brace expansion, chord chains and release bindings.
//!
//! The same binding in each format reads into the same keymap, apart from where it was written:
//!
//! ```rust
//! use komorebi_hotkeys::{Format, ParseOptions, parse};
//!
//! let options = ParseOptions::default();
//! let skhd = parse(Format::Skhd, "alt + shift - 0x21 : komorebic cycle-focus previous\n", &options);
//! let whkd = parse(Format::Whkd, "alt + shift + oem_4 : komorebic cycle-focus previous\n", &options);
//! let sxhkd = parse(
//!     Format::Sxhkd,
//!     "alt + shift + bracketleft\n    komorebic cycle-focus previous\n",
//!     &options,
//! );
//!
//! let binding = |parsed: &komorebi_hotkeys::Parsed| {
//!     let binding = &parsed.keymap.modes[0].bindings[0];
//!     (binding.chord.clone(), binding.body.clone())
//! };
//! assert_eq!(binding(&skhd), binding(&whkd));
//! assert_eq!(binding(&whkd), binding(&sxhkd));
//! assert_eq!(skhd.keymap.modes[0].bindings[0].chord[0].to_string(), "Alt+{");
//! ```
//!
//! A diagnostic points at the text it is about:
//!
//! ```rust
//! use komorebi_hotkeys::{Format, ParseOptions, Severity, parse};
//!
//! let text = "alt - h : komorebic focus left\nlalt - l : komorebic focus right\n";
//! let parsed = parse(Format::Skhd, text, &ParseOptions::default());
//! assert_eq!(parsed.keymap.modes[0].bindings[1].chord[0].to_string(), "Alt+L");
//! // KDE has no left Alt, so the binding is on either Alt, and the reader says so
//! let diagnostic = &parsed.diagnostics[0];
//! assert_eq!(diagnostic.severity, Severity::Warning);
//! assert_eq!(diagnostic.span.line, 2);
//! assert_eq!(&text[diagnostic.span.start..diagnostic.span.end], "lalt");
//! ```

#![deny(missing_docs)]

mod ir;
pub mod qt;
pub mod skhd;
pub mod sxhkd;
mod text;
pub mod whkd;

pub use ir::App;
pub use ir::Binding;
pub use ir::Body;
pub use ir::DEFAULT_MODE;
pub use ir::Define;
pub use ir::DefineValue;
pub use ir::Diagnostic;
pub use ir::Edge;
pub use ir::Keymap;
pub use ir::Mode;
pub use ir::Pause;
pub use ir::Run;
pub use ir::Scope;
pub use ir::Severity;
pub use ir::Span;
pub use ir::Support;
pub use ir::Then;

/// Reads the text of a file in a format.
///
/// This is [`skhd::parse`], [`whkd::parse`] or [`sxhkd::parse`], by `format`. Reading never fails:
/// what cannot be read is left out, with a [`Diagnostic`] that says why.
///
/// # Example
///
/// ```rust
/// use komorebi_hotkeys::{Format, ParseOptions, parse};
///
/// let parsed = parse(Format::Whkd, ".shell pwsh\nalt + h : komorebic focus left\n", &ParseOptions::default());
/// assert_eq!(parsed.keymap.format, Format::Whkd);
/// assert_eq!(parsed.keymap.modes[0].bindings[0].chord[0].to_string(), "Alt+H");
/// ```
pub fn parse(format: Format, text: &str, options: &ParseOptions) -> Parsed {
    match format {
        Format::Skhd => skhd::parse(text, options),
        Format::Whkd => whkd::parse(text, options),
        Format::Sxhkd => sxhkd::parse(text, options),
    }
}

/// Options for reading a file.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ParseOptions {
    /// The keyboard layout to read positional key names with: skhd's macOS keycodes and whkd's
    /// `oem_*` keys, and the [shifted symbol rule](qt#the-shifted-symbol-rule).
    ///
    /// The default for this is [`Layout::Us`](qt::Layout::Us), the only layout.
    pub layout: qt::Layout,
}

/// A file, read: its keymap, and what the reader has to say about it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Parsed {
    /// The bindings that could be read.
    pub keymap: Keymap,
    /// The diagnostics, in the order of the text they are about.
    pub diagnostics: Vec<Diagnostic>,
}

/// The file formats this crate reads.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Format {
    /// skhd's `skhdrc`, macOS's hotkey daemon, and the dialect of skhd.zig.
    Skhd,
    /// whkd's `whkdrc`, the hotkey daemon komorebi for Windows uses.
    Whkd,
    /// sxhkd's `sxhkdrc`, the X11 hotkey daemon.
    Sxhkd,
}

impl Format {
    /// Returns the name the format's daemon gives its file.
    ///
    /// # Example
    ///
    /// ```rust
    /// use komorebi_hotkeys::Format;
    ///
    /// assert_eq!(Format::Whkd.file_name(), "whkdrc");
    /// ```
    pub const fn file_name(self) -> &'static str {
        match self {
            Format::Skhd => "skhdrc",
            Format::Whkd => "whkdrc",
            Format::Sxhkd => "sxhkdrc",
        }
    }
}
