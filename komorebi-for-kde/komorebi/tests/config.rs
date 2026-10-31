//! `komorebi.json` against an in-memory desktop: what the window manager does with the settings,
//! the workspaces, the rules and the paddings the file gives it, how it comes out again as
//! [`StaticConfig`], and what happens when it is loaded a second time.
//!
//! The file is loaded the way the daemon loads it: [`StaticConfig::preload`] makes the window
//! manager, [`WindowManager::init`] reads the desktop, and [`StaticConfig::postload`] gives the
//! monitors their part of the file. The settings the file sets are process-wide, so the tests of
//! this file take turns, and each starts from the paddings of a window manager with no file.
//!
//! The fake's one monitor is 1920x1080 with a work area of 1920x1050.
//!
//! Window ids are in the 38_000s. The rules of the file use names of their own, since the rules
//! of a test are still there in the next one.

use color_eyre::eyre;
use komorebi::DEFAULT_CONTAINER_PADDING;
use komorebi::DEFAULT_WORKSPACE_PADDING;
use komorebi::backend::fake::FakeBackend;
use komorebi::backend::with_backend;
use komorebi::core::CrossBoundaryBehaviour;
use komorebi::core::DefaultLayout;
use komorebi::core::FloatingLayerBehaviour;
use komorebi::core::Layout;
use komorebi::core::MonocleFocusBehaviour;
use komorebi::core::MoveBehaviour;
use komorebi::core::OperationBehaviour;
use komorebi::core::Placement;
use komorebi::core::Rect;
use komorebi::core::SocketMessage;
use komorebi::core::WindowContainerBehaviour;
use komorebi::static_config::StaticConfig;
use komorebi::static_config::StaticConfigExt;
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

/// A desktop and the window manager loaded from a `komorebi.json`, which is kept for as long as the
/// test holds its turn
struct Loaded {
    fake: Arc<FakeBackend>,
    wm: Arc<Mutex<WindowManager>>,
    _turn: MutexGuard<'static, ()>,
}

/// A unix socket path, which is limited to ~104 bytes on macOS, and which the temp dir alone can
/// exceed
fn short_path(suffix: &str) -> PathBuf {
    static NEXT: AtomicU32 = AtomicU32::new(0);
    PathBuf::from(format!(
        "/tmp/kmc-{}-{}.{suffix}",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::SeqCst)
    ))
}

fn write_config(json: &str) -> PathBuf {
    let path = short_path("json");
    std::fs::write(&path, json).unwrap();
    path
}

fn added(id: u32) -> WindowManagerEvent {
    WindowManagerEvent::Show(
        SystemNotification::KWin(KWinNotification::WindowAdded),
        Window::from(id),
    )
}

/// Loads `json` over a fake desktop with one monitor, as `main` does
fn load(json: &str) -> Loaded {
    let turn = TURN.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
    DEFAULT_WORKSPACE_PADDING.store(5, Ordering::SeqCst);
    DEFAULT_CONTAINER_PADDING.store(5, Ordering::SeqCst);

    let fake = Arc::new(FakeBackend::with_one_monitor());
    let path = write_config(json);
    let (_sender, receiver) = crossbeam_channel::bounded(1);
    let socket = short_path("sock");
    let listener = UnixListener::bind(&socket).unwrap();

    let wm = with_backend(fake.clone(), || -> eyre::Result<WindowManager> {
        let mut wm = StaticConfig::preload(&path, receiver, Some(listener))?;
        wm.init()?;
        Ok(wm)
    })
    .unwrap();
    let wm = Arc::new(Mutex::new(wm));
    with_backend(fake.clone(), || StaticConfig::postload(&path, &wm)).unwrap();
    with_backend(fake.clone(), || wm.lock().retile_all(false)).unwrap();

    let _ = std::fs::remove_file(&socket);

    Loaded {
        fake,
        wm,
        _turn: turn,
    }
}

impl Loaded {
    fn open(&self, id: u32, exe: &str, class: &str) {
        self.fake
            .open_window(id, exe, class, &format!("window {id}"));
        with_backend(self.fake.clone(), || {
            self.wm.lock().process_event(added(id))
        })
        .unwrap();
    }

