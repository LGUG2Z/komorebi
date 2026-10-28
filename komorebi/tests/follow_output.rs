//! Windows that something else moves to another output, against an in-memory desktop.
//!
//! A window moved to another output by a shortcut is on that monitor's workspace from then on. The
//! same holds for the windows the compositor takes back to a monitor that is plugged in again.
//!
//! Window ids are in the 50_000s; the monitor cache is process-wide, so the tests that unplug a
//! monitor use names of their own.

use komorebi::backend::fake::FakeBackend;
use komorebi::backend::with_backend;
use komorebi::core::Rect;
use komorebi::monitor_reconciliator::reconcile_display_change;
use komorebi::window::Window;
use komorebi::window_manager::WindowManager;
use komorebi::window_manager_event::KWinNotification;
use komorebi::window_manager_event::SystemNotification;
use komorebi::window_manager_event::WindowManagerEvent;
use std::sync::Arc;

fn area(left: i32) -> Rect {
    Rect {
        left,
        top: 0,
        right: 1920,
        bottom: 1080,
    }
}

fn work_area(left: i32) -> Rect {
    Rect {
        left,
        top: 0,
        right: 1920,
        bottom: 1050,
    }
}

fn desktop_of(names: &[&str]) -> (Arc<FakeBackend>, WindowManager) {
    let fake = Arc::new(FakeBackend::new());
    for (i, name) in names.iter().enumerate() {
        fake.add_monitor(
            50 + i as u32,
            name,
            area(1920 * i as i32),
            work_area(1920 * i as i32),
        );
    }

    let (_sender, receiver) = crossbeam_channel::bounded(1);
    static NEXT: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);
    let socket = std::path::PathBuf::from(format!(
        "/tmp/kmh-{}-{}.sock",
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

fn added(id: u32) -> WindowManagerEvent {
    WindowManagerEvent::Show(
        SystemNotification::KWin(KWinNotification::WindowAdded),
        Window::from(id),
    )
}

fn moved_to_another_output(id: u32) -> WindowManagerEvent {
    WindowManagerEvent::OutputChange(
        SystemNotification::KWin(KWinNotification::WindowOutputChanged),
        Window::from(id),
    )
}

/// where a window opens: somewhere in the area that starts at `left`
fn somewhere_in(left: i32) -> Rect {
    Rect {
        left: left + 200,
        top: 200,
        right: 800,
        bottom: 600,
    }
}

fn open_on(fake: &Arc<FakeBackend>, wm: &mut WindowManager, id: u32, left: i32) {
    fake.open_window(id, "foot", "foot", &format!("terminal {id}"));
    fake.move_window_by_user(id, somewhere_in(left));
    with_backend(fake.clone(), || wm.process_event(added(id))).unwrap();
}

fn containers_on(wm: &WindowManager, monitor: usize) -> usize {
    wm.monitors()[monitor]
        .focused_workspace()
        .map_or(0, |w| w.containers().len())
}

fn inside(rect: &Rect, area: &Rect) -> bool {
    rect.left >= area.left
        && rect.top >= area.top
        && rect.left + rect.right <= area.left + area.right
        && rect.top + rect.bottom <= area.top + area.bottom
}

fn overlap(a: &Rect, b: &Rect) -> bool {
    a.left < b.left + b.right
        && b.left < a.left + a.right
        && a.top < b.top + b.bottom
        && b.top < a.top + a.bottom
}

fn assert_tiled_in(fake: &FakeBackend, ids: &[u32], area: &Rect) {
    let rects: Vec<Rect> = ids.iter().map(|id| fake.rect_of(*id).unwrap()).collect();
    for (i, a) in rects.iter().enumerate() {
        assert!(inside(a, area), "{a:?} is not inside {area:?}");
        for b in &rects[i + 1..] {
            assert!(!overlap(a, b), "{a:?} and {b:?} overlap");
        }
    }
}

#[test]
fn the_windows_of_a_monitor_that_goes_are_tiled_on_the_one_that_is_left_and_come_back_with_it() {
    let (fake, mut wm) = desktop_of(&["Test-A", "Test-B"]);

    for id in [50_001, 50_002, 50_003] {
        open_on(&fake, &mut wm, id, 0);
    }
    for id in [50_004, 50_005] {
        open_on(&fake, &mut wm, id, 1920);
    }
    assert_eq!(wm.monitors().len(), 2);
    assert_eq!(containers_on(&wm, 0), 3);
    assert_eq!(containers_on(&wm, 1), 2);

    // the second monitor is unplugged, and the compositor takes its windows to the first
    fake.remove_monitor(51);
    for (id, at) in [(50_004, 300), (50_005, 340)] {
        fake.move_window_by_user(id, somewhere_in(at - 200));
    }
    with_backend(fake.clone(), || reconcile_display_change(&mut wm)).unwrap();

    assert_eq!(
        wm.monitors().len(),
        1,
        "the monitor that went is still there"
    );
    assert_eq!(containers_on(&wm, 0), 5, "its windows were not taken over");
    for id in 50_001..=50_005 {
        assert_eq!(
            wm.known_window_ids.get(&id).map(|(m, _)| *m),
            Some(0),
            "window {id} is not managed on the monitor that is left"
        );
    }
    assert_tiled_in(&fake, &[50_001, 50_002, 50_003, 50_004, 50_005], &area(0));

    // it is plugged back in
    fake.add_monitor(51, "Test-B", area(1920), work_area(1920));
    with_backend(fake.clone(), || reconcile_display_change(&mut wm)).unwrap();

    assert_eq!(wm.monitors().len(), 2, "the monitor did not come back");
    assert_eq!(
        containers_on(&wm, 1),
        0,
        "it came back with windows that were tiled elsewhere"
    );

    // and the compositor, a moment later, takes two windows back to it
    for id in [50_004, 50_005] {
        fake.move_window_by_user(id, somewhere_in(1920));
        with_backend(fake.clone(), || {
            wm.process_event(moved_to_another_output(id))
        })
        .unwrap();
    }

    assert_eq!(containers_on(&wm, 0), 3);
    assert_eq!(containers_on(&wm, 1), 2);
    assert_tiled_in(&fake, &[50_001, 50_002, 50_003], &work_area(0));
    assert_tiled_in(&fake, &[50_004, 50_005], &work_area(1920));
}

#[test]
fn a_window_sent_to_another_screen_by_a_shortcut_is_tiled_there() {
    let (fake, mut wm) = desktop_of(&["Test-E", "Test-F"]);
    for id in [50_201, 50_202, 50_203] {
        open_on(&fake, &mut wm, id, 0);
    }

    // the user sends a window to the next screen, and the compositor puts it there
    fake.move_window_by_user(50_202, somewhere_in(1920));
    with_backend(fake.clone(), || {
        wm.process_event(moved_to_another_output(50_202))
    })
    .unwrap();

    assert_eq!(containers_on(&wm, 0), 2);
    assert_eq!(containers_on(&wm, 1), 1);
    assert_eq!(wm.known_window_ids.get(&50_202).map(|(m, _)| *m), Some(1));
    assert_tiled_in(&fake, &[50_201, 50_203], &work_area(0));
    assert_tiled_in(&fake, &[50_202], &work_area(1920));
}

#[test]
fn nothing_is_followed_while_the_user_drags_a_window() {
    let (fake, mut wm) = desktop_of(&["Test-I", "Test-J"]);
    for id in [50_401, 50_402, 50_403] {
        open_on(&fake, &mut wm, id, 0);
    }

    // the window is over the other monitor, and the user has not let go of it yet
    fake.move_window_by_user(50_402, somewhere_in(1920));
    wm.pending_move_op = Arc::new(Some((0, 0, 50_402)));
    let moved = with_backend(fake.clone(), || wm.follow_windows_to_their_monitors()).unwrap();
    assert!(!moved, "a window was followed during a move");
    assert_eq!(containers_on(&wm, 0), 3);
    assert_eq!(containers_on(&wm, 1), 0);

    // the same for the edge of a window
    wm.pending_move_op = Arc::new(None);
    wm.pending_resize_op = Arc::new(Some((50_401, None)));
    let moved = with_backend(fake.clone(), || wm.follow_windows_to_their_monitors()).unwrap();
    assert!(!moved, "a window was followed during a resize");
    assert_eq!(containers_on(&wm, 0), 3);

    // and once they have let go, it is
    wm.pending_resize_op = Arc::new(None);
    let moved = with_backend(fake.clone(), || wm.follow_windows_to_their_monitors()).unwrap();
    assert!(moved, "the window was not followed after the drag");
    assert_eq!(containers_on(&wm, 0), 2);
    assert_eq!(containers_on(&wm, 1), 1);
    assert_eq!(wm.known_window_ids.get(&50_402).map(|(m, _)| *m), Some(1));
}
