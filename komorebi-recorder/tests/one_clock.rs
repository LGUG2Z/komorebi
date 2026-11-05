//! One clock for every time argument, and delivery delay that is not a slow handler, through the
//! real binary.
//!
//! Testkit time: video `t = 0` is 12:00:05 UTC, so video 2.0 s is 12:00:07.

use komorebi_recorder::testkit::SessionBuilder;
use serde_json::Value;
use serde_json::json;
use std::process::Command;

fn run(args: &[&str]) -> std::process::Output {
    Command::new(env!("CARGO_BIN_EXE_komorebi-recorder"))
        .args(args)
        .output()
        .expect("run komorebi-recorder")
}

fn json(args: &[&str]) -> Value {
    let output = run(args);
    assert!(
        output.status.success(),
        "{args:?} failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    serde_json::from_slice(&output.stdout).expect("JSON")
}

fn move_end(pid: i64) -> Value {
    json!({"type": "MoveEnd", "content": [{"type": "Accessibility", "content": "AXWindowMoved"}, pid, 7]})
}

fn focus(pid: i64) -> Value {
    json!({"type": "FocusChange", "content": [{"type": "Accessibility", "content": "AXApplicationActivated"}, pid, 7]})
}

const MOVE_HEADER: &str = "process_event: komorebi::process_event: processing event: MoveEnd for process 10 with notification AXWindowMoved";
const FOCUS_HEADER: &str = "process_event: komorebi::process_event: processing event: FocusChange for process 11 with notification AXApplicationActivated";
const PUSH: &str =
    "process_event: komorebi::notify: pushed notification to subscriber: komorebi-recorder-7";

#[test]
fn at_takes_a_log_time_and_a_video_time_to_the_same_place() {
    let (session, _) = SessionBuilder::new("one-clock-at")
        .event(3.0, move_end(10), None)
        .log(2.0, "INFO", MOVE_HEADER)
        .build();
    let dir = session.dir.display().to_string();

    // 12:00:07 UTC is 2 s into the video
    for time in [
        "2.0",
        "logtime:12:00:07",
        "log-time:12:00:07.000",
        "wall:12:00:07",
        "wall:2026-10-26T12:00:07Z",
    ] {
        let answer = json(&["at", &dir, time, "--window", "100ms", "--json"]);
        let center = answer["center_s"].as_f64().expect("center");
        assert!((center - 2.0).abs() < 1e-3, "{time}: {center}");
    }

    // offsets work, on a log time as on a video time
    let answer = json(&[
        "at",
        &dir,
        "logtime:12:00:07+1s",
        "--window",
        "100ms",
        "--json",
    ]);
    assert!((answer["center_s"].as_f64().expect("center") - 3.0).abs() < 1e-3);
    let answer = json(&["at", &dir, "1.0+1s", "--window", "100ms", "--json"]);
    assert!((answer["center_s"].as_f64().expect("center") - 2.0).abs() < 1e-3);

    // the log time finds the log line, which is at video 2.0
    let answer = json(&[
        "at",
        &dir,
        "logtime:12:00:07",
        "--window",
        "100ms",
        "--json",
    ]);
    assert!(answer["count"].as_u64().expect("count") >= 1, "{answer}");

    // a bare time of day is video time, as it always was
    let answer = json(&["at", &dir, "0:02.5", "--window", "100ms", "--json"]);
    assert!((answer["center_s"].as_f64().expect("center") - 2.5).abs() < 1e-3);

    // an event is a time too
    let answer = json(&["at", &dir, "event:1.end", "--window", "100ms", "--json"]);
    assert!((answer["center_s"].as_f64().expect("center") - 3.0).abs() < 0.05);
}

#[test]
fn range_and_locate_take_a_log_time() {
    let (session, _) = SessionBuilder::new("one-clock-range")
        .event(3.0, move_end(10), None)
        .build();
    let dir = session.dir.display().to_string();

    let answer = json(&[
        "range",
        &dir,
        "logtime:12:00:06",
        "logtime:12:00:08",
        "--json",
    ]);
    assert!((answer["from_s"].as_f64().expect("from") - 1.0).abs() < 1e-3);
    assert!((answer["to_s"].as_f64().expect("to") - 3.0).abs() < 1e-3);

    let located = json(&["locate", &dir, "--utc", "logtime:12:00:07.500", "--json"]);
    assert!(
        (located["video_s"].as_f64().expect("video") - 2.5).abs() < 1e-3,
        "{located}"
    );

    let output = run(&["at", &dir, "logtime:noon", "--json"]);
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("logtime:15:46:22.743"));
}

/// Four notifications komorebi sent half a second apart, all received within 3 ms at 5 s.
fn batched(name: &str, subscriber: bool) -> String {
    let mut builder = SessionBuilder::new(name)
        .event(5.000, move_end(10), None)
        .event(5.001, focus(11), None)
        .event(5.002, move_end(10), None)
        .event(5.003, focus(11), None)
        .log(2.0, "INFO", MOVE_HEADER)
        .log(2.005, "DEBUG", PUSH)
        .log(2.5, "INFO", FOCUS_HEADER)
        .log(2.505, "DEBUG", PUSH)
        .log(3.0, "INFO", MOVE_HEADER)
        .log(3.005, "DEBUG", PUSH)
        .log(3.5, "INFO", FOCUS_HEADER)
        .log(3.505, "DEBUG", PUSH);
    if subscriber {
        builder = builder.subscriber("komorebi-recorder-7");
    }
    let (session, _) = builder.build();
    session.dir.display().to_string()
}

