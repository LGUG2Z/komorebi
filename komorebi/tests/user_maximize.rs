//! A window the user maximizes (with its title bar, or by opening it maximized), against an
//! in-memory desktop.
//!
//! The compositor does the maximizing and reports it. komorebi takes the window out of the layout
//! as `komorebic toggle-maximize` does, leaves the layout alone while it is maximized, and puts it
//! back in its tile when it is restored, by the user or by komorebi.
//!
//! Window ids are in the 30_000s.

use komorebi::backend::fake::Command;
use komorebi::backend::fake::FakeBackend;
use komorebi::backend::with_backend;
use komorebi::core::OperationDirection;
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

fn desktop() -> (Arc<FakeBackend>, WindowManager) {
    let fake = Arc::new(FakeBackend::with_one_monitor());
    let (_sender, receiver) = crossbeam_channel::bounded(1);
    static NEXT: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);
    let socket = std::path::PathBuf::from(format!(
        "/tmp/kmf-{}-{}.sock",
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

fn tiled(first: u32, count: u32) -> (Arc<FakeBackend>, WindowManager) {
    let (fake, mut wm) = desktop();
    with_backend(fake.clone(), || {
        for id in first..first + count {
            fake.open_window(id, "foot", "foot", &format!("terminal {id}"));
            wm.process_event(added(id)).unwrap();
        }
    });
    (fake, wm)
}

fn command(fake: &Arc<FakeBackend>, wm: &mut WindowManager, message: SocketMessage) {
    with_backend(fake.clone(), || wm.process_command(message, Vec::new())).unwrap();
}

fn work_area() -> Rect {
    Rect {
        left: 0,
        top: 0,
        right: 1920,
        bottom: 1050,
    }
}

fn maximized(id: u32, on: bool) -> WindowManagerEvent {
    WindowManagerEvent::MaximizeChange(
        SystemNotification::KWin(KWinNotification::WindowMaximizedChanged),
        Window::from(id),
        on,
    )
}

/// the user maximizes a window with its title bar: the compositor does it, and says so
fn user_maximizes(fake: &Arc<FakeBackend>, wm: &mut WindowManager, id: u32, on: bool) {
    fake.user_maximize(id, on);
    with_backend(fake.clone(), || wm.process_event(maximized(id, on))).unwrap();
}

#[test]
fn a_window_the_user_maximizes_leaves_the_layout_like_toggle_maximize_makes_it() {
    let (fake, mut wm) = tiled(30_061, 3);

    user_maximizes(&fake, &mut wm, 30_062, true);

    let workspace = wm.focused_workspace().unwrap();
    assert_eq!(workspace.maximized_window.map(|w| w.id), Some(30_062));
    assert_eq!(workspace.containers().len(), 2, "its tile is gone");
    assert!(fake.is_maximized(30_062));
    assert_eq!(fake.rect_of(30_062).unwrap(), work_area());

    // the others fill the room it left, and do not overlap
    let (a, b) = (fake.rect_of(30_061).unwrap(), fake.rect_of(30_063).unwrap());
    assert!(a.right > 800 && b.right > 800, "{a:?} {b:?}");
}

#[test]
fn nothing_that_is_about_the_layout_happens_while_a_window_is_maximized() {
    let (fake, mut wm) = tiled(30_071, 3);
    user_maximizes(&fake, &mut wm, 30_072, true);
    let rects: Vec<Rect> = [30_071, 30_072, 30_073]
        .iter()
        .map(|id| fake.rect_of(*id).unwrap())
        .collect();
    fake.clear_commands();

    for message in [
        SocketMessage::MoveWindow(OperationDirection::Left),
        SocketMessage::MoveWindow(OperationDirection::Right),
        SocketMessage::StackWindow(OperationDirection::Left),
        SocketMessage::UnstackWindow,
        SocketMessage::StackAll,
        SocketMessage::Promote,
        SocketMessage::ToggleFloat,
        SocketMessage::ToggleMonocle,
        SocketMessage::ToggleLock,
        SocketMessage::SendContainerToWorkspaceNumber(1),
        SocketMessage::MoveContainerToWorkspaceNumber(1),
    ] {
        let name = message.to_string();
        command(&fake, &mut wm, message);

        let workspace = wm.focused_workspace().unwrap();
        assert_eq!(
            workspace.maximized_window.map(|w| w.id),
            Some(30_072),
            "{name} took the window out of its maximized state"
        );
        assert_eq!(workspace.containers().len(), 2, "{name} changed the tiles");
        assert!(workspace.floating_windows().is_empty(), "{name}");
        assert!(workspace.monocle_container.is_none(), "{name}");
        assert!(fake.is_maximized(30_072), "{name}");
    }

    let now: Vec<Rect> = [30_071, 30_072, 30_073]
        .iter()
        .map(|id| fake.rect_of(*id).unwrap())
        .collect();
    assert_eq!(rects, now, "a window moved");
    assert!(
        !fake
            .commands()
            .iter()
            .any(|c| matches!(c, Command::SetRect(..) | Command::SetWindowWorkspace(..))),
        "{:?}",
        fake.commands()
    );
}

#[test]
fn toggle_maximize_restores_a_window_the_user_maximized() {
    let (fake, mut wm) = tiled(30_081, 3);
    let before: Vec<Rect> = (30_081..30_084)
        .map(|id| fake.rect_of(id).unwrap())
        .collect();
    user_maximizes(&fake, &mut wm, 30_083, true);

    command(&fake, &mut wm, SocketMessage::ToggleMaximize);
    // the compositor says the window was restored; komorebi did that itself
    with_backend(fake.clone(), || wm.process_event(maximized(30_083, false))).unwrap();

    let workspace = wm.focused_workspace().unwrap();
    assert!(workspace.maximized_window.is_none());
    assert_eq!(workspace.containers().len(), 3);
    assert!(!fake.is_maximized(30_083));
    let after: Vec<Rect> = (30_081..30_084)
        .map(|id| fake.rect_of(id).unwrap())
        .collect();
    assert_eq!(before, after);
}

#[test]
fn the_user_restoring_the_window_with_its_title_bar_puts_it_back_in_its_tile() {
    let (fake, mut wm) = tiled(30_091, 3);
    let before: Vec<Rect> = (30_091..30_094)
        .map(|id| fake.rect_of(id).unwrap())
        .collect();
    user_maximizes(&fake, &mut wm, 30_091, true);
    assert_eq!(wm.focused_workspace().unwrap().containers().len(), 2);

    user_maximizes(&fake, &mut wm, 30_091, false);

    let workspace = wm.focused_workspace().unwrap();
    assert!(workspace.maximized_window.is_none());
    assert_eq!(workspace.containers().len(), 3);
    let after: Vec<Rect> = (30_091..30_094)
        .map(|id| fake.rect_of(id).unwrap())
        .collect();
    assert_eq!(before, after, "every tile is where it was");

    // and the commands work again
    command(
        &fake,
        &mut wm,
        SocketMessage::MoveWindow(OperationDirection::Right),
    );
    let moved: Vec<Rect> = (30_091..30_094)
        .map(|id| fake.rect_of(id).unwrap())
        .collect();
    assert_ne!(before, moved, "the move did nothing after the restore");
}

#[test]
fn komorebis_own_maximize_echoing_back_changes_nothing() {
    let (fake, mut wm) = tiled(30_101, 2);
    command(&fake, &mut wm, SocketMessage::ToggleMaximize);
    let workspace = wm.focused_workspace().unwrap();
    assert_eq!(workspace.containers().len(), 1);

    // the compositor reports the maximize komorebi asked for
    with_backend(fake.clone(), || wm.process_event(maximized(30_102, true))).unwrap();

    let workspace = wm.focused_workspace().unwrap();
    assert_eq!(workspace.maximized_window.map(|w| w.id), Some(30_102));
    assert_eq!(workspace.containers().len(), 1);
}

#[test]
fn a_window_that_opens_maximized_is_the_maximized_one() {
    let (fake, mut wm) = tiled(30_111, 2);

    fake.open_window(30_113, "firefox", "firefox", "web");
    fake.user_maximize(30_113, true);
    with_backend(fake.clone(), || wm.process_event(added(30_113))).unwrap();

    let workspace = wm.focused_workspace().unwrap();
    assert_eq!(workspace.maximized_window.map(|w| w.id), Some(30_113));
    assert_eq!(workspace.containers().len(), 2, "the two tiles that were");
    assert!(fake.is_maximized(30_113));
}
