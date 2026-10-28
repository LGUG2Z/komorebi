//! `changes` and `sheet` through the real binary, on sessions whose video says which frame it is.

use komorebi_recorder::testkit;
use komorebi_recorder::testkit::SessionBuilder;
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

/// Display 1: 3 s at 30 fps that changes for a second, holds still for a second, then changes for
/// a second. Display 2: a 2 s video that only starts 1 s in. An event lands in the second change.
fn session(name: &str) -> Option<String> {
    if !testkit::ffmpeg_available() {
        eprintln!("skipped: ffmpeg is not available to make a test video");
        return None;
    }

    let (session, _) = SessionBuilder::new(name)
        .video_showing(1, 0.0, 90, 30, |frame| match frame {
            0..=29 => frame,
            30..=59 => 29,
            _ => frame,
        })
        .video(2, 1.0, 60, 30)
        .event(
            2.05,
            json!({"type": "FocusChange", "content": [{"type": "Accessibility", "content": "AXApplicationActivated"}, 5, 1]}),
            None,
        )
        .build();
    Some(session.dir.display().to_string())
}

#[test]
fn the_picture_changing_is_found_where_it_changed_and_not_where_it_held_still() {
    let Some(dir) = session("video-changes") else {
        return;
    };

    let answer = json(&["changes", &dir, "--display", "1", "--json"]);
    assert_eq!(answer["schema"], "changes/1");
    let spans = answer["data"]["spans"].as_array().expect("spans");
    assert_eq!(spans.len(), 2, "{spans:#?}");

    let first = &spans[0];
    assert!(first["start_video_s"].as_f64().expect("start") < 0.1);
    assert!((first["end_video_s"].as_f64().expect("end") - 0.97).abs() < 0.06);
    let second = &spans[1];
    assert!((second["start_video_s"].as_f64().expect("start") - 2.0).abs() < 0.06);
    assert!((second["end_video_s"].as_f64().expect("end") - 2.97).abs() < 0.06);

    // the event that landed in the second change is named there and not in the first
    assert_eq!(second["events"], json!([1]));
    assert_eq!(first["events"], json!([]));

    // counting up changes the low bits, which are the blocks on the right of the picture, top to
    // bottom
    let bbox = second["bbox"].as_array().expect("bbox");
    assert!(bbox[0].as_f64().expect("x") >= 0.5, "{bbox:?}");
    assert!(bbox[2].as_f64().expect("width") > 0.3, "{bbox:?}");
    assert!(bbox[3].as_f64().expect("height") > 0.9, "{bbox:?}");
    assert!(second["bbox_points"].is_array());
    assert!(second["peak_share"].as_f64().expect("share") > 0.0);
}

#[test]
fn a_display_with_no_video_for_part_of_the_range_has_a_gap() {
    let Some(dir) = session("video-gaps") else {
        return;
    };

    let answer = json(&["changes", &dir, "--from", "0", "--to", "4", "--json"]);
    let gaps = answer["data"]["gaps"].as_array().expect("gaps");
    let for_two: Vec<(f64, f64)> = gaps
        .iter()
        .filter(|gap| gap["display"] == 2)
        .map(|gap| {
            (
                gap["from_video_s"].as_f64().expect("from"),
                gap["to_video_s"].as_f64().expect("to"),
            )
        })
        .collect();
    assert_eq!(for_two.len(), 2, "{gaps:#?}");
    assert!((for_two[0].0 - 0.0).abs() < 1e-6 && (for_two[0].1 - 1.0).abs() < 1e-6);
    assert!((for_two[1].0 - 3.0).abs() < 1e-6 && (for_two[1].1 - 4.0).abs() < 1e-6);

    // display 1 covers the first three seconds, so it only has a gap after them
    assert!(gaps.iter().filter(|gap| gap["display"] == 1).count() <= 1);

    assert_eq!(
        recorder(&["changes", &dir, "--display", "9"]).status.code(),
        Some(3)
    );
    assert_eq!(
        recorder(&["changes", &dir, "--sensitivity", "0"])
            .status
            .code(),
        Some(2)
    );
}

