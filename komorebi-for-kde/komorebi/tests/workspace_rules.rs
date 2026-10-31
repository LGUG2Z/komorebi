//! Workspace rules against an in-memory desktop: the messages that add, clear and enforce them,
//! the rules a `komorebi.json` gives, and what happens to the window a rule names.
//!
//! A rule is looked at on every event, before the event is handled, and a window is in a container
//! only once its `Show` has been handled: so a window opens with the `Show` of its event and is
//! sent to its workspace at the next, the activation KWin reports after it. The rules are
//! process-wide, so the tests of this file take turns and each clears them first, and they name
//! the applications of their own.
//!
//! Window ids are in the 42_000s.

use komorebi::backend::fake::FakeBackend;
use komorebi::backend::with_backend;
use komorebi::core::ApplicationIdentifier;
use komorebi::core::Rect;
use komorebi::core::SocketMessage;
use komorebi::static_config::StaticConfig;
use komorebi::window::Window;
use komorebi::window_manager::WindowManager;
use komorebi::window_manager_event::KWinNotification;
use komorebi::window_manager_event::SystemNotification;
use komorebi::window_manager_event::WindowManagerEvent;
use parking_lot::Mutex;
use std::os::unix::net::UnixListener;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::MutexGuard;
use std::sync::atomic::AtomicU32;
use std::sync::atomic::Ordering;

/// What the tests of this file take turns with
static TURN: std::sync::Mutex<()> = std::sync::Mutex::new(());

/// A window manager over a fake desktop, which holds its turn for as long as the test does
struct Desktop {
    fake: Arc<FakeBackend>,
    wm: WindowManager,
    _turn: MutexGuard<'static, ()>,
}

fn short_path(suffix: &str) -> PathBuf {
    static NEXT: AtomicU32 = AtomicU32::new(0);
    PathBuf::from(format!(
        "/tmp/kmw-{}-{}.{suffix}",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::SeqCst)
    ))
}

fn event(kind: KWinNotification, id: u32) -> WindowManagerEvent {
    match kind {
        KWinNotification::WindowActivated => {
            WindowManagerEvent::FocusChange(SystemNotification::KWin(kind), Window::from(id))
        }
        _ => WindowManagerEvent::Show(SystemNotification::KWin(kind), Window::from(id)),
    }
}

impl Desktop {
    /// A desktop with one monitor, whose first monitor has the workspaces `web`, `code` and
    /// `chat`
    fn new() -> Self {
        let turn = TURN.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
        let fake = Arc::new(FakeBackend::with_one_monitor());
        let mut desktop = Self::on(fake, turn);
        desktop.send(SocketMessage::ClearAllWorkspaceRules);
        desktop.send(SocketMessage::EnsureNamedWorkspaces(
            0,
            vec!["web".into(), "code".into(), "chat".into()],
        ));
        desktop
    }

    fn on(fake: Arc<FakeBackend>, turn: MutexGuard<'static, ()>) -> Self {
        let (_sender, receiver) = crossbeam_channel::bounded(1);
        let wm = with_backend(fake.clone(), || {
            let mut wm = WindowManager::new(receiver, Some(short_path("sock"))).unwrap();
            wm.init().unwrap();
            wm
        });
        Self {
            fake,
            wm,
            _turn: turn,
        }
    }

    fn send(&mut self, message: SocketMessage) {
        with_backend(self.fake.clone(), || {
            self.wm.process_command(message, Vec::new())
        })
        .unwrap();
    }

    fn handle(&mut self, event: WindowManagerEvent) {
        with_backend(self.fake.clone(), || self.wm.process_event(event)).unwrap();
    }

    /// Opens a window of an application and has it activated, as KWin reports a new window
    fn open(&mut self, id: u32, exe: &str) {
        self.fake.open_window(id, exe, exe, &format!("window {id}"));
        self.handle(event(KWinNotification::WindowAdded, id));
        self.handle(event(KWinNotification::WindowActivated, id));
    }

    /// Which workspace of the first monitor has the window in a container, if one does
    fn workspace_of(&self, id: u32) -> Option<usize> {
        self.wm.monitors()[0]
            .workspaces()
            .iter()
            .position(|workspace| workspace.contains_window(id))
    }

    fn shown(&self) -> usize {
        self.wm.monitors()[0].focused_workspace_idx()
    }
}

#[test]
fn a_rule_sends_the_window_of_an_application_to_its_workspace() {
    let mut desktop = Desktop::new();
    desktop.send(SocketMessage::WorkspaceRule(
        ApplicationIdentifier::Exe,
        String::from("rule-sends"),
        0,
        1,
    ));

    desktop.open(42_001, "rule-sends");
    desktop.open(42_002, "foot");

    // the window of the application is on the workspace the rule names, and on its desktop,
    // hidden, since it left the workspace in front; the other stays, and the workspace in front is
    // the one it was
    assert_eq!(desktop.workspace_of(42_001), Some(1));
    assert_eq!(desktop.fake.workspace_of(42_001), Some(1));
    assert!(desktop.fake.is_hidden(42_001));
    assert_eq!(desktop.workspace_of(42_002), Some(0));
    assert!(!desktop.fake.is_hidden(42_002));
    assert_eq!(desktop.shown(), 0);
    assert!(!desktop.fake.on_screen(42_001));

    // when the workspace is shown, the window is brought back and tiled in it
    desktop.send(SocketMessage::FocusWorkspaceNumber(1));
    assert!(!desktop.fake.is_hidden(42_001));
    assert!(desktop.fake.on_screen(42_001));
    assert_eq!(
        desktop.fake.rect_of(42_001),
        Some(Rect {
            left: 10,
            top: 10,
            right: 1900,
            bottom: 1030
        })
    );
}

