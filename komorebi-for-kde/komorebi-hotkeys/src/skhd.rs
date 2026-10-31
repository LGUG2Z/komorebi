//! Reading an skhdrc.
//!
//! An skhdrc is the file of skhd, the hotkey daemon of macOS, and of skhd.zig, which reads the
//! same syntax and adds to it. A binding is the modifiers, a `-`, the key, and then what it does:
//!
//! ```text
//! alt - h : komorebic focus left                  # runs a command
//! alt + shift - 0x21 : komorebic cycle-focus previous  # a key named by its macOS keycode
//! alt - r ; resize                                 # switches to a mode
//! resize < escape ; default                        # a binding of the mode `resize`
//! alt - n [                                        # one command per application
//!     "firefox" : komorebic focus-workspace 1
//!     "kitty"   ~                                  # kitty gets the key
//!     *         : komorebic focus-workspace 0      # every other application
//! ]
//! ```
//!
//! [`parse`] reads it into a [`Keymap`]:
//!
//! ```rust
//! use komorebi_hotkeys::{Body, ParseOptions, Run, skhd};
//!
//! let parsed = skhd::parse(
//!     "alt + shift - 0x21 : komorebic cycle-focus previous # 0x21 is [\n",
//!     &ParseOptions::default(),
//! );
//! assert!(parsed.diagnostics.is_empty());
//!
//! let binding = &parsed.keymap.modes[0].bindings[0];
//! // Shift+[ types {, which is what KWin matches
//! assert_eq!(binding.chord[0].to_string(), "Alt+{");
//! assert_eq!(binding.body, Body::Run(Run::Command("komorebic cycle-focus previous".into())));
//! ```
//!
//! # What is read, and how
//!
//! - **Bindings**: `mods - key : command`. The modifiers are `alt`, `shift`, `ctrl`, `cmd` (which
//!   is Meta), `hyper` (all four) and `meh` (all but `cmd`). The key is a literal (`h`, `return`,
//!   `f1`, `sound_up`...) or a macOS keycode in hex (`0x21`), read as the key at that position on
//!   a US keyboard. A trailing ` # comment` is not part of the command, and a line that ends in
//!   `\` continues on the next.
//! - **Modes**: `:: name` declares a mode, `:: name : command` with a command to run on entering
//!   it; `key ; name` switches to it, and `name < key : command` (or `a, b < key ...`) binds a key
//!   in it. A mode may be used before it is declared.
//! - **Per-application bindings**: `key [ "app" : command ... ]`, with `"app" ~` for an
//!   application that gets the key itself and `*` for every other application.
//! - **`.blacklist [ "app" ... ]`**: applications in which no binding is active.
//! - **`.define`** (skhd.zig): `.define name : command {{1}}` is a command template, used as
//!   `@name("argument")`, and `.define name ["app", ...]` a group of applications, used as
//!   `@name` in a per-application list. Both are expanded where they are used.
//!
//! # What is not, and the diagnostics about it
//!
//! - `lalt`, `ralt` and the other side-specific modifiers are the modifier itself, since KDE does
//!   not tell the sides apart ([`Support::Approximated`]).
//! - A capturing mode (`:: name @`) does not swallow the keys it has no binding for: they reach
//!   the application ([`Support::Approximated`]).
//! - `->` does not pass the key on to the application ([`Support::Approximated`]).
//! - `.shell` is not followed: commands run through `sh -c` (a note).
//! - `fn`, a binding on a keycode that has no KDE key, `.load`, `.alias`, skhd.zig's key
//!   forwarding (`|`) and key sequences (`,`), and any other directive are left out
//!   ([`Support::Unsupported`]).
//!
//! Text that is not valid skhd syntax is an error and is skipped; reading goes on at the next
//! line.
//!
//! [`Keymap`]: crate::Keymap
//! [`Support::Approximated`]: crate::Support::Approximated
//! [`Support::Unsupported`]: crate::Support::Unsupported

use crate::App;
use crate::Binding;
use crate::Body;
use crate::DEFAULT_MODE;
use crate::Define;
use crate::DefineValue;
use crate::Diagnostic;
use crate::Edge;
use crate::Format;
use crate::Keymap;
use crate::Mode;
use crate::ParseOptions;
use crate::Parsed;
use crate::Run;
use crate::Scope;
use crate::Span;
use crate::Support;
use crate::Then;
use crate::qt::Combo;
use crate::qt::Key;
use crate::qt::Layout;
use crate::qt::Mods;
use crate::qt::key_to_qt;
use crate::text;
use crate::text::Lines;

/// Reads an skhdrc.
///
/// Reading never fails: what cannot be read is left out, with a [`Diagnostic`] that says why.
/// The [module documentation](self) lists what is read and what is not.
///
/// # Example
///
/// ```rust
/// use komorebi_hotkeys::{ParseOptions, Severity, skhd};
///
/// let parsed = skhd::parse(
///     "alt - h : komorebic focus left\nfn - j : komorebic focus down\n",
///     &ParseOptions::default(),
/// );
/// assert_eq!(parsed.keymap.modes[0].bindings.len(), 1);
/// assert_eq!(
///     parsed.diagnostics[0].to_string(),
///     "line 2: warning: the fn modifier does not exist on a PC keyboard",
/// );
/// ```
pub fn parse(text: &str, options: &ParseOptions) -> Parsed {
    let mut reader = Reader::new(text, options.layout);
    reader.run();
    reader.finish()
}

