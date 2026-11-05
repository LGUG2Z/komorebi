//! The checks behind `komorebic check`: the configuration, and the desktop komorebi has to run on.
//!
//! Each check is a function that returns [`Finding`]s, so that each can be tested on its own.
//! `komorebic` gathers them in a [`Report`], one [`Section`] at a time, prints
//! [`Report::render`], and exits with status 1 when [`Report::has_errors`] is true.
//!
//! - [`check_configuration`] reads `komorebi.json` the way komorebi does and shows where it is
//!   wrong, with the lines around the mistake ([`snippet`]). It also returns the [`Wants`] of the
//!   file: what the desktop is going to be asked for.
//! - [`check_keys`] tells a key for another platform from a typo. One `komorebi.json` can be shared
//!   with komorebi on Windows and macOS, so a key that only they read is accepted and listed once
//!   as ignored; a key that no platform has is a warning, with the nearest key komorebi has.
//! - [`check_environment`] looks at the desktop: a KDE Plasma session on Wayland, KWin 6 on the
//!   session bus, komorebi and its script in KWin, the border and animation effects, the Plasma
//!   shell when there are wallpapers, and what starts komorebi at login.
//!
//! The two effects are not part of KWin, and only komorebi's builds of them do everything komorebi
//! asks: a color for each kind of window (KDE Rounded Corners) and every `animation.style`
//! (Geometry Change). The stock builds have a color for the active window and one for the others,
//! and one curve, an exponential ease out. The check tells a build by what it contains.
//!
//! A [`Finding`] has a [`Level`]: an error is something that stops komorebi, or a part of it, from
//! working (a file that is not JSON, no KWin on the bus); a warning is something that will not do
//! what was meant (a typo, a stock effect asked for a curve it does not have, a workspace that is
//! not tiled); a note is something to know.
//!
//! # Example
//!
//! ```rust
//! use komorebi::check::Finding;
//! use komorebi::check::Report;
//!
//! let mut report = Report::default();
//! report.push(
//!     "Configuration",
//!     vec![
//!         Finding::ok("komorebi.json is valid"),
//!         Finding::warning("`bordr` is not a setting komorebi has")
//!             .hint("did you mean `border`?"),
//!     ],
//! );
//!
//! assert_eq!(
//!     report.render(),
//!     concat!(
//!         "Configuration\n",
//!         "  ok      komorebi.json is valid\n",
//!         "  warning `bordr` is not a setting komorebi has\n",
//!         "          did you mean `border`?\n",
//!         "\n",
//!         "0 error(s), 1 warning(s)\n",
//!     ),
//! );
//! assert!(!report.has_errors());
//! ```
//!
//! # What is guaranteed, and by what
//!
//! | Rule | Enforced by | Proof |
//! |---|---|---|
//! | A file that is not JSON, or has a value komorebi does not read, is an error that shows the line | [`check_configuration`] parses the file as JSON, then as a [`StaticConfig`] | `check::tests::a_file_that_is_not_json_is_an_error_with_the_line`, `check::tests::a_value_komorebi_does_not_know_is_an_error_with_the_line` |
//! | A key for another platform is a note, and a key that no platform has is a warning with the nearest key | [`check_keys`], with the keys of [`known_keys`] and this module's lists of other platforms' keys | `check::tests::keys_for_another_platform_are_told_apart_from_mistakes` |
//! | The keys are the ones komorebi reads, at every level | [`known_keys`] reads them from the JSON schema of [`StaticConfig`] | `check::tests::the_keys_come_from_the_schema_at_every_level`, `check::tests::a_whole_example_configuration_has_nothing_the_check_does_not_know` |
//! | Placement rules that no window can use are a note | [`check_configuration`] compares the `initial_window_placement_rules` of a workspace with its `window_container_behaviour`, or the file's when it has none: windows that are appended to a container have no place of their own | `check::tests::placement_rules_that_are_not_used_while_windows_are_appended_are_noted` |
//! | A wallpaper that is not a file is an error, and a picture no theme can be made from is a warning | [`check_configuration`] looks at the wallpaper of each monitor and workspace once, and asks the theme generator for a palette when the wallpaper does not turn it off | `check::tests::a_wallpaper_that_is_not_there_and_a_rules_file_that_is_not_there_are_found` for the error, the `vm-check` VM suite for the warning |
//! | No session bus, or no KWin on it, is an error, and the desktop is looked at no further | [`check_environment`] stops at the first finding the rest depends on | `komorebic/tests/linux_check.rs::without_a_session_bus_the_desktop_is_an_error`, the `vm-check` VM suite |
//! | komorebi running without its script in KWin is an error | [`check_environment`] compares the daemon's socket with KWin's answer to `isScriptLoaded` | the `vm-check` VM suite |
//! | An effect is komorebi's build because of what it contains, not because of its name | [`check_environment`] looks for the colors interface in the border effect, and for a `Curve` setting in the animation effect's script | `check::tests::the_border_effect_is_komorebis_when_it_has_the_colours_interface`, `check::tests::the_effects_are_told_apart_by_what_they_contain` |
//! | Effects are looked for where KWin looks, not where the shell that runs the command would | [`check_environment`] reads KWin's environment from `/proc` | the `vm-check` VM suite |
//! | No D-Bus call delays the check by more than two seconds | convention: every call [`check_environment`] makes goes through a timeout | `komorebic/tests/linux_check.rs::a_session_bus_that_never_answers_is_given_up_on` |
//! | `komorebic check` exits with status 1 if and only if a finding is an error | `komorebic`, through [`Report::has_errors`] | `komorebic/tests/linux_check.rs` |

#![deny(missing_docs)]

use crate::DATA_DIR;
use crate::static_config::StaticConfig;
use std::collections::BTreeSet;
use std::path::Path;
use std::path::PathBuf;
use std::time::Duration;

/// How much a [`Finding`] matters.
///
/// Levels are ordered from [`Level::Ok`] to [`Level::Error`], so `level >= Level::Warning` picks
/// the findings that need doing something about.
///
/// ```rust
/// use komorebi::check::Level;
///
/// assert!(Level::Ok < Level::Note);
/// assert!(Level::Note < Level::Warning);
/// assert!(Level::Warning < Level::Error);
/// ```
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Level {
    /// Right as it is.
    Ok,
    /// Something to know.
    Note,
    /// Something that will not do what was meant.
    Warning,
    /// Something that stops komorebi, or a part of it, from working.
    Error,
}

/// One thing a check found: how much it matters, what it is, and what to do about it.
///
/// ```rust
/// use komorebi::check::Finding;
/// use komorebi::check::Level;
///
/// let finding =
///     Finding::warning("`bordr` is not a setting komorebi has").hint("did you mean `border`?");
///
/// assert_eq!(finding.level, Level::Warning);
/// assert_eq!(finding.hint.as_deref(), Some("did you mean `border`?"));
/// ```
#[derive(Clone, Debug)]
pub struct Finding {
    /// How much it matters.
    pub level: Level,
    /// The description of what was found, on one line or more.
    pub message: String,
    /// Advice on what to do about it, or more detail, which [`Report::render`] shows under the
    /// message.
    pub hint: Option<String>,
}

impl Finding {
    /// Returns a finding at this level, with no hint.
    fn new(level: Level, message: impl Into<String>) -> Self {
        Self {
            level,
            message: message.into(),
            hint: None,
        }
    }

    /// Returns a finding at [`Level::Ok`], with no hint.
    pub fn ok(message: impl Into<String>) -> Self {
        Self::new(Level::Ok, message)
    }

    /// Returns a finding at [`Level::Note`], with no hint.
    pub fn note(message: impl Into<String>) -> Self {
        Self::new(Level::Note, message)
    }

    /// Returns a finding at [`Level::Warning`], with no hint.
    pub fn warning(message: impl Into<String>) -> Self {
        Self::new(Level::Warning, message)
    }

    /// Returns a finding at [`Level::Error`], with no hint.
    pub fn error(message: impl Into<String>) -> Self {
        Self::new(Level::Error, message)
    }

    /// Returns the finding with this hint, in place of any it had.
    #[must_use]
    pub fn hint(mut self, hint: impl Into<String>) -> Self {
        self.hint = Some(hint.into());
        self
    }
}

/// The findings of one part of a check, under a title.
#[derive(Clone, Debug)]
pub struct Section {
    /// The title the section is printed under.
    pub title: String,
    /// The findings, in the order they are printed.
    pub findings: Vec<Finding>,
}

/// The findings of a whole check, one [`Section`] at a time, in the order they were pushed.
///
/// The default is a report with no sections. The [module docs](self#example) have an example.
#[derive(Clone, Debug, Default)]
pub struct Report {
    /// The sections, in the order they are printed.
    pub sections: Vec<Section>,
}

