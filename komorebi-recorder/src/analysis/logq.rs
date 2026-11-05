//! Structured questions about komorebi's log: which lines, how many, and counted by what.
//!
//! The log is long, interleaves threads and wraps every line in a span prefix, so an investigation
//! that only has `grep` ends up stripping colour codes and counting by hand. `logq` reads each line
//! as [`super::logparse`] does and keeps the lines that satisfy a *query*.
//!
//! # The query language
//!
//! A query is predicates joined by `and` and `or` (`and` binds tighter; parentheses group):
//!
//! ```text
//! level=ERROR
//! message~"ghost|orphan" and hwnd=7804828
//! command=MoveWindow and (level=WARN or level=ERROR)
//! ```
//!
//! A predicate is `field op value`, where `op` is one of
//!
//! | op | holds when |
//! |---|---|
//! | `=` | the field equals the value (case-sensitive, except `level`) |
//! | `!=` | the field does not equal the value, or the line has no such field |
//! | `~` | the regular expression matches somewhere in the field (case-insensitive; start it with `(?-i)` for case-sensitive) |
//! | `!~` | the regular expression does not match, or the line has no such field |
//!
//! A value is a bare word (no spaces, no `)`), or `"quoted"` with `\"` and `\\` for a quote and a
//! backslash. Two numbers compare as numbers, so `hwnd=7804828` and `hwnd=0x77179C` are the same
//! window. A field with several values (the same field on two spans) matches `=` and `~` if any of
//! them does.
//!
//! | field | is |
//! |---|---|
//! | `level` | `ERROR`, `WARN`, `INFO`, `DEBUG` or `TRACE` (case does not matter) |
//! | `target` | the module that logged, `komorebi::window_manager` |
//! | `message` | the message, without the span chain |
//! | `line` | the line number in `komorebi.log` |
//! | `spans` | the span chain as written, `process_command{MoveWindow(Right)}:move_container_in_direction{direction=Right}` |
//! | `command` | the command a `process_command{X(..)}` span is handling: `MoveWindow` |
//! | `event` | the `event` field of a span, or else the event in a `processing event: X` message |
//! | anything else | the field of that name on any span: `hwnd`, `winevent`, `direction`, `idx`, ... |
//!
//! A continuation line (a panic message, a backtrace) belongs to the line before it and has only a
//! `message`, a `line` and the time of the line before it.
//!
//! [`Analysis::logq`] answers a [`LogqQuery`] with [`LogqData`]. Each hit has a `reference`
//! (`log:345`) that `at`, `trace`, `code` and `annotate --ref` accept.
//!
//! # Example
//!
//! ```rust
//! use komorebi_recorder::analysis::logparse::parse_line;
//! use komorebi_recorder::analysis::logq::Query;
//!
//! let query = Query::parse("command=MoveWindow and hwnd=0x10 and nothing!=x")?;
//! let line = parse_line(
//!     1,
//!     "2026-10-28T15:46:20.088347Z  INFO process_command{MoveWindow(Right)}:move_it{hwnd=16}: komorebi::w: moving",
//!     None,
//!     false,
//! );
//! assert!(query.matches(&line));
//!
//! // a bad query is a usage error whose hint names the fields
//! let error = Query::parse("level ERROR").unwrap_err();
//! assert!(format!("{error:?}").contains("command"));
//! # Ok::<(), Box<dyn std::error::Error>>(())
//! ```

use super::Analysis;
use super::budget::Budget;
use super::envelope;
use super::logparse::Level;
use super::logparse::Line;
use crate::timecode;
use color_eyre::eyre;
use regex::Regex;
use regex::RegexBuilder;
use schemars::JsonSchema;
use serde::Serialize;
use std::collections::HashMap;
use std::collections::HashSet;
use std::fmt::Write;

/// The schema name that identifies a [`LogqData`] in an envelope (`logq/1`); listed by the `schema`
/// command.
pub const SCHEMA_LOGQ: &str = "logq/1";

