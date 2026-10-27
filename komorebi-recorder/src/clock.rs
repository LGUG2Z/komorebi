//! The master clock: the one monotonic clock every record in a session is stamped on, and the
//! conversions between it, the wall clock and video time.
//!
//! Every source in a recording is expressed in nanoseconds on one host monotonic clock: capture
//! frame timestamps, the OS trace, komorebi's subscriber events, incidents. komorebi's own log
//! lines are the exception. They carry a UTC wall clock time, and are mapped
//! onto the master clock with the [`Anchor`]s sampled while recording.
//!
//! # Which clock is the master clock
//!
//! [`mono_now_ns`] reads it, through [`crate::platform::clock`], and [`Host::clock`] names it: the
//! field holds the [`ClockKind`] the session was recorded with, which the manifest stores as
//! `host.clock`. It does not name the wall clock, or any [`ReferenceClock`].
//!
//! | [`ClockKind`] | `host.clock` | read with |
//! |---|---|---|
//! | [`Mach`](crate::platform::ClockKind::Mach) | `mach` | `mach_absolute_time`, on macOS |
//! | [`Qpc`](crate::platform::ClockKind::Qpc) | `qpc` | `QueryPerformanceCounter`, on Windows |
//! | [`Std`](crate::platform::ClockKind::Std) | `std` | [`std::time::Instant`] since the first reading in the process, plus one second so that it is never zero, on a host with no capture |
//!
//! `mach_absolute_time` pauses while the machine sleeps. Whether the Windows counter does is not
//! assumed: `doctor --probe clock` measures it, and where the OS can announce a sleep the
//! recorder listens for it (`platform::power`) rather than inferring it from the clocks alone.
//!
//! The `*_mach_ns` field names throughout a session are historical: they mean "master monotonic
//! nanoseconds", whichever clock that was, and keeping them means a session written by any build
//! is readable by any other. `host.clock` is what says which clock the numbers are on.
//!
//! # The four scales
//!
//! | Scale | Unit | Where it appears | Converted with |
//! |---|---|---|---|
//! | host ticks | ticks of the OS counter | raw counter reads, the unified log's `machTimestamp` | [`Timebase`] |
//! | master clock | nanoseconds, `u64` | every `*_mach_ns` field | (the scale the others convert to and from) |
//! | wall time | microseconds since the Unix epoch, `i64`, UTC | komorebi's log lines, every `wall_us` field | [`ClockMap`] |
//! | video time | seconds, `f64`, since the first captured frame | timecodes (see [`crate::timecode`]), `video_s` fields | subtracting the master clock time of video `t = 0` |
//!
//! Video `t = 0` is the master clock time of the earliest first frame across displays, recorded in
//! the manifest as `video_t0_mach_ns`; with no frame at all it is the recording's start. A time
//! before the first frame is negative. The session manifest's `video_seconds` and
//! `mach_at_video_seconds` do that subtraction in each direction.
//!
//! # How the pieces relate
//!
//! * [`Timebase`] says what one host tick is worth. It is read from the OS once ([`timebase`]),
//!   and stored in the manifest, so that a session recorded on one machine is read on another with
//!   the tick size of the machine that recorded it.
//! * [`Anchor`] pairs one wall clock reading with the master clock time at which it was made.
//!   The recording loop samples one about every 5 seconds, and one on either side of every sleep.
//! * [`ClockMap`] is built from a session's anchors, and maps between wall time and the master
//!   clock by interpolating between them.
//! * [`JumpDetector`] compares the wall clock with the master clock over each loop iteration, and
//!   reports a [`Jump`] when they disagree. The two anchors of a [`Jump`] are the pair of anchors
//!   [`ClockMap`] needs to treat a sleep as a step, rather than as drift.
//! * [`ReferenceClock`] is another host clock, read next to the master clock so that a probe can
//!   say what the master clock does across a sleep.
//!
//! # Example: ticks, master clock, wall time and video time
//!
//! ```rust
//! use komorebi_recorder::clock::{Anchor, ClockMap, Timebase};
//!
//! // a counter that ticks at 24 MHz, which is 125/3 nanoseconds a tick
//! let timebase = Timebase::from_frequency(24_000_000);
//! assert_eq!(timebase, Timebase { numer: 125, denom: 3 });
//!
//! // host ticks -> master clock nanoseconds
//! let mach_ns = timebase.ticks_to_ns(2_400_000_000);
//! assert_eq!(mach_ns, 100_000_000_000);
//!
//! // two anchors taken 5 seconds apart, as the recorder does
//! let map = ClockMap::new(vec![
//!     Anchor { wall_us: 1_800_000_000_000_000, mach_ns: 100_000_000_000 },
//!     Anchor { wall_us: 1_800_000_005_000_000, mach_ns: 105_000_000_000 },
//! ]);
//!
//! // master clock -> wall time, and back
//! let wall_us = map.mach_to_wall(102_500_000_000).expect("the map has anchors");
//! assert_eq!(wall_us, 1_800_000_002_500_000);
//! assert_eq!(map.wall_to_mach(wall_us), Some(102_500_000_000));
//!
//! // master clock -> video time, for a first frame that was captured at 101 s on the master clock
//! let video_t0_mach_ns = 101_000_000_000_i128;
//! let video_s = (102_500_000_000_i128 - video_t0_mach_ns) as f64 / 1e9;
//! assert_eq!(video_s, 1.5);
//!
//! // and back, from video time to the master clock
//! assert_eq!(video_t0_mach_ns + (video_s * 1e9).round() as i128, 102_500_000_000);
//!
//! // a time before the first frame is negative in video time
//! assert_eq!((100_500_000_000_i128 - video_t0_mach_ns) as f64 / 1e9, -0.5);
//! ```
//!
//! [`Host::clock`]: crate::platform::Host::clock
//! [`ClockKind`]: crate::platform::ClockKind

