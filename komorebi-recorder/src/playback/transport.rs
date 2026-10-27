//! The playhead: where in the recording it is, and whether and how fast it is moving.
//!
//! A [`Transport`] is plain arithmetic. It has no window, no decoder and no clock of its own: every
//! method that depends on time takes `now`, a reading of any monotonic clock in seconds (the
//! [`Playback`](super::Playback) engine passes the seconds since it started). That makes it exact
//! and testable to the millisecond without waiting for anything.
//!
//! Position is computed from a starting point, a rate and the time that has passed, never
//! accumulated frame by frame, so it does not drift whatever the frame rate of the video or of the
//! window drawing it. Times are session video seconds: seconds from the start of the recording's
//! video, so a recording [`Manifest::video_span`] reports as `(0.0, d)` is a transport of
//! duration `d`.
//!
//! # The model
//!
//! A transport remembers where the playhead was at one moment (the anchor), whether it is playing,
//! the rate and an optional loop region. Asked for the position at a later `now`, it adds
//! `rate * (now - anchor time)` to the anchor and then applies the ends of the recording or the
//! loop region. Calls that change how it moves first settle the position at `now`, so a change of
//! rate or direction never jumps the playhead.
//!
//! | rule | detail |
//! |---|---|
//! | range | the position is always within `0.0..=duration_s`; [`Transport::seek`] clamps |
//! | ends | playing into an end stops there; [`Transport::is_playing`] then returns `false` |
//! | speed | the magnitude of the rate is within `MIN_RATE..=MAX_RATE`; the sign is the direction |
//! | restart | playing forwards from the end (or backwards from the start) starts over at the other end |
//! | loop | a region repeats for as long as the transport is playing; it has no end |
//!
//! `now` must not decrease between calls. That is not checked: an earlier `now` moves the
//! playhead the other way.
//!
//! The limits are [`MIN_RATE`] and [`MAX_RATE`].
//!
//! [`Manifest::video_span`]: crate::session::Manifest::video_span

/// The fastest the playhead may move, as a multiple of real time, in either direction.
///
/// [`Transport::play`] and [`Transport::set_rate`] clamp the magnitude of a rate to
/// [`MIN_RATE`]`..=MAX_RATE`.
pub const MAX_RATE: f64 = 8.0;
/// The slowest the playhead may move, as a multiple of real time, in either direction.
///
/// A rate of zero is not a rate: it becomes `MIN_RATE` (forwards). Pause instead.
pub const MIN_RATE: f64 = 0.1;

/// The playhead over a recording of a fixed duration: where it is, and whether and how fast it
/// moves.
///
/// Every value is a position in session video seconds, and every method that depends on time takes
/// `now` in seconds from any monotonic clock. The position is guaranteed to stay within
/// `0.0..=duration_s`. See the [module documentation](self) for the model.
///
/// A transport does not decode or draw anything, and nothing here is shared between threads:
/// [`Playback`](super::Playback) keeps one behind a lock and hands out `now`.
///
/// # Example
///
/// A ten second recording, played from the 2 s mark and then paused:
///
/// ```rust
/// use komorebi_recorder::playback::Transport;
///
/// let mut transport = Transport::new(10.0);
/// assert_eq!(transport.position(0.0), 0.0);
/// assert!(!transport.is_playing(0.0));
///
/// // `now` is any monotonic clock: here, 100.0 s on it
/// transport.seek(2.0, 100.0);
/// transport.play(1.0, 100.0);
/// assert_eq!(transport.position(100.5), 2.5);
///
/// transport.pause(101.0);
/// assert_eq!(transport.position(150.0), 3.0, "paused means still");
/// ```
///
/// # Invariants
///
/// That the position stays within the recording, that the rate stays within its limits and that a
/// loop region is never empty are kept by every method, and the fields are private so that no
/// other way of making a `Transport` can bypass them. The compiler enforces the second part:
///
/// ```rust,compile_fail,E0451
/// use komorebi_recorder::playback::Transport;
///
/// let transport = Transport {
///     duration_s: -1.0,
///     anchor_s: 50.0,
///     anchored_at: 0.0,
///     playing: false,
///     rate: 0.0,
///     looping: None,
/// };
/// ```
///
/// [`Transport::new`] is the way in, and it treats a negative duration as zero:
///
/// ```rust
/// use komorebi_recorder::playback::Transport;
///
/// let transport = Transport::new(-1.0);
/// assert_eq!(transport.duration_s(), 0.0);
/// assert_eq!(transport.position(0.0), 0.0);
/// ```
#[derive(Debug, Clone, PartialEq)]
pub struct Transport {
    duration_s: f64,
    /// Where it was at `anchored_at`.
    anchor_s: f64,
    anchored_at: f64,
    playing: bool,
    /// Speed as a multiple of real time; negative plays backwards.
    rate: f64,
    looping: Option<(f64, f64)>,
}

