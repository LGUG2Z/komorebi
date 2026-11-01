//! Reading an sxhkdrc.
//!
//! An sxhkdrc is the file of sxhkd, the hotkey daemon of X11. A binding is two lines: the hotkey,
//! its modifiers and key joined by `+`, and below it the command, indented:
//!
//! ```text
//! super + {h,j,k,l}                   # one binding for each key in the braces
//!     komorebic focus {left,down,up,right}
//!
//! super + w ; {h,l}                   # a chord chain: super + w, then h or l
//!     komorebic move {left,right}
//!
//! super + r : {h,l}                   # a chain that stays open after h or l
//!     komorebic resize-edge {left,right} increase
//!
//! super + @space                      # runs when space is released
//!     komorebic toggle-float
//! ```
//!
//! [`parse`] reads it into a [`Keymap`]:
//!
//! ```rust
//! use komorebi_hotkeys::{ParseOptions, sxhkd};
//!
//! let parsed = sxhkd::parse(
//!     "super + {_,shift + }{1-2}\n    komorebic {focus,move-to}-workspace {0-1}\n",
//!     &ParseOptions::default(),
//! );
//! let bindings: Vec<String> = parsed.keymap.modes[0]
//!     .bindings
//!     .iter()
//!     .map(|binding| binding.source.clone())
//!     .collect();
//! assert_eq!(
//!     bindings,
//!     [
//!         "super + 1: komorebic focus-workspace 0",
//!         "super + 2: komorebic focus-workspace 1",
//!         "super + shift + 1: komorebic move-to-workspace 0",
//!         "super + shift + 2: komorebic move-to-workspace 1",
//!     ],
//! );
//! // Shift+1 types !, which is what KWin matches
//! assert_eq!(parsed.keymap.modes[0].bindings[2].chord[0].to_string(), "Meta+!");
//! ```
//!
//! # What is read, and how
//!
//! - **Bindings**: a line that does not start with a space is a hotkey, and the next line that does
//!   is its command. Lines that are empty or start with `#` are comments, and a line that ends in
//!   `\` continues on the next. The modifiers are `super` (Meta), `alt`, `ctrl` (or `control`),
//!   `shift`, `mod1` (Alt) and `mod4` (Meta); the key is an X keysym (`h`, `Return`,
//!   `bracketleft`, `XF86AudioPlay`...).
//! - **Braces**: `{a,b,c}` in the hotkey and in the command makes one binding per element, the
//!   first element of the hotkey with the first of the command, and so on. A hotkey with several
//!   pairs of braces makes every combination, the last pair changing fastest. `_` is the empty
//!   element, `a-z` or `1-9` a range, and `\{` a brace that is not one.
//! - **Chord chains**: `a ; b` is a [`Binding`] whose chord is `a` then `b`. In `a : b` the chain
//!   stays open after `b`, until Escape: it is read as a [`Mode`] named after `a` (`Meta+R`), with
//!   a binding for `a` that switches to it, the bindings after the `:` in it, and Escape to leave
//!   it.
//! - **Release bindings**: `@` before the key makes the binding fire when the key is released.
//!
//! # What is not, and the diagnostics about it
//!
//! - In sxhkd a key that is not the next chord of a chain ends the chain. Here only Escape does,
//!   or the chord timeout ([`Support::Approximated`]).
//! - A release binding fires on the first key-up of any key of its combo
//!   ([`Support::Approximated`]).
//! - `~` does not pass the key on to the application ([`Support::Approximated`]).
//! - A command that starts with `;` runs like every other, without waiting for it (a note).
//! - The modifiers `hyper`, `meta`, `mode_switch`, `lock`, `mod2`, `mod3`, `mod5` and `any`, mouse
//!   buttons and keypad keys have no KDE equivalent, and their bindings are left out
//!   ([`Support::Unsupported`]).
//!
//! [`Binding`]: crate::Binding
//! [`Keymap`]: crate::Keymap
//! [`Mode`]: crate::Mode
//! [`Support::Approximated`]: crate::Support::Approximated
//! [`Support::Unsupported`]: crate::Support::Unsupported

use crate::Binding;
use crate::Body;
use crate::DEFAULT_MODE;
use crate::Diagnostic;
use crate::Edge;
use crate::Format;
use crate::Keymap;
use crate::Mode;
use crate::ParseOptions;
use crate::Parsed;
use crate::Run;
use crate::Span;
use crate::Support;
use crate::qt::Combo;
use crate::qt::Key;
use crate::qt::Layout;
use crate::qt::Mods;
use crate::qt::key_to_qt;
use crate::text;
use crate::text::Lines;

