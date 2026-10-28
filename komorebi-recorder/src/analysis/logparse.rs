//! Reading komorebi's plaintext log as structured lines.
//!
//! komorebi logs with `tracing_subscriber`'s formatter, so a line is
//!
//! ```text
//! 2026-10-26T23:09:16.688494Z  INFO process_event:focus_window{idx=0}: komorebi::container: focusing window
//! ```
//!
//! a time, a level, the chain of *spans* the code was inside (`process_event` then `focus_window`
//! with its fields), the *target* (the module that logged), and the message. The span chain is what
//! lets a line be tied to the handler that ran: everything logged while handling one event or
//! command starts with that handler's span. Lines with no timestamp (panic messages, backtraces)
//! belong to the line before them.
//!
//! The "plaintext" log still carries ANSI escape codes inside span fields, so they are stripped
//! before parsing.
//!
//! [`parse`] reads a whole slice of a log into [`Line`]s and [`parse_line`] reads one. Reading
//! never fails: a line that does not have the expected shape is a [`Line`] with less in it (no
//! level, no spans, no target), and the text is kept in `message`. The groups of lines that belong
//! to one handler are found from the result by [`super::blocks`], and what those lines mean by the
//! flavor's rules.
//!
//! # Example
//!
//! ```rust
//! use komorebi_recorder::analysis::logparse::{Level, parse};
//!
//! let log = b"\
//! 2026-10-26T23:09:16.688494Z  INFO process_event:focus_window{idx=0}: komorebi::container: focusing window
//! thread 'main' panicked at src/x.rs:1:1:
//! ";
//! let lines = parse(log);
//! assert_eq!(lines.len(), 2);
//!
//! let line = &lines[0];
//! assert_eq!(line.level, Some(Level::Info));
//! assert_eq!(line.root_span(), Some("process_event"));
//! assert_eq!(line.field("idx"), Some("0"));
//! assert_eq!(line.target.as_deref(), Some("komorebi::container"));
//! assert_eq!(line.message, "focusing window");
//!
//! // a line with no timestamp belongs to the line before it
//! assert!(lines[1].continuation);
//! assert_eq!(lines[1].wall_us, lines[0].wall_us);
//! ```

use crate::sources::komorebi_log;
use crate::timeline::strip_ansi;
use chrono::DateTime;
use chrono::Utc;
use schemars::JsonSchema;
use serde::Serialize;

/// The level of a log line: `tracing`'s five, from the most severe.
///
/// Levels are ordered from the most severe to the most verbose, so `Level::Error < Level::Trace`,
/// and serialized in uppercase (`"ERROR"`, `"WARN"`, `"INFO"`, `"DEBUG"`, `"TRACE"`), as the log
/// spells them.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, JsonSchema)]
#[serde(rename_all = "UPPERCASE")]
pub enum Level {
    /// Something failed.
    Error,
    /// Something looked wrong but komorebi carried on.
    Warn,
    /// An ordinary event worth recording. komorebi's default level.
    Info,
    /// Detail for someone debugging komorebi.
    Debug,
    /// The finest detail: what komorebi logs only when asked to.
    Trace,
}

impl Level {
    fn parse(text: &str) -> Option<Self> {
        match text {
            "ERROR" => Some(Self::Error),
            "WARN" => Some(Self::Warn),
            "INFO" => Some(Self::Info),
            "DEBUG" => Some(Self::Debug),
            "TRACE" => Some(Self::Trace),
            _ => None,
        }
    }
}

/// One span the code was inside when it logged.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, JsonSchema)]
pub struct Span {
    /// The span's name: `process_event`, `focus_window`.
    pub name: String,
    /// The span's fields as `(name, value)` pairs, in the order the log has them. A value is as the
    /// log prints it, with ANSI escapes removed and the quotes around a string value taken off;
    /// escapes inside the quotes are not decoded.
    pub fields: Vec<(String, String)>,
}

