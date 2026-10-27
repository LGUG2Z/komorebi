//! The agent's entry points through the real binary: `brief`, `overview`, `search`, `guide` and
//! `schema`, on a session shaped like a docking bug with the user's bug region marked.

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

const LAYOUT: [[i64; 4]; 2] = [[0, 0, 500, 800], [500, 0, 500, 800]];

fn screen(second: [i64; 4]) -> Value {
    state(
        vec![monitor(
            1,
            [0, 0, 1000, 800],
            vec![workspace(
                "one",
                vec![
                    container("a", vec![window(1, "Ghostty", Some(LAYOUT[0]))]),
                    container("b", vec![window(2, "Brave", Some(second))]),
                ],
                1,
                &LAYOUT,
            )],
            0,
        )],
        0,
    )
}

/// A retile, its echo, then at 5 s a display goes away and the next focus change leaves window 2
/// parked off screen on the workspace that is showing.
fn session(name: &str, mark_bug: bool) -> String {
    let notification = |kind: &str, what: &str| json!({"type": kind, "content": [{"type": "Accessibility", "content": what}, 11, 2]});

    let (session, _) = SessionBuilder::new(name)
        .event(2.1, json!({"type": "Retile"}), Some(screen(LAYOUT[1])))
        .event(3.05, notification("MoveEnd", "AXWindowMoved"), Some(screen(LAYOUT[1])))
        .event(5.2, notification("FocusChange", "AXApplicationActivated"), Some(screen([-499, 790, 500, 800])))
        .log(2.0, "INFO", "process_command: komorebi::process_command: processing command: Retile")
        .log(2.2, "INFO", "process_command: komorebi::process_command: processed command: Retile")
        .log(3.0, "INFO", "process_event: komorebi::process_event: processing event: MoveEnd for process 11 with notification AXWindowMoved")
        .log(5.0, "INFO", "process_event: komorebi::process_event: processing event: FocusChange for process 11 with notification AXApplicationActivated")
        .log(5.1, "ERROR", "process_event:window_rect: komorebi::macos_api: failed to get window position: AXError::InvalidUIElement")
        .incident(4.9, "display-disconnected", "display 3 went away")
        .komorebi_commit("0000000000000000000000000000000000000000")
        .build();
    let dir = session.dir.display().to_string();

    if mark_bug {
        let output = recorder(&[
            "annotate",
            &dir,
            "add",
            "--kind",
            "bug",
            "--start",
            "4.5",
            "--end",
            "7",
            "--text",
            "After unplugging the monitor my Brave window is gone from the screen",
            "--expected",
            "it should move to the laptop display",
        ]);
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
    }
    dir
}

#[test]
fn the_brief_leads_with_the_users_words_and_points_at_the_event_that_broke_it() {
    let dir = session("brief-bug", true);

    let answer = json(&["brief", &dir, "--json"]);
    assert_eq!(answer["schema"], "brief/1");
    let data = &answer["data"];

    // the user's description, verbatim, with the region
    let bug = &data["bugs"][0]["bug"];
    assert_eq!(
        bug["text"],
        "After unplugging the monitor my Brave window is gone from the screen"
    );
    assert_eq!(bug["expected"], "it should move to the laptop display");
    assert!((bug["start_video_s"].as_f64().expect("start") - 4.5).abs() < 1e-6);
    assert!((bug["end_video_s"].as_f64().expect("end") - 7.0).abs() < 1e-6);

    let region = &data["bugs"][0]["region"];

    // the first suspect is the focus change that left the window parked, and says why
    let suspect = &region["suspects"][0];
    assert_eq!(suspect["event"]["seq"], 3);
    let reasons: Vec<&str> = suspect["reasons"]
        .as_array()
        .expect("reasons")
        .iter()
        .map(|reason| reason.as_str().expect("reason"))
        .collect();
    assert!(
        reasons
            .iter()
            .any(|reason| reason.contains("visible-window-hidden")),
        "{reasons:?}"
    );
    assert!(
        reasons
            .iter()
            .any(|reason| reason.contains("logged 1 error")),
        "{reasons:?}"
    );

    // the state as the bug began was fine and as it ended was wrong
    assert_eq!(
        region["state_at_start"]["violations"]
            .as_array()
            .expect("v")
            .len(),
        0
    );
    assert_eq!(
        region["state_at_end"]["violations"][0]["kind"],
        "visible-window-hidden"
    );

    // the display that went away is there, and so is what changed
    assert_eq!(region["incidents"][0]["kind"], "display-disconnected");
    let changes: Vec<&str> = region["changes"]
        .as_array()
        .expect("changes")
        .iter()
        .map(|change| change["kind"].as_str().expect("kind"))
        .collect();
    assert!(changes.contains(&"window-rect"), "{changes:?}");

    // the echo of the retile is counted but not listed
    assert_eq!(region["events"]["echoes_hidden"], 1);
    let listed: Vec<&str> = region["events"]["listed"]
        .as_array()
        .expect("listed")
        .iter()
        .map(|event| event["class"].as_str().expect("class"))
        .collect();
    assert!(!listed.contains(&"echo"), "{listed:?}");
    // a commit the repository does not have cannot be compared, which is said, not assumed
    assert_eq!(data["skew"]["verdict"], "unknown");
    assert!(
        answer["warnings"]
            .as_array()
            .expect("warnings")
            .iter()
            .any(|w| w.as_str().is_some_and(|text| text.contains("0000000000")))
    );
}

