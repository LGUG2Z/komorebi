//! The workspace commands beyond showing a workspace and moving a window to one by its index
//! (`workspaces.rs`), against an in-memory desktop: naming a workspace and finding it by its name,
//! cycling through the workspaces, going back to the last one, adding and closing workspaces, and
//! setting the layout and the tiling of a workspace that need not be in front. The messages go in
//! through [`WindowManager::process_command`]; what comes out is the workspaces of the window
//! manager and what the compositor was asked to show.
//!
//! Window ids are in the 31_000s.

use komorebi::backend::fake::Command;
use komorebi::backend::fake::FakeBackend;
use komorebi::backend::with_backend;
use komorebi::core::CycleDirection;
use komorebi::core::DefaultLayout;
use komorebi::core::Layout;
use komorebi::core::Rect;
use komorebi::core::SocketMessage;
use komorebi::core::StateQuery;
use komorebi::window::Window;
use komorebi::window_manager::WindowManager;
use komorebi::window_manager_event::KWinNotification;
use komorebi::window_manager_event::SystemNotification;
use komorebi::window_manager_event::WindowManagerEvent;
use std::sync::Arc;

const MONITOR: u32 = 1;

fn added(id: u32) -> WindowManagerEvent {
    WindowManagerEvent::Show(
        SystemNotification::KWin(KWinNotification::WindowAdded),
        Window::from(id),
    )
}

/// A window manager over `fake`, with a window for each of `ids`, opened in that order
fn desktop_on(fake: FakeBackend, ids: &[u32]) -> (Arc<FakeBackend>, WindowManager) {
    let fake = Arc::new(fake);
    let (_sender, receiver) = crossbeam_channel::bounded(1);
    // unix socket paths are limited to ~104 bytes on macOS, which the temp dir alone can exceed
    static NEXT: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);
    let socket = std::path::PathBuf::from(format!(
        "/tmp/kmw-{}-{}.sock",
        std::process::id(),
        NEXT.fetch_add(1, std::sync::atomic::Ordering::SeqCst)
    ));

    let mut wm = with_backend(fake.clone(), || {
        let mut wm = WindowManager::new(receiver, Some(socket)).unwrap();
        wm.init().unwrap();
        wm
    });
    for &id in ids {
        fake.open_window(id, "foot", "foot", &format!("terminal {id}"));
        with_backend(fake.clone(), || wm.process_event(added(id))).unwrap();
    }

    (fake, wm)
}

/// A window manager over a fake desktop with one 1920x1080 monitor
fn desktop(ids: &[u32]) -> (Arc<FakeBackend>, WindowManager) {
    desktop_on(FakeBackend::with_one_monitor(), ids)
}

fn send(
    fake: &Arc<FakeBackend>,
    wm: &mut WindowManager,
    message: SocketMessage,
) -> color_eyre::Result<()> {
    with_backend(fake.clone(), || wm.process_command(message, Vec::new()))
}

fn query(fake: &Arc<FakeBackend>, wm: &mut WindowManager, query: StateQuery) -> String {
    let mut answer = Vec::new();
    with_backend(fake.clone(), || {
        wm.process_command(SocketMessage::Query(query), &mut answer)
    })
    .unwrap();
    String::from_utf8(answer).unwrap()
}

/// The ids of the windows of each workspace of the focused monitor
fn windows(wm: &WindowManager) -> Vec<Vec<u32>> {
    wm.focused_monitor()
        .unwrap()
        .workspaces()
        .iter()
        .map(|workspace| {
            workspace
                .containers()
                .iter()
                .flat_map(|container| container.windows().iter().map(|window| window.id))
                .collect()
        })
        .collect()
}

fn names(wm: &WindowManager) -> Vec<Option<String>> {
    wm.focused_monitor()
        .unwrap()
        .workspaces()
        .iter()
        .map(|workspace| workspace.name.clone())
        .collect()
}

#[test]
fn a_named_workspace_is_found_by_its_name_and_shown() {
    let (fake, mut wm) = desktop(&[31_001]);

    assert!(
        send(
            &fake,
            &mut wm,
            SocketMessage::WorkspaceName(0, 2, String::from("mail")),
        )
        .is_err(),
        "there is no workspace 2 to name yet"
    );
    send(&fake, &mut wm, SocketMessage::EnsureWorkspaces(0, 3)).unwrap();
    send(
        &fake,
        &mut wm,
        SocketMessage::WorkspaceName(0, 2, String::from("mail")),
    )
    .unwrap();
    assert_eq!(query(&fake, &mut wm, StateQuery::FocusedWorkspaceName), "0");

    fake.clear_commands();
    send(
        &fake,
        &mut wm,
        SocketMessage::FocusNamedWorkspace(String::from("mail")),
    )
    .unwrap();

    assert_eq!(wm.focused_workspace_idx().unwrap(), 2);
    assert_eq!(fake.shown_workspace(MONITOR), 2);
    assert!(
        fake.commands()
            .contains(&Command::SwitchWorkspace(MONITOR, 2)),
        "{:?}",
        fake.commands()
    );
    assert_eq!(
        query(&fake, &mut wm, StateQuery::FocusedWorkspaceName),
        "mail"
    );
}

