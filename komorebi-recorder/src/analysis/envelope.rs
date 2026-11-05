//! The shape every investigation command prints with `--json`, and how a failure maps to an exit
//! code.
//!
//! An agent driving the CLI should never have to guess whether a command worked, whether the output
//! was cut short, or why it failed, so every command answers with the same envelope and exits with
//! a code that names the kind of failure it was.
//!
//! There are two halves. An [`Envelope`] is what a command prints: `ok`, the `schema` that names
//! the shape of its `data`, the `data`, any `warnings`, whether the output was `truncated` (and the
//! `next_cursor` to ask for the rest) and, on failure, an `error`. An [`ExitKind`] is the kind of a
//! failure, and decides the process's exit code. Code that fails returns an [`eyre::Report`] built
//! by [`usage`], [`not_found`], [`unusable`] or [`over_budget`]; [`exit_kind`] reads the kind back
//! out of a report, however much context was added to it on the way up, and
//! `crate::cli::execute` turns it into the exit code and, with `--json`, an envelope.
//!
//! | Exit code | [`ExitKind`] | Meaning |
//! |---|---|---|
//! | 0 | [`Ok`](ExitKind::Ok) | the command worked |
//! | 1 | [`Error`](ExitKind::Error) | something went wrong that is none of the kinds below |
//! | 2 | [`Usage`](ExitKind::Usage) | the command line was wrong |
//! | 3 | [`NotFound`](ExitKind::NotFound) | the session, or something asked for in it, does not exist |
//! | 4 | [`Unusable`](ExitKind::Unusable) | the session exists but cannot answer: still recording, or it recorded nothing |
//! | 5 | [`Budget`](ExitKind::Budget) | the answer would have exceeded the time or size budget |
//! | 6 | [`Assertion`](ExitKind::Assertion) | `assert` measured the recording and a check did not hold |
//!
//! The codes are stable. [`ENVELOPE_VERSION`] is bumped when the envelope's own fields change;
//! the shape of each command's `data` is versioned by its `schema` (`brief/1`), and the
//! `schemas` module has them.
//!
//! # Example
//!
//! A failure, with a hint for what to do next, and what a command makes of it:
//!
//! ```rust
//! use komorebi_recorder::analysis::envelope::{self, Envelope, ExitKind};
//!
//! let failure = envelope::with_hint(
//!     envelope::unusable("/tmp/s is still being recorded"),
//!     "stop the recording first",
//! );
//! assert_eq!(envelope::exit_kind(&failure), ExitKind::Unusable);
//! assert_eq!(envelope::exit_kind(&failure).code(), 4);
//!
//! // with --json the same failure is printed as an envelope with `ok: false`
//! let json = serde_json::to_value(Envelope::failed(&failure))?;
//! assert_eq!(json["ok"], false);
//! assert_eq!(json["schema"], "error/1");
//! assert_eq!(json["error"]["kind"], "unusable");
//! assert_eq!(json["error"]["code"], 4);
//! assert_eq!(json["error"]["hint"], "stop the recording first");
//!
//! // and a success has the same fields, with `data` in place of `error`
//! let json = serde_json::to_value(Envelope::ok("demo/1", vec![1, 2, 3]))?;
//! assert_eq!(json["ok"], true);
//! assert_eq!(json["data"], serde_json::json!([1, 2, 3]));
//! assert!(json["error"].is_null());
//! # Ok::<(), Box<dyn std::error::Error>>(())
//! ```

use color_eyre::eyre;
use schemars::JsonSchema;
use serde::Serialize;
use std::fmt;

/// Bumped when the envelope's own fields change (each command's `data` is versioned by `schema`).
pub const ENVELOPE_VERSION: u32 = 1;

