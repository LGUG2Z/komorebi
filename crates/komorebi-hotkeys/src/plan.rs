//! Planning: which bindings become KDE shortcut files, and which komorebi handles itself.
//!
//! A binding can be realized in two ways:
//!
//! - As a **static shortcut**: a `.desktop` file ([`kde`](crate::kde)) that KDE runs the command
//!   of. It is visible and can be changed in System Settings, and it works with no komorebi process
//!   running. It can only be one combo, pressed, that runs a command in every application.
//! - Through the **engine**: komorebi registers the keys with KDE itself and decides what a key
//!   does when it is pressed, so it can realize modes, chord chains, release bindings,
//!   per-application bindings, a blacklist and a pause binding. It is only as robust as the
//!   komorebi process.
//!
//! [`plan`] sorts the bindings of a keymap into the two, by [`PlanMode`]. With
//! [`PlanMode::Auto`], a binding is a static shortcut when it is in the default mode, a single
//! combo, pressed and the same in every application, and the file declares no modes, no blacklist
//! and no pause binding; everything else goes through the engine.
//!
//! ```rust
//! use komorebi_hotkeys::{Format, ParseOptions, parse};
//! use komorebi_hotkeys::plan::{PlanMode, PlanOptions, plan};
//!
//! let parsed = parse(
//!     Format::Skhd,
//!     "alt - h : komorebic focus left\nalt - n [\n  \"firefox\" : komorebic retile\n]\n",
//!     &ParseOptions::default(),
//! );
//! let plan = plan(&parsed.keymap, &PlanOptions { mode: PlanMode::Auto });
//!
//! assert_eq!(plan.static_files.len(), 1);
//! assert_eq!(plan.static_files[0].combo.to_string(), "Alt+H");
//! assert_eq!(plan.engine.modes[0].bindings.len(), 1);
//! assert_eq!(
//!     plan.diagnostics[0].to_string(),
//!     "line 2: note: Alt+N goes through the engine: it does something else in each application",
//! );
//! ```

use crate::Binding;
use crate::Body;
use crate::DEFAULT_MODE;
use crate::Diagnostic;
use crate::Edge;
use crate::Keymap;
use crate::Mode;
use crate::Run;
use crate::Support;
use crate::kde::Shortcut;
use crate::text;

/// How [`plan`] realizes the bindings.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum PlanMode {
    /// Every binding that can be a static shortcut is one, and the engine handles the rest.
    Auto,
    /// The engine handles every binding.
    ///
    /// This is the default. On KWin 6.8.1 a static shortcut ran its command 165 to 436 ms after
    /// the key (a median of 233 ms over 8 presses, in a virtual machine), while the engine was
    /// told of the key 3 ms after it (measured on 2026-10-27).
    #[default]
    Engine,
    /// Every binding that can be a static shortcut is one, and the rest are left out, with a
    /// warning each. No komorebi process needs to run for the keys to work.
    Static,
}

/// Options for [`plan`].
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct PlanOptions {
    /// How the bindings are realized.
    ///
    /// The default for this is [`PlanMode::Engine`].
    pub mode: PlanMode,
}

/// The bindings of a keymap, sorted into static shortcuts and the engine's.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Plan {
    /// The bindings that become `.desktop` files, which [`kde::desktop_files`] writes.
    ///
    /// [`kde::desktop_files`]: crate::kde::desktop_files
    pub static_files: Vec<Shortcut>,
    /// The keymap the engine realizes: the modes of the original, each with the bindings that go
    /// through the engine, and its blacklist and pause binding, unless the plan is
    /// [`PlanMode::Static`].
    pub engine: Keymap,
    /// Why a binding goes where it goes, or is left out.
    pub diagnostics: Vec<Diagnostic>,
}

impl Plan {
    /// Returns true if and only if the engine has a binding or a pause binding to realize.
    ///
    /// # Example
    ///
    /// ```rust
    /// use komorebi_hotkeys::{Format, ParseOptions, parse};
    /// use komorebi_hotkeys::plan::{PlanMode, PlanOptions, plan};
    ///
    /// let parsed = parse(Format::Skhd, "alt - h : komorebic focus left\n", &ParseOptions::default());
    /// assert!(!plan(&parsed.keymap, &PlanOptions { mode: PlanMode::Auto }).needs_engine());
    /// assert!(plan(&parsed.keymap, &PlanOptions::default()).needs_engine());
    /// ```
    pub fn needs_engine(&self) -> bool {
        self.engine.pause.is_some() || self.engine.modes.iter().any(|m| !m.bindings.is_empty())
    }
}