/// Read what the sheet shows in a cell back out of the PNG as a frame number.
fn frame_shown_in(png_path: &str, frame: &Value) -> u32 {
    let decoder = png::Decoder::new(std::io::BufReader::new(
        std::fs::File::open(png_path).expect("png"),
    ));
    let mut reader = decoder.read_info().expect("info");
    let mut pixels = vec![0; reader.output_buffer_size().expect("size")];
    let info = reader.next_frame(&mut pixels).expect("frame");

    let (x, y) = (
        frame["x"].as_u64().expect("x") as usize,
        frame["y"].as_u64().expect("y") as usize,
    );
    let (width, height) = (
        frame["width"].as_u64().expect("width") as usize,
        frame["height"].as_u64().expect("height") as usize,
    );
    let mut bgra = Vec::with_capacity(width * height * 4);
    for row in 0..height {
        for column in 0..width {
            let at = ((y + row) * info.width as usize + x + column) * 4;
            bgra.extend_from_slice(&[pixels[at + 2], pixels[at + 1], pixels[at], 255]);
        }
    }

    testkit::numbered_frame_index(&bgra, width as u32, height as u32)
}

#[test]
fn a_sheet_shows_the_frame_that_was_on_screen_at_each_moment() {
    let Some(dir) = session("video-sheet") else {
        return;
    };

    let answer = json(&[
        "sheet",
        &dir,
        "--display",
        "1",
        "--at",
        "0.5",
        "--at",
        "1.5",
        "--at",
        "2.5",
        "--width",
        "128",
        "--json",
    ]);
    assert_eq!(answer["schema"], "sheet/1");
    let path = answer["data"]["path"].as_str().expect("path");
    assert!(path.ends_with(".png") && std::path::Path::new(path).is_file());

    let frames = answer["data"]["frames"].as_array().expect("frames");
    assert_eq!(frames.len(), 3);
    // 0.5 s is frame 15; the picture then holds on frame 29's until 2.0 s; 2.5 s is frame 75
    assert_eq!(frame_shown_in(path, &frames[0]), 15);
    assert_eq!(frame_shown_in(path, &frames[1]), 29);
    assert_eq!(frame_shown_in(path, &frames[2]), 75);

    // each frame says when it really was on screen, never later than asked
    for frame in frames {
        let shown = frame["shown_video_s"].as_f64().expect("shown");
        let asked = frame["requested_video_s"].as_f64().expect("asked");
        assert!(shown <= asked + 1e-6 && asked - shown < 1.1, "{frame}");
    }
}

#[test]
fn a_sheet_marks_the_bug_region_and_names_the_events_around_a_frame() {
    let Some(dir) = session("video-sheet-bug") else {
        return;
    };
    let bug = json(&[
        "annotate",
        &dir,
        "add",
        "--kind",
        "bug",
        "--start",
        "2",
        "--end",
        "3",
        "--text",
        "the layout jumped",
        "--json",
    ]);
    let id = bug["data"]["annotation"]["id"]
        .as_str()
        .expect("id")
        .to_string();

    let answer = json(&[
        "sheet",
        &dir,
        "--display",
        "1",
        "--bug",
        &id,
        "--count",
        "4",
        "--json",
    ]);
    let frames = answer["data"]["frames"].as_array().expect("frames");
    assert_eq!(frames.len(), 4);

    // the range runs from before the region to its end; frames inside it are marked
    let marked: Vec<bool> = frames
        .iter()
        .map(|frame| !frame["bugs"].as_array().expect("bugs").is_empty())
        .collect();
    assert_eq!(marked, [false, false, true, true]);
    assert_eq!(frames[3]["bugs"], json!([id]));

    // the focus change at 2.05 s is near the frame at 2.0 s
    let near: Vec<&Value> = frames
        .iter()
        .filter(|frame| frame["events"] == json!([1]))
        .collect();
    assert_eq!(near.len(), 1, "{frames:#?}");
}

