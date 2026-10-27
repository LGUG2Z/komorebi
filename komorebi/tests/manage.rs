//! Minimizing, closing, unmanaging and managing the focused window, pausing the window manager,
//! and the unmanaged window operation behavior, against an in-memory desktop: the messages go in
//! through [`WindowManager::process_command`], and what comes out is what the backend was asked
//! for and the order of the containers.
//!
//! `UnmanageFocusedWindow` and `ManageFocusedWindow` raise an event on the process-wide channel of
//! [`window_manager_event_listener`], where the daemon's events thread would take it: the tests
//! that read the channel hold [`CHANNEL`] so that they do not take each other's events.
//!
//! Window ids are in the 27_000s.

use komorebi::backend::fake::Command;
use komorebi::backend::fake::FakeBackend;
use komorebi::backend::with_backend;
use komorebi::core::OperationBehaviour;
use komorebi::core::Rect;
use komorebi::core::SocketMessage;
use komorebi::state::State;
use komorebi::window::Window;
use komorebi::window_manager::WindowManager;
use komorebi::window_manager_event::KWinNotification;
use komorebi::window_manager_event::SystemNotification;
use komorebi::window_manager_event::WindowManagerEvent;
use komorebi::window_manager_event_listener;
use std::sync::Arc;
use std::sync::Mutex;
use std::time::Duration;

static CHANNEL: Mutex<()> = Mutex::new(());

fn added(id: u32) -> WindowManagerEvent {
    WindowManagerEvent::Show(
        SystemNotification::KWin(KWinNotification::WindowAdded),
        Window::from(id),
    )
}

fn minimized(id: u32) -> WindowManagerEvent {
    WindowManagerEvent::Minimize(
        SystemNotification::KWin(KWinNotification::WindowMinimized),
        Window::from(id),
    )
}

/// A window manager over a fake desktop with one 1920x1080 monitor, and a window for each of
/// `ids`, opened in that order. The last one has the focus.
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

    let mut wm = with_backend(fake.clone(), || {
        let mut wm = WindowManager::new(receiver, Some(socket)).unwrap();
        wm.init().unwrap();
        wm
    });
    for &id in ids {
        open(&fake, &mut wm, id);
    }
    fake.set_foreground(ids.last().copied());

    (fake, wm)
}

fn open(fake: &Arc<FakeBackend>, wm: &mut WindowManager, id: u32) {
    fake.open_window(id, "foot", "foot", &format!("terminal {id}"));
    with_backend(fake.clone(), || wm.process_event(added(id))).unwrap();
}

fn send(
    fake: &Arc<FakeBackend>,
    wm: &mut WindowManager,
    message: SocketMessage,
) -> color_eyre::Result<()> {
    with_backend(fake.clone(), || wm.process_command(message, Vec::new()))
}

