//! `mark` through the real binary, against a synthetic session whose recorder lock is held by the
//! test, the way a recorder holds it.

use komorebi_recorder::lock::RecorderLock;
use komorebi_recorder::testkit::SessionBuilder;
use serde_json::Value;
use std::process::Command;
use std::process::Output;

fn recorder(args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_komorebi-recorder"))
        .args(args)
        // never pick up the environment of whoever runs the tests
        .env_remove("KOMOREBI_RECORDER_SESSION")
        .output()
        .expect("run komorebi-recorder")
}

fn stdout(output: &Output) -> String {
    String::from_utf8_lossy(&output.stdout).into_owned()
}

fn json(output: &Output) -> Value {
    serde_json::from_slice(&output.stdout)
        .unwrap_or_else(|error| panic!("not JSON ({error}): {}", stdout(output)))
}

#[test]
fn a_marker_lands_on_a_session_being_recorded_and_is_listed_like_a_note() {
    let (session, _) = SessionBuilder::new("mark-running").build();
    let dir = session.dir.display().to_string();
    let _lock = RecorderLock::acquire(&session.dir).expect("hold the recorder lock");

    // `annotations` and `brief` refuse nothing here, but `view`-style loading would: marking must
    // not depend on it
    let first = recorder(&["mark", "step 1: open it", "--session", &dir]);
    assert!(
        first.status.success(),
        "{}",
        String::from_utf8_lossy(&first.stderr)
    );
    let text = stdout(&first);
    assert!(text.starts_with("marked "), "{text}");
    assert!(
        text.contains("(a-") && text.trim_end().ends_with("step 1: open it"),
        "{text}"
    );

    let second = recorder(&["mark", "step 2", "--session", &dir, "--json"]);
    assert!(second.status.success());
    let envelope = json(&second);
    assert_eq!(envelope["schema"], "mark/1");
    assert_eq!(envelope["ok"], true);
    assert_eq!(envelope["data"]["kind"], "note");
    assert_eq!(envelope["data"]["text"], "step 2");
    assert!(envelope["data"]["at_mach_ns"].as_u64().is_some());

    let listed = recorder(&["annotations", &dir, "--json", "--kind", "note"]);
    let annotations = json(&listed)["data"]["annotations"].clone();
    let annotations = annotations.as_array().expect("array");
    assert_eq!(annotations.len(), 2);
    for note in annotations {
        assert_eq!(note["author"], "script");
        assert_eq!(note["open_ended"], false);
        assert_eq!(note["start_mach_ns"], note["end_mach_ns"]);
    }

    let brief = json(&recorder(&["brief", &dir, "--json"]));
    assert_eq!(
        brief["data"]["notes"].as_array().map(Vec::len),
        Some(2),
        "{brief}"
    );
}

#[test]
fn the_environment_names_the_session() {
    let (session, _) = SessionBuilder::new("mark-env").build();
    let _lock = RecorderLock::acquire(&session.dir).expect("hold the recorder lock");

    let output = Command::new(env!("CARGO_BIN_EXE_komorebi-recorder"))
        .args(["mark", "from the environment"])
        .env("KOMOREBI_RECORDER_SESSION", &session.dir)
        .output()
        .expect("run");
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let file = std::fs::read_to_string(session.dir.join("annotations.jsonl")).expect("written");
    assert!(file.contains("from the environment"));
}

#[test]
fn a_finished_session_is_refused_and_pointed_at_annotate() {
    let (session, _) = SessionBuilder::new("mark-finished").build();
    let dir = session.dir.display().to_string();

    let output = recorder(&["mark", "too late", "--session", &dir, "--json"]);
    assert_eq!(output.status.code(), Some(2));
    assert!(String::from_utf8_lossy(&output.stderr).contains("annotate"));
    assert_eq!(json(&output)["ok"], false);
    assert!(!session.dir.join("annotations.jsonl").exists());
}

#[test]
fn no_session_to_mark_is_not_found() {
    let missing = std::env::temp_dir().join("komorebi-recorder-no-such-session");
    let output = recorder(&[
        "mark",
        "nobody is listening",
        "--session",
        &missing.display().to_string(),
    ]);
    assert_eq!(output.status.code(), Some(3));
    assert!(String::from_utf8_lossy(&output.stderr).contains("not a session directory"));
}