const FIELDS_HINT: &str = "write predicates joined by `and`/`or`, e.g. `level=ERROR and message~\"ghost\"`; \
operators are =, !=, ~ and !~; fields are level, target, message, line, spans, command, event, or \
any span field such as hwnd, winevent or direction";

#[derive(Debug, Clone, PartialEq, Eq)]
enum Field {
    Level,
    Target,
    Message,
    Line,
    Spans,
    Command,
    Event,
    Span(String),
}

impl Field {
    fn new(name: &str) -> Self {
        match name {
            "level" => Self::Level,
            "target" => Self::Target,
            "message" => Self::Message,
            "line" => Self::Line,
            "spans" => Self::Spans,
            "command" => Self::Command,
            "event" => Self::Event,
            other => Self::Span(other.to_string()),
        }
    }

    fn name(&self) -> &str {
        match self {
            Self::Level => "level",
            Self::Target => "target",
            Self::Message => "message",
            Self::Line => "line",
            Self::Spans => "spans",
            Self::Command => "command",
            Self::Event => "event",
            Self::Span(name) => name,
        }
    }

    /// Every value this field has on `line`: none, one, or several for a span field that occurs on
    /// more than one span.
    fn values(&self, line: &Line) -> Vec<String> {
        match self {
            Self::Level => line
                .level
                .map(|level| level_text(level).to_string())
                .into_iter()
                .collect(),
            Self::Target => line.target.clone().into_iter().collect(),
            Self::Message => vec![line.message.clone()],
            Self::Line => vec![line.number.to_string()],
            Self::Spans => {
                if line.chain.is_empty() {
                    Vec::new()
                } else {
                    vec![line.chain.clone()]
                }
            }
            Self::Command => command_of(line).into_iter().collect(),
            Self::Event => {
                let mut values = span_values(line, "event");
                if values.is_empty() {
                    values.extend(after_marker(&line.message, "processing event: "));
                }
                values
            }
            Self::Span(name) => {
                let mut values = span_values(line, name);
                if name == "hwnd" {
                    // a window is decimal in a span and hex in many messages
                    let hex: Vec<String> = values
                        .iter()
                        .filter_map(|value| as_number(value))
                        .map(|number| format!("0x{number:x}"))
                        .collect();
                    values.extend(hex);
                }
                values
            }
        }
    }
}

fn level_text(level: Level) -> &'static str {
    match level {
        Level::Error => "ERROR",
        Level::Warn => "WARN",
        Level::Info => "INFO",
        Level::Debug => "DEBUG",
        Level::Trace => "TRACE",
    }
}

fn span_values(line: &Line, name: &str) -> Vec<String> {
    line.spans
        .iter()
        .flat_map(|span| span.fields.iter())
        .filter(|(key, _)| key == name)
        .map(|(_, value)| value.clone())
        .collect()
}

fn word(text: &str) -> Option<String> {
    let end = text
        .find(|c: char| !(c.is_alphanumeric() || c == '_'))
        .unwrap_or(text.len());
    (end > 0).then(|| text[..end].to_string())
}

fn after_marker(text: &str, marker: &str) -> Option<String> {
    text.split_once(marker).and_then(|(_, rest)| word(rest))
}

/// The command a `process_command{X(..)}` span handles: `MoveWindow` for
/// `process_command{MoveWindow(Right)}`.
fn command_of(line: &Line) -> Option<String> {
    const OPEN: &str = "process_command{";
    line.chain
        .find(OPEN)
        .and_then(|at| word(&line.chain[at + OPEN.len()..]))
        .or_else(|| after_marker(&line.message, "processing command: "))
}

