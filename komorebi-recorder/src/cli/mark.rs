//! `mark`: dropping a named marker onto a recording's timeline while it is being recorded.
//!
//! A script that drives komorebi (or anything else on the machine) can say where it is:
//! `komorebi-recorder mark "step 3: move window right"`. The marker is a note, written to the
//! session's `annotations.jsonl` at the instant the command runs, so it shows up in `annotations`,
//! `brief`, `search`, `overview` and the viewer like any other note, with the author `script`.
//!
//! The note is a point in time: its start and end are the same nanosecond of the master clock
//! ([`crate::clock::mono_now_ns`], which every process on the machine agrees on). It does not go
//! through `cli::load`, which refuses a session that is still recording, and it does not take
//! the recorder's lock: [`crate::annotations::add`] only appends one line, which is safe beside the
//! recorder and beside other marks.
//!
//! # Which session
//!
//! In this order ([`resolve_session`]):
//!
//! 1. `--session DIR`;
//! 2. the environment variable [`SESSION_ENV`] (`record --until-exit` sets it for the command it
//!    runs);
//! 3. the newest session under komorebi's `recordings/` directory whose recorder is running.
//!
//! A session no recorder is working on is refused: a finished recording has no "now" to mark, and
//! `annotate <session> add` is how to put a note at a time in it. Covered by `tests/mark.rs`.

use super::print_json;
use crate::analysis::envelope;
use crate::analysis::envelope::Envelope;
use crate::annotations;
use crate::annotations::Kind;
use crate::annotations::New;
use crate::clock;
use crate::lock;
use crate::lock::Recorder;
use crate::session::Session;
use crate::timecode;
use crate::timeline;
use clap::Args;
use color_eyre::eyre;
use schemars::JsonSchema;
use serde::Serialize;
use std::path::Path;
use std::path::PathBuf;

/// The environment variable that names the session to mark: `KOMOREBI_RECORDER_SESSION`, which
/// `record --until-exit` sets for its command.
pub use super::until_exit::SESSION_ENV;

/// The author a marker is written under.
pub const AUTHOR_SCRIPT: &str = "script";

/// The `schema` of the output of `mark` with `--json` ([`MarkData`]).
pub const SCHEMA_MARK: &str = "mark/1";

/// The arguments of `mark`.
#[derive(Args, Debug)]
pub struct MarkArgs {
    /// What to call this moment, e.g. "step 3: move window right"
    pub text: String,
    /// The session being recorded [default: `$KOMOREBI_RECORDER_SESSION`, else the newest recording
    /// in komorebi's data directory that is still running]
    #[arg(long, value_name = "DIR")]
    pub session: Option<PathBuf>,
    /// The kind of annotation to drop: `note`, or `bug` for a point where the bug is seen
    #[arg(long, value_enum, default_value = "note")]
    pub kind: MarkKind,
    /// Machine-readable JSON output
    #[arg(long)]
    pub json: bool,
}

/// The kinds of annotation `mark` writes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, clap::ValueEnum)]
pub enum MarkKind {
    /// A remark about a moment
    Note,
    /// A point where the bug is seen
    Bug,
}

/// What `mark` answers with `--json`.
#[derive(Debug, Serialize, JsonSchema)]
pub struct MarkData {
    /// The id of the annotation written, such as `a-3f9c1e02`.
    pub id: String,
    /// The kind of annotation: `note` unless `--kind` said otherwise.
    pub kind: String,
    /// The text of the marker.
    pub text: String,
    /// The session directory it was written to.
    pub session: String,
    /// The moment of the marker on the master clock, in nanoseconds.
    pub at_mach_ns: u64,
    /// The moment as a UTC time in RFC 3339 form with microseconds.
    pub at_wall_utc: Option<String>,
    /// The moment in seconds of video time. `None` until the recording has captured its first
    /// frame, where video time starts.
    pub video_s: Option<f64>,
}

