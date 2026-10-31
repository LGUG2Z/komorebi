//! The key binding subcommands: `export-kde-shortcuts` and `hotkeys`.
//!
//! `export-kde-shortcuts` and `hotkeys compile` are front ends over [`komorebi_hotkeys`]: they read
//! a key binding file ([`komorebi_hotkeys::parse`]), sort its bindings into KDE shortcut files and
//! the engine's ([`komorebi_hotkeys::plan::plan`]), print what the crate has to say about the file,
//! and write the shortcut files ([`komorebi_hotkeys::kde::write`]). `export-kde-shortcuts` does
//! this for an skhdrc, as static shortcuts only; `hotkeys compile` for a file of any of the three
//! formats, in any [`PlanMode`].
//!
//! The other `hotkeys` subcommands are about the engine of the daemon. `hotkeys state` prints the
//! keys komorebi has registered with KDE ([`komorebi::kwin::hotkeys::registered`]) and what the
//! engine is doing (the daemon's answer to `HotkeysState`), `hotkeys reload` has the daemon read
//! its key binding file again (`ReloadHotkeys`), and `hotkeys release` unregisters every key
//! komorebi registered ([`komorebi::kwin::hotkeys::release`]), with or without a daemon.

use super::HOME_DIR;
use clap::Parser;
use clap::Subcommand;
use clap::ValueEnum;
use color_eyre::eyre;
use komorebi_client::SocketMessage;
use komorebi_client::check::Finding;
use komorebi_hotkeys::Diagnostic;
use komorebi_hotkeys::Format;
use komorebi_hotkeys::ParseOptions;
use komorebi_hotkeys::Parsed;
use komorebi_hotkeys::Severity;
use komorebi_hotkeys::kde;
use komorebi_hotkeys::plan::Plan;
use komorebi_hotkeys::plan::PlanMode;
use komorebi_hotkeys::plan::PlanOptions;
use std::path::Path;
use std::path::PathBuf;
use std::process::Command;
use std::process::Stdio;

#[derive(Parser)]
pub struct ExportKdeShortcuts {
    /// The skhdrc to translate [default: skhdrc in the komorebi config directory]
    #[clap(short, long)]
    skhdrc: Option<PathBuf>,
    /// Where to write the shortcut files [default: $XDG_DATA_HOME/kglobalaccel]
    #[clap(short, long)]
    output: Option<PathBuf>,
    /// Activate shortcuts that are new straight away, instead of at the next login
    #[clap(long)]
    reload: bool,
    /// Show the shortcuts that would be written without writing anything
    #[clap(long)]
    dry_run: bool,
}

#[derive(Parser)]
pub struct Hotkeys {
    #[clap(subcommand)]
    command: HotkeysCommand,
}

#[derive(Subcommand)]
enum HotkeysCommand {
    /// Plan the bindings of a key binding file, and write those that are static shortcuts as KDE
    /// shortcut files
    Compile(Compile),
    /// Show the keys komorebi has registered with KDE, and what the engine of the daemon is doing
    State,
    /// Have the daemon read its key binding file again
    Reload,
    /// Unregister every key komorebi has registered with KDE, whether a daemon runs or not
    Release,
}

#[derive(Parser)]
struct Compile {
    /// The key binding file [default: the first of skhdrc, whkdrc and sxhkdrc in the komorebi
    /// config directory]
    file: Option<PathBuf>,
    /// The format of the file [default: from the end of its name: skhdrc, whkdrc or sxhkdrc]
    #[clap(short, long, value_enum)]
    format: Option<KeyFormat>,
    /// Which bindings are static shortcuts: none (engine), every binding a shortcut file can
    /// express (auto), or only those, leaving the others out (static)
    #[clap(short, long, value_enum, default_value_t = PlanMode::default().into())]
    mode: KeyPlanMode,
    /// Where to write the shortcut files [default: $XDG_DATA_HOME/kglobalaccel]
    #[clap(short, long)]
    output: Option<PathBuf>,
    /// Write nothing, and fail, when a diagnostic is a warning or an error: something in the
    /// file is not realized the way its own hotkey daemon realizes it
    #[clap(long)]
    strict: bool,
    /// Activate shortcuts that are new straight away, instead of at the next login
    #[clap(long)]
    reload: bool,
    /// Show the shortcuts that would be written without writing anything
    #[clap(long)]
    dry_run: bool,
}

