//! The thread that moves animated windows
//!
//! Every animated window used to get a thread of its own that slept between frames, so windows
//! moved out of step with each other, and changing a window's destination meant waiting for its
//! thread to notice and stop. Here there is one clock for all of them:
//!
//! - A *frame thread* steps every animated window together, using the same `now`, and works out
//!   what has to be written to each. It never talks to an app, so it never has to wait for one.
//! - A *lane* per app does the talking. Every AX call is a synchronous round trip to the app that
//!   owns the window, so the lanes make them side by side rather than one after another. The
//!   frame thread does not start a frame until the last has been applied to every moving window:
//!   neighbouring windows have to be on the same frame or the gaps between them open and close.
//!   An app that takes longer than a few frames is not waited for, and its lane drops the frames
//!   it could not keep up with.
//!
//! Nothing in here holds the window manager's lock, and the only lock it holds itself is released
//! before any AX call is made.

use super::stats;
use super::step::MotionSpec;
use super::throttle::Kind;
use super::throttle::Throttle;
use super::tracks::Commit;
use super::tracks::NOTIFICATION_GRACE;
use super::tracks::Retargeted;
use super::tracks::Tracks;
use super::tracks::Verdict;
use super::tracks::Verify;
use super::tracks::VerifyOutcome;
use super::tracks::Wakeup;
use super::tracks::WindowId;
use super::tracks::Write;
use super::tracks::WriteOutcome;
use super::tracks::decide;
use crate::AccessibilityUiElement;
use crate::accessibility::AccessibilityApi;
use crate::accessibility::attribute_constants::kAXPositionAttribute;
use crate::accessibility::attribute_constants::kAXSizeAttribute;
use crate::accessibility::enhanced_ui;
use crate::accessibility::enhanced_ui::EnhancedUiGuard;
use crate::accessibility::error::AccessibilityError;
use crate::arrival;
use crate::border_manager;
use crate::core::Rect;
use crate::macos_api::MacosApi;
use crate::reaper;
use crossbeam_channel::Receiver;
use crossbeam_channel::RecvTimeoutError;
use crossbeam_channel::SendError;
use crossbeam_channel::Sender;
use objc2::rc::autoreleasepool;
use objc2_application_services::AXValueType;
use objc2_core_foundation::CGFloat;
use objc2_core_foundation::CGPoint;
use objc2_core_foundation::CGSize;
use parking_lot::Condvar;
use parking_lot::Mutex;
use parking_lot::MutexGuard;
use std::collections::HashMap;
use std::collections::HashSet;
use std::sync::LazyLock;
use std::sync::OnceLock;
use std::time::Duration;
use std::time::Instant;

/// How long an app has to answer before we stop waiting for it
const MESSAGING_TIMEOUT: f32 = 1.0;

/// How long a lane waits for work before it exits
const LANE_IDLE: Duration = Duration::from_secs(5);

/// How long to wait for the writes already on their way to a window before writing to it directly
const TAKE_OVER_PATIENCE: Duration = Duration::from_millis(250);

/// How long to wait for the writes on their way when the daemon is shutting down
const SHUTDOWN_PATIENCE: Duration = Duration::from_millis(500);

/// How long asking a window where it is has to take to be worth a mention in the log
const SLOW_READ: Duration = Duration::from_millis(15);

/// How many frames' time the frame thread waits for the last frame to be applied to every window
/// before it carries on without the slowest app
const BARRIER_PATIENCE_FRAMES: u32 = 1;

/// A frame is this much of a period late before it counts as having overrun
const OVERRUN_TOLERANCE: f64 = 0.5;

/// Everything the animator needs to talk to a window and the application that owns it
#[derive(Debug, Clone)]
pub struct WindowHandle {
    pub element: AccessibilityUiElement,
    pub app: AccessibilityUiElement,
}

struct State {
    tracks: Tracks<WindowHandle>,
    shutting_down: bool,
}

struct Animator {
    state: Mutex<State>,
    /// Signalled when there may be something for the frame thread to do
    wake: Condvar,
    /// Signalled when the writes on their way to a window have all been accounted for
    settled: Condvar,
}