/// How the process exits: the kind of a failure, and the exit code that goes with it.
///
/// Serialized in kebab case (`ok`, `error`, `usage`, `not-found`, `unusable`, `budget`,
/// `assertion`) in an
/// envelope's `error.kind`. [`ExitKind::code`] gives the exit code.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, JsonSchema)]
#[serde(rename_all = "kebab-case")]
pub enum ExitKind {
    /// The command worked: exit code 0.
    Ok,
    /// Something went wrong that is not one of the cases below: exit code 1. This is also the kind
    /// of any error that does not carry one.
    Error,
    /// The command line was wrong: exit code 2. For example an argument that does not parse (a
    /// time like `soon`) or a path that points outside the session.
    Usage,
    /// The session, or something asked for in it, does not exist: exit code 3.
    NotFound,
    /// The session exists but cannot answer: still recording, or it recorded nothing. Exit code 4.
    Unusable,
    /// The answer would have exceeded the time or size budget: exit code 5. Narrowing the request
    /// is the way forward.
    Budget,
    /// `assert` measured the recording and a check did not hold: exit code 6. Not a failure of the
    /// command: the answer is "no".
    Assertion,
}

impl ExitKind {
    /// Returns the process exit code of this kind: 0 for [`ExitKind::Ok`], then 1 to 6 in the order
    /// the variants are declared.
    ///
    /// The mapping is stable, so a script may rely on the numbers (`exit_codes_are_stable` pins
    /// them).
    ///
    /// # Example
    ///
    /// ```rust
    /// use komorebi_recorder::analysis::envelope::ExitKind;
    ///
    /// assert_eq!(ExitKind::Ok.code(), 0);
    /// assert_eq!(ExitKind::Usage.code(), 2);
    /// assert_eq!(ExitKind::NotFound.code(), 3);
    /// assert_eq!(ExitKind::Budget.code(), 5);
    /// assert_eq!(ExitKind::Assertion.code(), 6);
    /// ```
    #[must_use]
    pub fn code(self) -> u8 {
        match self {
            Self::Ok => 0,
            Self::Error => 1,
            Self::Usage => 2,
            Self::NotFound => 3,
            Self::Unusable => 4,
            Self::Budget => 5,
            Self::Assertion => 6,
        }
    }
}

/// An error that carries how the process should exit and, optionally, what to do about it.
///
/// It is not built directly: [`usage`], [`not_found`], [`unusable`] and [`over_budget`] make one and
/// return it as an [`eyre::Report`], and [`with_hint`] adds the hint. Its [`Display`](fmt::Display)
/// is the message, followed by the hint in parentheses when there is one.
#[derive(Debug)]
pub struct CliError {
    /// How the process should exit.
    pub kind: ExitKind,
    /// A description of what went wrong, for a reader.
    pub message: String,
    /// A suggestion for what the caller can do next, if there is one.
    pub hint: Option<String>,
}

impl fmt::Display for CliError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.message)?;
        if let Some(hint) = &self.hint {
            write!(f, " ({hint})")?;
        }
        Ok(())
    }
}

impl std::error::Error for CliError {}

fn failure(kind: ExitKind, message: impl Into<String>) -> eyre::Report {
    eyre::Report::new(CliError {
        kind,
        message: message.into(),
        hint: None,
    })
}

/// Returns an error of kind [`ExitKind::Usage`] with this message: the command line was wrong.
///
/// # Example
///
/// ```rust
/// use komorebi_recorder::analysis::envelope::{self, ExitKind};
///
/// let error = envelope::usage("an empty time");
/// assert_eq!(envelope::exit_kind(&error), ExitKind::Usage);
/// assert_eq!(error.to_string(), "an empty time");
/// ```
pub fn usage(message: impl Into<String>) -> eyre::Report {
    failure(ExitKind::Usage, message)
}

/// Returns an error of kind [`ExitKind::NotFound`] with this message: the session, or something
/// asked for in it, does not exist.
pub fn not_found(message: impl Into<String>) -> eyre::Report {
    failure(ExitKind::NotFound, message)
}

/// Returns an error of kind [`ExitKind::Unusable`] with this message: the session exists but
/// cannot answer.
pub fn unusable(message: impl Into<String>) -> eyre::Report {
    failure(ExitKind::Unusable, message)
}

