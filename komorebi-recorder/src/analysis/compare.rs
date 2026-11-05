//! `compare`: two recordings side by side, as numbers.
//!
//! For a before-and-after of a fix, or a good run against a bad one: how long each recording is,
//! how many frames it has and how steadily they came, how much the screen moved, what the analysis
//! found wrong, and how many annotations it carries, with the difference (`b` minus `a`) of each.
//!
//! | Measure | Where it comes from |
//! |---|---|
//! | `duration_s`, `frames`, `effective_fps` | the `frames-*.csv` logs of every display segment: real frames, and the intervals between them over the time they span |
//! | `median_gap_ms`, `worst_gap_ms`, `late_gaps` | the gaps between consecutive real frames of a segment; `late_gaps` counts those over 100 ms and over twice the median |
//! | `motion` | [`Analysis::motion`] over every display at sensitivity 1: segments, their total time, the largest share and mean brightness delta |
//! | `findings` | [`Analysis::findings`], counted by kind |
//! | `annotations`, `bugs` | the session's annotations |
//!
//! Frames come when the screen changes, so a still screen has long gaps and a low effective
//! frame rate without anything being wrong: compare like with like.

use super::Analysis;
use super::motion::ScanOptions;
use super::video::SessionVideo;
use crate::annotations;
use color_eyre::eyre;
use schemars::JsonSchema;
use serde::Serialize;
use std::collections::BTreeMap;
use std::sync::atomic::AtomicBool;

/// The schema name that identifies a [`CompareData`] in an envelope (`compare/1`).
pub const SCHEMA_COMPARE: &str = "compare/1";

/// A gap between frames over this is late (and over twice the median).
const LATE_GAP_S: f64 = 0.1;

/// How the pictures of a recording moved.
#[derive(Debug, Clone, PartialEq, Serialize, JsonSchema)]
pub struct MotionMetrics {
    /// How many segments of motion, over all displays.
    pub segments: usize,
    /// Their total duration, in seconds.
    pub active_s: f64,
    /// The largest share of a display that changed between two frames.
    pub peak_share: f64,
    /// The largest mean brightness difference between two frames, 0 to 255.
    pub peak_mean_delta: f64,
}

/// The measures of one recording.
#[derive(Debug, Clone, PartialEq, Serialize, JsonSchema)]
pub struct Side {
    /// The session directory.
    pub session: String,
    /// The length of the video, in seconds.
    pub duration_s: f64,
    /// Real frames over every display segment.
    pub frames: usize,
    /// Intervals between frames over the time they span, per second; `0` with fewer than two
    /// frames.
    pub effective_fps: f64,
    /// The median gap between consecutive frames, in milliseconds.
    pub median_gap_ms: f64,
    /// The longest gap between consecutive frames, in milliseconds.
    pub worst_gap_ms: f64,
    /// When the longest gap began, in seconds of video time.
    pub worst_gap_at_video_s: Option<f64>,
    /// Gaps over 100 ms and over twice the median.
    pub late_gaps: usize,
    /// The motion measures; `None` if the video could not be decoded (see `notes`).
    pub motion: Option<MotionMetrics>,
    /// How many findings of each kind.
    pub findings: BTreeMap<String, usize>,
    /// How many findings in all.
    pub findings_total: usize,
    /// How many annotations of any kind.
    pub annotations: usize,
    /// How many of them are bug regions.
    pub bugs: usize,
}

/// `b` minus `a` for each measure. A motion measure is `None` if either side has none.
#[derive(Debug, Clone, PartialEq, Serialize, JsonSchema)]
pub struct Deltas {
    /// Duration, in seconds.
    pub duration_s: f64,
    /// Frames.
    pub frames: i64,
    /// Effective frames per second.
    pub effective_fps: f64,
    /// Median frame gap, in milliseconds.
    pub median_gap_ms: f64,
    /// Worst frame gap, in milliseconds.
    pub worst_gap_ms: f64,
    /// Late gaps.
    pub late_gaps: i64,
    /// Motion segments.
    pub motion_segments: Option<i64>,
    /// Total time in motion, in seconds.
    pub active_s: Option<f64>,
    /// Peak share of a display changed.
    pub peak_share: Option<f64>,
    /// Peak mean brightness delta.
    pub peak_mean_delta: Option<f64>,
    /// Findings in all.
    pub findings_total: i64,
    /// Findings by kind, for every kind either side has.
    pub findings: BTreeMap<String, i64>,
    /// Annotations.
    pub annotations: i64,
}