impl Report {
    /// Adds a section of findings under this title, after the others.
    pub fn push(&mut self, title: &str, findings: Vec<Finding>) {
        self.sections.push(Section {
            title: title.to_string(),
            findings,
        });
    }

    /// Returns the number of findings at this level, in every section.
    pub fn count(&self, level: Level) -> usize {
        self.sections
            .iter()
            .flat_map(|s| &s.findings)
            .filter(|f| f.level == level)
            .count()
    }

    /// Returns true if and only if a finding of the report is at [`Level::Error`].
    pub fn has_errors(&self) -> bool {
        self.count(Level::Error) > 0
    }

    /// Returns the report as text: a section at a time, under its title, each finding after its
    /// level, the hint under the message, and then the number of errors and warnings.
    ///
    /// A message or hint of several lines keeps its lines, each indented under the first.
    pub fn render(&self) -> String {
        let mut out = String::new();

        for section in &self.sections {
            out.push_str(&section.title);
            out.push('\n');

            for finding in &section.findings {
                let label = match finding.level {
                    Level::Ok => "ok",
                    Level::Note => "note",
                    Level::Warning => "warning",
                    Level::Error => "error",
                };

                let mut lines = finding.message.lines();
                out.push_str(&format!(
                    "  {label:<8}{}\n",
                    lines.next().unwrap_or_default()
                ));
                for line in lines {
                    out.push_str(&format!("          {line}\n"));
                }
                if let Some(hint) = &finding.hint {
                    for line in hint.lines() {
                        out.push_str(&format!("          {line}\n"));
                    }
                }
            }
            out.push('\n');
        }

        out.push_str(&format!(
            "{} error(s), {} warning(s)\n",
            self.count(Level::Error),
            self.count(Level::Warning)
        ));

        out
    }
}

/// The parts of the desktop a configuration needs, which [`check_environment`] checks are there.
///
/// Returned by [`check_configuration`]. The default asks for nothing.
#[derive(Clone, Debug, Default)]
pub struct Wants {
    /// Whether `border` is on.
    pub borders: bool,
    /// The `animation.style` asked for (the `movement` one, when styles are given for each kind of
    /// animation), if animations are on. It is `Linear`, komorebi's default style, when the file
    /// names no style.
    pub animation: Option<String>,
    /// Whether a monitor or a workspace has a wallpaper.
    pub wallpapers: bool,
}

// ---- the configuration ------------------------------------------------------------------------

/// The keys at the root of the configuration that only another platform reads. komorebi does not
/// have them; they are accepted, so that one file can be shared, and do nothing here.
const IGNORED_ROOT: &[&str] = &[
    "bar_configurations",
    "border_implementation",
    "border_overflow_applications",
    "border_style",
    "border_z_order",
    "invisible_borders",
    "layered_applications",
    "minimum_window_height",
    "minimum_window_width",
    "monitor_index_preferences",
    "object_name_change_applications",
    "object_name_change_title_ignore_list",
    "remove_titlebar_applications",
    "slow_application_compensation_time",
    "slow_application_identifiers",
    "stackbar",
    "tray_and_multi_window_applications",
    "window_handling_behaviour",
    "window_hiding_behaviour",
];

/// The keys at the root of the configuration that komorebi reads, because komorebi for macOS has
/// them, and that do nothing on KDE.
const NO_EFFECT_ROOT: &[&str] = &["tabbed_applications", "titleless_applications"];
/// The keys of a monitor that komorebi reads, because komorebi for macOS has them, and that do
/// nothing on KDE.
const NO_EFFECT_MONITOR: &[&str] = &["window_hiding_position"];

/// The keys of a workspace that komorebi for Windows has and komorebi on KDE does not support yet:
/// custom layouts.
const UNSUPPORTED_WORKSPACE: &[&str] = &["custom_layout", "custom_layout_rules"];

/// Returns the lines of a file up to `line`, the way a compiler shows them: at most two lines
/// before it, each after its number, and a caret under `column`.
///
/// Lines and columns count from 1, as `serde_json`'s errors do. A line past the end is the last
/// line, and a column past the end of the line is its end. A file with no lines is shown as
/// nothing.
///
/// ```rust
/// use komorebi::check::snippet;
///
/// let raw = "{\n  \"border\": true,\n  \"border_width\": ,\n}\n";
///
/// assert_eq!(
///     snippet(raw, 3, 19),
///     concat!(
///         "1 | {\n",
///         "2 |   \"border\": true,\n",
///         "3 |   \"border_width\": ,\n",
///         "  |                   ^\n",
///     ),
/// );
/// assert_eq!(snippet("", 1, 1), "");
/// ```
pub fn snippet(raw: &str, line: usize, column: usize) -> String {
    let lines: Vec<&str> = raw.lines().collect();
    if lines.is_empty() {
        return String::new();
    }

    let line = line.clamp(1, lines.len());
    let first = line.saturating_sub(2).max(1);
    let width = line.to_string().len();

    let mut out = String::new();
    for n in first..=line {
        out.push_str(&format!("{n:>width$} | {}\n", lines[n - 1]));
    }
    out.push_str(&format!(
        "{:>width$} | {}^\n",
        "",
        " ".repeat(column.saturating_sub(1).min(lines[line - 1].len()))
    ));

    out
}

/// Returns the error for a file that could not be read as `what`: the path, `what`, serde's
/// message, and the lines up to where serde stopped ([`snippet`]).
fn json_error(path: &Path, raw: &str, error: &serde_json::Error, what: &str) -> Finding {
    Finding::error(format!(
        "{} {what}: {error}\n{}",
        path.display(),
        snippet(raw, error.line(), error.column()).trim_end()
    ))
}

/// Returns the edit distance (Levenshtein) between two strings: the number of characters to insert,
/// delete or replace to turn one into the other.
fn distance(a: &str, b: &str) -> usize {
    let b: Vec<char> = b.chars().collect();
    let mut row: Vec<usize> = (0..=b.len()).collect();

    for (i, ca) in a.chars().enumerate() {
        let mut previous = row[0];
        row[0] = i + 1;
        for (j, cb) in b.iter().enumerate() {
            let above = row[j + 1];
            row[j + 1] = (previous + usize::from(ca != *cb))
                .min(row[j] + 1)
                .min(above + 1);
            previous = above;
        }
    }

    row[b.len()]
}

/// Returns the known key nearest to `key`, if one is near enough to be what was meant: at most
/// three edits away, and fewer edits than the shorter of the two has characters.
fn closest<'a>(key: &str, known: impl Iterator<Item = &'a String>) -> Option<&'a String> {
    known
        .map(|candidate| (distance(key, candidate), candidate))
        .filter(|(d, candidate)| *d <= 3 && *d < key.len().min(candidate.len()))
        .min_by_key(|(d, _)| *d)
        .map(|(_, candidate)| candidate)
}

/// The keys each level of the configuration may have.
///
/// Made by [`known_keys`] and read by [`check_keys`].
#[derive(Clone, Debug, Default)]
pub struct KnownKeys {
    /// The keys at the root of the file.
    pub root: BTreeSet<String>,
    /// The keys of a monitor, in `monitors`.
    pub monitor: BTreeSet<String>,
    /// The keys of a workspace, in a monitor's `workspaces`.
    pub workspace: BTreeSet<String>,
}

/// The keys of the configuration, read from the JSON schema of [`StaticConfig`].
#[cfg(feature = "schemars")]
mod schema {
    use super::KnownKeys;
    use crate::static_config::StaticConfig;
    use serde_json::Value;
    use std::collections::BTreeSet;

