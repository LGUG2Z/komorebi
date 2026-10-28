//! Moving the focus with `focus` and `cycle-focus`, against an in-memory desktop: the messages go
//! in through [`WindowManager::process_command`], and what comes out is the window the backend is
//! asked to focus and whether the pointer goes with it.
//!
//! Window ids are in the 19_000s.

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
        "/tmp/kmf-{}-{}.sock",
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

fn focused(wm: &WindowManager) -> u32 {
    wm.focused_window().unwrap().id
}

#[test]
fn focus_gives_the_focus_to_the_window_on_that_side_and_moves_nothing() {
    let (fake, mut wm) = desktop_with(&[19_001, 19_002]);
    let tiles = (fake.rect_of(19_001), fake.rect_of(19_002));
    assert_eq!(focused(&wm), 19_002);

    send(
        &fake,
        &mut wm,
        SocketMessage::FocusWindow(OperationDirection::Left),
    );

    assert_eq!(focused(&wm), 19_001);
    assert_eq!(fake.commands(), [Command::Focus(19_001, true)]);

    send(
        &fake,
        &mut wm,
        SocketMessage::FocusWindow(OperationDirection::Right),
    );

    assert_eq!(focused(&wm), 19_002);
    assert_eq!(
        fake.commands(),
        [Command::Focus(19_001, true), Command::Focus(19_002, true)]
    );
    assert_eq!((fake.rect_of(19_001), fake.rect_of(19_002)), tiles);
}

#[test]
fn focus_towards_a_side_with_no_window_is_an_error_and_changes_nothing() {
    // the two windows are side by side: there is nothing above them
    let (fake, mut wm) = desktop_with(&[19_011, 19_012]);

    let result = with_backend(fake.clone(), || {
        wm.process_command(
            SocketMessage::FocusWindow(OperationDirection::Up),
            Vec::new(),
        )
    });

    assert_eq!(
        result.map_err(|error| error.to_string()),
        Err(String::from(
            "there is no container or monitor in this direction"
        ))
    );
    assert_eq!(focused(&wm), 19_012);
    assert!(fake.commands().is_empty(), "{:?}", fake.commands());
}

#[test]
fn cycle_focus_goes_round_the_containers_in_order() {
    let (fake, mut wm) = desktop_with(&[19_021, 19_022, 19_023]);
    assert_eq!(focused(&wm), 19_023);

    send(
        &fake,
        &mut wm,
        SocketMessage::CycleFocusWindow(CycleDirection::Next),
    );
    assert_eq!(focused(&wm), 19_021, "next from the last is the first");

    send(
        &fake,
        &mut wm,
        SocketMessage::CycleFocusWindow(CycleDirection::Next),
    );
    assert_eq!(focused(&wm), 19_022);

    send(
        &fake,
        &mut wm,
        SocketMessage::CycleFocusWindow(CycleDirection::Previous),
    );
    send(
        &fake,
        &mut wm,
        SocketMessage::CycleFocusWindow(CycleDirection::Previous),
    );
    assert_eq!(focused(&wm), 19_023, "previous from the first is the last");

    assert_eq!(
        fake.commands(),
        [
            Command::Focus(19_021, true),
            Command::Focus(19_022, true),
            Command::Focus(19_021, true),
            Command::Focus(19_023, true),
        ]
    );
}

#[test]
fn cycle_focus_on_an_empty_workspace_is_an_error() {
    let (fake, mut wm) = desktop_with(&[]);

    let result = with_backend(fake.clone(), || {
        wm.process_command(
            SocketMessage::CycleFocusWindow(CycleDirection::Next),
            Vec::new(),
        )
    });

    assert!(result.is_err());
    assert!(fake.commands().is_empty(), "{:?}", fake.commands());
}

#[test]
fn without_mouse_follows_focus_the_pointer_stays_where_it_is() {
    let (fake, mut wm) = desktop_with(&[19_031, 19_032]);

    send(&fake, &mut wm, SocketMessage::MouseFollowsFocus(false));
    send(
        &fake,
        &mut wm,
        SocketMessage::FocusWindow(OperationDirection::Left),
    );
    send(&fake, &mut wm, SocketMessage::ToggleMouseFollowsFocus);
    send(
        &fake,
        &mut wm,
        SocketMessage::FocusWindow(OperationDirection::Right),
    );

    assert_eq!(
        fake.commands(),
        [Command::Focus(19_031, false), Command::Focus(19_032, true)]
    );
    assert!(wm.mouse_follows_focus);
}
