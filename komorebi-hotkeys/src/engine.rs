//! The engine: the bindings komorebi realizes itself, as a state machine with no I/O.
//!
//! The engine is what makes modes, chord chains, release bindings, per-application bindings, a
//! blacklist and a pause binding work on KDE, where a shortcut is a key KDE's global shortcut
//! daemon, kglobalacceld, either grabs or does not. The engine owns a set of keys, its *active*
//! set: KDE swallows an active key and says that it was pressed, and lets any other key reach the
//! focused application. After every event the engine computes the set it wants from its state
//! (the mode, the chord being typed, the focused application, whether it is paused) and returns
//! the difference as [`Effect`]s, with the commands to run.
//!
//! The engine does no I/O and reads no clock: the caller feeds it events and the time, and carries
//! out the effects. With kglobalacceld that is:
//!
//! - [`Effect::Activate`]: `setShortcutKeys(action, [combo], 6)` for the action of the combo,
//!   after `doRegister` the first time, so that the key is grabbed;
//! - [`Effect::Deactivate`]: `setInactive(action)`, so that the key reaches the application;
//! - [`Effect::Komorebic`] and [`Effect::Shell`]: the command, `komorebic`'s in-process and any
//!   other through `sh -c`;
//! - `globalShortcutPressed` and `globalShortcutReleased` of an action become
//!   [`Engine::on_pressed`] and [`Engine::on_released`] with its combo. A key held down also sends
//!   `globalShortcutRepeated`, which the engine has no input for, so a held key runs its binding
//!   once.
//!
//! ```rust
//! use std::time::Instant;
//! use komorebi_hotkeys::{Format, ParseOptions, parse};
//! use komorebi_hotkeys::engine::{Effect, Engine, EngineOptions};
//! use komorebi_hotkeys::qt::{Combo, Key, Mods, key_to_qt};
//!
//! let text = "alt - r ; resize\n:: resize\nresize < h : komorebic resize-edge left increase\n\
//!             resize < escape ; default\n";
//! let keymap = parse(Format::Skhd, text, &ParseOptions::default()).keymap;
//! let key = |name| key_to_qt(Format::Skhd, name).unwrap();
//! let alt_r = Combo::new(Mods::ALT, key("r"));
//! let h = Combo::new(Mods::NONE, key("h"));
//! let escape = Combo::new(Mods::NONE, Key::ESCAPE);
//!
//! let mut engine = Engine::new(keymap, EngineOptions::default());
//! assert_eq!(engine.start(), [Effect::Activate(alt_r)]);
//!
//! // alt + r enters the mode: its keys are grabbed, the default mode's given back
//! let now = Instant::now();
//! assert_eq!(
//!     engine.on_pressed(alt_r, now),
//!     [Effect::Deactivate(alt_r), Effect::Activate(h), Effect::Activate(escape)],
//! );
//! assert_eq!(
//!     engine.on_pressed(h, now),
//!     [Effect::Komorebic("resize-edge left increase".into())],
//! );
//! assert_eq!(engine.on_pressed(escape, now).len(), 3);
//! assert_eq!(engine.mode(), "default");
//! ```
//!
//! # What the engine does
//!
//! - **Modes**: a [`Run::Mode`] switches the mode, runs its [`Mode::on_enter`] command, and makes
//!   the new mode's keys the active ones. A capturing mode does not capture: only its own keys are
//!   active.
//! - **Chord chains**: the first combo of a chain is an active key. When it is pressed, only the
//!   combos that can come next, and Escape, are active, until one of them is pressed or the chord
//!   timeout ([`EngineOptions::chord_timeout`]) passes, which [`Engine::on_tick`] checks. Escape,
//!   unless a chain is bound to it, abandons the chain. A key that is not the next of the chain is
//!   not active, so it reaches the application and does not end the chain.
//! - **Per-application bindings**: a scoped binding's key is active if and only if a [`Scope`]
//!   resolves for the focused application ([`Engine::on_focus`]) to an action. A [`Then::Ignore`],
//!   or an application no scope names when there is no [`App::Other`] scope, leaves the key
//!   inactive, so that it reaches the application.
//! - **The blacklist**: while a blacklisted application has the focus, no key is active but the
//!   pause binding's.
//! - **The pause binding**: pressing it turns every other key off, or on again, and runs its hook.
//! - **Release bindings**: pressing the key does nothing; [`Engine::on_released`] runs the action.
//!
//! An application is matched by name, without regard to case, against the executable, the
//! `app_id` and the desktop entry of the focused window ([`Focus`]), and against the aliases
//! [`EngineOptions::app_aliases`] gives the name.
//!
//! [`App::Other`]: crate::App::Other
//! [`Mode::on_enter`]: crate::Mode::on_enter
//! [`Run::Mode`]: crate::Run::Mode
//! [`Scope`]: crate::Scope
//! [`Then::Ignore`]: crate::Then::Ignore

