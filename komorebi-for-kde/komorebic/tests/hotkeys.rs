//! `export-kde-shortcuts` and `hotkeys compile` as a person runs them: the shortcut files they
//! write for a key binding file, what they print, and when they refuse.

use std::path::Path;
use std::path::PathBuf;
use std::process::Command;
use std::process::Output;
use std::sync::atomic::AtomicU32;
use std::sync::atomic::Ordering;

/// A directory for one test: a komorebi config directory in `config`, and shortcut files in
/// `shortcuts`
struct Scratch(PathBuf);

impl Scratch {
    fn new() -> Self {
        static NEXT: AtomicU32 = AtomicU32::new(0);
        let dir = std::env::temp_dir().join(format!(
            "komorebic-hotkeys-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::SeqCst)
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("config")).unwrap();
        Self(dir)
    }

    fn config(&self) -> PathBuf {
        self.0.join("config")
    }

    fn shortcuts(&self) -> PathBuf {
        self.0.join("shortcuts")
    }

    /// Writes `text` to the config directory as `name`, and returns its path
    fn file(&self, name: &str, text: &str) -> PathBuf {
        let path = self.config().join(name);
        std::fs::write(&path, text).unwrap();
        path
    }

    fn komorebic(&self, args: &[&str]) -> Output {
        Command::new(env!("CARGO_BIN_EXE_komorebic"))
            .args(args)
            .env("KOMOREBI_CONFIG_HOME", self.config())
            .output()
            .unwrap()
    }

    /// The names of the files in the shortcut directory, sorted
    fn written(&self) -> Vec<String> {
        let Ok(entries) = std::fs::read_dir(self.shortcuts()) else {
            return Vec::new();
        };
        let mut names: Vec<String> = entries
            .map(|entry| entry.unwrap().file_name().into_string().unwrap())
            .collect();
        names.sort();
        names
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn stdout(output: &Output) -> String {
    String::from_utf8_lossy(&output.stdout).into_owned()
}

fn stderr(output: &Output) -> String {
    String::from_utf8_lossy(&output.stderr).into_owned()
}

fn fixture(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../komorebi-hotkeys/tests/fixtures")
        .join(name)
}

const SKHDRC: &str = "\
alt - h : komorebic focus left
alt - l : komorebic focus right
lalt - j : komorebic focus down
:: resize
alt - r ; resize
resize < h : komorebic resize-axis horizontal decrease
";

#[test]
fn export_kde_shortcuts_writes_a_file_for_each_binding_a_shortcut_can_be() {
    let scratch = Scratch::new();
    scratch.file("skhdrc", SKHDRC);

    let output = scratch.komorebic(&[
        "export-kde-shortcuts",
        "--output",
        scratch.shortcuts().to_str().unwrap(),
    ]);

    assert!(output.status.success(), "{output:?}");
    assert_eq!(
        scratch.written(),
        [
            "komorebi-focus-down.desktop",
            "komorebi-focus-left.desktop",
            "komorebi-focus-right.desktop",
        ]
    );
    let left =
        std::fs::read_to_string(scratch.shortcuts().join("komorebi-focus-left.desktop")).unwrap();
    assert!(left.contains("\nX-KDE-Shortcuts=Alt+H\n"), "{left}");
    assert!(left.contains("\nExec=komorebic focus left\n"), "{left}");
    assert!(
        stdout(&output).starts_with("Alt+H                    komorebic focus left\n"),
        "{output:?}"
    );
    assert!(
        stdout(&output).contains("wrote 3 shortcuts to "),
        "{output:?}"
    );
    // the modes are left out, and lalt is either Alt: the diagnostics say so, with the file
    let diagnostics = stderr(&output);
    assert!(
        diagnostics.contains("skhdrc: line 3: warning: "),
        "{diagnostics}"
    );
    assert!(
        diagnostics.contains("skhdrc: line 5: warning: "),
        "{diagnostics}"
    );
    assert!(diagnostics.contains("is left out"), "{diagnostics}");
}

#[test]
fn a_dry_run_writes_nothing_and_says_what_a_write_would_change() {
    let scratch = Scratch::new();
    let skhdrc = scratch.file("my-bindings", SKHDRC);
    let shortcuts = scratch.shortcuts();
    std::fs::create_dir_all(&shortcuts).unwrap();
    std::fs::write(shortcuts.join("mine.desktop"), "[Desktop Entry]\n").unwrap();

    let output = scratch.komorebic(&[
        "export-kde-shortcuts",
        "--skhdrc",
        skhdrc.to_str().unwrap(),
        "--output",
        shortcuts.to_str().unwrap(),
        "--dry-run",
    ]);

    assert!(output.status.success(), "{output:?}");
    assert_eq!(scratch.written(), ["mine.desktop"]);
    assert!(
        stdout(&output).contains("3 shortcuts would be written to "),
        "{output:?}"
    );
    assert!(
        stdout(&output).contains("(3 new, 0 changed, 0 removed), 0 bindings need the engine"),
        "{output:?}"
    );
}

#[test]
fn compile_writes_the_same_shortcut_files_from_each_format() {
    let written = |file: &str| {
        let scratch = Scratch::new();
        let output = scratch.komorebic(&[
            "hotkeys",
            "compile",
            "--mode",
            "auto",
            fixture(file).to_str().unwrap(),
            "--output",
            scratch.shortcuts().to_str().unwrap(),
        ]);
        assert!(output.status.success(), "{output:?}");
        scratch.written()
    };

    let skhd = written("sample.skhdrc");

    assert_eq!(skhd.len(), 50);
    assert_eq!(skhd, written("sample.whkdrc"));
    assert_eq!(skhd, written("sample.sxhkdrc"));
}

#[test]
fn compile_without_a_file_reads_the_first_file_of_the_config_directory() {
    let scratch = Scratch::new();
    scratch.file("whkdrc", ".shell pwsh\nalt + h : komorebic focus left\n");
    scratch.file("sxhkdrc", "alt + l\n    komorebic focus right\n");

    let output = scratch.komorebic(&[
        "hotkeys",
        "compile",
        "--mode",
        "static",
        "--output",
        scratch.shortcuts().to_str().unwrap(),
    ]);

    assert!(output.status.success(), "{output:?}");
    // skhdrc, whkdrc, sxhkdrc: the whkdrc is the first that exists
    assert_eq!(scratch.written(), ["komorebi-focus-left.desktop"]);
}

#[test]
fn compile_in_engine_mode_writes_no_shortcut_and_removes_the_ones_it_wrote() {
    let scratch = Scratch::new();
    let skhdrc = scratch.file("skhdrc", SKHDRC);
    let compile = |mode: &str| {
        scratch.komorebic(&[
            "hotkeys",
            "compile",
            "--mode",
            mode,
            skhdrc.to_str().unwrap(),
            "--output",
            scratch.shortcuts().to_str().unwrap(),
        ])
    };
    assert!(compile("static").status.success());
    assert_eq!(scratch.written().len(), 3);

    let output = compile("engine");

    assert!(output.status.success(), "{output:?}");
    assert!(scratch.written().is_empty());
    assert!(
        stdout(&output).contains("wrote 0 shortcuts to "),
        "{output:?}"
    );
    assert!(
        stdout(&output).contains("(0 new, 0 changed, 3 removed), 5 bindings need the engine"),
        "{output:?}"
    );
}

#[test]
fn strict_refuses_a_file_with_a_warning_and_writes_nothing() {
    let scratch = Scratch::new();
    let skhdrc = scratch.file("skhdrc", SKHDRC);

    let output = scratch.komorebic(&[
        "hotkeys",
        "compile",
        "--strict",
        "--mode",
        "auto",
        skhdrc.to_str().unwrap(),
        "--output",
        scratch.shortcuts().to_str().unwrap(),
    ]);

    assert!(!output.status.success(), "{output:?}");
    assert!(scratch.written().is_empty());
    assert!(stderr(&output).contains("--strict is set"), "{output:?}");

    let clean = scratch.file("clean-skhdrc", "alt - h : komorebic focus left\n");
    let output = scratch.komorebic(&[
        "hotkeys",
        "compile",
        "--strict",
        "--mode",
        "auto",
        clean.to_str().unwrap(),
        "--output",
        scratch.shortcuts().to_str().unwrap(),
    ]);
    assert!(output.status.success(), "{output:?}");
    assert_eq!(scratch.written(), ["komorebi-focus-left.desktop"]);
}

#[test]
fn a_file_whose_name_says_no_format_needs_one() {
    let scratch = Scratch::new();
    let file = scratch.file("bindings.conf", "alt + h\n    komorebic focus left\n");
    let shortcuts = scratch.shortcuts();
    let compile = |extra: &[&str]| {
        let mut args = vec!["hotkeys", "compile", "--mode", "static"];
        args.extend_from_slice(extra);
        args.extend([
            file.to_str().unwrap(),
            "--output",
            shortcuts.to_str().unwrap(),
        ]);
        scratch.komorebic(&args)
    };

    let output = compile(&[]);
    assert!(!output.status.success(), "{output:?}");
    assert!(stderr(&output).contains("give its --format"), "{output:?}");

    let output = compile(&["--format", "sxhkd"]);
    assert!(output.status.success(), "{output:?}");
    assert_eq!(scratch.written(), ["komorebi-focus-left.desktop"]);
}

#[test]
fn state_and_release_without_a_session_bus_say_that_kde_cannot_be_reached() {
    let scratch = Scratch::new();
    let run = |args: &[&str]| {
        Command::new(env!("CARGO_BIN_EXE_komorebic"))
            .args(args)
            .env(
                "DBUS_SESSION_BUS_ADDRESS",
                "unix:path=/nonexistent/komorebic-test.bus",
            )
            .env("XDG_RUNTIME_DIR", &scratch.0)
            .output()
            .unwrap()
    };

    let output = run(&["hotkeys", "release"]);
    assert!(!output.status.success(), "{output:?}");
    assert!(
        stderr(&output).contains("could not release the keys"),
        "{output:?}"
    );

    let output = run(&["hotkeys", "state"]);
    assert!(!output.status.success(), "{output:?}");
    assert!(
        stderr(&output).contains("could not ask KDE for the keys"),
        "{output:?}"
    );
}

#[test]
fn reload_without_a_daemon_fails() {
    let scratch = Scratch::new();

    let output = Command::new(env!("CARGO_BIN_EXE_komorebic"))
        .args(["hotkeys", "reload"])
        .env("XDG_RUNTIME_DIR", &scratch.0)
        .output()
        .unwrap();

    assert!(!output.status.success(), "{output:?}");
}