/// Two recordings side by side: the `data` of `compare`.
#[derive(Debug, Clone, PartialEq, Serialize, JsonSchema)]
pub struct CompareData {
    /// The first recording.
    pub a: Side,
    /// The second recording.
    pub b: Side,
    /// `b` minus `a`.
    pub deltas: Deltas,
    /// What could not be measured, and why.
    pub notes: Vec<String>,
}

/// The gaps between real frames in the text of a `frames-*.csv`, as `(time of the earlier frame
/// in ns, gap in ns)`, and how many frames there were. Rows that do not parse (a header, a torn
/// last row) and repeat rows (`-`) are ignored.
///
/// # Example
///
/// ```rust
/// use komorebi_recorder::analysis::compare::frame_gaps;
///
/// let (gaps, frames) = frame_gaps("index,pts,received\n0,1000,1\n-,1500,1\n1,3000,1\n2,3500,1\n3,");
/// assert_eq!(frames, 3);
/// assert_eq!(gaps, [(1000, 2000), (3000, 500)]);
/// ```
#[must_use]
pub fn frame_gaps(csv: &str) -> (Vec<(u64, u64)>, usize) {
    let mut gaps = Vec::new();
    let mut frames = 0;
    let mut previous: Option<u64> = None;
    for line in csv.lines() {
        let mut fields = line.split(',');
        let (Some(index), Some(pts), Some(_), None) =
            (fields.next(), fields.next(), fields.next(), fields.next())
        else {
            continue;
        };
        if index.parse::<u64>().is_err() {
            continue;
        }
        let Ok(pts) = pts.parse::<u64>() else {
            continue;
        };

        frames += 1;
        if let Some(before) = previous {
            gaps.push((before, pts.saturating_sub(before)));
        }
        previous = Some(pts);
    }
    (gaps, frames)
}

impl Analysis {
    /// Measures this recording as one [`Side`] of a comparison, as the [module](self) describes.
    ///
    /// The frame logs are read from the session. If the video cannot be decoded (the host has no
    /// decoder, or there is no video) the motion measures are `None` and the reason goes in
    /// `notes`. `cancel` stops the motion scan early.
    ///
    /// # Errors
    ///
    /// Returns an error if the findings cannot be computed (a recorded state is missing or
    /// unreadable) or the annotations cannot be read.
    pub fn side(&self, cancel: Option<&AtomicBool>, notes: &mut Vec<String>) -> eyre::Result<Side> {
        let mut all_gaps: Vec<(u64, u64)> = Vec::new();
        let (mut frames, mut span_ns, mut intervals) = (0_usize, 0_u128, 0_usize);
        for entry in &self.manifest.displays {
            let path = self.session.path(&entry.result.frames_file);
            let Ok(text) = std::fs::read_to_string(&path) else {
                continue;
            };
            let (gaps, count) = frame_gaps(&text);
            frames += count;
            intervals += gaps.len();
            span_ns += gaps.iter().map(|(_, gap)| u128::from(*gap)).sum::<u128>();
            all_gaps.extend(gaps);
        }

        let mut sorted: Vec<u64> = all_gaps.iter().map(|(_, gap)| *gap).collect();
        sorted.sort_unstable();
        let median_ns = sorted.get(sorted.len() / 2).copied().unwrap_or(0);
        let worst = all_gaps.iter().max_by_key(|(_, gap)| *gap).copied();
        let late_ns = (LATE_GAP_S * 1e9) as u64;
        let late_gaps = all_gaps
            .iter()
            .filter(|(_, gap)| *gap > late_ns && *gap > median_ns.saturating_mul(2))
            .count();

        let video = SessionVideo::new(&self.manifest, self.guard());
        let displays = video.displays();
        let (_, duration) = self.manifest.video_span();
        let motion = if displays.is_empty() {
            notes.push(format!(
                "{} recorded no video, so there are no motion measures",
                self.session.dir.display()
            ));
            None
        } else {
            match self.motion(
                &video,
                &ScanOptions {
                    displays: &displays,
                    from_s: 0.0,
                    to_s: duration,
                    crop: None,
                    cancel,
                },
                1.0,
            ) {
                Ok(data) => Some(MotionMetrics {
                    segments: data.segments.len(),
                    active_s: data.active_s(),
                    peak_share: data
                        .segments
                        .iter()
                        .map(|segment| segment.peak_share)
                        .fold(0.0, f64::max),
                    peak_mean_delta: data
                        .segments
                        .iter()
                        .map(|segment| segment.peak_mean_delta)
                        .fold(0.0, f64::max),
                }),
                Err(error) => {
                    notes.push(format!(
                        "{}: motion could not be measured: {error}",
                        self.session.dir.display()
                    ));
                    None
                }
            }
        };

        let mut findings: BTreeMap<String, usize> = BTreeMap::new();
        let found = self.findings()?;
        for finding in &found {
            *findings.entry(finding.kind.clone()).or_default() += 1;
        }
        let loaded = annotations::load(&self.session.dir)?;

        Ok(Side {
            session: self.session.dir.display().to_string(),
            duration_s: duration,
            frames,
            effective_fps: if span_ns == 0 {
                0.0
            } else {
                intervals as f64 / (span_ns as f64 / 1e9)
            },
            median_gap_ms: median_ns as f64 / 1e6,
            worst_gap_ms: worst.map_or(0.0, |(_, gap)| gap as f64 / 1e6),
            worst_gap_at_video_s: worst.map(|(at, _)| self.video_s(at)),
            late_gaps,
            motion,
            findings_total: found.len(),
            findings,
            annotations: loaded.annotations.len(),
            bugs: loaded.bugs().count(),
        })
    }