/// A binding that waits for the modes to be known.
struct Pending {
    modes: Vec<(String, Span)>,
    binding: Binding,
}

/// A mode declaration.
struct Declared {
    name: String,
    capture: bool,
    on_enter: Option<String>,
}

struct Reader<'a> {
    src: &'a str,
    pos: usize,
    lines: Lines,
    layout: Layout,
    keymap: Keymap,
    diagnostics: Vec<Diagnostic>,
    declared: Vec<Declared>,
    pending: Vec<Pending>,
    /// Where the `[` of the list being read is, so that an error inside skips the rest of it.
    open_list: Option<usize>,
}

impl<'a> Reader<'a> {
    fn new(src: &'a str, layout: Layout) -> Reader<'a> {
        Reader {
            src,
            pos: 0,
            lines: Lines::new(src),
            layout,
            keymap: Keymap::new(Format::Skhd),
            diagnostics: Vec::new(),
            declared: Vec::new(),
            pending: Vec::new(),
            open_list: None,
        }
    }

    fn run(&mut self) {
        loop {
            self.skip_blank();
            let Some(c) = self.peek() else { break };
            let start = self.pos;
            let result = if c == '.' {
                self.directive(start)
            } else if self.rest().starts_with("::") {
                self.mode_declaration()
            } else {
                self.binding(start)
            };
            if let Err(diagnostic) = result {
                self.diagnostics.push(diagnostic);
                self.recover();
            }
            if self.pos == start {
                // nothing was consumed: never loop on the same byte
                self.skip_line();
            }
        }
    }

    fn finish(mut self) -> Parsed {
        let mut keymap = self.keymap;
        for declared in self.declared {
            if declared.name == DEFAULT_MODE {
                keymap.modes[0].capture = declared.capture;
                keymap.modes[0].on_enter = declared.on_enter;
            } else {
                keymap.modes.push(Mode {
                    name: declared.name,
                    capture: declared.capture,
                    on_enter: declared.on_enter,
                    bindings: Vec::new(),
                });
            }
        }

        for pending in self.pending {
            if let Some(target) = switches_to(&pending.binding.body)
                && keymap.mode(&target).is_none()
            {
                self.diagnostics.push(text::error(
                    pending.binding.span,
                    format!("there is no mode `{target}` to switch to"),
                ));
                continue;
            }
            for (name, span) in &pending.modes {
                match keymap.modes.iter_mut().find(|mode| mode.name == *name) {
                    Some(mode) => mode.bindings.push(pending.binding.clone()),
                    None => self.diagnostics.push(text::error(
                        *span,
                        format!("the mode `{name}` is not declared"),
                    )),
                }
            }
        }

        self.diagnostics.sort_by_key(|d| d.span.start);
        Parsed {
            keymap,
            diagnostics: self.diagnostics,
        }
    }

    fn binding(&mut self, start: usize) -> Result<(), Diagnostic> {
        let modes = self.mode_list()?;
        let combo = self.keysym()?;
        self.skip_spaces();

        if self.rest().starts_with("->") {
            let span = self.span_here(2);
            self.pos += 2;
            self.diagnostics.push(text::approximated(
                span,
                "`->` passes the key on in skhd; here the application does not get it",
            ));
            self.skip_spaces();
        }

        let body = match self.peek() {
            Some(':') => {
                self.pos += 1;
                Body::Run(Run::Command(self.command()?))
            }
            Some(';') => {
                self.pos += 1;
                self.skip_spaces();
                let name = self
                    .word()
                    .ok_or_else(|| self.error_here("expected the name of a mode after `;`"))?;
                self.end_of_line()?;
                Body::Run(Run::Mode(name.to_string()))
            }
            Some('[') => {
                self.open_list = Some(self.pos);
                self.pos += 1;
                let scopes = self.application_list()?;
                self.open_list = None;
                Body::Scoped(scopes)
            }
            Some('|') => {
                let span = self.span_here(1);
                self.skip_line();
                self.diagnostics.push(text::unsupported(
                    span,
                    "key forwarding (`|`) is not supported: the binding is left out",
                ));
                return Ok(());
            }
            Some(',') => {
                let span = self.span_here(1);
                self.recover();
                self.diagnostics.push(text::unsupported(
                    span,
                    "key sequences (`,`) are not supported: the binding is left out",
                ));
                return Ok(());
            }
            _ => return Err(self.error_here("expected `:` and a command, `;` and a mode, or `[`")),
        };

        let Some(combo) = combo else {
            // a key or modifier that cannot be realized, already reported
            return Ok(());
        };
        let modes = if modes.is_empty() {
            vec![(DEFAULT_MODE.to_string(), self.lines.span(start, start))]
        } else {
            modes
        };
        let span = self.lines.span(start, self.pos);
        self.pending.push(Pending {
            modes,
            binding: Binding {
                chord: vec![combo],
                edge: Edge::Press,
                body,
                span,
                source: text::one_line(&self.src[start..self.pos]),
            },
        });
        Ok(())
    }

    /// `mode, mode <` before a binding, or nothing.
    fn mode_list(&mut self) -> Result<Vec<(String, Span)>, Diagnostic> {
        let save = self.pos;
        let Some((first, first_span)) = self.word_spanned() else {
            return Ok(Vec::new());
        };
        self.skip_spaces();
        if !matches!(self.peek(), Some(',' | '<')) {
            self.pos = save;
            return Ok(Vec::new());
        }

        let mut modes = vec![(first.to_string(), first_span)];
        while self.peek() == Some(',') {
            self.pos += 1;
            self.skip_spaces();
            let (name, span) = self
                .word_spanned()
                .ok_or_else(|| self.error_here("expected the name of a mode after `,`"))?;
            modes.push((name.to_string(), span));
            self.skip_spaces();
        }
        if self.peek() != Some('<') {
            return Err(self.error_here("expected `<` after the modes"));
        }
        self.pos += 1;
        self.skip_spaces();
        Ok(modes)
    }

    /// `mods - key` or `key`. `None` when the keys are valid but cannot be realized.
    fn keysym(&mut self) -> Result<Option<Combo>, Diagnostic> {
        let first = self
            .word_spanned()
            .ok_or_else(|| self.error_here("expected a key"))?;
        self.skip_spaces();

        let is_dash = self.peek() == Some('-') && !self.rest().starts_with("->");
        let (modifiers, key) = if self.peek() == Some('+') || is_dash {
            let mut modifiers = vec![first];
            while self.peek() == Some('+') {
                self.pos += 1;
                self.skip_spaces();
                modifiers.push(
                    self.word_spanned()
                        .ok_or_else(|| self.error_here("expected a modifier after `+`"))?,
                );
                self.skip_spaces();
            }
            if self.peek() != Some('-') {
                return Err(self.error_here("expected `-` and a key after the modifiers"));
            }
            self.pos += 1;
            self.skip_spaces();
            let key = self
                .word_spanned()
                .ok_or_else(|| self.error_here("expected a key after `-`"))?;
            (modifiers, key)
        } else {
            (Vec::new(), first)
        };

        let mut mods = Mods::NONE;
        let mut realizable = true;
        for (word, span) in modifiers {
            let lower = word.to_ascii_lowercase();
            mods |= match lower.as_str() {
                "alt" => Mods::ALT,
                "shift" => Mods::SHIFT,
                "ctrl" => Mods::CTRL,
                "cmd" => Mods::META,
                "lalt" | "ralt" => {
                    self.diagnostics.push(text::one_sided(span, word, "Alt"));
                    Mods::ALT
                }
                "lshift" | "rshift" => {
                    self.diagnostics.push(text::one_sided(span, word, "Shift"));
                    Mods::SHIFT
                }
                "lctrl" | "rctrl" => {
                    self.diagnostics.push(text::one_sided(span, word, "Ctrl"));
                    Mods::CTRL
                }
                "lcmd" | "rcmd" => {
                    self.diagnostics.push(text::one_sided(span, word, "Meta"));
                    Mods::META
                }
                "hyper" => Mods::META | Mods::CTRL | Mods::ALT | Mods::SHIFT,
                "meh" => Mods::CTRL | Mods::ALT | Mods::SHIFT,
                "fn" => {
                    self.diagnostics.push(text::unsupported(
                        span,
                        "the fn modifier does not exist on a PC keyboard",
                    ));
                    realizable = false;
                    Mods::NONE
                }
                _ if word.starts_with('$') => {
                    self.diagnostics.push(text::unsupported(
                        span,
                        format!("`{word}` is an alias, and `.alias` is not supported"),
                    ));
                    realizable = false;
                    Mods::NONE
                }
                _ => {
                    return Err(text::error(span, format!("unknown modifier `{word}`")));
                }
            };
        }

        let (name, span) = key;
        let key = match self.key(name) {
            Some(key) => key,
            None if name.to_ascii_lowercase().starts_with("0x") => {
                self.diagnostics.push(text::unsupported(
                    span,
                    format!("no KDE key for the macOS keycode {name}"),
                ));
                return Ok(None);
            }
            None if name.starts_with('$') => {
                self.diagnostics.push(text::unsupported(
                    span,
                    format!("`{name}` is an alias, and `.alias` is not supported"),
                ));
                return Ok(None);
            }
            None => return Err(text::error(span, format!("no KDE key for `{name}`"))),
        };

        Ok(realizable.then(|| Combo::new(mods, key)))
    }

    fn key(&self, name: &str) -> Option<Key> {
        match self.layout {
            Layout::Us => key_to_qt(Format::Skhd, name),
        }
    }

    /// The command after a `:`, to the end of the line, with continued lines joined and a
    /// trailing comment dropped; a template is expanded.
    fn command(&mut self) -> Result<String, Diagnostic> {
        let start = self.pos;
        let raw = self.rest_of_line();
        let joined = text::join_continued(raw);
        let command = text::strip_comment(&joined).trim().to_string();
        let span = self.lines.span(start, self.pos);
        if command.is_empty() {
            return Err(text::error(span, "the command is empty"));
        }
        if command.starts_with('@') {
            return self.expand(&command, span);
        }
        Ok(command)
    }

    /// `@name("a", "b")`: the command template `name`, with its placeholders replaced.
    fn expand(&self, call: &str, span: Span) -> Result<String, Diagnostic> {
        let call = &call[1..];
        let (name, arguments) = match call.split_once('(') {
            Some((name, rest)) => {
                let Some(inside) = rest.trim_end().strip_suffix(')') else {
                    return Err(text::error(
                        span,
                        "expected `)` at the end of the arguments",
                    ));
                };
                (
                    name.trim(),
                    template_arguments(inside).ok_or_else(|| {
                        text::error(
                            span,
                            "the arguments of a template are strings in double quotes",
                        )
                    })?,
                )
            }
            None => (call.trim(), Vec::new()),
        };

        let template = self
            .keymap
            .defines
            .iter()
            .find_map(|define| match &define.value {
                DefineValue::Command(template) if define.name == name => Some(template),
                _ => None,
            });
        let Some(template) = template else {
            return Err(text::error(
                span,
                format!("there is no command template `{name}`"),
            ));
        };

        let mut command = template.clone();
        for (index, argument) in arguments.iter().enumerate() {
            command = command.replace(&format!("{{{{{}}}}}", index + 1), argument);
        }
        if let Some(at) = command.find("{{")
            && command[at + 2..].starts_with(|c: char| c.is_ascii_digit())
        {
            return Err(text::error(
                span,
                format!(
                    "`@{name}` needs more arguments than the {} it has",
                    arguments.len()
                ),
            ));
        }
        Ok(command)
    }

    /// `[ "app" : command | "app" ~ | * ... | @group ... ]`, after the `[`.
    fn application_list(&mut self) -> Result<Vec<Scope>, Diagnostic> {
        let mut scopes = Vec::new();
        loop {
            self.skip_blank();
            let apps = match self.peek() {
                None => return Err(self.error_here("the list is not closed with `]`")),
                Some(']') => {
                    self.pos += 1;
                    break;
                }
                Some('"') => vec![App::Named(self.string()?)],
                Some('*') => {
                    self.pos += 1;
                    vec![App::Other]
                }
                Some('@') => {
                    self.pos += 1;
                    let (name, span) = self
                        .word_spanned()
                        .ok_or_else(|| self.error_here("expected the name of a group after `@`"))?;
                    let group = self
                        .keymap
                        .defines
                        .iter()
                        .find_map(|define| match &define.value {
                            DefineValue::Apps(apps) if define.name == name => Some(apps),
                            _ => None,
                        });
                    match group {
                        Some(apps) => apps.iter().cloned().map(App::Named).collect(),
                        None => {
                            return Err(text::error(
                                span,
                                format!("there is no group of applications `{name}`"),
                            ));
                        }
                    }
                }
                _ => {
                    return Err(
                        self.error_here("expected an application in double quotes, `*` or `]`")
                    );
                }
            };

            self.skip_spaces();
            let then = match self.peek() {
                Some('~') => {
                    self.pos += 1;
                    Then::Ignore
                }
                Some(':') => {
                    self.pos += 1;
                    Then::Run(Run::Command(self.command()?))
                }
                _ => return Err(self.error_here("expected `:` and a command, or `~`")),
            };
            for app in apps {
                scopes.push(Scope {
                    app,
                    then: then.clone(),
                });
            }
        }
        if scopes.is_empty() {
            return Err(text::error(
                self.lines.span(self.pos - 1, self.pos),
                "the list of applications is empty",
            ));
        }
        Ok(scopes)
    }

    /// `:: name`, `:: name @`, `:: name : command`.
    fn mode_declaration(&mut self) -> Result<(), Diagnostic> {
        self.pos += 2;
        self.skip_spaces();
        let (name, name_span) = self
            .word_spanned()
            .ok_or_else(|| self.error_here("expected the name of a mode after `::`"))?;
        let name = name.to_string();
        self.skip_spaces();
        let capture = self.peek() == Some('@');
        if capture {
            self.pos += 1;
            self.skip_spaces();
        }
        let on_enter = if self.peek() == Some(':') {
            self.pos += 1;
            Some(self.command()?)
        } else {
            self.end_of_line()?;
            None
        };

        if self.declared.iter().any(|declared| declared.name == name) {
            return Err(text::error(
                name_span,
                format!("the mode `{name}` is declared twice"),
            ));
        }
        if capture {
            self.diagnostics.push(text::approximated(
                name_span,
                format!(
                    "`{name}` captures every key in skhd; here the keys it has no binding for \
                     reach the application"
                ),
            ));
        }
        self.declared.push(Declared {
            name,
            capture,
            on_enter,
        });
        Ok(())
    }

    fn directive(&mut self, start: usize) -> Result<(), Diagnostic> {
        self.pos += 1;
        let (name, name_span) = self
            .word_spanned()
            .ok_or_else(|| self.error_here("expected a directive after `.`"))?;
        self.skip_spaces();
        match name {
            "blacklist" => {
                if self.peek() != Some('[') {
                    return Err(self.error_here("expected `[` after `.blacklist`"));
                }
                self.open_list = Some(self.pos);
                self.pos += 1;
                loop {
                    self.skip_blank();
                    match self.peek() {
                        Some(']') => {
                            self.pos += 1;
                            break;
                        }
                        Some('"') => {
                            let app = self.string()?;
                            self.keymap.blacklist.push(app);
                        }
                        Some(',') => self.pos += 1,
                        _ => {
                            return Err(
                                self.error_here("expected an application in double quotes, or `]`")
                            );
                        }
                    }
                }
                self.open_list = None;
            }
            "shell" => {
                let shell = self.rest_of_line().trim().trim_matches('"').to_string();
                self.diagnostics.push(text::note(
                    self.lines.span(start, self.pos),
                    Support::Approximated,
                    format!("commands run through `sh -c`, not `{shell}`"),
                ));
            }
            "load" => {
                let file = self.rest_of_line().trim().to_string();
                self.diagnostics.push(text::unsupported(
                    self.lines.span(start, self.pos),
                    format!("`.load` is not followed: the bindings in {file} are not read"),
                ));
            }
            "define" => {
                let (define, define_span) = self
                    .word_spanned()
                    .ok_or_else(|| self.error_here("expected a name after `.define`"))?;
                let define = define.to_string();
                self.skip_spaces();
                let value = match self.peek() {
                    Some(':') => {
                        self.pos += 1;
                        let raw = self.rest_of_line();
                        let template = text::strip_comment(&text::join_continued(raw))
                            .trim()
                            .to_string();
                        DefineValue::Command(template)
                    }
                    Some('[') => {
                        self.open_list = Some(self.pos);
                        self.pos += 1;
                        let mut apps = Vec::new();
                        loop {
                            self.skip_blank();
                            match self.peek() {
                                Some(']') => {
                                    self.pos += 1;
                                    break;
                                }
                                Some(',') => self.pos += 1,
                                Some('"') => apps.push(self.string()?),
                                _ => {
                                    return Err(self.error_here(
                                        "expected an application in double quotes, or `]`",
                                    ));
                                }
                            }
                        }
                        self.open_list = None;
                        DefineValue::Apps(apps)
                    }
                    _ => {
                        return Err(self.error_here(
                            "expected `:` and a command, or a list of applications in `[ ]`",
                        ));
                    }
                };
                if self.keymap.defines.iter().any(|d| d.name == define) {
                    return Err(text::error(
                        define_span,
                        format!("`{define}` is defined twice"),
                    ));
                }
                self.keymap.defines.push(Define {
                    name: define,
                    value,
                    span: self.lines.span(start, self.pos),
                });
            }
            other => {
                let other = other.to_string();
                self.recover();
                self.diagnostics.push(text::unsupported(
                    name_span,
                    format!("the directive `.{other}` is not supported"),
                ));
            }
        }
        Ok(())
    }

    // the cursor

    fn rest(&self) -> &'a str {
        &self.src[self.pos..]
    }

