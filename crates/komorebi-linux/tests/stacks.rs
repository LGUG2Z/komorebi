//! Stacking windows in one container, taking the windows a stack does not show out of sight, and
//! choosing which window of a stack is in front, against an in-memory desktop.
//!
//! A container tiles one window, so the other windows of a stack are hidden: the backend is asked
//! to take them out of sight, and the window manager records them as hidden so that what the
//! compositor reports about them is not mistaken for the user's doing. The messages go in through
//! [`WindowManager::process_command`], and what comes out is which windows the containers hold and
//! which of them the fake desktop has hidden.
//!
//! Window ids are in the 33_000s; the hidden-window table is process-wide.

use komorebi::backend::fake::FakeBackend;
use komorebi::backend::with_backend;
use komorebi::core::CycleDirection;
use komorebi::core::OperationDirection;
use komorebi::core::SocketMessage;
use komorebi::core::WindowContainerBehaviour;
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

fn removed(id: u32) -> WindowManagerEvent {
    WindowManagerEvent::Destroy(
        SystemNotification::KWin(KWinNotification::WindowRemoved),
        Window::from(id),
    )
}

fn minimized(id: u32) -> WindowManagerEvent {
    WindowManagerEvent::Minimize(
        SystemNotification::KWin(KWinNotification::WindowMinimized),
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
        "/tmp/kms-{}-{}.sock",
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

/// Opens a window, which KWin gives the focus, as it does a new window
fn open(fake: &Arc<FakeBackend>, wm: &mut WindowManager, id: u32) {
    fake.open_window(id, "foot", "foot", &format!("terminal {id}"));
    with_backend(fake.clone(), || wm.process_event(added(id))).unwrap();
    fake.set_foreground(Some(id));
    with_backend(fake.clone(), || wm.process_event(activated(id))).unwrap();
}

fn send(
    fake: &Arc<FakeBackend>,
    wm: &mut WindowManager,
    message: SocketMessage,
) -> color_eyre::Result<()> {
    with_backend(fake.clone(), || wm.process_command(message, Vec::new()))
}

fn process(
    fake: &Arc<FakeBackend>,
    wm: &mut WindowManager,
    event: WindowManagerEvent,
) -> color_eyre::Result<()> {
    with_backend(fake.clone(), || wm.process_event(event))
}

/// The ids of the windows of each container of the focused workspace, in order, the one in front
/// first
fn containers(wm: &WindowManager) -> Vec<Vec<u32>> {
    wm.focused_workspace()
        .unwrap()
        .containers()
        .iter()
        .map(|container| {
            let front = container.focused_window().unwrap().id;
            let mut ids = vec![front];
            ids.extend(
                container
                    .windows()
                    .iter()
                    .map(|window| window.id)
                    .filter(|id| *id != front),
            );
            ids
        })
        .collect()
}

/// The windows among `ids` that the fake desktop has hidden
fn hidden(fake: &FakeBackend, ids: &[u32]) -> Vec<u32> {
    ids.iter()
        .copied()
        .filter(|id| fake.is_hidden(*id))
        .collect()
}

#[test]
fn a_minimize_komorebi_asked_for_is_not_mistaken_for_the_user() {
    let (fake, mut wm) = desktop_with(&[33_001]);

    // komorebi hides it (as it does the back of a stack), and the compositor reports a minimize
    with_backend(fake.clone(), || Window::from(33_001).hide());
    process(&fake, &mut wm, minimized(33_001)).unwrap();

    assert!(wm.focused_workspace().unwrap().contains_window(33_001));
    assert!(!wm.minimized_windows.contains_key(&33_001));
}

#[test]
fn a_minimize_the_user_asked_for_takes_the_window_out_of_the_layout() {
    let (fake, mut wm) = desktop_with(&[33_011, 33_012]);

    process(&fake, &mut wm, minimized(33_011)).unwrap();

    assert!(!wm.focused_workspace().unwrap().contains_window(33_011));
    assert!(wm.minimized_windows.contains_key(&33_011));
}

#[test]
fn stacking_puts_two_windows_in_one_container_and_hides_the_one_behind() {
    let (fake, mut wm) = desktop_with(&[33_021, 33_022, 33_023]);
    let ids = [33_021, 33_022, 33_023];
    assert!(hidden(&fake, &ids).is_empty());

    send(
        &fake,
        &mut wm,
        SocketMessage::StackWindow(OperationDirection::Up),
    )
    .unwrap();

    // 33_023 is the bottom right tile and had the focus: it goes onto 33_022, above it
    assert_eq!(containers(&wm), [vec![33_021], vec![33_023, 33_022]]);
    assert_eq!(hidden(&fake, &ids), [33_022]);
    assert_eq!(
        fake.foreground_window().and_then(|id| id.to_u32()),
        Some(33_023)
    );
}

#[test]
fn stacking_where_the_layout_has_nothing_changes_nothing() {
    let (fake, mut wm) = desktop_with(&[33_031, 33_032]);

    // the focused container is the last one, and BSP has nothing after it
    send(
        &fake,
        &mut wm,
        SocketMessage::StackWindow(OperationDirection::Right),
    )
    .unwrap();

    assert_eq!(containers(&wm), [vec![33_031], vec![33_032]]);
    assert!(hidden(&fake, &[33_031, 33_032]).is_empty());
}

#[test]
fn unstacking_gives_the_window_in_front_a_container_of_its_own_and_shows_the_next() {
    let (fake, mut wm) = desktop_with(&[33_041, 33_042, 33_043]);
    let ids = [33_041, 33_042, 33_043];
    send(
        &fake,
        &mut wm,
        SocketMessage::StackWindow(OperationDirection::Up),
    )
    .unwrap();
    assert_eq!(hidden(&fake, &ids), [33_042]);

    send(&fake, &mut wm, SocketMessage::UnstackWindow).unwrap();

    assert_eq!(
        containers(&wm),
        [vec![33_041], vec![33_042], vec![33_043]],
        "the window that was in front gets a container after the stack"
    );
    assert!(hidden(&fake, &ids).is_empty());
}

#[test]
fn a_window_alone_in_its_container_cannot_be_unstacked() {
    let (fake, mut wm) = desktop_with(&[33_051, 33_052]);

    let error = send(&fake, &mut wm, SocketMessage::UnstackWindow).unwrap_err();

    assert_eq!(
        error.to_string(),
        "a container must have at least one window"
    );
    assert_eq!(containers(&wm), [vec![33_051], vec![33_052]]);
}

#[test]
fn stack_all_puts_every_window_in_one_container_and_unstack_all_gives_them_each_one_back() {
    let (fake, mut wm) = desktop_with(&[33_061, 33_062, 33_063]);
    let ids = [33_061, 33_062, 33_063];

    send(&fake, &mut wm, SocketMessage::StackAll).unwrap();

    assert_eq!(containers(&wm), [vec![33_063, 33_061, 33_062]]);
    assert_eq!(hidden(&fake, &ids), [33_061, 33_062]);

    send(&fake, &mut wm, SocketMessage::UnstackAll).unwrap();

    let containers = containers(&wm);
    assert_eq!(containers.len(), 3);
    assert!(containers.iter().all(|windows| windows.len() == 1));
    assert!(hidden(&fake, &ids).is_empty());
    assert_eq!(
        wm.focused_window().unwrap().id,
        33_063,
        "the window that had the focus keeps it"
    );
}

#[test]
fn closing_the_window_in_front_of_a_stack_brings_the_next_one_forward() {
    let (fake, mut wm) = desktop_with(&[33_071, 33_072]);
    let ids = [33_071, 33_072];
    send(&fake, &mut wm, SocketMessage::StackAll).unwrap();
    assert_eq!(containers(&wm), [vec![33_072, 33_071]]);
    assert_eq!(hidden(&fake, &ids), [33_071]);

    fake.close_window_now(33_072);
    process(&fake, &mut wm, removed(33_072)).unwrap();

    assert_eq!(containers(&wm), [vec![33_071]]);
    assert!(hidden(&fake, &ids).is_empty());
    assert_eq!(
        fake.foreground_window().and_then(|id| id.to_u32()),
        Some(33_071)
    );
}

#[test]
fn a_window_closed_while_it_is_hidden_leaves_the_stack_alone() {
    let (fake, mut wm) = desktop_with(&[33_081, 33_082]);
    send(&fake, &mut wm, SocketMessage::StackAll).unwrap();

    fake.close_window_now(33_081);
    process(&fake, &mut wm, removed(33_081)).unwrap();

    assert_eq!(containers(&wm), [vec![33_082]]);
    assert!(!fake.is_hidden(33_082));
}

#[test]
fn the_back_of_a_stack_stays_hidden_across_a_workspace_round_trip() {
    let (fake, mut wm) = desktop_with(&[33_091, 33_092]);
    let ids = [33_091, 33_092];
    send(&fake, &mut wm, SocketMessage::StackAll).unwrap();
    assert_eq!(hidden(&fake, &ids), [33_091]);

    send(&fake, &mut wm, SocketMessage::FocusWorkspaceNumber(1)).unwrap();
    send(&fake, &mut wm, SocketMessage::FocusWorkspaceNumber(0)).unwrap();

    assert_eq!(containers(&wm), [vec![33_092, 33_091]]);
    assert_eq!(hidden(&fake, &ids), [33_091]);
}

#[test]
fn a_stack_goes_to_another_workspace_whole_and_the_back_of_it_stays_hidden() {
    let (fake, mut wm) = desktop_with(&[33_101, 33_102]);
    let ids = [33_101, 33_102];
    send(&fake, &mut wm, SocketMessage::StackAll).unwrap();

    send(
        &fake,
        &mut wm,
        SocketMessage::SendContainerToWorkspaceNumber(1),
    )
    .unwrap();
    send(&fake, &mut wm, SocketMessage::FocusWorkspaceNumber(1)).unwrap();

    assert_eq!(
        wm.focused_workspace()
            .unwrap()
            .containers()
            .iter()
            .map(|container| container.windows().len())
            .collect::<Vec<_>>(),
        [2]
    );
    assert_eq!(fake.workspace_of(33_101), Some(1));
    assert_eq!(fake.workspace_of(33_102), Some(1));
    assert_eq!(hidden(&fake, &ids), [33_101]);
}

#[test]
fn with_append_a_window_that_opens_joins_the_focused_container() {
    let (fake, mut wm) = desktop_with(&[33_111]);
    let ids = [33_111, 33_112, 33_113];

    send(
        &fake,
        &mut wm,
        SocketMessage::ToggleWindowContainerBehaviour,
    )
    .unwrap();
    assert_eq!(
        wm.window_management_behaviour(0, 0).current_behaviour,
        WindowContainerBehaviour::Append
    );
    open(&fake, &mut wm, 33_112);
    open(&fake, &mut wm, 33_113);

    assert_eq!(containers(&wm), [vec![33_113, 33_111, 33_112]]);
    assert_eq!(hidden(&fake, &ids), [33_111, 33_112]);

    send(
        &fake,
        &mut wm,
        SocketMessage::ToggleWindowContainerBehaviour,
    )
    .unwrap();
    assert_eq!(
        wm.window_management_behaviour(0, 0).current_behaviour,
        WindowContainerBehaviour::Create
    );
}

#[test]
fn an_empty_workspace_has_nothing_to_append_to() {
    let (fake, mut wm) = desktop_with(&[]);

    send(
        &fake,
        &mut wm,
        SocketMessage::ToggleWindowContainerBehaviour,
    )
    .unwrap();
    open(&fake, &mut wm, 33_121);
    open(&fake, &mut wm, 33_122);

    assert_eq!(
        containers(&wm),
        [vec![33_122, 33_121]],
        "the second window joins the first, which got a container of its own"
    );
}

#[test]
fn the_behavior_of_one_workspace_does_not_change_the_others() {
    let (fake, mut wm) = desktop_with(&[33_131]);
    send(&fake, &mut wm, SocketMessage::EnsureWorkspaces(0, 2)).unwrap();

    send(
        &fake,
        &mut wm,
        SocketMessage::ToggleWorkspaceWindowContainerBehaviour,
    )
    .unwrap();

    assert_eq!(
        wm.window_management_behaviour(0, 0).current_behaviour,
        WindowContainerBehaviour::Append
    );
    assert_eq!(
        wm.window_management_behaviour(0, 1).current_behaviour,
        WindowContainerBehaviour::Create,
        "an empty workspace that is not set to append creates"
    );
    assert_eq!(
        wm.window_management_behaviour.current_behaviour,
        WindowContainerBehaviour::Create,
        "the window manager's own is unchanged"
    );

    send(
        &fake,
        &mut wm,
        SocketMessage::ToggleWorkspaceWindowContainerBehaviour,
    )
    .unwrap();
    assert_eq!(
        wm.window_management_behaviour(0, 0).current_behaviour,
        WindowContainerBehaviour::Create
    );
}

#[test]
fn with_the_float_override_a_window_that_opens_floats() {
    let (fake, mut wm) = desktop_with(&[33_141]);

    send(&fake, &mut wm, SocketMessage::ToggleFloatOverride).unwrap();
    fake.open_window(33_142, "foot", "foot", "terminal 33142");
    process(&fake, &mut wm, added(33_142)).unwrap();

    let workspace = wm.focused_workspace().unwrap();
    assert_eq!(containers(&wm), [vec![33_141]]);
    assert!(workspace.floating_windows().iter().any(|w| w.id == 33_142));

    send(&fake, &mut wm, SocketMessage::ToggleFloatOverride).unwrap();
    open(&fake, &mut wm, 33_143);
    assert_eq!(containers(&wm), [vec![33_141], vec![33_143]]);
}

#[test]
fn the_float_override_of_a_workspace_takes_the_opposite_of_the_global_one_the_first_time() {
    let (fake, mut wm) = desktop_with(&[33_151]);
    send(&fake, &mut wm, SocketMessage::ToggleFloatOverride).unwrap();
    assert!(wm.window_management_behaviour(0, 0).float_override);

    send(&fake, &mut wm, SocketMessage::ToggleWorkspaceFloatOverride).unwrap();
    assert!(
        !wm.window_management_behaviour(0, 0).float_override,
        "the workspace's own override is the opposite of the global one that was on"
    );
    assert!(wm.window_management_behaviour.float_override);

    send(&fake, &mut wm, SocketMessage::ToggleWorkspaceFloatOverride).unwrap();
    assert!(wm.window_management_behaviour(0, 0).float_override);
}

#[test]
fn restoring_every_window_brings_back_the_hidden_ones_unless_it_is_told_to_ignore_that() {
    let (fake, mut wm) = desktop_with(&[33_161, 33_162, 33_163]);
    let ids = [33_161, 33_162, 33_163];
    send(&fake, &mut wm, SocketMessage::StackAll).unwrap();
    assert_eq!(hidden(&fake, &ids).len(), 2);

    with_backend(fake.clone(), || wm.restore_all_windows(true)).unwrap();
    assert_eq!(
        hidden(&fake, &ids).len(),
        2,
        "with ignore_restore they stay hidden"
    );

    with_backend(fake.clone(), || wm.restore_all_windows(false)).unwrap();
    assert!(hidden(&fake, &ids).is_empty());
}

#[test]
fn the_state_has_the_new_window_behavior_and_the_float_override() {
    let (fake, mut wm) = desktop_with(&[33_171]);
    let mut state = Vec::new();

    send(
        &fake,
        &mut wm,
        SocketMessage::ToggleWindowContainerBehaviour,
    )
    .unwrap();
    send(&fake, &mut wm, SocketMessage::ToggleFloatOverride).unwrap();
    with_backend(fake.clone(), || {
        wm.process_command(SocketMessage::State, &mut state)
    })
    .unwrap();

    let state: serde_json::Value = serde_json::from_slice(&state).unwrap();
    assert_eq!(state["new_window_behaviour"], "Append");
    assert_eq!(state["float_override"], true);
}

/// A fake desktop with the windows `ids` in one stack, the last in front
fn stack_of(ids: &[u32]) -> (Arc<FakeBackend>, WindowManager) {
    let (fake, mut wm) = desktop_with(ids);
    send(&fake, &mut wm, SocketMessage::StackAll).unwrap();
    (fake, wm)
}

#[test]
fn cycling_the_stack_brings_the_next_window_to_the_front_and_hides_the_one_that_was() {
    let (fake, mut wm) = stack_of(&[33_201, 33_202, 33_203]);
    let ids = [33_201, 33_202, 33_203];
    assert_eq!(containers(&wm), [vec![33_203, 33_201, 33_202]]);

    // the stack is 33_201, 33_202, 33_203 with the last in front: next wraps to the first
    send(
        &fake,
        &mut wm,
        SocketMessage::CycleStack(CycleDirection::Next),
    )
    .unwrap();

    assert_eq!(wm.focused_window().unwrap().id, 33_201);
    assert_eq!(hidden(&fake, &ids), [33_202, 33_203]);
    assert_eq!(
        fake.foreground_window().and_then(|id| id.to_u32()),
        Some(33_201)
    );

    send(
        &fake,
        &mut wm,
        SocketMessage::CycleStack(CycleDirection::Previous),
    )
    .unwrap();

    assert_eq!(wm.focused_window().unwrap().id, 33_203);
    assert_eq!(hidden(&fake, &ids), [33_201, 33_202]);
    assert_eq!(
        fake.foreground_window().and_then(|id| id.to_u32()),
        Some(33_203)
    );
}

#[test]
fn cycling_a_window_that_is_alone_is_an_error() {
    let (fake, mut wm) = desktop_with(&[33_211, 33_212]);

    for message in [
        SocketMessage::CycleStack(CycleDirection::Next),
        SocketMessage::CycleStackIndex(CycleDirection::Previous),
    ] {
        let error = send(&fake, &mut wm, message).unwrap_err();
        assert_eq!(
            error.to_string(),
            "there is only one window in this container"
        );
    }
    assert_eq!(containers(&wm), [vec![33_211], vec![33_212]]);
}

#[test]
fn cycling_the_index_moves_the_front_window_along_the_stack_and_keeps_it_in_front() {
    let (fake, mut wm) = stack_of(&[33_221, 33_222, 33_223]);
    let ids = [33_221, 33_222, 33_223];
    let order = |wm: &WindowManager| -> Vec<u32> {
        wm.focused_workspace()
            .unwrap()
            .containers()
            .iter()
            .flat_map(|container| container.windows().iter().map(|window| window.id))
            .collect()
    };
    assert_eq!(order(&wm), [33_221, 33_222, 33_223]);

    // 33_223 is in front and last: the next place wraps to the first
    send(
        &fake,
        &mut wm,
        SocketMessage::CycleStackIndex(CycleDirection::Next),
    )
    .unwrap();

    assert_eq!(order(&wm), [33_223, 33_222, 33_221]);
    assert_eq!(wm.focused_window().unwrap().id, 33_223);
    assert_eq!(hidden(&fake, &ids), [33_221, 33_222]);
    assert_eq!(
        fake.foreground_window().and_then(|id| id.to_u32()),
        Some(33_223)
    );
}

#[test]
fn focus_stack_window_brings_the_window_at_an_index_to_the_front() {
    let (fake, mut wm) = stack_of(&[33_231, 33_232, 33_233]);
    let ids = [33_231, 33_232, 33_233];

    send(&fake, &mut wm, SocketMessage::FocusStackWindow(1)).unwrap();

    assert_eq!(wm.focused_window().unwrap().id, 33_232);
    assert_eq!(hidden(&fake, &ids), [33_231, 33_233]);
    assert_eq!(
        fake.foreground_window().and_then(|id| id.to_u32()),
        Some(33_232)
    );

    send(&fake, &mut wm, SocketMessage::FocusStackWindow(0)).unwrap();
    assert_eq!(wm.focused_window().unwrap().id, 33_231);
    assert_eq!(hidden(&fake, &ids), [33_232, 33_233]);
}

#[test]
fn focus_stack_window_refuses_an_index_the_stack_does_not_have() {
    let (fake, mut wm) = stack_of(&[33_241, 33_242]);

    let error = send(&fake, &mut wm, SocketMessage::FocusStackWindow(5)).unwrap_err();

    assert_eq!(
        error.to_string(),
        "there is no window in this container at index 5"
    );
    assert_eq!(wm.focused_window().unwrap().id, 33_242);
}

#[test]
fn focus_stack_window_on_a_window_that_is_alone_takes_index_0_only() {
    let (fake, mut wm) = desktop_with(&[33_251]);

    send(&fake, &mut wm, SocketMessage::FocusStackWindow(0)).unwrap();
    let error = send(&fake, &mut wm, SocketMessage::FocusStackWindow(1)).unwrap_err();

    assert_eq!(
        error.to_string(),
        "there is only one window in this container"
    );
}

#[test]
fn the_window_the_user_focuses_in_the_back_of_a_stack_comes_to_the_front() {
    let (fake, mut wm) = stack_of(&[33_261, 33_262]);
    let ids = [33_261, 33_262];
    assert_eq!(hidden(&fake, &ids), [33_261]);

    // the user raises the hidden window (a click in the task manager): KWin gives it the focus
    fake.set_foreground(Some(33_261));
    process(&fake, &mut wm, activated(33_261)).unwrap();

    assert_eq!(wm.focused_window().unwrap().id, 33_261);
    assert_eq!(hidden(&fake, &ids), [33_262]);
}