    /// Compares this recording (`a`) with `other` (`b`): see the [module](self) documentation.
    ///
    /// # Errors
    ///
    /// The same as [`Analysis::side`], for either recording.
    ///
    /// # Example
    ///
    /// A session compared with itself differs in nothing:
    ///
    /// ```rust
    /// use komorebi_recorder::analysis::Analysis;
    /// use komorebi_recorder::testkit::SessionBuilder;
    ///
    /// let (session, _) = SessionBuilder::new("doc-compare").length(5.0).build();
    /// let analysis = Analysis::open(&session.dir)?;
    /// let data = analysis.compare(&analysis, None)?;
    /// assert_eq!(data.a.duration_s, data.b.duration_s);
    /// assert_eq!(data.deltas.duration_s, 0.0);
    /// assert_eq!(data.deltas.findings_total, 0);
    /// # Ok::<(), Box<dyn std::error::Error>>(())
    /// ```
    pub fn compare(
        &self,
        other: &Analysis,
        cancel: Option<&AtomicBool>,
    ) -> eyre::Result<CompareData> {
        let mut notes = Vec::new();
        let a = self.side(cancel, &mut notes)?;
        let b = other.side(cancel, &mut notes)?;
        let deltas = deltas(&a, &b);
        Ok(CompareData {
            a,
            b,
            deltas,
            notes,
        })
    }
}

fn deltas(a: &Side, b: &Side) -> Deltas {
    let both = |f: fn(&MotionMetrics) -> f64| match (&a.motion, &b.motion) {
        (Some(x), Some(y)) => Some(f(y) - f(x)),
        _ => None,
    };
    let kinds: std::collections::BTreeSet<&String> =
        a.findings.keys().chain(b.findings.keys()).collect();
    Deltas {
        duration_s: b.duration_s - a.duration_s,
        frames: b.frames as i64 - a.frames as i64,
        effective_fps: b.effective_fps - a.effective_fps,
        median_gap_ms: b.median_gap_ms - a.median_gap_ms,
        worst_gap_ms: b.worst_gap_ms - a.worst_gap_ms,
        late_gaps: b.late_gaps as i64 - a.late_gaps as i64,
        motion_segments: both(|m| m.segments as f64).map(|d| d as i64),
        active_s: both(|m| m.active_s),
        peak_share: both(|m| m.peak_share),
        peak_mean_delta: both(|m| m.peak_mean_delta),
        findings_total: b.findings_total as i64 - a.findings_total as i64,
        findings: kinds
            .into_iter()
            .map(|kind| {
                let count = |side: &Side| side.findings.get(kind).copied().unwrap_or(0) as i64;
                (kind.clone(), count(b) - count(a))
            })
            .collect(),
        annotations: b.annotations as i64 - a.annotations as i64,
    }
}