    fn open_terminal(&self, id: u32) {
        self.open(id, "foot", "foot");
    }

    fn send(&self, message: SocketMessage) {
        with_backend(self.fake.clone(), || {
            self.wm.lock().process_command(message, Vec::new())
        })
        .unwrap();
    }

    fn rect(&self, id: u32) -> Rect {
        self.fake.rect_of(id).unwrap()
    }

    /// The windows of each container of the focused workspace
    fn containers(&self) -> Vec<Vec<u32>> {
        let wm = self.wm.lock();
        let workspace = wm.focused_workspace().unwrap();
        workspace
            .containers()
            .iter()
            .map(|container| container.windows().iter().map(|window| window.id).collect())
            .collect()
    }

    /// The floating windows of the focused workspace
    fn floating(&self) -> Vec<u32> {
        let wm = self.wm.lock();
        let workspace = wm.focused_workspace().unwrap();
        workspace
            .floating_windows()
            .iter()
            .map(|window| window.id)
            .collect()
    }

    /// The names of the workspaces of the first monitor
    fn workspace_names(&self) -> Vec<Option<String>> {
        self.wm.lock().monitors()[0]
            .workspaces()
            .iter()
            .map(|workspace| workspace.name.clone())
            .collect()
    }
}

fn rect(left: i32, top: i32, right: i32, bottom: i32) -> Rect {
    Rect {
        left,
        top,
        right,
        bottom,
    }
}

#[test]
fn a_configuration_that_says_nothing_leaves_the_defaults() {
    let config = load("{}");
    let wm = config.wm.lock();

    assert_eq!(wm.resize_delta, 50);
    assert!(wm.mouse_follows_focus);
    assert_eq!(wm.cross_monitor_move_behaviour, MoveBehaviour::Swap);
    assert_eq!(wm.cross_boundary_behaviour, CrossBoundaryBehaviour::Monitor);
    assert_eq!(
        wm.unmanaged_window_operation_behaviour,
        OperationBehaviour::Op
    );
    assert_eq!(wm.work_area_offset, None);
    assert_eq!(
        wm.window_management_behaviour.toggle_float_placement,
        Placement::CenterAndResize
    );
    assert_eq!(
        wm.window_management_behaviour.float_rule_placement,
        Placement::None
    );
    assert_eq!(
        wm.window_management_behaviour.current_behaviour,
        WindowContainerBehaviour::Create
    );
    assert!(!wm.window_management_behaviour.float_override);
    assert_eq!(
        wm.window_management_behaviour.float_override_placement,
        Placement::None
    );
    assert_eq!(
        wm.window_management_behaviour.floating_layer_behaviour,
        FloatingLayerBehaviour::Tile
    );
    assert_eq!(
        wm.window_management_behaviour.floating_layer_placement,
        Placement::Center
    );
    assert_eq!(wm.monocle_focus_behaviour, MonocleFocusBehaviour::NoOp);
    // the monitor has the workspace it started with, which no one named
    assert_eq!(wm.monitors()[0].workspaces().len(), 1);
    assert_eq!(wm.monitors()[0].workspaces()[0].name, None);
}

