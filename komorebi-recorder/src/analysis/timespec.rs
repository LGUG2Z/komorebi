//! Naming a moment: every command that takes a time takes any of these.
//!
//! | form | meaning |
//! |---|---|
//! | `12.5`, `1:02.5`, `-1.5` | video time, see [`crate::timecode`] |
//! | `wall:2026-10-26T20:00:00Z` | a UTC time (RFC 3339) |
//! | `logtime:15:46:22.743`, `wall:15:46:22.743` | a UTC time of day, as komorebi's log writes it, on the session's date |
//! | `mach:123456789` | the recorder's master clock, in nanoseconds |
//! | `event:12`, `log:345`, `incident:2` | when that record happened |
//! | `bug:a-3f9c1e02`, `bug:a-3f9c1e02.start`, `bug:a-3f9c1e02.end` | an annotation's start or end |
//!
//! Any of them can be followed by `+3s` or `-500ms` to move it. The units are `ms`, `s` and `m`
//! (minutes), and an offset without one (`12.5+3`) is not an offset. An event or annotation can be
//! followed by `.start` or `.end` first (`bug:a-3f9c1e02.end+2s`); the default is the start. The
//! suffix is accepted after any item reference, but only an event and an annotation have two edges:
//! on a log line or an incident it changes nothing.
//!
//! # Log time
//!
//! komorebi's log timestamps are UTC, and a log line is a natural thing to quote: `logtime:15:46:22.743`
//! (`log-time:` is the same) names that time of day, `HH:MM:SS` with an optional fraction of 1 to 6
//! digits (milliseconds, or microseconds). `wall:` followed by a time of day instead of an RFC 3339
//! time (`wall:15:46:22.743`) means the same. It needs no date: it is taken from the session, on the
//! date the recording started, and if the recording crosses midnight UTC, on the date on which the
//! time falls inside the recording. A time of day outside the recording is placed on the date
//! nearest to it, so a log line from just before the recording began is just before it.
//!
//! A bare `15:46:22.743` is never a log time: it is video time, `1:02:03` is one hour, two minutes
//! and three seconds into the video, and a time of day is not told apart from that by its digits (a
//! recording can be two hours long, and `01:02:03` is both). So log time always has its prefix, and
//! a prefix never changes what a name without one means.
//!
//! Resolving a name never guesses: an unknown event, a log line without a timestamp or an
//! annotation that does not exist is an error with the "not found" exit code. The one exception is
//! an annotation whose end was never set, which ends when the recording did.
//!
//! # How a name is read
//!
//! A name is read in two steps, and both are in this module. [`TimeSpec`]'s [`FromStr`] splits off
//! the offset and reads the rest as an [`Anchor`], with no session needed: this checks the syntax
//! only. [`TimeSpec::resolve`] then turns the anchor into a moment on the master clock with the
//! session's [`Manifest`] (for video time and wall time) and a [`Lookup`] (for the things a name
//! can point at), and adds the offset. The result is a [`Resolved`], on both the master clock and
//! the video timeline.
//!
//! The master clock is the host's master monotonic clock, in nanoseconds; fields named
//! `*_mach_ns` are readings of it (`mach_` is a historical spelling, and the manifest's
//! `host.clock` says `mach`, `qpc` or `std`).
//!
//! # Example
//!
//! ```rust
//! use komorebi_recorder::analysis::ids::ItemRef;
//! use komorebi_recorder::analysis::timespec::{Anchor, Edge, TimeSpec};
//!
//! // a video time, moved three seconds later
//! let spec: TimeSpec = "1:02.5+3s".parse()?;
//! assert_eq!(spec.anchor, Anchor::Video(62.5));
//! assert_eq!(spec.offset_s, 3.0);
//!
//! // when komorebi finished handling event 12, half a second before
//! let spec: TimeSpec = "event:12.end-500ms".parse()?;
//! assert_eq!(
//!     spec.anchor,
//!     Anchor::Item { item: ItemRef::Event(12), edge: Edge::End }
//! );
//! assert_eq!(spec.offset_s, -0.5);
//!
//! // a UTC time. The `+01:00` zone is not an offset: it does not end in a unit.
//! let spec: TimeSpec = "wall:2026-10-26T20:00:00+01:00".parse()?;
//! assert_eq!(spec.offset_s, 0.0);
//!
//! // nonsense is an error that describes what was expected
//! let error = "soon".parse::<TimeSpec>().unwrap_err();
//! assert_eq!(error.to_string(), "invalid timecode 'soon'");
//! # Ok::<(), Box<dyn std::error::Error>>(())
//! ```

