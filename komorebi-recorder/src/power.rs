//! The machine sleeping, waking, its lid closing, its displays going dark, the session locking: what
//! the OS says about them, and what a recording does about each.
//!
//! On a Mac a sleep announces itself by the host clock pausing while the wall clock does not, so
//! the recording loop finds it by comparing the two (see [`crate::clock::JumpDetector`]). Whether
//! the performance counter does the same on Windows is something to be measured
//! (`doctor --probe clock`), and even if it does, hearing about a sleep when it happens beats
//! inferring it afterwards. So where the OS can say so, a watcher listens
//! ([`crate::platform::power`]), stamps each thing it hears with a clock anchor the moment it hears
//! it, and this module turns them into what a recording should do about them: incidents, anchors
//! that map the wall clock to the host clock on either side of a sleep, and whether captures must
//! be replaced or the displays and the session looked at again at once.
//!
//! # The requirement
//!
//! komorebi bugs show up mostly when a laptop is docked, undocked or woken. A recording that died
//! or lost its video in exactly those moments would be useless, so a recording has to ride out
//! monitors connecting and disconnecting, sleep, lock, unlock and wake without losing data. For
//! power, that comes to four things, which [`PowerTracker`] asks of the recording loop:
//!
//! * nothing is trusted across a sleep: every capture is finalized and replaced on waking, because
//!   a stream can stall across a sleep without saying so;
//! * the manifest is checkpointed before the machine sleeps, because it may not come back;
//! * the wall clock is anchored to the host clock on either side of a sleep, so komorebi's log
//!   keeps lining up with the video;
//! * the displays and the session are looked at at once, not at the next poll.
//!
//! # The pieces
//!
//! | Type | Is |
//! |---|---|
//! | [`PowerKind`] | what the OS said: a suspend, a resume, the lid, the displays' power, a lock |
//! | [`PowerEvent`] | one such thing, with the clock [`Anchor`] sampled when it was heard |
//! | [`PowerTracker`] | the state machine that turns each [`PowerEvent`] into a [`Reaction`] |
//! | [`Reaction`] | what the recording loop is to do: incidents to write, anchors to add, and what to do now |
//!
//! [`PowerTracker`] does no I/O and reads no clock: it is given everything it needs in the event,
//! so every transition can be tested with made-up times. The recording loop (in the `recorder`
//! module) applies each [`Reaction`]. On hosts with no power watcher (macOS, and the `unsupported`
//! host) no [`PowerEvent`] is ever produced, and a sleep is found from the clocks alone.
//!
//! A sleep that the OS announced is not recorded a second time when the clocks show it too: the
//! recording loop keeps the two anchors of that second finding, and nothing else.
//!
//! # What each event asks for
//!
//! | [`PowerKind`] | incidents | anchors | replace captures | poll displays | poll session | checkpoint |
//! |---|---|---|---|---|---|---|
//! | `Suspend` | `suspend` | the event's | | | | yes |
//! | `Resume` | `wake` | the event's | yes | yes | yes | |
//! | `LidClosed` | `lid-closed` | | | yes | | |
//! | `LidOpened` | `lid-opened` | | | yes | | |
//! | `DisplaysOff` | `display-asleep` | | | yes | | |
//! | `DisplaysOn` | `display-awake` | | | yes | | |
//! | `SessionLocked`, `SessionUnlocked` | none | | | | yes | |
//! | `DisplaysChanged` | none | | | yes | | |
//!
//! A lock writes no incident because [`crate::system_state::SessionTracker`] writes it when it
//! sees the state change; the event only makes the session be looked at now.
//!
//! # Example
//!
//! ```rust
//! use komorebi_recorder::clock::Anchor;
//! use komorebi_recorder::incidents::kind;
//! use komorebi_recorder::power::{PowerEvent, PowerKind, PowerTracker};
//!
//! let event = |kind: PowerKind, wall_s: i64, mach_s: u64| PowerEvent {
//!     kind,
//!     anchor: Anchor { wall_us: wall_s * 1_000_000, mach_ns: mach_s * 1_000_000_000 },
//! };
//!
//! let mut tracker = PowerTracker::default();
//!
//! // the machine is about to sleep: bracket it, say so, and save the manifest
//! let suspend = tracker.observe(event(PowerKind::Suspend, 1_000, 100));
//! assert_eq!(suspend.anchors.len(), 1);
//! assert!(suspend.checkpoint);
//! assert!(!suspend.replace_captures);
//! assert_eq!(suspend.incidents[0].kind, kind::SUSPEND);
//!
//! // it wakes 10 minutes later by the wall clock, and the host clock moved 1 second: it paused
//! let resume = tracker.observe(event(PowerKind::Resume, 1_600, 101));
//! assert!(resume.replace_captures && resume.poll_displays && resume.poll_session);
//! assert_eq!(resume.incidents[0].kind, kind::WAKE);
//! assert!(resume.incidents[0].detail.contains("600.0 s asleep"));
//! assert!(resume.incidents[0].detail.contains("skips the sleep"));
//!
//! // a resume with no suspend before it still replaces the captures
//! let again = tracker.observe(event(PowerKind::Resume, 5_000, 900));
//! assert!(again.replace_captures);
//! assert!(again.incidents[0].detail.contains("was not seen"));
//! ```