#[test]
fn the_text_brief_starts_with_the_bug_as_reported() {
    let dir = session("brief-text", true);
    let brief = text(&["brief", &dir]);

    assert!(
        brief.starts_with("BUG, as the user reported it:"),
        "{brief}"
    );
    let words = brief
        .find("After unplugging the monitor my Brave window is gone")
        .expect("words");
    let evidence = brief.find("most worth tracing first").expect("suspects");
    assert!(
        words < evidence,
        "the user's words come before the evidence"
    );
    assert!(brief.contains("komorebi-recorder trace"), "{brief}");
    assert!(brief.contains("--kind finding --about"), "{brief}");
}

#[test]
fn without_a_bug_region_the_brief_says_so_and_digests_the_whole_recording() {
    let dir = session("brief-no-bug", false);
    let answer = json(&["brief", &dir, "--json"]);

    assert_eq!(answer["data"]["bugs"].as_array().expect("bugs").len(), 0);
    assert!(
        answer["data"]["whole_session"]["events"]["total"]
            .as_u64()
            .expect("total")
            >= 3
    );
    let next = answer["data"]["next"][0].as_str().expect("next");
    assert!(next.contains("no bug region was marked"), "{next}");

    assert!(text(&["brief", &dir]).starts_with("NO BUG REGION WAS MARKED"));
}

#[test]
fn findings_in_the_bug_region_rank_above_the_rest() {
    let dir = session("overview-rank", true);

    let answer = json(&["overview", &dir, "--json"]);
    let findings = answer["data"]["findings"].as_array().expect("findings");
    assert!(findings.len() >= 3, "{findings:#?}");

    // the stuck window overlaps the bug region and is an error, so it leads
    assert_eq!(findings[0]["kind"], "invariant:visible-window-hidden");
    assert_eq!(findings[0]["persisted_to_end"], true);
    assert_eq!(findings[0]["bugs"].as_array().expect("bugs").len(), 1);
    // and an ERROR line in the log and the unplugged display are found too
    let kinds: Vec<&str> = findings
        .iter()
        .map(|f| f["kind"].as_str().expect("kind"))
        .collect();
    assert!(kinds.contains(&"log-error"), "{kinds:?}");
    assert!(
        kinds.contains(&"incident:display-disconnected"),
        "{kinds:?}"
    );

    // scores are descending
    let scores: Vec<i64> = findings
        .iter()
        .map(|f| f["score"].as_i64().expect("score"))
        .collect();
    assert!(
        scores.windows(2).all(|pair| pair[0] >= pair[1]),
        "{scores:?}"
    );

    assert!(
        answer["data"]["histogram"]
            .as_array()
            .expect("histogram")
            .len()
            > 5
    );
    assert!(text(&["overview", &dir]).contains("findings, most worth looking at first"));
}