/// [`Format`], for the command line
#[derive(Clone, Copy, ValueEnum)]
enum KeyFormat {
    Skhd,
    Whkd,
    Sxhkd,
}

impl From<KeyFormat> for Format {
    fn from(format: KeyFormat) -> Self {
        match format {
            KeyFormat::Skhd => Format::Skhd,
            KeyFormat::Whkd => Format::Whkd,
            KeyFormat::Sxhkd => Format::Sxhkd,
        }
    }
}

/// [`PlanMode`], for the command line
#[derive(Clone, Copy, ValueEnum)]
enum KeyPlanMode {
    Auto,
    Engine,
    Static,
}

impl From<PlanMode> for KeyPlanMode {
    fn from(mode: PlanMode) -> Self {
        match mode {
            PlanMode::Auto => KeyPlanMode::Auto,
            PlanMode::Engine => KeyPlanMode::Engine,
            PlanMode::Static => KeyPlanMode::Static,
        }
    }
}

impl From<KeyPlanMode> for PlanMode {
    fn from(mode: KeyPlanMode) -> Self {
        match mode {
            KeyPlanMode::Auto => PlanMode::Auto,
            KeyPlanMode::Engine => PlanMode::Engine,
            KeyPlanMode::Static => PlanMode::Static,
        }
    }
}

/// What to export, and how
struct Export {
    file: PathBuf,
    format: Format,
    mode: PlanMode,
    output: Option<PathBuf>,
    strict: bool,
    reload: bool,
    dry_run: bool,
}

/// `komorebic export-kde-shortcuts`: the bindings of an skhdrc that can be KDE shortcut files
pub fn export_kde_shortcuts(args: ExportKdeShortcuts) -> eyre::Result<()> {
    export(Export {
        file: args.skhdrc.unwrap_or_else(|| HOME_DIR.join("skhdrc")),
        format: Format::Skhd,
        mode: PlanMode::Static,
        output: args.output,
        strict: false,
        reload: args.reload,
        dry_run: args.dry_run,
    })
}

/// `komorebic hotkeys`
pub fn hotkeys(args: Hotkeys) -> eyre::Result<()> {
    match args.command {
        HotkeysCommand::Compile(args) => {
            let file = match args.file {
                Some(file) => file,
                None => find_file(&HOME_DIR)?,
            };
            let format = match args.format {
                Some(format) => format.into(),
                None => format_of(&file)?,
            };
            export(Export {
                file,
                format,
                mode: args.mode.into(),
                output: args.output,
                strict: args.strict,
                reload: args.reload,
                dry_run: args.dry_run,
            })
        }
        HotkeysCommand::State => {
            let registered = komorebi::kwin::hotkeys::registered().map_err(|error| {
                eyre::Report::msg(format!("could not ask KDE for the keys: {error}"))
            })?;
            // no daemon: no engine
            let engine = komorebi_client::send_query(&SocketMessage::HotkeysState)
                .ok()
                .and_then(|answer| serde_json::from_str(&answer).ok())
                .unwrap_or(serde_json::Value::Null);
            let state = serde_json::json!({ "registered": registered, "engine": engine });
            println!("{}", serde_json::to_string_pretty(&state)?);
            Ok(())
        }
        HotkeysCommand::Reload => {
            komorebi_client::send_message(&SocketMessage::ReloadHotkeys)?;
            Ok(())
        }
        HotkeysCommand::Release => {
            let released = komorebi::kwin::hotkeys::release().map_err(|error| {
                eyre::Report::msg(format!("could not release the keys: {error}"))
            })?;
            if released.is_empty() {
                println!("komorebi had no key registered");
            } else {
                println!("released {} keys: {}", released.len(), released.join(", "));
            }
            Ok(())
        }
    }
}

const FORMATS: [Format; 3] = [Format::Skhd, Format::Whkd, Format::Sxhkd];

/// The first key binding file of the three formats in `dir`, and its format
fn find(dir: &Path) -> Option<(PathBuf, Format)> {
    FORMATS
        .into_iter()
        .map(|format| (dir.join(format.file_name()), format))
        .find(|(path, _)| path.is_file())
}