use crate::clock::Anchor;
use crate::incidents::Draft;
use crate::incidents::kind;

/// What the OS said about the machine's power or session.
///
/// A watcher produces these where the OS announces them (Windows, in `platform::power`); on a Mac
/// none is produced. They are what [`PowerTracker::observe`] is fed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PowerKind {
    /// The machine is about to sleep or hibernate.
    Suspend,
    /// The machine woke, by itself (a timer) or because the user did. A `Resume` with no
    /// [`Suspend`](PowerKind::Suspend) before it is possible, and is handled.
    Resume,
    /// The laptop lid was closed.
    LidClosed,
    /// The laptop lid was opened.
    LidOpened,
    /// The displays were turned off (not the machine put to sleep).
    DisplaysOff,
    /// The displays were turned on.
    DisplaysOn,
    /// The session was locked.
    SessionLocked,
    /// The session was unlocked.
    SessionUnlocked,
    /// Displays were added or removed, or changed size or scale.
    DisplaysChanged,
}

/// Something the OS said, and when, on both clocks.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PowerEvent {
    /// What the OS said.
    pub kind: PowerKind,
    /// Sampled the moment the message was received.
    pub anchor: Anchor,
}

/// What to do about what was heard: what [`PowerTracker::observe`] returns, for the recording loop
/// to apply.
///
/// The default reaction asks for nothing. Each field is a request, and the loop decides how to
/// carry it out; see the table in the [module docs](self) for what each [`PowerKind`] asks for.
#[derive(Debug, Default, PartialEq)]
pub struct Reaction {
    /// Incidents to write, each dated when the OS said it, not when it was handled.
    pub incidents: Vec<Draft>,
    /// Anchors to add to the clock map: either side of a sleep.
    pub anchors: Vec<Anchor>,
    /// The machine slept: replace every capture rather than trust it.
    pub replace_captures: bool,
    /// Look at the displays now rather than at the next poll.
    pub poll_displays: bool,
    /// Look at the session's lock state now.
    pub poll_session: bool,
    /// Write the manifest now: a machine about to sleep may not come back.
    pub checkpoint: bool,
}

/// Turns what the OS says into reactions, remembering the suspend that a resume answers.
///
/// A state machine with one piece of state: the anchor of the last [`PowerKind::Suspend`] that has
/// not been answered by a [`PowerKind::Resume`]. A suspend sets it, and a resume reads it, to say
/// how long the machine slept, and clears it. No other event reads or changes it. A new tracker
/// has seen no suspend.
///
/// The transitions, in the table of the [module docs](self), are covered by the tests in this
/// module; the example there walks through a sleep.
///
/// # Example: a second resume has no suspend to answer
///
/// ```rust
/// use komorebi_recorder::clock::Anchor;
/// use komorebi_recorder::power::{PowerEvent, PowerKind, PowerTracker};
///
/// let at = |kind: PowerKind, wall_s: i64, mach_s: u64| PowerEvent {
///     kind,
///     anchor: Anchor { wall_us: wall_s * 1_000_000, mach_ns: mach_s * 1_000_000_000 },
/// };
///
/// let mut tracker = PowerTracker::default();
/// tracker.observe(at(PowerKind::Suspend, 1_000, 100));
/// let first = tracker.observe(at(PowerKind::Resume, 1_100, 101));
/// assert!(first.incidents[0].detail.contains("100.0 s asleep"));
///
/// // the suspend was used up by the first resume
/// let second = tracker.observe(at(PowerKind::Resume, 5_000, 900));
/// assert!(second.incidents[0].detail.contains("was not seen"));
///
/// // the host clock counted the sleep (it moved as far as the wall clock): a gap, not a skip
/// tracker.observe(at(PowerKind::Suspend, 6_000, 1_000));
/// let counted = tracker.observe(at(PowerKind::Resume, 6_600, 1_600));
/// assert!(counted.incidents[0].detail.contains("includes it as a gap"));
/// ```
#[derive(Debug, Default)]
pub struct PowerTracker {
    suspended_at: Option<Anchor>,
}