use super::envelope;
use super::ids::ItemRef;
use super::ids::is_annotation_id;
use crate::clock::ClockMap;
use crate::session::Manifest;
use crate::timecode;
use chrono::DateTime;
use chrono::NaiveTime;
use chrono::Timelike;
use chrono::Utc;
use color_eyre::eyre;
use std::str::FromStr;

/// Which end of a span a name means: an event or an annotation has a start and an end.
///
/// The default is [`Edge::Start`], which is what a name with no `.start` or `.end` means.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Edge {
    /// The start: for an event, when komorebi began handling it, where the log has it; for an
    /// annotation, its start.
    #[default]
    Start,
    /// The end: for an event, when the notification was received; for an annotation, its end, or
    /// the end of the recording if it has none.
    End,
}

/// The point in the session a [`TimeSpec`] starts from, before its offset is added.
#[derive(Debug, Clone, PartialEq)]
pub enum Anchor {
    /// A time on the video timeline, in seconds: `12.5`. It is negative before the video's `t = 0`.
    Video(f64),
    /// A UTC time (`wall:...`), mapped to the master clock through the session's clock anchors.
    Wall(DateTime<Utc>),
    /// A UTC time of day with no date (`logtime:15:46:22.743`, `wall:15:46:22.743`): komorebi's log
    /// timestamps without the day. [`TimeSpec::resolve`] gives it the session's date.
    TimeOfDay(NaiveTime),
    /// A reading of the master clock in nanoseconds (`mach:...`).
    Mach(u64),
    /// When a recorded item happened: an event, a log line, an incident or an annotation. `edge`
    /// picks the start or the end of an event or an annotation.
    Item {
        /// The thing pointed at.
        item: ItemRef,
        /// Which end of it.
        edge: Edge,
    },
}

/// A parsed time argument: an [`Anchor`] and an offset to move it by.
///
/// Parse one from text with [`str::parse`], which needs no session, and turn it into a moment with
/// [`TimeSpec::resolve`]. The module documentation has the forms a time can take.
///
/// # Errors
///
/// Parsing fails with a usage error (the exit kind `Usage` of the `envelope` module), whose
/// message describes what was expected, when:
///
/// - the text is empty or only whitespace: `an empty time`;
/// - a `wall:` time is not RFC 3339: `'<text>': expected an RFC 3339 time like
///   wall:2026-10-26T19:43:35.062613Z`;
/// - a `logtime:` (or `log-time:`) time is not `HH:MM:SS` with an optional fraction of 1 to 6
///   digits: `'<text>': expected a UTC time of day like logtime:15:46:22.743`;
/// - a `mach:` time is not a whole number of nanoseconds: `'<text>': mach: takes nanoseconds, e.g.
///   mach:123456`;
/// - the text starts like an item reference (`event:`, `log:`, `incident:`, `ann:`, `bug:`,
///   `note:`, `finding:`) or is an annotation id, and does not parse as one: see [`ItemRef`];
/// - anything else is not a video timecode: `invalid timecode '<text>'`, where the text is what
///   remains after an offset was split off (see [`timecode::parse`]).
///
/// Nothing about the session is checked here: `event:99999` parses.
///
/// # Example
///
/// ```rust
/// use komorebi_recorder::analysis::timespec::{Anchor, TimeSpec};
///
/// let spec: TimeSpec = "mach:5-9s".parse()?;
/// assert_eq!(spec.anchor, Anchor::Mach(5));
/// assert_eq!(spec.offset_s, -9.0);
///
/// // an offset needs a unit, so this is not an offset and not a timecode
/// assert!("12.5+3".parse::<TimeSpec>().is_err());
///
/// // a time of day as komorebi's log writes it needs its prefix: without it, this is video time
/// let spec: TimeSpec = "logtime:15:46:22.743+1s".parse()?;
/// assert!(matches!(spec.anchor, Anchor::TimeOfDay(_)));
/// assert_eq!("1:02:03".parse::<TimeSpec>()?.anchor, Anchor::Video(3723.0));
/// # Ok::<(), Box<dyn std::error::Error>>(())
/// ```
#[derive(Debug, Clone, PartialEq)]
pub struct TimeSpec {
    /// Where the name points, before the offset.
    pub anchor: Anchor,
    /// Added to the anchor, in seconds; negative to move earlier. 0 when the name has no offset.
    pub offset_s: f64,
}

