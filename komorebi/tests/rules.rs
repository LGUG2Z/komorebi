//! The rules that decide whether a window is managed, against an in-memory desktop.
//!
//! Window ids are in the 10_000s; the rule tables are process-wide, so a test removes the rule it
//! adds.

use komorebi::backend::fake::FakeBackend;
use komorebi::backend::with_backend;
use komorebi::core::ApplicationIdentifier;
use komorebi::core::config_generation::IdWithIdentifier;
use komorebi::core::config_generation::MatchingRule;
use komorebi::core::config_generation::MatchingStrategy;
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

#[test]
fn an_ignore_rule_keeps_a_window_unmanaged() {
    let (fake, mut wm) = desktop();
    fake.open_window(
        10_051,
        "secret-app",
        "org.example.secret",
        "hidden from komorebi",
    );

    let rule = MatchingRule::Simple(IdWithIdentifier {
        kind: ApplicationIdentifier::Class,
        id: "org.example.secret".to_string(),
        matching_strategy: Some(MatchingStrategy::Equals),
    });
    komorebi::IGNORE_IDENTIFIERS.lock().push(rule.clone());

    let result = with_backend(fake.clone(), || wm.process_event(added(10_051)));

    komorebi::IGNORE_IDENTIFIERS.lock().retain(|r| r != &rule);
    result.unwrap();

    assert!(!wm.focused_workspace().unwrap().contains_window(10_051));
}
