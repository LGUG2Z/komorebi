//! From a recorded log line to the source that wrote it.
//!
//! This is what turns "komorebi did something odd at 00:12" into "this statement, in this function,
//! ran": each line of a handler's output is matched to the `tracing` statement in komorebi's source
//! that produced it (see [`super::catalog`]), shown with the code around it, and described by the
//! role of the part of komorebi it came from. Every answer carries whether the source is the
//! version that was recorded.
//!
//! The `code` command is a thin layer over three functions on [`Analysis`], each returning
//! [`CodeData`]:
//!
//! | asked | function |
//! |---|---|
//! | what wrote these log lines | [`Analysis::code_for_lines`] |
//! | what wrote everything one event's handler logged | [`Analysis::code_for_event`] |
//! | which statements say this (no log line needed) | [`Analysis::code_search`] |
//!
//! # Flavor
//!
//! The statements come from the catalog of the flavor the session recorded
//! ([`Analysis::catalog`]) and the role of each line from that flavor's rules
//! ([`Rules::role`](super::flavor::Rules::role)), so a session recorded from komorebi for Windows
//! is explained by komorebi for Windows' source, on either host.
//! If the recorder was built without that flavor's source the catalog is empty: no line is
//! matched, and [`CodeData::skew`] carries the warning on how to build one.
//!
//! # What a match does and does not say
//!
//! A [`Source`] names the statement whose text a line fits
//! ([`Confidence`](super::catalog::Confidence) reports how well), not that the statement ran in the
//! version that was recorded: that is [`LineSource::file_changed`] and [`CodeData::skew`]. A line
//! with no [`Source`] is one no statement in the indexed source could have written, which for a
//! line from a komorebi that is not the indexed one is a finding in itself.

use super::Analysis;
use super::catalog::Skew;
use super::catalog::Source;
use super::catalog::Statement;
use super::envelope;
use color_eyre::eyre;
use schemars::JsonSchema;
use serde::Serialize;
use std::path::Path;

/// The schema name that identifies a [`CodeData`] in an envelope (`code/1`); listed by the
/// `schema` command.
pub const SCHEMA_CODE: &str = "code/1";

/// A few lines of komorebi's source around a statement.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, JsonSchema)]
pub struct Snippet {
    /// The file, relative to the checkout of komorebi and `/`-separated.
    pub file: String,
    /// The statement's line.
    pub line: usize,
    /// `(line number, text)` of each line shown, in order and without line endings.
    pub lines: Vec<(usize, String)>,
}

/// One line of komorebi's log, with the statement of komorebi's source that wrote it.
#[derive(Debug, Clone, PartialEq, Serialize, JsonSchema)]
pub struct LineSource {
    /// The line of the session's `komorebi.log`, counting from 1.
    pub line: u64,
    /// When the line was logged, in seconds of video time; `None` if the line has no time or it
    /// cannot be placed on the session's clock.
    pub video_s: Option<f64>,
    /// The line's level in capitals (`INFO`); `None` for a line that is not a log record, such as
    /// a panic message.
    pub level: Option<String>,
    /// The module the line was logged from (its target), such as `komorebi::reaper`; `None` where
    /// the line has none.
    pub target: Option<String>,
    /// The line's message, without ANSI escapes.
    pub message: String,
    /// The part of komorebi that logged it, in a sentence. `None` if the line has no target.
    pub role: Option<String>,
    /// That part changes state without sending a notification.
    pub changes_state_silently: bool,
    /// The statement that wrote it, if one matched. `None` for a line without a target and level,
    /// and for one that no statement in the indexed source could have written.
    pub source: Option<Source>,
    /// The file the statement is in differs between the recorded komorebi and the indexed source,
    /// so it may not be the statement that ran. Also `true` when it cannot be told which files
    /// differ ([`Skew::file_changed`]). `false` when there is no `source`.
    pub file_changed: bool,
    /// The statement and the lines around it. `None` if there is no `source`, or the checkout of
    /// komorebi was not given or does not have the file.
    pub snippet: Option<Snippet>,
}

