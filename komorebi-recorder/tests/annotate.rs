//! `annotate` and `annotations` through the real binary, against a synthetic session.

use komorebi_recorder::testkit::SessionBuilder;
use komorebi_recorder::testkit::state_with_windows;
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

fn stdout(output: &Output) -> String {
    String::from_utf8_lossy(&output.stdout).into_owned()
}

fn envelope(output: &Output) -> Value {
    serde_json::from_slice(&output.stdout)
        .unwrap_or_else(|error| panic!("not JSON ({error}): {}", stdout(output)))
}

fn session(name: &str) -> String {
    let (session, _) = SessionBuilder::new(name)
        .event(
            2.0,
            json!({"type": "Show", "content": []}),
            Some(state_with_windows(&[(1, "one")])),
        )
        .event(
            4.0,
            json!({"type": "FocusChange", "content": []}),
            Some(state_with_windows(&[(1, "one"), (2, "two")])),
        )
        .log(3.0, "INFO", "komorebi::process_event: processing event")
        .build();
    session.dir.display().to_string()
}

#[test]
fn a_bug_region_is_added_listed_and_read_back_verbatim() {
    let dir = session("annotate-bug");

    let added = recorder(&[
        "annotate",
        &dir,
        "add",
        "--kind",
        "bug",
        "--start",
        "2.5",
        "--end",
        "6",
        "--text",
        "the new window opens on the wrong monitor",
        "--expected",
        "it should open on the focused monitor",
        "--json",
    ]);
    assert!(
        added.status.success(),
        "{}",
        String::from_utf8_lossy(&added.stderr)
    );
    let added = envelope(&added);
    assert_eq!(added["ok"], true);
    assert_eq!(added["schema"], "annotation/1");
    let id = added["data"]["annotation"]["id"]
        .as_str()
        .expect("id")
        .to_string();
    assert!(
        (added["data"]["annotation"]["start_video_s"]
            .as_f64()
            .expect("start")
            - 2.5)
            .abs()
            < 1e-6
    );
    assert!(
        (added["data"]["annotation"]["end_video_s"]
            .as_f64()
            .expect("end")
            - 6.0)
            .abs()
            < 1e-6
    );

    let listed = envelope(&recorder(&["annotations", &dir, "--kind", "bug", "--json"]));
    let annotations = listed["data"]["annotations"].as_array().expect("array");
    assert_eq!(annotations.len(), 1);
    assert_eq!(annotations[0]["id"], id.as_str());
    assert_eq!(
        annotations[0]["text"],
        "the new window opens on the wrong monitor"
    );
    assert_eq!(annotations[0]["open_ended"], false);

    // the text rendering leads with what the user said is wrong
    let text = stdout(&recorder(&["annotations", &dir]));
    assert!(text.contains("what goes wrong"), "{text}");
    assert!(
        text.contains("the new window opens on the wrong monitor"),
        "{text}"
    );
    assert!(text.contains("00:02.500 -> 00:06.000"), "{text}");
}

#[test]
fn a_bug_can_be_marked_relative_to_an_event_and_edited_and_deleted() {
    let dir = session("annotate-edit");

    let added = envelope(&recorder(&[
        "annotate",
        &dir,
        "add",
        "--kind",
        "bug",
        "--start",
        "event:2-1s",
        "--open-ended",
        "--text",
        "tiles overlap",
        "--json",
    ]));
    let id = added["data"]["annotation"]["id"]
        .as_str()
        .expect("id")
        .to_string();
    // event 2 is at 4s
    assert!(
        (added["data"]["annotation"]["start_video_s"]
            .as_f64()
            .expect("start")
            - 3.0)
            .abs()
            < 1e-6
    );
    assert_eq!(added["data"]["annotation"]["open_ended"], true);

    let edited = envelope(&recorder(&[
        "annotate",
        &dir,
        "edit",
        &id,
        "--end",
        "event:2.end+2s",
        "--json",
    ]));
    assert_eq!(edited["data"]["annotation"]["open_ended"], false);
    assert!(
        (edited["data"]["annotation"]["end_video_s"]
            .as_f64()
            .expect("end")
            - 6.0)
            .abs()
            < 1e-6
    );

    let deleted = recorder(&["annotate", &dir, "delete", &id, "--json"]);
    assert!(deleted.status.success());
    let listed = envelope(&recorder(&["annotations", &dir, "--json"]));
    assert_eq!(
        listed["data"]["annotations"]
            .as_array()
            .expect("array")
            .len(),
        0
    );
}

