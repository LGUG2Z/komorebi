//! `findings`, `brief --compact`, log references by text or time, and `record --until-exit`,
//! through the real binary.

use komorebi_recorder::testkit::SessionBuilder;
use serde_json::Value;
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

fn stderr(output: &Output) -> String {
    String::from_utf8_lossy(&output.stderr).into_owned()
}

fn envelope(output: &Output) -> Value {
    serde_json::from_slice(&output.stdout)
        .unwrap_or_else(|error| panic!("not JSON ({error}): {}", stdout(output)))
}

/// A session with an error and a warning in komorebi's log, and a bug the user marked.
fn session(name: &str) -> String {
    let (session, _) = SessionBuilder::new(name)
        .log(2.0, "INFO", "komorebi::process_event: processing event")
        .log(
            3.0,
            "ERROR",
            "komorebi::window: could not place window 4711",
        )
        .log(4.0, "WARN", "komorebi::reaper: window 12 is gone")
        .log(5.0, "INFO", "komorebi::process_event: processing event")
        .build();
    let dir = session.dir.display().to_string();

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
        "the window never appears",
    ]);
    assert!(added.status.success(), "{}", stderr(&added));
    dir
}

#[test]
fn findings_lists_detector_findings_with_stable_ids() {
    let dir = session("findings-ids");

    let first = envelope(&recorder(&["findings", &dir, "--json"]));
    assert_eq!(first["ok"], true);
    assert_eq!(first["schema"], "findings/1");
    let items = first["data"]["findings"].as_array().expect("findings");
    assert_eq!(first["data"]["detector"], 2);
    assert_eq!(first["data"]["agent"], 0);

    let error = items
        .iter()
        .find(|item| item["kind"] == "log-error")
        .expect("log-error");
    let id = error["id"].as_str().expect("id");
    assert!(id.starts_with("f-") && id.len() == 8, "{id}");
    assert_eq!(error["source"], "detector");
    assert_eq!(error["severity"], "warn");
    assert_eq!(error["refs"][0], "log:2");

    // the same finding has the same id the next time
    let second = envelope(&recorder(&["findings", &dir, "--json"]));
    let again = second["data"]["findings"]
        .as_array()
        .expect("findings")
        .iter()
        .find(|item| item["kind"] == "log-error")
        .expect("log-error");
    assert_eq!(again["id"], id);

    // the text form leads with the count and shows the id
    let text = stdout(&recorder(&["findings", &dir]));
    assert!(text.starts_with("2 finding(s)"), "{text}");
    assert!(text.contains(id), "{text}");
}

#[test]
fn findings_merges_what_an_agent_concluded_and_filters() {
    let dir = session("findings-merge");

    let added = recorder(&[
        "annotate",
        &dir,
        "add",
        "--kind",
        "finding",
        "--start",
        "3",
        "--text",
        "the placement failed because the window was already gone",
        "--ref",
        "log:2",
    ]);
    assert!(added.status.success(), "{}", stderr(&added));

    let all = envelope(&recorder(&["findings", &dir, "--json"]));
    let items = all["data"]["findings"].as_array().expect("findings");
    assert_eq!(all["data"]["total"], 3);
    assert_eq!(all["data"]["agent"], 1);
    // an agent's conclusions come first
    assert_eq!(items[0]["source"], "agent");
    assert_eq!(items[0]["kind"], "agent-finding");
    assert!(items[0]["id"].as_str().expect("id").starts_with("a-"));
    assert!(items[0]["severity"].is_null());

    let by_kind = envelope(&recorder(&[
        "findings",
        &dir,
        "--kind",
        "log-warning",
        "--json",
    ]));
    assert_eq!(by_kind["data"]["total"], 1);
    assert_eq!(by_kind["data"]["findings"][0]["kind"], "log-warning");

    let by_severity = envelope(&recorder(&[
        "findings",
        &dir,
        "--severity",
        "warn",
        "--json",
    ]));
    assert_eq!(by_severity["data"]["total"], 1);
    assert_eq!(by_severity["data"]["findings"][0]["kind"], "log-error");

    let limited = envelope(&recorder(&["findings", &dir, "--limit", "1", "--json"]));
    assert_eq!(
        limited["data"]["findings"]
            .as_array()
            .expect("findings")
            .len(),
        1
    );
    assert_eq!(limited["data"]["total"], 3);
    assert_eq!(limited["truncated"], true);
}

#[test]
fn a_compact_brief_is_short_and_keeps_the_users_words() {
    let dir = session("brief-compact");

    let text = recorder(&["brief", &dir, "--compact"]);
    assert!(text.status.success(), "{}", stderr(&text));
    let text = stdout(&text);
    assert!(text.lines().count() <= 40, "{text}");
    assert!(text.contains("the window never appears"), "{text}");
    assert!(text.contains("could not place window 4711"), "{text}");
    assert!(
        text.contains(&format!("komorebi-recorder findings {dir}")),
        "{text}"
    );

    let json = envelope(&recorder(&["brief", &dir, "--compact", "--json"]));
    assert_eq!(json["schema"], "brief-compact/1");
    assert_eq!(
        json["data"]["regions"][0]["text"],
        "the window never appears"
    );
    let top = json["data"]["top_findings"]
        .as_array()
        .expect("top findings");
    assert!(top.len() <= 3 && !top.is_empty());
    assert!(top[0]["id"].as_str().expect("id").starts_with("f-"));
    assert!(!json["data"]["next"].as_array().expect("next").is_empty());
}

