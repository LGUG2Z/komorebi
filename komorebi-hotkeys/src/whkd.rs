//! Reading a whkdrc.
//!
//! A whkdrc is the file of whkd, the hotkey daemon komorebi for Windows uses. A binding is the
//! keys, joined by `+`, then `:` and the command; the last key is the one that is pressed, the
//! others are modifiers:
//!
//! ```text
//! .shell pwsh                         # the shell whkd runs commands with
//! .pause alt + shift + p              # turns every other binding off and on
//! .pause_hook komorebic toggle-pause  # runs when it does
//!
//! alt + n [                           # one command per application
//!     firefox : komorebic focus-workspace 1
//!     kitty   : Ignore                # kitty gets the key
//!     Default : komorebic focus-workspace 0
//! ]
//!
//! alt + h : komorebic focus left
//! alt + shift + oem_4 : komorebic cycle-focus previous  # oem_4 is [
//! ```
//!
//! [`parse`] reads it into a [`Keymap`]:
//!
//! ```rust
//! use komorebi_hotkeys::{ParseOptions, whkd};
//!
//! let parsed = whkd::parse(
//!     ".shell pwsh\nalt + shift + oem_4 : komorebic cycle-focus previous # oem_4 is [\n",
//!     &ParseOptions::default(),
//! );
//! let binding = &parsed.keymap.modes[0].bindings[0];
//! // Shift+[ types {, which is what KWin matches
//! assert_eq!(binding.chord[0].to_string(), "Alt+{");
//! // commands run through `sh -c`, not PowerShell, which the reader notes
//! assert_eq!(
//!     parsed.diagnostics[0].to_string(),
//!     "line 1: note: commands run through `sh -c`, not `pwsh`",
//! );
//! ```
//!
//! # What is read, and how
//!
//! - **Bindings**: `keys : command`. The modifiers are `alt`, `ctrl`, `shift` and `win` (which is
//!   Meta), and the key is a Windows virtual key name with or without `VK_` (`h`, `1`, `return`,
//!   `oem_4`...), read as on a US keyboard. A `#` starts a comment, wherever it is, as in whkd.
//! - **Per-application bindings**: `keys [ app : command ... ]`, with `app : Ignore` for an
//!   application that gets the key itself and `Default` for every other application. An
//!   application's name may have spaces in it.
//! - **`.pause keys`** is the binding that turns the others off and on, and `.pause_hook command`
//!   the command it runs each time.
//!
//! whkd itself requires `.shell` first, then `.pause` and `.pause_hook`, then the per-application
//! bindings, then the others. This reader takes them in any order.
//!
//! # What is not, and the diagnostics about it
//!
//! - `.shell` is not followed: commands run through `sh -c` (a note).
//! - `lalt`, `ralt`, `lwin` and the other side-specific modifiers are the modifier itself, since KDE
//!   does not tell the sides apart ([`Support::Approximated`]).
//! - A key that has no KDE key, such as the keypad's, is left out ([`Support::Unsupported`]).
//!
//! Text that whkd would not read is an error and is skipped.
//!
//! [`Keymap`]: crate::Keymap
//! [`Support::Approximated`]: crate::Support::Approximated
//! [`Support::Unsupported`]: crate::Support::Unsupported

use crate::App;
use crate::Binding;
use crate::Body;
use crate::Diagnostic;
use crate::Edge;
use crate::Format;
use crate::Keymap;
use crate::ParseOptions;
use crate::Parsed;
use crate::Pause;
use crate::Run;
use crate::Scope;
use crate::Span;
use crate::Support;
use crate::Then;
use crate::qt::Combo;
use crate::qt::Layout;
use crate::qt::Mods;
use crate::qt::key_to_qt;
use crate::text;
use crate::text::Lines;

