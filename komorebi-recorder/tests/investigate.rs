//! `state`, `diff` and `events` through the real binary, on a session shaped like a real bug: a
//! command retiles, the OS echoes it back, then a focus change leaves a window parked off screen.

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
    serde_json::from_slice(&output.stdout).unwrap_or_else(|error| {
        panic!(
            "{args:?} printed no JSON ({error}): {}",
            String::from_utf8_lossy(&output.stdout)
        )
    })
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

const LAYOUT: [[i64; 4]; 2] = [[0, 0, 500, 800], [500, 0, 500, 800]];

fn tidy() -> Value {
    state(
        vec![monitor(
            1,
            [0, 0, 1000, 800],
            vec![workspace(
                "one",
                vec![
                    container("a", vec![window(1, "Ghostty", Some(LAYOUT[0]))]),
                    container("b", vec![window(2, "Brave", Some(LAYOUT[1]))]),
                ],
                0,
                &LAYOUT,
            )],
            0,
        )],
        0,
    )
}

/// Window 2 parked at the bottom-left sliver although its workspace is showing.
fn stuck() -> Value {
    state(
        vec![monitor(
            1,
            [0, 0, 1000, 800],
            vec![workspace(
                "one",
                vec![
                    container("a", vec![window(1, "Ghostty", Some(LAYOUT[0]))]),
                    container("b", vec![window(2, "Brave", Some([-499, 790, 500, 800]))]),
                ],
                1,
                &LAYOUT,
            )],
            0,
        )],
        0,
    )
}

fn session(name: &str) -> String {
    let move_end = json!({"type": "MoveEnd", "content": [{"type": "Accessibility", "content": "AXWindowMoved"}, 10, 7]});
    let focus = json!({"type": "FocusChange", "content": [{"type": "Accessibility", "content": "AXApplicationActivated"}, 11, 2]});

    let (session, _) = SessionBuilder::new(name)
        .event(2.1, json!({"type": "Retile"}), Some(tidy()))
        .event(3.05, move_end, Some(tidy()))
        .event(5.2, focus, Some(stuck()))
        .log(2.0, "INFO", "process_command: komorebi::process_command: processing command: Retile")
        .log(2.2, "INFO", "process_command: komorebi::process_command: processed command: Retile")
        .log(3.0, "INFO", "process_event: komorebi::process_event: processing event: MoveEnd for process 10 with notification AXWindowMoved")
        .log(5.0, "INFO", "process_event: komorebi::process_event: processing event: FocusChange for process 11 with notification AXApplicationActivated")
        .log(5.1, "ERROR", "process_event:window_rect: komorebi::macos_api: failed to get window position: AXError::InvalidUIElement")
        .build();
    session.dir.display().to_string()
}

#[test]
fn events_are_classified_and_paired_with_the_handler_that_ran() {
    let dir = session("investigate-events");
    let answer = json(&["events", &dir, "--json"]);

    assert_eq!(answer["schema"], "events/1");
    let events = answer["data"]["events"].as_array().expect("events");
    assert_eq!(events.len(), 3);

    assert_eq!(events[0]["name"], "Retile");
    assert_eq!(events[0]["class"], "command");
    assert_eq!(events[0]["handler"]["quality"], "header");

    // the move changed nothing right after a command: an echo, and it says of what
    assert_eq!(events[1]["class"], "echo");
    assert_eq!(events[1]["state_changed"], false);
    assert!(
        events[1]["reason"]
            .as_str()
            .expect("reason")
            .contains("Retile")
    );

    // the focus change took 200 ms to handle and logged an error
    assert_eq!(events[2]["class"], "focus");
    assert_eq!(events[2]["state_changed"], true);
    assert_eq!(events[2]["handler"]["errors"], 1);
    let ms = events[2]["handler"]["handler_ms"].as_f64().expect("ms");
    assert!((ms - 200.0).abs() < 5.0, "handler took {ms} ms");
}

#[test]
fn events_filter_by_class_and_page_with_a_cursor() {
    let dir = session("investigate-filter");

    let quiet = json(&["events", &dir, "--hide-echo", "--json"]);
    let names: Vec<&str> = quiet["data"]["events"]
        .as_array()
        .expect("events")
        .iter()
        .map(|event| event["name"].as_str().expect("name"))
        .collect();
    assert_eq!(names, ["Retile", "FocusChange"]);

    let only_focus = json(&["events", &dir, "--class", "focus", "--json"]);
    assert_eq!(only_focus["data"]["matching"], 1);

    let first = json(&["events", &dir, "--limit", "2", "--json"]);
    assert_eq!(first["truncated"], true);
    let cursor = first["next_cursor"].as_str().expect("cursor").to_string();
    let rest = json(&["events", &dir, "--cursor", &cursor, "--json"]);
    assert_eq!(rest["data"]["events"].as_array().expect("events").len(), 1);
    assert_eq!(rest["data"]["events"][0]["seq"], 3);
    assert_eq!(rest["truncated"], false);
}

