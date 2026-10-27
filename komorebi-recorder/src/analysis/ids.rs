//! Stable names for things in a session, so a command's output can be fed straight back in.
//!
//! An event is `event:<seq>`, a line of the recorded komorebi log is `log:<line>`, a recorder
//! incident is `incident:<n>` (1-based line of `recorder.jsonl`) and a user or agent annotation is
//! `bug:<id>`, `note:<id>` or `finding:<id>`. All of them are accepted wherever a time is, see
//! `crate::analysis::timespec`.
//!
//! [`ItemRef`] parses and prints these names. An annotation id is `a-` and eight lowercase
//! hexadecimal digits (`a-3f9c1e02`), made by [`new_annotation_id`] and recognized by
//! [`is_annotation_id`]. This module only names things: whether `event:12` exists in a session is
//! asked of the session when the name is resolved to a time.

use color_eyre::eyre;
use std::collections::hash_map::RandomState;
use std::fmt;
use std::hash::BuildHasher;
use std::hash::Hasher;
use std::str::FromStr;

/// A reference to one thing in a session: an event, a line of komorebi's log, a recorder incident
/// or an annotation.
///
/// A reference is parsed from its text with [`str::parse`] ([`FromStr`]) and printed with
/// [`Display`](fmt::Display), and the two agree: the printed form of a reference (with a valid
/// annotation id, if it is an annotation) parses back to it. Parsing accepts more than printing
/// writes, so the text of a reference is not always what the reference prints as.
///
/// | Text | Reference |
/// |---|---|
/// | `event:12` | [`ItemRef::Event`]`(12)` |
/// | `log:345` | [`ItemRef::Log`]`(345)` |
/// | `incident:2` | [`ItemRef::Incident`]`(2)` |
/// | `ann:a-3f9c1e02`, `bug:a-3f9c1e02`, `note:a-3f9c1e02`, `finding:a-3f9c1e02`, `a-3f9c1e02` | [`ItemRef::Annotation`]`("a-3f9c1e02")` |
///
/// The kind of an annotation (a bug region, a note or a finding) is not part of the reference:
/// all four prefixes, and the bare id, are the same reference, and it prints as `ann:<id>`.
///
/// # Errors
///
/// Parsing fails with a usage error (the exit kind `Usage` of the `envelope` module) when:
///
/// - the text, trimmed of surrounding whitespace, is neither an annotation id nor has a `:`:
///   `'<text>' is not an item reference`;
/// - the prefix is not `event`, `log`, `incident`, `ann`, `bug`, `note` or `finding`:
///   `unknown reference '<text>'`;
/// - the part after `event:`, `log:` or `incident:` is not a whole number (so it is not empty,
///   negative or fractional), or does not fit in a `u64`;
/// - the part after `ann:`, `bug:`, `note:` or `finding:` is not an annotation id:
///   `'<id>' is not an annotation id like a-3f9c1e02`.
///
/// # Example
///
/// ```rust
/// use komorebi_recorder::analysis::ids::ItemRef;
///
/// let event: ItemRef = "event:12".parse()?;
/// assert_eq!(event, ItemRef::Event(12));
/// assert_eq!(event.to_string(), "event:12");
///
/// // the kind of annotation does not matter, and a bare id is one
/// let note: ItemRef = "note:a-3f9c1e02".parse()?;
/// assert_eq!(note, "bug:a-3f9c1e02".parse()?);
/// assert_eq!(note, "a-3f9c1e02".parse()?);
/// assert_eq!(note.to_string(), "ann:a-3f9c1e02");
///
/// // mistakes are errors that say what was wrong
/// let error = "event:twelve".parse::<ItemRef>().unwrap_err();
/// assert_eq!(
///     error.to_string(),
///     "'event:twelve': an event sequence number must be a whole number"
/// );
/// let error = "frame:3".parse::<ItemRef>().unwrap_err();
/// assert!(error.to_string().starts_with("unknown reference 'frame:3'"));
/// # Ok::<(), Box<dyn std::error::Error>>(())
/// ```
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ItemRef {
    /// A komorebi notification, by its [`seq`](super::events::EventRecord::seq).
    Event(u64),
    /// A line of the session's recorded `komorebi.log`, 1-based.
    Log(u64),
    /// A line of the recorder's incident log, 1-based.
    Incident(u64),
    /// An annotation (a bug region, a note or a finding), by its id: `a-` and eight lowercase hex
    /// digits.
    Annotation(String),
}

impl fmt::Display for ItemRef {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Event(seq) => write!(f, "event:{seq}"),
            Self::Log(line) => write!(f, "log:{line}"),
            Self::Incident(number) => write!(f, "incident:{number}"),
            Self::Annotation(id) => write!(f, "ann:{id}"),
        }
    }
}