/// Takes the event about window `id` off the channel the daemon's events thread reads, and has
/// the window manager handle it, as that thread does
fn handle_raised(fake: &Arc<FakeBackend>, wm: &mut WindowManager, id: u32) -> WindowManagerEvent {
    let events = window_manager_event_listener::event_rx();
    loop {
        let event = events
            .recv_timeout(Duration::from_secs(5))
            .expect("an event about the window is raised");
        if event.window_id() == Some(id) {
            with_backend(fake.clone(), || wm.process_event(event)).unwrap();
            return event;
        }
    }
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

#[test]
fn minimize_and_close_ask_the_compositor_about_the_window_that_has_the_focus() {
    let (fake, mut wm) = desktop_with(&[27_001, 27_002]);
    fake.clear_commands();

    send(&fake, &mut wm, SocketMessage::Minimize).unwrap();
    assert_eq!(fake.window_commands(), [Command::Minimize(27_002)]);
    // the window leaves the layout when the compositor reports it minimized, not before
    assert_eq!(order(&wm), [27_001, 27_002]);
    with_backend(fake.clone(), || wm.process_event(minimized(27_002))).unwrap();
    assert_eq!(order(&wm), [27_001]);

    fake.set_foreground(Some(27_001));
    fake.clear_commands();
    send(&fake, &mut wm, SocketMessage::Close).unwrap();
    assert_eq!(fake.window_commands(), [Command::Close(27_001)]);

    fake.set_foreground(None);
    assert_eq!(
        send(&fake, &mut wm, SocketMessage::Close).map_err(|error| error.to_string()),
        Err(String::from("there is no foreground window"))
    );
}

#[test]
fn unmanage_takes_the_focused_window_out_until_manage_takes_it_back() {
    let _channel = CHANNEL
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    let (fake, mut wm) = desktop_with(&[27_011, 27_012]);
    let tile = fake.rect_of(27_012).unwrap();

    send(&fake, &mut wm, SocketMessage::UnmanageFocusedWindow).unwrap();
    let event = handle_raised(&fake, &mut wm, 27_012);
    assert!(
        matches!(event, WindowManagerEvent::Unmanage(_)),
        "{event:?}"
    );

    assert_eq!(order(&wm), [27_011]);
    // the window stays open, centered in the 1920x1050 work area with the size of its tile
    assert_eq!(
        fake.rect_of(27_012),
        Some(Rect {
            left: (1920 - tile.right) / 2,
            top: (1050 - tile.bottom) / 2,
            right: tile.right,
            bottom: tile.bottom,
        })
    );

    // the compositor announcing it again does not bring it back
    with_backend(fake.clone(), || wm.process_event(added(27_012))).unwrap();
    assert_eq!(order(&wm), [27_011]);

    send(&fake, &mut wm, SocketMessage::ManageFocusedWindow).unwrap();
    let event = handle_raised(&fake, &mut wm, 27_012);
    assert!(matches!(event, WindowManagerEvent::Manage(_)), "{event:?}");

    assert_eq!(order(&wm), [27_011, 27_012]);
}

#[test]
fn while_paused_no_event_changes_the_layout_and_manage_takes_in_a_window_it_missed() {
    let _channel = CHANNEL
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    let (fake, mut wm) = desktop_with(&[27_021]);

    send(&fake, &mut wm, SocketMessage::TogglePause).unwrap();
    assert!(State::from(&wm).is_paused);
    open(&fake, &mut wm, 27_022);
    assert_eq!(order(&wm), [27_021], "a window that opens is left alone");

    send(&fake, &mut wm, SocketMessage::TogglePause).unwrap();
    assert!(!State::from(&wm).is_paused);
    open(&fake, &mut wm, 27_023);
    assert_eq!(
        order(&wm),
        [27_021, 27_023],
        "the event that was missed is not handled afterwards"
    );

    fake.set_foreground(Some(27_022));
    send(&fake, &mut wm, SocketMessage::ManageFocusedWindow).unwrap();
    handle_raised(&fake, &mut wm, 27_022);
    assert_eq!(order(&wm), [27_021, 27_023, 27_022]);
}

#[test]
fn with_no_op_every_message_is_refused_while_the_focused_window_is_not_tiled() {
    let (fake, mut wm) = desktop_with(&[27_031, 27_032]);
    // a dialog, which komorebi does not manage, takes the focus
    fake.open_window(27_033, "foot", "foot", "a dialog");
    fake.set_normal(27_033, false);

    send(
        &fake,
        &mut wm,
        SocketMessage::UnmanagedWindowOperationBehaviour(OperationBehaviour::NoOp),
    )
    .unwrap();
    assert_eq!(
        State::from(&wm).unmanaged_window_operation_behaviour,
        OperationBehaviour::NoOp
    );
    send(&fake, &mut wm, SocketMessage::Retile).unwrap();

    fake.set_foreground(Some(27_033));
    let refused = Err(String::from(
        "ignoring commands while active window is not managed by komorebi",
    ));
    assert_eq!(
        send(&fake, &mut wm, SocketMessage::Retile).map_err(|error| error.to_string()),
        refused
    );
    // this message too, while the dialog has the focus
    assert_eq!(
        send(
            &fake,
            &mut wm,
            SocketMessage::UnmanagedWindowOperationBehaviour(OperationBehaviour::Op),
        )
        .map_err(|error| error.to_string()),
        refused
    );

    fake.set_foreground(Some(27_032));
    send(
        &fake,
        &mut wm,
        SocketMessage::UnmanagedWindowOperationBehaviour(OperationBehaviour::Op),
    )
    .unwrap();
    fake.set_foreground(Some(27_033));
    send(&fake, &mut wm, SocketMessage::Retile).unwrap();
}
