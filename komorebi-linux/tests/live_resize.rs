//! The tiles next to an edge follow it while it is being dragged, against an in-memory desktop.
//!
//! A rect is `left, top, width, height` (`right` and `bottom` are the size); the work area of the
//! fake monitor is 1920x1050. Window ids are in the 21_000s to the 25_000s.

use komorebi::backend::fake::Command;
use komorebi::backend::fake::FakeBackend;
use komorebi::backend::with_backend;
use komorebi::core::DefaultLayout;
use komorebi::core::Rect;
use komorebi::core::SocketMessage;
use komorebi::window::Window;
use komorebi::window_manager::WindowManager;
use komorebi::window_manager_event::KWinNotification;
use komorebi::window_manager_event::SystemNotification;
use komorebi::window_manager_event::WindowManagerEvent;
use std::sync::Arc;

fn added(id: u32) -> WindowManagerEvent {
    WindowManagerEvent::Show(
        SystemNotification::KWin(KWinNotification::WindowAdded),
        Window::from(id),
    )
}

fn resize_started(id: u32) -> WindowManagerEvent {
    WindowManagerEvent::ResizeStart(
        SystemNotification::KWin(KWinNotification::InteractiveResizeStarted),
        Window::from(id),
    )
}

fn resize_stepped(id: u32) -> WindowManagerEvent {
    WindowManagerEvent::ResizeStep(
        SystemNotification::KWin(KWinNotification::InteractiveResizeStepped),
        Window::from(id),
    )
}

fn resize_finished(id: u32) -> WindowManagerEvent {
    WindowManagerEvent::ResizeEnd(
        SystemNotification::KWin(KWinNotification::InteractiveResizeFinished),
        Window::from(id),
    )
}

fn desktop() -> (Arc<FakeBackend>, WindowManager) {
    let fake = Arc::new(FakeBackend::with_one_monitor());
    let (_sender, receiver) = crossbeam_channel::bounded(1);
    static NEXT: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);
    let socket = std::path::PathBuf::from(format!(
        "/tmp/kmr-{}-{}.sock",
        std::process::id(),
        NEXT.fetch_add(1, std::sync::atomic::Ordering::SeqCst)
    ));

    let wm = with_backend(fake.clone(), || {
        let mut wm = WindowManager::new(receiver, Some(socket)).unwrap();
        wm.init().unwrap();
        wm
    });

    (fake, wm)
}

/// Windows `first..first+count`, tiled in a layout, the last one focused
fn tiled(layout: DefaultLayout, first: u32, count: u32) -> (Arc<FakeBackend>, WindowManager) {
    let (fake, mut wm) = desktop();
    with_backend(fake.clone(), || {
        wm.process_command(SocketMessage::ChangeLayout(layout), Vec::new())
            .unwrap();
        for id in first..first + count {
            fake.open_window(id, "foot", "foot", &format!("terminal {id}"));
            wm.process_event(added(id)).unwrap();
        }
    });
    (fake, wm)
}

/// The user drags an edge of a window: `to` is what the frame is when they let go
fn drag(fake: &Arc<FakeBackend>, wm: &mut WindowManager, id: u32, to: Rect) {
    with_backend(fake.clone(), || {
        wm.process_event(resize_started(id)).unwrap();
        fake.move_window_by_user(id, to);
        wm.process_event(resize_finished(id)).unwrap();
    });
}

fn grown_left(r: Rect, by: i32) -> Rect {
    Rect {
        left: r.left - by,
        right: r.right + by,
        ..r
    }
}

fn overlap(a: &Rect, b: &Rect) -> bool {
    a.left < b.left + b.right
        && b.left < a.left + a.right
        && a.top < b.top + b.bottom
        && b.top < a.top + a.bottom
}

fn assert_tiling(fake: &FakeBackend, ids: &[u32]) {
    let rects: Vec<Rect> = ids.iter().map(|id| fake.rect_of(*id).unwrap()).collect();
    for (i, a) in rects.iter().enumerate() {
        assert!(
            a.left >= 0 && a.top >= 0 && a.left + a.right <= 1920 && a.top + a.bottom <= 1050,
            "{a:?} is outside the work area"
        );
        for b in &rects[i + 1..] {
            assert!(!overlap(a, b), "{a:?} and {b:?} overlap");
        }
    }
}

/// the user has taken hold of an edge and has it at `to`, part way through the drag
fn step(fake: &Arc<FakeBackend>, wm: &mut WindowManager, id: u32, to: Rect) {
    with_backend(fake.clone(), || {
        fake.move_window_by_user(id, to);
        wm.process_event(resize_stepped(id)).unwrap();
    });
}

