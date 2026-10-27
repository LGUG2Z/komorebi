//! Parsing and formatting of video timecodes: the way a position in a recording is typed on a
//! command line and shown back.
//!
//! A timecode is a number of seconds of video time, an `f64`, counted from the first captured
//! frame of a session (video `00:00.000`). A time before the first frame is negative.
//!
//! | Function | Does |
//! |---|---|
//! | [`parse`] | reads a timecode into seconds |
//! | [`parse_span`] | reads a duration (`500ms`, `2m`) into seconds |
//! | [`format()`] | writes seconds as a timecode |
//!
//! Accepted by [`parse`]: `12.5` (seconds), `12`, `00:12.500`, `1:02`, `1:02:03` and `1:02:03.25`.
//! A leading `-` is accepted so that offsets before the start of the video can be expressed.
//! [`format()`] writes `MM:SS.mmm`, or `H:MM:SS.mmm` from one hour on, and [`parse`] reads back
//! what it writes.
//!
//! This module has no idea what a recording is: it converts between text and a number of seconds,
//! and places nothing on a clock. Turning video seconds into a time on the master clock is the
//! session manifest's `mach_at_video_seconds`, and the reverse its `video_seconds`.
//!
//! # Example
//!
//! ```rust
//! use komorebi_recorder::timecode;
//!
//! assert_eq!(timecode::parse("00:12.500")?, 12.5);
//! assert_eq!(timecode::parse("1:02:03")?, 3723.0);
//! assert_eq!(timecode::parse("-1.5")?, -1.5);
//!
//! assert_eq!(timecode::format(12.5), "00:12.500");
//! assert_eq!(timecode::format(3723.25), "1:02:03.250");
//!
//! // a span is a length of time, not a position
//! assert_eq!(timecode::parse_span("500ms")?, 0.5);
//! assert_eq!(timecode::parse_span("2m")?, 120.0);
//! # Ok::<(), Box<dyn std::error::Error>>(())
//! ```

use color_eyre::eyre;

/// Parses a timecode into seconds.
///
/// The forms are `[[H:]M:]S[.fraction]`, optionally preceded by `-`, with surrounding whitespace
/// ignored: `12.5`, `12`, `00:12.500`, `1:02`, `1:02:03` and `1:02:03.25`. The hours and minutes
/// are whole numbers, and the seconds are any non-negative number Rust reads as an `f64`. The
/// parts are not range checked, so `1:75` is `135` seconds and `90:00` is `5400`. A `-` negates
/// the whole timecode, and is only accepted once, at the start.
///
/// # Errors
///
/// Returns an error if the input
///
/// * is empty (after trimming, and after a leading `-`): `empty timecode`;
/// * has more than three `:`-separated parts: `expected [[H:]M:]S[.fraction]`;
/// * has an hours or minutes part that is not an unsigned whole number (`1.5:30`, `1:-2:30`);
/// * has a seconds part, the last one, that is not a number, or is negative, `NaN` or infinite
///   (`abc`, `--1`, `1:-2`, `nan`, `inf`).
///
/// # Example
///
/// ```rust
/// use komorebi_recorder::timecode::parse;
///
/// assert_eq!(parse("12.5")?, 12.5);
/// assert_eq!(parse("1:02")?, 62.0);
/// assert_eq!(parse("1:02:03.25")?, 3723.25);
/// assert_eq!(parse("-0:01.500")?, -1.5);
///
/// // the parts are not range checked
/// assert_eq!(parse("1:75")?, 135.0);
///
/// let error = parse("1:2:3:4").unwrap_err();
/// assert_eq!(error.to_string(), "invalid timecode '1:2:3:4': expected [[H:]M:]S[.fraction]");
/// assert_eq!(parse("").unwrap_err().to_string(), "empty timecode");
/// assert!(parse("nan").is_err());
/// # Ok::<(), Box<dyn std::error::Error>>(())
/// ```
pub fn parse(input: &str) -> eyre::Result<f64> {
    let trimmed = input.trim();
    let (negative, body) = match trimmed.strip_prefix('-') {
        Some(rest) => (true, rest),
        None => (false, trimmed),
    };

    if body.is_empty() {
        eyre::bail!("empty timecode");
    }

    let parts: Vec<&str> = body.split(':').collect();
    if parts.len() > 3 {
        eyre::bail!("invalid timecode '{input}': expected [[H:]M:]S[.fraction]");
    }

    let mut seconds = 0.0_f64;
    for (index, part) in parts.iter().enumerate() {
        let is_last = index == parts.len() - 1;
        let invalid = || eyre::Report::msg(format!("invalid timecode '{input}'"));
        let value: f64 = if is_last {
            part.parse().map_err(|_| invalid())?
        } else {
            f64::from(part.parse::<u32>().map_err(|_| invalid())?)
        };

        if !value.is_finite() || value < 0.0 {
            eyre::bail!("invalid timecode '{input}'");
        }

        seconds = seconds * 60.0 + value;
    }

    Ok(if negative { -seconds } else { seconds })
}