use crate::platform;
use serde::Deserialize;
use serde::Serialize;
use std::time::SystemTime;
use std::time::UNIX_EPOCH;

/// The size of one tick of the host clock: `numer / denom` nanoseconds.
///
/// It is read from the OS once ([`timebase`]) and stored in a session's manifest as `timebase`, so
/// that a session recorded on one machine is converted with the tick size of the machine that
/// recorded it, and not with the reader's. The unified log's `machTimestamp` is in ticks of this
/// size, and the manifest's timebase is what converts it.
///
/// A timebase this crate produces has a non-zero `numer` and `denom`. The fields are public and
/// are read back from `session.json`, so a hand-edited manifest can hold zeros, which make
/// [`ticks_to_ns`](Timebase::ticks_to_ns) and [`ns_to_ticks`](Timebase::ns_to_ticks) panic.
///
/// # Example
///
/// ```rust
/// use komorebi_recorder::clock::Timebase;
///
/// // Apple silicon's counter: 24 MHz, so 125/3 nanoseconds a tick
/// let timebase = Timebase { numer: 125, denom: 3 };
/// assert_eq!(timebase.ticks_to_ns(24_000_000), 1_000_000_000);
/// assert_eq!(timebase.ns_to_ticks(1_000_000_000), 24_000_000);
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Timebase {
    /// The numerator of the nanoseconds a tick is worth.
    pub numer: u32,
    /// The denominator of the nanoseconds a tick is worth.
    pub denom: u32,
}

impl Timebase {
    /// Returns the timebase of a counter that ticks `hertz` times a second, as
    /// `QueryPerformanceCounter` does.
    ///
    /// One tick is `1e9 / hertz` nanoseconds. The fraction is reduced, so a 10 MHz counter is
    /// `100/1`, not `1000000000/10000000`. A counter faster than about 4.29 GHz has a reduced
    /// denominator that does not fit in a `u32`: both parts are then halved until they do, which
    /// keeps the ratio approximately. A `hertz` of zero is treated as one.
    ///
    /// The result always has a non-zero `numer` and `denom`.
    ///
    /// # Example
    ///
    /// ```rust
    /// use komorebi_recorder::clock::Timebase;
    ///
    /// // the usual `QueryPerformanceFrequency`: 10 MHz, which is 100 ns a tick
    /// assert_eq!(Timebase::from_frequency(10_000_000), Timebase { numer: 100, denom: 1 });
    ///
    /// // Apple silicon's counter
    /// assert_eq!(Timebase::from_frequency(24_000_000), Timebase { numer: 125, denom: 3 });
    ///
    /// // a nanosecond counter, and a nonsense zero, are both fine
    /// assert_eq!(Timebase::from_frequency(1_000_000_000), Timebase { numer: 1, denom: 1 });
    /// assert_eq!(Timebase::from_frequency(0).ticks_to_ns(5), 5_000_000_000);
    /// ```
    #[must_use]
    pub fn from_frequency(hertz: u64) -> Self {
        fn gcd(a: u64, b: u64) -> u64 {
            if b == 0 { a } else { gcd(b, a % b) }
        }

        let hertz = hertz.max(1);
        let divisor = gcd(1_000_000_000, hertz);
        let (mut numer, mut denom) = (1_000_000_000 / divisor, hertz / divisor);
        while numer > u64::from(u32::MAX) || denom > u64::from(u32::MAX) {
            numer = (numer / 2).max(1);
            denom = (denom / 2).max(1);
        }

        Self {
            numer: numer as u32,
            denom: denom as u32,
        }
    }

    /// Returns `ticks` host clock ticks as nanoseconds: `ticks * numer / denom`.
    ///
    /// The product is computed in 128 bits, so it cannot overflow, and the quotient is truncated
    /// toward zero. A result that does not fit in a `u64` is truncated to its low 64 bits.
    ///
    /// # Panics
    ///
    /// Panics if `denom` is zero.
    ///
    /// # Example
    ///
    /// ```rust
    /// use komorebi_recorder::clock::Timebase;
    ///
    /// let timebase = Timebase { numer: 125, denom: 3 };
    /// assert_eq!(timebase.ticks_to_ns(24_000_000), 1_000_000_000);
    ///
    /// // one tick is 41.67 ns, truncated
    /// assert_eq!(timebase.ticks_to_ns(1), 41);
    /// ```
    #[must_use]
    pub fn ticks_to_ns(self, ticks: u64) -> u64 {
        (u128::from(ticks) * u128::from(self.numer) / u128::from(self.denom)) as u64
    }

