//! Scripted checks over a recording: the engine of `assert`.
//!
//! A script that drives komorebi while it is recorded ends up wanting a yes or no: did a layout
//! mismatch show up, did the log stay free of errors, did the picture settle? `assert` answers with
//! its exit code, by evaluating *checks* with the analyses the other commands use. There is no
//! komorebi-specific language here: a check is a kind and what to measure, and a comparison.
//!
//! ```text
//! <kind> <args...> <metric><op><number>
//! ```
//!
//! `<op>` is one of `==`, `!=`, `<=`, `>=`, `<`, `>`. The arguments are `key=value` words
//! separated by spaces; a value with spaces is quoted with `"` or `'`. The kinds are listed in
//! [`KINDS`]:
//!
//! | Kind | Arguments | Metrics |
//! |---|---|---|
//! | `findings` | `kind=K`, `severity=error\|warn\|info` | `count` |
//! | `log` | a [`logq`](crate::analysis::logq) query, as written for `logq` (optionally quoted) | `count` |
//! | `motion` | `region=x,y,w,h`, `from=T`, `to=T` | `segments`, `active_ms`, `peak_share`, `peak_delta` |
//! | `blank` | `region=x,y,w,h` (required), `from=T`, `to=T`, `color=white` | `ms` (the longest stretch), `total_ms`, `stretches` |
//! | `annotations` | `kind=note\|bug\|finding` | `count` |
//!
//! `T` is any time `state --at` takes: video time, `logtime:HH:MM:SS.mmm`, `event:N`, `bug:<id>`,
//! offsets of those.
//!
//! A check ends as [`Status::Pass`], [`Status::Fail`] (it was measured and the comparison does
//! not hold) or [`Status::Error`] (it could not be measured: no video, a host that cannot decode
//! it, a time that does not exist). A malformed check is a usage error before anything is
//! measured. [`AssertData::exit_kind`] turns the lot into the exit code: errors are never
//! counted as a pass.
//!
//! Adding a kind is one struct that implements [`Check`] and one line in [`KINDS`].
//!
//! # Example
//!
//! ```rust
//! use komorebi_recorder::analysis::Analysis;
//! use komorebi_recorder::analysis::assertions::{self, Status};
//! use komorebi_recorder::testkit::SessionBuilder;
//!
//! let (session, _) = SessionBuilder::new("doc-assert")
//!     .log(2.0, "ERROR", "komorebi::window: could not place window 4711")
//!     .build();
//! let analysis = Analysis::open(&session.dir)?;
//!
//! let data = assertions::run_all(
//!     &analysis,
//!     &[
//!         "log \"level=ERROR\" count==0".to_string(),
//!         "annotations kind=note count>=0".to_string(),
//!     ],
//! )?;
//! assert_eq!(data.checks[0].status, Status::Fail);
//! assert_eq!(data.checks[0].observed, serde_json::json!(1));
//! assert_eq!(data.checks[1].status, Status::Pass);
//! assert_eq!(data.exit_kind().code(), 6);
//! # Ok::<(), Box<dyn std::error::Error>>(())
//! ```

use super::Analysis;
use super::blank::DEFAULT_MIN_DURATION_S;
use super::blank::DEFAULT_VARIANCE;
use super::blank::StretchKind;
use super::budget::Budget;
use super::envelope;
use super::envelope::ExitKind;
use super::findings::FindingsQuery;
use super::invariants::Severity;
use super::logq::LogqQuery;
use super::motion::ScanOptions;
use super::pixels::Crop;
use super::video::SessionVideo;
use crate::annotations;
use crate::timecode;
use color_eyre::eyre;
use regex::Regex;
use schemars::JsonSchema;
use serde::Serialize;
use serde_json::Value;
use std::collections::BTreeMap;
use std::fmt;
use std::fmt::Write;
use std::sync::LazyLock;

/// The name that appears in the `schema` of `assert --json`.
pub const SCHEMA_ASSERT: &str = "assert/1";

/// How an observed number is compared with the one in the check.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Op {
    /// `==`
    Eq,
    /// `!=`
    Ne,
    /// `<=`
    Le,
    /// `>=`
    Ge,
    /// `<`
    Lt,
    /// `>`
    Gt,
}

impl Op {
    fn parse(text: &str) -> Option<Self> {
        Some(match text {
            "==" => Self::Eq,
            "!=" => Self::Ne,
            "<=" => Self::Le,
            ">=" => Self::Ge,
            "<" => Self::Lt,
            ">" => Self::Gt,
            _ => return None,
        })
    }

    /// Returns whether `observed <op> expected` holds.
    #[must_use]
    pub fn holds(self, observed: f64, expected: f64) -> bool {
        match self {
            Self::Eq => (observed - expected).abs() < f64::EPSILON,
            Self::Ne => (observed - expected).abs() >= f64::EPSILON,
            Self::Le => observed <= expected,
            Self::Ge => observed >= expected,
            Self::Lt => observed < expected,
            Self::Gt => observed > expected,
        }
    }
}

