//! The animator's bookkeeping, kept free of threads, clocks and macOS APIs
//!
//! [`Tracks`] knows which windows are in flight, where each one is, where it is going and what has
//! to be written to it next. It never performs a write itself: [`Tracks::tick`] hands back a list of
//! [`Commit`]s for the runtime to carry out, and the runtime reports how they went. That keeps the
//! interesting behaviour (retargeting, coalescing what changed, verifying a window arrived,
//! recognising our own AX notifications) testable with a fake clock.

use super::step::FrameF;
use super::step::Motion;
use super::step::MotionSpec;
use super::step::step;
use crate::core::Rect;
use crate::core::animation::AnimationStyle;
use std::collections::HashMap;
use std::collections::HashSet;
use std::time::Duration;
use std::time::Instant;

pub type WindowId = u32;

/// How many times a finished window is checked against where it was supposed to end up
const VERIFY_ATTEMPTS: u8 = 3;
const FIRST_VERIFY_DELAY: Duration = Duration::from_millis(50);
const NEXT_VERIFY_DELAY: Duration = Duration::from_millis(100);

/// How long a lane may sit on something it was handed before it is assumed to have lost it
///
/// A lane that panics, or could not be started, takes whatever it was holding with it. Without a
/// limit the window would be waited on forever, which would leave it looking as if it were still
/// being animated.
const STALE_AFTER: Duration = Duration::from_secs(3);

/// How many windows are remembered as having had their journey cut short, before the oldest news is
/// let go, as nothing guarantees that anybody will ever ask
const MAX_UNFINISHED: usize = 1024;

/// A window that cannot be written to this many times in a row is given up on
const MAX_WRITE_FAILURES: u8 = 5;

/// How long after our last write a move/resize notification is still assumed to be an echo of it
///
/// Apps report moves and resizes late, and in bursts when they are busy. Being generous costs
/// nothing: a person grabbing a window has the mouse button down, which is checked separately.
pub const NOTIFICATION_GRACE: Duration = Duration::from_millis(300);

/// How many of the frames we wrote are remembered to recognise echoes of them
const RING: usize = 8;

/// How close (in pixels) a reported window origin has to be to one we wrote to count as an echo
///
/// Only the origin is compared. Apps that snap their size to a grid, or refuse to be smaller than
/// some size, report a size that is nothing like what we asked for, and that must not be mistaken
/// for somebody dragging the window: being wrong that way cancels an animation that was fine, and
/// being wrong the other way round only lets an animation finish that was about to anyway.
const ECHO_TOLERANCE: u32 = 8;

/// How long a window is remembered after it stops animating
const RECENT_TTL: Duration = Duration::from_secs(1);

/// How long an app is remembered after the last time one of its windows was animated
pub const PROCESS_MEMORY: Duration = Duration::from_secs(5);

/// How many times as long as an app takes to apply a size it is left between being sent one
///
/// An app that takes 60ms to lay itself out cannot be sent a size every 16ms: the sizes would queue
/// up in front of everything else it is asked to do, the positions of its windows included, and
/// whatever the app is shown at the end of the queue is no more current than what it was shown
/// instead. Sending them as often as it can keep up with, and a little less, leaves it room.
const SIZE_BACKOFF: u32 = 2;

/// No app is left longer than this between being sent one size and the next
const LONGEST_SIZE_GAP: Duration = Duration::from_millis(250);

/// How long the last frame we put a window in is trusted to be where it still is
///
/// Asking an app where its window is takes a round trip to it, and doing that for every window
/// before an animation can start means the windows start moving one after another instead of
/// together. Anything that moves a window behind our back is reported to us by the app, which is
/// when what we remember is forgotten, so this is only the backstop for reports that go missing.
pub const KNOWN_FRAME_TTL: Duration = Duration::from_secs(10);

/// An animation that has run this much longer than it was supposed to is ended by force
const OVERRUN_ALLOWANCE: Duration = Duration::from_secs(1);

/// No animation is allowed to run for longer than this, however long it is configured to take
const LONGEST_ALLOWANCE: Duration = Duration::from_secs(24 * 60 * 60);