impl CompareData {
    /// Renders the comparison as a table with a column for each recording and one for the
    /// difference.
    ///
    /// # Example
    ///
    /// ```rust
    /// use komorebi_recorder::analysis::Analysis;
    /// use komorebi_recorder::testkit::SessionBuilder;
    ///
    /// let (session, _) = SessionBuilder::new("doc-compare-text").length(5.0).build();
    /// let analysis = Analysis::open(&session.dir)?;
    /// let text = analysis.compare(&analysis, None)?.to_text();
    /// assert!(text.contains("duration"));
    /// assert!(text.contains("findings"));
    /// # Ok::<(), Box<dyn std::error::Error>>(())
    /// ```
    #[must_use]
    pub fn to_text(&self) -> String {
        use std::fmt::Write;

        let (a, b, d) = (&self.a, &self.b, &self.deltas);
        let mut out = String::new();
        let _ = writeln!(out, "a: {}\nb: {}\n", a.session, b.session);
        let _ = writeln!(out, "{:<24}{:>14}{:>14}{:>14}", "", "a", "b", "b - a");
        let mut row = |name: &str, x: String, y: String, delta: String| {
            let _ = writeln!(out, "{name:<24}{x:>14}{y:>14}{delta:>14}");
        };
        let number = |value: f64, places: usize| format!("{value:.places$}");
        let signed = |value: f64, places: usize| format!("{value:+.places$}");
        let whole = |value: i64| format!("{value:+}");

        row(
            "duration (s)",
            number(a.duration_s, 2),
            number(b.duration_s, 2),
            signed(d.duration_s, 2),
        );
        row(
            "frames",
            a.frames.to_string(),
            b.frames.to_string(),
            whole(d.frames),
        );
        row(
            "effective fps",
            number(a.effective_fps, 1),
            number(b.effective_fps, 1),
            signed(d.effective_fps, 1),
        );
        row(
            "median frame gap (ms)",
            number(a.median_gap_ms, 1),
            number(b.median_gap_ms, 1),
            signed(d.median_gap_ms, 1),
        );
        row(
            "worst frame gap (ms)",
            number(a.worst_gap_ms, 1),
            number(b.worst_gap_ms, 1),
            signed(d.worst_gap_ms, 1),
        );
        row(
            "late frame gaps",
            a.late_gaps.to_string(),
            b.late_gaps.to_string(),
            whole(d.late_gaps),
        );

        let dash = || "-".to_string();
        row(
            "motion segments",
            a.motion
                .as_ref()
                .map_or_else(dash, |m| m.segments.to_string()),
            b.motion
                .as_ref()
                .map_or_else(dash, |m| m.segments.to_string()),
            d.motion_segments.map_or_else(dash, whole),
        );
        row(
            "time in motion (s)",
            a.motion
                .as_ref()
                .map_or_else(dash, |m| number(m.active_s, 2)),
            b.motion
                .as_ref()
                .map_or_else(dash, |m| number(m.active_s, 2)),
            d.active_s.map_or_else(dash, |v| signed(v, 2)),
        );
        row(
            "peak motion (% pixels)",
            a.motion
                .as_ref()
                .map_or_else(dash, |m| number(m.peak_share * 100.0, 1)),
            b.motion
                .as_ref()
                .map_or_else(dash, |m| number(m.peak_share * 100.0, 1)),
            d.peak_share.map_or_else(dash, |v| signed(v * 100.0, 1)),
        );
        row(
            "peak mean delta",
            a.motion
                .as_ref()
                .map_or_else(dash, |m| number(m.peak_mean_delta, 1)),
            b.motion
                .as_ref()
                .map_or_else(dash, |m| number(m.peak_mean_delta, 1)),
            d.peak_mean_delta.map_or_else(dash, |v| signed(v, 1)),
        );
        row(
            "findings",
            a.findings_total.to_string(),
            b.findings_total.to_string(),
            whole(d.findings_total),
        );
        for (kind, delta) in &d.findings {
            row(
                &format!("  {kind}"),
                a.findings.get(kind).copied().unwrap_or(0).to_string(),
                b.findings.get(kind).copied().unwrap_or(0).to_string(),
                whole(*delta),
            );
        }
        row(
            "annotations",
            a.annotations.to_string(),
            b.annotations.to_string(),
            whole(d.annotations),
        );
        for note in &self.notes {
            let _ = writeln!(out, "note: {note}");
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn side(frames: usize, kinds: &[(&str, usize)]) -> Side {
        Side {
            session: String::new(),
            duration_s: 10.0,
            frames,
            effective_fps: 30.0,
            median_gap_ms: 33.0,
            worst_gap_ms: 100.0,
            worst_gap_at_video_s: None,
            late_gaps: 0,
            motion: None,
            findings: kinds.iter().map(|(k, n)| ((*k).to_string(), *n)).collect(),
            findings_total: kinds.iter().map(|(_, n)| n).sum(),
            annotations: 0,
            bugs: 0,
        }
    }

    #[test]
    fn frame_gaps_ignore_headers_repeats_and_torn_rows() {
        let (gaps, frames) = frame_gaps("0,100,100\n-,150,150\n1,300,300\n2,3");
        assert_eq!(frames, 2);
        assert_eq!(gaps, [(100, 200)]);
    }

    #[test]
    fn deltas_cover_the_kinds_of_either_side() {
        let a = side(100, &[("slow-handler", 2)]);
        let b = side(80, &[("event-storm", 1), ("slow-handler", 1)]);
        let d = deltas(&a, &b);
        assert_eq!(d.frames, -20);
        assert_eq!(d.findings_total, 0);
        assert_eq!(d.findings["slow-handler"], -1);
        assert_eq!(d.findings["event-storm"], 1);
        assert_eq!(d.motion_segments, None);
    }
}
