//! More than one monitor, against an in-memory desktop with two 1920x1080 monitors side by side:
//! the focus and a move that cross to the next monitor at the edge of one, the monitor commands,
//! moving workspaces between monitors, a window that opens on the monitor that is not focused, and
//! `eager-focus`. The messages go in through [`WindowManager::process_command`]; what comes out is
//! the monitors of the window manager and where the fake has the windows.
//!
//! Window ids are in the 32_000s.

use komorebi::backend::Backend;
use komorebi::backend::fake::FakeBackend;
use komorebi::backend::with_backend;
use komorebi::core::CycleDirection;
use komorebi::core::MoveBehaviour;
use komorebi::core::OperationDirection;
use komorebi::core::Rect;
use komorebi::core::SocketMessage;
use komorebi::window::Window;
use komorebi::window_manager::WindowManager;
use komorebi::window_manager_event::KWinNotification;
use komorebi::window_manager_event::SystemNotification;
use komorebi::window_manager_event::WindowManagerEvent;
use std::sync::Arc;

const LEFT: u32 = 1;
const RIGHT: u32 = 2;

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

/// The bounds of the monitor on the right, which is also its work area
fn right_half() -> Rect {
    Rect {
        left: 1920,
        top: 0,
        right: 1920,
        bottom: 1080,
    }
}

/// A window manager over a fake desktop with the monitor of `with_one_monitor` on the left and a
/// second one on the right
fn two_monitors() -> (Arc<FakeBackend>, WindowManager) {
    let fake = FakeBackend::with_one_monitor();
    fake.add_monitor(RIGHT, "Virtual-1", right_half(), right_half());
    let fake = Arc::new(fake);
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
        wm
    });

    (fake, wm)
}

/// Opens a window on the left monitor, or on the right one, and has the window manager show it
fn open(fake: &Arc<FakeBackend>, wm: &mut WindowManager, id: u32, on_right: bool) {
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
    with_backend(fake.clone(), || wm.process_event(added(id))).unwrap();
}

fn send(
    fake: &Arc<FakeBackend>,
    wm: &mut WindowManager,
    message: SocketMessage,
) -> color_eyre::Result<()> {
    with_backend(fake.clone(), || wm.process_command(message, Vec::new()))
}

/// The ids of the windows of the focused workspace of each monitor
fn windows(wm: &WindowManager) -> Vec<Vec<u32>> {
    wm.monitors()
        .iter()
        .map(|monitor| {
            monitor
                .focused_workspace()
                .unwrap()
                .containers()
                .iter()
                .flat_map(|container| container.windows().iter().map(|window| window.id))
                .collect()
        })
        .collect()
}

fn on_right(fake: &FakeBackend, id: u32) -> bool {
    fake.rect_of(id).is_some_and(|rect| rect.left >= 1920)
}

#[test]
fn a_window_that_opens_on_the_monitor_that_is_not_focused_focuses_it() {
    let (fake, mut wm) = two_monitors();
    open(&fake, &mut wm, 32_001, false);
    assert_eq!(wm.focused_monitor_idx(), 0);

    open(&fake, &mut wm, 32_002, true);

    assert_eq!(wm.focused_monitor_idx(), 1);
    assert_eq!(windows(&wm), [vec![32_001], vec![32_002]]);
    assert!(on_right(&fake, 32_002));
}

#[test]
fn the_focus_crosses_to_the_monitor_on_that_side() {
    let (fake, mut wm) = two_monitors();
    open(&fake, &mut wm, 32_011, true);
    open(&fake, &mut wm, 32_012, false);
    assert_eq!(wm.focused_monitor_idx(), 0);

    send(
        &fake,
        &mut wm,
        SocketMessage::FocusWindow(OperationDirection::Right),
    )
    .unwrap();

    assert_eq!(wm.focused_monitor_idx(), 1);
    assert_eq!(fake.foreground_window(), Some(32_011));

    // and with no monitor further right, it is an error that changes nothing
    let result = send(
        &fake,
        &mut wm,
        SocketMessage::FocusWindow(OperationDirection::Right),
    );
    assert_eq!(
        result.map_err(|error| error.to_string()),
        Err(String::from(
            "there is no container or monitor in this direction"
        ))
    );
    assert_eq!(wm.focused_monitor_idx(), 1);
}

