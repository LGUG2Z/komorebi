//! The options of the layouts, `layout-ratios` and `scrolling-layout-columns`, and the scrolling
//! layout's strip, against an in-memory desktop: the messages go in through
//! [`WindowManager::process_command`], and what comes out is where the backend is asked to put
//! each window.
//!
//! Window ids are in the 22_000s.

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
use std::num::NonZeroUsize;
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

/// A window manager over a fake desktop with one 1920x1080 monitor (1920x1050 without the panel)
fn desktop() -> (Arc<FakeBackend>, WindowManager) {
    let fake = Arc::new(FakeBackend::with_one_monitor());
    let (_sender, receiver) = crossbeam_channel::bounded(1);
    // unix socket paths are limited to ~104 bytes on macOS, which the temp dir alone can exceed
    static NEXT: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);
    let socket = std::path::PathBuf::from(format!(
        "/tmp/kmo-{}-{}.sock",
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

fn open(fake: &Arc<FakeBackend>, wm: &mut WindowManager, id: u32) {
    fake.open_window(id, "foot", "foot", &format!("terminal {id}"));
    with_backend(fake.clone(), || wm.process_event(added(id))).unwrap();
}

fn send(fake: &Arc<FakeBackend>, wm: &mut WindowManager, message: SocketMessage) {
    with_backend(fake.clone(), || wm.process_command(message, Vec::new())).unwrap();
}

fn columns(count: usize) -> SocketMessage {
    SocketMessage::ScrollingLayoutColumns(NonZeroUsize::new(count).unwrap())
}

fn in_view(rect: Rect) -> bool {
    rect.left >= 0 && rect.left + rect.right <= 1920
}

fn out_of_view(rect: Rect) -> bool {
    rect.left >= 1920 || rect.left + rect.right <= 0
}

#[test]
fn layout_ratios_give_the_first_column_its_share_of_the_work_area() {
    let (fake, mut wm) = desktop();
    send(
        &fake,
        &mut wm,
        SocketMessage::ChangeLayout(DefaultLayout::Columns),
    );
    open(&fake, &mut wm, 22_001);
    open(&fake, &mut wm, 22_002);
    let even = fake.rect_of(22_001).unwrap();

    send(
        &fake,
        &mut wm,
        SocketMessage::LayoutRatios(Some(vec![0.75]), None),
    );

    let (first, second) = (fake.rect_of(22_001).unwrap(), fake.rect_of(22_002).unwrap());
    assert!(first.right > even.right, "{first:?} is wider than {even:?}");
    assert!(
        first.right > 2 * second.right,
        "three quarters against one: {first:?} {second:?}"
    );
    assert_eq!(
        wm.focused_workspace()
            .unwrap()
            .layout_options
            .and_then(|options| options.column_ratios)
            .and_then(|ratios| ratios[0]),
        Some(0.75)
    );
}

#[test]
fn the_scrolling_strip_runs_past_the_edge_of_the_work_area_with_the_focused_window_in_view() {
    let (fake, mut wm) = desktop();
    send(
        &fake,
        &mut wm,
        SocketMessage::ChangeLayout(DefaultLayout::Scrolling),
    );
    send(&fake, &mut wm, columns(2));
    for id in 22_011..=22_015 {
        open(&fake, &mut wm, id);
    }

    let rects: Vec<Rect> = (22_011..=22_015)
        .map(|id| fake.rect_of(id).unwrap())
        .collect();
    assert!(
        in_view(rects[4]),
        "the focused window is in view: {rects:?}"
    );
    assert!(
        in_view(rects[3]),
        "two columns: so is the one before it: {rects:?}"
    );
    for rect in &rects[..3] {
        assert!(
            out_of_view(*rect),
            "the first three are scrolled out of view: {rects:?}"
        );
    }
    // not clamped: every column is as wide as the others, at the same distance from the next
    let pitch = rects[4].left - rects[3].left;
    assert_eq!(rects[0].left, rects[3].left - 3 * pitch, "{rects:?}");
    assert!(
        rects.iter().all(|rect| rect.right == rects[0].right),
        "{rects:?}"
    );
}

#[test]
fn the_scrolling_strip_scrolls_to_the_window_that_has_the_focus() {
    let (fake, mut wm) = desktop();
    send(
        &fake,
        &mut wm,
        SocketMessage::ChangeLayout(DefaultLayout::Scrolling),
    );
    send(&fake, &mut wm, columns(2));
    for id in 22_021..=22_025 {
        open(&fake, &mut wm, id);
    }

    for id in (22_021..=22_024).rev() {
        send(
            &fake,
            &mut wm,
            SocketMessage::FocusWindow(OperationDirection::Left),
        );
        // the compositor reports the focus it was asked to give, which is what scrolls the strip
        with_backend(fake.clone(), || wm.process_event(activated(id))).unwrap();
    }

    assert_eq!(wm.focused_window().unwrap().id, 22_021);
    assert!(in_view(fake.rect_of(22_021).unwrap()));
    assert!(out_of_view(fake.rect_of(22_025).unwrap()));
}

#[test]
fn scrolling_layout_columns_sets_how_many_columns_are_in_view() {
    let (fake, mut wm) = desktop();
    send(
        &fake,
        &mut wm,
        SocketMessage::ChangeLayout(DefaultLayout::Scrolling),
    );
    for id in 22_031..=22_034 {
        open(&fake, &mut wm, id);
    }

    send(&fake, &mut wm, columns(4));
    let four: Vec<Rect> = (22_031..=22_034)
        .map(|id| fake.rect_of(id).unwrap())
        .collect();
    assert!(four.iter().all(|rect| in_view(*rect)), "{four:?}");

    send(&fake, &mut wm, columns(1));
    let one: Vec<Rect> = (22_031..=22_034)
        .map(|id| fake.rect_of(id).unwrap())
        .collect();
    assert_eq!(
        one.iter().filter(|rect| in_view(**rect)).count(),
        1,
        "{one:?}"
    );
}
