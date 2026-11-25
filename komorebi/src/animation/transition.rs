//! The geometry of a workspace sliding in and out of a monitor.
//!
//! The windows of the workspace that is left slide out of the monitor, and the
//! windows of the workspace that is entered slide in from the other side, as
//! if the workspaces were next to each other on a strip and the monitor looked
//! at one place on it. Everything here is arithmetic on rects in virtual
//! screen coordinates, so that it can be tested without a screen.

use crate::animation::engine::AnimationEngine;
use crate::animation::ghost_dispatcher::GhostDispatcher;
use crate::core::OperationDirection;
use crate::core::Rect;
use lazy_static::lazy_static;
use parking_lot::Mutex;
use std::collections::HashMap;
use std::time::Duration;
use std::time::Instant;

/// Which side of the monitor the workspace that is entered comes in from. The
/// workspace that is left goes out on the other side.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SlideDirection {
    FromLeft,
    FromRight,
}

impl SlideDirection {
    /// The direction in which going from the workspace at `from` to the one at
    /// `to` is: to a workspace with a higher index is to the right, and to one
    /// with a lower index to the left. There is none for the same workspace.
    pub fn between(from: usize, to: usize) -> Option<Self> {
        match to.cmp(&from) {
            std::cmp::Ordering::Greater => Some(Self::FromRight),
            std::cmp::Ordering::Less => Some(Self::FromLeft),
            std::cmp::Ordering::Equal => None,
        }
    }

    /// The direction of going to the workspace in a direction, as when moving
    /// across the edge of a workspace to the next one, which is the same
    /// whichever of the two indices is the higher, so that going right from
    /// the last workspace to the first comes in from the right too.
    pub fn of_operation(direction: OperationDirection) -> Option<Self> {
        match direction {
            OperationDirection::Left => Some(Self::FromLeft),
            OperationDirection::Right => Some(Self::FromRight),
            OperationDirection::Up | OperationDirection::Down => None,
        }
    }

    /// How far, in pixels to the right, the windows of the workspace that is
    /// entered start from where they belong on a monitor `monitor_width` wide
    pub fn incoming_offset(self, monitor_width: i32) -> i32 {
        match self {
            Self::FromRight => monitor_width,
            Self::FromLeft => -monitor_width,
        }
    }

    /// How far, in pixels to the right, the windows of the workspace that is
    /// left end up from where they are
    pub fn outgoing_offset(self, monitor_width: i32) -> i32 {
        -self.incoming_offset(monitor_width)
    }
}

/// A rect moved `dx` to the right
pub fn shifted(rect: Rect, dx: i32) -> Rect {
    Rect {
        left: rect.left + dx,
        ..rect
    }
}

/// Where a ghost of `source` size, shown in `dest`, is to be drawn so that none
/// of it is outside `clip`: the part of `dest` that is inside, and the part of
/// the source that goes there, in pixels of the source. Nothing if no part of
/// it is inside.
///
/// A ghost is drawn on a host that covers every monitor, and a ghost that slides
/// out of a monitor would otherwise be drawn on the one next to it.
pub fn clip_thumbnail(dest: Rect, source: (i32, i32), clip: Rect) -> Option<(Rect, Rect)> {
    let (source_width, source_height) = source;

    if dest.right <= 0 || dest.bottom <= 0 || source_width <= 0 || source_height <= 0 {
        return None;
    }

    let left = dest.left.max(clip.left);
    let top = dest.top.max(clip.top);
    let right = (dest.left + dest.right).min(clip.left + clip.right);
    let bottom = (dest.top + dest.bottom).min(clip.top + clip.bottom);

    if right <= left || bottom <= top {
        return None;
    }

    // the part of the source that is under that part of the destination, which
    // is as much of it as the destination is stretched by
    let scale = |offset: i32, dest_size: i32, source_size: i32| -> i32 {
        let scaled = i64::from(offset) * i64::from(source_size);
        let divisor = i64::from(dest_size);
        // rounded to the nearest pixel
        ((scaled * 2 + divisor) / (divisor * 2)) as i32
    };

    let source_left = scale(left - dest.left, dest.right, source_width).clamp(0, source_width);
    let source_right = scale(right - dest.left, dest.right, source_width).clamp(0, source_width);
    let source_top = scale(top - dest.top, dest.bottom, source_height).clamp(0, source_height);
    let source_bottom =
        scale(bottom - dest.top, dest.bottom, source_height).clamp(0, source_height);

    if source_right <= source_left || source_bottom <= source_top {
        return None;
    }

    Some((
        Rect {
            left,
            top,
            right: right - left,
            bottom: bottom - top,
        },
        Rect {
            left: source_left,
            top: source_top,
            right: source_right - source_left,
            bottom: source_bottom - source_top,
        },
    ))
}