#[test]
fn the_settings_of_the_configuration_are_the_window_managers() {
    let config = load(
        r#"{
            "resize_delta": 100,
            "mouse_follows_focus": false,
            "cross_monitor_move_behaviour": "Insert",
            "cross_boundary_behaviour": "Workspace",
            "unmanaged_window_operation_behaviour": "NoOp",
            "toggle_float_placement": "None",
            "float_rule_placement": "Center",
            "window_container_behaviour": "Append",
            "float_override": true,
            "float_override_placement": "CenterAndResize",
            "floating_layer_behaviour": "Float",
            "floating_layer_placement": "None",
            "monocle_focus_behaviour": "Cycle",
            "global_work_area_offset": { "left": 10, "top": 20, "right": 30, "bottom": 40 }
        }"#,
    );
    let wm = config.wm.lock();

    assert_eq!(wm.resize_delta, 100);
    assert!(!wm.mouse_follows_focus);
    assert_eq!(wm.cross_monitor_move_behaviour, MoveBehaviour::Insert);
    assert_eq!(
        wm.cross_boundary_behaviour,
        CrossBoundaryBehaviour::Workspace
    );
    assert_eq!(
        wm.unmanaged_window_operation_behaviour,
        OperationBehaviour::NoOp
    );
    assert_eq!(
        wm.window_management_behaviour.toggle_float_placement,
        Placement::None
    );
    assert_eq!(
        wm.window_management_behaviour.float_rule_placement,
        Placement::Center
    );
    assert_eq!(
        wm.window_management_behaviour.current_behaviour,
        WindowContainerBehaviour::Append
    );
    assert!(wm.window_management_behaviour.float_override);
    assert_eq!(
        wm.window_management_behaviour.float_override_placement,
        Placement::CenterAndResize
    );
    assert_eq!(
        wm.window_management_behaviour.floating_layer_behaviour,
        FloatingLayerBehaviour::Float
    );
    assert_eq!(
        wm.window_management_behaviour.floating_layer_placement,
        Placement::None
    );
    assert_eq!(wm.monocle_focus_behaviour, MonocleFocusBehaviour::Cycle);
    assert_eq!(wm.work_area_offset, Some(rect(10, 20, 30, 40)));
}

#[test]
fn with_append_a_window_that_opens_joins_the_focused_container() {
    let config = load(r#"{ "window_container_behaviour": "Append" }"#);

    config.open_terminal(38_091);
    config.open_terminal(38_092);

    // the first window has no container to join, the second joins it
    let containers = config.containers();
    assert_eq!(containers.len(), 1);
    assert_eq!(containers[0].len(), 2);
}

#[test]
fn the_behaviour_of_a_workspace_comes_before_the_one_for_every_workspace() {
    let config = load(
        r#"{
            "window_container_behaviour": "Append",
            "monitors": [
                {
                    "workspaces": [
                        { "name": "stack", "layout": "BSP" },
                        { "name": "tiles", "layout": "BSP", "window_container_behaviour": "Create" }
                    ]
                }
            ]
        }"#,
    );

    {
        let wm = config.wm.lock();
        let workspaces = wm.monitors()[0].workspaces();
        assert_eq!(workspaces[0].window_container_behaviour, None);
        assert_eq!(
            workspaces[1].window_container_behaviour,
            Some(WindowContainerBehaviour::Create)
        );
    }

    config.open_terminal(38_101);
    config.open_terminal(38_102);
    assert_eq!(config.containers().len(), 1, "the first workspace appends");

    config.send(SocketMessage::FocusWorkspaceNumber(1));
    config.open_terminal(38_103);
    config.open_terminal(38_104);
    assert_eq!(config.containers().len(), 2, "the second creates");
}

#[test]
fn window_container_behaviour_rules_change_the_behaviour_with_the_number_of_containers() {
    let config = load(
        r#"{
            "monitors": [
                {
                    "workspaces": [
                        {
                            "name": "chat",
                            "layout": "BSP",
                            "window_container_behaviour_rules": { "2": "Append" }
                        }
                    ]
                }
            ]
        }"#,
    );

    config.open_terminal(38_111);
    config.open_terminal(38_112);
    assert_eq!(config.containers().len(), 2, "below the threshold: create");

    config.open_terminal(38_113);
    let containers = config.containers();
    assert_eq!(containers.len(), 2, "from the threshold: append");
    assert_eq!(containers.iter().map(Vec::len).sum::<usize>(), 3);
}

