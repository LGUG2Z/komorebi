//! The pointer picks the monitor: `focus-workspace`, `focus-workspaces`, `focus-last-workspace`,
//! `cycle-empty-workspace`, `close-workspace` and the cycles act on the monitor the pointer is on
//! when the workspace shown there is empty (the behaviour Windows always had; see
//! docs/dev/known-issues.md item 1), against an in-memory desktop with two monitors side by side.
//!
//! Window ids are in the 33_000s.

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

const RIGHT: u32 = 2;

fn added(id: u32) -> WindowManagerEvent {
    WindowManagerEvent::Show(
        SystemNotification::KWin(KWinNotification::WindowAdded),
        Window::from(id),
    )
}

/// The bounds of the monitor on the right, which is also its work area
fn right_half() -> Rect {
    Rect {
        left: 1920,
        top: 0,
        right: 1920,
        bottom: 1080,
    }
}

/// A window manager over a fake desktop with the monitor of `with_one_monitor` on the left and a
/// second one on the right
fn two_monitors() -> (Arc<FakeBackend>, WindowManager) {
    let fake = FakeBackend::with_one_monitor();
    fake.add_monitor(RIGHT, "Virtual-1", right_half(), right_half());
    let fake = Arc::new(fake);
    let (_sender, receiver) = crossbeam_channel::bounded(1);
    // unix socket paths are limited to ~104 bytes on macOS, which the temp dir alone can exceed
    static NEXT: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);
    let socket = std::path::PathBuf::from(format!(
        "/tmp/kmm-{}-{}.sock",
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

fn open_on_the_left(fake: &Arc<FakeBackend>, wm: &mut WindowManager, id: u32) {
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

/// The left monitor has a window and the focus; the right one is empty and has the pointer
fn pointer_on_the_empty_monitor() -> (Arc<FakeBackend>, WindowManager) {
    let (fake, mut wm) = two_monitors();
    open_on_the_left(&fake, &mut wm, 33_001);
    fake.set_cursor(2500, 300);
    assert_eq!(wm.focused_monitor_idx(), 0);
    (fake, wm)
}

#[test]
fn focus_workspace_switches_the_monitor_under_the_pointer_when_it_is_empty() {
    let (fake, mut wm) = pointer_on_the_empty_monitor();

    send(&fake, &mut wm, SocketMessage::FocusWorkspaceNumber(1)).unwrap();

    assert_eq!(
        wm.focused_monitor_idx(),
        1,
        "the pointer's monitor is focused first"
    );
    assert_eq!(wm.monitors()[1].focused_workspace_idx(), 1);
    assert_eq!(
        wm.monitors()[0].focused_workspace_idx(),
        0,
        "and the monitor that had the focus keeps its workspace"
    );
}

#[test]
fn focus_workspaces_starts_from_the_monitor_under_the_pointer() {
    let (fake, mut wm) = pointer_on_the_empty_monitor();

    send(&fake, &mut wm, SocketMessage::FocusWorkspaceNumbers(1)).unwrap();

    assert_eq!(wm.focused_monitor_idx(), 1);
    assert_eq!(wm.monitors()[0].focused_workspace_idx(), 1);
    assert_eq!(wm.monitors()[1].focused_workspace_idx(), 1);
}

#[test]
fn focus_last_workspace_acts_on_the_monitor_under_the_pointer() {
    let (fake, mut wm) = pointer_on_the_empty_monitor();

    send(&fake, &mut wm, SocketMessage::FocusLastWorkspace).unwrap();

    assert_eq!(wm.focused_monitor_idx(), 1);
}

#[test]
fn a_pointer_on_the_focused_monitor_changes_nothing() {
    let (fake, mut wm) = two_monitors();
    open_on_the_left(&fake, &mut wm, 33_002);
    fake.set_cursor(500, 300);

    send(&fake, &mut wm, SocketMessage::FocusWorkspaceNumber(1)).unwrap();

    assert_eq!(wm.focused_monitor_idx(), 0);
    assert_eq!(wm.monitors()[0].focused_workspace_idx(), 1);
}