impl ItemRef {
    /// Returns whether `input` starts with one of the prefixes [`ItemRef`] understands: `event`,
    /// `log`, `incident`, `ann`, `bug`, `note` or `finding`, followed by a `:`.
    ///
    /// The part after the colon is not looked at, so a true answer does not mean that `input`
    /// parses. A bare annotation id has no prefix; use [`is_annotation_id`] for that.
    ///
    /// # Example
    ///
    /// ```rust
    /// use komorebi_recorder::analysis::ids::ItemRef;
    ///
    /// assert!(ItemRef::has_prefix("event:12"));
    /// assert!(ItemRef::has_prefix("event:twelve")); // has the prefix, but would not parse
    /// assert!(!ItemRef::has_prefix("a-3f9c1e02"));
    /// assert!(!ItemRef::has_prefix("12.5"));
    /// ```
    #[must_use]
    pub fn has_prefix(input: &str) -> bool {
        input
            .split_once(':')
            .is_some_and(|(prefix, _)| PREFIXES.contains(&prefix))
    }
}

const PREFIXES: [&str; 7] = ["event", "log", "incident", "ann", "bug", "note", "finding"];

impl FromStr for ItemRef {
    type Err = eyre::Report;

    fn from_str(input: &str) -> Result<Self, Self::Err> {
        let input = input.trim();
        // a bare annotation id is fine
        if is_annotation_id(input) {
            return Ok(Self::Annotation(input.to_string()));
        }

        let (prefix, rest) = input
            .split_once(':')
            .ok_or_else(|| super::envelope::usage(format!("'{input}' is not an item reference")))?;
        let number = |what: &str| -> eyre::Result<u64> {
            rest.parse().map_err(|_| {
                super::envelope::usage(format!("'{input}': {what} must be a whole number"))
            })
        };

        match prefix {
            "event" => Ok(Self::Event(number("an event sequence number")?)),
            "log" => Ok(Self::Log(number("a log line")?)),
            "incident" => Ok(Self::Incident(number("an incident number")?)),
            "ann" | "bug" | "note" | "finding" if is_annotation_id(rest) => {
                Ok(Self::Annotation(rest.to_string()))
            }
            "ann" | "bug" | "note" | "finding" => Err(super::envelope::usage(format!(
                "'{rest}' is not an annotation id like a-3f9c1e02"
            ))),
            _ => Err(super::envelope::usage(format!(
                "unknown reference '{input}': expected event:<seq>, log:<line>, incident:<n> or bug:<id>"
            ))),
        }
    }
}

/// Returns whether `text` is an annotation id: `a-` and exactly eight lowercase hexadecimal digits.
///
/// Uppercase digits are not accepted, and nothing around the id is trimmed.
///
/// # Example
///
/// ```rust
/// use komorebi_recorder::analysis::ids::is_annotation_id;
///
/// assert!(is_annotation_id("a-3f9c1e02"));
/// assert!(!is_annotation_id("a-3F9C1E02")); // lowercase only
/// assert!(!is_annotation_id("a-3f9c1e0")); // eight digits, not seven
/// assert!(!is_annotation_id("bug:a-3f9c1e02"));
/// ```
#[must_use]
pub fn is_annotation_id(text: &str) -> bool {
    text.strip_prefix("a-").is_some_and(|hex| {
        hex.len() == 8
            && hex
                .bytes()
                .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
    })
}

/// Returns a fresh annotation id, one that [`is_annotation_id`] accepts.
///
/// The id is random per process (the standard library seeds its hasher from the OS) and mixed with
/// the clock and the process id, so two recorders marking at once do not collide in practice.
/// That is not guaranteed: an id has 32 bits, and nothing here checks that an id is unused.
///
/// # Example
///
/// ```rust
/// use komorebi_recorder::analysis::ids::{is_annotation_id, new_annotation_id};
///
/// let id = new_annotation_id();
/// assert!(is_annotation_id(&id));
/// assert_eq!(id.len(), 10); // `a-` and eight digits
/// ```
#[must_use]
pub fn new_annotation_id() -> String {
    let mut hasher = RandomState::new().build_hasher();
    hasher.write_u64(crate::clock::mono_now_ns());
    hasher.write_u32(std::process::id());
    format!("a-{:08x}", hasher.finish() as u32)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn references_round_trip() {
        for text in ["event:12", "log:345", "incident:2", "ann:a-3f9c1e02"] {
            let item: ItemRef = text.parse().expect(text);
            assert_eq!(item.to_string(), text);
        }
    }

    #[test]
    fn annotation_kinds_and_bare_ids_are_aliases() {
        let expected = ItemRef::Annotation("a-3f9c1e02".to_string());
        for text in [
            "bug:a-3f9c1e02",
            "note:a-3f9c1e02",
            "finding:a-3f9c1e02",
            "a-3f9c1e02",
        ] {
            assert_eq!(text.parse::<ItemRef>().expect(text), expected);
        }
    }

    #[test]
    fn bad_references_are_usage_errors() {
        for text in [
            "event:x",
            "log:",
            "bug:123",
            "frame:3",
            "nonsense",
            "bug:a-XYZ",
        ] {
            let error = text.parse::<ItemRef>().expect_err(text);
            assert_eq!(
                super::super::envelope::exit_kind(&error),
                super::super::envelope::ExitKind::Usage,
                "{text}"
            );
        }
    }

    #[test]
    fn new_ids_look_right_and_do_not_collide() {
        let mut seen = std::collections::HashSet::new();
        for _ in 0..1000 {
            let id = new_annotation_id();
            assert!(is_annotation_id(&id), "{id}");
            seen.insert(id);
        }
        assert!(seen.len() > 990, "only {} distinct ids", seen.len());
    }
}
