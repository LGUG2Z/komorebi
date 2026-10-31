//! Bringing a window to the largest tile with `promote`, `promote-swap`, `promote-window`, and the
//! focus to it with `promote-focus`, against an in-memory desktop: the messages go in through
//! [`WindowManager::process_command`], and what comes out is the order of the containers, where
//! the backend is asked to put each window, and which window has the focus.
//!
//! Window ids are in the 24_000s.

use komorebi::backend::fake::Command;
use komorebi::backend::fake::FakeBackend;
use komorebi::backend::with_backend;
use komorebi::core::DefaultLayout;
use komorebi::core::OperationDirection;
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

/// A window manager over a fake desktop with one 1920x1080 monitor in `layout`, and a window for
/// each of `ids`, opened in that order. The last one has the focus, and the commands it took to
/// get there are cleared.
fn desktop_with(layout: DefaultLayout, ids: &[u32]) -> (Arc<FakeBackend>, WindowManager) {
    let fake = Arc::new(FakeBackend::with_one_monitor());
    let (_sender, receiver) = crossbeam_channel::bounded(1);
    // unix socket paths are limited to ~104 bytes on macOS, which the temp dir alone can exceed
    static NEXT: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);
    let socket = std::path::PathBuf::from(format!(
        "/tmp/kmp-{}-{}.sock",
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

    fake.clear_commands();
    (fake, wm)
}

fn send(fake: &Arc<FakeBackend>, wm: &mut WindowManager, message: SocketMessage) {
    with_backend(fake.clone(), || wm.process_command(message, Vec::new())).unwrap();
}

/// The window of each container of the focused workspace, in order
fn order(wm: &WindowManager) -> Vec<u32> {
    wm.focused_workspace()
        .unwrap()
        .containers()
        .iter()
        .map(|container| container.focused_window().unwrap().id)
        .collect()
}

fn focused(wm: &WindowManager) -> u32 {
    wm.focused_window().unwrap().id
}

#[test]
fn promote_moves_the_focused_window_to_the_largest_tile_and_the_others_along() {
    let (fake, mut wm) = desktop_with(DefaultLayout::BSP, &[24_001, 24_002, 24_003]);
    let largest = fake.rect_of(24_001);

    send(&fake, &mut wm, SocketMessage::Promote);

    assert_eq!(order(&wm), [24_003, 24_001, 24_002]);
    assert_eq!(focused(&wm), 24_003);
    assert_eq!(fake.rect_of(24_003), largest);
}

#[test]
fn promote_swap_swaps_with_the_largest_tile_and_swaps_back() {
    let (fake, mut wm) = desktop_with(DefaultLayout::BSP, &[24_011, 24_012, 24_013]);

    send(&fake, &mut wm, SocketMessage::PromoteSwap);

    assert_eq!(order(&wm), [24_013, 24_012, 24_011]);
    assert_eq!(focused(&wm), 24_013);

    send(&fake, &mut wm, SocketMessage::PromoteSwap);

    assert_eq!(
        order(&wm),
        [24_011, 24_012, 24_013],
        "the largest tile is swapped back"
    );
    assert_eq!(focused(&wm), 24_013);
}

#[test]
fn promote_focus_gives_the_focus_to_the_largest_tile_and_moves_nothing() {
    let ids = [24_021, 24_022, 24_023];
    let (fake, mut wm) = desktop_with(DefaultLayout::BSP, &ids);
    let tiles: Vec<_> = ids.iter().map(|&id| fake.rect_of(id)).collect();

    send(&fake, &mut wm, SocketMessage::PromoteFocus);

    assert_eq!(focused(&wm), 24_021);
    assert_eq!(order(&wm), ids);
    assert_eq!(
        ids.iter().map(|&id| fake.rect_of(id)).collect::<Vec<_>>(),
        tiles
    );
    assert!(
        fake.commands().contains(&Command::Focus(24_021, true)),
        "{:?}",
        fake.commands()
    );
}

#[test]
fn promote_window_promotes_the_window_on_that_side() {
    let (fake, mut wm) = desktop_with(DefaultLayout::Columns, &[24_031, 24_032, 24_033]);

    send(
        &fake,
        &mut wm,
        SocketMessage::PromoteWindow(OperationDirection::Left),
    );

    assert_eq!(order(&wm), [24_032, 24_031, 24_033]);
    assert_eq!(focused(&wm), 24_032);
}

#[test]
fn nothing_is_promoted_in_the_grid_layout() {
    let ids = [24_041, 24_042, 24_043];
    let (fake, mut wm) = desktop_with(DefaultLayout::Grid, &ids);

    send(&fake, &mut wm, SocketMessage::Promote);
    send(&fake, &mut wm, SocketMessage::PromoteSwap);
    send(&fake, &mut wm, SocketMessage::PromoteFocus);

    assert_eq!(order(&wm), ids);
    assert_eq!(focused(&wm), 24_043);
    assert!(fake.commands().is_empty(), "{:?}", fake.commands());
}