impl PowerTracker {
    /// Returns what the recording should do about `event`, and updates what the tracker
    /// remembers.
    ///
    /// The reaction depends only on the event and on whether a suspend is waiting for its resume.
    /// For a [`PowerKind::Resume`], the `wake` incident says how long the machine slept by the
    /// wall clock and how far the host clock moved over the same time, and so whether video time
    /// skips the sleep (the host clock moved less than half as far as the wall clock) or includes
    /// it as a gap with no frames. With no suspend waiting, it says that the moment the machine
    /// went to sleep was not seen, and the captures are replaced all the same.
    pub fn observe(&mut self, event: PowerEvent) -> Reaction {
        let mut reaction = Reaction::default();
        let at = event.anchor.mach_ns;

        match event.kind {
            PowerKind::Suspend => {
                self.suspended_at = Some(event.anchor);
                reaction.anchors.push(event.anchor);
                reaction.checkpoint = true;
                reaction.incidents.push(
                    Draft::new(
                        kind::SUSPEND,
                        "the machine is going to sleep; nothing is recorded until it wakes",
                    )
                    .at(at),
                );
            }
            PowerKind::Resume => {
                reaction.anchors.push(event.anchor);
                reaction.replace_captures = true;
                reaction.poll_displays = true;
                reaction.poll_session = true;
                reaction
                    .incidents
                    .push(Draft::new(kind::WAKE, self.woke(event.anchor)).at(at));
                self.suspended_at = None;
            }
            PowerKind::LidClosed => {
                reaction.poll_displays = true;
                reaction
                    .incidents
                    .push(Draft::new(kind::LID_CLOSED, "the laptop lid was closed").at(at));
            }
            PowerKind::LidOpened => {
                reaction.poll_displays = true;
                reaction
                    .incidents
                    .push(Draft::new(kind::LID_OPENED, "the laptop lid was opened").at(at));
            }
            PowerKind::DisplaysOff => {
                reaction.poll_displays = true;
                reaction.incidents.push(
                    Draft::new(
                        kind::DISPLAY_ASLEEP,
                        "the displays were turned off; their video pauses until they are back",
                    )
                    .at(at),
                );
            }
            PowerKind::DisplaysOn => {
                reaction.poll_displays = true;
                reaction
                    .incidents
                    .push(Draft::new(kind::DISPLAY_AWAKE, "the displays were turned on").at(at));
            }
            PowerKind::SessionLocked | PowerKind::SessionUnlocked => {
                // `SessionTracker` writes the incident when it sees the state change; this makes
                // it look now rather than up to half a second from now
                reaction.poll_session = true;
            }
            PowerKind::DisplaysChanged => {
                reaction.poll_displays = true;
            }
        }

        reaction
    }