static ANIMATOR: OnceLock<&'static Animator> = OnceLock::new();

/// The animator, started the first time something is animated
fn animator() -> &'static Animator {
    ANIMATOR.get_or_init(|| {
        let animator: &'static Animator = Box::leak(Box::new(Animator {
            state: Mutex::new(State {
                tracks: Tracks::new(Instant::now()),
                shutting_down: false,
            }),
            wake: Condvar::new(),
            settled: Condvar::new(),
        }));

        std::thread::Builder::new()
            .name(String::from("komorebi-animator"))
            .spawn(move || animator.run())
            .expect("could not start the animator thread");

        animator
    })
}

/// Where a window is to be animated to, and how
#[derive(Debug, Clone)]
pub struct Journey {
    pub window_id: WindowId,
    pub pid: i32,
    pub handle: WindowHandle,
    pub target: Rect,
    pub motion: MotionSpec,
    /// If given, the window is sent there instantly once it has arrived, unless it is retargeted
    /// again first
    pub after: Option<Rect>,
}

/// Animate a window along a journey
///
/// If the window is already being animated it carries on from wherever it has got to, without
/// waiting for anything. Otherwise `read_current` is asked where it is now, which is the one time an
/// animation needs to ask the app anything before it starts.
pub fn retarget(
    journey: &Journey,
    read_current: impl FnOnce() -> Result<Rect, AccessibilityError>,
) -> Result<(), AccessibilityError> {
    let Journey {
        window_id,
        pid,
        handle,
        target,
        motion: spec,
        after,
    } = journey.clone();

    let animator = animator();

    let known = {
        let mut state = animator.state.lock();

        if state.shutting_down {
            return Ok(());
        }

        let now = Instant::now();

        match state.tracks.retarget(window_id, target, spec, now) {
            retargeted @ (Retargeted::Updated | Retargeted::Unchanged) => {
                if after.is_some() {
                    state.tracks.set_after(window_id, after);
                }

                drop(state);

                if retargeted == Retargeted::Updated {
                    animator.wake.notify_all();
                }

                return Ok(());
            }
            Retargeted::NotTracked => state.tracks.known_frame(window_id, now),
        }
    };

    // neither the window nor the app it belongs to is allowed to hold up the animator for long
    let _ = AccessibilityApi::set_messaging_timeout(&handle.element, MESSAGING_TIMEOUT);
    let _ = AccessibilityApi::set_messaging_timeout(&handle.app, MESSAGING_TIMEOUT);

    // asking the app where its window is takes a round trip to it, which is time the windows that
    // come after this one in the layout spend waiting to start moving
    let start = match known {
        Some(start) => start,
        None => {
            let asked = Instant::now();
            let start = read_current()?;

            // the window manager does this while the windows that come after this one wait
            // for their turn to start moving
            if asked.elapsed() >= SLOW_READ {
                tracing::debug!(
                    target: "komorebi::animation",
                    "asking window {window_id} (process {pid}) where it was took {:?}",
                    asked.elapsed()
                );
            }

            start
        }
    };

    // a window that is already there only needs animating if there is somewhere to go next
    if start == target && after.is_none() {
        return Ok(());
    }

    let mut state = animator.state.lock();

    if state.shutting_down {
        return Ok(());
    }

    state.tracks.insert(
        window_id,
        handle.clone(),
        pid,
        start,
        target,
        spec,
        Instant::now(),
    );
    state.tracks.set_after(window_id, after);

    drop(state);
    animator.wake.notify_all();

    Ok(())
}

/// Forget where a window was last put, because something other than the animator has moved it
pub fn forget(window_id: WindowId) {
    if let Some(animator) = ANIMATOR.get() {
        animator.state.lock().tracks.forget(window_id);
    }
}