/// Reads an sxhkdrc.
///
/// Reading never fails: what cannot be read is left out, with a [`Diagnostic`] that says why.
/// The [module documentation](self) lists what is read and what is not.
///
/// # Example
///
/// ```rust
/// use komorebi_hotkeys::{Body, ParseOptions, Run, sxhkd};
///
/// let parsed = sxhkd::parse("super + w ; h\n    komorebic move left\n", &ParseOptions::default());
/// let binding = &parsed.keymap.modes[0].bindings[0];
/// assert_eq!(binding.chord.len(), 2);
/// assert_eq!(binding.chord[0].to_string(), "Meta+W");
/// assert_eq!(binding.chord[1].to_string(), "H");
/// assert_eq!(binding.body, Body::Run(Run::Command("komorebic move left".into())));
/// ```
pub fn parse(text: &str, options: &ParseOptions) -> Parsed {
    let lines = Lines::new(text);
    let mut reader = Reader {
        layout: options.layout,
        keymap: Keymap::new(Format::Sxhkd),
        diagnostics: Vec::new(),
        chains: Vec::new(),
    };

    // the hotkey waiting for its command: its text and span
    let mut hotkey: Option<(String, Span)> = None;
    for (start, end, line) in logical_lines(text) {
        let span = lines.span(start, end);
        if line.trim().is_empty() || line.starts_with('#') {
            continue;
        }
        if line.starts_with(char::is_whitespace) {
            let command = line.trim();
            if command.starts_with('#') {
                continue;
            }
            match hotkey.take() {
                Some((keys, keys_span)) => {
                    let span = lines.span(keys_span.start, end);
                    reader.bindings(&keys, keys_span, command, span);
                }
                None => reader
                    .diagnostics
                    .push(text::error(span, "a command with no hotkey before it")),
            }
        } else if let Some((_, keys_span)) = hotkey.replace((line.trim().to_string(), span)) {
            reader
                .diagnostics
                .push(text::error(keys_span, "the hotkey has no command below it"));
        }
    }
    if let Some((_, keys_span)) = hotkey {
        reader
            .diagnostics
            .push(text::error(keys_span, "the hotkey has no command below it"));
    }

    let mut diagnostics = reader.diagnostics;
    diagnostics.sort_by_key(|d| d.span.start);
    Parsed {
        keymap: reader.keymap,
        diagnostics,
    }
}

/// The lines of the text, with the ones that end in `\` joined to the next: the byte range of
/// each and its text.
fn logical_lines(text: &str) -> Vec<(usize, usize, String)> {
    let mut out = Vec::new();
    let mut start = None;
    let mut offset = 0;
    for raw in text.split_inclusive('\n') {
        let line_start = offset;
        offset += raw.len();
        let line = raw.trim_end_matches(['\n', '\r']);
        let first = *start.get_or_insert(line_start);
        if line.ends_with('\\') && offset < text.len() {
            continue;
        }
        let end = line_start + line.len();
        out.push((first, end, text::join_continued(&text[first..end])));
        start = None;
    }
    out
}

/// One chord of a hotkey: the combo, and its `~` and `@`.
struct Chord {
    combo: Combo,
    replay: bool,
    release: bool,
}

struct Reader {
    layout: Layout,
    keymap: Keymap,
    diagnostics: Vec<Diagnostic>,
    /// The chain prefixes already reported, so that each is reported once.
    chains: Vec<Vec<Combo>>,
}

impl Reader {
    /// Reads a hotkey and its command, which may make several bindings.
    fn bindings(&mut self, keys: &str, keys_span: Span, command: &str, span: Span) {
        let (Some(hotkeys), Some(commands)) =
            (self.expand(keys, keys_span), self.expand(command, span))
        else {
            return;
        };
        let commands = match commands.len() {
            1 => vec![commands[0].clone(); hotkeys.len()],
            n if n == hotkeys.len() => commands,
            n => {
                self.diagnostics.push(text::error(
                    span,
                    format!(
                        "the hotkey has {} alternatives and the command {n}",
                        hotkeys.len()
                    ),
                ));
                return;
            }
        };

        for (hotkey, command) in hotkeys.iter().zip(&commands) {
            let hotkey = hotkey.trim();
            let mut command = text::strip_comment(command).trim();
            if let Some(synchronous) = command.strip_prefix(';') {
                command = synchronous.trim_start();
                self.diagnostics.push(text::note(
                    span,
                    Support::Approximated,
                    "a command that starts with `;` runs like any other, without waiting for it",
                ));
            }
            if command.is_empty() {
                self.diagnostics
                    .push(text::error(span, "the command is empty"));
                continue;
            }
            let Some((chords, lock)) = self.chain(hotkey, keys_span) else {
                continue;
            };
            let source = format!("{}: {command}", text::one_line(hotkey));
            self.add(chords, lock, command, span, source);
        }
    }

