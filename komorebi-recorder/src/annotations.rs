//! What people and agents say about a recording: where a bug starts and ends, notes, findings.
//!
//! The most important annotation is the **bug region**: the stretch of the recording during which
//! the user saw something wrong, with a description of what it looked like. It is the ground truth
//! of *what* went wrong that every investigation starts from; the rest of the session is evidence
//! of *why*. An agent's conclusion is written back as a `finding` that points at its evidence.
//!
//! They live in `annotations.jsonl`, one JSON object per line, **append-only**: the viewer and an
//! agent may both add to it, even while the recording is still running, and a crash
//! mid-write can only damage the last line. Changing or removing an annotation appends an `edit` or
//! `delete` line for its id; reading folds the lines in order. Times are on the master clock
//! (`*_mach_ns`), not in video seconds, so they stay right if `reindex` moves video t=0.
//!
//! A region whose end was never set is *open-ended*: the bug was still happening when the recording
//! stopped. Readers treat it as ending when the recording did and say so.
//!
//! # The three kinds
//!
//! | [`Kind`] | What it is | Written by |
//! |---|---|---|
//! | [`Kind::Bug`] | a region of the recording where the bug was visible, in the user's words | the viewer, `komorebi-recorder annotate` (author [`AUTHOR_USER`]) |
//! | [`Kind::Note`] | a remark about a moment or region | `komorebi-recorder annotate` |
//! | [`Kind::Finding`] | an agent's conclusion about the cause of a bug, with [`Ref`]s to its evidence | `komorebi-recorder annotate` (author `agent` unless told otherwise) |
//!
//! The kind is a lowercase word in the file, and prints the same way:
//!
//! ```rust
//! use komorebi_recorder::annotations::Kind;
//!
//! assert_eq!(Kind::Finding.to_string(), "finding");
//! assert_eq!(serde_json::to_string(&Kind::Bug)?, r#""bug""#);
//! assert_eq!(serde_json::from_str::<Kind>(r#""note""#)?, Kind::Note);
//! # Ok::<(), Box<dyn std::error::Error>>(())
//! ```
//!
//! # Writing and reading
//!
//! | To | Use | Lines appended |
//! |---|---|---|
//! | add an annotation | [`add`] with a [`New`] | one `add` |
//! | change one | [`edit`] with a [`Patch`] | one `edit` carrying only the changed fields |
//! | remove one | [`delete`] | one `delete`, a tombstone: the earlier lines stay |
//! | read them | [`load`], or [`fold`] over text already read | none |
//! | check evidence | [`validate_refs`] | none |
//!
//! Every write goes through [`append`], which serializes writers on `annotations.lock` and
//! closes off a line a crashed writer left unterminated. [`fold`] never fails: a line it cannot
//! understand is skipped and reported in [`Loaded::warnings`], and the rest of the file is still
//! read.
//!
//! # What is enforced here, and what is not
//!
//! This module checks that an end is not before its start ([`add`], [`edit`]) and, on request, that
//! evidence exists ([`validate_refs`]). It does not require a bug to have a description, a finding
//! to have evidence, or a finding to name the bug it is about.
//! [`Annotation::needs_description`] flags a bug that is still without a description. `komorebi-recorder annotate add` is stricter (it requires a
//! description, and evidence for a finding), and a hand-written line is held to neither: [`fold`]
//! accepts it.
//!
//! # Example
//!
//! A user marks a region that is still open, adds the description afterwards, and an agent records
//! its conclusion. All of it is lines in one file, folded on read:
//!
//! ```rust
//! use komorebi_recorder::annotations::{Kind, Ref, fold};
//!
//! let text = [
//!     r#"{"v":1,"id":"a-00000001","op":"add","kind":"bug","start_mach_ns":2000000000}"#,
//!     r#"{"v":1,"id":"a-00000001","op":"edit","end_mach_ns":5000000000,"text":"tiles overlap"}"#,
//!     r#"{"v":1,"id":"a-00000002","op":"add","kind":"finding","start_mach_ns":2000000000,"about":"a-00000001","refs":[{"kind":"event","seq":12}]}"#,
//! ]
//! .join("\n");
//!
//! let loaded = fold(&text);
//! assert!(loaded.warnings.is_empty());
//!
//! let bug = loaded.get("a-00000001").expect("the bug");
//! assert_eq!(bug.kind, Kind::Bug);
//! assert_eq!(bug.end_mach_ns, Some(5_000_000_000));
//! assert_eq!(bug.text, "tiles overlap");
//!
//! let finding = loaded.get("a-00000002").expect("the finding");
//! assert_eq!(finding.about.as_deref(), Some("a-00000001"));
//! assert_eq!(finding.refs, [Ref::Event { seq: 12 }]);
//! ```
//!
//! Writing goes through the file system; [`add`], [`edit`], [`delete`] and [`load`] have examples
//! that use a temporary session directory.

use crate::analysis::envelope;
use crate::analysis::flavor::Flavor;
use crate::analysis::guard::SessionPath;
use crate::analysis::guard::is_plain_relative;
use crate::analysis::ids;
use crate::clock;
use crate::clock::ClockMap;
use crate::session::Manifest;
use crate::session::Session;
use crate::timeline;
use color_eyre::eyre;
use color_eyre::eyre::WrapErr;
use schemars::JsonSchema;
use serde::Deserialize;
use serde::Serialize;
use std::collections::BTreeMap;
use std::fmt;
use std::fs::OpenOptions;
use std::io::Read;
use std::io::Seek;
use std::io::SeekFrom;
use std::io::Write;
use std::path::Path;
use std::path::PathBuf;
use std::str::FromStr;

/// The name, inside a session directory, of the file the annotations are appended to.
pub const ANNOTATIONS_FILE: &str = "annotations.jsonl";
/// The name, inside a session directory, of the file writers lock so that their lines do not
/// interleave.
///
/// It is created empty and nothing is ever written to it. It is a separate file because Windows
/// locks are mandatory: locking [`ANNOTATIONS_FILE`] itself would make it unreadable for everyone
/// else while a line is added.
pub const ANNOTATIONS_LOCK_FILE: &str = "annotations.lock";

/// The version written to every line.
///
/// [`fold`] skips a line whose version is greater than this one, and reports it in
/// [`Loaded::warnings`]. A line with a lower version is read as written.
pub const LINE_VERSION: u32 = 1;

/// The author recorded when none is given: [`add`] uses it for a [`New`] without an author, and
/// [`fold`] for an `add` line without one.
pub const AUTHOR_USER: &str = "user";

/// The kind of an annotation: `bug`, `note` or `finding`.
///
/// The kind is fixed when the annotation is added: an edit cannot change it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum Kind {
    /// A region where the bug was visible, described by the user.
    Bug,
    /// A remark about a moment or a region.
    Note,
    /// An agent's conclusion about what caused a bug, pointing at its evidence.
    Finding,
}

impl fmt::Display for Kind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Bug => "bug",
            Self::Note => "note",
            Self::Finding => "finding",
        })
    }
}

/// The operation a [`Line`] performs on the annotation it names.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Op {
    /// Creates the annotation. A line with no `op` is an `add`.
    Add,
    /// Changes the fields the line carries and leaves the rest as they were.
    Edit,
    /// Removes the annotation. The id cannot be added again.
    Delete,
}

/// Evidence a finding points at.
///
/// Each kind names something in the session, or in komorebi's source, that a reader can open; the
/// annotate command checks that it exists. On the command line a reference is written `event:12`,
/// `log:345`, `log:oslog/komorebi.ndjson:77`, `code:komorebi/src/window.rs:210` or `frame:12.5`.
/// In `annotations.jsonl` and in `--json` output it is an object tagged with `kind`, such as
/// `{"kind":"event","seq":12}`.
///
/// The text form is read by [`str::parse`] and printed by [`Display`](fmt::Display); the example is
/// on the [`FromStr`] implementation, and [`validate_refs`] does the checking.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "kind", rename_all = "lowercase")]
pub enum Ref {
    /// A komorebi subscriber event.
    Event {
        /// The event's sequence number, as in `events.jsonl` and the `#<seq>` of the `events`
        /// command.
        seq: u64,
    },
    /// A line of a recorded text file (the komorebi log slice by default).
    Log {
        /// The file, as a path relative to the session directory.
        file: String,
        /// The line, counted from 1.
        line: u64,
    },
    /// A line of komorebi's source, relative to the repository root.
    Code {
        /// The file, as a path relative to the root of komorebi's repository.
        path: String,
        /// The line, counted from 1.
        line: u64,
    },
    /// A moment in the video.
    Frame {
        /// Seconds of video time: from the first frame of any display.
        video_s: f64,
    },
}

