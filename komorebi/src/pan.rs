//! Moving the strip of the Scrolling layout across the screen without changing which window has
//! focus: by a step, and with the fingers on a touchpad.
//!
//! A gesture is a begin, any number of moves, and an end, or a cancel. The strip follows the
//! fingers for as long as they are down, and the workspace is laid out a step at a time as the
//! windows of a drag are, with the pictures that cover them, and when the fingers lift it
//! comes to rest with a column against an edge of the screen.

use crate::core::PanDirection;
use crate::core::PanGesture;
use crate::core::Rect;
use crate::live_resize;
use crate::live_resize::Snapping;
use crate::window_manager::WindowManager;
use crate::windows_api::WindowsApi;
use color_eyre::eyre;
use komorebi_scrolling::ScrollingSpace;
use komorebi_scrolling::SwipeTracker;
use parking_lot::Mutex;
use std::time::Instant;

/// How far the fingers go across a touchpad, in centimetres, to move the strip as far as the
/// width of the screen
const CM_PER_SCREEN: f64 = 10.0;

/// A gesture that is going on
struct Pan {
    monitor_idx: usize,
    workspace_idx: usize,
    /// What the strip was before the gesture, for putting it back if it is given up
    saved: ScrollingSpace<String>,
    /// How fast the fingers are going
    tracker: SwipeTracker,
    /// How far into the strip the screen was when it began
    offset: f64,
    /// When it began, and the time of the touchpad at the first move, for the log
    began: Instant,
    first_t: Option<f64>,
}

static PAN: Mutex<Option<Pan>> = Mutex::new(None);

/// How far the strip is moved on a screen that is `screen_width` pixels wide by fingers that
/// move `cm` centimetres, in pixels, to the right if it is positive
fn distance_on_screen(cm: f64, screen_width: i32) -> f64 {
    cm * f64::from(screen_width) / CM_PER_SCREEN
}

impl WindowManager {
    /// Move the view of the focused workspace one step, a column, to the left or the right, and
    /// let it come to rest with a column against an edge of the screen. Focus goes to the nearest column in view if the focused one is
    /// not. Nothing happens in a layout that is not Scrolling.
    #[tracing::instrument(skip(self))]
    pub fn pan(&mut self, direction: PanDirection) -> eyre::Result<()> {
        let workspace = self.focused_workspace_mut()?;

        if workspace.begin_scrolling_pan().is_none() {
            return Ok(());
        }

        let step = workspace
            .scrolling_pan_step()
            .and_then(|step| u32::try_from(step).ok())
            .unwrap_or_default();

        workspace.pan_scrolling(direction.strip_offset(step));
        let focus_moved = workspace.end_scrolling_pan(0.0);

        self.update_focused_workspace(focus_moved && self.mouse_follows_focus, focus_moved)
    }

    /// What the fingers of a gesture do
    pub fn pan_gesture(&mut self, gesture: PanGesture) -> eyre::Result<()> {
        match gesture {
            PanGesture::Begin => self.pan_begin(),
            PanGesture::Move { dx, t, .. } => self.pan_move(dx, t),
            PanGesture::End => self.pan_end(),
            PanGesture::Cancel => self.pan_cancel(),
        }
    }

    fn pan_begin(&mut self) -> eyre::Result<()> {
        // a gesture that was not finished is given up, as a new one begins
        self.pan_cancel()?;

        let monitor_idx = self.focused_monitor_idx();
        let workspace_idx = self.focused_workspace_idx()?;

        let Some(saved) = self.focused_workspace_mut()?.begin_scrolling_pan() else {
            return Ok(());
        };

        live_resize::begin_steps();

        // the pictures that the strip is moved by are taken now, all at once, and not as the
        // first move of the fingers asks for them, one after the other
        let work_area = self.focused_monitor_work_area()?;
        let workspace = self.focused_workspace()?;
        let near: Vec<_> = origins(&workspace.scrolling_lefts(), |hwnd| {
            WindowsApi::window_rect(hwnd).ok()
        })
        .into_iter()
        .filter(|(_, origin)| live_resize::is_near(origin, &work_area))
        .collect();
        let offset = workspace.scrolling_view_offset().unwrap_or_default();
        live_resize::cover_all(&near);

        *PAN.lock() = Some(Pan {
            monitor_idx,
            workspace_idx,
            saved,
            tracker: SwipeTracker::new(),
            offset,
            began: Instant::now(),
            first_t: None,
        });

        Ok(())
    }