#[test]
fn a_moment_a_display_had_no_video_is_skipped_not_invented() {
    let Some(dir) = session("video-sheet-gap") else {
        return;
    };

    let answer = json(&[
        "sheet",
        &dir,
        "--display",
        "2",
        "--at",
        "0.5",
        "--at",
        "1.5",
        "--json",
    ]);
    assert_eq!(
        answer["data"]["frames"].as_array().expect("frames").len(),
        1
    );
    let skipped = answer["data"]["skipped"].as_array().expect("skipped");
    assert_eq!(skipped.len(), 1);
    assert_eq!(skipped[0]["display"], 2);

    // nothing to show at all is not found
    assert_eq!(
        recorder(&["sheet", &dir, "--display", "2", "--at", "0.5"])
            .status
            .code(),
        Some(3)
    );
    // a sheet that would be absurdly large is refused with how to fix it
    let huge = recorder(&[
        "sheet",
        &dir,
        "--count",
        "400",
        "--columns",
        "40",
        "--width",
        "1000",
    ]);
    assert_eq!(huge.status.code(), Some(2));
}

#[test]
fn motion_finds_the_segments_and_a_region_limits_where_it_looks() {
    let Some(dir) = session("video-motion") else {
        return;
    };

    let answer = json(&["motion", &dir, "--display", "1", "--json"]);
    assert_eq!(answer["schema"], "motion/1");
    let segments = answer["data"]["segments"].as_array().expect("segments");
    assert_eq!(segments.len(), 2, "{segments:#?}");
    assert!(segments[0]["peak_share"].as_f64().expect("share") > 0.0);
    assert!(segments[0]["peak_mean_delta"].as_f64().expect("delta") > 0.0);
    let duration = segments[1]["duration_s"].as_f64().expect("duration");
    assert!((duration - 0.97).abs() < 0.1, "{duration}");
    assert_eq!(segments[1]["events"], json!([1]));

    // counting changes the blocks on the right of the picture: the left half of the display (the
    // test display is 1000 x 800 points) holds still, the right half moves in both segments
    let left = json(&[
        "motion",
        &dir,
        "--display",
        "1",
        "--region",
        "0,0,500,800",
        "--json",
    ]);
    assert_eq!(left["data"]["segments"], json!([]));
    let right = json(&[
        "motion",
        &dir,
        "--display",
        "1",
        "--region",
        "500,0,500,800",
        "--json",
    ]);
    let moved = right["data"]["segments"].as_array().expect("segments");
    assert_eq!(moved.len(), 2, "{moved:#?}");
    assert!(moved[0]["bbox_points"][0].as_f64().expect("x") >= 500.0);

    // a region off the display is a usage error, so is a malformed one
    for bad in ["5000,0,10,10", "1,2,3"] {
        let output = recorder(&["motion", &dir, "--region", bad]);
        assert_eq!(output.status.code(), Some(2), "{bad}");
    }
}