/// The file `log:<line>` refers to when it names no file: komorebi's plaintext log in the session
/// directory.
pub const DEFAULT_LOG_FILE: &str = "komorebi.log";

impl fmt::Display for Ref {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Event { seq } => write!(f, "event:{seq}"),
            Self::Log { file, line } => write!(f, "log:{file}:{line}"),
            Self::Code { path, line } => write!(f, "code:{path}:{line}"),
            Self::Frame { video_s } => write!(f, "frame:{}", crate::timecode::format(*video_s)),
        }
    }
}

impl FromStr for Ref {
    type Err = eyre::Report;

    /// Reads a reference: `event:12`, `log:345`, `log:oslog/komorebi.ndjson:77`,
    /// `code:komorebi/src/window.rs:210` or `frame:12.5`.
    ///
    /// For `log:` and `code:` the line is what follows the last colon. A `log:` whose rest is a
    /// whole number is a line of [`DEFAULT_LOG_FILE`]. The time of a `frame:` is any form
    /// [`crate::timecode::parse`] reads. [`Display`](fmt::Display) prints the same forms, with the
    /// file of a `log:` always spelled out.
    ///
    /// # Errors
    ///
    /// Returns a usage error (`komorebi-recorder` exits with code 2 on it) when the text has no
    /// colon, the kind is not `event`, `log`, `code` or `frame`, the sequence number is not a whole
    /// number, or a `log:` or `code:` does not end in a colon and a whole number. A `frame:` whose
    /// time cannot be parsed returns the error of [`crate::timecode::parse`], which is an ordinary
    /// error (exit code 1).
    ///
    /// This only checks the spelling; whether the thing exists is [`validate_refs`]'s question.
    ///
    /// # Example
    ///
    /// ```rust
    /// use komorebi_recorder::annotations::Ref;
    ///
    /// let evidence: Ref = "code:komorebi/src/window.rs:210".parse()?;
    /// assert_eq!(
    ///     evidence,
    ///     Ref::Code { path: "komorebi/src/window.rs".to_string(), line: 210 },
    /// );
    /// assert_eq!(
    ///     serde_json::to_string(&evidence)?,
    ///     r#"{"kind":"code","path":"komorebi/src/window.rs","line":210}"#,
    /// );
    ///
    /// // `log:<line>` means the komorebi log; printing spells the file out
    /// let line: Ref = "log:345".parse()?;
    /// assert_eq!(line.to_string(), "log:komorebi.log:345");
    /// assert_eq!(line.to_string().parse::<Ref>()?, line);
    ///
    /// // a video time is printed as a timecode
    /// let frame: Ref = "frame:12.5".parse()?;
    /// assert_eq!(frame.to_string(), "frame:00:12.500");
    ///
    /// // a malformed reference is refused, and says what was expected
    /// let error = "cat:1".parse::<Ref>().unwrap_err();
    /// assert_eq!(error.to_string(), "'cat:1': unknown kind of reference");
    /// # Ok::<(), Box<dyn std::error::Error>>(())
    /// ```
    fn from_str(input: &str) -> Result<Self, Self::Err> {
        let bad = |why: &str| envelope::usage(format!("'{input}': {why}"));
        let (kind, rest) = input.split_once(':').ok_or_else(|| {
            bad("expected event:<seq>, log:<line>, code:<path>:<line> or frame:<time>")
        })?;
        // `<anything>:<line>`: the line is after the last colon
        let file_and_line = |rest: &str| -> eyre::Result<(String, u64)> {
            let (file, line) = rest
                .rsplit_once(':')
                .ok_or_else(|| bad("expected <path>:<line>"))?;
            Ok((
                file.to_string(),
                line.parse()
                    .map_err(|_| bad("the line must be a whole number"))?,
            ))
        };

        match kind {
            "event" => Ok(Self::Event {
                seq: rest.parse().map_err(|_| bad("an event sequence number"))?,
            }),
            "log" => match rest.parse::<u64>() {
                Ok(line) => Ok(Self::Log {
                    file: DEFAULT_LOG_FILE.to_string(),
                    line,
                }),
                Err(_) => {
                    let (file, line) = file_and_line(rest)?;
                    Ok(Self::Log { file, line })
                }
            },
            "code" => {
                let (path, line) = file_and_line(rest)?;
                Ok(Self::Code { path, line })
            }
            "frame" => Ok(Self::Frame {
                video_s: crate::timecode::parse(rest)?,
            }),
            _ => Err(bad("unknown kind of reference")),
        }
    }
}

/// One line of `annotations.jsonl`: a single JSON object, the unit [`append`] writes and [`fold`]
/// reads.
///
/// Only an `add` carries everything. An `edit` carries the id and whichever fields it changes
/// (a field left `None` is absent from the JSON and leaves the annotation's value alone), and a
/// `delete` only the id. A field left `None` is left out of the JSON, except `op`, which is written
/// as `null`.
///
/// An `edit` line cannot change the `kind`, the `author` or what the annotation is `about`: [`fold`]
/// ignores those fields on an `edit`.
///
/// # Example
///
/// ```rust
/// use komorebi_recorder::annotations::{Kind, Line, Op};
///
/// let line = Line {
///     v: 1,
///     id: "a-00000001".to_string(),
///     op: Some(Op::Add),
///     kind: Some(Kind::Note),
///     start_mach_ns: Some(2_000_000_000),
///     text: Some("focus moved here".to_string()),
///     ..Line::default()
/// };
/// assert_eq!(
///     serde_json::to_string(&line)?,
///     r#"{"v":1,"id":"a-00000001","op":"add","kind":"note","start_mach_ns":2000000000,"text":"focus moved here"}"#,
/// );
///
/// // a line that says nothing else: `op` is the only optional field that is still written
/// let bare = Line { v: 1, id: "a-00000001".to_string(), ..Line::default() };
/// assert_eq!(serde_json::to_string(&bare)?, r#"{"v":1,"id":"a-00000001","op":null}"#);
/// # Ok::<(), Box<dyn std::error::Error>>(())
/// ```
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Line {
    /// The format version of the line (see [`LINE_VERSION`]).
    pub v: u32,
    /// The id of the annotation the line is about. [`add`] makes one of `a-` and eight
    /// hexadecimal digits.
    pub id: String,
    /// The operation the line performs; `None` reads as [`Op::Add`].
    pub op: Option<Op>,
    /// The kind of annotation. Required on an `add`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub kind: Option<Kind>,
    /// Where the annotation starts, in nanoseconds on the host's master monotonic clock. Required
    /// on an `add`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub start_mach_ns: Option<u64>,
    /// Where the annotation ends, on the same clock. `None` on an `add` makes the region
    /// open-ended.
    ///
    /// Once an end is set it can be moved but not cleared.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub end_mach_ns: Option<u64>,
    /// A short title.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    /// The description: what a bug looks like, the remark of a note or the conclusion of a finding.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub text: Option<String>,
    /// What should have happened instead. An `edit` that sets it to the empty string clears it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub expected: Option<String>,
    /// The displays it was seen on. An `edit` replaces the list.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub displays: Option<Vec<u32>>,
    /// Who wrote it. Read as [`AUTHOR_USER`] when absent.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub author: Option<String>,
    /// For a finding: the bug annotation it explains.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub about: Option<String>,
    /// The evidence of a finding. An `edit` replaces the list.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub refs: Option<Vec<Ref>>,
    /// When the line was written, in microseconds since the Unix epoch on the wall clock.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub at_wall_us: Option<i64>,
}