    fn pan_move(&mut self, dx_cm: f64, t_ms: f64) -> eyre::Result<()> {
        let mut guard = PAN.lock();
        let Some(pan) = guard.as_mut() else {
            return Ok(());
        };

        // the workspace that the fingers are on is not the one that is on show any more
        if self.focused_monitor_idx() != pan.monitor_idx
            || self.focused_workspace_idx()? != pan.workspace_idx
        {
            drop(guard);
            return self.pan_cancel();
        }

        let workspace = self.focused_workspace_mut()?;
        let Some(width) = workspace.scrolling_view_width() else {
            return Ok(());
        };

        let dx = distance_on_screen(dx_cm, width);
        workspace.pan_scrolling(dx);
        pan.tracker.push(t_ms / 1000.0, dx);

        // how far behind the fingers a move is handled is how far the wall clock has got ahead
        // of the touchpad's since the first
        let first = *pan.first_t.get_or_insert(t_ms);
        let fingers = t_ms - first;
        let wall = pan.began.elapsed().as_secs_f64() * 1000.0;
        tracing::trace!(
            "pan: move of {dx:.1} px at {fingers:.0} ms of the fingers, handled at {wall:.0} ms ({:+.0} ms)",
            wall - fingers
        );
        let offset = workspace.scrolling_view_offset().unwrap_or(pan.offset);
        let shift = pan.offset - offset;
        drop(guard);

        // the windows are under their pictures, and it is the pictures that go with the fingers
        if live_resize::shift_covers(shift) {
            return Ok(());
        }

        // without them the workspace is laid out a step at a time
        let _snapping = Snapping::new(true);
        self.update_focused_workspace(false, false)
    }

    fn pan_end(&mut self) -> eyre::Result<()> {
        let Some(pan) = PAN.lock().take() else {
            return Ok(());
        };

        if self.focused_monitor_idx() != pan.monitor_idx
            || self.focused_workspace_idx()? != pan.workspace_idx
        {
            return Ok(());
        }

        let focus_moved = self
            .focused_workspace_mut()?
            .end_scrolling_pan(pan.tracker.velocity());

        // the end of it is laid out at once, and the windows that were covered are shown when
        // this is dropped, but the pictures of them are not sent to where they go: they glide
        // there, from where the fingers left them, as the windows of an animation do
        let _snapping = Snapping::new(false);
        {
            let _gliding = live_resize::Gliding::begin();
            self.update_focused_workspace(focus_moved && self.mouse_follows_focus, focus_moved)?;
        }

        live_resize::glide();
        Ok(())
    }

    fn pan_cancel(&mut self) -> eyre::Result<()> {
        let Some(pan) = PAN.lock().take() else {
            return Ok(());
        };

        if self.focused_monitor_idx() != pan.monitor_idx
            || self.focused_workspace_idx()? != pan.workspace_idx
        {
            return Ok(());
        }

        self.focused_workspace_mut()?
            .cancel_scrolling_pan(pan.saved);

        let _snapping = Snapping::new(false);
        {
            let _gliding = live_resize::Gliding::begin();
            self.update_focused_workspace(false, false)?;
        }

        live_resize::glide();
        Ok(())
    }
}