fn findings(dir: &str) -> Vec<Value> {
    json(&["overview", dir, "--json"])["data"]["findings"]
        .as_array()
        .expect("findings")
        .clone()
}

#[test]
fn notifications_received_in_a_clump_are_delivery_batching_not_slow_handlers() {
    let dir = batched("one-clock-batched", true);
    let found = findings(&dir);

    let batching = found
        .iter()
        .find(|finding| finding["kind"] == "delivery-batching")
        .unwrap_or_else(|| panic!("no delivery-batching in {found:#?}"));
    assert_eq!(batching["severity"], "info");
    let summary = batching["summary"].as_str().expect("summary");
    assert!(summary.contains("4 events"), "{summary}");
    assert!(summary.contains("delivery delay"), "{summary}");

    // komorebi handled each in 5 ms: nothing was slow
    assert!(
        found
            .iter()
            .all(|finding| finding["kind"] != "slow-handler"),
        "{found:#?}"
    );

    // the events say when komorebi sent them, beside when they were received
    let events = json(&["events", &dir, "--json"]);
    let first = &events["data"]["events"][0];
    assert!((first["video_s"].as_f64().expect("received") - 5.0).abs() < 1e-3);
    assert!(
        (first["handler"]["pushed_video_s"].as_f64().expect("pushed") - 2.005).abs() < 1e-3,
        "{first}"
    );
    assert!((first["handler"]["delivery_ms"].as_f64().expect("delivery") - 2995.0).abs() < 1.0);
    assert!((first["handler"]["handler_ms"].as_f64().expect("handler") - 5.0).abs() < 1.0);

    // and a trace of one says it was delivery delay
    let trace = String::from_utf8_lossy(&run(&["trace", &dir, "event:2"]).stdout).into_owned();
    assert!(trace.contains("delivery delay"), "{trace}");
}

#[test]
fn without_komorebis_push_time_a_slow_handler_may_be_delivery_delay() {
    let dir = batched("one-clock-unpushed", false);
    let found = findings(&dir);

    let slow = found
        .iter()
        .find(|finding| finding["kind"] == "slow-handler")
        .unwrap_or_else(|| panic!("no slow-handler in {found:#?}"));
    // 3 s is long enough to have been a warning when the numbers were trusted
    assert_eq!(slow["severity"], "info");
    assert!(
        slow["summary"]
            .as_str()
            .expect("summary")
            .contains("may be delivery delay"),
        "{slow}"
    );
    // without push lines only the one event with a header to go by is paired, so there is no
    // batch to describe
}

#[test]
fn a_handler_that_really_was_slow_is_still_a_slow_handler() {
    // received 1.5 s after the handler began, and the push says komorebi sent it just then
    let (session, _) = SessionBuilder::new("one-clock-slow")
        .event(3.5, move_end(10), None)
        .log(2.0, "INFO", MOVE_HEADER)
        .log(3.49, "DEBUG", PUSH)
        .subscriber("komorebi-recorder-7")
        .build();
    let dir = session.dir.display().to_string();

    let found = findings(&dir);
    let slow = found
        .iter()
        .find(|finding| finding["kind"] == "slow-handler")
        .unwrap_or_else(|| panic!("no slow-handler in {found:#?}"));
    assert_eq!(slow["severity"], "warn");
    assert!(
        found
            .iter()
            .all(|finding| finding["kind"] != "delivery-batching")
    );
}

#[test]
fn the_events_listing_shows_komorebis_time_and_which_it_is() {
    // pushed lines present: the time is komorebi's, and the received time follows
    let dir = batched("one-clock-ktime-pushed", true);
    let events = json(&["events", &dir, "--json"]);
    let first = &events["data"]["events"][0];
    assert_eq!(first["komorebi_time"]["source"], "pushed");
    assert!((first["komorebi_time"]["video_s"].as_f64().expect("time") - 2.005).abs() < 1e-3);
    assert!(
        events["data"].get("komorebi_time_note").is_none(),
        "{events}"
    );
    let text = String::from_utf8_lossy(&run(&["events", &dir]).stdout).into_owned();
    assert!(text.contains("00:02.005 p"), "{text}");
    assert!(text.contains("received 00:05.000"), "{text}");

    // no subscriber to match the lines against: received, and it says so
    let dir = batched("one-clock-ktime-received", false);
    let events = json(&["events", &dir, "--json"]);
    let first = &events["data"]["events"][0];
    assert_eq!(first["komorebi_time"]["source"], "received", "{first}");
    assert!(events["data"]["komorebi_time_note"].is_string(), "{events}");
    let text = String::from_utf8_lossy(&run(&["events", &dir]).stdout).into_owned();
    assert!(text.contains("00:05.000 r"), "{text}");
    assert!(text.contains("note: "), "{text}");
}