#[test]
fn search_finds_words_across_sources_and_gives_references() {
    let dir = session("search", true);

    let answer = json(&["search", &dir, "InvalidUIElement", "--json"]);
    let hits = answer["data"]["hits"].as_array().expect("hits");
    assert_eq!(hits.len(), 1);
    assert_eq!(hits[0]["source"], "komorebi-log");
    assert_eq!(hits[0]["reference"], "log:5");

    // events, incidents and annotations are searched too, case-insensitively
    let many = json(&["search", &dir, "brave", "--json"]);
    let sources: Vec<&str> = many["data"]["hits"]
        .as_array()
        .expect("hits")
        .iter()
        .map(|hit| hit["source"].as_str().expect("source"))
        .collect();
    assert!(sources.contains(&"annotations"), "{sources:?}");

    let incident = json(&["search", &dir, "display 3", "--json"]);
    assert_eq!(incident["data"]["hits"][0]["reference"], "incident:1");

    let regex = json(&[
        "search",
        &dir,
        "^processing (event|command)",
        "--regex",
        "--json",
    ]);
    // two event headers and one command header; `processed command` does not match
    assert_eq!(regex["data"]["matching"], 3);

    // a reference from a hit goes straight into another command
    let traced = json(&["code", &dir, "log:5", "--json"]);
    assert_eq!(traced["data"]["lines"][0]["message"], hits[0]["text"]);

    assert_eq!(
        recorder(&["search", &dir, "(", "--regex"]).status.code(),
        Some(2)
    );
    assert_eq!(recorder(&["search", &dir, ""]).status.code(), Some(2));
}

#[test]
fn search_pages_with_a_cursor() {
    let dir = session("search-page", true);

    let first = json(&["search", &dir, "process", "--limit", "2", "--json"]);
    assert_eq!(first["truncated"], true);
    let cursor = first["next_cursor"].as_str().expect("cursor").to_string();
    let next = json(&[
        "search", &dir, "process", "--limit", "2", "--cursor", &cursor, "--json",
    ]);
    let first_refs: Vec<&str> = first["data"]["hits"]
        .as_array()
        .expect("hits")
        .iter()
        .map(|hit| hit["reference"].as_str().expect("ref"))
        .collect();
    for hit in next["data"]["hits"].as_array().expect("hits") {
        assert!(!first_refs.contains(&hit["reference"].as_str().expect("ref")));
    }
}

#[test]
fn the_guide_and_the_schemas_describe_the_outputs() {
    let guide = text(&["guide"]);
    assert!(guide.contains("## 1. Start with the brief"));
    assert!(guide.contains("annotate <session> add --kind finding"));
    let wrapped = json(&["guide", "--json"]);
    assert!(
        wrapped["data"]["guide"]
            .as_str()
            .expect("guide")
            .contains("Investigating a komorebi recording")
    );

    let listed = text(&["schema"]);
    for name in [
        "brief/1",
        "overview/1",
        "state/1",
        "trace/1",
        "code/1",
        "error/1",
    ] {
        assert!(listed.contains(name), "{name} missing from:\n{listed}");
    }

    let schema = json_stdout(&["schema", "brief/1"]);
    assert_eq!(schema["title"], "BriefData");
    assert!(schema["properties"]["bugs"].is_object());
    assert_eq!(recorder(&["schema", "nothing/9"]).status.code(), Some(3));
}

/// Every `schema` an envelope names can be asked for.
#[test]
fn every_command_names_a_schema_that_exists() {
    let dir = session("schema-names", true);
    let ids = json(&["annotations", &dir, "--json"]);
    let bug = ids["data"]["annotations"][0]["id"]
        .as_str()
        .expect("id")
        .to_string();

    let commands: Vec<Vec<String>> = vec![
        vec!["brief".into(), dir.clone()],
        vec!["overview".into(), dir.clone()],
        vec![
            "state".into(),
            dir.clone(),
            "--at".into(),
            "event:3.end".into(),
        ],
        vec![
            "diff".into(),
            dir.clone(),
            "event:3".into(),
            "event:3.end".into(),
        ],
        vec!["events".into(), dir.clone()],
        vec!["trace".into(), dir.clone(), "event:3".into()],
        vec!["window".into(), dir.clone(), "brave".into()],
        vec!["code".into(), dir.clone(), "log:5".into()],
        vec!["search".into(), dir.clone(), "event".into()],
        vec!["annotations".into(), dir.clone()],
        vec![
            "annotate".into(),
            dir.clone(),
            "edit".into(),
            bug,
            "--title".into(),
            "t".into(),
        ],
    ];

    for command in commands {
        let mut args: Vec<&str> = command.iter().map(String::as_str).collect();
        args.push("--json");
        let answer = json(&args);
        let name = answer["schema"].as_str().expect("schema");
        assert!(
            recorder(&["schema", name]).status.success(),
            "{args:?} names the schema '{name}', which `schema` does not know"
        );
    }
}

fn json_stdout(args: &[&str]) -> Value {
    json(args)
}