/// Where the pictures of the windows go when the strip has not been moved: each window's own
/// rectangle, with its left edge where the strip puts it. The strip does not say where the
/// screen is, so the windows that are where it puts them (the ones on the screen) say that, and
/// the rest, which are parked at an edge, are put where they would be.
///
/// `lefts` is each window with how far its left edge is from that of the screen, and whether
/// it is on the screen, and `rect_of` is where a window is. Nothing is covered if no window is
/// where it is put.
fn origins(
    lefts: &[(isize, i32, bool)],
    rect_of: impl Fn(isize) -> Option<Rect>,
) -> Vec<(isize, Rect)> {
    let Some(offset) = lefts
        .iter()
        .filter(|(_, _, on_screen)| *on_screen)
        .find_map(|(hwnd, left, _)| Some(rect_of(*hwnd)?.left - left))
    else {
        return vec![];
    };

    lefts
        .iter()
        .filter_map(|(hwnd, left, _)| {
            let rect = rect_of(*hwnd)?;
            Some((
                *hwnd,
                Rect {
                    left: left + offset,
                    ..rect
                },
            ))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::SocketMessage;

    #[test]
    fn pictures_go_where_the_strip_puts_the_windows_and_parked_windows_are_not_where_they_are() {
        let at = |left| Rect {
            left,
            top: 30,
            right: 400,
            bottom: 300,
        };
        // the strip: one window past the left edge (parked, so not where it is put), two on the
        // screen, which starts at 10, and one past the right edge
        let lefts = [
            (1, -300, false),
            (2, 50, true),
            (3, 450, true),
            (4, 900, false),
        ];
        let rects = |hwnd| match hwnd {
            1 => Some(at(-385)),
            2 => Some(at(60)),
            3 => Some(at(460)),
            4 => Some(at(1205)),
            _ => None,
        };

        let origins = origins(&lefts, rects);
        let lefts_of: Vec<_> = origins
            .iter()
            .map(|(hwnd, rect)| (*hwnd, rect.left))
            .collect();

        assert_eq!(lefts_of, vec![(1, -290), (2, 60), (3, 460), (4, 910)]);
        // and nothing else of where they are changes
        assert!(
            origins
                .iter()
                .all(|(_, rect)| rect.top == 30 && rect.right == 400)
        );
    }

    #[test]
    fn nothing_is_covered_when_no_window_is_where_it_is_put() {
        let lefts = [(1, -300, false), (2, 900, false)];
        assert!(origins(&lefts, |_| Some(Rect::default())).is_empty());
        // or when the windows cannot be found
        assert!(origins(&[(1, 0, true)], |_| None).is_empty());
    }

    #[test]
    fn fingers_that_go_across_a_touchpad_move_the_strip_across_the_screen() {
        assert!((distance_on_screen(10.0, 5000) - 5000.0).abs() < 1e-9);
        assert!((distance_on_screen(1.0, 5000) - 500.0).abs() < 1e-9);
        assert!((distance_on_screen(-0.5, 1200) + 60.0).abs() < 1e-9);
        assert!(distance_on_screen(0.0, 1200).abs() < f64::EPSILON);
    }

    /// The moves of a swipe of about two seconds at about 125 times a second, with a time that
    /// is a millisecond or two out for each, as a touchpad gives them, the fingers going at
    /// `lift` centimetres a second for the last 200 milliseconds and at `earlier` before that
    /// (eased in from rest). Each is how far they went since the one before, and when.
    fn swipe(earlier: f64, lift: f64) -> Vec<(f64, f64)> {
        let mut moves = Vec::new();
        let mut t = 118_000.0;

        for step in 1..=250 {
            let jitter = f64::from((step * 37) % 7 - 3) * 0.5;
            let next = 118_000.0 + f64::from(step) * 8.0 + jitter;
            let dt = (next - t) / 1000.0;

            let elapsed = f64::from(step) * 8.0;
            let speed = if elapsed > 1800.0 {
                lift
            } else {
                earlier * (elapsed / 1800.0)
            };

            moves.push((speed * dt, next));
            t = next;
        }

        moves
    }

    /// How fast the strip is going when the fingers lift, in centimetres of the fingers a
    /// second, for these moves on a screen that is 5000 pixels wide
    fn lift_speed(moves: &[(f64, f64)]) -> f64 {
        let mut tracker = SwipeTracker::new();

        for (cm, t) in moves {
            tracker.push(t / 1000.0, distance_on_screen(*cm, 5000));
        }

        // a centimetre of the fingers is 500 pixels on that screen
        tracker.velocity() / 500.0
    }

    #[test]
    fn the_speed_that_the_fingers_lift_at_is_what_the_strip_is_let_go_at() {
        // fast, with the time of every move a little out
        let speed = lift_speed(&swipe(8.0, 16.0));
        assert!((speed - 16.0).abs() < 0.8, "16 cm/s, got {speed:.2}");

        // a flick
        let speed = lift_speed(&swipe(20.0, 33.0));
        assert!((speed - 33.0).abs() < 1.7, "33 cm/s, got {speed:.2}");

        // one that eased to a stop and lifts almost still
        let speed = lift_speed(&swipe(8.0, 0.2));
        assert!(speed < 0.5, "almost still, got {speed:.2}");
    }

    /// The moves of what whkd writes for a swipe, which is what komorebic tells it of them: how
    /// far the fingers went, in centimetres, and when
    fn moves_of(transcript: &str) -> Vec<(f64, f64)> {
        let field = |line: &str, key: &str| -> Option<f64> {
            line.split_whitespace()
                .find_map(|word| word.strip_prefix(key)?.strip_prefix('='))
                .and_then(|value| value.parse().ok())
        };

        transcript
            .lines()
            .filter(|line| line.starts_with("move "))
            .filter_map(|line| Some((field(line, "dx")?, field(line, "t")?)))
            .collect()
    }

    #[test]
    fn the_swipes_that_whkd_writes_lift_at_the_speed_that_they_were_made_to() {
        let long = lift_speed(&moves_of(include_str!(
            "../../komorebic/fixtures/pan3-long.transcript.txt"
        )));
        let flick = lift_speed(&moves_of(include_str!(
            "../../komorebic/fixtures/pan3-flick.transcript.txt"
        )));
        let slow = lift_speed(&moves_of(include_str!(
            "../../komorebic/fixtures/pan3-slow.transcript.txt"
        )));

        // 0.128 cm every 8 ms, and 0.267 every 8
        assert!((long - 16.0).abs() < 0.8, "16 cm/s, got {long:.2}");
        assert!((flick - 33.0).abs() < 1.7, "33 cm/s, got {flick:.2}");
        // slowing from 1.9 cm/s to 0.75 over the last 150 ms, which is what the speed is the
        // average of: a nudge, and not a throw
        assert!(slow > 1.0 && slow < 2.5, "about 1.6 cm/s, got {slow:.2}");
        assert!(flick > long && long > 5.0 * slow);
    }

    #[test]
    fn what_has_hardly_moved_is_not_going_anywhere() {
        assert!(lift_speed(&[]).abs() < f64::EPSILON);
        assert!(lift_speed(&[(1.0, 100.0)]).abs() < f64::EPSILON);
        // two moves at the same time have no speed
        assert!(lift_speed(&[(1.0, 100.0), (1.0, 100.0)]).abs() < f64::EPSILON);
    }

    #[test]
    fn a_gesture_goes_over_the_socket_as_it_is() {
        for gesture in [
            PanGesture::Begin,
            PanGesture::Move {
                dx: 0.42,
                dy: -0.03,
                t: 118_345.0,
            },
            PanGesture::End,
            PanGesture::Cancel,
        ] {
            let message = SocketMessage::PanGesture(gesture);
            let json = serde_json::to_string(&message).unwrap();
            let SocketMessage::PanGesture(back) = serde_json::from_str(&json).unwrap() else {
                panic!("not a gesture: {json}");
            };

            assert_eq!(back, gesture, "{json}");
        }
    }
}