    /// Adds to `out` the schemas a node stands for, once references (`$ref`), alternatives
    /// (`anyOf`, `oneOf`, `allOf`) and arrays (`items`) are looked through. A reference that does
    /// not resolve adds nothing, and the search stops 12 levels down, so that a schema that refers
    /// to itself ends.
    fn flatten<'a>(root: &'a Value, node: &'a Value, out: &mut Vec<&'a Value>, depth: usize) {
        if depth > 12 {
            return;
        }

        if let Some(target) = node
            .get("$ref")
            .and_then(Value::as_str)
            .and_then(|r| r.strip_prefix("#/"))
        {
            let mut at = root;
            for part in target.split('/') {
                match at.get(part) {
                    Some(next) => at = next,
                    None => return,
                }
            }
            flatten(root, at, out, depth + 1);
            return;
        }

        out.push(node);

        for key in ["anyOf", "oneOf", "allOf"] {
            if let Some(alternatives) = node.get(key).and_then(Value::as_array) {
                for alternative in alternatives {
                    flatten(root, alternative, out, depth + 1);
                }
            }
        }

        if let Some(items) = node.get("items") {
            flatten(root, items, out, depth + 1);
        }
    }

    /// Returns the names of the properties of every schema a node stands for.
    fn properties(root: &Value, node: &Value) -> BTreeSet<String> {
        let mut nodes = vec![];
        flatten(root, node, &mut nodes, 0);

        nodes
            .iter()
            .filter_map(|n| n.get("properties").and_then(Value::as_object))
            .flat_map(|p| p.keys().cloned())
            .collect()
    }

    /// Returns the schema of the property `key` of a node, from the first schema the node stands
    /// for that has it.
    fn child<'a>(root: &'a Value, node: &'a Value, key: &str) -> Option<&'a Value> {
        let mut nodes = vec![];
        flatten(root, node, &mut nodes, 0);

        nodes
            .iter()
            .find_map(|n| n.get("properties").and_then(|p| p.get(key)))
    }

    /// Returns the keys of each level, or `None` when the schema has no `monitors` or no
    /// `workspaces` to find them in.
    pub fn known_keys() -> Option<KnownKeys> {
        let root: serde_json::Value =
            serde_json::from_str(&komorebi_config::schema::view::<StaticConfig>(
                komorebi_config::schema::Platform::CURRENT,
            ))
            .ok()?;
        let monitors = child(&root, &root, "monitors")?;
        let workspaces = child(&root, monitors, "workspaces");

        // `monitors` is looked through to the monitor, and that to its workspaces
        let mut monitor_nodes = vec![];
        flatten(&root, monitors, &mut monitor_nodes, 0);
        let monitor_node = monitor_nodes
            .iter()
            .find(|n| n.get("properties").is_some())
            .copied()?;
        let workspaces = workspaces.or_else(|| child(&root, monitor_node, "workspaces"))?;

        Some(KnownKeys {
            root: properties(&root, &root),
            monitor: properties(&root, monitors),
            workspace: properties(&root, workspaces),
        })
    }
}

/// Returns the keys each level of the configuration may have, read from the JSON schema of
/// [`StaticConfig`].
///
/// Returns `None` when komorebi is built without the `schemars` feature (it is on by default):
/// [`check_configuration`] then does not look at the keys.
///
/// ```rust
/// let Some(known) = komorebi::check::known_keys() else {
///     return; // built without the `schemars` feature
/// };
///
/// assert!(known.root.contains("border"));
/// assert!(known.monitor.contains("workspaces"));
/// assert!(known.workspace.contains("layout"));
/// // a key of komorebi for Windows only
/// assert!(!known.root.contains("border_style"));
/// ```
#[cfg(feature = "schemars")]
pub fn known_keys() -> Option<KnownKeys> {
    schema::known_keys()
}

/// Returns `None`: komorebi is built without the `schemars` feature, so there is no schema to
/// read the keys from, and [`check_configuration`] does not look at them.
#[cfg(not(feature = "schemars"))]
pub fn known_keys() -> Option<KnownKeys> {
    None
}

/// Returns what the keys of every level of a configuration say: a key that only another platform
/// reads is not a mistake, and one that no platform reads is, usually a typo.
///
/// - The keys that only another platform reads, and the ones komorebi reads but that do nothing
///   on KDE, are listed in one [`Level::Note`], the first finding.
/// - A workspace key that komorebi for Windows has and komorebi on KDE does not support yet
///   (`custom_layout`, `custom_layout_rules`) is a [`Level::Warning`].
/// - Any other key that is not in `known` is a [`Level::Warning`], with the nearest key in `known`
///   as the hint when one is near enough to be what was meant.
///
/// `$schema` is never a finding.
///
/// ```rust
/// use komorebi::check::KnownKeys;
/// use komorebi::check::Level;
/// use komorebi::check::check_keys;
///
/// let known = KnownKeys {
///     root: ["border", "monitors"].map(String::from).into(),
///     ..KnownKeys::default()
/// };
/// let file = serde_json::json!({ "border": true, "stackbar": {}, "bordr": true });
///
/// let findings = check_keys(&file, &known);
/// assert_eq!(findings.len(), 2);
/// assert_eq!(findings[0].level, Level::Note);
/// assert_eq!(
///     findings[0].message,
///     "for another platform, accepted and ignored here: stackbar"
/// );
/// assert_eq!(findings[1].level, Level::Warning);
/// assert_eq!(findings[1].message, "`bordr` is not a setting komorebi has");
/// assert_eq!(findings[1].hint.as_deref(), Some("did you mean `border`?"));
/// ```
pub fn check_keys(value: &serde_json::Value, known: &KnownKeys) -> Vec<Finding> {
    let mut findings = vec![];
    let mut ignored: Vec<String> = vec![];

    let mut look = |level: &str,
                    object: &serde_json::Map<String, serde_json::Value>,
                    known: &BTreeSet<String>,
                    no_effect: &[&str],
                    unsupported: &[&str],
                    ignored_here: &[&str]| {
        for key in object.keys() {
            if key == "$schema" {
                continue;
            }

            if known.contains(key) {
                if no_effect.contains(&key.as_str()) {
                    ignored.push(format!("{level}{key}"));
                }
            } else if unsupported.contains(&key.as_str()) {
                findings.push(
                    Finding::warning(format!(
                        "`{key}` ({}) is not supported on KDE yet, and does nothing",
                        level.trim_end_matches('.').trim_end_matches('/')
                    ))
                    .hint("it is for Windows; the rest of the file is read as usual"),
                );
            } else if ignored_here.contains(&key.as_str()) {
                ignored.push(format!("{level}{key}"));
            } else {
                let suggestion = closest(key, known.iter())
                    .map(|k| format!("did you mean `{k}`?"))
                    .unwrap_or_default();
                findings.push(
                    Finding::warning(format!("`{level}{key}` is not a setting komorebi has"))
                        .hint(suggestion),
                );
            }
        }
    };

    if let Some(root) = value.as_object() {
        look("", root, &known.root, NO_EFFECT_ROOT, &[], IGNORED_ROOT);

        for (m, monitor) in root
            .get("monitors")
            .and_then(|m| m.as_array())
            .into_iter()
            .flatten()
            .enumerate()
        {
            if let Some(monitor) = monitor.as_object() {
                let at = format!("monitors[{m}].");
                look(&at, monitor, &known.monitor, NO_EFFECT_MONITOR, &[], &[]);

                for (w, workspace) in monitor
                    .get("workspaces")
                    .and_then(|w| w.as_array())
                    .into_iter()
                    .flatten()
                    .enumerate()
                {
                    if let Some(workspace) = workspace.as_object() {
                        let at = format!("monitors[{m}].workspaces[{w}].");
                        look(
                            &at,
                            workspace,
                            &known.workspace,
                            &[],
                            UNSUPPORTED_WORKSPACE,
                            &[],
                        );
                    }
                }
            }
        }
    }

    if !ignored.is_empty() {
        findings.insert(
            0,
            Finding::note(format!(
                "for another platform, accepted and ignored here: {}",
                ignored.join(", ")
            )),
        );
    }

    findings
}

/// Returns the findings about the configuration file at `path`, and what it asks of the desktop.
///
/// The file is read as JSON, then as a [`StaticConfig`], the way komorebi reads it; the first of
/// the two that fails is the one finding, a [`Level::Error`] that shows the line. A file that is
/// not there is a [`Level::Note`]: komorebi starts with its defaults. A file that is read is an
/// [`Level::Ok`], followed by the findings of [`check_keys`] about its keys and those about what
/// its settings mean: application specific configuration files and wallpapers that are not there,
/// workspaces that are not tiled, placement rules that are not used, and wallpapers no theme can
/// be made from.
///
/// The [`Wants`] are the default unless the file is read.
///
/// ```rust
/// use komorebi::check::Level;
/// use komorebi::check::check_configuration;
/// use std::path::Path;
///
/// let (findings, wants) = check_configuration(Path::new("/nonexistent/komorebi.json"));
///
/// assert_eq!(findings.len(), 1);
/// assert_eq!(findings[0].level, Level::Note);
/// assert!(!wants.borders && wants.animation.is_none() && !wants.wallpapers);
/// ```
///
/// The files that are read are covered by the tests in `komorebi/src/check.rs` and
/// `komorebic/tests/linux_check.rs`.
pub fn check_configuration(path: &Path) -> (Vec<Finding>, Wants) {
    let mut findings = vec![];
    let mut wants = Wants::default();

    if !path.exists() {
        findings.push(
            Finding::note(format!(
                "there is no {}: komorebi starts with its defaults",
                path.display()
            ))
            .hint("`komorebic quickstart` has an example to begin from"),
        );
        return (findings, wants);
    }

    let raw = match std::fs::read_to_string(path) {
        Ok(raw) => raw,
        Err(error) => {
            findings.push(Finding::error(format!(
                "could not read {}: {error}",
                path.display()
            )));
            return (findings, wants);
        }
    };

    let value = match serde_json::from_str::<serde_json::Value>(&raw) {
        Ok(value) => value,
        Err(error) => {
            findings.push(json_error(path, &raw, &error, "is not valid JSON"));
            return (findings, wants);
        }
    };

    let config = match serde_json::from_str::<StaticConfig>(&raw) {
        Ok(config) => config,
        Err(error) => {
            findings.push(
                json_error(path, &raw, &error, "does not match what komorebi reads")
                    .hint("`komorebic static-config-schema` has the schema, for an editor to check against"),
            );
            return (findings, wants);
        }
    };

    findings.push(Finding::ok(format!("{} is valid", path.display())));

    if let Some(known) = known_keys() {
        findings.extend(check_keys(&value, &known));
    }

    findings.extend(check_meaning(&config, &value, &mut wants));

    (findings, wants)
}

