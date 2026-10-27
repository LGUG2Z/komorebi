//! The things that stand out in a recording, ranked by how likely they are to matter.
//!
//! A session holds thousands of events and log lines, nearly all of them ordinary. This looks for
//! the things that are not: states that are wrong and for how long, windows that flap, bursts of
//! events, slow handlers, errors, and what happened to the machine (a display unplugged, a sleep).
//! Each is a [`Finding`] with the event or log line to look at next. Findings that overlap, or come
//! just before, a region the user marked as the bug rank first.
//!
//! [`Analysis::overview`] is what the `overview` command prints, and `brief` starts from the same
//! findings. [`Analysis::findings`] runs the detectors and [`Analysis::rank`] orders what they
//! found.
//!
//! # What is looked for
//!
//! Each detector has fixed thresholds, listed here so that an absent finding can be read as "none
//! of these happened", not "nothing was wrong":
//!
//! | `kind` | what it is | severity |
//! |---|---|---|
//! | `invariant:<kind>` | a violation found by [`invariants::check`] in the recorded states (`invariants::check_probed` for a recording made with `--probe-windows`), from the first state that has it to the first that does not, or to the end of the recording | the violation's own; one that was gone again within 0.5 s (for the probe's findings: seen at a single probe and gone at the window's next) is a transition in progress and is lowered one step (error to warn, anything else to info) |
//! | `window-flapping` | one window's hidden state flipping (parked in komorebi's hiding position, or brought back) at least 4 times within 10 s | warn |
//! | `focus-oscillation` | focus changing at least 4 times within 5 s between at most two windows | warn |
//! | `event-storm` | 20 or more events within 1 s | warn for 60 or more, else info |
//! | `slow-handler` | a handler that took at least 250 ms from its header to its notification: the 10 slowest | warn for 1 s or more, else info |
//! | `monitor-churn` | two or more display notifications, each within 5 s of the first | warn |
//! | `log-error`, `log-warning` | komorebi log lines at `ERROR` or `WARN`: one finding for each module and message, where messages that differ only in digits are the same | warn for `log-error`, info for `log-warning` |
//! | `incident:<kind>` | something the recorder noted about the machine: a display plugged or unplugged, a sleep, a capture that failed | warn for `capture-failed`, `log-stream-lost` and `komorebi-exited`, else info |
//!
//! The same violation across several consecutive states is one finding giving how long it lasted,
//! not one per event. A violation is told apart from another by its kind, the windows,
//! monitor and workspace it is about.
//!
//! # Flavor
//!
//! The state detectors read what the komorebi of the recorded flavor reported (see
//! [`super::invariants`]). komorebi for Windows hides a window without moving it, so for a Windows
//! session `window-flapping` never fires and the hidden-window violations come only from the probe:
//! without `--probe-windows` the overview reports nothing about whether a window is really hidden,
//! and that is not the same as nothing being wrong. The probe's findings are made by
//! `analysis::probed`. The other detectors do not depend on the flavor, except that the cause named
//! in a `slow-handler` finding is the flavor's own
//! ([`Rules::slow_handler_cause`](super::flavor::Rules::slow_handler_cause)).

use super::Analysis;
use super::classify::Class;
use super::events::Family;
use super::invariants;
use super::invariants::Severity;
use super::logparse::Level;
use crate::annotations::Annotation;
use crate::annotations::Kind;
use crate::incidents::INCIDENTS_FILE;
use crate::incidents::Incident;
use crate::timecode;
use color_eyre::eyre;
use schemars::JsonSchema;
use serde::Serialize;
use std::collections::BTreeMap;
use std::collections::BTreeSet;
use std::collections::HashMap;

/// The schema name that identifies an [`OverviewData`] in an envelope (`overview/1`); listed by
/// the `schema` command.
pub const SCHEMA_OVERVIEW: &str = "overview/1";

/// How long before a bug region starts a finding still counts as a possible cause, in
/// nanoseconds: 10 s. [`Analysis::rank`] gives such a finding a lower boost than one inside the
/// region.
pub const PRE_ROLL_NS: u64 = 10_000_000_000;

