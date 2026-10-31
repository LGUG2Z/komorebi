//! Resizing a tile by dragging an edge, against an in-memory desktop. See docs/drag-resize.md for
//! what went wrong and why these cases exist.
//!
//! A rect is `left, top, width, height` (`right` and `bottom` are the size); the work area of the
//! fake monitor is 1920x1050. Window ids are in the 10_000s, the 20_000s and the 53_000s.

use komorebi::backend::fake::FakeBackend;
use komorebi::backend::with_backend;
use komorebi::core::DefaultLayout;
use komorebi::core::OperationDirection;
use komorebi::core::Rect;
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

fn resize_started(id: u32) -> WindowManagerEvent {
    WindowManagerEvent::ResizeStart(
        SystemNotification::KWin(KWinNotification::InteractiveResizeStarted),
        Window::from(id),
    )
}

fn resize_finished(id: u32) -> WindowManagerEvent {
    WindowManagerEvent::ResizeEnd(
        SystemNotification::KWin(KWinNotification::InteractiveResizeFinished),
        Window::from(id),
    )
}

fn desktop() -> (Arc<FakeBackend>, WindowManager) {
    let fake = Arc::new(FakeBackend::with_one_monitor());
    let (_sender, receiver) = crossbeam_channel::bounded(1);
    static NEXT: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);
    let socket = std::path::PathBuf::from(format!(
        "/tmp/kmr-{}-{}.sock",
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

fn open(fake: &FakeBackend, id: u32) {
    fake.open_window(id, "foot", "foot", &format!("terminal {id}"));
}

/// Windows `first..first+count`, tiled in a layout, the last one focused
fn tiled(layout: DefaultLayout, first: u32, count: u32) -> (Arc<FakeBackend>, WindowManager) {
    let (fake, mut wm) = desktop();
    with_backend(fake.clone(), || {
        wm.process_command(SocketMessage::ChangeLayout(layout), Vec::new())
            .unwrap();
        for id in first..first + count {
            fake.open_window(id, "foot", "foot", &format!("terminal {id}"));
            wm.process_event(added(id)).unwrap();
        }
    });
    (fake, wm)
}

/// The user drags an edge of a window: `to` is what the frame is when they let go
fn drag(fake: &Arc<FakeBackend>, wm: &mut WindowManager, id: u32, to: Rect) {
    with_backend(fake.clone(), || {
        wm.process_event(resize_started(id)).unwrap();
        fake.move_window_by_user(id, to);
        wm.process_event(resize_finished(id)).unwrap();
    });
}

fn grown_left(r: Rect, by: i32) -> Rect {
    Rect {
        left: r.left - by,
        right: r.right + by,
        ..r
    }
}

fn overlap(a: &Rect, b: &Rect) -> bool {
    a.left < b.left + b.right
        && b.left < a.left + a.right
        && a.top < b.top + b.bottom
        && b.top < a.top + a.bottom
}

fn assert_tiling(fake: &FakeBackend, ids: &[u32]) {
    let rects: Vec<Rect> = ids.iter().map(|id| fake.rect_of(*id).unwrap()).collect();
    for (i, a) in rects.iter().enumerate() {
        assert!(
            a.left >= 0 && a.top >= 0 && a.left + a.right <= 1920 && a.top + a.bottom <= 1050,
            "{a:?} is outside the work area"
        );
        for b in &rects[i + 1..] {
            assert!(!overlap(a, b), "{a:?} and {b:?} overlap");
        }
    }
}

#[test]
fn resizing_a_tile_by_hand_moves_the_split() {
    let (fake, mut wm) = desktop();
    open(&fake, 10_101);
    open(&fake, 10_102);

    with_backend(fake.clone(), || {
        wm.process_event(added(10_101))?;
        wm.process_event(added(10_102))?;
        let b = fake.rect_of(10_102).unwrap();

        // the focused window is the right tile; the user drags its left edge 200px to the left
        wm.process_event(resize_started(10_102))?;
        fake.move_window_by_user(
            10_102,
            Rect {
                left: b.left - 200,
                right: b.right + 200,
                ..b
            },
        );
        wm.process_event(resize_finished(10_102))
    })
    .unwrap();

    let (a, b) = (fake.rect_of(10_101).unwrap(), fake.rect_of(10_102).unwrap());
    assert!(
        a.right < 900,
        "the left tile should have shrunk: {a:?} {b:?}"
    );
    assert!(
        b.right > 1000,
        "the right tile should have grown: {a:?} {b:?}"
    );
    assert!(!overlap(&a, &b));
}

#[test]
fn a_long_drag_of_a_narrow_tile_is_not_dropped() {
    // an ultrawide layout's right-hand stack is the narrowest thing there is, and the layout used to
    // refuse any drag of more than half of a tile's own width, without a word
    let (fake, mut wm) = tiled(DefaultLayout::UltrawideVerticalStack, 20_001, 5);
    // 20_001 is the center, 20_002 the left column, 20_003..20_005 the stack on the right
    let tile = fake.rect_of(20_003).unwrap();
    assert!(tile.left > 1000, "{tile:?} should be on the right");

    // the focused window is 20_005, not the one being dragged
    drag(&fake, &mut wm, 20_003, grown_left(tile, 300));

    let after = fake.rect_of(20_003).unwrap();
    assert!(
        after.right >= tile.right + 290,
        "dragged by 300 and got {tile:?} -> {after:?}"
    );
    let center = fake.rect_of(20_001).unwrap();
    assert!(center.right > 0 && center.left + center.right <= after.left);
    assert_tiling(&fake, &[20_001, 20_002, 20_003, 20_004, 20_005]);

    // and the whole stack moved with it: they share the edge
    for id in [20_004, 20_005] {
        assert_eq!(fake.rect_of(id).unwrap().left, after.left, "window {id}");
    }
}

#[test]
fn a_drag_further_than_the_neighbour_can_give_goes_as_far_as_it_can() {
    let (fake, mut wm) = tiled(DefaultLayout::UltrawideVerticalStack, 20_011, 5);
    let tile = fake.rect_of(20_013).unwrap();

    // far past the center tile's whole width
    drag(&fake, &mut wm, 20_013, grown_left(tile, 1000));

    let after = fake.rect_of(20_013).unwrap();
    let center = fake.rect_of(20_011).unwrap();
    assert!(
        after.right > tile.right + 400,
        "it grew: {tile:?} -> {after:?}"
    );
    // the floor is a tenth of the work area, before the paddings the layout then takes off
    assert!(
        center.right >= 140 && center.right < 300,
        "the center tile is squeezed to about a tenth of the screen, not out of existence: {center:?}"
    );
    assert_tiling(&fake, &[20_011, 20_012, 20_013, 20_014, 20_015]);
}

#[test]
fn shrinking_a_tile_by_more_than_it_can_is_granted_as_far_as_it_goes() {
    let (fake, mut wm) = tiled(DefaultLayout::UltrawideVerticalStack, 20_021, 5);
    let tile = fake.rect_of(20_023).unwrap();

    // the user pulls the left edge inward by more than the tile is wide
    drag(
        &fake,
        &mut wm,
        20_023,
        Rect {
            left: tile.left + tile.right - 20,
            right: 20,
            ..tile
        },
    );

    let after = fake.rect_of(20_023).unwrap();
    assert!(after.right < tile.right, "it shrank: {tile:?} -> {after:?}");
    assert!(after.right >= 140, "but not to nothing: {after:?}");
    assert_tiling(&fake, &[20_021, 20_022, 20_023, 20_024, 20_025]);
}

#[test]
fn a_drag_is_measured_from_where_the_window_was_not_from_its_tile() {
    // a terminal snaps its size to its cells, so it never quite fills its tile; the horizontal split
    // between the two windows on the right must stay where it is when the *left* edge is dragged
    let (fake, mut wm) = tiled(DefaultLayout::BSP, 20_031, 3);
    let top_right = fake.rect_of(20_032).unwrap();
    let split_before = fake.rect_of(20_033).unwrap().top;

    let snapped = Rect {
        right: top_right.right - 1,
        bottom: top_right.bottom - 8,
        ..top_right
    };
    fake.move_window_by_user(20_032, snapped);

    drag(&fake, &mut wm, 20_032, grown_left(snapped, 100));

    let after = fake.rect_of(20_032).unwrap();
    assert!(
        after.left <= top_right.left - 90,
        "{top_right:?} -> {after:?}"
    );
    assert_eq!(
        fake.rect_of(20_033).unwrap().top,
        split_before,
        "the split between the two on the right moved, though only a left edge was dragged"
    );
    assert_tiling(&fake, &[20_031, 20_032, 20_033]);
}

#[test]
fn the_window_that_was_dragged_is_the_one_resized_not_the_focused_one() {
    let (fake, mut wm) = tiled(DefaultLayout::BSP, 20_041, 3);
    // 20_043 (the bottom right tile) has focus; 20_042 (top right) is dragged by its bottom edge
    let top_right = fake.rect_of(20_042).unwrap();
    let bottom_right = fake.rect_of(20_043).unwrap();

    drag(
        &fake,
        &mut wm,
        20_042,
        Rect {
            bottom: top_right.bottom + 100,
            ..top_right
        },
    );

    let (a, b) = (fake.rect_of(20_042).unwrap(), fake.rect_of(20_043).unwrap());
    assert!(a.bottom >= top_right.bottom + 90, "{top_right:?} -> {a:?}");
    assert!(
        b.bottom <= bottom_right.bottom - 90,
        "{bottom_right:?} -> {b:?}"
    );
    assert_tiling(&fake, &[20_041, 20_042, 20_043]);
}

#[test]
fn dragging_an_edge_that_has_no_neighbour_puts_the_window_back() {
    let (fake, mut wm) = tiled(DefaultLayout::BSP, 20_051, 2);
    let left = fake.rect_of(20_051).unwrap();
    let right = fake.rect_of(20_052).unwrap();

    // the outer left edge of the left tile has nothing to take room from
    drag(&fake, &mut wm, 20_051, grown_left(left, 80));

    assert_eq!(fake.rect_of(20_051).unwrap(), left);
    assert_eq!(fake.rect_of(20_052).unwrap(), right);
}

#[test]
fn a_drag_that_changed_nothing_changes_nothing() {
    let (fake, mut wm) = tiled(DefaultLayout::BSP, 20_061, 3);
    let before: Vec<Rect> = (20_061..20_064)
        .map(|id| fake.rect_of(id).unwrap())
        .collect();

    // a click on an edge: started, finished, nowhere
    drag(&fake, &mut wm, 20_062, fake.rect_of(20_062).unwrap());

    let after: Vec<Rect> = (20_061..20_064)
        .map(|id| fake.rect_of(id).unwrap())
        .collect();
    assert_eq!(before, after);
}

#[test]
fn a_corner_resizes_both_of_its_edges() {
    let (fake, mut wm) = tiled(DefaultLayout::BSP, 20_071, 3);
    // the bottom right tile, by its top left corner: up into the tile above, left into the one beside
    let tile = fake.rect_of(20_073).unwrap();
    let above = fake.rect_of(20_072).unwrap();
    let beside = fake.rect_of(20_071).unwrap();

    drag(
        &fake,
        &mut wm,
        20_073,
        Rect {
            left: tile.left - 100,
            top: tile.top - 100,
            right: tile.right + 100,
            bottom: tile.bottom + 100,
        },
    );

    let after = fake.rect_of(20_073).unwrap();
    assert!(
        after.left <= tile.left - 90 && after.top <= tile.top - 90,
        "{tile:?} -> {after:?}"
    );
    assert!(fake.rect_of(20_072).unwrap().bottom < above.bottom);
    assert!(fake.rect_of(20_071).unwrap().right < beside.right);
    assert_tiling(&fake, &[20_071, 20_072, 20_073]);
}

#[test]
fn the_edge_of_a_stack_is_the_edge_of_its_tile() {
    let (fake, mut wm) = tiled(DefaultLayout::BSP, 53_001, 3);
    // 53_003 has the focus: it goes in front of 53_002, whose tile is the top right one
    with_backend(fake.clone(), || {
        wm.process_command(
            SocketMessage::StackWindow(OperationDirection::Up),
            Vec::new(),
        )
    })
    .unwrap();
    let tile = fake.rect_of(53_003).unwrap();
    let beside = fake.rect_of(53_001).unwrap();
    assert_eq!(wm.focused_workspace().unwrap().containers().len(), 2);

    drag(&fake, &mut wm, 53_003, grown_left(tile, 100));

    let after = fake.rect_of(53_003).unwrap();
    assert!(
        after.left <= tile.left - 90,
        "the stack's tile grew: {tile:?} -> {after:?}"
    );
    assert!(
        fake.rect_of(53_001).unwrap().right <= beside.right - 90,
        "and the tile beside it gave up the room"
    );
    assert_tiling(&fake, &[53_001, 53_003]);
}

#[test]
fn a_floating_window_dragged_by_its_edge_is_left_as_the_user_made_it() {
    let (fake, mut wm) = tiled(DefaultLayout::BSP, 53_011, 2);
    with_backend(fake.clone(), || {
        // 53_012 has the focus
        wm.process_command(SocketMessage::ToggleFloat, Vec::new())
    })
    .unwrap();
    let tile = fake.rect_of(53_011).unwrap();
    let floating = fake.rect_of(53_012).unwrap();
    let dragged = grown_left(floating, 100);

    drag(&fake, &mut wm, 53_012, dragged);

    assert_eq!(fake.rect_of(53_012), Some(dragged));
    assert_eq!(
        fake.rect_of(53_011),
        Some(tile),
        "a floating window has no tile to resize, and takes nothing from the one that has"
    );
}

#[test]
fn the_window_of_a_monocle_dragged_by_its_edge_is_left_as_the_user_made_it() {
    let (fake, mut wm) = tiled(DefaultLayout::BSP, 53_021, 2);
    with_backend(fake.clone(), || {
        // 53_022 has the focus
        wm.process_command(SocketMessage::ToggleMonocle, Vec::new())
    })
    .unwrap();
    let monocle = fake.rect_of(53_022).unwrap();
    let dragged = Rect {
        right: monocle.right - 100,
        ..monocle
    };

    drag(&fake, &mut wm, 53_022, dragged);

    assert_eq!(fake.rect_of(53_022), Some(dragged));
    assert!(wm.focused_workspace().unwrap().monocle_container.is_some());
}

#[test]
fn the_end_of_a_resize_is_where_the_user_took_the_edge_when_the_frame_has_not_got_there() {
    // KWin's frame is the client's, and when the user lets go the client is still drawing at the
    // size they asked for: here 140 of the 200 pixels, with the frame events that follow the end
    // having come before the window manager gets to it. The backend has kept what the user asked
    // for, and the drag counts for that
    let (fake, mut wm) = desktop();
    open(&fake, 53_031);
    open(&fake, 53_032);

    with_backend(fake.clone(), || {
        wm.process_event(added(53_031))?;
        wm.process_event(added(53_032))?;
        let tile = fake.rect_of(53_032).unwrap();

        wm.process_event(resize_started(53_032))?;
        fake.move_window_by_user(53_032, grown_left(tile, 140));
        fake.set_drag_end(53_032, grown_left(tile, 200));
        wm.process_event(resize_finished(53_032))?;

        assert_eq!(
            fake.rect_of(53_032).unwrap().left,
            tile.left - 200,
            "the tile took the 200 pixels the user asked for"
        );
        Ok::<(), color_eyre::eyre::Report>(())
    })
    .unwrap();

    assert_tiling(&fake, &[53_031, 53_032]);
}
