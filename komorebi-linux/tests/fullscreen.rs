//! A window the user puts in fullscreen, against an in-memory desktop.
//!
//! The compositor covers the monitor with a fullscreen window, and komorebi leaves it there: the
//! other windows are tiled around it, and it goes back to its tile when it leaves fullscreen.
//!
//! Window ids are in the 10_000s.

use komorebi::backend::fake::Command;
use komorebi::backend::fake::FakeBackend;
use komorebi::backend::with_backend;
use komorebi::core::Rect;
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

fn fullscreen(id: u32, on: bool) -> WindowManagerEvent {
    WindowManagerEvent::FullscreenChange(
        SystemNotification::KWin(KWinNotification::WindowFullScreenChanged),
        Window::from(id),
        on,
    )
}

/// A window manager over a fake desktop with one 1920x1080 monitor (1920x1050 without the panel)
fn desktop() -> (Arc<FakeBackend>, WindowManager) {
    desktop_on(FakeBackend::with_one_monitor())
}

fn desktop_on(fake: FakeBackend) -> (Arc<FakeBackend>, WindowManager) {
    let fake = Arc::new(fake);
    let (_sender, receiver) = crossbeam_channel::bounded(1);
    // unix socket paths are limited to ~104 bytes on macOS, which the temp dir alone can exceed
    static NEXT: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);
    let socket = std::path::PathBuf::from(format!(
        "/tmp/kmb-{}-{}.sock",
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

fn open(fake: &FakeBackend, id: u32) {
    fake.open_window(id, "foot", "foot", &format!("terminal {id}"));
}

fn work_area() -> Rect {
    Rect {
        left: 0,
        top: 0,
        right: 1920,
        bottom: 1050,
    }
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

#[test]
fn a_fullscreen_window_is_left_covering_the_monitor_and_goes_back_to_its_tile_afterwards() {
    let ids = [10_091, 10_092, 10_093];

    // which window's tile changes when a fourth opens? (it depends on the layout; the one that
    // does is the one to put in fullscreen, so that a stale frame is the wrong one)
    let (control, mut wm) = desktop();
    for id in ids {
        open(&control, id);
    }
    let before: Vec<Rect> = with_backend(control.clone(), || {
        for id in ids {
            wm.process_event(added(id)).unwrap();
        }
        ids.map(|id| control.rect_of(id).unwrap()).to_vec()
    });
    open(&control, 10_094);
    with_backend(control.clone(), || wm.process_event(added(10_094))).unwrap();
    let moved = ids
        .into_iter()
        .zip(before)
        .find(|(id, was)| control.rect_of(*id).unwrap() != *was)
        .map(|(id, _)| id)
        .expect("opening a window moved none of the tiles");

    // the same again, with that window fullscreen while the fourth opens
    let (fake, mut wm) = desktop();
    for id in ids {
        open(&fake, id);
    }
    with_backend(fake.clone(), || {
        for id in ids {
            wm.process_event(added(id)).unwrap();
        }
    });
    let tile = fake.rect_of(moved).unwrap();

    // the window goes fullscreen; the compositor covers the monitor with it
    fake.set_fullscreen(moved, true);
    let covering = fake.rect_of(moved).unwrap();
    fake.clear_commands();
    with_backend(fake.clone(), || wm.process_event(fullscreen(moved, true))).unwrap();

    // a window opens meanwhile, and everything is retiled around the others
    open(&fake, 10_094);
    with_backend(fake.clone(), || wm.process_event(added(10_094))).unwrap();

    assert!(
        !fake
            .commands()
            .iter()
            .any(|c| matches!(c, Command::SetRect(id, _) if *id == moved)),
        "a fullscreen window was moved: {:?}",
        fake.commands()
    );
    assert_eq!(fake.rect_of(moved), Some(covering));

    // it leaves fullscreen: the compositor gives it the frame it had, which is not where it belongs
    // now that there is another window, and komorebi puts it in its tile
    fake.set_fullscreen(moved, false);
    assert_eq!(
        fake.rect_of(moved),
        Some(tile),
        "the fake gives back the old frame"
    );
    with_backend(fake.clone(), || wm.process_event(fullscreen(moved, false))).unwrap();

    let now = fake.rect_of(moved).unwrap();
    assert_ne!(
        now, tile,
        "the window is still where it was before the fourth opened"
    );
    assert_eq!(
        Some(now),
        control.rect_of(moved),
        "and it is where it would have been without fullscreen"
    );
    for other in ids.into_iter().filter(|id| *id != moved).chain([10_094]) {
        assert!(
            !overlap(&now, &fake.rect_of(other).unwrap()),
            "{now:?} overlaps window {other}"
        );
    }
    assert!(inside(&now, &work_area()));
}
