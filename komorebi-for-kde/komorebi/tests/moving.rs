//! Moving the focused window with `move` and `cycle-move`, against an in-memory desktop: the
//! messages go in through [`WindowManager::process_command`], and what comes out is the order of
//! the containers, where the backend is asked to put each window, and which window has the
//! focus.
//!
//! Window ids are in the 20_000s.

use komorebi::backend::fake::Command;
use komorebi::backend::fake::FakeBackend;
use komorebi::backend::with_backend;
use komorebi::core::CycleDirection;
use komorebi::core::OperationDirection;
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

/// A window manager over a fake desktop with one 1920x1080 monitor, and a window for each of
/// `ids`, opened in that order. The last one has the focus, and the commands it took to get there
/// are cleared.
fn desktop_with(ids: &[u32]) -> (Arc<FakeBackend>, WindowManager) {
    let fake = Arc::new(FakeBackend::with_one_monitor());
    let (_sender, receiver) = crossbeam_channel::bounded(1);
    // unix socket paths are limited to ~104 bytes on macOS, which the temp dir alone can exceed
    static NEXT: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);
    let socket = std::path::PathBuf::from(format!(
        "/tmp/kmm-{}-{}.sock",
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

    fake.clear_commands();
    (fake, wm)
}

fn send(fake: &Arc<FakeBackend>, wm: &mut WindowManager, message: SocketMessage) {
    with_backend(fake.clone(), || wm.process_command(message, Vec::new())).unwrap();
}

/// The window of each container of the focused workspace, in order
fn order(wm: &WindowManager) -> Vec<u32> {
    wm.focused_workspace()
        .unwrap()
        .containers()
        .iter()
        .map(|container| container.focused_window().unwrap().id)
        .collect()
}

fn focused(wm: &WindowManager) -> u32 {
    wm.focused_window().unwrap().id
}

#[test]
fn move_swaps_the_focused_window_with_the_one_on_that_side() {
    let (fake, mut wm) = desktop_with(&[20_001, 20_002]);
    let (left, right) = (fake.rect_of(20_001), fake.rect_of(20_002));

    send(
        &fake,
        &mut wm,
        SocketMessage::MoveWindow(OperationDirection::Left),
    );

    assert_eq!(order(&wm), [20_002, 20_001]);
    assert_eq!(
        focused(&wm),
        20_002,
        "the window that moved keeps the focus"
    );
    assert_eq!(fake.rect_of(20_002), left);
    assert_eq!(fake.rect_of(20_001), right);
    assert!(
        fake.commands().contains(&Command::Focus(20_002, true)),
        "{:?}",
        fake.commands()
    );

    send(
        &fake,
        &mut wm,
        SocketMessage::MoveWindow(OperationDirection::Right),
    );

    assert_eq!(order(&wm), [20_001, 20_002]);
    assert_eq!((fake.rect_of(20_001), fake.rect_of(20_002)), (left, right));
}

#[test]
fn move_towards_a_side_with_no_window_is_an_error_and_moves_nothing() {
    // the two windows are side by side: there is nothing below them
    let (fake, mut wm) = desktop_with(&[20_011, 20_012]);

    let result = with_backend(fake.clone(), || {
        wm.process_command(
            SocketMessage::MoveWindow(OperationDirection::Down),
            Vec::new(),
        )
    });

    assert_eq!(
        result.map_err(|error| error.to_string()),
        Err(String::from(
            "there is no container or monitor in this direction"
        ))
    );
    assert_eq!(order(&wm), [20_011, 20_012]);
    assert!(fake.commands().is_empty(), "{:?}", fake.commands());
}

#[test]
fn cycle_move_swaps_with_the_next_container_and_wraps_around() {
    let (fake, mut wm) = desktop_with(&[20_021, 20_022, 20_023]);
    let first_tile = fake.rect_of(20_021);

    send(
        &fake,
        &mut wm,
        SocketMessage::CycleMoveWindow(CycleDirection::Next),
    );

    assert_eq!(
        order(&wm),
        [20_023, 20_022, 20_021],
        "next from the last is the first"
    );
    assert_eq!(focused(&wm), 20_023);
    assert_eq!(fake.rect_of(20_023), first_tile);

    send(
        &fake,
        &mut wm,
        SocketMessage::CycleMoveWindow(CycleDirection::Previous),
    );

    assert_eq!(
        order(&wm),
        [20_021, 20_022, 20_023],
        "previous from the first is the last"
    );
    assert_eq!(focused(&wm), 20_023);
}

#[test]
fn a_lock_stays_with_its_index_when_a_window_is_moved_onto_it() {
    let (fake, mut wm) = desktop_with(&[20_031, 20_032]);
    wm.focused_workspace_mut().unwrap().containers_mut()[0].locked = true;

    send(
        &fake,
        &mut wm,
        SocketMessage::MoveWindow(OperationDirection::Left),
    );

    let containers = wm.focused_workspace().unwrap().containers();
    assert_eq!(order(&wm), [20_032, 20_031]);
    assert!(containers[0].locked, "the first index is still locked");
    assert!(!containers[1].locked, "the window that was locked is not");
}
