//! The layout leaves room for a border the compositor draws outside each window, and only then.
//!
//! This file has one test because it changes the process-wide border settings, which every other
//! test that tiles windows would see.

use komorebi::backend::fake::FakeBackend;
use komorebi::backend::with_backend;
use komorebi::border_manager;
use komorebi::window::Window;
use komorebi::window_manager::WindowManager;
use komorebi::window_manager_event::KWinNotification;
use komorebi::window_manager_event::SystemNotification;
use komorebi::window_manager_event::WindowManagerEvent;
use std::sync::Arc;
use std::sync::atomic::Ordering;

#[test]
fn a_border_the_compositor_draws_takes_room_around_each_window_and_none_is_taken_without_one() {
    let fake = Arc::new(FakeBackend::with_one_monitor());
    fake.set_borders_drawn(true);

    let (_sender, receiver) = crossbeam_channel::bounded(1);
    let socket = std::path::PathBuf::from(format!("/tmp/kmb-borders-{}.sock", std::process::id()));
    let mut wm = with_backend(fake.clone(), || {
        let mut wm = WindowManager::new(receiver, Some(socket)).unwrap();
        wm.init().unwrap();
        wm
    });

    // opened after the window manager started, so that it is managed (and tiled) by this event
    fake.open_window(90_001, "foot", "foot", "terminal");
    with_backend(fake.clone(), || {
        wm.process_event(WindowManagerEvent::Show(
            SystemNotification::KWin(KWinNotification::WindowAdded),
            Window::from(90_001),
        ))
        .unwrap();
    });
    let without = fake.rect_of(90_001).unwrap();

    // borders on, and the compositor says it is drawing them
    border_manager::BORDER_WIDTH.store(10, Ordering::SeqCst);
    border_manager::BORDER_OFFSET.store(2, Ordering::SeqCst);
    with_backend(fake.clone(), || {
        border_manager::send_notification();
        wm.retile_all(false).unwrap();
    });
    assert!(border_manager::BORDER_DRAWN.load(Ordering::SeqCst));

    let with = fake.rect_of(90_001).unwrap();
    let room = 10 + 2;
    assert_eq!(with.left, without.left + room, "{without:?} -> {with:?}");
    assert_eq!(with.top, without.top + room, "{without:?} -> {with:?}");
    assert_eq!(
        with.right,
        without.right - 2 * room,
        "{without:?} -> {with:?}"
    );
    assert_eq!(
        with.bottom,
        without.bottom - 2 * room,
        "{without:?} -> {with:?}"
    );

    // borders off: the room is given back
    border_manager::BORDER_ENABLED.store(false, Ordering::SeqCst);
    with_backend(fake.clone(), || {
        border_manager::send_notification();
        wm.retile_all(false).unwrap();
    });
    assert!(!border_manager::BORDER_DRAWN.load(Ordering::SeqCst));
    assert_eq!(fake.rect_of(90_001).unwrap(), without);
}
