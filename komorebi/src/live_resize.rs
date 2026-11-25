//! The tiles next to the edge of a window that is dragged to resize it follow
//! the edge while it is dragged, and not only once it is let go.
//!
//! Windows says where a window that is being dragged is on every step, with
//! a location change. They come far faster than anything can be laid out, so
//! they are only a signal that the window has moved: one waits at most, and what
//! is laid out is where the window is when it is looked at, which is the newest.
//! Steps are laid out at most once every frame, so what is on the screen is
//! as smooth as the pointer, and no more work is done than is seen.
//!
//! Every step, and the end of the drag, is worked out from the layout as it was
//! when the user took hold of the window, and not from the step before: a drag
//! ends the same with its steps as without them, and one that goes back to where
//! it began puts the neighbours back.

use crate::WindowsApi;
use crate::animation::ANIMATION_DURATION_GLOBAL;
use crate::animation::GHOST_MOVEMENT_ENABLED;
use crate::animation::ghost::GhostWindow;
use crate::animation::ghost::is_swap_chain_window;
use crate::animation::ghost::nudge_swap_chain_window;
use crate::animation::lerp::Lerp;
use crate::animation::snapshot;
use crate::border_manager;
use crate::com::SetCloak;
use crate::core::AnimationStyle;
use crate::core::OperationDirection;
use crate::core::Rect;
use crate::core::Sizing;
use crate::window::Window;
use crate::window_manager::WindowManager;
use crate::workspace::WorkspaceLayer;
use color_eyre::eyre;
use crossbeam_channel::Sender;
use crossbeam_channel::bounded;
use komorebi_scrolling::ScrollingSpace;
use parking_lot::Mutex;
use std::collections::HashMap;
use std::sync::Arc;
use std::sync::OnceLock;
use std::sync::atomic::AtomicBool;
use std::sync::atomic::AtomicIsize;
use std::sync::atomic::AtomicUsize;
use std::sync::atomic::Ordering;
use std::time::Duration;
use std::time::Instant;

/// The least time between two steps that are laid out: a frame of a display
/// that is refreshed 60 times a second
const STEP_INTERVAL: Duration = Duration::from_millis(16);

/// Lays out what is laid out while it is alive at once, and not animated: the
/// neighbours of a window that is dragged follow it as it goes, and are not to be
/// seen catching up
pub struct Snapping {
    /// Whether it is for a step of a drag, and not for its end
    paced: bool,
}

impl Snapping {
    /// `paced` is for the steps of a drag, which leave a window that has not
    /// got to where it was last sent for the next step: the end of a drag puts
    /// everything where it is to be
    #[must_use]
    pub fn new(paced: bool) -> Self {
        PACED.store(paced, Ordering::SeqCst);
        STEPPING.store(true, Ordering::SeqCst);
        Self { paced }
    }
}

impl Drop for Snapping {
    fn drop(&mut self) {
        STEPPING.store(false, Ordering::SeqCst);
        PACED.store(false, Ordering::SeqCst);

        // the end of a drag has laid the workspace out, which is where the windows that
        // were covered are to go: they are sent there, hidden, and shown from where they
        // are not waited for by the window manager
        if !self.paced
            && COVERS
                .lock()
                .as_ref()
                .is_some_and(|covers| !covers.is_empty())
        {
            std::thread::spawn(|| uncover(true));
        }
    }
}

