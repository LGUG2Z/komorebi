//! The scrolling layout, against an in-memory desktop: columns of several windows and the commands
//! that make them, the width and the place of a column, panning the view, where a window that is
//! out of view rests, and what a drag of the edge of a column does.
//!
//! KWin lets a window sit anywhere, so the columns of a strip that are past the edge of a monitor
//! can be on the next monitor over. Those windows are hidden, and still belong to the monitor whose
//! strip they are in.
//!
//! Window ids are in the 10_000s.

use komorebi::backend::fake::FakeBackend;
use komorebi::backend::with_backend;
use komorebi::core::DefaultLayout;
use komorebi::core::OperationDirection;
use komorebi::core::PanDirection;
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

fn open(fake: &FakeBackend, id: u32) {
    fake.open_window(id, "foot", "foot", &format!("terminal {id}"));
}

#[test]
fn a_scrolling_strip_on_the_next_monitor_over_does_not_take_focus_with_it() {
    // KWin lets a window sit anywhere, so the part of the strip past the right edge of the first
    // monitor would be on the second one. Those windows are hidden, and still belong to the first.
    let fake = FakeBackend::with_one_monitor();
    fake.add_monitor(
        2,
        "Virtual-1",
        Rect {
            left: 1920,
            top: 0,
            right: 1920,
            bottom: 1080,
        },
        Rect {
            left: 1920,
            top: 0,
            right: 1920,
            bottom: 1050,
        },
    );
    let (fake, mut wm) = desktop_on(fake);
    for id in 10_201..=10_205 {
        open(&fake, id);
    }

    with_backend(fake.clone(), || {
        wm.process_command(
            SocketMessage::ChangeLayout(DefaultLayout::Scrolling),
            Vec::new(),
        )?;
        wm.process_command(
            SocketMessage::ScrollingLayoutColumns(std::num::NonZeroUsize::new(2).unwrap()),
            Vec::new(),
        )?;
        for id in 10_201..=10_205 {
            wm.process_event(added(id))?;
        }
        // back to the start of the strip, which leaves the later columns past the monitor's edge
        for _ in 0..4 {
            wm.process_command(
                SocketMessage::FocusWindow(komorebi::core::OperationDirection::Left),
                Vec::new(),
            )?;
        }

        // (the compositor's focus event is what scrolls the strip, and the fake sends none)
        wm.update_focused_workspace(false, false)?;

        // the part of the strip past the edge would be on the second monitor, so it is hidden
        // there and not left on the screen of another monitor
        assert!(
            fake.is_hidden(10_204),
            "the test needs a window past the edge, hidden: {:?}",
            fake.rect_of(10_204)
        );

        wm.process_event(WindowManagerEvent::FocusChange(
            SystemNotification::KWin(KWinNotification::WindowActivated),
            Window::from(10_204),
        ))
    })
    .unwrap();

    assert_eq!(
        wm.focused_monitor_idx(),
        0,
        "focus moved to the monitor the strip spills onto"
    );
}

fn send(fake: &Arc<FakeBackend>, wm: &mut WindowManager, message: SocketMessage) {
    with_backend(fake.clone(), || wm.process_command(message, Vec::new())).unwrap();
}

/// A window manager on one monitor, the scrolling layout with `columns` columns in view, and
/// windows `first..first + count`, the last of them focused
fn strip(columns: usize, first: u32, count: u32) -> (Arc<FakeBackend>, WindowManager) {
    let (fake, mut wm) = desktop_on(FakeBackend::with_one_monitor());
    for id in first..first + count {
        open(&fake, id);
    }
    send(
        &fake,
        &mut wm,
        SocketMessage::ChangeLayout(DefaultLayout::Scrolling),
    );
    send(
        &fake,
        &mut wm,
        SocketMessage::ScrollingLayoutColumns(std::num::NonZeroUsize::new(columns).unwrap()),
    );
    with_backend(fake.clone(), || {
        for id in first..first + count {
            wm.process_event(added(id)).unwrap();
        }
    });
    (fake, wm)
}

fn focus(fake: &Arc<FakeBackend>, wm: &mut WindowManager, direction: OperationDirection) {
    send(fake, wm, SocketMessage::FocusWindow(direction));
    // the compositor reports the focus it was asked to give, which is what scrolls the strip
    let id = wm.focused_window().unwrap().id;
    with_backend(fake.clone(), || {
        wm.process_event(WindowManagerEvent::FocusChange(
            SystemNotification::KWin(KWinNotification::WindowActivated),
            Window::from(id),
        ))
    })
    .unwrap();
}