/// Whether any of `rect` is inside `clip`
fn intersects(rect: Rect, clip: Rect) -> bool {
    rect.left < clip.left + clip.right
        && rect.left + rect.right > clip.left
        && rect.top < clip.top + clip.bottom
        && rect.top + rect.bottom > clip.top
}

/// A window of the workspace that is left, and where it slides to
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Exit {
    pub hwnd: isize,
    pub from: Rect,
    pub to: Rect,
}

/// What switching to another workspace on a monitor is to slide
#[derive(Debug, Clone, PartialEq)]
pub struct Plan {
    pub direction: SlideDirection,
    /// The monitor, outside of which nothing of it is to be seen
    pub clip: Rect,
    pub exits: Vec<Exit>,
    /// The windows of the workspace that is entered, which are placed by the
    /// layout and slide in to where it puts them
    pub enters: Vec<isize>,
}

/// What to slide on the monitor `clip`, given the windows of the workspace that
/// is left and where they are, and the windows of the one that is entered.
///
/// A window that is not inside the monitor to begin with is not slid out of it:
/// it is not seen, and no ghost of it would be either. There is nothing to slide
/// when there are no windows at all.
pub fn plan(
    direction: SlideDirection,
    clip: Rect,
    outgoing: &[(isize, Rect)],
    incoming: &[isize],
) -> Option<Plan> {
    let offset = direction.outgoing_offset(clip.right);

    let exits: Vec<Exit> = outgoing
        .iter()
        .filter(|(_, rect)| intersects(*rect, clip))
        .map(|(hwnd, rect)| Exit {
            hwnd: *hwnd,
            from: *rect,
            to: shifted(*rect, offset),
        })
        .collect();

    if exits.is_empty() && incoming.is_empty() {
        return None;
    }

    Some(Plan {
        direction,
        clip,
        exits,
        enters: incoming.to_vec(),
    })
}

/// Start the workspace transition: the windows that slide out are put on
/// their animations, which hide them, and the ones that slide in wait for the
/// layout to put them in their places.
///
/// This is to be done before the workspaces are switched, so that hiding the
/// windows of the one that is left and showing those of the one that is entered
/// is left to the animations, and inside an animation batch, so that all of them
/// start together.
pub fn start(plan: &Plan) {
    let duration = crate::animation::workspace_transition_duration();
    let style = crate::animation::workspace_transition_style();

    tracing::debug!(
        "workspace transition: {} slide out and {} slide in from the {:?} of {:?}",
        plan.exits.len(),
        plan.enters.len(),
        plan.direction,
        plan.clip
    );

    for exit in &plan.exits {
        expect_exit(exit.hwnd);

        let slide = GhostDispatcher::slide_out(exit.hwnd, exit.from, exit.to, plan.clip, style);
        match AnimationEngine::animate(slide, duration) {
            // if it cannot be animated it is left for `finish_pending` to hide
            Ok(()) => claim(exit.hwnd),
            Err(error) => tracing::warn!(
                "workspace transition: failed to animate hwnd {} out: {error}",
                exit.hwnd
            ),
        }
    }

    for hwnd in &plan.enters {
        expect_enter(*hwnd, plan.direction, plan.clip);
    }
}

/// A window that has been told to slide in is laid out: the animation that
/// slides it in, from the side of the monitor it comes in from to where it is
/// put, if it is to be seen in the monitor at all
pub fn slide_in(
    window: crate::window::Window,
    layout: &Rect,
    top: bool,
) -> Option<color_eyre::eyre::Result<()>> {
    let (direction, clip) = take_enter(window.hwnd)?;
    tracing::debug!(
        "workspace transition: hwnd {} to {layout:?} in {clip:?}, inside it: {}",
        window.hwnd,
        intersects(*layout, clip)
    );

    // a window that is put somewhere that is not in the monitor, as a window of
    // a scrolling layout that is scrolled out of view is, is shown there at once
    if !intersects(*layout, clip) {
        finish_enter(window.hwnd);
        window.restore();
        return None;
    }

    let start = shifted(*layout, direction.incoming_offset(clip.right));

    // its border is not to be seen until the window is, so held from here on
    crate::border_manager::set_held(window.hwnd, true);

    let slide = GhostDispatcher::slide_in(
        window.hwnd,
        start,
        *layout,
        top,
        clip,
        crate::animation::workspace_transition_style(),
    );

    Some(AnimationEngine::animate(
        slide,
        crate::animation::workspace_transition_duration(),
    ))
}