/// An annotation after folding every line about it.
///
/// This is what [`fold`] and [`load`] produce. [`Annotation::view`] turns it into the form shown to
/// a reader.
#[derive(Debug, Clone, PartialEq)]
pub struct Annotation {
    /// The id. [`add`] makes one of `a-` and eight hexadecimal digits.
    pub id: String,
    /// The kind of annotation.
    pub kind: Kind,
    /// Where it starts, in nanoseconds on the host's master monotonic clock.
    pub start_mach_ns: u64,
    /// Where it ends, on the same clock. `None` means the region is open-ended: use
    /// [`Annotation::end_or_session_end`] for a time to draw.
    pub end_mach_ns: Option<u64>,
    /// A short title; empty if there is none.
    pub title: String,
    /// The description: what a bug looks like, the remark of a note or the conclusion of a finding;
    /// empty if there is none.
    pub text: String,
    /// What should have happened instead.
    pub expected: Option<String>,
    /// The displays it was seen on; empty means any.
    pub displays: Vec<u32>,
    /// Who wrote it: [`AUTHOR_USER`] or whatever name the writer gave.
    pub author: String,
    /// For a finding: the id of the bug it explains.
    pub about: Option<String>,
    /// The evidence a finding points at.
    pub refs: Vec<Ref>,
    /// When the `add` line was written, in microseconds since the Unix epoch on the wall clock.
    pub created_wall_us: Option<i64>,
    /// When the last `edit` line was written, on the same clock; `None` if it was never edited.
    pub edited_wall_us: Option<i64>,
}

impl Annotation {
    /// Returns `true` if and only if this is a bug whose description is empty or blank.
    ///
    /// A bug that says nothing about what went wrong is not much use to an agent, so the flag is
    /// carried to the commands' output as [`AnnotationView::needs_description`].
    #[must_use]
    pub fn needs_description(&self) -> bool {
        self.kind == Kind::Bug && self.text.trim().is_empty()
    }

    /// Returns `true` if and only if the region has no end: it was still happening when the
    /// recording ended.
    #[must_use]
    pub fn open_ended(&self) -> bool {
        self.end_mach_ns.is_none()
    }

    /// Returns the end of the annotation on the master clock, or, for an open region, when the
    /// recording ended.
    ///
    /// The result is never before [`Annotation::start_mach_ns`]: an end that was written earlier
    /// than the start (by hand, since [`add`] and [`edit`] refuse it) is lifted to the start. The
    /// end of the recording is the manifest's stop time; failing that the end of its video, and
    /// failing that the moment recording began.
    #[must_use]
    pub fn end_or_session_end(&self, manifest: &Manifest) -> u64 {
        self.end_mach_ns
            .unwrap_or_else(|| crate::analysis::timespec::session_end_ns(manifest))
            .max(self.start_mach_ns)
    }
}

/// An annotation as it is shown to a reader, with both clocks filled in.
///
/// This is the shape of an annotation in the `annotation/1`, `annotations/1`, `brief/1` and
/// `overview/1` outputs. [`Annotation::view`] makes it.
#[derive(Debug, Clone, PartialEq, Serialize, JsonSchema)]
pub struct AnnotationView {
    /// The id, such as `a-3f9c1e02`.
    pub id: String,
    /// The kind of annotation.
    pub kind: Kind,
    /// Who wrote it.
    pub author: String,
    /// A short title; empty if there is none.
    pub title: String,
    /// What the bug looks like, in the user's words.
    pub text: String,
    /// What should have happened instead.
    pub expected: Option<String>,
    /// The displays it was seen on; empty means any.
    pub displays: Vec<u32>,
    /// Where it starts, in seconds of video time (negative if it starts before the first frame).
    pub start_video_s: f64,
    /// The end, or the end of the recording for an open-ended region, in seconds of video time.
    pub end_video_s: f64,
    /// The start as a UTC time in RFC 3339 form with microseconds; `None` if the session has no
    /// clock anchors to convert with.
    pub start_wall_utc: Option<String>,
    /// The end, as `start_wall_utc` is.
    pub end_wall_utc: Option<String>,
    /// The start, in nanoseconds on the host's master monotonic clock.
    pub start_mach_ns: u64,
    /// The end on the same clock; `None` for an open-ended region.
    pub end_mach_ns: Option<u64>,
    /// The region has no end: it was still happening when the recording stopped.
    pub open_ended: bool,
    /// A bug without a description.
    pub needs_description: bool,
    /// For a finding: the id of the bug it explains.
    pub about: Option<String>,
    /// The evidence a finding points at.
    pub refs: Vec<Ref>,
}

impl Annotation {
    /// Returns the annotation as a reader sees it: the times on the video, wall and master clocks,
    /// and an open region closed at the end of the recording.
    ///
    /// The video times come from `manifest` ([`Manifest::video_seconds`]) and the UTC times from
    /// its clock anchors, so a view is only as good as the manifest it is given. `end_video_s` and
    /// `end_wall_utc` use [`Annotation::end_or_session_end`]; `end_mach_ns` stays `None` for an
    /// open region.
    ///
    /// # Example
    ///
    /// ```rust
    /// use komorebi_recorder::annotations::fold;
    /// use komorebi_recorder::testkit::{SessionBuilder, mach_ns};
    ///
    /// // a 10 second recording whose video starts at t=0
    /// let (_session, manifest) = SessionBuilder::new("doc-annotation-view").build();
    ///
    /// let text = format!(
    ///     r#"{{"v":1,"id":"a-00000001","op":"add","kind":"bug","start_mach_ns":{}}}"#,
    ///     mach_ns(3.0)
    /// );
    /// let bug = fold(&text).annotations.remove(0);
    ///
    /// let view = bug.view(&manifest);
    /// assert!(view.open_ended && view.needs_description);
    /// assert_eq!(view.start_video_s, 3.0);
    /// // the recording stopped a second after its video ended
    /// assert_eq!(view.end_video_s, 11.0);
    /// assert_eq!(view.end_mach_ns, None);
    /// assert_eq!(view.start_wall_utc.as_deref(), Some("2026-10-26T12:00:08.000000Z"));
    /// ```
    #[must_use]
    pub fn view(&self, manifest: &Manifest) -> AnnotationView {
        let clock = ClockMap::new(manifest.anchors.clone());
        let end_ns = self.end_or_session_end(manifest);
        let wall = |mach_ns: u64| {
            clock
                .mach_to_wall(mach_ns)
                .and_then(timeline::format_wall_us)
        };

        AnnotationView {
            id: self.id.clone(),
            kind: self.kind,
            author: self.author.clone(),
            title: self.title.clone(),
            text: self.text.clone(),
            expected: self.expected.clone(),
            displays: self.displays.clone(),
            start_video_s: manifest.video_seconds(self.start_mach_ns),
            end_video_s: manifest.video_seconds(end_ns),
            start_wall_utc: wall(self.start_mach_ns),
            end_wall_utc: wall(end_ns),
            start_mach_ns: self.start_mach_ns,
            end_mach_ns: self.end_mach_ns,
            open_ended: self.open_ended(),
            needs_description: self.needs_description(),
            about: self.about.clone(),
            refs: self.refs.clone(),
        }
    }
}

/// The annotations of a session and what was wrong with the file.
///
/// The default value is a session with none: no annotations and no warnings. It is also what
/// [`load`] returns when the session has no `annotations.jsonl`.
#[derive(Debug, Default)]
pub struct Loaded {
    /// The annotations that are left after every line is applied, ordered by start and then by id.
    pub annotations: Vec<Annotation>,
    /// One message for each line that [`fold`] skipped, in file order. A caller that builds an
    /// answer from the annotations should show these with it: a skipped line is an annotation that
    /// is missing.
    pub warnings: Vec<String>,
}

impl Loaded {
    /// Returns the annotation with this id, or `None` if there is none (it was never added, or it
    /// was deleted).
    #[must_use]
    pub fn get(&self, id: &str) -> Option<&Annotation> {
        self.annotations
            .iter()
            .find(|annotation| annotation.id == id)
    }

    /// Returns the bug regions, in the order of [`Loaded::annotations`].
    pub fn bugs(&self) -> impl Iterator<Item = &Annotation> {
        self.annotations
            .iter()
            .filter(|annotation| annotation.kind == Kind::Bug)
    }
}

