//! The scrolling layout, against an in-memory desktop.
//!
//! KWin lets a window sit anywhere, so the columns of a strip that are past the edge of a monitor
//! can be on the next monitor over. Those windows still belong to the monitor whose strip they are
//! in.
//!
//! Window ids are in the 10_000s.

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
    // monitor is on the second one. Those windows still belong to the first.
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

        let past_the_edge = fake.rect_of(10_204).unwrap();
        assert!(
            past_the_edge.left >= 1920,
            "the test needs a window on the second monitor, got {past_the_edge:?}"
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