impl Transport {
    /// Returns a paused transport at the start of a recording `duration_s` seconds long.
    ///
    /// The rate is `1.0` and there is no loop region. A negative `duration_s` is treated as `0.0`.
    ///
    /// # Example
    ///
    /// ```rust
    /// use komorebi_recorder::playback::Transport;
    ///
    /// let transport = Transport::new(-3.0);
    /// assert_eq!(transport.duration_s(), 0.0);
    /// assert_eq!(transport.rate(), 1.0);
    /// assert_eq!(transport.looping(), None);
    /// ```
    #[must_use]
    pub fn new(duration_s: f64) -> Self {
        Self {
            duration_s: duration_s.max(0.0),
            anchor_s: 0.0,
            anchored_at: 0.0,
            playing: false,
            rate: 1.0,
            looping: None,
        }
    }

    /// Returns the duration of the recording in seconds, as given to [`Transport::new`].
    #[must_use]
    pub fn duration_s(&self) -> f64 {
        self.duration_s
    }

    /// Returns the speed it plays at when playing, as a multiple of real time: negative for
    /// backwards.
    ///
    /// A paused transport still has a rate, the one it resumes at. The magnitude is guaranteed to be
    /// within [`MIN_RATE`]`..=`[`MAX_RATE`].
    #[must_use]
    pub fn rate(&self) -> f64 {
        self.rate
    }

    /// Returns the loop region as `(from, to)` in session video seconds, if one is set.
    ///
    /// It is guaranteed that `from < to` and that both lie within `0.0..=duration_s`. See
    /// [`Transport::set_loop`].
    #[must_use]
    pub fn looping(&self) -> Option<(f64, f64)> {
        self.looping
    }

    /// Returns where the playhead is at `now`, in session video seconds.
    ///
    /// A paused transport is where it was left. A playing one has moved `rate * (now - then)` from
    /// where it was at `then`, the last time something changed how it moves, and:
    ///
    /// - stops at `0.0` or `duration_s` if that takes it past an end, or
    /// - wraps within the loop region, if there is one, when it passes the far end of the region
    ///   in its direction of travel (see [`Transport::set_loop`]).
    ///
    /// The result is guaranteed to be within `0.0..=duration_s`.
    ///
    /// # Example
    ///
    /// ```rust
    /// use komorebi_recorder::playback::Transport;
    ///
    /// let mut transport = Transport::new(10.0);
    /// transport.seek(8.0, 0.0);
    /// transport.play(1.0, 0.0);
    /// assert_eq!(transport.position(1.5), 9.5);
    /// assert_eq!(transport.position(5.0), 10.0, "it stops at the end");
    ///
    /// // twice as fast, backwards, from the middle
    /// transport.seek(5.0, 10.0);
    /// transport.play(-2.0, 10.0);
    /// assert_eq!(transport.position(11.0), 3.0);
    /// ```
    #[must_use]
    pub fn position(&self, now: f64) -> f64 {
        if !self.playing {
            return self.anchor_s;
        }

        let raw = self.anchor_s + self.rate * (now - self.anchored_at);
        if let Some((from, to)) = self.looping
            && to > from
        {
            if self.rate > 0.0 && raw >= to && self.anchor_s <= to {
                return from + (raw - from).rem_euclid(to - from);
            }
            if self.rate < 0.0 && raw <= from && self.anchor_s >= from {
                return to - (to - raw).rem_euclid(to - from);
            }
        }

        raw.clamp(0.0, self.duration_s)
    }

