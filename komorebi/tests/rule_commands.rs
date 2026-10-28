//! The messages that add and show rules, and the title update that lets a rule decide once a
//! window has its title, against an in-memory desktop: the messages go in through
//! [`WindowManager::process_command`], and what comes out is which windows are tiled, which float
//! and what the daemon answers.
//!
//! The rule tables are process-wide, so each test names applications no other test of this file
//! names, and takes out the ignore rules it adds.
//!
//! Window ids are in the 29_000s.

use komorebi::backend::fake::FakeBackend;
use komorebi::backend::with_backend;
use komorebi::core::ApplicationIdentifier;
use komorebi::core::SocketMessage;
use komorebi::core::config_generation::MatchingRule;
use komorebi::window::Window;
use komorebi::window_manager::WindowManager;
use komorebi::window_manager_event::KWinNotification;
use komorebi::window_manager_event::SystemNotification;
use komorebi::window_manager_event::WindowManagerEvent;
use komorebi::window_manager_event_listener;
use std::sync::Arc;
use std::time::Duration;

fn added(id: u32) -> WindowManagerEvent {
    WindowManagerEvent::Show(
        SystemNotification::KWin(KWinNotification::WindowAdded),
        Window::from(id),
    )
}

fn activated(id: u32) -> WindowManagerEvent {
    WindowManagerEvent::FocusChange(
        SystemNotification::KWin(KWinNotification::WindowActivated),
        Window::from(id),
    )
}

fn title_changed(id: u32) -> WindowManagerEvent {
    WindowManagerEvent::TitleUpdate(
        SystemNotification::KWin(KWinNotification::CaptionChanged),
        Window::from(id),
    )
}