/// Reads a whkdrc.
///
/// Reading never fails: what cannot be read is left out, with a [`Diagnostic`] that says why.
/// The [module documentation](self) lists what is read and what is not.
///
/// # Example
///
/// ```rust
/// use komorebi_hotkeys::{App, Body, ParseOptions, Then, whkd};
///
/// let parsed = whkd::parse(
///     "alt + q [\n    Default : komorebic close\n    Google Chrome : Ignore\n]\n",
///     &ParseOptions::default(),
/// );
/// let Body::Scoped(scopes) = &parsed.keymap.modes[0].bindings[0].body else {
///     panic!("a binding per application");
/// };
/// assert_eq!(scopes[0].app, App::Other);
/// assert_eq!(scopes[1].app, App::Named("Google Chrome".into()));
/// assert_eq!(scopes[1].then, Then::Ignore);
/// ```
pub fn parse(text: &str, options: &ParseOptions) -> Parsed {
    let lines = Lines::new(text);
    let mut keymap = Keymap::new(Format::Whkd);
    let mut diagnostics = Vec::new();
    let mut hook: Option<(String, Span)> = None;
    // the keys of the list being read, where it started, and its scopes
    let mut list: Option<(Option<Combo>, usize, Vec<Scope>)> = None;

    let mut offset = 0;
    for raw in text.split_inclusive('\n') {
        let start = offset;
        offset += raw.len();
        let line = raw.trim_end_matches(['\n', '\r']);
        // a `#` starts a comment wherever it is
        let content = line.split('#').next().unwrap_or_default();
        let trimmed = content.trim();
        if trimmed.is_empty() {
            continue;
        }
        let indent = content.len() - content.trim_start().len();
        let at = start + indent;
        let span = lines.span(at, at + trimmed.len());

        if let Some((combo, list_start, scopes)) = &mut list {
            if trimmed.starts_with(']') {
                let end = at + 1;
                if let Some(combo) = *combo {
                    if scopes.is_empty() {
                        diagnostics.push(text::error(span, "the list of applications is empty"));
                    } else {
                        keymap.modes[0].bindings.push(Binding {
                            chord: vec![combo],
                            edge: Edge::Press,
                            body: Body::Scoped(std::mem::take(scopes)),
                            span: lines.span(*list_start, end),
                            source: text::one_line(&text[*list_start..end]),
                        });
                    }
                }
                list = None;
                continue;
            }
            match trimmed.split_once(':') {
                Some((app, command)) => {
                    let app = app.split_whitespace().collect::<Vec<_>>().join(" ");
                    let command = command.trim();
                    if app.is_empty() {
                        diagnostics.push(text::error(span, "expected an application before `:`"));
                    } else if command.is_empty() {
                        diagnostics.push(text::error(span, "the command is empty"));
                    } else {
                        scopes.push(Scope {
                            app: if app == "Default" {
                                App::Other
                            } else {
                                App::Named(app)
                            },
                            then: if command == "Ignore" {
                                Then::Ignore
                            } else {
                                Then::Run(Run::Command(command.to_string()))
                            },
                        });
                    }
                }
                None => diagnostics.push(text::error(
                    span,
                    "expected `application : command`, or `]` to close the list",
                )),
            }
            continue;
        }

        if let Some(directive) = trimmed.strip_prefix('.') {
            let (name, argument) = directive
                .split_once(char::is_whitespace)
                .map_or((directive, ""), |(name, argument)| (name, argument.trim()));
            match name {
                "shell" => diagnostics.push(text::note(
                    span,
                    Support::Approximated,
                    format!("commands run through `sh -c`, not `{argument}`"),
                )),
                "pause" => {
                    if let Some(combo) = keys(argument, at, &lines, options, &mut diagnostics) {
                        keymap.pause = Some(Pause {
                            combo,
                            hook: None,
                            span,
                        });
                    }
                }
                "pause_hook" if argument.is_empty() => {
                    diagnostics.push(text::error(span, "the command is empty"));
                }
                "pause_hook" => hook = Some((argument.to_string(), span)),
                _ => diagnostics.push(text::error(
                    span,
                    format!("`.{name}` is not a whkd directive"),
                )),
            }
            continue;
        }

        if let Some(keys_text) = trimmed.strip_suffix('[') {
            let combo = keys(keys_text, at, &lines, options, &mut diagnostics);
            list = Some((combo, at, Vec::new()));
            continue;
        }

        let Some((keys_text, command)) = trimmed.split_once(':') else {
            diagnostics.push(text::error(span, "expected `keys : command`"));
            continue;
        };
        let command = command.trim();
        if command.is_empty() {
            diagnostics.push(text::error(span, "the command is empty"));
            continue;
        }
        if let Some(combo) = keys(keys_text, at, &lines, options, &mut diagnostics) {
            keymap.modes[0].bindings.push(Binding {
                chord: vec![combo],
                edge: Edge::Press,
                body: Body::Run(Run::Command(command.to_string())),
                span: lines.span(at, start + line.trim_end().len()),
                source: line.trim().to_string(),
            });
        }
    }

    if let Some((_, list_start, _)) = list {
        diagnostics.push(text::error(
            lines.span(list_start, list_start + 1),
            "the list is not closed with `]`",
        ));
    }
    match (&mut keymap.pause, hook) {
        (Some(pause), Some((hook, _))) => pause.hook = Some(hook),
        (None, Some((_, span))) => diagnostics.push(text::note(
            span,
            Support::Exact,
            "`.pause_hook` runs only with a `.pause` binding, and there is none",
        )),
        (_, None) => {}
    }

    diagnostics.sort_by_key(|d| d.span.start);
    Parsed {
        keymap,
        diagnostics,
    }
}

