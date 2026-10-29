//! Geometry lag: how long, and by how much, a window was off the rect komorebi's layout gave it.
//!
//! komorebi's `State` says where each tiled window should be (`layout_rects`, the target of the last
//! layout pass), and the window track ([`crate::sources::windowtrack`]) says where the OS had the
//! window, many times a second. A window that is not where the layout put it for a moment is
//! ordinary: it is being moved. One that stays there is the bug people see as a window that "catches
//! up late" or never does. This module finds each such stretch, an *episode*, and says when it
//! began and ended, how far off the window was at worst and at the end, and, where the track has
//! komorebi's border windows, whether the border or the window was the one that lagged.
//!
//! The `layout-mismatch` invariant ([`super::invariants`]) judges one state at a time; this is the
//! time dimension over the track, so it can say how long.
//!
//! # What is measured
//!
//! - **A window off its layout rect** ([`Subject::Window`]). The target is the rect the latest state
//!   gave the window's container, for the window in front of a tiled container on a showing
//!   workspace (the same windows `layout-mismatch` looks at: not a floating, monocle or maximized
//!   window). The offset is the window's edge minus the target's edge, for each of left, top, right
//!   and bottom, so a negative `left` is a window that sticks out to the left. A window that is
//!   hidden, minimized or cloaked has no meaningful offset and ends the episode.
//! - **A border off its window** ([`Subject::Border`]). Only when the track has rows marked
//!   `"border": true` (komorebi's border is a window of its own); a track without them is normal and
//!   says nothing about borders. A border is drawn around its window, larger by a width the recorder
//!   does not know, so the border's rect is not compared with the window's directly. What is
//!   measured is whether the border is *evenly* around the window: each edge's gap is compared
//!   with the median of the four gaps, and the offset is how far each edge is from where an even
//!   border would put it. A border row names its window with `border_of` when the track has it;
//!   otherwise it belongs to the window it overlaps most (at least half of their union), and keeps
//!   that window while that window is there.
//!
//! An episode's [`Lagged`] says which one lagged: `window` (the window was off its layout rect and
//! the border went along with it), `border` (the window was where it should be and its border was
//! not), `both`, or `unknown` (a window episode with no border to compare).
//!
//! # What is not flagged
//!
//! - **Under the tolerance.** An offset of [`DEFAULT_TOLERANCE`] or less on every edge is not off.
//!   The default is 8, a little more than the 4 of `layout-mismatch`, because this compares the
//!   *track's* rect (what the OS reports for the window, which on Windows may include the invisible
//!   resize border) with the layout, where that one compares komorebi's own read. It is a guess to
//!   be tuned against real Windows recordings; `--tolerance` changes it.
//! - **A window that is animating.** komorebi can animate a window's move or resize over a fraction
//!   of a second, during which the window is, by design, off the new layout rect. State snapshots
//!   already carry the target of the animation, so there is nothing to tell the animation apart by
//!   except time: for [`DEFAULT_GRACE_MS`] (500 ms, which is komorebi's default animation duration
//!   of 250 ms and the same again for the layout to be applied and sampled) after the layout rect
//!   of a window changes, or the window first has one, it is not flagged, and an episode is
//!   counted from the end of that grace if the window is still off. The grace is
//!   `--grace-ms`. The border gets the same grace after its window last moved, so that a border
//!   that follows a moving window a frame behind is not a lag.
//!
//! An episode ends at the first sample that is back in tolerance (or the window is gone or hidden),
//! so its duration can be longer than the real one by up to the sampling interval (33 ms at the
//! default 30 Hz; a still desktop is sampled once a second, but nothing is moving then). One still
//! going at the end of the recording is `persisted_to_end` and lasts to the end of the track.
//!
//! # Hosts
//!
//! Rects are in pixels on Windows and in display points on a Mac (the units of the track), and
//! [`LagData::unit`] says which. For **KDE** the track is made from komorebi's own state, so a
//! window's rect in it and its layout rect are one source, and comparing them finds komorebi's
//! disagreements with itself and not the compositor's lag: the answer has no episodes and says so
//! ([`LagData::independent`] is `false`), and the detector raises no findings.
//!
//! # Example
//!
//! A window that is slow to reach its half of the screen:
//!
//! ```rust
//! use komorebi_recorder::analysis::Analysis;
//! use komorebi_recorder::analysis::lag::{Lagged, LagOptions, Subject};
//! use komorebi_recorder::testkit::{SessionBuilder, container, monitor, state, window, workspace};
//! use serde_json::json;
//!
//! let layout = [[0, 0, 960, 1040]];
//! let one = state(vec![monitor(1, [0, 0, 1920, 1080], vec![workspace(
//!     "main",
//!     vec![container("c", vec![window(7, "term", Some(layout[0]))])],
//!     0,
//!     &layout,
//! )], 0)], 0);
//!
//! let (session, _) = SessionBuilder::new("doc-lag")
//!     .length(10.0)
//!     .event(1.0, json!({"type": "Retile"}), Some(one))
//!     // 1.0 to 4.0 s: 100 points to the right of where the layout put it, then in place
//!     .window_track(&[
//!         (1.0, &[(7, "term", [100, 0, 1060, 1040])]),
//!         (4.0, &[(7, "term", [0, 0, 960, 1040])]),
//!     ])
//!     .build();
//!
//! let data = Analysis::open(&session.dir)?.geometry_lag(&LagOptions::default())?;
//! assert_eq!(data.episodes.len(), 1);
//! let episode = &data.episodes[0];
//! assert_eq!((episode.subject, episode.window_id), (Subject::Window, 7));
//! assert_eq!(episode.lagged, Lagged::Unknown); // there is no border in this track
//! assert_eq!(episode.max_px, 100);
//! assert_eq!(episode.max_offset.left, 100);
//! // off from the end of the 500 ms grace after the layout, 1.5 s, until 4.0 s
//! assert_eq!(episode.duration_ms, 2500);
//! # Ok::<(), Box<dyn std::error::Error>>(())
//! ```