/// Decides which session a mark goes to.
///
/// `explicit` is `--session`, `env` the value of [`SESSION_ENV`] (empty counts as unset) and
/// `recordings` the directory to search for the newest running recording, which is the
/// name-ordered last directory in it whose recorder holds its lock (recordings are named for the
/// UTC time they began).
///
/// # Errors
///
/// - a not-found error if a named directory does not exist, or if nothing was named and no recording
///   in `recordings` is running; the message gives the three ways to name a session;
/// - a usage error if the session named is not being recorded: a finished recording has no "now" to
///   mark.
///
/// # Example
///
/// ```rust
/// use komorebi_recorder::cli::mark::resolve_session;
/// use komorebi_recorder::lock::RecorderLock;
///
/// let root = std::env::temp_dir().join(format!("komorebi-recorder-doc-resolve-{}", std::process::id()));
/// let (old, new) = (root.join("20260101T000000Z"), root.join("20260102T000000Z"));
/// std::fs::create_dir_all(&old)?;
/// std::fs::create_dir_all(&new)?;
/// let _old = RecorderLock::acquire(&old)?;
/// let _new = RecorderLock::acquire(&new)?;
///
/// // nothing named: the newest recording that is running
/// assert_eq!(resolve_session(None, None, &root)?, new);
/// // the environment beats the search, and --session beats the environment
/// assert_eq!(resolve_session(None, Some(old.clone().into()), &root)?, old);
/// assert_eq!(resolve_session(Some(&new), Some(old.clone().into()), &root)?, new);
/// # std::fs::remove_dir_all(&root)?;
/// # Ok::<(), Box<dyn std::error::Error>>(())
/// ```
pub fn resolve_session(
    explicit: Option<&Path>,
    env: Option<std::ffi::OsString>,
    recordings: &Path,
) -> eyre::Result<PathBuf> {
    let named = explicit
        .map(Path::to_path_buf)
        .or_else(|| env.filter(|value| !value.is_empty()).map(PathBuf::from));

    if let Some(dir) = named {
        if !dir.is_dir() {
            return Err(envelope::not_found(format!(
                "{} is not a session directory",
                dir.display()
            )));
        }
        return match lock::recorder(&dir) {
            Recorder::Running(_) => Ok(dir),
            Recorder::NotRunning | Recorder::Unknown => Err(envelope::with_hint(
                envelope::usage(format!(
                    "{} is not being recorded, and a finished recording has no \"now\" to mark",
                    dir.display()
                )),
                format!(
                    "put a note at a time in it with `komorebi-recorder annotate {} add --kind note \
                     --start <t> --text <text>`",
                    dir.display()
                ),
            )),
        };
    }

    let mut candidates: Vec<PathBuf> = std::fs::read_dir(recordings)
        .into_iter()
        .flatten()
        .flatten()
        .map(|entry| entry.path())
        .filter(|path| path.is_dir())
        .collect();
    candidates.sort();
    candidates
        .into_iter()
        .rev()
        .find(|dir| matches!(lock::recorder(dir), Recorder::Running(_)))
        .ok_or_else(|| {
            envelope::not_found(format!(
                "no recording to mark: name the session with --session <dir>, or with the \
                 environment variable {SESSION_ENV} (`record --until-exit` sets it for its command), \
                 or start a recording; none under {} is running",
                recordings.display()
            ))
        })
}

/// Writes the marker and returns what was written.
///
/// # Errors
///
/// Returns a usage error if the text is blank, and whatever [`annotations::add`] fails with.
pub fn write_mark(dir: &Path, text: &str, kind: MarkKind) -> eyre::Result<MarkData> {
    if text.trim().is_empty() {
        return Err(envelope::usage("a marker needs some text"));
    }

    let at = clock::mono_now_ns();
    let at_wall_us = clock::wall_now_us();
    let kind = match kind {
        MarkKind::Note => Kind::Note,
        MarkKind::Bug => Kind::Bug,
    };
    let id = annotations::add(
        dir,
        New {
            kind: Some(kind),
            // a point in time: an annotation with no end would run to the end of the recording
            start_mach_ns: at,
            end_mach_ns: Some(at),
            text: text.to_string(),
            author: Some(AUTHOR_SCRIPT.to_string()),
            ..New::default()
        },
    )?;

    // video time starts at the first frame, which the manifest records once there is one; before
    // that the wall time is all there is
    let video_s = Session::open(dir)
        .and_then(|session| session.manifest())
        .ok()
        .filter(|manifest| manifest.video_t0_mach_ns.is_some())
        .map(|manifest| manifest.video_seconds(at));

    Ok(MarkData {
        id,
        kind: kind.to_string(),
        text: text.to_string(),
        session: dir.display().to_string(),
        at_mach_ns: at,
        at_wall_utc: timeline::format_wall_us(at_wall_us),
        video_s,
    })
}

