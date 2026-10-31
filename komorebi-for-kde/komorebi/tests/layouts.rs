//! Changing, cycling and flipping the layout of the focused workspace, turning its tiling off and
//! on, and preselecting the place of the next window, against an in-memory desktop: the messages
//! go in through [`WindowManager::process_command`], and what comes out is where the backend is
//! asked to put each window.
//!
//! Window ids are in the 21_000s.

use komorebi::backend::fake::FakeBackend;
use komorebi::backend::with_backend;
use komorebi::core::Axis;
use komorebi::core::CycleDirection;
use komorebi::core::DefaultLayout;
use komorebi::core::OperationDirection;
use komorebi::core::Rect;
use komorebi::core::SocketMessage;
use komorebi::core::StateQuery;
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
/// `ids`, opened in that order. The last one has the focus.
fn desktop_with(ids: &[u32]) -> (Arc<FakeBackend>, WindowManager) {
    let fake = Arc::new(FakeBackend::with_one_monitor());
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
            open(&fake, &mut wm, id);
        }
        wm
    });

    (fake, wm)
}

fn open(fake: &Arc<FakeBackend>, wm: &mut WindowManager, id: u32) {
    fake.open_window(id, "foot", "foot", &format!("terminal {id}"));
    with_backend(fake.clone(), || wm.process_event(added(id))).unwrap();
}

fn send(fake: &Arc<FakeBackend>, wm: &mut WindowManager, message: SocketMessage) {
    with_backend(fake.clone(), || wm.process_command(message, Vec::new())).unwrap();
}

fn layout(fake: &Arc<FakeBackend>, wm: &mut WindowManager) -> String {
    let mut reply = Vec::new();
    with_backend(fake.clone(), || {
        wm.process_command(
            SocketMessage::Query(StateQuery::FocusedWorkspaceLayout),
            &mut reply,
        )
    })
    .unwrap();
    String::from_utf8(reply).unwrap()
}

fn rects(fake: &FakeBackend, ids: &[u32]) -> Vec<Rect> {
    ids.iter().map(|&id| fake.rect_of(id).unwrap()).collect()
}

/// The window of each container of the focused workspace, in order (0 for an empty container)
fn order(wm: &WindowManager) -> Vec<u32> {
    wm.focused_workspace()
        .unwrap()
        .containers()
        .iter()
        .map(|container| container.focused_window().map_or(0, |window| window.id))
        .collect()
}

#[test]
fn change_layout_tiles_the_workspace_again_in_the_layout_it_names() {
    let ids = [21_001, 21_002, 21_003];
    let (fake, mut wm) = desktop_with(&ids);
    assert_eq!(layout(&fake, &mut wm), "BSP");

    send(
        &fake,
        &mut wm,
        SocketMessage::ChangeLayout(DefaultLayout::Columns),
    );

    assert_eq!(layout(&fake, &mut wm), "Columns");
    let tiles = rects(&fake, &ids);
    assert!(
        tiles
            .iter()
            .all(|tile| tile.top == tiles[0].top && tile.bottom == tiles[0].bottom),
        "columns are as high as each other: {tiles:?}"
    );
    assert!(
        tiles[0].left < tiles[1].left && tiles[1].left < tiles[2].left,
        "the columns are in the order of the containers: {tiles:?}"
    );
}

#[test]
fn cycle_layout_goes_through_the_layouts_in_order_and_wraps_around() {
    let (fake, mut wm) = desktop_with(&[21_011]);

    send(
        &fake,
        &mut wm,
        SocketMessage::CycleLayout(CycleDirection::Next),
    );
    assert_eq!(layout(&fake, &mut wm), "Columns");

    send(
        &fake,
        &mut wm,
        SocketMessage::CycleLayout(CycleDirection::Previous),
    );
    assert_eq!(layout(&fake, &mut wm), "BSP");

    let mut seen = vec![];
    for _ in 0..9 {
        send(
            &fake,
            &mut wm,
            SocketMessage::CycleLayout(CycleDirection::Next),
        );
        seen.push(layout(&fake, &mut wm));
    }
    assert_eq!(
        seen,
        [
            "Columns",
            "Rows",
            "VerticalStack",
            "HorizontalStack",
            "UltrawideVerticalStack",
            "Grid",
            "RightMainVerticalStack",
            "Scrolling",
            "BSP",
        ],
        "the one after the last is the first"
    );

    // komorebi-layouts leaves the scrolling layout out going backwards
    send(
        &fake,
        &mut wm,
        SocketMessage::CycleLayout(CycleDirection::Previous),
    );
    assert_eq!(layout(&fake, &mut wm), "RightMainVerticalStack");
}