#[test]
fn with_the_float_override_a_window_that_opens_floats_where_the_placement_says() {
    let config = load(r#"{ "float_override": true, "float_override_placement": "Center" }"#);

    config.open_terminal(38_121);

    assert_eq!(config.floating(), [38_121]);
    assert!(config.containers().is_empty());
    // the window keeps its 800x600 and is centered in the work area, 1920x1050
    let window = config.rect(38_121);
    assert_eq!((window.right, window.bottom), (800, 600));
    assert!((2 * window.left + window.right - 1920).abs() <= 1);
    assert!((2 * window.top + window.bottom - 1050).abs() <= 1);
}

#[test]
fn the_float_override_of_a_workspace_comes_before_the_one_for_every_workspace() {
    let config = load(
        r#"{
            "float_override": true,
            "monitors": [
                { "workspaces": [ { "name": "tiles", "layout": "BSP", "float_override": false } ] }
            ]
        }"#,
    );

    config.open_terminal(38_131);

    assert_eq!(config.containers(), [vec![38_131]]);
    assert!(config.floating().is_empty());
}

#[test]
fn the_floating_layer_behaviour_floats_what_opens_while_a_workspace_is_on_its_floating_layer() {
    let config =
        load(r#"{ "floating_layer_behaviour": "Float", "floating_layer_placement": "Center" }"#);
    config.open_terminal(38_141);
    config.open_terminal(38_142);
    assert_eq!(
        config.containers().len(),
        2,
        "on the tiling layer they tile"
    );

    config.send(SocketMessage::ToggleFloat);
    assert_eq!(config.floating(), [38_142]);
    config.open_terminal(38_143);

    assert_eq!(config.floating(), [38_142, 38_143]);
    assert_eq!(config.containers(), [vec![38_141]]);
}

#[test]
fn the_floating_layer_behaviour_of_a_workspace_comes_before_a_monitors_and_that_before_the_default()
{
    let config = load(
        r#"{
            "floating_layer_behaviour": "Float",
            "monitors": [
                {
                    "floating_layer_behaviour": "Tile",
                    "workspaces": [
                        { "name": "monitors", "layout": "BSP" },
                        { "name": "own", "layout": "BSP", "floating_layer_behaviour": "Float" }
                    ]
                }
            ]
        }"#,
    );

    let wm = config.wm.lock();
    assert_eq!(
        wm.window_management_behaviour(0, 0)
            .floating_layer_behaviour,
        FloatingLayerBehaviour::Tile,
        "the monitor's, over the one for every monitor"
    );
    assert_eq!(
        wm.window_management_behaviour(0, 1)
            .floating_layer_behaviour,
        FloatingLayerBehaviour::Float,
        "the workspace's, over the monitor's"
    );
}

#[test]
fn the_default_paddings_and_the_offset_decide_where_a_tile_is() {
    let config = load(
        r#"{
            "default_workspace_padding": 20,
            "default_container_padding": 10,
            "global_work_area_offset": { "left": 100, "top": 0, "right": 200, "bottom": 0 }
        }"#,
    );

    config.open_terminal(38_001);

    // the offset moves the left edge of the work area 100 to the right and makes it 200 narrower;
    // the workspace padding and the container padding, 20 and 10, are between it and the window
    assert_eq!(config.rect(38_001), rect(130, 30, 1660, 990));
}

#[test]
fn a_monitor_and_a_workspace_can_have_a_padding_and_an_offset_of_their_own() {
    let config = load(
        r#"{
            "default_workspace_padding": 20,
            "default_container_padding": 10,
            "monitors": [
                {
                    "container_padding": 0,
                    "workspace_padding": 0,
                    "work_area_offset": { "left": 50, "top": 0, "right": 50, "bottom": 0 },
                    "workspaces": [
                        { "name": "plain", "layout": "BSP" },
                        {
                            "name": "padded",
                            "layout": "BSP",
                            "workspace_padding": 30,
                            "container_padding": 0,
                            "work_area_offset": { "left": 0, "top": 10, "right": 0, "bottom": 10 }
                        }
                    ]
                }
            ]
        }"#,
    );

    // the monitor's offset and no padding at all
    config.open_terminal(38_011);
    assert_eq!(config.rect(38_011), rect(50, 0, 1870, 1050));

    // the workspace's own offset and padding come before the monitor's
    config.send(SocketMessage::FocusWorkspaceNumber(1));
    config.open_terminal(38_012);
    assert_eq!(config.rect(38_012), rect(30, 40, 1860, 980));
}

