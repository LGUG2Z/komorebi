//! The window track: where every top-level window was, sampled at video rate.
//!
//! The recurring question about a recording is "which window was where, in *this* frame". The
//! other sources cannot say: komorebi's `State` is what komorebi believes and arrives at event
//! times, and the per-event probe (`winprobe`) reads one window at the moment of one event. This
//! source asks the OS for every window's rect, many times a second, so any frame has an answer.
//!
//! It is on by default wherever windows can be listed (`record`, at [`DEFAULT_HZ`]); `record
//! --window-track=HZ` picks another rate (clamped to [`MIN_HZ`]..=[`MAX_HZ`]) and
//! `--no-window-track` turns it off. It writes `windows.jsonl` in the session directory.
//!
//! # The file
//!
//! One [`TrackRow`] per line. A row is the **whole** list of windows at an instant, front to
//! back, so a reader never has to replay changes: the state at a time is the last row at or
//! before it (see `Analysis::window_track` for that). A row is written only when something
//! changed since the previous sample, plus a full row marked `"key":true` once a second
//! ([`KEYFRAME_INTERVAL_NS`]), so a still desktop costs about one row a second.
//!
//! ```json
//! {"mach_ns":5000000000,"key":true,"windows":[{"id":131072,"owner":"chrome","title":"Docs","rect":[0,0,960,1040],"visible":true,"minimized":false,"cloaked":false}]}
//! ```
//!
//! What counts as a change is any difference in which windows there are, their order (front to
//! back), rect, visibility, minimized or cloaked state. A window's *title* is not compared, since
//! browsers and terminals change theirs constantly; the title in a row is the current one when the
//! row was written, so it can be a second old on a row that was only a keyframe.
//!
//! Rects are in the host's own units: pixels in the virtual screen on Windows, display points on
//! macOS. Both are the units of a display's `frame` in `session.json`, which is how the `sheet
//! --windows` overlay places them on a frame. `cloaked` is `null` or absent on macOS.
//!
//! # Threads and cost
//!
//! A [`WindowTracker`] owns one thread (`window-track`) that owns the file, as the event recorder's
//! writer does. It enumerates windows once per tick, compares with the previous sample, and
//! appends and flushes at most once a second. If a tick is late by more than a whole interval the
//! sampler does not try to catch up, and counts it in [`WindowTrackInfo::late_ticks`]; the recorder
//! warns when that is common.
//!
//! # Example
//!
//! Change detection is plain data and needs no OS:
//!
//! ```rust
//! use komorebi_recorder::sources::windowtrack::{ChangeFilter, WindowSample};
//!
//! let window = |left| WindowSample {
//!     id: 7,
//!     owner: "term".to_string(),
//!     title: None,
//!     rect: [left, 0, left + 100, 100],
//!     visible: true,
//!     minimized: false,
//!     cloaked: Some(false),
//! };
//!
//! let mut filter = ChangeFilter::default();
//! // the first sample is always written, a still desktop is not, a move is
//! assert!(filter.observe(0, vec![window(0)]).is_some());
//! assert!(filter.observe(33_000_000, vec![window(0)]).is_none());
//! assert!(filter.observe(66_000_000, vec![window(5)]).is_some());
//! // and a second later it is written again as a keyframe
//! assert!(filter.observe(1_100_000_000, vec![window(5)]).is_some_and(|row| row.key));
//! ```

use crate::clock;
use color_eyre::eyre;
use color_eyre::eyre::WrapErr;
use serde::Deserialize;
use serde::Serialize;
use std::fs::File;
use std::io::BufWriter;
use std::io::Write;
use std::path::Path;
use std::sync::Arc;
use std::sync::atomic::AtomicBool;
use std::sync::atomic::Ordering;
use std::thread::JoinHandle;
use std::time::Duration;
use std::time::Instant;

/// The file the track is written to, in the session directory.
pub const WINDOWS_FILE: &str = "windows.jsonl";

/// The rate used when `record` is not told one.
pub const DEFAULT_HZ: u32 = 30;
/// The slowest rate that can be asked for.
pub const MIN_HZ: u32 = 1;
/// The fastest rate that can be asked for.
pub const MAX_HZ: u32 = 120;

/// How long a row can go without being repeated in full, in nanoseconds.
pub const KEYFRAME_INTERVAL_NS: u64 = 1_000_000_000;

