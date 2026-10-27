//! Building synthetic sessions for tests, without a screen, komorebi or a recorder.
//!
//! A real recording needs screen-capture permission and a running komorebi, so everything that
//! reads a session is tested against sessions assembled here: a manifest with a clock, komorebi
//! subscriber events with their states, a plaintext log, and recorder incidents, all placed at
//! chosen moments of video time. Not part of the public interface.

use crate::clock::Anchor;
use crate::incidents::INCIDENTS_FILE;
use crate::incidents::Incident;
use crate::session::Manifest;
use crate::session::Moment;
use crate::session::Session;
use crate::sources::komorebi_events::EventRow;
use crate::sources::komorebi_events::state_file_name;
use crate::timeline;
use chrono::Utc;
use serde_json::Value;
use std::path::PathBuf;

/// Video t=0 on the mach clock.
pub const T0_MACH_NS: u64 = 5_000_000_000;
/// What the wall clock reads at mach 0, in microseconds: 2026-10-26T12:00:00Z.
pub const WALL_AT_MACH_ZERO_US: i64 = 1_793_016_000_000_000;

/// The mach time of `video_s` seconds into the video.
#[must_use]
pub fn mach_ns(video_s: f64) -> u64 {
    (T0_MACH_NS as i64 + (video_s * 1e9).round() as i64).max(0) as u64
}

/// The wall time, in microseconds, of `video_s` seconds into the video.
#[must_use]
pub fn wall_us(video_s: f64) -> i64 {
    WALL_AT_MACH_ZERO_US + (mach_ns(video_s) / 1_000) as i64
}

struct PendingVideo {
    display: u32,
    start_s: f64,
    frames: u32,
    fps: u32,
    /// Which picture each frame shows.
    shown: Box<dyn Fn(u32) -> u32>,
}

struct PendingOsRecord {
    video_s: f64,
    /// `komorebi`, `system`, or an application's name.
    source: String,
    pid: i64,
    process: String,
    message: String,
}

struct PendingEvent {
    video_s: f64,
    event: Value,
    state: Option<Value>,
    probe: Option<crate::sources::winprobe::WindowFacts>,
}

/// A session under construction. Times are seconds of video time.
pub struct SessionBuilder {
    dir: PathBuf,
    length_s: f64,
    events: Vec<PendingEvent>,
    log: Vec<(f64, String, String)>,
    incidents: Vec<Incident>,
    os_records: Vec<PendingOsRecord>,
    flavor: crate::analysis::flavor::Flavor,
    komorebi_commit: Option<String>,
    videos: Vec<PendingVideo>,
}