thread_local! {
    static IN_PLACE: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

/// Puts the windows that the thread lays out while it is alive where they go at once, and
/// not by an animation, and not under a picture of them: the commands that resize a window
/// are many and quick, and the picture of a window that is made as big or as small as another
/// is a little dimmer and softer than the window, which is more to be seen of a layout that is
/// animated for every one of them than of a drag, which has its pictures, for what it does to
/// the windows that follow its edge, whichever the animations are.
pub struct InPlace {
    previous: bool,
}

impl InPlace {
    #[must_use]
    pub fn new() -> Self {
        Self {
            previous: IN_PLACE.with(|in_place| in_place.replace(true)),
        }
    }
}

impl Default for InPlace {
    fn default() -> Self {
        Self::new()
    }
}

impl Drop for InPlace {
    fn drop(&mut self) {
        IN_PLACE.with(|in_place| in_place.set(self.previous));
    }
}

/// Whether the thread is laying windows out in place, for a command that resizes them
pub fn is_in_place() -> bool {
    IN_PLACE.with(std::cell::Cell::get)
}

/// How long a window is given to get to where it was sent before it is sent
/// somewhere else anyway
const IN_FLIGHT: Duration = Duration::from_millis(100);

/// Whether what is laid out for a step leaves windows that are yet to get where
/// they were sent
static PACED: AtomicBool = AtomicBool::new(false);

/// Whether a window was left for the next step, so there is to be one
static DEFERRED: AtomicBool = AtomicBool::new(false);

/// Where the windows were last sent in a drag, and when
static SENT: Mutex<Option<HashMap<isize, (Rect, Instant)>>> = Mutex::new(None);

/// A window that is moved has to do all of the layout of what is in it, which
/// takes a window such as File Explorer a good deal longer than the time between
/// two steps. A move sent for every step waits in the queue of the window, which
/// is busy for as long as there are any: it falls behind, and goes on catching up
/// after the drag is over. So a window is not sent anywhere while it is yet to get
/// to where it was sent last: the step after the next is for where it is to be then,
/// and never for where it was.
pub fn is_catching_up(hwnd: isize, layout: &Rect) -> bool {
    if !PACED.load(Ordering::Relaxed) {
        return false;
    }

    let sent = SENT
        .lock()
        .as_ref()
        .and_then(|sent| sent.get(&hwnd).copied());

    let Some((target, at)) = sent else {
        return false;
    };

    let arrived = WindowsApi::window_rect(hwnd).is_ok_and(|rect| rect == target);

    if arrived || at.elapsed() >= IN_FLIGHT {
        return false;
    }

    // there is to be a step to put it where it is to be, if the drag goes
    // no further
    if target != *layout {
        DEFERRED.store(true, Ordering::SeqCst);
    }

    true
}

/// How long after a drag ends the windows are looked at for ones that did not get
/// where it left them, and then again, for the ones that were sent again
const SETTLE_AFTER: [Duration; 2] = [Duration::from_millis(250), Duration::from_millis(350)];

/// How many pixels a window can be from where it is to be, on any side, and be there: what
/// rounding, a terminal's grid of cells and the frame a window has are.
const SETTLE_TOLERANCE: i32 = 2;

/// Which drag the windows are being looked at after
static GENERATION: AtomicUsize = AtomicUsize::new(0);

static SETTLE: OnceLock<Sender<(usize, usize, usize)>> = OnceLock::new();

/// Whether a window that is at `rect` is not where it is to be, `layout`
fn is_off(rect: &Rect, layout: &Rect) -> bool {
    [
        rect.left - layout.left,
        rect.top - layout.top,
        rect.right - layout.right,
        rect.bottom - layout.bottom,
    ]
    .iter()
    .any(|difference| difference.abs() > SETTLE_TOLERANCE)
}

/// Whether what is laid out is for a step of a drag, and not for its end
pub fn is_pacing() -> bool {
    PACED.load(Ordering::Relaxed)
}

/// A window has been sent to `layout`
pub fn sent(hwnd: isize, layout: &Rect) {
    SENT.lock()
        .get_or_insert_with(HashMap::new)
        .insert(hwnd, (*layout, Instant::now()));
}

/// What a window that is resized by a drag is covered by while it is
struct Cover {
    /// A picture of the window
    ghost: GhostWindow,
    /// Where the picture is
    at: Rect,
    /// Where the picture is when the strip has not been moved, which is not where the window is
    /// if it is parked at the edge of the screen
    origin: Rect,
    /// Where the window is to go, which is where the last step put the picture
    target: Rect,
}

static COVERS: Mutex<Option<HashMap<isize, Cover>>> = Mutex::new(None);

/// Whether a window is covered, which is to say hidden, and put where it is to go when
/// the drag is over
pub fn is_covered(hwnd: isize) -> bool {
    COVERS
        .lock()
        .as_ref()
        .is_some_and(|covers| covers.contains_key(&hwnd))
}

/// A workspace is about to be laid out a step at a time by something that is not a drag: the
/// pictures of the last of them are taken away, and the windows that were sent somewhere
/// are forgotten.
pub fn begin_steps() {
    GENERATION.fetch_add(1, Ordering::SeqCst);
    uncover(false);
    SENT.lock().take();
}

/// A window that is to be resized for a step of a drag, from `from` to `to`, and
/// whether it is not to be sent there: it is covered.
///
/// A window that is resized has nothing to show for the frames between the old size
/// going and the new one having anything in it (a terminal that renders through a swap
/// chain is not there at all, and the wallpaper shows through where it was), and
/// one that is slow to take a move, which File Explorer is, is seen at every size
/// that it is on the way through, laying itself out again. So a picture of it is put
/// where it is, the window is hidden under it, and every step moves the picture, with
/// one call to DWM, and does nothing to the window. When the drag is over the window
/// is put where it is to go, and made to lay itself out, unseen, as for a move that is
/// animated, and then shown, and the picture fades out over it.
///
/// Only with ghost movement, which is what puts pictures of windows on the screen.
pub fn follow(hwnd: isize, from: &Rect, to: &Rect) -> bool {
    if !GHOST_MOVEMENT_ENABLED.load(Ordering::Relaxed) {
        return false;
    }

    let mut covers = COVERS.lock();
    let covers = covers.get_or_insert_with(HashMap::new);

    if let std::collections::hash_map::Entry::Vacant(vacant) = covers.entry(hwnd) {
        // a window that is not where anyone can see it has nothing to cover
        if from == to
            || crate::HIDDEN_HWNDS.lock().contains(&hwnd)
            || WindowsApi::is_window_cloaked(hwnd).unwrap_or(false)
        {
            return false;
        }

        // the picture is taken before the window is touched
        match GhostWindow::create(hwnd, *from, None) {
            Ok(ghost) => {
                border_manager::set_ghosted(hwnd, true);
                SetCloak(Window { hwnd }.hwnd(), 1, 2);

                vacant.insert(Cover {
                    ghost,
                    at: *from,
                    origin: *from,
                    target: *to,
                });
            }
            Err(error) => {
                tracing::debug!("live resize: no cover for hwnd {hwnd}: {error}");
                return false;
            }
        }
    }

    let Some(cover) = covers.get_mut(&hwnd) else {
        return false;
    };

    cover.target = *to;

    // where a glide is to end is all that it is told: it is the glide that moves the picture
    if GLIDING.load(Ordering::SeqCst) {
        return true;
    }

    match cover.ghost.update(*to, 255) {
        Ok(()) => {
            cover.at = *to;

            // the border of a covered window is left to the cover, which is to say that it
            // is where the picture is: nothing else tells it
            border_manager::animate_to(hwnd, *to);

            tracing::trace!(
                "live resize: cover of hwnd {hwnd} to {to:?}: {}",
                cover.ghost.describe()
            );
        }
        Err(error) => {
            tracing::debug!(
                "live resize: the cover of hwnd {hwnd} was not moved to {to:?}: {error}"
            );
        }
    }

    true
}

/// Whether the covers are to be told where they go and not moved there, as they are moved to
/// it by [`glide`]
static GLIDING: AtomicBool = AtomicBool::new(false);

/// Covers are told where they go, and not moved there, while this is held
pub struct Gliding;

impl Gliding {
    pub fn begin() -> Self {
        GLIDING.store(true, Ordering::SeqCst);
        Self
    }
}

impl Drop for Gliding {
    fn drop(&mut self) {
        GLIDING.store(false, Ordering::SeqCst);
    }
}

/// Where a picture is `progress` of the way (0 to 1) from where it was to where it goes: it
/// slows as it gets there, as the windows of the animations do
fn glide_frame(from: &Rect, to: &Rect, progress: f64) -> Rect {
    from.lerp(*to, progress.clamp(0.0, 1.0), AnimationStyle::EaseOutCubic)
}

/// Move the pictures of the covers from where they are to where their windows are to go, as
/// long as an animation takes, and nothing else is done until they are there. For the windows
/// of a strip that is let go of, which then does not jump to where it comes to rest.
pub fn glide() {
    let duration = Duration::from_millis(ANIMATION_DURATION_GLOBAL.load(Ordering::SeqCst));

    let mut covers = COVERS.lock();
    let Some(covers) = covers.as_mut() else {
        return;
    };

    let moving: Vec<_> = covers
        .iter()
        .filter(|(_, cover)| cover.at != cover.target)
        .map(|(hwnd, cover)| (*hwnd, cover.at, cover.target))
        .collect();

    if moving.is_empty() || duration.is_zero() {
        return;
    }

    let started = Instant::now();

    loop {
        let progress = started.elapsed().as_secs_f64() / duration.as_secs_f64();

        for (hwnd, from, to) in &moving {
            let rect = glide_frame(from, to, progress.min(1.0));

            if let Some(cover) = covers.get_mut(hwnd)
                && cover.ghost.update(rect, 255).is_ok()
            {
                cover.at = rect;
                border_manager::animate_to(*hwnd, rect);
            }
        }

        if progress >= 1.0 {
            break;
        }

        // one frame of the screen at a time, as the animations are
        unsafe {
            let _ = windows::Win32::Graphics::Dwm::DwmFlush();
        }
    }
}

/// Whether a window at `rect` is near enough to a work area to be seen soon: within a width
/// of it
pub fn is_near(rect: &Rect, work_area: &Rect) -> bool {
    // a Rect has a width and a height where it would have a right and a bottom
    let width = work_area.right;
    rect.left + rect.right > work_area.left - width
        && rect.left < work_area.left + work_area.right + width
}

/// Cover the windows that are given with where their pictures are to be, which is not where
/// they are if they are parked at the edge of the screen, all of them at once, so that the
/// first step of something that moves them does not wait for a picture of each to be taken
/// one after the other. A window that is covered already is left as it is. Only with ghost
/// movement.
pub fn cover_all(windows: &[(isize, Rect)]) {
    if !GHOST_MOVEMENT_ENABLED.load(Ordering::Relaxed) {
        return;
    }

    let wanted: Vec<_> = windows
        .iter()
        .copied()
        .filter(|(hwnd, _)| !is_covered(*hwnd))
        .filter(|(hwnd, _)| {
            !crate::HIDDEN_HWNDS.lock().contains(hwnd)
                && !WindowsApi::is_window_cloaked(*hwnd).unwrap_or(false)
        })
        .collect();

    let made: Vec<_> = wanted
        .into_iter()
        .map(|(hwnd, origin)| {
            std::thread::spawn(move || {
                let rect = WindowsApi::window_rect(hwnd).ok()?;

                // the picture is taken before the window is touched
                match GhostWindow::create(hwnd, origin, None) {
                    Ok(ghost) => Some((hwnd, rect, origin, ghost)),
                    Err(error) => {
                        tracing::debug!("cover: no cover for hwnd {hwnd}: {error}");
                        None
                    }
                }
            })
        })
        .collect();

    let mut covers = COVERS.lock();
    let covers = covers.get_or_insert_with(HashMap::new);

    for (hwnd, rect, origin, ghost) in made
        .into_iter()
        .filter_map(|made| made.join().ok().flatten())
    {
        border_manager::set_ghosted(hwnd, true);
        SetCloak(Window { hwnd }.hwnd(), 1, 2);

        covers.insert(
            hwnd,
            Cover {
                ghost,
                at: origin,
                origin,
                target: rect,
            },
        );
    }
}

/// `rect` moved `shift` pixels to the right, or to the left if it is negative
fn shifted(rect: &Rect, shift: f64) -> Rect {
    #[allow(clippy::cast_possible_truncation)]
    let by = shift.round() as i32;

    Rect {
        left: rect.left + by,
        ..*rect
    }
}

/// Put the pictures of the covers where they are when the strip is `shift` pixels from where
/// it was when they were taken, which is all there is to a step of a pan: the windows are not
/// touched and nothing is laid out. Whether there were any.
pub fn shift_covers(shift: f64) -> bool {
    let mut covers = COVERS.lock();
    let Some(covers) = covers.as_mut().filter(|covers| !covers.is_empty()) else {
        return false;
    };

    for (hwnd, cover) in covers.iter_mut() {
        let rect = shifted(&cover.origin, shift);

        if cover.ghost.update(rect, 255).is_ok() {
            cover.at = rect;
            border_manager::animate_to(*hwnd, rect);
        }
    }

    true
}

/// Take the covers away. After a drag `show` is true: each window is put where it is
/// to go, made to lay itself out there and to build its swap chain again, all of it
/// hidden, and then shown, and the picture fades out over it. For another drag, which
/// has begun before that, they are just taken away, and the windows put where they
/// were last to go.
pub fn uncover(show: bool) {
    let covers = COVERS.lock().take().unwrap_or_default();

    if !show {
        for (hwnd, cover) in covers {
            if WindowsApi::is_window(hwnd) {
                let _ = WindowsApi::position_window(hwnd, &cover.target, false, true);
            }
            finish_cover(hwnd, cover.ghost);
        }

        return;
    }

    // a window that is slow to lay itself out is not waited for by the others
    let reveals: Vec<_> = covers
        .into_iter()
        .map(|(hwnd, cover)| std::thread::spawn(move || reveal(hwnd, cover)))
        .collect();

    for reveal in reveals {
        let _ = reveal.join();
    }
}

/// Put a window that was covered where it is to go, show it, and fade its picture out
fn reveal(hwnd: isize, cover: Cover) {
    let Cover {
        ghost, at, target, ..
    } = cover;

    if WindowsApi::is_window(hwnd) {
        if WindowsApi::position_window(hwnd, &target, false, false).is_ok() {
            // it is hidden, so it is not going to lay itself out before it is shown, and
            // File Explorer takes its time over it when it is
            snapshot::paint_now(hwnd);

            // and the last thing to be done to the size of a window that renders through
            // a swap chain before it is shown is the size change that makes it build it again
            if is_swap_chain_window(hwnd) {
                nudge_swap_chain_window(hwnd, &target, false);
            }
        }

        if is_still_ours(hwnd) {
            SetCloak(Window { hwnd }.hwnd(), 1, 0);
            ghost.fade_out(at, None);
        }
    }

    finish_cover(hwnd, ghost);
}

/// A window of the workspace that is not hidden by anything but the cover
fn is_still_ours(hwnd: isize) -> bool {
    !crate::HIDDEN_HWNDS.lock().contains(&hwnd)
}

/// The picture is gone, and the window, if it is there, has its border back
fn finish_cover(hwnd: isize, ghost: GhostWindow) {
    if WindowsApi::is_window(hwnd) && is_still_ours(hwnd) {
        SetCloak(Window { hwnd }.hwnd(), 1, 0);
    }

    border_manager::set_ghosted(hwnd, false);
    let _ = ghost.dispose();

    if WindowsApi::is_window(hwnd) {
        border_manager::send_notification(Some(hwnd));
    }
}

/// How soon a window that a step left alone, as it had not got where it was sent, is
/// looked at again: a window that is quick to move, which a terminal is, is there
/// in well under the time between two steps, and waiting for the next step would
/// have it move half as often as it can
const RETRY_AFTER: Duration = Duration::from_millis(6);

/// The window that has its edge dragged, or 0
static DRAGGED: AtomicIsize = AtomicIsize::new(0);

/// Whether the window manager has been told of a step that it has not yet got to
static STEP_WAITING: AtomicBool = AtomicBool::new(false);

/// Whether the workspace is being laid out for a step of a drag
static STEPPING: AtomicBool = AtomicBool::new(false);

static STEPS: OnceLock<Sender<()>> = OnceLock::new();

/// What the workspace was when the user took hold of the edge of a window
struct Kept {
    hwnd: isize,
    monitor_idx: usize,
    workspace_idx: usize,
    /// Where the window was, which is what the drag is measured from
    started_from: Rect,
    containers: usize,
    resize_dimensions: Vec<Option<Rect>>,
    latest_layout: Vec<Rect>,
    /// What a scrolling layout has been given: the widths of its columns and where
    /// the strip is
    scrolling: Option<ScrollingSpace<String>>,
    /// Whether a step has laid out the workspace differently from this
    stepped: bool,
}

static KEPT: Mutex<Option<Kept>> = Mutex::new(None);

/// Whether the window is one that the user has hold of by an edge, which is
/// theirs until they let go: it is not put on its tile by a layout
pub fn is_dragging(hwnd: isize) -> bool {
    hwnd != 0 && DRAGGED.load(Ordering::Relaxed) == hwnd
}

/// Whether the workspace is being laid out for a step of a drag, which is to
/// be seen as it is done, and not animated
pub fn is_stepping() -> bool {
    STEPPING.load(Ordering::Relaxed)
}

/// The window has moved: if it is the one being dragged, a step is to be laid out
/// unless one is waiting to be, as it will look for where the window is.
pub fn window_moved(hwnd: isize) {
    if !is_dragging(hwnd) || STEP_WAITING.swap(true, Ordering::AcqRel) {
        return;
    }

    if let Some(steps) = STEPS.get() {
        // a step is waiting if it is full, which is all there is to say
        let _ = steps.try_send(());
    }
}

/// The user has taken hold of the window, by its title bar or an edge: it is
/// theirs until they let go
pub fn taken_hold_of(hwnd: isize) {
    STEP_WAITING.store(false, Ordering::SeqCst);
    DRAGGED.store(hwnd, Ordering::SeqCst);
}

/// The user has let go: nothing of the window is left alone, and there is no more
/// to follow. What was kept is for the end of the drag to put back.
pub fn let_go(hwnd: isize) {
    if is_dragging(hwnd) {
        DRAGGED.store(0, Ordering::SeqCst);
    }
}

/// The edges a drag moved, as a window manager is asked to move them: which
/// edge, whether the tile is to grow or shrink at it, and by how much. A shared
/// edge is moved by half of what the layout is given.
///
/// A rect is (left, top, width, height), so these are worked out from the
/// edges.
pub fn edges_moved(from: &Rect, to: &Rect) -> Vec<(OperationDirection, Sizing, i32)> {
    let edges = |r: &Rect| (r.left, r.top, r.left + r.right, r.top + r.bottom);
    let (start_left, start_top, start_right, start_bottom) = edges(from);
    let (end_left, end_top, end_right, end_bottom) = edges(to);

    [
        (OperationDirection::Left, end_left - start_left),
        (OperationDirection::Up, end_top - start_top),
        (OperationDirection::Right, end_right - start_right),
        (OperationDirection::Down, end_bottom - start_bottom),
    ]
    .into_iter()
    .filter(|(_, by)| *by != 0)
    .map(|(edge, by)| {
        let outward = match edge {
            OperationDirection::Left | OperationDirection::Up => by < 0,
            OperationDirection::Right | OperationDirection::Down => by > 0,
        };

        let sizing = if outward {
            Sizing::Increase
        } else {
            Sizing::Decrease
        };

        (edge, sizing, by.abs() * 2)
    })
    .collect()
}

/// Whether a window that is now `to` has been resized from `from`, and not just
/// moved
pub fn is_resized(from: &Rect, to: &Rect) -> bool {
    from.right != to.right || from.bottom != to.bottom
}

/// Follow what is signalled by `window_moved`
pub fn listen_for_steps(wm: Arc<Mutex<WindowManager>>) {
    let (tx, rx) = bounded::<()>(1);
    if STEPS.set(tx).is_err() {
        return;
    }

    // A window can be sent where it is to be and not get there: one that is busy with
    // what an earlier move made it do is seen to be left where that put it. So when
    // a drag is over the windows are looked at, a quarter of a second after, when
    // they have done what they were going to, and any that are not where they are to
    // be are sent there again, and looked at once more after that.
    let (settle_tx, settle_rx) = bounded::<(usize, usize, usize)>(1);
    if SETTLE.set(settle_tx).is_ok() {
        let wm = wm.clone();

        std::thread::spawn(move || {
            while let Ok((generation, monitor_idx, workspace_idx)) = settle_rx.recv() {
                for wait in SETTLE_AFTER {
                    std::thread::sleep(wait);

                    // another drag has begun
                    if GENERATION.load(Ordering::SeqCst) != generation {
                        break;
                    }

                    match wm.lock().live_resize_settle(monitor_idx, workspace_idx) {
                        Ok(0) => {
                            tracing::debug!(
                                "live resize: after the drag, every window is where it is to be"
                            );
                            break;
                        }
                        Ok(sent) => {
                            tracing::debug!(
                                "live resize: after the drag, {sent} window(s) were not where they are to be"
                            );
                        }
                        Err(error) => {
                            tracing::debug!("live resize: {error}");
                            break;
                        }
                    }
                }

                // the windows have done what they were going to, if there is
                // no other drag that the pictures are for
                if GENERATION.load(Ordering::SeqCst) == generation {
                    uncover(true);
                }
            }
        });
    }

    std::thread::spawn(move || {
        while rx.recv().is_ok() {
            let began = Instant::now();

            // from here a move is signalled again, and the newest is looked at
            STEP_WAITING.store(false, Ordering::Release);

            if let Err(error) = wm.lock().live_resize_step() {
                tracing::debug!("live resize: {error}");
            }

            // a window was left to get where it was sent, and the drag may
            // have stopped short of another step: look at it again soon, and
            // otherwise no more often than the display shows steps
            let deferred = DEFERRED.swap(false, Ordering::SeqCst);
            if deferred {
                // after the step, and not from when it began: a step takes
                // a good part of this, and the window manager is not to be
                // held by steps that follow one another
                std::thread::sleep(RETRY_AFTER);
            } else if let Some(left) = STEP_INTERVAL.checked_sub(began.elapsed()) {
                std::thread::sleep(left);
            }

            if deferred
                && DRAGGED.load(Ordering::Relaxed) != 0
                && !STEP_WAITING.swap(true, Ordering::AcqRel)
                && let Some(steps) = STEPS.get()
            {
                let _ = steps.try_send(());
            }
        }
    });
}

/// How far a window can have got between the user taking hold of an edge and the start of the
/// drag being handled, which a fast drag makes a good many pixels
const DRAG_LAG_ALLOWANCE: i32 = 64;

/// Where a drag by the edges of a window began, given the rect the window had when the start of
/// the drag was handled, the rect it has now and the tile it was put in.
///
/// The start is handled after the drag has begun, so an edge that has moved since then was taken
/// from the tile's edge, not from where it had got to: measuring from the latter loses what the
/// user dragged before that, which is more the longer the drag. An edge that has not moved is
/// not being dragged and is left as it was read, as is an edge that is too far from its tile
/// for that to be the reason (a window that cannot be as small as its tile).
pub fn drag_origin(started_from: &Rect, now: &Rect, tile: Option<&Rect>) -> Rect {
    let Some(tile) = tile else {
        return *started_from;
    };

    let edge = |started: i32, now: i32, tile: i32| {
        if now != started && (started - tile).abs() <= DRAG_LAG_ALLOWANCE {
            tile
        } else {
            started
        }
    };

    // A Rect is (left, top, width, height); work in edges instead
    let left = edge(started_from.left, now.left, tile.left);
    let top = edge(started_from.top, now.top, tile.top);
    let right = edge(
        started_from.left + started_from.right,
        now.left + now.right,
        tile.left + tile.right,
    );
    let bottom = edge(
        started_from.top + started_from.bottom,
        now.top + now.bottom,
        tile.top + tile.bottom,
    );

    Rect {
        left,
        top,
        right: right - left,
        bottom: bottom - top,
    }
}

impl WindowManager {
    /// The user has taken hold of a window by its title bar or an edge, which
    /// is not known to be either yet: what the workspace is is kept in case it
    /// is an edge.
    pub fn live_resize_begin(&mut self, hwnd: isize, started_from: Rect) {
        GENERATION.fetch_add(1, Ordering::SeqCst);
        uncover(false);
        *KEPT.lock() = None;
        SENT.lock().take();

        let monitor_idx = self.focused_monitor_idx();
        let Ok(workspace_idx) = self.focused_workspace_idx() else {
            return;
        };
        let Ok(workspace) = self.focused_workspace() else {
            return;
        };

        let follows = workspace.tile
            && workspace.layer == WorkspaceLayer::Tiling
            && workspace.maximized_window.is_none()
            && workspace.monocle_container.is_none()
            // the columns of a scrolling layout are not resized by what is kept
            && workspace.container_idx_for_window(hwnd).is_some();

        if !follows {
            return;
        }

        *KEPT.lock() = Some(Kept {
            hwnd,
            monitor_idx,
            workspace_idx,
            started_from,
            containers: workspace.containers().len(),
            resize_dimensions: workspace.resize_dimensions.clone(),
            latest_layout: workspace.latest_layout.clone(),
            scrolling: workspace.scrolling.clone(),
            stepped: false,
        });
    }

    /// Lay out the workspace for where the user has the edge of the window now
    pub fn live_resize_step(&mut self) -> eyre::Result<()> {
        if self.is_paused {
            return Ok(());
        }

        let mut kept = KEPT.lock();
        let Some(kept) = kept.as_mut() else {
            return Ok(());
        };

        // let go already
        if !is_dragging(kept.hwnd) {
            return Ok(());
        }

        let now = WindowsApi::window_rect(kept.hwnd)?;

        // a window moved by its title bar changes no tile until it is let go,
        // and a drag that has gone back to where it began is a step that puts
        // the neighbours back
        if !is_resized(&kept.started_from, &now) && !kept.stepped {
            return Ok(());
        }

        if self.focused_monitor_idx() != kept.monitor_idx
            || self.focused_workspace_idx()? != kept.workspace_idx
        {
            return Ok(());
        }

        let monitor_offset = self.work_area_offset;
        let workspace = self.focused_workspace_mut()?;

        // a window that opens or closes in the middle of a drag changes what the
        // layout is made of: there is nothing to follow until the end
        if !workspace.tile || workspace.containers().len() != kept.containers {
            return Ok(());
        }

        workspace.resize_dimensions = kept.resize_dimensions.clone();
        workspace.latest_layout = kept.latest_layout.clone();
        workspace.scrolling = kept.scrolling.clone();

        if let Some(idx) = workspace.container_idx_for_window(kept.hwnd) {
            workspace.focus_container(idx);
        }

        let tile = workspace
            .container_idx_for_window(kept.hwnd)
            .and_then(|idx| kept.latest_layout.get(idx));
        let started_from = drag_origin(&kept.started_from, &now, tile);

        for (edge, sizing, by) in edges_moved(&started_from, &now) {
            self.resize_window(edge, sizing, by, false)?;
        }

        kept.stepped = true;

        let _snapping = Snapping::new(true);
        self.focused_monitor_mut()
            .ok_or_else(|| eyre::eyre!("there is no monitor"))
            .and_then(|monitor| monitor.update_focused_workspace(monitor_offset))
    }

    /// The user has let go: the workspace is what it was when they took hold,
    /// for the end of the drag to work out what it did from there, as it does
    /// when there were no steps.
    ///
    /// The neighbours of a window that was followed are where the last step put
    /// them, a few pixels from where the end of the drag does, which is not to be
    /// animated: they are laid out at once for as long as what is returned lives.
    #[must_use]
    pub fn live_resize_end(&mut self) -> Option<Snapping> {
        let kept = KEPT.lock().take()?;

        if !kept.stepped {
            return None;
        }

        SENT.lock().take();
        let snapping = Snapping::new(false);

        if self.focused_monitor_idx() != kept.monitor_idx
            || self.focused_workspace_idx().ok() != Some(kept.workspace_idx)
        {
            return Some(snapping);
        }

        if let Ok(workspace) = self.focused_workspace_mut()
            && workspace.containers().len() == kept.containers
        {
            workspace.resize_dimensions = kept.resize_dimensions;
            workspace.latest_layout = kept.latest_layout;
            workspace.scrolling = kept.scrolling;
        }

        // a step may have left a window short of where the end of the drag sends it
        // a step may have left a window short of where the end of the drag sends it
        if let Some(settle) = SETTLE.get() {
            let _ = settle.try_send((
                GENERATION.load(Ordering::SeqCst),
                kept.monitor_idx,
                kept.workspace_idx,
            ));
        }

        Some(snapping)
    }

    /// Send the windows of the workspace that are not where its layout puts them
    /// to where it does, and say how many there were. Nothing is done for another
    /// workspace than the one that is on show.
    pub fn live_resize_settle(
        &mut self,
        monitor_idx: usize,
        workspace_idx: usize,
    ) -> eyre::Result<usize> {
        if self.is_paused
            || self.focused_monitor_idx() != monitor_idx
            || self.focused_workspace_idx()? != workspace_idx
        {
            return Ok(0);
        }

        let workspace = self.focused_workspace()?;

        if !workspace.tile
            || workspace.monocle_container.is_some()
            || workspace.maximized_window.is_some()
        {
            return Ok(0);
        }

        let mut sent = 0;

        for (idx, container) in workspace.containers().iter().enumerate() {
            let (Some(layout), Some(window)) =
                (workspace.latest_layout.get(idx), container.focused_window())
            else {
                continue;
            };

            if crate::HIDDEN_HWNDS.lock().contains(&window.hwnd)
                || is_covered(window.hwnd)
                || is_dragging(window.hwnd)
                || window.is_miminized()
            {
                continue;
            }

            let Ok(rect) = WindowsApi::window_rect(window.hwnd) else {
                continue;
            };

            if is_off(&rect, layout) {
                tracing::debug!(
                    "live resize: hwnd {} is at {rect:?} after the drag, sent again to {layout:?}",
                    window.hwnd
                );
                let _ = WindowsApi::position_window(window.hwnd, layout, false, true);
                sent += 1;
            }
        }

        Ok(sent)
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn a_picture_is_shifted_along_the_screen_and_nowhere_else() {
        let rect = Rect {
            left: 100,
            top: 20,
            right: 400,
            bottom: 300,
        };

        assert_eq!(super::shifted(&rect, 0.0), rect);
        assert_eq!(super::shifted(&rect, 50.4).left, 150);
        assert_eq!(super::shifted(&rect, -250.6).left, -151);
        assert_eq!(super::shifted(&rect, -250.6).top, 20);
        assert_eq!(super::shifted(&rect, 9.0).right, 400);
    }

    #[test]
    fn a_picture_glides_from_where_it_was_to_where_it_goes_and_slows_as_it_gets_there() {
        let from = Rect {
            left: 0,
            top: 0,
            right: 400,
            bottom: 300,
        };
        let to = Rect {
            left: 1000,
            top: 100,
            right: 400,
            bottom: 300,
        };

        assert_eq!(super::glide_frame(&from, &to, 0.0), from);
        assert_eq!(super::glide_frame(&from, &to, 1.0), to);
        assert_eq!(super::glide_frame(&from, &to, 7.0), to, "no further");
        assert_eq!(super::glide_frame(&from, &to, -1.0), from);

        // more than half of the way in the first half of the time
        assert!(super::glide_frame(&from, &to, 0.5).left > 500);
        // and the size is not changed on the way when it is not to be
        assert_eq!(super::glide_frame(&from, &to, 0.3).right, 400);
    }

    #[test]
    fn a_window_is_near_a_work_area_within_a_width_of_it() {
        let area = Rect {
            left: 0,
            top: 0,
            right: 1000,
            bottom: 800,
        };
        let at = |left: i32, right: i32| Rect {
            left,
            top: 0,
            right: right - left,
            bottom: 800,
        };

        assert!(super::is_near(&at(0, 1000), &area));
        assert!(super::is_near(&at(900, 1900), &area), "just in view");
        assert!(super::is_near(&at(1900, 2900), &area), "a width away");
        assert!(!super::is_near(&at(2100, 3100), &area), "two widths away");
        assert!(super::is_near(&at(-1900, -900), &area));
        assert!(!super::is_near(&at(-3100, -2100), &area));
    }

    use super::*;

    fn rect(left: i32, top: i32, width: i32, height: i32) -> Rect {
        Rect {
            left,
            top,
            right: width,
            bottom: height,
        }
    }

    /// What `edges_moved` says, in words, as the edges and the sizing are not
    /// comparable
    fn moved(from: &Rect, to: &Rect) -> Vec<(&'static str, &'static str, i32)> {
        edges_moved(from, to)
            .into_iter()
            .map(|(edge, sizing, by)| {
                let edge = match edge {
                    OperationDirection::Left => "left",
                    OperationDirection::Right => "right",
                    OperationDirection::Up => "up",
                    OperationDirection::Down => "down",
                };
                let sizing = match sizing {
                    Sizing::Increase => "increase",
                    Sizing::Decrease => "decrease",
                };
                (edge, sizing, by)
            })
            .collect()
    }

    #[test]
    fn dragging_the_right_edge_out_moves_that_edge_alone() {
        let moved = moved(&rect(0, 0, 100, 50), &rect(0, 0, 130, 50));

        assert_eq!(moved, vec![("right", "increase", 60)]);
    }

    #[test]
    fn dragging_the_left_edge_in_shrinks_the_tile_there() {
        let moved = moved(&rect(0, 0, 100, 50), &rect(20, 0, 80, 50));

        assert_eq!(moved, vec![("left", "decrease", 40)]);
    }

    #[test]
    fn a_corner_moves_two_edges() {
        let moved = moved(&rect(10, 10, 100, 50), &rect(10, 10, 90, 70));

        assert_eq!(
            moved,
            vec![("right", "decrease", 20), ("down", "increase", 40)]
        );
    }

    #[test]
    fn a_window_that_is_where_it_was_moved_no_edge() {
        let at = rect(5, 5, 100, 50);
        assert!(moved(&at, &at).is_empty());
    }

    #[test]
    fn a_window_that_changed_size_was_resized_and_one_that_did_not_was_moved() {
        assert!(is_resized(&rect(0, 0, 100, 50), &rect(0, 0, 101, 50)));
        assert!(is_resized(&rect(0, 0, 100, 50), &rect(0, 0, 100, 49)));
        assert!(!is_resized(&rect(0, 0, 100, 50), &rect(40, 30, 100, 50)));
    }

    #[test]
    fn a_window_is_where_it_is_to_be_to_within_a_couple_of_pixels() {
        let layout = rect(100, 50, 400, 300);

        assert!(!is_off(&layout, &layout));
        assert!(!is_off(&rect(102, 48, 402, 298), &layout));
        // the 48 pixels that a window was found short of the end of a drag by
        assert!(is_off(&rect(100, 50, 352, 300), &layout));
        assert!(is_off(&rect(103, 50, 400, 300), &layout));
    }

    #[test]
    fn only_the_window_that_has_its_edge_dragged_is_left_alone() {
        let_go(DRAGGED.load(Ordering::SeqCst));
        assert!(!is_dragging(0));
        assert!(!is_dragging(7));

        taken_hold_of(7);
        assert!(is_dragging(7));
        assert!(!is_dragging(8));

        let_go(8);
        assert!(is_dragging(7));
        let_go(7);
        assert!(!is_dragging(7));
    }

    #[test]
    fn a_window_that_has_not_got_where_it_was_sent_is_left_for_the_next_step() {
        // there is no window with this handle, so it never gets anywhere
        let hwnd = 0x7fff_0001;
        let there = rect(0, 0, 100, 50);
        let elsewhere = rect(10, 0, 90, 50);

        SENT.lock().take();
        DEFERRED.store(false, Ordering::SeqCst);

        {
            let _steps = Snapping::new(true);

            // nothing was sent to it yet
            assert!(!is_catching_up(hwnd, &there));

            sent(hwnd, &there);
            assert!(is_catching_up(hwnd, &there));
            assert!(
                !DEFERRED.load(Ordering::SeqCst),
                "it is where it is to be sent"
            );

            assert!(is_catching_up(hwnd, &elsewhere));
            assert!(
                DEFERRED.load(Ordering::SeqCst),
                "it is to be sent elsewhere"
            );
        }

        // the end of a drag puts everything where it is to be
        let _end = Snapping::new(false);
        assert!(!is_catching_up(hwnd, &elsewhere));

        SENT.lock().take();
    }

    fn edges_rect(left: i32, top: i32, right: i32, bottom: i32) -> Rect {
        Rect {
            left,
            top,
            right: right - left,
            bottom: bottom - top,
        }
    }

    #[test]
    fn an_edge_dragged_before_the_start_was_handled_is_measured_from_the_tile() {
        let tile = edges_rect(27, 27, 623, 725);
        // read after the right edge had gone 7px
        let started_from = edges_rect(27, 27, 630, 725);
        let now = edges_rect(27, 27, 683, 725);

        assert_eq!(
            drag_origin(&started_from, &now, Some(&tile)),
            edges_rect(27, 27, 623, 725)
        );
    }

    #[test]
    fn an_edge_that_did_not_move_is_left_as_it_was_read() {
        // a window that cannot be as narrow as its tile, dragged by the other edge
        let tile = edges_rect(27, 27, 623, 725);
        let started_from = edges_rect(27, 27, 650, 725);
        let now = edges_rect(27, 27, 650, 760);

        assert_eq!(
            drag_origin(&started_from, &now, Some(&tile)),
            edges_rect(27, 27, 650, 725)
        );
    }

    #[test]
    fn an_edge_far_from_its_tile_is_left_as_it_was_read() {
        let tile = edges_rect(27, 27, 623, 725);
        let started_from = edges_rect(27, 27, 800, 725);
        let now = edges_rect(27, 27, 830, 725);

        assert_eq!(drag_origin(&started_from, &now, Some(&tile)), started_from);
    }

    #[test]
    fn there_is_nothing_to_go_by_without_a_tile() {
        let started_from = edges_rect(27, 27, 630, 725);
        let now = edges_rect(27, 27, 683, 725);

        assert_eq!(drag_origin(&started_from, &now, None), started_from);
    }

    #[test]
    fn windows_are_laid_out_in_place_for_as_long_as_the_guard_lives_on_its_thread_alone() {
        assert!(!is_in_place());

        {
            let _outer = InPlace::new();
            assert!(is_in_place());

            {
                let _inner = InPlace::new();
                assert!(is_in_place());
            }
            assert!(is_in_place(), "the outer guard is still alive");

            std::thread::spawn(|| assert!(!is_in_place()))
                .join()
                .unwrap();
        }

        assert!(!is_in_place());
    }
}
