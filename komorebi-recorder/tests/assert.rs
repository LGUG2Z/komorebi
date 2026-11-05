//! `assert` through the real binary: checks over findings, the log, annotations, motion and blank
//! regions, and what the exit code says.

use komorebi_recorder::testkit;
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

/// A session with an error and a warning in komorebi's log, and one note.
fn session(name: &str) -> String {
    let (session, _) = SessionBuilder::new(name)
        .log(2.0, "INFO", "komorebi::process_event: processing event")
        .log(
            3.0,
            "ERROR",
            "komorebi::window: could not place window 4711",
        )
        .log(4.0, "WARN", "komorebi::reaper: window 12 is gone")
        .build();
    let dir = session.dir.display().to_string();

    let added = recorder(&[
        "annotate", &dir, "add", "--kind", "note", "--start", "2.5", "--text", "step one",
    ]);
    assert!(added.status.success(), "{}", stderr(&added));
    dir
}

#[test]
fn passing_checks_exit_zero_and_print_a_line_each() {
    let dir = session("assert-pass");
    let output = recorder(&[
        "assert",
        &dir,
        "--check",
        "log \"level=ERROR\" count==1",
        "--check",
        "log message~\"window 12\" count>=1",
        "--check",
        "findings kind=layout-mismatch count<=0",
        "--check",
        "annotations kind=note count>=1",
        "--check",
        "annotations kind=bug count==0",
    ]);
    assert_eq!(output.status.code(), Some(0), "{}", stderr(&output));

    let text = stdout(&output);
    let lines: Vec<&str> = text.lines().collect();
    assert_eq!(lines.len(), 6, "{text}");
    assert_eq!(lines[0], "PASS log \"level=ERROR\" count==1 (observed 1)");
    assert_eq!(
        lines[2],
        "PASS findings kind=layout-mismatch count<=0 (observed 0)"
    );
    assert_eq!(lines[5], "all 5 check(s) passed");
}

#[test]
fn a_failing_check_exits_six_and_is_named() {
    let dir = session("assert-fail");
    let output = recorder(&[
        "assert",
        &dir,
        "--check",
        "log \"level=ERROR\" count==0",
        "--check",
        "findings kind=log-error count==1",
        "--check",
        "annotations kind=note count>=3",
    ]);
    assert_eq!(output.status.code(), Some(6), "{}", stderr(&output));

    let text = stdout(&output);
    assert!(
        text.contains("FAIL log \"level=ERROR\" count==0 (observed 1)"),
        "{text}"
    );
    // the line that broke it is shown under the failure
    assert!(text.contains("could not place window 4711"), "{text}");
    assert!(
        text.contains("PASS findings kind=log-error count==1 (observed 1)"),
        "{text}"
    );
    assert!(
        text.contains("FAIL annotations kind=note count>=3 (observed 1)"),
        "{text}"
    );
    assert!(text.contains("2 of 3 check(s): 2 failed"), "{text}");
}

#[test]
fn json_is_an_assert_envelope_and_the_exit_code_still_says_the_result() {
    let dir = session("assert-json");

    let passing = recorder(&[
        "assert",
        &dir,
        "--check",
        "log \"level=WARN\" count==1",
        "--json",
    ]);
    assert_eq!(passing.status.code(), Some(0));
    let envelope: Value = serde_json::from_slice(&passing.stdout).expect("JSON");
    assert_eq!(envelope["ok"], true);
    assert_eq!(envelope["schema"], "assert/1");
    assert_eq!(envelope["data"]["passed"], true);
    assert_eq!(envelope["data"]["checks"][0]["passed"], true);
    assert_eq!(envelope["data"]["checks"][0]["observed"], 1);
    assert_eq!(
        envelope["data"]["checks"][0]["check"],
        "log \"level=WARN\" count==1"
    );

    let failing = recorder(&[
        "assert",
        &dir,
        "--check",
        "log \"level=ERROR\" count==0",
        "--check",
        "log \"level=INFO\" count==1",
        "--json",
    ]);
    assert_eq!(failing.status.code(), Some(6));
    // one document, not an answer followed by an error envelope
    let envelope: Value = serde_json::from_slice(&failing.stdout).expect("one JSON document");
    assert_eq!(envelope["ok"], false);
    assert_eq!(envelope["schema"], "assert/1");
    assert_eq!(envelope["error"]["kind"], "assertion");
    assert_eq!(envelope["error"]["code"], 6);
    assert_eq!(envelope["data"]["passed"], false);
    assert_eq!(envelope["data"]["failed"], 1);
    let checks = envelope["data"]["checks"].as_array().expect("checks");
    assert_eq!(checks[0]["passed"], false);
    assert_eq!(checks[0]["status"], "fail");
    assert_eq!(checks[0]["observed"], 1);
    assert!(
        checks[0]["detail"]
            .as_str()
            .expect("detail")
            .contains("4711")
    );
    assert_eq!(checks[1]["passed"], true);

    let schema = recorder(&["schema", "assert/1"]);
    assert!(schema.status.success(), "{}", stderr(&schema));
}