#[test]
fn a_log_reference_can_be_a_text_a_time_or_a_video_time() {
    let dir = session("ref-resolve");
    let add = |reference: &str| {
        recorder(&[
            "annotate", &dir, "add", "--kind", "finding", "--start", "3", "--text", "because",
            "--ref", reference, "--json",
        ])
    };

    // by text: resolved to its line, and the line is shown
    let by_text = add("log:\"could not place\"");
    assert!(by_text.status.success(), "{}", stderr(&by_text));
    let by_text = envelope(&by_text);
    let refs = by_text["data"]["annotation"]["refs"][0].clone();
    assert_eq!(refs["file"], "komorebi.log");
    assert_eq!(refs["line"], 2);
    let note = by_text["warnings"][0].as_str().expect("a note");
    assert!(
        note.contains("log:2: ERROR komorebi::window: could not place window 4711"),
        "{note}"
    );

    // by UTC time (video 4.0 is 12:00:09.000 on the testkit's clock)
    let by_time = envelope(&add("log:12:00:09.000"));
    assert_eq!(by_time["data"]["annotation"]["refs"][0]["line"], 3);

    // by video time
    let by_video = envelope(&add("log:5.0"));
    assert_eq!(by_video["data"]["annotation"]["refs"][0]["line"], 4);

    // a line number is unchanged, and has nothing to say
    let by_number = envelope(&add("log:1"));
    assert_eq!(by_number["data"]["annotation"]["refs"][0]["line"], 1);
    assert!(
        by_number["warnings"]
            .as_array()
            .expect("warnings")
            .is_empty()
    );
}

#[test]
fn an_ambiguous_text_is_a_usage_error_that_lists_candidates() {
    let dir = session("ref-ambiguous");
    let output = recorder(&[
        "annotate",
        &dir,
        "add",
        "--kind",
        "finding",
        "--start",
        "3",
        "--text",
        "because",
        "--ref",
        "log:\"processing event\"",
        "--json",
    ]);

    assert_eq!(output.status.code(), Some(2), "{}", stderr(&output));
    let failure = envelope(&output);
    assert_eq!(failure["ok"], false);
    let message = failure["error"]["message"].as_str().expect("message");
    assert!(message.contains("matches 2 lines"), "{message}");
    assert!(
        message.contains("log:1: INFO komorebi::process_event"),
        "{message}"
    );
    assert!(
        message.contains("log:4: INFO komorebi::process_event"),
        "{message}"
    );

    // nothing was written
    let listed = envelope(&recorder(&[
        "annotations",
        &dir,
        "--kind",
        "finding",
        "--json",
    ]));
    assert!(
        listed["data"]["annotations"]
            .as_array()
            .expect("array")
            .is_empty()
    );
}

#[test]
fn the_new_outputs_are_registered_schemas() {
    let listed = stdout(&recorder(&["schema"]));
    for name in [
        "record/1",
        "displays/1",
        "reindex/1",
        "selftest/1",
        "findings/1",
        "brief-compact/1",
    ] {
        assert!(listed.contains(name), "{name} is not listed: {listed}");
        let schema = recorder(&["schema", name]);
        assert!(schema.status.success(), "{name}: {}", stderr(&schema));
        assert!(envelope(&schema).is_object());
    }
}

#[test]
fn reindex_has_a_json_form() {
    let dir = session("reindex-json");
    let output = recorder(&["reindex", &dir, "--json"]);
    assert!(output.status.success(), "{}", stderr(&output));

    let envelope = envelope(&output);
    assert_eq!(envelope["schema"], "reindex/1");
    assert!(envelope["data"]["entries"].as_u64().is_some());
    assert!(envelope["data"]["recovered"].as_array().is_some());
}

#[test]
fn until_exit_needs_a_command_and_a_command_needs_until_exit() {
    let missing = recorder(&["record", "--until-exit"]);
    assert_eq!(missing.status.code(), Some(2));

    let stray = recorder(&["record", "--", "cmd"]);
    assert_eq!(stray.status.code(), Some(2));
}

/// A real recording of everything that needs no screen and no komorebi, which a command stops.
#[test]
fn recording_until_a_command_exits_reports_the_commands_status() {
    let out = std::env::temp_dir().join(format!(
        "komorebi-recorder-until-exit-{}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&out);
    let out_arg = out.display().to_string();
    let command: &[&str] = if cfg!(windows) {
        &["cmd", "/c", "exit 3"]
    } else {
        &["sh", "-c", "exit 3"]
    };

    let mut args = vec![
        "record",
        "--no-video",
        "--no-komorebi",
        "--no-oslog",
        "--json",
        "--until-exit",
        "--out",
        &out_arg,
        "--",
    ];
    args.extend_from_slice(command);
    let output = recorder(&args);

    // the recorder's status is about the recording, which finished
    assert_eq!(output.status.code(), Some(0), "{}", stderr(&output));
    let envelope = envelope(&output);
    assert_eq!(envelope["schema"], "record/1");
    assert_eq!(envelope["data"]["child"]["end"], "exited");
    assert_eq!(envelope["data"]["child"]["exit_code"], 3);
    assert_eq!(envelope["data"]["child"]["success"], false);
    assert!(out.join("session.json").exists());
    let _ = std::fs::remove_dir_all(&out);
}
