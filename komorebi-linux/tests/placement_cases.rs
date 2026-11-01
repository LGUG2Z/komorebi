//! What a placement rule does beside the other things that decide where a window goes: how a rule
//! names a program, an app id and a title, a place that was preselected, and a locked container.
//!
//! `placement.rs` has the forms of the setting and the places they name. Window ids are in the
//! 61_000s.

use komorebi::backend::fake::FakeBackend;
use komorebi::backend::with_backend;
use komorebi::core::InitialWindowPlacementRules;
use komorebi::core::OperationDirection;
use komorebi::core::SocketMessage;
use komorebi::window::Window;
use komorebi::window_manager::WindowManager;
use komorebi::window_manager_event::KWinNotification;
use komorebi::window_manager_event::SystemNotification;
use komorebi::window_manager_event::WindowManagerEvent;
use std::sync::Arc;

fn desktop(rules: &str) -> (Arc<FakeBackend>, WindowManager) {
    let fake = Arc::new(FakeBackend::with_one_monitor());
    let (_sender, receiver) = crossbeam_channel::bounded(1);
    // unix socket paths are limited to ~104 bytes on macOS, which the temp dir alone can exceed
    static NEXT: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);
    let socket = std::path::PathBuf::from(format!(
        "/tmp/kmq-{}-{}.sock",
        std::process::id(),
        NEXT.fetch_add(1, std::sync::atomic::Ordering::SeqCst)
    ));

    let mut wm = with_backend(fake.clone(), || {
        let mut wm = WindowManager::new(receiver, Some(socket)).unwrap();
        wm.init().unwrap();
        wm
    });

    let rules: InitialWindowPlacementRules = serde_json::from_str(rules).unwrap();
    wm.monitors_mut()[0].workspaces_mut()[0].initial_window_placement_rules = Some(rules);

    (fake, wm)
}

fn open(fake: &Arc<FakeBackend>, wm: &mut WindowManager, id: u32, exe: &str, class: &str) {
    fake.open_window(id, exe, class, &format!("{exe} {id}"));
    with_backend(fake.clone(), || {
        wm.process_event(WindowManagerEvent::Show(
            SystemNotification::KWin(KWinNotification::WindowAdded),
            Window::from(id),
        ))
    })
    .unwrap();
}

/// the ids of the windows of the containers, in order
fn order(fake: &Arc<FakeBackend>, wm: &WindowManager) -> Vec<u32> {
    with_backend(fake.clone(), || {
        wm.focused_workspace()
            .unwrap()
            .containers()
            .iter()
            .filter_map(|c| c.focused_window().map(|w| w.id))
            .collect()
    })
}

fn send(fake: &Arc<FakeBackend>, wm: &mut WindowManager, message: SocketMessage) {
    with_backend(fake.clone(), || wm.process_command(message, Vec::new())).unwrap();
}

fn focus(fake: &Arc<FakeBackend>, wm: &mut WindowManager, id: u32) {
    fake.set_foreground(Some(id));
    with_backend(fake.clone(), || {
        wm.process_event(WindowManagerEvent::FocusChange(
            SystemNotification::KWin(KWinNotification::WindowActivated),
            Window::from(id),
        ))
    })
    .unwrap();
}

#[test]
fn a_rule_may_name_the_program_the_way_windows_does() {
    // `.exe` is ignored in the id, so a configuration written for Windows works here
    let (fake, mut wm) = desktop(
        r#"{ "Primary": { "kind": "Exe", "id": "firefox.exe", "matching_strategy": "Equals" } }"#,
    );

    open(&fake, &mut wm, 61_001, "foot", "foot");
    open(&fake, &mut wm, 61_002, "foot", "foot");
    open(&fake, &mut wm, 61_003, "firefox", "firefox");

    assert_eq!(order(&fake, &wm), [61_003, 61_001, 61_002]);
}

#[test]
fn a_rule_may_name_the_app_id_of_the_window() {
    let (fake, mut wm) = desktop(
        r#"{ "Last": { "kind": "Class", "id": "special", "matching_strategy": "Equals" } }"#,
    );

    open(&fake, &mut wm, 61_011, "foot", "special");
    open(&fake, &mut wm, 61_012, "foot", "foot");
    open(&fake, &mut wm, 61_013, "foot", "foot");

    // a window that no rule matches goes after the focused container, which is the last to open
    assert_eq!(order(&fake, &wm), [61_011, 61_012, 61_013]);

    open(&fake, &mut wm, 61_014, "foot", "special");
    focus(&fake, &mut wm, 61_011);
    open(&fake, &mut wm, 61_015, "foot", "foot");
    open(&fake, &mut wm, 61_016, "foot", "special");

    // 61_015 went after the focused 61_011; 61_016 matches and went to the end
    assert_eq!(
        order(&fake, &wm),
        [61_011, 61_015, 61_012, 61_013, 61_014, 61_016]
    );
}

#[test]
fn a_place_that_was_preselected_wins_over_the_rules() {
    let (fake, mut wm) = desktop(r#""Last""#);
    open(&fake, &mut wm, 61_021, "foot", "foot");
    open(&fake, &mut wm, 61_022, "foot", "foot");
    focus(&fake, &mut wm, 61_021);

    send(
        &fake,
        &mut wm,
        SocketMessage::PreselectDirection(OperationDirection::Right),
    );
    open(&fake, &mut wm, 61_023, "foot", "foot");

    // the rule says the end of the list; the place that was reserved is the one in the middle
    assert_eq!(order(&fake, &wm), [61_021, 61_023, 61_022]);

    // and the rule is back for the next window
    open(&fake, &mut wm, 61_024, "foot", "foot");
    assert_eq!(order(&fake, &wm), [61_021, 61_023, 61_022, 61_024]);
}

#[test]
fn a_rule_that_asks_for_the_place_of_a_locked_container_gets_the_place_after_it() {
    let (fake, mut wm) = desktop(r#""Primary""#);
    open(&fake, &mut wm, 61_031, "foot", "foot");
    send(&fake, &mut wm, SocketMessage::ToggleLock);

    open(&fake, &mut wm, 61_032, "foot", "foot");
    open(&fake, &mut wm, 61_033, "foot", "foot");

    // the first place is the locked container's, so each window takes the one after it
    assert_eq!(order(&fake, &wm), [61_031, 61_033, 61_032]);
}