use super::Analysis;
use super::flavor::Flavor;
use super::invariants::Severity;
use super::overview::Finding;
use super::state::Snapshot;
use crate::sources::windowtrack::KEYFRAME_INTERVAL_NS;
use crate::sources::windowtrack::WINDOWS_FILE;
use color_eyre::eyre;
use color_eyre::eyre::WrapErr;
use schemars::JsonSchema;
use serde::Deserialize;
use serde::Serialize;
use serde_json::Value;
use std::collections::BTreeMap;
use std::collections::HashMap;

/// The schema name that identifies a [`LagData`] in an envelope (`lag/1`).
pub const SCHEMA_LAG: &str = "lag/1";

/// The default of [`LagOptions::tolerance`], in pixels or points.
pub const DEFAULT_TOLERANCE: i64 = 8;

/// The default of [`LagOptions::grace_ms`].
pub const DEFAULT_GRACE_MS: u64 = 500;

/// An episode this long, or one still going at the end, is a `warn` finding; a shorter one is
/// `info`.
const WARN_AFTER_NS: u64 = 1_000_000_000;

/// What counts as lag, and what does not.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LagOptions {
    /// The most an edge may be off, in the units of the track, before the window is off.
    pub tolerance: i64,
    /// How long after a layout change (or a window moving, for its border) nothing is flagged, in
    /// milliseconds.
    pub grace_ms: u64,
    /// Only this window, by id.
    pub window: Option<u64>,
}

impl Default for LagOptions {
    fn default() -> Self {
        Self {
            tolerance: DEFAULT_TOLERANCE,
            grace_ms: DEFAULT_GRACE_MS,
            window: None,
        }
    }
}

/// What was off its rect.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, JsonSchema)]
#[serde(rename_all = "kebab-case")]
pub enum Subject {
    /// A window, off the rect komorebi's layout gave it.
    Window,
    /// A border, not evenly around its window.
    Border,
}

/// Which one lagged.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, JsonSchema)]
#[serde(rename_all = "kebab-case")]
pub enum Lagged {
    /// The window was off its layout rect and its border (if it has one) stayed with it.
    Window,
    /// The window was where the layout put it, and its border was not around it.
    Border,
    /// Both were off.
    Both,
    /// A window was off its layout rect and there is no border in the track to say about.
    Unknown,
}

/// A signed offset on each edge: where the thing was minus where it should have been.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, JsonSchema)]
pub struct Edges {
    /// The left edge: negative is too far left.
    pub left: i64,
    /// The top edge: negative is too high.
    pub top: i64,
    /// The right edge: positive is too far right.
    pub right: i64,
    /// The bottom edge: positive is too low.
    pub bottom: i64,
}

impl Edges {
    fn of(values: [i64; 4]) -> Self {
        Self {
            left: values[0],
            top: values[1],
            right: values[2],
            bottom: values[3],
        }
    }

    /// The largest offset of any edge, ignoring its sign.
    #[must_use]
    pub fn largest(&self) -> i64 {
        [self.left, self.top, self.right, self.bottom]
            .iter()
            .map(|edge| edge.abs())
            .max()
            .unwrap_or(0)
    }
}

/// One stretch of a window (or its border) being off its rect.
#[derive(Debug, Clone, PartialEq, Serialize, JsonSchema)]
pub struct LagEpisode {
    /// What was off.
    pub subject: Subject,
    /// Which one lagged; see the [module](self).
    pub lagged: Lagged,
    /// The window's id: its handle on Windows. For a border, the window it belongs to.
    pub window_id: u64,
    /// The program that owns the window, if the track knew it.
    pub owner: String,
    /// When it began (after the grace), on the master clock, in nanoseconds.
    pub start_mach_ns: u64,
    /// When it ended, on the master clock, in nanoseconds.
    pub end_mach_ns: u64,
    /// When it began, in seconds of video time.
    pub start_video_s: f64,
    /// When it ended, in seconds of video time.
    pub end_video_s: f64,
    /// How long it lasted, in milliseconds.
    pub duration_ms: u64,
    /// The largest offset of any edge, at the worst sample, ignoring its sign.
    pub max_px: i64,
    /// The offsets of each edge at the worst sample.
    pub max_offset: Edges,
    /// The offsets of each edge at the last sample that was off.
    pub final_offset: Edges,
    /// Still the case when the track ended.
    pub persisted_to_end: bool,
}