/// Returns the findings about what the settings of a file that was read mean, and fills in `wants`.
///
/// It looks for application specific configuration files and wallpapers that are not there,
/// workspaces with no layout (which komorebi does not tile), `initial_window_placement_rules` that
/// are not used because windows are appended to a container, and wallpapers that no theme can be
/// made from.
fn check_meaning(
    config: &StaticConfig,
    value: &serde_json::Value,
    wants: &mut Wants,
) -> Vec<Finding> {
    let mut findings = vec![];

    // the application specific configuration that is named is there
    let asc_paths: Vec<&PathBuf> = match &config.app_specific_configuration_path {
        Some(crate::static_config::AppSpecificConfigurationPath::Single(path)) => vec![path],
        Some(crate::static_config::AppSpecificConfigurationPath::Multiple(paths)) => {
            paths.iter().collect()
        }
        None => vec![],
    };
    for path in asc_paths {
        if !path.exists() {
            findings.push(
                Finding::warning(format!(
                    "the application specific configuration {} does not exist",
                    path.display()
                ))
                .hint("komorebi starts without it: the windows that it names are managed like any other"),
            );
        }
    }

    // a workspace that has been given a name and nothing else is not tiled
    for (m, monitor) in config.monitors.iter().flatten().enumerate() {
        for (w, workspace) in monitor.workspaces.iter().enumerate() {
            let tiles = workspace.layout.is_some()
                || workspace.tile.is_some()
                || workspace
                    .layout_rules
                    .as_ref()
                    .is_some_and(|r| !r.is_empty());

            if !tiles {
                findings.push(
                    Finding::warning(format!(
                        "workspace {w} of monitor {m}{} has no `layout`, so komorebi does not tile it",
                        if workspace.name.is_empty() {
                            String::new()
                        } else {
                            format!(" (\"{}\")", workspace.name)
                        }
                    ))
                    .hint("give it a `layout` (\"BSP\", say), or `\"tile\": true` for the default"),
                );
            }

            // the workspace's own setting is the one that counts, and the file's is the default
            let behaviour = workspace
                .window_container_behaviour
                .or(config.window_container_behaviour);
            if workspace.initial_window_placement_rules.is_some()
                && behaviour == Some(crate::core::WindowContainerBehaviour::Append)
            {
                findings.push(Finding::note(format!(
                    "workspace {w} of monitor {m} has initial_window_placement_rules, which are not used while windows are appended to a container (`window_container_behaviour`: \"Append\")"
                )));
            }
        }
    }

    // wallpapers are there, and a theme can be made of them
    let mut wallpapers = vec![];
    for monitor in config.monitors.iter().flatten() {
        wallpapers.extend(monitor.wallpaper.iter());
        for workspace in &monitor.workspaces {
            wallpapers.extend(workspace.wallpaper.iter());
        }
    }
    wants.wallpapers = !wallpapers.is_empty();

    let mut seen = BTreeSet::new();
    for wallpaper in wallpapers {
        if !seen.insert(wallpaper.path.clone()) {
            continue;
        }

        if !wallpaper.path.is_file() {
            findings.push(Finding::error(format!(
                "the wallpaper {} is not a file",
                wallpaper.path.display()
            )));
        } else if wallpaper.generate_theme.unwrap_or(true) {
            let variant = wallpaper
                .theme_options
                .as_ref()
                .and_then(|o| o.theme_variant)
                .unwrap_or_default();
            if let Err(error) = komorebi_themes::generate_base16_palette(&wallpaper.path, variant) {
                findings.push(
                    Finding::warning(format!(
                        "no theme can be made from {} ({error}): the borders keep the colors they have",
                        wallpaper.path.display()
                    ))
                    .hint("a picture needs eight different colors; `\"generate_theme\": false` turns this off"),
                );
            }
        }
    }

    // what the desktop is going to be asked for
    wants.borders = value
        .get("border")
        .and_then(|b| b.as_bool())
        .unwrap_or(false);

    let animation = value.get("animation");
    let enabled = animation.and_then(|a| a.get("enabled")).is_some_and(|e| {
        e.as_bool().unwrap_or_else(|| {
            e.as_object()
                .is_some_and(|o| o.values().any(|v| v.as_bool() == Some(true)))
        })
    });
    if enabled {
        let default = crate::animation::DEFAULT_ANIMATION_STYLE.to_string();
        let style = animation.and_then(|a| a.get("style"));
        wants.animation = Some(match style {
            Some(serde_json::Value::String(style)) => style.clone(),
            Some(serde_json::Value::Object(styles)) => styles
                .get("movement")
                .and_then(|s| s.as_str())
                .unwrap_or(&default)
                .to_string(),
            _ => default,
        });
    }

    findings
}

// ---- the desktop ------------------------------------------------------------------------------

/// Returns true if and only if the file can be read and contains these bytes.
fn file_contains(path: &Path, needle: &[u8]) -> bool {
    std::fs::read(path)
        .map(|bytes| bytes.windows(needle.len()).any(|w| w == needle))
        .unwrap_or(false)
}

/// Returns the environment of a process, from `/proc/<pid>/environ`, or an empty map when it cannot
/// be read. The processes of the user who runs the check can be read.
fn environment_of(pid: u32) -> std::collections::HashMap<String, String> {
    std::fs::read(format!("/proc/{pid}/environ"))
        .map(|bytes| {
            bytes
                .split(|b| *b == 0)
                .filter_map(|entry| {
                    let entry = String::from_utf8_lossy(entry);
                    let (key, value) = entry.split_once('=')?;
                    Some((key.to_string(), value.to_string()))
                })
                .collect()
        })
        .unwrap_or_default()
}

/// Returns the directories of a colon-separated list, such as `QT_PLUGIN_PATH`, without the empty
/// entries.
fn split_paths(value: Option<&String>) -> Vec<PathBuf> {
    value
        .map(|v| {
            v.split(':')
                .filter(|p| !p.is_empty())
                .map(PathBuf::from)
                .collect()
        })
        .unwrap_or_default()
}

/// Returns the paths at which KWin may find the plugin file of an effect, in the order they are
/// looked at.
///
/// The directories are those of `QT_PLUGIN_PATH` in KWin's environment (`kwin_env`), then those of
/// `QT_PLUGIN_PATH` in this process's environment, then the places NixOS and the common
/// distributions put Qt 6 plugins, then `~/.local/lib/qt6/plugins`. Each path is
/// `<dir>/kwin/effects/plugins/<file>`.
///
/// ```rust
/// use std::collections::HashMap;
/// use std::path::Path;
///
/// let kwin_env = HashMap::from([("QT_PLUGIN_PATH".to_string(), "/opt/qt/plugins".to_string())]);
/// let candidates = komorebi::check::plugin_candidates(&kwin_env, "kwin4_effect_shapecorners.so");
///
/// let first = Path::new("/opt/qt/plugins/kwin/effects/plugins/kwin4_effect_shapecorners.so");
/// assert_eq!(candidates[0], first);
///
/// let nixos = Path::new("/run/current-system/sw/lib/qt-6/plugins")
///     .join("kwin/effects/plugins/kwin4_effect_shapecorners.so");
/// assert!(candidates.contains(&nixos));
/// ```
pub fn plugin_candidates(
    kwin_env: &std::collections::HashMap<String, String>,
    file: &str,
) -> Vec<PathBuf> {
    let mut dirs = split_paths(kwin_env.get("QT_PLUGIN_PATH"));
    dirs.extend(split_paths(std::env::var("QT_PLUGIN_PATH").ok().as_ref()));
    for fixed in [
        "/run/current-system/sw/lib/qt-6/plugins",
        "/usr/lib/qt6/plugins",
        "/usr/lib64/qt6/plugins",
        "/usr/lib/x86_64-linux-gnu/qt6/plugins",
        "/usr/lib/aarch64-linux-gnu/qt6/plugins",
    ] {
        dirs.push(PathBuf::from(fixed));
    }
    if let Some(home) = dirs::home_dir() {
        dirs.push(home.join(".local/lib/qt6/plugins"));
    }

    dirs.into_iter()
        .map(|dir| dir.join("kwin/effects/plugins").join(file))
        .collect()
}

