//! The desktop after the machine wakes up or the screen is unlocked, against an in-memory desktop.
//!
//! While the screen is locked or asleep, what komorebi asks for is not done and the monitors can
//! change. On waking komorebi looks at the monitors again and puts every window in its tile.
//!
//! Window ids are in the 50_000s; the monitor cache is process-wide, so the tests that unplug a
//! monitor use names of their own.

use komorebi::backend::fake::FakeBackend;
use komorebi::backend::with_backend;
use komorebi::core::Rect;
use komorebi::monitor_reconciliator::wake_up;
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
fn windows_that_did_not_get_where_they_were_sent_while_the_screen_was_off_are_put_there_on_waking()
{
    let (fake, mut wm) = desktop_of(&["Test-J"]);
    for id in [50_501, 50_502, 50_503] {
        open_on(&fake, &mut wm, id, 0);
    }
    let tiles: Vec<Rect> = [50_501, 50_502, 50_503]
        .iter()
        .map(|id| fake.rect_of(*id).unwrap())
        .collect();

    // the screen was locked and asleep: what komorebi asked for was not done, and the windows are
    // wherever the compositor had them
    fake.move_window_by_user(50_502, somewhere_in(0));
    fake.move_window_by_user(50_503, somewhere_in(300));

    with_backend(fake.clone(), || wake_up(&mut wm)).unwrap();

    let now: Vec<Rect> = [50_501, 50_502, 50_503]
        .iter()
        .map(|id| fake.rect_of(*id).unwrap())
        .collect();
    assert_eq!(tiles, now, "the windows are not in their tiles");
}

#[test]
fn a_monitor_unplugged_while_the_machine_slept_is_dealt_with_on_waking() {
    let (fake, mut wm) = desktop_of(&["Test-K", "Test-L"]);
    open_on(&fake, &mut wm, 50_601, 0);
    open_on(&fake, &mut wm, 50_602, 1920);

    // while it slept the second monitor went, and the compositor took its window to the first
    fake.remove_monitor(51);
    fake.move_window_by_user(50_602, somewhere_in(0));

    with_backend(fake.clone(), || wake_up(&mut wm)).unwrap();

    assert_eq!(wm.monitors().len(), 1);
    assert_eq!(containers_on(&wm, 0), 2);
    assert_tiled_in(&fake, &[50_601, 50_602], &work_area(0));
}

#[test]
fn a_paused_window_manager_is_left_alone_on_waking() {
    let (fake, mut wm) = desktop_of(&["Test-M"]);
    open_on(&fake, &mut wm, 50_701, 0);
    let tile = fake.rect_of(50_701).unwrap();
    fake.move_window_by_user(50_701, somewhere_in(300));
    let moved = fake.rect_of(50_701).unwrap();
    assert_ne!(tile, moved);

    wm.is_paused = true;
    with_backend(fake.clone(), || wake_up(&mut wm)).unwrap();

    assert_eq!(
        fake.rect_of(50_701),
        Some(moved),
        "a paused window manager moved a window"
    );
}
