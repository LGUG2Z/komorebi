#![cfg(target_os = "linux")]
//! `komorebic check`, as a person runs it: a configuration in, a report and an exit status out.

use std::path::Path;
use std::process::Command;
use std::process::Output;

fn check(config: Option<&str>, extra: &[&str]) -> (Output, String) {
    static NEXT: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);
    let dir = std::env::temp_dir().join(format!(
        "kmc-cli-{}-{}",
        std::process::id(),
        NEXT.fetch_add(1, std::sync::atomic::Ordering::SeqCst)
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();

    let mut command = Command::new(env!("CARGO_BIN_EXE_komorebic"));
    command
        .arg("check")
        .arg("--configuration-only")
        .env("KOMOREBI_CONFIG_HOME", &dir)
        // there is no desktop to ask about, and nothing from this one to leak in
        .env_remove("DBUS_SESSION_BUS_ADDRESS");

    if let Some(config) = config {
        let path = dir.join("komorebi.json");
        std::fs::write(&path, config).unwrap();
        command.arg("--komorebi-config").arg(&path);
    }
    command.args(extra);

    let output = command.output().unwrap();
    let text = String::from_utf8_lossy(&output.stdout).into_owned();
    let _ = std::fs::remove_dir_all(Path::new(&dir));
    (output, text)
}

#[test]
fn a_good_configuration_passes() {
    let (output, text) = check(
        Some(
            r#"{ "border": true, "monitors": [ { "workspaces": [ { "name": "a", "layout": "BSP" } ] } ] }"#,
        ),
        &[],
    );

    assert!(output.status.success(), "{text}");
    assert!(text.contains("is valid"), "{text}");
    assert!(text.contains("0 error(s), 0 warning(s)"), "{text}");
}

#[test]
fn a_file_that_is_not_json_fails_and_shows_the_line() {
    let (output, text) = check(Some("{\n  \"border\": tru,\n}\n"), &[]);

    assert_eq!(output.status.code(), Some(1), "{text}");
    assert!(text.contains("not valid JSON"), "{text}");
    assert!(text.contains("2 |   \"border\": tru,"), "{text}");
    assert!(text.contains("^"), "{text}");
}

#[test]
fn a_value_that_does_not_exist_fails_and_shows_the_line() {
    let (output, text) = check(
        Some("{\n  \"cross_boundary_behaviour\": \"Sideways\"\n}\n"),
        &[],
    );

    assert_eq!(output.status.code(), Some(1), "{text}");
    assert!(
        text.contains("does not match what komorebi reads"),
        "{text}"
    );
    assert!(text.contains("2 |"), "{text}");
}

#[test]
fn a_typo_is_a_warning_with_the_word_that_was_meant_and_does_not_fail() {
    let (output, text) = check(
        Some(r#"{ "bordr_width": 10, "border_style": "System" }"#),
        &[],
    );

    assert!(output.status.success(), "{text}");
    assert!(text.contains("`bordr_width` is not a setting"), "{text}");
    assert!(text.contains("did you mean `border_width`?"), "{text}");
    // a key for another platform is not a mistake
    assert!(
        text.contains("accepted and ignored here: border_style"),
        "{text}"
    );
    assert!(text.contains("1 warning(s)"), "{text}");
}

#[test]
fn no_file_is_not_an_error() {
    let (output, text) = check(None, &[]);

    assert!(output.status.success(), "{text}");
    assert!(text.contains("starts with its defaults"), "{text}");
}

#[test]
fn configuration_only_leaves_the_desktop_out() {
    let (output, text) = check(None, &[]);

    assert!(output.status.success(), "{text}");
    assert!(!text.contains("The desktop"), "{text}");
}

/// Runs `komorebic check` with no `--configuration-only`, in a config home and a config directory
/// of its own, against the session bus at `bus`.
fn check_desktop(dir: &Path, bus: &Path) -> (Output, String) {
    let output = Command::new(env!("CARGO_BIN_EXE_komorebic"))
        .arg("check")
        .env("KOMOREBI_CONFIG_HOME", dir)
        .env("XDG_CONFIG_HOME", dir.join("config"))
        // a bus that is not where the address says, whatever the environment has to offer: with
        // no address, a connection looks for the user's bus in the runtime directory
        .env(
            "DBUS_SESSION_BUS_ADDRESS",
            format!("unix:path={}", bus.display()),
        )
        .output()
        .unwrap();
    let text = String::from_utf8_lossy(&output.stdout).into_owned();
    (output, text)
}

fn scratch(name: &str) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!("kmc-cli-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

#[test]
fn without_a_session_bus_the_desktop_is_an_error() {
    let dir = scratch("desktop");

    let (output, text) = check_desktop(&dir, &dir.join("no-bus"));

    assert_eq!(output.status.code(), Some(1), "{text}");
    assert!(text.contains("The desktop"), "{text}");
    assert!(
        text.contains("could not connect to the session bus"),
        "{text}"
    );

    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn a_session_bus_that_never_answers_is_given_up_on() {
    let dir = scratch("stuck");
    let bus = dir.join("stuck.bus");
    // it takes the connection and says nothing, so the handshake never ends
    let listener = std::os::unix::net::UnixListener::bind(&bus).unwrap();
    let holder = std::thread::spawn(move || listener.accept().map(|(stream, _)| stream));

    let started = std::time::Instant::now();
    let (output, text) = check_desktop(&dir, &bus);

    assert_eq!(output.status.code(), Some(1), "{text}");
    assert!(
        text.contains("could not connect to the session bus"),
        "{text}"
    );
    assert!(
        started.elapsed() < std::time::Duration::from_secs(10),
        "{:?}",
        started.elapsed()
    );

    let _ = holder.join();
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn what_starts_komorebi_at_login_is_said_whether_or_not_the_desktop_is_there() {
    let dir = scratch("login");
    let no_bus = dir.join("no-bus");

    let (_, text) = check_desktop(&dir, &no_bus);
    assert!(text.contains("komorebi is not started at login"), "{text}");

    let enabled = Command::new(env!("CARGO_BIN_EXE_komorebic"))
        .arg("enable-autostart")
        .env("KOMOREBI_CONFIG_HOME", &dir)
        .env("XDG_CONFIG_HOME", dir.join("config"))
        .output()
        .unwrap();
    assert!(enabled.status.success(), "{enabled:?}");

    let (_, text) = check_desktop(&dir, &no_bus);
    let entry = dir.join("config/autostart/komorebi.desktop");
    assert!(
        text.contains(&format!(
            "komorebi is started at login by {}",
            entry.display()
        )),
        "{text}"
    );

    let _ = std::fs::remove_dir_all(dir);
}

/// Runs `komorebic check --configuration-only` in the config home `dir`, with a data home of its
/// own: the section of the key bindings is the one this looks at.
fn check_keys(dir: &Path) -> (Output, String) {
    let output = Command::new(env!("CARGO_BIN_EXE_komorebic"))
        .arg("check")
        .arg("--configuration-only")
        .env("KOMOREBI_CONFIG_HOME", dir)
        .env("XDG_DATA_HOME", dir.join("data"))
        .output()
        .unwrap();
    let text = String::from_utf8_lossy(&output.stdout).into_owned();
    (output, text)
}

#[test]
fn without_a_key_binding_file_the_section_says_where_key_bindings_come_from() {
    let dir = scratch("keys-none");

    let (output, text) = check_keys(&dir);

    assert!(output.status.success(), "{text}");
    assert!(text.contains("Key bindings"), "{text}");
    assert!(
        text.contains("there is no skhdrc, whkdrc or sxhkdrc in"),
        "{text}"
    );
    assert!(text.contains("komorebic hotkeys compile"), "{text}");

    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn the_key_binding_file_of_the_config_home_is_read_and_a_line_it_cannot_read_is_a_warning() {
    let dir = scratch("keys-file");
    std::fs::write(
        dir.join("skhdrc"),
        "alt - h : komorebic focus left\nalt - nosuchkey : komorebic focus up\n",
    )
    .unwrap();

    let (output, text) = check_keys(&dir);

    // a warning is not a reason to fail
    assert!(output.status.success(), "{text}");
    assert!(text.contains("skhdrc has 1 key binding"), "{text}");
    assert!(
        text.contains("1 line of it cannot be read and left out"),
        "{text}"
    );
    assert!(text.contains("line 2: error: "), "{text}");
    assert!(text.contains("1 warning(s)"), "{text}");

    let _ = std::fs::remove_dir_all(dir);
}

// the data home is where `dirs` looks on Linux only
#[cfg(target_os = "linux")]
#[test]
fn shortcut_files_the_file_does_not_call_for_are_a_warning_until_compile_replaces_them() {
    let dir = scratch("keys-stale");
    std::fs::write(dir.join("skhdrc"), "alt - h : komorebic focus left\n").unwrap();
    let shortcuts = dir.join("data/kglobalaccel");
    std::fs::create_dir_all(&shortcuts).unwrap();
    std::fs::write(
        shortcuts.join("komorebi-old.desktop"),
        "[Desktop Entry]\nX-Komorebi-Generated=true\n",
    )
    .unwrap();

    let (output, text) = check_keys(&dir);

    assert!(output.status.success(), "{text}");
    assert!(
        text.contains(
            "the shortcut files are not the ones `komorebic hotkeys compile` would write"
        ),
        "{text}"
    );
    assert!(text.contains("1 to remove"), "{text}");
    assert!(
        text.contains("komorebic hotkeys compile --reload"),
        "{text}"
    );

    let compiled = Command::new(env!("CARGO_BIN_EXE_komorebic"))
        .args(["hotkeys", "compile"])
        .env("KOMOREBI_CONFIG_HOME", &dir)
        .env("XDG_DATA_HOME", dir.join("data"))
        .output()
        .unwrap();
    assert!(compiled.status.success(), "{compiled:?}");

    let (_, text) = check_keys(&dir);
    assert!(!text.contains("are not the ones"), "{text}");
    assert!(
        text.contains("the shortcut files are the ones")
            || text.contains("no shortcut files are needed"),
        "{text}"
    );

    let _ = std::fs::remove_dir_all(dir);
}