/// What [`Analysis::geometry_lag`] found.
#[derive(Debug, Clone, PartialEq, Serialize, JsonSchema)]
pub struct LagData {
    /// What rects are in: `px` for komorebi for Windows (and KDE), `points` on a Mac.
    pub unit: String,
    /// The tolerance that was used, in `unit`.
    pub tolerance: i64,
    /// The grace that was used, in milliseconds.
    pub grace_ms: u64,
    /// Whether the recording has a window track.
    pub track_available: bool,
    /// Whether the track is a measurement apart from komorebi's layout. `false` for KDE, where it
    /// is made from komorebi's own state: then there are no episodes, by design.
    pub independent: bool,
    /// Whether the track has any border windows (rows marked `"border": true`). Without, the
    /// episodes of windows are `unknown` for which one lagged, and there are none for borders.
    pub borders_seen: bool,
    /// What the reader should know about how far to trust the answer, if anything.
    pub note: Option<String>,
    /// The episodes, earliest first.
    pub episodes: Vec<LagEpisode>,
}

// What the detector reads: the track and the layouts, as plain data, so that it is tested on
// synthetic ones.

/// A window of the track at one instant.
#[derive(Debug, Clone)]
pub(super) struct Sample {
    pub id: u64,
    pub owner: String,
    pub rect: [i64; 4],
    /// Visible, not minimized and not cloaked: its rect means something.
    pub shown: bool,
    pub border: bool,
    pub border_of: Option<u64>,
}

/// A row of the track.
#[derive(Debug, Clone)]
pub(super) struct Row {
    pub mach_ns: u64,
    pub windows: Vec<Sample>,
}

/// Where the layout said each tiled window should be, from the state at one moment.
#[derive(Debug, Clone)]
pub(super) struct Layout {
    pub mach_ns: u64,
    pub targets: Vec<(u64, [i64; 4])>,
}

#[derive(Deserialize)]
struct RawRow {
    mach_ns: u64,
    #[serde(default)]
    windows: Vec<RawWindow>,
}

// Read apart from the track's own type so that the fields about borders, which a track may or may
// not have, are read where they are and ignored where they are not.
#[derive(Deserialize)]
struct RawWindow {
    id: u64,
    #[serde(default)]
    owner: String,
    rect: [i64; 4],
    #[serde(default)]
    visible: bool,
    #[serde(default)]
    minimized: bool,
    #[serde(default)]
    cloaked: Option<bool>,
    #[serde(default)]
    border: bool,
    #[serde(default)]
    border_of: Option<Value>,
}

/// Reads the text of `windows.jsonl`, skipping lines that are not rows, in time order.
pub(super) fn parse_rows(text: &str) -> Vec<Row> {
    let mut rows: Vec<Row> = text
        .lines()
        .filter_map(|line| serde_json::from_str::<RawRow>(line).ok())
        .map(|raw| Row {
            mach_ns: raw.mach_ns,
            windows: raw
                .windows
                .into_iter()
                .map(|window| Sample {
                    id: window.id,
                    owner: window.owner,
                    rect: window.rect,
                    shown: window.visible && !window.minimized && window.cloaked != Some(true),
                    border: window.border,
                    border_of: window.border_of.as_ref().and_then(Value::as_u64),
                })
                .collect(),
        })
        .collect();
    rows.sort_by_key(|row| row.mach_ns);
    rows
}

/// The windows a state gives a rect to: the one in front of each tiled container of each showing
/// workspace that is tiling normally.
pub(super) fn targets_of(snapshot: &Snapshot) -> Vec<(u64, [i64; 4])> {
    let mut targets = Vec::new();
    for monitor in &snapshot.monitors {
        let Some(workspace) = monitor.focused_workspace() else {
            continue;
        };
        if !workspace.tiling
            || workspace.layer == "Floating"
            || workspace.monocle.is_some()
            || workspace.maximized.is_some()
        {
            continue;
        }
        for (index, container) in workspace.containers.iter().enumerate() {
            let (Some(rect), Some(window)) = (
                workspace.layout_rects.get(index),
                container.focused_window(),
            ) else {
                continue;
            };
            if rect.width > 0 && rect.height > 0 {
                targets.push((
                    window.id,
                    [rect.left, rect.top, rect.right_edge(), rect.bottom_edge()],
                ));
            }
        }
    }
    targets
}

fn edges_minus(actual: [i64; 4], expected: [i64; 4]) -> [i64; 4] {
    std::array::from_fn(|edge| actual[edge] - expected[edge])
}

fn largest(offsets: [i64; 4]) -> i64 {
    offsets.iter().map(|edge| edge.abs()).max().unwrap_or(0)
}

fn area(rect: [i64; 4]) -> i64 {
    (rect[2] - rect[0]).max(0) * (rect[3] - rect[1]).max(0)
}

