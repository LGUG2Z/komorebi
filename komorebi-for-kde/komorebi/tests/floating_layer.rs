//! The floating layer of a workspace, against an in-memory desktop: which of the two layers a
//! workspace is on, `toggle-workspace-layer`, and what the focus, move and resize commands do to
//! the floating windows while it is the layer in front, and which windows float when they open on
//! it. The messages go in through
//! [`WindowManager::process_command`], the events through [`WindowManager::process_event`].
//!
//! Window ids are in the 37_000s.

use komorebi::backend::fake::Command;
use komorebi::backend::fake::FakeBackend;
use komorebi::backend::with_backend;
use komorebi::core::CycleDirection;
use komorebi::core::FloatingLayerBehaviour;
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
use komorebi::workspace::WorkspaceLayer;
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
    desktop_on(FakeBackend::with_one_monitor(), ids)
}

/// A window manager over `fake`, and a window for each of `ids`, opened in that order
fn desktop_on(fake: FakeBackend, ids: &[u32]) -> (Arc<FakeBackend>, WindowManager) {
    let fake = Arc::new(fake);
    let (_sender, receiver) = crossbeam_channel::bounded(1);
    // unix socket paths are limited to ~104 bytes on macOS, which the temp dir alone can exceed
    static NEXT: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);
    let socket = std::path::PathBuf::from(format!(
        "/tmp/kml-{}-{}.sock",
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

/// KWin gives the focus to a window: the window has it, and the window manager is told
fn focus(fake: &Arc<FakeBackend>, wm: &mut WindowManager, id: u32) {
    fake.set_foreground(Some(id));
    event(fake, wm, activated(id));
}

fn layer(wm: &WindowManager) -> WorkspaceLayer {
    wm.focused_workspace().unwrap().layer
}

fn tiled(wm: &WindowManager) -> Vec<u32> {
    wm.focused_workspace()
        .unwrap()
        .containers()
        .iter()
        .map(|container| container.focused_window().unwrap().id)
        .collect()
}

fn floating(wm: &WindowManager) -> Vec<u32> {
    wm.focused_workspace()
        .unwrap()
        .floating_windows()
        .iter()
        .map(|window| window.id)
        .collect()
}

/// Floats `ids`, in that order, each given the focus first, and puts them side by side along the
/// top of the work area, 400 pixels wide, 100 apart, so that the nearest on each side is known.
/// The last one has the focus, and the workspace is on its floating layer.
fn float_side_by_side(fake: &Arc<FakeBackend>, wm: &mut WindowManager, ids: &[u32]) {
    for (i, &id) in ids.iter().enumerate() {
        focus(fake, wm, id);
        send(fake, wm, SocketMessage::ToggleFloat);
        fake.move_window_by_user(
            id,
            Rect {
                left: 100 + 500 * i as i32,
                top: 200,
                right: 400,
                bottom: 300,
            },
        );
    }
}

/// The window the backend was last asked to give the focus to
fn last_focused(fake: &FakeBackend) -> Option<u32> {
    fake.commands()
        .iter()
        .rev()
        .find_map(|command| match command {
            Command::Focus(id, _) => Some(*id),
            _ => None,
        })
}

#[test]
fn floating_a_window_puts_the_workspace_on_the_floating_layer_and_focusing_a_tile_takes_it_back() {
    let (fake, mut wm) = desktop_with(&[37_001, 37_002]);
    assert_eq!(layer(&wm), WorkspaceLayer::Tiling);

    send(&fake, &mut wm, SocketMessage::ToggleFloat);

    assert_eq!(floating(&wm), [37_002]);
    assert_eq!(layer(&wm), WorkspaceLayer::Floating);

    focus(&fake, &mut wm, 37_001);

    assert_eq!(layer(&wm), WorkspaceLayer::Tiling);

    focus(&fake, &mut wm, 37_002);

    assert_eq!(layer(&wm), WorkspaceLayer::Floating);

    send(&fake, &mut wm, SocketMessage::ToggleFloat);

    assert_eq!(layer(&wm), WorkspaceLayer::Tiling);
    assert_eq!(tiled(&wm), [37_002, 37_001]);
}

#[test]
fn toggle_workspace_layer_raises_the_windows_of_the_layer_and_focuses_one_of_them() {
    let (fake, mut wm) = desktop_with(&[37_011, 37_012, 37_013]);
    focus(&fake, &mut wm, 37_011);
    send(&fake, &mut wm, SocketMessage::ToggleFloat);
    focus(&fake, &mut wm, 37_012);
    assert_eq!(layer(&wm), WorkspaceLayer::Tiling);
    fake.clear_commands();

    send(&fake, &mut wm, SocketMessage::ToggleWorkspaceLayer);

    assert_eq!(layer(&wm), WorkspaceLayer::Floating);
    assert!(
        fake.commands().contains(&Command::Raise(37_011)),
        "{:?}",
        fake.commands()
    );
    assert_eq!(last_focused(&fake), Some(37_011));
    fake.clear_commands();

    send(&fake, &mut wm, SocketMessage::ToggleWorkspaceLayer);

    assert_eq!(layer(&wm), WorkspaceLayer::Tiling);
    for id in [37_012, 37_013] {
        assert!(
            fake.commands().contains(&Command::Raise(id)),
            "the tile of {id} is raised: {:?}",
            fake.commands()
        );
    }
    assert_eq!(last_focused(&fake), Some(37_012), "the focused container's");
    assert_eq!(floating(&wm), [37_011], "nothing was floated or unfloated");
}

#[test]
fn focusing_in_a_direction_on_the_floating_layer_goes_to_the_nearest_floating_window() {
    let (fake, mut wm) = desktop_with(&[37_021, 37_022, 37_023]);
    float_side_by_side(&fake, &mut wm, &[37_021, 37_022, 37_023]);
    fake.set_foreground(Some(37_021));

    send(
        &fake,
        &mut wm,
        SocketMessage::FocusWindow(OperationDirection::Right),
    );
    assert_eq!(last_focused(&fake), Some(37_022));

    fake.set_foreground(Some(37_022));
    send(
        &fake,
        &mut wm,
        SocketMessage::FocusWindow(OperationDirection::Right),
    );
    assert_eq!(last_focused(&fake), Some(37_023));

    fake.set_foreground(Some(37_023));
    send(
        &fake,
        &mut wm,
        SocketMessage::FocusWindow(OperationDirection::Left),
    );
    assert_eq!(last_focused(&fake), Some(37_022));

    fake.set_foreground(Some(37_023));
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
    assert_eq!(layer(&wm), WorkspaceLayer::Floating);
}

#[test]
fn cycling_the_focus_on_the_floating_layer_goes_round_the_floating_windows_in_order() {
    let (fake, mut wm) = desktop_with(&[37_031, 37_032, 37_033]);
    float_side_by_side(&fake, &mut wm, &[37_031, 37_032, 37_033]);
    fake.set_foreground(Some(37_031));

    let mut visited = vec![];
    for direction in [
        CycleDirection::Next,
        CycleDirection::Next,
        CycleDirection::Next,
        CycleDirection::Previous,
        CycleDirection::Previous,
    ] {
        send(&fake, &mut wm, SocketMessage::CycleFocusWindow(direction));
        visited.push(last_focused(&fake).unwrap());
    }

    assert_eq!(visited, [37_032, 37_033, 37_031, 37_033, 37_032]);
}

#[test]
fn moving_on_the_floating_layer_moves_the_floating_window_by_the_resize_delta() {
    let (fake, mut wm) = desktop_with(&[37_041, 37_042]);
    float_side_by_side(&fake, &mut wm, &[37_041]);
    let before = fake.rect_of(37_041).unwrap();

    send(
        &fake,
        &mut wm,
        SocketMessage::MoveWindow(OperationDirection::Right),
    );

    let after = fake.rect_of(37_041).unwrap();
    assert_eq!(after.left, before.left + 50);
    assert_eq!((after.top, after.right, after.bottom), (200, 400, 300));
    assert_eq!(floating(&wm), [37_041]);
    assert_eq!(tiled(&wm), [37_042], "the tiles are not moved");

    for _ in 0..40 {
        send(
            &fake,
            &mut wm,
            SocketMessage::MoveWindow(OperationDirection::Up),
        );
    }
    assert_eq!(
        fake.rect_of(37_041).unwrap().top,
        0,
        "never out of the work area"
    );
}

#[test]
fn resizing_on_the_floating_layer_changes_the_frame_of_the_floating_window() {
    let (fake, mut wm) = desktop_with(&[37_051, 37_052]);
    float_side_by_side(&fake, &mut wm, &[37_051]);
    let before = fake.rect_of(37_051).unwrap();
    let tile = fake.rect_of(37_052).unwrap();

    send(
        &fake,
        &mut wm,
        SocketMessage::ResizeWindowEdge(OperationDirection::Right, Sizing::Increase),
    );

    let after = fake.rect_of(37_051).unwrap();
    assert_eq!(after.right, before.right + 100, "twice the resize delta");
    assert_eq!(after.left, before.left);
    assert_eq!(fake.rect_of(37_052), Some(tile), "the tile is where it was");
}

#[test]
fn a_direction_to_preselect_is_ignored_on_the_floating_layer() {
    let (fake, mut wm) = desktop_with(&[37_061, 37_062]);
    float_side_by_side(&fake, &mut wm, &[37_061]);

    send(
        &fake,
        &mut wm,
        SocketMessage::PreselectDirection(OperationDirection::Left),
    );

    assert!(
        wm.focused_workspace()
            .unwrap()
            .preselected_container_idx
            .is_none()
    );
    assert_eq!(tiled(&wm), [37_062]);
}

#[test]
fn a_floating_window_that_is_maximized_leaves_the_floating_windows_and_comes_back_as_a_tile() {
    let (fake, mut wm) = desktop_with(&[37_071, 37_072]);
    float_side_by_side(&fake, &mut wm, &[37_071]);

    send(&fake, &mut wm, SocketMessage::ToggleMaximize);

    let workspace = wm.focused_workspace().unwrap();
    assert_eq!(workspace.maximized_window.map(|w| w.id), Some(37_071));
    assert!(workspace.floating_windows().is_empty());
    assert!(fake.is_maximized(37_071));

    send(&fake, &mut wm, SocketMessage::ToggleMaximize);

    assert!(!fake.is_maximized(37_071));
    assert!(tiled(&wm).contains(&37_071));
    assert!(floating(&wm).is_empty());
}

#[test]
fn the_state_shows_the_layer() {
    let (fake, mut wm) = desktop_with(&[37_081, 37_082]);
    let layer_in_state = |fake: &Arc<FakeBackend>, wm: &WindowManager| {
        let state = with_backend(fake.clone(), || {
            serde_json::to_value(State::from(wm)).unwrap()
        });
        state["monitors"]["elements"][0]["workspaces"]["elements"][0]["layer"].clone()
    };
    assert_eq!(layer_in_state(&fake, &wm), "Tiling");

    send(&fake, &mut wm, SocketMessage::ToggleFloat);

    assert_eq!(layer_in_state(&fake, &wm), "Floating");
}

#[test]
fn going_back_to_the_tiles_hides_the_floating_windows_and_going_to_them_shows_them_again() {
    let (fake, mut wm) = desktop_with(&[37_061, 37_062, 37_063]);
    focus(&fake, &mut wm, 37_061);
    send(&fake, &mut wm, SocketMessage::ToggleFloat);
    focus(&fake, &mut wm, 37_062);
    assert!(!fake.is_hidden(37_061));

    send(&fake, &mut wm, SocketMessage::ToggleWorkspaceLayer);
    assert_eq!(layer(&wm), WorkspaceLayer::Floating);
    assert!(!fake.is_hidden(37_061));

    send(&fake, &mut wm, SocketMessage::ToggleWorkspaceLayer);
    assert_eq!(layer(&wm), WorkspaceLayer::Tiling);
    assert!(
        fake.is_hidden(37_061),
        "the floating window is out of sight"
    );
    assert!(!fake.is_hidden(37_062) && !fake.is_hidden(37_063));

    send(&fake, &mut wm, SocketMessage::ToggleWorkspaceLayer);
    assert!(!fake.is_hidden(37_061), "and it is shown again");
}

#[test]
fn the_floating_layer_behaviour_of_the_workspace_wins_over_the_monitors_and_the_window_managers() {
    let (fake, mut wm) = desktop_with(&[37_071]);
    send(&fake, &mut wm, SocketMessage::ToggleFloat);
    assert_eq!(layer(&wm), WorkspaceLayer::Floating);
    let overridden =
        |wm: &WindowManager| wm.window_management_behaviour(0, 0).floating_layer_override;

    assert!(!overridden(&wm), "windows are tiled by default");

    wm.window_management_behaviour.floating_layer_behaviour = FloatingLayerBehaviour::Float;
    assert!(overridden(&wm), "the window manager's behaviour");

    wm.focused_monitor_mut().unwrap().floating_layer_behaviour = Some(FloatingLayerBehaviour::Tile);
    assert!(
        !overridden(&wm),
        "the monitor's wins over the window manager's"
    );

    wm.focused_workspace_mut().unwrap().floating_layer_behaviour =
        Some(FloatingLayerBehaviour::Float);
    assert!(overridden(&wm), "the workspace's wins over the monitor's");

    wm.focused_workspace_mut().unwrap().layer = WorkspaceLayer::Tiling;
    assert!(
        !overridden(&wm),
        "only a workspace on its floating layer floats new windows"
    );
}

#[test]
fn a_window_that_opens_on_the_floating_layer_floats_when_the_behaviour_says_so() {
    let (fake, mut wm) = desktop_with(&[37_081, 37_082]);
    send(&fake, &mut wm, SocketMessage::ToggleFloat);
    wm.focused_workspace_mut().unwrap().floating_layer_behaviour =
        Some(FloatingLayerBehaviour::Float);

    fake.open_window(37_083, "foot", "foot", "terminal 37083");
    event(&fake, &mut wm, added(37_083));

    assert_eq!(floating(&wm), [37_082, 37_083]);
    assert_eq!(tiled(&wm), [37_081]);
    assert_eq!(layer(&wm), WorkspaceLayer::Floating);

    wm.focused_workspace_mut().unwrap().floating_layer_behaviour =
        Some(FloatingLayerBehaviour::Tile);
    fake.open_window(37_084, "foot", "foot", "terminal 37084");
    event(&fake, &mut wm, added(37_084));

    assert_eq!(tiled(&wm), [37_081, 37_084]);
    assert_eq!(layer(&wm), WorkspaceLayer::Tiling);
}

#[test]
fn a_window_sent_to_another_workspace_puts_that_workspace_on_the_layer_it_arrived_on() {
    let (fake, mut wm) = desktop_with(&[37_111, 37_112]);
    focus(&fake, &mut wm, 37_112);
    send(&fake, &mut wm, SocketMessage::ToggleFloat);
    assert_eq!(floating(&wm), [37_112]);

    send(
        &fake,
        &mut wm,
        SocketMessage::SendContainerToWorkspaceNumber(1),
    );
    let target = &wm.focused_monitor().unwrap().workspaces()[1];
    assert_eq!(target.floating_windows().len(), 1);
    assert_eq!(target.layer, WorkspaceLayer::Floating);

    focus(&fake, &mut wm, 37_111);
    send(
        &fake,
        &mut wm,
        SocketMessage::SendContainerToWorkspaceNumber(1),
    );
    let target = &wm.focused_monitor().unwrap().workspaces()[1];
    assert_eq!(target.containers().len(), 1);
    assert_eq!(target.layer, WorkspaceLayer::Tiling);
}

#[test]
fn focusing_a_floating_window_towards_a_monitor_crosses_to_it_and_to_its_monocle_window() {
    let right_half = Rect {
        left: 1920,
        top: 0,
        right: 1920,
        bottom: 1080,
    };
    let fake = FakeBackend::with_one_monitor();
    fake.add_monitor(2, "Virtual-1", right_half, right_half);
    let (fake, mut wm) = desktop_on(fake, &[37_121]);
    fake.open_window(37_122, "foot", "foot", "terminal 37122");
    fake.move_window_by_user(
        37_122,
        Rect {
            left: 2020,
            top: 100,
            right: 800,
            bottom: 600,
        },
    );
    event(&fake, &mut wm, added(37_122));
    focus(&fake, &mut wm, 37_122);
    send(&fake, &mut wm, SocketMessage::ToggleMonocle);
    focus(&fake, &mut wm, 37_121);
    send(&fake, &mut wm, SocketMessage::ToggleFloat);
    assert_eq!(wm.focused_monitor_idx(), 0);
    assert_eq!(layer(&wm), WorkspaceLayer::Floating);

    send(
        &fake,
        &mut wm,
        SocketMessage::FocusWindow(OperationDirection::Right),
    );

    assert_eq!(wm.focused_monitor_idx(), 1);
    assert_eq!(
        last_focused(&fake),
        Some(37_122),
        "the monocle window of the monitor"
    );
}
