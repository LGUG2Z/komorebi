//! The JSON Schema of every command's output, so a caller can know what to expect.
//!
//! An [`Envelope`] names the command whose `data` it carries in its `schema` field (`brief/1`).
//! [`ALL`] lists those names with a sentence about each and a function that produces the JSON
//! Schema of the `data` (generated from the Rust type with `schemars`, so the schema and the output
//! cannot drift apart). [`find`] looks one up by name. `komorebi-recorder schema` prints the list,
//! and `komorebi-recorder schema <name>` prints one schema.
//!
//! The list is written by hand, one entry per name. `guide --json` names its `data` `guide/1`,
//! which is not listed.

use super::answers::AnnotationData;
use super::answers::AnnotationsData;
use super::answers::DeletedData;
use super::answers::DiffData;
use super::answers::EventsData;
use super::answers::StateData;
use super::blank::BlankData;
use super::brief::BriefData;
use super::changes::ChangesData;
use super::code::CodeData;
use super::compare::CompareData;
use super::envelope::Envelope;
use super::framediff::FrameDiffData;
use super::history::HistoryData;
use super::lag::LagData;
use super::logq::LogqData;
use super::measure::MeasureData;
use super::motion::MotionData;
use super::overview::OverviewData;
use super::search::SearchData;
use super::sheet::SheetData;
use super::trace::TraceData;
use schemars::schema_for;
use serde_json::Value;

/// A command output's schema: its name, a sentence about it, and the JSON Schema of its `data`.
///
/// The schema is produced on demand by calling [`Entry::schema`]: it is not stored.
pub struct Entry {
    /// The name that appears in an envelope's `schema`: the command and a version, `brief/1`.
    pub name: &'static str,
    /// A sentence naming the command that prints this and describing its `data`.
    pub about: &'static str,
    /// Returns the JSON Schema of the output's `data`. For `error/1` it is the schema of the whole
    /// failed [`Envelope`], because a failed command has no `data`. It returns `null` if the
    /// schema could not be turned into JSON.
    pub schema: fn() -> Value,
}

fn of<T: schemars::JsonSchema>() -> Value {
    serde_json::to_value(schema_for!(T)).unwrap_or(Value::Null)
}

