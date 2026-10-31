//! `initial_window_placement_rules`: where in the list of containers a window that opens is put.
//!
//! The default is after the focused container. A workspace can say another place for every window
//! (`"Primary"`, `"Last"`, `2`, ...), or a place for the windows that match a rule.

use komorebi::backend::fake::FakeBackend;
use komorebi::backend::with_backend;
use komorebi::core::InitialWindowPlacementRules;
use komorebi::static_config::StaticConfig;
use komorebi::window::Window;
use komorebi::window_manager::WindowManager;
use komorebi::window_manager_event::KWinNotification;
use komorebi::window_manager_event::SystemNotification;
use komorebi::window_manager_event::WindowManagerEvent;
use std::sync::Arc;

fn desktop(rules: Option<&str>) -> (Arc<FakeBackend>, WindowManager) {
    let fake = Arc::new(FakeBackend::with_one_monitor());
    let (_sender, receiver) = crossbeam_channel::bounded(1);
    static NEXT: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);
    let socket = std::path::PathBuf::from(format!(
        "/tmp/kmp-{}-{}.sock",
        std::process::id(),
        NEXT.fetch_add(1, std::sync::atomic::Ordering::SeqCst)
    ));

    let mut wm = with_backend(fake.clone(), || {
        let mut wm = WindowManager::new(receiver, Some(socket)).unwrap();
        wm.init().unwrap();
        wm
    });

    if let Some(rules) = rules {
        let rules: InitialWindowPlacementRules = serde_json::from_str(rules).unwrap();
        wm.monitors_mut()[0].workspaces_mut()[0].initial_window_placement_rules = Some(rules);
    }

    (fake, wm)
}

fn open(fake: &Arc<FakeBackend>, wm: &mut WindowManager, id: u32, exe: &str, title: &str) {
    fake.open_window(id, exe, exe, title);
    with_backend(fake.clone(), || {
        wm.process_event(WindowManagerEvent::Show(
            SystemNotification::KWin(KWinNotification::WindowAdded),
            Window::from(id),
        ))
    })
    .unwrap();
}

/// the titles of the containers, in order
fn order(fake: &Arc<FakeBackend>, wm: &WindowManager) -> Vec<String> {
    with_backend(fake.clone(), || {
        wm.focused_workspace()
            .unwrap()
            .containers()
            .iter()
            .map(|c| c.focused_window().unwrap().title().unwrap())
            .collect()
    })
}

/// open windows with these titles, one after another (the exe is the title's first word)
fn open_all(fake: &Arc<FakeBackend>, wm: &mut WindowManager, base: u32, titles: &[&str]) {
    for (i, title) in titles.iter().enumerate() {
        let exe = title.split_whitespace().next().unwrap();
        open(fake, wm, base + i as u32, exe, title);
    }
}

#[test]
fn without_rules_a_window_goes_after_the_focused_one() {
    let (fake, mut wm) = desktop(None);
    open_all(&fake, &mut wm, 61_000, &["a", "b", "c"]);
    assert_eq!(order(&fake, &wm), ["a", "b", "c"]);
}