/// One line of the recorded log.
#[derive(Debug, Clone, PartialEq, Serialize, JsonSchema)]
pub struct Line {
    /// 1-based line number in the session's `komorebi.log`.
    pub number: u64,
    /// The line's wall-clock time in microseconds since the epoch, or the previous line's for a
    /// continuation.
    pub wall_us: Option<i64>,
    /// No timestamp of its own: a panic message or backtrace frame.
    pub continuation: bool,
    /// The line's level. `None` for a continuation, and for a line whose level is not one of the
    /// five.
    pub level: Option<Level>,
    /// The spans the code was inside, outermost first. Empty for a line logged outside any span,
    /// such as one from another thread.
    pub spans: Vec<Span>,
    /// The span chain as the log wrote it, ANSI removed: `process_command{MoveWindow(Right)}:move_container_in_direction{direction=Right}`.
    /// Unlike [`Line::spans`] it keeps a span's braces exactly, so a span that is not `key=value`
    /// fields (the argument of `process_command`) is still there. Empty when `spans` is. Not part
    /// of the serialized line.
    #[serde(skip)]
    #[schemars(skip)]
    pub chain: String,
    /// The module that logged, e.g. `komorebi::container`. `None` for a continuation and for a line
    /// in which none was found.
    pub target: Option<String>,
    /// The message with ANSI escapes removed. For a continuation, the whole line.
    pub message: String,
}

impl Line {
    /// Returns the name of the outermost span, which identifies the handler or task the line was
    /// logged from, or `None` if the line was logged outside any span.
    #[must_use]
    pub fn root_span(&self) -> Option<&str> {
        self.spans.first().map(|span| span.name.as_str())
    }

    /// Returns the value of the field called `name` of any span of the line, or `None` if no span
    /// has one. If several spans have a field of that name, it is the outermost span's.
    #[must_use]
    pub fn field(&self, name: &str) -> Option<&str> {
        self.spans
            .iter()
            .flat_map(|span| span.fields.iter())
            .find(|(key, _)| key == name)
            .map(|(_, value)| value.as_str())
    }
}

/// Returns the lines of a log slice, parsed: one [`Line`] for each line of `slice`, in file order,
/// numbered from 1.
///
/// A line that does not start with a timestamp is a continuation, and takes the time of the line
/// before it. Bytes that are not UTF-8 are replaced, and ANSI escape codes are removed (see
/// [`strip_ansi`]) before a line is read.
///
/// The first line of `slice` is line 1, so for a slice that is not a whole log the numbers are
/// relative to its start.
#[must_use]
pub fn parse(slice: &[u8]) -> Vec<Line> {
    komorebi_log::index_lines(slice)
        .into_iter()
        .map(|indexed| {
            let start = indexed.offset as usize;
            let end = start + indexed.len as usize;
            let raw = String::from_utf8_lossy(&slice[start..end]);
            let text = strip_ansi(raw.trim_end_matches(['\n', '\r']));
            parse_line(
                indexed.number,
                &text,
                indexed.timestamp,
                indexed.is_continuation,
            )
        })
        .collect()
}