fn rect(fake: &FakeBackend, id: u32) -> Rect {
    fake.rect_of(id).unwrap()
}

fn in_view(rect: Rect) -> bool {
    rect.left >= 0 && rect.left + rect.right <= 1920
}

#[test]
fn consuming_puts_two_windows_in_one_column_and_expelling_takes_one_out() {
    let (fake, mut wm) = strip(3, 10_301, 3);
    focus(&fake, &mut wm, OperationDirection::Left);
    focus(&fake, &mut wm, OperationDirection::Left);
    assert_eq!(wm.focused_window().unwrap().id, 10_301);

    send(
        &fake,
        &mut wm,
        SocketMessage::Consume(OperationDirection::Right),
    );

    let (first, second, third) = (
        rect(&fake, 10_301),
        rect(&fake, 10_302),
        rect(&fake, 10_303),
    );
    assert_eq!(first.left, second.left, "one column: {first:?} {second:?}");
    assert_eq!(first.right, second.right);
    assert!(
        second.top >= first.top + first.bottom,
        "one above the other"
    );
    assert!(first.bottom < 600, "and each has a share of the height");
    assert_ne!(second.left, third.left);
    assert_eq!(wm.focused_window().unwrap().id, 10_301, "focus stays");

    send(&fake, &mut wm, SocketMessage::Expel);

    let (first, second) = (rect(&fake, 10_301), rect(&fake, 10_302));
    assert_ne!(first.left, second.left, "a column each again");
    assert_eq!(first.top, second.top);
}

#[test]
fn a_column_is_moved_and_focused_whole_and_its_windows_up_and_down() {
    let (fake, mut wm) = strip(3, 10_311, 3);
    focus(&fake, &mut wm, OperationDirection::Left);
    focus(&fake, &mut wm, OperationDirection::Left);
    send(
        &fake,
        &mut wm,
        SocketMessage::Consume(OperationDirection::Right),
    );

    // right goes to the next column, and down to the other window of this one
    focus(&fake, &mut wm, OperationDirection::Down);
    assert_eq!(wm.focused_window().unwrap().id, 10_312);
    focus(&fake, &mut wm, OperationDirection::Up);
    assert_eq!(wm.focused_window().unwrap().id, 10_311);
    focus(&fake, &mut wm, OperationDirection::Right);
    assert_eq!(wm.focused_window().unwrap().id, 10_313);

    // the column of two moves as one: the window of the column on the right is now before it
    focus(&fake, &mut wm, OperationDirection::Left);
    send(
        &fake,
        &mut wm,
        SocketMessage::MoveWindow(OperationDirection::Right),
    );
    assert!(rect(&fake, 10_313).left < rect(&fake, 10_311).left);
    assert_eq!(rect(&fake, 10_311).left, rect(&fake, 10_312).left);
}

#[test]
fn toggling_the_stack_of_a_column_stacks_its_windows_and_spreads_them_out_again() {
    let (fake, mut wm) = strip(3, 10_321, 3);
    focus(&fake, &mut wm, OperationDirection::Left);
    focus(&fake, &mut wm, OperationDirection::Left);
    send(
        &fake,
        &mut wm,
        SocketMessage::Consume(OperationDirection::Right),
    );

    send(&fake, &mut wm, SocketMessage::ToggleColumnStack);
    assert_eq!(wm.focused_workspace().unwrap().containers().len(), 2);
    let shown = [10_321, 10_322]
        .into_iter()
        .filter(|id| !fake.is_hidden(*id))
        .count();
    assert_eq!(shown, 1, "one window of the stack shows at a time");
    assert_eq!(
        rect(&fake, 10_321).bottom,
        rect(&fake, 10_323).bottom,
        "and takes the height of the column"
    );

    send(&fake, &mut wm, SocketMessage::ToggleColumnStack);
    assert_eq!(wm.focused_workspace().unwrap().containers().len(), 3);
    assert!(
        !fake.is_hidden(10_321) && !fake.is_hidden(10_322),
        "every window of a stack that is spread out is shown"
    );
    assert_eq!(rect(&fake, 10_321).left, rect(&fake, 10_322).left);
    assert_ne!(rect(&fake, 10_321).top, rect(&fake, 10_322).top);
}