    /// Puts the bindings of one hotkey in their modes.
    fn add(
        &mut self,
        chords: Vec<Chord>,
        lock: Option<usize>,
        command: &str,
        span: Span,
        source: String,
    ) {
        let released = chords.last().is_some_and(|chord| chord.release);
        if released {
            self.diagnostics.push(text::approximated(
                span,
                "a release binding fires when any key of its combo is released first",
            ));
        }
        if chords.iter().any(|chord| chord.replay) {
            self.diagnostics.push(text::approximated(
                span,
                "`~` passes the key on in sxhkd; here the application does not get it",
            ));
        }
        if chords[..chords.len() - 1].iter().any(|chord| chord.release) {
            self.diagnostics.push(text::approximated(
                span,
                "`@` on a chord before the last one is read as a press",
            ));
        }
        let edge = if released { Edge::Release } else { Edge::Press };
        let combos: Vec<Combo> = chords.iter().map(|chord| chord.combo).collect();

        let Some(lock) = lock else {
            if combos.len() > 1 {
                self.report_chain(&combos[..combos.len() - 1], span, false);
            }
            self.keymap.modes[0].bindings.push(Binding {
                chord: combos,
                edge,
                body: Body::Run(Run::Command(command.to_string())),
                span,
                source,
            });
            return;
        };

        // `prefix : tail`: the prefix switches to a mode in which the tail is bound
        let (prefix, tail) = combos.split_at(lock + 1);
        let name = prefix
            .iter()
            .map(ToString::to_string)
            .collect::<Vec<_>>()
            .join(", ");
        if self.keymap.mode(&name).is_none() {
            self.report_chain(prefix, span, true);
            self.keymap.modes[0].bindings.push(Binding {
                chord: prefix.to_vec(),
                edge: Edge::Press,
                body: Body::Run(Run::Mode(name.clone())),
                span,
                source: source.clone(),
            });
            let mut mode = Mode::new(name.clone());
            mode.bindings.push(Binding {
                chord: vec![Combo::new(Mods::NONE, Key::ESCAPE)],
                edge: Edge::Press,
                body: Body::Run(Run::Mode(DEFAULT_MODE.to_string())),
                span,
                source: source.clone(),
            });
            self.keymap.modes.push(mode);
        }
        if let Some(mode) = self.keymap.modes.iter_mut().find(|mode| mode.name == name) {
            let escape = mode.bindings.len() - 1;
            mode.bindings.insert(
                escape,
                Binding {
                    chord: tail.to_vec(),
                    edge,
                    body: Body::Run(Run::Command(command.to_string())),
                    span,
                    source,
                },
            );
        }
    }

    fn report_chain(&mut self, prefix: &[Combo], span: Span, locked: bool) {
        if self.chains.iter().any(|reported| reported == prefix) {
            return;
        }
        self.chains.push(prefix.to_vec());
        let name = prefix
            .iter()
            .map(ToString::to_string)
            .collect::<Vec<_>>()
            .join(", ");
        let msg = if locked {
            format!(
                "in sxhkd a key that is not in the chain after `{name}` ends it; here only Escape \
                 does"
            )
        } else {
            format!(
                "in sxhkd a key that is not in the chain after `{name}` ends it; here only Escape \
                 and the chord timeout do"
            )
        };
        self.diagnostics.push(text::approximated(span, msg));
    }

    /// The chords of a hotkey, and the index of the chord after which the chain stays open (the
    /// last `:`). `None`, with a diagnostic, when the hotkey cannot be read or realized.
    fn chain(&mut self, hotkey: &str, span: Span) -> Option<(Vec<Chord>, Option<usize>)> {
        let mut chords = Vec::new();
        let mut lock = None;
        let mut rest = hotkey;
        loop {
            let end = rest.find([';', ':']).unwrap_or(rest.len());
            chords.push(self.chord(&rest[..end], span)?);
            let Some(separator) = rest[end..].chars().next() else {
                break;
            };
            if separator == ':' {
                lock = Some(chords.len() - 1);
            }
            rest = &rest[end + 1..];
            if rest.trim().is_empty() {
                self.diagnostics.push(text::error(
                    span,
                    format!("expected a chord after `{separator}`"),
                ));
                return None;
            }
        }
        Some((chords, lock))
    }