/// Folds the lines of `annotations.jsonl` into annotations, skipping and reporting the lines it
/// cannot understand instead of failing the read.
///
/// The file is written by several programs and by people, and the rest of it is still worth having
/// when a line is damaged.
///
/// Lines are applied in order. A blank line is ignored. A line is skipped, with a message in
/// [`Loaded::warnings`] that names its line number, when it:
///
/// - is not a JSON object of the shape of a [`Line`];
/// - has a version above [`LINE_VERSION`];
/// - is an `add` without a `kind` or without a `start_mach_ns`;
/// - is an `add` for an id that is already there, or was deleted: the first `add` stands, and an id
///   cannot be used again;
/// - is an `edit` or a `delete` for an id that is not there.
///
/// An `edit` sets the fields it carries. It can move an end but cannot clear one, it replaces
/// `displays` and `refs` as a whole, and an empty `expected` clears the expectation. It cannot
/// change the `kind`, the `author` or what the annotation is `about`. It is not checked against
/// the annotation's start, so a hand-written `edit` can put the end before the start (see
/// [`Annotation::end_or_session_end`]); [`add`] and [`edit`] refuse to write one.
///
/// This never fails and does not read a file: it takes the text, so that a caller with the bytes in
/// hand (or a test) does not need a directory. [`load`] reads the file and calls it.
///
/// # Example
///
/// ```rust
/// use komorebi_recorder::annotations::fold;
///
/// let text = [
///     r#"{"v":1,"id":"a-00000001","op":"add","kind":"bug","start_mach_ns":2000000000,"expected":"stays put"}"#,
///     "this is not JSON",
///     r#"{"v":1,"id":"a-00000001","op":"edit","expected":""}"#,
///     r#"{"v":1,"id":"a-00000002","op":"delete"}"#,
/// ]
/// .join("\n");
///
/// let loaded = fold(&text);
///
/// // the damaged line cost nothing but itself
/// assert_eq!(loaded.annotations.len(), 1);
/// assert_eq!(loaded.annotations[0].expected, None, "an empty `expected` clears it");
/// assert_eq!(loaded.warnings.len(), 2);
/// assert!(loaded.warnings[0].starts_with("annotations.jsonl line 2 was skipped: "));
/// assert_eq!(
///     loaded.warnings[1],
///     "annotations.jsonl line 4 deletes a-00000002, which does not exist, and was skipped",
/// );
/// ```
#[must_use]
pub fn fold(text: &str) -> Loaded {
    let mut live: BTreeMap<String, Annotation> = BTreeMap::new();
    let mut deleted: Vec<String> = Vec::new();
    let mut warnings = Vec::new();

    for (index, raw) in text.lines().enumerate() {
        let number = index + 1;
        if raw.trim().is_empty() {
            continue;
        }

        let line: Line = match serde_json::from_str(raw) {
            Ok(line) => line,
            Err(error) => {
                warnings.push(format!(
                    "{ANNOTATIONS_FILE} line {number} was skipped: {error}"
                ));
                continue;
            }
        };

        if line.v > LINE_VERSION {
            warnings.push(format!(
                "{ANNOTATIONS_FILE} line {number} was written by a newer komorebi-recorder (v{}) and was skipped",
                line.v
            ));
            continue;
        }

        match line.op.unwrap_or(Op::Add) {
            Op::Add => {
                let (Some(kind), Some(start)) = (line.kind, line.start_mach_ns) else {
                    warnings.push(format!(
                        "{ANNOTATIONS_FILE} line {number} adds {} without a kind and a start and was skipped",
                        line.id
                    ));
                    continue;
                };

                if live.contains_key(&line.id) || deleted.contains(&line.id) {
                    warnings.push(format!(
                        "{ANNOTATIONS_FILE} line {number} adds {} again and was skipped",
                        line.id
                    ));
                    continue;
                }

                live.insert(
                    line.id.clone(),
                    Annotation {
                        id: line.id,
                        kind,
                        start_mach_ns: start,
                        end_mach_ns: line.end_mach_ns,
                        title: line.title.unwrap_or_default(),
                        text: line.text.unwrap_or_default(),
                        expected: line.expected,
                        displays: line.displays.unwrap_or_default(),
                        author: line.author.unwrap_or_else(|| AUTHOR_USER.to_string()),
                        about: line.about,
                        refs: line.refs.unwrap_or_default(),
                        created_wall_us: line.at_wall_us,
                        edited_wall_us: None,
                    },
                );
            }
            Op::Edit => {
                let Some(annotation) = live.get_mut(&line.id) else {
                    warnings.push(format!(
                        "{ANNOTATIONS_FILE} line {number} edits {}, which does not exist, and was skipped",
                        line.id
                    ));
                    continue;
                };

                if let Some(start) = line.start_mach_ns {
                    annotation.start_mach_ns = start;
                }
                if let Some(end) = line.end_mach_ns {
                    annotation.end_mach_ns = Some(end);
                }
                if let Some(title) = line.title {
                    annotation.title = title;
                }
                if let Some(text) = line.text {
                    annotation.text = text;
                }
                if let Some(expected) = line.expected {
                    annotation.expected = Some(expected).filter(|value| !value.is_empty());
                }
                if let Some(displays) = line.displays {
                    annotation.displays = displays;
                }
                if let Some(refs) = line.refs {
                    annotation.refs = refs;
                }
                annotation.edited_wall_us = line.at_wall_us;
            }
            Op::Delete => {
                if live.remove(&line.id).is_some() {
                    deleted.push(line.id);
                } else {
                    warnings.push(format!(
                        "{ANNOTATIONS_FILE} line {number} deletes {}, which does not exist, and was skipped",
                        line.id
                    ));
                }
            }
        }
    }

    let mut annotations: Vec<Annotation> = live.into_values().collect();
    annotations.sort_by(|a, b| (a.start_mach_ns, &a.id).cmp(&(b.start_mach_ns, &b.id)));
    Loaded {
        annotations,
        warnings,
    }
}

/// Reads a session's annotations from the `annotations.jsonl` in `dir`.
///
/// A session with none has no file, which is not an error: the result is empty. Bytes that are not
/// valid UTF-8 (a crash can leave half a multi-byte character at the end of the file) are replaced,
/// so that the rest of the file is still read. Lines that cannot be understood are reported in
/// [`Loaded::warnings`] (see [`fold`]).
///
/// # Errors
///
/// Returns an error if the file exists but cannot be read, for example because of its permissions
/// or because the path is a directory.
///
/// # Example
///
/// ```rust
/// use komorebi_recorder::annotations::load;
///
/// let dir = std::env::temp_dir().join(format!("komorebi-recorder-doc-load-{}", std::process::id()));
/// std::fs::create_dir_all(&dir)?;
///
/// // nothing was ever annotated
/// let loaded = load(&dir)?;
/// assert!(loaded.annotations.is_empty() && loaded.warnings.is_empty());
/// # std::fs::remove_dir_all(&dir)?;
/// # Ok::<(), Box<dyn std::error::Error>>(())
/// ```
pub fn load(dir: &Path) -> eyre::Result<Loaded> {
    let path = dir.join(ANNOTATIONS_FILE);
    match std::fs::read(&path) {
        // a crash can leave half a multi-byte character at the end; do not lose the rest
        Ok(bytes) => Ok(fold(&String::from_utf8_lossy(&bytes))),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(Loaded::default()),
        Err(error) => Err(error).wrap_err_with(|| format!("could not read {}", path.display())),
    }
}