impl SessionBuilder {
    /// A session in a fresh temporary directory, 10 s of video starting 1 s after the recorder
    /// started and ending 1 s before it stopped.
    #[must_use]
    pub fn new(name: &str) -> Self {
        let dir = std::env::temp_dir().join(format!(
            "komorebi-recorder-testkit-{name}-{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        Self {
            dir,
            length_s: 10.0,
            events: Vec::new(),
            log: Vec::new(),
            incidents: Vec::new(),
            os_records: Vec::new(),
            flavor: crate::analysis::flavor::Flavor::Macos,
            komorebi_commit: None,
            videos: Vec::new(),
        }
    }

    /// A display's video file starting `start_s` into the recording: `frames` frames at `fps`,
    /// each numbered (see [`write_numbered_video`]). Needs `ffmpeg`; without it no file is written
    /// and the session has no video, so tests that need one check [`ffmpeg_available`] first.
    #[must_use]
    pub fn video(self, display: u32, start_s: f64, frames: u32, fps: u32) -> Self {
        self.video_showing(display, start_s, frames, fps, |frame| frame)
    }

    /// Like [`SessionBuilder::video`] but frame `n` shows the picture numbered `shown(n)`.
    #[must_use]
    pub fn video_showing(
        mut self,
        display: u32,
        start_s: f64,
        frames: u32,
        fps: u32,
        shown: impl Fn(u32) -> u32 + 'static,
    ) -> Self {
        self.videos.push(PendingVideo {
            display,
            start_s,
            frames,
            fps,
            shown: Box::new(shown),
        });
        self
    }

    /// Say which komorebi was recorded (komorebi for Mac unless told otherwise).
    #[must_use]
    pub fn flavor(mut self, flavor: crate::analysis::flavor::Flavor) -> Self {
        self.flavor = flavor;
        self
    }

    /// Say the recorded komorebi was built from this commit.
    #[must_use]
    pub fn komorebi_commit(mut self, commit: &str) -> Self {
        self.komorebi_commit = Some(commit.to_string());
        self
    }

    /// Say the recording lasted this many seconds of video time.
    #[must_use]
    pub fn length(mut self, seconds: f64) -> Self {
        self.length_s = seconds;
        self
    }

    /// A komorebi subscriber event received at `video_s`, with the state after it if it changed.
    #[must_use]
    pub fn event(mut self, video_s: f64, event: Value, state: Option<Value>) -> Self {
        self.events.push(PendingEvent {
            video_s,
            event,
            state,
            probe: None,
        });
        self
    }

    /// What Windows said about the window of the event added last, as a recording made with
    /// `--probe-windows` stores it.
    #[must_use]
    pub fn probe(mut self, facts: crate::sources::winprobe::WindowFacts) -> Self {
        if let Some(last) = self.events.last_mut() {
            last.probe = Some(facts);
        }
        self
    }

    /// One line of komorebi's plaintext log at `video_s`, e.g. `log(2.0, "INFO", "komorebi::x: hello")`.
    #[must_use]
    pub fn log(mut self, video_s: f64, level: &str, message: &str) -> Self {
        self.log
            .push((video_s, level.to_string(), message.to_string()));
        self
    }

    /// Add an incident of `kind` (see [`crate::incidents::kind`]) that happened at `video_s`.
    #[must_use]
    pub fn incident(mut self, video_s: f64, kind: &str, detail: &str) -> Self {
        self.incidents.push(Incident {
            mach_ns: mach_ns(video_s),
            wall_us: wall_us(video_s),
            kind: kind.to_string(),
            detail: detail.to_string(),
            display: None,
        });
        self
    }

    /// A record of the OS trace at `video_s`: from `komorebi`'s own process, from the `system`, or
    /// from the application with this name.
    #[must_use]
    pub fn os_record(
        mut self,
        video_s: f64,
        source: &str,
        pid: i64,
        process: &str,
        message: &str,
    ) -> Self {
        self.os_records.push(PendingOsRecord {
            video_s,
            source: source.to_string(),
            pid,
            process: process.to_string(),
            message: message.to_string(),
        });
        self
    }

    /// Write the session and index its timeline.
    #[must_use]
    pub fn build(mut self) -> (Session, Manifest) {
        let session = Session::create(Some(self.dir.clone()), Utc::now()).expect("create");

        let start = Moment {
            mach_ns: mach_ns(-1.0),
            wall_us: wall_us(-1.0),
        };
        let stop = Moment {
            mach_ns: mach_ns(self.length_s + 1.0),
            wall_us: wall_us(self.length_s + 1.0),
        };
        let mut manifest = Manifest::new(
            start,
            vec![
                Anchor {
                    mach_ns: start.mach_ns,
                    wall_us: start.wall_us,
                },
                Anchor {
                    mach_ns: stop.mach_ns,
                    wall_us: stop.wall_us,
                },
            ],
        );
        manifest.stop = Some(stop);
        manifest.komorebi.flavor = self.flavor;
        manifest.status = crate::session::STATUS_COMPLETE.to_string();
        manifest.video_t0_mach_ns = Some(T0_MACH_NS);
        manifest.video_end_mach_ns = Some(mach_ns(self.length_s));
        manifest.events.events = self.events.len() as u64;
        if let Some(commit) = &self.komorebi_commit {
            manifest.komorebi.versions = vec![crate::sources::komorebi_version::KomorebiVersion {
                pid: Some(1),
                version: "0.1.0".to_string(),
                branch: Some("master".to_string()),
                commit: Some(commit.clone()),
            }];
        }

        self.events.sort_by(|a, b| a.video_s.total_cmp(&b.video_s));
        let mut state_id = 0_u64;
        let mut previous: Option<Value> = None;
        let mut rows = String::new();
        for (index, pending) in self.events.iter().enumerate() {
            if let Some(state) = &pending.state
                && previous.as_ref() != Some(state)
            {
                state_id += 1;
                std::fs::write(
                    session.path(state_file_name(state_id)),
                    serde_json::to_string(state).expect("state"),
                )
                .expect("state file");
                previous = Some(state.clone());
            }

            let row = EventRow {
                seq: index as u64 + 1,
                recv_wall_us: wall_us(pending.video_s),
                recv_mach_ns: mach_ns(pending.video_s),
                event: Some(pending.event.clone()),
                state_id: (state_id > 0).then_some(state_id),
                parse_error: None,
                raw: None,
                probe: pending.probe.clone(),
            };
            rows.push_str(&serde_json::to_string(&row).expect("row"));
            rows.push('\n');
        }
        std::fs::write(session.path("events.jsonl"), rows).expect("events");

        self.log.sort_by(|a, b| a.0.total_cmp(&b.0));
        // tracing_subscriber right-aligns the level in five columns
        let log: String = self
            .log
            .iter()
            .map(|(video_s, level, message)| {
                format!(
                    "{} {level:>5} {message}\n",
                    timeline::format_wall_us(wall_us(*video_s)).expect("time"),
                )
            })
            .collect();
        std::fs::write(session.path("komorebi.log"), log).expect("log");

        self.incidents.sort_by_key(|incident| incident.mach_ns);
        let incidents: String = self
            .incidents
            .iter()
            .map(|incident| serde_json::to_string(incident).expect("incident") + "\n")
            .collect();
        std::fs::write(session.path(INCIDENTS_FILE), incidents).expect("incidents");

        // the OS trace, one file per source the way the recorder writes them
        self.os_records
            .sort_by(|a, b| a.video_s.total_cmp(&b.video_s));
        let mut files: std::collections::BTreeMap<String, String> =
            std::collections::BTreeMap::new();
        for record in &self.os_records {
            let relative = match record.source.as_str() {
                "komorebi" => "oslog/komorebi.ndjson".to_string(),
                "system" => "oslog/system.ndjson".to_string(),
                "eventlog" => "oslog/eventlog.ndjson".to_string(),
                app => format!("oslog/apps/{app}-{}.ndjson", record.pid),
            };
            let ticks = crate::clock::timebase().ns_to_ticks(mach_ns(record.video_s));
            let line = serde_json::json!({
                "machTimestamp": ticks,
                "timestamp": "2026-10-26 12:00:00.000000-0700",
                "processID": record.pid,
                "processImagePath": format!("/Applications/{}", record.process),
                "subsystem": "com.apple.accessibility.axserver",
                "category": "AX",
                "messageType": "Debug",
                "eventMessage": record.message,
            });
            let file = files.entry(relative).or_default();
            file.push_str(&line.to_string());
            file.push('\n');
        }
        for (relative, text) in &files {
            std::fs::write(session.path(relative), text).expect("os trace file");
        }

        for video in &self.videos {
            let name = crate::capture::video_file_name(video.display, 1);
            let wrote = write_video(&session.path(&name), video.frames, video.fps, &*video.shown);
            if !wrote {
                continue;
            }

            let length = f64::from(video.frames) / f64::from(video.fps);
            manifest.displays.push(crate::session::DisplayEntry {
                result: crate::capture::DisplayResult {
                    display: crate::capture::DisplayDescriptor {
                        id: video.display,
                        frame: [0.0, 0.0, 1000.0, 800.0],
                        width_points: 1000,
                        height_points: 800,
                        asleep: false,
                        scale: 1.0,
                    },
                    segment: 1,
                    scale: 1.0,
                    width_px: NUMBERED_WIDTH as usize,
                    height_px: NUMBERED_HEIGHT as usize,
                    codec: crate::capture::Codec::Hevc,
                    file: name,
                    frames_file: crate::capture::frames_file_name(video.display, 1),
                    video_t0_mach_ns: Some(mach_ns(video.start_s)),
                    video_end_mach_ns: Some(mach_ns(video.start_s + length)),
                    end_reason: Some(crate::capture::EndReason::Stopped),
                    frames: u64::from(video.frames),
                    dropped: 0,
                    idle_callbacks: 0,
                    latency: crate::capture::LatencyStats::default(),
                    error: None,
                },
                offset_s: video.start_s,
                end_s: Some(video.start_s + length),
            });
        }

        session.write_manifest(&manifest).expect("manifest");
        timeline::build(&session).expect("timeline");
        (session, manifest)
    }
}

/// komorebi-style log text from `(video_s, level, text after the level)` lines: the timestamp and
/// the right-aligned level are added the way `tracing_subscriber` does.
#[must_use]
pub fn log_text(lines: &[(f64, &str, &str)]) -> String {
    lines
        .iter()
        .map(|(video_s, level, text)| {
            format!(
                "{} {level:>5} {text}\n",
                timeline::format_wall_us(wall_us(*video_s)).expect("time")
            )
        })
        .collect()
}

/// A komorebi for Windows window as it appears in a `State` or an event.
///
/// `rect` is `[left, top, width, height]`. The shape follows komorebi for Windows' source: every
/// field beside the handle is read live when it is serialized.
#[must_use]
pub fn windows_window(hwnd: i64, exe: &str, rect: Option<[i64; 4]>) -> Value {
    let [left, top, width, height] = rect.unwrap_or([0, 0, 0, 0]);
    serde_json::json!({
        "hwnd": hwnd,
        "title": format!("window {hwnd}"),
        "exe": exe,
        "class": "Window",
        // komorebi's `right` and `bottom` are a width and a height
        "rect": {"left": left, "top": top, "right": width, "bottom": height},
    })
}

/// A komorebi for Windows `State` with one monitor and one tiling workspace whose containers each
/// hold the windows given, so tests can say where windows are without the rest of komorebi's state.
#[must_use]
pub fn windows_state(monitor_id: i64, containers: Vec<Vec<Value>>) -> Value {
    let containers: Vec<Value> = containers
        .into_iter()
        .enumerate()
        .map(|(index, windows)| {
            serde_json::json!({
                "id": format!("c{index}"),
                "locked": false,
                "windows": {"elements": windows, "focused": 0},
            })
        })
        .collect();
    serde_json::json!({
        "monitors": {"elements": [{
            "id": monitor_id,
            "name": "DISPLAY1",
            "device": "DISPLAY1",
            "device_id": "SIM1234-5&abcdef&0&UID1",
            "size": {"left": 0, "top": 0, "right": 1920, "bottom": 1080},
            "work_area_size": {"left": 0, "top": 0, "right": 1920, "bottom": 1040},
            "workspaces": {"elements": [{
                "name": "main",
                "containers": {"elements": containers, "focused": 0},
                "monocle_container": null,
                "maximized_window": null,
                "floating_windows": {"elements": [], "focused": 0},
                "layout": {"Default": "BSP"},
                "latest_layout": [],
                "tile": true,
                "layer": "Tiling",
            }], "focused": 0},
        }], "focused": 0},
        "is_paused": false,
        "virtual_desktop_id": null,
    })
}

/// A komorebi window as it appears in a `State`: `rect` is `[left, top, width, height]`.
#[must_use]
pub fn window(id: u64, exe: &str, rect: Option<[i64; 4]>) -> Value {
    let mut value = serde_json::json!({
        "id": id,
        "details": {"title": format!("window {id}"), "exe": exe},
    });
    if let Some([left, top, width, height]) = rect {
        // komorebi's `right` and `bottom` are a width and a height
        value["rect"] =
            serde_json::json!({"left": left, "top": top, "right": width, "bottom": height});
    }
    value
}

/// A container of komorebi's `State`, named `id`, that holds these windows.
#[must_use]
pub fn container(id: &str, windows: Vec<Value>) -> Value {
    serde_json::json!({"id": id, "windows": {"elements": windows, "focused": 0}})
}

/// A tiling workspace; `layout` is the rects of komorebi's last layout pass, one per container.
#[must_use]
pub fn workspace(name: &str, containers: Vec<Value>, focused: usize, layout: &[[i64; 4]]) -> Value {
    let rects: Vec<Value> = layout
        .iter()
        .map(|[left, top, width, height]| {
            serde_json::json!({"left": left, "top": top, "right": width, "bottom": height})
        })
        .collect();
    serde_json::json!({
        "name": name,
        "layer": "Tiling",
        "tile": true,
        "layout": {"Default": "Columns"},
        "latest_layout": rects,
        "containers": {"elements": containers, "focused": focused},
        "floating_windows": {"elements": [], "focused": 0},
    })
}

/// A monitor of display `id` at `[left, top, width, height]`, hiding windows bottom-left.
#[must_use]
pub fn monitor(id: u64, size: [i64; 4], workspaces: Vec<Value>, focused_workspace: usize) -> Value {
    let [left, top, width, height] = size;
    let rect = serde_json::json!({"left": left, "top": top, "right": width, "bottom": height});
    serde_json::json!({
        "id": id,
        "device": format!("display {id}"),
        "size": rect,
        "work_area_size": rect,
        "window_hiding_position": "BottomLeft",
        "workspaces": {"elements": workspaces, "focused": focused_workspace},
    })
}

/// A `State` of these monitors with the given one focused.
#[must_use]
pub fn state(monitors: Vec<Value>, focused_monitor: usize) -> Value {
    serde_json::json!({
        "monitors": {"elements": monitors, "focused": focused_monitor},
        "is_paused": false,
    })
}

/// A minimal `State`: one 1000x800 monitor, one workspace, one container per `(window id, exe)`,
/// each window in the place the layout says (columns of equal width).
#[must_use]
pub fn state_with_windows(windows: &[(u64, &str)]) -> Value {
    let count = windows.len().max(1) as i64;
    let width = 1000 / count;
    let layout: Vec<[i64; 4]> = (0..count)
        .map(|index| [index * width, 0, width, 800])
        .collect();
    let containers = windows
        .iter()
        .enumerate()
        .map(|(index, (id, exe))| {
            container(
                &format!("c{id}"),
                vec![window(*id, exe, Some(layout[index]))],
            )
        })
        .collect();

    state(
        vec![monitor(
            1,
            [0, 0, 1000, 800],
            vec![workspace("one", containers, 0, &layout)],
            0,
        )],
        0,
    )
}

/// How wide a numbered test video is, in pixels: sixteen blocks of eight, one per bit of the
/// frame number.
pub const NUMBERED_WIDTH: u32 = 128;
/// How tall a numbered test video is, in pixels.
pub const NUMBERED_HEIGHT: u32 = 64;

/// Writes a test video whose every frame says which frame it is.
///
/// The frame number is drawn in binary as sixteen black or white blocks, which survives lossy
/// compression. Returns `false` if there is no `ffmpeg` to make it with; tests that need a video say
/// so and stop.
#[must_use]
pub fn write_numbered_video(path: &std::path::Path, frames: u32, fps: u32) -> bool {
    write_video(path, frames, fps, |frame| frame)
}

/// Like [`write_numbered_video`], but frame `n` shows the picture of frame `shown(n)`, so a video
/// can hold still (`shown` stays put) and change (`shown` advances).
#[must_use]
pub fn write_video(
    path: &std::path::Path,
    frames: u32,
    fps: u32,
    shown: impl Fn(u32) -> u32,
) -> bool {
    use std::io::Write;
    use std::process::Command;
    use std::process::Stdio;

    // the encoders to try, best first: any of them gives a video the decoder must read
    for encoder in [
        ["-c:v", "hevc_videotoolbox", "-tag:v", "hvc1"],
        ["-c:v", "libx264", "-tag:v", "avc1"],
    ] {
        let spawned = Command::new("ffmpeg")
            .args([
                "-y",
                "-loglevel",
                "error",
                "-f",
                "rawvideo",
                "-pix_fmt",
                "gray",
            ])
            .args([
                "-s",
                &format!("{NUMBERED_WIDTH}x{NUMBERED_HEIGHT}"),
                "-r",
                &fps.to_string(),
                "-i",
                "-",
            ])
            .args(["-g", "30", "-pix_fmt", "yuv420p"])
            .args(encoder)
            .arg(path)
            .stdin(Stdio::piped())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn();
        let Ok(mut child) = spawned else {
            return false;
        };

        if let Some(mut stdin) = child.stdin.take() {
            for frame in 0..frames {
                let mut row = vec![0_u8; NUMBERED_WIDTH as usize];
                for bit in 0..16 {
                    let value = if shown(frame) >> (15 - bit) & 1 == 1 {
                        255
                    } else {
                        0
                    };
                    row[bit * 8..bit * 8 + 8].fill(value);
                }
                for _ in 0..NUMBERED_HEIGHT {
                    if stdin.write_all(&row).is_err() {
                        break;
                    }
                }
            }
        }

        if child.wait().is_ok_and(|status| status.success()) && path.is_file() {
            return true;
        }
    }

    false
}

/// Which frame a decoded frame of a numbered video is, read back from its blocks. The frame may
/// be any size at least sixteen pixels wide: each block is sampled at its middle.
#[must_use]
pub fn numbered_frame_index(bgra: &[u8], width: u32, height: u32) -> u32 {
    let row = (height / 2) as usize * width as usize * 4;
    let mut index = 0_u32;
    for bit in 0..16_usize {
        let x = (bit * width as usize / 16) + width as usize / 32;
        // blue channel of a grey pixel
        let value = bgra[row + x * 4];
        index = index << 1 | u32::from(value >= 128);
    }
    index
}

/// The checkout of komorebi for Mac that the tests which read komorebi's source read it from.
///
/// That is the catalog and `code`. It is the checkout `KOMOREBI_MAC_SRC` names, which `nix develop`
/// sets to the revision pinned in `flake.lock`.
///
/// # Panics
///
/// Panics, naming the variable, when it is not set: those tests have nothing to check without a
/// tree.
#[must_use]
pub fn komorebi_mac_src() -> PathBuf {
    std::env::var_os("KOMOREBI_MAC_SRC")
        .map(PathBuf::from)
        .filter(|path| !path.as_os_str().is_empty())
        .unwrap_or_else(|| {
            panic!(
                "KOMOREBI_MAC_SRC is not set: it names the checkout of komorebi for Mac that these \
                 tests read (`nix develop` sets it)"
            )
        })
}

/// Whether the tests that read komorebi for Mac's source can run: `KOMOREBI_MAC_SRC` names a
/// checkout, and this build indexed it (an empty catalog means it was built without one).
///
/// A machine that has neither (Windows, or a Mac outside `nix develop`) has nothing for those
/// tests to check, which is not a failure: they use [`skip_without_komorebi_mac_source`] to say so
/// and stop, as the tests that need `ffmpeg` do.
#[must_use]
pub fn komorebi_mac_source_available() -> bool {
    std::env::var_os("KOMOREBI_MAC_SRC").is_some_and(|path| !path.is_empty())
        && !crate::analysis::catalog::of(crate::analysis::flavor::Flavor::Macos).is_empty()
}

/// Ends the test that uses it, saying why, on a machine that has no komorebi for Mac source to
/// read (see [`komorebi_mac_source_available`]). Use it first in a test that needs the source.
#[macro_export]
macro_rules! skip_without_komorebi_mac_source {
    () => {
        if !$crate::testkit::komorebi_mac_source_available() {
            eprintln!(
                "skipped: KOMOREBI_MAC_SRC is not set, or this build was made without it \
                 (`nix develop` sets it)"
            );
            return;
        }
    };
}

/// Whether `ffmpeg` is there to make test videos with.
#[must_use]
pub fn ffmpeg_available() -> bool {
    std::process::Command::new("ffmpeg")
        .arg("-version")
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .is_ok_and(|status| status.success())
}