/// Returns an error of kind [`ExitKind::Budget`] with this message: the answer would have
/// exceeded the time or size budget.
///
/// The message should say how to narrow the request. It is made from an `Exceeded` of the
/// `budget` module where the scan has one.
pub fn over_budget(message: impl Into<String>) -> eyre::Report {
    failure(ExitKind::Budget, message)
}

/// Returns the same failure with a pointer to what the caller can do next.
///
/// If `report` is a [`CliError`], its kind is kept and the hint is set on it (replacing an earlier
/// hint). That holds for a [`CliError`] that has had context added with `wrap_err` as well, but the
/// result is then the [`CliError`] alone: the context that was added is not kept. Any other error
/// becomes the context of the hint: the hint is then the outermost message, and the kind is
/// [`ExitKind::Error`].
///
/// # Example
///
/// ```rust
/// use komorebi_recorder::analysis::envelope::{self, ExitKind};
///
/// let error = envelope::with_hint(
///     envelope::unusable("still recording"),
///     "run `reindex` if it died",
/// );
/// assert_eq!(envelope::exit_kind(&error), ExitKind::Unusable);
/// assert_eq!(error.to_string(), "still recording (run `reindex` if it died)");
/// ```
pub fn with_hint(report: eyre::Report, hint: impl Into<String>) -> eyre::Report {
    match report.downcast::<CliError>() {
        Ok(mut error) => {
            error.hint = Some(hint.into());
            eyre::Report::new(error)
        }
        Err(report) => report.wrap_err(hint.into()),
    }
}

/// Returns how `report` should make the process exit.
///
/// The kind is that of the first [`CliError`] anywhere in the report's chain of causes, so context
/// added with `wrap_err` on the way up does not change it. A report with none is
/// [`ExitKind::Error`].
///
/// # Example
///
/// ```rust
/// use color_eyre::eyre::WrapErr;
/// use komorebi_recorder::analysis::envelope::{self, ExitKind};
///
/// let report = Err::<(), _>(envelope::not_found("no such event"))
///     .wrap_err("while resolving the start")
///     .unwrap_err();
/// assert_eq!(envelope::exit_kind(&report), ExitKind::NotFound);
///
/// // an error from anywhere else is the generic kind
/// let plain = color_eyre::eyre::Report::msg("boom");
/// assert_eq!(envelope::exit_kind(&plain), ExitKind::Error);
/// ```
#[must_use]
pub fn exit_kind(report: &eyre::Report) -> ExitKind {
    report
        .chain()
        .find_map(|cause| cause.downcast_ref::<CliError>())
        .map_or(ExitKind::Error, |error| error.kind)
}

/// Marks a failure whose answer the command has already printed, so that
/// `crate::cli::execute` only sets the exit code and prints nothing more (no second envelope).
///
/// Made by [`reported`]; the kind is still read with [`exit_kind`].
#[derive(Debug)]
pub struct Reported;

impl fmt::Display for Reported {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("the answer was already printed")
    }
}

impl std::error::Error for Reported {}

/// Returns an error of this kind whose answer the command has already printed (see [`Reported`]).
pub fn reported(kind: ExitKind, message: impl Into<String>) -> eyre::Report {
    failure(kind, message).wrap_err(Reported)
}

/// Returns whether `report` was made by [`reported`].
#[must_use]
pub fn is_reported(report: &eyre::Report) -> bool {
    report.downcast_ref::<Reported>().is_some()
}

/// The `error` of a failed [`Envelope`]: the kind of failure and what to do about it.
#[derive(Debug, Serialize, JsonSchema)]
pub struct ErrorBody {
    /// The kind of failure, in kebab case.
    pub kind: ExitKind,
    /// The process exit code of `kind`, which is also this command's exit status.
    pub code: u8,
    /// The message of the outermost error. When the error has a hint this includes it, in
    /// parentheses, as [`CliError`]'s `Display` does.
    pub message: String,
    /// A suggestion for what the caller can do next, if the error gave one.
    pub hint: Option<String>,
}