impl fmt::Display for Op {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Eq => "==",
            Self::Ne => "!=",
            Self::Le => "<=",
            Self::Ge => ">=",
            Self::Lt => "<",
            Self::Gt => ">",
        })
    }
}

/// The comparison that ends a check: `count<=0`, `active_ms>=250`, `peak_share<0.2`.
#[derive(Debug, Clone, PartialEq)]
pub struct Comparison {
    /// What is measured: `count`, `segments`, `ms`, ...
    pub metric: String,
    /// How it is compared.
    pub op: Op,
    /// What it is compared with.
    pub value: f64,
}

impl Comparison {
    /// Returns whether the comparison holds for this measurement.
    #[must_use]
    pub fn holds(&self, observed: f64) -> bool {
        self.op.holds(observed, self.value)
    }

    fn require(&self, kind: &str, metrics: &[&str]) -> eyre::Result<()> {
        if metrics.contains(&self.metric.as_str()) {
            return Ok(());
        }
        Err(envelope::usage(format!(
            "a `{kind}` check measures {}, not `{}`",
            metrics.join(", "),
            self.metric
        )))
    }
}

impl fmt::Display for Comparison {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}{}{}", self.metric, self.op, number_text(self.value))
    }
}

/// What a check kind is given: the text between the kind and the comparison, and the comparison.
#[derive(Debug, Clone)]
pub struct CheckArgs {
    /// The arguments as written, without the kind and without the comparison.
    pub text: String,
    /// The comparison at the end of the check.
    pub comparison: Comparison,
}

impl CheckArgs {
    /// Splits the arguments into `key=value` pairs, honouring `"` and `'` quotes around a value or
    /// a whole word.
    ///
    /// # Errors
    ///
    /// Returns a usage error for a word that is not `key=value`, a key that is not in `allowed`,
    /// a key given twice or a quote that is never closed.
    pub fn pairs(&self, allowed: &[&str]) -> eyre::Result<BTreeMap<String, String>> {
        let mut pairs = BTreeMap::new();
        for word in words(&self.text)? {
            let (key, value) = word
                .split_once('=')
                .filter(|(key, _)| !key.is_empty())
                .ok_or_else(|| {
                    envelope::usage(format!(
                        "`{word}` is not key=value (keys: {})",
                        allowed.join(", ")
                    ))
                })?;
            if !allowed.contains(&key) {
                return Err(envelope::usage(format!(
                    "unknown argument `{key}` (arguments: {})",
                    allowed.join(", ")
                )));
            }
            if pairs.insert(key.to_string(), value.to_string()).is_some() {
                return Err(envelope::usage(format!("`{key}` is given twice")));
            }
        }
        Ok(pairs)
    }
}

/// Splits `text` into words on whitespace; a quote (`"` or `'`) groups what is inside it, and a
/// backslash in quotes escapes the quote character.
fn words(text: &str) -> eyre::Result<Vec<String>> {
    let mut out = Vec::new();
    let mut current = String::new();
    let mut started = false;
    let mut quote: Option<char> = None;
    let mut chars = text.chars().peekable();

    while let Some(c) = chars.next() {
        match quote {
            Some(q) if c == q => quote = None,
            Some(q) if c == '\\' && chars.peek() == Some(&q) => {
                current.push(q);
                chars.next();
            }
            Some(_) => current.push(c),
            None if c == '"' || c == '\'' => {
                quote = Some(c);
                started = true;
            }
            None if c.is_whitespace() => {
                if started {
                    out.push(std::mem::take(&mut current));
                    started = false;
                }
            }
            None => {
                current.push(c);
                started = true;
            }
        }
    }
    if quote.is_some() {
        return Err(envelope::usage(format!(
            "a quote is never closed in `{text}`"
        )));
    }
    if started {
        out.push(current);
    }
    Ok(out)
}

/// Removes one pair of quotes that wrap the whole text, if there is one.
fn unwrap_quoted(text: &str) -> String {
    let text = text.trim();
    let Some(q) = text.chars().next().filter(|c| *c == '"' || *c == '\'') else {
        return text.to_string();
    };

    let mut out = String::new();
    let mut chars = text[1..].chars().peekable();
    while let Some(c) = chars.next() {
        if c == '\\' && chars.peek() == Some(&q) {
            out.push(q);
            chars.next();
        } else if c == q {
            // the quote closes: it wraps the whole text only if nothing follows
            return if chars.peek().is_none() {
                out
            } else {
                text.to_string()
            };
        } else {
            out.push(c);
        }
    }
    text.to_string()
}

static TAIL: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?s)^(.*?)\s*\b([A-Za-z_]+)\s*(==|!=|<=|>=|<|>)\s*(-?[0-9]+(?:\.[0-9]+)?)\s*$")
        .expect("the comparison pattern is valid")
});

fn number_text(value: f64) -> String {
    if value.fract() == 0.0 && value.abs() < 1e15 {
        format!("{value:.0}")
    } else {
        let text = format!("{value:.3}");
        text.trim_end_matches('0').trim_end_matches('.').to_string()
    }
}

