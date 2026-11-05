//! [`Lookup`] over the files of a session, without opening it for analysis.
//!
//! A `TimeSpec` such as `event:12+1s` or `bug:a-3f9c1e02.end` names a moment by pointing at
//! something the session recorded, and resolving it asks a [`Lookup`] when that thing happened.
//! `Analysis` is one, but opening an `Analysis` refuses a session that is still being recorded
//! and parses the events and the log up front. [`SessionLookup`] reads only the file each question
//! needs, so it works on a session that is still recording: `annotate` uses it to mark a bug region
//! while the recording goes on.
//!
//! It answers differently from `Analysis` about the start of an event: see [`SessionLookup`].

use super::envelope;
use super::timespec::Edge;
use super::timespec::Lookup;
use crate::annotations;
use crate::incidents::INCIDENTS_FILE;
use crate::incidents::Incident;
use crate::session::Manifest;
use crate::session::Session;
use crate::timeline;
use crate::timeline::Locator;
use color_eyre::eyre;

/// A [`Lookup`] that reads the files of a session as they are asked about.
///
/// Every question reads its file again from the start (`events.jsonl`, `komorebi.log`,
/// `recorder.jsonl` or the annotations), which suits resolving the handful of times a command line
/// names, and not resolving many. Every failure to find what is asked for is reported with the
/// not-found exit kind and a message saying what is missing.
///
/// # Difference from `Analysis`
///
/// [`Edge`] is ignored for an event: `event:12` and `event:12.end` both resolve to the moment the
/// notification was received, which is when komorebi had finished handling it. `Analysis`
/// resolves `event:12` (the start edge) to when the handler began, where komorebi's log shows it.
///
///
/// # Example
///
/// ```rust
/// use komorebi_recorder::analysis::lookup::SessionLookup;
/// use komorebi_recorder::analysis::timespec::Edge;
/// use komorebi_recorder::analysis::timespec::Lookup;
/// use komorebi_recorder::analysis::timespec::TimeSpec;
/// use komorebi_recorder::testkit::SessionBuilder;
/// use serde_json::json;
///
/// let (session, manifest) = SessionBuilder::new("doc-session-lookup")
///     .event(2.0, json!({"type": "Retile"}), None)
///     .incident(4.0, "display-disconnected", "display 2")
///     .build();
/// let lookup = SessionLookup::new(&session, &manifest);
///
/// // one second after event 1 was received
/// let moment = "event:1+1s".parse::<TimeSpec>()?.resolve(&manifest, &lookup)?;
/// assert!((moment.video_s - 3.0).abs() < 1e-6);
///
/// // both edges of an event are when it was received
/// assert_eq!(
///     lookup.event_mach_ns(1, Edge::Start)?,
///     lookup.event_mach_ns(1, Edge::End)?
/// );
///
/// // incidents are numbered from 1
/// assert!(lookup.incident_mach_ns(1).is_ok());
/// assert!(lookup.incident_mach_ns(2).is_err());
/// assert!(lookup.event_mach_ns(99, Edge::End).is_err());
/// # Ok::<(), Box<dyn std::error::Error>>(())
/// ```
///
/// Used end to end, against real files, by
/// `tests/annotate.rs::a_bug_can_be_marked_relative_to_an_event_and_edited_and_deleted` and
/// `tests/annotate.rs::annotating_works_on_a_session_that_is_still_recording`.
pub struct SessionLookup<'a> {
    /// The session whose files are read.
    pub session: &'a Session,
    /// The session's manifest, which maps the master clock to video time.
    pub manifest: &'a Manifest,
}

impl<'a> SessionLookup<'a> {
    /// Returns a lookup over `session`, whose manifest is `manifest`.
    #[must_use]
    pub fn new(session: &'a Session, manifest: &'a Manifest) -> Self {
        Self { session, manifest }
    }

    fn located(&self, locator: &Locator) -> eyre::Result<u64> {
        timeline::locate(self.session, self.manifest, locator)
            .map(|located| located.mach_ns)
            .map_err(|error| envelope::not_found(format!("{error:#}")))
    }
}

impl Lookup for SessionLookup<'_> {
    fn event_mach_ns(&self, seq: u64, _edge: Edge) -> eyre::Result<u64> {
        self.located(&Locator::EventSeq(seq))
    }

    fn log_mach_ns(&self, line: u64) -> eyre::Result<u64> {
        self.located(&Locator::LogLine(line))
    }

    fn incident_mach_ns(&self, number: u64) -> eyre::Result<u64> {
        let text = std::fs::read_to_string(self.session.path(INCIDENTS_FILE))
            .map_err(|_| envelope::not_found("the session recorded no incidents"))?;
        let index = usize::try_from(number.saturating_sub(1)).unwrap_or(usize::MAX);
        text.lines()
            .nth(index)
            .filter(|_| number >= 1)
            .and_then(|line| serde_json::from_str::<Incident>(line).ok())
            .map(|incident| incident.mach_ns)
            .ok_or_else(|| envelope::not_found(format!("there is no incident {number}")))
    }

    fn annotation_span(&self, id: &str) -> eyre::Result<(u64, Option<u64>)> {
        let loaded = annotations::load(&self.session.dir)?;
        loaded
            .get(id)
            .map(|annotation| (annotation.start_mach_ns, annotation.end_mach_ns))
            .ok_or_else(|| envelope::not_found(format!("there is no annotation {id}")))
    }
}
