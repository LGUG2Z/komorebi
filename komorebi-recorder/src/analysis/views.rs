//! Answers shaped for a reader: what the commands print as JSON and the viewer draws.
//!
//! The analysis keeps what it reads in the forms that are convenient to compute with (an
//! [`EventRecord`](super::events::EventRecord), a handler block, a state snapshot). This module has
//! the shapes of the pieces that more than one answer shows, and the methods of [`Analysis`] that
//! build them: an event with its class and handler ([`EventView`], [`HandlerView`]), a moment on
//! every clock ([`Moment`]), and where a shown state came from ([`StateOrigin`]). The shapes
//! serialize to JSON and have JSON Schemas; the `schemas` module names the answers they are part
//! of.
//!
//! Times in these shapes are in the units a reader thinks in: seconds on the video timeline
//! (`*_video_s`, `video_s`), milliseconds for durations, and UTC wall time as text. The master
//! clock (`mach_ns`, nanoseconds on the host's master monotonic clock; `mach_` is a historical
//! spelling) is given as well, for passing back as a time argument.

use super::blocks::Quality;
use super::classify::Class;
use super::events::EventInfo;
use super::facade::Analysis;
use super::state::Snapshot;
use crate::timecode;
use schemars::JsonSchema;
use serde::Serialize;

/// The handler that produced an event, as far as the log shows.
///
/// A handler is the block of komorebi's log lines that carry one event's or command's span, from its
/// header on. All its fields come from those lines, so they are only as good as the log level
/// komorebi ran at: at the default `info` level there is little to see.
#[derive(Debug, Clone, PartialEq, Serialize, JsonSchema)]
pub struct HandlerView {
    /// How the event was tied to this handler (a `Quality`).
    pub quality: Quality,
    /// The number of the handler's header line in the session's `komorebi.log`, counting from 1.
    pub header_line: u64,
    /// The number of the last line the handler logged.
    pub last_line: u64,
    /// When the handler's header was logged, in video time.
    pub started_video_s: f64,
    /// At least how long the handler took, in milliseconds: header to the event being received.
    pub handler_ms: Option<f64>,
    /// When komorebi logged sending the notification (its `pushed notification to subscriber`
    /// line), in video time: komorebi's own send time, next to the `video_s` the recorder received
    /// it at. Absent when the log has no such line for the event.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pushed_video_s: Option<f64>,
    /// Milliseconds between komorebi sending the notification and the recorder receiving it. A
    /// large value is delivery delay (the notification was queued), not komorebi being slow.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub delivery_ms: Option<f64>,
    /// How many log lines the handler logged, header included.
    pub lines: usize,
    /// How many of those lines are at level ERROR.
    pub errors: u32,
    /// How many of those lines are at level WARN.
    pub warnings: u32,
}

/// One komorebi event as a reader sees it: what it was, when, what kind of event, whether it
/// changed the state, and the handler that ran.
///
/// This is an element of the `events` answer and the event a `trace` is about. The fields of the
/// event's [`EventInfo`] are flattened into it, so its JSON has `family`, `name`, `notification`,
/// `pid`, `window_id`, `argument` (and `exe`, if there is one) beside the fields below. Built by
/// [`Analysis::event_view`].
#[derive(Debug, Clone, PartialEq, Serialize, JsonSchema)]
pub struct EventView {
    /// The event's sequence number in the session, counting from 1: what `event:<seq>` names.
    pub seq: u64,
    /// When the recorder received the notification, which is when the handler finished, in video
    /// time (seconds since the video's `t = 0`; negative before it).
    pub video_s: f64,
    /// The same moment as `video_s`, as nanoseconds on the host's master monotonic clock.
    pub mach_ns: u64,
    /// The same moment on the wall clock, as RFC 3339 UTC with microseconds
    /// (`2026-10-26T19:43:35.062613Z`). `None` if the session's clock anchors cannot place it.
    pub wall_utc: Option<String>,
    /// The notification, as the flavor's rules read it.
    #[serde(flatten)]
    pub info: EventInfo,
    /// [`EventInfo::summary`]: one line saying what the event is.
    pub summary: String,
    /// The kind of event it was.
    pub class: Class,
    /// Why it was given that class.
    pub reason: String,
    /// Whether this event changed komorebi's state: `Some(true)` if its state differs from the
    /// previous event's. `None` when either event has no recorded state, and for the first event.
    pub state_changed: Option<bool>,
    /// The id of the state snapshot taken after this event (`states/<id>.json`), if one was
    /// recorded.
    pub state_id: Option<u64>,
    /// The handler the log shows for this event, if one could be tied to it.
    pub handler: Option<HandlerView>,
}