#[test]
fn a_name_no_workspace_has_changes_nothing() {
    let (fake, mut wm) = desktop(&[31_011]);
    send(&fake, &mut wm, SocketMessage::EnsureWorkspaces(0, 2)).unwrap();
    fake.clear_commands();

    send(
        &fake,
        &mut wm,
        SocketMessage::FocusNamedWorkspace(String::from("nowhere")),
    )
    .unwrap();
    send(
        &fake,
        &mut wm,
        SocketMessage::NamedWorkspaceTiling(String::from("nowhere"), false),
    )
    .unwrap();

    assert_eq!(wm.focused_workspace_idx().unwrap(), 0);
    assert!(fake.commands().is_empty(), "{:?}", fake.commands());
}

#[test]
fn ensure_named_workspaces_gives_a_monitor_workspaces_with_those_names() {
    let (fake, mut wm) = desktop(&[]);

    send(
        &fake,
        &mut wm,
        SocketMessage::EnsureNamedWorkspaces(0, vec![String::from("web"), String::from("code")]),
    )
    .unwrap();

    assert_eq!(
        names(&wm),
        [Some(String::from("web")), Some(String::from("code"))]
    );
}

#[test]
fn cycle_workspace_goes_round_the_workspaces_of_the_monitor() {
    let (fake, mut wm) = desktop(&[31_021]);
    send(&fake, &mut wm, SocketMessage::EnsureWorkspaces(0, 3)).unwrap();
    let mut shown = vec![];

    for direction in [
        CycleDirection::Next,
        CycleDirection::Next,
        CycleDirection::Next,
        CycleDirection::Previous,
    ] {
        send(
            &fake,
            &mut wm,
            SocketMessage::CycleFocusWorkspace(direction),
        )
        .unwrap();
        shown.push((
            wm.focused_workspace_idx().unwrap(),
            fake.shown_workspace(MONITOR),
        ));
    }

    assert_eq!(shown, [(1, 1), (2, 2), (0, 0), (2, 2)]);
}

#[test]
fn cycle_empty_workspace_goes_to_the_next_workspace_without_windows() {
    let (fake, mut wm) = desktop(&[31_031, 31_032]);
    send(&fake, &mut wm, SocketMessage::EnsureWorkspaces(0, 3)).unwrap();
    // the focused window (31_032) on workspace 1: workspace 2 is the only empty one
    send(
        &fake,
        &mut wm,
        SocketMessage::SendContainerToWorkspaceNumber(1),
    )
    .unwrap();

    send(
        &fake,
        &mut wm,
        SocketMessage::CycleFocusEmptyWorkspace(CycleDirection::Next),
    )
    .unwrap();

    assert_eq!(wm.focused_workspace_idx().unwrap(), 2);
    assert_eq!(windows(&wm), [vec![31_031], vec![31_032], vec![]]);
}

#[test]
fn focus_last_workspace_goes_back_and_forth_between_the_last_two() {
    let (fake, mut wm) = desktop(&[31_041]);
    send(&fake, &mut wm, SocketMessage::FocusWorkspaceNumber(2)).unwrap();
    let mut shown = vec![];

    for _ in 0..3 {
        send(&fake, &mut wm, SocketMessage::FocusLastWorkspace).unwrap();
        shown.push(fake.shown_workspace(MONITOR));
    }

    assert_eq!(shown, [0, 2, 0]);
}

#[test]
fn send_to_last_workspace_takes_the_window_to_the_one_in_front_before_and_stays() {
    let (fake, mut wm) = desktop(&[31_051, 31_052]);
    send(&fake, &mut wm, SocketMessage::FocusWorkspaceNumber(1)).unwrap();
    send(&fake, &mut wm, SocketMessage::FocusWorkspaceNumber(0)).unwrap();

    send(&fake, &mut wm, SocketMessage::SendContainerToLastWorkspace).unwrap();

    assert_eq!(wm.focused_workspace_idx().unwrap(), 0);
    assert_eq!(windows(&wm), [vec![31_051], vec![31_052]]);
    assert_eq!(fake.workspace_of(31_052), Some(1));
}

#[test]
fn move_to_last_workspace_takes_the_window_to_the_one_in_front_before_and_shows_it() {
    let (fake, mut wm) = desktop(&[31_055, 31_056]);
    send(&fake, &mut wm, SocketMessage::FocusWorkspaceNumber(1)).unwrap();
    send(&fake, &mut wm, SocketMessage::FocusWorkspaceNumber(0)).unwrap();

    send(&fake, &mut wm, SocketMessage::MoveContainerToLastWorkspace).unwrap();

    assert_eq!(wm.focused_workspace_idx().unwrap(), 1);
    assert_eq!(fake.shown_workspace(MONITOR), 1);
    assert_eq!(windows(&wm), [vec![31_055], vec![31_056]]);
    assert!(fake.on_screen(31_056) && !fake.on_screen(31_055));
}