/// What a window is about to be made to do by a workspace that is being
/// switched, which it is not to be made to do any other way meanwhile
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Pending {
    /// Slide out of the monitor, and be hidden when its ghost is up
    Exit,
    /// Slide into the monitor, and be shown when it has got there
    Enter {
        direction: SlideDirection,
        clip: Rect,
    },
    /// Sliding into the monitor, which has been taken over by its animation:
    /// the window is still not to be shown by anything else, as a layout of the
    /// workspace that is done meanwhile would, until the animation shows it
    Entering,
}

/// How long something that is pending can be: a transition that does not get
/// round to what it is waiting to do must not leave a window hidden, or
/// shown, because of it, so what is pending for longer than this is not.
const PENDING_LIFETIME: Duration = Duration::from_secs(2);

struct Entry {
    pending: Pending,
    since: Instant,
    /// Whether an animation that is to do it has been made, which is not
    /// finished by `finish_pending`: the animation does it when it starts
    claimed: bool,
}

#[derive(Default)]
struct Registry {
    entries: HashMap<isize, Entry>,
}

impl Registry {
    fn expect(&mut self, hwnd: isize, pending: Pending, now: Instant) {
        self.entries.insert(
            hwnd,
            Entry {
                pending,
                since: now,
                claimed: false,
            },
        );
    }

    fn claim(&mut self, hwnd: isize) {
        if let Some(entry) = self.entries.get_mut(&hwnd) {
            entry.claimed = true;
        }
    }

    fn current(&self, hwnd: isize, now: Instant) -> Option<Pending> {
        self.entries
            .get(&hwnd)
            .filter(|entry| now.saturating_duration_since(entry.since) < PENDING_LIFETIME)
            .map(|entry| entry.pending)
    }

    /// Take what is pending for the window if it is something `wanted`, and
    /// nothing otherwise: what is pending for it is anyone else's to take, and
    /// is left as it is. What has been pending for too long is not pending, and
    /// is got rid of whoever asks.
    fn take_if(
        &mut self,
        hwnd: isize,
        now: Instant,
        wanted: impl FnOnce(&Pending) -> bool,
    ) -> Option<Pending> {
        let Some(pending) = self.current(hwnd, now) else {
            self.entries.remove(&hwnd);
            return None;
        };

        if !wanted(&pending) {
            return None;
        }

        self.entries.remove(&hwnd);
        Some(pending)
    }

    /// Everything that is pending and has no animation to do it, which is no
    /// longer pending. What is claimed stays until the animation takes it, or
    /// it has been pending for too long.
    fn drain(&mut self, now: Instant) -> Vec<(isize, Pending)> {
        let mut unclaimed = vec![];

        self.entries.retain(|hwnd, entry| {
            if entry.claimed {
                return now.saturating_duration_since(entry.since) < PENDING_LIFETIME;
            }

            unclaimed.push((*hwnd, entry.pending));
            false
        });

        unclaimed
    }
}

lazy_static! {
    static ref PENDING: Mutex<Registry> = Mutex::new(Registry::default());
}

/// The window is to slide out of the monitor: hiding it is left to the
/// animation, which hides it when its ghost is up
pub fn expect_exit(hwnd: isize) {
    PENDING.lock().expect(hwnd, Pending::Exit, Instant::now());
}

/// The window is to slide into the monitor: showing it is left to the
/// animation, which shows it when it has got there
pub fn expect_enter(hwnd: isize, direction: SlideDirection, clip: Rect) {
    PENDING
        .lock()
        .expect(hwnd, Pending::Enter { direction, clip }, Instant::now());
}

/// An animation has been made that is to do what is pending for the window
pub fn claim(hwnd: isize) {
    PENDING.lock().claim(hwnd);
}

/// Whether hiding the window is left to the animation that is to slide it out
pub fn is_exiting(hwnd: isize) -> bool {
    matches!(
        PENDING.lock().current(hwnd, Instant::now()),
        Some(Pending::Exit)
    )
}

/// Whether showing the window is left to the animation that is to slide it in
pub fn is_entering(hwnd: isize) -> bool {
    matches!(
        PENDING.lock().current(hwnd, Instant::now()),
        Some(Pending::Enter { .. } | Pending::Entering)
    )
}

/// Whether the window is on its way into the monitor, with its animation
pub fn is_sliding_in(hwnd: isize) -> bool {
    matches!(
        PENDING.lock().current(hwnd, Instant::now()),
        Some(Pending::Entering)
    )
}

/// The animation takes over hiding the window: say whether it was to
pub fn take_exit(hwnd: isize) -> bool {
    PENDING
        .lock()
        .take_if(hwnd, Instant::now(), |pending| {
            matches!(pending, Pending::Exit)
        })
        .is_some()
}