impl Analysis {
    /// Returns event number `index` of the session as a reader sees it, or `None` if there is no
    /// such event.
    ///
    /// `index` counts from 0 over [`Analysis::events`], in the order the events were received. It
    /// is a position, not a sequence number: event `seq` is at [`Analysis::event_index`]. The
    /// handler is filled in when the log could be tied to the event (see [`Analysis::pairing`]).
    ///
    /// Covered by `events_are_classified_and_paired_with_the_handler_that_ran` in
    /// `tests/investigate.rs`, which reads these views as the `events` command prints them.
    #[must_use]
    pub fn event_view(&self, index: usize) -> Option<EventView> {
        let event = self.events().get(index)?;
        let pairing = self.pairing();
        let classification = pairing.classes.get(index)?;
        let pair = pairing.pairs.get(index)?;
        let log = self.log();

        let handler = pair
            .block
            .and_then(|id| log.blocks.get(id))
            .map(|block| HandlerView {
                quality: pair.quality,
                header_line: block.header_line,
                last_line: block.last_line,
                started_video_s: self.video_s(block.header_mach_ns),
                handler_ms: pair.handler_ms,
                pushed_video_s: pair.pushed_mach_ns.map(|ns| self.video_s(ns)),
                delivery_ms: pair.delivery_ms,
                lines: block.lines.len(),
                errors: block.errors,
                warnings: block.warnings,
            });

        Some(EventView {
            seq: event.seq,
            video_s: self.video_s(event.recv_mach_ns),
            mach_ns: event.recv_mach_ns,
            wall_utc: self.wall_utc(event.recv_mach_ns),
            summary: event.info.summary(),
            info: event.info.clone(),
            class: classification.class,
            reason: classification.reason.clone(),
            state_changed: self.state_changed(index),
            state_id: event.state_id,
            handler,
        })
    }
}

/// A moment on every clock a reader might think in.
///
/// Every field names the same instant, and each can be given back to a command as a time:
/// `video_s` or `timecode` as they are, `mach:<mach_ns>`, or `wall:<wall_utc>`.
#[derive(Debug, Clone, PartialEq, Serialize, JsonSchema)]
pub struct Moment {
    /// Seconds since the video's `t = 0`; negative before it.
    pub video_s: f64,
    /// `video_s` as a timecode: `00:12.500`, or `1:02:03.250` from an hour on, with a leading `-`
    /// when negative (see [`crate::timecode::format`]).
    pub timecode: String,
    /// Nanoseconds on the host's master monotonic clock (`mach_` is a historical spelling).
    pub mach_ns: u64,
    /// The wall clock, as RFC 3339 UTC with microseconds. `None` if the session's clock anchors
    /// cannot place the moment.
    pub wall_utc: Option<String>,
}

impl Analysis {
    /// Returns the moment `mach_ns` (nanoseconds on the master clock) on every clock a reader might
    /// think in.
    #[must_use]
    pub fn moment(&self, mach_ns: u64) -> Moment {
        let video_s = self.video_s(mach_ns);
        Moment {
            video_s,
            timecode: timecode::format(video_s),
            mach_ns,
            wall_utc: self.wall_utc(mach_ns),
        }
    }
}

/// Where a state shown to a reader came from.
///
/// A state is known only from the notifications komorebi sent, and the state recorded when the
/// recording began, so a state "at" a moment is the latest one before it. This records which one,
/// and how stale it may be.
#[derive(Debug, Clone, PartialEq, Serialize, JsonSchema)]
pub struct StateOrigin {
    /// `event` (the notification of that event) or `start` (the state when recording began).
    pub source: String,
    /// The sequence number of the event whose notification carried the state; `None` for `start`.
    pub seq: Option<u64>,
    /// The id of the stored state (`states/<id>.json`); `None` for `start`.
    pub state_id: Option<u64>,
    /// How long before the asked-for moment the state was taken. komorebi can change state without
    /// sending a notification, so a large number means the state may be out of date.
    pub age_ms: Option<f64>,
}

/// Returns a one-line description of a snapshot for headings: how many monitors and windows it has,
/// such as `2 monitors, 17 windows`.
///
/// The words are singular for exactly one (`1 monitor, 1 window`), and plural otherwise, 0 included.
///
/// # Example
///
/// ```rust
/// use komorebi_recorder::analysis::flavor::Flavor;
/// use komorebi_recorder::analysis::state::Snapshot;
/// use komorebi_recorder::analysis::views::headline;
/// use serde_json::json;
///
/// let state = json!({"monitors": {"elements": [{
///     "id": 1,
///     "workspaces": {"elements": [{
///         "name": "one",
///         "containers": {"elements": [
///             {"id": "a", "windows": {"elements": [{"id": 1}], "focused": 0}},
///             {"id": "b", "windows": {"elements": [{"id": 2}], "focused": 0}}
///         ], "focused": 0}
///     }], "focused": 0}
/// }], "focused": 0}});
///
/// let snapshot = Snapshot::parse(Flavor::Macos, &state);
/// assert_eq!(headline(&snapshot), "1 monitor, 2 windows");
///
/// let empty = Snapshot::parse(Flavor::Macos, &json!({}));
/// assert_eq!(headline(&empty), "0 monitors, 0 windows");
/// ```
#[must_use]
pub fn headline(snapshot: &Snapshot) -> String {
    let windows = snapshot.windows().len();
    let monitors = snapshot.monitors.len();
    format!(
        "{monitors} monitor{}, {windows} window{}",
        if monitors == 1 { "" } else { "s" },
        if windows == 1 { "" } else { "s" }
    )
}