/// Something in the recording that is worth looking at, with where to look.
///
/// What is looked for, and with which thresholds, is in the [module](self) documentation. A
/// finding is evidence, not a conclusion: a violation can be a state komorebi was in the middle of
/// leaving, and a burst of events can be a burst of echoes.
#[derive(Debug, Clone, PartialEq, Serialize, JsonSchema)]
pub struct Finding {
    /// `invariant:<kind>` (including the ones read from Windows' own account of a window when the
    /// recording was made with `--probe-windows`), `window-flapping`, `focus-oscillation`,
    /// `event-storm`, `slow-handler`, `log-error`, `log-warning`, `monitor-churn` or
    /// `incident:<kind>`.
    pub kind: String,
    /// How serious it is. For an `invariant:` finding it is the violation's, lowered one step if it
    /// did not last.
    pub severity: Severity,
    /// Higher is more worth looking at. `0` until [`Analysis::rank`] has scored the finding.
    pub score: i64,
    /// A sentence saying what was seen and, where that applies, for how long.
    pub summary: String,
    /// When it began, in seconds of video time.
    pub start_video_s: f64,
    /// When it ended, in seconds of video time; the same as `start_video_s` for a single moment.
    pub end_video_s: f64,
    /// When it began, in nanoseconds on the host's master monotonic clock.
    pub start_mach_ns: u64,
    /// When it ended, in nanoseconds on the host's master monotonic clock.
    pub end_mach_ns: u64,
    /// References to look at: `event:12`, `log:345`, `incident:2`. For a state that was wrong,
    /// the event whose state first showed it.
    pub refs: Vec<String>,
    /// The ids of the windows it is about, if it is about any.
    pub window_ids: Vec<u64>,
    /// Still the case when the recording ended.
    pub persisted_to_end: bool,
    /// The ids of the bug regions it overlaps, or comes within the pre-roll ([`PRE_ROLL_NS`])
    /// before.
    pub bugs: Vec<String>,
    /// A command that digs into it, ready to run. `None` for an incident.
    pub next: Option<String>,
}

/// An incident the recorder noted about the machine, such as a display being unplugged.
#[derive(Debug, Clone, PartialEq, Serialize, JsonSchema)]
pub struct IncidentView {
    /// `incident:<number>`: the incident's position in the session's incidents (`recorder.jsonl`),
    /// counting from 1.
    pub number: u64,
    /// When it happened, in seconds of video time.
    pub video_s: f64,
    /// The kind of incident, e.g. `display-disconnected`.
    pub kind: String,
    /// A sentence saying what the recorder saw.
    pub detail: String,
    /// The display it is about, if it is about one.
    pub display: Option<u32>,
}

/// One display that was recorded.
#[derive(Debug, Clone, PartialEq, Serialize, JsonSchema)]
pub struct DisplayInfo {
    /// The display id, the same number as the `--display` of the recording.
    pub id: u32,
    /// The width of the display's video in pixels, in its first segment.
    pub width_px: usize,
    /// The height of the display's video in pixels, in its first segment.
    pub height_px: usize,
    /// How many video files the display has: a display that was unplugged or slept and came back
    /// starts a new one.
    pub segments: usize,
}

/// The facts about a session an investigator wants before anything else.
#[derive(Debug, Clone, PartialEq, Serialize, JsonSchema)]
pub struct SessionInfo {
    /// The session directory.
    pub dir: String,
    /// `recording`, `complete` or `recovered`.
    pub status: String,
    /// When the recording started, as an RFC 3339 UTC time.
    pub started_utc: Option<String>,
    /// Seconds of video: the span findings and bug regions are placed in.
    pub duration_s: f64,
    /// The version of the recorder that made the session.
    pub recorder_version: String,
    /// The macOS version, when the session was recorded on a Mac. See `host` for any OS.
    pub macos_version: Option<String>,
    /// The host the session was recorded on.
    pub host: crate::platform::Host,
    /// The displays that were recorded, by id.
    pub displays: Vec<DisplayInfo>,
    /// The komorebi that was running, as its binary reported: `version branch commit`, with the
    /// commit shortened to 10 characters.
    pub komorebi: Vec<String>,
    /// The level komorebi was logging at, if the recorder saw it.
    pub komorebi_log_level: Option<String>,
    /// How many komorebi notifications were recorded.
    pub events: usize,
    /// How many lines komorebi's log has.
    pub log_lines: usize,
    /// How many incidents were recorded.
    pub incidents: usize,
    /// `host-clock` if the video's frame timestamps were verified to be on the same clock as the
    /// logs, `unverified` if not (then the alignment of video and logs is unreliable).
    pub sync_quality: String,
    /// Things the recorder said went wrong while recording.
    pub warnings: Vec<String>,
}

/// A slice of the recording and how much happened in it, for drawing a picture of the whole.
#[derive(Debug, Clone, PartialEq, Serialize, JsonSchema)]
pub struct Bucket {
    /// Where the slice starts, in seconds of video time.
    pub start_s: f64,
    /// Where the slice ends, in seconds of video time.
    pub end_s: f64,
    /// How many komorebi events were received in it.
    pub events: usize,
    /// Events that are not echoes of komorebi's own moves.
    pub real_events: usize,
    /// How many `ERROR` lines komorebi logged in it.
    pub log_errors: usize,
    /// How many `WARN` lines komorebi logged in it.
    pub log_warnings: usize,
    /// How many incidents happened in it.
    pub incidents: usize,
}