    /// Returns `ns` nanoseconds as host clock ticks: `ns * denom / numer`.
    ///
    /// The product is computed in 128 bits, so it cannot overflow, and the quotient is truncated
    /// toward zero. A result that does not fit in a `u64` is truncated to its low 64 bits.
    ///
    /// # Panics
    ///
    /// Panics if `numer` is zero.
    ///
    /// # Example: a round trip can lose a tick
    ///
    /// Both directions truncate, so a tick count that goes to nanoseconds and back comes back
    /// equal only when the nanoseconds were exact:
    ///
    /// ```rust
    /// use komorebi_recorder::clock::Timebase;
    ///
    /// let timebase = Timebase { numer: 125, denom: 3 };
    /// assert_eq!(timebase.ns_to_ticks(timebase.ticks_to_ns(24_000_000)), 24_000_000);
    /// assert_eq!(timebase.ns_to_ticks(timebase.ticks_to_ns(1)), 0);
    /// ```
    #[must_use]
    pub fn ns_to_ticks(self, ns: u64) -> u64 {
        (u128::from(ns) * u128::from(self.denom) / u128::from(self.numer)) as u64
    }
}

/// Returns what one tick of the host clock is worth in nanoseconds.
///
/// It is the host's own: from `mach_timebase_info` on macOS (125/3 on Apple silicon, and 1/1 if the
/// OS reports an unusable one), from [`Timebase::from_frequency`] of `QueryPerformanceFrequency` on
/// Windows (10 MHz if that cannot be read), and 1/1 on a host with no capture, whose clock already
/// counts nanoseconds. A session records the timebase it was written with in its manifest; use that
/// one, not this, to convert a recorded session's ticks.
#[must_use]
pub fn timebase() -> Timebase {
    platform::clock::timebase()
}

/// Another clock the host has, read alongside the master clock to find out what the master clock
/// does while the machine sleeps.
///
/// A reference clock is never used to stamp a record. `doctor` reads each one together with the
/// master clock and the wall clock, and reports how far each moved across a sleep.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
pub struct ReferenceClock {
    /// The name of the OS clock, as the OS spells it (`mach_continuous_time`).
    pub name: String,
    /// Whether it keeps counting while the machine is suspended.
    pub counts_sleep: bool,
    /// Its reading, in nanoseconds from an origin that the clock chooses: only differences between
    /// two readings of the same clock mean anything.
    pub ns: u64,
}

/// Returns a reading of each of the host's other clocks (see [`ReferenceClock`]).
///
/// On macOS that is `mach_continuous_time`, which counts the time the machine sleeps. On Windows
/// it is `QueryInterruptTime`, which counts it, and `QueryUnbiasedInterruptTimePrecise`, which
/// does not. A host with no capture has none.
#[must_use]
pub fn reference_clocks() -> Vec<ReferenceClock> {
    platform::clock::reference_clocks()
}

/// Returns the master clock in nanoseconds: the host monotonic clock every record in a session is
/// on.
///
/// On macOS this is `mach_absolute_time`, which pauses while the machine sleeps; on Windows it is
/// `QueryPerformanceCounter`. The `*_mach_ns` fields in a session keep their historical names and
/// hold this value; the manifest's `host.clock` says which clock it was.
///
/// The origin is chosen by the OS, so a reading means something only next to another reading of the
/// same clock. The clock is monotonic: it does not go backwards, and `doctor` reports whether it
/// did while it was read.
///
/// # Example
///
/// ```rust
/// use komorebi_recorder::clock::mono_now_ns;
///
/// let first = mono_now_ns();
/// let second = mono_now_ns();
/// assert!(second >= first);
/// ```
#[must_use]
pub fn mono_now_ns() -> u64 {
    platform::clock::now_ns()
}

/// Returns the wall clock in microseconds since the Unix epoch (UTC).
///
/// Unlike [`mono_now_ns`], the wall clock can be stepped, forwards or backwards, by the user or by
/// time synchronization, and keeps counting while the machine sleeps. A time before 1970, which
/// means the clock is badly wrong, is returned as a negative number rather than as an error.
///
/// # Example
///
/// ```rust
/// use komorebi_recorder::clock::wall_now_us;
///
/// // after 2020-01-01 and before 2100-01-01
/// let now = wall_now_us();
/// assert!(now > 1_577_836_800_000_000);
/// assert!(now < 4_102_444_800_000_000);
/// ```
#[must_use]
pub fn wall_now_us() -> i64 {
    match SystemTime::now().duration_since(UNIX_EPOCH) {
        Ok(since) => since.as_micros() as i64,
        // before 1970: the clock is badly wrong, but a negative time is still a time
        Err(before) => -(before.duration().as_micros() as i64),
    }
}