/// The source of some of komorebi's log, or the statements that match a search: the `data` of the
/// `code` command.
#[derive(Debug, Clone, PartialEq, Serialize, JsonSchema)]
pub struct CodeData {
    /// How the indexed source compares with the komorebi that was recorded. It applies to
    /// everything below.
    pub skew: Skew,
    /// The thing asked about, e.g. `event:12` or `log:345`, or `grep <text>`.
    pub about: String,
    /// One per log line asked about, in the order asked. Empty for a search.
    pub lines: Vec<LineSource>,
    /// Statements whose text contains what was searched for (`--grep`), in source order. Empty
    /// unless the answer is to a search.
    pub statements: Vec<Statement>,
}

/// A few lines of source around `statement`, `context` lines on each side, read from the checkout
/// at `root`. `None` without a checkout, or if its file for the statement cannot be read.
fn snippet(root: Option<&Path>, statement: &Statement, context: usize) -> Option<Snippet> {
    let text = std::fs::read_to_string(root?.join(&statement.file)).ok()?;
    let all: Vec<&str> = text.lines().collect();
    let first = statement.line.saturating_sub(context).max(1);
    let last = (statement.line + context).min(all.len());
    (first <= last).then(|| Snippet {
        file: statement.file.clone(),
        line: statement.line,
        lines: (first..=last)
            .map(|number| (number, all[number - 1].to_string()))
            .collect(),
    })
}

impl Analysis {
    /// Returns the statement of komorebi's source that wrote each of the lines `numbers` of the
    /// session's komorebi log.
    ///
    /// `about` is only a label, kept in [`CodeData::about`]. `numbers` are line numbers of
    /// `komorebi.log`, counting from 1. `context` is how many lines of source to show on each side
    /// of a statement.
    ///
    /// `root` is a checkout of komorebi, from which the snippets are read and against which the
    /// skew is computed. With `None` no snippets are returned, and the skew is worked out with the
    /// checkout that [`default_code_root`](crate::annotations::default_code_root) finds for the
    /// session's flavor, if any. The `code` command passes the checkout it resolved, so it gets
    /// snippets whenever one exists.
    ///
    /// # Errors
    ///
    /// Returns an error if a number in `numbers` is `0` or past the last line of the log (the
    /// not-found exit kind). The first bad number fails the whole call: no partial answer is
    /// returned.
    ///
    /// # Example
    ///
    /// ```rust
    /// use komorebi_recorder::analysis::Analysis;
    /// use komorebi_recorder::testkit::SessionBuilder;
    ///
    /// let (session, _) = SessionBuilder::new("doc-code-lines")
    ///     .log(2.0, "INFO", "komorebi::reaper: reaping window: 4711")
    ///     .build();
    /// let analysis = Analysis::open(&session.dir)?;
    ///
    /// // the first line of komorebi.log, with no checkout to show source from
    /// let data = analysis.code_for_lines("log:1", &[1], 3, None)?;
    /// assert_eq!(data.about, "log:1");
    /// assert_eq!(data.lines[0].target.as_deref(), Some("komorebi::reaper"));
    /// assert_eq!(data.lines[0].message, "reaping window: 4711");
    /// assert!(data.lines[0].snippet.is_none());
    ///
    /// // a line that is not in the log is an error, not an empty answer
    /// assert!(analysis.code_for_lines("log:9", &[9], 3, None).is_err());
    /// # Ok::<(), Box<dyn std::error::Error>>(())
    /// ```
    ///
    /// Which statement a line is matched to depends on the source the recorder was built with, so
    /// it is covered by `tests/code.rs::a_log_line_is_traced_to_the_statement_that_wrote_it`, which
    /// also covers the role, and
    /// `tests/code.rs::only_the_files_that_changed_since_the_recorded_commit_are_doubted`, which
    /// covers [`LineSource::file_changed`].
    pub fn code_for_lines(
        &self,
        about: &str,
        numbers: &[u64],
        context: usize,
        root: Option<&Path>,
    ) -> eyre::Result<CodeData> {
        let log = self.log();
        let skew = match root {
            Some(root) => self.skew_in(Some(root)),
            None => self.skew(),
        };
        let mut lines = Vec::new();
        for number in numbers {
            let line = usize::try_from(number.saturating_sub(1))
                .ok()
                .filter(|_| *number >= 1)
                .and_then(|index| log.lines.get(index))
                .ok_or_else(|| {
                    envelope::not_found(format!(
                        "komorebi.log has {} lines; there is no line {number}",
                        log.lines.len()
                    ))
                })?;

            let source = match (&line.target, line.level) {
                (Some(target), Some(level)) => self.catalog().find(target, level, &line.message),
                _ => None,
            };
            let role = line
                .target
                .as_deref()
                .map(|target| self.flavor().rules().role(target));

            let file_changed = source
                .as_ref()
                .is_some_and(|source| skew.file_changed(&source.statement.file));
            lines.push(LineSource {
                line: line.number,
                video_s: line
                    .wall_us
                    .and_then(|wall_us| self.clock().wall_to_mach(wall_us))
                    .map(|mach_ns| self.video_s(mach_ns)),
                level: line.level.map(|level| format!("{level:?}").to_uppercase()),
                target: line.target.clone(),
                message: line.message.clone(),
                role: role.map(|role| role.summary.to_string()),
                changes_state_silently: role.is_some_and(|role| role.changes_state_silently),
                file_changed,
                snippet: source
                    .as_ref()
                    .and_then(|source| snippet(root, &source.statement, context)),
                source,
            });
        }

        Ok(CodeData {
            skew,
            about: about.to_string(),
            lines,
            statements: Vec::new(),
        })
    }