/// Appends one line to the session's `annotations.jsonl`, atomically with respect to other
/// writers.
///
/// Writers are serialized for the duration so two programs marking at once cannot interleave, and
/// a line a crashed writer left unterminated is closed off first so it cannot swallow ours. The
/// line is written with a single write, so a reader never sees it in pieces. The lock is on
/// [`ANNOTATIONS_LOCK_FILE`] and is released when the call returns, or by the operating system if
/// the process dies first. Readers take no lock.
///
/// [`add`], [`edit`] and [`delete`] are the usual way to write; this is what they call, for a
/// caller that has a [`Line`] of its own.
///
/// # Errors
///
/// Returns an error if the lock file or the annotations file cannot be opened or created, the
/// lock cannot be taken, or reading the last byte of the file, seeking, or writing fails.
///
/// # Example
///
/// A line a crashed writer left unterminated costs only itself:
///
/// ```rust
/// use komorebi_recorder::annotations::{ANNOTATIONS_FILE, Kind, LINE_VERSION, Line, Op, append, load};
///
/// let dir = std::env::temp_dir().join(format!("komorebi-recorder-doc-append-{}", std::process::id()));
/// std::fs::create_dir_all(&dir)?;
/// std::fs::write(dir.join(ANNOTATIONS_FILE), r#"{"v":1,"id":"a-0000"#)?;
///
/// append(&dir, &Line {
///     v: LINE_VERSION,
///     id: "a-00000001".to_string(),
///     op: Some(Op::Add),
///     kind: Some(Kind::Note),
///     start_mach_ns: Some(1_000),
///     ..Line::default()
/// })?;
///
/// let loaded = load(&dir)?;
/// assert_eq!(loaded.annotations.len(), 1);
/// assert_eq!(loaded.warnings.len(), 1, "the torn line is reported");
/// # std::fs::remove_dir_all(&dir)?;
/// # Ok::<(), Box<dyn std::error::Error>>(())
/// ```
///
/// Covered by the tests `concurrent_writers_do_not_interleave_lines` and
/// `a_line_a_crashed_writer_left_unterminated_does_not_swallow_the_next`.
pub fn append(dir: &Path, line: &Line) -> eyre::Result<()> {
    let path = dir.join(ANNOTATIONS_FILE);

    // Serialize writers on a file of their own. Windows locks are mandatory, so locking the
    // annotations file itself would make it unreadable for everyone else while a line is added.
    let lock_path = dir.join(ANNOTATIONS_LOCK_FILE);
    let lock = OpenOptions::new()
        .create(true)
        .truncate(false)
        .write(true)
        .open(&lock_path)
        .wrap_err_with(|| format!("could not open {}", lock_path.display()))?;
    lock.lock()
        .wrap_err_with(|| format!("could not lock {}", lock_path.display()))?;

    let mut file = OpenOptions::new()
        .create(true)
        .read(true)
        .append(true)
        .open(&path)
        .wrap_err_with(|| format!("could not open {}", path.display()))?;

    let length = file.metadata()?.len();
    let mut text = String::new();
    if length > 0 {
        let mut last = [0_u8; 1];
        file.seek(SeekFrom::Start(length - 1))?;
        file.read_exact(&mut last)?;
        if last[0] != b'\n' {
            text.push('\n');
        }
    }

    text.push_str(&serde_json::to_string(line)?);
    text.push('\n');
    // one write, so a reader never sees the line in pieces
    file.write_all(text.as_bytes())?;
    file.flush()?;

    // released when `lock` is dropped, and by the OS if we die first
    Ok(())
}

/// The fields of a new annotation, for [`add`].
///
/// Every field has a default (empty, `None` or zero), so a caller names the ones it has and ends
/// with `..New::default()`. The default is not a usable annotation on its own: a start of zero is
/// the epoch of the master clock, not the start of the recording.
#[derive(Debug, Clone, Default)]
pub struct New {
    /// The kind of annotation. The default, `None`, makes a [`Kind::Note`].
    pub kind: Option<Kind>,
    /// Where it starts, in nanoseconds on the host's master monotonic clock.
    pub start_mach_ns: u64,
    /// Where it ends, on the same clock. `None` makes the region open-ended. [`add`] refuses an end
    /// before the start.
    pub end_mach_ns: Option<u64>,
    /// A short title.
    pub title: String,
    /// The description: what a bug looks like, the remark of a note or the conclusion of a finding.
    pub text: String,
    /// What should have happened instead.
    pub expected: Option<String>,
    /// The displays it was seen on; empty means any.
    pub displays: Vec<u32>,
    /// Who is saying this. The default, `None`, is [`AUTHOR_USER`].
    pub author: Option<String>,
    /// For a finding: the id of the bug it explains.
    pub about: Option<String>,
    /// The evidence of a finding. [`add`] does not check it: see [`validate_refs`].
    pub refs: Vec<Ref>,
}

/// Adds an annotation to the session in `dir` and returns its id.
///
/// The id is `a-` and eight hexadecimal digits, drawn at random. Nothing checks that it is unused:
/// if it matches an id already in the file, [`fold`] skips the new `add` as a repeat and reports
/// it. The time of the write is recorded as [`Annotation::created_wall_us`].
///
/// The session may still be recording: the file is only ever appended to.
///
/// # Errors
///
/// Returns a usage error if `new.end_mach_ns` is before `new.start_mach_ns`, and an I/O error under
/// the conditions of [`append`].
///
/// # Example
///
/// ```rust
/// use komorebi_recorder::annotations::{Kind, New, add, load};
///
/// let dir = std::env::temp_dir().join(format!("komorebi-recorder-doc-add-{}", std::process::id()));
/// std::fs::create_dir_all(&dir)?;
///
/// let id = add(&dir, New {
///     kind: Some(Kind::Bug),
///     start_mach_ns: 2_000_000_000,
///     end_mach_ns: Some(5_000_000_000),
///     text: "tiles overlap after the dock".to_string(),
///     ..New::default()
/// })?;
///
/// let loaded = load(&dir)?;
/// let bug = loaded.get(&id).expect("it was added");
/// assert_eq!(bug.kind, Kind::Bug);
/// assert_eq!(bug.author, "user");
/// assert!(!bug.open_ended() && !bug.needs_description());
///
/// // a region cannot end before it starts
/// let error = add(&dir, New { start_mach_ns: 5, end_mach_ns: Some(2), ..New::default() })
///     .unwrap_err();
/// assert_eq!(error.to_string(), "the end of a region is before its start");
/// # std::fs::remove_dir_all(&dir)?;
/// # Ok::<(), Box<dyn std::error::Error>>(())
/// ```
pub fn add(dir: &Path, new: New) -> eyre::Result<String> {
    if let Some(end) = new.end_mach_ns
        && end < new.start_mach_ns
    {
        return Err(envelope::usage("the end of a region is before its start"));
    }

    let id = ids::new_annotation_id();
    append(
        dir,
        &Line {
            v: LINE_VERSION,
            id: id.clone(),
            op: Some(Op::Add),
            kind: Some(new.kind.unwrap_or(Kind::Note)),
            start_mach_ns: Some(new.start_mach_ns),
            end_mach_ns: new.end_mach_ns,
            title: Some(new.title),
            text: Some(new.text),
            expected: new.expected,
            displays: Some(new.displays),
            author: Some(new.author.unwrap_or_else(|| AUTHOR_USER.to_string())),
            about: new.about,
            refs: Some(new.refs),
            at_wall_us: Some(clock::wall_now_us()),
        },
    )?;

    Ok(id)
}

/// The fields of an annotation an edit may change, for [`edit`]; `None` leaves a field alone.
///
/// The kind, the author and what the annotation is about are not here: they cannot be changed
/// after the annotation is added.
#[derive(Debug, Clone, Default)]
pub struct Patch {
    /// The new start, in nanoseconds on the host's master monotonic clock.
    pub start_mach_ns: Option<u64>,
    /// The new end, on the same clock. There is no way to clear an end once it is set.
    pub end_mach_ns: Option<u64>,
    /// The new title.
    pub title: Option<String>,
    /// The new description.
    pub text: Option<String>,
    /// What should have happened instead. The empty string clears it.
    pub expected: Option<String>,
    /// The displays it was seen on, replacing the list.
    pub displays: Option<Vec<u32>>,
    /// The evidence, replacing the list. [`edit`] does not check it: see [`validate_refs`].
    pub refs: Option<Vec<Ref>>,
}