    /// What to say about waking: how long the machine slept, by the wall clock and by the host
    /// clock, which is what tells whether the host clock counts a sleep.
    fn woke(&self, resumed: Anchor) -> String {
        let Some(suspended) = self.suspended_at else {
            return "the machine woke from sleep (the moment it went to sleep was not seen)"
                .to_string();
        };

        let wall_s = (resumed.wall_us - suspended.wall_us) as f64 / 1e6;
        let host_s = resumed.mach_ns.saturating_sub(suspended.mach_ns) as f64 / 1e9;
        format!(
            "the machine woke after about {wall_s:.1} s asleep by the wall clock; the host clock \
             moved {host_s:.1} s over the same time, so video time {}",
            if host_s < wall_s * 0.5 {
                "skips the sleep"
            } else {
                "includes it as a gap with no frames"
            }
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn at(wall_s: i64, mach_s: u64, kind: PowerKind) -> PowerEvent {
        PowerEvent {
            kind,
            anchor: Anchor {
                wall_us: wall_s * 1_000_000,
                mach_ns: mach_s * 1_000_000_000,
            },
        }
    }

    #[test]
    fn a_sleep_is_bracketed_by_anchors_and_everything_is_replaced_on_waking() {
        let mut tracker = PowerTracker::default();

        let suspend = tracker.observe(at(1000, 100, PowerKind::Suspend));
        assert_eq!(suspend.anchors.len(), 1);
        assert!(
            suspend.checkpoint,
            "a machine about to sleep may not come back"
        );
        assert!(!suspend.replace_captures);
        assert_eq!(suspend.incidents[0].kind, kind::SUSPEND);
        assert_eq!(suspend.incidents[0].mach_ns, Some(100_000_000_000));

        let resume = tracker.observe(at(1600, 101, PowerKind::Resume));
        assert_eq!(resume.anchors.len(), 1);
        assert!(resume.replace_captures && resume.poll_displays && resume.poll_session);
        assert_eq!(resume.incidents[0].kind, kind::WAKE);
        let detail = &resume.incidents[0].detail;
        assert!(detail.contains("600.0 s asleep"), "{detail}");
        assert!(detail.contains("moved 1.0 s"), "{detail}");
        assert!(detail.contains("skips the sleep"), "{detail}");
    }

    #[test]
    fn a_host_clock_that_counted_the_sleep_is_said_to_have_a_gap_instead() {
        let mut tracker = PowerTracker::default();
        tracker.observe(at(1000, 100, PowerKind::Suspend));
        let resume = tracker.observe(at(1600, 700, PowerKind::Resume));
        let detail = &resume.incidents[0].detail;
        assert!(detail.contains("includes it as a gap"), "{detail}");
    }

    #[test]
    fn waking_with_no_suspend_seen_still_replaces_the_captures() {
        let mut tracker = PowerTracker::default();
        let resume = tracker.observe(at(1600, 700, PowerKind::Resume));
        assert!(resume.replace_captures);
        assert!(resume.incidents[0].detail.contains("was not seen"));
    }

    #[test]
    fn a_second_wake_does_not_reuse_the_first_suspend() {
        let mut tracker = PowerTracker::default();
        tracker.observe(at(1000, 100, PowerKind::Suspend));
        tracker.observe(at(1100, 101, PowerKind::Resume));
        let again = tracker.observe(at(5000, 900, PowerKind::Resume));
        assert!(again.incidents[0].detail.contains("was not seen"));
    }

    #[test]
    fn lid_and_display_power_are_incidents_that_make_the_displays_be_looked_at() {
        let mut tracker = PowerTracker::default();
        for (event, expected) in [
            (PowerKind::LidClosed, kind::LID_CLOSED),
            (PowerKind::LidOpened, kind::LID_OPENED),
            (PowerKind::DisplaysOff, kind::DISPLAY_ASLEEP),
            (PowerKind::DisplaysOn, kind::DISPLAY_AWAKE),
        ] {
            let reaction = tracker.observe(at(1, 1, event));
            assert!(reaction.poll_displays, "{event:?}");
            assert_eq!(reaction.incidents[0].kind, expected, "{event:?}");
            assert!(!reaction.replace_captures, "{event:?}");
        }
    }

    #[test]
    fn a_lock_makes_the_session_be_looked_at_without_writing_an_incident_twice() {
        let mut tracker = PowerTracker::default();
        for event in [PowerKind::SessionLocked, PowerKind::SessionUnlocked] {
            let reaction = tracker.observe(at(1, 1, event));
            assert!(reaction.poll_session);
            assert!(
                reaction.incidents.is_empty(),
                "{event:?}: SessionTracker reports it"
            );
        }
    }

    #[test]
    fn a_display_change_only_makes_the_displays_be_looked_at() {
        let reaction = PowerTracker::default().observe(at(1, 1, PowerKind::DisplaysChanged));
        assert!(reaction.poll_displays);
        assert!(reaction.incidents.is_empty());
        assert!(reaction.anchors.is_empty());
    }
}
