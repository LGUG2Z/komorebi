//! `code`: from a recorded log line to the line of komorebi's source that wrote it.

use komorebi_recorder::analysis::catalog;
use komorebi_recorder::analysis::flavor::Flavor;
use komorebi_recorder::testkit::SessionBuilder;
use serde_json::Value;
use serde_json::json;
use std::process::Command;
use std::process::Output;

fn recorder(args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_komorebi-recorder"))
        .args(args)
        .output()
        .expect("run komorebi-recorder")
}

fn json(args: &[&str]) -> Value {
    let output = recorder(args);
    assert!(
        output.status.success(),
        "{args:?} failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    serde_json::from_slice(&output.stdout).expect("JSON")
}

/// A session whose log was written by real komorebi statements, recorded by the komorebi this
/// recorder was built against (so there is no version skew) or by another one.
fn session(name: &str, commit: Option<&str>) -> String {
    let mut builder = SessionBuilder::new(name)
        .event(
            2.3,
            json!({"type": "FocusChange", "content": [{"type": "Accessibility", "content": "AXApplicationActivated"}, 7, 3]}),
            None,
        )
        .log(2.0, "INFO", "process_event: komorebi::process_event: processing event: FocusChange for process 7 with notification AXApplicationActivated")
        .log(2.1, "INFO", "process_event:focus_window{idx=0}: komorebi_core::container: focusing window")
        .log(2.2, "INFO", "komorebi::reaper: reaping window: 4711")
        .log(2.25, "INFO", "komorebi::border_manager: invalidating border observer");
    if let Some(commit) = commit {
        builder = builder.komorebi_commit(commit);
    }
    builder.build().0.dir.display().to_string()
}

#[test]
fn a_log_line_is_traced_to_the_statement_that_wrote_it() {
    komorebi_recorder::skip_without_komorebi_mac_source!();
    let recorded = catalog::of(Flavor::Macos).commit().unwrap_or("unknown");
    let dir = session("code-line", Some(recorded));

    let answer = json(&["code", &dir, "log:3", "--context", "1", "--json"]);
    assert_eq!(answer["schema"], "code/1");

    let line = &answer["data"]["lines"][0];
    assert_eq!(line["target"], "komorebi::reaper");
    assert_eq!(line["message"], "reaping window: 4711");
    let source = &line["source"]["statement"];
    assert_eq!(source["file"], "komorebi-macos/src/reaper.rs");
    assert!(
        source["template"]
            .as_str()
            .expect("template")
            .starts_with("reaping window")
    );
    // the reaper logs this from three places, and the answer says so rather than picking one
    assert_eq!(line["source"]["confidence"], "ambiguous");
    assert_eq!(
        line["source"]["alternatives"]
            .as_array()
            .expect("alternatives")
            .len(),
        2
    );

    // the reaper changes state without telling subscribers, which is the point of saying so
    assert_eq!(line["changes_state_silently"], true);
    assert!(
        line["role"]
            .as_str()
            .expect("role")
            .contains("no longer exist")
    );

    // and the code around the statement is there, with the statement in the middle
    let snippet = line["snippet"]["lines"].as_array().expect("snippet");
    assert_eq!(snippet.len(), 3);
    let middle = snippet[1][1].as_str().expect("text");
    assert!(middle.contains("reaping window"), "{middle}");
}

#[test]
fn an_event_is_explained_by_everything_its_handler_logged() {
    komorebi_recorder::skip_without_komorebi_mac_source!();
    let recorded = catalog::of(Flavor::Macos).commit().unwrap_or("unknown");
    let dir = session("code-event", Some(recorded));

    let answer = json(&["code", &dir, "event:1", "--json"]);
    let lines = answer["data"]["lines"].as_array().expect("lines");

    // the header and the focus line are the handler's; the reaper and border manager are not
    assert_eq!(lines.len(), 2, "{lines:#?}");
    assert_eq!(lines[0]["source"]["statement"]["function"], "process_event");
    assert_eq!(lines[1]["source"]["statement"]["owner"], "Container");
    assert_eq!(lines[1]["source"]["statement"]["function"], "focus_window");
}

#[test]
fn a_commit_that_cannot_be_compared_is_a_warning_on_every_answer() {
    komorebi_recorder::skip_without_komorebi_mac_source!();
    let dir = session(
        "code-skew",
        Some("0000000000000000000000000000000000000000"),
    );

    let answer = json(&["code", &dir, "log:3", "--json"]);
    assert_eq!(answer["data"]["skew"]["verdict"], "unknown");
    // nothing is vouched for when it cannot be told
    assert_eq!(answer["data"]["lines"][0]["file_changed"], true);
    let warnings = answer["warnings"].as_array().expect("warnings");
    assert!(
        warnings.iter().any(|warning| warning
            .as_str()
            .is_some_and(|text| text.contains("0000000000"))),
        "{warnings:?}"
    );

    let text = String::from_utf8_lossy(&recorder(&["code", &dir, "log:3"]).stdout).into_owned();
    assert!(text.starts_with("WARNING:"), "{text}");
}

fn git(args: &[&str]) -> Option<String> {
    let root = komorebi_recorder::testkit::komorebi_mac_src();
    let output = Command::new("git")
        .arg("-C")
        .arg(root)
        .args(args)
        .output()
        .ok()?;
    output
        .status
        .success()
        .then(|| String::from_utf8_lossy(&output.stdout).trim().to_string())
}

/// An older komorebi: only the files that really differ from it are doubted, the rest are as good
/// as if the commits were the same.
#[test]
fn only_the_files_that_changed_since_the_recorded_commit_are_doubted() {
    komorebi_recorder::skip_without_komorebi_mac_source!();
    let Some(history) = git(&["log", "-2", "--format=%H", "--", "komorebi-macos/src"]) else {
        return;
    };
    let Some(older) = history.lines().nth(1) else {
        return;
    };
    let Some(indexed) = catalog::of(Flavor::Macos).commit() else {
        return;
    };
    let Some(changed) = git(&[
        "diff",
        "--name-only",
        older,
        indexed,
        "--",
        "komorebi-macos/src",
        "komorebi-client/src",
        "komorebi-layouts/src",
        "crates/komorebi-core/src",
        "crates/komorebi-transport/src",
    ]) else {
        return;
    };
    let changed: Vec<&str> = changed.lines().collect();

    let dir = session("code-older", Some(older));
    let answer = json(&["code", &dir, "log:1", "log:2", "log:3", "--json"]);
    let lines = answer["data"]["lines"].as_array().expect("lines");
    assert_eq!(lines.len(), 3);
    for line in lines {
        let file = line["source"]["statement"]["file"].as_str().expect("file");
        assert_eq!(
            line["file_changed"],
            changed.contains(&file),
            "{file} (changed files: {changed:?})"
        );
    }

    let verdict = answer["data"]["skew"]["verdict"].as_str().expect("verdict");
    assert_eq!(
        verdict,
        if changed.is_empty() {
            "same"
        } else {
            "different"
        }
    );
}

#[test]
fn a_session_that_did_not_record_the_version_says_it_is_unknown() {
    let dir = session("code-unknown", None);
    let answer = json(&["code", &dir, "log:3", "--json"]);
    assert_eq!(answer["data"]["skew"]["verdict"], "unknown");
}

#[test]
fn a_line_no_statement_writes_is_said_so_and_a_bad_reference_is_not_found() {
    komorebi_recorder::skip_without_komorebi_mac_source!();
    let dir = session("code-missing", None);

    let none = json(&["code", &dir, "log:4", "--json"]);
    // the border manager line is a real statement; a made-up one is not
    assert!(none["data"]["lines"][0]["source"].is_object());

    let (made_up, _) = SessionBuilder::new("code-made-up")
        .log(1.0, "INFO", "komorebi::reaper: a message nothing writes")
        .build();
    let dir_made_up = made_up.dir.display().to_string();
    let unknown = json(&["code", &dir_made_up, "log:1", "--json"]);
    assert!(unknown["data"]["lines"][0]["source"].is_null());

    let beyond = recorder(&["code", &dir, "log:99"]);
    assert_eq!(beyond.status.code(), Some(3));
    let no_event = recorder(&["code", &dir, "event:99"]);
    assert_eq!(no_event.status.code(), Some(3));
    let nothing = recorder(&["code", &dir]);
    assert_eq!(nothing.status.code(), Some(2));
    let wrong_kind = recorder(&["code", &dir, "incident:1"]);
    assert_eq!(wrong_kind.status.code(), Some(2));
}

#[test]
fn statements_can_be_searched_by_their_text() {
    komorebi_recorder::skip_without_komorebi_mac_source!();
    let dir = session("code-grep", None);

    let answer = json(&["code", &dir, "--grep", "reaping window", "--json"]);
    let statements = answer["data"]["statements"].as_array().expect("statements");
    let in_reaper = statements
        .iter()
        .filter(|statement| statement["file"] == "komorebi-macos/src/reaper.rs")
        .count();
    assert_eq!(in_reaper, 3, "{statements:#?}");
}