#[test]
fn a_window_a_rule_moves_between_workspaces_that_are_out_of_view_is_put_on_the_new_desktop() {
    let mut desktop = Desktop::new();
    desktop.send(SocketMessage::WorkspaceRule(
        ApplicationIdentifier::Exe,
        String::from("rule-between"),
        0,
        1,
    ));
    desktop.open(42_021, "rule-between");
    assert_eq!(desktop.workspace_of(42_021), Some(1));
    assert_eq!(desktop.fake.workspace_of(42_021), Some(1));

    // the rule now names another workspace, and the window moves between two that are out of view:
    // nothing hides it, so what takes it to the desktop of its new workspace is the assignment
    desktop.send(SocketMessage::ClearAllWorkspaceRules);
    desktop.send(SocketMessage::WorkspaceRule(
        ApplicationIdentifier::Exe,
        String::from("rule-between"),
        0,
        2,
    ));
    desktop.send(SocketMessage::EnforceWorkspaceRules);

    assert_eq!(desktop.workspace_of(42_021), Some(2));
    assert_eq!(desktop.fake.workspace_of(42_021), Some(2));
    assert_eq!(desktop.shown(), 0);
}

#[test]
fn an_initial_rule_moves_a_window_once_and_a_permanent_one_every_time() {
    let mut desktop = Desktop::new();
    desktop.send(SocketMessage::InitialWorkspaceRule(
        ApplicationIdentifier::Exe,
        String::from("rule-once"),
        0,
        1,
    ));
    desktop.send(SocketMessage::WorkspaceRule(
        ApplicationIdentifier::Exe,
        String::from("rule-always"),
        0,
        2,
    ));
    desktop.open(42_011, "rule-once");
    desktop.open(42_012, "rule-always");
    assert_eq!(desktop.workspace_of(42_011), Some(1));
    assert_eq!(desktop.workspace_of(42_012), Some(2));

    // the user takes each of them to the first workspace
    for (id, from) in [(42_011, 1), (42_012, 2)] {
        desktop.send(SocketMessage::FocusWorkspaceNumber(from));
        desktop.send(SocketMessage::MoveContainerToWorkspaceNumber(0));
        assert_eq!(desktop.workspace_of(id), Some(0));
    }
    // the next event is about a window of the user's: it looks at the rules again
    desktop.open(42_013, "foot");

    assert_eq!(
        desktop.workspace_of(42_011),
        Some(0),
        "an initial rule is applied once"
    );
    assert_eq!(
        desktop.workspace_of(42_012),
        Some(2),
        "a permanent rule is applied again"
    );

    // `EnforceWorkspaceRules` forgets what the initial rules did
    desktop.send(SocketMessage::EnforceWorkspaceRules);
    assert_eq!(desktop.workspace_of(42_011), Some(1));
}

#[test]
fn a_rule_can_name_its_workspace_and_a_workspace_that_is_not_there_is_made() {
    let mut desktop = Desktop::new();
    desktop.send(SocketMessage::NamedWorkspaceRule(
        ApplicationIdentifier::Exe,
        String::from("rule-named"),
        String::from("chat"),
    ));
    desktop.send(SocketMessage::InitialNamedWorkspaceRule(
        ApplicationIdentifier::Exe,
        String::from("rule-named-once"),
        String::from("code"),
    ));
    // a name no workspace has: the rule is not made
    desktop.send(SocketMessage::NamedWorkspaceRule(
        ApplicationIdentifier::Exe,
        String::from("rule-nameless"),
        String::from("nowhere"),
    ));
    desktop.send(SocketMessage::WorkspaceRule(
        ApplicationIdentifier::Exe,
        String::from("rule-far"),
        0,
        5,
    ));

    desktop.open(42_021, "rule-named");
    desktop.open(42_022, "rule-named-once");
    desktop.open(42_023, "rule-nameless");
    desktop.open(42_024, "rule-far");

    assert_eq!(desktop.workspace_of(42_021), Some(2));
    assert_eq!(desktop.workspace_of(42_022), Some(1));
    assert_eq!(desktop.workspace_of(42_023), Some(0));
    assert_eq!(desktop.workspace_of(42_024), Some(5));
    assert_eq!(desktop.wm.monitors()[0].workspaces().len(), 6);
}

