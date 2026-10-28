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
    command.arg("check").env("KOMOREBI_CONFIG_HOME", &dir);

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