fn number(value: f64) -> Value {
    if value.fract() == 0.0 && value.abs() < 1e15 {
        Value::from(value as i64)
    } else {
        serde_json::Number::from_f64(value).map_or(Value::Null, Value::Number)
    }
}

/// What a check measured.
#[derive(Debug, Clone, PartialEq)]
pub struct CheckOutcome {
    /// The number that was compared, as JSON.
    pub observed: Value,
    /// Whether the comparison holds.
    pub passed: bool,
    /// What to look at when it does not: the findings, log lines or segments behind the number.
    pub detail: Option<String>,
}

impl CheckOutcome {
    fn compare(
        comparison: &Comparison,
        observed: f64,
        detail: impl FnOnce() -> Option<String>,
    ) -> Self {
        let passed = comparison.holds(observed);
        Self {
            observed: number(observed),
            passed,
            detail: if passed { None } else { detail() },
        }
    }
}

/// One kind of check. Implement it and add a line to [`KINDS`].
pub trait Check: Sized {
    /// The word a check starts with.
    const NAME: &'static str;
    /// One line on what the kind checks and its arguments, listed in a usage error.
    const ABOUT: &'static str;

    /// Reads the arguments of a check of this kind.
    ///
    /// # Errors
    ///
    /// Returns a usage error for an argument or a metric this kind does not have, or a value that
    /// does not parse.
    fn parse(args: &CheckArgs) -> eyre::Result<Self>;

    /// Measures the recording and compares.
    ///
    /// # Errors
    ///
    /// Returns an error when the recording cannot be measured (it recorded no video, the host
    /// cannot decode it, a time does not exist); the caller reports it as [`Status::Error`].
    fn run(&self, analysis: &Analysis) -> eyre::Result<CheckOutcome>;
}

/// A parsed check of any kind, ready to run.
pub trait RunCheck {
    /// See [`Check::run`].
    ///
    /// # Errors
    ///
    /// As [`Check::run`].
    fn run(&self, analysis: &Analysis) -> eyre::Result<CheckOutcome>;
}

impl<T: Check> RunCheck for T {
    fn run(&self, analysis: &Analysis) -> eyre::Result<CheckOutcome> {
        Check::run(self, analysis)
    }
}

/// A kind of check, as listed in [`KINDS`].
pub struct CheckKind {
    /// The word a check starts with.
    pub name: &'static str,
    /// One line on the kind and its arguments.
    pub about: &'static str,
    parse: fn(&CheckArgs) -> eyre::Result<Box<dyn RunCheck>>,
}

const fn kind<T: Check + 'static>() -> CheckKind {
    CheckKind {
        name: T::NAME,
        about: T::ABOUT,
        parse: |args| Ok(Box::new(T::parse(args)?)),
    }
}

/// Every kind of check.
pub const KINDS: &[CheckKind] = &[
    kind::<FindingsCheck>(),
    kind::<LogCheck>(),
    kind::<MotionCheck>(),
    kind::<BlankCheck>(),
    kind::<AnnotationsCheck>(),
];

fn kinds_listing() -> String {
    KINDS
        .iter()
        .map(|kind| format!("  {}", kind.about))
        .collect::<Vec<_>>()
        .join("\n")
}

/// A check that was read: the text it was written as, and what runs it.
pub struct Assertion {
    /// The check as written (surrounding whitespace removed).
    pub text: String,
    check: Box<dyn RunCheck>,
}

impl Assertion {
    /// Reads a check.
    ///
    /// # Errors
    ///
    /// Returns a usage error, listing the valid kinds, if the kind is unknown, the comparison is
    /// missing or malformed, or the kind rejects its arguments.
    ///
    /// # Example
    ///
    /// ```rust
    /// use komorebi_recorder::analysis::assertions::Assertion;
    ///
    /// assert!(Assertion::parse("findings kind=log-error count<=0").is_ok());
    ///
    /// let error = Assertion::parse("frames count==1").err().expect("unknown kind");
    /// assert!(error.to_string().contains("unknown check kind `frames`"));
    /// assert!(error.to_string().contains("findings"));
    /// assert!(Assertion::parse("findings count=1").is_err());
    /// ```
    pub fn parse(text: &str) -> eyre::Result<Self> {
        let text = text.trim();
        let (name, rest) = text.split_once(char::is_whitespace).unwrap_or((text, ""));

        let Some(entry) = KINDS.iter().find(|kind| kind.name == name) else {
            return Err(envelope::usage(format!(
                "unknown check kind `{name}` in `{text}`; the kinds are:\n{}",
                kinds_listing()
            )));
        };

        let Some(captures) = TAIL.captures(rest) else {
            return Err(envelope::usage(format!(
                "`{text}` does not end in a comparison like `count<=0` (operators: == != <= >= < >); {}",
                entry.about
            )));
        };
        let comparison = Comparison {
            metric: captures[2].to_string(),
            op: Op::parse(&captures[3]).expect("the pattern only matches operators"),
            value: captures[4]
                .parse()
                .map_err(|_| envelope::usage(format!("`{}` is not a number", &captures[4])))?,
        };

        let args = CheckArgs {
            text: captures[1].to_string(),
            comparison,
        };
        let check = (entry.parse)(&args).map_err(|error| {
            let message = format!("in `{text}`: {error}");
            error_with_context(error, &message)
        })?;
        Ok(Self {
            text: text.to_string(),
            check,
        })
    }