    /// Returns `true` if and only if the playhead is moving at `now`: it was set playing and has
    /// not run into an end.
    ///
    /// With a loop region set it is moving for as long as it was set playing, since a loop has no
    /// end. That includes a playhead the region does not capture, which has run to an end of the
    /// recording and stays there (see [`Transport::set_loop`]).
    ///
    /// # Example
    ///
    /// ```rust
    /// use komorebi_recorder::playback::Transport;
    ///
    /// let mut transport = Transport::new(10.0);
    /// transport.seek(9.0, 0.0);
    /// transport.play(1.0, 0.0);
    /// assert!(transport.is_playing(0.5));
    /// assert!(!transport.is_playing(2.0), "it ran into the end at 1.0");
    /// ```
    #[must_use]
    pub fn is_playing(&self, now: f64) -> bool {
        if !self.playing {
            return false;
        }
        if self.looping.is_some() {
            return true;
        }

        let raw = self.anchor_s + self.rate * (now - self.anchored_at);
        !(self.rate > 0.0 && raw >= self.duration_s || self.rate < 0.0 && raw <= 0.0)
    }

    /// Settle: if it ran into an end it stops there.
    fn settle(&mut self, now: f64) {
        let here = self.position(now);
        let moving = self.is_playing(now);
        self.anchor_s = here;
        self.anchored_at = now;
        self.playing = moving;
    }

    /// Moves the playhead to `time` at `now`, clamped to `0.0..=duration_s`.
    ///
    /// The rate and the loop region are untouched, and a playing transport carries on playing from
    /// the new place while a paused one stays paused. A `time` outside the recording is clamped,
    /// not an error.
    ///
    /// A transport that ran into an end and stopped there still counts as set playing, so seeking
    /// it sets it moving again from the new place. [`Transport::pause`] first to keep it still.
    ///
    /// # Example
    ///
    /// ```rust
    /// use komorebi_recorder::playback::Transport;
    ///
    /// let mut transport = Transport::new(10.0);
    /// transport.play(1.0, 0.0);
    /// transport.seek(6.0, 2.0);
    /// assert!(transport.is_playing(2.0));
    /// assert_eq!(transport.position(3.0), 7.0);
    ///
    /// transport.seek(99.0, 3.0);
    /// assert_eq!(transport.position(3.0), 10.0);
    /// transport.seek(-5.0, 3.0);
    /// assert_eq!(transport.position(3.0), 0.0);
    ///
    /// // paused stays paused
    /// transport.pause(3.0);
    /// transport.seek(4.0, 4.0);
    /// assert!(!transport.is_playing(5.0));
    /// assert_eq!(transport.position(5.0), 4.0);
    /// ```
    ///
    /// # Example: seeking after running into the end
    ///
    /// ```rust
    /// use komorebi_recorder::playback::Transport;
    ///
    /// let mut transport = Transport::new(10.0);
    /// transport.seek(8.0, 0.0);
    /// transport.play(1.0, 0.0);
    /// assert!(!transport.is_playing(5.0), "stopped at the end");
    ///
    /// transport.seek(1.0, 6.0);
    /// assert!(transport.is_playing(6.5), "moving again");
    /// assert_eq!(transport.position(7.0), 2.0);
    /// ```
    pub fn seek(&mut self, time: f64, now: f64) {
        self.anchor_s = time.clamp(0.0, self.duration_s);
        self.anchored_at = now;
    }

    /// Starts playing at `rate` from where the playhead is at `now`: negative for backwards.
    ///
    /// The magnitude of `rate` is clamped to [`MIN_RATE`]`..=`[`MAX_RATE`], so a rate of zero plays
    /// forwards at `MIN_RATE`. Playing forwards from the end of the recording (or backwards from the
    /// start) starts over from the other end.
    ///
    /// # Example
    ///
    /// ```rust
    /// use komorebi_recorder::playback::transport::{Transport, MAX_RATE};
    ///
    /// let mut transport = Transport::new(10.0);
    /// transport.seek(10.0, 0.0);
    /// transport.play(1.0, 0.0);
    /// assert_eq!(transport.position(1.0), 1.0, "from the end it starts over");
    ///
    /// // backwards, from where it is now, and no faster than the limit
    /// transport.play(-1000.0, 1.0);
    /// assert_eq!(transport.rate(), -MAX_RATE);
    /// assert_eq!(transport.position(1.0), 1.0);
    /// assert_eq!(transport.position(1.125), 0.0);
    /// ```
    pub fn play(&mut self, rate: f64, now: f64) {
        self.settle(now);
        let magnitude = rate.abs().clamp(MIN_RATE, MAX_RATE);
        self.rate = if rate < 0.0 { -magnitude } else { magnitude };

        // playing forwards from the end (or backwards from the start) starts over
        if self.rate > 0.0 && self.anchor_s >= self.duration_s {
            self.anchor_s = 0.0;
        } else if self.rate < 0.0 && self.anchor_s <= 0.0 {
            self.anchor_s = self.duration_s;
        }
        self.anchored_at = now;
        self.playing = true;
    }

