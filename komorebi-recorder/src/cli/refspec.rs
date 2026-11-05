//! Evidence references that name a log line by what it says or when it was written.
//!
//! `annotate add --ref log:345` says which line of komorebi's log by number, which an agent has to
//! find out first. [`resolve`] lets it say what it means instead, and turns that into the
//! `log:<line>` that [`crate::annotations::Ref`] reads:
//!
//! | Written | Means |
//! |---|---|
//! | `log:345`, `log:<file>:<line>` | that line, as before |
//! | `log:"some text"` (or in single quotes) | the one line of `komorebi.log` that contains the text, ignoring case |
//! | `log:HH:MM:SS.mmm` | the line written at that UTC time, as the log has it; as precise as it is written (`HH:MM:SS` matches the whole second) |
//! | `log:12.5`, `log:1:02.5` | the line nearest that time of the video |
//!
//! A text or a UTC time that matches more than one line is a usage error that lists the first few as
//! `log:N: text`, so the author can pick one or say more. A match is reported back as the resolved
//! `log:N` and the text of the line, so that the author can check it is the statement they meant.
//! Quote the whole argument for the shell: `--ref 'log:"could not find"'`.

use crate::analysis::envelope;
use crate::annotations::DEFAULT_LOG_FILE;
use crate::clock::ClockMap;
use crate::session::Manifest;
use crate::session::Session;
use crate::sources::komorebi_log;
use crate::timecode;
use chrono::DateTime;
use chrono::Timelike;
use chrono::Utc;
use color_eyre::eyre;

/// How many candidate lines an ambiguous reference lists.
const CANDIDATES: usize = 5;

/// A reference, with what to tell the author if it was not written as a line number.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Resolved {
    /// The reference to parse: `text` itself, or the `log:<line>` it resolved to.
    pub reference: String,
    /// For a resolved reference: what it resolved to, and the text of the line.
    pub note: Option<String>,
}

/// How the part of a `log:` reference after the colon is to be read.
#[derive(Debug, PartialEq)]
enum Spelling {
    /// A line number, or `<file>:<line>`: [`crate::annotations::Ref`] reads it.
    Line,
    /// Text to find in the log.
    Text(String),
    /// A UTC time of day in microseconds, and how many microseconds the way it was written spans.
    Utc { micros_of_day: u64, span: u64 },
    /// Seconds of video time.
    Video(f64),
    /// Words that are not quoted: an error, with a hint.
    Unquoted,
}

fn classify(rest: &str) -> Spelling {
    if rest.parse::<u64>().is_ok() {
        return Spelling::Line;
    }

    for quote in ['"', '\''] {
        if rest.len() >= 2 && rest.starts_with(quote) && rest.ends_with(quote) {
            return Spelling::Text(rest[1..rest.len() - 1].to_string());
        }
    }

    if let Some(utc) = utc_of_day(rest) {
        return utc;
    }

    let numeric = rest
        .chars()
        .all(|character| character.is_ascii_digit() || character == ':' || character == '.');
    if numeric
        && rest.contains('.')
        && let Ok(video_s) = timecode::parse(rest)
    {
        return Spelling::Video(video_s);
    }

    // `<file>:<line>` is for Ref; anything else with words in it was probably meant as text
    let is_file_and_line = rest
        .rsplit_once(':')
        .is_some_and(|(_, line)| line.parse::<u64>().is_ok());
    if !is_file_and_line && rest.contains(char::is_whitespace) {
        return Spelling::Unquoted;
    }

    Spelling::Line
}

