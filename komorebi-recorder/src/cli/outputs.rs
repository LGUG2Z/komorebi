//! What `record`, `displays`, `reindex` and `selftest` print with `--json`.
//!
//! The investigation commands answer in the envelope of [`crate::analysis::envelope`] and so do
//! these, so that an agent driving the whole workflow (record, check what was recorded, investigate)
//! parses one shape. Each type here is the `data` of one envelope, named by its `SCHEMA_*` constant
//! and listed by `komorebi-recorder schema`. Without `--json` the commands print what they always
//! did.

use schemars::JsonSchema;
use serde::Serialize;

/// The `schema` of `record --json`: [`RecordData`].
pub const SCHEMA_RECORD: &str = "record/1";
/// The `schema` of `displays --json`: [`DisplaysData`].
pub const SCHEMA_DISPLAYS: &str = "displays/1";
/// The `schema` of `reindex --json`: [`ReindexData`].
pub const SCHEMA_REINDEX: &str = "reindex/1";
/// The `schema` of `selftest --json`: [`SelftestData`].
pub const SCHEMA_SELFTEST: &str = "selftest/1";

/// How the command of `record --until-exit` ended.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, JsonSchema)]
#[serde(rename_all = "kebab-case")]
pub enum ChildEnd {
    /// It ran to its end on its own, which stopped the recording; see `exit_code`.
    Exited,
    /// The recording ended first (Ctrl-C, `--duration`, or the recorder failing), and the command
    /// was terminated; it has no exit code that means anything.
    Stopped,
    /// The command could not be started (see `error`); the recording was stopped.
    FailedToStart,
    /// The recording failed before the command was started, so it never ran.
    NotStarted,
}

/// What became of the command of `record --until-exit`.
///
/// The recorder's own exit status is about the recording and never the command's: read
/// `exit_code` and `success` for that.
#[derive(Debug, Clone, PartialEq, Serialize, JsonSchema)]
pub struct ChildReport {
    /// The command and its arguments, as given after `--`.
    pub command: Vec<String>,
    /// How it ended.
    pub end: ChildEnd,
    /// Its exit code, when it [`Exited`](ChildEnd::Exited) with one (`None` if it was ended by a
    /// signal, or was stopped).
    pub exit_code: Option<i32>,
    /// Whether it exited with status 0. `None` unless it exited.
    pub success: Option<bool>,
    /// Why it could not be started.
    pub error: Option<String>,
}

/// One display of a recording, with the frames recorded for it.
#[derive(Debug, Clone, PartialEq, Serialize, JsonSchema)]
pub struct RecordedDisplay {
    /// The display id (what `--display` takes).
    pub id: u32,
    /// Frames of video written for it, across its segments.
    pub frames: u64,
    /// How many video files it has: a display that was unplugged or slept and came back starts a
    /// new one.
    pub segments: usize,
}

/// The `data` of `record --json`: what the recording that has just finished holds.
#[derive(Debug, Clone, PartialEq, Serialize, JsonSchema)]
pub struct RecordData {
    /// The session directory, as an absolute path where it could be made one.
    pub session: String,
    /// Seconds of video (or, with no video, of recording).
    pub duration_s: f64,
    /// The displays that were recorded, with their frames; empty for a recording without video.
    pub displays: Vec<RecordedDisplay>,
    /// Bytes of komorebi's log recorded.
    pub log_bytes: u64,
    /// komorebi events recorded.
    pub events: u64,
    /// OS trace records recorded, across streams.
    pub oslog_events: u64,
    /// What the recorder warned about.
    pub warnings: Vec<String>,
    /// The command of `--until-exit`, and how it ended; `None` without `--until-exit`.
    pub child: Option<ChildReport>,
}

/// One display that can be recorded.
#[derive(Debug, Clone, PartialEq, Serialize, JsonSchema)]
pub struct DisplayView {
    /// The id `--display` takes.
    pub id: u32,
    /// Width in points.
    pub width_points: i64,
    /// Height in points.
    pub height_points: i64,
    /// Where the display's top left corner is, x.
    pub x: f64,
    /// Where the display's top left corner is, y.
    pub y: f64,
}

/// The `data` of `displays --json`.
#[derive(Debug, Clone, PartialEq, Serialize, JsonSchema)]
pub struct DisplaysData {
    /// The displays that can be recorded.
    pub displays: Vec<DisplayView>,
}

/// The `data` of `reindex --json`.
#[derive(Debug, Clone, PartialEq, Serialize, JsonSchema)]
pub struct ReindexData {
    /// The session directory.
    pub session: String,
    /// Notes on what was recovered; empty if the session had been finished properly.
    pub recovered: Vec<String>,
    /// How many records the rebuilt index has.
    pub entries: u64,
    /// How many of those are from each source.
    pub by_source: std::collections::BTreeMap<String, u64>,
}

/// One reading of the self-test.
#[derive(Debug, Clone, PartialEq, Serialize, JsonSchema)]
pub struct SelftestSample {
    /// Where in the video it was taken, in seconds.
    pub video_s: f64,
    /// The clock shown in the video minus the clock the manifest predicts, in milliseconds.
    pub error_ms: f64,
}

/// The `data` of `selftest --json`. A self-test that fails is reported as a failed envelope instead.
#[derive(Debug, Clone, PartialEq, Serialize, JsonSchema)]
pub struct SelftestData {
    /// The self-test's recording.
    pub session: String,
    /// Whether the worst error was within the tolerance (always `true` here).
    pub passed: bool,
    /// The largest disagreement, in milliseconds.
    pub max_abs_error_ms: f64,
    /// The mean disagreement, in milliseconds: the clock is painted slightly before it is shown.
    pub mean_bias_ms: f64,
    /// The tolerance asked for, in milliseconds.
    pub tolerance_ms: f64,
    /// The readings.
    pub samples: Vec<SelftestSample>,
}