#[test]
fn workspaces_are_named_and_laid_out_as_the_configuration_says() {
    let config = load(
        r#"{
            "monitors": [
                {
                    "workspaces": [
                        { "name": "web", "layout": "BSP" },
                        { "name": "code", "layout": "Columns" },
                        { "name": "chat", "layout": "Rows", "tile": true }
                    ]
                }
            ]
        }"#,
    );

    assert_eq!(
        config.workspace_names(),
        [
            Some(String::from("web")),
            Some(String::from("code")),
            Some(String::from("chat"))
        ]
    );
    let wm = config.wm.lock();
    let layouts: Vec<_> = wm.monitors()[0]
        .workspaces()
        .iter()
        .map(|workspace| workspace.layout.clone())
        .collect();
    assert_eq!(
        layouts,
        [
            Layout::Default(DefaultLayout::BSP),
            Layout::Default(DefaultLayout::Columns),
            Layout::Default(DefaultLayout::Rows),
        ]
    );
}

#[test]
fn a_workspace_with_no_layout_does_not_tile() {
    let config = load(
        r#"{
            "monitors": [
                {
                    "workspaces": [
                        { "name": "free" },
                        { "name": "tiled", "tile": true },
                        { "name": "off", "layout": "BSP", "tile": false }
                    ]
                }
            ]
        }"#,
    );

    // the first workspace was given no layout and does not say it tiles: its window is left where
    // it opened (800x600 at 100,100)
    config.open_terminal(38_021);
    assert_eq!(config.rect(38_021), rect(100, 100, 800, 600));

    config.send(SocketMessage::FocusWorkspaceNumber(1));
    config.open_terminal(38_022);
    assert_eq!(config.rect(38_022), rect(10, 10, 1900, 1030));

    config.send(SocketMessage::FocusWorkspaceNumber(2));
    config.open_terminal(38_023);
    assert_eq!(config.rect(38_023), rect(100, 100, 800, 600));
}

#[test]
fn layout_rules_change_the_layout_with_the_number_of_windows() {
    let config = load(
        r#"{
            "default_workspace_padding": 0,
            "default_container_padding": 0,
            "monitors": [
                {
                    "workspaces": [
                        {
                            "name": "grow",
                            "layout": "BSP",
                            "layout_rules": { "3": "Columns", "2": "Rows" }
                        }
                    ]
                }
            ]
        }"#,
    );

    config.open_terminal(38_031);
    assert_eq!(config.rect(38_031), rect(0, 0, 1920, 1050));

    // two windows: rows, one above the other
    config.open_terminal(38_032);
    assert_eq!(config.rect(38_031), rect(0, 0, 1920, 525));
    assert_eq!(config.rect(38_032), rect(0, 525, 1920, 525));

    // three: columns, side by side
    config.open_terminal(38_033);
    assert_eq!(config.rect(38_031), rect(0, 0, 640, 1050));
    assert_eq!(config.rect(38_032), rect(640, 0, 640, 1050));
    assert_eq!(config.rect(38_033), rect(1280, 0, 640, 1050));

    // the rules are the workspace's, in the order of their thresholds
    let wm = config.wm.lock();
    assert_eq!(
        wm.monitors()[0].workspaces()[0].layout_rules,
        [
            (2, Layout::Default(DefaultLayout::Rows)),
            (3, Layout::Default(DefaultLayout::Columns))
        ]
    );
}