    /// `mod + mod + [~][@]keysym`.
    fn chord(&mut self, chord: &str, span: Span) -> Option<Chord> {
        let mut replay = false;
        let mut release = false;
        let mut names = Vec::new();
        for part in chord.split('+') {
            let mut name = part.trim();
            loop {
                if let Some(rest) = name.strip_prefix('~') {
                    replay = true;
                    name = rest;
                } else if let Some(rest) = name.strip_prefix('@') {
                    release = true;
                    name = rest;
                } else {
                    break;
                }
            }
            if name.is_empty() {
                self.diagnostics.push(text::error(
                    span,
                    format!("expected a key name between the `+` of `{}`", chord.trim()),
                ));
                return None;
            }
            names.push(name);
        }

        let (key, modifiers) = names.split_last()?;
        let mut mods = Mods::NONE;
        for &modifier in modifiers {
            mods |= match modifier.to_ascii_lowercase().as_str() {
                "super" | "mod4" => Mods::META,
                "alt" | "mod1" => Mods::ALT,
                "ctrl" | "control" => Mods::CTRL,
                "shift" => Mods::SHIFT,
                "hyper" | "meta" | "mode_switch" | "lock" | "mod2" | "mod3" | "mod5" | "any" => {
                    self.diagnostics.push(text::unsupported(
                        span,
                        format!("the modifier `{modifier}` has no KDE equivalent: the binding is left out"),
                    ));
                    return None;
                }
                _ => {
                    self.diagnostics
                        .push(text::error(span, format!("`{modifier}` is not a modifier")));
                    return None;
                }
            };
        }

        let found = match self.layout {
            Layout::Us => key_to_qt(Format::Sxhkd, key),
        };
        let Some(found) = found else {
            let lower = key.to_ascii_lowercase();
            let diagnostic = if lower.starts_with("button") {
                text::unsupported(
                    span,
                    format!("mouse buttons are not supported: the binding on `{key}` is left out"),
                )
            } else if lower.starts_with("kp_") {
                text::unsupported(
                    span,
                    format!("keypad keys are not supported: the binding on `{key}` is left out"),
                )
            } else {
                text::error(span, format!("no KDE key for `{key}`"))
            };
            self.diagnostics.push(diagnostic);
            return None;
        };

        Some(Chord {
            combo: Combo::new(mods, found),
            replay,
            release,
        })
    }

    /// The text with its braces expanded: one string per combination. `None`, with a diagnostic,
    /// when a brace is not closed.
    fn expand(&mut self, text: &str, span: Span) -> Option<Vec<String>> {
        match expand(text) {
            Ok(expanded) => Some(expanded),
            Err(msg) => {
                self.diagnostics.push(text::error(span, msg));
                None
            }
        }
    }
}

/// Expands the braces of an sxhkdrc hotkey or command.
fn expand(text: &str) -> Result<Vec<String>, String> {
    // the text as literal pieces and the alternatives of each pair of braces
    let mut pieces: Vec<Vec<String>> = vec![vec![String::new()]];
    let mut chars = text.chars();
    let mut inside: Option<Vec<String>> = None;
    while let Some(c) = chars.next() {
        match (c, &mut inside) {
            ('\\', _) if matches!(chars.clone().next(), Some('{' | '}')) => {
                let escaped = chars.next().unwrap_or_default();
                push(&mut pieces, &mut inside, escaped);
            }
            ('{', None) => inside = Some(vec![String::new()]),
            ('{', Some(_)) => return Err("a brace inside braces".to_string()),
            (',', Some(elements)) => elements.push(String::new()),
            ('}', Some(_)) => {
                let elements = inside.take().unwrap_or_default();
                let mut alternatives = Vec::new();
                for element in elements {
                    alternatives.extend(alternatives_of(&element));
                }
                pieces.push(alternatives);
                pieces.push(vec![String::new()]);
            }
            ('}', None) => return Err("a `}` with no `{` before it".to_string()),
            (c, _) => push(&mut pieces, &mut inside, c),
        }
    }
    if inside.is_some() {
        return Err("a `{` is not closed with `}`".to_string());
    }

    let mut expanded = vec![String::new()];
    for alternatives in pieces {
        expanded = expanded
            .iter()
            .flat_map(|prefix| alternatives.iter().map(move |a| format!("{prefix}{a}")))
            .collect();
    }
    Ok(expanded)
}