/// The JSON object every command prints with `--json`.
///
/// Every command prints this shape, whatever it answers: `data` varies and is described by
/// `schema`, and the other fields mean the same everywhere. On success `ok` is `true`, `data` is
/// the answer and `error` is `null`. On failure `ok` is `false`, `data` is `null`, and `error`
/// describes what happened. On failure the process exit code is `error.code`; on success it is 0.
///
/// The fields are serialized in this order: `ok`, `schema`, `envelope_version`, `data`,
/// `warnings`, `truncated`, `next_cursor`, `error`. They are always present; an absent value is
/// `null` or an empty list.
///
/// # Example
///
/// ```rust
/// use komorebi_recorder::analysis::envelope::Envelope;
///
/// let envelope = Envelope::ok("events/1", vec!["event:1", "event:2"])
///     .warnings(vec!["the log level was info".to_string()])
///     .truncated(Some("2".to_string()));
///
/// let json = serde_json::to_value(&envelope)?;
/// assert_eq!(json["ok"], true);
/// assert_eq!(json["schema"], "events/1");
/// assert_eq!(json["warnings"][0], "the log level was info");
/// // cut short: pass the cursor back as `--cursor` for the rest
/// assert_eq!(json["truncated"], true);
/// assert_eq!(json["next_cursor"], "2");
/// # Ok::<(), Box<dyn std::error::Error>>(())
/// ```
#[derive(Debug, Serialize, JsonSchema)]
pub struct Envelope<T> {
    /// Whether the command worked.
    pub ok: bool,
    /// Which command's `data` this is, e.g. `brief/1`; see `komorebi-recorder schema`.
    pub schema: String,
    /// The version of the envelope's own fields: [`ENVELOPE_VERSION`].
    pub envelope_version: u32,
    /// The command's answer, shaped as `schema` says. `None` (`null`) on failure.
    pub data: Option<T>,
    /// Things that went wrong reading the session that did not stop the answer, shown with it.
    pub warnings: Vec<String>,
    /// The output was cut short; pass `next_cursor` back as `--cursor` for the rest.
    pub truncated: bool,
    /// Where to continue from when `truncated`, as the command's `--cursor` takes it. `None`
    /// (`null`) when the output was not cut short, or was but the command has no cursor to give.
    pub next_cursor: Option<String>,
    /// Why the command failed. `None` (`null`) on success.
    pub error: Option<ErrorBody>,
}

impl<T> Envelope<T> {
    /// Returns the envelope of a successful command: `ok`, with `data` as the answer to the command
    /// whose output is described by `schema`.
    ///
    /// The envelope has no warnings, is not truncated and has no error; add warnings with
    /// [`Envelope::warnings`] and mark a cut-short answer with [`Envelope::truncated`].
    #[must_use]
    pub fn ok(schema: &str, data: T) -> Self {
        Self {
            ok: true,
            schema: schema.to_string(),
            envelope_version: ENVELOPE_VERSION,
            data: Some(data),
            warnings: Vec::new(),
            truncated: false,
            next_cursor: None,
            error: None,
        }
    }

    /// Returns the envelope as a failed one that still carries its `data`: `ok` is `false` and
    /// `error` has `kind` and its exit code. For a command whose answer is the point even when it
    /// is "no" (`assert`).
    #[must_use]
    pub fn with_error(mut self, kind: ExitKind, message: impl Into<String>) -> Self {
        self.ok = false;
        self.error = Some(ErrorBody {
            kind,
            code: kind.code(),
            message: message.into(),
            hint: None,
        });
        self
    }

    /// Returns the envelope with `warnings` as its warnings. They replace any that were there.
    #[must_use]
    pub fn warnings(mut self, warnings: Vec<String>) -> Self {
        self.warnings = warnings;
        self
    }

    /// Returns the envelope marked as cut short, with `next_cursor` as how to ask for the rest.
    ///
    /// A `next_cursor` of `None` still marks the output truncated.
    #[must_use]
    pub fn truncated(mut self, next_cursor: Option<String>) -> Self {
        self.truncated = true;
        self.next_cursor = next_cursor;
        self
    }
}

