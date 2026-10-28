//! The padding around and between the tiles, and the offsets that keep parts of the work area
//! free of tiles, against an in-memory desktop: the messages go in through
//! [`WindowManager::process_command`], and what comes out is where the backend was asked to put
//! the windows.
//!
//! The fake's one monitor is 1920x1080 with a work area of 1920x1050, and a workspace falls back to
//! a workspace padding and a container padding of 5 pixels each.
//!
//! Window ids are in the 28_000s.

use komorebi::backend::fake::FakeBackend;
use komorebi::backend::with_backend;
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

/// A window manager over a fake desktop with one 1920x1080 monitor, and a window for each of
/// `ids`, opened in that order
fn desktop_with(ids: &[u32]) -> (Arc<FakeBackend>, WindowManager) {
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
    for &id in ids {
        open(&fake, &mut wm, id);
    }

    (fake, wm)
}

fn open(fake: &Arc<FakeBackend>, wm: &mut WindowManager, id: u32) {
    fake.open_window(id, "foot", "foot", &format!("terminal {id}"));
    with_backend(fake.clone(), || wm.process_event(added(id))).unwrap();
}

fn send(
    fake: &Arc<FakeBackend>,
    wm: &mut WindowManager,
    message: SocketMessage,
) -> color_eyre::Result<()> {
    with_backend(fake.clone(), || wm.process_command(message, Vec::new()))
}

fn rect(left: i32, top: i32, right: i32, bottom: i32) -> Rect {
    Rect {
        left,
        top,
        right,
        bottom,
    }
}

/// A desktop with one window and no padding, so that the window's frame is its tile
fn one_window_without_padding(id: u32) -> (Arc<FakeBackend>, WindowManager) {
    let (fake, mut wm) = desktop_with(&[id]);
    send(&fake, &mut wm, SocketMessage::FocusedWorkspacePadding(0)).unwrap();
    send(
        &fake,
        &mut wm,
        SocketMessage::FocusedWorkspaceContainerPadding(0),
    )
    .unwrap();
    assert_eq!(fake.rect_of(id), Some(rect(0, 0, 1920, 1050)));
    (fake, wm)
}

#[test]
fn the_paddings_of_the_focused_workspace_are_set_and_adjusted() {
    let (fake, mut wm) = desktop_with(&[28_001]);
    assert_eq!(fake.rect_of(28_001), Some(rect(10, 10, 1900, 1030)));

    // with no padding of its own, the workspace has none to adjust
    assert_eq!(
        send(
            &fake,
            &mut wm,
            SocketMessage::AdjustWorkspacePadding(Sizing::Increase, 5)
        )
        .map_err(|error| error.to_string()),
        Err(String::from("there is no workspace padding"))
    );

    send(&fake, &mut wm, SocketMessage::FocusedWorkspacePadding(20)).unwrap();
    assert_eq!(fake.rect_of(28_001), Some(rect(25, 25, 1870, 1000)));

    send(
        &fake,
        &mut wm,
        SocketMessage::FocusedWorkspaceContainerPadding(0),
    )
    .unwrap();
    assert_eq!(fake.rect_of(28_001), Some(rect(20, 20, 1880, 1010)));

    send(
        &fake,
        &mut wm,
        SocketMessage::AdjustWorkspacePadding(Sizing::Increase, 5),
    )
    .unwrap();
    assert_eq!(fake.rect_of(28_001), Some(rect(25, 25, 1870, 1000)));

    send(
        &fake,
        &mut wm,
        SocketMessage::AdjustContainerPadding(Sizing::Increase, 10),
    )
    .unwrap();
    assert_eq!(fake.rect_of(28_001), Some(rect(35, 35, 1850, 980)));

    // a padding is not made smaller than nothing: a decrease past 0 changes nothing
    send(
        &fake,
        &mut wm,
        SocketMessage::AdjustContainerPadding(Sizing::Decrease, 30),
    )
    .unwrap();
    assert_eq!(fake.rect_of(28_001), Some(rect(35, 35, 1850, 980)));
}