/// Intersection over union of two rects, in percent.
fn overlap_percent(a: [i64; 4], b: [i64; 4]) -> i64 {
    let inter = area([
        a[0].max(b[0]),
        a[1].max(b[1]),
        a[2].min(b[2]),
        a[3].min(b[3]),
    ]);
    let union = area(a) + area(b) - inter;
    if union == 0 { 0 } else { inter * 100 / union }
}

/// How far a border's edges are from an even border around `window`: each edge's gap, less the
/// median of the four gaps.
fn border_offsets(border: [i64; 4], window: [i64; 4]) -> [i64; 4] {
    // how far each edge sticks out past the window
    let gaps = [
        window[0] - border[0],
        window[1] - border[1],
        border[2] - window[2],
        border[3] - window[3],
    ];
    let mut sorted = gaps;
    sorted.sort_unstable();
    let median = (sorted[1] + sorted[2]) / 2;
    // positive is the edge further out than the median, as for a window
    [
        median - gaps[0],
        median - gaps[1],
        gaps[2] - median,
        gaps[3] - median,
    ]
}

#[derive(Default)]
struct Seen {
    target: Option<[i64; 4]>,
    target_since: u64,
    rect: Option<[i64; 4]>,
    rect_since: u64,
}

struct Run {
    owner: String,
    start: u64,
    max_px: i64,
    max_offset: [i64; 4],
    final_offset: [i64; 4],
    // whether the other kind of thing (the border for a window, the window for a border) was
    // looked at, and whether it was off, at any sample of the run
    other_seen: bool,
    other_off: bool,
}

