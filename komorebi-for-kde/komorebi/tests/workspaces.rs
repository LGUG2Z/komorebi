//! Workspaces as the compositor's virtual desktops, against an in-memory desktop.
//!
//! Workspace `i` is the compositor's desktop `i` on each monitor. komorebi asks the compositor to
//! show a workspace and to put a window on one. It follows a switch or a move that something else
//! made without undoing it, and does nothing when the compositor reports what komorebi did itself.
//!
//! Window ids are in the 10_000s and unique per test, because the "hidden window" table is
//! process-wide.

use komorebi::backend::fake::Command;
use komorebi::backend::fake::FakeBackend;
use komorebi::backend::with_backend;
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

fn removed(id: u32) -> WindowManagerEvent {
    WindowManagerEvent::Destroy(
        SystemNotification::KWin(KWinNotification::WindowRemoved),
        Window::from(id),
    )
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

fn open(fake: &FakeBackend, id: u32) {
    fake.open_window(id, "foot", "foot", &format!("terminal {id}"));
}

#[test]
fn a_destroyed_window_is_removed_from_a_workspace_that_is_not_in_front() {
    let (fake, mut wm) = desktop();
    open(&fake, 10_031);
    open(&fake, 10_032);

    with_backend(fake.clone(), || {
        wm.process_event(added(10_031))?;
        wm.process_event(added(10_032))?;
        // send the focused window (10_032) to workspace 1 and stay on workspace 0
        wm.process_command(SocketMessage::SendContainerToWorkspaceNumber(1), Vec::new())?;
        assert!(wm.known_window_ids.contains_key(&10_032));

        fake.close_window_now(10_032);
        wm.process_event(removed(10_032))
    })
    .unwrap();

    assert!(!wm.known_window_ids.contains_key(&10_032));
    assert!(wm.known_window_ids.contains_key(&10_031));
}

fn activated(id: u32) -> WindowManagerEvent {
    WindowManagerEvent::FocusChange(
        SystemNotification::KWin(KWinNotification::WindowActivated),
        Window::from(id),
    )
}

fn workspace_changed(monitor_id: u32, idx: usize) -> WindowManagerEvent {
    WindowManagerEvent::WorkspaceChange(
        SystemNotification::KWin(KWinNotification::WorkspaceChanged),
        monitor_id,
        idx,
    )
}

const MONITOR: u32 = 1;

#[test]
fn switching_workspace_asks_the_compositor_to_show_the_other_desktop() {
    let (fake, mut wm) = desktop();
    open(&fake, 10_061);
    open(&fake, 10_062);

    with_backend(fake.clone(), || {
        wm.process_event(added(10_061))?;
        wm.process_event(added(10_062))?;
        wm.process_command(SocketMessage::SendContainerToWorkspaceNumber(1), Vec::new())?;
        fake.clear_commands();
        wm.process_command(SocketMessage::FocusWorkspaceNumber(1), Vec::new())
    })
    .unwrap();

    assert_eq!(fake.shown_workspace(MONITOR), 1);
    assert!(
        fake.commands()
            .iter()
            .any(|c| matches!(c, Command::SwitchWorkspace(MONITOR, 1))),
        "{:?}",
        fake.commands()
    );
    // the compositor shows the windows of the workspace in front and not the others; komorebi
    // hides nothing
    assert!(
        fake.on_screen(10_062),
        "the window on workspace 1 is not shown"
    );
    assert!(
        !fake.on_screen(10_061),
        "the window on workspace 0 is still shown"
    );
    assert!(
        !fake
            .commands()
            .iter()
            .any(|c| matches!(c, Command::Hide(_))),
        "komorebi hid a window for a workspace switch: {:?}",
        fake.commands()
    );
}

#[test]
fn a_window_sent_to_another_workspace_is_put_on_that_desktop() {
    let (fake, mut wm) = desktop();
    open(&fake, 10_071);
    open(&fake, 10_072);

    with_backend(fake.clone(), || {
        wm.process_event(added(10_071))?;
        wm.process_event(added(10_072))?;
        wm.process_command(SocketMessage::SendContainerToWorkspaceNumber(1), Vec::new())
    })
    .unwrap();

    assert_eq!(fake.workspace_of(10_071), Some(0));
    assert_eq!(fake.workspace_of(10_072), Some(1));
    assert_eq!(
        wm.focused_workspace_idx().unwrap(),
        0,
        "send does not follow"
    );
    assert!(!fake.on_screen(10_072));
}

#[test]
fn moving_a_window_to_another_workspace_puts_it_on_the_desktop_before_showing_it() {
    // focusing a window that is not yet on the desktop being shown would take the monitor back
    let (fake, mut wm) = desktop();
    open(&fake, 10_081);
    open(&fake, 10_082);

    with_backend(fake.clone(), || {
        wm.process_event(added(10_081))?;
        wm.process_event(added(10_082))?;
        fake.clear_commands();
        wm.process_command(SocketMessage::MoveContainerToWorkspaceNumber(1), Vec::new())
    })
    .unwrap();

    let commands = fake.commands();
    let assigned = commands
        .iter()
        .position(|c| matches!(c, Command::SetWindowWorkspace(10_082, 1)))
        .expect("the window was never put on workspace 1's desktop");
    let switched = commands
        .iter()
        .position(|c| matches!(c, Command::SwitchWorkspace(MONITOR, 1)))
        .expect("the monitor was never switched");
    let focused = commands
        .iter()
        .position(|c| matches!(c, Command::Focus(10_082, _)))
        .expect("the moved window was never focused");

    assert!(assigned < switched, "{commands:?}");
    assert!(assigned < focused, "{commands:?}");
    assert_eq!(wm.focused_workspace_idx().unwrap(), 1);
    assert!(fake.on_screen(10_082));
}

#[test]
fn a_workspace_switch_by_someone_else_is_followed_not_undone() {
    let (fake, mut wm) = desktop();
    open(&fake, 10_091);
    open(&fake, 10_092);

    with_backend(fake.clone(), || {
        wm.process_event(added(10_091))?;
        wm.process_event(added(10_092))?;
        wm.process_command(SocketMessage::SendContainerToWorkspaceNumber(1), Vec::new())
    })
    .unwrap();

    // a pager, a shortcut, or an app raising a window of workspace 1: the compositor shows it
    fake.show_workspace_by_user(MONITOR, 1);
    fake.clear_commands();
    with_backend(fake.clone(), || {
        wm.process_event(workspace_changed(MONITOR, 1))
    })
    .unwrap();

    assert_eq!(
        wm.focused_workspace_idx().unwrap(),
        1,
        "komorebi did not follow"
    );
    assert!(
        !fake
            .commands()
            .iter()
            .any(|c| matches!(c, Command::SwitchWorkspace(..))),
        "komorebi asked for a switch in reply to one: {:?}",
        fake.commands()
    );
    assert_eq!(fake.shown_workspace(MONITOR), 1);
}

#[test]
fn the_compositor_reporting_komorebis_own_switch_changes_nothing() {
    let (fake, mut wm) = desktop();
    open(&fake, 10_101);

    with_backend(fake.clone(), || {
        wm.process_event(added(10_101))?;
        wm.process_command(SocketMessage::FocusWorkspaceNumber(1), Vec::new())?;
        fake.clear_commands();
        wm.process_event(workspace_changed(MONITOR, 1))
    })
    .unwrap();

    assert_eq!(wm.focused_workspace_idx().unwrap(), 1);
    assert!(fake.commands().is_empty(), "{:?}", fake.commands());
}

#[test]
fn a_window_open_on_a_desktop_at_startup_starts_on_that_workspace() {
    let fake = FakeBackend::with_one_monitor();
    fake.open_window(10_111, "foot", "foot", "terminal 10111");
    fake.open_window(10_112, "foot", "foot", "terminal 10112");
    // the user had put one on the second desktop, and was looking at the first
    fake.show_workspace_by_user(MONITOR, 0);
    {
        use komorebi::backend::Backend;
        fake.set_window_workspace(10_112, 1).unwrap();
        fake.clear_commands();
    }
    let (_fake, wm) = desktop_on(fake);

    let workspaces = wm.focused_monitor().unwrap().workspaces();
    let ids = |idx: usize| -> Vec<u32> {
        workspaces
            .get(idx)
            .map(|w| {
                w.containers()
                    .iter()
                    .flat_map(|c| c.windows().iter().map(|w| w.id))
                    .collect()
            })
            .unwrap_or_default()
    };
    assert_eq!(ids(0), [10_111]);
    assert_eq!(ids(1), [10_112]);
}

#[test]
fn focus_going_to_a_window_on_the_workspace_in_front_changes_nothing() {
    let (fake, mut wm) = desktop();
    open(&fake, 10_411);
    open(&fake, 10_412);

    with_backend(fake.clone(), || {
        wm.process_event(added(10_411))?;
        wm.process_event(added(10_412))?;
        fake.clear_commands();
        wm.process_event(activated(10_411))
    })
    .unwrap();

    assert_eq!(wm.focused_workspace_idx().unwrap(), 0);
    assert!(!fake.is_hidden(10_411) && !fake.is_hidden(10_412));
}

fn moved_to_desktop(id: u32, idx: usize) -> WindowManagerEvent {
    WindowManagerEvent::WorkspaceMove(
        SystemNotification::KWin(KWinNotification::WindowMovedToDesktop),
        Window::from(id),
        idx,
    )
}

#[test]
fn a_window_moved_to_another_desktop_by_someone_else_changes_workspace() {
    let (fake, mut wm) = desktop();
    open(&fake, 10_501);
    open(&fake, 10_502);

    with_backend(fake.clone(), || {
        wm.process_event(added(10_501))?;
        wm.process_event(added(10_502))?;
        // "Move to Desktop 2" in the window menu
        wm.process_event(moved_to_desktop(10_502, 1))
    })
    .unwrap();

    let ids = |idx: usize| -> Vec<u32> {
        wm.focused_monitor()
            .unwrap()
            .workspaces()
            .get(idx)
            .map(|w| {
                w.containers()
                    .iter()
                    .flat_map(|c| c.windows().iter().map(|w| w.id))
                    .collect()
            })
            .unwrap_or_default()
    };
    assert_eq!(ids(0), [10_501]);
    assert_eq!(ids(1), [10_502]);

    // the tile left behind was given to the other window
    let remaining = fake.rect_of(10_501).unwrap();
    assert!(remaining.right > 1500, "{remaining:?}");
}

#[test]
fn komorebis_own_move_coming_back_as_an_event_moves_nothing() {
    let (fake, mut wm) = desktop();
    open(&fake, 10_511);
    open(&fake, 10_512);

    with_backend(fake.clone(), || {
        wm.process_event(added(10_511))?;
        wm.process_event(added(10_512))?;
        wm.process_command(SocketMessage::SendContainerToWorkspaceNumber(1), Vec::new())?;
        fake.clear_commands();
        wm.process_event(moved_to_desktop(10_512, 1))
    })
    .unwrap();

    assert!(
        fake.commands().is_empty(),
        "an event about komorebi's own move caused commands: {:?}",
        fake.commands()
    );
    assert_eq!(wm.focused_workspace_idx().unwrap(), 0);
}