#[test]
fn the_paddings_of_a_workspace_are_set_by_its_indices() {
    let (fake, mut wm) = desktop_with(&[28_011]);

    send(&fake, &mut wm, SocketMessage::ContainerPadding(0, 0, 0)).unwrap();
    send(&fake, &mut wm, SocketMessage::WorkspacePadding(0, 0, 0)).unwrap();
    assert_eq!(fake.rect_of(28_011), Some(rect(0, 0, 1920, 1050)));

    assert_eq!(
        send(&fake, &mut wm, SocketMessage::WorkspacePadding(1, 0, 0))
            .map_err(|error| error.to_string()),
        Err(String::from("there is no monitor"))
    );
}

#[test]
fn the_workspaces_offset_wins_over_the_monitors_which_wins_over_the_global_one() {
    let (fake, mut wm) = one_window_without_padding(28_021);

    send(
        &fake,
        &mut wm,
        SocketMessage::WorkAreaOffset(rect(0, 40, 0, 40)),
    )
    .unwrap();
    assert_eq!(fake.rect_of(28_021), Some(rect(0, 40, 1920, 1010)));

    send(
        &fake,
        &mut wm,
        SocketMessage::MonitorWorkAreaOffset(0, rect(100, 0, 100, 0)),
    )
    .unwrap();
    assert_eq!(fake.rect_of(28_021), Some(rect(100, 0, 1820, 1050)));

    send(
        &fake,
        &mut wm,
        SocketMessage::WorkspaceWorkAreaOffset(0, 0, rect(0, 0, 0, 200)),
    )
    .unwrap();
    assert_eq!(fake.rect_of(28_021), Some(rect(0, 0, 1920, 850)));

    // a monitor or a workspace that is not there is ignored
    send(
        &fake,
        &mut wm,
        SocketMessage::MonitorWorkAreaOffset(3, rect(0, 0, 0, 0)),
    )
    .unwrap();
    send(
        &fake,
        &mut wm,
        SocketMessage::WorkspaceWorkAreaOffset(0, 3, rect(0, 0, 0, 0)),
    )
    .unwrap();
    assert_eq!(fake.rect_of(28_021), Some(rect(0, 0, 1920, 850)));
}

#[test]
fn the_window_based_offset_is_taken_up_to_its_window_limit_and_can_be_turned_off() {
    let (fake, mut wm) = one_window_without_padding(28_031);
    // the monitor's window-based offset applies while the workspace has at most one window
    let monitor = wm.monitors_mut().get_mut(0).unwrap();
    monitor.window_based_work_area_offset = Some(rect(200, 0, 400, 0));
    assert_eq!(monitor.window_based_work_area_offset_limit, 1);

    send(&fake, &mut wm, SocketMessage::Retile).unwrap();
    assert_eq!(fake.rect_of(28_031), Some(rect(200, 0, 1520, 1050)));

    open(&fake, &mut wm, 28_032);
    assert_eq!(fake.rect_of(28_031), Some(rect(0, 0, 960, 1050)));
    assert_eq!(fake.rect_of(28_032), Some(rect(960, 0, 960, 1050)));

    fake.close_window_now(28_032);
    with_backend(fake.clone(), || wm.process_event(removed(28_032))).unwrap();
    assert_eq!(fake.rect_of(28_031), Some(rect(200, 0, 1520, 1050)));

    send(
        &fake,
        &mut wm,
        SocketMessage::ToggleWindowBasedWorkAreaOffset,
    )
    .unwrap();
    assert_eq!(fake.rect_of(28_031), Some(rect(0, 0, 1920, 1050)));

    send(
        &fake,
        &mut wm,
        SocketMessage::ToggleWindowBasedWorkAreaOffset,
    )
    .unwrap();
    assert_eq!(fake.rect_of(28_031), Some(rect(200, 0, 1520, 1050)));
}

fn removed(id: u32) -> WindowManagerEvent {
    WindowManagerEvent::Destroy(
        SystemNotification::KWin(KWinNotification::WindowRemoved),
        Window::from(id),
    )
}