    /// Runs the check; one that cannot be measured is [`Status::Error`], never a pass.
    #[must_use]
    pub fn run(&self, analysis: &Analysis) -> CheckResult {
        match self.check.run(analysis) {
            Ok(outcome) => CheckResult {
                check: self.text.clone(),
                status: if outcome.passed {
                    Status::Pass
                } else {
                    Status::Fail
                },
                passed: outcome.passed,
                observed: outcome.observed,
                detail: outcome.detail,
                error_kind: None,
            },
            Err(error) => {
                let kind = match envelope::exit_kind(&error) {
                    ExitKind::Error => ExitKind::Unusable,
                    kind => kind,
                };
                CheckResult {
                    check: self.text.clone(),
                    status: Status::Error,
                    passed: false,
                    observed: Value::Null,
                    detail: Some(error.to_string()),
                    error_kind: Some(kind),
                }
            }
        }
    }
}

/// Keeps a usage error a usage error while saying which check it came from.
fn error_with_context(error: eyre::Report, message: &str) -> eyre::Report {
    match envelope::exit_kind(&error) {
        ExitKind::Usage => envelope::usage(message),
        _ => error,
    }
}

/// How a check ended.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum Status {
    /// Measured, and the comparison holds.
    Pass,
    /// Measured, and the comparison does not hold.
    Fail,
    /// Could not be measured.
    Error,
}

/// The result of one check.
#[derive(Debug, Clone, PartialEq, Serialize, JsonSchema)]
pub struct CheckResult {
    /// The check as written.
    pub check: String,
    /// `pass`, `fail` or `error`.
    pub status: Status,
    /// Whether the check passed; `false` for `fail` and for `error`.
    pub passed: bool,
    /// The number that was measured; `null` for `error`.
    #[schemars(with = "Option<f64>")]
    pub observed: Value,
    /// For a failure, what is behind the number; for an error, why it could not be measured.
    pub detail: Option<String>,
    /// For an error, the kind of failure (`unusable`, `not-found`, `usage`, `budget`).
    pub error_kind: Option<ExitKind>,
}

/// The `data` of `assert --json`.
#[derive(Debug, Clone, PartialEq, Serialize, JsonSchema)]
pub struct AssertData {
    /// Whether every check passed.
    pub passed: bool,
    /// How many checks failed.
    pub failed: usize,
    /// How many checks could not be measured.
    pub errors: usize,
    /// One result per check, in the order they were given.
    pub checks: Vec<CheckResult>,
}

impl AssertData {
    /// The exit kind of the run: [`ExitKind::Ok`] if everything passed, else the kind of the first
    /// check that could not be measured, else [`ExitKind::Assertion`].
    #[must_use]
    pub fn exit_kind(&self) -> ExitKind {
        if let Some(kind) = self.checks.iter().find_map(|check| check.error_kind) {
            kind
        } else if self.failed > 0 {
            ExitKind::Assertion
        } else {
            ExitKind::Ok
        }
    }

    /// A sentence for the summary line and an envelope's error.
    #[must_use]
    pub fn summary(&self) -> String {
        let total = self.checks.len();
        if self.passed {
            return format!("all {total} check(s) passed");
        }
        let mut parts = Vec::new();
        if self.failed > 0 {
            parts.push(format!("{} failed", self.failed));
        }
        if self.errors > 0 {
            parts.push(format!("{} could not be checked", self.errors));
        }
        format!(
            "{} of {total} check(s): {}",
            self.failed + self.errors,
            parts.join(", ")
        )
    }

    /// One line per check (`PASS`, `FAIL` or `ERROR`, the check, what was observed), the detail of
    /// a failure under it, and a summary line.
    #[must_use]
    pub fn to_text(&self) -> String {
        let mut out = String::new();
        for check in &self.checks {
            match check.status {
                Status::Pass | Status::Fail => {
                    let word = if check.passed { "PASS" } else { "FAIL" };
                    let observed = match &check.observed {
                        Value::String(text) => text.clone(),
                        other => other.to_string(),
                    };
                    let _ = writeln!(out, "{word} {} (observed {observed})", check.check);
                    if let Some(detail) = &check.detail {
                        for line in detail.lines() {
                            let _ = writeln!(out, "     {line}");
                        }
                    }
                }
                Status::Error => {
                    let _ = writeln!(
                        out,
                        "ERROR {}: {}",
                        check.check,
                        check.detail.as_deref().unwrap_or("could not be checked")
                    );
                }
            }
        }
        let _ = writeln!(out, "{}", self.summary());
        out
    }
}