/// The whole recording at a glance: the `data` of the `overview` command.
#[derive(Debug, Clone, PartialEq, Serialize, JsonSchema)]
pub struct OverviewData {
    /// The facts about the session.
    pub session: SessionInfo,
    /// The regions the user marked, verbatim.
    pub bugs: Vec<crate::annotations::AnnotationView>,
    /// Ranked, most worth looking at first. At most the limit asked for.
    pub findings: Vec<Finding>,
    /// How many findings there were, including any left out by the limit.
    pub findings_total: usize,
    /// How much happened over the recording, in 60 buckets (fewer for a very short recording).
    pub histogram: Vec<Bucket>,
    /// Everything that happened to the machine: displays, sleep, lock, restarts.
    pub incidents: Vec<IncidentView>,
}

impl Analysis {
    /// Returns the whole recording at a glance: the session's facts, the findings ranked, a
    /// histogram and the incidents.
    ///
    /// The findings are those of [`Analysis::findings`] ranked by [`Analysis::rank`] against the
    /// bug regions the user marked ([`Analysis::bugs`]). `limit` cuts the list after that many;
    /// [`OverviewData::findings_total`] still counts all of them.
    ///
    /// # Errors
    ///
    /// Returns an error if the annotations file cannot be read, or if a recorded state cannot be
    /// read (see [`Analysis::findings`]).
    ///
    /// # Example
    ///
    /// ```rust
    /// use komorebi_recorder::analysis::Analysis;
    /// use komorebi_recorder::testkit::SessionBuilder;
    /// use komorebi_recorder::testkit::container;
    /// use komorebi_recorder::testkit::monitor;
    /// use komorebi_recorder::testkit::state;
    /// use komorebi_recorder::testkit::window;
    /// use komorebi_recorder::testkit::workspace;
    /// use serde_json::json;
    ///
    /// // The only window of the only workspace is parked in komorebi's hiding position, so
    /// // nothing is on screen although komorebi says it should be.
    /// let layout = [[0, 0, 1000, 800]];
    /// let parked = window(1, "Ghostty", Some([-999, 790, 1000, 800]));
    /// let screen = monitor(
    ///     1,
    ///     [0, 0, 1000, 800],
    ///     vec![workspace("one", vec![container("a", vec![parked])], 0, &layout)],
    ///     0,
    /// );
    /// let (session, _) = SessionBuilder::new("doc-overview")
    ///     .event(2.0, json!({"type": "Retile"}), Some(state(vec![screen], 0)))
    ///     .build();
    /// let analysis = Analysis::open(&session.dir)?;
    ///
    /// let overview = analysis.overview(None)?;
    /// assert_eq!(overview.session.events, 1);
    /// assert_eq!(overview.findings_total, 1);
    ///
    /// let finding = &overview.findings[0];
    /// assert_eq!(finding.kind, "invariant:visible-window-hidden");
    /// assert_eq!(finding.refs, ["event:1"]);
    /// // nothing changed it before the recording ended
    /// assert!(finding.persisted_to_end);
    /// # Ok::<(), Box<dyn std::error::Error>>(())
    /// ```
    ///
    /// Ranking against a bug region is covered by
    /// `tests/brief.rs::findings_in_the_bug_region_rank_above_the_rest`, and the findings of a
    /// Windows session by
    /// `tests/windows_flavor.rs::a_window_komorebi_shows_that_windows_cloaked_is_a_finding_in_the_overview`.
    pub fn overview(&self, limit: Option<usize>) -> eyre::Result<OverviewData> {
        let bugs = self.bugs()?;
        let refs: Vec<&Annotation> = bugs.iter().collect();
        let ranked = self.rank(self.findings()?, &refs);
        let total = ranked.len();

        Ok(OverviewData {
            session: self.session_info(),
            bugs: bugs.iter().map(|bug| bug.view(&self.manifest)).collect(),
            findings: ranked
                .into_iter()
                .take(limit.unwrap_or(usize::MAX))
                .collect(),
            findings_total: total,
            histogram: self.histogram(60),
            incidents: self.incidents(),
        })
    }

    /// Returns what happened to the recording environment: displays, sleep, lock, restarts, in
    /// the order they were recorded.
    ///
    /// A session that recorded no incidents has none to return, which is not an error. Each is
    /// numbered by its line in `recorder.jsonl` (so `incident:2` is the second line), and a line
    /// that cannot be read is skipped, leaving a gap in the numbers.
    ///
    /// # Example
    ///
    /// ```rust
    /// use komorebi_recorder::analysis::Analysis;
    /// use komorebi_recorder::testkit::SessionBuilder;
    ///
    /// let (session, _) = SessionBuilder::new("doc-incidents")
    ///     .incident(3.0, "display-disconnected", "display 2 was unplugged")
    ///     .build();
    /// let analysis = Analysis::open(&session.dir)?;
    ///
    /// let incidents = analysis.incidents();
    /// assert_eq!(incidents.len(), 1);
    /// assert_eq!(incidents[0].number, 1);
    /// assert_eq!(incidents[0].kind, "display-disconnected");
    /// assert!((incidents[0].video_s - 3.0).abs() < 1e-6);
    /// # Ok::<(), Box<dyn std::error::Error>>(())
    /// ```
    #[must_use]
    pub fn incidents(&self) -> Vec<IncidentView> {
        let Ok(text) = std::fs::read_to_string(self.session.path(INCIDENTS_FILE)) else {
            return Vec::new();
        };

        text.lines()
            .enumerate()
            .filter_map(|(index, line)| {
                let incident: Incident = serde_json::from_str(line).ok()?;
                Some(IncidentView {
                    number: index as u64 + 1,
                    video_s: self.video_s(incident.mach_ns),
                    kind: incident.kind,
                    detail: incident.detail,
                    display: incident.display,
                })
            })
            .collect()
    }