/// One window at one instant. See the module docs for the units of `rect`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WindowSample {
    /// The window's id: the handle on Windows (komorebi's `hwnd`), the window number on a Mac.
    pub id: u32,
    /// The program that owns it: its file name without `.exe` on Windows, the owner name on a Mac.
    /// Empty if it could not be found out.
    pub owner: String,
    /// The window's title, if it has one and the host would say.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    /// Left, top, right and bottom edges. For a minimized window on Windows these are the
    /// parked coordinates Windows reports, which are far off screen.
    pub rect: [i32; 4],
    /// Whether the host says the window is shown (on screen, on a Mac).
    pub visible: bool,
    /// Whether the window is minimized. Always false on a Mac, which does not say.
    pub minimized: bool,
    /// Whether the window is cloaked (hidden by the shell, its application, or komorebi, while
    /// still "visible"). `None` where the host has no such thing (a Mac).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cloaked: Option<bool>,
}

impl WindowSample {
    /// Whether `other` is this window in the same place and state; titles are not compared.
    #[must_use]
    pub fn same_as(&self, other: &Self) -> bool {
        self.id == other.id
            && self.owner == other.owner
            && self.rect == other.rect
            && self.visible == other.visible
            && self.minimized == other.minimized
            && self.cloaked == other.cloaked
    }
}

/// One line of `windows.jsonl`: every window at one instant, front to back.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TrackRow {
    /// When the windows were listed, in nanoseconds on the host's master clock.
    pub mach_ns: u64,
    /// Whether the row is there only because a second passed, not because anything changed.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub key: bool,
    /// The windows, front to back.
    pub windows: Vec<WindowSample>,
}

/// Decides which samples become rows: the first, any that differ from the previous sample, and one
/// per [`KEYFRAME_INTERVAL_NS`] otherwise. See the module docs for an example.
#[derive(Debug, Default)]
pub struct ChangeFilter {
    previous: Vec<WindowSample>,
    last_written_ns: Option<u64>,
}

impl ChangeFilter {
    /// Returns the row to write for a sample taken at `mach_ns`, or `None` if nothing changed and
    /// no keyframe is due. A sample is compared with the previous *sample*, not the previous row.
    pub fn observe(&mut self, mach_ns: u64, windows: Vec<WindowSample>) -> Option<TrackRow> {
        let changed = self.last_written_ns.is_none()
            || windows.len() != self.previous.len()
            || windows
                .iter()
                .zip(&self.previous)
                .any(|(now, before)| !now.same_as(before));
        let due = self
            .last_written_ns
            .is_none_or(|at| mach_ns.saturating_sub(at) >= KEYFRAME_INTERVAL_NS);

        self.previous.clone_from(&windows);
        if changed || due {
            self.last_written_ns = Some(mach_ns);
            Some(TrackRow {
                mach_ns,
                key: !changed,
                windows,
            })
        } else {
            None
        }
    }
}

/// What a recording says about its window track, in `session.json` as `window_track`.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct WindowTrackInfo {
    /// The file, relative to the session directory.
    pub file: String,
    /// The rate asked for, in samples a second.
    pub hz: u32,
    /// How many rows were written.
    #[serde(default)]
    pub rows: u64,
    /// How many of those were keyframes only.
    #[serde(default)]
    pub keyframes: u64,
    /// How many samples were taken.
    #[serde(default)]
    pub ticks: u64,
    /// How many samples came more than a whole interval after they were due.
    #[serde(default)]
    pub late_ticks: u64,
    /// Why the sampler stopped early, if it did.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

impl WindowTrackInfo {
    /// What the reader of the recording should be told about this track, if anything: it stopped
    /// early, or more than one sample in ten was late.
    #[must_use]
    pub fn warning(&self) -> Option<String> {
        if let Some(error) = &self.error {
            return Some(format!("the window track stopped early: {error}"));
        }
        if self.ticks >= 10 && self.late_ticks * 10 > self.ticks {
            return Some(format!(
                "the window track fell behind: {} of {} samples were late, so it is coarser than \
                 the {} Hz asked for",
                self.late_ticks, self.ticks, self.hz
            ));
        }
        None
    }
}