    fn peek(&self) -> Option<char> {
        self.rest().chars().next()
    }

    fn span_here(&self, len: usize) -> Span {
        self.lines
            .span(self.pos, (self.pos + len).min(self.src.len()))
    }

    fn error_here(&self, msg: &str) -> Diagnostic {
        let len = self.peek().map_or(0, char::len_utf8);
        let found = match self.peek() {
            None => "the end of the file".to_string(),
            Some('\n' | '\r') => "the end of the line".to_string(),
            Some(c) => format!("`{c}`"),
        };
        text::error(self.span_here(len), format!("{msg}, found {found}"))
    }

    /// Skips spaces, tabs, line ends and comments.
    fn skip_blank(&mut self) {
        loop {
            let rest = self.rest();
            let trimmed = rest.trim_start();
            self.pos += rest.len() - trimmed.len();
            if trimmed.starts_with('#') {
                self.skip_line();
            } else {
                break;
            }
        }
    }

    /// Skips spaces and tabs, and a `\` that continues the line.
    fn skip_spaces(&mut self) {
        loop {
            let rest = self.rest();
            if rest.starts_with([' ', '\t']) {
                self.pos += 1;
            } else if rest.starts_with("\\\n") {
                self.pos += 2;
            } else if rest.starts_with("\\\r\n") {
                self.pos += 3;
            } else {
                break;
            }
        }
    }