    /// Returns the facts about the session an investigator wants before anything else.
    ///
    /// This reads the manifest, and counts the events, the log lines and the incidents, so the
    /// events and the log are parsed if nothing has asked for them yet. It cannot fail: what is
    /// missing is empty or `None`.
    #[must_use]
    pub fn session_info(&self) -> SessionInfo {
        let manifest = &self.manifest;
        let mut displays: BTreeMap<u32, DisplayInfo> = BTreeMap::new();
        for entry in &manifest.displays {
            let info = displays
                .entry(entry.result.display.id)
                .or_insert(DisplayInfo {
                    id: entry.result.display.id,
                    width_px: entry.result.width_px,
                    height_px: entry.result.height_px,
                    segments: 0,
                });
            info.segments += 1;
        }

        SessionInfo {
            dir: self.session.dir.display().to_string(),
            status: manifest.status.clone(),
            started_utc: crate::timeline::format_wall_us(manifest.start.wall_us),
            duration_s: manifest.video_span().1,
            recorder_version: manifest.recorder_version.clone(),
            macos_version: manifest.macos_version.clone(),
            host: manifest.host(),
            displays: displays.into_values().collect(),
            komorebi: manifest
                .komorebi
                .versions
                .iter()
                .map(|version| {
                    format!(
                        "{} {} {}",
                        version.version,
                        version.branch.as_deref().unwrap_or(""),
                        version
                            .commit
                            .as_deref()
                            .map_or("", |commit| &commit[..commit.len().min(10)])
                    )
                    .trim()
                    .to_string()
                })
                .collect(),
            komorebi_log_level: manifest
                .komorebi
                .log_level
                .as_ref()
                .map(|level| level.level.clone()),
            events: self.events().len(),
            log_lines: self.log().lines.len(),
            incidents: self.incidents().len(),
            sync_quality: manifest.sync_quality.clone(),
            warnings: manifest.warnings.clone(),
        }
    }

    /// Returns everything the detectors found, in time order and unranked. Use
    /// [`Analysis::rank`] to order them by how much they matter.
    ///
    /// Findings are sorted by start time and then by kind. Each state is read from the session
    /// once, however many events carried it. What is looked for is in the [module](self)
    /// documentation.
    ///
    /// # Errors
    ///
    /// Returns an error if a recorded state cannot be read: its file is missing (not found) or is
    /// not valid JSON.
    pub fn findings(&self) -> eyre::Result<Vec<Finding>> {
        let mut found = Vec::new();
        self.find_state_problems(&mut found)?;
        self.find_probed_problems(&mut found)?;
        self.find_event_problems(&mut found);
        self.find_log_problems(&mut found);
        self.find_incidents(&mut found);
        found.sort_by_key(|finding| (finding.start_mach_ns, finding.kind.clone()));
        Ok(found)
    }

    pub(super) fn finding(
        &self,
        kind: &str,
        severity: Severity,
        summary: String,
        start_mach_ns: u64,
        end_mach_ns: u64,
    ) -> Finding {
        Finding {
            kind: kind.to_string(),
            severity,
            score: 0,
            summary,
            start_video_s: self.video_s(start_mach_ns),
            end_video_s: self.video_s(end_mach_ns.max(start_mach_ns)),
            start_mach_ns,
            end_mach_ns: end_mach_ns.max(start_mach_ns),
            refs: Vec::new(),
            window_ids: Vec::new(),
            persisted_to_end: false,
            bugs: Vec::new(),
            next: None,
        }
    }

    /// The end of the recording on the master clock.
    fn session_end_ns(&self) -> u64 {
        crate::analysis::timespec::session_end_ns(&self.manifest)
    }