/// Returns `hz` limited to [`MIN_HZ`]..=[`MAX_HZ`].
#[must_use]
pub fn clamp_hz(hz: u32) -> u32 {
    hz.clamp(MIN_HZ, MAX_HZ)
}

/// The sampler thread of a recording that was asked for a window track.
pub struct WindowTracker {
    stop: Arc<AtomicBool>,
    handle: Option<JoinHandle<WindowTrackInfo>>,
    hz: u32,
}

impl WindowTracker {
    /// Starts sampling into `windows.jsonl` in `dir`, at `hz` (limited by [`clamp_hz`]).
    ///
    /// # Errors
    ///
    /// The file cannot be created, or the thread cannot be spawned.
    pub fn start(dir: &Path, hz: u32) -> eyre::Result<Self> {
        let hz = clamp_hz(hz);
        let path = dir.join(WINDOWS_FILE);
        let file =
            File::create(&path).wrap_err_with(|| format!("could not create {}", path.display()))?;
        let stop = Arc::new(AtomicBool::new(false));
        let flag = Arc::clone(&stop);
        let handle = std::thread::Builder::new()
            .name("window-track".to_string())
            .spawn(move || sample_loop(BufWriter::new(file), hz, &flag))
            .wrap_err("could not start the window track thread")?;
        Ok(Self {
            stop,
            handle: Some(handle),
            hz,
        })
    }

    /// The manifest entry for a track that has just started and has nothing yet.
    #[must_use]
    pub fn initial_info(&self) -> WindowTrackInfo {
        WindowTrackInfo {
            file: WINDOWS_FILE.to_string(),
            hz: self.hz,
            ..WindowTrackInfo::default()
        }
    }

    /// Stops the sampler, waits for it to flush and close the file, and returns what it did.
    pub fn stop(mut self) -> WindowTrackInfo {
        self.finish()
    }

    fn finish(&mut self) -> WindowTrackInfo {
        self.stop.store(true, Ordering::SeqCst);
        let mut info = self.initial_info();
        match self.handle.take().map(JoinHandle::join) {
            Some(Ok(done)) => info = done,
            Some(Err(_)) => info.error = Some("the sampler thread panicked".to_string()),
            None => {}
        }
        info
    }
}

impl Drop for WindowTracker {
    fn drop(&mut self) {
        if self.handle.is_some() {
            let _ = self.finish();
        }
    }
}

fn sample_loop(mut out: BufWriter<File>, hz: u32, stop: &AtomicBool) -> WindowTrackInfo {
    let interval = Duration::from_secs_f64(1.0 / f64::from(hz));
    let mut info = WindowTrackInfo {
        file: WINDOWS_FILE.to_string(),
        hz,
        ..WindowTrackInfo::default()
    };
    let mut filter = ChangeFilter::default();
    let mut due = Instant::now();
    let mut flushed = Instant::now();

    while !stop.load(Ordering::SeqCst) {
        let mach_ns = clock::mono_now_ns();
        let windows = crate::platform::window::list();
        info.ticks += 1;

        if let Some(row) = filter.observe(mach_ns, windows) {
            let written = serde_json::to_writer(&mut out, &row)
                .map_err(std::io::Error::from)
                .and_then(|()| out.write_all(b"\n"));
            if let Err(error) = written {
                info.error = Some(error.to_string());
                break;
            }
            info.rows += 1;
            info.keyframes += u64::from(row.key);
        }
        if flushed.elapsed() >= Duration::from_secs(1) {
            let _ = out.flush();
            flushed = Instant::now();
        }

        due += interval;
        let now = Instant::now();
        if now > due + interval {
            // not catching up: that would be a burst of samples of the same instant
            info.late_ticks += 1;
            due = now;
        } else if let Some(wait) = due.checked_duration_since(now) {
            // in slices, so a stop is noticed within a tenth of a second at any rate
            let mut left = wait;
            while !left.is_zero() && !stop.load(Ordering::SeqCst) {
                let slice = left.min(Duration::from_millis(100));
                std::thread::sleep(slice);
                left = left.saturating_sub(slice);
            }
        }
    }

    if let Err(error) = out.flush() {
        info.error.get_or_insert_with(|| error.to_string());
    }
    info
}

#[cfg(test)]
mod tests {
    use super::*;