#[test]
fn a_move_at_the_edge_of_a_monitor_swaps_with_the_window_on_the_next_one_by_default() {
    let (fake, mut wm) = two_monitors();
    open(&fake, &mut wm, 32_021, true);
    open(&fake, &mut wm, 32_022, false);

    send(
        &fake,
        &mut wm,
        SocketMessage::MoveWindow(OperationDirection::Right),
    )
    .unwrap();

    assert_eq!(windows(&wm), [vec![32_021], vec![32_022]]);
    assert_eq!(wm.focused_monitor_idx(), 1);
    assert!(on_right(&fake, 32_022) && !on_right(&fake, 32_021));
}

#[test]
fn with_insert_a_move_at_the_edge_puts_the_window_in_among_those_of_the_next_monitor() {
    let (fake, mut wm) = two_monitors();
    open(&fake, &mut wm, 32_031, true);
    open(&fake, &mut wm, 32_032, false);
    send(
        &fake,
        &mut wm,
        SocketMessage::CrossMonitorMoveBehaviour(MoveBehaviour::Insert),
    )
    .unwrap();

    send(
        &fake,
        &mut wm,
        SocketMessage::MoveWindow(OperationDirection::Right),
    )
    .unwrap();

    assert!(windows(&wm)[0].is_empty());
    let mut right = windows(&wm)[1].clone();
    right.sort_unstable();
    assert_eq!(right, [32_031, 32_032]);
    assert!(on_right(&fake, 32_031) && on_right(&fake, 32_032));
}

#[test]
fn with_no_op_a_move_at_the_edge_moves_nothing() {
    let (fake, mut wm) = two_monitors();
    open(&fake, &mut wm, 32_041, true);
    open(&fake, &mut wm, 32_042, false);
    send(
        &fake,
        &mut wm,
        SocketMessage::CrossMonitorMoveBehaviour(MoveBehaviour::NoOp),
    )
    .unwrap();
    fake.clear_commands();

    send(
        &fake,
        &mut wm,
        SocketMessage::MoveWindow(OperationDirection::Right),
    )
    .unwrap();

    assert_eq!(windows(&wm), [vec![32_042], vec![32_041]]);
    assert_eq!(wm.focused_monitor_idx(), 0);
}

#[test]
fn toggle_cross_monitor_move_behaviour_switches_between_swap_and_insert() {
    let (fake, mut wm) = two_monitors();

    send(
        &fake,
        &mut wm,
        SocketMessage::ToggleCrossMonitorMoveBehaviour,
    )
    .unwrap();
    assert_eq!(wm.cross_monitor_move_behaviour, MoveBehaviour::Insert);
    send(
        &fake,
        &mut wm,
        SocketMessage::ToggleCrossMonitorMoveBehaviour,
    )
    .unwrap();
    assert_eq!(wm.cross_monitor_move_behaviour, MoveBehaviour::Swap);
}

#[test]
fn a_monitor_is_focused_by_its_index_or_in_order() {
    let (fake, mut wm) = two_monitors();
    open(&fake, &mut wm, 32_051, false);
    open(&fake, &mut wm, 32_052, true);

    send(&fake, &mut wm, SocketMessage::FocusMonitorNumber(0)).unwrap();
    assert_eq!(wm.focused_monitor_idx(), 0);
    assert_eq!(fake.foreground_window(), Some(32_051));

    send(
        &fake,
        &mut wm,
        SocketMessage::CycleFocusMonitor(CycleDirection::Next),
    )
    .unwrap();
    assert_eq!(wm.focused_monitor_idx(), 1);
    assert_eq!(fake.foreground_window(), Some(32_052));

    assert!(send(&fake, &mut wm, SocketMessage::FocusMonitorNumber(5)).is_err());
    assert_eq!(wm.focused_monitor_idx(), 1);
}