    fn find_state_problems(&self, found: &mut Vec<Finding>) -> eyre::Result<()> {
        let end = self.session_end_ns();
        let dir = self.session.dir.display().to_string();

        // violations: when each appeared and when it went away
        type Key = (String, Vec<u64>, Option<u64>, Option<String>);
        struct Open {
            violation: invariants::Violation,
            start_ns: u64,
            seq: u64,
        }
        let mut open: HashMap<Key, Open> = HashMap::new();
        let mut closed: Vec<(Open, u64)> = Vec::new();

        // windows' hidden flags and focus, to find flapping and oscillation
        let mut hidden_before: HashMap<u64, bool> = HashMap::new();
        let mut flips: HashMap<u64, Vec<(u64, u64)>> = HashMap::new();
        let mut exes: HashMap<u64, String> = HashMap::new();
        let mut focus_series: Vec<(u64, u64, Option<u64>)> = Vec::new();

        let mut last_state_id = None;
        for event in self.events() {
            let Some(state_id) = event.state_id else {
                continue;
            };
            if last_state_id == Some(state_id) {
                continue;
            }
            last_state_id = Some(state_id);
            let snapshot = self.snapshot(state_id)?;

            let violations = invariants::check(&snapshot);
            let mut current: HashMap<Key, invariants::Violation> = HashMap::new();
            for violation in violations {
                let mut ids = violation.window_ids.clone();
                ids.sort_unstable();
                let key = (
                    violation.kind.clone(),
                    ids,
                    violation.monitor_id,
                    violation.workspace.clone(),
                );
                current.entry(key).or_insert(violation);
            }

            let gone: Vec<Key> = open
                .keys()
                .filter(|key| !current.contains_key(*key))
                .cloned()
                .collect();
            for key in gone {
                if let Some(opened) = open.remove(&key) {
                    closed.push((opened, event.recv_mach_ns));
                }
            }
            for (key, violation) in current {
                open.entry(key).or_insert(Open {
                    violation,
                    start_ns: event.recv_mach_ns,
                    seq: event.seq,
                });
            }

            for (location, window) in snapshot.windows() {
                exes.entry(window.id).or_insert_with(|| window.exe.clone());
                let hidden = window.rect.as_ref().is_some_and(|rect| {
                    snapshot
                        .monitors
                        .iter()
                        .find(|monitor| monitor.id == location.monitor_id)
                        .is_some_and(|monitor| snapshot.is_hidden_position(rect, monitor))
                });
                if let Some(before) = hidden_before.insert(window.id, hidden)
                    && before != hidden
                {
                    flips
                        .entry(window.id)
                        .or_default()
                        .push((event.recv_mach_ns, event.seq));
                }
            }
            focus_series.push((
                event.recv_mach_ns,
                event.seq,
                snapshot.focus().and_then(|focus| focus.window_id),
            ));
        }

        let mut push_violation = |opened: Open, ended: Option<u64>| {
            let persisted = ended.is_none();
            let end_ns = ended.unwrap_or(end);
            let lasted = end_ns.saturating_sub(opened.start_ns);
            // a state that was only wrong for an instant is usually a transition in progress
            let severity = if !persisted && lasted < 500_000_000 {
                match opened.violation.severity {
                    Severity::Error => Severity::Warn,
                    _ => Severity::Info,
                }
            } else {
                opened.violation.severity
            };

            let mut finding = self.finding(
                &format!("invariant:{}", opened.violation.kind),
                severity,
                format!(
                    "{}{}",
                    opened.violation.message,
                    if persisted {
                        ", and it was still so when the recording ended".to_string()
                    } else {
                        format!(", for {:.1} s", lasted as f64 / 1e9)
                    }
                ),
                opened.start_ns,
                end_ns,
            );
            finding.refs = vec![format!("event:{}", opened.seq)];
            finding.window_ids = opened.violation.window_ids.clone();
            finding.persisted_to_end = persisted;
            finding.next = Some(format!(
                "komorebi-recorder trace {dir} event:{}",
                opened.seq
            ));
            found.push(finding);
        };
        for (opened, ended) in closed {
            push_violation(opened, Some(ended));
        }
        for (_, opened) in open {
            push_violation(opened, None);
        }

        // a window parked and un-parked over and over
        for (window, times) in &flips {
            let mut best: Option<(usize, usize)> = None;
            for (start, (first, _)) in times.iter().enumerate() {
                let within = times[start..]
                    .iter()
                    .take_while(|(time, _)| time.saturating_sub(*first) <= 10_000_000_000)
                    .count();
                if within >= 4 && best.is_none_or(|(_, count)| within > count) {
                    best = Some((start, within));
                }
            }
            if let Some((start, count)) = best {
                let (first, seq) = times[start];
                let last = times[start + count - 1].0;
                let name = exes
                    .get(window)
                    .map_or_else(String::new, |exe| format!("{exe} "));
                let mut finding = self.finding(
                    "window-flapping",
                    Severity::Warn,
                    format!(
                        "{name}window {window} was parked off screen and brought back {count} times in {:.1} s",
                        last.saturating_sub(first) as f64 / 1e9
                    ),
                    first,
                    last,
                );
                finding.refs = vec![format!("event:{seq}")];
                finding.window_ids = vec![*window];
                finding.next = Some(format!("komorebi-recorder window {dir} {window}"));
                found.push(finding);
            }
        }

        // focus bouncing between two windows
        let mut start = 0;
        while start < focus_series.len() {
            let origin = focus_series[start].0;
            let mut changes = 0;
            let mut windows = BTreeSet::new();
            let mut last = None;
            let mut stop = start;
            for (index, (time, _, window)) in focus_series.iter().enumerate().skip(start) {
                if time.saturating_sub(origin) > 5_000_000_000 {
                    break;
                }
                if last.is_some() && *window != last {
                    changes += 1;
                }
                if let Some(window) = window {
                    windows.insert(*window);
                }
                last = *window;
                stop = index;
            }

            if changes >= 4 && windows.len() <= 2 {
                let (first, seq, _) = focus_series[start];
                let (end_time, _, _) = focus_series[stop];
                let names: Vec<String> = windows.iter().map(ToString::to_string).collect();
                let mut finding = self.finding(
                    "focus-oscillation",
                    Severity::Warn,
                    format!(
                        "focus bounced between window {} {changes} times in {:.1} s",
                        names.join(" and "),
                        end_time.saturating_sub(first) as f64 / 1e9
                    ),
                    first,
                    end_time,
                );
                finding.refs = vec![format!("event:{seq}")];
                finding.window_ids = windows.iter().copied().collect();
                finding.next = Some(format!("komorebi-recorder events {dir} --from event:{seq}"));
                found.push(finding);
                start = stop + 1;
            } else {
                start += 1;
            }
        }

        Ok(())
    }