    fn window(id: u32, left: i32) -> WindowSample {
        WindowSample {
            id,
            owner: "app".to_string(),
            title: Some("t".to_string()),
            rect: [left, 0, left + 10, 10],
            visible: true,
            minimized: false,
            cloaked: None,
        }
    }

    #[test]
    fn only_changes_and_a_keyframe_each_second_are_written() {
        let mut filter = ChangeFilter::default();
        let ms = 1_000_000;
        let first = filter.observe(0, vec![window(1, 0)]).expect("first sample");
        assert!(!first.key);
        // a still desktop, sampled at 30 Hz for 0.9 s, writes nothing
        for tick in 1..=27 {
            assert!(filter.observe(tick * 33 * ms, vec![window(1, 0)]).is_none());
        }
        // a different title is not a change
        let mut retitled = window(1, 0);
        retitled.title = Some("other".to_string());
        assert!(filter.observe(900 * ms, vec![retitled]).is_none());
        // the keyframe falls due at one second
        let key = filter
            .observe(1000 * ms, vec![window(1, 0)])
            .expect("keyframe");
        assert!(key.key);
        assert!(filter.observe(1033 * ms, vec![window(1, 0)]).is_none());
    }

    #[test]
    fn a_move_a_new_window_a_state_change_and_a_reorder_each_count() {
        let mut filter = ChangeFilter::default();
        filter.observe(0, vec![window(1, 0), window(2, 0)]);
        let changed = |filter: &mut ChangeFilter, at, windows| filter.observe(at, windows);
        assert!(changed(&mut filter, 1, vec![window(1, 5), window(2, 0)]).is_some());
        assert!(
            changed(
                &mut filter,
                2,
                vec![window(1, 5), window(2, 0), window(3, 0)]
            )
            .is_some()
        );
        assert!(
            changed(
                &mut filter,
                3,
                vec![window(2, 0), window(1, 5), window(3, 0)]
            )
            .is_some()
        );
        let mut hidden = window(3, 0);
        hidden.cloaked = Some(true);
        assert!(changed(&mut filter, 4, vec![window(2, 0), window(1, 5), hidden]).is_some());
        assert!(changed(&mut filter, 5, vec![window(2, 0)]).is_some());
        assert!(changed(&mut filter, 6, vec![window(2, 0)]).is_none());
    }

    #[test]
    fn rows_round_trip_and_leave_out_what_is_empty() {
        let row = TrackRow {
            mach_ns: 5,
            key: false,
            windows: vec![window(1, 0)],
        };
        let text = serde_json::to_string(&row).unwrap();
        assert!(!text.contains("key") && !text.contains("cloaked"), "{text}");
        assert_eq!(serde_json::from_str::<TrackRow>(&text).unwrap(), row);
    }

    #[test]
    fn rates_are_clamped_and_a_slow_sampler_is_called_out() {
        assert_eq!((clamp_hz(0), clamp_hz(30), clamp_hz(1000)), (1, 30, 120));
        let mut info = WindowTrackInfo {
            ticks: 100,
            late_ticks: 5,
            hz: 30,
            ..WindowTrackInfo::default()
        };
        assert!(info.warning().is_none());
        info.late_ticks = 50;
        assert!(info.warning().unwrap().contains("fell behind"));
        info.error = Some("disk full".to_string());
        assert!(info.warning().unwrap().contains("disk full"));
    }

    #[test]
    fn the_sampler_writes_a_first_row_and_a_keyframe() {
        let dir = std::env::temp_dir().join(format!("window-track-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let tracker = WindowTracker::start(&dir, 60).unwrap();
        std::thread::sleep(Duration::from_millis(1300));
        let info = tracker.stop();
        assert!(info.ticks > 10, "{info:?}");
        assert!(info.rows >= 1 && info.error.is_none(), "{info:?}");

        let text = std::fs::read_to_string(dir.join(WINDOWS_FILE)).unwrap();
        let rows: Vec<TrackRow> = text
            .lines()
            .map(|l| serde_json::from_str(l).unwrap())
            .collect();
        assert_eq!(rows.len() as u64, info.rows);
        assert!(
            rows.windows(2)
                .all(|pair| pair[0].mach_ns < pair[1].mach_ns)
        );
        let _ = std::fs::remove_dir_all(&dir);
    }
}