/// Sorts the bindings of a keymap into static shortcuts and the engine's.
///
/// The [module documentation](self) says which binding goes where. Bindings keep their order. The
/// diagnostics are notes that say why a binding goes through the engine under
/// [`PlanMode::Auto`], and warnings for what [`PlanMode::Static`] leaves out.
///
/// # Example
///
/// ```rust
/// use komorebi_hotkeys::{Format, ParseOptions, Severity, Support, parse};
/// use komorebi_hotkeys::plan::{PlanMode, PlanOptions, plan};
///
/// let parsed = parse(
///     Format::Sxhkd,
///     "super + h\n    komorebic focus left\nsuper + w ; h\n    komorebic move left\n",
///     &ParseOptions::default(),
/// );
/// let plan = plan(&parsed.keymap, &PlanOptions { mode: PlanMode::Static });
/// assert_eq!(plan.static_files.len(), 1);
/// assert!(!plan.needs_engine());
/// // the chain needs the engine, so a static plan leaves it out
/// assert_eq!(plan.diagnostics[0].severity, Severity::Warning);
/// assert_eq!(plan.diagnostics[0].support, Support::Unsupported);
/// ```
pub fn plan(keymap: &Keymap, options: &PlanOptions) -> Plan {
    let mut engine = Keymap {
        format: keymap.format,
        modes: keymap
            .modes
            .iter()
            .map(|mode| Mode {
                bindings: Vec::new(),
                ..mode.clone()
            })
            .collect(),
        blacklist: keymap.blacklist.clone(),
        pause: keymap.pause.clone(),
        defines: keymap.defines.clone(),
    };
    let mut static_files = Vec::new();
    let mut diagnostics = Vec::new();

    let first_span = keymap
        .modes
        .iter()
        .flat_map(|mode| &mode.bindings)
        .map(|binding| binding.span)
        .next()
        .unwrap_or_default();
    let whole_file = if keymap.declares_modes() {
        Some("the file declares modes")
    } else if !keymap.blacklist.is_empty() {
        Some("the file has a blacklist")
    } else if keymap.pause.is_some() {
        Some("the file has a pause binding")
    } else {
        None
    };

    match options.mode {
        PlanMode::Engine => {
            for (index, mode) in keymap.modes.iter().enumerate() {
                engine.modes[index].bindings = mode.bindings.clone();
            }
        }
        PlanMode::Auto => {
            if let Some(reason) = whole_file {
                diagnostics.push(text::note(
                    first_span,
                    Support::Exact,
                    format!("every binding goes through the engine: {reason}"),
                ));
            }
            for (index, mode) in keymap.modes.iter().enumerate() {
                for binding in &mode.bindings {
                    match (whole_file, needs_engine(&mode.name, binding)) {
                        (None, None) => static_files.push(shortcut(keymap, binding)),
                        (Some(_), _) => engine.modes[index].bindings.push(binding.clone()),
                        (None, Some(reason)) => {
                            diagnostics.push(text::note(
                                binding.span,
                                Support::Exact,
                                format!("{} goes through the engine: {reason}", chord(binding)),
                            ));
                            engine.modes[index].bindings.push(binding.clone());
                        }
                    }
                }
            }
        }
        PlanMode::Static => {
            if !keymap.blacklist.is_empty() {
                diagnostics.push(text::approximated(
                    first_span,
                    "the blacklist is not applied: a static shortcut works in every application",
                ));
            }
            if let Some(pause) = &keymap.pause {
                diagnostics.push(text::unsupported(
                    pause.span,
                    "the pause binding is left out: it needs the engine",
                ));
            }
            for mode in &keymap.modes {
                for binding in &mode.bindings {
                    match needs_engine(&mode.name, binding) {
                        None => static_files.push(shortcut(keymap, binding)),
                        Some(reason) => diagnostics.push(text::unsupported(
                            binding.span,
                            format!(
                                "{} is left out: {reason}, which needs the engine",
                                chord(binding)
                            ),
                        )),
                    }
                }
            }
            engine = Keymap::new(keymap.format);
        }
    }

    diagnostics.sort_by_key(|d| d.span.start);
    Plan {
        static_files,
        engine,
        diagnostics,
    }
}

/// Why a binding cannot be a static shortcut, if it cannot.
fn needs_engine(mode: &str, binding: &Binding) -> Option<String> {
    if mode != DEFAULT_MODE {
        return Some(format!("it is in the mode `{mode}`"));
    }
    if binding.chord.len() > 1 {
        return Some("it is a chord chain".to_string());
    }
    if binding.edge == Edge::Release {
        return Some("it fires when the key is released".to_string());
    }
    match &binding.body {
        Body::Scoped(_) => Some("it does something else in each application".to_string()),
        Body::Run(Run::Mode(_)) => Some("it switches modes".to_string()),
        Body::Run(Run::Command(_)) if binding.chord[0].key().code() == u32::from(',') => {
            Some("a comma cannot be written in `X-KDE-Shortcuts`".to_string())
        }
        Body::Run(Run::Command(_)) => None,
    }
}

fn shortcut(keymap: &Keymap, binding: &Binding) -> Shortcut {
    let command = match &binding.body {
        Body::Run(Run::Command(command)) => command.clone(),
        _ => String::new(),
    };
    Shortcut {
        combo: binding.chord[0],
        command,
        comment: format!("from the {}: {}", keymap.format.file_name(), binding.source),
    }
}