/// `mod + mod + key`, starting at byte `at` of the text. `None` when the keys cannot be read or
/// realized, with a diagnostic.
fn keys(
    keys: &str,
    at: usize,
    lines: &Lines,
    options: &ParseOptions,
    diagnostics: &mut Vec<Diagnostic>,
) -> Option<Combo> {
    let mut names = Vec::new();
    let mut offset = 0;
    for part in keys.split('+') {
        let name = part.trim();
        let start = at + offset + (part.len() - part.trim_start().len());
        offset += part.len() + 1;
        let span = lines.span(start, start + name.len());
        if name.is_empty() || !name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_') {
            diagnostics.push(text::error(
                span,
                format!("expected a key name between the `+`, found `{name}`"),
            ));
            return None;
        }
        names.push((name, span));
    }

    let (key, modifiers) = names.split_last()?;
    let mut mods = Mods::NONE;
    for &(name, span) in modifiers {
        mods |= match name.to_ascii_lowercase().as_str() {
            "alt" | "menu" => Mods::ALT,
            "ctrl" | "control" => Mods::CTRL,
            "shift" => Mods::SHIFT,
            "win" => Mods::META,
            "lalt" | "ralt" | "lmenu" | "rmenu" => {
                diagnostics.push(text::one_sided(span, name, "Alt"));
                Mods::ALT
            }
            "lctrl" | "rctrl" | "lcontrol" | "rcontrol" => {
                diagnostics.push(text::one_sided(span, name, "Ctrl"));
                Mods::CTRL
            }
            "lshift" | "rshift" => {
                diagnostics.push(text::one_sided(span, name, "Shift"));
                Mods::SHIFT
            }
            "lwin" | "rwin" => {
                diagnostics.push(text::one_sided(span, name, "Meta"));
                Mods::META
            }
            _ => {
                diagnostics.push(text::error(span, format!("`{name}` is not a modifier")));
                return None;
            }
        };
    }

    let (name, span) = *key;
    let found = match options.layout {
        Layout::Us => key_to_qt(Format::Whkd, name),
    };
    match found {
        Some(key) => Some(Combo::new(mods, key)),
        None => {
            let lower = name.to_ascii_lowercase();
            let lower = lower.strip_prefix("vk_").unwrap_or(&lower);
            if WITHOUT_KDE_KEY.contains(&lower) || lower.starts_with("numpad") {
                diagnostics.push(text::unsupported(
                    span,
                    format!("no KDE key for `{name}`: the binding is left out"),
                ));
            } else {
                diagnostics.push(text::error(
                    span,
                    format!("`{name}` is not a key whkd knows"),
                ));
            }
            None
        }
    }
}