    /// Moves to the end of the line (before the line break).
    fn skip_line(&mut self) {
        let rest = self.rest();
        self.pos += rest.find('\n').unwrap_or(rest.len());
    }

    /// The rest of the line and every line it continues on, moving past them.
    fn rest_of_line(&mut self) -> &'a str {
        let start = self.pos;
        loop {
            self.skip_line();
            let line = self.src[start..self.pos].trim_end_matches('\r');
            if line.ends_with('\\') && self.pos < self.src.len() {
                self.pos += 1;
            } else {
                break;
            }
        }
        &self.src[start..self.pos]
    }

    /// Requires nothing but a comment until the end of the line.
    fn end_of_line(&mut self) -> Result<(), Diagnostic> {
        self.skip_spaces();
        match self.peek() {
            None | Some('\n' | '\r' | '#') => {
                self.skip_line();
                Ok(())
            }
            _ => Err(self.error_here("expected the end of the line")),
        }
    }

    /// After an error: skips the rest of the line, and the rest of a list it opened.
    fn recover(&mut self) {
        self.skip_line();
        let opened = self.open_list.take().is_some()
            || (self.src[..self.pos].rsplit('\n').next())
                .is_some_and(|line| text::strip_comment(line).trim_end().ends_with('['));
        if opened {
            while self.pos < self.src.len() {
                self.pos += 1;
                let start = self.pos;
                self.skip_line();
                if self.src[start..self.pos].trim_start().starts_with(']') {
                    break;
                }
            }
        }
    }

    fn word_spanned(&mut self) -> Option<(&'a str, Span)> {
        let start = self.pos;
        let len = self
            .rest()
            .find(|c: char| !(c.is_ascii_alphanumeric() || c == '_' || c == '$'))
            .unwrap_or(self.rest().len());
        if len == 0 {
            return None;
        }
        self.pos += len;
        Some((&self.src[start..self.pos], self.lines.span(start, self.pos)))
    }

    fn word(&mut self) -> Option<&'a str> {
        self.word_spanned().map(|(word, _)| word)
    }

    /// A string in double quotes, without the quotes.
    fn string(&mut self) -> Result<String, Diagnostic> {
        let start = self.pos;
        let rest = &self.rest()[1..];
        match rest.find(['"', '\n']) {
            Some(end) if rest[end..].starts_with('"') => {
                self.pos += end + 2;
                Ok(rest[..end].to_string())
            }
            _ => Err(text::error(
                self.lines.span(start, start + 1),
                "the string is not closed with `\"`",
            )),
        }
    }
}