/// The animation takes over showing the window: which way it is to slide in
/// from, and what it is not to be seen outside of
///
/// Showing the window is left to the animation from here on, until it calls
/// `finish_enter`: a window is not shown by the layout of its workspace that
/// is done while it slides in, which is done for every switch of workspaces
/// when the focus change that follows it is handled.
pub fn take_enter(hwnd: isize) -> Option<(SlideDirection, Rect)> {
    let now = Instant::now();
    let mut pending = PENDING.lock();

    // only a window that is yet to slide in is taken: one that is already doing
    // so is left sliding in, however many times the layout that follows the
    // switch asks
    match pending.take_if(hwnd, now, |pending| {
        matches!(pending, Pending::Enter { .. })
    }) {
        Some(Pending::Enter { direction, clip }) => {
            pending.expect(hwnd, Pending::Entering, now);
            pending.claim(hwnd);
            Some((direction, clip))
        }
        _ => None,
    }
}

/// The animation of a window that slides in is about to show it
pub fn finish_enter(hwnd: isize) {
    PENDING.lock().take_if(hwnd, Instant::now(), |pending| {
        matches!(pending, Pending::Enter { .. } | Pending::Entering)
    });
}

/// Do now what was left to no animation: hide the windows that were to slide
/// out and show the ones that were to slide in
pub fn finish_pending() {
    // out of the registry first, or what is done would be left to the animation
    let pending = PENDING.lock().drain(Instant::now());

    for (hwnd, pending) in pending {
        let window = crate::window::Window { hwnd };
        match pending {
            Pending::Exit => window.hide(),
            Pending::Enter { .. } | Pending::Entering => {
                window.restore();
                focus_if_asked(hwnd);
            }
        }
    }
}

/// Windows that were asked to be focused while they were hidden, and whether the
/// cursor was to follow, for them to be when they are shown.
///
/// Focusing a window that is cloaked shows it, wherever it is to be, which is
/// the last thing that is to happen to one that has yet to slide in.
#[derive(Default)]
struct FocusLater {
    asked: HashMap<isize, (bool, Instant)>,
}

impl FocusLater {
    fn ask(&mut self, hwnd: isize, mouse_follows_focus: bool, now: Instant) {
        self.asked.insert(hwnd, (mouse_follows_focus, now));
    }

    /// Whether the window was asked to be focused, and whether the cursor was to
    /// follow. What was asked too long ago is not wanted any more, and the window
    /// is not focused out of the blue because of it.
    fn take(&mut self, hwnd: isize, now: Instant) -> Option<bool> {
        self.asked
            .retain(|_, (_, since)| now.saturating_duration_since(*since) < PENDING_LIFETIME);

        self.asked
            .remove(&hwnd)
            .map(|(mouse_follows_focus, _)| mouse_follows_focus)
    }
}

lazy_static! {
    static ref FOCUS_LATER: Mutex<FocusLater> = Mutex::new(FocusLater::default());
}

/// The window is to be focused, but is hidden and is to be shown by an
/// animation: it is focused when it is
pub fn focus_when_shown(hwnd: isize, mouse_follows_focus: bool) {
    FOCUS_LATER
        .lock()
        .ask(hwnd, mouse_follows_focus, Instant::now());
}

/// The window that was asked to be focused when it is shown is, if it was
pub fn focus_if_asked(hwnd: isize) {
    let asked = FOCUS_LATER.lock().take(hwnd, Instant::now());

    if let Some(mouse_follows_focus) = asked
        && let Err(error) = (crate::window::Window { hwnd }).focus(mouse_follows_focus)
    {
        tracing::warn!("workspace transition: failed to focus hwnd {hwnd}: {error}");
    }
}

/// Finishes what is pending when it goes out of scope, however that is
pub struct PendingGuard;