/// Returns the paths at which KWin may find the script of a scripted effect, in the order KWin
/// looks: the user's data directory, then each of the system's.
///
/// The user's directory is `XDG_DATA_HOME` in KWin's environment (`kwin_env`), else this process's
/// data directory; the system's are `XDG_DATA_DIRS` in KWin's environment, else `/usr/local/share`
/// and `/usr/share`. The places Nix installs to follow, which a process started outside a login
/// does not have in its `XDG_DATA_DIRS` but KWin, started by the session, does. A directory named
/// twice is looked at once. Each path is `<dir>/kwin/effects/<effect>/contents/code/main.js`.
///
/// ```rust
/// use std::collections::HashMap;
/// use std::path::Path;
///
/// let kwin_env = HashMap::from([
///     ("XDG_DATA_HOME".to_string(), "/home/me/.local/share".to_string()),
///     ("XDG_DATA_DIRS".to_string(), "/usr/share".to_string()),
/// ]);
/// let candidates =
///     komorebi::check::script_effect_candidates(&kwin_env, "kwin4_effect_geometry_change");
///
/// let script = "kwin/effects/kwin4_effect_geometry_change/contents/code/main.js";
/// assert_eq!(candidates[0], Path::new("/home/me/.local/share").join(script));
/// assert_eq!(candidates[1], Path::new("/usr/share").join(script));
/// ```
pub fn script_effect_candidates(
    kwin_env: &std::collections::HashMap<String, String>,
    effect: &str,
) -> Vec<PathBuf> {
    let mut dirs = vec![];

    match kwin_env.get("XDG_DATA_HOME").filter(|d| !d.is_empty()) {
        Some(home) => dirs.push(PathBuf::from(home)),
        None => dirs.extend(dirs::data_dir()),
    }

    let system = split_paths(kwin_env.get("XDG_DATA_DIRS"));
    if system.is_empty() {
        dirs.push(PathBuf::from("/usr/local/share"));
        dirs.push(PathBuf::from("/usr/share"));
    } else {
        dirs.extend(system);
    }

    // where Nix puts what is installed, which a process started outside a login does not have in
    // its XDG_DATA_DIRS but KWin, started by the session, does
    dirs.push(PathBuf::from("/run/current-system/sw/share"));
    dirs.push(PathBuf::from("/nix/var/nix/profiles/default/share"));
    if let Some(home) = dirs::home_dir() {
        dirs.push(home.join(".nix-profile/share"));
    }
    if let Ok(user) = std::env::var("USER") {
        dirs.push(PathBuf::from(format!(
            "/etc/profiles/per-user/{user}/share"
        )));
    }

    // one copy is one copy, however many ways there are to name where it is
    let mut seen = BTreeSet::new();
    dirs.retain(|dir| seen.insert(dir.clone()));

    dirs.into_iter()
        .map(|dir| {
            dir.join("kwin/effects")
                .join(effect)
                .join("contents/code/main.js")
        })
        .collect()
}

/// The plugin id of KDE Rounded Corners, the border effect.
const BORDER_EFFECT: &str = "kwin4_effect_shapecorners";
/// The plugin id of Geometry Change, the animation effect.
const ANIMATION_EFFECT: &str = "kwin4_effect_geometry_change";
/// The D-Bus interface of komorebi's build of the border effect, whose name is in its plugin file.
const COLOURS_INTERFACE: &[u8] = b"org.komorebi.BorderColours1";

/// Returns the findings about the border effect, KDE Rounded Corners: whether KWin has it, and
/// whether it is komorebi's build, which takes a color for each window.
///
/// `supported` is KWin's answer to `isEffectSupported` (`None` when there was none). The build is
/// komorebi's when its object is on the bus (`on_bus`, true once the effect is loaded) or when the
/// plugin file KWin would load ([`plugin_candidates`]) contains the name of its interface. The
/// stock one has a color for the active window and one for the others. What is missing is a
/// [`Level::Warning`] when the configuration asks for borders, and a [`Level::Note`] when it does
/// not.
fn border_effect_findings(
    supported: Option<bool>,
    kwin_env: &std::collections::HashMap<String, String>,
    wants: &Wants,
    on_bus: bool,
) -> Vec<Finding> {
    let mut findings = vec![];
    let installed = supported.unwrap_or(false);

    if !installed {
        let finding = if wants.borders {
            Finding::warning(
                "the border effect (KDE Rounded Corners) is not installed: no borders will be drawn",
            )
        } else {
            Finding::note(
                "the border effect (KDE Rounded Corners) is not installed (borders are off anyway)",
            )
        };
        findings.push(
            finding.hint("komorebi's nix/kde-rounded-corners.nix builds the one it is made for"),
        );
        return findings;
    }

    let candidates = plugin_candidates(kwin_env, "kwin4_effect_shapecorners.so");
    let plugin = candidates.iter().find(|p| p.exists());

    let patched = on_bus || plugin.is_some_and(|plugin| file_contains(plugin, COLOURS_INTERFACE));

    if patched {
        findings.push(Finding::ok(
            "the border effect is komorebi's build: every kind of window has its own color",
        ));
    } else if plugin.is_some() || wants.borders {
        let finding = if wants.borders {
            Finding::warning(
                "the border effect is the stock one: borders have two colors, one for the focused window and one for the rest",
            )
        } else {
            Finding::note(
                "the border effect is the stock one (borders are off, so it does not matter)",
            )
        };
        findings.push(
            finding.hint(
                "single, stack, monocle, floating and locked-container colors need komorebi's build of KDE Rounded Corners (nix/kde-rounded-corners.nix)",
            ),
        );
    } else {
        findings.push(Finding::note("the border effect is installed"));
    }

    findings
}

/// Returns the findings about the animation effect, Geometry Change: whether KWin has it, and
/// whether the copy KWin uses is komorebi's build, which reads its curve from a `Curve` setting.
///
/// `supported` is KWin's answer to `isEffectSupported` (`None` when there was none). The copy KWin
/// uses is the first of [`script_effect_candidates`] that exists, and it is komorebi's build when
/// its script reads `Curve`. The stock one eases out exponentially and nothing else, which is a
/// [`Level::Warning`] only when the configuration asks for another `animation.style`. A later
/// copy that is not the same build as the one KWin uses is a warning too: the user's copy hides
/// the system's.
fn animation_effect_findings(
    supported: Option<bool>,
    kwin_env: &std::collections::HashMap<String, String>,
    wants: &Wants,
) -> Vec<Finding> {
    let mut findings = vec![];

    if !supported.unwrap_or(false) {
        let finding = if wants.animation.is_some() {
            Finding::warning(
                "the animation effect (Geometry Change) is not installed: windows will not be animated",
            )
        } else {
            Finding::note(
                "the animation effect (Geometry Change) is not installed (animations are off anyway)",
            )
        };
        findings.push(finding.hint(
            "it is a KWin script effect: install the `kwin4_effect_geometry_change` package",
        ));
        return findings;
    }

    let candidates = script_effect_candidates(kwin_env, ANIMATION_EFFECT);
    let found: Vec<&PathBuf> = candidates.iter().filter(|p| p.exists()).collect();
    let curve = |path: &Path| file_contains(path, b"readConfig(\"Curve\"");

    match found.first() {
        None => findings.push(Finding::note("the animation effect is installed")),
        Some(first) => {
            let patched = curve(first);

            if patched {
                findings.push(Finding::ok(
                    "the animation effect is komorebi's build: every `animation.style` is a curve of its own",
                ));
            } else if wants
                .animation
                .as_deref()
                .is_some_and(|s| s != "EaseOutExpo")
            {
                findings.push(
                    Finding::warning(format!(
                        "the animation effect is the stock one: `animation.style` \"{}\" is ignored, windows ease out exponentially",
                        wants.animation.clone().unwrap_or_default()
                    ))
                    .hint("komorebi's build (nix/kwin-effect-geometry-change.nix) reads the curve"),
                );
            } else {
                findings.push(Finding::note(
                    "the animation effect is the stock one: it eases out exponentially",
                ));
            }

            // KWin takes the first copy it finds, and the user's own comes before the system's
            if let Some(shadowed) = found.iter().skip(1).find(|p| curve(p) != patched) {
                findings.push(
                    Finding::warning(format!(
                        "{} is used, and shadows {}, which is not the same build",
                        first.display(),
                        shadowed.display()
                    ))
                    .hint("delete the copy in your home directory if it is not meant to be there, and log in again"),
                );
            }
        }
    }

    findings
}