use crate::App;
use crate::Binding;
use crate::Body;
use crate::DEFAULT_MODE;
use crate::Edge;
use crate::Keymap;
use crate::Mode;
use crate::Run;
use crate::Then;
use crate::qt::Combo;
use crate::qt::Key;
use crate::qt::Mods;
use std::collections::BTreeMap;
use std::collections::BTreeSet;
use std::time::Duration;
use std::time::Instant;

/// Options for an [`Engine`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EngineOptions {
    /// How long the engine waits for the next combo of a chord chain before it abandons the chain.
    ///
    /// The default for this is 3 seconds.
    pub chord_timeout: Duration,
    /// More names for an application, by the name a file gives it: `"firefox"` might also be
    /// `["org.mozilla.firefox", "librewolf"]`. The names are matched without regard to case.
    ///
    /// The default for this is no aliases.
    pub app_aliases: BTreeMap<String, Vec<String>>,
}

impl Default for EngineOptions {
    fn default() -> EngineOptions {
        EngineOptions {
            chord_timeout: Duration::from_millis(3000),
            app_aliases: BTreeMap::new(),
        }
    }
}

/// The window that has the focus, as the names an application can be matched by.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Focus {
    /// The file name of the executable (`firefox`).
    pub exe: String,
    /// The Wayland `app_id`, or the X11 class (`org.mozilla.firefox`).
    pub app_id: String,
    /// The name of the desktop entry (`firefox`).
    pub desktop_entry: String,
}

/// What the caller of an [`Engine`] does after an event.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Effect {
    /// Grab the combo: its presses come to the engine, and the application does not get it.
    Activate(Combo),
    /// Give the combo back: it reaches the focused application.
    Deactivate(Combo),
    /// Run `komorebic` with these arguments, as written after `komorebic `.
    Komorebic(String),
    /// Run this command through `sh -c`.
    Shell(String),
}

/// A chord chain being typed: the combos so far, and when the last was pressed.
#[derive(Clone, Debug)]
struct Pending {
    prefix: Vec<Combo>,
    since: Instant,
}

/// The bindings of a keymap, as a state machine.
///
/// An engine is made from the keymap of a [`Plan`](crate::plan::Plan)'s engine and its options,
/// then [`started`](Engine::start); each event returns the [`Effect`]s to carry out, in order:
/// the commands to run first, then the combos to deactivate, then the combos to activate. It is
/// guaranteed that after the effects are carried out, the active combos are exactly the ones the
/// engine wants for its state, and that a combo is activated only when it is not active, and
/// deactivated only when it is.
///
/// The [module documentation](self) describes what the engine does.
#[derive(Debug)]
pub struct Engine {
    keymap: Keymap,
    options: EngineOptions,
    mode: usize,
    pending: Option<Pending>,
    focus: Option<Focus>,
    paused: bool,
    active: BTreeSet<Combo>,
    /// Release bindings whose key is down, and what each runs when it is released.
    held: Vec<(Combo, Run)>,
}

impl Engine {
    /// Returns an engine in the default mode, with no active combo and no focused application.
    pub fn new(keymap: Keymap, options: EngineOptions) -> Engine {
        let mode = keymap
            .modes
            .iter()
            .position(|mode| mode.name == DEFAULT_MODE)
            .unwrap_or_default();
        Engine {
            keymap,
            options,
            mode,
            pending: None,
            focus: None,
            paused: false,
            active: BTreeSet::new(),
            held: Vec::new(),
        }
    }

    /// Returns the effects that activate the combos of the default mode.
    pub fn start(&mut self) -> Vec<Effect> {
        let mut effects = Vec::new();
        self.reconcile(&mut effects);
        effects
    }

    /// Returns the effects that deactivate every active combo, for when the engine stops: a combo
    /// left active stays grabbed by KDE, and dead, after its process is gone.
    pub fn stop(&mut self) -> Vec<Effect> {
        let effects = self
            .active
            .iter()
            .map(|&combo| Effect::Deactivate(combo))
            .collect();
        self.active.clear();
        effects
    }