/// Reads every check, then runs them all against `analysis`.
///
/// # Errors
///
/// Returns a usage error, naming the check, if any check is malformed: nothing is measured then.
/// A check that cannot be measured is not an error here; it is [`Status::Error`] in the result.
pub fn run_all(analysis: &Analysis, checks: &[String]) -> eyre::Result<AssertData> {
    let parsed = parse_all(checks)?;
    Ok(run_parsed(analysis, &parsed))
}

/// Reads every check without running any.
///
/// # Errors
///
/// As [`Assertion::parse`], and a usage error if `checks` is empty.
pub fn parse_all(checks: &[String]) -> eyre::Result<Vec<Assertion>> {
    if checks.is_empty() {
        return Err(envelope::usage(format!(
            "assert needs at least one --check; the kinds are:\n{}",
            kinds_listing()
        )));
    }
    checks.iter().map(|text| Assertion::parse(text)).collect()
}

/// Runs checks that were read.
#[must_use]
pub fn run_parsed(analysis: &Analysis, checks: &[Assertion]) -> AssertData {
    let checks: Vec<CheckResult> = checks.iter().map(|check| check.run(analysis)).collect();
    let failed = checks
        .iter()
        .filter(|check| check.status == Status::Fail)
        .count();
    let errors = checks
        .iter()
        .filter(|check| check.status == Status::Error)
        .count();
    AssertData {
        passed: failed == 0 && errors == 0,
        failed,
        errors,
        checks,
    }
}

// ---- findings -------------------------------------------------------------------------------

/// `findings kind=K severity=S count<=N`: how many findings (detectors' and an agent's) match.
#[derive(Debug)]
pub struct FindingsCheck {
    kind: Option<String>,
    severity: Option<Severity>,
    comparison: Comparison,
}

impl Check for FindingsCheck {
    const NAME: &'static str = "findings";
    const ABOUT: &'static str = "findings [kind=K] [severity=error|warn|info] count<op>N   (K: log-error, invariant, invariant:<kind>, agent-finding, ...)";

    fn parse(args: &CheckArgs) -> eyre::Result<Self> {
        args.comparison.require(Self::NAME, &["count"])?;
        let pairs = args.pairs(&["kind", "severity"])?;
        let severity = pairs
            .get("severity")
            .map(|text| match text.as_str() {
                "error" => Ok(Severity::Error),
                "warn" => Ok(Severity::Warn),
                "info" => Ok(Severity::Info),
                other => Err(envelope::usage(format!(
                    "severity is error, warn or info, not `{other}`"
                ))),
            })
            .transpose()?;
        Ok(Self {
            kind: pairs.get("kind").cloned(),
            severity,
            comparison: args.comparison.clone(),
        })
    }

    fn run(&self, analysis: &Analysis) -> eyre::Result<CheckOutcome> {
        let data = analysis.merged_findings(&FindingsQuery {
            kind: self.kind.clone(),
            severity: self.severity,
            limit: None,
        })?;
        Ok(CheckOutcome::compare(
            &self.comparison,
            data.total as f64,
            || {
                let mut lines: Vec<String> = data
                    .findings
                    .iter()
                    .take(5)
                    .map(|item| {
                        format!(
                            "{} {} at {}: {}",
                            item.id,
                            item.kind,
                            timecode::format(item.start_video_s),
                            item.summary
                        )
                    })
                    .collect();
                if data.total > lines.len() {
                    lines.push(format!("... {} more", data.total - lines.len()));
                }
                Some(lines.join("\n"))
            },
        ))
    }
}

// ---- log ------------------------------------------------------------------------------------

/// `log "<logq query>" count==N`: how many lines of komorebi's log match.
#[derive(Debug)]
pub struct LogCheck {
    query: String,
    comparison: Comparison,
}

impl Check for LogCheck {
    const NAME: &'static str = "log";
    const ABOUT: &'static str = "log \"<logq query>\" count<op>N   (the query is the `logq` language: level=ERROR and message~\"ghost\")";

    fn parse(args: &CheckArgs) -> eyre::Result<Self> {
        args.comparison.require(Self::NAME, &["count"])?;
        let query = unwrap_quoted(&args.text);
        if query.is_empty() {
            return Err(envelope::usage("a `log` check needs a query"));
        }
        Ok(Self {
            query,
            comparison: args.comparison.clone(),
        })
    }

    fn run(&self, analysis: &Analysis) -> eyre::Result<CheckOutcome> {
        let budget = Budget::unlimited().reading_at_most(512 * 1024 * 1024);
        let query = |limit: Option<usize>, count_only: bool| LogqQuery {
            query: self.query.clone(),
            from_mach_ns: None,
            to_mach_ns: None,
            group_by: None,
            skip: 0,
            limit,
            count_only,
        };

        let data = analysis.logq(&query(None, true), &budget)?;
        let mut outcome = CheckOutcome::compare(&self.comparison, data.matching as f64, || None);
        if !outcome.passed && data.matching > 0 {
            let shown = analysis.logq(&query(Some(3), false), &budget)?;
            let mut lines: Vec<String> = shown
                .hits
                .iter()
                .map(|hit| {
                    format!(
                        "{} {}: {}",
                        hit.reference,
                        timecode::format(hit.video_s),
                        hit.text
                    )
                })
                .collect();
            if data.matching > lines.len() {
                lines.push(format!("... {} more", data.matching - lines.len()));
            }
            outcome.detail = Some(lines.join("\n"));
        }
        Ok(outcome)
    }
}