#[derive(Debug, Clone, PartialEq)]
pub struct Write<H> {
    pub window_id: WindowId,
    pub pid: i32,
    pub handle: H,
    /// Identifies the retarget this write belongs to
    pub generation: u64,
    /// The whole frame this write brings the window to
    pub frame: Rect,
    /// Set when the origin changed since the last write
    pub position: Option<(i32, i32)>,
    /// Set when the size changed since the last write
    pub size: Option<(i32, i32)>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Verify<H> {
    pub window_id: WindowId,
    pub pid: i32,
    pub handle: H,
    pub generation: u64,
    pub target: Rect,
    /// Whether the window is dropped even if it is still not where it should be
    pub last_attempt: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Commit<H> {
    Write(Write<H>),
    Verify(Verify<H>),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WriteOutcome {
    Written,
    /// The window was no longer being animated by the time the write came to be made, so it was not
    /// made, and nothing is known about where the window is as a result
    Skipped,
    /// `gone` means the window no longer exists, as opposed to it merely being busy
    Failed {
        gone: bool,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VerifyOutcome {
    /// The window is where it should be
    Settled,
    /// The window was elsewhere and has been told to go to the target again
    Reissued,
    Failed {
        gone: bool,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Retargeted {
    /// The window was not being animated
    NotTracked,
    /// The window is already heading there in the same way
    Unchanged,
    Updated,
}

/// When the animator needs to run next
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Wakeup {
    /// There is nothing to do until something is retargeted or reported
    Idle,
    /// A window is moving, so a frame is due
    Frame,
    /// Only windows that are being verified are left, and the first check is due at this time
    At(Instant),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Phase {
    Moving,
    Verifying {
        attempts_left: u8,
        next_at: Instant,
        /// A verification has been handed out and has not been reported on yet
        awaiting: bool,
    },
}

#[derive(Debug, Clone)]
struct Track<H> {
    handle: H,
    pid: i32,
    current: FrameF,
    target: Rect,
    motion: Motion,
    /// When something was last handed out to be done to this window
    last_dispatch: Instant,
    /// When the animation is ended by force, wherever it has got to
    deadline: Instant,
    /// The frame most recently handed out to be written
    dispatched: Rect,
    /// The frame most recently reported as written
    confirmed: Rect,
    generation: u64,
    phase: Phase,
    failures: u8,
    /// Where the window is sent, instantly, once it has arrived and been verified
    after: Option<Rect>,
    /// When a size was last handed out to be applied to this window
    last_size_dispatch: Instant,
}

#[derive(Debug, Clone)]
struct Recent {
    last_write: Instant,
    frames: [Rect; RING],
    len: usize,
    next: usize,
}

impl Recent {
    fn new(now: Instant) -> Self {
        Self {
            last_write: now,
            frames: [Rect::default(); RING],
            len: 0,
            next: 0,
        }
    }

    fn push(&mut self, frame: Rect) {
        self.frames[self.next] = frame;
        self.next = (self.next + 1) % RING;
        self.len = (self.len + 1).min(RING);
    }

    fn frames(&self) -> impl Iterator<Item = Rect> + '_ {
        self.frames.iter().take(self.len).copied()
    }
}

/// What is known about a window a move/resize notification arrived for
#[derive(Debug, Clone, PartialEq)]
pub struct NotificationContext {
    /// The window is still being animated, as opposed to having only just stopped
    pub animating: bool,
    /// The frames we most recently wrote, newest last
    pub written: Vec<Rect>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Verdict {
    /// Not ours: let the window manager see it
    Forward,
    /// An echo of our own write: swallow it
    Suppress,
    /// Somebody else is moving the window: let the window manager see it and stop animating it
    ForwardAndCancel,
}

/// Decide what to do with a move/resize notification
///
/// `observed` is the frame the window has right now, and is only consulted while the mouse button is
/// down: that is the one situation where a notification could be a person dragging the window rather
/// than an echo of one of our own writes.
pub fn decide(
    context: Option<&NotificationContext>,
    mouse_down: bool,
    observed: Option<Rect>,
) -> Verdict {
    let Some(context) = context else {
        return Verdict::Forward;
    };

    if !mouse_down {
        return Verdict::Suppress;
    }

    match observed {
        Some(observed)
            if context
                .written
                .iter()
                .any(|written| close_enough(*written, observed)) =>
        {
            Verdict::Suppress
        }
        _ => Verdict::ForwardAndCancel,
    }
}

fn close_enough(a: Rect, b: Rect) -> bool {
    a.left.abs_diff(b.left) <= ECHO_TOLERANCE && a.top.abs_diff(b.top) <= ECHO_TOLERANCE
}

#[derive(Debug)]
pub struct Tracks<H> {
    tracks: HashMap<WindowId, Track<H>>,
    in_flight: HashMap<WindowId, u32>,
    recent: HashMap<WindowId, Recent>,
    /// The same, by process: apps do not always report a move on the window we moved, as with the
    /// tabs of a native tabbed window, which all move together but report as whichever is showing
    recent_processes: HashMap<i32, Recent>,
    /// Windows whose journey ended before they had settled, which is to say they were cut short
    unfinished: HashSet<WindowId>,
    /// Where each window was last put, and when
    known: HashMap<WindowId, (Rect, Instant)>,
    /// How long each app takes to apply a new size
    size_cost: HashMap<i32, Duration>,
    next_generation: u64,
    last_prune: Instant,
}

impl<H: Clone> Tracks<H> {
    pub fn new(now: Instant) -> Self {
        Self {
            tracks: HashMap::new(),
            in_flight: HashMap::new(),
            recent: HashMap::new(),
            recent_processes: HashMap::new(),
            unfinished: HashSet::new(),
            known: HashMap::new(),
            size_cost: HashMap::new(),
            next_generation: 1,
            last_prune: now,
        }
    }

    pub fn is_tracked(&self, window_id: WindowId) -> bool {
        self.tracks.contains_key(&window_id)
    }

    pub fn is_empty(&self) -> bool {
        self.tracks.is_empty()
    }

    pub fn len(&self) -> usize {
        self.tracks.len()
    }

    pub fn in_flight(&self, window_id: WindowId) -> u32 {
        self.in_flight.get(&window_id).copied().unwrap_or_default()
    }

    pub fn total_in_flight(&self) -> u32 {
        self.in_flight.values().sum()
    }

    /// Where a window is right now, as far as the animation is concerned
    pub fn current_frame(&self, window_id: WindowId) -> Option<Rect> {
        self.tracks
            .get(&window_id)
            .map(|track| track.current.round())
    }

    pub fn target(&self, window_id: WindowId) -> Option<Rect> {
        self.tracks.get(&window_id).map(|track| track.target)
    }

    /// Aim an animated window somewhere new
    ///
    /// The window carries on from where it is, so there is nothing to wait for and, for the
    /// exponential motion, no restart: the velocity follows from the remaining distance.
    pub fn retarget(
        &mut self,
        window_id: WindowId,
        target: Rect,
        spec: MotionSpec,
        now: Instant,
    ) -> Retargeted {
        let Some(track) = self.tracks.get_mut(&window_id) else {
            return Retargeted::NotTracked;
        };

        if track.target == target && spec.matches(&track.motion) {
            return Retargeted::Unchanged;
        }

        track.target = target;
        track.motion = spec.begin(track.current, now);
        track.deadline = now + allowance(spec);
        track.generation = self.next_generation;
        self.next_generation += 1;
        track.phase = Phase::Moving;
        track.failures = 0;
        track.after = None;

        Retargeted::Updated
    }

    /// Send a window somewhere else, instantly, as soon as it has arrived where it is going
    ///
    /// This only lasts until the window is next retargeted, as that means it is no longer going
    /// where this was meant to follow on from.
    pub fn set_after(&mut self, window_id: WindowId, after: Option<Rect>) {
        if let Some(track) = self.tracks.get_mut(&window_id) {
            track.after = after;
        }
    }

    /// Start animating a window that was not being animated
    ///
    /// `start` is where the window is. If the window is already tracked this retargets it instead.
    #[allow(clippy::too_many_arguments)]
    pub fn insert(
        &mut self,
        window_id: WindowId,
        handle: H,
        pid: i32,
        start: Rect,
        target: Rect,
        spec: MotionSpec,
        now: Instant,
    ) -> Retargeted {
        if self.is_tracked(window_id) {
            return self.retarget(window_id, target, spec, now);
        }

        let current = FrameF::from(start);
        let generation = self.next_generation;
        self.next_generation += 1;
        self.unfinished.remove(&window_id);

        self.tracks.insert(
            window_id,
            Track {
                handle,
                pid,
                current,
                target,
                motion: spec.begin(current, now),
                last_dispatch: now,
                deadline: now + allowance(spec),
                dispatched: start,
                confirmed: start,
                generation,
                phase: Phase::Moving,
                failures: 0,
                after: None,
                last_size_dispatch: now.checked_sub(Duration::from_secs(1)).unwrap_or(now),
            },
        );

        Retargeted::Updated
    }

    /// Stop animating a window, leaving it wherever it has got to
    ///
    /// Returns the process the window belongs to if it was being animated.
    pub fn remove(&mut self, window_id: WindowId) -> Option<i32> {
        self.remove_track(window_id).map(|(pid, _)| pid)
    }

    /// Stop animating a window, and say where it was heading
    pub fn remove_track(&mut self, window_id: WindowId) -> Option<(i32, Rect)> {
        let track = self.tracks.remove(&window_id)?;

        // however it ended, it ended before the window had settled
        self.mark_unfinished(window_id);

        // whatever was on its way to the window may or may not have got there
        self.known.remove(&window_id);

        Some((track.pid, track.target))
    }

    /// Record how long an app took to apply a size to one of its windows
    pub fn note_size_cost(&mut self, pid: i32, took: Duration) {
        let cost = match self.size_cost.get(&pid) {
            Some(known) => known.mul_f64(0.7) + took.mul_f64(0.3),
            None => took,
        };

        self.size_cost.insert(pid, cost);
    }

    /// Where a window is, as far as we know without having to ask it
    ///
    /// This is where it is now if it is being animated, or otherwise where we last put it, for as
    /// long as nothing is known to have moved it since.
    pub fn known_frame(&self, window_id: WindowId, now: Instant) -> Option<Rect> {
        if let Some(frame) = self.current_frame(window_id) {
            return Some(frame);
        }

        self.known
            .get(&window_id)
            .filter(|(_, at)| now.saturating_duration_since(*at) <= KNOWN_FRAME_TTL)
            .map(|(frame, _)| *frame)
    }

    /// Stop trusting what is remembered about where a window is
    pub fn forget(&mut self, window_id: WindowId) {
        self.known.remove(&window_id);
    }

    /// Whether there are windows that are moving, and have writes that have not been applied yet
    ///
    /// A frame is not worth starting while this is so: the windows that have been written to would
    /// be a frame ahead of the ones that have not, and the gaps between neighbouring windows
    /// would open and close.
    pub fn moving_windows_are_waiting(&self) -> bool {
        self.tracks.iter().any(|(window_id, track)| {
            matches!(track.phase, Phase::Moving) && self.in_flight(*window_id) > 0
        })
    }

    /// Whether a window's journey was cut short, which is to say it ended in any way other than the
    /// window being found where it was sent
    ///
    /// A journey is cut short by a window being grabbed, taken over, given up on, or not being
    /// where it was sent when it was looked at, as well as by one that was to go on to somewhere
    /// else afterwards never getting there. This is only reported once.
    pub fn take_unfinished(&mut self, window_id: WindowId) -> bool {
        self.unfinished.remove(&window_id)
    }

    fn mark_unfinished(&mut self, window_id: WindowId) {
        if self.unfinished.len() >= MAX_UNFINISHED {
            self.unfinished.clear();
        }

        self.unfinished.insert(window_id);
    }

    /// Which retarget a window is currently being animated for
    pub fn generation(&self, window_id: WindowId) -> Option<u64> {
        self.tracks.get(&window_id).map(|track| track.generation)
    }

    /// Whether any window of an app has been animated lately, or is being animated now
    pub fn was_animating_recently(&self, pid: i32, now: Instant) -> bool {
        self.tracks.values().any(|track| track.pid == pid)
            || self.recent_processes.get(&pid).is_some_and(|recent| {
                now.saturating_duration_since(recent.last_write) <= PROCESS_MEMORY
            })
    }

    /// Stop animating every window
    pub fn drain(&mut self) -> Vec<(WindowId, i32)> {
        let mut drained = Vec::new();

        for (window_id, track) in self.tracks.drain() {
            self.known.remove(&window_id);
            self.unfinished.insert(window_id);

            drained.push((window_id, track.pid));
        }

        drained
    }

    /// When the animator has to run next
    pub fn wakeup(&self) -> Wakeup {
        let mut wakeup = Wakeup::Idle;

        for (&window_id, track) in &self.tracks {
            match track.phase {
                Phase::Moving => return Wakeup::Frame,
                Phase::Verifying {
                    next_at,
                    awaiting: false,
                    ..
                } => {
                    // with writes still outstanding there is nothing to check yet, and reporting
                    // them is what wakes the animator up again: an overdue time here would make it
                    // spin until they come back. If they never do, the window is given up on
                    let due = if self.in_flight(window_id) > 0 {
                        track.last_dispatch + STALE_AFTER
                    } else {
                        next_at
                    };

                    wakeup = earliest(wakeup, due);
                }
                Phase::Verifying { awaiting: true, .. } => {
                    wakeup = earliest(wakeup, track.last_dispatch + STALE_AFTER);
                }
            }
        }

        wakeup
    }

    /// Advance every window by one frame
    ///
    /// Only what changed is handed back: a window that did not move since its last write needs no
    /// write, and a window that moved but did not change size needs no size write.
    pub fn tick(&mut self, now: Instant) -> Vec<Commit<H>> {
        let mut commits = Vec::new();
        let mut abandoned = Vec::new();

        for (&window_id, track) in &mut self.tracks {
            match track.phase {
                Phase::Moving => {
                    let target = FrameF::from(track.target);
                    let (frame, finished) = if now >= track.deadline {
                        (target, true)
                    } else {
                        let stepped = step(&track.motion, target, now);
                        (stepped.frame, stepped.finished)
                    };

                    track.current = frame;
                    let mut rounded = if finished {
                        track.target
                    } else {
                        frame.round()
                    };

                    // an app that cannot keep up with sizes is sent them as often as it can, and the
                    // window carries on moving in the meantime. The size it ends up is never held back
                    if !finished
                        && (rounded.right != track.dispatched.right
                            || rounded.bottom != track.dispatched.bottom)
                        && now.saturating_duration_since(track.last_size_dispatch)
                            < size_gap(&self.size_cost, track.pid)
                    {
                        rounded.right = track.dispatched.right;
                        rounded.bottom = track.dispatched.bottom;
                    }

                    if rounded != track.dispatched {
                        let position = (rounded.left != track.dispatched.left
                            || rounded.top != track.dispatched.top)
                            .then_some((rounded.left, rounded.top));
                        let size = (rounded.right != track.dispatched.right
                            || rounded.bottom != track.dispatched.bottom)
                            .then_some((rounded.right, rounded.bottom));

                        if size.is_some() {
                            track.last_size_dispatch = now;
                        }

                        track.dispatched = rounded;
                        track.last_dispatch = now;
                        *self.in_flight.entry(window_id).or_default() += 1;

                        let recent = self
                            .recent
                            .entry(window_id)
                            .or_insert_with(|| Recent::new(now));
                        recent.last_write = now;
                        recent.push(rounded);

                        let recent = self
                            .recent_processes
                            .entry(track.pid)
                            .or_insert_with(|| Recent::new(now));
                        recent.last_write = now;
                        recent.push(rounded);

                        commits.push(Commit::Write(Write {
                            window_id,
                            pid: track.pid,
                            handle: track.handle.clone(),
                            generation: track.generation,
                            frame: rounded,
                            position,
                            size,
                        }));
                    }

                    if finished {
                        track.phase = Phase::Verifying {
                            attempts_left: VERIFY_ATTEMPTS,
                            next_at: now + FIRST_VERIFY_DELAY,
                            awaiting: false,
                        };
                    }
                }
                Phase::Verifying {
                    attempts_left,
                    next_at,
                    awaiting: false,
                } => {
                    let mut outstanding = self.in_flight.get(&window_id).copied().unwrap_or(0);

                    // whatever was handed out has not come back in far too long
                    if outstanding > 0
                        && now.saturating_duration_since(track.last_dispatch) >= STALE_AFTER
                    {
                        tracing::warn!(
                            "{outstanding} writes to window {window_id} were never reported on, forgetting them"
                        );
                        self.in_flight.remove(&window_id);
                        outstanding = 0;
                    }

                    // a verification only makes sense once everything we wrote has been applied
                    if now >= next_at && outstanding == 0 {
                        track.phase = Phase::Verifying {
                            attempts_left,
                            next_at,
                            awaiting: true,
                        };
                        track.last_dispatch = now;
                        *self.in_flight.entry(window_id).or_default() += 1;

                        commits.push(Commit::Verify(Verify {
                            window_id,
                            pid: track.pid,
                            handle: track.handle.clone(),
                            generation: track.generation,
                            target: track.target,
                            last_attempt: attempts_left <= 1,
                        }));
                    }
                }
                Phase::Verifying { awaiting: true, .. } => {
                    // the verification never came back, so the window is not worth waiting on
                    if now.saturating_duration_since(track.last_dispatch) >= STALE_AFTER {
                        abandoned.push(window_id);
                    }
                }
            }
        }

        for window_id in abandoned {
            tracing::warn!("the verification of window {window_id} was never reported on");
            self.remove_track(window_id);
            self.in_flight.remove(&window_id);
        }

        self.prune(now);

        commits
    }

    /// Record how `count` coalesced writes to a window went
    ///
    /// Returns the process the window belongs to if it was given up on.
    pub fn write_done(
        &mut self,
        window_id: WindowId,
        count: u32,
        frame: Rect,
        outcome: WriteOutcome,
        now: Instant,
    ) -> Option<i32> {
        self.settle_in_flight(window_id, count);

        if let Some(recent) = self.recent.get_mut(&window_id) {
            recent.last_write = now;
        }

        if let Some(pid) = self.tracks.get(&window_id).map(|track| track.pid)
            && let Some(recent) = self.recent_processes.get_mut(&pid)
        {
            recent.last_write = now;
        }

        let track = self.tracks.get_mut(&window_id)?;

        match outcome {
            WriteOutcome::Skipped => None,
            WriteOutcome::Written => {
                track.confirmed = frame;
                track.failures = 0;
                self.known.insert(window_id, (frame, now));

                None
            }
            WriteOutcome::Failed { gone: true } => self.remove(window_id),
            WriteOutcome::Failed { gone: false } => {
                track.failures += 1;

                if track.failures >= MAX_WRITE_FAILURES {
                    return self.remove(window_id);
                }

                // try again with the next frame: act as though this write never happened
                track.dispatched = track.confirmed;
                track.phase = Phase::Moving;

                None
            }
        }
    }

    /// Record how a verification went
    ///
    /// Returns the process the window belongs to if it is no longer being animated.
    pub fn verify_done(
        &mut self,
        window_id: WindowId,
        generation: u64,
        outcome: VerifyOutcome,
        now: Instant,
    ) -> Option<i32> {
        self.settle_in_flight(window_id, 1);

        let track = self.tracks.get_mut(&window_id)?;

        // the window was retargeted while it was being checked, so the result is about a target
        // that nobody wants any more
        if track.generation != generation {
            return None;
        }

        let Phase::Verifying { attempts_left, .. } = track.phase else {
            return None;
        };

        match outcome {
            VerifyOutcome::Settled => {
                let Some(after) = track.after.take() else {
                    let target = track.target;

                    // this is how a journey is meant to end, so it is not cut short
                    let pid = self.tracks.remove(&window_id).map(|track| track.pid);

                    // it is where it was sent, which is worth remembering for the next time
                    self.known.insert(window_id, (target, now));

                    return pid;
                };

                // carry straight on: the next frame completes the move in one step
                track.target = after;
                track.motion = Motion {
                    start: track.current,
                    started: now,
                    duration: Duration::ZERO,
                    style: AnimationStyle::Linear,
                };
                track.deadline = now + OVERRUN_ALLOWANCE;
                track.generation = self.next_generation;
                self.next_generation += 1;
                track.phase = Phase::Moving;
                track.failures = 0;

                None
            }
            VerifyOutcome::Failed { .. } => self.remove(window_id),
            VerifyOutcome::Reissued => {
                let attempts_left = attempts_left.saturating_sub(1);

                if attempts_left == 0 {
                    tracing::debug!(
                        "window {window_id} did not reach {:?}, giving up on it",
                        track.target
                    );

                    return self.remove(window_id);
                }

                track.phase = Phase::Verifying {
                    attempts_left,
                    next_at: now + NEXT_VERIFY_DELAY,
                    awaiting: false,
                };

                None
            }
        }
    }

    /// What is known about a window that a move/resize notification has just arrived for
    ///
    /// A notification is taken to be an echo of what we did if it is for a window we are moving, or
    /// have just moved, or for any window of an app that has one of its windows being moved: native
    /// tabs move together but report as whichever is showing, and apps are not obliged to report on
    /// the window we moved.
    pub fn notification_context(
        &mut self,
        window_id: WindowId,
        pid: i32,
        now: Instant,
    ) -> Option<NotificationContext> {
        let window_is_animating = self.is_tracked(window_id);
        let process_is_animating = self.tracks.values().any(|track| track.pid == pid);
        let animating = window_is_animating || process_is_animating;

        let by_window = self.recent.get(&window_id);
        let by_process = self.recent_processes.get(&pid);

        let is_recent = |recent: Option<&Recent>| {
            recent.is_some_and(|recent| {
                now.saturating_duration_since(recent.last_write) <= NOTIFICATION_GRACE
            })
        };

        if !animating && !is_recent(by_window) && !is_recent(by_process) {
            return None;
        }

        let mut written = by_window
            .into_iter()
            .chain(by_process)
            .flat_map(Recent::frames)
            .collect::<Vec<_>>();

        written.extend(
            self.tracks
                .iter()
                .filter(|(id, track)| **id == window_id || track.pid == pid)
                .map(|(_, track)| track.target),
        );

        if written.is_empty() {
            return None;
        }

        Some(NotificationContext { animating, written })
    }

    fn settle_in_flight(&mut self, window_id: WindowId, count: u32) {
        if let Some(in_flight) = self.in_flight.get_mut(&window_id) {
            *in_flight = in_flight.saturating_sub(count);

            if *in_flight == 0 {
                self.in_flight.remove(&window_id);
            }
        }
    }

    fn prune(&mut self, now: Instant) {
        if now.saturating_duration_since(self.last_prune) < RECENT_TTL {
            return;
        }

        self.last_prune = now;

        let tracks = &self.tracks;
        self.recent.retain(|window_id, recent| {
            tracks.contains_key(window_id)
                || now.saturating_duration_since(recent.last_write) < RECENT_TTL
        });
        self.known
            .retain(|_, (_, at)| now.saturating_duration_since(*at) <= KNOWN_FRAME_TTL);
        self.recent_processes.retain(|pid, recent| {
            tracks.values().any(|track| track.pid == *pid)
                || now.saturating_duration_since(recent.last_write) < PROCESS_MEMORY
        });
    }
}

/// How long to leave an app between one size and the next
fn size_gap(costs: &HashMap<i32, Duration>, pid: i32) -> Duration {
    costs.get(&pid).map_or(Duration::ZERO, |cost| {
        (*cost * SIZE_BACKOFF).min(LONGEST_SIZE_GAP)
    })
}

fn earliest(wakeup: Wakeup, at: Instant) -> Wakeup {
    match wakeup {
        Wakeup::At(earlier) if earlier <= at => Wakeup::At(earlier),
        _ => Wakeup::At(at),
    }
}

fn allowance(spec: MotionSpec) -> Duration {
    // a deadline too far away to be represented would panic when it was added to the clock
    spec.duration
        .saturating_add(OVERRUN_ALLOWANCE)
        .min(LONGEST_ALLOWANCE)
}

#[cfg(test)]
mod tests {
    use super::*;

    const A: WindowId = 1;
    const B: WindowId = 2;
    const FRAME: Duration = Duration::from_millis(16);

    fn rect(left: i32, top: i32, width: i32, height: i32) -> Rect {
        Rect {
            left,
            top,
            right: width,
            bottom: height,
        }
    }

    fn timed(millis: u64) -> MotionSpec {
        MotionSpec {
            duration: Duration::from_millis(millis),
            style: AnimationStyle::Linear,
        }
    }

    /// The motion most tests animate with: quick enough that they do not have to run for long
    fn quick() -> MotionSpec {
        timed(300)
    }

    fn tracks(now: Instant) -> Tracks<&'static str> {
        Tracks::new(now)
    }

    fn writes(commits: &[Commit<&'static str>]) -> Vec<Write<&'static str>> {
        commits
            .iter()
            .filter_map(|commit| match commit {
                Commit::Write(write) => Some(write.clone()),
                Commit::Verify(_) => None,
            })
            .collect()
    }

    fn verifies(commits: &[Commit<&'static str>]) -> Vec<Verify<&'static str>> {
        commits
            .iter()
            .filter_map(|commit| match commit {
                Commit::Verify(verify) => Some(verify.clone()),
                Commit::Write(_) => None,
            })
            .collect()
    }

    /// Run frames until the window has been told to go to its target, reporting every write as done
    fn run_to_target(
        tracks: &mut Tracks<&'static str>,
        window_id: WindowId,
        mut now: Instant,
    ) -> Instant {
        let target = tracks.target(window_id).unwrap();

        for _ in 0..600 {
            now += FRAME;

            for write in writes(&tracks.tick(now)) {
                tracks.write_done(write.window_id, 1, write.frame, WriteOutcome::Written, now);

                if write.frame == target {
                    return now;
                }
            }
        }

        panic!("window {window_id} never reached {target:?}");
    }

    #[test]
    fn an_untracked_window_is_not_retargeted() {
        let now = Instant::now();
        let mut tracks = tracks(now);

        assert_eq!(
            tracks.retarget(A, rect(0, 0, 10, 10), quick(), now),
            Retargeted::NotTracked
        );
        assert!(tracks.is_empty());
    }

    #[test]
    fn a_new_window_does_not_write_what_it_already_has() {
        let now = Instant::now();
        let mut tracks = tracks(now);
        let start = rect(0, 0, 800, 600);

        tracks.insert(A, "a", 1, start, start, quick(), now);

        assert!(writes(&tracks.tick(now + FRAME)).is_empty());
    }

    #[test]
    fn a_pure_move_only_writes_the_position() {
        let now = Instant::now();
        let mut tracks = tracks(now);

        tracks.insert(
            A,
            "a",
            1,
            rect(0, 0, 800, 600),
            rect(1000, 0, 800, 600),
            quick(),
            now,
        );

        for tick in 1..=10 {
            for write in writes(&tracks.tick(now + FRAME * tick)) {
                assert!(write.position.is_some());
                assert_eq!(write.size, None);
            }
        }
    }

    #[test]
    fn a_pure_resize_only_writes_the_size() {
        let now = Instant::now();
        let mut tracks = tracks(now);

        tracks.insert(
            A,
            "a",
            1,
            rect(100, 100, 800, 600),
            rect(100, 100, 400, 600),
            quick(),
            now,
        );

        let mut saw_write = false;
        for tick in 1..=10 {
            for write in writes(&tracks.tick(now + FRAME * tick)) {
                saw_write = true;
                assert_eq!(write.position, None);
                assert!(write.size.is_some());
            }
        }

        assert!(saw_write);
    }

    #[test]
    fn the_final_write_is_exactly_the_target() {
        let now = Instant::now();
        let mut tracks = tracks(now);
        let target = rect(-1900, 12, 641, 481);

        tracks.insert(A, "a", 1, rect(0, 0, 800, 600), target, quick(), now);

        let mut last = None;
        let mut at = now;
        for _ in 0..600 {
            at += FRAME;

            for write in writes(&tracks.tick(at)) {
                tracks.write_done(write.window_id, 1, write.frame, WriteOutcome::Written, at);
                last = Some(write.frame);
            }

            if tracks.wakeup() != Wakeup::Frame {
                break;
            }
        }

        assert_eq!(last, Some(target));
    }

    #[test]
    fn retargeting_the_same_way_does_not_restart_the_animation() {
        let now = Instant::now();
        let mut tracks = tracks(now);
        let target = rect(1000, 0, 800, 600);

        tracks.insert(A, "a", 1, rect(0, 0, 800, 600), target, timed(250), now);
        tracks.tick(now + Duration::from_millis(100));
        let halfway = tracks.current_frame(A).unwrap();

        assert_eq!(
            tracks.retarget(A, target, timed(250), now + Duration::from_millis(100)),
            Retargeted::Unchanged
        );

        tracks.tick(now + Duration::from_millis(125));
        assert!(tracks.current_frame(A).unwrap().left > halfway.left);
    }

    #[test]
    fn retargeting_carries_on_from_where_the_window_is() {
        let now = Instant::now();
        let mut tracks = tracks(now);

        tracks.insert(
            A,
            "a",
            1,
            rect(0, 0, 800, 600),
            rect(1000, 0, 800, 600),
            quick(),
            now,
        );

        for tick in 1..=8 {
            tracks.tick(now + FRAME * tick);
        }

        let before = tracks.current_frame(A).unwrap();
        assert!(before.left > 0);

        let later = now + FRAME * 8;
        assert_eq!(
            tracks.retarget(A, rect(-500, 0, 800, 600), quick(), later),
            Retargeted::Updated
        );
        assert_eq!(tracks.current_frame(A), Some(before));

        tracks.tick(later + FRAME);
        let after = tracks.current_frame(A).unwrap();
        assert!(after.left < before.left);
        assert!(before.left - after.left < before.left + 500);
    }

    #[test]
    fn a_timed_retarget_starts_its_clock_over_from_the_current_frame() {
        let now = Instant::now();
        let mut tracks = tracks(now);

        tracks.insert(
            A,
            "a",
            1,
            rect(0, 0, 800, 600),
            rect(1000, 0, 800, 600),
            timed(200),
            now,
        );

        let later = now + Duration::from_millis(100);
        tracks.tick(later);
        let halfway = tracks.current_frame(A).unwrap();
        assert_eq!(halfway.left, 500);

        tracks.retarget(A, rect(0, 0, 800, 600), timed(200), later);
        assert_eq!(tracks.current_frame(A), Some(halfway));

        // half of the new duration takes it half of the way back
        tracks.tick(later + Duration::from_millis(100));
        assert_eq!(tracks.current_frame(A).unwrap().left, 250);
    }

    #[test]
    fn windows_are_stepped_together_on_the_same_clock() {
        let now = Instant::now();
        let mut tracks = tracks(now);

        for (id, name) in [(A, "a"), (B, "b")] {
            tracks.insert(
                id,
                name,
                1,
                rect(0, 0, 800, 600),
                rect(1000, 0, 800, 600),
                timed(200),
                now,
            );
        }

        tracks.tick(now + Duration::from_millis(100));

        assert_eq!(tracks.current_frame(A), tracks.current_frame(B));
    }

    #[test]
    fn a_finished_window_is_verified_after_a_delay() {
        let now = Instant::now();
        let mut tracks = tracks(now);
        let target = rect(300, 0, 800, 600);

        tracks.insert(A, "a", 1, rect(0, 0, 800, 600), target, quick(), now);
        let done = run_to_target(&mut tracks, A, now);

        assert_eq!(tracks.wakeup(), Wakeup::At(done + FIRST_VERIFY_DELAY));
        assert!(verifies(&tracks.tick(done + FIRST_VERIFY_DELAY - FRAME)).is_empty());

        let due = done + FIRST_VERIFY_DELAY;
        let commits = tracks.tick(due);
        let verification = verifies(&commits);
        assert_eq!(verification.len(), 1);
        assert_eq!(verification[0].target, target);
        assert!(!verification[0].last_attempt);

        // a verification in flight is not issued twice, and nothing is due until it is given up on
        assert!(verifies(&tracks.tick(due + FRAME)).is_empty());
        assert_eq!(tracks.wakeup(), Wakeup::At(due + STALE_AFTER));

        assert_eq!(
            tracks.verify_done(
                A,
                verification[0].generation,
                VerifyOutcome::Settled,
                due + FRAME
            ),
            Some(1)
        );
        assert!(tracks.is_empty());
    }

    #[test]
    fn a_window_carries_on_to_its_follow_up_frame_once_it_has_arrived() {
        let now = Instant::now();
        let mut tracks = tracks(now);
        let slide = rect(-780, 100, 800, 600);
        let park = rect(-799, 1079, 800, 600);

        tracks.insert(A, "a", 1, rect(100, 100, 800, 600), slide, quick(), now);
        tracks.set_after(A, Some(park));

        let at = run_to_target(&mut tracks, A, now) + FIRST_VERIFY_DELAY;
        let verification = verifies(&tracks.tick(at)).remove(0);

        // arriving does not end the animation, it carries on to the follow-up frame
        assert_eq!(
            tracks.verify_done(A, verification.generation, VerifyOutcome::Settled, at),
            None
        );
        assert!(tracks.is_tracked(A));
        assert_eq!(tracks.target(A), Some(park));
        assert_eq!(tracks.wakeup(), Wakeup::Frame);

        // and the very next frame is the whole of it, in one step
        let next = at + FRAME;
        let writes = writes(&tracks.tick(next));
        assert_eq!(writes.len(), 1);
        assert_eq!(writes[0].frame, park);
        assert_eq!(writes[0].position, Some((park.left, park.top)));

        tracks.write_done(A, 1, park, WriteOutcome::Written, next);

        // which is then verified like any other, and this time there is nothing left to follow on to
        let later = next + FIRST_VERIFY_DELAY;
        let verification = verifies(&tracks.tick(later)).remove(0);
        assert_eq!(verification.target, park);
        assert_eq!(
            tracks.verify_done(A, verification.generation, VerifyOutcome::Settled, later),
            Some(1)
        );
        assert!(tracks.is_empty());
    }

    #[test]
    fn retargeting_forgets_the_follow_up_frame() {
        let now = Instant::now();
        let mut tracks = tracks(now);

        tracks.insert(
            A,
            "a",
            1,
            rect(100, 100, 800, 600),
            rect(-780, 100, 800, 600),
            quick(),
            now,
        );
        tracks.set_after(A, Some(rect(-799, 1079, 800, 600)));

        // the window is wanted back in view before it has finished leaving
        tracks.retarget(A, rect(300, 100, 800, 600), quick(), now + FRAME);

        let at = run_to_target(&mut tracks, A, now + FRAME) + FIRST_VERIFY_DELAY;
        let verification = verifies(&tracks.tick(at)).remove(0);

        assert_eq!(
            tracks.verify_done(A, verification.generation, VerifyOutcome::Settled, at),
            Some(1)
        );
        assert!(tracks.is_empty());
    }

    #[test]
    fn a_window_stopped_before_its_follow_up_is_reported_once() {
        let now = Instant::now();
        let mut tracks = tracks(now);

        tracks.insert(
            A,
            "a",
            1,
            rect(100, 100, 800, 600),
            rect(-780, 100, 800, 600),
            quick(),
            now,
        );
        tracks.set_after(A, Some(rect(-799, 1079, 800, 600)));
        tracks.tick(now + FRAME);

        // somebody grabs it, or it stops answering, before it has got out of the way
        tracks.remove(A);

        assert!(tracks.take_unfinished(A));
        assert!(!tracks.take_unfinished(A));
    }

    #[test]
    fn any_journey_that_is_cut_short_is_reported_once_whether_or_not_it_had_somewhere_to_go_next() {
        let now = Instant::now();
        let mut tracks = tracks(now);

        // a slide out of view, with nowhere to go afterwards
        tracks.insert(
            A,
            "a",
            1,
            rect(100, 100, 800, 600),
            rect(-780, 100, 800, 600),
            quick(),
            now,
        );
        tracks.tick(now + FRAME);
        tracks.remove(A);

        assert!(tracks.take_unfinished(A));
        assert!(!tracks.take_unfinished(A));
    }

    #[test]
    fn a_journey_cut_short_while_it_was_being_verified_is_reported() {
        let now = Instant::now();
        let mut tracks = tracks(now);

        tracks.insert(
            A,
            "a",
            1,
            rect(0, 0, 800, 600),
            rect(300, 0, 800, 600),
            quick(),
            now,
        );
        let at = run_to_target(&mut tracks, A, now) + FIRST_VERIFY_DELAY;
        assert_eq!(verifies(&tracks.tick(at)).len(), 1);

        // it got there, but has not been looked at yet when somebody grabs it
        tracks.remove(A);

        assert!(tracks.take_unfinished(A));
    }

    #[test]
    fn a_journey_that_ends_with_the_window_where_it_was_sent_is_not_cut_short() {
        let now = Instant::now();
        let mut tracks = tracks(now);

        tracks.insert(
            A,
            "a",
            1,
            rect(0, 0, 800, 600),
            rect(300, 0, 800, 600),
            quick(),
            now,
        );
        let at = run_to_target(&mut tracks, A, now) + FIRST_VERIFY_DELAY;
        let verification = verifies(&tracks.tick(at)).remove(0);
        tracks.verify_done(A, verification.generation, VerifyOutcome::Settled, at);

        assert!(tracks.is_empty());
        assert!(!tracks.take_unfinished(A));
    }

    #[test]
    fn a_journey_that_never_gets_where_it_was_sent_is_cut_short() {
        let now = Instant::now();
        let mut tracks = tracks(now);

        tracks.insert(
            A,
            "a",
            1,
            rect(0, 0, 800, 600),
            rect(300, 0, 800, 600),
            quick(),
            now,
        );
        let mut at = run_to_target(&mut tracks, A, now);

        // the app keeps leaving the window where it likes
        while !tracks.is_empty() {
            at += Duration::from_millis(120);
            for verification in verifies(&tracks.tick(at)) {
                tracks.verify_done(A, verification.generation, VerifyOutcome::Reissued, at);
            }
        }

        assert!(tracks.take_unfinished(A));
    }

    #[test]
    fn a_window_that_stops_answering_has_its_journey_cut_short() {
        let now = Instant::now();
        let mut tracks = tracks(now);

        tracks.insert(
            A,
            "a",
            1,
            rect(0, 0, 800, 600),
            rect(1000, 0, 800, 600),
            quick(),
            now,
        );
        let write = writes(&tracks.tick(now + FRAME)).remove(0);
        tracks.write_done(
            A,
            1,
            write.frame,
            WriteOutcome::Failed { gone: true },
            now + FRAME,
        );

        assert!(tracks.take_unfinished(A));
    }

    #[test]
    fn nobody_has_to_ask_for_the_news_to_be_kept_within_bounds() {
        let now = Instant::now();
        let mut tracks = tracks(now);

        for window_id in 0..(MAX_UNFINISHED as u32 * 3) {
            tracks.insert(
                window_id,
                "a",
                1,
                rect(0, 0, 800, 600),
                rect(300, 0, 800, 600),
                quick(),
                now,
            );
            tracks.remove(window_id);
        }

        assert!(tracks.unfinished.len() <= MAX_UNFINISHED);
    }

    #[test]
    fn a_window_that_finished_its_journey_is_not_reported() {
        let now = Instant::now();
        let mut tracks = tracks(now);

        tracks.insert(
            A,
            "a",
            1,
            rect(100, 100, 800, 600),
            rect(-780, 100, 800, 600),
            quick(),
            now,
        );
        tracks.set_after(A, Some(rect(-799, 1079, 800, 600)));

        let at = run_to_target(&mut tracks, A, now) + FIRST_VERIFY_DELAY;
        let verification = verifies(&tracks.tick(at)).remove(0);
        tracks.verify_done(A, verification.generation, VerifyOutcome::Settled, at);

        let next = at + FRAME;
        for write in writes(&tracks.tick(next)) {
            tracks.write_done(A, 1, write.frame, WriteOutcome::Written, next);
        }
        let later = next + FIRST_VERIFY_DELAY;
        let verification = verifies(&tracks.tick(later)).remove(0);
        tracks.verify_done(A, verification.generation, VerifyOutcome::Settled, later);

        assert!(tracks.is_empty());
        assert!(!tracks.take_unfinished(A));
    }

    #[test]
    fn a_skipped_write_says_nothing_about_where_the_window_is() {
        let now = Instant::now();
        let mut tracks = tracks(now);

        tracks.insert(
            A,
            "a",
            1,
            rect(0, 0, 800, 600),
            rect(1000, 0, 800, 600),
            timed(400),
            now,
        );
        let stale = writes(&tracks.tick(now + Duration::from_millis(100))).remove(0);

        // the window was let go of and picked up again somewhere else before the write was made
        tracks.remove(A);
        tracks.insert(
            A,
            "a",
            1,
            rect(500, 0, 800, 600),
            rect(900, 0, 800, 600),
            timed(400),
            now + Duration::from_millis(110),
        );
        tracks.write_done(
            A,
            1,
            stale.frame,
            WriteOutcome::Skipped,
            now + Duration::from_millis(120),
        );

        // so a later failure does not send it back to the frame that was never written
        let write = writes(&tracks.tick(now + Duration::from_millis(200))).remove(0);
        tracks.write_done(
            A,
            1,
            write.frame,
            WriteOutcome::Failed { gone: false },
            now + Duration::from_millis(201),
        );
        let retry = writes(&tracks.tick(now + Duration::from_millis(220))).remove(0);
        assert!(retry.frame.left >= 500);
    }

    #[test]
    fn recently_animated_apps_are_remembered_for_a_while() {
        let now = Instant::now();
        let mut tracks = tracks(now);

        assert!(!tracks.was_animating_recently(7, now));

        tracks.insert(
            A,
            "a",
            7,
            rect(0, 0, 800, 600),
            rect(1000, 0, 800, 600),
            quick(),
            now,
        );
        assert!(tracks.was_animating_recently(7, now));
        assert!(!tracks.was_animating_recently(8, now));

        let write = writes(&tracks.tick(now + FRAME)).remove(0);
        tracks.write_done(A, 1, write.frame, WriteOutcome::Written, now + FRAME);
        tracks.remove(A);

        assert!(tracks.was_animating_recently(7, now + PROCESS_MEMORY / 2));
        assert!(!tracks.was_animating_recently(7, now + PROCESS_MEMORY * 2));
    }

    #[test]
    fn the_generation_of_a_window_follows_its_retargets() {
        let now = Instant::now();
        let mut tracks = tracks(now);

        assert_eq!(tracks.generation(A), None);

        tracks.insert(
            A,
            "a",
            1,
            rect(0, 0, 800, 600),
            rect(1000, 0, 800, 600),
            quick(),
            now,
        );
        let first = tracks.generation(A).unwrap();

        tracks.retarget(A, rect(5, 0, 800, 600), quick(), now + FRAME);
        assert_ne!(tracks.generation(A), Some(first));
    }

    #[test]
    fn only_the_origin_decides_whether_a_notification_is_ours() {
        let now = Instant::now();
        let mut tracks = tracks(now);

        tracks.insert(
            A,
            "a",
            1,
            rect(0, 0, 800, 600),
            rect(1000, 0, 800, 600),
            quick(),
            now,
        );
        let write = writes(&tracks.tick(now + FRAME)).remove(0);
        let context = tracks.notification_context(A, 1, now + FRAME).unwrap();

        // a terminal that snaps to its character grid, or an app with a minimum size, ends up a
        // long way from the size it was asked for, but exactly where it was asked to be
        assert_eq!(
            decide(
                Some(&context),
                true,
                Some(rect(write.frame.left, write.frame.top, 640, 380))
            ),
            Verdict::Suppress
        );
        assert_eq!(
            decide(
                Some(&context),
                true,
                Some(rect(
                    write.frame.left + ECHO_TOLERANCE as i32,
                    write.frame.top,
                    800,
                    600
                ))
            ),
            Verdict::Suppress
        );
        assert_eq!(
            decide(
                Some(&context),
                true,
                Some(rect(
                    write.frame.left + ECHO_TOLERANCE as i32 + 1,
                    write.frame.top + 300,
                    800,
                    600
                ))
            ),
            Verdict::ForwardAndCancel
        );
    }

    #[test]
    fn a_window_that_has_been_animated_is_known_to_be_where_it_was_put() {
        let now = Instant::now();
        let mut tracks = tracks(now);
        let target = rect(300, 0, 800, 600);

        assert_eq!(tracks.known_frame(A, now), None);

        tracks.insert(A, "a", 1, rect(0, 0, 800, 600), target, quick(), now);

        // while it moves, it is wherever the animation has got to
        let write = writes(&tracks.tick(now + FRAME)).remove(0);
        assert_eq!(tracks.known_frame(A, now + FRAME), Some(write.frame));
        tracks.write_done(A, 1, write.frame, WriteOutcome::Written, now + FRAME);

        let at = run_to_target(&mut tracks, A, now + FRAME) + FIRST_VERIFY_DELAY;
        let verification = verifies(&tracks.tick(at)).remove(0);
        tracks.verify_done(A, verification.generation, VerifyOutcome::Settled, at);

        // and once it has stopped, it is where it was sent
        assert!(tracks.is_empty());
        assert_eq!(tracks.known_frame(A, at), Some(target));
        assert_eq!(tracks.known_frame(A, at + KNOWN_FRAME_TTL * 2), None);
    }

    #[test]
    fn what_is_known_about_a_window_can_be_taken_back() {
        let now = Instant::now();
        let mut tracks = tracks(now);

        tracks.insert(
            A,
            "a",
            1,
            rect(0, 0, 800, 600),
            rect(300, 0, 800, 600),
            quick(),
            now,
        );
        let at = run_to_target(&mut tracks, A, now) + FIRST_VERIFY_DELAY;
        let verification = verifies(&tracks.tick(at)).remove(0);
        tracks.verify_done(A, verification.generation, VerifyOutcome::Settled, at);
        assert!(tracks.known_frame(A, at).is_some());

        // somebody moved it
        tracks.forget(A);
        assert_eq!(tracks.known_frame(A, at), None);
    }

    #[test]
    fn a_window_that_was_stopped_is_not_known_to_be_anywhere() {
        let now = Instant::now();
        let mut tracks = tracks(now);

        tracks.insert(
            A,
            "a",
            1,
            rect(0, 0, 800, 600),
            rect(1000, 0, 800, 600),
            quick(),
            now,
        );
        let write = writes(&tracks.tick(now + FRAME)).remove(0);
        tracks.write_done(A, 1, write.frame, WriteOutcome::Written, now + FRAME);

        // the writes that were still on their way might have landed
        tracks.remove(A);

        assert_eq!(tracks.known_frame(A, now + FRAME), None);
    }

    #[test]
    fn a_frame_is_not_worth_starting_while_moving_windows_are_still_being_written_to() {
        let now = Instant::now();
        let mut tracks = tracks(now);

        assert!(!tracks.moving_windows_are_waiting());

        for (id, name) in [(A, "a"), (B, "b")] {
            tracks.insert(
                id,
                name,
                1,
                rect(0, 0, 800, 600),
                rect(1000, 0, 800, 600),
                quick(),
                now,
            );
        }

        let commits = writes(&tracks.tick(now + FRAME));
        assert_eq!(commits.len(), 2);
        assert!(tracks.moving_windows_are_waiting());

        // one app answers and the other does not
        tracks.write_done(A, 1, commits[0].frame, WriteOutcome::Written, now + FRAME);
        assert!(tracks.moving_windows_are_waiting());

        tracks.write_done(B, 1, commits[1].frame, WriteOutcome::Written, now + FRAME);
        assert!(!tracks.moving_windows_are_waiting());
    }

    #[test]
    fn a_window_that_is_only_being_verified_does_not_hold_up_the_others() {
        let now = Instant::now();
        let mut tracks = tracks(now);

        tracks.insert(
            A,
            "a",
            1,
            rect(0, 0, 800, 600),
            rect(300, 0, 800, 600),
            quick(),
            now,
        );
        let at = run_to_target(&mut tracks, A, now) + FIRST_VERIFY_DELAY;
        assert_eq!(verifies(&tracks.tick(at)).len(), 1);

        // A is being verified, and nothing that is moving is waiting for anything
        assert!(!tracks.moving_windows_are_waiting());
    }

    /// Run a swap of tiles of different sizes, reporting every write as done, and return every
    /// write that was made
    fn swap_writes(size_cost: Option<Duration>) -> Vec<Write<&'static str>> {
        let now = Instant::now();
        let mut tracks = tracks(now);

        if let Some(cost) = size_cost {
            tracks.note_size_cost(1, cost);
        }

        // a tile that is large and goes to a small slot, which is also to the right
        tracks.insert(
            A,
            "a",
            1,
            rect(0, 0, 1800, 1500),
            rect(1900, 0, 900, 700),
            quick(),
            now,
        );

        let mut all = Vec::new();
        let mut at = now;
        for _ in 0..600 {
            at += FRAME;

            for write in writes(&tracks.tick(at)) {
                tracks.write_done(A, 1, write.frame, WriteOutcome::Written, at);
                all.push(write);
            }

            if tracks.wakeup() != Wakeup::Frame {
                break;
            }
        }

        all
    }

    fn size_writes(writes: &[Write<&'static str>]) -> usize {
        writes.iter().filter(|write| write.size.is_some()).count()
    }

    #[test]
    fn the_size_of_a_window_changes_along_with_its_position() {
        let writes = swap_writes(None);

        assert!(size_writes(&writes) > 5);
        assert_eq!(writes.last().unwrap().frame, rect(1900, 0, 900, 700));
    }

    #[test]
    fn an_app_that_is_slow_to_apply_a_size_is_sent_them_less_often_and_never_stops_moving() {
        let quick = swap_writes(None);
        let slow = swap_writes(Some(Duration::from_millis(60)));

        // far fewer sizes, but as many moves as an app that keeps up
        assert!(
            size_writes(&slow) * 3 < size_writes(&quick),
            "{} vs {}",
            size_writes(&slow),
            size_writes(&quick)
        );
        assert!(slow.iter().filter(|write| write.position.is_some()).count() + 2 >= quick.len());

        // and it ends up the size it was meant to be, whatever it was held back from
        let last = slow.last().unwrap();
        assert_eq!(last.frame, rect(1900, 0, 900, 700));
        assert_eq!(last.size.unwrap_or((900, 700)), (900, 700));
    }

    #[test]
    fn what_is_learnt_about_an_app_is_smoothed() {
        let now = Instant::now();
        let mut tracks = tracks(now);

        tracks.note_size_cost(1, Duration::from_millis(100));
        tracks.note_size_cost(1, Duration::from_millis(10));

        // one quick answer does not make a slow app quick
        let gap = size_gap(&tracks.size_cost, 1);
        assert!(gap > Duration::from_millis(100), "{gap:?}");
        assert_eq!(size_gap(&tracks.size_cost, 2), Duration::ZERO);

        // and nobody is left longer than this
        tracks.note_size_cost(3, Duration::from_secs(10));
        assert_eq!(size_gap(&tracks.size_cost, 3), LONGEST_SIZE_GAP);
    }

    #[test]
    fn an_app_nothing_is_known_about_is_sent_sizes_every_frame() {
        let now = Instant::now();
        let mut tracks = tracks(now);

        tracks.insert(
            A,
            "a",
            1,
            rect(0, 0, 1800, 1500),
            rect(1900, 0, 900, 700),
            timed(1000),
            now,
        );

        for tick in 1..=5 {
            let writes = writes(&tracks.tick(now + FRAME * tick));
            assert!(writes[0].size.is_some(), "frame {tick}");

            tracks.write_done(
                A,
                1,
                writes[0].frame,
                WriteOutcome::Written,
                now + FRAME * tick,
            );
        }
    }

    #[test]
    fn a_window_that_keeps_missing_its_target_is_given_up_on() {
        let now = Instant::now();
        let mut tracks = tracks(now);

        tracks.insert(
            A,
            "a",
            1,
            rect(0, 0, 800, 600),
            rect(300, 0, 800, 600),
            quick(),
            now,
        );
        let mut at = run_to_target(&mut tracks, A, now);

        let mut attempts = 0;
        while !tracks.is_empty() {
            at += Duration::from_millis(120);
            for verification in verifies(&tracks.tick(at)) {
                attempts += 1;
                tracks.verify_done(A, verification.generation, VerifyOutcome::Reissued, at);
            }

            assert!(attempts <= VERIFY_ATTEMPTS, "kept verifying forever");
        }

        assert_eq!(attempts, VERIFY_ATTEMPTS);
    }

    #[test]
    fn the_last_verification_says_so() {
        let now = Instant::now();
        let mut tracks = tracks(now);

        tracks.insert(
            A,
            "a",
            1,
            rect(0, 0, 800, 600),
            rect(300, 0, 800, 600),
            quick(),
            now,
        );
        let mut at = run_to_target(&mut tracks, A, now);

        let mut flags = Vec::new();
        while !tracks.is_empty() {
            at += Duration::from_millis(120);
            for verification in verifies(&tracks.tick(at)) {
                flags.push(verification.last_attempt);
                tracks.verify_done(A, verification.generation, VerifyOutcome::Reissued, at);
            }
        }

        assert_eq!(flags, vec![false, false, true]);
    }

    #[test]
    fn a_retarget_during_verification_discards_its_result() {
        let now = Instant::now();
        let mut tracks = tracks(now);

        tracks.insert(
            A,
            "a",
            1,
            rect(0, 0, 800, 600),
            rect(300, 0, 800, 600),
            quick(),
            now,
        );
        let at = run_to_target(&mut tracks, A, now) + FIRST_VERIFY_DELAY;
        let verification = verifies(&tracks.tick(at)).remove(0);

        tracks.retarget(A, rect(900, 0, 800, 600), quick(), at);

        assert_eq!(
            tracks.verify_done(A, verification.generation, VerifyOutcome::Settled, at),
            None
        );
        assert!(tracks.is_tracked(A));
        assert_eq!(tracks.wakeup(), Wakeup::Frame);
    }

    #[test]
    fn verification_waits_for_outstanding_writes() {
        let now = Instant::now();
        let mut tracks = tracks(now);

        tracks.insert(
            A,
            "a",
            1,
            rect(0, 0, 800, 600),
            rect(300, 0, 800, 600),
            quick(),
            now,
        );

        // never report the writes as done
        let mut at = now;
        let mut outstanding = 0;
        for _ in 0..600 {
            at += FRAME;
            outstanding += writes(&tracks.tick(at)).len() as u32;
            if tracks.wakeup() != Wakeup::Frame {
                break;
            }
        }

        assert_eq!(tracks.in_flight(A), outstanding);
        assert!(verifies(&tracks.tick(at + Duration::from_secs(1))).is_empty());
    }

    #[test]
    fn an_overdue_verification_with_outstanding_writes_does_not_keep_the_animator_awake() {
        let now = Instant::now();
        let mut tracks = tracks(now);

        tracks.insert(
            A,
            "a",
            1,
            rect(0, 0, 800, 600),
            rect(300, 0, 800, 600),
            quick(),
            now,
        );

        // never report the writes as done
        let mut at = now;
        loop {
            at += FRAME;
            tracks.tick(at);

            if tracks.wakeup() != Wakeup::Frame {
                break;
            }
        }

        assert!(tracks.in_flight(A) > 0);

        // the only time worth waking up for is the one at which the writes are given up on
        match tracks.wakeup() {
            Wakeup::At(look_again) => assert!(look_again > at),
            other => panic!("expected a time in the future, got {other:?}"),
        }
    }

    #[test]
    fn writes_that_are_never_reported_on_do_not_hold_a_window_forever() {
        let now = Instant::now();
        let mut tracks = tracks(now);

        tracks.insert(
            A,
            "a",
            1,
            rect(0, 0, 800, 600),
            rect(300, 0, 800, 600),
            quick(),
            now,
        );

        // the lane that was handed these never reports back
        let mut at = now;
        loop {
            at += FRAME;
            tracks.tick(at);

            if tracks.wakeup() != Wakeup::Frame {
                break;
            }
        }
        assert!(tracks.in_flight(A) > 0);

        // there is a time to wake up and look again, which is not now
        let Wakeup::At(look_again) = tracks.wakeup() else {
            panic!("expected to be woken to look again");
        };
        assert!(look_again > at);
        assert!(verifies(&tracks.tick(look_again - FRAME)).is_empty());

        // by then the writes are forgotten, and the window is verified like any other
        let commits = tracks.tick(look_again);
        assert_eq!(verifies(&commits).len(), 1);
        assert_eq!(tracks.in_flight(A), 1);
    }

    #[test]
    fn a_verification_that_is_never_reported_on_does_not_hold_a_window_forever() {
        let now = Instant::now();
        let mut tracks = tracks(now);

        tracks.insert(
            A,
            "a",
            1,
            rect(0, 0, 800, 600),
            rect(300, 0, 800, 600),
            quick(),
            now,
        );
        let at = run_to_target(&mut tracks, A, now) + FIRST_VERIFY_DELAY;
        assert_eq!(verifies(&tracks.tick(at)).len(), 1);

        // the verification is out there and never comes back
        let Wakeup::At(look_again) = tracks.wakeup() else {
            panic!("expected to be woken to look again");
        };
        assert_eq!(look_again, at + STALE_AFTER);

        tracks.tick(look_again);
        assert!(tracks.is_empty());
        assert_eq!(tracks.total_in_flight(), 0);
    }

    #[test]
    fn coalesced_writes_are_all_accounted_for() {
        let now = Instant::now();
        let mut tracks = tracks(now);

        tracks.insert(
            A,
            "a",
            1,
            rect(0, 0, 800, 600),
            rect(1000, 0, 800, 600),
            quick(),
            now,
        );

        let mut last = None;
        for tick in 1..=4 {
            for write in writes(&tracks.tick(now + FRAME * tick)) {
                last = Some(write.frame);
            }
        }

        assert_eq!(tracks.in_flight(A), 4);

        tracks.write_done(A, 4, last.unwrap(), WriteOutcome::Written, now + FRAME * 5);
        assert_eq!(tracks.in_flight(A), 0);
        assert_eq!(tracks.total_in_flight(), 0);
    }

    #[test]
    fn a_busy_window_is_retried_with_the_next_frame() {
        let now = Instant::now();
        let mut tracks = tracks(now);

        tracks.insert(
            A,
            "a",
            1,
            rect(0, 0, 800, 600),
            rect(1000, 0, 800, 600),
            timed(400),
            now,
        );

        let first = writes(&tracks.tick(now + Duration::from_millis(100))).remove(0);
        assert_eq!(
            tracks.write_done(
                A,
                1,
                first.frame,
                WriteOutcome::Failed { gone: false },
                now + Duration::from_millis(101)
            ),
            None
        );

        // the failed write is not treated as having happened, so the next frame writes it again
        let second = writes(&tracks.tick(now + Duration::from_millis(120)));
        assert_eq!(second.len(), 1);
        assert!(second[0].position.is_some());
        assert!(tracks.is_tracked(A));
    }

    #[test]
    fn a_window_that_never_accepts_a_write_is_given_up_on() {
        let now = Instant::now();
        let mut tracks = tracks(now);

        tracks.insert(
            A,
            "a",
            7,
            rect(0, 0, 800, 600),
            rect(1000, 0, 800, 600),
            timed(10_000),
            now,
        );

        let mut released = None;
        for tick in 1..=(MAX_WRITE_FAILURES as u32 + 1) {
            let at = now + Duration::from_millis(500) * tick;
            for write in writes(&tracks.tick(at)) {
                released =
                    tracks.write_done(A, 1, write.frame, WriteOutcome::Failed { gone: false }, at);
            }
        }

        assert_eq!(released, Some(7));
        assert!(tracks.is_empty());
    }

    #[test]
    fn a_window_that_is_gone_is_dropped_at_once() {
        let now = Instant::now();
        let mut tracks = tracks(now);

        tracks.insert(
            A,
            "a",
            7,
            rect(0, 0, 800, 600),
            rect(1000, 0, 800, 600),
            quick(),
            now,
        );

        let write = writes(&tracks.tick(now + FRAME)).remove(0);

        assert_eq!(
            tracks.write_done(
                A,
                1,
                write.frame,
                WriteOutcome::Failed { gone: true },
                now + FRAME
            ),
            Some(7)
        );
        assert!(tracks.is_empty());
    }

    #[test]
    fn removing_a_window_stops_it_being_written() {
        let now = Instant::now();
        let mut tracks = tracks(now);

        tracks.insert(
            A,
            "a",
            3,
            rect(0, 0, 800, 600),
            rect(1000, 0, 800, 600),
            quick(),
            now,
        );
        tracks.tick(now + FRAME);

        assert_eq!(tracks.remove(A), Some(3));
        assert_eq!(tracks.remove(A), None);
        assert!(tracks.tick(now + FRAME * 2).is_empty());
        assert_eq!(tracks.wakeup(), Wakeup::Idle);
    }

    #[test]
    fn draining_stops_everything() {
        let now = Instant::now();
        let mut tracks = tracks(now);

        for (id, pid) in [(A, 1), (B, 2)] {
            tracks.insert(
                id,
                "x",
                pid,
                rect(0, 0, 800, 600),
                rect(1000, 0, 800, 600),
                quick(),
                now,
            );
        }

        let mut drained = tracks.drain();
        drained.sort_unstable();

        assert_eq!(drained, vec![(A, 1), (B, 2)]);
        assert!(tracks.is_empty());
    }

    #[test]
    fn a_stuck_animation_is_ended_by_force() {
        let now = Instant::now();
        let mut tracks = tracks(now);
        let target = rect(1000, 0, 800, 600);

        tracks.insert(
            A,
            "a",
            1,
            rect(0, 0, 800, 600),
            target,
            timed(10_000_000),
            now,
        );

        // far beyond the duration nobody would wait for
        let later = now + Duration::from_secs(20_000);
        let commits = tracks.tick(later);
        let writes = writes(&commits);

        assert_eq!(writes.len(), 1);
        assert_eq!(writes[0].frame, target);

        tracks.write_done(A, 1, target, WriteOutcome::Written, later);
        assert_eq!(tracks.wakeup(), Wakeup::At(later + FIRST_VERIFY_DELAY));
    }

    #[test]
    fn an_absurd_duration_does_not_overflow_the_clock() {
        let now = Instant::now();
        let mut tracks = tracks(now);

        tracks.insert(
            A,
            "a",
            1,
            rect(0, 0, 800, 600),
            rect(1000, 0, 800, 600),
            MotionSpec {
                duration: Duration::from_millis(u64::MAX),
                style: AnimationStyle::Linear,
            },
            now,
        );

        // getting this far means that nothing overflowed; at this speed there is barely any movement
        tracks.tick(now + Duration::from_secs(1));
        assert!(tracks.is_tracked(A));
    }

    #[test]
    fn only_verifying_windows_let_the_animator_sleep_until_they_are_due() {
        let now = Instant::now();
        let mut tracks = tracks(now);

        assert_eq!(tracks.wakeup(), Wakeup::Idle);

        tracks.insert(
            A,
            "a",
            1,
            rect(0, 0, 800, 600),
            rect(300, 0, 800, 600),
            quick(),
            now,
        );
        assert_eq!(tracks.wakeup(), Wakeup::Frame);

        let done = run_to_target(&mut tracks, A, now);
        assert_eq!(tracks.wakeup(), Wakeup::At(done + FIRST_VERIFY_DELAY));
    }

    #[test]
    fn notifications_for_unknown_windows_are_forwarded() {
        let now = Instant::now();
        let mut tracks = tracks(now);

        let context = tracks.notification_context(A, 1, now);

        assert_eq!(context, None);
        assert_eq!(decide(context.as_ref(), false, None), Verdict::Forward);
        assert_eq!(decide(context.as_ref(), true, None), Verdict::Forward);
    }

    #[test]
    fn echoes_of_our_own_writes_are_suppressed_while_animating() {
        let now = Instant::now();
        let mut tracks = tracks(now);

        tracks.insert(
            A,
            "a",
            1,
            rect(0, 0, 800, 600),
            rect(1000, 0, 800, 600),
            quick(),
            now,
        );
        let write = writes(&tracks.tick(now + FRAME)).remove(0);

        let context = tracks.notification_context(A, 1, now + FRAME).unwrap();
        assert!(context.animating);

        // the mouse is up, so nothing is dragging it
        assert_eq!(decide(Some(&context), false, None), Verdict::Suppress);

        // the mouse is down and the window is where we put it: still our own echo
        assert_eq!(
            decide(Some(&context), true, Some(write.frame)),
            Verdict::Suppress
        );
        assert_eq!(
            decide(
                Some(&context),
                true,
                Some(rect(write.frame.left + 1, write.frame.top - 1, 800, 600))
            ),
            Verdict::Suppress
        );
    }

    #[test]
    fn a_window_grabbed_mid_animation_is_let_go() {
        let now = Instant::now();
        let mut tracks = tracks(now);

        tracks.insert(
            A,
            "a",
            1,
            rect(0, 0, 800, 600),
            rect(1000, 0, 800, 600),
            quick(),
            now,
        );
        tracks.tick(now + FRAME);

        let context = tracks.notification_context(A, 1, now + FRAME).unwrap();

        assert_eq!(
            decide(Some(&context), true, Some(rect(640, 300, 800, 600))),
            Verdict::ForwardAndCancel
        );
        assert_eq!(
            decide(Some(&context), true, None),
            Verdict::ForwardAndCancel
        );
    }

    #[test]
    fn late_echoes_are_suppressed_within_the_grace_period_only() {
        let now = Instant::now();
        let mut tracks = tracks(now);

        tracks.insert(
            A,
            "a",
            1,
            rect(0, 0, 800, 600),
            rect(300, 0, 800, 600),
            quick(),
            now,
        );
        let done = run_to_target(&mut tracks, A, now);

        // still verifying: tracked, so still animating
        assert!(tracks.notification_context(A, 1, done).unwrap().animating);

        tracks.remove(A);

        let context = tracks
            .notification_context(A, 1, done + NOTIFICATION_GRACE / 2)
            .unwrap();
        assert!(!context.animating);
        assert_eq!(decide(Some(&context), false, None), Verdict::Suppress);

        assert_eq!(
            tracks.notification_context(A, 1, done + NOTIFICATION_GRACE * 2),
            None
        );
    }

    #[test]
    fn a_notification_for_another_window_of_an_app_being_moved_is_an_echo() {
        let now = Instant::now();
        let mut tracks = tracks(now);
        let target = rect(1000, 0, 800, 600);

        // a native tab group: the tab we moved is not the one that reports
        tracks.insert(A, "a", 7, rect(0, 0, 800, 600), target, quick(), now);
        let write = writes(&tracks.tick(now + FRAME)).remove(0);

        let context = tracks.notification_context(B, 7, now + FRAME).unwrap();
        assert!(context.animating);
        assert_eq!(decide(Some(&context), false, None), Verdict::Suppress);
        assert_eq!(
            decide(Some(&context), true, Some(write.frame)),
            Verdict::Suppress
        );
        assert_eq!(
            decide(Some(&context), true, Some(rect(5, 5, 300, 200))),
            Verdict::ForwardAndCancel
        );

        // an app we are not moving is none of our business
        assert_eq!(tracks.notification_context(B, 8, now + FRAME), None);
    }

    #[test]
    fn the_echoes_of_a_finished_app_are_still_recognised_for_a_while() {
        let now = Instant::now();
        let mut tracks = tracks(now);

        tracks.insert(
            A,
            "a",
            7,
            rect(0, 0, 800, 600),
            rect(300, 0, 800, 600),
            quick(),
            now,
        );
        let done = run_to_target(&mut tracks, A, now);
        tracks.remove(A);

        let context = tracks
            .notification_context(B, 7, done + NOTIFICATION_GRACE / 2)
            .unwrap();
        assert!(!context.animating);

        assert_eq!(
            tracks.notification_context(B, 7, done + NOTIFICATION_GRACE * 2),
            None
        );
    }

    #[test]
    fn only_the_most_recent_writes_are_remembered() {
        let now = Instant::now();
        let mut tracks = tracks(now);

        tracks.insert(
            A,
            "a",
            1,
            rect(0, 0, 800, 600),
            rect(5000, 0, 800, 600),
            timed(100_000),
            now,
        );

        for tick in 1..=40 {
            tracks.tick(now + Duration::from_secs(tick));
        }

        let context = tracks
            .notification_context(A, 1, now + Duration::from_secs(40))
            .unwrap();

        // what was written to the window and to its app, and the target
        assert_eq!(context.written.len(), RING * 2 + 1);
        assert_eq!(*context.written.last().unwrap(), rect(5000, 0, 800, 600));
    }

    #[test]
    fn forgotten_windows_are_pruned() {
        let now = Instant::now();
        let mut tracks = tracks(now);

        tracks.insert(
            A,
            "a",
            1,
            rect(0, 0, 800, 600),
            rect(1000, 0, 800, 600),
            quick(),
            now,
        );
        tracks.tick(now + FRAME);
        tracks.remove(A);

        assert!(tracks.recent.contains_key(&A));

        // any later tick prunes what has not been written to for a while
        tracks.tick(now + RECENT_TTL * 3);
        assert!(!tracks.recent.contains_key(&A));
    }
}