/// Changes an annotation: appends an `edit` line carrying only the fields `patch` sets.
///
/// The change is checked against the annotation as it reads when the call is made: the start (the
/// patch's, else the annotation's) must not be after the end (the patch's, else the annotation's).
///
/// # Errors
///
/// - a not-found error if `id` is not an annotation of the session (never added, or deleted);
/// - a usage error if the change would put the end before the start;
/// - an error if the annotations cannot be read ([`load`]) or the line cannot be written
///   ([`append`]).
///
/// # Example
///
/// Closing a region that was added without an end:
///
/// ```rust
/// use komorebi_recorder::annotations::{Kind, New, Patch, add, edit, load};
///
/// let dir = std::env::temp_dir().join(format!("komorebi-recorder-doc-edit-{}", std::process::id()));
/// std::fs::create_dir_all(&dir)?;
/// let id = add(&dir, New { kind: Some(Kind::Bug), start_mach_ns: 2_000, ..New::default() })?;
/// assert!(load(&dir)?.get(&id).expect("added").open_ended());
///
/// edit(&dir, &id, Patch {
///     end_mach_ns: Some(9_000),
///     text: Some("tiles overlap after the dock".to_string()),
///     ..Patch::default()
/// })?;
/// let loaded = load(&dir)?;
/// let bug = loaded.get(&id).expect("still there");
/// assert_eq!(bug.end_mach_ns, Some(9_000));
/// assert!(bug.edited_wall_us.is_some());
///
/// // the end is checked against the start the annotation already has
/// let error = edit(&dir, &id, Patch { end_mach_ns: Some(1_000), ..Patch::default() })
///     .unwrap_err();
/// assert_eq!(error.to_string(), "the end of a region is before its start");
/// # std::fs::remove_dir_all(&dir)?;
/// # Ok::<(), Box<dyn std::error::Error>>(())
/// ```
pub fn edit(dir: &Path, id: &str, patch: Patch) -> eyre::Result<()> {
    let loaded = load(dir)?;
    let current = loaded
        .get(id)
        .ok_or_else(|| envelope::not_found(format!("there is no annotation {id}")))?;

    let start = patch.start_mach_ns.unwrap_or(current.start_mach_ns);
    if let Some(end) = patch.end_mach_ns.or(current.end_mach_ns)
        && end < start
    {
        return Err(envelope::usage("the end of a region is before its start"));
    }

    append(
        dir,
        &Line {
            v: LINE_VERSION,
            id: id.to_string(),
            op: Some(Op::Edit),
            start_mach_ns: patch.start_mach_ns,
            end_mach_ns: patch.end_mach_ns,
            title: patch.title,
            text: patch.text,
            expected: patch.expected,
            displays: patch.displays,
            refs: patch.refs,
            at_wall_us: Some(clock::wall_now_us()),
            ..Line::default()
        },
    )
}

/// Removes an annotation: appends a `delete` line, a tombstone. The earlier lines stay in the file,
/// and the id cannot be added again.
///
/// # Errors
///
/// Returns a not-found error if `id` is not an annotation of the session (never added, or already
/// deleted), and an error under the conditions of [`load`] and [`append`].
///
/// # Example
///
/// ```rust
/// use komorebi_recorder::annotations::{ANNOTATIONS_FILE, New, add, delete, load};
///
/// let dir = std::env::temp_dir().join(format!("komorebi-recorder-doc-delete-{}", std::process::id()));
/// std::fs::create_dir_all(&dir)?;
/// let id = add(&dir, New { start_mach_ns: 1_000, ..New::default() })?;
///
/// delete(&dir, &id)?;
/// assert!(load(&dir)?.get(&id).is_none());
/// // nothing was rewritten: the `add` and the tombstone are both in the file
/// assert_eq!(std::fs::read_to_string(dir.join(ANNOTATIONS_FILE))?.lines().count(), 2);
/// // and it cannot be deleted twice
/// assert!(delete(&dir, &id).is_err());
/// # std::fs::remove_dir_all(&dir)?;
/// # Ok::<(), Box<dyn std::error::Error>>(())
/// ```
pub fn delete(dir: &Path, id: &str) -> eyre::Result<()> {
    if load(dir)?.get(id).is_none() {
        return Err(envelope::not_found(format!("there is no annotation {id}")));
    }

    append(
        dir,
        &Line {
            v: LINE_VERSION,
            id: id.to_string(),
            op: Some(Op::Delete),
            at_wall_us: Some(clock::wall_now_us()),
            ..Line::default()
        },
    )
}

/// Returns where to find the source of `flavor`'s komorebi to check [`Ref::Code`] against, if it
/// can be found.
///
/// The candidates, in order, are:
///
/// 1. the flavor's own environment variable: `KOMOREBI_MAC_SRC` for komorebi for Mac,
///    `KOMOREBI_WINDOWS_SRC` for komorebi for Windows;
/// 2. `KOMOREBI_SOURCE`, which serves either flavor;
/// 3. the checkout this recorder was built against, if it was built with one for this flavor (the
///    `KOMOREBI_MAC_ROOT` or `KOMOREBI_WINDOWS_ROOT` that its build script sets) and that
///    directory is still on disk.
///
/// The first candidate that has a `komorebi/src` directory is returned. A candidate that does not
/// have one is skipped, not reported, so a variable that is set but wrong falls through to the
/// next. `None` means no candidate qualified; [`validate_refs`] then cannot check [`Ref::Code`]
/// and reports it in a warning.
///
/// The `--komorebi-src` option of the commands that take one overrides this.
///
/// This reads the environment, so there is no example: its behavior depends on the machine it
/// runs on.
#[must_use]
pub fn default_code_root(flavor: Flavor) -> Option<PathBuf> {
    let (variable, built_from) = match flavor {
        Flavor::Macos => (
            "KOMOREBI_MAC_SRC",
            option_env!("KOMOREBI_MAC_ROOT").map(PathBuf::from),
        ),
        Flavor::Windows => (
            "KOMOREBI_WINDOWS_SRC",
            option_env!("KOMOREBI_WINDOWS_ROOT").map(PathBuf::from),
        ),
    };

    [variable, "KOMOREBI_SOURCE"]
        .into_iter()
        .filter_map(std::env::var_os)
        .map(PathBuf::from)
        .chain(built_from)
        .find(|root| root.join("komorebi/src").is_dir())
}

fn count_lines(path: &Path) -> std::io::Result<u64> {
    let bytes = std::fs::read(path)?;
    Ok(bytes.split(|byte| *byte == b'\n').count() as u64 - u64::from(bytes.last() == Some(&b'\n')))
}

