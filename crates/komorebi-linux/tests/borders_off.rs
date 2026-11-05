//! With borders off, the compositor is told no window has a color; with them on again, it is told.
//!
//! This file has one test because it changes the process-wide border settings, which every other
//! test that tiles windows would see (their layouts would gain or lose room for a border in the middle
//! of the test).

use komorebi::backend::fake::FakeBackend;
use komorebi::backend::with_backend;
use komorebi::core::SocketMessage;
use komorebi::window::Window;
use komorebi::window_manager::WindowManager;
use komorebi::window_manager_event::KWinNotification;
use komorebi::window_manager_event::SystemNotification;
use komorebi::window_manager_event::WindowManagerEvent;
use std::sync::Arc;

#[test]
fn with_borders_off_no_window_has_a_colour() {
    let fake = Arc::new(FakeBackend::with_one_monitor());
    let (_sender, receiver) = crossbeam_channel::bounded(1);
    let socket = std::path::PathBuf::from(format!("/tmp/kmb-off-{}.sock", std::process::id()));
    let mut wm = with_backend(fake.clone(), || {
        let mut wm = WindowManager::new(receiver, Some(socket)).unwrap();
        wm.init().unwrap();
        wm
    });

    for id in [91_001, 91_002] {
        fake.open_window(id, "foot", "foot", "terminal");
        with_backend(fake.clone(), || {
            wm.process_event(WindowManagerEvent::Show(
                SystemNotification::KWin(KWinNotification::WindowAdded),
                Window::from(id),
            ))
            .unwrap();
        });
    }
    assert_eq!(fake.border_colour_of(91_002), Some(0x42A5F5));

    with_backend(fake.clone(), || {
        wm.process_command(SocketMessage::Border(false), Vec::new())
    })
    .unwrap();
    assert_eq!(fake.border_colour_of(91_001), None);
    assert_eq!(fake.border_colour_of(91_002), None);

    with_backend(fake.clone(), || {
        wm.process_command(SocketMessage::Border(true), Vec::new())
    })
    .unwrap();
    assert_eq!(fake.border_colour_of(91_002), Some(0x42A5F5));
}