#[test]
fn layout_rules_can_be_added_and_cleared_while_the_daemon_runs() {
    let config = load(
        r#"{
            "default_workspace_padding": 0,
            "default_container_padding": 0,
            "monitors": [ { "workspaces": [ { "name": "rules", "layout": "BSP" } ] } ]
        }"#,
    );
    config.open_terminal(38_041);
    config.open_terminal(38_042);
    // two windows in BSP: side by side
    assert_eq!(config.rect(38_041), rect(0, 0, 960, 1050));

    config.send(SocketMessage::NamedWorkspaceLayoutRule(
        String::from("rules"),
        2,
        DefaultLayout::Rows,
    ));
    assert_eq!(config.rect(38_041), rect(0, 0, 1920, 525));

    config.send(SocketMessage::ClearNamedWorkspaceLayoutRules(String::from(
        "rules",
    )));
    // the layout the rule gave stays: clearing takes the rules away and does not undo them
    assert!(
        config.wm.lock().monitors()[0].workspaces()[0]
            .layout_rules
            .is_empty()
    );
    assert_eq!(config.rect(38_041), rect(0, 0, 1920, 525));

    config.send(SocketMessage::WorkspaceLayoutRule(
        0,
        0,
        2,
        DefaultLayout::Columns,
    ));
    assert_eq!(config.rect(38_041), rect(0, 0, 960, 1050));
    config.send(SocketMessage::ClearWorkspaceLayoutRules(0, 0));
    assert!(
        config.wm.lock().monitors()[0].workspaces()[0]
            .layout_rules
            .is_empty()
    );
}

#[test]
fn work_area_offset_rules_change_the_offset_with_the_number_of_windows() {
    let config = load(
        r#"{
            "default_workspace_padding": 0,
            "default_container_padding": 0,
            "monitors": [
                {
                    "workspaces": [
                        {
                            "name": "margins",
                            "layout": "Columns",
                            "work_area_offset_rules": {
                                "2": { "left": 100, "top": 0, "right": 100, "bottom": 0 }
                            }
                        }
                    ]
                }
            ]
        }"#,
    );

    config.open_terminal(38_051);
    assert_eq!(config.rect(38_051), rect(0, 0, 1920, 1050));

    config.open_terminal(38_052);
    assert_eq!(config.rect(38_051), rect(100, 0, 910, 1050));
    assert_eq!(config.rect(38_052), rect(1010, 0, 910, 1050));
}

#[test]
fn layout_options_of_a_workspace_replace_the_defaults_of_its_layout() {
    let config = load(
        r#"{
            "default_workspace_padding": 0,
            "default_container_padding": 0,
            "layout_defaults": {
                "Columns": { "layout_options": { "column_ratios": [0.75] } }
            },
            "monitors": [
                {
                    "workspaces": [
                        { "name": "default", "layout": "Columns" },
                        {
                            "name": "own",
                            "layout": "Columns",
                            "layout_options": { "column_ratios": [0.25] }
                        },
                        {
                            "name": "rules",
                            "layout": "Columns",
                            "layout_options_rules": {
                                "3": { "column_ratios": [0.5] }
                            }
                        },
                        { "name": "other", "layout": "Rows" }
                    ]
                }
            ]
        }"#,
    );

    // layout_defaults: the first of two columns has three quarters of the work area
    config.open_terminal(38_061);
    config.open_terminal(38_062);
    assert_eq!(config.rect(38_061), rect(0, 0, 1440, 1050));

    // a workspace that has options of its own does not have the defaults, even if it has less
    config.send(SocketMessage::FocusWorkspaceNumber(1));
    config.open_terminal(38_063);
    config.open_terminal(38_064);
    assert_eq!(config.rect(38_063), rect(0, 0, 480, 1050));

    // a workspace with rules and no options of its own does not have the defaults either: the
    // columns are even until there are three
    config.send(SocketMessage::FocusWorkspaceNumber(2));
    config.open_terminal(38_065);
    config.open_terminal(38_066);
    assert_eq!(config.rect(38_065), rect(0, 0, 960, 1050));
    config.open_terminal(38_067);
    assert_eq!(config.rect(38_065), rect(0, 0, 960, 1050));

    // the defaults are of one layout: the workspace with another layout has none
    config.send(SocketMessage::FocusWorkspaceNumber(3));
    config.open_terminal(38_068);
    config.open_terminal(38_069);
    assert_eq!(config.rect(38_068), rect(0, 0, 1920, 525));
}