/// Returns the output of the future, or `None` when it does not finish within two seconds.
async fn within<T>(future: impl std::future::Future<Output = T>) -> Option<T> {
    tokio::time::timeout(Duration::from_secs(2), future)
        .await
        .ok()
}

/// Returns the findings about the session, KWin, komorebi and its script, the effects and, when
/// there are wallpapers, the Plasma shell.
///
/// It stops at the first finding everything after it depends on: no session bus, a bus that does
/// not answer, or no KWin on it. Every D-Bus call has two seconds to answer ([`within`]).
async fn desktop_findings(wants: &Wants) -> Vec<Finding> {
    use zbus::Connection;
    use zbus::proxy::Proxy;

    let mut findings = vec![];

    // the session
    let desktop = std::env::var("XDG_CURRENT_DESKTOP").unwrap_or_default();
    if desktop.to_uppercase().contains("KDE") {
        findings.push(Finding::ok("this is a KDE Plasma session"));
    } else {
        findings.push(Finding::warning(format!(
            "this does not look like a KDE Plasma session (XDG_CURRENT_DESKTOP is \"{desktop}\")"
        )));
    }

    let wayland = std::env::var("XDG_SESSION_TYPE").is_ok_and(|t| t == "wayland")
        || std::env::var_os("WAYLAND_DISPLAY").is_some();
    if wayland {
        findings.push(Finding::ok("it is a Wayland session"));
    } else {
        findings.push(
            Finding::warning("this is not a Wayland session")
                .hint("komorebi is made for and tried on KWin on Wayland"),
        );
    }

    // the bus, and KWin on it
    let connection = match within(Connection::session()).await {
        Some(Ok(connection)) => connection,
        _ => {
            findings.push(Finding::error("could not connect to the session bus").hint(
                "DBUS_SESSION_BUS_ADDRESS has to be that of the desktop session (not so over ssh)",
            ));
            return findings;
        }
    };

    let Ok(dbus) = zbus::fdo::DBusProxy::new(&connection).await else {
        findings.push(Finding::error("the session bus does not answer"));
        return findings;
    };

    let has_owner = |name: &'static str| {
        let dbus = dbus.clone();
        async move {
            let name = zbus::names::BusName::try_from(name).ok()?;
            within(dbus.name_has_owner(name)).await?.ok()
        }
    };

    if has_owner("org.kde.KWin").await != Some(true) {
        findings.push(
            Finding::error("KWin is not on the session bus")
                .hint("komorebi works through KWin and nothing else"),
        );
        return findings;
    }

    // KWin: which, and the environment it looks for effects in
    let kwin_name = zbus::names::BusName::try_from("org.kde.KWin").ok();
    let kwin_pid = match kwin_name {
        Some(name) => within(dbus.get_connection_unix_process_id(name))
            .await
            .and_then(Result::ok),
        None => None,
    };
    let kwin_env = kwin_pid.map(environment_of).unwrap_or_default();

    let kwin = Proxy::new(&connection, "org.kde.KWin", "/KWin", "org.kde.KWin")
        .await
        .ok();
    let information = match &kwin {
        Some(kwin) => within(kwin.call::<_, _, String>("supportInformation", &()))
            .await
            .and_then(Result::ok)
            .unwrap_or_default(),
        None => String::new(),
    };
    let version = information
        .lines()
        .find_map(|l| l.strip_prefix("KWin version: "))
        .map(str::trim);

    match version.and_then(|v| v.split('.').next()?.parse::<u32>().ok()) {
        Some(major) if major >= 6 => findings.push(Finding::ok(format!(
            "KWin {} is there",
            version.unwrap_or_default()
        ))),
        Some(_) => findings.push(
            Finding::error(format!("KWin {} is too old", version.unwrap_or_default()))
                .hint("komorebi needs KWin 6 (Plasma 6)"),
        ),
        None => findings.push(Finding::note(
            "KWin is there (its version could not be read)",
        )),
    }

    // komorebi itself, and its script in KWin
    let running = std::os::unix::net::UnixStream::connect(DATA_DIR.join("komorebi.sock")).is_ok();
    let scripting = Proxy::new(
        &connection,
        "org.kde.KWin",
        "/Scripting",
        "org.kde.kwin.Scripting",
    )
    .await
    .ok();
    let script_loaded = match &scripting {
        Some(scripting) => {
            within(scripting.call::<_, _, bool>("isScriptLoaded", &("komorebi-bridge",)))
                .await
                .and_then(Result::ok)
        }
        None => None,
    };

    match (running, script_loaded) {
        (true, Some(true)) => findings.push(Finding::ok(
            "komorebi is running, and its script is in KWin",
        )),
        (true, _) => findings.push(
            Finding::error("komorebi is running but its script is not in KWin")
                .hint("see `komorebic log`; restarting komorebi loads it again"),
        ),
        (false, Some(true)) => findings.push(
            Finding::warning("komorebi is not running, but a script of it is still in KWin")
                .hint("starting komorebi replaces it"),
        ),
        (false, _) => {
            findings.push(Finding::note("komorebi is not running").hint("`komorebic start`"))
        }
    }

    // the effects
    let effects = Proxy::new(
        &connection,
        "org.kde.KWin",
        "/Effects",
        "org.kde.kwin.Effects",
    )
    .await
    .ok();
    let supported = |name: &'static str| {
        let effects = effects.clone();
        async move {
            let effects = effects?;
            within(effects.call::<_, _, bool>("isEffectSupported", &(name,)))
                .await
                .and_then(Result::ok)
        }
    };

    // komorebi's border effect puts an object on KWin's name once it is loaded
    let colours_on_bus = match Proxy::new(
        &connection,
        "org.kde.KWin",
        "/org/komorebi/BorderColours",
        "org.freedesktop.DBus.Introspectable",
    )
    .await
    {
        Ok(proxy) => within(proxy.call::<_, _, String>("Introspect", &()))
            .await
            .and_then(Result::ok)
            .is_some_and(|xml| xml.contains("org.komorebi.BorderColours1")),
        Err(_) => false,
    };

    findings.extend(border_effect_findings(
        supported(BORDER_EFFECT).await,
        &kwin_env,
        wants,
        colours_on_bus,
    ));
    findings.extend(animation_effect_findings(
        supported(ANIMATION_EFFECT).await,
        &kwin_env,
        wants,
    ));

    // the shell, for wallpapers
    if wants.wallpapers {
        if has_owner("org.kde.plasmashell").await == Some(true) {
            findings.push(Finding::ok("the Plasma shell is there, for the wallpapers"));
        } else {
            findings.push(
                Finding::warning("the Plasma shell is not on the bus: wallpapers will not be set")
                    .hint("komorebi asks for them again for half a minute after it starts, for a shell that is late"),
            );
        }
    }

    findings
}

/// Returns what starts komorebi at login: an autostart entry
/// (`~/.config/autostart/komorebi.desktop`), else a systemd user unit (the user's, then
/// `/etc/systemd/user`, then NixOS's), else a [`Level::Note`] that nothing does.
fn autostart_findings() -> Vec<Finding> {
    let entry = dirs::config_dir().map(|d| d.join("autostart/komorebi.desktop"));
    let unit = [
        dirs::config_dir().map(|d| d.join("systemd/user/komorebi.service")),
        Some(PathBuf::from("/etc/systemd/user/komorebi.service")),
        Some(PathBuf::from(
            "/run/current-system/etc/systemd/user/komorebi.service",
        )),
    ];

    if let Some(entry) = entry.filter(|e| e.exists()) {
        vec![Finding::ok(format!(
            "komorebi is started at login by {}",
            entry.display()
        ))]
    } else if let Some(unit) = unit.into_iter().flatten().find(|u| u.exists()) {
        vec![Finding::ok(format!(
            "komorebi is started at login by {}",
            unit.display()
        ))]
    } else {
        vec![Finding::note("komorebi is not started at login").hint("`komorebic enable-autostart`")]
    }
}