/// Parses a duration such as `1s`, `500ms`, `1.5` (seconds) or `2m` into seconds.
///
/// A number followed by `ms` (milliseconds), `s` (seconds) or `m` (minutes), with nothing after it
/// meaning seconds. Whitespace around the input, and between the number and its unit, is ignored.
/// Hours are not a unit: use minutes, or a timecode with [`parse`].
///
/// # Errors
///
/// Returns an error if the number is not a number Rust reads as an `f64` (`soon`, or an empty
/// string), or if it is negative, `NaN` or infinite: a span cannot be negative.
///
/// # Example
///
/// ```rust
/// use komorebi_recorder::timecode::parse_span;
///
/// assert_eq!(parse_span("1s")?, 1.0);
/// assert_eq!(parse_span("500ms")?, 0.5);
/// assert_eq!(parse_span("1.5")?, 1.5);
/// assert_eq!(parse_span("2m")?, 120.0);
///
/// assert!(parse_span("-1s").is_err());
/// assert!(parse_span("soon").is_err());
/// assert_eq!(
///     parse_span("soon").unwrap_err().to_string(),
///     "invalid duration 'soon': expected e.g. 1s, 500ms or 1.5"
/// );
/// # Ok::<(), Box<dyn std::error::Error>>(())
/// ```
pub fn parse_span(input: &str) -> eyre::Result<f64> {
    let trimmed = input.trim();
    let (number, scale) = if let Some(rest) = trimmed.strip_suffix("ms") {
        (rest, 0.001)
    } else if let Some(rest) = trimmed.strip_suffix('s') {
        (rest, 1.0)
    } else if let Some(rest) = trimmed.strip_suffix('m') {
        (rest, 60.0)
    } else {
        (trimmed, 1.0)
    };

    let value: f64 = number.trim().parse().map_err(|_| {
        eyre::Report::msg(format!(
            "invalid duration '{input}': expected e.g. 1s, 500ms or 1.5"
        ))
    })?;

    if !value.is_finite() || value < 0.0 {
        eyre::bail!("invalid duration '{input}': must be a non-negative number");
    }

    Ok(value * scale)
}

/// Returns `seconds` as a timecode: `MM:SS.mmm`, or `H:MM:SS.mmm` from one hour on.
///
/// The value is rounded to the nearest millisecond. Minutes and seconds are two digits and
/// milliseconds three; hours have as many digits as they need. A negative value is written with a
/// leading `-`, which a value that rounds to zero keeps (`-00:00.000`). The result is not
/// specified for a `NaN` or infinite `seconds`.
///
/// [`parse`] reads back what this writes: a value that is a whole number of milliseconds comes
/// back equal, to within floating point error (the test `format_parse_roundtrip`).
///
/// # Example
///
/// ```rust
/// use komorebi_recorder::timecode::{format, parse};
///
/// assert_eq!(format(0.0), "00:00.000");
/// assert_eq!(format(12.5), "00:12.500");
/// assert_eq!(format(62.0), "01:02.000");
/// assert_eq!(format(3723.25), "1:02:03.250");
/// assert_eq!(format(-1.5), "-00:01.500");
///
/// // rounded to the millisecond
/// assert_eq!(format(1.2346), "00:01.235");
///
/// assert!((parse(&format(7322.125))? - 7322.125).abs() < 1e-9);
/// # Ok::<(), Box<dyn std::error::Error>>(())
/// ```
#[must_use]
pub fn format(seconds: f64) -> String {
    let sign = if seconds < 0.0 { "-" } else { "" };
    let total_ms = (seconds.abs() * 1_000.0).round() as u64;
    let ms = total_ms % 1_000;
    let total_s = total_ms / 1_000;
    let s = total_s % 60;
    let m = (total_s / 60) % 60;
    let h = total_s / 3_600;

    if h > 0 {
        format!("{sign}{h}:{m:02}:{s:02}.{ms:03}")
    } else {
        format!("{sign}{m:02}:{s:02}.{ms:03}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn close(a: f64, b: f64) -> bool {
        (a - b).abs() < 1e-9
    }

    #[test]
    fn parses_plain_seconds() {
        assert!(close(parse("12.5").unwrap(), 12.5));
        assert!(close(parse("12").unwrap(), 12.0));
        assert!(close(parse("0.04").unwrap(), 0.04));
    }

    #[test]
    fn parses_clock_forms() {
        assert!(close(parse("00:12.500").unwrap(), 12.5));
        assert!(close(parse("1:02").unwrap(), 62.0));
        assert!(close(parse("1:02:03").unwrap(), 3723.0));
        assert!(close(parse("1:02:03.25").unwrap(), 3723.25));
    }

    #[test]
    fn parses_negative_offsets() {
        assert!(close(parse("-1.5").unwrap(), -1.5));
        assert!(close(parse("-0:01.500").unwrap(), -1.5));
    }

    #[test]
    fn rejects_garbage() {
        for bad in [
            "", "abc", "1:2:3:4", "1.5:30", "::", "1:-2", "--1", "nan", "inf",
        ] {
            assert!(parse(bad).is_err(), "{bad:?} should not parse");
        }
    }

    #[test]
    fn parses_spans() {
        assert!(close(parse_span("1s").unwrap(), 1.0));
        assert!(close(parse_span("500ms").unwrap(), 0.5));
        assert!(close(parse_span("1.5").unwrap(), 1.5));
        assert!(close(parse_span("2m").unwrap(), 120.0));
        assert!(parse_span("-1s").is_err());
        assert!(parse_span("soon").is_err());
        assert!(parse_span("").is_err());
    }

    #[test]
    fn formats() {
        assert_eq!(format(12.5), "00:12.500");
        assert_eq!(format(0.0), "00:00.000");
        assert_eq!(format(62.0), "01:02.000");
        assert_eq!(format(3723.25), "1:02:03.250");
        assert_eq!(format(-1.5), "-00:01.500");
    }

    #[test]
    fn format_parse_roundtrip() {
        for value in [0.0, 0.001, 12.5, 59.999, 3600.0, 7322.125] {
            assert!(close(parse(&format(value)).unwrap(), value));
        }
    }
}