/// The first key binding file of the three formats in `dir`
fn find_file(dir: &Path) -> eyre::Result<PathBuf> {
    find(dir).map(|(path, _)| path).ok_or_else(|| {
        eyre::Report::msg(format!(
            "there is no skhdrc, whkdrc or sxhkdrc in {}: name the file to read",
            dir.display()
        ))
    })
}

/// The format a file's name ends with
fn format_of(file: &Path) -> eyre::Result<Format> {
    let name = file
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or_default();
    FORMATS
        .into_iter()
        .find(|format| name.ends_with(format.file_name()))
        .ok_or_else(|| {
            eyre::Report::msg(format!(
                "{} does not end with skhdrc, whkdrc or sxhkdrc: give its --format",
                file.display()
            ))
        })
}

/// Where the shortcut files go when no directory is given: `kglobalaccel` in the data directory
fn shortcut_dir() -> Option<PathBuf> {
    dirs::data_dir().map(|dir| dir.join("kglobalaccel"))
}

/// What reading and planning a file have to say about it, in the order of the file
fn diagnostics<'a>(parsed: &'a Parsed, plan: &'a Plan) -> Vec<&'a Diagnostic> {
    let mut diagnostics: Vec<&Diagnostic> =
        parsed.diagnostics.iter().chain(&plan.diagnostics).collect();
    diagnostics.sort_by_key(|diagnostic| diagnostic.span.start);
    diagnostics
}

fn export(export: Export) -> eyre::Result<()> {
    let file = &export.file;
    let text = std::fs::read_to_string(file).map_err(|error| {
        eyre::Report::msg(format!("could not read {}: {error}", file.display()))
    })?;
    let parsed = komorebi_hotkeys::parse(export.format, &text, &ParseOptions::default());
    let plan = komorebi_hotkeys::plan::plan(&parsed.keymap, &PlanOptions { mode: export.mode });

    let diagnostics = diagnostics(&parsed, &plan);
    for diagnostic in &diagnostics {
        eprintln!("{}: {diagnostic}", file.display());
    }
    if export.strict
        && diagnostics
            .iter()
            .any(|diagnostic| diagnostic.severity >= Severity::Warning)
    {
        eyre::bail!(
            "{} is not realized as it says (the warnings and errors above), and --strict is set",
            file.display()
        );
    }

    for shortcut in &plan.static_files {
        println!("{:<24} {}", shortcut.combo.to_string(), shortcut.command);
    }
    let engine: usize = plan.engine.modes.iter().map(|m| m.bindings.len()).sum();
    let files = kde::desktop_files(&plan.static_files);
    let output = export
        .output
        .unwrap_or_else(|| shortcut_dir().expect("there is no data directory"));

    if export.dry_run {
        let diff = kde::diff(&output, &files)?;
        println!(
            "{} shortcuts would be written to {} ({} new, {} changed, {} removed), {} bindings need the engine",
            files.len(),
            output.display(),
            diff.added.len(),
            diff.changed.len(),
            diff.removed.len(),
            engine
        );
        return Ok(());
    }

    let written = kde::write(&output, &files)?;
    println!(
        "wrote {} shortcuts to {} ({} new, {} changed, {} removed), {} bindings need the engine",
        files.len(),
        output.display(),
        written.added.len(),
        written.changed.len(),
        written.removed.len(),
        engine
    );

    // KWin 6.8 runs the global shortcut daemon itself and reads these files when it starts. A
    // shortcut it has not met can be handed over now; one it already runs keeps what it loaded
    // until the next login.
    if export.reload {
        for name in written.added.iter().chain(&written.unchanged) {
            let registered = Command::new("busctl")
                .args([
                    "--user",
                    "call",
                    "org.kde.kglobalaccel",
                    "/kglobalaccel",
                    "org.kde.KGlobalAccel",
                    "doRegister",
                    "as",
                    "4",
                    name,
                    "_launch",
                    "",
                    "",
                ])
                .stdout(Stdio::null())
                .status()
                .is_ok_and(|status| status.success());
            if !registered {
                eyre::bail!("could not register {name} with KWin: is this a Plasma session?");
            }
        }
        println!(
            "{} shortcuts are active now",
            written.added.len() + written.unchanged.len()
        );
        if !(written.changed.is_empty() && written.removed.is_empty()) {
            println!(
                "{} changed and {} removed ones take effect at your next login",
                written.changed.len(),
                written.removed.len()
            );
        }
    } else if !files.is_empty() || !written.removed.is_empty() {
        println!("they take effect at your next login; --reload activates new ones straight away");
    }

    Ok(())
}