// ---- annotations ----------------------------------------------------------------------------

/// `annotations kind=note count>=N`: how many bug regions, notes or findings were recorded.
#[derive(Debug)]
pub struct AnnotationsCheck {
    kind: Option<annotations::Kind>,
    comparison: Comparison,
}

impl Check for AnnotationsCheck {
    const NAME: &'static str = "annotations";
    const ABOUT: &'static str = "annotations [kind=note|bug|finding] count<op>N";

    fn parse(args: &CheckArgs) -> eyre::Result<Self> {
        args.comparison.require(Self::NAME, &["count"])?;
        let pairs = args.pairs(&["kind"])?;
        let kind = pairs
            .get("kind")
            .map(|text| match text.as_str() {
                "note" => Ok(annotations::Kind::Note),
                "bug" => Ok(annotations::Kind::Bug),
                "finding" => Ok(annotations::Kind::Finding),
                other => Err(envelope::usage(format!(
                    "annotation kind is note, bug or finding, not `{other}`"
                ))),
            })
            .transpose()?;
        Ok(Self {
            kind,
            comparison: args.comparison.clone(),
        })
    }

    fn run(&self, analysis: &Analysis) -> eyre::Result<CheckOutcome> {
        let loaded = annotations::load(&analysis.session.dir)?;
        let count = loaded
            .annotations
            .iter()
            .filter(|annotation| self.kind.is_none_or(|kind| annotation.kind == kind))
            .count();
        Ok(CheckOutcome::compare(
            &self.comparison,
            count as f64,
            || None,
        ))
    }
}

// ---- video: motion and blank ----------------------------------------------------------------

/// The part of a video check that picks what to scan.
#[derive(Debug)]
struct Scan {
    crop: Option<Crop>,
    from: Option<String>,
    to: Option<String>,
}

impl Scan {
    fn parse(pairs: &BTreeMap<String, String>) -> eyre::Result<Self> {
        Ok(Self {
            crop: pairs
                .get("region")
                .map(|text| Crop::parse(text))
                .transpose()?,
            from: pairs.get("from").cloned(),
            to: pairs.get("to").cloned(),
        })
    }

    /// Runs `scan` on every display with video over the range.
    fn with<T>(
        &self,
        analysis: &Analysis,
        scan: impl FnOnce(&SessionVideo, &ScanOptions) -> eyre::Result<T>,
    ) -> eyre::Result<T> {
        let video = SessionVideo::new(&analysis.manifest, analysis.guard());
        let displays = video.displays();
        if displays.is_empty() {
            return Err(envelope::unusable("the session recorded no video"));
        }

        let (_, end) = analysis.manifest.video_span();
        let moment = |text: &Option<String>, default: f64| -> eyre::Result<f64> {
            text.as_deref().map_or(Ok(default), |text| {
                analysis.resolve(text).map(|resolved| resolved.video_s)
            })
        };
        let (from_s, to_s) = (moment(&self.from, 0.0)?, moment(&self.to, end)?);
        if to_s < from_s {
            return Err(envelope::usage("the end of the range is before its start"));
        }

        scan(
            &video,
            &ScanOptions {
                displays: &displays,
                from_s,
                to_s,
                crop: self.crop,
                cancel: None,
            },
        )
    }
}

/// `motion region=x,y,w,h from=T to=T segments<=N`: how much the picture moved.
///
/// The metrics are `segments` (how many stretches of motion), `active_ms` (the time they add up
/// to), `peak_share` (the largest share of the area changing, 0 to 1) and `peak_delta` (the
/// largest mean brightness change, 0 to 255).
#[derive(Debug)]
pub struct MotionCheck {
    scan: Scan,
    comparison: Comparison,
}

impl Check for MotionCheck {
    const NAME: &'static str = "motion";
    const ABOUT: &'static str =
        "motion [region=x,y,w,h] [from=T] [to=T] segments|active_ms|peak_share|peak_delta<op>N";

    fn parse(args: &CheckArgs) -> eyre::Result<Self> {
        args.comparison.require(
            Self::NAME,
            &["segments", "active_ms", "peak_share", "peak_delta"],
        )?;
        Ok(Self {
            scan: Scan::parse(&args.pairs(&["region", "from", "to"])?)?,
            comparison: args.comparison.clone(),
        })
    }