/// Stop animating a window so that it can be written to directly
///
/// Anything already on its way to the window is allowed to finish first, so that it cannot land on
/// top of what the caller writes next. Returns where the window was heading, if it was animated.
pub fn take_over(window_id: WindowId) -> Option<Rect> {
    let animator = ANIMATOR.get()?;
    let mut state = animator.state.lock();

    let target = remove(animator, &mut state, window_id);

    // the caller is about to move the window itself
    state.tracks.forget(window_id);

    if state.tracks.in_flight(window_id) > 0 {
        let deadline = Instant::now() + TAKE_OVER_PATIENCE;

        while state.tracks.in_flight(window_id) > 0 {
            if animator
                .settled
                .wait_until(&mut state, deadline)
                .timed_out()
            {
                tracing::debug!("gave up waiting for window {window_id}'s animation to settle");
                break;
            }
        }
    }

    target
}

/// Stop animating a window and leave it wherever it has got to
pub fn cancel(window_id: WindowId) {
    let Some(animator) = ANIMATOR.get() else {
        return;
    };

    let mut state = animator.state.lock();
    remove(animator, &mut state, window_id);
    state.tracks.forget(window_id);
}

fn remove(animator: &Animator, state: &mut State, window_id: WindowId) -> Option<Rect> {
    let (pid, target) = state.tracks.remove_track(window_id)?;

    if let Some(lane) = LANES.lock().get(&pid) {
        let _ = lane.send(LaneMessage::Release(window_id));
    }

    animator.wake.notify_all();

    Some(target)
}

/// Whether a window was stopped before it got to where it was going to be sent afterwards
///
/// This is only reported once.
pub fn take_unfinished(window_id: WindowId) -> bool {
    ANIMATOR
        .get()
        .is_some_and(|animator| animator.state.lock().tracks.take_unfinished(window_id))
}

/// Whether a window is being animated right now
pub fn is_animating(window_id: WindowId) -> bool {
    ANIMATOR
        .get()
        .is_some_and(|animator| animator.state.lock().tracks.is_tracked(window_id))
}

/// Where an animated window is heading
pub fn target(window_id: WindowId) -> Option<Rect> {
    ANIMATOR.get()?.state.lock().tracks.target(window_id)
}

/// Where an animated window is right now
pub fn current_frame(window_id: WindowId) -> Option<Rect> {
    ANIMATOR.get()?.state.lock().tracks.current_frame(window_id)
}

/// Decide what to do about a move/resize notification for a window
///
/// Our own writes make apps send notifications, which the window manager must not mistake for
/// somebody moving a window. A window that somebody is dragging is let go of.
///
/// `read_frame` asks the app where the window is. It is only called when the mouse button is down,
/// which is the one time a notification could be a person rather than an echo.
pub fn notification_verdict(
    window_id: WindowId,
    pid: i32,
    mouse_down: bool,
    read_frame: impl FnOnce() -> Option<Rect>,
) -> Verdict {
    let Some(animator) = ANIMATOR.get() else {
        return Verdict::Forward;
    };

    let mut state = animator.state.lock();
    let context = state
        .tracks
        .notification_context(window_id, pid, Instant::now());

    if context.is_none() {
        // somebody other than us has moved it, so we no longer know where it is
        state.tracks.forget(window_id);
        drop(state);

        tracing::trace!(
            target: "komorebi::animation",
            "notification for window {window_id} of process {pid} is not an echo of anything we did"
        );

        return Verdict::Forward;
    }

    drop(state);

    let observed = if mouse_down { read_frame() } else { None };
    let verdict = decide(context.as_ref(), mouse_down, observed);

    if verdict == Verdict::ForwardAndCancel {
        cancel(window_id);
    }

    verdict
}

static THROTTLE: LazyLock<Mutex<Throttle>> = LazyLock::new(|| Mutex::new(Throttle::default()));

/// Whether a move or resize notification may be passed on to the window manager
///
/// The notifications that arrive while the mouse button is up, from an app that has had windows
/// animated lately, are throttled so that an app that cannot stop reporting cannot fill the window
/// manager's event channel, which would cost it real events such as focus changes. Nothing is
/// throttled while the mouse button is down, as that is somebody moving a window, and nothing is
/// throttled for apps that have not been animated.
pub fn admit_notification(window_id: WindowId, pid: i32, resized: bool, mouse_down: bool) -> bool {
    let Some(animator) = ANIMATOR.get() else {
        return true;
    };

    if mouse_down {
        return true;
    }

    // only apps we have been moving are throttled: an app that moves its own windows for its own
    // reasons is reporting something the window manager may want to know about
    if !animator
        .state
        .lock()
        .tracks
        .was_animating_recently(pid, Instant::now())
    {
        return true;
    }

    let kind = if resized { Kind::Resized } else { Kind::Moved };

    let admitted = THROTTLE.lock().admit(window_id, kind, Instant::now());

    if !admitted {
        tracing::trace!(
            target: "komorebi::animation",
            "dropped a {kind:?} notification for window {window_id}: too soon after the last"
        );
    }

    admitted
}