/// `n` and the noun, in the singular when there is one
fn count(n: usize, singular: &str, plural: &str) -> String {
    format!("{n} {}", if n == 1 { singular } else { plural })
}

/// The first five diagnostics of a kind, one to a line, for the hint of a finding
fn lines(diagnostics: &[&Diagnostic], severity: Severity) -> (usize, String) {
    let of_kind: Vec<String> = diagnostics
        .iter()
        .filter(|diagnostic| diagnostic.severity == severity)
        .map(|diagnostic| diagnostic.to_string())
        .collect();
    (
        of_kind.len(),
        of_kind.into_iter().take(5).collect::<Vec<_>>().join("\n"),
    )
}

/// The key bindings, for `komorebic check`: the file `hotkeys compile` would read, what reading
/// and planning it have to say, and whether the shortcut files there are the ones it would write
/// for it.
pub fn check(home: &Path) -> Vec<Finding> {
    check_in(home, shortcut_dir().as_deref(), PlanMode::default())
}

/// [`check`], for the key binding file in `home`, planned in `mode`, against the shortcut files in
/// `shortcuts`
fn check_in(home: &Path, shortcuts: Option<&Path>, mode: PlanMode) -> Vec<Finding> {
    let Some((file, format)) = find(home) else {
        return vec![
            Finding::note(format!(
                "there is no skhdrc, whkdrc or sxhkdrc in {}",
                home.display()
            ))
            .hint(
                "key bindings are KDE global shortcuts: they come from a key binding file through `komorebic hotkeys compile`, or are set up in System Settings",
            ),
        ];
    };
    let text = match std::fs::read_to_string(&file) {
        Ok(text) => text,
        Err(error) => {
            return vec![Finding::warning(format!(
                "could not read {}: {error}",
                file.display()
            ))];
        }
    };

    let parsed = komorebi_hotkeys::parse(format, &text, &ParseOptions::default());
    let plan = komorebi_hotkeys::plan::plan(&parsed.keymap, &PlanOptions { mode });
    let diagnostics = diagnostics(&parsed, &plan);

    let bindings: usize = parsed.keymap.modes.iter().map(|m| m.bindings.len()).sum();
    let engine: usize = plan.engine.modes.iter().map(|m| m.bindings.len()).sum();
    let mut findings = vec![Finding::ok(format!(
        "{} has {}: {} as shortcut files, {} in the engine",
        file.display(),
        count(bindings, "key binding", "key bindings"),
        plan.static_files.len(),
        engine
    ))];

    let (errors, shown) = lines(&diagnostics, Severity::Error);
    if errors > 0 {
        findings.push(
            Finding::warning(format!(
                "{} of it cannot be read and left out",
                count(errors, "line", "lines")
            ))
            .hint(shown),
        );
    }
    let (warnings, shown) = lines(&diagnostics, Severity::Warning);
    if warnings > 0 {
        findings.push(
            Finding::note(format!(
                "{} not realized as {} would",
                count(warnings, "binding is", "bindings are"),
                format.file_name().trim_end_matches("rc")
            ))
            .hint(shown),
        );
    }

    let files = kde::desktop_files(&plan.static_files);
    match shortcuts.map(|dir| kde::diff(dir, &files)) {
        Some(Ok(diff)) if diff.added.is_empty() && diff.changed.is_empty() && diff.removed.is_empty() => {
            findings.push(Finding::ok(if files.is_empty() {
                "no shortcut files are needed, and komorebi has written none"
            } else {
                "the shortcut files are the ones `komorebic hotkeys compile` writes"
            }));
        }
        Some(Ok(diff)) => findings.push(
            Finding::warning(format!(
                "the shortcut files are not the ones `komorebic hotkeys compile` would write: {} to add, {} to change, {} to remove",
                diff.added.len(),
                diff.changed.len(),
                diff.removed.len()
            ))
            .hint("`komorebic hotkeys compile --reload`"),
        ),
        Some(Err(error)) => findings.push(Finding::warning(format!(
            "could not look at the shortcut files: {error}"
        ))),
        None => {}
    }

    findings
}