/// Returns one ANSI-free line, parsed.
///
/// `number` is the line's number in its log. `time` is the timestamp the log's reader found for it
/// (see [`komorebi_log::index_lines`]): the timestamp at the start of `text` is skipped, not read
/// again, so `time` is what [`Line::wall_us`] is made from. A `continuation` is a line with no
/// timestamp of its own; it is not read for a level, spans or a target, and `text` is its
/// message.
///
/// A line that is not a continuation but whose second word is not a level has no level, spans or
/// target, and its message is the text after the timestamp.
///
/// # Example
///
/// Quoted field values may hold spaces and braces, and a target that is a single word is not mistaken
/// for a span:
///
/// ```rust
/// use komorebi_recorder::analysis::logparse::{Level, parse_line};
///
/// let line = parse_line(
///     3,
///     r#"2026-10-26T03:37:19.781106Z  WARN process_event:observe{title="A {weird} title" pid=7}: komorebi::application: registered"#,
///     None,
///     false,
/// );
/// assert_eq!(line.number, 3);
/// assert_eq!(line.level, Some(Level::Warn));
/// assert_eq!(line.field("title"), Some("A {weird} title"));
/// assert_eq!(line.field("pid"), Some("7"));
/// assert_eq!(line.message, "registered");
///
/// let line = parse_line(4, "2026-10-26T03:37:19.1Z  INFO komorebi: started", None, false);
/// assert!(line.spans.is_empty());
/// assert_eq!(line.target.as_deref(), Some("komorebi"));
/// ```
#[must_use]
pub fn parse_line(
    number: u64,
    text: &str,
    time: Option<DateTime<Utc>>,
    continuation: bool,
) -> Line {
    if continuation {
        return Line {
            number,
            wall_us: time.map(|time| time.timestamp_micros()),
            continuation,
            level: None,
            spans: Vec::new(),
            chain: String::new(),
            target: None,
            message: text.to_string(),
        };
    }

    // `<time> <level> <body>`; the level is right-aligned so there can be two spaces
    let mut parts = text.splitn(2, char::is_whitespace);
    let _timestamp = parts.next();
    let after_time = parts.next().unwrap_or("").trim_start();
    let (level_text, body) = after_time
        .split_once(char::is_whitespace)
        .unwrap_or((after_time, ""));

    let Some(level) = Level::parse(level_text) else {
        return Line {
            number,
            wall_us: time.map(|time| time.timestamp_micros()),
            continuation: false,
            level: None,
            spans: Vec::new(),
            chain: String::new(),
            target: None,
            message: after_time.to_string(),
        };
    };

    let (spans, chain, target, message) = parse_body(body.trim_start());
    Line {
        number,
        wall_us: time.map(|time| time.timestamp_micros()),
        continuation: false,
        level: Some(level),
        spans,
        chain,
        target,
        message,
    }
}

fn is_ident(character: char) -> bool {
    character.is_alphanumeric() || character == '_'
}

/// Split `{...}` off the front of `text`, honoring quotes, returning the inside and what follows.
fn braced(text: &str) -> Option<(&str, &str)> {
    let mut depth = 0_u32;
    let mut quoted = false;
    let mut escaped = false;
    for (index, character) in text.char_indices() {
        if quoted {
            match character {
                _ if escaped => escaped = false,
                '\\' => escaped = true,
                '"' => quoted = false,
                _ => {}
            }
            continue;
        }

        match character {
            '"' => quoted = true,
            '{' => depth += 1,
            '}' => {
                depth -= 1;
                if depth == 0 {
                    return Some((&text[1..index], &text[index + 1..]));
                }
            }
            _ => {}
        }
    }

    None
}

fn parse_fields(text: &str) -> Vec<(String, String)> {
    let mut fields = Vec::new();
    let mut rest = text.trim();
    // komorebi for Windows writes a command's span as `process_command{FocusWindow(Left)}`, a bare
    // value with no field name: keep it as the `message` field the flavor's rules read
    if !rest.is_empty() && !rest.contains('=') {
        return vec![("message".to_string(), rest.to_string())];
    }
    while !rest.is_empty() {
        let Some((key, after)) = rest.split_once('=') else {
            break;
        };

        let (value, remainder) = if let Some(quoted) = after.strip_prefix('"') {
            let mut end = quoted.len();
            let mut escaped = false;
            for (index, character) in quoted.char_indices() {
                match character {
                    _ if escaped => escaped = false,
                    '\\' => escaped = true,
                    '"' => {
                        end = index;
                        break;
                    }
                    _ => {}
                }
            }
            (&quoted[..end], quoted.get(end + 1..).unwrap_or(""))
        } else {
            let end = after.find(' ').unwrap_or(after.len());
            (&after[..end], &after[end..])
        };

        fields.push((key.trim().to_string(), value.to_string()));
        rest = remainder.trim_start();
    }

    fields
}

