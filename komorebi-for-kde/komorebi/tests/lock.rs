//! Locking containers with `toggle-lock` and the messages that lock and unlock a container by its
//! indices, against an in-memory desktop: the messages go in through
//! [`WindowManager::process_command`], and what comes out is the order of the containers.
//!
//! Window ids are in the 26_000s.

use komorebi::backend::fake::FakeBackend;
use komorebi::backend::with_backend;
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

/// A window manager over a fake desktop with one 1920x1080 monitor, and a window for each of
/// `ids`, opened in that order. The last one has the focus.
fn desktop_with(ids: &[u32]) -> (Arc<FakeBackend>, WindowManager) {
    let fake = Arc::new(FakeBackend::with_one_monitor());
    let (_sender, receiver) = crossbeam_channel::bounded(1);
    // unix socket paths are limited to ~104 bytes on macOS, which the temp dir alone can exceed
    static NEXT: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);
    let socket = std::path::PathBuf::from(format!(
        "/tmp/kmk-{}-{}.sock",
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

/// The window of each container of the focused workspace, in order
fn order(wm: &WindowManager) -> Vec<u32> {
    wm.focused_workspace()
        .unwrap()
        .containers()
        .iter()
        .map(|container| container.focused_window().unwrap().id)
        .collect()
}

fn locked(wm: &WindowManager) -> Vec<bool> {
    wm.focused_workspace()
        .unwrap()
        .containers()
        .iter()
        .map(|container| container.locked)
        .collect()
}

#[test]
fn toggle_lock_locks_the_focused_container_and_unlocks_it() {
    let (fake, mut wm) = desktop_with(&[26_001, 26_002]);

    send(&fake, &mut wm, SocketMessage::ToggleLock).unwrap();
    assert_eq!(locked(&wm), [false, true]);

    send(&fake, &mut wm, SocketMessage::ToggleLock).unwrap();
    assert_eq!(locked(&wm), [false, false]);
}

#[test]
fn a_window_that_opens_does_not_push_a_locked_container_from_its_index() {
    let (fake, mut wm) = desktop_with(&[26_011, 26_012]);
    // the first window gets the focus, and its container is locked: a new window goes after it,
    // where the second one is
    with_backend(fake.clone(), || wm.process_event(activated(26_011))).unwrap();
    send(
        &fake,
        &mut wm,
        SocketMessage::LockMonitorWorkspaceContainer(0, 0, 1),
    )
    .unwrap();

    open(&fake, &mut wm, 26_013);

    assert_eq!(
        order(&wm),
        [26_011, 26_012, 26_013],
        "the second stays second"
    );
    assert_eq!(locked(&wm), [false, true, false]);
}

#[test]
fn a_container_is_locked_and_unlocked_by_its_indices() {
    let (fake, mut wm) = desktop_with(&[26_021, 26_022]);

    send(
        &fake,
        &mut wm,
        SocketMessage::LockMonitorWorkspaceContainer(0, 0, 0),
    )
    .unwrap();
    assert_eq!(locked(&wm), [true, false]);

    send(
        &fake,
        &mut wm,
        SocketMessage::UnlockMonitorWorkspaceContainer(0, 0, 0),
    )
    .unwrap();
    assert_eq!(locked(&wm), [false, false]);

    // a container that is not there is ignored, a monitor that is not there is an error
    send(
        &fake,
        &mut wm,
        SocketMessage::LockMonitorWorkspaceContainer(0, 0, 9),
    )
    .unwrap();
    assert_eq!(
        send(
            &fake,
            &mut wm,
            SocketMessage::LockMonitorWorkspaceContainer(3, 0, 0),
        )
        .map_err(|error| error.to_string()),
        Err(String::from("no monitor at the given index"))
    );
}