#[test]
fn malformed_checks_are_usage_errors_that_list_the_kinds() {
    let dir = session("assert-usage");

    for bad in [
        "nonsense count==1",
        "findings",
        "findings count=1",
        "findings count<=lots",
        "log count==0",
        "motion bogus=1 segments==0",
        "blank segments==0",
    ] {
        let output = recorder(&["assert", &dir, "--check", bad]);
        assert_eq!(output.status.code(), Some(2), "{bad}: {}", stderr(&output));
    }

    let output = recorder(&["assert", &dir, "--check", "nonsense count==1"]);
    let message = stderr(&output);
    for kind in ["findings", "log", "motion", "blank", "annotations"] {
        assert!(message.contains(kind), "{message}");
    }

    // a bad logq query is a usage error of that check, not a pass; so is no check at all
    let output = recorder(&["assert", &dir, "--check", "log \"level=\" count==0"]);
    assert_eq!(output.status.code(), Some(2), "{}", stderr(&output));
    assert_eq!(recorder(&["assert", &dir]).status.code(), Some(2));

    // a typo in one check measures nothing, whatever the others would say
    let output = recorder(&[
        "assert",
        &dir,
        "--check",
        "log \"level=ERROR\" count==1",
        "--check",
        "oops count==0",
    ]);
    assert_eq!(output.status.code(), Some(2));
    assert_eq!(stdout(&output), "");
}

#[test]
fn a_session_that_does_not_exist_is_not_found() {
    let output = recorder(&[
        "assert",
        "no-such-session-dir",
        "--check",
        "findings count==0",
    ]);
    assert_eq!(output.status.code(), Some(3), "{}", stderr(&output));
}

#[test]
fn a_check_that_cannot_be_measured_is_an_error_not_a_pass() {
    // no video was recorded, so a motion check has nothing to look at
    let dir = session("assert-no-video");
    let output = recorder(&[
        "assert",
        &dir,
        "--check",
        "annotations count>=1",
        "--check",
        "motion segments<=100",
        "--json",
    ]);
    assert_eq!(output.status.code(), Some(4), "{}", stderr(&output));
    let envelope: Value = serde_json::from_slice(&output.stdout).expect("JSON");
    assert_eq!(envelope["ok"], false);
    assert_eq!(envelope["error"]["kind"], "unusable");
    assert_eq!(envelope["data"]["passed"], false);
    assert_eq!(envelope["data"]["errors"], 1);
    assert_eq!(envelope["data"]["checks"][0]["status"], "pass");
    let motion = &envelope["data"]["checks"][1];
    assert_eq!(motion["status"], "error");
    assert_eq!(motion["passed"], false);
    assert!(
        motion["detail"]
            .as_str()
            .expect("detail")
            .contains("no video")
    );

    let text = recorder(&["assert", &dir, "--check", "motion segments<=100"]);
    assert_eq!(text.status.code(), Some(4));
    assert!(
        stdout(&text).starts_with("ERROR motion segments<=100"),
        "{}",
        stdout(&text)
    );
}

fn walk(dir: &std::path::Path) -> Vec<(String, u64)> {
    let mut out = Vec::new();
    for entry in std::fs::read_dir(dir).expect("read dir").flatten() {
        let path = entry.path();
        if path.is_dir() {
            out.extend(walk(&path));
        } else {
            out.push((
                path.display().to_string(),
                entry.metadata().map_or(0, |meta| meta.len()),
            ));
        }
    }
    out.sort();
    out
}

#[test]
fn assert_leaves_the_session_as_it_found_it() {
    let dir = session("assert-readonly");
    let before = walk(std::path::Path::new(&dir));
    let _ = recorder(&[
        "assert",
        &dir,
        "--check",
        "findings count>=0",
        "--check",
        "log \"level=ERROR\" count==1",
    ]);
    assert_eq!(walk(std::path::Path::new(&dir)), before);
}

#[test]
fn motion_and_blank_checks_measure_the_video() {
    if !testkit::ffmpeg_available() {
        eprintln!("skipped: ffmpeg is not available to make a test video");
        return;
    }

    // 3 s at 30 fps: counting for a second, still for half a second, white for half a second,
    // counting again
    let (session, _) = SessionBuilder::new("assert-video")
        .video_showing(1, 0.0, 90, 30, |frame| match frame {
            0..=29 => frame,
            30..=44 => 29,
            45..=59 => 0xFFFF,
            _ => frame,
        })
        .build();
    let dir = session.dir.display().to_string();

    let output = recorder(&[
        "assert",
        &dir,
        "--check",
        "motion region=500,0,500,800 segments>=2",
        "--check",
        "motion region=0,0,500,800 to=1.4 segments==0",
        "--check",
        "motion from=1.1 to=1.4 active_ms<=100",
        "--check",
        "blank region=0,0,1000,800 color=white ms>=300",
        "--check",
        "blank region=0,0,1000,800 color=white ms<=100",
        "--check",
        "blank region=0,0,1000,800 from=0.5 color=black stretches==0",
    ]);
    let text = stdout(&output);
    assert_eq!(output.status.code(), Some(6), "{text}{}", stderr(&output));
    let lines: Vec<&str> = text.lines().collect();
    assert!(
        lines[0].starts_with("PASS motion region=500,0,500,800 segments>=2"),
        "{text}"
    );
    assert!(
        lines[1].starts_with("PASS motion region=0,0,500,800 to=1.4 segments==0"),
        "{text}"
    );
    assert!(lines[2].starts_with("PASS motion from=1.1"), "{text}");
    assert!(lines[3].starts_with("PASS blank"), "{text}");
    assert!(lines[4].starts_with("FAIL blank"), "{text}");
    assert!(lines[6].starts_with("PASS blank"), "{text}");

    let answer: Value = serde_json::from_slice(
        &recorder(&["assert", &dir, "--check", "motion peak_share>0", "--json"]).stdout,
    )
    .expect("JSON");
    assert_eq!(answer["data"]["passed"], true);
    assert!(answer["data"]["checks"][0]["observed"].is_number());

    // times are TimeSpecs; one that does not exist is that check's error
    let output = recorder(&["assert", &dir, "--check", "motion from=event:9 segments==0"]);
    assert_eq!(output.status.code(), Some(3), "{}", stdout(&output));
}