#[test]
fn framediff_says_how_much_changed_and_where_and_writes_the_picture() {
    let Some(dir) = session("video-framediff") else {
        return;
    };
    let png = std::path::Path::new(&dir).join("diff.png");
    let png_arg = png.display().to_string();

    // the picture is counting at 0.2 s and 0.5 s
    let moved = json(&[
        "framediff",
        &dir,
        "0.2",
        "0.5",
        "--display",
        "1",
        "--png",
        &png_arg,
        "--json",
    ]);
    assert_eq!(moved["schema"], "framediff/1");
    let data = &moved["data"];
    assert!(data["changed_share"].as_f64().expect("share") > 0.0);
    assert!(data["peak_delta"].as_u64().expect("peak") > 100);
    assert!(data["bbox"][0].as_f64().expect("x") >= 0.5);
    assert!(png.is_file());
    assert_eq!(&std::fs::read(&png).expect("png")[1..4], b"PNG");

    // it holds still from 1.1 s to 1.8 s, and a recording is the same as itself
    let still = json(&["framediff", &dir, "1.1", "1.8", "--display", "1", "--json"]);
    assert_eq!(still["data"]["changed_pixels"], 0);
    assert!(still["data"]["bbox"].is_null());
    let same = json(&[
        "framediff",
        &dir,
        "0.5",
        "--display",
        "1",
        "--against",
        &dir,
        "--json",
    ]);
    assert_eq!(same["data"]["changed_pixels"], 0);

    // without a second time or another session there is nothing to compare with
    assert_eq!(
        recorder(&["framediff", &dir, "0.5", "--display", "1"])
            .status
            .code(),
        Some(2)
    );
}

#[test]
fn blank_finds_the_region_going_white_and_the_picture_freezing() {
    if !testkit::ffmpeg_available() {
        eprintln!("skipped: ffmpeg is not available to make a test video");
        return;
    }

    // a still picture for a second, all white for half a second, the picture again
    let (session, _) = SessionBuilder::new("video-blank")
        .video_showing(1, 0.0, 90, 30, |frame| match frame {
            30..=44 => 0xFFFF,
            _ => 0x5A5A,
        })
        .build();
    let dir = session.dir.display().to_string();

    let answer = json(&["blank", &dir, "--region", "0,0,1000,800", "--json"]);
    assert_eq!(answer["schema"], "blank/1");
    let stretches = answer["data"]["stretches"].as_array().expect("stretches");
    let blank: Vec<&Value> = stretches
        .iter()
        .filter(|stretch| stretch["kind"] == "blank")
        .collect();
    assert_eq!(blank.len(), 1, "{stretches:#?}");
    assert_eq!(blank[0]["colour"], "white");
    assert!((blank[0]["start_video_s"].as_f64().expect("start") - 1.0).abs() < 0.06);
    assert!((blank[0]["duration_s"].as_f64().expect("duration") - 0.5).abs() < 0.1);

    // the picture before and after repeats, frame after frame
    let frozen = stretches
        .iter()
        .filter(|stretch| stretch["kind"] == "frozen")
        .count();
    assert!(frozen >= 2, "{stretches:#?}");

    // a minimum length drops what is shorter, and a region is required
    let long = json(&[
        "blank",
        &dir,
        "--region",
        "0,0,1000,800",
        "--min-ms",
        "5000",
        "--json",
    ]);
    assert_eq!(long["data"]["stretches"], json!([]));
    assert_eq!(recorder(&["blank", &dir]).status.code(), Some(2));
}

#[test]
fn compare_sets_two_recordings_side_by_side() {
    let Some(dir) = session("video-compare") else {
        return;
    };
    // frames come every 33 ms, except for a hole of 300 ms
    let mut csv = String::from("index,pts_mach_ns,received_mach_ns\n");
    let mut at = 1_000_000_000_u64;
    for index in 0..60 {
        csv.push_str(&format!("{index},{at},{at}\n"));
        at += if index == 29 { 300_000_000 } else { 33_000_000 };
    }
    std::fs::write(std::path::Path::new(&dir).join("frames-1.csv"), csv).expect("frame log");

    let answer = json(&["compare", &dir, &dir, "--json"]);
    assert_eq!(answer["schema"], "compare/1");
    let data = &answer["data"];
    assert_eq!(data["a"]["frames"], 60);
    assert!((data["a"]["worst_gap_ms"].as_f64().expect("gap") - 300.0).abs() < 1.0);
    assert_eq!(data["a"]["late_gaps"], 1);
    // two on display 1 and one on display 2
    assert_eq!(data["a"]["motion"]["segments"], 3);
    assert_eq!(data["deltas"]["frames"], 0);
    assert_eq!(data["deltas"]["worst_gap_ms"], 0.0);

    let text = recorder(&["compare", &dir, &dir]);
    assert!(text.status.success());
    assert!(String::from_utf8_lossy(&text.stdout).contains("worst frame gap"));
}

