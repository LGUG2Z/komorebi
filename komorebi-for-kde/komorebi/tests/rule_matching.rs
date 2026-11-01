//! A manage rule rescuing a window an ignore rule covers, through the KDE daemon. Which window a
//! rule matches (kinds, strategies, composite, last wins) is tested once for all daemons in
//! `crates/komorebi-config/tests/rule_matching.rs`; what stays here is the daemon's `should_manage`
//! on the fake backend.

use komorebi::backend::fake::FakeBackend;
use komorebi::backend::with_backend;
use komorebi::core::ApplicationIdentifier;
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

// mirrors: manage_rules / a window an ignore rule by <kind> matches is not managed
// mirrors: manage_rules / with a manage rule by <kind> too, a window it matches is managed
// mirrors: manage_rules / ... and tiled: <base> gives it room
#[test]
fn a_manage_rule_rescues_a_window_an_ignore_rule_covers() {
    let fake = Arc::new(FakeBackend::with_one_monitor());
    let (_sender, receiver) = crossbeam_channel::bounded(1);
    let socket = std::path::PathBuf::from(format!("/tmp/kmr-{}.sock", std::process::id()));
    let mut wm = with_backend(fake.clone(), || {
        let mut wm = WindowManager::new(receiver, Some(socket)).unwrap();
        wm.init().unwrap();
        wm
    });

    fake.open_window(10_901, "kwrite", "org.kde.kwrite", "ignored window");
    fake.open_window(10_902, "kwrite", "org.kde.kwrite", "rescued window");

    let send = |wm: &mut WindowManager, message| {
        with_backend(fake.clone(), || wm.process_command(message, Vec::new())).unwrap();
    };
    // the rule tables are process-wide and this is the only test of this file that adds to them
    send(
        &mut wm,
        SocketMessage::IgnoreRule(ApplicationIdentifier::Class, "org.kde.kwrite".to_string()),
    );

    with_backend(fake.clone(), || wm.process_event(added(10_901))).unwrap();
    assert!(!wm.focused_workspace().unwrap().contains_window(10_901));

    send(
        &mut wm,
        SocketMessage::ManageRule(ApplicationIdentifier::Title, "rescued window".to_string()),
    );
    with_backend(fake.clone(), || wm.process_event(added(10_902))).unwrap();

    assert!(wm.focused_workspace().unwrap().contains_window(10_902));
    assert!(!wm.focused_workspace().unwrap().contains_window(10_901));
}