    fn find_event_problems(&self, found: &mut Vec<Finding>) {
        let events = self.events();
        let pairing = self.pairing();
        let dir = self.session.dir.display().to_string();

        // bursts: twenty or more events within a second
        let mut start = 0;
        while start < events.len() {
            let mut stop = start;
            while stop + 1 < events.len()
                && events[stop + 1].recv_mach_ns - events[start].recv_mach_ns <= 1_000_000_000
            {
                stop += 1;
            }

            let count = stop - start + 1;
            if count >= 20 {
                let mut by_class: BTreeMap<String, usize> = BTreeMap::new();
                for index in start..=stop {
                    *by_class
                        .entry(format!("{:?}", pairing.classes[index].class).to_lowercase())
                        .or_default() += 1;
                }
                let breakdown: Vec<String> = by_class
                    .iter()
                    .map(|(class, count)| format!("{count} {class}"))
                    .collect();
                let mut finding = self.finding(
                    "event-storm",
                    if count >= 60 {
                        Severity::Warn
                    } else {
                        Severity::Info
                    },
                    format!(
                        "{count} events in {:.1} s ({})",
                        (events[stop].recv_mach_ns - events[start].recv_mach_ns) as f64 / 1e9,
                        breakdown.join(", ")
                    ),
                    events[start].recv_mach_ns,
                    events[stop].recv_mach_ns,
                );
                finding.refs = vec![
                    format!("event:{}", events[start].seq),
                    format!("event:{}", events[stop].seq),
                ];
                finding.next = Some(format!(
                    "komorebi-recorder events {dir} --from event:{} --to event:{} --hide-echo",
                    events[start].seq, events[stop].seq
                ));
                found.push(finding);
                start = stop + 1;
            } else {
                start += 1;
            }
        }

        // handlers that took a long time from their header to their notification
        let mut slow: Vec<(usize, f64)> = pairing
            .pairs
            .iter()
            .enumerate()
            .filter_map(|(index, pair)| pair.handler_ms.map(|ms| (index, ms)))
            .filter(|(_, ms)| *ms >= 250.0)
            .collect();
        slow.sort_by(|a, b| b.1.total_cmp(&a.1));
        for (index, ms) in slow.into_iter().take(10) {
            let event = &events[index];
            let mut finding = self.finding(
                "slow-handler",
                if ms >= 1000.0 {
                    Severity::Warn
                } else {
                    Severity::Info
                },
                format!(
                    "{} took at least {ms:.0} ms to handle: {}",
                    event.info.summary(),
                    self.flavor().rules().slow_handler_cause()
                ),
                event.recv_mach_ns.saturating_sub((ms * 1e6) as u64),
                event.recv_mach_ns,
            );
            finding.refs = vec![format!("event:{}", event.seq)];
            finding.next = Some(format!("komorebi-recorder trace {dir} event:{}", event.seq));
            found.push(finding);
        }

        // display changes arriving together
        let monitor: Vec<usize> = (0..events.len())
            .filter(|index| events[*index].info.family == Family::Monitor)
            .collect();
        let mut start = 0;
        while start < monitor.len() {
            let mut stop = start;
            while stop + 1 < monitor.len()
                && events[monitor[stop + 1]].recv_mach_ns - events[monitor[start]].recv_mach_ns
                    <= 5_000_000_000
            {
                stop += 1;
            }
            if stop > start {
                let (first, last) = (&events[monitor[start]], &events[monitor[stop]]);
                let mut finding = self.finding(
                    "monitor-churn",
                    Severity::Warn,
                    format!(
                        "{} display notifications in {:.1} s ({})",
                        stop - start + 1,
                        (last.recv_mach_ns - first.recv_mach_ns) as f64 / 1e9,
                        first.info.summary()
                    ),
                    first.recv_mach_ns,
                    last.recv_mach_ns,
                );
                finding.refs = vec![
                    format!("event:{}", first.seq),
                    format!("event:{}", last.seq),
                ];
                finding.next = Some(format!("komorebi-recorder trace {dir} event:{}", first.seq));
                found.push(finding);
                start = stop + 1;
            } else {
                start += 1;
            }
        }

        let _ = Class::Echo;
    }