/// The span chain, target and message of the part of a line after its level.
fn parse_body(body: &str) -> (Vec<Span>, String, Option<String>, String) {
    let mut spans = Vec::new();
    // each span as written, parallel to `spans`
    let mut raws: Vec<&str> = Vec::new();
    let mut cursor = body;

    // spans: `name`, `name{fields}`, joined by `:`; the chain ends at `: `
    loop {
        let ident_len = cursor.find(|c: char| !is_ident(c)).unwrap_or(cursor.len());
        if ident_len == 0 {
            break;
        }

        let name = &cursor[..ident_len];
        let after = &cursor[ident_len..];
        // `komorebi::x` is a target, not a span
        if after.starts_with("::") || after.is_empty() {
            break;
        }

        let (fields, tail, raw) = if after.starts_with('{') {
            match braced(after) {
                Some((inside, tail)) => (
                    parse_fields(inside),
                    tail,
                    &cursor[..cursor.len() - tail.len()],
                ),
                None => break,
            }
        } else {
            (Vec::new(), after, name)
        };

        if let Some(rest) = tail.strip_prefix(": ") {
            spans.push(Span {
                name: name.to_string(),
                fields,
            });
            raws.push(raw);
            cursor = rest;
            break;
        }

        if let Some(rest) = tail.strip_prefix(':') {
            spans.push(Span {
                name: name.to_string(),
                fields,
            });
            raws.push(raw);
            cursor = rest;
            continue;
        }

        break;
    }

    // the target: `a::b::c: ` (a lone `komorebi: ` also counts)
    let target_len = cursor
        .find(|c: char| !(is_ident(c) || c == ':'))
        .unwrap_or(cursor.len());
    let candidate = &cursor[..target_len];
    if let Some(target) = candidate.strip_suffix(':')
        && !target.is_empty()
        && !target.ends_with(':')
        && cursor[target_len..].starts_with(' ')
    {
        return (
            spans,
            raws.join(":"),
            Some(target.to_string()),
            cursor[target_len + 1..].to_string(),
        );
    }

    // no target: what looked like the last span may have been a single-word target
    if spans.last().is_some_and(|last| last.fields.is_empty())
        && let Some(last) = spans.pop()
    {
        raws.pop();
        return (spans, raws.join(":"), Some(last.name), cursor.to_string());
    }

    (spans, raws.join(":"), None, cursor.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = include_str!("../../tests/fixtures/komorebi_plaintext_sample.log");

    fn line(text: &str) -> Line {
        let time = komorebi_log::parse_timestamp(text);
        parse_line(1, &strip_ansi(text), time, time.is_none())
    }

    #[test]
    fn the_chain_keeps_each_span_as_written() {
        let parsed = line(
            "2026-10-26T23:09:16.688494Z  INFO process_command{MoveWindow(Right)}:move_container_in_direction{direction=Right}: komorebi::window_manager: moving",
        );
        assert_eq!(
            parsed.chain,
            "process_command{MoveWindow(Right)}:move_container_in_direction{direction=Right}"
        );

        // a lone word before the message is a target, so it is not part of the chain
        let lone = line("2026-10-26T23:09:16.688494Z  INFO a{x=1}:komorebi: hello");
        assert_eq!(lone.chain, "a{x=1}");
        assert_eq!(lone.target.as_deref(), Some("komorebi"));

        let bare = line("2026-10-26T23:09:16.688494Z  INFO komorebi::x: hello");
        assert_eq!(bare.chain, "");
    }

    #[test]
    fn a_handler_header_has_one_span_a_target_and_a_message() {
        let parsed = line(
            "2026-10-26T23:09:16.685293Z  INFO process_event: komorebi::process_event: processing event: FocusChange for process 79469 with notification NSWorkspaceDidActivateApplicationNotification",
        );

        assert_eq!(parsed.level, Some(Level::Info));
        assert_eq!(parsed.root_span(), Some("process_event"));
        assert_eq!(parsed.spans.len(), 1);
        assert_eq!(parsed.target.as_deref(), Some("komorebi::process_event"));
        assert_eq!(
            parsed.message,
            "processing event: FocusChange for process 79469 with notification NSWorkspaceDidActivateApplicationNotification"
        );
        assert!(parsed.wall_us.is_some());
    }

    #[test]
    fn a_line_logged_from_another_thread_has_no_span() {
        let parsed = line(
            "2026-10-26T23:09:16.610799Z  INFO komorebi::border_manager: invalidating border observer",
        );

        assert!(parsed.spans.is_empty());
        assert_eq!(parsed.root_span(), None);
        assert_eq!(parsed.target.as_deref(), Some("komorebi::border_manager"));
        assert_eq!(parsed.message, "invalidating border observer");
    }

    #[test]
    fn nested_spans_keep_their_fields_after_the_ansi_is_stripped() {
        let raw = "2026-10-26T23:09:16.688494Z  INFO process_event:focus_window{\u{1b}[3midx\u{1b}[0m\u{1b}[2m=\u{1b}[0m0}: komorebi::container: focusing window";
        let parsed = parse(raw.as_bytes()).remove(0);

        assert_eq!(
            parsed.spans,
            vec![
                Span {
                    name: "process_event".to_string(),
                    fields: vec![]
                },
                Span {
                    name: "focus_window".to_string(),
                    fields: vec![("idx".to_string(), "0".to_string())]
                },
            ]
        );
        assert_eq!(parsed.field("idx"), Some("0"));
        assert_eq!(parsed.target.as_deref(), Some("komorebi::container"));
        assert_eq!(parsed.message, "focusing window");
    }

    #[test]
    fn spans_with_several_fields_and_a_span_after_them_parse() {
        let parsed = line(
            "2026-10-26T03:37:19.781106Z ERROR process_event:update_focused_workspace{follow_focus=true trigger_focus=false}:update_focused_workspace:window_rect: komorebi::macos_api: failed to get window position: AXError::InvalidUIElement",
        );

        let names: Vec<&str> = parsed.spans.iter().map(|span| span.name.as_str()).collect();
        assert_eq!(
            names,
            [
                "process_event",
                "update_focused_workspace",
                "update_focused_workspace",
                "window_rect"
            ]
        );
        assert_eq!(parsed.field("follow_focus"), Some("true"));
        assert_eq!(parsed.field("trigger_focus"), Some("false"));
        assert_eq!(parsed.level, Some(Level::Error));
        assert_eq!(parsed.target.as_deref(), Some("komorebi::macos_api"));
        assert_eq!(
            parsed.message,
            "failed to get window position: AXError::InvalidUIElement"
        );
    }

    #[test]
    fn quoted_field_values_may_contain_spaces_and_braces() {
        let parsed = line(
            r#"2026-10-26T03:37:19.781106Z  INFO process_event:observe{title="A {weird} title" pid=7}: komorebi::application: registered"#,
        );

        assert_eq!(parsed.field("title"), Some("A {weird} title"));
        assert_eq!(parsed.field("pid"), Some("7"));
        assert_eq!(parsed.target.as_deref(), Some("komorebi::application"));
        assert_eq!(parsed.message, "registered");
    }

    #[test]
    fn a_message_containing_colons_is_not_mistaken_for_structure() {
        let parsed = line(
            "2026-10-26T03:37:19.1Z  WARN komorebi::application: failed to register observer for process 7 (X): AXError::CannotComplete, 3 retries left",
        );

        assert!(parsed.spans.is_empty());
        assert_eq!(parsed.target.as_deref(), Some("komorebi::application"));
        assert_eq!(
            parsed.message,
            "failed to register observer for process 7 (X): AXError::CannotComplete, 3 retries left"
        );
    }

    #[test]
    fn a_single_word_target_is_not_taken_for_a_span() {
        let parsed = line("2026-10-26T03:37:19.1Z  INFO komorebi: started");
        assert!(parsed.spans.is_empty());
        assert_eq!(parsed.target.as_deref(), Some("komorebi"));
        assert_eq!(parsed.message, "started");
    }

    #[test]
    fn continuation_lines_take_the_time_of_the_line_before() {
        let parsed = parse(
            b"2026-10-26T03:37:19.703632Z  INFO komorebi::x: before\nthread 'main' panicked at src/x.rs:1:1:\n   0: frame\n",
        );

        assert_eq!(parsed.len(), 3);
        assert!(!parsed[0].continuation);
        assert!(parsed[1].continuation && parsed[2].continuation);
        assert_eq!(parsed[1].wall_us, parsed[0].wall_us);
        assert_eq!(parsed[1].message, "thread 'main' panicked at src/x.rs:1:1:");
        assert_eq!(parsed[2].level, None);
    }

    #[test]
    fn every_line_of_a_real_log_parses_into_levels_and_targets() {
        let parsed = parse(SAMPLE.as_bytes());
        assert_eq!(parsed.len(), SAMPLE.lines().count());

        let structured: Vec<&Line> = parsed.iter().filter(|line| !line.continuation).collect();
        assert!(structured.len() > 40);
        for line in &structured {
            assert!(line.level.is_some(), "line {} has no level", line.number);
            assert!(
                line.target
                    .as_deref()
                    .is_some_and(|target| target.starts_with("komorebi")),
                "line {} has no komorebi target: {line:?}",
                line.number
            );
            assert!(
                !line.message.is_empty(),
                "line {} has no message",
                line.number
            );
            assert!(
                !line.message.contains('\u{1b}'),
                "line {} still has ANSI in its message",
                line.number
            );
        }

        // the span chains of the real log were recovered
        assert!(
            parsed
                .iter()
                .any(|line| line.root_span() == Some("process_event"))
        );
        assert!(parsed.iter().any(|line| line.field("idx") == Some("0")));
        assert!(parsed.iter().any(|line| line.level == Some(Level::Error)));
        assert!(parsed.iter().any(|line| line.level == Some(Level::Warn)));
    }

    /// Run with `--ignored` on a machine that runs komorebi: every line of its newest real log
    /// must parse, so a komorebi that logs differently is noticed.
    #[test]
    #[ignore = "reads the real komorebi log of the machine"]
    fn the_newest_real_log_parses_completely() {
        let dir = crate::komorebi_link::data_dir().to_path_buf();
        let newest = std::fs::read_dir(&dir)
            .expect("data dir")
            .filter_map(Result::ok)
            .filter(|entry| {
                entry
                    .file_name()
                    .to_string_lossy()
                    .starts_with(komorebi_log::PLAINTEXT_PREFIX)
            })
            .max_by_key(|entry| entry.file_name())
            .expect("a plaintext log");

        let bytes = std::fs::read(newest.path()).expect("read");
        let parsed = parse(&bytes);
        let structured: Vec<&Line> = parsed.iter().filter(|line| !line.continuation).collect();
        let without_target = structured
            .iter()
            .filter(|line| line.target.is_none())
            .count();
        let without_level = structured
            .iter()
            .filter(|line| line.level.is_none())
            .count();

        eprintln!(
            "{}: {} lines, {} structured, {without_target} without a target, {without_level} without a level",
            newest.path().display(),
            parsed.len(),
            structured.len()
        );
        assert_eq!(without_level, 0);
        assert!(without_target * 1000 <= structured.len());
    }

    #[test]
    fn a_span_with_a_bare_value_is_its_message() {
        let parsed = parse(
            b"2026-10-28T15:46:20.730173Z  INFO process_command{FocusWindow(Left)}: komorebi::process_command: processed
",
        );
        let fields = &parsed[0].spans[0].fields;
        assert_eq!(
            fields,
            &[("message".to_string(), "FocusWindow(Left)".to_string())]
        );
    }
}