pub(super) fn mark(args: &MarkArgs) -> eyre::Result<()> {
    let dir = resolve_session(
        args.session.as_deref(),
        std::env::var_os(SESSION_ENV),
        &crate::session::default_recordings_dir(),
    )?;
    let marked = write_mark(&dir, &args.text, args.kind)?;

    if args.json {
        return print_json(&Envelope::ok(SCHEMA_MARK, marked));
    }

    let when = match (marked.video_s, &marked.at_wall_utc) {
        (Some(video_s), _) => timecode::format(video_s),
        // 2026-10-26T12:00:08.123456Z -> 12:00:08.12Z
        (None, Some(wall)) => format!("{}Z", wall.get(11..22).unwrap_or(wall)),
        (None, None) => "now".to_string(),
    };
    println!("marked {when} ({}): {}", marked.id, marked.text);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::lock::RecorderLock;

    fn scratch(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "komorebi-recorder-mark-{name}-{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("create");
        dir
    }

    fn running(root: &Path, name: &str) -> (PathBuf, RecorderLock) {
        let dir = root.join(name);
        std::fs::create_dir_all(&dir).expect("create");
        let lock = RecorderLock::acquire(&dir).expect("lock");
        (dir, lock)
    }

    #[test]
    fn the_newest_running_recording_is_the_default() {
        let root = scratch("newest");
        let (_old, _a) = running(&root, "20260101T000000Z");
        let (new, _b) = running(&root, "20260102T000000Z");
        // newer still, but its recorder is gone
        let finished = root.join("20260103T000000Z");
        std::fs::create_dir_all(&finished).expect("create");
        drop(RecorderLock::acquire(&finished).expect("lock"));

        assert_eq!(resolve_session(None, None, &root).expect("found"), new);
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn the_environment_beats_the_search_and_the_flag_beats_the_environment() {
        let root = scratch("order");
        let (old, _a) = running(&root, "20260101T000000Z");
        let (new, _b) = running(&root, "20260102T000000Z");

        assert_eq!(
            resolve_session(None, Some(old.clone().into()), &root).expect("env"),
            old
        );
        assert_eq!(
            resolve_session(Some(&new), Some(old.into()), &root).expect("flag"),
            new
        );
        // an empty variable is not set
        assert_eq!(
            resolve_session(None, Some(String::new().into()), &root).expect("empty"),
            new
        );
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn nothing_running_says_how_to_name_a_session() {
        let root = scratch("none");
        let error = resolve_session(None, None, &root).unwrap_err();
        assert_eq!(
            envelope::exit_kind(&error),
            envelope::ExitKind::NotFound,
            "{error:?}"
        );
        let message = format!("{error}");
        assert!(message.contains("--session") && message.contains(SESSION_ENV));

        // not even a recordings directory
        let error = resolve_session(None, None, &root.join("missing")).unwrap_err();
        assert_eq!(envelope::exit_kind(&error), envelope::ExitKind::NotFound);
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn a_finished_session_is_refused_with_a_pointer_to_annotate() {
        let root = scratch("finished");
        let finished = root.join("done");
        std::fs::create_dir_all(&finished).expect("create");
        drop(RecorderLock::acquire(&finished).expect("lock"));

        let error = resolve_session(Some(&finished), None, &root).unwrap_err();
        assert_eq!(envelope::exit_kind(&error), envelope::ExitKind::Usage);
        assert!(format!("{error:?}").contains("annotate"), "{error:?}");

        // no lock file at all is no recorder either
        let bare = root.join("bare");
        std::fs::create_dir_all(&bare).expect("create");
        assert!(resolve_session(Some(&bare), None, &root).is_err());
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn a_marker_is_one_point_note_by_the_script() {
        let root = scratch("line");
        let (dir, _lock) = running(&root, "now");

        let marked = write_mark(&dir, "step 3: move window right", MarkKind::Note).expect("mark");
        let text = std::fs::read_to_string(dir.join(annotations::ANNOTATIONS_FILE)).expect("read");
        assert_eq!(text.lines().count(), 1);
        assert!(text.ends_with('\n'));

        let line: serde_json::Value = serde_json::from_str(text.trim()).expect("json");
        assert_eq!(line["v"], 1);
        assert_eq!(line["id"], marked.id.as_str());
        assert_eq!(line["op"], "add");
        assert_eq!(line["kind"], "note");
        assert_eq!(line["author"], "script");
        assert_eq!(line["title"], "");
        assert_eq!(line["text"], "step 3: move window right");
        assert_eq!(line["start_mach_ns"], line["end_mach_ns"]);
        assert_eq!(line["start_mach_ns"].as_u64(), Some(marked.at_mach_ns));

        let loaded = annotations::load(&dir).expect("load");
        let note = loaded.get(&marked.id).expect("read back");
        assert!(!note.open_ended());
        // no manifest here, so only the wall time is known
        assert_eq!(marked.video_s, None);
        assert!(marked.at_wall_utc.is_some());
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn blank_text_is_refused_and_writes_nothing() {
        let root = scratch("blank");
        let (dir, _lock) = running(&root, "now");
        let error = write_mark(&dir, "  ", MarkKind::Note).unwrap_err();
        assert_eq!(envelope::exit_kind(&error), envelope::ExitKind::Usage);
        assert!(!dir.join(annotations::ANNOTATIONS_FILE).exists());
        let _ = std::fs::remove_dir_all(&root);
    }
}
