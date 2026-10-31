//! Looking at the windows after a drag that the windows next to the edge followed, for the ones
//! that did not get to where the layout puts them.
//!
//! A window can be told where to go and not get there: an app that is busy with an earlier write
//! answers late, or not at all, and the window stays where that left it. The steps of a drag do not
//! wait to be told what the windows turned out to be, so nothing else notices. A quarter of a
//! second after the drag, when the apps have done what they were going to, the windows are looked at
//! and any that are not where they are to be are sent there again, and looked at once more after
//! that.

use crate::accessibility::AccessibilityApi;
use crate::animation::animator;
use crate::core::Rect;
use crate::macos_api::MacosApi;
use crate::window::Snapping;
use crate::window::dragged_window;
use crate::window::is_parked;
use crate::window_manager::WindowManager;
use color_eyre::eyre;
use crossbeam_channel::Sender;
use crossbeam_channel::bounded;
use parking_lot::Mutex;
use std::sync::Arc;
use std::sync::OnceLock;
use std::sync::atomic::AtomicUsize;
use std::sync::atomic::Ordering;
use std::time::Duration;

/// How long after a drag ends the windows are looked at for ones that did not get where it left
/// them, and then again, for the ones that were sent again
const SETTLE_AFTER: [Duration; 2] = [Duration::from_millis(250), Duration::from_millis(350)];

/// How many pixels a window can be from where it is to be, on any side, and be there: what rounding,
/// a terminal's grid of cells and the frame a window has are.
const SETTLE_TOLERANCE: i32 = 2;

/// How long, in seconds, an app is waited for to say where its window is, while the window manager
/// is waiting for it: the windows are looked at one after the other with nothing else going on
const LOOK_TIMEOUT: f32 = 0.25;

/// Which drag the windows are being looked at after
static GENERATION: AtomicUsize = AtomicUsize::new(0);

/// The generation of the drag, and the monitor and the workspace that it was on
type Request = (usize, usize, usize);

static SETTLE: OnceLock<Sender<Request>> = OnceLock::new();

/// A drag begins, and the windows are not looked at for the one before any more.
pub fn begin() {
    GENERATION.fetch_add(1, Ordering::SeqCst);
}

/// Whether no other drag has begun since the one that `generation` is of
fn is_current(generation: usize) -> bool {
    GENERATION.load(Ordering::SeqCst) == generation
}

/// Ask for the windows of a workspace to be looked at after a drag that ended on it.
pub fn request(monitor_idx: usize, workspace_idx: usize) {
    if let Some(settle) = SETTLE.get() {
        let _ = settle.try_send((
            GENERATION.load(Ordering::SeqCst),
            monitor_idx,
            workspace_idx,
        ));
    }
}

