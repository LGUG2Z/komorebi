//! A session recorded against komorebi for Windows, read end to end through the real commands.
//!
//! The session is built from what komorebi for Windows' source says it sends and logs, because no
//! recording of it has been read yet (see `docs/dev/windows-checkpoints.md`, W0). When a real one
//! is available this should be run against it too; until then this is what keeps the Windows rules
//! consistent with themselves.

use komorebi_recorder::analysis::flavor::Flavor;
use komorebi_recorder::testkit::SessionBuilder;
use komorebi_recorder::testkit::windows_state;
use komorebi_recorder::testkit::windows_window;
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

/// The handler span komorebi for Windows opens for an event, and the line it ends with.
const FOCUS_SPAN: &str = r#"process_event{event="FocusChange" winevent="SystemForeground" hwnd=131072}: komorebi::process_event:"#;
const MOVE_SPAN: &str = r#"process_event{event="MoveResizeEnd" winevent="SystemMoveSizeEnd" hwnd=131072}: komorebi::process_event:"#;
const WINDOW: &str = "(hwnd: 131072, title: window 131072, exe: chrome.exe, class: Window)";

fn session(name: &str) -> String {
    let before = windows_state(
        65_537,
        vec![vec![windows_window(
            131_072,
            "chrome.exe",
            Some([0, 0, 960, 1040]),
        )]],
    );
    let after = windows_state(
        65_537,
        vec![vec![windows_window(
            131_072,
            "chrome.exe",
            Some([100, 100, 800, 600]),
        )]],
    );
    let (session, _) = SessionBuilder::new(name)
        .flavor(Flavor::Windows)
        .log(2.0, "DEBUG", &format!("{FOCUS_SPAN} focusing window"))
        .log(2.01, "INFO", &format!("{FOCUS_SPAN} processed: {WINDOW}"))
        .event(
            2.02,
            json!({"type": "FocusChange", "content": ["SystemForeground",
                windows_window(131_072, "chrome.exe", Some([0, 0, 960, 1040]))]}),
            Some(before),
        )
        .log(5.0, "DEBUG", &format!("{MOVE_SPAN} moving with mouse"))
        .log(5.01, "INFO", &format!("{MOVE_SPAN} processed: {WINDOW}"))
        .event(
            5.02,
            json!({"type": "MoveResizeEnd", "content": ["SystemMoveSizeEnd",
                windows_window(131_072, "chrome.exe", Some([100, 100, 800, 600]))]}),
            Some(after),
        )
        .event(7.0, json!({"type": "SessionLocked"}), None)
        .event(8.0, json!("LeftAssociatedVirtualDesktop"), None)
        .build();
    session.dir.display().to_string()
}

#[test]
fn events_are_read_with_their_window_handle_program_and_class() {
    let dir = session("windows-flavor-events");
    let answer = run(&["events", &dir, "--json"]);
    let events = answer["data"]["events"].as_array().expect("events");
    assert_eq!(events.len(), 4);

    let focus = &events[0];
    assert_eq!(focus["name"], "FocusChange");
    assert_eq!(focus["family"], "window-manager");
    assert_eq!(focus["window_id"], 131_072);
    assert_eq!(focus["exe"], "chrome.exe");
    assert_eq!(focus["notification"], "SystemForeground");
    assert_eq!(focus["class"], "focus");

    assert_eq!(events[1]["name"], "MoveResizeEnd");
    assert_eq!(events[1]["class"], "drag");

    // notifications about the machine are not commands
    assert_eq!(events[2]["name"], "SessionLocked");
    assert_eq!(events[2]["family"], "monitor");
    assert_eq!(events[2]["class"], "lock");
    assert_eq!(events[3]["family"], "monitor");
    assert_eq!(events[3]["class"], "lock");
}

#[test]
fn a_handler_is_found_from_its_span_and_ends_at_its_processed_line() {
    let dir = session("windows-flavor-handlers");
    let answer = run(&["events", &dir, "--json"]);
    let events = answer["data"]["events"].as_array().expect("events");

    for index in [0, 1] {
        let handler = &events[index]["handler"];
        assert!(
            handler.is_object(),
            "event {index} has no handler: {}",
            events[index]
        );
        assert_eq!(
            handler["lines"], 2,
            "the handler logged two lines: {handler}"
        );
    }
    // the handler of the first did not swallow the second's lines
    assert_ne!(
        events[0]["handler"]["header_line"],
        events[1]["handler"]["header_line"]
    );
}

#[test]
fn the_state_is_read_by_window_handle_and_a_trace_says_what_moved() {
    let dir = session("windows-flavor-trace");
    let trace = run(&["trace", &dir, "event:2", "--json"]);
    let changes = trace["data"]["changes"].as_array().expect("changes");
    assert!(
        changes.iter().any(|change| change["text"]
            .as_str()
            .is_some_and(|text| text.contains("131072"))),
        "the move is described by the window's handle: {changes:#?}"
    );

    let state = run(&["state", &dir, "--at", "6", "--window", "chrome", "--json"]);
    let windows = state["data"]["matches"].as_array().expect("matches");
    assert_eq!(windows.len(), 1, "{state:#}");
    assert_eq!(windows[0]["window"]["id"], 131_072);
}