/// Finds the episodes of windows off their layout rects and borders off their windows.
///
/// A sample is judged at each time a row or a layout was written, and at the end of a grace after
/// each. Samples after `end_ns` are not looked at, and an episode still going then ends there.
pub(super) fn detect(
    rows: &[Row],
    layouts: &[Layout],
    options: &LagOptions,
    end_ns: u64,
) -> Vec<LagEpisode> {
    let grace = options.grace_ms.saturating_mul(1_000_000);

    let mut points: Vec<u64> = rows
        .iter()
        .map(|row| row.mach_ns)
        .chain(layouts.iter().map(|layout| layout.mach_ns))
        .collect();
    let after_grace: Vec<u64> = points.iter().map(|at| at.saturating_add(grace)).collect();
    points.extend(after_grace);
    points.sort_unstable();
    points.dedup();
    points.retain(|at| *at <= end_ns);

    let mut seen: HashMap<u64, Seen> = HashMap::new();
    let mut pairing: HashMap<u64, u64> = HashMap::new();
    let mut open: BTreeMap<(Subject, u64), Run> = BTreeMap::new();
    let mut done: Vec<(Subject, u64, Run, u64, bool)> = Vec::new();
    let (mut next_row, mut next_layout) = (0, 0);

    for at in points {
        while next_row < rows.len() && rows[next_row].mach_ns <= at {
            next_row += 1;
        }
        while next_layout < layouts.len() && layouts[next_layout].mach_ns <= at {
            next_layout += 1;
        }
        let Some(row) = next_row.checked_sub(1).map(|index| &rows[index]) else {
            continue;
        };
        let targets: &[(u64, [i64; 4])] = next_layout
            .checked_sub(1)
            .map_or(&[], |index| &layouts[index].targets);

        // what changed since the last sample decides when a grace began
        for window in row.windows.iter().filter(|window| !window.border) {
            let entry = seen.entry(window.id).or_default();
            if entry.rect != Some(window.rect) {
                entry.rect = Some(window.rect);
                entry.rect_since = at;
            }
        }
        for (id, entry) in &mut seen {
            let target = targets
                .iter()
                .find(|(target, _)| target == id)
                .map(|(_, rect)| *rect);
            if entry.target != target {
                entry.target = target;
                entry.target_since = at;
            }
        }
        for (id, rect) in targets {
            // a window with a target that the track has not shown yet
            let entry = seen.entry(*id).or_default();
            if entry.target != Some(*rect) {
                entry.target = Some(*rect);
                entry.target_since = at;
            }
        }

        let by_id: HashMap<u64, &Sample> = row
            .windows
            .iter()
            .filter(|window| !window.border)
            .map(|window| (window.id, window))
            .collect();

        // windows off their layout rects
        let mut window_off: HashMap<u64, (&Sample, [i64; 4])> = HashMap::new();
        for window in by_id.values().filter(|window| window.shown) {
            let Some(entry) = seen.get(&window.id) else {
                continue;
            };
            let Some(target) = entry.target else {
                continue;
            };
            if at < entry.target_since.saturating_add(grace) {
                continue;
            }
            let offsets = edges_minus(window.rect, target);
            if largest(offsets) > options.tolerance {
                window_off.insert(window.id, (window, offsets));
            }
        }

        // borders off their windows: by the window they belong to, the worst of them
        let mut border_state: HashMap<u64, Option<(String, [i64; 4])>> = HashMap::new();
        for border in row.windows.iter().filter(|window| window.border) {
            if !border.shown || area(border.rect) == 0 {
                continue;
            }
            let owner = border
                .border_of
                .filter(|id| by_id.contains_key(id))
                .or_else(|| {
                    pairing
                        .get(&border.id)
                        .copied()
                        .filter(|id| by_id.contains_key(id))
                })
                .or_else(|| {
                    by_id
                        .values()
                        .filter(|window| {
                            window.shown && overlap_percent(window.rect, border.rect) >= 50
                        })
                        .max_by_key(|window| overlap_percent(window.rect, border.rect))
                        .map(|window| window.id)
                });
            let Some(owner) = owner else { continue };
            pairing.insert(border.id, owner);

            let window = by_id[&owner];
            let moved = seen
                .get(&owner)
                .is_some_and(|entry| at < entry.rect_since.saturating_add(grace));
            if !window.shown || moved {
                continue;
            }
            let offsets = border_offsets(border.rect, window.rect);
            let worse = border_state
                .get(&owner)
                .and_then(Option::as_ref)
                .is_none_or(|(_, worst)| largest(offsets) > largest(*worst));
            if largest(offsets) > options.tolerance {
                if worse {
                    border_state.insert(owner, Some((window.owner.clone(), offsets)));
                }
            } else {
                border_state.entry(owner).or_insert(None);
            }
        }
        let border_off: HashMap<u64, (&str, [i64; 4])> = border_state
            .iter()
            .filter_map(|(id, off)| {
                off.as_ref()
                    .map(|(owner, offsets)| (*id, (owner.as_str(), *offsets)))
            })
            .collect();

        // runs: continue those still off, start new ones, close the rest
        let mut still: Vec<(Subject, u64)> = Vec::new();
        for (id, (window, offsets)) in &window_off {
            still.push((Subject::Window, *id));
            let run = open.entry((Subject::Window, *id)).or_insert_with(|| Run {
                owner: window.owner.clone(),
                start: at,
                max_px: 0,
                max_offset: [0; 4],
                final_offset: [0; 4],
                other_seen: false,
                other_off: false,
            });
            update(run, *offsets);
            if let Some(state) = border_state.get(id) {
                run.other_seen = true;
                run.other_off |= state.is_some();
            }
        }
        for (id, (owner, offsets)) in &border_off {
            still.push((Subject::Border, *id));
            let run = open.entry((Subject::Border, *id)).or_insert_with(|| Run {
                owner: (*owner).to_string(),
                start: at,
                max_px: 0,
                max_offset: [0; 4],
                final_offset: [0; 4],
                other_seen: false,
                other_off: false,
            });
            update(run, *offsets);
            run.other_seen = true;
            run.other_off |= window_off.contains_key(id);
        }
        let ended: Vec<(Subject, u64)> = open
            .keys()
            .filter(|key| !still.contains(key))
            .copied()
            .collect();
        for key in ended {
            if let Some(run) = open.remove(&key) {
                done.push((key.0, key.1, run, at, false));
            }
        }
    }
    for ((subject, id), run) in open {
        done.push((subject, id, run, end_ns, true));
    }

    let mut episodes: Vec<LagEpisode> = done
        .into_iter()
        .map(|(subject, window_id, run, end, persisted_to_end)| {
            let end = end.max(run.start);
            let lagged = match (subject, run.other_seen, run.other_off) {
                (Subject::Window, false, _) => Lagged::Unknown,
                (Subject::Window, true, false) => Lagged::Window,
                (Subject::Border, _, false) => Lagged::Border,
                (_, _, true) => Lagged::Both,
            };
            LagEpisode {
                subject,
                lagged,
                window_id,
                owner: run.owner,
                start_mach_ns: run.start,
                end_mach_ns: end,
                start_video_s: 0.0,
                end_video_s: 0.0,
                duration_ms: (end - run.start) / 1_000_000,
                max_px: run.max_px,
                max_offset: Edges::of(run.max_offset),
                final_offset: Edges::of(run.final_offset),
                persisted_to_end,
            }
        })
        .collect();
    episodes.sort_by_key(|episode| (episode.start_mach_ns, episode.window_id, episode.subject));
    episodes
}

fn update(run: &mut Run, offsets: [i64; 4]) {
    if largest(offsets) > run.max_px {
        run.max_px = largest(offsets);
        run.max_offset = offsets;
    }
    run.final_offset = offsets;
}