    /// Stops the playhead where it is at `now`.
    ///
    /// The rate is kept, so [`Transport::toggle`] resumes at the same speed.
    pub fn pause(&mut self, now: f64) {
        self.settle(now);
        self.playing = false;
    }

    /// Plays if the playhead is not moving at `now`, and pauses if it is.
    ///
    /// Playing resumes at the current rate. A playhead stopped at an end is not moving, so toggling
    /// it plays, starting over as [`Transport::play`] does.
    ///
    /// # Example
    ///
    /// ```rust
    /// use komorebi_recorder::playback::Transport;
    ///
    /// let mut transport = Transport::new(10.0);
    /// transport.set_rate(2.0, 0.0);
    /// transport.toggle(0.0);
    /// assert!(transport.is_playing(1.0));
    /// assert_eq!(transport.position(1.0), 2.0);
    ///
    /// transport.toggle(1.0);
    /// assert!(!transport.is_playing(2.0));
    /// assert_eq!(transport.position(2.0), 2.0);
    ///
    /// // resuming carries on from there at the same speed
    /// transport.toggle(5.0);
    /// assert_eq!(transport.position(6.0), 4.0);
    /// ```
    pub fn toggle(&mut self, now: f64) {
        if self.is_playing(now) {
            self.pause(now);
        } else {
            let rate = self.rate;
            self.play(rate, now);
        }
    }

    /// Changes the speed without moving the playhead and without starting or stopping it.
    ///
    /// The magnitude of `rate` is clamped to [`MIN_RATE`]`..=`[`MAX_RATE`] as in
    /// [`Transport::play`]. If the playhead is moving at `now` it carries on from where it is at the
    /// new speed; otherwise it stays still and the new rate applies when it next plays.
    ///
    /// # Example
    ///
    /// ```rust
    /// use komorebi_recorder::playback::transport::{Transport, MAX_RATE, MIN_RATE};
    ///
    /// let mut transport = Transport::new(10.0);
    /// transport.play(1.0, 0.0);
    /// transport.set_rate(4.0, 2.0);
    /// assert_eq!(transport.position(2.0), 2.0, "the playhead did not jump");
    /// assert_eq!(transport.position(3.0), 6.0);
    ///
    /// // paused stays paused
    /// transport.pause(3.0);
    /// transport.set_rate(0.5, 4.0);
    /// assert!(!transport.is_playing(5.0));
    ///
    /// transport.set_rate(1000.0, 5.0);
    /// assert_eq!(transport.rate(), MAX_RATE);
    /// transport.set_rate(0.0, 5.0);
    /// assert_eq!(transport.rate(), MIN_RATE);
    /// ```
    pub fn set_rate(&mut self, rate: f64, now: f64) {
        let was_playing = self.is_playing(now);
        self.settle(now);
        let magnitude = rate.abs().clamp(MIN_RATE, MAX_RATE);
        self.rate = if rate < 0.0 { -magnitude } else { magnitude };
        self.playing = was_playing;
    }