#[test]
fn move_to_monitor_follows_the_window_and_send_to_monitor_does_not() {
    let (fake, mut wm) = two_monitors();
    open(&fake, &mut wm, 32_061, false);
    open(&fake, &mut wm, 32_062, false);

    send(
        &fake,
        &mut wm,
        SocketMessage::SendContainerToMonitorNumber(1),
    )
    .unwrap();
    assert_eq!(wm.focused_monitor_idx(), 0);
    assert_eq!(windows(&wm), [vec![32_061], vec![32_062]]);
    assert!(on_right(&fake, 32_062));

    send(
        &fake,
        &mut wm,
        SocketMessage::MoveContainerToMonitorNumber(1),
    )
    .unwrap();
    assert_eq!(wm.focused_monitor_idx(), 1);
    assert!(windows(&wm)[0].is_empty());
    assert!(on_right(&fake, 32_061) && on_right(&fake, 32_062));
}

#[test]
fn a_window_sent_to_a_workspace_of_another_monitor_goes_on_its_desktop() {
    let (fake, mut wm) = two_monitors();
    open(&fake, &mut wm, 32_071, false);
    open(&fake, &mut wm, 32_072, false);

    send(
        &fake,
        &mut wm,
        SocketMessage::SendContainerToMonitorWorkspaceNumber(1, 2),
    )
    .unwrap();

    assert_eq!(wm.focused_monitor_idx(), 0);
    let right = &wm.monitors()[1];
    assert_eq!(right.focused_workspace_idx(), 2);
    assert!(right.workspaces()[2].contains_window(32_072));
    assert_eq!(fake.workspace_of(32_072), Some(2));
}

#[test]
fn a_named_workspace_on_another_monitor_takes_the_window_sent_to_it() {
    let (fake, mut wm) = two_monitors();
    open(&fake, &mut wm, 32_081, false);
    open(&fake, &mut wm, 32_082, false);
    send(
        &fake,
        &mut wm,
        SocketMessage::WorkspaceName(1, 0, String::from("side")),
    )
    .unwrap();

    send(
        &fake,
        &mut wm,
        SocketMessage::MoveContainerToNamedWorkspace(String::from("side")),
    )
    .unwrap();

    assert_eq!(wm.focused_monitor_idx(), 1);
    assert_eq!(windows(&wm), [vec![32_081], vec![32_082]]);
}

#[test]
fn a_workspace_moves_to_another_monitor_and_workspaces_are_swapped_between_monitors() {
    let (fake, mut wm) = two_monitors();
    open(&fake, &mut wm, 32_091, false);
    open(&fake, &mut wm, 32_092, true);

    send(&fake, &mut wm, SocketMessage::FocusMonitorNumber(0)).unwrap();
    send(
        &fake,
        &mut wm,
        SocketMessage::SwapWorkspacesToMonitorNumber(1),
    )
    .unwrap();
    assert_eq!(windows(&wm), [vec![32_092], vec![32_091]]);

    send(&fake, &mut wm, SocketMessage::FocusMonitorNumber(0)).unwrap();
    send(
        &fake,
        &mut wm,
        SocketMessage::MoveWorkspaceToMonitorNumber(1),
    )
    .unwrap();
    assert_eq!(wm.focused_monitor_idx(), 1);
    let right = &wm.monitors()[1];
    assert_eq!(right.workspaces().len(), 2);
    assert!(right.focused_workspace().unwrap().contains_window(32_092));
}

#[test]
fn eager_focus_finds_the_window_of_an_executable_on_any_monitor_and_workspace() {
    let (fake, mut wm) = two_monitors();
    open(&fake, &mut wm, 32_101, false);
    fake.open_window(32_102, "firefox", "firefox", "a browser");
    with_backend(fake.clone(), || wm.process_event(added(32_102))).unwrap();
    // the browser on workspace 1 of the right monitor, and the focus back on the terminal
    send(
        &fake,
        &mut wm,
        SocketMessage::SendContainerToMonitorWorkspaceNumber(1, 1),
    )
    .unwrap();
    with_backend(fake.clone(), || wm.process_event(activated(32_101))).unwrap();
    send(&fake, &mut wm, SocketMessage::FocusMonitorNumber(0)).unwrap();

    send(
        &fake,
        &mut wm,
        SocketMessage::EagerFocus(String::from("firefox")),
    )
    .unwrap();

    assert_eq!(wm.focused_monitor_idx(), 1);
    assert_eq!(wm.focused_workspace_idx().unwrap(), 1);
    assert_eq!(fake.foreground_window(), Some(32_102));
    assert_eq!(fake.shown_workspace(RIGHT), 1);
    assert_eq!(fake.shown_workspace(LEFT), 0);
}