#[test]
fn rules_are_cleared_by_workspace_by_name_and_all_at_once() {
    let mut desktop = Desktop::new();
    let add = |desktop: &mut Desktop, exe: &str, workspace: usize| {
        desktop.send(SocketMessage::WorkspaceRule(
            ApplicationIdentifier::Exe,
            exe.to_string(),
            0,
            workspace,
        ));
    };
    let listed = |desktop: &mut Desktop| -> Vec<String> {
        let mut reply = Vec::new();
        with_backend(desktop.fake.clone(), || {
            desktop
                .wm
                .process_command(SocketMessage::GlobalState, &mut reply)
        })
        .unwrap();
        let state: serde_json::Value = serde_json::from_slice(&reply).unwrap();
        state["workspace_rules"]
            .as_array()
            .unwrap()
            .iter()
            .map(|rule| rule["matching_rule"]["id"].as_str().unwrap().to_string())
            .collect()
    };

    add(&mut desktop, "clear-a", 1);
    add(&mut desktop, "clear-a", 1);
    assert_eq!(listed(&mut desktop), ["clear-a"], "a rule is added once");

    desktop.send(SocketMessage::ClearWorkspaceRules(0, 1));
    assert!(listed(&mut desktop).is_empty());

    add(&mut desktop, "clear-b", 2);
    desktop.send(SocketMessage::ClearNamedWorkspaceRules(String::from(
        "chat",
    )));
    assert!(listed(&mut desktop).is_empty());

    add(&mut desktop, "clear-c", 1);
    add(&mut desktop, "clear-d", 2);
    assert_eq!(listed(&mut desktop).len(), 2);
    desktop.send(SocketMessage::ClearAllWorkspaceRules);
    assert!(listed(&mut desktop).is_empty());
}

#[test]
fn a_window_goes_to_a_workspace_of_another_monitor_and_is_tiled_there() {
    let turn = TURN.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
    let fake = Arc::new(FakeBackend::with_one_monitor());
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
            bottom: 1080,
        },
    );
    let mut desktop = Desktop::on(fake, turn);
    desktop.send(SocketMessage::ClearAllWorkspaceRules);
    desktop.send(SocketMessage::WorkspaceRule(
        ApplicationIdentifier::Exe,
        String::from("rule-monitor"),
        1,
        0,
    ));

    desktop.open(42_041, "rule-monitor");
    desktop.send(SocketMessage::Retile);

    assert!(desktop.wm.monitors()[1].workspaces()[0].contains_window(42_041));
    assert!(!desktop.wm.monitors()[0].workspaces()[0].contains_window(42_041));
    let rect = desktop.fake.rect_of(42_041).unwrap();
    assert!(
        rect.left >= 1920,
        "the window is on the second monitor: {rect:?}"
    );
}

/// Loads a `komorebi.json` over a desktop that has a window of `exe` open already
fn load(
    json: &str,
    exe: &str,
    id: u32,
) -> (
    Arc<FakeBackend>,
    Arc<Mutex<WindowManager>>,
    MutexGuard<'static, ()>,
) {
    let turn = TURN.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
    let fake = Arc::new(FakeBackend::with_one_monitor());
    fake.open_window(id, exe, exe, "an application that was open");
    let path = short_path("json");
    std::fs::write(&path, json).unwrap();
    let socket = short_path("sock");
    let (_sender, receiver) = crossbeam_channel::bounded(1);

    let wm = with_backend(fake.clone(), || {
        let mut wm =
            StaticConfig::preload(&path, receiver, Some(UnixListener::bind(&socket).unwrap()))
                .unwrap();
        wm.init().unwrap();
        wm
    });
    let wm = Arc::new(Mutex::new(wm));
    with_backend(fake.clone(), || StaticConfig::postload(&path, &wm)).unwrap();
    let _ = std::fs::remove_file(&socket);
    let _ = std::fs::remove_file(&path);
    (fake, wm, turn)
}

#[test]
fn the_rules_of_komorebi_json_send_the_windows_that_are_open_and_come_out_in_the_generated_file() {
    let (_fake, wm, _turn) = load(
        r#"{
            "monitors": [
                {
                    "workspaces": [
                        { "name": "web", "layout": "BSP" },
                        {
                            "name": "code",
                            "layout": "BSP",
                            "workspace_rules": [
                                { "kind": "Exe", "id": "config-rule", "matching_strategy": "Equals" }
                            ],
                            "initial_workspace_rules": [
                                { "kind": "Class", "id": "config-initial", "matching_strategy": "Equals" }
                            ]
                        }
                    ]
                }
            ]
        }"#,
        "config-rule",
        42_051,
    );

    // the window that was open when the daemon started is where the file says it goes
    {
        let wm = wm.lock();
        let workspaces = wm.monitors()[0].workspaces();
        assert!(workspaces[1].contains_window(42_051));
        assert!(!workspaces[0].contains_window(42_051));
    }

    // the rules are the window manager's, with the workspace they send to, and the file the
    // window manager generates has them again
    let generated = StaticConfig::from(&*wm.lock());
    let code = &generated.monitors.unwrap()[0].workspaces[1];
    assert_eq!(code.workspace_rules.as_ref().map(Vec::len), Some(1));
    assert_eq!(code.initial_workspace_rules.as_ref().map(Vec::len), Some(1));
}
