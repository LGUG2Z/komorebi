//! What `toggle-maximize` does to the rest of the window manager, against an in-memory desktop: a
//! window that is maximized is in no container, the messages about the tiles do nothing until it
//! is restored, and the state has it. The focus crosses to a monitor where one is maximized, and a
//! window that is maximized is hidden and shown with its workspace.
//!
//! Window ids are in the 36_000s.

use komorebi::backend::fake::Command;
use komorebi::backend::fake::FakeBackend;
use komorebi::workspace::WorkspaceExt;
use komorebi::backend::with_backend;
use komorebi::core::CycleDirection;
use komorebi::core::OperationDirection;
use komorebi::core::Rect;
use komorebi::core::SocketMessage;
use komorebi::state::State;
use komorebi::window::Window;
use komorebi::window_manager::WindowManager;
use komorebi::window_manager_event::KWinNotification;
use komorebi::window_manager_event::SystemNotification;
use komorebi::window_manager_event::WindowManagerEvent;
use komorebi_backend::Backend as _;
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

/// A window manager over a fake desktop with one 1920x1080 monitor (1920x1050 without the panel),
/// and a window for each of `ids`, opened in that order
fn desktop_with(ids: &[u32]) -> (Arc<FakeBackend>, WindowManager) {
    let fake = Arc::new(FakeBackend::with_one_monitor());
    let (_sender, receiver) = crossbeam_channel::bounded(1);
    // unix socket paths are limited to ~104 bytes on macOS, which the temp dir alone can exceed
    static NEXT: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);
    let socket = std::path::PathBuf::from(format!(
        "/tmp/kmx-{}-{}.sock",
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

/// A window manager over a fake desktop with two monitors side by side, and a window for each of
/// `left` on the one on the left and of `right` on the one on the right, opened in that order
fn two_monitors_with(left: &[u32], right: &[u32]) -> (Arc<FakeBackend>, WindowManager) {
    let fake = FakeBackend::with_one_monitor();
    let right_half = Rect {
        left: 1920,
        top: 0,
        right: 1920,
        bottom: 1080,
    };
    fake.add_monitor(2, "Virtual-1", right_half, right_half);
    let fake = Arc::new(fake);
    let (_sender, receiver) = crossbeam_channel::bounded(1);
    // unix socket paths are limited to ~104 bytes on macOS, which the temp dir alone can exceed
    static NEXT: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(1000);
    let socket = std::path::PathBuf::from(format!(
        "/tmp/kmx-{}-{}.sock",
        std::process::id(),
        NEXT.fetch_add(1, std::sync::atomic::Ordering::SeqCst)
    ));

    let wm = with_backend(fake.clone(), || {
        let mut wm = WindowManager::new(receiver, Some(socket)).unwrap();
        wm.init().unwrap();
        for (ids, on_right) in [(left, false), (right, true)] {
            for &id in ids {
                fake.open_window(id, "foot", "foot", &format!("terminal {id}"));
                if on_right {
                    fake.move_window_by_user(
                        id,
                        Rect {
                            left: 2020,
                            top: 100,
                            right: 800,
                            bottom: 600,
                        },
                    );
                }
                wm.process_event(added(id)).unwrap();
            }
        }
        wm
    });

    (fake, wm)
}

fn try_send(
    fake: &Arc<FakeBackend>,
    wm: &mut WindowManager,
    message: SocketMessage,
) -> Result<(), String> {
    with_backend(fake.clone(), || wm.process_command(message, Vec::new()))
        .map_err(|error| error.to_string())
}

fn send(fake: &Arc<FakeBackend>, wm: &mut WindowManager, message: SocketMessage) {
    try_send(fake, wm, message).unwrap();
}

fn event(fake: &Arc<FakeBackend>, wm: &mut WindowManager, event: WindowManagerEvent) {
    with_backend(fake.clone(), || wm.process_event(event)).unwrap();
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

#[test]
fn a_window_goes_back_in_a_container_of_its_own_at_the_index_it_came_from() {
    let (fake, mut wm) = desktop_with(&[36_001, 36_002, 36_003]);
    event(&fake, &mut wm, activated(36_002));

    send(&fake, &mut wm, SocketMessage::ToggleMaximize);

    assert_eq!(maximized(&wm), Some(36_002));
    assert_eq!(tiled(&wm), [36_001, 36_003]);
    assert!(wm.focused_workspace().unwrap().contains_window(36_002));
    assert!(
        wm.focused_workspace()
            .unwrap()
            .contains_managed_window(36_002)
    );

    send(&fake, &mut wm, SocketMessage::ToggleMaximize);

    assert_eq!(maximized(&wm), None);
    assert_eq!(tiled(&wm), [36_001, 36_002, 36_003]);
    assert_eq!(wm.focused_workspace().unwrap().focused_container_idx(), 1);
}

#[test]
fn while_a_window_is_maximized_what_is_about_the_tiles_does_nothing() {
    let (fake, mut wm) = desktop_with(&[36_011, 36_012]);
    send(&fake, &mut wm, SocketMessage::ToggleMaximize);
    let asked = fake.window_commands();

    for message in [
        SocketMessage::MoveWindow(OperationDirection::Left),
        SocketMessage::CycleMoveWindow(CycleDirection::Next),
        SocketMessage::UnstackWindow,
        SocketMessage::StackAll,
        SocketMessage::UnstackAll,
        SocketMessage::CycleStack(CycleDirection::Next),
        SocketMessage::CycleStackIndex(CycleDirection::Next),
        SocketMessage::FocusStackWindow(0),
        SocketMessage::MoveContainerToWorkspaceNumber(1),
        SocketMessage::SendContainerToWorkspaceNumber(1),
        SocketMessage::MoveContainerToMonitorNumber(1),
        SocketMessage::SendContainerToMonitorNumber(1),
        SocketMessage::Promote,
        SocketMessage::ToggleLock,
        SocketMessage::PreselectDirection(OperationDirection::Left),
    ] {
        send(&fake, &mut wm, message);
    }

    assert_eq!(maximized(&wm), Some(36_012));
    assert_eq!(tiled(&wm), [36_011]);
    assert!(wm.focused_workspace().unwrap().monocle_container.is_none());
    assert!(
        wm.focused_workspace()
            .unwrap()
            .floating_windows()
            .is_empty()
    );
    assert_eq!(
        fake.window_commands(),
        asked,
        "nothing was asked of the backend"
    );
}

#[test]
fn a_retile_while_a_window_is_maximized_asks_the_compositor_to_maximize_it_again() {
    let (fake, mut wm) = desktop_with(&[36_021, 36_022]);
    send(&fake, &mut wm, SocketMessage::ToggleMaximize);
    fake.clear_commands();

    send(&fake, &mut wm, SocketMessage::Retile);

    assert_eq!(fake.window_commands(), [Command::Maximize(36_022, true)]);
    assert!(fake.is_maximized(36_022));
}

#[test]
fn the_window_in_monocle_that_is_maximized_leaves_the_monocle() {
    let (fake, mut wm) = desktop_with(&[36_031, 36_032]);
    event(&fake, &mut wm, activated(36_031));
    send(&fake, &mut wm, SocketMessage::ToggleMonocle);

    send(&fake, &mut wm, SocketMessage::ToggleMaximize);

    let workspace = wm.focused_workspace().unwrap();
    assert!(workspace.monocle_container.is_none());
    assert_eq!(maximized(&wm), Some(36_031));
    assert_eq!(workspace.maximized_window_restore_idx, Some(0));

    send(&fake, &mut wm, SocketMessage::ToggleMaximize);

    assert_eq!(tiled(&wm), [36_031, 36_032]);
}

#[test]
fn a_window_that_opens_meanwhile_is_tiled_when_the_maximized_one_is_restored() {
    let (fake, mut wm) = desktop_with(&[36_041, 36_042]);
    send(&fake, &mut wm, SocketMessage::ToggleMaximize);

    fake.open_window(36_043, "foot", "foot", "terminal 36043");
    event(&fake, &mut wm, added(36_043));

    assert_eq!(maximized(&wm), Some(36_042));
    assert_eq!(tiled(&wm), [36_041, 36_043]);

    send(&fake, &mut wm, SocketMessage::ToggleMaximize);

    let rects: Vec<_> = tiled(&wm)
        .iter()
        .map(|&id| fake.rect_of(id).unwrap())
        .collect();
    assert_eq!(rects.len(), 3);
    for (i, a) in rects.iter().enumerate() {
        for b in &rects[i + 1..] {
            let overlap = a.left < b.left + b.right
                && b.left < a.left + a.right
                && a.top < b.top + b.bottom
                && b.top < a.top + a.bottom;
            assert!(!overlap, "{a:?} and {b:?} overlap");
        }
    }
}

#[test]
fn toggle_maximize_without_a_window_is_an_error() {
    let (fake, mut wm) = desktop_with(&[]);

    assert_eq!(
        try_send(&fake, &mut wm, SocketMessage::ToggleMaximize),
        Err(String::from("there is no container"))
    );
    assert_eq!(maximized(&wm), None);
}

#[test]
fn the_state_shows_the_maximized_window_and_the_index_it_goes_back_to() {
    let (fake, mut wm) = desktop_with(&[36_051, 36_052]);
    send(&fake, &mut wm, SocketMessage::ToggleMaximize);

    let state = with_backend(fake.clone(), || {
        serde_json::to_value(State::from(&wm)).unwrap()
    });
    let workspace = &state["monitors"]["elements"][0]["workspaces"]["elements"][0];

    assert_eq!(workspace["maximized_window"]["id"], 36_052);
    assert_eq!(workspace["maximized_window_restore_idx"], 1);
}

#[test]
fn focusing_towards_a_monitor_with_a_maximized_window_gives_it_the_focus() {
    let (fake, mut wm) = two_monitors_with(&[36_061, 36_062], &[36_063, 36_064]);
    event(&fake, &mut wm, activated(36_064));
    send(&fake, &mut wm, SocketMessage::ToggleMaximize);
    event(&fake, &mut wm, activated(36_062));
    assert_eq!(wm.focused_monitor_idx(), 0);

    send(
        &fake,
        &mut wm,
        SocketMessage::FocusWindow(OperationDirection::Right),
    );

    assert_eq!(wm.focused_monitor_idx(), 1);
    assert_eq!(
        fake.foreground_window().and_then(|id| id.to_u32()),
        Some(36_064)
    );
}

#[test]
fn a_maximized_window_is_hidden_with_its_workspace_and_shown_and_focused_with_it() {
    let (fake, mut wm) = desktop_with(&[36_071, 36_072]);
    send(&fake, &mut wm, SocketMessage::ToggleMaximize);
    let workspace = wm.focused_workspace_mut().unwrap();

    with_backend(fake.clone(), || workspace.hide(None));
    assert!(fake.is_hidden(36_071) && fake.is_hidden(36_072));

    fake.clear_commands();
    with_backend(fake.clone(), || workspace.restore(true, 0, &None)).unwrap();
    assert!(!fake.is_hidden(36_072), "the maximized window is shown");
    assert!(
        fake.window_commands()
            .ends_with(&[Command::Focus(36_072, true)]),
        "and has the focus, ahead of the window of the container: {:?}",
        fake.window_commands()
    );
}

#[test]
fn a_workspace_with_a_maximized_window_is_not_closed() {
    let (fake, mut wm) = desktop_with(&[]);
    send(&fake, &mut wm, SocketMessage::EnsureWorkspaces(0, 2));
    send(&fake, &mut wm, SocketMessage::FocusWorkspaceNumber(1));
    fake.open_window(36_081, "foot", "foot", "terminal 36081");
    event(&fake, &mut wm, added(36_081));
    send(&fake, &mut wm, SocketMessage::ToggleMaximize);
    assert!(wm.focused_workspace().unwrap().containers().is_empty());

    send(&fake, &mut wm, SocketMessage::CloseWorkspace);

    assert_eq!(wm.focused_monitor().unwrap().workspaces().len(), 2);
    assert_eq!(maximized(&wm), Some(36_081));
}

#[test]
fn eager_focus_finds_the_maximized_window_of_an_executable() {
    let (fake, mut wm) = desktop_with(&[36_091]);
    fake.open_window(36_092, "kate", "kate", "terminal 36092");
    event(&fake, &mut wm, added(36_092));
    send(&fake, &mut wm, SocketMessage::ToggleMaximize);
    event(&fake, &mut wm, activated(36_091));
    assert_eq!(
        fake.foreground_window().and_then(|id| id.to_u32()),
        Some(36_091)
    );

    send(
        &fake,
        &mut wm,
        SocketMessage::EagerFocus(String::from("kate")),
    );

    assert_eq!(
        fake.foreground_window().and_then(|id| id.to_u32()),
        Some(36_092)
    );
}
