//! `komorebic toggle-maximize`, against an in-memory desktop.
//!
//! The compositor maximizes the window over the whole work area; komorebi takes it out of the
//! layout, and puts it back in its tile when it is restored.
//!
//! Window ids are in the 30_000s.

use komorebi::backend::fake::FakeBackend;
use komorebi::backend::with_backend;
use komorebi::core::DefaultLayout;
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

fn desktop() -> (Arc<FakeBackend>, WindowManager) {
    let fake = Arc::new(FakeBackend::with_one_monitor());
    let (_sender, receiver) = crossbeam_channel::bounded(1);
    static NEXT: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);
    let socket = std::path::PathBuf::from(format!(
        "/tmp/kmf-{}-{}.sock",
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

fn tiled(first: u32, count: u32) -> (Arc<FakeBackend>, WindowManager) {
    let (fake, mut wm) = desktop();
    with_backend(fake.clone(), || {
        for id in first..first + count {
            fake.open_window(id, "foot", "foot", &format!("terminal {id}"));
            wm.process_event(added(id)).unwrap();
        }
    });
    (fake, wm)
}

fn command(fake: &Arc<FakeBackend>, wm: &mut WindowManager, message: SocketMessage) {
    with_backend(fake.clone(), || wm.process_command(message, Vec::new())).unwrap();
}

fn work_area() -> Rect {
    Rect {
        left: 0,
        top: 0,
        right: 1920,
        bottom: 1050,
    }
}

#[test]
fn toggle_maximize_takes_the_window_out_of_the_layout_and_back() {
    let (fake, mut wm) = tiled(30_041, 3);
    let before: Vec<Rect> = (30_041..30_044)
        .map(|id| fake.rect_of(id).unwrap())
        .collect();

    command(&fake, &mut wm, SocketMessage::ToggleMaximize);

    // the focused window (30_043) is maximized: the compositor's, the whole work area
    assert!(fake.is_maximized(30_043));
    assert_eq!(fake.rect_of(30_043).unwrap(), work_area());
    let workspace = wm.focused_workspace().unwrap();
    assert_eq!(workspace.maximized_window.map(|w| w.id), Some(30_043));
    assert_eq!(workspace.containers().len(), 2);

    command(&fake, &mut wm, SocketMessage::ToggleMaximize);

    assert!(!fake.is_maximized(30_043));
    let workspace = wm.focused_workspace().unwrap();
    assert!(workspace.maximized_window.is_none());
    assert_eq!(workspace.containers().len(), 3);
    let after: Vec<Rect> = (30_041..30_044)
        .map(|id| fake.rect_of(id).unwrap())
        .collect();
    assert_eq!(before, after, "every tile is where it was");
}

#[test]
fn a_maximized_window_that_closes_leaves_the_layout_whole() {
    let (fake, mut wm) = tiled(30_051, 3);
    command(&fake, &mut wm, SocketMessage::ToggleMaximize);

    with_backend(fake.clone(), || {
        fake.close_window_now(30_053);
        wm.process_event(WindowManagerEvent::Destroy(
            SystemNotification::KWin(KWinNotification::WindowRemoved),
            Window::from(30_053),
        ))
    })
    .unwrap();

    let workspace = wm.focused_workspace().unwrap();
    assert!(workspace.maximized_window.is_none());
    assert_eq!(workspace.containers().len(), 2);
}

#[test]
fn maximizing_works_in_every_layout() {
    for layout in [
        DefaultLayout::BSP,
        DefaultLayout::Columns,
        DefaultLayout::UltrawideVerticalStack,
    ] {
        let (fake, mut wm) = desktop();
        let first = 30_100 + 10 * layout as u32;
        with_backend(fake.clone(), || {
            wm.process_command(SocketMessage::ChangeLayout(layout), Vec::new())
                .unwrap();
            for id in first..first + 3 {
                fake.open_window(id, "foot", "foot", &format!("terminal {id}"));
                wm.process_event(added(id)).unwrap();
            }
        });
        let before: Vec<Rect> = (first..first + 3)
            .map(|id| fake.rect_of(id).unwrap())
            .collect();

        command(&fake, &mut wm, SocketMessage::ToggleMaximize);
        command(&fake, &mut wm, SocketMessage::ToggleMaximize);

        let after: Vec<Rect> = (first..first + 3)
            .map(|id| fake.rect_of(id).unwrap())
            .collect();
        assert_eq!(before, after, "{layout}");
    }
}
