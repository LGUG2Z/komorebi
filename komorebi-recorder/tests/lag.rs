//! `lag` and the `geometry-lag` finding, through the real binary.

use komorebi_recorder::analysis::flavor::Flavor;
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

fn json_of(output: &Output) -> Value {
    serde_json::from_slice(&output.stdout).expect("JSON")
}

fn session(name: &str, flavor: Flavor) -> String {
    let layout = [[0, 0, 960, 1040]];
    let one = state(
        vec![monitor(
            1,
            [0, 0, 1920, 1080],
            vec![workspace(
                "main",
                vec![container("c", vec![window(7, "term", Some(layout[0]))])],
                0,
                &layout,
            )],
            0,
        )],
        0,
    );
    let (session, _) = SessionBuilder::new(name)
        .flavor(flavor)
        .length(10.0)
        .event(1.0, json!({"type": "Retile"}), Some(one))
        .window_track(&[
            (1.0, &[(7, "term", [100, 0, 1060, 1040])]),
            (4.0, &[(7, "term", [0, 0, 960, 1040])]),
        ])
        .build();
    session.dir.display().to_string()
}

#[test]
fn lag_lists_the_episode_as_json_and_as_text() {
    let dir = session("lag-cli", Flavor::Macos);
    let answer = json_of(&recorder(&["lag", &dir, "--json"]));
    assert_eq!(answer["schema"], "lag/1");
    let episode = &answer["data"]["episodes"][0];
    assert_eq!(episode["window_id"], 7);
    assert_eq!(episode["duration_ms"], 2500);
    assert_eq!(episode["max_px"], 100);
    assert_eq!(answer["data"]["unit"], "points");

    let text = String::from_utf8_lossy(&recorder(&["lag", &dir]).stdout).into_owned();
    assert!(text.starts_with("1 episode(s) of geometry lag"), "{text}");
    assert!(text.contains("term (window 7) was off its layout rect for 2.5 s"));

    // a wider grace than the episode leaves nothing
    let none = json_of(&recorder(&["lag", &dir, "--grace-ms", "5000", "--json"]));
    assert!(none["data"]["episodes"].as_array().unwrap().is_empty());
}

#[test]
fn the_finding_is_listed_and_kde_has_none() {
    let dir = session("lag-findings", Flavor::Macos);
    let found = json_of(&recorder(&[
        "findings",
        &dir,
        "--kind",
        "geometry-lag",
        "--json",
    ]));
    assert_eq!(found["data"]["detector"], 1);
    assert_eq!(found["data"]["findings"][0]["kind"], "geometry-lag:window");

    let kde = session("lag-kde-cli", Flavor::Kde);
    let none = json_of(&recorder(&[
        "findings",
        &kde,
        "--kind",
        "geometry-lag",
        "--json",
    ]));
    assert_eq!(none["data"]["total"], 0);
    let answer = json_of(&recorder(&["lag", &kde, "--json"]));
    assert_eq!(answer["data"]["independent"], false);
    assert!(
        answer["data"]["note"]
            .as_str()
            .unwrap()
            .contains("not independent")
    );
}