#[test]
fn a_bug_without_an_end_or_a_description_is_refused_with_the_usage_exit_code() {
    let dir = session("annotate-refuse");

    let no_end = recorder(&[
        "annotate", &dir, "add", "--kind", "bug", "--start", "2", "--text", "x",
    ]);
    assert_eq!(no_end.status.code(), Some(2));
    assert!(String::from_utf8_lossy(&no_end.stderr).contains("--end"));

    let no_text = recorder(&[
        "annotate", &dir, "add", "--kind", "bug", "--start", "2", "--end", "3",
    ]);
    assert_eq!(no_text.status.code(), Some(2));
}

#[test]
fn a_finding_must_cite_evidence_that_exists() {
    let dir = session("annotate-finding");
    let bug = envelope(&recorder(&[
        "annotate", &dir, "add", "--kind", "bug", "--start", "2", "--end", "5", "--text", "wrong",
        "--json",
    ]));
    let bug_id = bug["data"]["annotation"]["id"]
        .as_str()
        .expect("id")
        .to_string();

    // no evidence at all
    let bare = recorder(&[
        "annotate", &dir, "add", "--kind", "finding", "--about", &bug_id, "--text", "cause",
    ]);
    assert_eq!(bare.status.code(), Some(2));

    // evidence that is not there is not found
    let missing = recorder(&[
        "annotate", &dir, "add", "--kind", "finding", "--about", &bug_id, "--text", "cause",
        "--ref", "event:99",
    ]);
    assert_eq!(
        missing.status.code(),
        Some(3),
        "{}",
        String::from_utf8_lossy(&missing.stderr)
    );

    // real evidence is accepted and the finding inherits the bug's region
    let found = envelope(&recorder(&[
        "annotate",
        &dir,
        "add",
        "--kind",
        "finding",
        "--about",
        &bug_id,
        "--text",
        "the show handler tiled it twice",
        "--ref",
        "event:1",
        "--ref",
        "log:1",
        "--json",
    ]));
    assert_eq!(found["ok"], true);
    assert_eq!(found["data"]["annotation"]["kind"], "finding");
    assert_eq!(found["data"]["annotation"]["about"], bug_id.as_str());
    assert!(
        (found["data"]["annotation"]["start_video_s"]
            .as_f64()
            .expect("start")
            - 2.0)
            .abs()
            < 1e-6
    );
    assert_eq!(
        found["data"]["annotation"]["refs"]
            .as_array()
            .expect("refs")
            .len(),
        2
    );
}

#[test]
fn failures_in_json_mode_are_an_error_envelope_with_the_exit_code() {
    let dir = session("annotate-json-error");
    let output = recorder(&[
        "annotate",
        &dir,
        "edit",
        "a-00000000",
        "--text",
        "x",
        "--json",
    ]);
    assert_eq!(output.status.code(), Some(3));
    let body = envelope(&output);
    assert_eq!(body["ok"], false);
    assert_eq!(body["error"]["kind"], "not-found");
    assert_eq!(body["error"]["code"], 3);
}

#[test]
fn annotating_works_on_a_session_that_is_still_recording() {
    let (session, mut manifest) = SessionBuilder::new("annotate-live").build();
    manifest.status = "recording".to_string();
    session.write_manifest(&manifest).expect("manifest");
    let dir = session.dir.display().to_string();

    let output = recorder(&[
        "annotate",
        &dir,
        "add",
        "--kind",
        "bug",
        "--start",
        "1",
        "--open-ended",
        "--text",
        "x",
        "--json",
    ]);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );

    // but reading the timeline of a live session says so, with the unusable exit code
    let at = recorder(&["at", &dir, "1"]);
    assert_eq!(at.status.code(), Some(4));
}