    /// Returns the name of the current mode.
    pub fn mode(&self) -> &str {
        self.keymap
            .modes
            .get(self.mode)
            .map_or(DEFAULT_MODE, |mode| mode.name.as_str())
    }

    /// Returns the combos that are active, in order.
    pub fn active(&self) -> impl Iterator<Item = Combo> + '_ {
        self.active.iter().copied()
    }

    /// Handles the press of an active combo, at `now`.
    ///
    /// A combo that is not active (KDE should not report one) does nothing but put the active set
    /// right.
    pub fn on_pressed(&mut self, combo: Combo, now: Instant) -> Vec<Effect> {
        let mut effects = Vec::new();

        if let Some(pause) = &self.keymap.pause
            && pause.combo == combo
        {
            self.paused = !self.paused;
            self.pending = None;
            if let Some(hook) = pause.hook.clone() {
                effects.push(command(&hook));
            }
            self.reconcile(&mut effects);
            return effects;
        }
        if !self.active.contains(&combo) {
            self.reconcile(&mut effects);
            return effects;
        }

        let prefix = self.pending.take().map(|p| p.prefix).unwrap_or_default();
        let at = prefix.len();
        let candidates: Vec<(usize, Run, Edge)> = self
            .current()
            .bindings
            .iter()
            .filter(|b| b.chord.len() > at && b.chord[..at] == prefix[..] && b.chord[at] == combo)
            .filter_map(|b| Some((b.chord.len(), self.resolve(b)?.clone(), b.edge)))
            .collect();

        if let Some((_, run, edge)) = candidates.iter().find(|(len, _, _)| *len == at + 1) {
            match edge {
                Edge::Press => self.run(run.clone(), &mut effects),
                Edge::Release => self.held.push((combo, run.clone())),
            }
        } else if !candidates.is_empty() {
            let mut prefix = prefix;
            prefix.push(combo);
            self.pending = Some(Pending { prefix, since: now });
        }
        // anything else, Escape included, abandons the chain
        self.reconcile(&mut effects);
        effects
    }

    /// Handles the release of an active combo: runs the release binding whose key it is, if its
    /// press was handled.
    pub fn on_released(&mut self, combo: Combo) -> Vec<Effect> {
        let mut effects = Vec::new();
        if let Some(index) = self.held.iter().position(|(held, _)| *held == combo) {
            let (_, run) = self.held.remove(index);
            self.run(run, &mut effects);
            self.reconcile(&mut effects);
        }
        effects
    }

    /// Handles a change of the focused window; `None` is no window.
    pub fn on_focus(&mut self, focus: Option<Focus>) -> Vec<Effect> {
        self.focus = focus;
        let mut effects = Vec::new();
        self.reconcile(&mut effects);
        effects
    }

    /// Handles the passing of time: abandons a chord chain whose last combo was pressed
    /// [`EngineOptions::chord_timeout`] or longer before `now`.
    pub fn on_tick(&mut self, now: Instant) -> Vec<Effect> {
        let mut effects = Vec::new();
        if let Some(pending) = &self.pending
            && now.saturating_duration_since(pending.since) >= self.options.chord_timeout
        {
            self.pending = None;
            self.reconcile(&mut effects);
        }
        effects
    }

    fn current(&self) -> &Mode {
        &self.keymap.modes[self.mode]
    }

    /// Runs an action: a command, or a switch to a mode.
    fn run(&mut self, run: Run, effects: &mut Vec<Effect>) {
        match run {
            Run::Command(text) => effects.push(command(&text)),
            Run::Mode(name) => {
                if let Some(index) = self.keymap.modes.iter().position(|m| m.name == name) {
                    self.mode = index;
                    self.pending = None;
                    if let Some(on_enter) = &self.keymap.modes[index].on_enter {
                        effects.push(command(on_enter));
                    }
                }
            }
        }
    }

    /// What a binding does for the focused application, if anything.
    fn resolve<'a>(&self, binding: &'a Binding) -> Option<&'a Run> {
        match &binding.body {
            Body::Run(run) => Some(run),
            Body::Scoped(scopes) => {
                let named = scopes.iter().find(|scope| match &scope.app {
                    App::Named(name) => self.focused(name),
                    App::Other => false,
                });
                let scope = named.or_else(|| scopes.iter().find(|s| s.app == App::Other))?;
                match &scope.then {
                    Then::Run(run) => Some(run),
                    Then::Ignore => None,
                }
            }
        }
    }

    /// Whether the focused window is the application with this name.
    fn focused(&self, name: &str) -> bool {
        let Some(focus) = &self.focus else {
            return false;
        };
        let aliases = self
            .options
            .app_aliases
            .iter()
            .filter(|(key, _)| key.eq_ignore_ascii_case(name))
            .flat_map(|(_, aliases)| aliases.iter().map(String::as_str));
        std::iter::once(name).chain(aliases).any(|name| {
            [&focus.exe, &focus.app_id, &focus.desktop_entry]
                .iter()
                .any(|candidate| !candidate.is_empty() && candidate.eq_ignore_ascii_case(name))
        })
    }

    /// The combos the engine wants active in its state.
    fn wanted(&self) -> BTreeSet<Combo> {
        let mut wanted = BTreeSet::new();
        if let Some(pause) = &self.keymap.pause {
            wanted.insert(pause.combo);
        }
        let blacklisted = self.keymap.blacklist.iter().any(|app| self.focused(app));
        if self.paused || blacklisted {
            return wanted;
        }

        let prefix = self
            .pending
            .as_ref()
            .map(|p| p.prefix.as_slice())
            .unwrap_or_default();
        let at = prefix.len();
        for binding in &self.current().bindings {
            if binding.chord.len() > at
                && binding.chord[..at] == *prefix
                && self.resolve(binding).is_some()
            {
                wanted.insert(binding.chord[at]);
            }
        }
        if at > 0 {
            wanted.insert(Combo::new(Mods::NONE, Key::ESCAPE));
        }
        wanted.extend(self.held.iter().map(|(combo, _)| *combo));
        wanted
    }

    /// Makes the active set the wanted one, and says how.
    fn reconcile(&mut self, effects: &mut Vec<Effect>) {
        let wanted = self.wanted();
        effects.extend(
            self.active
                .difference(&wanted)
                .map(|&c| Effect::Deactivate(c)),
        );
        effects.extend(
            wanted
                .difference(&self.active)
                .map(|&c| Effect::Activate(c)),
        );
        self.active = wanted;
    }
}

