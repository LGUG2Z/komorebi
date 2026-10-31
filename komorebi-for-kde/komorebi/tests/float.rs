//! Floating a window with `toggle-float`, against an in-memory desktop: the messages go in
//! through [`WindowManager::process_command`], and what comes out is which windows the layout
//! tiles and where the backend is asked to put them.
//!
//! Window ids are in the 25_000s.

use komorebi::backend::fake::FakeBackend;
use komorebi::backend::with_backend;
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

/// A window manager over a fake desktop with one 1920x1080 monitor (1920x1050 without the panel),
/// and a window for each of `ids`, opened in that order. The last one has the focus.
fn desktop_with(ids: &[u32]) -> (Arc<FakeBackend>, WindowManager) {
    let fake = Arc::new(FakeBackend::with_one_monitor());
    let (_sender, receiver) = crossbeam_channel::bounded(1);
    // unix socket paths are limited to ~104 bytes on macOS, which the temp dir alone can exceed
    static NEXT: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);
    let socket = std::path::PathBuf::from(format!(
        "/tmp/kmt-{}-{}.sock",
        std::process::id(),
        NEXT.fetch_add(1, std::sync::atomic::Ordering::SeqCst)
    ));

    let wm = with_backend(fake.clone(), || {
        let mut wm = WindowManager::new(receiver, Some(socket)).unwrap();
        wm.init().unwrap();
        for &id in ids {
            fake.open_window(id, "foot", "foot", &format!("terminal {id}"));
            wm.process_event(added(id)).unwrap();
        }
        wm
    });

    (fake, wm)
}

fn send(fake: &Arc<FakeBackend>, wm: &mut WindowManager, message: SocketMessage) {
    with_backend(fake.clone(), || wm.process_command(message, Vec::new())).unwrap();
}

fn tiled(wm: &WindowManager) -> Vec<u32> {
    wm.focused_workspace()
        .unwrap()
        .containers()
        .iter()
        .map(|container| container.focused_window().unwrap().id)
        .collect()
}

fn floating(wm: &WindowManager) -> Vec<u32> {
    wm.focused_workspace()
        .unwrap()
        .floating_windows()
        .iter()
        .map(|window| window.id)
        .collect()
}

#[test]
fn toggle_float_takes_the_focused_window_out_of_the_layout_and_puts_it_back() {
    let (fake, mut wm) = desktop_with(&[25_001, 25_002]);
    let half = fake.rect_of(25_001).unwrap();

    send(&fake, &mut wm, SocketMessage::ToggleFloat);

    assert_eq!(tiled(&wm), [25_001]);
    assert_eq!(floating(&wm), [25_002]);
    assert!(
        fake.rect_of(25_001).unwrap().right > half.right,
        "the window left in the layout has the work area"
    );
    assert!(wm.focused_workspace().unwrap().contains_window(25_002));

    send(&fake, &mut wm, SocketMessage::ToggleFloat);

    assert_eq!(tiled(&wm), [25_002, 25_001]);
    assert!(floating(&wm).is_empty());
}

#[test]
fn a_floating_window_is_left_where_it_is_by_a_retile() {
    let (fake, mut wm) = desktop_with(&[25_011, 25_012]);
    send(&fake, &mut wm, SocketMessage::ToggleFloat);
    let by_hand = Rect {
        left: 300,
        top: 200,
        right: 640,
        bottom: 480,
    };
    fake.move_window_by_user(25_012, by_hand);

    send(&fake, &mut wm, SocketMessage::Retile);

    assert_eq!(fake.rect_of(25_012), Some(by_hand));
}

#[test]
fn toggle_float_without_a_focused_window_is_an_error() {
    let (fake, mut wm) = desktop_with(&[25_021]);
    fake.set_foreground(None);

    let result = with_backend(fake.clone(), || {
        wm.process_command(SocketMessage::ToggleFloat, Vec::new())
    });

    assert_eq!(
        result.map_err(|error| error.to_string()),
        Err(String::from("no foreground window"))
    );
    assert_eq!(tiled(&wm), [25_021]);
}