/// A decimal or `0x` hexadecimal number.
fn as_number(text: &str) -> Option<u64> {
    let text = text.trim();
    match text.strip_prefix("0x").or_else(|| text.strip_prefix("0X")) {
        Some(hex) => u64::from_str_radix(hex, 16).ok(),
        None => text.parse().ok(),
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Op {
    Eq,
    Ne,
    Match,
    NotMatch,
}

#[derive(Debug, Clone)]
struct Predicate {
    field: Field,
    op: Op,
    value: String,
    regex: Option<Regex>,
}

impl Predicate {
    fn equals(&self, candidate: &str) -> bool {
        if self.field == Field::Level {
            return candidate.eq_ignore_ascii_case(&self.value);
        }
        candidate == self.value
            || matches!((as_number(candidate), as_number(&self.value)), (Some(a), Some(b)) if a == b)
    }

    fn holds(&self, line: &Line) -> bool {
        let values = self.field.values(line);
        let regex = self.regex.as_ref();
        match self.op {
            Op::Eq => values.iter().any(|value| self.equals(value)),
            Op::Ne => !values.iter().any(|value| self.equals(value)),
            Op::Match => regex.is_some_and(|re| values.iter().any(|value| re.is_match(value))),
            Op::NotMatch => regex.is_none_or(|re| !values.iter().any(|value| re.is_match(value))),
        }
    }
}

#[derive(Debug, Clone)]
enum Expr {
    Pred(Predicate),
    And(Vec<Expr>),
    Or(Vec<Expr>),
}

impl Expr {
    fn holds(&self, line: &Line) -> bool {
        match self {
            Self::Pred(predicate) => predicate.holds(line),
            Self::And(all) => all.iter().all(|expr| expr.holds(line)),
            Self::Or(any) => any.iter().any(|expr| expr.holds(line)),
        }
    }

    fn fields<'a>(&'a self, out: &mut Vec<&'a Field>) {
        match self {
            Self::Pred(predicate) => out.push(&predicate.field),
            Self::And(items) | Self::Or(items) => items.iter().for_each(|item| item.fields(out)),
        }
    }
}

/// A parsed query: [`Query::parse`] reads it once and [`Query::matches`] tests a line.
///
/// See the [module](self) for the language.
#[derive(Debug, Clone)]
pub struct Query {
    text: String,
    expr: Expr,
}

fn bad(message: impl Into<String>) -> eyre::Report {
    envelope::with_hint(envelope::usage(message), FIELDS_HINT)
}

struct Reader<'a> {
    text: &'a str,
    at: usize,
}