/// Every schema, by the name that appears in an envelope's `schema`.
///
/// The order is the order `komorebi-recorder schema` lists them in.
pub const ALL: &[Entry] = &[
    Entry {
        name: "brief/1",
        about: "`brief`: the user's bug regions and what the recording shows around each",
        schema: of::<BriefData>,
    },
    Entry {
        name: "overview/1",
        about: "`overview`: ranked findings, a histogram of the recording and its incidents",
        schema: of::<OverviewData>,
    },
    Entry {
        name: "state/1",
        about: "`state`: komorebi's state at a moment, with anything wrong in it",
        schema: of::<StateData>,
    },
    Entry {
        name: "diff/1",
        about: "`diff`: what changed between two moments",
        schema: of::<DiffData>,
    },
    Entry {
        name: "events/1",
        about: "`events`: komorebi's events, classified, with the handler that ran",
        schema: of::<EventsData>,
    },
    Entry {
        name: "trace/1",
        about: "`trace`: one event end to end with attributed changes",
        schema: of::<TraceData>,
    },
    Entry {
        name: "window/1",
        about: "`window`: one window's life through the recording",
        schema: of::<HistoryData>,
    },
    Entry {
        name: "code/1",
        about: "`code`: log lines mapped to komorebi's source",
        schema: of::<CodeData>,
    },
    Entry {
        name: "search/1",
        about: "`search`: where a word appears in the recording",
        schema: of::<SearchData>,
    },
    Entry {
        name: "logq/1",
        about: "`logq`: lines of komorebi's log matching a field query, counted or grouped",
        schema: of::<LogqData>,
    },
    Entry {
        name: "changes/1",
        about: "`changes`: when and where the picture changed on each display",
        schema: of::<ChangesData>,
    },
    Entry {
        name: "sheet/1",
        about: "`sheet`: a contact sheet image and where each frame is on it",
        schema: of::<SheetData>,
    },
    Entry {
        name: "motion/1",
        about: "`motion`: activity segments of a display or a region of it, with peak share and brightness delta",
        schema: of::<MotionData>,
    },
    Entry {
        name: "framediff/1",
        about: "`framediff`: how two frames differ: changed share, brightness delta, box of change",
        schema: of::<FrameDiffData>,
    },
    Entry {
        name: "blank/1",
        about: "`blank`: stretches where a region was blank (one colour) or frozen",
        schema: of::<BlankData>,
    },
    Entry {
        name: "measure/1",
        about: "`measure`: mean or peak brightness or drawn width of a region, on a fixed-rate timeline",
        schema: of::<MeasureData>,
    },
    Entry {
        name: "compare/1",
        about: "`compare`: two recordings side by side with the difference of each measure",
        schema: of::<CompareData>,
    },
    Entry {
        name: "annotation/1",
        about: "`annotate add|edit`: the annotation as it now reads",
        schema: of::<AnnotationData>,
    },
    Entry {
        name: "annotations/1",
        about: "`annotations`: bug regions, notes and findings",
        schema: of::<AnnotationsData>,
    },
    Entry {
        name: "annotation-deleted/1",
        about: "`annotate delete`: the id removed",
        schema: of::<DeletedData>,
    },
    Entry {
        name: "mark/1",
        about: "`mark`: the marker dropped on a recording that is still going",
        schema: of::<crate::cli::mark::MarkData>,
    },
    Entry {
        name: "doctor/1",
        about: "`doctor`: what this machine can do for a recording",
        schema: of::<crate::doctor::Report>,
    },
    Entry {
        name: "probe/1",
        about: "`doctor --probe`: what one measurement found",
        schema: of::<crate::doctor::ProbeReport>,
    },
    Entry {
        name: "brief-compact/1",
        about: "`brief --compact`: the brief cut down to the user's words, the top findings and what to run next",
        schema: of::<super::compact::CompactBrief>,
    },
    Entry {
        name: "findings/1",
        about: "`findings`: detector findings (with stable ids) and an agent's findings, together",
        schema: of::<super::findings::FindingsData>,
    },
    Entry {
        name: "lag/1",
        about: "`lag`: episodes of a window off its layout rect (or a border off its window), with duration and offsets",
        schema: of::<LagData>,
    },
    Entry {
        name: "assert/1",
        about: "`assert`: pass, fail or error for each scripted check, with what was observed",
        schema: of::<super::assertions::AssertData>,
    },
    Entry {
        name: "record/1",
        about: "`record --json`: the finished session, its displays and frames, warnings and the --until-exit command's status",
        schema: of::<crate::cli::outputs::RecordData>,
    },
    Entry {
        name: "displays/1",
        about: "`displays --json`: the displays that can be recorded",
        schema: of::<crate::cli::outputs::DisplaysData>,
    },
    Entry {
        name: "reindex/1",
        about: "`reindex --json`: what was recovered and how many records the index has",
        schema: of::<crate::cli::outputs::ReindexData>,
    },
    Entry {
        name: "selftest/1",
        about: "`selftest --json`: the video-to-clock readings of a passing self-test",
        schema: of::<crate::cli::outputs::SelftestData>,
    },
    Entry {
        name: "error/1",
        about: "any failed command: the envelope with `ok: false` and an `error`",
        schema: of::<Envelope<()>>,
    },
];

/// Returns the schema named `name`, or `None` if there is none.
///
/// The name is matched exactly, version included: `brief/1`, not `brief`.
///
/// # Example
///
/// ```rust
/// use komorebi_recorder::analysis::schemas;
///
/// let entry = schemas::find("brief/1").expect("the brief schema is listed");
/// assert_eq!(entry.name, "brief/1");
/// assert!((entry.schema)().is_object());
///
/// assert!(schemas::find("brief").is_none());
/// assert!(schemas::ALL.iter().any(|entry| entry.name == "error/1"));
/// ```
#[must_use]
pub fn find(name: &str) -> Option<&'static Entry> {
    ALL.iter().find(|entry| entry.name == name)
}
