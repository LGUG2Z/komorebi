//! Moving the strip of the Scrolling layout across the screen without changing which window has
//! focus: by a step, and with the fingers on a touchpad.
//!
//! A gesture is a begin, any number of moves, and an end, or a cancel. The strip follows the
//! fingers for as long as they are down, and the workspace is laid out a step at a time as the
//! windows of a drag are, and when the fingers lift it comes to rest with a column against an
//! edge of the screen. Where the strip rests is laid out the way every layout is, so that with
//! animations on the windows glide there from where the fingers left them.

use crate::core::PanDirection;
use crate::core::PanGesture;
use crate::window::Snapping;
use crate::window::steps_begin;
use crate::window::steps_end;
use crate::window_manager::WindowManager;
use crate::workspace::Workspace;
use color_eyre::eyre;
use komorebi_layouts::scrolling::model::ScrollingSpace;
use komorebi_layouts::scrolling::model::SwipeTracker;
use std::time::Instant;

/// How far the fingers go across a touchpad, in centimetres, to move the strip as far as the
/// width of the screen
const CM_PER_SCREEN: f64 = 10.0;

/// A gesture that is going on
#[derive(Debug)]
pub struct Pan {
    monitor_idx: usize,
    workspace_idx: usize,
    /// What the strip was before the gesture, for putting it back if it is given up
    saved: ScrollingSpace<String>,
    /// How fast the fingers are going
    tracker: SwipeTracker,
    /// When it began, and the time of the touchpad at the first move, for the log
    began: Instant,
    first_t: Option<f64>,
}

/// How far the strip is moved on a screen that is `screen_width` pixels wide by fingers that
/// move `cm` centimetres, in pixels, to the right if it is positive
fn distance_on_screen(cm: f64, screen_width: i32) -> f64 {
    cm * f64::from(screen_width) / CM_PER_SCREEN
}

impl WindowManager {
    /// Move the view of the focused workspace one step, a column, to the left or the right, and
    /// let it come to rest with a column against an edge of the screen. Focus goes to the nearest
    /// column in view if the focused one is not. Nothing happens in a layout that is not Scrolling.
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

        self.update_focused_workspace(focus_moved, focus_moved)
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

    /// Whether the workspace that a gesture is about is the one that is on show
    fn pan_is_on_show(&self, pan: &Pan) -> bool {
        self.focused_monitor_idx() == pan.monitor_idx
            && self
                .focused_workspace_idx()
                .is_ok_and(|idx| idx == pan.workspace_idx)
    }

    /// The workspace that a gesture is about, which is not the one that is on show if the fingers
    /// have been on the touchpad while it was changed
    fn pan_workspace_mut(&mut self, pan: &Pan) -> Option<&mut Workspace> {
        self.monitors_mut()
            .get_mut(pan.monitor_idx)?
            .workspaces_mut()
            .get_mut(pan.workspace_idx)
    }

    fn pan_begin(&mut self) -> eyre::Result<()> {
        // a gesture that was not finished is given up, as a new one begins
        self.pan_cancel()?;

        let monitor_idx = self.focused_monitor_idx();
        let workspace_idx = self.focused_workspace_idx()?;

        let Some(saved) = self.focused_workspace_mut()?.begin_scrolling_pan() else {
            return Ok(());
        };

        steps_begin();

        self.pan = Some(Pan {
            monitor_idx,
            workspace_idx,
            saved,
            tracker: SwipeTracker::new(),
            began: Instant::now(),
            first_t: None,
        });

        Ok(())
    }