impl Reader<'_> {
    fn rest(&self) -> &str {
        &self.text[self.at..]
    }

    fn next_char(&self) -> String {
        self.rest()
            .chars()
            .next()
            .map_or_else(String::new, String::from)
    }

    fn skip_space(&mut self) {
        let trimmed = self.rest().trim_start();
        self.at = self.text.len() - trimmed.len();
    }

    fn eat(&mut self, prefix: &str) -> bool {
        if self.rest().starts_with(prefix) {
            self.at += prefix.len();
            true
        } else {
            false
        }
    }

    /// Eats the keyword `and`/`or` when it is a whole word.
    fn keyword(&mut self, word: &str) -> bool {
        self.skip_space();
        let rest = self.rest();
        let Some(head) = rest.get(..word.len()) else {
            return false;
        };
        let after = rest[word.len()..].chars().next();
        if head.eq_ignore_ascii_case(word) && after.is_none_or(|c| c.is_whitespace() || c == '(') {
            self.at += word.len();
            true
        } else {
            false
        }
    }

    fn or(&mut self) -> eyre::Result<Expr> {
        let mut items = vec![self.and()?];
        while self.keyword("or") {
            items.push(self.and()?);
        }
        Ok(if items.len() == 1 {
            items.remove(0)
        } else {
            Expr::Or(items)
        })
    }

    fn and(&mut self) -> eyre::Result<Expr> {
        let mut items = vec![self.atom()?];
        while self.keyword("and") {
            items.push(self.atom()?);
        }
        Ok(if items.len() == 1 {
            items.remove(0)
        } else {
            Expr::And(items)
        })
    }

    fn atom(&mut self) -> eyre::Result<Expr> {
        self.skip_space();
        if self.eat("(") {
            let inner = self.or()?;
            self.skip_space();
            if !self.eat(")") {
                return Err(bad(format!(
                    "missing `)` at position {} of the query",
                    self.at + 1
                )));
            }
            return Ok(inner);
        }

        let name_len = self
            .rest()
            .find(|c: char| !(c.is_alphanumeric() || matches!(c, '_' | '.' | '-')))
            .unwrap_or(self.rest().len());
        if name_len == 0 {
            return Err(bad(format!(
                "expected a field name at position {} of the query, found `{}`",
                self.at + 1,
                self.next_char()
            )));
        }
        let name = self.rest()[..name_len].to_string();
        self.at += name_len;
        self.skip_space();

        let op = if self.eat("!=") {
            Op::Ne
        } else if self.eat("!~") {
            Op::NotMatch
        } else if self.eat("=") {
            Op::Eq
        } else if self.eat("~") {
            Op::Match
        } else {
            return Err(bad(format!(
                "expected one of = != ~ !~ after `{name}`; found `{}`",
                self.next_char()
            )));
        };
        self.skip_space();

        let value = self.value()?;
        let regex = if matches!(op, Op::Match | Op::NotMatch) {
            Some(
                RegexBuilder::new(&value)
                    .case_insensitive(true)
                    .build()
                    .map_err(|error| bad(format!("`{value}` is not a valid pattern: {error}")))?,
            )
        } else {
            None
        };

        Ok(Expr::Pred(Predicate {
            field: Field::new(&name),
            op,
            value,
            regex,
        }))
    }

    fn value(&mut self) -> eyre::Result<String> {
        if self.eat("\"") {
            let mut value = String::new();
            let mut chars = self.rest().char_indices();
            while let Some((index, c)) = chars.next() {
                match c {
                    '\\' => match chars.next() {
                        Some((_, escaped @ ('"' | '\\'))) => value.push(escaped),
                        Some((_, other)) => {
                            value.push('\\');
                            value.push(other);
                        }
                        None => break,
                    },
                    '"' => {
                        self.at += index + 1;
                        return Ok(value);
                    }
                    other => value.push(other),
                }
            }
            return Err(bad("a quoted value is missing its closing `\"`"));
        }

        let end = self
            .rest()
            .find(|c: char| c.is_whitespace() || c == ')')
            .unwrap_or(self.rest().len());
        if end == 0 {
            return Err(bad(format!(
                "expected a value at position {} of the query",
                self.at + 1
            )));
        }
        let value = self.rest()[..end].to_string();
        self.at += end;
        Ok(value)
    }
}

impl Query {
    /// Reads a query.
    ///
    /// # Errors
    ///
    /// Returns a usage error, with a hint that names the valid fields, if the query is empty, is
    /// not `field op value` predicates joined by `and`/`or`, or has an invalid regular expression.
    pub fn parse(text: &str) -> eyre::Result<Self> {
        if text.trim().is_empty() {
            return Err(bad("the query is empty"));
        }

        let mut reader = Reader { text, at: 0 };
        let expr = reader.or()?;
        reader.skip_space();
        if !reader.rest().is_empty() {
            return Err(bad(format!(
                "unexpected `{}` at position {}: join predicates with `and` or `or`",
                reader.rest(),
                reader.at + 1
            )));
        }

        Ok(Self {
            text: text.to_string(),
            expr,
        })
    }

    /// Returns whether `line` satisfies the query.
    #[must_use]
    pub fn matches(&self, line: &Line) -> bool {
        self.expr.holds(line)
    }

    /// The query as it was written.
    #[must_use]
    pub fn text(&self) -> &str {
        &self.text
    }

    /// The span fields the query names (not the built-in ones), without repeats.
    fn span_fields(&self) -> Vec<&str> {
        let mut fields = Vec::new();
        self.expr.fields(&mut fields);
        let mut names: Vec<&str> = fields
            .into_iter()
            .filter_map(|field| match field {
                Field::Span(name) => Some(name.as_str()),
                _ => None,
            })
            .collect();
        names.sort_unstable();
        names.dedup();
        names
    }
}