    fn find_log_problems(&self, found: &mut Vec<Finding>) {
        #[derive(Default)]
        struct Group {
            count: usize,
            first: u64,
            last: u64,
            first_line: u64,
            sample: String,
        }

        let mut groups: BTreeMap<(Level, String, String), Group> = BTreeMap::new();
        for line in &self.log().lines {
            let (Some(level), Some(wall_us)) = (line.level, line.wall_us) else {
                continue;
            };
            if !matches!(level, Level::Error | Level::Warn) {
                continue;
            }
            let Some(mach_ns) = self.clock().wall_to_mach(wall_us) else {
                continue;
            };

            // the same problem with different numbers is one problem
            let shape: String = line
                .message
                .chars()
                .map(|character| {
                    if character.is_ascii_digit() {
                        '#'
                    } else {
                        character
                    }
                })
                .collect();
            let group = groups
                .entry((level, line.target.clone().unwrap_or_default(), shape))
                .or_default();
            if group.count == 0 {
                group.first = mach_ns;
                group.first_line = line.number;
                group.sample = line.message.clone();
            }
            group.count += 1;
            group.last = mach_ns;
        }

        let dir = self.session.dir.display().to_string();
        for ((level, target, _), group) in groups {
            let mut finding = self.finding(
                if level == Level::Error {
                    "log-error"
                } else {
                    "log-warning"
                },
                if level == Level::Error {
                    Severity::Warn
                } else {
                    Severity::Info
                },
                format!(
                    "{}x {} {target}: {}",
                    group.count,
                    format!("{level:?}").to_uppercase(),
                    group.sample
                ),
                group.first,
                group.last,
            );
            finding.refs = vec![format!("log:{}", group.first_line)];
            finding.next = Some(format!(
                "komorebi-recorder code {dir} log:{}",
                group.first_line
            ));
            found.push(finding);
        }
    }

    fn find_incidents(&self, found: &mut Vec<Finding>) {
        for incident in self.incidents() {
            let mach_ns = self.manifest.mach_at_video_seconds(incident.video_s);
            let severity = match incident.kind.as_str() {
                "capture-failed" | "log-stream-lost" | "komorebi-exited" => Severity::Warn,
                _ => Severity::Info,
            };
            let mut finding = self.finding(
                &format!("incident:{}", incident.kind),
                severity,
                format!(
                    "{}{}: {}",
                    incident.kind,
                    incident
                        .display
                        .map_or_else(String::new, |display| format!(" (display {display})")),
                    incident.detail
                ),
                mach_ns,
                mach_ns,
            );
            finding.refs = vec![format!("incident:{}", incident.number)];
            found.push(finding);
        }
    }

    /// Ranks findings: errors above warnings above notes, and anything in or just before a bug
    /// region above everything outside one.
    ///
    /// Each finding is given a [`Finding::score`], and the result is ordered by it, highest first,
    /// with earlier findings first among equal scores. The score is the sum of:
    ///
    /// | | points |
    /// |---|---|
    /// | severity: error, warn, info | 300, 100, 20 |
    /// | for each bug region the finding overlaps | 1000 |
    /// | for each bug region that starts within [`PRE_ROLL_NS`] after the finding ended | 400 |
    /// | the finding was still so when the recording ended | 50 |
    ///
    /// The ids of the bug regions that raised the score are added to [`Finding::bugs`]. An open
    /// region (one with no end) lasts to the end of the recording. With no bug regions only the
    /// severity and persistence count.
    ///
    /// Covered by `tests/brief.rs::findings_in_the_bug_region_rank_above_the_rest`.
    pub fn rank(&self, mut findings: Vec<Finding>, bugs: &[&Annotation]) -> Vec<Finding> {
        for finding in &mut findings {
            let base = match finding.severity {
                Severity::Error => 300,
                Severity::Warn => 100,
                Severity::Info => 20,
            };

            for bug in bugs {
                let (start, end) = (bug.start_mach_ns, bug.end_or_session_end(&self.manifest));
                if finding.start_mach_ns <= end && finding.end_mach_ns >= start {
                    finding.bugs.push(bug.id.clone());
                    finding.score += 1000;
                } else if finding.end_mach_ns < start && start - finding.end_mach_ns <= PRE_ROLL_NS
                {
                    finding.bugs.push(bug.id.clone());
                    finding.score += 400;
                }
            }

            finding.score += base;
            if finding.persisted_to_end {
                finding.score += 50;
            }
        }

        findings.sort_by(|a, b| {
            b.score
                .cmp(&a.score)
                .then(a.start_mach_ns.cmp(&b.start_mach_ns))
        });
        findings
    }