#[test]
fn a_column_is_made_as_wide_as_the_screen_and_put_back() {
    let (fake, mut wm) = strip(3, 10_331, 2);
    let before = rect(&fake, 10_332);

    send(&fake, &mut wm, SocketMessage::ScrollingLayoutFullWidth);
    let full = rect(&fake, 10_332);
    assert!(full.right > 1800, "{full:?}");
    assert!(in_view(full), "{full:?}");

    send(&fake, &mut wm, SocketMessage::ScrollingLayoutFullWidth);
    assert!((rect(&fake, 10_332).right - before.right).abs() <= 1);
}

#[test]
fn expanding_fills_the_room_that_the_columns_in_view_leave() {
    let (fake, mut wm) = strip(3, 10_341, 2);
    let before = rect(&fake, 10_342);

    send(&fake, &mut wm, SocketMessage::Expand);

    let after = rect(&fake, 10_342);
    assert!(after.right > before.right + 400, "{before:?} {after:?}");
    assert_eq!(rect(&fake, 10_341).left, 10, "the column before it stays");
}

#[test]
fn centring_the_focus_puts_the_focused_column_in_the_middle_of_the_screen() {
    let (fake, mut wm) = strip(3, 10_351, 4);
    focus(&fake, &mut wm, OperationDirection::Left);
    focus(&fake, &mut wm, OperationDirection::Left);

    send(&fake, &mut wm, SocketMessage::CenterFocus);

    let focused = rect(&fake, wm.focused_window().unwrap().id);
    let middle = focused.left + focused.right / 2;
    assert!((middle - 960).abs() <= 12, "{focused:?}");
}

#[test]
fn panning_moves_the_view_a_column_and_focus_goes_to_a_column_in_view() {
    let (fake, mut wm) = strip(3, 10_361, 6);
    for _ in 0..5 {
        focus(&fake, &mut wm, OperationDirection::Left);
    }
    assert_eq!(wm.focused_window().unwrap().id, 10_361);
    let before = rect(&fake, 10_364);

    send(&fake, &mut wm, SocketMessage::Pan(PanDirection::Right));

    let after = rect(&fake, 10_364);
    assert!(
        after.left < before.left,
        "the view went right: {before:?} {after:?}"
    );
    let focused = wm.focused_window().unwrap().id;
    assert_ne!(focused, 10_361, "the first column is out of view");
    assert!(in_view(rect(&fake, focused)), "and focus is in view");

    send(&fake, &mut wm, SocketMessage::Pan(PanDirection::Left));
    assert_eq!(rect(&fake, 10_364).left, before.left, "and back");
}

#[test]
fn a_window_that_scrolls_out_of_view_keeps_its_size_and_rests_at_the_edge() {
    let (fake, mut wm) = strip(2, 10_371, 4);
    let in_front = rect(&fake, 10_374);

    let first = rect(&fake, 10_371);
    assert_eq!(first.right, in_front.right, "the size is the same");
    assert!(
        first.left + first.right > 0 && first.left + first.right <= 30,
        "a few pixels of it are on the screen: {first:?}"
    );
    assert!(
        !fake.is_hidden(10_371),
        "with no monitor next to it, it is not hidden"
    );

    // and it comes back where the strip puts it when the focus goes to it
    for _ in 0..3 {
        focus(&fake, &mut wm, OperationDirection::Left);
    }
    with_backend(fake.clone(), || {
        wm.process_event(WindowManagerEvent::FocusChange(
            SystemNotification::KWin(KWinNotification::WindowActivated),
            Window::from(10_371),
        ))
        .unwrap();
    });
    assert!(in_view(rect(&fake, 10_371)));
}

