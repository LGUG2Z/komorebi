//! The application specific configuration against an in-memory desktop: the `applications.json`
//! files a `komorebi.json` names with `app_specific_configuration_path`, and what the rules in
//! them do to the windows that open.
//!
//! The rule lists are process-wide, so every test names the applications of its own, and each
//! loads its configuration the way the daemon does. Window ids are in the 39_000s.

use color_eyre::eyre;
use komorebi::backend::fake::FakeBackend;
use komorebi::backend::with_backend;
use komorebi::core::asc::ApplicationSpecificConfiguration;
use komorebi::core::asc::AscApplicationRulesOrSchema;
use komorebi::static_config::StaticConfig;
use komorebi::window::Window;
use komorebi::window_manager::WindowManager;
use komorebi::window_manager_event::KWinNotification;
use komorebi::window_manager_event::SystemNotification;
use komorebi::window_manager_event::WindowManagerEvent;
use std::os::unix::net::UnixListener;
use std::path::Path;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::AtomicU32;
use std::sync::atomic::Ordering;

/// A path for a file or a socket of a test, short enough for a unix socket path on macOS
fn short_path(suffix: &str) -> PathBuf {
    static NEXT: AtomicU32 = AtomicU32::new(0);
    PathBuf::from(format!(
        "/tmp/kma-{}-{}.{suffix}",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::SeqCst)
    ))
}

fn write(suffix: &str, text: &str) -> PathBuf {
    let path = short_path(suffix);
    std::fs::write(&path, text).unwrap();
    path
}

fn added(id: u32) -> WindowManagerEvent {
    WindowManagerEvent::Show(
        SystemNotification::KWin(KWinNotification::WindowAdded),
        Window::from(id),
    )
}

/// What the tests find out about a window the desktop opened
#[derive(Debug, PartialEq)]
enum Where {
    Tiled,
    Floating,
    Unmanaged,
}

struct Desktop {
    fake: Arc<FakeBackend>,
    wm: WindowManager,
}

impl Desktop {
    /// Loads a `komorebi.json` with `config` in it over a fake desktop, as the daemon does
    fn load(config: &str) -> eyre::Result<Self> {
        let fake = Arc::new(FakeBackend::with_one_monitor());
        let path = write("json", config);
        let (_sender, receiver) = crossbeam_channel::bounded(1);
        let socket = short_path("sock");

        let wm = with_backend(fake.clone(), || -> eyre::Result<WindowManager> {
            let mut wm =
                StaticConfig::preload(&path, receiver, Some(UnixListener::bind(&socket)?))?;
            wm.init()?;
            Ok(wm)
        });
        let _ = std::fs::remove_file(&socket);
        let _ = std::fs::remove_file(&path);

        Ok(Self { fake, wm: wm? })
    }

    /// Opens a window of an application, and says what became of it
    fn open(&mut self, id: u32, exe: &str, class: &str) -> Where {
        self.fake
            .open_window(id, exe, class, &format!("window {id}"));
        with_backend(self.fake.clone(), || self.wm.process_event(added(id))).unwrap();

        let workspace = self.wm.focused_workspace().unwrap();
        if workspace.containers().iter().any(|c| c.contains_window(id)) {
            Where::Tiled
        } else if workspace.floating_windows().iter().any(|w| w.id == id) {
            Where::Floating
        } else {
            Where::Unmanaged
        }
    }
}

fn applications(path: &Path) -> String {
    format!(
        r#"{{ "app_specific_configuration_path": {:?} }}"#,
        path.display().to_string()
    )
}

#[test]
fn the_rules_of_an_applications_file_ignore_manage_and_float_windows() {
    let file = write(
        "json",
        r#"{
            "asc one": {
                "ignore": [{ "kind": "Exe", "id": "asc-ignored", "matching_strategy": "Equals" }],
                "floating": [{ "kind": "Class", "id": "asc-floated", "matching_strategy": "Equals" }]
            },
            "asc two": {
                "manage": [{ "kind": "Exe", "id": "asc-managed", "matching_strategy": "Equals" }]
            }
        }"#,
    );
    let mut desktop = Desktop::load(&applications(&file)).unwrap();

    assert_eq!(desktop.open(39_001, "asc-ignored", "x"), Where::Unmanaged);
    assert_eq!(desktop.open(39_002, "foot", "asc-floated"), Where::Floating);
    assert_eq!(desktop.open(39_003, "asc-managed", "x"), Where::Tiled);
    assert_eq!(desktop.open(39_004, "foot", "foot"), Where::Tiled);
    let _ = std::fs::remove_file(file);
}