    fn pan_move(&mut self, dx_cm: f64, t_ms: f64) -> eyre::Result<()> {
        let Some(pan) = &self.pan else {
            return Ok(());
        };

        // the workspace that the fingers are on is not the one that is on show any more
        if !self.pan_is_on_show(pan) {
            return self.pan_cancel();
        }

        let workspace = self.focused_workspace_mut()?;
        let Some(width) = workspace.scrolling_view_width() else {
            return Ok(());
        };

        let dx = distance_on_screen(dx_cm, width);
        workspace.pan_scrolling(dx);

        if let Some(pan) = self.pan.as_mut() {
            pan.tracker.push(t_ms / 1000.0, dx);

            // how far behind the fingers a move is handled is how far the wall clock has got
            // ahead of the touchpad's since the first
            let first = *pan.first_t.get_or_insert(t_ms);
            let fingers = t_ms - first;
            let wall = pan.began.elapsed().as_secs_f64() * 1000.0;
            tracing::trace!(
                "pan: move of {dx:.1} px at {fingers:.0} ms of the fingers, handled at {wall:.0} ms ({:+.0} ms)",
                wall - fingers
            );
        }

        // the workspace is laid out a step at a time, straight into place
        let _snapping = Snapping::pan();
        self.update_focused_workspace(false, false)
    }

    fn pan_end(&mut self) -> eyre::Result<()> {
        let Some(pan) = self.pan.take() else {
            return Ok(());
        };

        steps_end();

        let velocity = pan.tracker.velocity();
        let on_show = self.pan_is_on_show(&pan);
        let Some(workspace) = self.pan_workspace_mut(&pan) else {
            return Ok(());
        };

        // a workspace that was changed in the middle of the gesture is let come to rest all the
        // same, and laid out when it is shown, from where it rests
        let focus_moved = workspace.end_scrolling_pan(velocity);

        if !on_show {
            return Ok(());
        }

        // what is laid out here is not a step: with animations on the windows glide to where the
        // strip rests from where the fingers left them, and without they go straight there
        self.update_focused_workspace(focus_moved, focus_moved)
    }