/// A paired reading of the wall clock and the master clock: the master clock time at which the
/// wall clock read `wall_us`.
///
/// An anchor is the only link between komorebi's log, which is stamped with the wall clock, and
/// everything else, which is on the master clock. A session keeps a series of them in its manifest
/// (`anchors`), and [`ClockMap`] interpolates between them.
///
/// # Example
///
/// ```rust
/// use komorebi_recorder::clock::{wall_now_us, Anchor};
///
/// let anchor = Anchor::sample();
/// // the wall clock was read a moment ago
/// assert!((wall_now_us() - anchor.wall_us).abs() < 1_000_000);
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Anchor {
    /// The wall clock, in microseconds since the Unix epoch (UTC).
    pub wall_us: i64,
    /// The master clock, in nanoseconds, at the same moment.
    pub mach_ns: u64,
}

impl Anchor {
    /// Takes a tight paired reading: several back-to-back samples, keeping the one whose wall read
    /// was bracketed by the narrowest pair of mach reads, which bounds the pairing error to the
    /// duration of a couple of syscalls.
    ///
    /// Seven samples are taken. The `mach_ns` of the one kept is the midpoint of the two master
    /// clock reads around its wall clock read.
    #[must_use]
    pub fn sample() -> Self {
        let mut best: Option<(u64, Anchor)> = None;
        for _ in 0..7 {
            let before = mono_now_ns();
            let wall_us = wall_now_us();
            let after = mono_now_ns();
            let width = after.saturating_sub(before);
            let anchor = Anchor {
                wall_us,
                mach_ns: before + width / 2,
            };

            if best.is_none_or(|(best_width, _)| width < best_width) {
                best = Some((width, anchor));
            }
        }

        best.map(|(_, anchor)| anchor).unwrap_or_else(|| Anchor {
            wall_us: wall_now_us(),
            mach_ns: mono_now_ns(),
        })
    }

    /// The wall clock minus the mach clock, in microseconds.
    fn offset_us(self) -> i128 {
        i128::from(self.wall_us) - i128::from(self.mach_ns / 1_000)
    }
}

/// How far the wall clock must outrun the mach clock between two close observations for it to
/// count as a jump. Observations are made every ~50 ms, so ordinary drift is microseconds.
const JUMP_THRESHOLD_NS: i128 = 1_000_000_000;

/// A discontinuity between the wall clock and the master clock, as [`JumpDetector::observe`]
/// reports it.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Jump {
    /// How far the wall clock moved beyond the master clock, in seconds. Positive: the machine
    /// slept (mach time pauses while asleep, wall time does not) or the clock stepped forward.
    /// Negative: the wall clock was stepped back.
    pub seconds: f64,
    /// The last good pairing before the jump: the previous observation.
    pub before: Anchor,
    /// The first pairing after the jump: the observation that found it.
    pub after: Anchor,
}

/// Notices sleep/wake and clock steps by watching both clocks.
///
/// Feed it a fresh [`Anchor`] every loop iteration. When the two clocks disagree about how much
/// time passed since the previous observation, the machine was suspended (or the clock was
/// stepped), and the two anchors on either side of the discontinuity are exactly what
/// [`ClockMap`] needs to map wall time to mach time correctly on both sides of it.
///
/// The detector is a state machine with one piece of state, the previous observation. It reports a
/// [`Jump`] when the wall clock and the master clock moved by amounts that differ by 1 second or
/// more since then. Observations are expected about every 50 ms, so ordinary drift is
/// microseconds, and a process that was merely descheduled for a long time is not a jump, because
/// both clocks moved together. A suspension shorter than 1 second is not reported.
///
/// On a host whose master clock keeps counting through a sleep, the two clocks do not disagree and
/// a sleep is not noticed this way: see the module docs, and `PowerTracker` for the OS's own
/// announcement of a sleep.
///
/// # Example: noticing a sleep, and mapping time across it
///
/// ```rust
/// use komorebi_recorder::clock::{Anchor, ClockMap, JumpDetector};
///
/// // wall time 2000 s and master clock time 1000 s are the same moment
/// let start = Anchor { wall_us: 2_000_000_000, mach_ns: 1_000_000_000_000 };
/// let mut detector = JumpDetector::new(start);
///
/// // 50 ms later on both clocks: nothing to report
/// let before = Anchor { wall_us: 2_000_050_000, mach_ns: 1_000_050_000_000 };
/// assert_eq!(detector.observe(before), None);
///
/// // the lid is closed for an hour: the wall clock moves on, the master clock barely does
/// let after = Anchor {
///     wall_us: 2_000_100_000 + 3_600_000_000,
///     mach_ns: 1_000_100_000_000,
/// };
/// let jump = detector.observe(after).expect("the clocks disagree by an hour");
/// assert!((jump.seconds - 3_600.0).abs() < 0.01);
/// assert_eq!((jump.before, jump.after), (before, after));
///
/// // with both anchors in the map, wall time maps correctly on either side of the sleep
/// let map = ClockMap::new(vec![jump.before, jump.after]);
/// assert_eq!(map.wall_to_mach(2_000_050_000), Some(1_000_050_000_000));
/// let two_seconds_after_waking = after.wall_us + 2_000_000;
/// assert_eq!(map.wall_to_mach(two_seconds_after_waking), Some(1_002_100_000_000));
///
/// // the detector carries on from the new position
/// let later = Anchor { wall_us: after.wall_us + 50_000, mach_ns: after.mach_ns + 50_000_000 };
/// assert_eq!(detector.observe(later), None);
/// ```
#[derive(Debug, Clone, Copy)]
pub struct JumpDetector {
    last: Anchor,
}