/// How long after a write a notification for it can still arrive
pub const fn notification_grace() -> Duration {
    NOTIFICATION_GRACE
}

/// Stop animating everything, and wait for what is on its way to land
///
/// Called before the daemon puts every window back where it found it, so that the animator cannot
/// move any of them afterwards.
pub fn shutdown() {
    let Some(animator) = ANIMATOR.get() else {
        return;
    };

    let mut state = animator.state.lock();
    state.shutting_down = true;

    for (window_id, pid) in state.tracks.drain() {
        if let Some(lane) = LANES.lock().get(&pid) {
            let _ = lane.send(LaneMessage::Release(window_id));
        }
    }

    animator.wake.notify_all();

    let deadline = Instant::now() + SHUTDOWN_PATIENCE;
    while state.tracks.total_in_flight() > 0 {
        if animator
            .settled
            .wait_until(&mut state, deadline)
            .timed_out()
        {
            break;
        }
    }
}

impl Animator {
    /// The frame thread
    fn run(&'static self) {
        let mut state = self.state.lock();
        let mut next_frame: Option<Instant> = None;
        let mut waiting_since: Option<Instant> = None;

        loop {
            if state.shutting_down {
                return;
            }

            let now = Instant::now();
            let wakeup = state.tracks.wakeup();

            match wakeup {
                Wakeup::Idle => {
                    next_frame = None;
                    waiting_since = None;

                    if let Some(summary) = stats::take_summary(now) {
                        tracing::debug!(
                            target: "komorebi::animation",
                            "animated for {:?}: {} frames ({:.1} fps, {} late) moved {} windows, {} writes ({} failed), writes took p50 {:?} p95 {:?} max {:?}",
                            summary.wall,
                            summary.ticks,
                            summary.effective_fps(),
                            summary.overruns,
                            summary.windows_moved,
                            summary.writes,
                            summary.failed_writes,
                            summary.write_p50,
                            summary.write_p95,
                            summary.write_max,
                        );
                    }

                    self.wake.wait(&mut state);
                    continue;
                }
                Wakeup::At(at) if at > now => {
                    next_frame = None;
                    self.wake.wait_until(&mut state, at);
                    continue;
                }
                Wakeup::At(_) => next_frame = None,
                Wakeup::Frame => {
                    let due = *next_frame.get_or_insert(now);

                    if now < due {
                        self.wake.wait_until(&mut state, due);
                        continue;
                    }

                    // A frame is only worth starting once the last one has been applied to every
                    // window. Otherwise the windows of quick apps would be a frame or more ahead of
                    // the windows of slow ones, and the gaps between neighbouring windows would open
                    // and close. Every window then moves at the pace of the slowest, which looks a
                    // good deal better than each moving at its own, unless an app is so slow that
                    // it is not worth waiting for.
                    if state.tracks.moving_windows_are_waiting() {
                        let patience = *waiting_since.get_or_insert(now)
                            + super::frame_period() * BARRIER_PATIENCE_FRAMES;

                        if now < patience {
                            self.settled.wait_until(&mut state, patience);
                            continue;
                        }
                    }

                    waiting_since = None;
                }
            }

            let commits = state.tracks.tick(now);

            if wakeup == Wakeup::Frame {
                let period = super::frame_period();
                let due = next_frame.unwrap_or(now);
                let moved = commits
                    .iter()
                    .filter(|commit| matches!(commit, Commit::Write(_)))
                    .count();

                stats::record_tick(
                    now,
                    moved,
                    now.saturating_duration_since(due).as_secs_f64()
                        > period.as_secs_f64() * OVERRUN_TOLERANCE,
                );

                // an absolute schedule, so that a slow frame is made up for rather than passed on,
                // unless it was so slow that the schedule is in the past
                next_frame = Some((due + period).max(Instant::now()));
            }

            if !commits.is_empty() {
                MutexGuard::unlocked(&mut state, || dispatch(commits));
            }
        }
    }
}

