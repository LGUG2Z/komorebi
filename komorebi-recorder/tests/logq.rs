//! `logq` through the real binary: field queries over komorebi's log, counts, group-by, the JSON
//! envelope and what a bad query says.

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

fn text(args: &[&str]) -> String {
    let output = recorder(args);
    assert!(
        output.status.success(),
        "{args:?}: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8_lossy(&output.stdout).into_owned()
}

fn json(args: &[&str]) -> Value {
    serde_json::from_str(&text(args)).expect("JSON")
}

fn session(name: &str) -> String {
    let (session, _) = SessionBuilder::new(name)
        .log(1.0, "INFO", "process_command{MoveWindow(Right)}:move_container_in_direction{direction=Right}: komorebi::window_manager: moving container")
        .log(2.0, "WARN", "process_event{event=\"Show\" winevent=\"ObjectShow\" hwnd=7804828}: komorebi::process_event: ghost tile for 0x77179c")
        .log(3.389, "ERROR", "process_event{event=\"Show\" winevent=\"ObjectShow\" hwnd=7804828}:update{follow=true}: komorebi::workspace: Ghost window dropped")
        .log(4.0, "INFO", "process_event{event=\"Hide\" winevent=\"ObjectHide\" hwnd=99}: komorebi::process_event: hidden")
        .log(5.0, "DEBUG", "komorebi::reaper: reaping")
        .build();
    session.dir.display().to_string()
}

#[test]
fn lines_are_found_by_field_and_listed_with_references() {
    let dir = session("logq-fields");

    let found = text(&["logq", &dir, "level=WARN or level=ERROR"]);
    let lines: Vec<&str> = found.lines().collect();
    assert_eq!(lines.len(), 2, "{found}");
    assert!(
        lines[0].starts_with("log:2  00:02.000  WARN   ghost tile"),
        "{found}"
    );
    assert!(
        lines[1].starts_with("log:3  00:03.389  ERROR  Ghost window"),
        "{found}"
    );

    // hwnd is the same window in decimal and hex
    for query in ["hwnd=7804828", "hwnd=0x77179C"] {
        assert_eq!(
            text(&["logq", &dir, query, "--count"]).trim(),
            "2",
            "{query}"
        );
    }

    assert_eq!(
        text(&["logq", &dir, "command=MoveWindow", "--count"]).trim(),
        "1"
    );
    assert_eq!(
        text(&["logq", &dir, "direction=Right and level=info", "--count"]).trim(),
        "1"
    );
    assert_eq!(
        text(&["logq", &dir, "message~ghost", "--count"]).trim(),
        "2"
    );
    assert_eq!(
        text(&["logq", &dir, "message~\"(?-i)^ghost\"", "--count"]).trim(),
        "1"
    );
    assert_eq!(
        text(&["logq", &dir, "event=Show and not=x", "--count"]).trim(),
        "0"
    );

    // the span chain can be shown, and is trimmed to a line
    let with_spans = text(&["logq", &dir, "command=MoveWindow", "--spans"]);
    assert!(
        with_spans.contains("process_command{MoveWindow(Right)}:move_container_in_direction{direction=Right}  moving container"),
        "{with_spans}"
    );
}

#[test]
fn counting_grouping_and_ranges() {
    let dir = session("logq-group");

    let grouped = text(&["logq", &dir, "hwnd~.", "--group-by", "hwnd"]);
    assert_eq!(grouped, "2  7804828\n1  99\n");

    let levels = text(&["logq", &dir, "message~.", "--group-by", "level"]);
    assert!(levels.starts_with("2  INFO\n"), "{levels}");

    let ranged = text(&[
        "logq",
        &dir,
        "message~.",
        "--from",
        "2.5",
        "--to",
        "4",
        "--count",
    ]);
    assert_eq!(ranged.trim(), "2");
}

#[test]
fn json_is_an_envelope_that_pages() {
    let dir = session("logq-json");

    let page = json(&["logq", &dir, "message~.", "--limit", "2", "--json"]);
    assert_eq!(page["ok"], true);
    assert_eq!(page["schema"], "logq/1");
    assert_eq!(page["truncated"], true);
    assert_eq!(page["next_cursor"], "2");
    assert_eq!(page["data"]["matching"], 5);
    assert_eq!(page["data"]["hits"].as_array().expect("hits").len(), 2);
    assert_eq!(page["data"]["hits"][0]["reference"], "log:1");
    assert_eq!(page["data"]["hits"][0]["source"], "komorebi-log");
    assert_eq!(page["data"]["hits"][0]["level"], "INFO");
    assert_eq!(
        page["data"]["hits"][0]["spans"],
        "process_command{MoveWindow(Right)}:move_container_in_direction{direction=Right}"
    );

    let next = json(&[
        "logq",
        &dir,
        "message~.",
        "--limit",
        "2",
        "--cursor",
        "2",
        "--json",
    ]);
    assert_eq!(next["data"]["hits"][0]["reference"], "log:3");

    let grouped = json(&["logq", &dir, "hwnd~.", "--group-by", "hwnd", "--json"]);
    assert_eq!(grouped["data"]["groups"][0]["value"], "7804828");
    assert_eq!(grouped["data"]["groups"][0]["count"], 2);

    // a field no line has is said, not silently empty
    let typo = json(&["logq", &dir, "hwdn=1", "--json"]);
    assert_eq!(typo["data"]["matching"], 0);
    assert!(typo["warnings"].to_string().contains("hwdn"), "{typo}");

    // the references are the ones `at` and `trace` take
    let at = recorder(&["at", &dir, "3.389"]);
    assert!(at.status.success());

    let schema = text(&["schema", "logq/1"]);
    assert!(schema.contains("matching"));
}

#[test]
fn a_bad_query_is_a_usage_error_with_a_hint() {
    let dir = session("logq-bad");

    for query in ["level ERROR", "", "message~\"(\""] {
        let output = recorder(&["logq", &dir, query, "--json"]);
        assert_eq!(output.status.code(), Some(2), "{query:?}");
        let envelope: Value = serde_json::from_slice(&output.stdout).expect("envelope");
        assert_eq!(envelope["ok"], false);
        assert_eq!(envelope["error"]["kind"], "usage");
        assert!(
            envelope["error"]["hint"]
                .as_str()
                .unwrap_or_default()
                .contains("hwnd"),
            "{envelope}"
        );
    }
}