#[test]
fn cycle_send_and_cycle_move_take_the_window_to_the_next_workspace() {
    let (fake, mut wm) = desktop(&[31_061, 31_062]);
    send(&fake, &mut wm, SocketMessage::EnsureWorkspaces(0, 2)).unwrap();

    send(
        &fake,
        &mut wm,
        SocketMessage::CycleSendContainerToWorkspace(CycleDirection::Next),
    )
    .unwrap();
    assert_eq!(wm.focused_workspace_idx().unwrap(), 0);
    assert_eq!(windows(&wm), [vec![31_061], vec![31_062]]);

    // from workspace 0, the previous workspace is the last one, and the window goes with it
    send(
        &fake,
        &mut wm,
        SocketMessage::CycleMoveContainerToWorkspace(CycleDirection::Previous),
    )
    .unwrap();
    assert_eq!(wm.focused_workspace_idx().unwrap(), 1);
    assert_eq!(windows(&wm), [vec![], vec![31_062, 31_061]]);
    assert_eq!(fake.shown_workspace(MONITOR), 1);
}

#[test]
fn new_workspace_adds_one_and_close_workspace_removes_it_when_it_is_empty_and_unnamed() {
    let (fake, mut wm) = desktop(&[31_071]);

    send(&fake, &mut wm, SocketMessage::NewWorkspace).unwrap();
    assert_eq!(windows(&wm).len(), 2);
    assert_eq!(wm.focused_workspace_idx().unwrap(), 1);
    assert_eq!(fake.shown_workspace(MONITOR), 1);

    send(&fake, &mut wm, SocketMessage::CloseWorkspace).unwrap();
    assert_eq!(windows(&wm).len(), 1);
    assert_eq!(wm.focused_workspace_idx().unwrap(), 0);
    assert_eq!(fake.shown_workspace(MONITOR), 0);

    // the last workspace, a workspace with a window, and a named workspace stay
    send(&fake, &mut wm, SocketMessage::CloseWorkspace).unwrap();
    assert_eq!(windows(&wm), [vec![31_071]]);
    send(&fake, &mut wm, SocketMessage::NewWorkspace).unwrap();
    send(
        &fake,
        &mut wm,
        SocketMessage::WorkspaceName(0, 1, String::from("kept")),
    )
    .unwrap();
    send(&fake, &mut wm, SocketMessage::CloseWorkspace).unwrap();
    assert_eq!(names(&wm), [None, Some(String::from("kept"))]);
}

#[test]
fn focus_workspaces_shows_the_workspace_on_every_monitor() {
    let fake = FakeBackend::with_one_monitor();
    let second = Rect {
        left: 1920,
        top: 0,
        right: 1920,
        bottom: 1080,
    };
    fake.add_monitor(2, "Virtual-1", second, second);
    let (fake, mut wm) = desktop_on(fake, &[]);

    send(&fake, &mut wm, SocketMessage::FocusWorkspaceNumbers(1)).unwrap();

    assert_eq!((fake.shown_workspace(1), fake.shown_workspace(2)), (1, 1));
    assert!(
        wm.monitors()
            .iter()
            .all(|monitor| monitor.focused_workspace_idx() == 1)
    );
}

#[test]
fn the_layout_and_the_tiling_of_a_workspace_are_set_by_its_index_or_its_name() {
    let (fake, mut wm) = desktop(&[31_081, 31_082]);
    send(
        &fake,
        &mut wm,
        SocketMessage::EnsureNamedWorkspaces(0, vec![String::from("main"), String::from("side")]),
    )
    .unwrap();

    send(
        &fake,
        &mut wm,
        SocketMessage::WorkspaceLayout(0, 0, DefaultLayout::Rows),
    )
    .unwrap();
    let workspace = wm.focused_workspace().unwrap();
    assert_eq!(workspace.layout, Layout::Default(DefaultLayout::Rows));
    // the workspace in front is tiled again: one row above the other
    let (first, second) = (fake.rect_of(31_081).unwrap(), fake.rect_of(31_082).unwrap());
    assert_eq!(first.left, second.left);
    assert_ne!(first.top, second.top);

    send(
        &fake,
        &mut wm,
        SocketMessage::NamedWorkspaceLayout(String::from("side"), DefaultLayout::Columns),
    )
    .unwrap();
    send(
        &fake,
        &mut wm,
        SocketMessage::NamedWorkspaceTiling(String::from("main"), false),
    )
    .unwrap();
    send(&fake, &mut wm, SocketMessage::WorkspaceTiling(0, 1, false)).unwrap();

    let workspaces = wm.focused_monitor().unwrap().workspaces();
    assert_eq!(
        workspaces[1].layout,
        Layout::Default(DefaultLayout::Columns)
    );
    assert!(!workspaces[0].tile && !workspaces[1].tile);
}