enum LaneMessage {
    Write(Write<WindowHandle>),
    Verify(Verify<WindowHandle>),
    /// The window is no longer being animated
    Release(WindowId),
}

static LANES: LazyLock<Mutex<HashMap<i32, Sender<LaneMessage>>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));

fn dispatch(commits: Vec<Commit<WindowHandle>>) {
    let mut lanes = LANES.lock();

    for commit in commits {
        let (pid, message) = match commit {
            Commit::Write(write) => (write.pid, LaneMessage::Write(write)),
            Commit::Verify(verify) => (verify.pid, LaneMessage::Verify(verify)),
        };

        send_to_lane(&mut lanes, pid, message);
    }
}

fn send_to_lane(lanes: &mut HashMap<i32, Sender<LaneMessage>>, pid: i32, message: LaneMessage) {
    let message = match lanes.get(&pid) {
        Some(lane) => match lane.send(message) {
            Ok(()) => return,
            // the lane has gone idle and exited since it was last used
            Err(SendError(message)) => message,
        },
        None => message,
    };

    let (sender, receiver) = crossbeam_channel::unbounded();

    let spawned = std::thread::Builder::new()
        .name(format!("komorebi-animator-{pid}"))
        .spawn(move || run_lane(pid, receiver));

    if let Err(error) = spawned {
        tracing::warn!("could not start an animation lane for process {pid}: {error}");
        return;
    }

    let _ = sender.send(message);
    lanes.insert(pid, sender);
}

/// Everything waiting to be done to one window
struct Job {
    window_id: WindowId,
    write: Option<Merged>,
    verify: Option<Verify<WindowHandle>>,
    release: bool,
}

/// Several writes to one window that were waiting at once, which only the last of matters
struct Merged {
    handle: WindowHandle,
    frame: Rect,
    position: Option<(i32, i32)>,
    size: Option<(i32, i32)>,
    /// How many writes this stands in for
    count: u32,
}

/// Fold what has queued up for a lane into one job per window, in the order the windows first appeared
fn coalesce(messages: Vec<LaneMessage>) -> Vec<Job> {
    let mut order = Vec::new();
    let mut jobs: HashMap<WindowId, Job> = HashMap::new();

    for message in messages {
        let window_id = match &message {
            LaneMessage::Write(write) => write.window_id,
            LaneMessage::Verify(verify) => verify.window_id,
            LaneMessage::Release(window_id) => *window_id,
        };

        let job = jobs.entry(window_id).or_insert_with(|| {
            order.push(window_id);

            Job {
                window_id,
                write: None,
                verify: None,
                release: false,
            }
        });

        match message {
            LaneMessage::Write(write) => {
                // writing again means the window is being animated again
                job.release = false;

                match &mut job.write {
                    Some(merged) => {
                        merged.handle = write.handle;
                        merged.frame = write.frame;
                        // whatever an earlier write changed that this one did not still has to be
                        // done, as the later write only carries what changed since the earlier
                        merged.position = write.position.or(merged.position);
                        merged.size = write.size.or(merged.size);
                        merged.count += 1;
                    }
                    None => {
                        job.write = Some(Merged {
                            handle: write.handle,
                            frame: write.frame,
                            position: write.position,
                            size: write.size,
                            count: 1,
                        });
                    }
                }
            }
            LaneMessage::Verify(verify) => job.verify = Some(verify),
            LaneMessage::Release(_) => job.release = true,
        }
    }

    order
        .into_iter()
        .filter_map(|window_id| jobs.remove(&window_id))
        .collect()
}