#[test]
fn every_file_of_a_list_is_read() {
    let first = write(
        "json",
        r#"{ "a": { "ignore": [{ "kind": "Exe", "id": "list-first", "matching_strategy": "Equals" }] } }"#,
    );
    let second = write(
        "json",
        r#"{ "b": { "ignore": [{ "kind": "Exe", "id": "list-second", "matching_strategy": "Equals" }] } }"#,
    );
    let mut desktop = Desktop::load(&format!(
        r#"{{ "app_specific_configuration_path": [{:?}, {:?}] }}"#,
        first.display().to_string(),
        second.display().to_string()
    ))
    .unwrap();

    assert_eq!(desktop.open(39_011, "list-first", "x"), Where::Unmanaged);
    assert_eq!(desktop.open(39_012, "list-second", "x"), Where::Unmanaged);
    assert_eq!(desktop.open(39_013, "foot", "foot"), Where::Tiled);
    let _ = (std::fs::remove_file(first), std::fs::remove_file(second));
}

#[test]
fn the_rules_of_the_configuration_and_of_the_file_add_up() {
    let file = write(
        "json",
        r#"{ "a": { "ignore": [{ "kind": "Exe", "id": "sum-file", "matching_strategy": "Equals" }] } }"#,
    );
    let mut desktop = Desktop::load(&format!(
        r#"{{
            "app_specific_configuration_path": {:?},
            "ignore_rules": [{{ "kind": "Exe", "id": "sum-config", "matching_strategy": "Equals" }}]
        }}"#,
        file.display().to_string()
    ))
    .unwrap();

    assert_eq!(desktop.open(39_021, "sum-file", "x"), Where::Unmanaged);
    assert_eq!(desktop.open(39_022, "sum-config", "x"), Where::Unmanaged);
    let _ = std::fs::remove_file(file);
}

#[test]
fn a_file_that_is_not_there_is_skipped_and_one_that_is_broken_is_an_error() {
    let missing = short_path("json");
    let mut desktop = Desktop::load(&applications(&missing)).unwrap();
    assert_eq!(desktop.open(39_031, "foot", "foot"), Where::Tiled);

    let broken = write("json", "{ not json");
    let error = Desktop::load(&applications(&broken))
        .err()
        .expect("a broken file is an error");
    assert!(!error.to_string().is_empty());
    let _ = std::fs::remove_file(broken);
}

#[test]
fn the_kinds_of_rule_of_other_platforms_are_read_and_do_nothing() {
    let file = write(
        "json",
        r#"{
            "$schema": "https://example.invalid/applications.schema.json",
            "kde": {
                "titleless": [{ "kind": "Exe", "id": "other-platform", "matching_strategy": "Equals" }],
                "tabbed": [{ "kind": "Exe", "id": "other-platform", "matching_strategy": "Equals" }]
            }
        }"#,
    );
    let mut desktop = Desktop::load(&applications(&file)).unwrap();

    assert_eq!(desktop.open(39_041, "other-platform", "x"), Where::Tiled);

    // the `$schema` entry is the text of a schema's address, and the rest are applications
    let loaded = ApplicationSpecificConfiguration::load(&file).unwrap();
    assert!(matches!(
        loaded["$schema"],
        AscApplicationRulesOrSchema::Schema(_)
    ));
    assert!(matches!(
        loaded["kde"],
        AscApplicationRulesOrSchema::AscApplicationRules(_)
    ));
    let _ = std::fs::remove_file(file);
}

#[test]
fn the_applications_file_komorebic_ships_floats_the_programs_it_names() {
    let shipped = Path::new(env!("CARGO_MANIFEST_DIR")).join("../komorebic/applications.json");
    let mut desktop = Desktop::load(&applications(&shipped)).unwrap();

    assert_eq!(
        desktop.open(39_051, "kcalc", "org.kde.kcalc"),
        Where::Floating
    );
    assert_eq!(
        desktop.open(39_052, "systemsettings", "org.kde.systemsettings"),
        Where::Floating
    );
    assert_eq!(desktop.open(39_053, "kate", "org.kde.kate"), Where::Tiled);
}
