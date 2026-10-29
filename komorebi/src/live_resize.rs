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
use crate::core::OperationDirection;
use crate::core::Rect;
use crate::core::Sizing;
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
use std::sync::atomic::Ordering;
use std::time::Duration;
use std::time::Instant;

/// The least time between two steps that are laid out: a frame of a display
/// that is refreshed 60 times a second
const STEP_INTERVAL: Duration = Duration::from_millis(16);

/// Lays out what is laid out while it is alive at once, and not animated: the
/// neighbours of a window that is dragged follow it as it goes, and are not to be
/// seen catching up
pub struct Snapping;

impl Snapping {
    /// `paced` is for the steps of a drag, which leave a window that has not
    /// got to where it was last sent for the next step: the end of a drag puts
    /// everything where it is to be
    fn new(paced: bool) -> Self {
        PACED.store(paced, Ordering::SeqCst);
        STEPPING.store(true, Ordering::SeqCst);
        Self
    }
}

impl Drop for Snapping {
    fn drop(&mut self) {
        STEPPING.store(false, Ordering::SeqCst);
        PACED.store(false, Ordering::SeqCst);
    }
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

impl WindowManager {
    /// The user has taken hold of a window by its title bar or an edge, which
    /// is not known to be either yet: what the workspace is is kept in case it
    /// is an edge.
    pub fn live_resize_begin(&mut self, hwnd: isize, started_from: Rect) {
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

        for (edge, sizing, by) in edges_moved(&kept.started_from, &now) {
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

        Some(snapping)
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
}
