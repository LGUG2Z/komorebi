//! A session recorded against komorebi for KDE, read end to end through the real commands.
//!
//! The command lines of the log and the commands' events are what a recording of komorebi for KDE
//! running against its in-memory desktop (`komorebi --fake-backend`) contained. The window events
//! are written from komorebi for KDE's source, because that run has no windows: no recording of a
//! real desktop has been read yet, and the rules say so (`verified` is `false`).

use komorebi_recorder::analysis::flavor::Flavor;
use komorebi_recorder::testkit::SessionBuilder;
use serde_json::Value;
use serde_json::json;
use std::process::Command;

fn run(args: &[&str]) -> Value {
    let output = Command::new(env!("CARGO_BIN_EXE_komorebi-recorder"))
        .args(args)
        .output()
        .expect("run komorebi-recorder");
    assert!(
        output.status.success(),
        "{args:?} failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    serde_json::from_slice(&output.stdout).expect("json")
}

/// The spans komorebi for KDE opens for a command and for an event.
const COMMAND: &str = "process_command: komorebi::process_command:";
const EVENT: &str = "handle_event: komorebi::process_event:";

fn window(id: u64, exe: &str) -> Value {
    json!({
        "id": id,
        "rect": {"left": 0, "top": 0, "right": 960, "bottom": 1040},
        "details": {"title": "a title", "exe": exe, "class": exe, "path": format!("/usr/bin/{exe}")}
    })
}

fn session(name: &str) -> String {
    let builder = SessionBuilder::new(name)
        .flavor(Flavor::Kde)
        .log(
            1.0,
            "INFO",
            &format!("{COMMAND} processing command: Retile"),
        )
        .log(
            1.01,
            "INFO",
            &format!("{COMMAND} processed command: Retile"),
        )
        .event(1.02, json!({"type": "Retile"}), None)
        .log(
            2.0,
            "INFO",
            &format!("{EVENT} processing event: Show for window 12 with notification WindowAdded"),
        )
        .log(
            2.005,
            "INFO",
            "handle_event: komorebi::workspace: adding window",
        );
    let (session, _) = builder
        .event(
            2.01,
            json!({"type": "Show", "content": [{"type": "KWin", "content": "WindowAdded"}, window(12, "konsole")]}),
            None,
        )
        .log(
            5.0,
            "INFO",
            &format!("{EVENT} processing event: MoveEnd for window 12 with notification InteractiveMoveFinished"),
        )
        .event(
            5.02,
            json!({"type": "MoveEnd", "content": [{"type": "KWin", "content": "InteractiveMoveFinished"}, window(12, "konsole")]}),
            None,
        )
        .event(7.0, json!("Wake"), None)
        .event(8.0, json!({"DisplayConnectionChange": 0}), None)
        .build();
    session.dir.display().to_string()
}

#[test]
fn events_are_read_with_their_window_and_program() {
    let dir = session("kde-flavor-events");
    let answer = run(&["events", &dir, "--json"]);
    let events = answer["data"]["events"].as_array().expect("events");
    assert_eq!(events.len(), 5);

    assert_eq!(events[0]["name"], "Retile");
    assert_eq!(events[0]["family"], "command");

    let show = &events[1];
    assert_eq!(show["name"], "Show");
    assert_eq!(show["family"], "window-manager");
    assert_eq!(show["window_id"], 12);
    assert_eq!(show["exe"], "konsole");
    assert_eq!(show["notification"], "WindowAdded");
    assert_eq!(show["class"], "lifecycle");

    assert_eq!(events[2]["name"], "MoveEnd");
    assert_eq!(events[2]["class"], "drag");

    // a wake is a notification about the machine, not a command
    assert_eq!(events[3]["name"], "Wake");
    assert_eq!(events[3]["family"], "monitor");
    assert_eq!(events[3]["class"], "lock");
    assert_eq!(events[4]["family"], "monitor");
    assert_eq!(events[4]["class"], "monitor");
}

#[test]
fn a_handler_is_found_from_its_span_and_its_header_line() {
    let dir = session("kde-flavor-handlers");
    let answer = run(&["events", &dir, "--json"]);
    let events = answer["data"]["events"].as_array().expect("events");

    // a command's block ends at `processed command`
    assert_eq!(events[0]["handler"]["lines"], 2, "{}", events[0]);
    // an event's block is the header and what komorebi logged while handling it
    assert!(events[1]["handler"].is_object(), "{}", events[1]);
    assert_eq!(events[1]["handler"]["lines"], 2, "{}", events[1]);
    assert_ne!(
        events[1]["handler"]["header_line"],
        events[2]["handler"]["header_line"]
    );
}

#[test]
fn the_analysis_says_the_kde_rules_are_not_yet_checked_against_a_recording() {
    let dir = session("kde-flavor-warning");
    let answer = run(&["events", &dir, "--json"]);
    let warnings = answer["warnings"].as_array().expect("warnings");
    assert!(
        warnings
            .iter()
            .any(|warning| warning.as_str().is_some_and(
                |text| text.contains("komorebi for KDE") && text.contains("not yet checked")
            )),
        "{warnings:?}"
    );
}