    /// Returns the source of everything the handler of event `seq` logged, header line first.
    ///
    /// The lines are those of the handler's block in komorebi's log (see [`Analysis::pairing`]),
    /// answered like [`Analysis::code_for_lines`], with `event:<seq>` as [`CodeData::about`].
    /// `context` and `root` are as for [`Analysis::code_for_lines`].
    ///
    /// # Errors
    ///
    /// Returns an error (the not-found exit kind) if:
    ///
    /// - there is no event numbered `seq`;
    /// - no handler lines were found in komorebi's log for the event: the log may not have been
    ///   recorded, or komorebi's log level may not show its handler.
    ///
    /// The lines of a handler are covered by
    /// `tests/code.rs::an_event_is_explained_by_everything_its_handler_logged`, and the missing
    /// event by
    /// `tests/code.rs::a_line_no_statement_writes_is_said_so_and_a_bad_reference_is_not_found`.
    pub fn code_for_event(
        &self,
        seq: u64,
        context: usize,
        root: Option<&Path>,
    ) -> eyre::Result<CodeData> {
        let index = self
            .event_index(seq)
            .ok_or_else(|| envelope::not_found(format!("there is no event #{seq}")))?;
        let block = self
            .pairing()
            .pairs
            .get(index)
            .and_then(|pair| pair.block)
            .and_then(|id| self.log().blocks.get(id))
            .ok_or_else(|| {
                envelope::not_found(format!(
                    "no handler lines were found in komorebi's log for event #{seq}: the log may \
                     not have been recorded, or komorebi's log level may not show its handler"
                ))
            })?;

        self.code_for_lines(&format!("event:{seq}"), &block.lines.clone(), context, root)
    }

    /// Returns the statements of komorebi's source whose message contains `text`, without needing
    /// a log line.
    ///
    /// The comparison ignores case and looks only at the statements' message templates, so
    /// `reaping` finds `reaping window: {window_id}`. The statements are those of the catalog of
    /// the session's flavor, in source order, in [`CodeData::statements`]; [`CodeData::lines`] is
    /// empty and [`CodeData::about`] is `grep <text>`.
    ///
    /// # Example
    ///
    /// ```rust
    /// use komorebi_recorder::analysis::Analysis;
    /// use komorebi_recorder::testkit::SessionBuilder;
    ///
    /// let (session, _) = SessionBuilder::new("doc-code-search").build();
    /// let analysis = Analysis::open(&session.dir)?;
    /// # if analysis.catalog().is_empty() { return Ok(()); } // built without the source
    ///
    /// let found = analysis.code_search("Reaping WINDOW");
    /// assert_eq!(found.about, "grep Reaping WINDOW");
    /// assert!(found.lines.is_empty());
    /// assert!(!found.statements.is_empty());
    /// assert!(found.statements.iter().any(|statement| statement.file == "komorebi-macos/src/reaper.rs"));
    /// # Ok::<(), Box<dyn std::error::Error>>(())
    /// ```
    ///
    /// Also covered by `tests/code.rs::statements_can_be_searched_by_their_text`.
    #[must_use]
    pub fn code_search(&self, text: &str) -> CodeData {
        let needle = text.to_lowercase();
        CodeData {
            skew: self.skew(),
            about: format!("grep {text}"),
            lines: Vec::new(),
            statements: self.catalog().statements_matching(&needle),
        }
    }
}
