//! Transparency for the windows that do not have focus, against an in-memory desktop.
//!
//! Window ids are in the 30_000s; the transparency settings and the rule tables are process-wide,
//! so the tests take turns.

use komorebi::TRANSPARENCY_BLACKLIST;
use komorebi::backend::fake::FakeBackend;
use komorebi::backend::with_backend;
use komorebi::core::ApplicationIdentifier;
use komorebi::core::SocketMessage;
use komorebi::core::config_generation::IdWithIdentifier;
use komorebi::core::config_generation::MatchingRule;
use komorebi::core::config_generation::MatchingStrategy;
use komorebi::state::GlobalState;
use komorebi::transparency_manager;
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

fn focused(id: u32) -> WindowManagerEvent {
    WindowManagerEvent::FocusChange(
        SystemNotification::KWin(KWinNotification::WindowActivated),
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

static TRANSPARENCY: std::sync::Mutex<()> = std::sync::Mutex::new(());

/// The settings are process-wide, as they are on Windows: one test at a time, from the defaults
fn transparency_settings() -> std::sync::MutexGuard<'static, ()> {
    let guard = TRANSPARENCY.lock().unwrap_or_else(|e| e.into_inner());
    komorebi::settings::settings().update(|settings| {
        settings.shared.transparency.enabled = false;
        settings.shared.transparency.alpha = 200;
    });
    TRANSPARENCY_BLACKLIST.lock().clear();
    guard
}

#[test]
fn unfocused_windows_are_transparent_and_the_focused_one_is_not() {
    let _settings = transparency_settings();
    let (fake, mut wm) = tiled(30_001, 3);
    command(&fake, &mut wm, SocketMessage::Transparency(true));
    command(&fake, &mut wm, SocketMessage::TransparencyAlpha(150));

    // 30_003 is the one with focus
    assert_eq!(fake.opacity_of(30_003), 255);
    assert_eq!(fake.opacity_of(30_001), 150);
    assert_eq!(fake.opacity_of(30_002), 150);

    // focus moves: the opacity goes with it
    with_backend(fake.clone(), || wm.process_event(focused(30_001))).unwrap();
    assert_eq!(fake.opacity_of(30_001), 255);
    assert_eq!(fake.opacity_of(30_003), 150);
}

#[test]
fn turning_transparency_off_makes_everything_opaque_again() {
    let _settings = transparency_settings();
    let (fake, mut wm) = tiled(30_011, 2);
    command(&fake, &mut wm, SocketMessage::Transparency(true));
    assert_eq!(fake.opacity_of(30_011), 200, "the default alpha");

    command(&fake, &mut wm, SocketMessage::ToggleTransparency);
    assert_eq!(fake.opacity_of(30_011), 255);
    assert_eq!(fake.opacity_of(30_012), 255);

    command(&fake, &mut wm, SocketMessage::ToggleTransparency);
    assert_eq!(fake.opacity_of(30_011), 200);
}

#[test]
fn a_monocled_window_is_not_transparent() {
    let _settings = transparency_settings();
    let (fake, mut wm) = tiled(30_021, 3);
    command(&fake, &mut wm, SocketMessage::Transparency(true));
    command(&fake, &mut wm, SocketMessage::ToggleMonocle);

    assert_eq!(fake.opacity_of(30_023), 255, "the one filling the screen");
}

#[test]
fn windows_the_rules_ignore_keep_their_opacity() {
    let _settings = transparency_settings();
    let (fake, mut wm) = desktop();
    with_backend(fake.clone(), || {
        fake.open_window(30_031, "foot", "foot", "terminal");
        fake.open_window(30_032, "mpv", "mpv-player", "video");
        fake.open_window(30_033, "foot", "foot", "terminal 2");
        wm.process_event(added(30_031)).unwrap();
        wm.process_event(added(30_032)).unwrap();
        wm.process_event(added(30_033)).unwrap();
    });

    TRANSPARENCY_BLACKLIST
        .lock()
        .push(MatchingRule::Simple(IdWithIdentifier {
            kind: ApplicationIdentifier::Class,
            id: String::from("mpv-player"),
            matching_strategy: Some(MatchingStrategy::Equals),
        }));

    command(&fake, &mut wm, SocketMessage::Transparency(true));

    assert_eq!(fake.opacity_of(30_031), 200);
    assert_eq!(
        fake.opacity_of(30_032),
        255,
        "a video player is never see-through"
    );

    TRANSPARENCY_BLACKLIST.lock().clear();
}

#[test]
fn what_komorebi_made_see_through_is_opaque_again_when_it_exits() {
    let _settings = transparency_settings();
    let (fake, mut wm) = tiled(30_041, 2);
    command(&fake, &mut wm, SocketMessage::Transparency(true));
    assert_eq!(fake.opacity_of(30_041), 200);

    with_backend(fake.clone(), || transparency_manager::restore(&mut wm));

    assert_eq!(fake.opacity_of(30_041), 255);
    assert!(wm.opacity_applied.is_empty());
}

#[test]
fn the_global_state_says_what_the_transparency_is() {
    let _settings = transparency_settings();
    let (fake, mut wm) = desktop();
    assert!(!GlobalState::default().transparency_enabled);

    command(&fake, &mut wm, SocketMessage::Transparency(true));
    command(&fake, &mut wm, SocketMessage::TransparencyAlpha(90));

    let state = GlobalState::default();
    assert!(state.transparency_enabled);
    assert_eq!(state.transparency_alpha, 90);
}