    fn pan_cancel(&mut self) -> eyre::Result<()> {
        let Some(pan) = self.pan.take() else {
            return Ok(());
        };

        steps_end();

        let on_show = self.pan_is_on_show(&pan);
        let Some(workspace) = self.pan_workspace_mut(&pan) else {
            return Ok(());
        };

        workspace.cancel_scrolling_pan(pan.saved);

        if !on_show {
            return Ok(());
        }

        self.update_focused_workspace(false, false)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::WindowManagerEvent;
    use crate::container::Container;
    use crate::core::DefaultLayout;
    use crate::core::Layout;
    use crate::core::Rect;
    use crate::core::SocketMessage;
    use crate::monitor;
    use crossbeam_channel::Receiver;
    use crossbeam_channel::Sender;
    use crossbeam_channel::bounded;
    use objc2_core_foundation::CFRunLoop;
    use std::path::PathBuf;
    use uuid::Uuid;

    /// The socket of a window manager that a test made, which is not left behind
    struct Socket(PathBuf);

    impl Drop for Socket {
        fn drop(&mut self) {
            let _ = std::fs::remove_file(&self.0);
        }
    }

    const AREA: Rect = Rect {
        left: 0,
        top: 0,
        right: 1200,
        bottom: 800,
    };

    /// A window manager with two workspaces on a monitor, the first with a strip of six
    /// containers that have no windows, so that nothing is laid out on a screen
    fn window_manager() -> (WindowManager, Socket) {
        let (_sender, receiver): (Sender<WindowManagerEvent>, Receiver<WindowManagerEvent>) =
            bounded(1);
        let path = PathBuf::from(format!("komorebi-test-{}.sock", Uuid::new_v4()));
        let socket = Socket(path.clone());

        let mut wm = WindowManager::new(&CFRunLoop::main().unwrap(), receiver, Some(path)).unwrap();
        wm.monitors_mut().push_back(monitor::new(
            0,
            AREA,
            AREA,
            "TestMonitor".to_string(),
            "TestDeviceID".to_string(),
        ));

        let monitor = wm.focused_monitor_mut().unwrap();
        monitor.focus_workspace(1).unwrap();
        monitor.focus_workspace(0).unwrap();

        let workspace = wm.focused_workspace_mut().unwrap();
        workspace.layout = Layout::Default(DefaultLayout::Scrolling);
        for _ in 0..6 {
            workspace.add_container_to_back(Container::default());
        }
        workspace.focus_container(0);
        workspace.layout_scrolling(&AREA, 0);

        (wm, socket)
    }

    fn strip_of(wm: &mut WindowManager, workspace_idx: usize) -> Vec<i32> {
        wm.monitors_mut()[0].workspaces_mut()[workspace_idx]
            .layout_scrolling(&AREA, 0)
            .iter()
            .map(|placement| placement.rect.left)
            .collect()
    }

    #[test]
    fn fingers_on_a_layout_that_is_not_scrolling_move_nothing() {
        let (mut wm, _socket) = window_manager();
        wm.focused_workspace_mut().unwrap().layout = Layout::Default(DefaultLayout::BSP);

        for gesture in [
            PanGesture::Begin,
            PanGesture::Move {
                dx: 1.0,
                dy: 0.0,
                t: 100.0,
            },
            PanGesture::End,
            PanGesture::Cancel,
        ] {
            wm.pan_gesture(gesture).unwrap();
            assert!(wm.pan.is_none(), "{gesture:?}");
        }

        wm.pan(PanDirection::Left).unwrap();
        wm.pan(PanDirection::Right).unwrap();
    }

    #[test]
    fn fingers_that_land_on_a_strip_are_a_gesture_until_they_lift() {
        let (mut wm, _socket) = window_manager();

        wm.pan_gesture(PanGesture::Begin).unwrap();
        assert!(wm.pan.is_some());

        // and it is not about moves that come with no fingers down
        wm.pan_gesture(PanGesture::Cancel).unwrap();
        assert!(wm.pan.is_none());
        wm.pan_gesture(PanGesture::Cancel).unwrap();
        wm.pan_gesture(PanGesture::End).unwrap();
    }

    #[test]
    fn a_gesture_that_is_given_up_after_the_workspace_changed_puts_its_strip_back() {
        let (mut wm, _socket) = window_manager();
        let before = strip_of(&mut wm, 0);

        wm.pan_gesture(PanGesture::Begin).unwrap();
        wm.monitors_mut()[0].workspaces_mut()[0].pan_scrolling(-500.0);
        assert_ne!(strip_of(&mut wm, 0), before, "the fingers moved the strip");

        // the workspace that is on show is another by the time that they give up
        wm.focused_monitor_mut()
            .unwrap()
            .focus_workspace(1)
            .unwrap();
        wm.pan_gesture(PanGesture::Cancel).unwrap();

        assert!(wm.pan.is_none());
        assert_eq!(strip_of(&mut wm, 0), before);
    }

    #[test]
    fn a_gesture_that_ends_after_the_workspace_changed_lets_its_strip_come_to_rest() {
        let (mut wm, _socket) = window_manager();

        wm.pan_gesture(PanGesture::Begin).unwrap();
        // 150 pixels short of a column, which is not where a strip rests
        wm.monitors_mut()[0].workspaces_mut()[0].pan_scrolling(-250.0);
        wm.focused_monitor_mut()
            .unwrap()
            .focus_workspace(1)
            .unwrap();
        wm.pan_gesture(PanGesture::End).unwrap();

        assert!(wm.pan.is_none());
        let rested = strip_of(&mut wm, 0);
        assert!(
            rested.iter().any(|left| *left == 0 || *left + 400 == 1200),
            "a column is against an edge: {rested:?}"
        );
    }

    #[test]
    fn a_gesture_that_begins_gives_up_the_one_that_was_not_finished() {
        let (mut wm, _socket) = window_manager();
        let before = strip_of(&mut wm, 0);

        wm.pan_gesture(PanGesture::Begin).unwrap();
        wm.monitors_mut()[0].workspaces_mut()[0].pan_scrolling(-500.0);

        // the fingers land again on a workspace that has no strip to move
        wm.focused_monitor_mut()
            .unwrap()
            .focus_workspace(1)
            .unwrap();
        wm.pan_gesture(PanGesture::Begin).unwrap();

        assert!(
            wm.pan.is_none(),
            "there is no strip on the workspace on show"
        );
        assert_eq!(strip_of(&mut wm, 0), before, "and the first is given up");
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

    /// The moves of what a gesture stream writes for a swipe, which is what komorebic tells
    /// komorebi of them: how far the fingers went, in centimetres, and when
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
    fn the_swipes_of_a_stream_lift_at_the_speed_that_they_were_made_to() {
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