/// Look at the windows after the drags that ask for it, on a thread of its own, since it waits.
pub fn listen_for_settling(wm: Arc<Mutex<WindowManager>>) {
    let (sender, receiver) = bounded::<Request>(1);

    if SETTLE.set(sender).is_err() {
        return;
    }

    std::thread::spawn(move || {
        while let Ok((generation, monitor_idx, workspace_idx)) = receiver.recv() {
            for wait in SETTLE_AFTER {
                std::thread::sleep(wait);

                // another drag has begun
                if !is_current(generation) {
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
        }
    });
}

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

/// Whether `layout` is wholly inside `work_area`, which is where a window is to be exactly where the
/// layout says. One that is partly outside is clamped by the layout, or is parked past an edge
/// of the screen, and is not at `layout`, however well it was sent.
fn is_inside(layout: &Rect, work_area: &Rect) -> bool {
    layout.left >= work_area.left
        && layout.top >= work_area.top
        && layout.left + layout.right <= work_area.left + work_area.right
        && layout.top + layout.bottom <= work_area.top + work_area.bottom
}

impl WindowManager {
    /// Send the windows of the workspace that are not where its layout puts them to where it does,
    /// and say how many there were. Nothing is done for another workspace than the one that is on
    /// show, nor while a drag is going on, which is not for the windows to be put back from.
    pub fn live_resize_settle(
        &mut self,
        monitor_idx: usize,
        workspace_idx: usize,
    ) -> eyre::Result<usize> {
        if self.is_paused
            || self.live_resize.is_some()
            || self.focused_monitor_idx() != monitor_idx
            || self.focused_workspace_idx()? != workspace_idx
        {
            return Ok(0);
        }

        let work_area = self.focused_monitor_work_area()?;
        let workspace = self.focused_workspace()?;

        if !workspace.tile
            || workspace.monocle_container.is_some()
            || workspace.maximized_window.is_some()
        {
            return Ok(0);
        }

        // the windows that are to be somewhere that can be looked at, with where that is
        let windows: Vec<_> = workspace
            .containers()
            .iter()
            .enumerate()
            .filter_map(|(idx, container)| {
                let layout = workspace.latest_layout.get(idx)?;
                let window = container.focused_window()?;

                // scrolling parks the windows that are out of view at an edge, and clamps the ones
                // that are partly out of view: where they are is not where the layout says
                let somewhere_else = is_parked(window.id)
                    || animator::is_animating(window.id)
                    || !is_inside(layout, &work_area);

                (!somewhere_else).then(|| (window.clone(), *layout))
            })
            .collect();

        // the windows are looked at before any is sent anywhere, which is not for another window
        // that is looked at after it to wait for
        let mut off = Vec::new();

        for (window, layout) in windows {
            // a drag has begun while the others were looked at
            if dragged_window().is_some() {
                return Ok(0);
            }

            let _ = AccessibilityApi::set_messaging_timeout(&window.element, LOOK_TIMEOUT);
            let rect = MacosApi::window_rect(&window.element).map(Rect::from);
            let _ = AccessibilityApi::set_messaging_timeout(&window.element, 0.0);

            if let Ok(rect) = rect
                && is_off(&rect, &layout)
            {
                tracing::debug!(
                    "live resize: window {} is at {rect:?} after the drag, sent again to {layout:?}",
                    window.id
                );
                off.push((window, layout));
            }
        }

        if off.is_empty() {
            return Ok(0);
        }

        // what is sent is sent straight there, as it is for the end of the drag
        let _snapping = Snapping::end();

        for (window, layout) in &off {
            if let Err(error) = window.set_position(layout) {
                tracing::debug!(
                    "live resize: could not send window {} again: {error}",
                    window.id
                );
            }
        }

        Ok(off.len())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::WindowManagerEvent;
    use crate::core::DefaultLayout;
    use crate::core::Layout;
    use crate::monitor;
    use crate::window::Window;
    use crossbeam_channel::Receiver;
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

    /// A window manager with a workspace of two windows that there is nothing behind, laid out
    /// side by side, and a second workspace
    fn window_manager() -> (WindowManager, Socket) {
        let (_sender, receiver): (_, Receiver<WindowManagerEvent>) = bounded(1);
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
        workspace.layout = Layout::Default(DefaultLayout::BSP);
        // ids that nothing else in the tests uses
        for id in 3_900_000_001..3_900_000_003 {
            workspace
                .new_container_for_window(&Window::from(id))
                .unwrap();
        }
        workspace.latest_layout = vec![
            Rect {
                left: 0,
                top: 0,
                right: 600,
                bottom: 800,
            },
            Rect {
                left: 600,
                top: 0,
                right: 600,
                bottom: 800,
            },
        ];

        (wm, socket)
    }

    #[test]
    fn nothing_is_looked_at_while_the_window_manager_is_paused() {
        let (mut wm, _socket) = window_manager();
        wm.is_paused = true;

        assert_eq!(wm.live_resize_settle(0, 0).unwrap(), 0);
    }

    #[test]
    fn nothing_is_looked_at_on_a_workspace_that_is_not_on_show() {
        let (mut wm, _socket) = window_manager();

        assert_eq!(wm.live_resize_settle(0, 1).unwrap(), 0);
        assert_eq!(wm.live_resize_settle(1, 0).unwrap(), 0);
    }

    #[test]
    fn nothing_is_looked_at_on_a_workspace_that_is_not_tiled() {
        let (mut wm, _socket) = window_manager();
        wm.focused_workspace_mut().unwrap().tile = false;

        assert_eq!(wm.live_resize_settle(0, 0).unwrap(), 0);
    }

    #[test]
    fn a_window_that_cannot_be_looked_at_is_not_sent_anywhere() {
        let (mut wm, _socket) = window_manager();

        // there is nothing to say where windows that are not there are
        assert_eq!(wm.live_resize_settle(0, 0).unwrap(), 0);
    }

    const LAYOUT: Rect = Rect {
        left: 100,
        top: 50,
        right: 400,
        bottom: 300,
    };

    #[test]
    fn a_window_is_where_it_is_to_be_within_a_couple_of_pixels_on_every_side() {
        assert!(!is_off(&LAYOUT, &LAYOUT));

        for (left, top, right, bottom) in [
            (2, 0, 0, 0),
            (-2, 0, 0, 0),
            (0, 2, 0, 0),
            (0, 0, 2, 0),
            (0, 0, 0, -2),
            (2, -2, 2, -2),
        ] {
            let rect = Rect {
                left: LAYOUT.left + left,
                top: LAYOUT.top + top,
                right: LAYOUT.right + right,
                bottom: LAYOUT.bottom + bottom,
            };

            assert!(!is_off(&rect, &LAYOUT), "{rect:?}");
        }
    }

    #[test]
    fn a_window_is_not_where_it_is_to_be_if_any_side_is_further_off_than_that() {
        for (left, top, right, bottom) in [
            (3, 0, 0, 0),
            (0, -3, 0, 0),
            (0, 0, 40, 0),
            (0, 0, 0, -3),
            (0, 0, -120, 0),
        ] {
            let rect = Rect {
                left: LAYOUT.left + left,
                top: LAYOUT.top + top,
                right: LAYOUT.right + right,
                bottom: LAYOUT.bottom + bottom,
            };

            assert!(is_off(&rect, &LAYOUT), "{rect:?}");
        }
    }

    #[test]
    fn a_layout_is_where_a_window_is_to_be_only_if_it_is_inside_the_work_area() {
        let work_area = Rect {
            left: 0,
            top: 0,
            right: 1200,
            bottom: 800,
        };
        let at = |left, top, right, bottom| Rect {
            left,
            top,
            right,
            bottom,
        };

        assert!(is_inside(&LAYOUT, &work_area));
        assert!(is_inside(&at(0, 0, 1200, 800), &work_area), "all of it");
        assert!(is_inside(&at(800, 0, 400, 800), &work_area), "to the edge");

        // a column that is partly in view, which the layout clamps, or past an edge of the screen,
        // which is parked
        assert!(!is_inside(&at(-100, 0, 400, 800), &work_area));
        assert!(!is_inside(&at(900, 0, 400, 800), &work_area));
        assert!(!is_inside(&at(1300, 0, 400, 800), &work_area));
        assert!(!is_inside(&at(0, -10, 400, 800), &work_area));
        assert!(!is_inside(&at(0, 100, 400, 800), &work_area));

        // and a work area that is not at the origin
        let second = at(1200, 0, 1200, 800);
        assert!(is_inside(&at(1300, 50, 400, 300), &second));
        assert!(!is_inside(&at(1100, 50, 400, 300), &second));
    }

    #[test]
    fn what_is_looked_at_after_a_drag_is_not_looked_at_after_the_next_one_begins() {
        let drag = GENERATION.load(Ordering::SeqCst);

        begin();

        assert!(!is_current(drag));
    }
}