#[test]
fn the_rules_of_the_configuration_decide_which_windows_are_tiled() {
    let config = load(
        r#"{
            "ignore_rules": [
                { "kind": "Exe", "id": "config-ignored", "matching_strategy": "Equals" }
            ],
            "floating_applications": [
                { "kind": "Class", "id": "config-floated", "matching_strategy": "Equals" }
            ],
            "manage_rules": [
                { "kind": "Exe", "id": "config-managed", "matching_strategy": "Equals" },
                { "kind": "Exe", "id": "config-ignored-but", "matching_strategy": "Equals" }
            ]
        }"#,
    );

    config.open(38_071, "config-ignored", "ignored");
    config.open(38_072, "foot", "config-floated");
    config.open(38_073, "foot", "foot");

    let wm = config.wm.lock();
    let workspace = &wm.monitors()[0].workspaces()[0];
    // the ignored window is not in the workspace and stays where it opened
    assert!(!workspace.contains_window(38_071));
    assert_eq!(config.rect(38_071), rect(100, 100, 800, 600));
    // the window that matches a float rule is the workspace's floating window
    assert!(
        !workspace
            .containers()
            .iter()
            .any(|c| c.contains_window(38_072))
    );
    assert!(workspace.floating_windows().iter().any(|w| w.id == 38_072));
    // the other is tiled
    assert!(
        workspace
            .containers()
            .iter()
            .any(|c| c.contains_window(38_073))
    );
}

#[test]
fn a_configuration_comes_out_again_as_the_settings_of_the_window_manager() {
    let config = load(
        r#"{
            "resize_delta": 100,
            "cross_monitor_move_behaviour": "Insert",
            "window_container_behaviour": "Append",
            "float_override": true,
            "float_override_placement": "Center",
            "floating_layer_behaviour": "Float",
            "floating_layer_placement": "None",
            "monocle_focus_behaviour": "Cycle",
            "default_workspace_padding": 20,
            "default_container_padding": 10,
            "monitors": [
                {
                    "floating_layer_behaviour": "Tile",
                    "workspaces": [
                        { "name": "web", "layout": "BSP", "layout_rules": { "2": "Rows" } },
                        {
                            "name": "code",
                            "layout": "Columns",
                            "workspace_padding": 0,
                            "window_container_behaviour": "Create",
                            "window_container_behaviour_rules": { "3": "Append" },
                            "float_override": false,
                            "floating_layer_behaviour": "Float"
                        },
                        { "name": "chat" }
                    ]
                }
            ]
        }"#,
    );

    let generated = StaticConfig::from(&*config.wm.lock());

    assert_eq!(generated.resize_delta, Some(100));
    assert_eq!(
        generated.cross_monitor_move_behaviour,
        Some(MoveBehaviour::Insert)
    );
    assert_eq!(generated.default_workspace_padding, Some(20));
    assert_eq!(generated.default_container_padding, Some(10));
    assert_eq!(
        generated.window_container_behaviour,
        Some(WindowContainerBehaviour::Append)
    );
    assert_eq!(generated.float_override, Some(true));
    assert_eq!(generated.float_override_placement, Some(Placement::Center));
    assert_eq!(
        generated.floating_layer_behaviour,
        Some(FloatingLayerBehaviour::Float)
    );
    assert_eq!(generated.floating_layer_placement, Some(Placement::None));
    assert_eq!(
        generated.monocle_focus_behaviour,
        Some(MonocleFocusBehaviour::Cycle)
    );

    let monitor = &generated.monitors.as_ref().unwrap()[0];
    assert_eq!(
        monitor.floating_layer_behaviour,
        Some(FloatingLayerBehaviour::Tile)
    );
    let workspaces = &monitor.workspaces;
    assert_eq!(workspaces.len(), 3);
    assert_eq!(workspaces[0].name, "web");
    assert_eq!(workspaces[0].layout, Some(DefaultLayout::BSP));
    assert_eq!(
        workspaces[0].layout_rules,
        Some([(2, DefaultLayout::Rows)].into_iter().collect())
    );
    // a padding that is the default is left out, one that is not is kept
    assert_eq!(workspaces[0].workspace_padding, None);
    assert_eq!(workspaces[1].workspace_padding, Some(0));
    // a workspace that does not tile has no layout in it
    assert_eq!(workspaces[2].layout, None);
    assert_eq!(workspaces[2].tile, Some(false));
    assert_eq!(
        workspaces[1].window_container_behaviour,
        Some(WindowContainerBehaviour::Create)
    );
    assert_eq!(
        workspaces[1].window_container_behaviour_rules,
        Some(
            [(3, WindowContainerBehaviour::Append)]
                .into_iter()
                .collect()
        )
    );
    assert_eq!(workspaces[1].float_override, Some(false));
    assert_eq!(
        workspaces[1].floating_layer_behaviour,
        Some(FloatingLayerBehaviour::Float)
    );
    assert_eq!(workspaces[0].window_container_behaviour_rules, None);

    // what is generated is a configuration that loads, and says the same again
    let json = serde_json::to_string_pretty(&generated).unwrap();
    assert_eq!(StaticConfig::read_raw(&json).unwrap(), generated);
}

