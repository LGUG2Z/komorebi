//! What the readers share: line numbers, diagnostics, comments and continued lines.

use crate::Diagnostic;
use crate::Severity;
use crate::Span;
use crate::Support;

/// The byte offset at which each line of a text starts.
pub(crate) struct Lines {
    starts: Vec<usize>,
}

impl Lines {
    pub(crate) fn new(text: &str) -> Lines {
        let mut starts = vec![0];
        starts.extend(text.match_indices('\n').map(|(i, _)| i + 1));
        Lines { starts }
    }

    /// The span from `start` to `end`, with the line of `start`.
    pub(crate) fn span(&self, start: usize, end: usize) -> Span {
        let line = self.starts.partition_point(|&s| s <= start);
        Span { start, end, line }
    }
}

pub(crate) fn error(span: Span, msg: impl Into<String>) -> Diagnostic {
    Diagnostic {
        span,
        severity: Severity::Error,
        support: Support::Unsupported,
        msg: msg.into(),
    }
}

pub(crate) fn approximated(span: Span, msg: impl Into<String>) -> Diagnostic {
    Diagnostic {
        span,
        severity: Severity::Warning,
        support: Support::Approximated,
        msg: msg.into(),
    }
}

pub(crate) fn unsupported(span: Span, msg: impl Into<String>) -> Diagnostic {
    Diagnostic {
        span,
        severity: Severity::Warning,
        support: Support::Unsupported,
        msg: msg.into(),
    }
}

pub(crate) fn note(span: Span, support: Support, msg: impl Into<String>) -> Diagnostic {
    Diagnostic {
        span,
        severity: Severity::Note,
        support,
        msg: msg.into(),
    }
}

/// The diagnostic for a side-specific modifier, which KDE does not have.
pub(crate) fn one_sided(span: Span, written: &str, kde: &str) -> Diagnostic {
    approximated(
        span,
        format!("`{written}` is {kde}: KDE does not tell the left and right {kde} apart"),
    )
}

/// Drops a trailing ` # comment` from a command, unless the `#` is quoted or part of a word.
pub(crate) fn strip_comment(command: &str) -> &str {
    let mut quote: Option<char> = None;
    let mut previous = ' ';
    for (index, c) in command.char_indices() {
        match (quote, c) {
            (Some(q), c) if c == q => quote = None,
            (None, '"' | '\'') => quote = Some(c),
            (None, '#') if previous.is_whitespace() => return &command[..index],
            _ => {}
        }
        previous = c;
    }
    command
}

/// Joins the lines of `text` that end in `\` with the next, with one space between the two.
pub(crate) fn join_continued(text: &str) -> String {
    let mut joined = String::new();
    let mut lines = text.split('\n').peekable();
    while let Some(line) = lines.next() {
        let line = line.strip_suffix('\r').unwrap_or(line);
        match line.strip_suffix('\\') {
            Some(continued) if lines.peek().is_some() => {
                joined.push_str(continued.trim_end());
                joined.push(' ');
                if let Some(next) = lines.peek_mut() {
                    *next = next.trim_start();
                }
            }
            _ => {
                joined.push_str(line);
                if lines.peek().is_some() {
                    joined.push('\n');
                }
            }
        }
    }
    joined
}

/// The text of a binding as it was written, on one line: continued lines are joined, and the
/// lines of a block are trimmed and joined with a space.
pub(crate) fn one_line(text: &str) -> String {
    let mut out = String::new();
    let mut lines = text.split('\n').peekable();
    while let Some(line) = lines.next() {
        let line = line.strip_suffix('\r').unwrap_or(line);
        match line.strip_suffix('\\') {
            Some(continued) if lines.peek().is_some() => {
                out.push_str(continued);
                out.push(' ');
            }
            _ => {
                out.push_str(line.trim_end());
                if lines.peek().is_some() {
                    out.push(' ');
                    if let Some(next) = lines.peek_mut() {
                        *next = next.trim_start();
                    }
                }
            }
        }
    }
    out.trim().to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_trailing_comment_is_not_part_of_the_command() {
        assert_eq!(
            strip_comment("komorebic unstack # 0x29 is ;").trim(),
            "komorebic unstack"
        );
        assert_eq!(
            strip_comment("echo '# not a comment'"),
            "echo '# not a comment'"
        );
        assert_eq!(strip_comment("echo a#b"), "echo a#b");
    }

    #[test]
    fn lines_are_counted_from_one() {
        let lines = Lines::new("a\nbc\n\nd");
        assert_eq!(lines.span(0, 1).line, 1);
        assert_eq!(lines.span(2, 3).line, 2);
        assert_eq!(lines.span(5, 5).line, 3);
        assert_eq!(lines.span(6, 7).line, 4);
    }

    #[test]
    fn a_continued_line_is_joined_with_one_space() {
        assert_eq!(
            join_continued("komorebic focus \\\n    left"),
            "komorebic focus left"
        );
        assert_eq!(join_continued("a\\\nb\\\n c\nd"), "a b c\nd");
        // a backslash on the last line continues nothing
        assert_eq!(join_continued("a \\"), "a \\");
    }

    #[test]
    fn a_binding_is_written_on_one_line() {
        assert_eq!(
            one_line("alt - x : komorebic \\\n    flip-layout"),
            "alt - x : komorebic      flip-layout"
        );
        assert_eq!(
            one_line("alt - n [\n    \"a\" : x\n    * ~\n]"),
            "alt - n [ \"a\" : x * ~ ]"
        );
        assert_eq!(
            one_line("alt - h                 : komorebic focus left"),
            "alt - h                 : komorebic focus left"
        );
    }
}