    fn run(&self, analysis: &Analysis) -> eyre::Result<CheckOutcome> {
        let data = self.scan.with(analysis, |video, options| {
            analysis.motion(video, options, 1.0)
        })?;
        let observed = match self.comparison.metric.as_str() {
            "segments" => data.segments.len() as f64,
            "active_ms" => (data.active_s() * 1000.0).round(),
            "peak_share" => data
                .segments
                .iter()
                .map(|segment| segment.peak_share)
                .fold(0.0, f64::max),
            _ => data
                .segments
                .iter()
                .map(|segment| segment.peak_mean_delta)
                .fold(0.0, f64::max),
        };
        Ok(CheckOutcome::compare(&self.comparison, observed, || {
            let mut lines: Vec<String> = data
                .segments
                .iter()
                .take(5)
                .map(|segment| {
                    format!(
                        "display {} {} for {:.0} ms (peak share {:.2})",
                        segment.display,
                        timecode::format(segment.start_video_s),
                        segment.duration_s * 1000.0,
                        segment.peak_share
                    )
                })
                .collect();
            if data.segments.len() > lines.len() {
                lines.push(format!("... {} more", data.segments.len() - lines.len()));
            }
            (!lines.is_empty()).then(|| lines.join("\n"))
        }))
    }
}

/// `blank region=x,y,w,h from=T to=T color=white ms<=N`: whether a region went one colour.
///
/// The metrics are `ms` (the longest blank stretch), `total_ms` (all of them added up) and
/// `stretches` (how many). `color` (also spelled `colour`) keeps only stretches of that colour
/// (`white`, `black`, `gray`, or a hex code); without it any blank colour counts. A frozen region
/// is not blank.
#[derive(Debug)]
pub struct BlankCheck {
    scan: Scan,
    colour: Option<String>,
    comparison: Comparison,
}

impl Check for BlankCheck {
    const NAME: &'static str = "blank";
    const ABOUT: &'static str =
        "blank region=x,y,w,h [from=T] [to=T] [color=white] ms|total_ms|stretches<op>N";

    fn parse(args: &CheckArgs) -> eyre::Result<Self> {
        args.comparison
            .require(Self::NAME, &["ms", "total_ms", "stretches"])?;
        let pairs = args.pairs(&["region", "from", "to", "color", "colour"])?;
        if !pairs.contains_key("region") {
            return Err(envelope::usage("a `blank` check needs region=x,y,w,h"));
        }
        Ok(Self {
            scan: Scan::parse(&pairs)?,
            colour: pairs.get("color").or_else(|| pairs.get("colour")).cloned(),
            comparison: args.comparison.clone(),
        })
    }