#[test]
fn a_window_that_is_hidden_past_the_edge_of_a_monitor_is_shown_where_the_strip_scrolls_to_it() {
    let fake = FakeBackend::with_one_monitor();
    fake.add_monitor(
        2,
        "Virtual-1",
        Rect {
            left: 1920,
            top: 0,
            right: 1920,
            bottom: 1080,
        },
        Rect {
            left: 1920,
            top: 0,
            right: 1920,
            bottom: 1050,
        },
    );
    let (fake, mut wm) = desktop_on(fake);
    for id in 10_381..=10_384 {
        open(&fake, id);
    }
    send(
        &fake,
        &mut wm,
        SocketMessage::ChangeLayout(DefaultLayout::Scrolling),
    );
    send(
        &fake,
        &mut wm,
        SocketMessage::ScrollingLayoutColumns(std::num::NonZeroUsize::new(2).unwrap()),
    );
    with_backend(fake.clone(), || {
        for id in 10_381..=10_384 {
            wm.process_event(added(id)).unwrap();
        }
    });
    for _ in 0..3 {
        focus(&fake, &mut wm, OperationDirection::Left);
    }
    assert!(
        fake.is_hidden(10_384),
        "past the right edge, next to a monitor"
    );
    assert!(!fake.is_hidden(10_381));

    // a window that rests or overhangs past an edge with a monitor next to it is hidden, so no
    // window of the strip reaches the other monitor
    send(&fake, &mut wm, SocketMessage::Pan(PanDirection::Right));
    for id in 10_381..=10_384 {
        let rect = rect(&fake, id);
        assert!(
            fake.is_hidden(id) || rect.left + rect.right <= 1920,
            "{id} reaches the monitor next to it: {rect:?}"
        );
    }

    // and it comes back, shown, when the focus goes to it
    for _ in 0..2 {
        focus(&fake, &mut wm, OperationDirection::Right);
    }
    assert_eq!(wm.focused_window().unwrap().id, 10_384);
    assert!(!fake.is_hidden(10_384));
    assert!(in_view(rect(&fake, 10_384)));
}

#[test]
fn a_window_that_will_not_get_as_narrow_widens_its_column_and_the_rest_make_room() {
    let (fake, mut wm) = desktop_on(FakeBackend::with_one_monitor());
    for id in 10_391..=10_393 {
        open(&fake, id);
    }
    // a client with a minimum width, as a GTK 4 application or Firefox has
    fake.set_min_size(10_392, 1300, 100);
    send(
        &fake,
        &mut wm,
        SocketMessage::ChangeLayout(DefaultLayout::Scrolling),
    );
    with_backend(fake.clone(), || {
        for id in 10_391..=10_393 {
            wm.process_event(added(id)).unwrap();
        }
    });

    let (wide, after) = (rect(&fake, 10_392), rect(&fake, 10_393));
    assert!(wide.right >= 1300, "{wide:?}");
    assert!(
        after.left < wide.left + wide.right,
        "until it is learned, the window covers its neighbour: {wide:?} {after:?}"
    );

    // the windows have had time to draw at the sizes they were sent, and the next layout learns
    // what they are
    std::thread::sleep(std::time::Duration::from_millis(500));
    send(
        &fake,
        &mut wm,
        SocketMessage::ScrollingLayoutColumns(std::num::NonZeroUsize::new(3).unwrap()),
    );

    let (wide, after) = (rect(&fake, 10_392), rect(&fake, 10_393));
    assert!(wide.right >= 1300, "{wide:?}");
    assert!(
        after.left >= wide.left + wide.right,
        "nothing overlaps now: {wide:?} {after:?}"
    );
}

#[test]
fn the_left_edge_of_a_column_is_where_it_is_dragged_to() {
    let (fake, mut wm) = strip(3, 10_401, 3);
    let before = rect(&fake, 10_403);

    with_backend(fake.clone(), || {
        wm.process_event(WindowManagerEvent::ResizeStart(
            SystemNotification::KWin(KWinNotification::InteractiveResizeStarted),
            Window::from(10_403),
        ))
        .unwrap();
        let dragged = Rect {
            left: before.left - 100,
            right: before.right + 100,
            ..before
        };
        fake.move_window_by_user(10_403, dragged);
        wm.process_event(WindowManagerEvent::ResizeEnd(
            SystemNotification::KWin(KWinNotification::InteractiveResizeFinished),
            Window::from(10_403),
        ))
        .unwrap();
    });

    let after = rect(&fake, 10_403);
    // to the pixel, give or take what a width that is not a whole number of them rounds to
    assert!(
        (after.left - (before.left - 100)).abs() <= 2,
        "{before:?} {after:?}"
    );
    assert!(
        ((after.left + after.right) - (before.left + before.right)).abs() <= 2,
        "the right edge stays: {before:?} {after:?}"
    );
}
