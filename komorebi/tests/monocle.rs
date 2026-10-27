//! The monocle of `toggle-monocle`, against an in-memory desktop: the messages go in through
//! [`WindowManager::process_command`] and the events through [`WindowManager::process_event`], and
//! what comes out is which containers the layout tiles, which window is the monocle and where the
//! backend is asked to put the windows, and which windows are out of sight.
//!
//! Window ids are in the 35_000s.

use komorebi::backend::Backend;
use komorebi::backend::fake::Command;
use komorebi::backend::fake::FakeBackend;
use komorebi::backend::with_backend;
use komorebi::core::MonocleFocusBehaviour;
use komorebi::core::OperationDirection;
use komorebi::core::Rect;
use komorebi::core::Sizing;
use komorebi::core::SocketMessage;
use komorebi::state::State;
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

fn removed(id: u32) -> WindowManagerEvent {
    WindowManagerEvent::Destroy(
        SystemNotification::KWin(KWinNotification::WindowRemoved),
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

    (fake, wm)
}

/// The bounds of the monitor on the right of the one of `with_one_monitor`, which is also its work
/// area
fn right_half() -> Rect {
    Rect {
        left: 1920,
        top: 0,
        right: 1920,
        bottom: 1080,
    }
}

/// A window manager over a fake desktop with two monitors side by side, and a window for each of
/// `left` on the one on the left and of `right` on the one on the right, opened in that order
fn two_monitors_with(left: &[u32], right: &[u32]) -> (Arc<FakeBackend>, WindowManager) {
    let fake = FakeBackend::with_one_monitor();
    fake.add_monitor(2, "Virtual-1", right_half(), right_half());
    let fake = Arc::new(fake);
    let (_sender, receiver) = crossbeam_channel::bounded(1);
    // unix socket paths are limited to ~104 bytes on macOS, which the temp dir alone can exceed
    static NEXT: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(1000);
    let socket = std::path::PathBuf::from(format!(
        "/tmp/kmm-{}-{}.sock",
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

/// The state the daemon shows, as JSON: a window is serialized with what the backend knows of it
fn state_of(fake: &Arc<FakeBackend>, wm: &WindowManager) -> serde_json::Value {
    with_backend(fake.clone(), || {
        serde_json::to_value(State::from(wm)).unwrap()
    })
}

/// The window of the monocle container, if there is one
fn monocle(wm: &WindowManager) -> Option<u32> {
    wm.focused_workspace()
        .unwrap()
        .monocle_container
        .as_ref()
        .map(|container| container.focused_window().unwrap().id)
}

/// Where the one window of a workspace of its own is put: the work area less the paddings
fn alone(id: u32) -> komorebi::core::Rect {
    let (fake, _wm) = desktop_with(&[id]);
    fake.rect_of(id).unwrap()
}

#[test]
fn toggle_monocle_gives_the_focused_window_the_work_area_and_toggling_again_puts_it_back() {
    let (fake, mut wm) = desktop_with(&[35_001, 35_002, 35_003]);
    event(&fake, &mut wm, activated(35_002));
    let tile = fake.rect_of(35_002).unwrap();
    let others = (fake.rect_of(35_001).unwrap(), fake.rect_of(35_003).unwrap());

    send(&fake, &mut wm, SocketMessage::ToggleMonocle);

    assert_eq!(monocle(&wm), Some(35_002));
    assert_eq!(tiled(&wm), [35_001, 35_003]);
    assert_eq!(fake.rect_of(35_002), Some(alone(35_009)));
    assert_ne!(fake.rect_of(35_002), Some(tile));
    assert!(
        fake.window_commands()
            .ends_with(&[Command::Focus(35_002, true)]),
        "the monocle window is given the focus"
    );
    assert_eq!(
        (fake.rect_of(35_001).unwrap(), fake.rect_of(35_003).unwrap()),
        others,
        "the windows behind it are not moved"
    );
    assert!(fake.is_hidden(35_001) && fake.is_hidden(35_003) && !fake.is_hidden(35_002));
    assert!(wm.focused_workspace().unwrap().contains_window(35_002));
    assert!(
        wm.focused_workspace()
            .unwrap()
            .contains_managed_window(35_002)
    );

    send(&fake, &mut wm, SocketMessage::ToggleMonocle);

    assert_eq!(monocle(&wm), None);
    assert_eq!(tiled(&wm), [35_001, 35_002, 35_003]);
    assert_eq!(fake.rect_of(35_002), Some(tile));
    assert_eq!(wm.focused_workspace().unwrap().focused_container_idx(), 1);
    assert!(
        [35_001, 35_002, 35_003]
            .iter()
            .all(|id| !fake.is_hidden(*id))
    );
}

#[test]
fn a_monocle_hides_the_other_windows_until_it_is_turned_off() {
    let (fake, mut wm) = desktop_with(&[35_091, 35_092, 35_093]);
    event(&fake, &mut wm, activated(35_092));
    send(&fake, &mut wm, SocketMessage::ToggleMonocle);
    send(
        &fake,
        &mut wm,
        SocketMessage::MonocleFocusBehaviour(MonocleFocusBehaviour::Cycle),
    );
    let hidden = |ids: [u32; 3]| ids.map(|id| fake.is_hidden(id));

    assert_eq!(hidden([35_091, 35_092, 35_093]), [true, false, true]);

    send(
        &fake,
        &mut wm,
        SocketMessage::FocusWindow(OperationDirection::Right),
    );
    assert_eq!(monocle(&wm), Some(35_093));
    assert_eq!(
        hidden([35_091, 35_092, 35_093]),
        [true, true, false],
        "the container the monocle gave up is hidden, and the one it took is not"
    );

    send(&fake, &mut wm, SocketMessage::ToggleMonocle);
    assert_eq!(hidden([35_091, 35_092, 35_093]), [false, false, false]);
}

#[test]
fn the_back_of_a_stack_stays_hidden_in_monocle_and_after_it() {
    let (fake, mut wm) = desktop_with(&[35_101, 35_102, 35_103]);
    event(&fake, &mut wm, activated(35_102));
    send(
        &fake,
        &mut wm,
        SocketMessage::StackWindow(OperationDirection::Left),
    );
    let in_front = wm
        .focused_workspace()
        .unwrap()
        .focused_container()
        .unwrap()
        .focused_window()
        .unwrap()
        .id;
    let behind = if in_front == 35_102 { 35_101 } else { 35_102 };
    assert!(fake.is_hidden(behind));

    send(&fake, &mut wm, SocketMessage::ToggleMonocle);
    assert_eq!(monocle(&wm), Some(in_front));
    assert!(
        fake.is_hidden(behind),
        "a stack in monocle is still a stack"
    );
    assert!(fake.is_hidden(35_103));

    send(&fake, &mut wm, SocketMessage::ToggleMonocle);
    assert!(fake.is_hidden(behind));
    assert!(!fake.is_hidden(in_front) && !fake.is_hidden(35_103));
}

#[test]
fn a_monocle_goes_back_to_its_index_even_when_a_window_opens_meanwhile() {
    let (fake, mut wm) = desktop_with(&[35_011, 35_012, 35_013]);
    event(&fake, &mut wm, activated(35_011));
    send(&fake, &mut wm, SocketMessage::ToggleMonocle);

    fake.open_window(35_014, "foot", "foot", "terminal 35014");
    event(&fake, &mut wm, added(35_014));
    assert_eq!(monocle(&wm), Some(35_011));
    assert_eq!(fake.rect_of(35_011), Some(alone(35_019)));

    send(&fake, &mut wm, SocketMessage::ToggleMonocle);

    assert_eq!(tiled(&wm)[0], 35_011);
    assert_eq!(tiled(&wm).len(), 4);
}

#[test]
fn toggle_monocle_on_a_workspace_without_windows_is_an_error() {
    let (fake, mut wm) = desktop_with(&[]);

    assert_eq!(
        try_send(&fake, &mut wm, SocketMessage::ToggleMonocle),
        Err(String::from("there is no container"))
    );
    assert_eq!(monocle(&wm), None);
}

#[test]
fn a_window_that_takes_the_focus_from_the_monocle_is_made_to_give_it_back() {
    let (fake, mut wm) = desktop_with(&[35_021, 35_022]);
    event(&fake, &mut wm, activated(35_021));
    send(&fake, &mut wm, SocketMessage::ToggleMonocle);
    fake.clear_commands();

    event(&fake, &mut wm, activated(35_022));

    assert_eq!(monocle(&wm), Some(35_021));
    assert!(
        fake.commands().contains(&Command::Focus(35_021, false)),
        "{:?}",
        fake.commands()
    );
}

#[test]
fn with_the_cycle_behaviour_focusing_in_a_direction_moves_the_monocle_to_the_next_container() {
    let (fake, mut wm) = desktop_with(&[35_031, 35_032, 35_033]);
    event(&fake, &mut wm, activated(35_031));
    send(&fake, &mut wm, SocketMessage::ToggleMonocle);
    send(
        &fake,
        &mut wm,
        SocketMessage::MonocleFocusBehaviour(MonocleFocusBehaviour::Cycle),
    );

    send(
        &fake,
        &mut wm,
        SocketMessage::FocusWindow(OperationDirection::Right),
    );

    assert_eq!(monocle(&wm), Some(35_032));
    assert_eq!(tiled(&wm), [35_031, 35_033]);
    assert_eq!(fake.rect_of(35_032), Some(alone(35_039)));

    send(
        &fake,
        &mut wm,
        SocketMessage::FocusWindow(OperationDirection::Left),
    );

    assert_eq!(monocle(&wm), Some(35_031));
    assert_eq!(tiled(&wm), [35_032, 35_033]);

    send(
        &fake,
        &mut wm,
        SocketMessage::CycleFocusWindow(komorebi::core::CycleDirection::Next),
    );

    assert_eq!(monocle(&wm), Some(35_032));
    assert_eq!(tiled(&wm), [35_031, 35_033]);
}

#[test]
fn with_the_no_op_behaviour_focusing_in_a_direction_leaves_the_monocle_alone() {
    let (fake, mut wm) = desktop_with(&[35_041, 35_042]);
    event(&fake, &mut wm, activated(35_041));
    send(&fake, &mut wm, SocketMessage::ToggleMonocle);
    assert_eq!(
        State::from(&wm).monocle_focus_behaviour,
        MonocleFocusBehaviour::NoOp
    );

    assert_eq!(
        try_send(
            &fake,
            &mut wm,
            SocketMessage::FocusWindow(OperationDirection::Right)
        ),
        Err(String::from(
            "there is no container or monitor in this direction"
        ))
    );
    assert_eq!(monocle(&wm), Some(35_041));

    send(&fake, &mut wm, SocketMessage::ToggleMonocleFocusBehaviour);
    assert_eq!(
        State::from(&wm).monocle_focus_behaviour,
        MonocleFocusBehaviour::Cycle
    );
    send(&fake, &mut wm, SocketMessage::ToggleMonocleFocusBehaviour);
    assert_eq!(
        State::from(&wm).monocle_focus_behaviour,
        MonocleFocusBehaviour::NoOp
    );
}

#[test]
fn what_is_about_the_tiles_is_refused_or_ignored_while_a_monocle_is_on() {
    let (fake, mut wm) = desktop_with(&[35_051, 35_052]);
    event(&fake, &mut wm, activated(35_051));
    send(&fake, &mut wm, SocketMessage::ToggleMonocle);

    for message in [
        SocketMessage::MoveWindow(OperationDirection::Right),
        SocketMessage::CycleMoveWindow(komorebi::core::CycleDirection::Next),
    ] {
        let refusal = try_send(&fake, &mut wm, message).unwrap_err();
        assert!(refusal.contains("monocle"), "{refusal}");
    }

    send(&fake, &mut wm, SocketMessage::ToggleFloat);

    assert_eq!(monocle(&wm), Some(35_051));
    assert_eq!(tiled(&wm), [35_052]);
    assert!(
        wm.focused_workspace()
            .unwrap()
            .floating_windows()
            .is_empty()
    );
}

#[test]
fn closing_the_window_in_monocle_tiles_the_others_again() {
    let (fake, mut wm) = desktop_with(&[35_061, 35_062]);
    event(&fake, &mut wm, activated(35_061));
    send(&fake, &mut wm, SocketMessage::ToggleMonocle);

    fake.close_window_now(35_061);
    event(&fake, &mut wm, removed(35_061));

    assert_eq!(monocle(&wm), None);
    assert_eq!(tiled(&wm), [35_062]);
    assert!(!fake.is_hidden(35_062), "the window left is shown again");
    assert_eq!(fake.rect_of(35_062), Some(alone(35_069)));
    assert!(!wm.focused_workspace().unwrap().contains_window(35_061));
    assert!(
        wm.focused_workspace()
            .unwrap()
            .monocle_container_restore_idx
            .is_none()
    );
}

#[test]
fn a_tile_that_was_resized_is_the_size_it_was_after_a_monocle() {
    let (fake, mut wm) = desktop_with(&[35_081, 35_082]);
    send(
        &fake,
        &mut wm,
        SocketMessage::ResizeWindowEdge(OperationDirection::Left, Sizing::Increase),
    );
    let resized = (fake.rect_of(35_081).unwrap(), fake.rect_of(35_082).unwrap());
    event(&fake, &mut wm, activated(35_081));

    send(&fake, &mut wm, SocketMessage::ToggleMonocle);
    send(&fake, &mut wm, SocketMessage::ToggleMonocle);

    assert_eq!(
        (fake.rect_of(35_081).unwrap(), fake.rect_of(35_082).unwrap()),
        resized,
        "a monocle that is turned off puts the tiles back as they were resized"
    );
}

#[test]
fn the_state_shows_the_monocle_container_and_the_index_it_goes_back_to() {
    let (fake, mut wm) = desktop_with(&[35_071, 35_072]);
    event(&fake, &mut wm, activated(35_072));
    send(&fake, &mut wm, SocketMessage::ToggleMonocle);

    let state = state_of(&fake, &wm);
    let workspace = &state["monitors"]["elements"][0]["workspaces"]["elements"][0];

    assert_eq!(
        workspace["monocle_container"]["windows"]["elements"][0]["id"],
        35_072
    );
    assert_eq!(workspace["monocle_container_restore_idx"], 1);
    assert_eq!(state["monocle_focus_behaviour"], "NoOp");

    send(&fake, &mut wm, SocketMessage::ToggleMonocle);
    let state = state_of(&fake, &wm);
    let workspace = &state["monitors"]["elements"][0]["workspaces"]["elements"][0];

    assert!(workspace["monocle_container"].is_null());
    assert!(workspace.get("monocle_container_restore_idx").is_none());
}

#[test]
fn focusing_towards_a_monitor_in_monocle_gives_the_focus_to_its_monocle_window() {
    let (fake, mut wm) = two_monitors_with(&[35_111, 35_112], &[35_113, 35_114]);
    event(&fake, &mut wm, activated(35_113));
    send(&fake, &mut wm, SocketMessage::ToggleMonocle);
    event(&fake, &mut wm, activated(35_112));
    assert_eq!(wm.focused_monitor_idx(), 0);
    let before = wm.monitors()[1]
        .focused_workspace()
        .unwrap()
        .focused_container_idx();

    send(
        &fake,
        &mut wm,
        SocketMessage::FocusWindow(OperationDirection::Right),
    );

    assert_eq!(wm.focused_monitor_idx(), 1);
    assert_eq!(fake.foreground_window(), Some(35_113));
    assert_eq!(
        wm.monitors()[1]
            .focused_workspace()
            .unwrap()
            .focused_container_idx(),
        before,
        "no container of the monitor in monocle is picked by the side the focus came from"
    );
}

#[test]
fn a_container_moved_to_a_monitor_in_monocle_turns_the_monocle_off_there() {
    let (fake, mut wm) = two_monitors_with(&[35_121, 35_122], &[35_123, 35_124]);
    event(&fake, &mut wm, activated(35_123));
    send(&fake, &mut wm, SocketMessage::ToggleMonocle);
    assert!(fake.is_hidden(35_124));
    event(&fake, &mut wm, activated(35_122));

    send(
        &fake,
        &mut wm,
        SocketMessage::MoveWindow(OperationDirection::Right),
    );

    assert!(
        wm.monitors()[1]
            .focused_workspace()
            .unwrap()
            .monocle_container
            .is_none()
    );
    assert!(
        [35_121, 35_122, 35_123, 35_124]
            .iter()
            .all(|id| !fake.is_hidden(*id)),
        "the windows the monocle hid are shown again"
    );
}

#[test]
fn a_container_sent_to_a_workspace_in_monocle_turns_the_monocle_off_there() {
    let (fake, mut wm) = desktop_with(&[35_131]);
    send(&fake, &mut wm, SocketMessage::FocusWorkspaceNumber(1));
    for id in [35_132, 35_133] {
        fake.open_window(id, "foot", "foot", &format!("terminal {id}"));
        event(&fake, &mut wm, added(id));
    }
    event(&fake, &mut wm, activated(35_132));
    send(&fake, &mut wm, SocketMessage::ToggleMonocle);
    assert!(fake.is_hidden(35_133));
    send(&fake, &mut wm, SocketMessage::FocusWorkspaceNumber(0));
    event(&fake, &mut wm, activated(35_131));

    send(
        &fake,
        &mut wm,
        SocketMessage::SendContainerToWorkspaceNumber(1),
    );

    let workspace = &wm.focused_monitor().unwrap().workspaces()[1];
    assert!(workspace.monocle_container.is_none());
    assert_eq!(workspace.containers().len(), 3);
    assert!(
        !fake.is_hidden(35_133),
        "the window the monocle hid is shown again"
    );
}

#[test]
fn eager_focus_finds_the_window_of_an_executable_in_the_monocle() {
    let (fake, mut wm) = desktop_with(&[35_141]);
    fake.open_window(35_142, "kate", "kate", "terminal 35142");
    event(&fake, &mut wm, added(35_142));
    send(&fake, &mut wm, SocketMessage::ToggleMonocle);
    event(&fake, &mut wm, activated(35_141));
    assert_eq!(monocle(&wm), Some(35_142));

    send(
        &fake,
        &mut wm,
        SocketMessage::EagerFocus(String::from("kate")),
    );

    assert_eq!(fake.foreground_window(), Some(35_142));
}
