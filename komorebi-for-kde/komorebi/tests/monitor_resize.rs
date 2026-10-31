//! A monitor that changes size (a resolution, a scale, a panel that moves), against an in-memory
//! desktop: its windows are tiled again in the work area it has now.
//!
//! Window ids are in the 50_000s; the monitor cache is process-wide, so each test names its
//! monitors differently.

use komorebi::backend::fake::FakeBackend;
use komorebi::backend::with_backend;
use komorebi::core::Rect;
use komorebi::desktop;
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
fn a_change_of_resolution_is_tiled_again() {
    let (fake, mut wm) = desktop_of(&["Test-I"]);
    for id in [50_401, 50_402] {
        open_on(&fake, &mut wm, id, 0);
    }

    let smaller = Rect {
        left: 0,
        top: 0,
        right: 1280,
        bottom: 720,
    };
    fake.resize_monitor(
        50,
        smaller,
        Rect {
            bottom: 700,
            ..smaller
        },
    );
    with_backend(fake.clone(), || desktop::update_monitor_work_areas(&mut wm)).unwrap();

    assert_tiled_in(
        &fake,
        &[50_401, 50_402],
        &Rect {
            bottom: 700,
            ..smaller
        },
    );
}