/// Reads `HH:MM:SS` or `HH:MM:SS.fraction` (up to microseconds; more digits are ignored).
fn utc_of_day(text: &str) -> Option<Spelling> {
    let (clock, fraction) = match text.split_once('.') {
        Some((clock, fraction)) => (clock, Some(fraction)),
        None => (text, None),
    };
    let mut parts = clock.split(':');
    let (hours, minutes, seconds) = (parts.next()?, parts.next()?, parts.next()?);
    if parts.next().is_some() || hours.len() > 2 || minutes.len() != 2 || seconds.len() != 2 {
        return None;
    }
    let (hours, minutes, seconds): (u64, u64, u64) = (
        hours.parse().ok()?,
        minutes.parse().ok()?,
        seconds.parse().ok()?,
    );
    if hours > 23 || minutes > 59 || seconds > 59 {
        return None;
    }

    let (micros, span) = match fraction {
        None => (0, 1_000_000),
        Some(fraction) if !fraction.is_empty() && fraction.chars().all(|c| c.is_ascii_digit()) => {
            let digits = fraction.len().min(6);
            let padded = format!("{:0<6}", &fraction[..digits]);
            (padded.parse().ok()?, 10_u64.pow(6 - digits as u32))
        }
        Some(_) => return None,
    };

    Some(Spelling::Utc {
        micros_of_day: ((hours * 60 + minutes) * 60 + seconds) * 1_000_000 + micros,
        span,
    })
}

/// A line of the log: its number, its own time (none for a continuation line) and its text.
struct Line {
    number: u64,
    own_time: Option<DateTime<Utc>>,
    text: String,
}

fn read_log(session: &Session) -> eyre::Result<Vec<Line>> {
    let bytes = std::fs::read(session.path(DEFAULT_LOG_FILE)).map_err(|_| {
        envelope::not_found(format!(
            "the session has no {DEFAULT_LOG_FILE}, so a log reference cannot be found by text or time"
        ))
    })?;

    Ok(komorebi_log::index_lines(&bytes)
        .into_iter()
        .zip(bytes.split_inclusive(|byte| *byte == b'\n'))
        .map(|(line, raw)| Line {
            number: line.number,
            own_time: (!line.is_continuation).then_some(line.timestamp).flatten(),
            text: String::from_utf8_lossy(raw).trim_end().to_string(),
        })
        .collect())
}

/// The line without its timestamp, for matching and showing.
fn message(line: &Line) -> &str {
    if line.own_time.is_some() {
        line.text
            .split_once(char::is_whitespace)
            .map_or("", |(_, rest)| rest.trim_start())
    } else {
        line.text.trim()
    }
}

fn shown(line: &Line) -> String {
    let message = message(line);
    let mut text: String = message.chars().take(160).collect();
    if message.chars().count() > 160 {
        text.push_str("...");
    }
    format!("log:{}: {text}", line.number)
}

fn ambiguous(what: &str, matches: &[&Line]) -> eyre::Report {
    let mut listed: Vec<String> = matches
        .iter()
        .take(CANDIDATES)
        .map(|line| format!("  {}", shown(line)))
        .collect();
    if matches.len() > CANDIDATES {
        listed.push(format!("  ... and {} more", matches.len() - CANDIDATES));
    }
    envelope::usage(format!(
        "{what} matches {} lines of {DEFAULT_LOG_FILE}; say which with log:<N>, or say more:\n{}",
        matches.len(),
        listed.join("\n")
    ))
}

fn resolved(input: &str, line: &Line) -> Resolved {
    Resolved {
        reference: format!("log:{}", line.number),
        note: Some(format!(
            "{input} resolved to {}; check that is the statement you meant",
            shown(line)
        )),
    }
}

fn micros_of_day(time: DateTime<Utc>) -> u64 {
    u64::from(time.num_seconds_from_midnight()) * 1_000_000
        + u64::from(time.timestamp_subsec_micros())
}

