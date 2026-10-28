//! Moving a window by dragging it with the pointer, against an in-memory desktop: swapping two
//! tiles by dragging one window onto another, and what the window container behavior, the float
//! override and a second monitor change about where the window goes.
//!
//! Window ids are in the 10_000s and the 51_000s.

use komorebi::backend::Backend;
use komorebi::backend::fake::FakeBackend;
use komorebi::backend::with_backend;
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

fn drag(kind: KWinNotification, id: u32, start: bool, resize: bool) -> WindowManagerEvent {
    let n = SystemNotification::KWin(kind);
    let w = Window::from(id);
    match (start, resize) {
        (true, false) => WindowManagerEvent::MoveStart(n, w),
        (false, false) => WindowManagerEvent::MoveEnd(n, w),
        (true, true) => WindowManagerEvent::ResizeStart(n, w),
        (false, true) => WindowManagerEvent::ResizeEnd(n, w),
    }
}

/// A window manager over a fake desktop with one 1920x1080 monitor (1920x1050 without the panel)
fn desktop() -> (Arc<FakeBackend>, WindowManager) {
    desktop_on(FakeBackend::with_one_monitor())
}

fn desktop_on(fake: FakeBackend) -> (Arc<FakeBackend>, WindowManager) {
    let fake = Arc::new(fake);
    let (_sender, receiver) = crossbeam_channel::bounded(1);
    // unix socket paths are limited to ~104 bytes on macOS, which the temp dir alone can exceed
    static NEXT: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);
    let socket = std::path::PathBuf::from(format!(
        "/tmp/kmb-{}-{}.sock",
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

fn activated(id: u32) -> WindowManagerEvent {
    WindowManagerEvent::FocusChange(
        SystemNotification::KWin(KWinNotification::WindowActivated),
        Window::from(id),
    )
}

fn open(fake: &FakeBackend, id: u32) {
    fake.open_window(id, "foot", "foot", &format!("terminal {id}"));
}

/// The bounds of the second monitor, on the right of the first, which is also its work area
fn right_half() -> Rect {
    Rect {
        left: 1920,
        top: 0,
        right: 1920,
        bottom: 1080,
    }
}

/// A window manager over a fake desktop with the monitor of `desktop` and a second one on its
/// right
fn two_monitors() -> (Arc<FakeBackend>, WindowManager) {
    let fake = FakeBackend::with_one_monitor();
    fake.add_monitor(2, "Virtual-1", right_half(), right_half());
    desktop_on(fake)
}

/// Opens a window on the second monitor
fn open_on_right(fake: &FakeBackend, id: u32) {
    open(fake, id);
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

/// The user takes hold of the window `id`, which they have given the focus, carries it to where
/// the pointer is `(x, y)` and lets go: its frame is then the same size as it was, at `to`
fn drag_to(
    fake: &Arc<FakeBackend>,
    wm: &mut WindowManager,
    id: u32,
    to: (i32, i32),
    pointer: (i32, i32),
) -> color_eyre::Result<()> {
    with_backend(fake.clone(), || {
        fake.set_foreground(Some(id));
        wm.process_event(activated(id))?;
        let frame = fake.rect_of(id).unwrap();

        wm.process_event(drag(
            KWinNotification::InteractiveMoveStarted,
            id,
            true,
            false,
        ))?;
        fake.move_window_by_user(
            id,
            Rect {
                left: to.0,
                top: to.1,
                ..frame
            },
        );
        fake.set_cursor(pointer.0, pointer.1);
        wm.process_event(drag(
            KWinNotification::InteractiveMoveFinished,
            id,
            false,
            false,
        ))
    })
}

#[test]
fn dragging_a_window_onto_another_swaps_them() {
    let (fake, mut wm) = desktop();
    open(&fake, 10_091);
    open(&fake, 10_092);

    with_backend(fake.clone(), || {
        wm.process_event(added(10_091))?;
        wm.process_event(added(10_092))?;

        let a = fake.rect_of(10_091).unwrap();
        let b = fake.rect_of(10_092).unwrap();
        assert!(
            a.left < b.left,
            "10_091 should be the left tile: {a:?} {b:?}"
        );

        // the user grabs the focused window (10_092, the right tile) and drops it on the left one
        wm.process_event(drag(
            KWinNotification::InteractiveMoveStarted,
            10_092,
            true,
            false,
        ))?;
        fake.move_window_by_user(
            10_092,
            Rect {
                left: a.left + 40,
                top: a.top + 40,
                ..b
            },
        );
        fake.set_cursor(a.left + 200, a.top + 200);
        wm.process_event(drag(
            KWinNotification::InteractiveMoveFinished,
            10_092,
            false,
            false,
        ))
    })
    .unwrap();

    let (a, b) = (fake.rect_of(10_091).unwrap(), fake.rect_of(10_092).unwrap());
    assert!(
        b.left < a.left,
        "they should have swapped sides: {a:?} {b:?}"
    );
}

#[test]
fn a_window_let_go_of_where_there_is_no_tile_goes_back_to_its_own() {
    let (fake, mut wm) = desktop();
    open(&fake, 51_001);
    open(&fake, 51_002);

    with_backend(fake.clone(), || {
        wm.process_event(added(51_001))?;
        wm.process_event(added(51_002))?;
        let b = fake.rect_of(51_002).unwrap();

        wm.process_event(drag(
            KWinNotification::InteractiveMoveStarted,
            51_002,
            true,
            false,
        ))?;
        fake.move_window_by_user(
            51_002,
            Rect {
                left: b.left - 300,
                top: b.top + 20,
                ..b
            },
        );
        // on the panel, below the work area
        fake.set_cursor(b.left + 100, 1070);
        wm.process_event(drag(
            KWinNotification::InteractiveMoveFinished,
            51_002,
            false,
            false,
        ))?;

        assert_eq!(
            fake.rect_of(51_002),
            Some(b),
            "it should be back on its tile"
        );
        assert_eq!(
            fake.cursor_pos(),
            Some((b.left + 100, 1070)),
            "the pointer should be where it was let go"
        );
        Ok::<(), color_eyre::eyre::Report>(())
    })
    .unwrap();
}

#[test]
fn a_floating_window_dragged_onto_a_tile_takes_nothing_from_it() {
    let (fake, mut wm) = desktop();
    open(&fake, 51_011);
    open(&fake, 51_012);

    with_backend(fake.clone(), || {
        wm.process_event(added(51_011))?;
        wm.process_event(added(51_012))?;
        fake.set_foreground(Some(51_012));
        wm.process_command(SocketMessage::ToggleFloat, Vec::new())?;
        let a = fake.rect_of(51_011).unwrap();

        wm.process_event(drag(
            KWinNotification::InteractiveMoveStarted,
            51_012,
            true,
            false,
        ))?;
        let dropped = Rect {
            left: a.left + 40,
            top: a.top + 40,
            right: 400,
            bottom: 300,
        };
        fake.move_window_by_user(51_012, dropped);
        fake.set_cursor(a.left + 200, a.top + 200);
        wm.process_event(drag(
            KWinNotification::InteractiveMoveFinished,
            51_012,
            false,
            false,
        ))?;

        assert_eq!(
            fake.rect_of(51_011),
            Some(a),
            "the tile should be as it was"
        );
        assert_eq!(
            fake.rect_of(51_012),
            Some(dropped),
            "the floating window should be where it was let go of"
        );
        Ok::<(), color_eyre::eyre::Report>(())
    })
    .unwrap();
}

#[test]
fn a_window_let_go_of_on_another_monitor_goes_to_that_monitor() {
    let (fake, mut wm) = two_monitors();
    open(&fake, 51_021);
    open(&fake, 51_022);
    open_on_right(&fake, 51_023);

    with_backend(fake.clone(), || {
        wm.process_event(added(51_021))?;
        wm.process_event(added(51_022))?;
        wm.process_event(added(51_023))
    })
    .unwrap();
    assert_eq!(windows(&wm), [vec![51_021, 51_022], vec![51_023]]);

    let tile = fake.rect_of(51_023).unwrap();
    drag_to(
        &fake,
        &mut wm,
        51_022,
        (tile.left + 40, tile.top + 40),
        (tile.left + 200, tile.top + 200),
    )
    .unwrap();

    let shown = windows(&wm);
    assert_eq!(
        shown,
        [vec![51_021], vec![51_022, 51_023]],
        "it should take the place of the container it was let go on"
    );
    assert!(
        fake.rect_of(51_022).unwrap().left >= 1920,
        "it should be tiled on the second monitor"
    );
    assert!(
        fake.rect_of(51_021).unwrap().right > 1800,
        "the window left behind should have the first monitor to itself"
    );
    let moved = fake.rect_of(51_022).unwrap();
    assert_eq!(
        fake.cursor_pos(),
        Some((moved.left + moved.right / 2, moved.top + moved.bottom / 2)),
        "the pointer should have followed it"
    );
}

#[test]
fn a_window_let_go_of_on_a_monitor_with_nothing_on_it_goes_there() {
    let (fake, mut wm) = two_monitors();
    open(&fake, 51_031);
    open(&fake, 51_032);

    with_backend(fake.clone(), || {
        wm.process_event(added(51_031))?;
        wm.process_event(added(51_032))
    })
    .unwrap();

    drag_to(&fake, &mut wm, 51_032, (2400, 300), (2500, 400)).unwrap();

    assert_eq!(windows(&wm), [vec![51_031], vec![51_032]]);
    assert!(fake.rect_of(51_032).unwrap().left >= 1920);
}

/// The ids of the windows of each container of the focused workspace, in order
fn containers(wm: &WindowManager) -> Vec<Vec<u32>> {
    wm.focused_workspace()
        .unwrap()
        .containers()
        .iter()
        .map(|container| container.windows().iter().map(|window| window.id).collect())
        .collect()
}

#[test]
fn with_append_a_window_let_go_of_on_a_tile_joins_the_container_of_that_tile() {
    let (fake, mut wm) = desktop();
    open(&fake, 51_041);
    open(&fake, 51_042);
    open(&fake, 51_043);

    with_backend(fake.clone(), || {
        wm.process_event(added(51_041))?;
        wm.process_event(added(51_042))?;
        wm.process_event(added(51_043))?;
        wm.process_command(SocketMessage::ToggleWindowContainerBehaviour, Vec::new())
    })
    .unwrap();
    assert_eq!(containers(&wm), [[51_041], [51_042], [51_043]]);

    let tile = fake.rect_of(51_041).unwrap();
    drag_to(
        &fake,
        &mut wm,
        51_043,
        (tile.left + 40, tile.top + 40),
        (tile.left + 200, tile.top + 200),
    )
    .unwrap();

    assert_eq!(containers(&wm), [vec![51_041, 51_043], vec![51_042]]);
    assert_eq!(
        fake.rect_of(51_043),
        Some(tile),
        "the window should be in front on the tile of the container it joined"
    );
}

#[test]
fn with_the_float_override_a_window_let_go_of_on_a_tile_floats_where_it_was_let_go_of() {
    let (fake, mut wm) = desktop();
    open(&fake, 51_051);
    open(&fake, 51_052);

    with_backend(fake.clone(), || {
        wm.process_event(added(51_051))?;
        wm.process_event(added(51_052))?;
        wm.process_command(SocketMessage::ToggleFloatOverride, Vec::new())
    })
    .unwrap();
    let (a, b) = (fake.rect_of(51_051).unwrap(), fake.rect_of(51_052).unwrap());

    drag_to(
        &fake,
        &mut wm,
        51_052,
        (a.left + 40, a.top + 40),
        (a.left + 200, a.top + 200),
    )
    .unwrap();

    let floating: Vec<u32> = wm
        .focused_workspace()
        .unwrap()
        .floating_windows()
        .iter()
        .map(|window| window.id)
        .collect();
    assert_eq!(containers(&wm), [[51_051]]);
    assert_eq!(floating, [51_052]);
    assert_eq!(
        fake.rect_of(51_052),
        Some(Rect {
            left: a.left + 40,
            top: a.top + 40,
            ..b
        }),
        "it should be where it was let go of"
    );
    assert_eq!(
        fake.rect_of(51_051),
        Some(Rect { right: 1900, ..a }),
        "the tile that is left should have the work area to itself"
    );
}

#[test]
fn a_floating_window_let_go_of_on_another_monitor_is_one_of_the_floating_windows_there() {
    let (fake, mut wm) = two_monitors();
    open(&fake, 51_061);
    open(&fake, 51_062);

    with_backend(fake.clone(), || {
        wm.process_event(added(51_061))?;
        wm.process_event(added(51_062))?;
        fake.set_foreground(Some(51_062));
        wm.process_command(SocketMessage::ToggleFloat, Vec::new())
    })
    .unwrap();
    let floating = |wm: &WindowManager, monitor: usize| -> Vec<u32> {
        wm.monitors()[monitor]
            .focused_workspace()
            .unwrap()
            .floating_windows()
            .iter()
            .map(|window| window.id)
            .collect()
    };
    assert_eq!(floating(&wm, 0), [51_062]);

    drag_to(&fake, &mut wm, 51_062, (2400, 300), (2500, 400)).unwrap();

    assert_eq!(floating(&wm, 0), Vec::<u32>::new());
    assert_eq!(floating(&wm, 1), [51_062]);
    assert_eq!(windows(&wm), [vec![51_061], vec![]]);
}

#[test]
fn after_a_swap_the_pointer_is_where_it_was_let_go() {
    let (fake, mut wm) = desktop();
    open(&fake, 51_081);
    open(&fake, 51_082);

    with_backend(fake.clone(), || {
        wm.process_event(added(51_081))?;
        wm.process_event(added(51_082))
    })
    .unwrap();
    let a = fake.rect_of(51_081).unwrap();

    drag_to(
        &fake,
        &mut wm,
        51_082,
        (a.left + 40, a.top + 40),
        (a.left + 200, a.top + 200),
    )
    .unwrap();

    assert!(fake.rect_of(51_082).unwrap().left < fake.rect_of(51_081).unwrap().left);
    assert_eq!(fake.cursor_pos(), Some((a.left + 200, a.top + 200)));
}