/// The thread that talks to one app on behalf of the animator
fn run_lane(pid: i32, receiver: Receiver<LaneMessage>) {
    // the windows of this app that are being animated, and whatever is keeping the app from
    // animating them itself while they are
    let mut active: HashSet<WindowId> = HashSet::new();
    let mut guard: Option<EnhancedUiGuard> = None;

    loop {
        let first = match receiver.recv_timeout(LANE_IDLE) {
            Ok(message) => message,
            Err(RecvTimeoutError::Timeout) => {
                let mut lanes = LANES.lock();

                // the animator only ever sends while holding this lock, so nothing can arrive
                // between looking and leaving
                if receiver.is_empty() {
                    lanes.remove(&pid);
                    return;
                }

                continue;
            }
            Err(RecvTimeoutError::Disconnected) => return,
        };

        let mut batch = vec![first];
        batch.extend(receiver.try_iter());

        for job in coalesce(batch) {
            autoreleasepool(|_| run_job(pid, job, &mut active, &mut guard));
        }

        if active.is_empty() {
            guard = None;
        }
    }
}

fn run_job(
    pid: i32,
    job: Job,
    active: &mut HashSet<WindowId>,
    guard: &mut Option<EnhancedUiGuard>,
) {
    let animator = animator();

    if let Some(write) = job.write {
        let tracked = animator.state.lock().tracks.is_tracked(job.window_id);

        // a window that has been taken over or cancelled since this was queued must be left alone
        let outcome = if tracked {
            active.insert(job.window_id);

            if guard.is_none() {
                *guard = enhanced_ui::disable(pid, &write.handle.app);
            }

            let started = Instant::now();
            let outcome = perform_write(job.window_id, &write);

            // how long the app takes to apply a size decides how often it is sent one
            if write.size.is_some() && outcome == WriteOutcome::Written {
                animator
                    .state
                    .lock()
                    .tracks
                    .note_size_cost(pid, started.elapsed());
            }

            outcome
        } else {
            WriteOutcome::Skipped
        };

        let mut state = animator.state.lock();
        let released = state.tracks.write_done(
            job.window_id,
            write.count,
            write.frame,
            outcome,
            Instant::now(),
        );
        let settled = state.tracks.in_flight(job.window_id) == 0;
        drop(state);

        if released.is_some() {
            active.remove(&job.window_id);
        }

        if settled || released.is_some() {
            animator.settled.notify_all();
            animator.wake.notify_all();
        }
    }

    if let Some(verify) = job.verify {
        // a verification is about a particular destination: if the window has been sent somewhere
        // else since, checking it against the old one would only send it back
        let current =
            animator.state.lock().tracks.generation(job.window_id) == Some(verify.generation);

        let outcome = if current {
            perform_verify(&verify)
        } else {
            VerifyOutcome::Settled
        };

        let mut state = animator.state.lock();
        let released =
            state
                .tracks
                .verify_done(job.window_id, verify.generation, outcome, Instant::now());
        let settled = state.tracks.in_flight(job.window_id) == 0;
        drop(state);

        if released.is_some() {
            active.remove(&job.window_id);
        }

        if settled || released.is_some() {
            animator.settled.notify_all();
        }

        animator.wake.notify_all();
    }

    if job.release {
        active.remove(&job.window_id);
    }
}

fn set_position(
    element: &AccessibilityUiElement,
    x: i32,
    y: i32,
) -> Result<(), AccessibilityError> {
    AccessibilityApi::set_attribute_ax_value(
        element,
        kAXPositionAttribute,
        AXValueType::CGPoint,
        CGPoint::new(x as CGFloat, y as CGFloat),
    )
}

fn set_size(
    element: &AccessibilityUiElement,
    width: i32,
    height: i32,
) -> Result<(), AccessibilityError> {
    AccessibilityApi::set_attribute_ax_value(
        element,
        kAXSizeAttribute,
        AXValueType::CGSize,
        CGSize::new(width as CGFloat, height as CGFloat),
    )
}

fn perform_write(window_id: WindowId, write: &Merged) -> WriteOutcome {
    let started = Instant::now();
    let element = &write.handle.element;

    let mut result = Ok(());

    if let Some((x, y)) = write.position {
        result = set_position(element, x, y);
    }

    if result.is_ok()
        && let Some((width, height)) = write.size
    {
        result = set_size(element, width, height);
    }

    stats::record_write(started.elapsed(), result.is_ok());

    match result {
        Ok(()) => {
            border_manager::follow(window_id, write.frame);

            WriteOutcome::Written
        }
        Err(error) => {
            let gone = reaper::notify_if_gone(window_id, element);
            tracing::debug!("could not move window {window_id}: {error} (gone: {gone})");

            WriteOutcome::Failed { gone }
        }
    }
}

