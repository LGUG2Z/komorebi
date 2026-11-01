//! Resizing the focused window from the command line, `resize-edge`, `resize-axis` and
//! `resize-delta`, and saving and loading the sizes, against an in-memory desktop: the messages go
//! in through [`WindowManager::process_command`], and what comes out is where the backend is asked
//! to put each window.
//!
//! Window ids are in the 23_000s.

use komorebi::backend::fake::FakeBackend;
use komorebi::backend::with_backend;
use komorebi::core::Axis;
use komorebi::core::DefaultLayout;
use komorebi::core::OperationDirection;
use komorebi::core::Rect;
use komorebi::core::Sizing;
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

/// A window manager over a fake desktop with one 1920x1080 monitor (1920x1050 without the panel)
/// in `layout`, and a window for each of `ids`, opened in that order. The last one has the focus.
fn desktop_with(layout: DefaultLayout, ids: &[u32]) -> (Arc<FakeBackend>, WindowManager) {
    let fake = Arc::new(FakeBackend::with_one_monitor());
    let (_sender, receiver) = crossbeam_channel::bounded(1);
    // unix socket paths are limited to ~104 bytes on macOS, which the temp dir alone can exceed
    static NEXT: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);
    let socket = std::path::PathBuf::from(format!(
        "/tmp/kmr-{}-{}.sock",
        std::process::id(),
        NEXT.fetch_add(1, std::sync::atomic::Ordering::SeqCst)
    ));

    let mut wm = with_backend(fake.clone(), || {
        let mut wm = WindowManager::new(receiver, Some(socket)).unwrap();
        wm.init().unwrap();
        wm
    });
    send(&fake, &mut wm, SocketMessage::ChangeLayout(layout));
    for &id in ids {
        fake.open_window(id, "foot", "foot", &format!("terminal {id}"));
        with_backend(fake.clone(), || wm.process_event(added(id))).unwrap();
    }

    (fake, wm)
}

fn send(fake: &Arc<FakeBackend>, wm: &mut WindowManager, message: SocketMessage) {
    with_backend(fake.clone(), || wm.process_command(message, Vec::new())).unwrap();
}

fn edge(direction: OperationDirection, sizing: Sizing) -> SocketMessage {
    SocketMessage::ResizeWindowEdge(direction, sizing)
}

fn rects(fake: &FakeBackend, ids: &[u32]) -> Vec<Rect> {
    ids.iter().map(|&id| fake.rect_of(id).unwrap()).collect()
}

#[test]
fn resize_edge_moves_the_edge_between_the_focused_window_and_its_neighbor() {
    let ids = [23_001, 23_002];
    let (fake, mut wm) = desktop_with(DefaultLayout::BSP, &ids);
    let before = rects(&fake, &ids);

    // the second window has the focus, on the right: its left edge goes left by half the delta
    // of 50, which is how komorebi-layouts moves an edge
    send(
        &fake,
        &mut wm,
        edge(OperationDirection::Left, Sizing::Increase),
    );

    let after = rects(&fake, &ids);
    assert_eq!(after[1].left, before[1].left - 25, "{before:?} {after:?}");
    assert_eq!(after[1].right, before[1].right + 25);
    assert_eq!(after[0].right, before[0].right - 25);

    send(
        &fake,
        &mut wm,
        edge(OperationDirection::Left, Sizing::Decrease),
    );

    assert_eq!(rects(&fake, &ids), before);
}

#[test]
fn resize_edge_on_a_side_with_no_neighbor_changes_nothing() {
    let ids = [23_011, 23_012];
    let (fake, mut wm) = desktop_with(DefaultLayout::BSP, &ids);
    let before = rects(&fake, &ids);

    send(
        &fake,
        &mut wm,
        edge(OperationDirection::Right, Sizing::Increase),
    );

    assert_eq!(rects(&fake, &ids), before);
    assert_eq!(
        wm.focused_workspace().unwrap().resize_dimensions,
        [None, None]
    );
}