/// A moment on both clocks: what a [`TimeSpec`] resolves to.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Resolved {
    /// The moment on the host's master monotonic clock, in nanoseconds (`mach_` is a historical
    /// spelling). It is never negative: a moment before the clock's zero is 0.
    pub mach_ns: u64,
    /// The same moment on the video timeline, in seconds since the video's `t = 0`. Negative before
    /// the video started.
    pub video_s: f64,
}

/// The lookups a [`TimeSpec`] needs from a session: when an event, a log line, an incident or an
/// annotation happened.
///
/// [`TimeSpec::resolve`] calls it for an [`Anchor::Item`]. It is implemented over the real files by
/// the analysis facade (`Analysis`) and by fakes in tests, so a name can be resolved without a
/// session on disk. Every method returns nanoseconds on the master clock, and has no default: an
/// implementor writes all four.
///
/// An item that does not exist, or has no time, should be a not-found error (the `NotFound` exit
/// kind of the `envelope` module), which [`TimeSpec::resolve`] passes on, so that a name never
/// resolves to a guess. Nothing enforces that for an implementor.
///
/// # Example
///
/// A lookup with one event and one annotation, and a manifest whose video starts at 1 second
/// on the master clock:
///
/// ```rust
/// use color_eyre::eyre;
/// use komorebi_recorder::analysis::timespec::{Edge, Lookup, TimeSpec};
/// use komorebi_recorder::clock::Anchor;
/// use komorebi_recorder::session::{Manifest, Moment};
///
/// struct Known;
///
/// impl Lookup for Known {
///     fn event_mach_ns(&self, seq: u64, edge: Edge) -> eyre::Result<u64> {
///         match (seq, edge) {
///             (1, Edge::Start) => Ok(4_000_000_000),
///             (1, Edge::End) => Ok(4_500_000_000),
///             _ => eyre::bail!("no event #{seq}"),
///         }
///     }
///     fn log_mach_ns(&self, line: u64) -> eyre::Result<u64> {
///         eyre::bail!("no log line {line}")
///     }
///     fn incident_mach_ns(&self, number: u64) -> eyre::Result<u64> {
///         eyre::bail!("no incident {number}")
///     }
///     fn annotation_span(&self, id: &str) -> eyre::Result<(u64, Option<u64>)> {
///         match id {
///             "a-00000001" => Ok((2_000_000_000, None)), // never closed
///             _ => eyre::bail!("no annotation {id}"),
///         }
///     }
/// }
///
/// let start = Moment { mach_ns: 1_000_000_000, wall_us: 1_000_000 };
/// let mut manifest = Manifest::new(
///     start,
///     vec![Anchor { mach_ns: 1_000_000_000, wall_us: 1_000_000 }],
/// );
/// manifest.video_t0_mach_ns = Some(1_000_000_000);
/// manifest.stop = Some(Moment { mach_ns: 10_000_000_000, wall_us: 10_000_000 });
///
/// let resolve = |text: &str| text.parse::<TimeSpec>()?.resolve(&manifest, &Known);
///
/// // the end of event 1 is 4.5 s on the master clock, which is 3.5 s into the video
/// let resolved = resolve("event:1.end")?;
/// assert_eq!(resolved.mach_ns, 4_500_000_000);
/// assert_eq!(resolved.video_s, 3.5);
///
/// // an annotation that was never closed ends when the recording did
/// assert_eq!(resolve("bug:a-00000001.end")?.mach_ns, 10_000_000_000);
///
/// // an item the lookup does not know is an error, not a guess
/// assert!(resolve("event:2").is_err());
/// # Ok::<(), Box<dyn std::error::Error>>(())
/// ```
pub trait Lookup {
    /// Returns when event `seq` happened. [`Edge::Start`] is when komorebi began handling it where
    /// known, [`Edge::End`] when the notification was received.
    ///
    /// # Errors
    ///
    /// Returns an error if the session has no such event.
    fn event_mach_ns(&self, seq: u64, edge: Edge) -> eyre::Result<u64>;