/// Checks that every piece of evidence exists, and returns warnings for what could not be checked.
///
/// The event was recorded, the log line is in the file, the video time is inside the recording,
/// the source line is in komorebi's code. A reference that is definitely wrong is an error, so an
/// agent cannot cite evidence that is not there.
///
/// | [`Ref`] | Checked against | A reference is wrong if |
/// |---|---|---|
/// | [`Ref::Event`] | `events.jsonl` of the session | no event has that sequence number, or the session recorded no events |
/// | [`Ref::Log`] | the named file of the session, found through the session path guard | the path leaves the session or is not a plain relative path, the file is absent, or the line is `0` or past the last |
/// | [`Ref::Frame`] | the span of video time in `manifest` | the time is negative or after the end of the video |
/// | [`Ref::Code`] | the file under `code_root` | the path is not a plain relative path, the file is absent, or the line is `0` or past the last |
///
/// Only [`Ref::Code`] can be left unchecked: with no `code_root` the path is still checked for
/// being a relative path, the reference is skipped and a warning reports it. This function returns on
/// the first wrong reference; later ones are not looked at. It does not write anything.
///
/// # Errors
///
/// Returns the first wrong reference of `refs` as an error: a not-found error for a missing
/// event, file, line, source file or video time (the messages say what was looked for and how many
/// lines or seconds there are), and a usage error for a `file` or `path` that is not a plain
/// relative path (absolute, containing `..`, or leading outside the session). Also returns an error
/// if `session` is not a session directory.
///
/// # Example
///
/// ```rust
/// use komorebi_recorder::annotations::{Ref, validate_refs};
/// use komorebi_recorder::testkit::SessionBuilder;
/// use serde_json::json;
///
/// // two seconds in, komorebi sent one event and logged one line
/// let (session, manifest) = SessionBuilder::new("doc-validate-refs")
///     .event(2.0, json!({"type": "Retile"}), None)
///     .log(2.0, "INFO", "komorebi::process_command: retiling")
///     .build();
///
/// let there = [
///     Ref::Event { seq: 1 },
///     Ref::Log { file: "komorebi.log".to_string(), line: 1 },
///     Ref::Frame { video_s: 4.0 },
/// ];
/// assert!(validate_refs(&session, &manifest, &there, None)?.is_empty());
///
/// // evidence that is not there is an error, so a finding cannot cite it
/// let error = validate_refs(&session, &manifest, &[Ref::Event { seq: 2 }], None).unwrap_err();
/// assert_eq!(error.to_string(), "there is no event #2");
///
/// // source that cannot be checked is a warning, not a pass
/// let unchecked = Ref::Code { path: "komorebi/src/window.rs".to_string(), line: 210 };
/// let warnings = validate_refs(&session, &manifest, &[unchecked], None)?;
/// assert_eq!(warnings.len(), 1);
/// # Ok::<(), Box<dyn std::error::Error>>(())
/// ```
///
/// Covered by the tests `evidence_that_exists_is_accepted`, `evidence_that_is_not_there_is_refused`
/// and `source_that_cannot_be_checked_is_a_warning_not_a_pass`, which also cover the source file
/// and the escapes.
pub fn validate_refs(
    session: &Session,
    manifest: &Manifest,
    refs: &[Ref],
    code_root: Option<&Path>,
) -> eyre::Result<Vec<String>> {
    let mut warnings = Vec::new();
    let guard = SessionPath::new(&session.dir)?;

    for reference in refs {
        match reference {
            Ref::Event { seq } => {
                let text = std::fs::read_to_string(session.path("events.jsonl"))
                    .map_err(|_| envelope::not_found("the session recorded no events"))?;
                #[derive(Deserialize)]
                struct Seq {
                    seq: u64,
                }
                let found = text
                    .lines()
                    .filter_map(|line| serde_json::from_str::<Seq>(line).ok())
                    .any(|row| row.seq == *seq);
                if !found {
                    return Err(envelope::not_found(format!("there is no event #{seq}")));
                }
            }
            Ref::Log { file, line } => {
                let path = guard.resolve(file)?;
                let lines = count_lines(&path)
                    .map_err(|_| envelope::not_found(format!("the session has no {file}")))?;
                if *line == 0 || *line > lines {
                    return Err(envelope::not_found(format!(
                        "{file} has {lines} lines; there is no line {line}"
                    )));
                }
            }
            Ref::Frame { video_s } => {
                let (_, end) = manifest.video_span();
                if *video_s < 0.0 || *video_s > end {
                    return Err(envelope::not_found(format!(
                        "video time {} is outside the recording (0 to {})",
                        crate::timecode::format(*video_s),
                        crate::timecode::format(end)
                    )));
                }
            }
            Ref::Code { path, line } => {
                if !is_plain_relative(path) {
                    return Err(envelope::usage(format!(
                        "'{path}' must be a path relative to the komorebi repository"
                    )));
                }

                let Some(root) = code_root else {
                    warnings.push(format!(
                        "code:{path}:{line} could not be checked: komorebi's source was not found \
                         (set KOMOREBI_SOURCE to the repository)"
                    ));
                    continue;
                };

                let lines = count_lines(&root.join(path)).map_err(|_| {
                    envelope::not_found(format!("{} has no file {path}", root.display()))
                })?;
                if *line == 0 || *line > lines {
                    return Err(envelope::not_found(format!(
                        "{path} has {lines} lines; there is no line {line}"
                    )));
                }
            }
        }
    }

    Ok(warnings)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::clock::Anchor;
    use crate::session::Moment;
    use chrono::Utc;

    fn temp(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "komorebi-recorder-annotations-{name}-{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("dir");
        dir
    }

    fn bug(start: u64, end: Option<u64>, text: &str) -> New {
        New {
            kind: Some(Kind::Bug),
            start_mach_ns: start,
            end_mach_ns: end,
            text: text.to_string(),
            ..New::default()
        }
    }

    #[test]
    fn a_region_has_a_start_an_end_and_a_description() {
        let dir = temp("region");
        let id = add(
            &dir,
            bug(2_000, Some(5_000), "the window jumps to the wrong monitor"),
        )
        .expect("add");

        let loaded = load(&dir).expect("load");
        let annotation = loaded.get(&id).expect("annotation");
        assert_eq!(annotation.kind, Kind::Bug);
        assert_eq!(annotation.start_mach_ns, 2_000);
        assert_eq!(annotation.end_mach_ns, Some(5_000));
        assert_eq!(annotation.text, "the window jumps to the wrong monitor");
        assert!(!annotation.open_ended());
        assert!(!annotation.needs_description());
        assert_eq!(annotation.author, AUTHOR_USER);
        assert!(loaded.warnings.is_empty());
    }

    #[test]
    fn a_region_can_be_opened_now_and_closed_later() {
        let dir = temp("open-close");
        let id = add(&dir, bug(2_000, None, "")).expect("start");

        let open = load(&dir).expect("load");
        assert!(open.get(&id).expect("open").open_ended());
        assert!(open.get(&id).expect("open").needs_description());

        edit(
            &dir,
            &id,
            Patch {
                end_mach_ns: Some(9_000),
                text: Some("tiles overlap after the dock".to_string()),
                ..Patch::default()
            },
        )
        .expect("close");

        let closed = load(&dir).expect("load");
        let annotation = closed.get(&id).expect("closed");
        assert_eq!(annotation.end_mach_ns, Some(9_000));
        assert_eq!(annotation.text, "tiles overlap after the dock");
        assert!(annotation.edited_wall_us.is_some());
        assert!(!annotation.needs_description());
    }

    #[test]
    fn an_end_before_the_start_is_refused() {
        let dir = temp("backwards");
        assert!(add(&dir, bug(5_000, Some(2_000), "x")).is_err());

        let id = add(&dir, bug(5_000, None, "x")).expect("add");
        let error = edit(
            &dir,
            &id,
            Patch {
                end_mach_ns: Some(1_000),
                ..Patch::default()
            },
        )
        .expect_err("backwards");
        assert_eq!(envelope::exit_kind(&error), envelope::ExitKind::Usage);
    }

    #[test]
    fn deleting_leaves_a_tombstone_and_hides_the_annotation() {
        let dir = temp("delete");
        let keep = add(&dir, bug(1_000, Some(2_000), "keep")).expect("keep");
        let drop = add(&dir, bug(3_000, Some(4_000), "drop")).expect("drop");

        delete(&dir, &drop).expect("delete");

        let loaded = load(&dir).expect("load");
        assert_eq!(loaded.annotations.len(), 1);
        assert!(loaded.get(&keep).is_some());
        // append-only: three lines, nothing rewritten
        let text = std::fs::read_to_string(dir.join(ANNOTATIONS_FILE)).expect("file");
        assert_eq!(text.lines().count(), 3);
        // it cannot be deleted or edited twice
        assert!(delete(&dir, &drop).is_err());
        assert!(edit(&dir, &drop, Patch::default()).is_err());
    }

    #[test]
    fn annotations_are_ordered_by_start() {
        let dir = temp("order");
        add(&dir, bug(9_000, None, "later")).expect("later");
        add(&dir, bug(1_000, None, "earlier")).expect("earlier");

        let starts: Vec<u64> = load(&dir)
            .expect("load")
            .annotations
            .iter()
            .map(|annotation| annotation.start_mach_ns)
            .collect();
        assert_eq!(starts, [1_000, 9_000]);
    }

    #[test]
    fn a_missing_file_is_an_empty_session_not_an_error() {
        let loaded = load(&temp("missing")).expect("load");
        assert!(loaded.annotations.is_empty());
        assert!(loaded.warnings.is_empty());
    }

    #[test]
    fn damaged_lines_are_skipped_and_reported_without_losing_the_rest() {
        let good = |id: &str, start: u64| {
            serde_json::to_string(&Line {
                v: LINE_VERSION,
                id: id.to_string(),
                op: Some(Op::Add),
                kind: Some(Kind::Bug),
                start_mach_ns: Some(start),
                ..Line::default()
            })
            .expect("line")
        };
        let text = format!(
            "{}\nthis is not json\n{{\"v\":1,\"id\":\"a-00000009\",\"op\":\"edit\",\"title\":\"x\"}}\n\
             {{\"v\":99,\"id\":\"a-0000000a\",\"op\":\"add\",\"kind\":\"bug\",\"start_mach_ns\":1}}\n\
             {{\"v\":1,\"id\":\"a-0000000b\",\"op\":\"add\"}}\n{}\n{}",
            good("a-00000001", 1),
            good("a-00000001", 2),
            good("a-00000002", 3),
        );

        let loaded = fold(&text);
        let ids: Vec<&str> = loaded.annotations.iter().map(|a| a.id.as_str()).collect();
        assert_eq!(ids, ["a-00000001", "a-00000002"]);
        // the duplicate add did not overwrite the first
        assert_eq!(loaded.annotations[0].start_mach_ns, 1);
        assert_eq!(loaded.warnings.len(), 5, "{:#?}", loaded.warnings);
    }

    #[test]
    fn a_line_a_crashed_writer_left_unterminated_does_not_swallow_the_next() {
        let dir = temp("torn");
        std::fs::write(dir.join(ANNOTATIONS_FILE), "{\"v\":1,\"id\":\"a-0000").expect("torn");

        let id = add(&dir, bug(1_000, None, "after the crash")).expect("add");
        let loaded = load(&dir).expect("load");

        assert_eq!(loaded.annotations.len(), 1);
        assert_eq!(loaded.annotations[0].id, id);
        assert_eq!(loaded.warnings.len(), 1, "the torn line is reported");
    }

    #[test]
    fn concurrent_writers_do_not_interleave_lines() {
        let dir = temp("concurrent");
        let threads: Vec<_> = (0..8)
            .map(|worker| {
                let dir = dir.clone();
                std::thread::spawn(move || {
                    for index in 0..25 {
                        add(&dir, bug(worker * 1000 + index, None, &"x".repeat(2000)))
                            .expect("add");
                    }
                })
            })
            .collect();
        for thread in threads {
            thread.join().expect("join");
        }

        let loaded = load(&dir).expect("load");
        assert_eq!(loaded.annotations.len(), 200);
        assert!(loaded.warnings.is_empty(), "{:#?}", loaded.warnings);
    }

    fn manifest() -> Manifest {
        let mut manifest = Manifest::new(
            Moment {
                mach_ns: 1_000_000_000,
                wall_us: 1_000_000,
            },
            vec![Anchor {
                mach_ns: 1_000_000_000,
                wall_us: 1_000_000,
            }],
        );
        manifest.video_t0_mach_ns = Some(1_000_000_000);
        manifest.stop = Some(Moment {
            mach_ns: 11_000_000_000,
            wall_us: 11_000_000,
        });
        manifest
    }

    #[test]
    fn the_view_has_both_clocks_and_closes_an_open_region_at_the_end() {
        let manifest = manifest();
        let annotation = Annotation {
            id: "a-00000001".to_string(),
            kind: Kind::Bug,
            start_mach_ns: 3_000_000_000,
            end_mach_ns: None,
            title: String::new(),
            text: String::new(),
            expected: None,
            displays: vec![],
            author: AUTHOR_USER.to_string(),
            about: None,
            refs: vec![],
            created_wall_us: None,
            edited_wall_us: None,
        };

        let view = annotation.view(&manifest);
        assert!((view.start_video_s - 2.0).abs() < 1e-9);
        assert!((view.end_video_s - 10.0).abs() < 1e-9);
        assert!(view.open_ended);
        assert!(view.needs_description);
        assert_eq!(
            view.start_wall_utc.as_deref(),
            Some("1970-01-01T00:00:03.000000Z")
        );
    }

    #[test]
    fn references_parse_and_print() {
        for (text, expected) in [
            ("event:12", Ref::Event { seq: 12 }),
            (
                "log:345",
                Ref::Log {
                    file: DEFAULT_LOG_FILE.to_string(),
                    line: 345,
                },
            ),
            (
                "log:oslog/komorebi.ndjson:77",
                Ref::Log {
                    file: "oslog/komorebi.ndjson".to_string(),
                    line: 77,
                },
            ),
            (
                "code:komorebi/src/window.rs:210",
                Ref::Code {
                    path: "komorebi/src/window.rs".to_string(),
                    line: 210,
                },
            ),
            ("frame:12.5", Ref::Frame { video_s: 12.5 }),
        ] {
            assert_eq!(text.parse::<Ref>().expect(text), expected);
        }

        assert_eq!(
            "log:oslog/komorebi.ndjson:77"
                .parse::<Ref>()
                .expect("ref")
                .to_string(),
            "log:oslog/komorebi.ndjson:77"
        );
        for bad in ["event:x", "code:no-line", "nonsense", "frame:", "cat:1"] {
            assert!(bad.parse::<Ref>().is_err(), "{bad}");
        }
    }

    fn session_with_evidence(name: &str) -> (Session, Manifest) {
        let dir = temp(name);
        let session = Session::create(Some(dir.clone()), Utc::now()).expect("create");
        let manifest = manifest();
        session.write_manifest(&manifest).expect("manifest");
        std::fs::write(dir.join("komorebi.log"), "one\ntwo\nthree\n").expect("log");
        std::fs::write(
            dir.join("events.jsonl"),
            "{\"seq\":1,\"recv_wall_us\":0,\"recv_mach_ns\":0}\n{\"seq\":2,\"recv_wall_us\":0,\"recv_mach_ns\":0}\n",
        )
        .expect("events");
        (session, manifest)
    }

    #[test]
    fn evidence_that_exists_is_accepted() {
        let (session, manifest) = session_with_evidence("evidence-ok");
        let root = temp("code-root");
        std::fs::create_dir_all(root.join("komorebi/src")).expect("src");
        std::fs::write(root.join("komorebi/src/window.rs"), "a\nb\n").expect("file");

        let warnings = validate_refs(
            &session,
            &manifest,
            &[
                Ref::Event { seq: 2 },
                Ref::Log {
                    file: "komorebi.log".to_string(),
                    line: 3,
                },
                Ref::Frame { video_s: 4.0 },
                Ref::Code {
                    path: "komorebi/src/window.rs".to_string(),
                    line: 2,
                },
            ],
            Some(&root),
        )
        .expect("valid");
        assert!(warnings.is_empty());
    }

    #[test]
    fn evidence_that_is_not_there_is_refused() {
        let (session, manifest) = session_with_evidence("evidence-bad");
        let root = temp("code-root-bad");
        std::fs::create_dir_all(root.join("komorebi/src")).expect("src");
        std::fs::write(root.join("komorebi/src/window.rs"), "a\nb\n").expect("file");

        for reference in [
            Ref::Event { seq: 3 },
            Ref::Log {
                file: "komorebi.log".to_string(),
                line: 4,
            },
            Ref::Log {
                file: "../escape".to_string(),
                line: 1,
            },
            Ref::Log {
                file: "komorebi.log".to_string(),
                line: 0,
            },
            Ref::Frame { video_s: 99.0 },
            Ref::Code {
                path: "komorebi/src/window.rs".to_string(),
                line: 3,
            },
            Ref::Code {
                path: "komorebi/src/missing.rs".to_string(),
                line: 1,
            },
            Ref::Code {
                path: "../x.rs".to_string(),
                line: 1,
            },
        ] {
            assert!(
                validate_refs(
                    &session,
                    &manifest,
                    std::slice::from_ref(&reference),
                    Some(&root)
                )
                .is_err(),
                "{reference} should be refused"
            );
        }
    }

    #[test]
    fn source_that_cannot_be_checked_is_a_warning_not_a_pass() {
        let (session, manifest) = session_with_evidence("evidence-unchecked");
        let warnings = validate_refs(
            &session,
            &manifest,
            &[Ref::Code {
                path: "komorebi/src/window.rs".to_string(),
                line: 2,
            }],
            None,
        )
        .expect("not an error");
        assert_eq!(warnings.len(), 1);
        assert!(warnings[0].contains("KOMOREBI_SOURCE"));
    }
}