#[test]
fn the_analysis_says_the_windows_rules_are_not_yet_checked_against_a_recording() {
    let dir = session("windows-flavor-warning");
    let answer = run(&["events", &dir, "--json"]);
    let warnings = answer["warnings"].as_array().expect("warnings");
    assert!(
        warnings
            .iter()
            .any(|warning| warning
                .as_str()
                .is_some_and(|text| text.contains("komorebi for Windows")
                    && text.contains("not yet checked"))),
        "{warnings:?}"
    );
}

#[test]
fn a_mac_session_does_not_get_that_warning() {
    let (mac, _) = SessionBuilder::new("windows-flavor-mac-control")
        .event(1.0, json!({"type": "Retile"}), None)
        .build();
    let answer = run(&["events", &mac.dir.display().to_string(), "--json"]);
    let warnings = answer["warnings"].as_array().expect("warnings");
    assert!(
        warnings
            .iter()
            .all(|warning| !warning.as_str().unwrap_or("").contains("not yet checked")),
        "{warnings:?}"
    );
}

/// A recording made with `--probe-windows` of komorebi showing a window that Windows has cloaked.
fn probed_session(name: &str) -> String {
    use komorebi_recorder::sources::winprobe::WindowFacts;

    let state = windows_state(
        65_537,
        vec![vec![windows_window(
            131_072,
            "chrome.exe",
            Some([0, 0, 960, 1040]),
        )]],
    );
    let cloaked = |at_ns| WindowFacts {
        hwnd: 131_072,
        at_ns,
        exists: true,
        visible: true,
        cloaked: true,
        minimized: false,
        rect: Some([0, 0, 960, 1040]),
        komorebi_says: Some("would manage: yes\n  matched: none".to_string()),
    };
    let focus = || {
        json!({"type": "FocusChange", "content": ["SystemForeground",
            windows_window(131_072, "chrome.exe", Some([0, 0, 960, 1040]))]})
    };

    let (session, _) = SessionBuilder::new(name)
        .flavor(Flavor::Windows)
        .event(2.0, focus(), Some(state.clone()))
        .probe(cloaked(komorebi_recorder::testkit::mach_ns(2.01)))
        .event(4.0, focus(), Some(state))
        .probe(cloaked(komorebi_recorder::testkit::mach_ns(4.01)))
        .build();
    session.dir.display().to_string()
}

#[test]
fn a_window_komorebi_shows_that_windows_cloaked_is_a_finding_in_the_overview() {
    let dir = probed_session("windows-flavor-probed-overview");
    let answer = run(&["overview", &dir, "--json"]);
    let findings = answer["data"]["findings"].as_array().expect("findings");
    let found = findings
        .iter()
        .find(|finding| finding["kind"] == "invariant:visible-window-hidden")
        .unwrap_or_else(|| panic!("no such finding in {findings:#?}"));

    assert_eq!(found["severity"], "error");
    assert!(
        found["summary"]
            .as_str()
            .is_some_and(|text| text.contains("cloaked")),
        "{found}"
    );
    assert_eq!(found["window_ids"][0], 131_072);
}

#[test]
fn a_trace_shows_what_windows_said_and_what_that_says_about_komorebis_state() {
    let dir = probed_session("windows-flavor-probed-trace");
    let trace = run(&["trace", &dir, "event:1", "--json"]);
    let probe = &trace["data"]["window_probe"];
    assert_eq!(probe["facts"]["cloaked"], true);
    assert_eq!(probe["violations"][0]["kind"], "visible-window-hidden");

    let text = Command::new(env!("CARGO_BIN_EXE_komorebi-recorder"))
        .args(["trace", &dir, "event:1"])
        .output()
        .expect("run");
    let text = String::from_utf8_lossy(&text.stdout);
    assert!(
        text.contains("what Windows said about window 131072"),
        "{text}"
    );
    assert!(text.contains("cloaked true"), "{text}");
    assert!(text.contains("komorebi says:"), "{text}");
}

#[test]
fn a_recording_without_probes_says_nothing_about_them() {
    let dir = session("windows-flavor-unprobed");
    let trace = run(&["trace", &dir, "event:1", "--json"]);
    assert!(trace["data"].get("window_probe").is_none(), "{trace}");
}

// Windows and KDE can be asked what the OS says about a window; macOS cannot
#[cfg(not(any(windows, target_os = "linux")))]
#[test]
fn a_host_that_cannot_be_asked_refuses_to_probe_windows_before_it_records_anything() {
    let out = std::env::temp_dir().join(format!("komorebi-probe-refused-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&out);

    let output = Command::new(env!("CARGO_BIN_EXE_komorebi-recorder"))
        .args([
            "record",
            "--probe-windows",
            "--no-video",
            "--duration",
            "1",
            "--out",
        ])
        .arg(&out)
        .output()
        .expect("run");

    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("this host cannot"), "{stderr}");
    assert!(!out.exists(), "no session was started");
}

#[test]
fn asking_for_window_probes_and_against_them_is_an_error_on_any_host() {
    let output = Command::new(env!("CARGO_BIN_EXE_komorebi-recorder"))
        .args([
            "record",
            "--probe-windows",
            "--no-probe-windows",
            "--no-video",
        ])
        .output()
        .expect("run");

    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("cannot be used with"), "{stderr}");
}