#[test]
fn the_state_before_an_event_is_what_the_previous_event_left() {
    let dir = session("investigate-state");

    // event 3 is the one that left window 2 parked: before it, nothing is wrong
    let before = json(&["state", &dir, "--at", "event:3", "--json"]);
    assert_eq!(
        before["data"]["violations"]
            .as_array()
            .expect("violations")
            .len(),
        0
    );
    assert_eq!(before["data"]["origin"]["seq"], 2);

    let after = json(&["state", &dir, "--at", "event:3.end", "--json"]);
    let violations = after["data"]["violations"].as_array().expect("violations");
    assert_eq!(violations.len(), 1);
    assert_eq!(violations[0]["kind"], "visible-window-hidden");
    assert_eq!(violations[0]["window_ids"][0], 2);
    assert_eq!(after["data"]["origin"]["seq"], 3);
    assert_eq!(after["data"]["focus"]["window_id"], 2);
}

#[test]
fn diff_names_what_an_event_did() {
    let dir = session("investigate-diff");

    let answer = json(&["diff", &dir, "event:3", "event:3.end", "--json"]);
    let changes = answer["data"]["changes"].as_array().expect("changes");
    let kinds: Vec<&str> = changes
        .iter()
        .map(|change| change["kind"].as_str().expect("kind"))
        .collect();
    assert!(kinds.contains(&"window-rect"), "{kinds:?}");
    assert!(kinds.contains(&"focus"), "{kinds:?}");

    let rect = changes
        .iter()
        .find(|change| change["kind"] == "window-rect")
        .expect("rect");
    assert_eq!(rect["hidden_before"], false);
    assert_eq!(rect["hidden_after"], true);
    assert!(rect["text"].as_str().expect("text").contains("now hidden"));

    let appeared = answer["data"]["violations_appeared"]
        .as_array()
        .expect("appeared");
    assert_eq!(appeared[0]["kind"], "visible-window-hidden");

    // the command before it changed nothing visible: an echo-sized event diffs empty
    let quiet = json(&["diff", &dir, "event:2", "event:2.end", "--json"]);
    assert_eq!(
        quiet["data"]["changes"].as_array().expect("changes").len(),
        0
    );
}

#[test]
fn state_can_be_asked_about_one_window() {
    let dir = session("investigate-window");

    let answer = json(&[
        "state",
        &dir,
        "--at",
        "event:3.end",
        "--window",
        "brave",
        "--json",
    ]);
    let found = &answer["data"]["matches"][0];
    assert_eq!(found["window"]["id"], 2);
    assert_eq!(found["hidden_position"], true);
    assert_eq!(found["layout_rect"]["left"], 500);
    assert_eq!(found["window"]["rect"]["left"], -499);

    let missing = recorder(&[
        "state",
        &dir,
        "--at",
        "event:3.end",
        "--window",
        "nothing-like-this",
    ]);
    assert_eq!(missing.status.code(), Some(3));
}

#[test]
fn the_text_forms_lead_with_what_matters() {
    let dir = session("investigate-text");

    let state = text(&["state", &dir, "--at", "event:3.end"]);
    assert!(state.contains("as of event #3"), "{state}");
    assert!(state.contains("visible-window-hidden"), "{state}");
    assert!(state.contains("[showing]"), "{state}");

    let diff = text(&["diff", &dir, "event:3", "event:3.end"]);
    assert!(diff.contains("now hidden"), "{diff}");
    assert!(diff.contains("wrong now, and not before"), "{diff}");

    let events = text(&["events", &dir, "--verbose"]);
    assert!(events.contains("echo"), "{events}");
    assert!(events.contains("the OS reporting back"), "{events}");
}

#[test]
fn a_time_before_any_state_was_recorded_is_not_found_not_a_guess() {
    let (session, _) = SessionBuilder::new("investigate-no-state").build();
    let dir = session.dir.display().to_string();

    let output = recorder(&["state", &dir, "--at", "1", "--json"]);
    assert_eq!(output.status.code(), Some(3));
    let body: Value = serde_json::from_slice(&output.stdout).expect("envelope");
    assert_eq!(body["ok"], false);
    assert_eq!(body["error"]["kind"], "not-found");
}

#[test]
fn a_session_that_is_still_recording_is_unusable_with_a_hint() {
    let (session, mut manifest) = SessionBuilder::new("investigate-live").build();
    manifest.status = "recording".to_string();
    session.write_manifest(&manifest).expect("manifest");
    let dir = session.dir.display().to_string();

    let output = recorder(&["events", &dir, "--json"]);
    assert_eq!(output.status.code(), Some(4));
    let body: Value = serde_json::from_slice(&output.stdout).expect("envelope");
    assert_eq!(body["error"]["kind"], "unusable");
    assert!(
        body["error"]["hint"]
            .as_str()
            .expect("hint")
            .contains("reindex")
    );
}