impl Envelope<()> {
    /// Returns the envelope of a failed command: `ok` is `false`, the `schema` is `error/1`, and
    /// the `error` has the kind and exit code of `report` ([`exit_kind`]).
    ///
    /// The `message` is the outermost error of the report, not the whole chain of causes. A
    /// [`CliError`] with a hint has the hint in its message as well as in `hint`. The `hint` is the
    /// one of the first [`CliError`] in the chain, if it has one.
    ///
    /// # Example
    ///
    /// ```rust
    /// use komorebi_recorder::analysis::envelope::{self, Envelope};
    ///
    /// let report = envelope::not_found("there is no event #99");
    /// let envelope = Envelope::failed(&report);
    ///
    /// assert!(!envelope.ok);
    /// assert!(envelope.data.is_none());
    /// let error = envelope.error.expect("a failed envelope has an error");
    /// assert_eq!(error.code, 3);
    /// assert_eq!(error.message, "there is no event #99");
    /// assert_eq!(error.hint, None);
    /// ```
    #[must_use]
    pub fn failed(report: &eyre::Report) -> Self {
        let kind = exit_kind(report);
        let hint = report
            .chain()
            .find_map(|cause| cause.downcast_ref::<CliError>())
            .and_then(|error| error.hint.clone());

        Self {
            ok: false,
            schema: "error/1".to_string(),
            envelope_version: ENVELOPE_VERSION,
            data: None,
            warnings: Vec::new(),
            truncated: false,
            next_cursor: None,
            error: Some(ErrorBody {
                kind,
                code: kind.code(),
                message: report
                    .chain()
                    .next()
                    .map_or_else(String::new, ToString::to_string),
                hint,
            }),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn exit_codes_are_stable() {
        let codes: Vec<u8> = [
            ExitKind::Ok,
            ExitKind::Error,
            ExitKind::Usage,
            ExitKind::NotFound,
            ExitKind::Unusable,
            ExitKind::Budget,
            ExitKind::Assertion,
        ]
        .iter()
        .map(|kind| kind.code())
        .collect();
        assert_eq!(codes, [0, 1, 2, 3, 4, 5, 6]);
    }

    #[test]
    fn a_plain_error_exits_with_the_generic_code() {
        assert_eq!(exit_kind(&eyre::Report::msg("boom")), ExitKind::Error);
    }

    #[test]
    fn the_kind_survives_added_context() {
        use color_eyre::eyre::WrapErr;

        let report = Err::<(), _>(not_found("no such event"))
            .wrap_err("while resolving the start")
            .unwrap_err();
        assert_eq!(exit_kind(&report), ExitKind::NotFound);
    }

    #[test]
    fn a_hint_travels_with_the_error() {
        let report = with_hint(unusable("still recording"), "run `reindex` if it died");
        assert_eq!(exit_kind(&report), ExitKind::Unusable);

        let envelope = Envelope::failed(&report);
        let body = envelope.error.expect("error body");
        assert_eq!(body.code, 4);
        assert_eq!(body.hint.as_deref(), Some("run `reindex` if it died"));
        assert!(!envelope.ok);
    }

    #[test]
    fn the_envelope_has_a_fixed_shape() {
        let envelope = Envelope::ok("demo/1", vec![1, 2, 3])
            .warnings(vec!["careful".to_string()])
            .truncated(Some("c2".to_string()));
        let value = serde_json::to_value(&envelope).expect("serialize");

        assert_eq!(value["ok"], true);
        assert_eq!(value["schema"], "demo/1");
        assert_eq!(value["envelope_version"], ENVELOPE_VERSION);
        assert_eq!(value["data"], serde_json::json!([1, 2, 3]));
        assert_eq!(value["warnings"], serde_json::json!(["careful"]));
        assert_eq!(value["truncated"], true);
        assert_eq!(value["next_cursor"], "c2");
        assert!(value["error"].is_null());
    }
}