    /// Sets the region `(from, to)` to repeat while playing, or clears it with `None`.
    ///
    /// Both ends are clamped to `0.0..=duration_s`, and a region that is then empty or backwards
    /// (`to <= from`) clears the loop like `None`. The playhead is settled at `now` first.
    ///
    /// While a region is set and the transport is playing, a playhead that reaches the far end of
    /// the region in its direction of travel wraps to the near end: forwards past `to` it returns
    /// from `from`, backwards past `from` it returns from `to`. A playhead that starts before the
    /// region plays on to it and wraps when it passes the far end, not when it enters. One that is
    /// already past the far end (after `to` going forwards, before `from` going backwards) never
    /// wraps: the region is ignored and it runs to the end of the recording, and
    /// [`Transport::is_playing`] stays `true`. [`Transport::seek`] into the region to start looping.
    ///
    /// # Example
    ///
    /// ```rust
    /// use komorebi_recorder::playback::Transport;
    ///
    /// let mut transport = Transport::new(10.0);
    /// transport.set_loop(Some((2.0, 4.0)), 0.0);
    /// transport.seek(3.0, 0.0);
    /// transport.play(1.0, 0.0);
    ///
    /// assert_eq!(transport.position(0.5), 3.5);
    /// assert_eq!(transport.position(1.5), 2.5, "wrapped past 4.0");
    /// assert_eq!(transport.position(100.0), 3.0);
    /// assert!(transport.is_playing(100.0), "a loop never ends");
    ///
    /// // backwards wraps the other way
    /// transport.play(-1.0, 100.0);
    /// assert_eq!(transport.position(100.5), 2.5);
    /// assert_eq!(transport.position(101.5), 3.5, "wrapped below 2.0");
    ///
    /// // an empty or backwards region is no region, and a long one is cut to the recording
    /// transport.set_loop(Some((5.0, 5.0)), 101.5);
    /// assert_eq!(transport.looping(), None);
    /// transport.set_loop(Some((2.0, 99.0)), 101.5);
    /// assert_eq!(transport.looping(), Some((2.0, 10.0)));
    /// transport.set_loop(None, 101.5);
    /// assert_eq!(transport.looping(), None);
    /// ```
    ///
    /// # Example: a playhead past the region is not looped
    ///
    /// ```rust
    /// use komorebi_recorder::playback::Transport;
    ///
    /// let mut transport = Transport::new(10.0);
    /// transport.set_loop(Some((2.0, 4.0)), 0.0);
    /// transport.seek(8.0, 0.0);
    /// transport.play(1.0, 0.0);
    ///
    /// assert_eq!(transport.position(100.0), 10.0, "it ran to the end");
    /// assert!(transport.is_playing(100.0));
    ///
    /// // seeking into the region starts the loop
    /// transport.seek(3.0, 100.0);
    /// assert_eq!(transport.position(101.5), 2.5);
    /// ```
    pub fn set_loop(&mut self, region: Option<(f64, f64)>, now: f64) {
        self.settle(now);
        self.looping = region
            .map(|(from, to)| {
                (
                    from.clamp(0.0, self.duration_s),
                    to.clamp(0.0, self.duration_s),
                )
            })
            .filter(|(from, to)| to > from);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn close(a: f64, b: f64) -> bool {
        (a - b).abs() < 1e-9
    }

    #[test]
    fn a_new_transport_is_paused_at_the_start() {
        let transport = Transport::new(10.0);
        assert!(close(transport.position(0.0), 0.0));
        assert!(close(transport.position(100.0), 0.0));
        assert!(!transport.is_playing(100.0));
    }

    #[test]
    fn playing_advances_with_the_clock_not_with_frames() {
        let mut transport = Transport::new(10.0);
        transport.seek(2.0, 50.0);
        transport.play(1.0, 50.0);

        assert!(close(transport.position(50.0), 2.0));
        assert!(close(transport.position(50.5), 2.5));
        assert!(close(transport.position(53.25), 5.25));
        assert!(transport.is_playing(53.25));
    }

    #[test]
    fn the_speed_scales_the_movement_and_can_run_backwards() {
        let mut transport = Transport::new(10.0);
        transport.seek(5.0, 0.0);
        transport.play(2.0, 0.0);
        assert!(close(transport.position(1.0), 7.0));

        transport.play(-0.5, 1.0);
        assert!(close(transport.position(1.0), 7.0));
        assert!(close(transport.position(3.0), 6.0));
        assert!(transport.rate() < 0.0);
    }

    #[test]
    fn pausing_freezes_where_it_was() {
        let mut transport = Transport::new(10.0);
        transport.play(1.0, 0.0);
        transport.pause(3.0);

        assert!(close(transport.position(3.0), 3.0));
        assert!(close(transport.position(99.0), 3.0));
        assert!(!transport.is_playing(99.0));

        // resuming carries on from there at the same speed
        transport.toggle(100.0);
        assert!(close(transport.position(101.0), 4.0));
    }

    #[test]
    fn it_stops_at_either_end() {
        let mut transport = Transport::new(10.0);
        transport.seek(8.0, 0.0);
        transport.play(1.0, 0.0);
        assert!(close(transport.position(5.0), 10.0));
        assert!(!transport.is_playing(5.0));

        transport.seek(1.0, 6.0);
        transport.play(-1.0, 6.0);
        assert!(close(transport.position(9.0), 0.0));
        assert!(!transport.is_playing(9.0));
    }

    #[test]
    fn playing_from_the_end_starts_over() {
        let mut transport = Transport::new(10.0);
        transport.seek(10.0, 0.0);
        transport.play(1.0, 0.0);
        assert!(close(transport.position(1.0), 1.0));

        transport.seek(0.0, 2.0);
        transport.play(-1.0, 2.0);
        assert!(close(transport.position(3.0), 9.0));
    }

    #[test]
    fn seeking_while_playing_keeps_playing_from_the_new_place() {
        let mut transport = Transport::new(10.0);
        transport.play(1.0, 0.0);
        transport.seek(6.0, 2.0);

        assert!(transport.is_playing(2.0));
        assert!(close(transport.position(2.0), 6.0));
        assert!(close(transport.position(3.0), 7.0));

        // and out of range is clamped, not an error
        transport.seek(99.0, 3.0);
        assert!(close(transport.position(3.0), 10.0));
        transport.seek(-5.0, 3.0);
        assert!(close(transport.position(3.0), 0.0));
    }

    #[test]
    fn a_region_repeats() {
        let mut transport = Transport::new(10.0);
        transport.set_loop(Some((2.0, 4.0)), 0.0);
        transport.seek(3.0, 0.0);
        transport.play(1.0, 0.0);

        assert!(close(transport.position(0.5), 3.5));
        assert!(close(transport.position(1.5), 2.5), "wrapped past 4.0");
        assert!(close(transport.position(100.0), 3.0));
        assert!(transport.is_playing(100.0), "a loop never ends");

        // backwards wraps the other way
        transport.play(-1.0, 100.0);
        assert!(close(transport.position(100.5), 2.5));
        assert!(close(transport.position(101.5), 3.5), "wrapped below 2.0");
    }

    #[test]
    fn playing_into_a_region_from_before_it_does_not_wrap_early() {
        let mut transport = Transport::new(10.0);
        transport.set_loop(Some((4.0, 6.0)), 0.0);
        transport.seek(1.0, 0.0);
        transport.play(1.0, 0.0);

        assert!(close(transport.position(2.0), 3.0));
        assert!(close(transport.position(4.0), 5.0));
        assert!(
            close(transport.position(5.5), 4.5),
            "wrapped once it passed the end of the region"
        );
    }

    #[test]
    fn a_degenerate_region_is_no_region() {
        let mut transport = Transport::new(10.0);
        transport.set_loop(Some((5.0, 5.0)), 0.0);
        assert_eq!(transport.looping(), None);
        transport.set_loop(Some((6.0, 3.0)), 0.0);
        assert_eq!(transport.looping(), None);
        transport.set_loop(Some((2.0, 99.0)), 0.0);
        assert_eq!(transport.looping(), Some((2.0, 10.0)));
    }

    #[test]
    fn changing_the_speed_does_not_move_the_playhead_or_start_it() {
        let mut transport = Transport::new(10.0);
        transport.play(1.0, 0.0);
        transport.set_rate(4.0, 2.0);
        assert!(close(transport.position(2.0), 2.0));
        assert!(close(transport.position(3.0), 6.0));

        transport.pause(3.0);
        transport.set_rate(0.5, 4.0);
        assert!(!transport.is_playing(5.0));
        assert!(close(transport.position(5.0), 6.0));
        assert!(close(transport.rate(), 0.5));

        // out-of-range speeds are held to the limits
        transport.set_rate(1000.0, 5.0);
        assert!(close(transport.rate(), MAX_RATE));
        transport.set_rate(0.0, 5.0);
        assert!(close(transport.rate(), MIN_RATE));
    }
}