    /// Returns when line `line` of komorebi's recorded log was written. Lines are numbered from 1.
    ///
    /// # Errors
    ///
    /// Returns an error if the log has no such line, or the line has no time.
    fn log_mach_ns(&self, line: u64) -> eyre::Result<u64>;

    /// Returns when incident `number` of the recorder's incident log happened. Incidents are
    /// numbered from 1.
    ///
    /// # Errors
    ///
    /// Returns an error if the session recorded no incidents, or has no such one.
    fn incident_mach_ns(&self, number: u64) -> eyre::Result<u64>;

    /// Returns an annotation's `(start, end)`; the end is `None` when it has none.
    ///
    /// # Errors
    ///
    /// Returns an error if the session has no annotation with this id.
    fn annotation_span(&self, id: &str) -> eyre::Result<(u64, Option<u64>)>;
}

impl FromStr for TimeSpec {
    type Err = eyre::Report;

    fn from_str(input: &str) -> Result<Self, Self::Err> {
        let trimmed = input.trim();
        if trimmed.is_empty() {
            return Err(envelope::usage("an empty time"));
        }

        let (head, offset_s) = split_offset(trimmed);
        Ok(Self {
            anchor: parse_anchor(head, input)?,
            offset_s,
        })
    }
}

/// Split a trailing `+3s` / `-500ms` off. The tail must end in a unit, so a `+02:00` time zone or
/// a `-` inside an id is never taken for an offset.
fn split_offset(text: &str) -> (&str, f64) {
    for (index, character) in text.char_indices().rev() {
        if index == 0 || (character != '+' && character != '-') {
            continue;
        }

        let tail = &text[index + 1..];
        if !(tail.ends_with('s') || tail.ends_with('m')) {
            continue;
        }

        if let Ok(span) = timecode::parse_span(tail) {
            let signed = if character == '-' { -span } else { span };
            return (&text[..index], signed);
        }
    }

    (text, 0.0)
}

/// Reads `HH:MM:SS` with an optional fraction of 1 to 6 digits.
fn parse_time_of_day(text: &str) -> Option<NaiveTime> {
    let (clock, fraction) = match text.split_once('.') {
        Some((clock, fraction)) => (clock, Some(fraction)),
        None => (text, None),
    };

    let mut parts = clock.split(':');
    let (hours, minutes, seconds) = (parts.next()?, parts.next()?, parts.next()?);
    if parts.next().is_some() {
        return None;
    }

    let number = |part: &str, width: std::ops::RangeInclusive<usize>| {
        (width.contains(&part.len()) && part.bytes().all(|byte| byte.is_ascii_digit()))
            .then(|| part.parse::<u32>().ok())
            .flatten()
    };
    let micros = match fraction {
        None => 0,
        Some(digits) => {
            let value = number(digits, 1..=6)?;
            // `.5` is half a second, not five microseconds
            value * 10_u32.pow(6 - u32::try_from(digits.len()).ok()?)
        }
    };

    NaiveTime::from_hms_micro_opt(
        number(hours, 1..=2)?,
        number(minutes, 2..=2)?,
        number(seconds, 2..=2)?,
        micros,
    )
}

