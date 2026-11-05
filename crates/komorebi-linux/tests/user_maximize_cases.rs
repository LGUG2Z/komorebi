//! Where a window the user maximizes can come from and go to, against an in-memory desktop: a
//! floating window, the window of the monocle, a second window, a window komorebi has not seen, and
//! a window that was already maximized when komorebi started.
//!
//! `user_maximize.rs` has the plain case. Window ids are in the 59_000s.

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

fn activated(id: u32) -> WindowManagerEvent {
    WindowManagerEvent::FocusChange(
        SystemNotification::KWin(KWinNotification::WindowActivated),
        Window::from(id),
    )
}

fn maximize_change(id: u32, on: bool) -> WindowManagerEvent {
    WindowManagerEvent::MaximizeChange(
        SystemNotification::KWin(KWinNotification::WindowMaximizedChanged),
        Window::from(id),
        on,
    )
}

fn socket() -> std::path::PathBuf {
    // unix socket paths are limited to ~104 bytes on macOS, which the temp dir alone can exceed
    static NEXT: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);
    std::path::PathBuf::from(format!(
        "/tmp/kmu-{}-{}.sock",
        std::process::id(),
        NEXT.fetch_add(1, std::sync::atomic::Ordering::SeqCst)
    ))
}

/// A window manager over a fake desktop with one 1920x1080 monitor (1920x1050 without the panel),
/// and a window for each of `ids`, opened in that order
fn desktop_with(ids: &[u32]) -> (Arc<FakeBackend>, WindowManager) {
    let fake = Arc::new(FakeBackend::with_one_monitor());
    let (_sender, receiver) = crossbeam_channel::bounded(1);

    let wm = with_backend(fake.clone(), || {
        let mut wm = WindowManager::new(receiver, Some(socket())).unwrap();
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

fn event(fake: &Arc<FakeBackend>, wm: &mut WindowManager, event: WindowManagerEvent) {
    with_backend(fake.clone(), || wm.process_event(event)).unwrap();
}

/// the compositor maximizes or restores a window for the user, and says so
fn user_maximizes(fake: &Arc<FakeBackend>, wm: &mut WindowManager, id: u32, on: bool) {
    fake.user_maximize(id, on);
    event(fake, wm, maximize_change(id, on));
}

/// give a window the focus the way the user does, which is what `toggle-float` and the others read
fn focus(fake: &Arc<FakeBackend>, wm: &mut WindowManager, id: u32) {
    fake.set_foreground(Some(id));
    event(fake, wm, activated(id));
}

/// The windows the layout tiles, in the order of their containers
fn tiled(wm: &WindowManager) -> Vec<u32> {
    wm.focused_workspace()
        .unwrap()
        .containers()
        .iter()
        .map(|container| container.focused_window().unwrap().id)
        .collect()
}

fn maximized(wm: &WindowManager) -> Option<u32> {
    wm.focused_workspace()
        .unwrap()
        .maximized_window
        .map(|window| window.id)
}

fn work_area() -> Rect {
    Rect {
        left: 0,
        top: 0,
        right: 1920,
        bottom: 1050,
    }
}

#[test]
fn a_floating_window_the_user_maximizes_leaves_the_floating_windows_and_comes_back_as_a_tile() {
    let (fake, mut wm) = desktop_with(&[59_001, 59_002, 59_003]);
    focus(&fake, &mut wm, 59_002);
    send(&fake, &mut wm, SocketMessage::ToggleFloat);
    let workspace = wm.focused_workspace().unwrap();
    assert_eq!(workspace.floating_windows().len(), 1);
    assert_eq!(tiled(&wm), [59_001, 59_003]);

    user_maximizes(&fake, &mut wm, 59_002, true);

    let workspace = wm.focused_workspace().unwrap();
    assert_eq!(maximized(&wm), Some(59_002));
    assert!(workspace.floating_windows().is_empty());
    assert_eq!(tiled(&wm), [59_001, 59_003]);

    user_maximizes(&fake, &mut wm, 59_002, false);

    // it is a tile now: the layer it was on is not one it can go back to
    assert_eq!(maximized(&wm), None);
    assert!(
        wm.focused_workspace()
            .unwrap()
            .floating_windows()
            .is_empty()
    );
    assert!(tiled(&wm).contains(&59_002));
    assert_eq!(tiled(&wm).len(), 3);
}

#[test]
fn the_window_in_the_monocle_that_the_user_maximizes_leaves_the_monocle() {
    let (fake, mut wm) = desktop_with(&[59_011, 59_012, 59_013]);
    focus(&fake, &mut wm, 59_012);
    send(&fake, &mut wm, SocketMessage::ToggleMonocle);
    assert!(wm.focused_workspace().unwrap().monocle_container.is_some());

    user_maximizes(&fake, &mut wm, 59_012, true);

    let workspace = wm.focused_workspace().unwrap();
    assert!(workspace.monocle_container.is_none());
    assert_eq!(maximized(&wm), Some(59_012));
    assert_eq!(tiled(&wm), [59_011, 59_013]);

    user_maximizes(&fake, &mut wm, 59_012, false);

    assert_eq!(maximized(&wm), None);
    assert_eq!(tiled(&wm), [59_011, 59_012, 59_013]);
}

#[test]
fn a_second_window_the_user_maximizes_puts_the_first_back_in_its_tile() {
    let (fake, mut wm) = desktop_with(&[59_021, 59_022, 59_023]);
    user_maximizes(&fake, &mut wm, 59_021, true);
    assert_eq!(tiled(&wm), [59_022, 59_023]);

    user_maximizes(&fake, &mut wm, 59_023, true);

    assert_eq!(maximized(&wm), Some(59_023));
    assert_eq!(tiled(&wm), [59_021, 59_022]);
    assert!(!fake.is_maximized(59_021), "the first one was restored");
    assert!(fake.is_maximized(59_023));
    assert_eq!(fake.rect_of(59_023), Some(work_area()));
}

#[test]
fn a_change_for_a_window_komorebi_has_not_taken_in_is_ignored() {
    let (fake, mut wm) = desktop_with(&[59_031, 59_032]);
    // open, and a normal window, but never shown to komorebi
    fake.open_window(59_033, "foot", "foot", "late");

    user_maximizes(&fake, &mut wm, 59_033, true);

    assert_eq!(maximized(&wm), None);
    assert_eq!(tiled(&wm), [59_031, 59_032]);
}

#[test]
fn restoring_a_window_that_is_not_maximized_changes_nothing() {
    let (fake, mut wm) = desktop_with(&[59_041, 59_042]);
    let before = (fake.rect_of(59_041), fake.rect_of(59_042));
    fake.clear_commands();

    event(&fake, &mut wm, maximize_change(59_041, false));

    assert_eq!(maximized(&wm), None);
    assert_eq!(tiled(&wm), [59_041, 59_042]);
    assert_eq!((fake.rect_of(59_041), fake.rect_of(59_042)), before);
    assert!(fake.commands().is_empty());
}

#[test]
fn a_window_that_is_maximized_when_komorebi_starts_is_the_maximized_one() {
    let fake = Arc::new(FakeBackend::with_one_monitor());
    for id in [59_051, 59_052, 59_053] {
        fake.open_window(id, "foot", "foot", &format!("terminal {id}"));
    }
    fake.user_maximize(59_052, true);
    let (_sender, receiver) = crossbeam_channel::bounded(1);

    let wm = with_backend(fake.clone(), || {
        let mut wm = WindowManager::new(receiver, Some(socket())).unwrap();
        wm.init().unwrap();
        wm
    });

    assert_eq!(maximized(&wm), Some(59_052));
    assert_eq!(tiled(&wm), [59_051, 59_053]);
    assert!(fake.is_maximized(59_052));
    assert_eq!(fake.rect_of(59_052), Some(work_area()));
}