#[cfg(test)]
mod tests {
    use super::*;
    use komorebi_client::check::Level;

    /// A directory of its own, removed when it goes out of scope
    struct Scratch(PathBuf);

    impl Scratch {
        fn new() -> Self {
            static NEXT: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);
            let dir = std::env::temp_dir().join(format!(
                "komorebic-hotkeys-unit-{}-{}",
                std::process::id(),
                NEXT.fetch_add(1, std::sync::atomic::Ordering::SeqCst)
            ));
            let _ = std::fs::remove_dir_all(&dir);
            std::fs::create_dir_all(&dir).unwrap();
            Self(dir)
        }

        fn file(&self, name: &str, text: &str) -> &Self {
            std::fs::write(self.0.join(name), text).unwrap();
            self
        }

        fn shortcuts(&self) -> PathBuf {
            self.0.join("shortcuts")
        }
    }

    impl Drop for Scratch {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    const SKHDRC: &str = "alt - h : komorebic focus left\nalt - l : komorebic focus right\n";

    /// The level and message of each finding
    fn said(findings: &[Finding]) -> Vec<(Level, String)> {
        findings
            .iter()
            .map(|finding| (finding.level, finding.message.clone()))
            .collect()
    }

    #[test]
    fn without_a_key_binding_file_there_is_a_note_that_says_where_they_come_from() {
        let home = Scratch::new();

        let findings = check_in(&home.0, Some(&home.shortcuts()), PlanMode::Auto);

        assert_eq!(findings.len(), 1);
        assert_eq!(findings[0].level, Level::Note);
        assert!(
            findings[0]
                .message
                .starts_with("there is no skhdrc, whkdrc or sxhkdrc in")
        );
        assert!(
            findings[0]
                .hint
                .as_deref()
                .is_some_and(|hint| hint.contains("komorebic hotkeys compile"))
        );
    }

    #[test]
    fn the_first_file_of_the_three_formats_is_the_one_that_is_read() {
        let home = Scratch::new();
        home.file("sxhkdrc", "super + h\n    komorebic focus left\n")
            .file(
                "whkdrc",
                "alt + h : komorebic focus left\nalt + l : komorebic focus right\n",
            );

        let findings = check_in(&home.0, None, PlanMode::Engine);

        assert_eq!(findings[0].level, Level::Ok);
        assert!(
            findings[0]
                .message
                .ends_with("whkdrc has 2 key bindings: 0 as shortcut files, 2 in the engine"),
            "{}",
            findings[0].message
        );
    }

    #[test]
    fn the_bindings_are_counted_by_where_the_plan_puts_them() {
        let home = Scratch::new();
        home.file(
            "sxhkdrc",
            "super + h\n    komorebic focus left\nsuper + l\n    komorebic focus right\nsuper + w ; h\n    komorebic move left\n",
        );

        let auto = check_in(&home.0, None, PlanMode::Auto);
        let engine = check_in(&home.0, None, PlanMode::Engine);

        assert!(
            auto[0]
                .message
                .ends_with("has 3 key bindings: 2 as shortcut files, 1 in the engine"),
            "{}",
            auto[0].message
        );
        assert!(
            engine[0]
                .message
                .ends_with("has 3 key bindings: 0 as shortcut files, 3 in the engine"),
            "{}",
            engine[0].message
        );
    }

    #[test]
    fn one_binding_is_not_plural() {
        let home = Scratch::new();
        home.file("skhdrc", "alt - h : komorebic focus left\n");

        let findings = check_in(&home.0, None, PlanMode::Auto);

        assert!(
            findings[0]
                .message
                .ends_with("has 1 key binding: 1 as shortcut files, 0 in the engine"),
            "{}",
            findings[0].message
        );
    }

    #[test]
    fn shortcut_files_that_are_the_ones_of_the_plan_are_ok() {
        let home = Scratch::new();
        home.file("skhdrc", SKHDRC);
        let parsed = komorebi_hotkeys::parse(Format::Skhd, SKHDRC, &ParseOptions::default());
        let plan = komorebi_hotkeys::plan::plan(
            &parsed.keymap,
            &PlanOptions {
                mode: PlanMode::Auto,
            },
        );
        kde::write(&home.shortcuts(), &kde::desktop_files(&plan.static_files)).unwrap();

        let findings = check_in(&home.0, Some(&home.shortcuts()), PlanMode::Auto);

        assert_eq!(
            said(&findings)[1],
            (
                Level::Ok,
                "the shortcut files are the ones `komorebic hotkeys compile` writes".to_string()
            )
        );
    }

    #[test]
    fn shortcut_files_that_are_not_the_ones_of_the_plan_are_a_warning_that_says_how_many() {
        let home = Scratch::new();
        home.file("skhdrc", SKHDRC);
        let old = "alt - h : komorebic focus left\nalt - x : komorebic focus down\n";
        let parsed = komorebi_hotkeys::parse(Format::Skhd, old, &ParseOptions::default());
        let plan = komorebi_hotkeys::plan::plan(
            &parsed.keymap,
            &PlanOptions {
                mode: PlanMode::Auto,
            },
        );
        kde::write(&home.shortcuts(), &kde::desktop_files(&plan.static_files)).unwrap();

        let findings = check_in(&home.0, Some(&home.shortcuts()), PlanMode::Auto);

        assert_eq!(findings[1].level, Level::Warning);
        assert_eq!(
            findings[1].message,
            "the shortcut files are not the ones `komorebic hotkeys compile` would write: 1 to add, 0 to change, 1 to remove"
        );
        assert_eq!(
            findings[1].hint.as_deref(),
            Some("`komorebic hotkeys compile --reload`")
        );
    }

    #[test]
    fn a_file_that_needs_no_shortcut_files_and_a_directory_without_any_agree() {
        let home = Scratch::new();
        home.file("skhdrc", SKHDRC);

        let findings = check_in(&home.0, Some(&home.shortcuts()), PlanMode::Engine);

        assert_eq!(
            said(&findings)[1],
            (
                Level::Ok,
                "no shortcut files are needed, and komorebi has written none".to_string()
            )
        );
    }

    #[test]
    fn a_line_that_cannot_be_read_is_a_warning_that_shows_the_line() {
        let home = Scratch::new();
        home.file(
            "skhdrc",
            "alt - h : komorebic focus left\nalt - nosuchkey : komorebic focus up\n",
        );

        let findings = check_in(&home.0, None, PlanMode::Auto);

        assert_eq!(findings[1].level, Level::Warning);
        assert_eq!(
            findings[1].message,
            "1 line of it cannot be read and left out"
        );
        assert!(
            findings[1]
                .hint
                .as_deref()
                .is_some_and(|hint| hint.starts_with("line 2: error: ")),
            "{:?}",
            findings[1].hint
        );
    }

    #[test]
    fn a_binding_that_is_not_realized_as_its_own_daemon_would_is_a_note() {
        let home = Scratch::new();
        home.file("skhdrc", "lalt - j : komorebic focus down\n");

        let findings = check_in(&home.0, None, PlanMode::Auto);

        assert_eq!(findings[1].level, Level::Note);
        assert_eq!(
            findings[1].message,
            "1 binding is not realized as skhd would"
        );
        assert!(
            findings[1]
                .hint
                .as_deref()
                .is_some_and(|hint| hint.starts_with("line 1: warning: ")),
            "{:?}",
            findings[1].hint
        );
    }

    #[test]
    fn a_hint_shows_five_lines_at_most() {
        let home = Scratch::new();
        let text: String = (0..8)
            .map(|n| format!("lalt - {} : x\n", (b'a' + n) as char))
            .collect();
        home.file("skhdrc", &text);

        let findings = check_in(&home.0, None, PlanMode::Auto);

        assert_eq!(
            findings[1].message,
            "8 bindings are not realized as skhd would"
        );
        assert_eq!(findings[1].hint.as_deref().unwrap().lines().count(), 5);
    }
}