impl LagEpisode {
    /// A sentence that says what happened, for a finding or a line of text.
    #[must_use]
    pub fn describe(&self, unit: &str) -> String {
        let who = if self.owner.is_empty() {
            format!("window {}", self.window_id)
        } else {
            format!("{} (window {})", self.owner, self.window_id)
        };
        let what = match self.subject {
            Subject::Window => format!("{who} was off its layout rect"),
            Subject::Border => format!("the border of {who} was not evenly around it"),
        };
        let lasted = if self.duration_ms >= 1000 {
            format!("{:.1} s", self.duration_ms as f64 / 1000.0)
        } else {
            format!("{} ms", self.duration_ms)
        };
        let edges = |edges: &Edges| {
            format!(
                "left {:+}, top {:+}, right {:+}, bottom {:+}",
                edges.left, edges.top, edges.right, edges.bottom
            )
        };
        let end = if self.persisted_to_end {
            format!(
                "still {} {unit} off when the recording ended",
                self.final_offset.largest()
            )
        } else {
            format!("{} {unit} off at the end", self.final_offset.largest())
        };
        let who_lagged = match (self.subject, self.lagged) {
            (Subject::Window, Lagged::Window) => ", and its border went along with it".to_string(),
            (Subject::Window, Lagged::Both) => ", and its border was off too".to_string(),
            (Subject::Border, Lagged::Border) => ", while the window was in its place".to_string(),
            (Subject::Border, Lagged::Both) => {
                ", and the window was off its layout rect too".to_string()
            }
            _ => String::new(),
        };
        format!(
            "{what} for {lasted}: up to {} {unit} ({}), {end}{who_lagged}",
            self.max_px,
            edges(&self.max_offset)
        )
    }

    /// How serious a finding about it is: `warn` for one that lasted a second or was still going at
    /// the end, else `info`.
    #[must_use]
    pub fn severity(&self) -> Severity {
        if self.persisted_to_end
            || self.end_mach_ns.saturating_sub(self.start_mach_ns) >= WARN_AFTER_NS
        {
            Severity::Warn
        } else {
            Severity::Info
        }
    }
}

impl Analysis {
    /// Finds the stretches in which a window was off the layout rect komorebi gave it, and a border
    /// off its window; see the [module](self) for what is measured and what is not flagged.
    ///
    /// A recording without a window track, or made on KDE (whose track is not independent of the
    /// layout), has no episodes, and [`LagData::note`] says why.
    ///
    /// # Errors
    ///
    /// The track exists and cannot be read, or a recorded state cannot.
    ///
    /// # Example
    ///
    /// See the [module docs](self).
    pub fn geometry_lag(&self, options: &LagOptions) -> eyre::Result<LagData> {
        let flavor = self.flavor();
        let mut data = LagData {
            unit: if flavor == Flavor::Macos {
                "points"
            } else {
                "px"
            }
            .to_string(),
            tolerance: options.tolerance,
            grace_ms: options.grace_ms,
            track_available: false,
            independent: flavor != Flavor::Kde,
            borders_seen: false,
            note: None,
            episodes: Vec::new(),
        };

        let path = self.guard().resolve(WINDOWS_FILE)?;
        if !path.exists() {
            data.note = Some(
                "the recording has no window track (it was made without --window-track), so there is nothing to compare the layout with"
                    .to_string(),
            );
            return Ok(data);
        }
        data.track_available = true;
        if flavor == Flavor::Kde {
            data.note = Some(
                "on KDE the window track is made from komorebi's own state, so it is not independent of the layout rect: comparing them finds komorebi disagreeing with itself, not the compositor lagging. No lag is reported"
                    .to_string(),
            );
            return Ok(data);
        }

        let text = std::fs::read_to_string(&path)
            .wrap_err_with(|| format!("could not read {}", path.display()))?;
        let rows = parse_rows(&text);
        data.borders_seen = rows
            .iter()
            .any(|row| row.windows.iter().any(|window| window.border));

        let mut layouts: Vec<Layout> = Vec::new();
        let mut last_state_id = None;
        for event in self.events() {
            let Some(state_id) = event.state_id else {
                continue;
            };
            if last_state_id == Some(state_id) {
                continue;
            }
            last_state_id = Some(state_id);
            layouts.push(Layout {
                mach_ns: event.recv_mach_ns,
                targets: targets_of(&*self.snapshot(state_id)?),
            });
        }
        layouts.sort_by_key(|layout| layout.mach_ns);

        // the track writes a row at least every second while it runs
        let last_row = rows.last().map_or(0, |row| row.mach_ns);
        let end_ns = super::timespec::session_end_ns(&self.manifest)
            .min(last_row.saturating_add(KEYFRAME_INTERVAL_NS))
            .max(last_row);

        let mut episodes = detect(&rows, &layouts, options, end_ns);
        episodes.retain(|episode| options.window.is_none_or(|id| episode.window_id == id));
        for episode in &mut episodes {
            episode.start_video_s = self.video_s(episode.start_mach_ns);
            episode.end_video_s = self.video_s(episode.end_mach_ns);
        }
        data.episodes = episodes;
        Ok(data)
    }