fn parse_anchor(head: &str, original: &str) -> eyre::Result<Anchor> {
    if let Some(rest) = head
        .strip_prefix("logtime:")
        .or_else(|| head.strip_prefix("log-time:"))
    {
        return parse_time_of_day(rest)
            .map(Anchor::TimeOfDay)
            .ok_or_else(|| {
                envelope::usage(format!(
                    "'{original}': expected a UTC time of day like logtime:15:46:22.743"
                ))
            });
    }

    if let Some(rest) = head.strip_prefix("wall:") {
        // `wall:15:46:22.743` is a time of day; anything else is read as RFC 3339
        if let Some(time) = parse_time_of_day(rest) {
            return Ok(Anchor::TimeOfDay(time));
        }

        let time = DateTime::parse_from_rfc3339(rest).map_err(|_| {
            envelope::usage(format!(
                "'{original}': expected an RFC 3339 time like wall:2026-10-26T19:43:35.062613Z"
            ))
        })?;
        return Ok(Anchor::Wall(time.with_timezone(&Utc)));
    }

    if let Some(rest) = head.strip_prefix("mach:") {
        let ns = rest.parse().map_err(|_| {
            envelope::usage(format!(
                "'{original}': mach: takes nanoseconds, e.g. mach:123456"
            ))
        })?;
        return Ok(Anchor::Mach(ns));
    }

    let (body, edge) = match head
        .strip_suffix(".start")
        .map(|body| (body, Edge::Start))
        .or_else(|| head.strip_suffix(".end").map(|body| (body, Edge::End)))
    {
        Some((body, edge)) => (body, edge),
        None => (head, Edge::Start),
    };

    if ItemRef::has_prefix(body) || is_annotation_id(body) {
        return Ok(Anchor::Item {
            item: body.parse()?,
            edge,
        });
    }

    timecode::parse(head)
        .map(Anchor::Video)
        .map_err(|error| envelope::usage(format!("{error}")))
}

impl TimeSpec {
    /// Returns the moment this name points at in `manifest`'s session, on the master clock and on
    /// the video timeline.
    ///
    /// The anchor is turned into a reading of the master clock (video time through the manifest's
    /// video `t = 0`, wall time through its clock anchors, an item through `lookup`), the offset is
    /// added, and a result below 0 becomes 0.
    ///
    /// # Errors
    ///
    /// - The `Unusable` exit kind, `the session has no clock anchors`, if the anchor is a `wall:`
    ///   time and the manifest has no clock anchors to map it with.
    /// - Whatever `lookup` returns for an [`Anchor::Item`]: an event, log line, incident or
    ///   annotation that does not exist is a not-found error (see [`Lookup`]).
    ///
    /// # Example
    ///
    /// A moment before the clock started clamps to 0 instead of failing. See [`Lookup`] for an
    /// example with items.
    ///
    /// ```rust
    /// use color_eyre::eyre;
    /// use komorebi_recorder::analysis::timespec::{Edge, Lookup, TimeSpec};
    /// use komorebi_recorder::session::{Manifest, Moment};
    ///
    /// struct Nothing;
    ///
    /// impl Lookup for Nothing {
    ///     fn event_mach_ns(&self, seq: u64, _: Edge) -> eyre::Result<u64> {
    ///         eyre::bail!("no event #{seq}")
    ///     }
    ///     fn log_mach_ns(&self, line: u64) -> eyre::Result<u64> {
    ///         eyre::bail!("no log line {line}")
    ///     }
    ///     fn incident_mach_ns(&self, number: u64) -> eyre::Result<u64> {
    ///         eyre::bail!("no incident {number}")
    ///     }
    ///     fn annotation_span(&self, id: &str) -> eyre::Result<(u64, Option<u64>)> {
    ///         eyre::bail!("no annotation {id}")
    ///     }
    /// }
    ///
    /// let manifest = Manifest::new(Moment { mach_ns: 0, wall_us: 0 }, Vec::new());
    ///
    /// let resolved = "mach:5-9s".parse::<TimeSpec>()?.resolve(&manifest, &Nothing)?;
    /// assert_eq!(resolved.mach_ns, 0);
    ///
    /// // a wall time needs clock anchors to be mapped with, and this manifest has none
    /// let error = "wall:2026-10-26T20:00:00Z"
    ///     .parse::<TimeSpec>()?
    ///     .resolve(&manifest, &Nothing)
    ///     .unwrap_err();
    /// assert_eq!(error.to_string(), "the session has no clock anchors");
    /// # Ok::<(), Box<dyn std::error::Error>>(())
    /// ```
    pub fn resolve(&self, manifest: &Manifest, lookup: &dyn Lookup) -> eyre::Result<Resolved> {
        let base_ns = match &self.anchor {
            Anchor::Video(seconds) => manifest.mach_at_video_seconds(*seconds),
            Anchor::Mach(ns) => *ns,
            Anchor::Wall(time) => ClockMap::new(manifest.anchors.clone())
                .wall_to_mach(time.timestamp_micros())
                .ok_or_else(|| envelope::unusable("the session has no clock anchors"))?,
            Anchor::TimeOfDay(time) => ClockMap::new(manifest.anchors.clone())
                .wall_to_mach(session_wall_us(*time, manifest))
                .ok_or_else(|| envelope::unusable("the session has no clock anchors"))?,
            Anchor::Item { item, edge } => match item {
                ItemRef::Event(seq) => lookup.event_mach_ns(*seq, *edge)?,
                ItemRef::Log(line) => lookup.log_mach_ns(*line)?,
                ItemRef::Incident(number) => lookup.incident_mach_ns(*number)?,
                ItemRef::Annotation(id) => {
                    let (start, end) = lookup.annotation_span(id)?;
                    match edge {
                        Edge::Start => start,
                        // an annotation that was never closed lasts until the recording ended
                        Edge::End => end.unwrap_or_else(|| session_end_ns(manifest)),
                    }
                }
            },
        };

        let shifted = i128::from(base_ns) + (self.offset_s * 1e9).round() as i128;
        let mach_ns = u64::try_from(shifted.max(0)).unwrap_or(u64::MAX);
        Ok(Resolved {
            mach_ns,
            video_s: manifest.video_seconds(mach_ns),
        })
    }
}