fn push(pieces: &mut [Vec<String>], inside: &mut Option<Vec<String>>, c: char) {
    match inside {
        Some(elements) => {
            if let Some(element) = elements.last_mut() {
                element.push(c);
            }
        }
        None => {
            if let Some(literal) = pieces.last_mut().and_then(|p| p.last_mut()) {
                literal.push(c);
            }
        }
    }
}

/// One element of a pair of braces: `_` is empty, `a-c` is `a`, `b`, `c`.
fn alternatives_of(element: &str) -> Vec<String> {
    if element.trim() == "_" {
        return vec![String::new()];
    }
    let chars: Vec<char> = element.trim().chars().collect();
    if let [from, '-', to] = chars[..]
        && from.is_ascii_alphanumeric()
        && to.is_ascii_alphanumeric()
        && from <= to
    {
        return (from..=to).map(String::from).collect();
    }
    vec![element.to_string()]
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Severity;

    fn read(text: &str) -> Parsed {
        parse(text, &ParseOptions::default())
    }

    fn messages(parsed: &Parsed) -> Vec<String> {
        parsed.diagnostics.iter().map(ToString::to_string).collect()
    }

    fn rendered(mode: &Mode) -> Vec<String> {
        mode.bindings
            .iter()
            .map(|binding| {
                let chord: Vec<String> = binding.chord.iter().map(ToString::to_string).collect();
                let body = match &binding.body {
                    Body::Run(Run::Command(command)) => command.clone(),
                    Body::Run(Run::Mode(mode)) => format!("mode {mode}"),
                    Body::Scoped(_) => "scoped".to_string(),
                };
                format!("{}: {body}", chord.join(", "))
            })
            .collect()
    }

    #[test]
    fn braces_make_one_binding_per_alternative() {
        assert_eq!(expand("a{b,c}d").unwrap(), ["abd", "acd"]);
        assert_eq!(
            expand("{_,shift + }{1-3}").unwrap(),
            ["1", "2", "3", "shift + 1", "shift + 2", "shift + 3"]
        );
        assert_eq!(expand("{a-c,x}").unwrap(), ["a", "b", "c", "x"]);
        assert_eq!(expand("echo \\{a\\}").unwrap(), ["echo {a}"]);
        assert_eq!(expand("no braces").unwrap(), ["no braces"]);
        assert_eq!(
            expand("{focus-workspace,x}").unwrap(),
            ["focus-workspace", "x"]
        );
        assert!(expand("{a,b").is_err());
        assert!(expand("a}").is_err());
        assert!(expand("{a,{b}}").is_err());
    }

    #[test]
    fn the_hotkey_and_the_command_expand_together() {
        let parsed = read(
            "super + {h,l}\n    komorebic focus {left,right}\nsuper + {1,2}\n    komorebic retile\n",
        );
        assert!(parsed.diagnostics.is_empty(), "{:?}", parsed.diagnostics);
        assert_eq!(
            rendered(&parsed.keymap.modes[0]),
            [
                "Meta+H: komorebic focus left",
                "Meta+L: komorebic focus right",
                "Meta+1: komorebic retile",
                "Meta+2: komorebic retile",
            ]
        );
        let parsed = read("super + {h,l}\n    echo {a,b,c}\n");
        assert_eq!(
            messages(&parsed),
            ["line 1: error: the hotkey has 2 alternatives and the command 3"]
        );
    }

    #[test]
    fn keysyms_and_modifiers() {
        let parsed = read(
            "mod4 + Return\n    a\nmod1 + control + bracketleft\n    b\nsuper + shift + 1\n    c\nXF86AudioRaiseVolume\n    d\n",
        );
        assert_eq!(
            rendered(&parsed.keymap.modes[0]),
            [
                "Meta+Return: a",
                "Ctrl+Alt+[: b",
                "Meta+!: c",
                "Volume Up: d"
            ]
        );
    }

    #[test]
    fn comments_continued_lines_and_spans() {
        let text = "# a comment\n\nsuper + \\\n  h\n    komorebic \\\n    focus left # trailing\n";
        let parsed = read(text);
        assert!(parsed.diagnostics.is_empty(), "{:?}", parsed.diagnostics);
        let binding = &parsed.keymap.modes[0].bindings[0];
        assert_eq!(binding.chord[0].to_string(), "Meta+H");
        assert_eq!(
            binding.body,
            Body::Run(Run::Command("komorebic focus left".into()))
        );
        assert_eq!(binding.span.line, 3);
        assert_eq!(
            &text[binding.span.start..binding.span.end],
            "super + \\\n  h\n    komorebic \\\n    focus left # trailing"
        );
        assert_eq!(binding.source, "super + h: komorebic focus left");
    }

    #[test]
    fn a_chain_is_one_binding_with_several_combos() {
        let parsed = read(
            "super + w ; h\n    komorebic move left\nsuper + w; l\n    komorebic move right\n",
        );
        assert_eq!(
            rendered(&parsed.keymap.modes[0]),
            [
                "Meta+W, H: komorebic move left",
                "Meta+W, L: komorebic move right",
            ]
        );
        // reported once for the chain
        assert_eq!(
            messages(&parsed),
            [
                "line 1: warning: in sxhkd a key that is not in the chain after `Meta+W` ends it; \
              here only Escape and the chord timeout do"
            ]
        );
    }

    #[test]
    fn a_chain_that_stays_open_is_a_mode() {
        let parsed = read("super + r : {h,l}\n    komorebic resize-edge {left,right} increase\n");
        let keymap = &parsed.keymap;
        assert_eq!(rendered(&keymap.modes[0]), ["Meta+R: mode Meta+R"]);
        let mode = keymap.mode("Meta+R").unwrap();
        assert!(!mode.capture);
        assert_eq!(
            rendered(mode),
            [
                "H: komorebic resize-edge left increase",
                "L: komorebic resize-edge right increase",
                "Esc: mode default",
            ]
        );
        assert_eq!(parsed.diagnostics.len(), 1);
        assert_eq!(parsed.diagnostics[0].support, Support::Approximated);
    }

    #[test]
    fn release_and_replay_are_approximated() {
        let parsed = read("super + @space\n    a\n~super + x\n    b\n");
        let bindings = &parsed.keymap.modes[0].bindings;
        assert_eq!(bindings[0].edge, Edge::Release);
        assert_eq!(bindings[1].edge, Edge::Press);
        assert_eq!(
            messages(&parsed),
            [
                "line 1: warning: a release binding fires when any key of its combo is released first",
                "line 3: warning: `~` passes the key on in sxhkd; here the application does not get it",
            ]
        );
    }

    #[test]
    fn what_cannot_be_read_is_reported_and_left_out() {
        for (text, message) in [
            (
                "super + button1\n    x\n",
                "line 1: warning: mouse buttons are not supported: the binding on `button1` is left out",
            ),
            (
                "super + KP_1\n    x\n",
                "line 1: warning: keypad keys are not supported: the binding on `KP_1` is left out",
            ),
            (
                "hyper + a\n    x\n",
                "line 1: warning: the modifier `hyper` has no KDE equivalent: the binding is left out",
            ),
            (
                "super + nope\n    x\n",
                "line 1: error: no KDE key for `nope`",
            ),
            ("h + super\n    x\n", "line 1: error: `h` is not a modifier"),
            (
                "super + \n    x\n",
                "line 1: error: expected a key name between the `+` of `super +`",
            ),
            (
                "super + a\n",
                "line 1: error: the hotkey has no command below it",
            ),
            (
                "    x\n",
                "line 1: error: a command with no hotkey before it",
            ),
            (
                "super + {a,b\n    x\n",
                "line 1: error: a `{` is not closed with `}`",
            ),
            (
                "super + a :\n    x\n",
                "line 1: error: expected a chord after `:`",
            ),
        ] {
            let parsed = read(text);
            assert!(parsed.keymap.modes[0].bindings.is_empty(), "{text}");
            assert_eq!(messages(&parsed), [message], "{text}");
            if message.contains("warning") {
                assert_eq!(parsed.diagnostics[0].severity, Severity::Warning);
            }
        }
    }

    #[test]
    fn a_synchronous_command_is_noted() {
        let parsed = read("super + a\n    ; komorebic retile\n");
        assert_eq!(
            rendered(&parsed.keymap.modes[0]),
            ["Meta+A: komorebic retile"]
        );
        assert_eq!(parsed.diagnostics[0].severity, Severity::Note);
    }
}