/// Returns the reference `text` stands for: itself, unless it is a `log:` reference that names a
/// line by text or time, in which case it is `log:<line>` with a note of what was chosen.
///
/// # Errors
///
/// A usage error if the text matches more than one line (the first few are listed), or is words
/// that are not quoted; a not-found error if nothing matches or the session has no
/// `komorebi.log`.
///
/// # Example
///
/// ```rust
/// use komorebi_recorder::cli::refspec::resolve;
/// use komorebi_recorder::testkit::SessionBuilder;
///
/// let (session, manifest) = SessionBuilder::new("doc-refspec")
///     .log(2.0, "INFO", "komorebi::process_event: processing event")
///     .log(3.0, "WARN", "komorebi::window: could not place the window")
///     .build();
///
/// // by what the line says
/// let found = resolve(&session, &manifest, "log:\"could not place\"")?;
/// assert_eq!(found.reference, "log:2");
/// assert!(found.note.is_some_and(|note| note.contains("could not place the window")));
///
/// // by when it was written (the testkit's clock reads 12:00:07 at video second 2.0)
/// let by_time = resolve(&session, &manifest, "log:12:00:07.000")?;
/// assert_eq!(by_time.reference, "log:1");
///
/// // a line number is left alone
/// assert_eq!(resolve(&session, &manifest, "log:7")?.reference, "log:7");
/// # Ok::<(), Box<dyn std::error::Error>>(())
/// ```
pub fn resolve(session: &Session, manifest: &Manifest, text: &str) -> eyre::Result<Resolved> {
    let unchanged = || Resolved {
        reference: text.to_string(),
        note: None,
    };
    let Some(rest) = text.strip_prefix("log:") else {
        return Ok(unchanged());
    };

    match classify(rest) {
        Spelling::Line => Ok(unchanged()),
        Spelling::Unquoted => Err(envelope::usage(format!(
            "'{text}': to find a log line by its text, quote the text: log:\"{rest}\" \
             (quote the whole argument for the shell)"
        ))),
        Spelling::Text(needle) => {
            if needle.trim().is_empty() {
                return Err(envelope::usage("'log:\"\"': there is no text to look for"));
            }
            let lines = read_log(session)?;
            let lowered = needle.to_lowercase();
            let matches: Vec<&Line> = lines
                .iter()
                .filter(|line| message(line).to_lowercase().contains(&lowered))
                .collect();
            match matches[..] {
                [] => Err(envelope::not_found(format!(
                    "no line of {DEFAULT_LOG_FILE} contains \"{needle}\""
                ))),
                [line] => Ok(resolved(text, line)),
                _ => Err(ambiguous(&format!("\"{needle}\""), &matches)),
            }
        }
        Spelling::Utc {
            micros_of_day: wanted,
            span,
        } => {
            let lines = read_log(session)?;
            let matches: Vec<&Line> = lines
                .iter()
                .filter(|line| {
                    line.own_time
                        .is_some_and(|time| (wanted..wanted + span).contains(&micros_of_day(time)))
                })
                .collect();
            match matches[..] {
                [] => Err(envelope::not_found(format!(
                    "no line of {DEFAULT_LOG_FILE} was written at {rest} UTC{}",
                    nearest(&lines, wanted).map_or_else(String::new, |line| format!(
                        "; the nearest is {}",
                        shown(line)
                    ))
                ))),
                [line] => Ok(resolved(text, line)),
                _ => Err(ambiguous(&format!("{rest} UTC"), &matches)),
            }
        }
        Spelling::Video(video_s) => {
            let lines = read_log(session)?;
            let clock = ClockMap::new(manifest.anchors.clone());
            let wall_us = clock
                .mach_to_wall(manifest.mach_at_video_seconds(video_s))
                .ok_or_else(|| {
                    envelope::unusable("the session has no clock anchors to convert a video time")
                })?;
            let wanted = DateTime::from_timestamp_micros(wall_us)
                .map(micros_of_day)
                .ok_or_else(|| envelope::usage(format!("{rest} is not a time")))?;
            let line = nearest(&lines, wanted).ok_or_else(|| {
                envelope::not_found(format!("{DEFAULT_LOG_FILE} has no timestamped line"))
            })?;
            Ok(resolved(text, line))
        }
    }
}