impl JumpDetector {
    /// Returns a detector whose previous observation is `first`.
    #[must_use]
    pub fn new(first: Anchor) -> Self {
        Self { last: first }
    }

    /// Compares `now` with the previous observation, and makes `now` the previous observation.
    ///
    /// Returns a [`Jump`] if the wall clock and the master clock moved by amounts that differ by
    /// at least 1 second, in either direction, and `None` otherwise. The observation is recorded
    /// either way, so one discontinuity is reported once.
    pub fn observe(&mut self, now: Anchor) -> Option<Jump> {
        let wall_ns = (i128::from(now.wall_us) - i128::from(self.last.wall_us)) * 1_000;
        let mach_ns = i128::from(now.mach_ns) - i128::from(self.last.mach_ns);
        let difference = wall_ns - mach_ns;
        let before = std::mem::replace(&mut self.last, now);

        (difference.abs() >= JUMP_THRESHOLD_NS).then_some(Jump {
            seconds: difference as f64 / 1e9,
            before,
            after: now,
        })
    }
}

/// Segments whose wall and mach durations disagree by more than this are treated as a clock step
/// (an NTP jump, or the machine sleeping while the mach clock was paused) rather than as drift.
const STEP_THRESHOLD_NS: i128 = 20_000_000;

/// A two-way map between wall clock time and the master clock, built by piecewise-linear
/// interpolation between the anchors sampled during a recording.
///
/// A map is built from a session's anchors (the manifest's `anchors`) and answers in both
/// directions: [`wall_to_mach`] turns a wall clock time, such as one of komorebi's log lines, into
/// the master clock, and [`mach_to_wall`] does the reverse, to show a master clock time as UTC.
///
/// Each pair of neighboring anchors, ordered by master clock time, is a segment. The wall clock
/// and the master clock tick at slightly different rates, so inside a segment a time is
/// interpolated linearly. If the two clocks moved by amounts that differ by more than 20 ms over a
/// segment, the segment holds a step rather than drift: the machine slept while the master clock
/// was paused, or the wall clock was stepped. Where in the segment the step happened is not
/// known, so a time inside it is taken from the nearer of the segment's two anchors, using that
/// anchor's offset between the clocks. Outside the first and last anchors, a time is taken
/// from the nearer end in the same way. With a single anchor, the map is a constant offset.
///
/// Both directions return `None` if, and only if, the map has no anchors.
///
/// A sleep inside a segment is placed at the nearer anchor, which is why the recorder adds an
/// anchor on either side of every sleep it finds (see [`JumpDetector`]).
///
/// The anchors of a map are guaranteed to be sorted by master clock time, with no two at the same
/// time, because [`ClockMap::new`] is the only way to make one that has any: the field is private.
///
/// ```rust,compile_fail,E0451
/// use komorebi_recorder::clock::{Anchor, ClockMap};
///
/// // an unsorted list of anchors, which `ClockMap::new` would put in order
/// let anchors = vec![
///     Anchor { wall_us: 2_005_000_000, mach_ns: 1_005_000_000_000 },
///     Anchor { wall_us: 2_000_000_000, mach_ns: 1_000_000_000_000 },
/// ];
/// let _map = ClockMap { anchors };
/// ```
///
/// # Example: a clock step
///
/// ```rust
/// use komorebi_recorder::clock::{Anchor, ClockMap};
///
/// let at = |wall_s: i64, mach_s: u64| Anchor {
///     wall_us: wall_s * 1_000_000,
///     mach_ns: mach_s * 1_000_000_000,
/// };
///
/// // the wall clock jumps forward 30 s between the second and third anchors, while the master
/// // clock moves 5 s: the machine slept, or the clock was stepped
/// let map = ClockMap::new(vec![at(2000, 1000), at(2005, 1005), at(2040, 1010)]);
///
/// // before the step, the clocks are 1000 s apart
/// assert_eq!(map.wall_to_mach(2_001_000_000), Some(1_001_000_000_000));
/// // after it, 1030 s apart
/// assert_eq!(map.wall_to_mach(2_039_000_000), Some(1_009_000_000_000));
/// // and the other direction agrees on both sides
/// assert_eq!(map.mach_to_wall(1_001_000_000_000), Some(2_001_000_000));
/// assert_eq!(map.mach_to_wall(1_009_000_000_000), Some(2_039_000_000));
///
/// // an empty map has no answer
/// assert_eq!(ClockMap::default().wall_to_mach(1), None);
/// ```
///
/// [`wall_to_mach`]: ClockMap::wall_to_mach
/// [`mach_to_wall`]: ClockMap::mach_to_wall
#[derive(Debug, Clone, Default)]
pub struct ClockMap {
    anchors: Vec<Anchor>,
}