#[test]
fn resize_axis_moves_both_edges_along_the_axis() {
    let ids = [23_021, 23_022, 23_023];
    let (fake, mut wm) = desktop_with(DefaultLayout::Columns, &ids);
    send(
        &fake,
        &mut wm,
        SocketMessage::CycleFocusWindow(komorebi::core::CycleDirection::Previous),
    );
    assert_eq!(wm.focused_window().unwrap().id, 23_022);
    let before = rects(&fake, &ids);

    send(
        &fake,
        &mut wm,
        SocketMessage::ResizeWindowAxis(Axis::Horizontal, Sizing::Increase),
    );

    let after = rects(&fake, &ids);
    assert_eq!(after[1].left, before[1].left - 25, "{before:?} {after:?}");
    assert_eq!(after[1].right, before[1].right + 50);
    assert_eq!(after[0].right, before[0].right - 25);
    assert_eq!(after[2].right, before[2].right - 25);
}

#[test]
fn resize_delta_sets_how_far_a_resize_goes() {
    let ids = [23_031, 23_032];
    let (fake, mut wm) = desktop_with(DefaultLayout::BSP, &ids);
    let before = rects(&fake, &ids);

    send(&fake, &mut wm, SocketMessage::ResizeDelta(120));
    send(
        &fake,
        &mut wm,
        edge(OperationDirection::Left, Sizing::Increase),
    );

    assert_eq!(wm.resize_delta, 120);
    assert_eq!(fake.rect_of(23_032).unwrap().left, before[1].left - 60);
}

#[test]
fn a_resize_leaves_every_tile_about_a_tenth_of_the_work_area_wide_at_least() {
    let ids = [23_041, 23_042];
    let (fake, mut wm) = desktop_with(DefaultLayout::BSP, &ids);

    send(&fake, &mut wm, SocketMessage::ResizeDelta(5000));
    send(
        &fake,
        &mut wm,
        edge(OperationDirection::Left, Sizing::Increase),
    );

    // a tenth of the work area, measured before the paddings are taken off the tiles
    let after = rects(&fake, &ids);
    assert!(after[0].right >= 1920 / 10 - 20, "{after:?}");
    assert!(
        after[0].right < 400,
        "it was granted as far as it goes: {after:?}"
    );
}

#[test]
fn retile_forgets_the_resizes_and_retile_with_resize_dimensions_keeps_them() {
    let ids = [23_051, 23_052];
    let (fake, mut wm) = desktop_with(DefaultLayout::BSP, &ids);
    let before = rects(&fake, &ids);
    send(
        &fake,
        &mut wm,
        edge(OperationDirection::Left, Sizing::Increase),
    );
    let resized = rects(&fake, &ids);

    send(&fake, &mut wm, SocketMessage::RetileWithResizeDimensions);
    assert_eq!(rects(&fake, &ids), resized);

    send(&fake, &mut wm, SocketMessage::Retile);
    assert_eq!(rects(&fake, &ids), before);
}

#[test]
fn sizes_saved_to_a_file_are_loaded_back_from_it() {
    let ids = [23_061, 23_062];
    let (fake, mut wm) = desktop_with(DefaultLayout::BSP, &ids);
    let path =
        std::env::temp_dir().join(format!("komorebi-resize-test-{}.json", std::process::id()));
    send(
        &fake,
        &mut wm,
        edge(OperationDirection::Left, Sizing::Increase),
    );
    let resized = rects(&fake, &ids);

    send(&fake, &mut wm, SocketMessage::Save(path.clone()));
    send(&fake, &mut wm, SocketMessage::Retile);
    assert_ne!(rects(&fake, &ids), resized);
    send(&fake, &mut wm, SocketMessage::Load(path.clone()));

    assert_eq!(rects(&fake, &ids), resized);
    std::fs::remove_file(path).unwrap();
}

#[test]
fn loading_sizes_from_a_file_that_is_not_there_is_an_error() {
    let (fake, mut wm) = desktop_with(DefaultLayout::BSP, &[23_071]);

    let result = with_backend(fake.clone(), || {
        wm.process_command(
            SocketMessage::Load("/nonexistent/komorebi-resize.json".into()),
            Vec::new(),
        )
    });

    assert_eq!(
        result.map_err(|error| error.to_string()),
        Err(String::from(
            "no file found at /nonexistent/komorebi-resize.json"
        ))
    );
}