/// How far (in pixels) a window may be from where it was sent without being sent there again
const SETTLE_TOLERANCE: u32 = 2;

/// Compare where a window turned out to be with where it was sent, and say what it turned out to be
///
/// Whether the position and the size are right is returned. Whether the window took the width it
/// was given is said to [`arrival`], every time and whatever the answer, as it forgets a window that
/// has taken its size since it last refused.
fn compare(window_id: WindowId, actual: Rect, target: Rect) -> (bool, bool) {
    arrival::report(window_id, target, actual);

    let position_is_right = actual.left.abs_diff(target.left) <= SETTLE_TOLERANCE
        && actual.top.abs_diff(target.top) <= SETTLE_TOLERANCE;
    let size_is_right = actual.right.abs_diff(target.right) <= SETTLE_TOLERANCE
        && actual.bottom.abs_diff(target.bottom) <= SETTLE_TOLERANCE;

    (position_is_right, size_is_right)
}

fn perform_verify(verify: &Verify<WindowHandle>) -> VerifyOutcome {
    let window_id = verify.window_id;
    let element = &verify.handle.element;
    let target = verify.target;

    let actual = match MacosApi::window_rect(element) {
        Ok(rect) => Rect::from(rect),
        Err(_) => {
            return VerifyOutcome::Failed {
                gone: reaper::notify_if_gone(window_id, element),
            };
        }
    };

    let (position_is_right, size_is_right) = compare(window_id, actual, target);

    if position_is_right && size_is_right {
        // the animation may have been cut short by a slow app, so make sure the border ends up
        // where the window did
        border_manager::follow(window_id, actual);

        return VerifyOutcome::Settled;
    }

    // some apps ignore a move that arrives while they are busy, or only partly apply a resize
    let mut result = Ok(());

    if !position_is_right {
        result = set_position(element, target.left, target.top);
    }

    if result.is_ok() && !size_is_right {
        result = set_size(element, target.right, target.bottom);
    }

    match result {
        Ok(()) => {
            tracing::debug!(
                "window {window_id} was at {actual:?} instead of {target:?}, moved it again{}",
                if verify.last_attempt {
                    " for the last time"
                } else {
                    ""
                }
            );
            border_manager::follow(window_id, target);

            // there is no verification after the last attempt, so look for the last time at what
            // came of it, as that is the last word on where the window ended up
            if verify.last_attempt
                && let Ok(rect) = MacosApi::window_rect(element)
            {
                compare(window_id, Rect::from(rect), target);
            }

            VerifyOutcome::Reissued
        }
        Err(_) => VerifyOutcome::Failed {
            gone: reaper::notify_if_gone(window_id, element),
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn handle() -> WindowHandle {
        WindowHandle {
            element: AccessibilityUiElement::default(),
            app: AccessibilityUiElement::default(),
        }
    }

    fn write(
        window_id: WindowId,
        position: Option<(i32, i32)>,
        size: Option<(i32, i32)>,
    ) -> Write<WindowHandle> {
        Write {
            window_id,
            pid: 1,
            handle: handle(),
            generation: 1,
            frame: Rect {
                left: position.map_or(0, |p| p.0),
                top: position.map_or(0, |p| p.1),
                right: size.map_or(100, |s| s.0),
                bottom: size.map_or(100, |s| s.1),
            },
            position,
            size,
        }
    }

    fn frame(width: i32) -> Rect {
        Rect {
            left: 100,
            top: 50,
            right: width,
            bottom: 800,
        }
    }

    #[test]
    fn a_window_that_would_not_get_as_narrow_is_reported_with_what_it_turned_out_to_be() {
        // ids that nothing else in the tests uses, as what is reported is shared by all of them
        let window_id = 4_100_000_001;

        let (position_is_right, size_is_right) = compare(window_id, frame(600), frame(400));

        assert!(position_is_right);
        assert!(!size_is_right);

        let arrival = arrival::take(window_id).expect("the refusal should have been reported");
        assert_eq!(arrival.target, frame(400));
        assert_eq!(arrival.actual, frame(600));
    }

    #[test]
    fn a_window_that_took_its_size_is_not_a_refusal_and_is_forgotten() {
        let window_id = 4_100_000_002;

        // it refused, and then it did not
        compare(window_id, frame(600), frame(400));
        let (position_is_right, size_is_right) = compare(window_id, frame(401), frame(400));

        assert!(position_is_right && size_is_right);
        assert_eq!(arrival::take(window_id), None);
    }

    #[test]
    fn a_window_that_is_narrower_than_it_was_told_to_be_is_not_a_refusal() {
        let window_id = 4_100_000_003;

        let (_, size_is_right) = compare(window_id, frame(300), frame(400));

        assert!(!size_is_right);
        assert_eq!(arrival::take(window_id), None);
    }

    #[test]
    fn a_window_that_is_in_the_wrong_place_but_the_right_size_is_not_a_refusal() {
        let window_id = 4_100_000_004;
        let mut actual = frame(400);
        actual.left += 300;

        let (position_is_right, size_is_right) = compare(window_id, actual, frame(400));

        assert!(!position_is_right);
        assert!(size_is_right);
        assert_eq!(arrival::take(window_id), None);
    }

    #[test]
    fn writes_to_one_window_are_folded_into_one() {
        let jobs = coalesce(vec![
            LaneMessage::Write(write(1, Some((10, 0)), None)),
            LaneMessage::Write(write(1, Some((20, 0)), None)),
            LaneMessage::Write(write(1, Some((30, 0)), None)),
        ]);

        assert_eq!(jobs.len(), 1);

        let merged = jobs[0].write.as_ref().unwrap();
        assert_eq!(merged.count, 3);
        assert_eq!(merged.position, Some((30, 0)));
        assert_eq!(merged.frame.left, 30);
    }

    #[test]
    fn a_folded_write_still_carries_what_only_an_earlier_write_changed() {
        let jobs = coalesce(vec![
            LaneMessage::Write(write(1, Some((10, 0)), Some((400, 300)))),
            LaneMessage::Write(write(1, Some((20, 0)), None)),
        ]);

        let merged = jobs[0].write.as_ref().unwrap();
        assert_eq!(merged.position, Some((20, 0)));
        assert_eq!(merged.size, Some((400, 300)));
    }

    #[test]
    fn windows_keep_the_order_they_first_appeared_in() {
        let jobs = coalesce(vec![
            LaneMessage::Write(write(7, Some((1, 0)), None)),
            LaneMessage::Write(write(3, Some((1, 0)), None)),
            LaneMessage::Write(write(7, Some((2, 0)), None)),
            LaneMessage::Write(write(5, Some((1, 0)), None)),
        ]);

        assert_eq!(
            jobs.iter().map(|job| job.window_id).collect::<Vec<_>>(),
            vec![7, 3, 5]
        );
    }

    #[test]
    fn writing_after_a_release_means_the_window_is_animated_again() {
        let jobs = coalesce(vec![
            LaneMessage::Release(1),
            LaneMessage::Write(write(1, Some((10, 0)), None)),
        ]);
        assert!(!jobs[0].release);

        let jobs = coalesce(vec![
            LaneMessage::Write(write(1, Some((10, 0)), None)),
            LaneMessage::Release(1),
        ]);
        assert!(jobs[0].release);
        assert!(jobs[0].write.is_some());
    }

    #[test]
    fn nothing_is_animating_before_anything_is_animated() {
        assert!(!is_animating(u32::MAX));
        assert_eq!(current_frame(u32::MAX), None);
        assert_eq!(take_over(u32::MAX), None);
        assert_eq!(
            notification_verdict(u32::MAX, 1, true, || None),
            Verdict::Forward
        );
        assert!(admit_notification(u32::MAX, 1, false, false));
        assert!(admit_notification(u32::MAX, 1, false, false));
    }
}