/// The RGB of one pixel of a PNG.
fn pixel(png_path: &str, x: usize, y: usize) -> [u8; 3] {
    let decoder = png::Decoder::new(std::io::BufReader::new(
        std::fs::File::open(png_path).expect("png"),
    ));
    let mut reader = decoder.read_info().expect("info");
    let mut pixels = vec![0; reader.output_buffer_size().expect("size")];
    let info = reader.next_frame(&mut pixels).expect("frame");
    let at = (y * info.width as usize + x) * 4;
    [pixels[at], pixels[at + 1], pixels[at + 2]]
}

#[test]
fn a_sheet_draws_the_windows_of_the_moment_and_says_so_when_there_is_no_track() {
    if !testkit::ffmpeg_available() {
        eprintln!("skipped: ffmpeg is not available to make a test video");
        return;
    }

    // the display is 1000 by 800 points: one window fills its left half, and from 2 s another
    // is in front of the right
    let (session, _) = SessionBuilder::new("video-sheet-windows")
        .video(1, 0.0, 90, 30)
        .window_track(&[
            (0.0, &[(7, "term", [0, 0, 500, 800])]),
            (
                2.0,
                &[
                    (9, "browser", [500, 0, 1000, 800]),
                    (7, "term", [0, 0, 500, 800]),
                ],
            ),
        ])
        .build();
    let dir = session.dir.display().to_string();
    let plain_out = session.path("plain.png").display().to_string();
    let drawn_out = session.path("drawn.png").display().to_string();

    let run = |windows: bool, out: &str| {
        let mut full: Vec<String> = [
            "sheet", &dir, "--at", "1", "--at", "2.5", "--width", "200", "--json", "--out", out,
        ]
        .iter()
        .map(ToString::to_string)
        .collect();
        if windows {
            full.push("--windows".to_string());
        }
        let refs: Vec<&str> = full.iter().map(String::as_str).collect();
        json(&refs)
    };

    let plain = run(false, &plain_out);
    assert!(plain["data"]["frames"][0].get("windows").is_none());
    let drawn = run(true, &drawn_out);
    let frames = drawn["data"]["frames"].as_array().expect("frames");
    assert_eq!(frames[0]["windows"], json!([7]));
    // front to back
    assert_eq!(frames[1]["windows"], json!([9, 7]));
    assert_eq!(drawn["warnings"], plain["warnings"]);

    // the border of the left window runs down the middle of the first cell, and is not in a sheet
    // without windows
    let (x, y, width, height) = (
        frames[0]["x"].as_u64().expect("x") as usize,
        frames[0]["y"].as_u64().expect("y") as usize,
        frames[0]["width"].as_u64().expect("width") as usize,
        frames[0]["height"].as_u64().expect("height") as usize,
    );
    let edge = (x + width / 2 - 1, y + height / 2);
    assert_ne!(
        pixel(&drawn_out, edge.0, edge.1),
        pixel(&plain_out, edge.0, edge.1)
    );
    let inside = (x + width / 4, y + height / 2);
    assert_eq!(
        pixel(&drawn_out, inside.0, inside.1),
        pixel(&plain_out, inside.0, inside.1)
    );

    // a session recorded without a track draws nothing, and says why
    let (bare, _) = SessionBuilder::new("video-sheet-no-windows")
        .video(1, 0.0, 90, 30)
        .build();
    let bare_dir = bare.dir.display().to_string();
    let answer = json(&["sheet", &bare_dir, "--at", "1", "--windows", "--json"]);
    let warnings = answer["warnings"].to_string();
    assert!(warnings.contains("no window track"), "{warnings}");
    assert!(answer["data"]["frames"][0].get("windows").is_none());
}