/// What to ask of the log.
#[derive(Debug, Clone, Default)]
pub struct LogqQuery {
    /// The query (see the [module](self)).
    pub query: String,
    /// Ignore lines earlier than this, in nanoseconds on the host's master monotonic clock.
    pub from_mach_ns: Option<u64>,
    /// Ignore lines later than this, in nanoseconds on the host's master monotonic clock.
    pub to_mach_ns: Option<u64>,
    /// Count the lines by the distinct values of this field (any field of the language).
    pub group_by: Option<String>,
    /// Hits (or groups) already returned: this many of the first are skipped.
    pub skip: usize,
    /// The most hits (or groups) to return; `None` for all of them.
    pub limit: Option<usize>,
    /// Return no hits and no groups, only [`LogqData::matching`].
    pub count_only: bool,
}

/// One matching line of the log, with a reference other commands accept.
#[derive(Debug, Clone, PartialEq, Serialize, JsonSchema)]
pub struct LogHit {
    /// Always `komorebi-log`, as for a log hit of `search`.
    pub source: String,
    /// When it was logged, in seconds of video time.
    pub video_s: f64,
    /// When it was logged, in nanoseconds on the host's master monotonic clock.
    pub mach_ns: u64,
    /// `log:<line>`, for `at`, `trace`, `code` and `annotate --ref`.
    pub reference: String,
    /// The line's level; absent for a continuation or a line with no level.
    pub level: Option<Level>,
    /// The span chain as written; empty if the line was logged outside any span.
    pub spans: String,
    /// The module that logged.
    pub target: Option<String>,
    /// The message.
    pub text: String,
}

/// How many matching lines had one value of the `group_by` field.
#[derive(Debug, Clone, PartialEq, Serialize, JsonSchema)]
pub struct Group {
    /// The value as the log wrote it (the first, for a field that is on several spans).
    pub value: String,
    /// How many matching lines have it.
    pub count: usize,
}

/// The answer to a [`LogqQuery`]: the `data` of the `logq` command.
#[derive(Debug, Clone, PartialEq, Serialize, JsonSchema)]
pub struct LogqData {
    /// The query as given.
    pub query: String,
    /// How many lines matched in all (the number `--count` prints), whatever the skip and limit.
    pub matching: usize,
    /// The matching lines of this page, in log order. Empty when only counting or grouping.
    pub hits: Vec<LogHit>,
    /// The field counted by, if grouping.
    pub group_by: Option<String>,
    /// The values of the field of this page, the most common first, ties in value order. Empty
    /// unless grouping.
    pub groups: Vec<Group>,
    /// How many distinct values there were, including any not returned.
    pub distinct: usize,
    /// When grouping, how many matching lines had no value for the field.
    pub without_value: usize,
    /// Things worth knowing about the answer, such as a field name that no line has.
    pub notes: Vec<String>,
}