/// A command as an effect: `komorebic`'s in-process, anything else through a shell.
fn command(text: &str) -> Effect {
    match text.strip_prefix("komorebic ") {
        Some(arguments) => Effect::Komorebic(arguments.trim().to_string()),
        None => Effect::Shell(text.to_string()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Format;
    use crate::ParseOptions;
    use crate::parse;
    use crate::qt::key_to_qt;

    fn engine(format: Format, text: &str) -> Engine {
        let parsed = parse(format, text, &ParseOptions::default());
        assert!(
            parsed
                .diagnostics
                .iter()
                .all(|d| d.severity != crate::Severity::Error),
            "{:?}",
            parsed.diagnostics
        );
        let mut engine = Engine::new(parsed.keymap, EngineOptions::default());
        engine.start();
        engine
    }

    fn combo(mods: Mods, name: &str) -> Combo {
        Combo::new(mods, key_to_qt(Format::Skhd, name).unwrap())
    }

    fn active(engine: &Engine) -> Vec<String> {
        engine.active().map(|c| c.to_string()).collect()
    }

    fn focus(name: &str) -> Option<Focus> {
        Some(Focus {
            exe: name.to_string(),
            ..Focus::default()
        })
    }

    #[test]
    fn start_activates_the_default_mode_and_stop_gives_everything_back() {
        let mut engine = Engine::new(
            parse(
                Format::Skhd,
                "alt - h : komorebic focus left\nalt - l : echo right\n",
                &ParseOptions::default(),
            )
            .keymap,
            EngineOptions::default(),
        );
        assert_eq!(
            engine.start(),
            [
                Effect::Activate(combo(Mods::ALT, "h")),
                Effect::Activate(combo(Mods::ALT, "l"))
            ]
        );
        assert!(engine.start().is_empty(), "nothing changed");
        assert_eq!(engine.stop().len(), 2);
        assert_eq!(engine.active().count(), 0);
    }

    #[test]
    fn a_press_runs_the_command() {
        let mut engine = engine(
            Format::Skhd,
            "alt - h : komorebic focus left\nalt - l : echo right\n",
        );
        let now = Instant::now();
        assert_eq!(
            engine.on_pressed(combo(Mods::ALT, "h"), now),
            [Effect::Komorebic("focus left".into())]
        );
        assert_eq!(
            engine.on_pressed(combo(Mods::ALT, "l"), now),
            [Effect::Shell("echo right".into())]
        );
        // the release of a press binding does nothing
        assert!(engine.on_released(combo(Mods::ALT, "h")).is_empty());
    }

    #[test]
    fn a_mode_has_its_own_keys() {
        let mut engine = engine(
            Format::Skhd,
            ":: resize @ : notify-send resize\nalt - r ; resize\nresize < h : komorebic resize-edge left increase\n\
             resize < escape ; default\ndefault, resize < alt - q : komorebic close\n",
        );
        let now = Instant::now();
        assert_eq!(active(&engine), ["Alt+Q", "Alt+R"]);
        let effects = engine.on_pressed(combo(Mods::ALT, "r"), now);
        assert_eq!(effects[0], Effect::Shell("notify-send resize".into()));
        assert_eq!(engine.mode(), "resize");
        assert_eq!(active(&engine), ["H", "Esc", "Alt+Q"]);
        assert_eq!(
            engine.on_pressed(combo(Mods::NONE, "h"), now),
            [Effect::Komorebic("resize-edge left increase".into())]
        );
        engine.on_pressed(Combo::new(Mods::NONE, Key::ESCAPE), now);
        assert_eq!(engine.mode(), "default");
        assert_eq!(active(&engine), ["Alt+Q", "Alt+R"]);
    }

    #[test]
    fn a_chain_activates_its_next_keys_until_it_ends() {
        let mut engine = engine(
            Format::Sxhkd,
            "super + w ; {h,l}\n    komorebic move {left,right}\nsuper + q\n    komorebic close\n",
        );
        let super_w = combo(Mods::META, "w");
        let h = combo(Mods::NONE, "h");
        let escape = Combo::new(Mods::NONE, Key::ESCAPE);
        let start = Instant::now();
        assert_eq!(active(&engine), ["Meta+Q", "Meta+W"]);

        // the prefix: only the next keys and Escape are active
        let effects = engine.on_pressed(super_w, start);
        assert!(
            !effects
                .iter()
                .any(|e| matches!(e, Effect::Komorebic(_) | Effect::Shell(_)))
        );
        assert_eq!(active(&engine), ["H", "L", "Esc"]);
        // the second key runs the binding and puts the default keys back
        assert_eq!(
            engine.on_pressed(h, start),
            [
                Effect::Komorebic("move left".into()),
                Effect::Deactivate(h),
                Effect::Deactivate(combo(Mods::NONE, "l")),
                Effect::Deactivate(escape),
                Effect::Activate(combo(Mods::META, "q")),
                Effect::Activate(super_w),
            ]
        );

        // Escape abandons the chain
        engine.on_pressed(super_w, start);
        assert!(
            engine
                .on_pressed(escape, start)
                .iter()
                .all(|e| matches!(e, Effect::Activate(_) | Effect::Deactivate(_)))
        );
        assert_eq!(active(&engine), ["Meta+Q", "Meta+W"]);

        // and so does the timeout, but not before
        engine.on_pressed(super_w, start);
        assert!(
            engine
                .on_tick(start + Duration::from_millis(2999))
                .is_empty()
        );
        assert!(
            !engine
                .on_tick(start + Duration::from_millis(3000))
                .is_empty()
        );
        assert_eq!(active(&engine), ["Meta+Q", "Meta+W"]);
    }

    #[test]
    fn a_chain_that_stays_open_is_left_with_escape() {
        let mut engine = engine(
            Format::Sxhkd,
            "alt + r : {h,l}\n    komorebic resize-edge {left,right} increase\n",
        );
        let now = Instant::now();
        engine.on_pressed(combo(Mods::ALT, "r"), now);
        assert_eq!(engine.mode(), "Alt+R");
        for _ in 0..3 {
            assert_eq!(
                engine.on_pressed(combo(Mods::NONE, "l"), now),
                [Effect::Komorebic("resize-edge right increase".into())]
            );
        }
        engine.on_pressed(Combo::new(Mods::NONE, Key::ESCAPE), now);
        assert_eq!(engine.mode(), "default");
    }

    #[test]
    fn a_scoped_key_is_active_only_where_it_does_something() {
        let mut engine = engine(
            Format::Whkd,
            "alt + n [\n    Default : komorebic focus-workspace 0\n    Firefox : komorebic focus-workspace 1\n    kitty : Ignore\n]\n\
             alt + m [\n    firefox : komorebic toggle-monocle\n]\n",
        );
        let alt_n = combo(Mods::ALT, "n");
        let alt_m = combo(Mods::ALT, "m");
        let now = Instant::now();

        // no window: only the Default scope applies
        assert_eq!(active(&engine), ["Alt+N"]);
        assert_eq!(
            engine.on_pressed(alt_n, now),
            [Effect::Komorebic("focus-workspace 0".into())]
        );

        // the application's own scope wins over Default, wherever Default is listed
        assert_eq!(engine.on_focus(focus("firefox")), [Effect::Activate(alt_m)]);
        assert_eq!(
            engine.on_pressed(alt_n, now),
            [Effect::Komorebic("focus-workspace 1".into())]
        );

        // Ignore: the key reaches kitty
        assert_eq!(
            engine.on_focus(focus("kitty")),
            [Effect::Deactivate(alt_m), Effect::Deactivate(alt_n)]
        );
        assert_eq!(active(&engine), Vec::<String>::new());
    }

    #[test]
    fn applications_are_matched_by_any_name_without_regard_to_case_and_by_alias() {
        let parsed = parse(
            Format::Skhd,
            "alt - m [\n  \"Firefox\" : komorebic toggle-monocle\n]\n",
            &ParseOptions::default(),
        );
        let options = EngineOptions {
            app_aliases: BTreeMap::from([("firefox".to_string(), vec!["librewolf".to_string()])]),
            ..EngineOptions::default()
        };
        let mut engine = Engine::new(parsed.keymap, options);
        engine.start();
        for focus in [
            Focus {
                exe: "firefox".into(),
                ..Focus::default()
            },
            Focus {
                app_id: "FIREFOX".into(),
                ..Focus::default()
            },
            Focus {
                desktop_entry: "firefox".into(),
                ..Focus::default()
            },
            Focus {
                exe: "librewolf".into(),
                ..Focus::default()
            },
        ] {
            engine.on_focus(Some(focus.clone()));
            assert_eq!(active(&engine), ["Alt+M"], "{focus:?}");
            engine.on_focus(None);
            assert_eq!(engine.active().count(), 0);
        }
        engine.on_focus(Some(Focus {
            exe: "chromium".into(),
            ..Focus::default()
        }));
        assert_eq!(engine.active().count(), 0);
    }

    #[test]
    fn the_blacklist_turns_every_key_off() {
        let mut engine = engine(
            Format::Skhd,
            ".blacklist [ \"steam\" ]\nalt - h : komorebic focus left\n",
        );
        assert_eq!(active(&engine), ["Alt+H"]);
        assert_eq!(
            engine.on_focus(focus("Steam")),
            [Effect::Deactivate(combo(Mods::ALT, "h"))]
        );
        assert_eq!(
            engine.on_focus(focus("konsole")),
            [Effect::Activate(combo(Mods::ALT, "h"))]
        );
    }

    #[test]
    fn the_pause_binding_turns_every_other_key_off_and_on() {
        let mut engine = engine(
            Format::Whkd,
            ".pause alt + shift + p\n.pause_hook komorebic toggle-pause\nalt + h : komorebic focus left\n",
        );
        let pause = combo(Mods::ALT | Mods::SHIFT, "p");
        let alt_h = combo(Mods::ALT, "h");
        let now = Instant::now();
        assert_eq!(active(&engine), ["Alt+H", "Alt+Shift+P"]);
        assert_eq!(
            engine.on_pressed(pause, now),
            [
                Effect::Komorebic("toggle-pause".into()),
                Effect::Deactivate(alt_h)
            ]
        );
        assert_eq!(
            engine.on_pressed(pause, now),
            [
                Effect::Komorebic("toggle-pause".into()),
                Effect::Activate(alt_h)
            ]
        );
    }

    #[test]
    fn a_release_binding_runs_when_the_key_is_released() {
        let mut engine = engine(
            Format::Sxhkd,
            "super + @space\n    komorebic toggle-float\n",
        );
        let key = combo(Mods::META, "space");
        assert!(engine.on_pressed(key, Instant::now()).is_empty());
        assert_eq!(
            engine.on_released(key),
            [Effect::Komorebic("toggle-float".into())]
        );
        assert!(engine.on_released(key).is_empty());
    }

    #[test]
    fn a_press_of_an_inactive_key_does_nothing() {
        let mut engine = engine(Format::Skhd, "alt - h : komorebic focus left\n");
        assert!(
            engine
                .on_pressed(combo(Mods::ALT, "j"), Instant::now())
                .is_empty()
        );
    }
}