#[test]
fn the_neighbours_follow_a_drag_while_it_is_going_on_and_the_dragged_window_is_left_alone() {
    let (fake, mut wm) = tiled(DefaultLayout::BSP, 21_001, 2);
    let (left, right) = (fake.rect_of(21_001).unwrap(), fake.rect_of(21_002).unwrap());

    // the right-hand window's left edge is taken, and pulled out to the left
    with_backend(fake.clone(), || {
        wm.process_event(resize_started(21_002)).unwrap()
    });
    fake.clear_commands();

    step(&fake, &mut wm, 21_002, grown_left(right, 60));
    let after_60 = fake.rect_of(21_001).unwrap();
    assert!(
        after_60.right < left.right,
        "the left neighbour did not give way part way through: {left:?} -> {after_60:?}"
    );

    step(&fake, &mut wm, 21_002, grown_left(right, 120));
    let after_120 = fake.rect_of(21_001).unwrap();
    assert!(
        after_120.right < after_60.right,
        "and gave way more as the drag went on: {after_60:?} -> {after_120:?}"
    );

    // the dragged window is the user's until they let go
    assert!(
        !fake
            .commands()
            .iter()
            .any(|c| matches!(c, Command::SetRect(21_002, _))),
        "the dragged window was moved: {:?}",
        fake.commands()
    );
    assert_eq!(fake.rect_of(21_002).unwrap(), grown_left(right, 120));

    // let go
    with_backend(fake.clone(), || {
        wm.process_event(resize_finished(21_002)).unwrap()
    });
    assert_eq!(
        fake.rect_of(21_001).unwrap(),
        after_120,
        "the end of the drag moved the neighbour to a place other than the one it followed to"
    );
    assert_tiling(&fake, &[21_001, 21_002]);
}

#[test]
fn a_drag_has_the_same_result_with_the_steps_as_without_them() {
    let ids = [22_001, 22_002, 22_003];
    let to = |fake: &FakeBackend| grown_left(fake.rect_of(22_002).unwrap(), 90);

    // all at once
    let (control, mut control_wm) = tiled(DefaultLayout::BSP, 22_001, 3);
    let target = to(&control);
    drag(&control, &mut control_wm, 22_002, target);

    // step by step
    let (fake, mut wm) = tiled(DefaultLayout::BSP, 22_001, 3);
    let from = fake.rect_of(22_002).unwrap();
    with_backend(fake.clone(), || {
        wm.process_event(resize_started(22_002)).unwrap()
    });
    for by in [15, 40, 75, 90] {
        step(&fake, &mut wm, 22_002, grown_left(from, by));
    }
    with_backend(fake.clone(), || {
        wm.process_event(resize_finished(22_002)).unwrap()
    });

    for id in ids {
        assert_eq!(
            fake.rect_of(id),
            control.rect_of(id),
            "window {id} is not where a drag without steps puts it"
        );
    }
}

#[test]
fn a_drag_that_is_taken_back_puts_the_neighbours_where_they_were() {
    let (fake, mut wm) = tiled(DefaultLayout::BSP, 23_001, 3);
    let before: Vec<Rect> = (23_001..23_004)
        .map(|id| fake.rect_of(id).unwrap())
        .collect();
    let tile = fake.rect_of(23_002).unwrap();

    with_backend(fake.clone(), || {
        wm.process_event(resize_started(23_002)).unwrap()
    });
    step(&fake, &mut wm, 23_002, grown_left(tile, 100));
    assert_ne!(fake.rect_of(23_001).unwrap(), before[0], "nothing followed");

    // back to where it began (the user pressed Escape, or dragged the edge back)
    fake.move_window_by_user(23_002, tile);
    with_backend(fake.clone(), || {
        wm.process_event(resize_finished(23_002)).unwrap()
    });

    let after: Vec<Rect> = (23_001..23_004)
        .map(|id| fake.rect_of(id).unwrap())
        .collect();
    assert_eq!(before, after, "the tiles are not where they were");
}

#[test]
fn a_step_with_no_drag_going_on_does_nothing() {
    let (fake, mut wm) = tiled(DefaultLayout::BSP, 24_001, 2);
    let before: Vec<Rect> = (24_001..24_003)
        .map(|id| fake.rect_of(id).unwrap())
        .collect();
    fake.clear_commands();

    with_backend(fake.clone(), || {
        wm.process_event(resize_stepped(24_002)).unwrap()
    });

    assert!(fake.commands().is_empty(), "{:?}", fake.commands());
    let after: Vec<Rect> = (24_001..24_003)
        .map(|id| fake.rect_of(id).unwrap())
        .collect();
    assert_eq!(before, after);
}

#[test]
fn a_window_opening_part_way_through_a_drag_does_not_break_it() {
    let (fake, mut wm) = tiled(DefaultLayout::BSP, 25_001, 3);
    let tile = fake.rect_of(25_002).unwrap();

    with_backend(fake.clone(), || {
        wm.process_event(resize_started(25_002)).unwrap()
    });
    step(&fake, &mut wm, 25_002, grown_left(tile, 50));

    // another window opens: the layout the drag began on is not the layout there is
    fake.open_window(25_004, "foot", "foot", "late");
    with_backend(fake.clone(), || wm.process_event(added(25_004)).unwrap());
    step(&fake, &mut wm, 25_002, grown_left(tile, 90));

    with_backend(fake.clone(), || {
        wm.process_event(resize_finished(25_002)).unwrap()
    });

    assert_tiling(&fake, &[25_001, 25_002, 25_003, 25_004]);
}