impl ClockMap {
    /// Returns a map over `anchors`, which may be given in any order.
    ///
    /// The anchors are sorted by master clock time, and of several with the same master clock time
    /// only the first given is kept.
    #[must_use]
    pub fn new(mut anchors: Vec<Anchor>) -> Self {
        anchors.sort_by_key(|anchor| anchor.mach_ns);
        anchors.dedup_by_key(|anchor| anchor.mach_ns);
        Self { anchors }
    }

    /// Returns the anchors the map was built from, ascending by master clock time, with no two
    /// sharing a master clock time.
    #[must_use]
    pub fn anchors(&self) -> &[Anchor] {
        &self.anchors
    }

    /// Returns true if and only if the map has no anchors, in which case every lookup returns
    /// `None`.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.anchors.is_empty()
    }

    /// Whether the segment between two anchors contains a clock step rather than plain drift.
    fn is_step(a: Anchor, b: Anchor) -> bool {
        let wall_ns = (i128::from(b.wall_us) - i128::from(a.wall_us)) * 1_000;
        let mach_ns = i128::from(b.mach_ns) - i128::from(a.mach_ns);
        (wall_ns - mach_ns).abs() > STEP_THRESHOLD_NS
    }

    /// Returns the master clock time, in nanoseconds, of the wall clock time `wall_us`
    /// (microseconds since the Unix epoch), or `None` if the map has no anchors.
    ///
    /// The result is never negative: a time that maps before the origin of the master clock is
    /// returned as zero. If the wall clock was stepped back, two segments can contain the same wall
    /// clock time; the segment earlier in master clock time is used.
    ///
    /// # Example
    ///
    /// ```rust
    /// use komorebi_recorder::clock::{Anchor, ClockMap};
    ///
    /// // wall = master + 1000 s, with the wall clock running slightly fast between the anchors
    /// let map = ClockMap::new(vec![
    ///     Anchor { wall_us: 2_000_000_000, mach_ns: 1_000_000_000_000 },
    ///     Anchor { wall_us: 2_005_000_500, mach_ns: 1_005_000_000_000 },
    /// ]);
    ///
    /// // halfway between them, interpolated
    /// assert_eq!(map.wall_to_mach(2_002_500_250), Some(1_002_500_000_000));
    ///
    /// // outside them, extended from the nearer end at a constant offset
    /// assert_eq!(map.wall_to_mach(1_999_000_000), Some(999_000_000_000));
    /// assert_eq!(map.wall_to_mach(2_010_000_500), Some(1_010_000_000_000));
    /// ```
    #[must_use]
    pub fn wall_to_mach(&self, wall_us: i64) -> Option<u64> {
        let (first, last) = (*self.anchors.first()?, *self.anchors.last()?);
        let extrapolate = |anchor: Anchor| {
            let mach = i128::from(wall_us) - anchor.offset_us();
            (mach * 1_000).max(0) as u64
        };

        if self.anchors.len() == 1 {
            return Some(extrapolate(first));
        }

        for pair in self.anchors.windows(2) {
            let (a, b) = (pair[0], pair[1]);
            let (lo, hi) = (a.wall_us.min(b.wall_us), a.wall_us.max(b.wall_us));
            if wall_us < lo || wall_us > hi {
                continue;
            }

            if Self::is_step(a, b) {
                // we cannot know when inside the segment the step happened, so trust whichever
                // end is closer in wall time
                let nearest = if (wall_us - a.wall_us).abs() <= (wall_us - b.wall_us).abs() {
                    a
                } else {
                    b
                };
                return Some(extrapolate(nearest));
            }

            let wall_span = i128::from(b.wall_us) - i128::from(a.wall_us);
            if wall_span == 0 {
                return Some(a.mach_ns);
            }

            let mach_span = i128::from(b.mach_ns) - i128::from(a.mach_ns);
            let into = i128::from(wall_us) - i128::from(a.wall_us);
            return Some((i128::from(a.mach_ns) + into * mach_span / wall_span).max(0) as u64);
        }

        // outside every segment: extend from whichever end is nearest in wall time
        let nearest = if (wall_us - first.wall_us).abs() <= (wall_us - last.wall_us).abs() {
            first
        } else {
            last
        };
        Some(extrapolate(nearest))
    }

    /// Returns the wall clock time, in microseconds since the Unix epoch, of the master clock time
    /// `mach_ns`, or `None` if the map has no anchors.
    ///
    /// The master clock has a resolution finer than a microsecond, and the result is truncated to
    /// whole microseconds. Within a segment that holds no step, a wall clock time taken through
    /// [`wall_to_mach`](ClockMap::wall_to_mach) and back comes back within 1 microsecond (the test
    /// `roundtrips_within_a_microsecond`); that is not guaranteed across a step.
    ///
    /// # Example
    ///
    /// ```rust
    /// use komorebi_recorder::clock::{Anchor, ClockMap};
    ///
    /// let map = ClockMap::new(vec![
    ///     Anchor { wall_us: 2_000_000_000, mach_ns: 1_000_000_000_000 },
    ///     Anchor { wall_us: 2_005_000_000, mach_ns: 1_005_000_000_000 },
    /// ]);
    ///
    /// let wall_us = map.mach_to_wall(1_002_345_678_000).expect("the map has anchors");
    /// assert_eq!(wall_us, 2_002_345_678);
    ///
    /// let back = map.wall_to_mach(wall_us).expect("the map has anchors");
    /// assert_eq!(back, 1_002_345_678_000);
    /// ```
    #[must_use]
    pub fn mach_to_wall(&self, mach_ns: u64) -> Option<i64> {
        let (first, last) = (*self.anchors.first()?, *self.anchors.last()?);
        let extrapolate = |anchor: Anchor| {
            let wall = i128::from(mach_ns / 1_000) + anchor.offset_us();
            wall as i64
        };

        if mach_ns <= first.mach_ns {
            return Some(extrapolate(first));
        }

        if mach_ns >= last.mach_ns {
            return Some(extrapolate(last));
        }

        for pair in self.anchors.windows(2) {
            let (a, b) = (pair[0], pair[1]);
            if mach_ns < a.mach_ns || mach_ns > b.mach_ns {
                continue;
            }

            if Self::is_step(a, b) {
                let nearest = if mach_ns - a.mach_ns <= b.mach_ns - mach_ns {
                    a
                } else {
                    b
                };
                return Some(extrapolate(nearest));
            }

            let mach_span = i128::from(b.mach_ns) - i128::from(a.mach_ns);
            let wall_span = i128::from(b.wall_us) - i128::from(a.wall_us);
            let into = i128::from(mach_ns) - i128::from(a.mach_ns);
            return Some((i128::from(a.wall_us) + into * wall_span / mach_span) as i64);
        }

        Some(extrapolate(last))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn anchor(wall_s: i64, mach_s: u64) -> Anchor {
        Anchor {
            wall_us: wall_s * 1_000_000,
            mach_ns: mach_s * 1_000_000_000,
        }
    }

    #[test]
    fn a_counters_frequency_is_a_timebase() {
        // the usual QueryPerformanceFrequency: 10 MHz, 100 ns a tick
        let ten_mhz = Timebase::from_frequency(10_000_000);
        assert_eq!(
            ten_mhz,
            Timebase {
                numer: 100,
                denom: 1
            }
        );
        assert_eq!(ten_mhz.ticks_to_ns(10_000_000), 1_000_000_000);
        assert_eq!(ten_mhz.ns_to_ticks(1_000_000_000), 10_000_000);

        // one that does not divide evenly
        let odd = Timebase::from_frequency(3_579_545);
        let one_second = odd.ns_to_ticks(1_000_000_000);
        assert!(
            (3_579_544..=3_579_546).contains(&one_second),
            "{one_second}"
        );

        // a nanosecond counter, and a nonsense zero, are both fine
        assert_eq!(
            Timebase::from_frequency(1_000_000_000),
            Timebase { numer: 1, denom: 1 }
        );
        assert_eq!(Timebase::from_frequency(0).ticks_to_ns(5), 5_000_000_000);

        // faster than u32 can hold: still close
        let fast = Timebase::from_frequency(10_000_000_000);
        assert!(fast.ticks_to_ns(10_000_000_000).abs_diff(1_000_000_000) < 10_000_000);
    }

    #[test]
    fn timebase_roundtrip() {
        let tb = Timebase {
            numer: 125,
            denom: 3,
        };
        let ns = tb.ticks_to_ns(24_000_000);
        assert_eq!(ns, 1_000_000_000);
        assert_eq!(tb.ns_to_ticks(ns), 24_000_000);
    }

    #[test]
    fn mach_is_monotonic_and_wall_is_sane() {
        let a = mono_now_ns();
        let b = mono_now_ns();
        assert!(b >= a);
        // after 2020 and before 2100
        assert!(wall_now_us() > 1_577_836_800_000_000);
        assert!(wall_now_us() < 4_102_444_800_000_000);
    }

    #[test]
    fn anchor_sample_is_close_to_now() {
        let anchor = Anchor::sample();
        let now = wall_now_us();
        assert!((now - anchor.wall_us).abs() < 50_000);
    }

    #[test]
    fn interpolates_between_anchors() {
        // wall = mach + 1000s, with the wall clock running 100ppm fast over the segment
        let a = anchor(2000, 1000);
        let b = Anchor {
            wall_us: 2005 * 1_000_000 + 500,
            mach_ns: 1005 * 1_000_000_000,
        };
        let map = ClockMap::new(vec![a, b]);

        let mach = map.wall_to_mach(2_002_500_000 + 250).expect("mach");
        assert_eq!(mach, 1_002_500_000_000);

        let wall = map.mach_to_wall(1_002_500_000_000).expect("wall");
        assert_eq!(wall, 2_002_500_000 + 250);
    }

    #[test]
    fn roundtrips_within_a_microsecond() {
        let map = ClockMap::new(vec![
            anchor(2000, 1000),
            anchor(2005, 1005),
            anchor(2010, 1010),
        ]);
        for wall in [2_000_123_456, 2_004_999_999, 2_007_777_777, 2_009_000_001] {
            let mach = map.wall_to_mach(wall).expect("mach");
            let back = map.mach_to_wall(mach).expect("wall");
            assert!((back - wall).abs() <= 1, "{wall} -> {mach} -> {back}");
        }
    }

    #[test]
    fn extrapolates_outside_the_anchor_range() {
        let map = ClockMap::new(vec![anchor(2000, 1000), anchor(2005, 1005)]);
        assert_eq!(
            map.wall_to_mach(1_999_000_000),
            Some(999_000_000_000),
            "before the first anchor"
        );
        assert_eq!(
            map.wall_to_mach(2_010_000_000),
            Some(1_010_000_000_000),
            "after the last anchor"
        );
    }

    #[test]
    fn handles_a_clock_step() {
        // the wall clock jumps forward 30s between the second and third anchors (an NTP step, or
        // the machine slept while mach time was paused)
        let map = ClockMap::new(vec![
            anchor(2000, 1000),
            anchor(2005, 1005),
            anchor(2040, 1010),
        ]);

        // before the step the offset is 1000s
        assert_eq!(map.wall_to_mach(2_001_000_000), Some(1_001_000_000_000));
        // after the step the offset is 1030s
        assert_eq!(map.wall_to_mach(2_039_000_000), Some(1_009_000_000_000));
        // and mach -> wall agrees on both sides
        assert_eq!(map.mach_to_wall(1_001_000_000_000), Some(2_001_000_000));
        assert_eq!(map.mach_to_wall(1_009_000_000_000), Some(2_039_000_000));
    }

    #[test]
    fn single_anchor_is_a_constant_offset() {
        let map = ClockMap::new(vec![anchor(2000, 1000)]);
        assert_eq!(map.wall_to_mach(2_003_000_000), Some(1_003_000_000_000));
        assert_eq!(map.mach_to_wall(1_003_000_000_000), Some(2_003_000_000));
    }

    #[test]
    fn the_jump_detector_ignores_ordinary_time_passing() {
        let mut detector = JumpDetector::new(anchor(2000, 1000));
        // 50 ms later on both clocks, with a little drift
        let tick = |wall_us: i64, mach_ns: u64| Anchor { wall_us, mach_ns };
        assert_eq!(
            detector.observe(tick(2_000_050_010, 1_000_050_000_000)),
            None
        );
        // a long but consistent interval (the process was merely descheduled)
        assert_eq!(
            detector.observe(tick(2_030_050_010, 1_030_050_000_000)),
            None
        );
    }

    #[test]
    fn the_jump_detector_notices_sleep_and_brackets_it() {
        let mut detector = JumpDetector::new(anchor(2000, 1000));
        let before = Anchor {
            wall_us: 2_000_050_000,
            mach_ns: 1_000_050_000_000,
        };
        assert_eq!(detector.observe(before), None);

        // the lid is closed for 3600 s: wall time moves on, mach time barely does
        let after = Anchor {
            wall_us: 2_000_100_000 + 3_600_000_000,
            mach_ns: 1_000_100_000_000,
        };
        let jump = detector.observe(after).expect("a jump");
        assert!((jump.seconds - 3_600.0).abs() < 0.01, "{}", jump.seconds);
        assert_eq!(
            jump.before, before,
            "the last good pairing before the sleep"
        );
        assert_eq!(jump.after, after);

        // and with those two anchors the map is right on both sides of the sleep
        let map = ClockMap::new(vec![jump.before, jump.after]);
        assert_eq!(map.wall_to_mach(2_000_050_000), Some(1_000_050_000_000));
        assert_eq!(
            map.wall_to_mach(2_000_100_000 + 3_600_000_000),
            Some(1_000_100_000_000)
        );
        assert_eq!(
            map.wall_to_mach(2_000_100_000 + 3_600_000_000 + 2_000_000),
            Some(1_000_100_000_000 + 2_000_000_000),
            "two seconds after waking"
        );

        // the detector carries on from the new position
        assert_eq!(
            detector.observe(Anchor {
                wall_us: after.wall_us + 50_000,
                mach_ns: after.mach_ns + 50_000_000
            }),
            None
        );
    }

    #[test]
    fn the_jump_detector_notices_the_clock_stepping_back() {
        let mut detector = JumpDetector::new(anchor(2000, 1000));
        let jump = detector
            .observe(Anchor {
                wall_us: 2_000_050_000 - 30_000_000,
                mach_ns: 1_000_050_000_000,
            })
            .expect("a jump");
        assert!(
            jump.seconds < -29.0 && jump.seconds > -31.0,
            "{}",
            jump.seconds
        );
    }

    #[test]
    fn empty_map_has_no_answer() {
        let map = ClockMap::default();
        assert_eq!(map.wall_to_mach(1), None);
        assert_eq!(map.mach_to_wall(1), None);
    }
}