/// Returns the wall-clock time, in microseconds since the epoch, of UTC time of day `time` on the
/// date that suits the session: the date on which it falls inside the recording (the recording can
/// cross midnight), or else the date on which it is nearest to the recording.
fn session_wall_us(time: NaiveTime, manifest: &Manifest) -> i64 {
    const DAY_US: i64 = 86_400_000_000;

    let first = manifest.start.wall_us;
    let last = manifest
        .stop
        .map(|stop| stop.wall_us)
        .into_iter()
        .chain(manifest.anchors.iter().map(|anchor| anchor.wall_us))
        .fold(first, i64::max);
    let of_day = i64::from(time.num_seconds_from_midnight()) * 1_000_000
        + i64::from(time.nanosecond() / 1000);

    (first.div_euclid(DAY_US) - 1..=last.div_euclid(DAY_US) + 1)
        .map(|day| day * DAY_US + of_day)
        // how far outside the recording: 0 inside it
        .min_by_key(|wall_us| (first - wall_us).max(wall_us - last).max(0))
        .unwrap_or(first)
}

/// Returns when the recording ended on the master clock, in nanoseconds.
///
/// That is the manifest's `stop`, else when video capture ended, else when the recording started.
/// A recording that has not stopped and has no video therefore ends where it began.
#[must_use]
pub fn session_end_ns(manifest: &Manifest) -> u64 {
    manifest
        .stop
        .map(|stop| stop.mach_ns)
        .or(manifest.video_end_mach_ns)
        .unwrap_or(manifest.start.mach_ns)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::clock::Anchor as ClockAnchor;
    use crate::session::Moment;

    struct Fake;

    impl Lookup for Fake {
        fn event_mach_ns(&self, seq: u64, edge: Edge) -> eyre::Result<u64> {
            match (seq, edge) {
                (1, Edge::Start) => Ok(4_000_000_000),
                (1, Edge::End) => Ok(4_500_000_000),
                _ => Err(envelope::not_found(format!("no event {seq}"))),
            }
        }

        fn log_mach_ns(&self, line: u64) -> eyre::Result<u64> {
            Ok(1_000_000_000 * line)
        }

        fn incident_mach_ns(&self, number: u64) -> eyre::Result<u64> {
            Err(envelope::not_found(format!("no incident {number}")))
        }

        fn annotation_span(&self, id: &str) -> eyre::Result<(u64, Option<u64>)> {
            match id {
                "a-00000001" => Ok((2_000_000_000, Some(3_000_000_000))),
                "a-00000002" => Ok((2_000_000_000, None)),
                _ => Err(envelope::not_found(format!("no annotation {id}"))),
            }
        }
    }

    /// Video t=0 is mach 1s; wall `1_000_000us` is mach 1s; the session ended at mach 10s.
    fn manifest() -> Manifest {
        let mut manifest = Manifest::new(
            Moment {
                mach_ns: 1_000_000_000,
                wall_us: 1_000_000,
            },
            vec![ClockAnchor {
                mach_ns: 1_000_000_000,
                wall_us: 1_000_000,
            }],
        );
        manifest.video_t0_mach_ns = Some(1_000_000_000);
        manifest.stop = Some(Moment {
            mach_ns: 10_000_000_000,
            wall_us: 10_000_000,
        });
        manifest
    }

    fn resolve(text: &str) -> eyre::Result<Resolved> {
        text.parse::<TimeSpec>()?.resolve(&manifest(), &Fake)
    }

    fn close(a: f64, b: f64) -> bool {
        (a - b).abs() < 1e-6
    }

    #[test]
    fn video_times_resolve_against_the_manifest() {
        let resolved = resolve("2.5").expect("resolve");
        assert_eq!(resolved.mach_ns, 3_500_000_000);
        assert!(close(resolved.video_s, 2.5));
        assert!(close(resolve("0:02.5").expect("clock").video_s, 2.5));
    }

    #[test]
    fn offsets_move_any_anchor() {
        assert!(close(resolve("2.5+1s").expect("plus").video_s, 3.5));
        assert!(close(resolve("2.5-500ms").expect("minus").video_s, 2.0));
        assert!(close(
            resolve("event:1.end-1s").expect("event").video_s,
            2.5
        ));
        // video t=0 is mach 1s here, so half a second before it is still on the clock
        assert!(close(
            resolve("-0.5").expect("negative video").video_s,
            -0.5
        ));
    }

    #[test]
    fn mach_and_wall_times_resolve() {
        assert!(close(
            resolve("mach:3000000000").expect("mach").video_s,
            2.0
        ));
        // wall 5s after the epoch is 4s after the anchor at wall 1s: mach 5s, video 4s
        let wall = resolve("wall:1970-01-01T00:00:05Z").expect("wall");
        assert!(close(wall.video_s, 4.0));
        // a numeric zone is not an offset
        let zoned = resolve("wall:1970-01-01T01:00:05+01:00").expect("zoned");
        assert!(close(zoned.video_s, 4.0));
        let moved = resolve("wall:1970-01-01T00:00:05Z+2s").expect("moved");
        assert!(close(moved.video_s, 6.0));
    }

    #[test]
    fn items_resolve_through_the_lookup() {
        assert!(close(resolve("event:1").expect("start").video_s, 3.0));
        assert!(close(resolve("event:1.end").expect("end").video_s, 3.5));
        assert!(close(resolve("log:7").expect("log").video_s, 6.0));
        assert!(close(resolve("bug:a-00000001").expect("bug").video_s, 1.0));
        assert!(close(
            resolve("bug:a-00000001.end").expect("bug end").video_s,
            2.0
        ));
        assert!(close(
            resolve("a-00000001.end+1s").expect("bare id").video_s,
            3.0
        ));
    }

    #[test]
    fn an_unclosed_annotation_ends_with_the_session() {
        let end = resolve("bug:a-00000002.end").expect("open-ended");
        assert_eq!(end.mach_ns, 10_000_000_000);
    }

    #[test]
    fn missing_things_are_not_found_not_guesses() {
        for text in ["event:99", "incident:1", "bug:a-0000ffff"] {
            let error = resolve(text).expect_err(text);
            assert_eq!(
                envelope::exit_kind(&error),
                envelope::ExitKind::NotFound,
                "{text}"
            );
        }
    }

    #[test]
    fn nonsense_is_a_usage_error() {
        for text in [
            "",
            "soon",
            "wall:yesterday",
            "mach:x",
            "event:one",
            "1:2:3:4",
        ] {
            let error = text
                .parse::<TimeSpec>()
                .map(|_| ())
                .expect_err(&format!("{text:?} should not parse"));
            assert_eq!(
                envelope::exit_kind(&error),
                envelope::ExitKind::Usage,
                "{text:?}"
            );
        }
    }

    #[test]
    fn a_log_time_is_a_utc_time_of_day_on_the_sessions_date() {
        // wall 5 s after the epoch is 4 s into the video
        for text in [
            "logtime:00:00:05",
            "log-time:00:00:05.000",
            "wall:00:00:05",
            "logtime:0:00:05.0",
        ] {
            let resolved = resolve(text).expect(text);
            assert!(close(resolved.video_s, 4.0), "{text}");
        }
        assert!(close(
            resolve("logtime:00:00:05.250").expect("ms").video_s,
            4.25
        ));
        assert!(close(
            resolve("logtime:00:00:05.000250").expect("us").video_s,
            4.00025
        ));
        assert!(close(
            resolve("logtime:00:00:05+2s").expect("offset").video_s,
            6.0
        ));
        assert!(close(
            resolve("wall:00:00:05-500ms").expect("offset").video_s,
            3.5
        ));
    }

    #[test]
    fn a_bare_time_of_day_is_still_video_time() {
        assert_eq!(
            "1:02:03".parse::<TimeSpec>().expect("clock").anchor,
            Anchor::Video(3723.0)
        );
        assert_eq!(
            "1:02.5".parse::<TimeSpec>().expect("minutes").anchor,
            Anchor::Video(62.5)
        );
        assert_eq!(
            "12.5".parse::<TimeSpec>().expect("seconds").anchor,
            Anchor::Video(12.5)
        );
        assert!(matches!(
            "wall:2026-10-26T20:00:00Z"
                .parse::<TimeSpec>()
                .expect("rfc")
                .anchor,
            Anchor::Wall(_)
        ));
    }

    #[test]
    fn malformed_log_times_are_usage_errors() {
        for text in [
            "logtime:",
            "logtime:12:00",
            "logtime:25:00:00",
            "logtime:12:60:00",
            "logtime:12:00:00.",
            "logtime:12:00:00.1234567",
            "logtime:12:00:00:00",
            "logtime:noon",
            "wall:12:00:99",
        ] {
            let error = text.parse::<TimeSpec>().expect_err(text);
            assert_eq!(
                envelope::exit_kind(&error),
                envelope::ExitKind::Usage,
                "{text}"
            );
        }
    }

    #[test]
    fn a_log_time_takes_the_date_on_which_it_falls_inside_a_recording_that_crosses_midnight() {
        let day = 86_400_000_000_i64;
        let start = 20_000 * day + (23 * 3600 + 59 * 60 + 50) * 1_000_000;
        let mut manifest = Manifest::new(
            Moment {
                mach_ns: 100_000_000_000,
                wall_us: start,
            },
            vec![ClockAnchor {
                mach_ns: 100_000_000_000,
                wall_us: start,
            }],
        );
        manifest.video_t0_mach_ns = Some(100_000_000_000);
        manifest.stop = Some(Moment {
            mach_ns: 130_000_000_000,
            wall_us: start + 30_000_000,
        });
        let video = |text: &str| {
            text.parse::<TimeSpec>()
                .and_then(|spec| spec.resolve(&manifest, &Fake))
                .expect(text)
                .video_s
        };

        // before midnight: the day the recording started
        assert!(close(video("logtime:23:59:55"), 5.0));
        // after midnight: the next day, which is inside the recording
        assert!(close(video("logtime:00:00:05"), 15.0));
        // outside the recording, the nearest date: a minute before and a minute after
        assert!(close(video("logtime:23:58:50"), -60.0));
        assert!(close(video("logtime:00:01:10"), 80.0));
    }

    #[test]
    fn times_before_the_clock_started_clamp_to_zero() {
        let resolved = resolve("mach:5-9s").expect("clamped");
        assert_eq!(resolved.mach_ns, 0);
    }
}