    /// Returns counts per time bucket: a picture of when things happened.
    ///
    /// `buckets` is the number of buckets wanted. The recording is divided into equal slices of at
    /// least half a second, so a very short recording has fewer buckets than asked for. The span
    /// is the video's, with a minimum of one second.
    /// Events and log lines before the video starts are not counted; those after it ends are
    /// counted in the last bucket.
    ///
    /// # Example
    ///
    /// ```rust
    /// use komorebi_recorder::analysis::Analysis;
    /// use komorebi_recorder::testkit::SessionBuilder;
    /// use serde_json::json;
    ///
    /// let (session, _) = SessionBuilder::new("doc-histogram")
    ///     .length(10.0)
    ///     .event(1.2, json!({"type": "Retile"}), None)
    ///     .event(1.8, json!({"type": "Retile"}), None)
    ///     .event(8.0, json!({"type": "Retile"}), None)
    ///     .build();
    /// let analysis = Analysis::open(&session.dir)?;
    ///
    /// // ten seconds in ten buckets of one second
    /// let histogram = analysis.histogram(10);
    /// assert_eq!(histogram.len(), 10);
    /// assert_eq!(histogram[1].events, 2);
    /// assert_eq!(histogram[8].events, 1);
    /// assert_eq!(histogram.iter().map(|bucket| bucket.events).sum::<usize>(), 3);
    /// # Ok::<(), Box<dyn std::error::Error>>(())
    /// ```
    #[must_use]
    pub fn histogram(&self, buckets: usize) -> Vec<Bucket> {
        let duration = self.manifest.video_span().1.max(1.0);
        let width = (duration / buckets.max(1) as f64).max(0.5);
        let count = (duration / width).ceil() as usize;
        let mut out: Vec<Bucket> = (0..count)
            .map(|index| Bucket {
                start_s: index as f64 * width,
                end_s: (index + 1) as f64 * width,
                events: 0,
                real_events: 0,
                log_errors: 0,
                log_warnings: 0,
                incidents: 0,
            })
            .collect();

        let place = |seconds: f64| -> Option<usize> {
            (seconds >= 0.0).then(|| ((seconds / width) as usize).min(count.saturating_sub(1)))
        };

        for (index, event) in self.events().iter().enumerate() {
            if let Some(slot) =
                place(self.video_s(event.recv_mach_ns)).and_then(|slot| out.get_mut(slot))
            {
                slot.events += 1;
                if self.pairing().classes[index].class != Class::Echo {
                    slot.real_events += 1;
                }
            }
        }
        for line in &self.log().lines {
            let (Some(level), Some(mach_ns)) = (
                line.level,
                line.wall_us
                    .and_then(|wall_us| self.clock().wall_to_mach(wall_us)),
            ) else {
                continue;
            };
            if let Some(slot) = place(self.video_s(mach_ns)).and_then(|slot| out.get_mut(slot)) {
                match level {
                    Level::Error => slot.log_errors += 1,
                    Level::Warn => slot.log_warnings += 1,
                    _ => {}
                }
            }
        }
        for incident in self.incidents() {
            if let Some(slot) = place(incident.video_s).and_then(|slot| out.get_mut(slot)) {
                slot.incidents += 1;
            }
        }

        out
    }

    /// Returns the bug regions the user marked: the annotations of kind `bug`, in the order
    /// [`crate::annotations::load`] gives them (by start time).
    ///
    /// # Errors
    ///
    /// Returns an error if the annotations file exists and cannot be read. A session with no
    /// annotations has no such file, which is not an error.
    pub fn bugs(&self) -> eyre::Result<Vec<Annotation>> {
        Ok(crate::annotations::load(&self.session.dir)?
            .annotations
            .into_iter()
            .filter(|annotation| annotation.kind == Kind::Bug)
            .collect())
    }
}

/// Returns the start of `finding` as a timecode, such as `00:12.345`.
///
/// # Example
///
/// ```rust
/// use komorebi_recorder::analysis::invariants::Severity;
/// use komorebi_recorder::analysis::overview::Finding;
/// use komorebi_recorder::analysis::overview::when;
///
/// let finding = Finding {
///     kind: "slow-handler".to_string(),
///     severity: Severity::Info,
///     score: 0,
///     summary: "FocusChange took at least 300 ms to handle".to_string(),
///     start_video_s: 12.345,
///     end_video_s: 12.345,
///     start_mach_ns: 0,
///     end_mach_ns: 0,
///     refs: vec![],
///     window_ids: vec![],
///     persisted_to_end: false,
///     bugs: vec![],
///     next: None,
/// };
/// assert_eq!(when(&finding), "00:12.345");
/// ```
#[must_use]
pub fn when(finding: &Finding) -> String {
    timecode::format(finding.start_video_s)
}