    fn run(&self, analysis: &Analysis) -> eyre::Result<CheckOutcome> {
        let data = self.scan.with(analysis, |video, options| {
            analysis.blank(video, options, DEFAULT_VARIANCE, DEFAULT_MIN_DURATION_S)
        })?;
        let stretches: Vec<_> = data
            .stretches
            .iter()
            .filter(|stretch| stretch.kind == StretchKind::Blank)
            .filter(|stretch| {
                self.colour
                    .as_ref()
                    .is_none_or(|wanted| stretch.colour.as_ref() == Some(wanted))
            })
            .collect();
        let observed = match self.comparison.metric.as_str() {
            "ms" => stretches
                .iter()
                .map(|stretch| (stretch.duration_s * 1000.0).round())
                .fold(0.0, f64::max),
            "total_ms" => stretches
                .iter()
                .map(|stretch| stretch.duration_s * 1000.0)
                .sum::<f64>()
                .round(),
            _ => stretches.len() as f64,
        };
        Ok(CheckOutcome::compare(&self.comparison, observed, || {
            let mut lines: Vec<String> = stretches
                .iter()
                .take(5)
                .map(|stretch| {
                    format!(
                        "display {} blank ({}) at {} for {:.0} ms",
                        stretch.display,
                        stretch.colour.as_deref().unwrap_or("?"),
                        timecode::format(stretch.start_video_s),
                        stretch.duration_s * 1000.0
                    )
                })
                .collect();
            if stretches.len() > lines.len() {
                lines.push(format!("... {} more", stretches.len() - lines.len()));
            }
            (!lines.is_empty()).then(|| lines.join("\n"))
        }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::analysis::envelope::exit_kind;

    fn comparison(text: &str) -> Comparison {
        let assertion_text = format!("annotations {text}");
        let captures = TAIL.captures(&assertion_text[12..]).expect("comparison");
        Comparison {
            metric: captures[2].to_string(),
            op: Op::parse(&captures[3]).expect("op"),
            value: captures[4].parse().expect("number"),
        }
    }

    #[test]
    fn every_operator_compares() {
        let cases = [
            ("count==2", 2.0, true),
            ("count==2", 3.0, false),
            ("count!=2", 3.0, true),
            ("count!=2", 2.0, false),
            ("count<=2", 2.0, true),
            ("count<=2", 3.0, false),
            ("count>=2", 2.0, true),
            ("count>=2", 1.0, false),
            ("count<2", 1.0, true),
            ("count<2", 2.0, false),
            ("count>2", 3.0, true),
            ("count>2", 2.0, false),
        ];
        for (text, observed, holds) in cases {
            assert_eq!(
                comparison(text).holds(observed),
                holds,
                "{text} on {observed}"
            );
        }
    }

    #[test]
    fn spaces_around_the_operator_and_decimals_are_read() {
        let tail = TAIL
            .captures("region=1,2,3,4 peak_share <= 0.25")
            .expect("tail");
        assert_eq!(&tail[1], "region=1,2,3,4");
        assert_eq!(&tail[2], "peak_share");
        assert_eq!(&tail[3], "<=");
        assert_eq!(&tail[4], "0.25");
    }

    #[test]
    fn words_honour_both_quotes() {
        assert_eq!(
            words(r#"kind=a "b=c d" 'e f'="g"  "#).expect("words"),
            ["kind=a", "b=c d", "e f=g"]
        );
        assert_eq!(
            words("k=\"say \\\"hi\\\"\"").expect("words"),
            ["k=say \"hi\""]
        );
        assert!(words("k=\"open").is_err());
    }

    #[test]
    fn a_log_query_may_be_quoted_or_bare() {
        let query = |text: &str| {
            let assertion = Assertion::parse(text).expect("parse");
            assertion.text
        };
        assert_eq!(
            query("log \"level=ERROR\" count==0"),
            "log \"level=ERROR\" count==0"
        );
        assert_eq!(unwrap_quoted(r#""level=ERROR""#), "level=ERROR");
        assert_eq!(unwrap_quoted("'message~\"ghost\"'"), "message~\"ghost\"");
        assert_eq!(unwrap_quoted(r#""a\"b""#), "a\"b");
        // quotes inside a query that does not start with one are kept: they are the query's
        assert_eq!(
            unwrap_quoted(r#"level=ERROR and message~"ghost""#),
            r#"level=ERROR and message~"ghost""#
        );
        // a quote that closes before the end does not wrap the whole text
        assert_eq!(
            unwrap_quoted(r#""a" or message~"b""#),
            r#""a" or message~"b""#
        );
    }

    #[test]
    fn malformed_checks_are_usage_errors_that_list_the_kinds() {
        for text in [
            "",
            "nonsense count==1",
            "findings",
            "findings count",
            "findings count=1",
            "findings count===1",
            "findings count<=x",
            "findings segments<=1",
            "findings kind count<=1",
            "findings colour=red count<=1",
            "findings severity=loud count<=1",
            "findings kind=a kind=b count<=1",
            "log count==0",
            "motion region=1,2,3 segments==0",
            "blank segments==0",
            "blank region=1,2,3,4 count==0",
            "annotations kind=memo count==0",
        ] {
            let error = Assertion::parse(text)
                .err()
                .unwrap_or_else(|| panic!("{text:?}"));
            assert_eq!(exit_kind(&error), ExitKind::Usage, "{text:?}: {error}");
        }

        let error = Assertion::parse("nonsense count==1").err().expect("error");
        let message = error.to_string();
        for kind in KINDS {
            assert!(message.contains(kind.about), "{message}");
        }
    }

    #[test]
    fn well_formed_checks_of_every_kind_parse() {
        for text in [
            "findings kind=layout-mismatch severity=error count<=0",
            "findings count>=1",
            "log \"level=ERROR\" count==0",
            "log level=ERROR and message~\"ghost\" count<3",
            "motion segments<=1",
            "motion region=0,0,100,100 from=1.5 to=event:3 active_ms>200",
            "motion peak_share<0.2",
            "motion peak_delta<=12.5",
            "blank region=10,10,300,200 from=bug:a-1 color=white ms<=100",
            "blank region=0,0,5,5 total_ms==0",
            "annotations kind=note count>=2",
            "annotations count!=0",
            "  findings   count<=0  ",
        ] {
            Assertion::parse(text).unwrap_or_else(|error| panic!("{text:?}: {error}"));
        }
    }

    #[test]
    fn numbers_print_without_noise() {
        assert_eq!(number_text(2.0), "2");
        assert_eq!(number_text(0.25), "0.25");
        assert_eq!(number(3.0), serde_json::json!(3));
        assert_eq!(number(0.5), serde_json::json!(0.5));
        assert_eq!(comparison("count<=0").to_string(), "count<=0");
    }

    #[test]
    fn errors_are_never_a_pass() {
        let data = AssertData {
            passed: false,
            failed: 0,
            errors: 1,
            checks: vec![CheckResult {
                check: "motion segments<=1".to_string(),
                status: Status::Error,
                passed: false,
                observed: Value::Null,
                detail: Some("the session recorded no video".to_string()),
                error_kind: Some(ExitKind::Unusable),
            }],
        };
        assert_eq!(data.exit_kind(), ExitKind::Unusable);
        assert!(
            data.to_text()
                .starts_with("ERROR motion segments<=1: the session recorded no video")
        );
        assert!(data.summary().contains("could not be checked"));
    }
}