/// The virtual key names whkd knows that are not a key here, other than the keypad's.
const WITHOUT_KDE_KEY: &[&str] = &[
    "multiply",
    "add",
    "separator",
    "subtract",
    "decimal",
    "divide",
    "print",
    "execute",
    "launch_app1",
    "launch_app2",
    "oem_8",
    "oem_102",
    "attn",
    "crsel",
    "exsel",
    "play",
    "zoom",
    "pa1",
    "oem_clear",
    "shift",
    "ctrl",
    "control",
    "alt",
    "menu",
    "win",
    "lwin",
    "rwin",
    "lshift",
    "rshift",
    "lctrl",
    "lcontrol",
    "rctrl",
    "rcontrol",
    "lalt",
    "lmenu",
    "ralt",
    "rmenu",
];

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

    fn keys_of(parsed: &Parsed) -> Vec<String> {
        parsed.keymap.modes[0]
            .bindings
            .iter()
            .map(|b| b.chord[0].to_string())
            .collect()
    }

    #[test]
    fn bindings_and_their_keys() {
        let parsed = read(
            ".shell cmd\n\
             alt + h     : komorebic focus left # so should comments at the end of a line\n\
             alt + 1 : komorebic focus-workspace 0\n\
             f11 : echo \"hello f11\"\n\
             win + shift + return : konsole\n\
             ctrl + alt + vk_left : komorebic stack left\n\
             alt + shift + oem_plus : komorebic resize-axis vertical increase\n",
        );
        assert_eq!(
            keys_of(&parsed),
            [
                "Alt+H",
                "Alt+1",
                "F11",
                "Meta+Shift+Return",
                "Ctrl+Alt+Left",
                "Alt++"
            ]
        );
        let first = &parsed.keymap.modes[0].bindings[0];
        assert_eq!(
            first.body,
            Body::Run(Run::Command("komorebic focus left".into()))
        );
        assert_eq!(first.span.line, 2);
        assert_eq!(
            first.source,
            "alt + h     : komorebic focus left # so should comments at the end of a line"
        );
        assert_eq!(
            messages(&parsed),
            ["line 1: note: commands run through `sh -c`, not `cmd`"]
        );
    }

    #[test]
    fn a_hash_starts_a_comment_wherever_it_is() {
        let parsed = read("alt + e : echo '#not a comment in a shell'\n");
        assert_eq!(
            parsed.keymap.modes[0].bindings[0].body,
            Body::Run(Run::Command("echo '".into()))
        );
    }

    #[test]
    fn a_binding_can_do_something_else_in_each_application() {
        let parsed = read(
            "alt + n [\n\
             \x20   # ProcessName as shown by `Get-Process`\n\
             \x20   Firefox       : echo \"hello firefox\"\n\
             \x20   Google Chrome : echo \"hello chrome\"\n\
             \x20   Zen Browser   : Ignore\n\
             \x20   Default       : echo \"hello world\"\n\
             ]\n\
             alt + h : x\n",
        );
        assert!(parsed.diagnostics.is_empty(), "{:?}", parsed.diagnostics);
        let bindings = &parsed.keymap.modes[0].bindings;
        assert_eq!(bindings.len(), 2);
        assert_eq!(
            bindings[0].body,
            Body::Scoped(vec![
                Scope {
                    app: App::Named("Firefox".into()),
                    then: Then::Run(Run::Command("echo \"hello firefox\"".into())),
                },
                Scope {
                    app: App::Named("Google Chrome".into()),
                    then: Then::Run(Run::Command("echo \"hello chrome\"".into())),
                },
                Scope {
                    app: App::Named("Zen Browser".into()),
                    then: Then::Ignore,
                },
                Scope {
                    app: App::Other,
                    then: Then::Run(Run::Command("echo \"hello world\"".into())),
                },
            ])
        );
        assert_eq!(bindings[0].span.line, 1);
        assert!(bindings[0].source.starts_with("alt + n [ # ProcessName"));
        assert!(bindings[0].source.ends_with("]"));
    }

    #[test]
    fn the_pause_binding_and_its_hook() {
        let parsed = read(
            ".shell pwsh                            # can be one of cmd | pwsh | powershell\n\
             .pause alt + shift + p                 # toggles all other hotkeys\n\
             .pause_hook komorebic toggle-pause     # another comment\n\
             alt + h : echo \"Hello\"\n",
        );
        let pause = parsed.keymap.pause.as_ref().unwrap();
        assert_eq!(pause.combo.to_string(), "Alt+Shift+P");
        assert_eq!(pause.hook.as_deref(), Some("komorebic toggle-pause"));
        assert_eq!(pause.span.line, 2);
        assert_eq!(keys_of(&parsed), ["Alt+H"]);
    }

    #[test]
    fn what_cannot_be_read_is_reported_and_left_out() {
        for (text, message) in [
            (
                "alt + numpad1 : x",
                "line 1: warning: no KDE key for `numpad1`: the binding is left out",
            ),
            (
                "alt + nope : x",
                "line 1: error: `nope` is not a key whkd knows",
            ),
            ("h + alt : x", "line 1: error: `h` is not a modifier"),
            (
                "alt + : x",
                "line 1: error: expected a key name between the `+`, found ``",
            ),
            (
                "alt + [ : x",
                "line 1: error: expected a key name between the `+`, found `[`",
            ),
            ("alt + h", "line 1: error: expected `keys : command`"),
            ("alt + h :", "line 1: error: the command is empty"),
            ("alt + h : # nothing", "line 1: error: the command is empty"),
            (
                ".blacklist steam",
                "line 1: error: `.blacklist` is not a whkd directive",
            ),
            (
                "alt + n [\n  firefox : x",
                "line 1: error: the list is not closed with `]`",
            ),
            (
                "alt + n [\n]",
                "line 2: error: the list of applications is empty",
            ),
            (
                ".pause_hook x",
                "line 1: note: `.pause_hook` runs only with a `.pause` binding, and there is none",
            ),
        ] {
            let parsed = read(text);
            assert!(parsed.keymap.modes[0].bindings.is_empty(), "{text}");
            assert_eq!(messages(&parsed), [message], "{text}");
        }
    }

    #[test]
    fn side_specific_modifiers_are_the_modifier() {
        let text = "lwin + h : x\nralt + j : y\n";
        let parsed = read(text);
        assert_eq!(keys_of(&parsed), ["Meta+H", "Alt+J"]);
        let diagnostic = &parsed.diagnostics[1];
        assert_eq!(diagnostic.severity, Severity::Warning);
        assert_eq!(diagnostic.support, Support::Approximated);
        assert_eq!(&text[diagnostic.span.start..diagnostic.span.end], "ralt");
        assert_eq!(diagnostic.span.line, 2);
    }

    #[test]
    fn reading_goes_on_after_an_error() {
        let parsed = read("alt + nope : x\nalt + h : y\nalt + n [\n  firefox x\n  kitty : z\n]\n");
        assert_eq!(keys_of(&parsed), ["Alt+H", "Alt+N"]);
        let Body::Scoped(scopes) = &parsed.keymap.modes[0].bindings[1].body else {
            panic!("{:?}", parsed.keymap.modes[0].bindings[1].body);
        };
        assert_eq!(scopes.len(), 1);
        assert_eq!(
            messages(&parsed),
            [
                "line 1: error: `nope` is not a key whkd knows",
                "line 4: error: expected `application : command`, or `]` to close the list",
            ]
        );
    }
}