/// The mode a binding switches to, if it does.
fn switches_to(body: &Body) -> Option<String> {
    match body {
        Body::Run(Run::Mode(name)) => Some(name.clone()),
        _ => None,
    }
}

/// `"a", "b"`: the arguments of a template.
fn template_arguments(inside: &str) -> Option<Vec<String>> {
    let inside = inside.trim();
    if inside.is_empty() {
        return Some(Vec::new());
    }
    inside
        .split(',')
        .map(|argument| {
            let argument = argument.trim();
            argument
                .strip_prefix('"')
                .and_then(|a| a.strip_suffix('"'))
                .map(str::to_string)
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Severity;

    fn read(text: &str) -> Parsed {
        parse(text, &ParseOptions::default())
    }

    /// The keys and the command of a one-line skhdrc, or the first diagnostic.
    fn one(line: &str) -> Result<(String, String), String> {
        let parsed = read(line);
        if let Some(diagnostic) = parsed.diagnostics.first() {
            return Err(diagnostic.msg.clone());
        }
        let binding = &parsed.keymap.modes[0].bindings[0];
        let Body::Run(Run::Command(command)) = &binding.body else {
            panic!("{line}: {:?}", binding.body);
        };
        Ok((binding.chord[0].to_string(), command.clone()))
    }

    fn errors(parsed: &Parsed) -> Vec<String> {
        parsed.diagnostics.iter().map(ToString::to_string).collect()
    }

    #[test]
    fn modifiers_and_letters_carry_over() {
        assert_eq!(
            one("alt - h : komorebic focus left").unwrap(),
            ("Alt+H".into(), "komorebic focus left".into())
        );
        assert_eq!(
            one("alt + shift - return : komorebic promote").unwrap().0,
            "Alt+Shift+Return"
        );
        assert_eq!(
            one("ctrl + alt - left : komorebic stack left").unwrap().0,
            "Ctrl+Alt+Left"
        );
        assert_eq!(
            one("alt - 1 : komorebic focus-workspace 0").unwrap().0,
            "Alt+1"
        );
        // no spaces, and no modifier
        assert_eq!(one("alt-h:x").unwrap(), ("Alt+H".into(), "x".into()));
        assert_eq!(one("f1 : x").unwrap().0, "F1");
    }

    #[test]
    fn modifiers_are_written_in_the_order_qt_writes_them() {
        assert_eq!(
            one("shift + alt + ctrl + cmd - a : x").unwrap().0,
            "Meta+Ctrl+Alt+Shift+A"
        );
        assert_eq!(one("hyper - a : x").unwrap().0, "Meta+Ctrl+Alt+Shift+A");
        assert_eq!(one("meh - a : x").unwrap().0, "Ctrl+Alt+Shift+A");
    }

    #[test]
    fn macos_keycodes_become_the_key_they_are() {
        assert_eq!(one("alt + shift - 0x21 : x").unwrap().0, "Alt+{");
        assert_eq!(one("alt + shift - 0x1E : x").unwrap().0, "Alt+}");
        assert_eq!(one("alt - 0x29 : x").unwrap().0, "Alt+;");
        assert_eq!(one("alt - 0x18 : x").unwrap().0, "Alt+=");
        assert_eq!(one("alt - 0x1B : x").unwrap().0, "Alt+-");
        assert_eq!(
            one("alt - 0xFF : x").unwrap_err(),
            "no KDE key for the macOS keycode 0xFF"
        );
    }

    #[test]
    fn shift_on_a_symbol_key_is_the_symbol_it_types() {
        assert_eq!(one("alt + shift - 1 : x").unwrap().0, "Alt+!");
        assert_eq!(one("alt + shift - 0x29 : x").unwrap().0, "Alt+:");
        assert_eq!(one("alt + shift - 0x18 : x").unwrap().0, "Alt++");
        assert_eq!(one("alt + shift - 0x1B : x").unwrap().0, "Alt+_");
        assert_eq!(one("ctrl + alt + shift - 9 : x").unwrap().0, "Ctrl+Alt+(");
        // letters and named keys keep their Shift
        assert_eq!(one("alt + shift - h : x").unwrap().0, "Alt+Shift+H");
        assert_eq!(
            one("alt + shift - return : x").unwrap().0,
            "Alt+Shift+Return"
        );
        assert_eq!(one("ctrl + shift - left : x").unwrap().0, "Ctrl+Shift+Left");
    }

    #[test]
    fn a_trailing_comment_is_not_part_of_the_command() {
        assert_eq!(
            one("alt - 0x29 : komorebic unstack # 0x29 is ;").unwrap().1,
            "komorebic unstack"
        );
        assert_eq!(
            one("alt - a : echo '# not a comment'").unwrap().1,
            "echo '# not a comment'"
        );
        assert_eq!(one("alt - a : echo a#b").unwrap().1, "echo a#b");
    }

    #[test]
    fn a_continued_line_is_one_binding() {
        let parsed = read("alt - a : komorebic focus \\\n  left\n# done\n");
        assert!(parsed.diagnostics.is_empty());
        let bindings = &parsed.keymap.modes[0].bindings;
        assert_eq!(bindings.len(), 1);
        assert_eq!(
            bindings[0].body,
            Body::Run(Run::Command("komorebic focus left".into()))
        );
        assert_eq!(bindings[0].span.line, 1);
        assert_eq!(bindings[0].source, "alt - a : komorebic focus    left");
    }

    #[test]
    fn what_cannot_be_read_is_reported_and_left_out() {
        for (line, message) in [
            (
                "fn - a : x",
                "the fn modifier does not exist on a PC keyboard",
            ),
            ("alt - a ; other", "there is no mode `other` to switch to"),
            ("alt - , : x", "expected a key after `-`, found `,`"),
            (
                "hyper - a",
                "expected `:` and a command, `;` and a mode, or `[`, found the end of the file",
            ),
            ("alt - a :", "the command is empty"),
            ("super - a : x", "unknown modifier `super`"),
            ("alt - nope : x", "no KDE key for `nope`"),
            (
                "ctrl - 1 | cmd - 1",
                "key forwarding (`|`) is not supported: the binding is left out",
            ),
            (
                "cmd - q, cmd - q : x",
                "key sequences (`,`) are not supported: the binding is left out",
            ),
            (
                "$hyper - a : x",
                "`$hyper` is an alias, and `.alias` is not supported",
            ),
            (
                ".alias $hyper cmd + alt",
                "the directive `.alias` is not supported",
            ),
        ] {
            let parsed = read(line);
            assert_eq!(
                parsed
                    .keymap
                    .modes
                    .iter()
                    .map(|m| m.bindings.len())
                    .sum::<usize>(),
                0,
                "{line}"
            );
            assert_eq!(
                parsed
                    .diagnostics
                    .iter()
                    .map(|d| d.msg.as_str())
                    .collect::<Vec<_>>(),
                [message],
                "{line}"
            );
        }
    }

    #[test]
    fn reading_goes_on_after_an_error() {
        let parsed =
            read("alt - a [\n  \"x\" : one\n  oops\n]\nalt - b : two\nalt - c : three # done\n");
        let bindings = &parsed.keymap.modes[0].bindings;
        assert_eq!(
            bindings
                .iter()
                .map(|b| b.chord[0].to_string())
                .collect::<Vec<_>>(),
            ["Alt+B", "Alt+C"]
        );
        assert_eq!(
            errors(&parsed),
            ["line 3: error: expected an application in double quotes, `*` or `]`, found `o`"]
        );
    }

    #[test]
    fn diagnostics_point_at_the_text_they_are_about() {
        let text = "alt - h : x\nlalt - j : y\n";
        let parsed = read(text);
        let diagnostic = &parsed.diagnostics[0];
        assert_eq!(diagnostic.severity, Severity::Warning);
        assert_eq!(diagnostic.support, Support::Approximated);
        assert_eq!(diagnostic.span.line, 2);
        assert_eq!(&text[diagnostic.span.start..diagnostic.span.end], "lalt");
        assert_eq!(
            diagnostic.msg,
            "`lalt` is Alt: KDE does not tell the left and right Alt apart"
        );
        // the binding is kept, with Alt
        assert_eq!(
            parsed.keymap.modes[0].bindings[1].chord[0].to_string(),
            "Alt+J"
        );
        let binding = &parsed.keymap.modes[0].bindings[1];
        assert_eq!(&text[binding.span.start..binding.span.end], "lalt - j : y");
    }

    #[test]
    fn modes_are_declared_entered_and_bound() {
        let parsed = read(
            ":: resize @ : notify-send resize\n\
             alt - r ; resize\n\
             resize < h : komorebic resize-edge left increase\n\
             resize < escape ; default\n\
             default, resize < alt - q : komorebic close\n",
        );
        let keymap = &parsed.keymap;
        assert_eq!(
            keymap
                .modes
                .iter()
                .map(|m| m.name.as_str())
                .collect::<Vec<_>>(),
            ["default", "resize"]
        );
        let resize = keymap.mode("resize").unwrap();
        assert!(resize.capture);
        assert_eq!(resize.on_enter.as_deref(), Some("notify-send resize"));
        assert_eq!(resize.bindings.len(), 3);
        assert_eq!(
            resize.bindings[1].body,
            Body::Run(Run::Mode("default".into()))
        );
        assert_eq!(resize.bindings[1].chord[0].to_string(), "Esc");
        let default = keymap.mode("default").unwrap();
        assert_eq!(default.bindings.len(), 2);
        assert_eq!(
            default.bindings[0].body,
            Body::Run(Run::Mode("resize".into()))
        );
        assert_eq!(
            errors(&parsed),
            [
                "line 1: warning: `resize` captures every key in skhd; here the keys it has no \
                 binding for reach the application"
            ]
        );
    }

    #[test]
    fn a_mode_may_be_used_before_it_is_declared_but_not_without() {
        let parsed = read("alt - r ; resize\nresize < h : x\nother < h : y\n:: resize\n");
        assert_eq!(parsed.keymap.mode("resize").unwrap().bindings.len(), 1);
        assert_eq!(
            errors(&parsed),
            ["line 3: error: the mode `other` is not declared"]
        );
    }

    #[test]
    fn the_default_mode_can_have_a_command() {
        let parsed = read(":: default : echo back\nalt - h : x\n");
        assert_eq!(parsed.keymap.modes.len(), 1);
        assert_eq!(
            parsed.keymap.modes[0].on_enter.as_deref(),
            Some("echo back")
        );
        assert!(!parsed.keymap.declares_modes());
    }

    #[test]
    fn a_binding_can_do_something_else_in_each_application() {
        let parsed = read(
            "alt - n [\n\
             \x20   \"firefox\" : komorebic focus-workspace 1 # a comment\n\
             \x20   \"kitty\" ~\n\
             \x20   * : komorebic focus-workspace 0\n\
             ]\n",
        );
        assert!(parsed.diagnostics.is_empty(), "{:?}", parsed.diagnostics);
        let binding = &parsed.keymap.modes[0].bindings[0];
        assert_eq!(
            binding.body,
            Body::Scoped(vec![
                Scope {
                    app: App::Named("firefox".into()),
                    then: Then::Run(Run::Command("komorebic focus-workspace 1".into())),
                },
                Scope {
                    app: App::Named("kitty".into()),
                    then: Then::Ignore,
                },
                Scope {
                    app: App::Other,
                    then: Then::Run(Run::Command("komorebic focus-workspace 0".into())),
                },
            ])
        );
        assert_eq!(binding.span.line, 1);
        assert_eq!(
            binding.source,
            "alt - n [ \"firefox\" : komorebic focus-workspace 1 # a comment \"kitty\" ~ * : \
             komorebic focus-workspace 0 ]"
        );
    }

    #[test]
    fn defines_are_expanded_where_they_are_used() {
        let parsed = read(
            ".define focus : komorebic focus {{1}} # a template\n\
             .define two : echo {{1}} {{2}}\n\
             .define browsers [\"firefox\", \"chromium\"]\n\
             alt - h : @focus(\"left\")\n\
             alt - b [\n  @browsers : komorebic toggle-monocle\n]\n\
             alt - t : @two(\"a\")\n\
             alt - u : @nothing\n",
        );
        let bindings = &parsed.keymap.modes[0].bindings;
        assert_eq!(
            bindings[0].body,
            Body::Run(Run::Command("komorebic focus left".into()))
        );
        let Body::Scoped(scopes) = &bindings[1].body else {
            panic!("{:?}", bindings[1].body)
        };
        assert_eq!(
            scopes.iter().map(|s| s.app.clone()).collect::<Vec<_>>(),
            [App::Named("firefox".into()), App::Named("chromium".into())]
        );
        assert_eq!(bindings.len(), 2);
        assert_eq!(
            parsed.keymap.defines[0].value,
            DefineValue::Command("komorebic focus {{1}}".into())
        );
        assert_eq!(
            errors(&parsed),
            [
                "line 8: error: `@two` needs more arguments than the 1 it has",
                "line 9: error: there is no command template `nothing`",
            ]
        );
    }

    #[test]
    fn the_blacklist_and_the_other_directives() {
        let parsed = read(
            ".blacklist [\n  \"steam\"\n  \"dota2\"\n]\n\
             .shell \"/bin/dash\"\n\
             .load \"~/.config/skhd/more\"\n\
             alt - h : x\n",
        );
        assert_eq!(parsed.keymap.blacklist, ["steam", "dota2"]);
        assert_eq!(parsed.keymap.modes[0].bindings.len(), 1);
        assert_eq!(
            errors(&parsed),
            [
                "line 5: note: commands run through `sh -c`, not `/bin/dash`",
                "line 6: warning: `.load` is not followed: the bindings in \
                 \"~/.config/skhd/more\" are not read",
            ]
        );
    }

    #[test]
    fn passing_the_key_on_is_approximated() {
        let parsed = read("cmd - p -> : echo hi\n");
        assert_eq!(parsed.keymap.modes[0].bindings.len(), 1);
        assert_eq!(parsed.diagnostics[0].support, Support::Approximated);
    }
}