#[test]
fn a_changed_configuration_is_applied_to_the_windows_that_are_there() {
    let config = load(
        r#"{
            "resize_delta": 100,
            "window_container_behaviour": "Append",
            "floating_layer_behaviour": "Float",
            "monocle_focus_behaviour": "Cycle",
            "monitors": [ { "workspaces": [ { "name": "one", "layout": "BSP" } ] } ]
        }"#,
    );
    config.open_terminal(38_081);
    assert_eq!(config.rect(38_081), rect(10, 10, 1900, 1030));

    let changed = write_config(
        r#"{
            "resize_delta": 30,
            "default_workspace_padding": 0,
            "default_container_padding": 0,
            "mouse_follows_focus": false,
            "monitors": [
                { "workspaces": [ { "name": "uno", "layout": "BSP" }, { "name": "dos" } ] }
            ]
        }"#,
    );
    config.send(SocketMessage::ReloadStaticConfiguration(changed));

    {
        let wm = config.wm.lock();
        assert_eq!(wm.resize_delta, 30);
        assert!(!wm.mouse_follows_focus);
        // a key the file leaves out is the default of its setting
        assert_eq!(
            wm.window_management_behaviour.current_behaviour,
            WindowContainerBehaviour::Create
        );
        assert_eq!(
            wm.window_management_behaviour.floating_layer_behaviour,
            FloatingLayerBehaviour::Tile
        );
        assert_eq!(wm.monocle_focus_behaviour, MonocleFocusBehaviour::NoOp);
    }
    assert_eq!(
        config.workspace_names(),
        [Some(String::from("uno")), Some(String::from("dos"))]
    );
    // the window stays in the workspace, and is tiled again with the new padding
    assert_eq!(config.rect(38_081), rect(0, 0, 1920, 1050));
}

#[test]
fn a_configuration_that_cannot_be_read_changes_nothing() {
    let config = load(r#"{ "resize_delta": 100 }"#);
    let broken = write_config(r#"{ "resize_delta": "wide" "#);
    let missing = short_path("json");

    for path in [broken, missing] {
        let result = with_backend(config.fake.clone(), || {
            config
                .wm
                .lock()
                .reload_static_configuration(&path)
                .map_err(|error| error.to_string())
        });
        assert!(result.is_err(), "{}", path.display());
    }

    assert_eq!(config.wm.lock().resize_delta, 100);
}

#[test]
fn a_rule_that_is_not_a_regular_expression_is_an_error_at_load() {
    let _turn = TURN.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
    let path = write_config(
        r#"{ "ignore_rules": [ { "kind": "Exe", "id": "(unclosed", "matching_strategy": "Regex" } ] }"#,
    );
    let (_sender, receiver) = crossbeam_channel::bounded(1);
    let socket = short_path("sock");

    let result = StaticConfig::preload(&path, receiver, Some(UnixListener::bind(&socket).unwrap()));

    let _ = std::fs::remove_file(&socket);
    assert!(result.is_err());
}