    /// Raises a finding for each episode of lag. KDE has none (see the [module](self)).
    pub(super) fn find_lag_problems(&self, found: &mut Vec<Finding>) -> eyre::Result<()> {
        let data = self.geometry_lag(&LagOptions::default())?;
        let dir = self.session.dir.display().to_string();
        for episode in data.episodes {
            let kind = match episode.subject {
                Subject::Window => "geometry-lag:window",
                Subject::Border => "geometry-lag:border",
            };
            let mut finding = self.finding(
                kind,
                episode.severity(),
                episode.describe(&data.unit),
                episode.start_mach_ns,
                episode.end_mach_ns,
            );
            finding.window_ids = vec![episode.window_id];
            finding.persisted_to_end = episode.persisted_to_end;
            finding.next = Some(format!(
                "komorebi-recorder lag {dir} --window {}",
                episode.window_id
            ));
            found.push(finding);
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const MS: u64 = 1_000_000;

    fn sample(id: u64, rect: [i64; 4]) -> Sample {
        Sample {
            id,
            owner: "term".to_string(),
            rect,
            shown: true,
            border: false,
            border_of: None,
        }
    }

    fn border(id: u64, rect: [i64; 4], of: Option<u64>) -> Sample {
        Sample {
            border: true,
            border_of: of,
            owner: String::new(),
            ..sample(id, rect)
        }
    }

    fn row(ms: u64, windows: Vec<Sample>) -> Row {
        Row {
            mach_ns: ms * MS,
            windows,
        }
    }

    fn layout(ms: u64, targets: &[(u64, [i64; 4])]) -> Layout {
        Layout {
            mach_ns: ms * MS,
            targets: targets.to_vec(),
        }
    }

    const HOME: [i64; 4] = [0, 0, 960, 1040];

    fn run(rows: &[Row], layouts: &[Layout], end_ms: u64) -> Vec<LagEpisode> {
        detect(rows, layouts, &LagOptions::default(), end_ms * MS)
    }

    #[test]
    fn a_window_in_place_is_not_off() {
        let rows = [
            row(0, vec![sample(1, HOME)]),
            row(2000, vec![sample(1, HOME)]),
        ];
        assert!(run(&rows, &[layout(0, &[(1, HOME)])], 3000).is_empty());
    }

    #[test]
    fn a_window_that_is_late_is_an_episode_from_the_end_of_the_grace() {
        let late = [100, 0, 1060, 1040];
        let rows = [
            row(1000, vec![sample(1, late)]),
            row(1100, vec![sample(1, [40, 0, 1000, 1040])]),
            row(3000, vec![sample(1, HOME)]),
        ];
        let found = run(&rows, &[layout(1000, &[(1, HOME)])], 4000);

        assert_eq!(found.len(), 1);
        let episode = &found[0];
        assert_eq!((episode.subject, episode.window_id), (Subject::Window, 1));
        // the grace ends at 1500 ms, and the window was home at 3000 ms
        assert_eq!(episode.start_mach_ns, 1500 * MS);
        assert_eq!(episode.end_mach_ns, 3000 * MS);
        assert_eq!(episode.duration_ms, 1500);
        // at 1500 ms the offset is the one of the row at 1100 ms: 40, not the 100 of the worst
        // sample, which was inside the grace
        assert_eq!(episode.max_px, 40);
        assert_eq!(episode.final_offset, Edges::of([40, 0, 40, 0]));
        assert!(!episode.persisted_to_end);
        assert_eq!(episode.lagged, Lagged::Unknown);
        assert_eq!(episode.severity(), Severity::Warn);
    }

    #[test]
    fn what_settles_within_the_grace_is_an_animation() {
        let rows = [
            row(1000, vec![sample(1, [300, 0, 1260, 1040])]),
            row(1100, vec![sample(1, [150, 0, 1110, 1040])]),
            row(1250, vec![sample(1, HOME)]),
        ];
        assert!(run(&rows, &[layout(1000, &[(1, HOME)])], 3000).is_empty());
    }

    #[test]
    fn a_new_layout_starts_the_grace_again() {
        let moved = [960, 0, 1920, 1040];
        let rows = [
            row(0, vec![sample(1, HOME)]),
            // the layout gives it the other half at 2000 ms, and it gets there at 2300 ms
            row(2300, vec![sample(1, moved)]),
        ];
        let layouts = [layout(0, &[(1, HOME)]), layout(2000, &[(1, moved)])];
        assert!(run(&rows, &layouts, 3000).is_empty());
    }

    #[test]
    fn an_offset_within_the_tolerance_is_not_off() {
        let near = [DEFAULT_TOLERANCE, 0, 960 + DEFAULT_TOLERANCE, 1040];
        let rows = [row(0, vec![sample(1, near)])];
        assert!(run(&rows, &[layout(0, &[(1, HOME)])], 5000).is_empty());

        let past = [DEFAULT_TOLERANCE + 1, 0, 960, 1040];
        let rows = [row(0, vec![sample(1, past)])];
        assert_eq!(run(&rows, &[layout(0, &[(1, HOME)])], 5000).len(), 1);
    }

    #[test]
    fn a_window_still_off_at_the_end_lasts_to_the_end() {
        let rows = [row(0, vec![sample(1, [0, 0, 700, 1040])])];
        let found = run(&rows, &[layout(0, &[(1, HOME)])], 2500);
        assert_eq!(found.len(), 1);
        assert!(found[0].persisted_to_end);
        assert_eq!(found[0].end_mach_ns, 2500 * MS);
        assert_eq!(found[0].max_offset, Edges::of([0, 0, -260, 0]));
    }

    #[test]
    fn a_hidden_or_gone_window_ends_the_episode() {
        let mut cloaked = sample(1, [0, 0, 700, 1040]);
        cloaked.shown = false;
        let rows = [
            row(0, vec![sample(1, [0, 0, 700, 1040])]),
            row(1000, vec![cloaked]),
            row(1600, vec![sample(1, [0, 0, 700, 1040])]),
            row(2000, vec![]),
        ];
        let found = run(&rows, &[layout(0, &[(1, HOME)])], 3000);
        // off from 500 ms (grace) to 1000 ms (cloaked), again from 1600 ms to 2000 ms (gone)
        assert_eq!(
            found
                .iter()
                .map(|episode| (episode.start_mach_ns / MS, episode.end_mach_ns / MS))
                .collect::<Vec<_>>(),
            [(500, 1000), (1600, 2000)]
        );
    }

    #[test]
    fn a_window_the_layout_does_not_place_is_not_judged() {
        let rows = [row(0, vec![sample(1, [0, 0, 700, 1040])])];
        assert!(run(&rows, &[layout(0, &[])], 3000).is_empty());
        assert!(run(&rows, &[], 3000).is_empty());
    }

    #[test]
    fn an_even_border_is_not_off_and_a_window_that_lags_takes_its_border_with_it() {
        // a border 4 around the window; the window is late, the border is with it
        let around = |rect: [i64; 4]| [rect[0] - 4, rect[1] - 4, rect[2] + 4, rect[3] + 4];
        let late = [200, 0, 1160, 1040];
        let rows = [
            row(0, vec![sample(1, late), border(9, around(late), Some(1))]),
            row(
                3000,
                vec![sample(1, HOME), border(9, around(HOME), Some(1))],
            ),
        ];
        let found = run(&rows, &[layout(0, &[(1, HOME)])], 4000);
        assert_eq!(found.len(), 1, "{found:#?}");
        assert_eq!(found[0].subject, Subject::Window);
        assert_eq!(found[0].lagged, Lagged::Window);
        assert!(found[0].describe("px").contains("border went along"));
    }

    #[test]
    fn a_border_left_behind_by_a_window_in_place_is_the_border_lagging() {
        let around = |rect: [i64; 4]| [rect[0] - 4, rect[1] - 4, rect[2] + 4, rect[3] + 4];
        let stale = around([0, 0, 960 - 120, 1040]);
        let rows = [
            row(0, vec![sample(1, HOME), border(9, around(HOME), None)]),
            // the window grew to its place and the border stayed at its old size
            row(100, vec![sample(1, HOME), border(9, stale, None)]),
            row(2000, vec![sample(1, HOME), border(9, around(HOME), None)]),
        ];
        let found = run(&rows, &[layout(0, &[(1, HOME)])], 3000);
        assert_eq!(found.len(), 1, "{found:#?}");
        let episode = &found[0];
        assert_eq!((episode.subject, episode.window_id), (Subject::Border, 1));
        assert_eq!(episode.lagged, Lagged::Border);
        assert_eq!(episode.owner, "term");
        assert_eq!(
            (episode.start_mach_ns, episode.end_mach_ns),
            (500 * MS, 2000 * MS)
        );
        // the right edge is 120 short of an even border
        assert_eq!(episode.max_offset.right, -120);
    }

    #[test]
    fn a_track_without_borders_has_no_border_episodes() {
        let rows = [
            row(0, vec![sample(1, HOME)]),
            row(2000, vec![sample(1, HOME)]),
        ];
        assert!(
            run(&rows, &[layout(0, &[(1, HOME)])], 3000)
                .iter()
                .all(|episode| episode.subject == Subject::Window)
        );
    }

    #[test]
    fn a_border_follows_a_moving_window_without_lag_within_the_grace() {
        let around = |rect: [i64; 4]| [rect[0] - 4, rect[1] - 4, rect[2] + 4, rect[3] + 4];
        let next = [100, 0, 1060, 1040];
        // the border is a frame behind the window, which is moving
        let rows = [
            row(0, vec![sample(1, HOME), border(9, around(HOME), Some(1))]),
            row(
                1000,
                vec![sample(1, next), border(9, around(HOME), Some(1))],
            ),
            row(
                1100,
                vec![sample(1, next), border(9, around(next), Some(1))],
            ),
        ];
        let found = run(&rows, &[layout(0, &[(1, next)])], 3000);
        assert!(
            found
                .iter()
                .all(|episode| episode.subject != Subject::Border),
            "{found:#?}"
        );
    }

    #[test]
    fn rows_are_read_with_or_without_the_fields_about_borders() {
        let rows = parse_rows(
            "{\"mach_ns\":5,\"windows\":[{\"id\":1,\"owner\":\"a\",\"rect\":[0,0,10,10],\"visible\":true,\"minimized\":false,\"cloaked\":false},{\"id\":2,\"owner\":\"\",\"rect\":[0,0,12,12],\"visible\":true,\"minimized\":false,\"border\":true,\"border_of\":1}]}\nnot a row\n{\"mach_ns\":1,\"windows\":[]}",
        );
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0].mach_ns, 1);
        assert!(!rows[1].windows[0].border);
        assert!(rows[1].windows[1].border);
        assert_eq!(rows[1].windows[1].border_of, Some(1));
        assert!(rows[1].windows[0].shown);
    }
}