/// The keys of a binding, as the diagnostics write them.
fn chord(binding: &Binding) -> String {
    binding
        .chord
        .iter()
        .map(ToString::to_string)
        .collect::<Vec<_>>()
        .join(", ")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Format;
    use crate::ParseOptions;
    use crate::Severity;
    use crate::parse;

    fn planned(format: Format, text: &str, mode: PlanMode) -> Plan {
        let parsed = parse(format, text, &ParseOptions::default());
        plan(&parsed.keymap, &PlanOptions { mode })
    }

    fn keys(plan: &Plan) -> (Vec<String>, Vec<String>) {
        let files = plan
            .static_files
            .iter()
            .map(|s| s.combo.to_string())
            .collect();
        let engine = plan
            .engine
            .modes
            .iter()
            .flat_map(|mode| &mode.bindings)
            .map(chord)
            .collect();
        (files, engine)
    }

    fn messages(plan: &Plan) -> Vec<String> {
        plan.diagnostics.iter().map(ToString::to_string).collect()
    }

    #[test]
    fn a_plain_binding_is_static_and_the_rest_goes_through_the_engine() {
        let plan = planned(
            Format::Sxhkd,
            "super + h\n    a\nsuper + w ; h\n    b\nsuper + @x\n    c\nsuper + comma\n    d\n",
            PlanMode::Auto,
        );
        assert_eq!(
            keys(&plan),
            (
                vec!["Meta+H".to_string()],
                vec![
                    "Meta+W, H".to_string(),
                    "Meta+X".to_string(),
                    "Meta+,".to_string()
                ]
            )
        );
        assert_eq!(
            messages(&plan),
            [
                "line 3: note: Meta+W, H goes through the engine: it is a chord chain",
                "line 5: note: Meta+X goes through the engine: it fires when the key is released",
                "line 7: note: Meta+, goes through the engine: a comma cannot be written in \
                 `X-KDE-Shortcuts`",
            ]
        );
        assert!(
            plan.diagnostics
                .iter()
                .all(|d| d.severity == Severity::Note)
        );
    }

    #[test]
    fn modes_a_blacklist_or_a_pause_binding_send_every_binding_through_the_engine() {
        for (format, text, reason) in [
            (
                Format::Skhd,
                ":: resize\nalt - h : a\nalt - r ; resize\nresize < h : b\n",
                "the file declares modes",
            ),
            (
                Format::Skhd,
                ".blacklist [ \"steam\" ]\nalt - h : a\n",
                "the file has a blacklist",
            ),
            (
                Format::Whkd,
                ".pause alt + p\nalt + h : a\n",
                "the file has a pause binding",
            ),
        ] {
            let plan = planned(format, text, PlanMode::Auto);
            assert!(plan.static_files.is_empty(), "{text}");
            assert!(plan.needs_engine(), "{text}");
            assert_eq!(
                plan.diagnostics
                    .iter()
                    .map(|d| d.msg.clone())
                    .collect::<Vec<_>>(),
                [format!("every binding goes through the engine: {reason}")],
                "{text}"
            );
        }
    }

    #[test]
    fn the_engine_plan_takes_everything() {
        let plan = planned(
            Format::Skhd,
            ":: resize\nalt - h : a\nalt - r ; resize\nresize < h : b\n",
            PlanMode::Engine,
        );
        assert!(plan.static_files.is_empty());
        assert!(plan.diagnostics.is_empty());
        assert_eq!(plan.engine.modes.len(), 2);
        assert_eq!(plan.engine.modes[0].bindings.len(), 2);
        assert_eq!(plan.engine.modes[1].bindings.len(), 1);
        assert_eq!(plan.engine.modes[1].name, "resize");
    }

    #[test]
    fn the_static_plan_leaves_out_what_needs_the_engine() {
        let plan = planned(
            Format::Skhd,
            ".blacklist [ \"steam\" ]\n:: resize\nalt - h : a\nalt - r ; resize\nresize < h : b\n",
            PlanMode::Static,
        );
        assert_eq!(keys(&plan), (vec!["Alt+H".to_string()], vec![]));
        assert!(!plan.needs_engine());
        assert_eq!(
            messages(&plan),
            [
                "line 3: warning: the blacklist is not applied: a static shortcut works in every \
                 application",
                "line 4: warning: Alt+R is left out: it switches modes, which needs the engine",
                "line 5: warning: H is left out: it is in the mode `resize`, which needs the engine",
            ]
        );
    }

    #[test]
    fn the_comment_says_where_the_binding_came_from() {
        let plan = planned(
            Format::Whkd,
            "alt + h : komorebic focus left # left\n",
            PlanMode::Auto,
        );
        assert_eq!(
            plan.static_files[0].comment,
            "from the whkdrc: alt + h : komorebic focus left # left"
        );
        let plan = planned(
            Format::Sxhkd,
            "super + h\n    komorebic focus left\n",
            PlanMode::Auto,
        );
        assert_eq!(
            plan.static_files[0].comment,
            "from the sxhkdrc: super + h: komorebic focus left"
        );
    }

    #[test]
    fn the_default_is_the_engine() {
        assert_eq!(PlanOptions::default().mode, PlanMode::Engine);
    }
}