/// The line written nearest to a time of day (the earlier of two equally near).
fn nearest(lines: &[Line], wanted: u64) -> Option<&Line> {
    lines
        .iter()
        .filter_map(|line| Some((line, micros_of_day(line.own_time?))))
        .min_by_key(|(_, micros)| micros.abs_diff(wanted))
        .map(|(line, _)| line)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testkit::SessionBuilder;

    #[test]
    fn how_a_log_reference_is_spelled() {
        assert_eq!(classify("345"), Spelling::Line);
        assert_eq!(classify("oslog/komorebi.ndjson:77"), Spelling::Line);
        assert_eq!(
            classify("\"two words\""),
            Spelling::Text("two words".to_string())
        );
        assert_eq!(classify("'it'"), Spelling::Text("it".to_string()));
        assert_eq!(
            classify("12:00:08.250"),
            Spelling::Utc {
                micros_of_day: 43_208_250_000,
                span: 1_000
            }
        );
        assert_eq!(
            classify("12:00:08"),
            Spelling::Utc {
                micros_of_day: 43_208_000_000,
                span: 1_000_000
            }
        );
        assert_eq!(classify("12.5"), Spelling::Video(12.5));
        assert_eq!(classify("1:02.5"), Spelling::Video(62.5));
        assert_eq!(classify("some words"), Spelling::Unquoted);
        // not a time of day
        assert_eq!(classify("25:00:00"), Spelling::Line);
    }

    fn session(name: &str) -> (Session, Manifest) {
        SessionBuilder::new(name)
            .log(2.0, "INFO", "komorebi::process_event: processing event")
            .log(3.0, "WARN", "komorebi::window: could not place the window")
            .log(3.0, "INFO", "komorebi::process_event: processing event")
            .log(5.0, "INFO", "komorebi::reaper: reaping orphans")
            .build()
    }

    #[test]
    fn text_resolves_to_its_one_line_and_says_which() {
        let (session, manifest) = session("refspec-text");
        let found = resolve(&session, &manifest, "log:\"REAPING\"").expect("one match");

        assert_eq!(found.reference, "log:4");
        let note = found.note.expect("a note");
        assert!(
            note.contains("log:4: INFO komorebi::reaper: reaping orphans"),
            "{note}"
        );
    }

    #[test]
    fn text_that_matches_several_lines_lists_candidates() {
        let (session, manifest) = session("refspec-ambiguous");
        let error = resolve(&session, &manifest, "log:\"processing event\"").unwrap_err();

        assert_eq!(envelope::exit_kind(&error), envelope::ExitKind::Usage);
        let message = error.to_string();
        assert!(message.contains("matches 2 lines"), "{message}");
        assert!(
            message.contains("log:1: INFO komorebi::process_event"),
            "{message}"
        );
        assert!(
            message.contains("log:3: INFO komorebi::process_event"),
            "{message}"
        );
    }

    #[test]
    fn text_that_matches_nothing_is_not_found() {
        let (session, manifest) = session("refspec-none");
        let error = resolve(&session, &manifest, "log:\"nothing like it\"").unwrap_err();
        assert_eq!(envelope::exit_kind(&error), envelope::ExitKind::NotFound);
    }

    #[test]
    fn a_utc_time_is_as_precise_as_it_is_written() {
        let (session, manifest) = session("refspec-utc");
        // video 2.0 is 12:00:07.000 (the testkit's video starts 5 s after the clock's zero)
        let exact = resolve(&session, &manifest, "log:12:00:07.000").expect("exact");
        assert_eq!(exact.reference, "log:1");

        // a whole second holds the two lines of 12:00:08
        let error = resolve(&session, &manifest, "log:12:00:08").unwrap_err();
        assert_eq!(envelope::exit_kind(&error), envelope::ExitKind::Usage);
        assert!(error.to_string().contains("log:2:"), "{error}");

        // nothing was written then: the nearest line is offered
        let error = resolve(&session, &manifest, "log:12:00:30.000").unwrap_err();
        assert_eq!(envelope::exit_kind(&error), envelope::ExitKind::NotFound);
        assert!(
            error.to_string().contains("the nearest is log:4"),
            "{error}"
        );
    }

    #[test]
    fn a_video_time_resolves_to_the_nearest_line() {
        let (session, manifest) = session("refspec-video");
        let found = resolve(&session, &manifest, "log:4.9").expect("nearest");
        assert_eq!(found.reference, "log:4");
    }

    #[test]
    fn what_is_not_a_text_or_time_reference_is_left_alone() {
        let (session, manifest) = session("refspec-alone");
        for text in [
            "log:2",
            "event:3",
            "code:komorebi/src/window.rs:210",
            "log:other.log:9",
        ] {
            let found = resolve(&session, &manifest, text).expect("left alone");
            assert_eq!(
                found,
                Resolved {
                    reference: text.to_string(),
                    note: None
                }
            );
        }

        let error = resolve(&session, &manifest, "log:could not place").unwrap_err();
        assert!(error.to_string().contains("quote the text"), "{error}");
    }
}