/// A window manager over a fake desktop with one 1920x1080 monitor and no windows
fn desktop() -> (Arc<FakeBackend>, WindowManager) {
    let fake = Arc::new(FakeBackend::with_one_monitor());
    let (_sender, receiver) = crossbeam_channel::bounded(1);
    // unix socket paths are limited to ~104 bytes on macOS, which the temp dir alone can exceed
    static NEXT: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);
    let socket = std::path::PathBuf::from(format!(
        "/tmp/kmr-{}-{}.sock",
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

/// Opens a window of the application `exe`, whose class is `class`, titled `title`
fn open(fake: &Arc<FakeBackend>, wm: &mut WindowManager, id: u32, app: (&str, &str, &str)) {
    let (exe, class, title) = app;
    fake.open_window(id, exe, class, title);
    with_backend(fake.clone(), || wm.process_event(added(id))).unwrap();
}

fn send(fake: &Arc<FakeBackend>, wm: &mut WindowManager, message: SocketMessage) {
    with_backend(fake.clone(), || wm.process_command(message, Vec::new())).unwrap();
}

/// What the daemon answers `message` with
fn ask(fake: &Arc<FakeBackend>, wm: &mut WindowManager, message: SocketMessage) -> String {
    let mut reply = Vec::new();
    with_backend(fake.clone(), || wm.process_command(message, &mut reply)).unwrap();
    String::from_utf8(reply).unwrap()
}

/// The windows of the focused workspace's containers, in order
fn tiled(wm: &WindowManager) -> Vec<u32> {
    wm.focused_workspace()
        .unwrap()
        .containers()
        .iter()
        .map(|container| container.focused_window().unwrap().id)
        .collect()
}

/// The floating windows of the focused workspace
fn floating(wm: &WindowManager) -> Vec<u32> {
    wm.focused_workspace()
        .unwrap()
        .floating_windows()
        .iter()
        .map(|window| window.id)
        .collect()
}

fn forget_ignore_rule(id: &str) {
    komorebi::IGNORE_IDENTIFIERS
        .lock()
        .retain(|rule| !matches!(rule, MatchingRule::Simple(rule) if rule.id == id));
}

const TERMINAL: (&str, &str, &str) = ("foot", "foot", "a terminal");

#[test]
fn an_ignore_rule_takes_the_windows_it_names_out_of_the_layout_and_keeps_new_ones_out() {
    let (fake, mut wm) = desktop();
    let game = ("game-29", "org.example.game", "a game");
    open(&fake, &mut wm, 29_001, game);
    open(&fake, &mut wm, 29_002, TERMINAL);
    assert_eq!(tiled(&wm), [29_001, 29_002]);

    send(
        &fake,
        &mut wm,
        SocketMessage::IgnoreRule(ApplicationIdentifier::Exe, String::from("game-29")),
    );
    assert_eq!(tiled(&wm), [29_002]);

    open(&fake, &mut wm, 29_003, game);
    forget_ignore_rule("game-29");
    assert_eq!(tiled(&wm), [29_002]);
}

#[test]
fn a_manage_rule_tiles_a_window_an_ignore_rule_covers() {
    let (fake, mut wm) = desktop();
    // plasmashell is ignored from the start
    let shell = ("plasmashell", "org.kde.plasmashell", "Desktop @ DP-1");
    open(&fake, &mut wm, 29_011, shell);
    assert!(tiled(&wm).is_empty());

    send(
        &fake,
        &mut wm,
        SocketMessage::ManageRule(
            ApplicationIdentifier::Class,
            String::from("org.kde.plasmashell"),
        ),
    );
    open(&fake, &mut wm, 29_012, shell);

    assert_eq!(tiled(&wm), [29_012]);
}

#[test]
fn a_session_float_rule_floats_the_focused_window_and_the_ones_like_it_until_it_is_cleared() {
    let (fake, mut wm) = desktop();
    let calculator = ("kcalc", "org.kde.kcalc", "KCalc");
    open(&fake, &mut wm, 29_021, calculator);
    open(&fake, &mut wm, 29_022, TERMINAL);
    // the user gives the calculator the focus
    fake.set_foreground(Some(29_021));
    with_backend(fake.clone(), || wm.process_event(activated(29_021))).unwrap();

    send(&fake, &mut wm, SocketMessage::SessionFloatRule);
    assert_eq!((tiled(&wm), floating(&wm)), (vec![29_022], vec![29_021]));
    let rules = ask(&fake, &mut wm, SocketMessage::SessionFloatRules);
    assert!(rules.contains("\"org.kde.kcalc\""), "{rules}");

    open(&fake, &mut wm, 29_023, calculator);
    assert_eq!(floating(&wm), [29_021, 29_023], "the next one floats too");

    send(&fake, &mut wm, SocketMessage::ClearSessionFloatRules);
    assert_eq!(ask(&fake, &mut wm, SocketMessage::SessionFloatRules), "[]");
    open(&fake, &mut wm, 29_024, calculator);
    assert_eq!(tiled(&wm), [29_022, 29_024]);
}

#[test]
fn a_window_whose_title_no_longer_matches_an_ignore_rule_is_tiled_when_the_title_changes() {
    let (fake, mut wm) = desktop();
    send(
        &fake,
        &mut wm,
        SocketMessage::IgnoreRule(ApplicationIdentifier::Title, String::from("pending-29")),
    );
    open(
        &fake,
        &mut wm,
        29_031,
        ("browser-29", "org.example.browser", "pending-29"),
    );
    assert!(tiled(&wm).is_empty());

    fake.set_title(29_031, "a page");
    with_backend(fake.clone(), || wm.process_event(title_changed(29_031))).unwrap();
    forget_ignore_rule("pending-29");

    // the title update announces the window again, on the channel the events thread reads
    let events = window_manager_event_listener::event_rx();
    let shown = loop {
        let event = events
            .recv_timeout(Duration::from_secs(5))
            .expect("the window is announced again");
        if event.window_id() == Some(29_031) {
            break event;
        }
    };
    assert!(matches!(shown, WindowManagerEvent::Show(..)), "{shown:?}");
    with_backend(fake.clone(), || wm.process_event(shown)).unwrap();
    assert_eq!(tiled(&wm), [29_031]);
}

#[test]
fn global_state_has_the_rules_and_debug_window_says_why_a_window_is_not_managed() {
    let (fake, mut wm) = desktop();
    open(&fake, &mut wm, 29_041, TERMINAL);
    fake.open_window(29_042, "krunner", "org.kde.krunner", "KRunner");

    let state: serde_json::Value =
        serde_json::from_str(&ask(&fake, &mut wm, SocketMessage::GlobalState)).unwrap();
    let ignored = state["ignore_identifiers"].to_string();
    assert!(ignored.contains("\"org.kde.krunner\""), "{ignored}");
    assert!(state["manage_identifiers"].is_array());
    assert!(state["data_dir"].is_string());

    let debug: serde_json::Value =
        serde_json::from_str(&ask(&fake, &mut wm, SocketMessage::DebugWindow(29_042))).unwrap();
    assert_eq!(debug["should_manage"], false);
    assert_eq!(debug["class"], "org.kde.krunner");
    assert_eq!(debug["matches_ignore_identifier"]["id"], "org.kde.krunner");

    let debug: serde_json::Value =
        serde_json::from_str(&ask(&fake, &mut wm, SocketMessage::DebugWindow(29_041))).unwrap();
    assert_eq!(debug["should_manage"], true);

    // a window there is not is not answered
    assert_eq!(ask(&fake, &mut wm, SocketMessage::DebugWindow(29_049)), "");
}