#[test]
fn flip_layout_mirrors_the_tiles_and_the_same_flip_again_puts_them_back() {
    let ids = [21_021, 21_022];
    let (fake, mut wm) = desktop_with(&ids);
    let before = rects(&fake, &ids);
    assert!(before[0].left < before[1].left);

    send(&fake, &mut wm, SocketMessage::FlipLayout(Axis::Horizontal));

    let flipped = rects(&fake, &ids);
    assert_eq!(
        flipped[0].left, before[1].left,
        "the first window has the right tile"
    );
    assert_eq!(
        flipped[1].left, before[0].left,
        "the second window has the left tile"
    );

    send(&fake, &mut wm, SocketMessage::FlipLayout(Axis::Horizontal));

    assert_eq!(rects(&fake, &ids), before);
}

#[test]
fn with_tiling_off_no_window_is_moved_until_it_is_back_on() {
    let ids = [21_031, 21_032];
    let (fake, mut wm) = desktop_with(&ids);
    let tiles = rects(&fake, &ids);

    send(&fake, &mut wm, SocketMessage::ToggleTiling);
    assert_eq!(layout(&fake, &mut wm), "None");

    let by_hand = Rect {
        left: 300,
        top: 300,
        right: 400,
        bottom: 400,
    };
    fake.move_window_by_user(21_031, by_hand);
    open(&fake, &mut wm, 21_033);
    send(&fake, &mut wm, SocketMessage::Retile);

    assert_eq!(
        fake.rect_of(21_031),
        Some(by_hand),
        "moved back by the retile"
    );
    assert_eq!(
        fake.rect_of(21_033).map(|rect| (rect.right, rect.bottom)),
        Some((800, 600)),
        "the new window keeps the size it opened with"
    );
    assert_eq!(
        order(&wm),
        [21_031, 21_032, 21_033],
        "it is in a container all the same"
    );

    send(&fake, &mut wm, SocketMessage::ToggleTiling);

    assert_eq!(layout(&fake, &mut wm), "BSP");
    assert_ne!(fake.rect_of(21_031), Some(by_hand));
    assert_ne!(
        rects(&fake, &ids),
        tiles,
        "three windows share the work area now"
    );
}

#[test]
fn preselect_reserves_a_tile_that_the_next_window_takes() {
    let ids = [21_041, 21_042];
    let (fake, mut wm) = desktop_with(&ids);
    let before = rects(&fake, &ids);

    send(
        &fake,
        &mut wm,
        SocketMessage::PreselectDirection(OperationDirection::Left),
    );

    assert_eq!(
        order(&wm),
        [21_041, 0, 21_042],
        "an empty container holds the place"
    );
    assert_ne!(
        fake.rect_of(21_042),
        Some(before[1]),
        "the reserved tile takes room"
    );

    open(&fake, &mut wm, 21_043);

    assert_eq!(order(&wm), [21_041, 21_043, 21_042]);
    assert_eq!(
        wm.focused_workspace().unwrap().preselected_container_idx,
        None
    );
}

#[test]
fn cancel_preselect_gives_the_reserved_tile_back() {
    let ids = [21_051, 21_052];
    let (fake, mut wm) = desktop_with(&ids);
    let before = rects(&fake, &ids);

    send(
        &fake,
        &mut wm,
        SocketMessage::PreselectDirection(OperationDirection::Left),
    );
    send(&fake, &mut wm, SocketMessage::CancelPreselect);

    assert_eq!(order(&wm), ids);
    assert_eq!(rects(&fake, &ids), before);
}

#[test]
fn a_second_preselect_is_ignored_while_a_place_is_reserved() {
    let (fake, mut wm) = desktop_with(&[21_061, 21_062]);

    send(
        &fake,
        &mut wm,
        SocketMessage::PreselectDirection(OperationDirection::Left),
    );
    send(
        &fake,
        &mut wm,
        SocketMessage::PreselectDirection(OperationDirection::Left),
    );

    assert_eq!(order(&wm), [21_061, 0, 21_062]);
}

#[test]
fn the_grid_layout_has_no_preselect() {
    let (fake, mut wm) = desktop_with(&[21_071, 21_072]);
    send(
        &fake,
        &mut wm,
        SocketMessage::ChangeLayout(DefaultLayout::Grid),
    );

    send(
        &fake,
        &mut wm,
        SocketMessage::PreselectDirection(OperationDirection::Left),
    );

    assert_eq!(order(&wm), [21_071, 21_072]);
}