/// Returns the findings about the desktop komorebi runs on, for a configuration that asks for
/// `wants`.
///
/// It looks, in this order, at: the session (KDE Plasma, Wayland); the session bus and KWin on it,
/// and KWin's version (KWin 6 is needed); whether komorebi is running and its script is in KWin;
/// the border and animation effects, and whether each is komorebi's build; the Plasma shell, when
/// there are wallpapers; and what starts komorebi at login. No session bus, or no KWin on it, is a
/// [`Level::Error`], and the findings about the desktop stop there; the one about what starts
/// komorebi comes last in any case.
///
/// Where KWin looks for effects is read from KWin's environment (`/proc/<pid>/environ` of the
/// process that owns `org.kde.KWin`), so the answer is KWin's and not that of the shell the command
/// is typed in. Every D-Bus call has two seconds to answer.
///
/// # Panics
///
/// Panics when called from within a tokio runtime: it builds a runtime and blocks on it.
/// `komorebic` calls it from its synchronous `main`.
///
/// # Example
///
/// Covered by `komorebic/tests/linux_check.rs::without_a_session_bus_the_desktop_is_an_error` and the
/// `vm-check` VM suite. The example needs a desktop session, so it is compiled and not run here:
///
/// ```rust,no_run
/// use komorebi::check::Level;
/// use komorebi::check::Report;
/// use komorebi::check::Wants;
/// use komorebi::check::check_environment;
///
/// let mut report = Report::default();
/// report.push("The desktop", check_environment(&Wants::default()));
///
/// assert_eq!(report.has_errors(), report.count(Level::Error) > 0);
/// ```
pub fn check_environment(wants: &Wants) -> Vec<Finding> {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build();

    let mut findings = match runtime {
        Ok(runtime) => runtime.block_on(desktop_findings(wants)),
        Err(error) => vec![Finding::error(format!("could not start to look: {error}"))],
    };

    findings.extend(autostart_findings());
    findings
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    fn texts(findings: &[Finding]) -> String {
        findings
            .iter()
            .map(|f| {
                format!(
                    "{:?}: {} {}",
                    f.level,
                    f.message,
                    f.hint.clone().unwrap_or_default()
                )
            })
            .collect::<Vec<_>>()
            .join("\n")
    }

    #[test]
    fn a_bad_line_is_shown_where_it_is() {
        let raw = "{\n  \"border\": true,\n  \"border_width\": ,\n  \"x\": 1\n}\n";
        let error = serde_json::from_str::<serde_json::Value>(raw).unwrap_err();
        let shown = snippet(raw, error.line(), error.column());

        assert!(shown.contains("3 |   \"border_width\": ,"), "{shown}");
        assert!(
            shown.lines().last().unwrap().trim_end().ends_with('^'),
            "{shown}"
        );
        // two lines of what came before it, and none of what comes after
        assert!(shown.contains("1 | {"), "{shown}");
        assert!(!shown.contains("\"x\": 1"), "{shown}");
    }

    #[test]
    fn an_error_at_the_end_of_the_file_is_shown_too() {
        let raw = "{ \"border\": true,";
        let error = serde_json::from_str::<serde_json::Value>(raw).unwrap_err();
        let shown = snippet(raw, error.line(), error.column());
        assert!(shown.contains("1 | { \"border\": true,"), "{shown}");
    }

    #[test]
    fn the_nearest_key_is_offered_for_a_typo() {
        let known: BTreeSet<String> = ["border", "border_width", "animation"]
            .map(String::from)
            .into();
        assert_eq!(
            closest("bordr", known.iter()).map(String::as_str),
            Some("border")
        );
        assert_eq!(
            closest("animtion", known.iter()).map(String::as_str),
            Some("animation")
        );
        assert_eq!(closest("something_else_entirely", known.iter()), None);
    }

    fn known() -> KnownKeys {
        KnownKeys {
            root: ["border", "monitors", "tabbed_applications"]
                .map(String::from)
                .into(),
            monitor: ["workspaces", "window_hiding_position"]
                .map(String::from)
                .into(),
            workspace: ["name", "layout"].map(String::from).into(),
        }
    }

    #[test]
    fn keys_for_another_platform_are_told_apart_from_mistakes() {
        let value = serde_json::json!({
            "$schema": "https://example.com/schema",
            "border": true,
            "border_style": "System",
            "stackbar": {},
            "tabbed_applications": [],
            "bordr": true,
            "monitors": [ { "window_hiding_position": "BottomRight", "workspaces": [
                { "name": "a", "layout": "BSP", "custom_layout": "x.json", "layuot": "BSP" }
            ] } ]
        });

        let findings = check_keys(&value, &known());
        let text = texts(&findings);

        // not mistakes: said once, as a note
        let note = findings.iter().find(|f| f.level == Level::Note).unwrap();
        for key in [
            "border_style",
            "stackbar",
            "tabbed_applications",
            "monitors[0].window_hiding_position",
        ] {
            assert!(
                note.message.contains(key),
                "{key} is not in: {}",
                note.message
            );
        }
        assert!(!text.contains("$schema"), "{text}");

        // a typo, with the key that was meant
        assert!(
            text.contains("`bordr` is not a setting") && text.contains("did you mean `border`?"),
            "{text}"
        );
        assert!(
            text.contains("`monitors[0].workspaces[0].layuot`")
                && text.contains("did you mean `layout`?"),
            "{text}"
        );

        // Windows' custom layouts: a warning that they are not here yet
        assert!(
            text.contains("`custom_layout`") && text.contains("not supported on KDE yet"),
            "{text}"
        );
    }

    #[test]
    fn a_file_that_is_not_json_is_an_error_with_the_line() {
        let dir = std::env::temp_dir().join(format!("kmc-bad-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("komorebi.json");
        std::fs::write(&path, "{\n  \"border\": tru,\n}\n").unwrap();

        let (findings, _) = check_configuration(&path);
        assert_eq!(findings.len(), 1);
        assert_eq!(findings[0].level, Level::Error);
        assert!(
            findings[0].message.contains("not valid JSON"),
            "{}",
            findings[0].message
        );
        assert!(
            findings[0].message.contains("2 |   \"border\": tru,"),
            "{}",
            findings[0].message
        );

        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn a_value_komorebi_does_not_know_is_an_error_with_the_line() {
        let dir = std::env::temp_dir().join(format!("kmc-value-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("komorebi.json");
        std::fs::write(
            &path,
            "{\n  \"cross_boundary_behaviour\": \"Sideways\"\n}\n",
        )
        .unwrap();

        let (findings, _) = check_configuration(&path);
        assert_eq!(findings[0].level, Level::Error, "{}", texts(&findings));
        assert!(
            findings[0].message.contains("Sideways") || findings[0].message.contains("variant"),
            "{}",
            findings[0].message
        );
        assert!(
            findings[0].message.contains("2 |"),
            "{}",
            findings[0].message
        );

        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn no_file_is_a_note_not_an_error() {
        let (findings, _) = check_configuration(Path::new("/nonexistent/komorebi.json"));
        assert_eq!(findings.len(), 1);
        assert_eq!(findings[0].level, Level::Note);
    }

    #[test]
    fn a_workspace_that_is_only_a_name_is_warned_about() {
        let dir = std::env::temp_dir().join(format!("kmc-ws-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("komorebi.json");
        std::fs::write(
            &path,
            r#"{ "border": true, "animation": { "enabled": true },
                 "monitors": [ { "workspaces": [ { "name": "chat" }, { "name": "code", "layout": "BSP" } ] } ] }"#,
        )
        .unwrap();

        let (findings, wants) = check_configuration(&path);
        let text = texts(&findings);
        assert!(
            text.contains("workspace 0 of monitor 0 (\"chat\") has no `layout`"),
            "{text}"
        );
        assert!(!text.contains("workspace 1"), "{text}");
        assert!(wants.borders);
        assert_eq!(wants.animation.as_deref(), Some("Linear"));

        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn what_the_file_asks_of_the_desktop_is_returned() {
        let dir = std::env::temp_dir().join(format!("kmc-wants-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("komorebi.json");
        let wants = |file: &str| {
            std::fs::write(&path, file).unwrap();
            check_configuration(&path).1
        };

        let nothing = wants("{}");
        assert!(!nothing.borders && nothing.animation.is_none() && !nothing.wallpapers);

        // no style is the default style, which the stock effect cannot do
        let default = wants(r#"{ "animation": { "enabled": true } }"#);
        assert_eq!(default.animation.as_deref(), Some("Linear"));
        let named = wants(r#"{ "animation": { "enabled": true, "style": "EaseOutExpo" } }"#);
        assert_eq!(named.animation.as_deref(), Some("EaseOutExpo"));

        // styles for each kind of animation: the one that moves windows is the one that counts
        let per_kind = wants(
            r#"{ "animation": { "enabled": { "movement": true },
                                "style": { "movement": "EaseInSine" } } }"#,
        );
        assert_eq!(per_kind.animation.as_deref(), Some("EaseInSine"));
        let no_style =
            wants(r#"{ "animation": { "enabled": { "movement": true }, "style": {} } }"#);
        assert_eq!(no_style.animation.as_deref(), Some("Linear"));
        assert!(
            wants(r#"{ "animation": { "enabled": { "movement": false } } }"#)
                .animation
                .is_none()
        );

        let off = wants(r#"{ "animation": { "enabled": false, "style": "EaseInSine" } }"#);
        assert!(off.animation.is_none());
        assert!(!wants(r#"{ "border": false }"#).borders);
        assert!(wants(r#"{ "border": true }"#).borders);

        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn placement_rules_that_are_not_used_while_windows_are_appended_are_noted() {
        let dir = std::env::temp_dir().join(format!("kmc-placement-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("komorebi.json");
        // "a" appends because the file says so, "b" has no rules to leave unused, and "c" has
        // rules and creates containers, whatever the file says
        std::fs::write(
            &path,
            r#"{
                "window_container_behaviour": "Append",
                "monitors": [ { "workspaces": [
                    { "name": "a", "layout": "BSP", "initial_window_placement_rules": "Primary" },
                    { "name": "b", "layout": "BSP" },
                    { "name": "c", "layout": "BSP", "window_container_behaviour": "Create",
                      "initial_window_placement_rules": "Last" }
                ] } ]
            }"#,
        )
        .unwrap();

        let text = texts(&check_configuration(&path).0);
        assert!(
            text.contains(
                "workspace 0 of monitor 0 has initial_window_placement_rules, which are not used"
            ),
            "{text}"
        );
        assert!(!text.contains("workspace 1"), "{text}");
        assert!(!text.contains("workspace 2"), "{text}");

        // on its own, a workspace that appends is told the same
        std::fs::write(
            &path,
            r#"{ "monitors": [ { "workspaces": [
                { "name": "a", "layout": "BSP", "window_container_behaviour": "Append",
                  "initial_window_placement_rules": 3 }
            ] } ] }"#,
        )
        .unwrap();
        let text = texts(&check_configuration(&path).0);
        assert!(
            text.contains("Note: workspace 0 of monitor 0 has initial_window_placement_rules"),
            "{text}"
        );

        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn a_wallpaper_that_is_not_there_and_a_rules_file_that_is_not_there_are_found() {
        let dir = std::env::temp_dir().join(format!("kmc-files-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("komorebi.json");
        std::fs::write(
            &path,
            r#"{ "app_specific_configuration_path": "/nonexistent/applications.json",
                 "monitors": [ { "wallpaper": { "path": "/nonexistent/sea.png" },
                                 "workspaces": [ { "name": "a", "layout": "BSP" } ] } ] }"#,
        )
        .unwrap();

        let (findings, wants) = check_configuration(&path);
        let text = texts(&findings);
        assert!(text.contains("Warning: the application specific configuration /nonexistent/applications.json does not exist"), "{text}");
        assert!(
            text.contains("Error: the wallpaper /nonexistent/sea.png is not a file"),
            "{text}"
        );
        assert!(wants.wallpapers);

        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn the_keys_come_from_the_schema_at_every_level() {
        let Some(known) = known_keys() else {
            return; // built without the schema
        };

        assert!(known.root.contains("border") && known.root.contains("monitors"));
        assert!(known.monitor.contains("workspaces"));
        assert!(
            known.workspace.contains("layout")
                && known.workspace.contains("initial_window_placement_rules")
        );
        assert!(!known.workspace.contains("custom_layout"));
        assert!(!known.root.contains("border_style"));
    }

    #[test]
    fn a_whole_example_configuration_has_nothing_the_check_does_not_know() {
        let Some(known) = known_keys() else {
            return;
        };
        let example = include_str!("../../docs/linux/komorebi.example.json");
        let value: serde_json::Value = serde_json::from_str(example).unwrap();

        let findings = check_keys(&value, &known);
        let mistakes: Vec<_> = findings
            .iter()
            .filter(|f| f.level >= Level::Warning)
            .collect();
        assert!(mistakes.is_empty(), "{}", texts(&findings));
    }

    #[test]
    fn the_effects_are_told_apart_by_what_they_contain() {
        let dir = std::env::temp_dir().join(format!("kmc-effects-{}", std::process::id()));
        let stock = dir.join("system/kwin/effects/kwin4_effect_geometry_change/contents/code");
        let user = dir.join("home/kwin/effects/kwin4_effect_geometry_change/contents/code");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&stock).unwrap();
        std::fs::create_dir_all(&user).unwrap();
        std::fs::write(
            stock.join("main.js"),
            "this.curve = QEasingCurve[effect.readConfig(\"Curve\", \"OutExpo\")];",
        )
        .unwrap();
        std::fs::write(user.join("main.js"), "curve: QEasingCurve.OutExpo").unwrap();

        let env: HashMap<String, String> = [
            (
                "XDG_DATA_HOME".to_string(),
                dir.join("home").display().to_string(),
            ),
            (
                "XDG_DATA_DIRS".to_string(),
                dir.join("system").display().to_string(),
            ),
        ]
        .into();

        let wants = Wants {
            animation: Some("Linear".into()),
            ..Wants::default()
        };
        let findings = animation_effect_findings(Some(true), &env, &wants);
        let text = texts(&findings);

        // the user's copy comes first, is the stock effect, and hides the one that has the curve
        assert!(text.contains("Warning: the animation effect is the stock one: `animation.style` \"Linear\" is ignored"), "{text}");
        assert!(
            text.contains("shadows") && text.contains("system/kwin/effects"),
            "{text}"
        );

        // without the user's copy it is komorebi's build
        std::fs::remove_file(user.join("main.js")).unwrap();
        let findings = animation_effect_findings(Some(true), &env, &wants);
        assert_eq!(findings.len(), 1, "{}", texts(&findings));
        assert_eq!(findings[0].level, Level::Ok);

        // not installed: a warning when animations are on, a note when they are not
        assert_eq!(
            animation_effect_findings(Some(false), &env, &wants)[0].level,
            Level::Warning
        );
        assert_eq!(
            animation_effect_findings(None, &env, &Wants::default())[0].level,
            Level::Note
        );

        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn the_border_effect_is_komorebis_when_it_has_the_colours_interface() {
        let dir = std::env::temp_dir().join(format!("kmc-border-{}", std::process::id()));
        let plugins = dir.join("kwin/effects/plugins");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&plugins).unwrap();

        let env: HashMap<String, String> =
            [("QT_PLUGIN_PATH".to_string(), dir.display().to_string())].into();
        let wants = Wants {
            borders: true,
            ..Wants::default()
        };

        std::fs::write(
            plugins.join("kwin4_effect_shapecorners.so"),
            b"\x7fELF stock effect",
        )
        .unwrap();
        let findings = border_effect_findings(Some(true), &env, &wants, false);
        assert_eq!(findings[0].level, Level::Warning, "{}", texts(&findings));

        std::fs::write(
            plugins.join("kwin4_effect_shapecorners.so"),
            b"\x7fELF org.komorebi.BorderColours1 more",
        )
        .unwrap();
        let findings = border_effect_findings(Some(true), &env, &wants, false);
        assert_eq!(findings[0].level, Level::Ok, "{}", texts(&findings));

        // or it is on the bus
        std::fs::remove_file(plugins.join("kwin4_effect_shapecorners.so")).unwrap();
        assert_eq!(
            border_effect_findings(Some(true), &HashMap::new(), &wants, true)[0].level,
            Level::Ok
        );

        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn the_report_reads_as_text_and_counts() {
        let mut report = Report::default();
        report.push(
            "Configuration",
            vec![
                Finding::ok("fine"),
                Finding::warning("a thing\nover two lines").hint("do this"),
            ],
        );
        report.push("Desktop", vec![Finding::error("broken")]);

        let text = report.render();
        assert!(text.contains("Configuration\n  ok      fine\n  warning a thing\n          over two lines\n          do this\n"), "{text}");
        assert!(text.contains("Desktop\n  error   broken\n"), "{text}");
        assert!(text.ends_with("1 error(s), 1 warning(s)\n"), "{text}");
        assert!(report.has_errors());
    }
}
