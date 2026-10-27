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