impl Drop for PendingGuard {
    fn drop(&mut self) {
        finish_pending();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rect(left: i32, top: i32, width: i32, height: i32) -> Rect {
        Rect {
            left,
            top,
            right: width,
            bottom: height,
        }
    }

    #[test]
    fn going_to_a_higher_workspace_is_to_the_right() {
        assert_eq!(
            SlideDirection::between(0, 1),
            Some(SlideDirection::FromRight)
        );
        assert_eq!(
            SlideDirection::between(1, 4),
            Some(SlideDirection::FromRight)
        );
        assert_eq!(
            SlideDirection::between(3, 2),
            Some(SlideDirection::FromLeft)
        );
        assert_eq!(
            SlideDirection::between(2, 0),
            Some(SlideDirection::FromLeft)
        );
        assert_eq!(SlideDirection::between(2, 2), None);
    }

    #[test]
    fn moving_across_an_edge_slides_the_way_it_moves() {
        assert_eq!(
            SlideDirection::of_operation(OperationDirection::Right),
            Some(SlideDirection::FromRight)
        );
        assert_eq!(
            SlideDirection::of_operation(OperationDirection::Left),
            Some(SlideDirection::FromLeft)
        );
        assert_eq!(SlideDirection::of_operation(OperationDirection::Up), None);
        assert_eq!(SlideDirection::of_operation(OperationDirection::Down), None);
    }

    #[test]
    fn the_two_workspaces_slide_opposite_ways_by_a_monitor_width() {
        let width = 5120;

        assert_eq!(SlideDirection::FromRight.incoming_offset(width), width);
        assert_eq!(SlideDirection::FromRight.outgoing_offset(width), -width);
        assert_eq!(SlideDirection::FromLeft.incoming_offset(width), -width);
        assert_eq!(SlideDirection::FromLeft.outgoing_offset(width), width);

        // the window at the left edge of a monitor that slides out to the left
        // ends up entirely outside it, and so does the one at the right edge
        let monitor = rect(0, 0, width, 2160);
        for window in [rect(0, 0, 1000, 2160), rect(width - 1000, 0, 1000, 2160)] {
            let gone = shifted(window, SlideDirection::FromRight.outgoing_offset(width));
            assert!(gone.left + gone.right <= monitor.left);
        }
    }

    #[test]
    fn a_ghost_inside_the_clip_is_not_cropped() {
        let (dest, source) =
            clip_thumbnail(rect(100, 50, 400, 300), (800, 600), rect(0, 0, 1920, 1080)).unwrap();

        assert_eq!(dest, rect(100, 50, 400, 300));
        assert_eq!(source, rect(0, 0, 800, 600));
    }

    #[test]
    fn a_ghost_that_leaves_on_the_left_is_cropped_on_the_left() {
        // half of it (200 of 400) is left of the monitor, at the same scale
        let (dest, source) =
            clip_thumbnail(rect(-200, 0, 400, 300), (400, 300), rect(0, 0, 1920, 1080)).unwrap();

        assert_eq!(dest, rect(0, 0, 200, 300));
        assert_eq!(source, rect(200, 0, 200, 300));
    }

    #[test]
    fn a_ghost_that_leaves_on_the_right_is_cropped_on_the_right() {
        let (dest, source) =
            clip_thumbnail(rect(1720, 0, 400, 300), (400, 300), rect(0, 0, 1920, 1080)).unwrap();

        assert_eq!(dest, rect(1720, 0, 200, 300));
        assert_eq!(source, rect(0, 0, 200, 300));
    }

    #[test]
    fn a_ghost_wider_than_the_clip_is_cropped_on_both_sides() {
        let (dest, source) = clip_thumbnail(
            rect(-100, 0, 2120, 100),
            (2120, 100),
            rect(0, 0, 1920, 1080),
        )
        .unwrap();

        assert_eq!(dest, rect(0, 0, 1920, 100));
        assert_eq!(source, rect(100, 0, 1920, 100));
    }

    #[test]
    fn the_part_of_the_source_follows_the_stretch_of_the_ghost() {
        // a source of 1000 pixels shown 500 wide, so each pixel is 2 of the source
        let (dest, source) =
            clip_thumbnail(rect(-100, 0, 500, 100), (1000, 200), rect(0, 0, 1920, 1080)).unwrap();

        assert_eq!(dest, rect(0, 0, 400, 100));
        assert_eq!(source, rect(200, 0, 800, 200));

        // and a source shown larger than it is
        let (dest, source) =
            clip_thumbnail(rect(-50, 0, 200, 100), (100, 50), rect(0, 0, 1920, 1080)).unwrap();

        assert_eq!(dest, rect(0, 0, 150, 100));
        assert_eq!(source, rect(25, 0, 75, 50));
    }

    #[test]
    fn a_ghost_cropped_vertically_keeps_the_matching_part_of_the_source() {
        let (dest, source) =
            clip_thumbnail(rect(0, -100, 400, 400), (400, 400), rect(0, 0, 1920, 1080)).unwrap();

        assert_eq!(dest, rect(0, 0, 400, 300));
        assert_eq!(source, rect(0, 100, 400, 300));
    }

    #[test]
    fn a_ghost_outside_the_clip_is_not_drawn() {
        let clip = rect(0, 0, 1920, 1080);

        // all of it to the left, all of it to the right, touching the edge
        assert!(clip_thumbnail(rect(-500, 0, 400, 300), (400, 300), clip).is_none());
        assert!(clip_thumbnail(rect(2000, 0, 400, 300), (400, 300), clip).is_none());
        assert!(clip_thumbnail(rect(-400, 0, 400, 300), (400, 300), clip).is_none());
        assert!(clip_thumbnail(rect(1920, 0, 400, 300), (400, 300), clip).is_none());

        // above and below
        assert!(clip_thumbnail(rect(0, -400, 400, 300), (400, 300), clip).is_none());
        assert!(clip_thumbnail(rect(0, 1080, 400, 300), (400, 300), clip).is_none());
    }

    #[test]
    fn nothing_without_a_size_is_drawn() {
        let clip = rect(0, 0, 1920, 1080);

        assert!(clip_thumbnail(rect(0, 0, 0, 300), (400, 300), clip).is_none());
        assert!(clip_thumbnail(rect(0, 0, 400, 0), (400, 300), clip).is_none());
        assert!(clip_thumbnail(rect(0, 0, 400, 300), (0, 300), clip).is_none());
        assert!(clip_thumbnail(rect(0, 0, 400, 300), (400, 0), clip).is_none());
    }

    #[test]
    fn a_monitor_to_the_left_of_the_primary_one_clips_in_its_own_coordinates() {
        // the monitor is at -1920, and the ghost slides out of it to the right,
        // towards the primary monitor, which is not where it is to be drawn
        let monitor = rect(-1920, 0, 1920, 1080);

        let (dest, source) = clip_thumbnail(rect(-300, 0, 600, 300), (600, 300), monitor).unwrap();

        assert_eq!(dest, rect(-300, 0, 300, 300));
        assert_eq!(source, rect(0, 0, 300, 300));

        // and the one that has slid right out of it
        assert!(clip_thumbnail(rect(0, 0, 600, 300), (600, 300), monitor).is_none());
    }

    #[test]
    fn what_is_drawn_never_goes_outside_the_clip_or_the_source() {
        let clip = rect(-1920, 100, 3840, 900);

        for left in (-6000..6000).step_by(137) {
            for (width, source_width) in [(100, 400), (700, 300), (1920, 1920), (4000, 123)] {
                let dest = rect(left, 300, width, 400);

                if let Some((drawn, part)) = clip_thumbnail(dest, (source_width, 90), clip) {
                    assert!(drawn.left >= clip.left);
                    assert!(drawn.left + drawn.right <= clip.left + clip.right);
                    assert!(drawn.top >= clip.top);
                    assert!(drawn.top + drawn.bottom <= clip.top + clip.bottom);
                    assert!(part.left >= 0 && part.top >= 0);
                    assert!(part.left + part.right <= source_width);
                    assert!(part.top + part.bottom <= 90);
                    assert!(drawn.right > 0 && drawn.bottom > 0);
                    assert!(part.right > 0 && part.bottom > 0);
                }
            }
        }
    }

    #[test]
    fn something_pending_is_pending_until_it_is_taken() {
        let mut registry = Registry::default();
        let now = Instant::now();
        let enter = Pending::Enter {
            direction: SlideDirection::FromRight,
            clip: rect(0, 0, 1920, 1080),
        };

        registry.expect(1, Pending::Exit, now);
        registry.expect(2, enter, now);

        assert_eq!(registry.current(1, now), Some(Pending::Exit));
        assert_eq!(registry.current(2, now), Some(enter));
        assert_eq!(registry.current(3, now), None);

        // looking does not take it
        assert_eq!(registry.current(2, now), Some(enter));

        assert_eq!(registry.take_if(2, now, |_| true), Some(enter));
        assert_eq!(registry.take_if(2, now, |_| true), None);
        assert_eq!(registry.current(2, now), None);

        // the other is where it was
        assert_eq!(registry.current(1, now), Some(Pending::Exit));
    }

    #[test]
    fn something_pending_for_too_long_is_not_pending() {
        let mut registry = Registry::default();
        let then = Instant::now();

        registry.expect(1, Pending::Exit, then);

        let almost = then + PENDING_LIFETIME - Duration::from_millis(1);
        let late = then + PENDING_LIFETIME;

        assert_eq!(registry.current(1, almost), Some(Pending::Exit));
        assert_eq!(registry.current(1, late), None);

        // and taking it gets nothing, and gets rid of it
        assert_eq!(registry.take_if(1, late, |_| true), None);
        assert!(registry.drain(late).is_empty());
    }

    #[test]
    fn expecting_again_starts_the_wait_again() {
        let mut registry = Registry::default();
        let then = Instant::now();
        let later = then + Duration::from_secs(1);

        registry.expect(1, Pending::Exit, then);
        registry.expect(1, Pending::Exit, later);

        assert_eq!(
            registry.current(1, later + Duration::from_millis(1500)),
            Some(Pending::Exit)
        );
    }

    #[test]
    fn draining_gives_everything_that_is_pending_once() {
        let mut registry = Registry::default();
        let now = Instant::now();

        registry.expect(1, Pending::Exit, now);
        registry.expect(2, Pending::Exit, now);

        let mut drained = registry.drain(now);
        drained.sort_by_key(|(hwnd, _)| *hwnd);

        assert_eq!(drained, vec![(1, Pending::Exit), (2, Pending::Exit)]);
        assert!(registry.drain(now).is_empty());
        assert_eq!(registry.current(1, now), None);
    }

    #[test]
    fn what_has_an_animation_to_do_it_is_left_pending_when_the_rest_is_finished() {
        let mut registry = Registry::default();
        let then = Instant::now();

        registry.expect(1, Pending::Exit, then);
        registry.expect(2, Pending::Exit, then);
        registry.claim(2);
        // nothing to claim is fine
        registry.claim(3);

        assert_eq!(registry.drain(then), vec![(1, Pending::Exit)]);
        assert_eq!(registry.current(2, then), Some(Pending::Exit));

        // until the animation takes it
        assert_eq!(registry.take_if(2, then, |_| true), Some(Pending::Exit));
        assert_eq!(registry.current(2, then), None);

        // or it was not taken for too long, which is the end of it
        registry.expect(4, Pending::Exit, then);
        registry.claim(4);
        assert!(registry.drain(then + PENDING_LIFETIME).is_empty());
        assert!(registry.entries.is_empty());
    }

    #[test]
    fn what_is_pending_for_a_window_is_taken_by_whoever_it_is_for() {
        // windows nothing else in the tests uses
        let (leaving, entering) = (0x7e57_1001, 0x7e57_1002);
        let clip = rect(-1920, 0, 1920, 1080);

        expect_exit(leaving);
        expect_enter(entering, SlideDirection::FromLeft, clip);

        assert!(is_exiting(leaving));
        assert!(!is_entering(leaving));
        assert!(is_entering(entering));
        assert!(!is_exiting(entering));

        assert!(take_exit(leaving));
        assert!(!is_exiting(leaving));
        assert_eq!(take_enter(entering), Some((SlideDirection::FromLeft, clip)));
        finish_enter(entering);
        assert!(!is_entering(entering));
    }

    #[test]
    fn windows_slide_out_to_the_side_opposite_to_the_one_the_others_come_in_from() {
        let monitor = rect(0, 0, 1920, 1080);
        let window = rect(100, 50, 800, 900);

        let plan = plan(SlideDirection::FromRight, monitor, &[(1, window)], &[2]).unwrap();

        assert_eq!(plan.enters, vec![2]);
        assert_eq!(
            plan.exits,
            vec![Exit {
                hwnd: 1,
                from: window,
                to: rect(100 - 1920, 50, 800, 900),
            }]
        );

        let plan = self::plan(SlideDirection::FromLeft, monitor, &[(1, window)], &[2]).unwrap();
        assert_eq!(plan.exits[0].to, rect(100 + 1920, 50, 800, 900));
    }

    #[test]
    fn what_slides_out_of_a_monitor_is_measured_from_where_the_monitor_is() {
        // a monitor to the left of the primary one, which a window slides out
        // of by the width of the monitor, not by its own distance from zero
        let monitor = rect(-1280, 0, 1280, 1024);
        let window = rect(-1000, 0, 500, 1000);

        let plan = plan(SlideDirection::FromRight, monitor, &[(1, window)], &[]).unwrap();

        assert_eq!(plan.exits[0].to, rect(-1000 - 1280, 0, 500, 1000));
        assert_eq!(plan.clip, monitor);
    }

    #[test]
    fn a_window_that_is_not_in_the_monitor_is_not_slid_out_of_it() {
        let monitor = rect(0, 0, 1920, 1080);
        let on_show = rect(0, 0, 960, 1080);
        // scrolled out of view, right of the monitor, and on the monitor next to it
        let scrolled = rect(1920, 0, 960, 1080);
        let next_door = rect(-960, 0, 960, 1080);
        // only just in
        let edge = rect(1919, 0, 960, 1080);

        let plan = plan(
            SlideDirection::FromRight,
            monitor,
            &[(1, on_show), (2, scrolled), (3, next_door), (4, edge)],
            &[9],
        )
        .unwrap();

        let slid: Vec<isize> = plan.exits.iter().map(|exit| exit.hwnd).collect();
        assert_eq!(slid, vec![1, 4]);
    }

    #[test]
    fn there_is_nothing_to_slide_without_windows() {
        let monitor = rect(0, 0, 1920, 1080);

        assert_eq!(plan(SlideDirection::FromRight, monitor, &[], &[]), None);

        // only windows coming in is a plan, and so is only windows going out
        assert!(plan(SlideDirection::FromRight, monitor, &[], &[1]).is_some());
        assert!(
            plan(
                SlideDirection::FromRight,
                monitor,
                &[(1, rect(0, 0, 10, 10))],
                &[]
            )
            .is_some()
        );
    }

    #[test]
    fn a_window_that_slides_in_is_not_shown_by_anything_else_until_its_animation_does() {
        // a window nothing else in the tests uses
        let hwnd = 0x7e57_1003;
        let clip = rect(0, 0, 1920, 1080);

        expect_enter(hwnd, SlideDirection::FromRight, clip);
        assert!(is_entering(hwnd));
        assert!(!is_sliding_in(hwnd));

        assert_eq!(take_enter(hwnd), Some((SlideDirection::FromRight, clip)));

        // the layout that is done after the switch finds it still hidden
        assert!(is_entering(hwnd));
        assert!(is_sliding_in(hwnd));
        // and it is not slid in twice
        assert_eq!(take_enter(hwnd), None);

        finish_enter(hwnd);
        assert!(!is_entering(hwnd));
        assert!(!is_sliding_in(hwnd));
    }

    #[test]
    fn a_window_that_is_sliding_in_has_its_animation_to_show_it_not_finish_pending() {
        let mut registry = Registry::default();
        let now = Instant::now();

        registry.expect(1, Pending::Entering, now);
        registry.claim(1);

        assert!(registry.drain(now).is_empty());
        assert_eq!(registry.current(1, now), Some(Pending::Entering));

        // it is not for ever, if its animation never gets to it
        assert_eq!(registry.current(1, now + PENDING_LIFETIME), None);
    }

    #[test]
    fn taking_what_is_not_wanted_leaves_it_pending() {
        let mut registry = Registry::default();
        let now = Instant::now();

        registry.expect(1, Pending::Entering, now);
        registry.claim(1);

        // asked for by something that is after another state
        assert_eq!(
            registry.take_if(1, now, |pending| matches!(pending, Pending::Exit)),
            None
        );
        assert_eq!(
            registry.take_if(1, now, |pending| matches!(pending, Pending::Enter { .. })),
            None
        );
        assert_eq!(registry.current(1, now), Some(Pending::Entering));

        // and by what is after it
        assert_eq!(
            registry.take_if(1, now, |pending| matches!(pending, Pending::Entering)),
            Some(Pending::Entering)
        );
        assert_eq!(registry.current(1, now), None);
    }

    #[test]
    fn what_is_pending_for_too_long_is_got_rid_of_by_anyone_asking() {
        let mut registry = Registry::default();
        let then = Instant::now();

        registry.expect(1, Pending::Entering, then);

        let late = then + PENDING_LIFETIME;
        assert_eq!(registry.take_if(1, late, |_| false), None);
        assert!(registry.entries.is_empty());
    }

    #[test]
    fn a_window_stays_swallowed_however_many_layouts_look_at_it_while_it_slides_in() {
        // a window nothing else in the tests uses
        let hwnd = 0x7e57_1004;
        let clip = rect(0, 0, 1920, 1080);

        expect_enter(hwnd, SlideDirection::FromRight, clip);

        // the layout of the switch takes it, and the layouts of the focus changes
        // that follow it find it on its way in
        assert!(take_enter(hwnd).is_some());
        for _ in 0..3 {
            assert_eq!(take_enter(hwnd), None);
            assert!(!take_exit(hwnd));
            assert!(is_entering(hwnd));
            assert!(is_sliding_in(hwnd));
        }

        // until its animation shows it
        finish_enter(hwnd);
        assert!(!is_entering(hwnd));
        assert!(!is_sliding_in(hwnd));
    }

    #[test]
    fn a_window_asked_to_be_focused_while_hidden_is_focused_once_when_shown() {
        let mut later = FocusLater::default();
        let now = Instant::now();

        later.ask(1, true, now);
        later.ask(2, false, now);

        assert_eq!(later.take(1, now), Some(true));
        assert_eq!(later.take(1, now), None, "once");
        assert_eq!(later.take(2, now), Some(false));
        assert_eq!(later.take(3, now), None);
    }

    #[test]
    fn a_window_is_not_focused_for_what_was_asked_for_it_long_ago() {
        let mut later = FocusLater::default();
        let then = Instant::now();

        later.ask(1, false, then);

        assert_eq!(later.take(1, then + PENDING_LIFETIME), None);
        assert!(later.asked.is_empty());
    }

    #[test]
    fn asking_again_is_what_counts() {
        let mut later = FocusLater::default();
        let then = Instant::now();

        later.ask(1, false, then);
        later.ask(1, true, then + Duration::from_secs(1));

        assert_eq!(
            later.take(1, then + Duration::from_millis(2500)),
            Some(true)
        );
    }
}
