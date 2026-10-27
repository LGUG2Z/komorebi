//! `trace` and `window`: one event end to end, and one window through the whole recording.

use komorebi_recorder::testkit::SessionBuilder;
use komorebi_recorder::testkit::container;
use komorebi_recorder::testkit::monitor;
use komorebi_recorder::testkit::state;
use komorebi_recorder::testkit::window;
use komorebi_recorder::testkit::workspace;
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

fn text(args: &[&str]) -> String {
    let output = recorder(args);
    assert!(
        output.status.success(),
        "{args:?}: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8_lossy(&output.stdout).into_owned()
}

fn one_workspace(containers: Vec<Value>, layout: &[[i64; 4]]) -> Value {
    state(
        vec![monitor(
            1,
            [0, 0, 1000, 800],
            vec![workspace("one", containers, 0, layout)],
            0,
        )],
        0,
    )
}

/// Two windows; then the second is reaped by the reaper (no notification of its own) and the next
/// focus change reports the result; then a Brave window appears again.
fn session(name: &str) -> String {
    let two = one_workspace(
        vec![
            container("a", vec![window(1, "Ghostty", Some([0, 0, 500, 800]))]),
            container("b", vec![window(2, "Brave", Some([500, 0, 500, 800]))]),
        ],
        &[[0, 0, 500, 800], [500, 0, 500, 800]],
    );
    let one = one_workspace(
        vec![container(
            "a",
            vec![window(1, "Ghostty", Some([0, 0, 1000, 800]))],
        )],
        &[[0, 0, 1000, 800]],
    );
    let back = one_workspace(
        vec![
            container("a", vec![window(1, "Ghostty", Some([0, 0, 500, 800]))]),
            container("c", vec![window(3, "Brave", Some([500, 0, 500, 800]))]),
        ],
        &[[0, 0, 500, 800], [500, 0, 500, 800]],
    );

    let notification = |name: &str, notification: &str| json!({"type": name, "content": [{"type": "Accessibility", "content": notification}, 7, 1]});

    let (session, _) = SessionBuilder::new(name)
        .event(1.0, notification("Show", "AXWindowCreated"), Some(two))
        .event(4.0, notification("FocusChange", "AXApplicationActivated"), Some(one))
        .event(6.0, notification("Show", "AXWindowCreated"), Some(back))
        .log(0.9, "INFO", "process_event: komorebi::process_event: processing event: Show for process 7 with notification AXWindowCreated")
        .log(3.5, "INFO", "komorebi::reaper: reaping window: 2")
        .log(3.6, "INFO", "komorebi::border_manager: invalidating border observer")
        .log(3.9, "INFO", "process_event: komorebi::process_event: processing event: FocusChange for process 7 with notification AXApplicationActivated")
        .log(3.95, "INFO", "process_event:focus_container{idx=0}: komorebi::workspace: focusing container")
        .log(5.9, "INFO", "process_event: komorebi::process_event: processing event: Show for process 7 with notification AXWindowCreated")
        .build();
    session.dir.display().to_string()
}

#[test]
fn a_change_with_no_event_of_its_own_is_blamed_on_the_reaper_by_its_log_line() {
    let dir = session("trace-reaper");

    let answer = json(&["trace", &dir, "event:2", "--json"]);
    let data = &answer["data"];
    assert_eq!(data["event"]["seq"], 2);
    assert_eq!(data["event"]["class"], "focus");

    // the handler's own lines, mapped to the source that wrote them
    let handler = data["handler"].as_array().expect("handler");
    assert_eq!(handler.len(), 2);
    assert_eq!(handler[1]["spans"], "process_event:focus_container");
    assert_eq!(
        handler[1]["source"]["function"],
        "Workspace::focus_container"
    );

    // the reaper and border manager lines are ambient: other threads, in the interval
    let ambient: Vec<&str> = data["ambient"]
        .as_array()
        .expect("ambient")
        .iter()
        .map(|line| line["message"].as_str().expect("message"))
        .collect();
    assert_eq!(
        ambient,
        ["reaping window: 2", "invalidating border observer"]
    );

    let changes = data["changes"].as_array().expect("changes");
    let removed = changes
        .iter()
        .find(|change| change["kind"] == "window-removed")
        .expect("window removed");
    assert_eq!(removed["attribution"]["actor"], "reaper");
    assert_eq!(removed["attribution"]["confidence"], "high");
    // and the evidence is the reaper's own log line (line 2 of the log)
    assert_eq!(removed["attribution"]["evidence"], json!([2]));

    // the surviving window growing to fill the space is the handler's re-layout
    let grew = changes
        .iter()
        .find(|change| change["kind"] == "window-rect")
        .expect("window rect");
    assert_eq!(grew["attribution"]["actor"], "handler");
}

#[test]
fn the_text_trace_reads_top_to_bottom() {
    let dir = session("trace-text");

    let text = text(&["trace", &dir, "2"]);
    let handler = text
        .find("what the handler logged")
        .expect("handler section");
    let ambient = text
        .find("logged by other threads meanwhile")
        .expect("ambient section");
    let changed = text
        .find("what changed, and who did it")
        .expect("changes section");
    assert!(handler < ambient && ambient < changed, "{text}");
    assert!(text.contains("reaper (high)"), "{text}");
    assert!(text.contains("evidence: log:2"), "{text}");
    assert!(text.contains("from komorebi/src/workspace.rs"), "{text}");
}

#[test]
fn an_event_with_no_log_says_so_instead_of_inventing_a_handler() {
    let (session, _) = SessionBuilder::new("trace-no-log")
        .event(
            1.0,
            json!({"type": "Retile"}),
            Some(one_workspace(vec![], &[])),
        )
        .build();
    let dir = session.dir.display().to_string();

    let answer = json(&["trace", &dir, "event:1", "--json"]);
    assert_eq!(
        answer["data"]["handler"].as_array().expect("handler").len(),
        0
    );
    let notes = answer["data"]["notes"].as_array().expect("notes");
    assert!(
        notes.iter().any(|note| note
            .as_str()
            .is_some_and(|text| text.contains("no handler lines"))),
        "{notes:?}"
    );

    assert_eq!(
        recorder(&["trace", &dir, "event:99"]).status.code(),
        Some(3)
    );
    assert_eq!(recorder(&["trace", &dir, "log:1"]).status.code(), Some(2));
}

#[test]
fn a_window_is_followed_through_every_state() {
    let dir = session("window-history");

    let answer = json(&["window", &dir, "brave", "--json"]);
    let windows = answer["data"]["windows"].as_array().expect("windows");
    let ids: Vec<u64> = windows
        .iter()
        .map(|window| window["id"].as_u64().expect("id"))
        .collect();
    assert_eq!(ids, [2, 3]);

    let entries = answer["data"]["entries"].as_array().expect("entries");
    let summary: Vec<(u64, u64, &str)> = entries
        .iter()
        .map(|entry| {
            (
                entry["seq"].as_u64().expect("seq"),
                entry["window_id"].as_u64().expect("window"),
                entry["changes"][0].as_str().unwrap_or(""),
            )
        })
        .collect();
    assert_eq!(summary.len(), 3, "{entries:#?}");
    assert_eq!((summary[0].0, summary[0].1), (1, 2));
    assert!(
        summary[0]
            .2
            .starts_with("appeared on monitor 1 workspace one")
    );
    assert_eq!(
        (summary[1].0, summary[1].1, summary[1].2),
        (2, 2, "went away")
    );
    assert_eq!((summary[2].0, summary[2].1), (3, 3));

    // asking for one window by id narrows it
    let one = json(&["window", &dir, "1", "--json"]);
    let changes: Vec<&str> = one["data"]["entries"]
        .as_array()
        .expect("entries")
        .iter()
        .flat_map(|entry| entry["changes"].as_array().expect("changes").iter())
        .map(|change| change.as_str().expect("text"))
        .collect();
    assert!(
        changes.iter().any(|change| change.contains("really at")),
        "{changes:?}"
    );

    assert_eq!(
        recorder(&["window", &dir, "nothing-like-this"])
            .status
            .code(),
        Some(3)
    );
}

#[test]
fn window_history_pages_with_a_cursor() {
    let dir = session("window-page");

    let first = json(&["window", &dir, "brave", "--limit", "2", "--json"]);
    assert_eq!(first["truncated"], true);
    let cursor = first["next_cursor"].as_str().expect("cursor").to_string();
    let rest = json(&["window", &dir, "brave", "--cursor", &cursor, "--json"]);
    assert_eq!(
        rest["data"]["entries"].as_array().expect("entries").len(),
        1
    );
    assert_eq!(rest["truncated"], false);
}