#[test]
fn one_place_for_every_window() {
    // the largest tile: each new window takes it, and the others move up
    let (fake, mut wm) = desktop(Some(r#""Primary""#));
    open_all(&fake, &mut wm, 61_100, &["a", "b", "c"]);
    assert_eq!(order(&fake, &wm), ["c", "b", "a"]);

    // the end of the list
    let (fake, mut wm) = desktop(Some(r#""Last""#));
    open_all(&fake, &mut wm, 61_200, &["a", "b", "c"]);
    assert_eq!(order(&fake, &wm), ["a", "b", "c"]);

    // the second tile
    let (fake, mut wm) = desktop(Some(r#""Secondary""#));
    open_all(&fake, &mut wm, 61_300, &["a", "b", "c", "d"]);
    assert_eq!(order(&fake, &wm), ["a", "d", "c", "b"]);

    // before the one in focus
    let (fake, mut wm) = desktop(Some(r#""BeforeFocused""#));
    open_all(&fake, &mut wm, 61_400, &["a", "b"]);
    assert_eq!(order(&fake, &wm), ["b", "a"]);

    // a place by number, from 1
    let (fake, mut wm) = desktop(Some("2"));
    open_all(&fake, &mut wm, 61_500, &["a", "b", "c"]);
    assert_eq!(order(&fake, &wm), ["a", "c", "b"]);
}

#[test]
fn a_place_that_is_not_there_is_the_default() {
    let (fake, mut wm) = desktop(Some("10"));
    open_all(&fake, &mut wm, 61_600, &["a", "b", "c"]);
    assert_eq!(order(&fake, &wm), ["a", "b", "c"]);

    // the second tile of a workspace that has one tile has no second tile
    let (fake, mut wm) = desktop(Some(r#""Secondary""#));
    open_all(&fake, &mut wm, 61_700, &["a", "b"]);
    assert_eq!(order(&fake, &wm), ["a", "b"]);
}

#[test]
fn a_place_for_the_windows_that_match_a_rule() {
    let rules = r#"{
        "Primary": { "kind": "Exe", "id": "firefox", "matching_strategy": "Equals" },
        "Last": [
            { "kind": "Exe", "id": "slack", "matching_strategy": "Equals" },
            { "kind": "Title", "id": "chat", "matching_strategy": "Contains" }
        ]
    }"#;
    let (fake, mut wm) = desktop(Some(rules));

    open_all(
        &fake,
        &mut wm,
        62_000,
        &[
            "term one",
            "term two",
            "firefox page",
            "slack x",
            "term three",
            "other chat room",
        ],
    );

    // firefox took the first tile; slack went to the end, and "term three" after it, where the focus
    // was; the window with "chat" in its title went to the end
    assert_eq!(
        order(&fake, &wm),
        [
            "firefox page",
            "term one",
            "term two",
            "slack x",
            "term three",
            "other chat room"
        ]
    );
}

#[test]
fn a_rule_of_several_conditions_needs_them_all() {
    // code, and a title with "work" in it
    let rules = r#"{
        "1": [[
            { "kind": "Exe", "id": "code", "matching_strategy": "Equals" },
            { "kind": "Title", "id": "work", "matching_strategy": "Contains" }
        ]]
    }"#;
    let (fake, mut wm) = desktop(Some(rules));

    open_all(
        &fake,
        &mut wm,
        63_000,
        &["term a", "code scratch", "code work"],
    );

    // "code scratch" has the exe but not the title: default. "code work" has both: first.
    assert_eq!(order(&fake, &wm), ["code work", "term a", "code scratch"]);
}

#[test]
fn the_first_rule_in_key_order_wins() {
    // a window that matches both: "1" is before "Primary" in key order
    let rules = r#"{
        "Primary": { "kind": "Exe", "id": "x", "matching_strategy": "Equals" },
        "2": { "kind": "Exe", "id": "x", "matching_strategy": "Equals" }
    }"#;
    let (fake, mut wm) = desktop(Some(rules));
    open_all(&fake, &mut wm, 64_000, &["a", "b", "x one"]);

    // "2" sorts before "Primary" ('2' < 'P'): second place
    assert_eq!(order(&fake, &wm), ["a", "x one", "b"]);
}

#[test]
fn a_workspace_in_komorebi_json_has_them() {
    let config: StaticConfig = serde_json::from_str(
        r#"{ "monitors": [ { "workspaces": [
            { "name": "one", "layout": "BSP", "initial_window_placement_rules": "Last" },
            { "name": "two", "layout": "BSP", "initial_window_placement_rules": {
                "Primary": { "kind": "Exe", "id": "firefox", "matching_strategy": "Equals" } } },
            { "name": "three", "layout": "BSP", "initial_window_placement_rules": 3 }
        ] } ] }"#,
    )
    .unwrap();

    let workspaces = &config.monitors.unwrap()[0].workspaces;
    assert!(matches!(
        workspaces[0].initial_window_placement_rules,
        Some(InitialWindowPlacementRules::Target(_))
    ));
    assert!(matches!(
        workspaces[1].initial_window_placement_rules,
        Some(InitialWindowPlacementRules::Rules(_))
    ));
    assert!(matches!(
        workspaces[2].initial_window_placement_rules,
        Some(InitialWindowPlacementRules::Target(_))
    ));

    // and it goes back out the way it came
    let again = serde_json::to_string(&workspaces[1]).unwrap();
    assert!(again.contains("\"Primary\""), "{again}");
}

#[test]
fn a_place_that_is_not_one_is_refused() {
    assert!(serde_json::from_str::<InitialWindowPlacementRules>(r#""Middle""#).is_err());
    assert!(serde_json::from_str::<InitialWindowPlacementRules>("true").is_err());
}

#[test]
fn the_rules_come_from_the_configuration_of_the_workspace() {
    let config: StaticConfig = serde_json::from_str(
        r#"{ "monitors": [ { "workspaces": [
            { "name": "one", "layout": "BSP", "initial_window_placement_rules": "Primary" }
        ] } ] }"#,
    )
    .unwrap();
    let workspace_config = config.monitors.unwrap().remove(0).workspaces.remove(0);

    let mut workspace = komorebi::workspace::Workspace::default();
    workspace
        .load_static_config(&workspace_config, None)
        .unwrap();

    assert_eq!(
        workspace.initial_window_placement_rules,
        workspace_config.initial_window_placement_rules
    );
}