impl Analysis {
    /// Runs a [`LogqQuery`] over the session's `komorebi.log`.
    ///
    /// Lines are tested in file order. A line that cannot be placed on the master clock never
    /// matches, because it could not be given a time or put in a range. `budget` is charged the
    /// size of every line read.
    ///
    /// # Errors
    ///
    /// Returns an error if the query is invalid (the usage exit kind), `group_by` is not a field
    /// name, or `budget` runs out (the budget exit kind).
    ///
    /// # Example
    ///
    /// ```rust
    /// use komorebi_recorder::analysis::Analysis;
    /// use komorebi_recorder::analysis::budget::Budget;
    /// use komorebi_recorder::analysis::logq::LogqQuery;
    /// use komorebi_recorder::testkit::SessionBuilder;
    ///
    /// let (session, _) = SessionBuilder::new("doc-logq")
    ///     .log(1.0, "INFO", "process_command{MoveWindow(Right)}:move_it{hwnd=16}: komorebi::w: moving")
    ///     .log(2.0, "WARN", "process_event{event=\"Show\" hwnd=16}: komorebi::e: ghost tile")
    ///     .log(3.0, "WARN", "komorebi::e: nothing to see")
    ///     .build();
    /// let analysis = Analysis::open(&session.dir)?;
    ///
    /// let found = analysis.logq(
    ///     &LogqQuery { query: "hwnd=0x10".to_string(), ..LogqQuery::default() },
    ///     &Budget::unlimited(),
    /// )?;
    /// assert_eq!(found.matching, 2);
    /// assert_eq!(found.hits[1].reference, "log:2");
    ///
    /// let grouped = analysis.logq(
    ///     &LogqQuery {
    ///         query: "level=warn".to_string(),
    ///         group_by: Some("target".to_string()),
    ///         ..LogqQuery::default()
    ///     },
    ///     &Budget::unlimited(),
    /// )?;
    /// assert_eq!(grouped.groups[0].value, "komorebi::e");
    /// assert_eq!(grouped.groups[0].count, 2);
    /// # Ok::<(), Box<dyn std::error::Error>>(())
    /// ```
    pub fn logq(&self, query: &LogqQuery, budget: &Budget) -> eyre::Result<LogqData> {
        let parsed = Query::parse(&query.query)?;
        let group_field = query
            .group_by
            .as_deref()
            .map(|name| {
                if name.is_empty() || name.contains(char::is_whitespace) {
                    Err(bad(format!("`{name}` is not a field to group by")))
                } else {
                    Ok(Field::new(name))
                }
            })
            .transpose()?;

        let lines = &self.log().lines;
        let limit = query.limit.unwrap_or(usize::MAX);
        let mut hits: Vec<LogHit> = Vec::new();
        let mut matching = 0_usize;
        let mut counts: HashMap<String, usize> = HashMap::new();
        let mut without_value = 0_usize;
        let names = parsed.span_fields();
        let mut seen_names: HashSet<&str> = HashSet::new();

        for line in lines {
            budget
                .charge((line.message.len() + line.chain.len() + 32) as u64)
                .map_err(|exceeded| {
                    envelope::over_budget(format!(
                        "the query {exceeded}; narrow it with --from/--to"
                    ))
                })?;

            for name in &names {
                if !seen_names.contains(name)
                    && line
                        .spans
                        .iter()
                        .any(|span| span.fields.iter().any(|(key, _)| key == name))
                {
                    seen_names.insert(name);
                }
            }

            let Some(mach_ns) = line
                .wall_us
                .and_then(|wall_us| self.clock().wall_to_mach(wall_us))
            else {
                continue;
            };
            if query.from_mach_ns.is_some_and(|from| mach_ns < from)
                || query.to_mach_ns.is_some_and(|to| mach_ns > to)
                || !parsed.matches(line)
            {
                continue;
            }

            matching += 1;
            if let Some(field) = &group_field {
                match field.values(line).into_iter().next() {
                    Some(value) => *counts.entry(value).or_default() += 1,
                    None => without_value += 1,
                }
            } else if !query.count_only && matching > query.skip && hits.len() < limit {
                hits.push(LogHit {
                    source: "komorebi-log".to_string(),
                    video_s: self.video_s(mach_ns),
                    mach_ns,
                    reference: format!("log:{}", line.number),
                    level: line.level,
                    spans: line.chain.clone(),
                    target: line.target.clone(),
                    text: line.message.clone(),
                });
            }
        }

        let mut notes = Vec::new();
        if matching == 0 {
            for name in &names {
                if !seen_names.contains(name) {
                    notes.push(format!(
                        "no line of the log has a span field called `{name}`; check the spelling (`logq <session> 'spans~{name}'` finds it in a span chain)"
                    ));
                }
            }
        }

        let mut groups: Vec<Group> = counts
            .into_iter()
            .map(|(value, count)| Group { value, count })
            .collect();
        groups.sort_by(|a, b| b.count.cmp(&a.count).then_with(|| a.value.cmp(&b.value)));
        let distinct = groups.len();
        if query.count_only {
            groups.clear();
        } else {
            groups.drain(..query.skip.min(groups.len()));
            groups.truncate(limit);
        }

        Ok(LogqData {
            query: query.query.clone(),
            matching,
            hits,
            group_by: group_field.map(|field| field.name().to_string()),
            groups,
            distinct,
            without_value,
            notes,
        })
    }
}

/// Returns `text` cut to `max` characters, with `…` if it was cut.
fn trimmed(text: &str, max: usize) -> String {
    if text.chars().count() <= max {
        return text.to_string();
    }
    let mut cut: String = text.chars().take(max.saturating_sub(1)).collect();
    cut.push('…');
    cut
}

/// Renders a [`LogqData`] as the text `logq` prints.
///
/// A hit is a line, `log:345  00:03.389  INFO   message`; with `spans` the span chain, cut to 100
/// characters, follows the level. A count is the bare number, and groups are `count  value` lines.
///
/// # Example
///
/// ```rust
/// use komorebi_recorder::analysis::logparse::Level;
/// use komorebi_recorder::analysis::logq::{LogHit, LogqData, logq_text};
///
/// let data = LogqData {
///     query: "level=INFO".to_string(),
///     matching: 1,
///     hits: vec![LogHit {
///         source: "komorebi-log".to_string(),
///         video_s: 3.389,
///         mach_ns: 0,
///         reference: "log:7".to_string(),
///         level: Some(Level::Info),
///         spans: String::new(),
///         target: None,
///         text: "hello".to_string(),
///     }],
///     group_by: None,
///     groups: Vec::new(),
///     distinct: 0,
///     without_value: 0,
///     notes: Vec::new(),
/// };
/// assert_eq!(logq_text(&data, false, false), "log:7  00:03.389  INFO   hello\n");
/// assert_eq!(logq_text(&data, true, false), "1\n");
/// ```
#[must_use]
pub fn logq_text(data: &LogqData, count_only: bool, spans: bool) -> String {
    let mut out = String::new();
    if count_only {
        let _ = writeln!(out, "{}", data.matching);
    } else if let Some(field) = &data.group_by {
        let width = data
            .groups
            .iter()
            .map(|group| group.count.to_string().len())
            .max()
            .unwrap_or(1);
        for group in &data.groups {
            let _ = writeln!(out, "{:>width$}  {}", group.count, group.value);
        }
        if data.distinct > data.groups.len() {
            let _ = writeln!(
                out,
                "... {} distinct `{field}` values in all",
                data.distinct
            );
        }
        if data.without_value > 0 {
            let _ = writeln!(out, "{:>width$}  (no `{field}`)", data.without_value);
        }
    } else {
        for hit in &data.hits {
            let level = hit.level.map_or("", level_text);
            let _ = write!(
                out,
                "{}  {}  {level:<5}  ",
                hit.reference,
                timecode::format(hit.video_s)
            );
            if spans && !hit.spans.is_empty() {
                let _ = write!(out, "{}  ", trimmed(&hit.spans, 100));
            }
            let _ = writeln!(out, "{}", hit.text);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::analysis::logparse::parse_line;

    fn line(text: &str) -> Line {
        parse_line(
            1,
            &format!("2026-10-28T15:46:20.088347Z {text}"),
            None,
            false,
        )
    }

    fn moving() -> Line {
        line(
            " INFO process_command{MoveWindow(Right)}:move_it{direction=Right hwnd=7804828}: komorebi::window_manager: Moving the Ghost",
        )
    }

    fn holds(query: &str, line: &Line) -> bool {
        Query::parse(query).expect("a valid query").matches(line)
    }

    #[test]
    fn built_in_fields() {
        let line = moving();
        assert!(holds("level=INFO", &line));
        assert!(holds("level=info", &line), "level ignores case");
        assert!(!holds("level=WARN", &line));
        assert!(holds("target=komorebi::window_manager", &line));
        assert!(holds("target~window_", &line));
        assert!(holds("message=\"Moving the Ghost\"", &line));
        assert!(
            !holds("message=\"moving the ghost\"", &line),
            "= is case-sensitive"
        );
        assert!(holds("message~\"moving THE ghost\"", &line), "~ is not");
        assert!(!holds("message~\"(?-i)moving\"", &line));
        assert!(holds("line=1", &line));
        assert!(holds(
            "spans~\"^process_command\\{MoveWindow\\(Right\\)\\}:move_it\"",
            &line
        ));
        assert!(holds("command=MoveWindow", &line));
        assert!(!holds("command=Move", &line));
        assert!(holds("direction=Right", &line));
        assert!(holds("nothing!=1", &line), "no such field is not equal");
        assert!(!holds("nothing=1", &line));
        assert!(!holds("nothing~1", &line));
        assert!(holds("nothing!~1", &line));
    }

    #[test]
    fn hwnd_is_the_same_in_decimal_and_hex() {
        let line = moving();
        assert!(holds("hwnd=7804828", &line));
        assert!(holds("hwnd=0x77179C", &line));
        assert!(holds("hwnd=0x77179c", &line));
        assert!(holds("hwnd~0x77179c", &line));
        assert!(!holds("hwnd=0x77179D", &line));
        assert!(holds("hwnd!=0x77179D", &line));
        assert!(!holds("hwnd!=7804828", &line));
    }

    #[test]
    fn event_and_command_come_from_a_span_or_the_message() {
        let event = line(
            " INFO process_event{event=\"FocusChange\" winevent=\"SystemForeground\" hwnd=330732}: komorebi::pe: handling",
        );
        assert!(holds(
            "event=FocusChange and winevent=SystemForeground",
            &event
        ));

        let derived =
            line(" INFO komorebi::process_event: processing event: MoveEnd for process 11");
        assert!(holds("event=MoveEnd", &derived));

        let command = line(" INFO komorebi::process_command: processing command: Retile");
        assert!(holds("command=Retile", &command));
    }

    #[test]
    fn and_or_and_parentheses() {
        let line = moving();
        assert!(holds("level=WARN or level=INFO", &line));
        assert!(holds(
            "level=INFO and direction=Right and command=MoveWindow",
            &line
        ));
        assert!(!holds("level=INFO and direction=Left", &line));
        // and binds tighter than or
        assert!(holds(
            "level=WARN and direction=Left or command=MoveWindow",
            &line
        ));
        assert!(!holds(
            "level=WARN and (direction=Left or command=MoveWindow)",
            &line
        ));
        assert!(holds(
            "(level=WARN or level=INFO) AND (command=X OR command=MoveWindow)",
            &line
        ));
        // `android` is a value here, not a keyword
        assert!(!holds("message=android", &line));
    }

    #[test]
    fn values_may_be_quoted() {
        let line = line(" WARN komorebi::x: said \"hi\" (twice)");
        assert!(holds(r#"message="said \"hi\" (twice)""#, &line));
        assert!(holds(r#"message~"\(twice\)""#, &line));
        assert!(holds("(message~twice)", &line));
    }

    #[test]
    fn a_bad_query_is_a_usage_error_naming_the_fields() {
        for bad_query in [
            "",
            "level",
            "level ERROR",
            "level=",
            "=x",
            "level=INFO message=x",
            "(level=INFO",
            "level=INFO and",
            "message~\"(\"",
            "message=\"open",
        ] {
            let error = Query::parse(bad_query).unwrap_err();
            assert_eq!(
                envelope::exit_kind(&error),
                envelope::ExitKind::Usage,
                "{bad_query:?}"
            );
            assert!(
                format!("{error:?}").contains("command"),
                "{bad_query:?} hint"
            );
        }
    }

    #[test]
    fn text_shows_groups_and_counts() {
        let data = LogqData {
            query: "x".to_string(),
            matching: 5,
            hits: Vec::new(),
            group_by: Some("level".to_string()),
            groups: vec![
                Group {
                    value: "INFO".to_string(),
                    count: 12,
                },
                Group {
                    value: "WARN".to_string(),
                    count: 3,
                },
            ],
            distinct: 3,
            without_value: 0,
            notes: Vec::new(),
        };
        assert_eq!(
            logq_text(&data, false, false),
            "12  INFO\n 3  WARN\n... 3 distinct `level` values in all\n"
        );
        assert_eq!(logq_text(&data, true, false), "5\n");
    }
}
