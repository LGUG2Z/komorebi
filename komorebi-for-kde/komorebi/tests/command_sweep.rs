//! A sweep of the commands that change the model, through [`WindowManager::process_command`] over
//! a fake desktop with two monitors and three windows, the way the `commands` end-to-end scenario
//! sweeps `komorebic` (`SWEEP` in `core/scenarios/commands.py`): after every command the daemon
//! must be alive, answer a state query with JSON, and still manage every window it was given.
//!
//! mirrors: commands / <command> leaves the daemon healthy (the model commands of the sweep, as
//! the `SocketMessage` each `komorebic` command is; the order is the scenario's, the ones that
//! need a window the fake cannot have (the pointer, the real compositor, files) left out)
//!
//! Not covered here, and so still end-to-end: that `komorebic` parses each command and writes
//! that message (see the CLI tests of `komorebic-core`), that the socket answers while a command
//! runs, a command that stalls the daemon, and anything the real OS does with the windows.

use komorebi::backend::fake::FakeBackend;
use komorebi::backend::with_backend;
use komorebi::core::Axis;
use komorebi::core::CycleDirection::Next;
use komorebi::core::CycleDirection::Previous;
use komorebi::core::DefaultLayout;
use komorebi::core::MoveBehaviour;
use komorebi::core::OperationBehaviour;
use komorebi::core::OperationDirection::Down;
use komorebi::core::OperationDirection::Left;
use komorebi::core::OperationDirection::Right;
use komorebi::core::OperationDirection::Up;
use komorebi::core::Rect;
use komorebi::core::Sizing::Decrease;
use komorebi::core::Sizing::Increase;
use komorebi::core::SocketMessage as M;
use komorebi::window::Window;
use komorebi::window_manager::WindowManager;
use komorebi::window_manager_event::KWinNotification;
use komorebi::window_manager_event::SystemNotification;
use komorebi::window_manager_event::WindowManagerEvent;
use std::sync::Arc;

const IDS: [u32; 3] = [31_001, 31_002, 31_003];

fn rect(left: i32) -> Rect {
    Rect {
        left,
        top: 0,
        right: 1920,
        bottom: 1080,
    }
}

fn desktop() -> (Arc<FakeBackend>, WindowManager) {
    let fake = Arc::new(FakeBackend::new());
    fake.add_monitor(50, "DP-1", rect(0), rect(0));
    fake.add_monitor(51, "DP-2", rect(1920), rect(1920));
    let (_sender, receiver) = crossbeam_channel::bounded(1);
    let socket = std::path::PathBuf::from(format!("/tmp/kcs-{}.sock", std::process::id()));
    let mut wm = with_backend(fake.clone(), || {
        let mut wm = WindowManager::new(receiver, Some(socket)).unwrap();
        wm.init().unwrap();
        wm
    });
    for id in IDS {
        fake.open_window(
            id,
            &format!("exe{id}"),
            &format!("class.{id}"),
            &format!("window {id}"),
        );
        with_backend(fake.clone(), || {
            wm.process_event(WindowManagerEvent::Show(
                SystemNotification::KWin(KWinNotification::WindowAdded),
                Window::from(id),
            ))
        })
        .unwrap();
    }
    (fake, wm)
}

fn managed(wm: &WindowManager, id: u32) -> bool {
    wm.monitors().iter().any(|monitor| {
        monitor
            .workspaces()
            .iter()
            .any(|workspace| workspace.contains_window(id))
    })
}

/// The messages of the sweep that the fake desktop can run, in the scenario's order.
fn sweep() -> Vec<(&'static str, M)> {
    vec![
        ("focus left", M::FocusWindow(Left)),
        ("focus right", M::FocusWindow(Right)),
        ("focus up", M::FocusWindow(Up)),
        ("focus down", M::FocusWindow(Down)),
        ("move left", M::MoveWindow(Left)),
        ("move right", M::MoveWindow(Right)),
        ("move up", M::MoveWindow(Up)),
        ("move down", M::MoveWindow(Down)),
        ("cycle-focus next", M::CycleFocusWindow(Next)),
        ("cycle-focus previous", M::CycleFocusWindow(Previous)),
        ("cycle-move next", M::CycleMoveWindow(Next)),
        ("cycle-move previous", M::CycleMoveWindow(Previous)),
        ("stack left", M::StackWindow(Left)),
        ("cycle-stack next", M::CycleStack(Next)),
        ("cycle-stack previous", M::CycleStack(Previous)),
        ("cycle-stack-index next", M::CycleStackIndex(Next)),
        ("focus-stack-window 0", M::FocusStackWindow(0)),
        ("unstack", M::UnstackWindow),
        ("stack-all", M::StackAll),
        ("unstack-all", M::UnstackAll),
        (
            "resize-edge left increase",
            M::ResizeWindowEdge(Left, Increase),
        ),
        (
            "resize-edge right decrease",
            M::ResizeWindowEdge(Right, Decrease),
        ),
        ("resize-edge up increase", M::ResizeWindowEdge(Up, Increase)),
        (
            "resize-edge down decrease",
            M::ResizeWindowEdge(Down, Decrease),
        ),
        (
            "resize-axis horizontal increase",
            M::ResizeWindowAxis(Axis::Horizontal, Increase),
        ),
        (
            "resize-axis vertical decrease",
            M::ResizeWindowAxis(Axis::Vertical, Decrease),
        ),
        ("resize-delta 50", M::ResizeDelta(50)),
        ("resize-delta 100", M::ResizeDelta(100)),
        ("promote", M::Promote),
        ("promote-swap", M::PromoteSwap),
        ("promote-focus", M::PromoteFocus),
        ("promote-window left", M::PromoteWindow(Left)),
        ("retile", M::Retile),
        ("flip-layout horizontal", M::FlipLayout(Axis::Horizontal)),
        ("flip-layout vertical", M::FlipLayout(Axis::Vertical)),
        (
            "flip-layout horizontal-and-vertical",
            M::FlipLayout(Axis::HorizontalAndVertical),
        ),
        (
            "flip-layout horizontal-and-vertical",
            M::FlipLayout(Axis::HorizontalAndVertical),
        ),
        ("cycle-layout next", M::CycleLayout(Next)),
        ("cycle-layout previous", M::CycleLayout(Previous)),
        (
            "layout-ratios --columns 0.6 0.4",
            M::LayoutRatios(Some(vec![0.6, 0.4]), None),
        ),
        (
            "change-layout columns",
            M::ChangeLayout(DefaultLayout::Columns),
        ),
        ("change-layout bsp", M::ChangeLayout(DefaultLayout::BSP)),
        ("preselect-direction left", M::PreselectDirection(Left)),
        ("cancel-preselect", M::CancelPreselect),
        ("toggle-monocle", M::ToggleMonocle),
        ("toggle-monocle", M::ToggleMonocle),
        ("toggle-maximize", M::ToggleMaximize),
        ("toggle-maximize", M::ToggleMaximize),
        ("toggle-float", M::ToggleFloat),
        ("toggle-float", M::ToggleFloat),
        ("toggle-lock", M::ToggleLock),
        ("toggle-lock", M::ToggleLock),
        ("toggle-tiling", M::ToggleTiling),
        ("toggle-tiling", M::ToggleTiling),
        ("toggle-pause", M::TogglePause),
        ("toggle-pause", M::TogglePause),
        ("toggle-workspace-layer", M::ToggleWorkspaceLayer),
        ("toggle-workspace-layer", M::ToggleWorkspaceLayer),
        (
            "toggle-window-container-behaviour",
            M::ToggleWindowContainerBehaviour,
        ),
        (
            "toggle-window-container-behaviour",
            M::ToggleWindowContainerBehaviour,
        ),
        ("toggle-float-override", M::ToggleFloatOverride),
        ("toggle-float-override", M::ToggleFloatOverride),
        (
            "toggle-workspace-window-container-behaviour",
            M::ToggleWorkspaceWindowContainerBehaviour,
        ),
        (
            "toggle-workspace-window-container-behaviour",
            M::ToggleWorkspaceWindowContainerBehaviour,
        ),
        (
            "toggle-workspace-float-override",
            M::ToggleWorkspaceFloatOverride,
        ),
        (
            "toggle-workspace-float-override",
            M::ToggleWorkspaceFloatOverride,
        ),
        (
            "ensure-named-workspaces 0 one two three",
            M::EnsureNamedWorkspaces(0, vec!["one".into(), "two".into(), "three".into()]),
        ),
        (
            "workspace-name 0 0 one",
            M::WorkspaceName(0, 0, "one".into()),
        ),
        ("ensure-workspaces 0 3", M::EnsureWorkspaces(0, 3)),
        (
            "workspace-tiling 0 0 disable",
            M::WorkspaceTiling(0, 0, false),
        ),
        (
            "workspace-tiling 0 0 enable",
            M::WorkspaceTiling(0, 0, true),
        ),
        (
            "named-workspace-tiling one disable",
            M::NamedWorkspaceTiling("one".into(), false),
        ),
        (
            "named-workspace-tiling one enable",
            M::NamedWorkspaceTiling("one".into(), true),
        ),
        ("focus-workspace 1", M::FocusWorkspaceNumber(1)),
        ("focus-workspace 0", M::FocusWorkspaceNumber(0)),
        ("focus-last-workspace", M::FocusLastWorkspace),
        ("focus-last-workspace", M::FocusLastWorkspace),
        ("focus-workspaces 1", M::FocusWorkspaceNumbers(1)),
        ("focus-workspaces 0", M::FocusWorkspaceNumbers(0)),
        ("cycle-workspace next", M::CycleFocusWorkspace(Next)),
        ("cycle-workspace previous", M::CycleFocusWorkspace(Previous)),
        (
            "cycle-empty-workspace next",
            M::CycleFocusEmptyWorkspace(Next),
        ),
        ("focus-workspace 0", M::FocusWorkspaceNumber(0)),
        (
            "focus-named-workspace two",
            M::FocusNamedWorkspace("two".into()),
        ),
        (
            "focus-named-workspace one",
            M::FocusNamedWorkspace("one".into()),
        ),
        ("send-to-workspace 1", M::SendContainerToWorkspaceNumber(1)),
        ("focus-workspace 1", M::FocusWorkspaceNumber(1)),
        ("move-to-workspace 0", M::MoveContainerToWorkspaceNumber(0)),
        ("focus-workspace 0", M::FocusWorkspaceNumber(0)),
        (
            "cycle-send-to-workspace next",
            M::CycleSendContainerToWorkspace(Next),
        ),
        ("focus-workspace 1", M::FocusWorkspaceNumber(1)),
        (
            "cycle-move-to-workspace previous",
            M::CycleMoveContainerToWorkspace(Previous),
        ),
        ("focus-workspace 0", M::FocusWorkspaceNumber(0)),
        ("send-to-last-workspace", M::SendContainerToLastWorkspace),
        ("move-to-last-workspace", M::MoveContainerToLastWorkspace),
        ("focus-workspace 0", M::FocusWorkspaceNumber(0)),
        ("new-workspace", M::NewWorkspace),
        ("close-workspace", M::CloseWorkspace),
        ("focus-workspace 0", M::FocusWorkspaceNumber(0)),
        (
            "workspace-layout 0 0 columns",
            M::WorkspaceLayout(0, 0, DefaultLayout::Columns),
        ),
        (
            "workspace-layout 0 0 bsp",
            M::WorkspaceLayout(0, 0, DefaultLayout::BSP),
        ),
        (
            "named-workspace-layout one columns",
            M::NamedWorkspaceLayout("one".into(), DefaultLayout::Columns),
        ),
        (
            "named-workspace-layout one bsp",
            M::NamedWorkspaceLayout("one".into(), DefaultLayout::BSP),
        ),
        (
            "workspace-layout-rule 0 0 2 rows",
            M::WorkspaceLayoutRule(0, 0, 2, DefaultLayout::Rows),
        ),
        (
            "workspace-layout-rule 0 0 3 columns",
            M::WorkspaceLayoutRule(0, 0, 3, DefaultLayout::Columns),
        ),
        (
            "clear-workspace-layout-rules 0 0",
            M::ClearWorkspaceLayoutRules(0, 0),
        ),
        (
            "named-workspace-layout-rule one 3 columns",
            M::NamedWorkspaceLayoutRule("one".into(), 3, DefaultLayout::Columns),
        ),
        (
            "clear-named-workspace-layout-rules one",
            M::ClearNamedWorkspaceLayoutRules("one".into()),
        ),
        (
            "workspace-layout 0 0 bsp",
            M::WorkspaceLayout(0, 0, DefaultLayout::BSP),
        ),
        ("focus-monitor 1", M::FocusMonitorNumber(1)),
        ("focus-monitor 0", M::FocusMonitorNumber(0)),
        ("cycle-monitor next", M::CycleFocusMonitor(Next)),
        ("cycle-monitor previous", M::CycleFocusMonitor(Previous)),
        (
            "focus-monitor-workspace 1 1",
            M::FocusMonitorWorkspaceNumber(1, 1),
        ),
        (
            "focus-monitor-workspace 0 0",
            M::FocusMonitorWorkspaceNumber(0, 0),
        ),
        ("focus-monitor 0", M::FocusMonitorNumber(0)),
        (
            "send-to-named-workspace two",
            M::SendContainerToNamedWorkspace("two".into()),
        ),
        (
            "focus-named-workspace two",
            M::FocusNamedWorkspace("two".into()),
        ),
        (
            "move-to-named-workspace one",
            M::MoveContainerToNamedWorkspace("one".into()),
        ),
        (
            "focus-named-workspace one",
            M::FocusNamedWorkspace("one".into()),
        ),
        ("send-to-monitor 1", M::SendContainerToMonitorNumber(1)),
        ("focus-monitor 1", M::FocusMonitorNumber(1)),
        ("move-to-monitor 0", M::MoveContainerToMonitorNumber(0)),
        ("focus-monitor 0", M::FocusMonitorNumber(0)),
        (
            "cycle-send-to-monitor next",
            M::CycleSendContainerToMonitor(Next),
        ),
        ("focus-monitor 1", M::FocusMonitorNumber(1)),
        (
            "cycle-move-to-monitor next",
            M::CycleMoveContainerToMonitor(Next),
        ),
        ("focus-monitor 0", M::FocusMonitorNumber(0)),
        (
            "send-to-monitor-workspace 1 1",
            M::SendContainerToMonitorWorkspaceNumber(1, 1),
        ),
        (
            "focus-monitor-workspace 1 1",
            M::FocusMonitorWorkspaceNumber(1, 1),
        ),
        (
            "move-to-monitor-workspace 0 0",
            M::MoveContainerToMonitorWorkspaceNumber(0, 0),
        ),
        ("focus-monitor 0", M::FocusMonitorNumber(0)),
        ("focus-workspace 0", M::FocusWorkspaceNumber(0)),
        (
            "move-workspace-to-monitor 1",
            M::MoveWorkspaceToMonitorNumber(1),
        ),
        (
            "cycle-move-workspace-to-monitor next",
            M::CycleMoveWorkspaceToMonitor(Next),
        ),
        (
            "swap-workspaces-with-monitor 1",
            M::SwapWorkspacesToMonitorNumber(1),
        ),
        (
            "swap-workspaces-with-monitor 1",
            M::SwapWorkspacesToMonitorNumber(1),
        ),
        (
            "global-work-area-offset 10 10 20 20",
            M::WorkAreaOffset(Rect {
                left: 10,
                top: 10,
                right: 20,
                bottom: 20,
            }),
        ),
        (
            "global-work-area-offset 0 0 0 0",
            M::WorkAreaOffset(Rect::default()),
        ),
        (
            "monitor-work-area-offset 0 10 10 20 20",
            M::MonitorWorkAreaOffset(
                0,
                Rect {
                    left: 10,
                    top: 10,
                    right: 20,
                    bottom: 20,
                },
            ),
        ),
        (
            "monitor-work-area-offset 0 0 0 0 0",
            M::MonitorWorkAreaOffset(0, Rect::default()),
        ),
        (
            "workspace-work-area-offset 0 0 10 10 20 20",
            M::WorkspaceWorkAreaOffset(
                0,
                0,
                Rect {
                    left: 10,
                    top: 10,
                    right: 20,
                    bottom: 20,
                },
            ),
        ),
        (
            "workspace-work-area-offset 0 0 0 0 0 0",
            M::WorkspaceWorkAreaOffset(0, 0, Rect::default()),
        ),
        (
            "toggle-window-based-work-area-offset",
            M::ToggleWindowBasedWorkAreaOffset,
        ),
        (
            "toggle-window-based-work-area-offset",
            M::ToggleWindowBasedWorkAreaOffset,
        ),
        (
            "focused-workspace-container-padding 20",
            M::FocusedWorkspaceContainerPadding(20),
        ),
        (
            "focused-workspace-container-padding 10",
            M::FocusedWorkspaceContainerPadding(10),
        ),
        (
            "focused-workspace-padding 20",
            M::FocusedWorkspacePadding(20),
        ),
        (
            "focused-workspace-padding 10",
            M::FocusedWorkspacePadding(10),
        ),
        (
            "adjust-container-padding increase 5",
            M::AdjustContainerPadding(Increase, 5),
        ),
        (
            "adjust-container-padding decrease 5",
            M::AdjustContainerPadding(Decrease, 5),
        ),
        (
            "adjust-workspace-padding increase 5",
            M::AdjustWorkspacePadding(Increase, 5),
        ),
        (
            "adjust-workspace-padding decrease 5",
            M::AdjustWorkspacePadding(Decrease, 5),
        ),
        ("container-padding 0 0 12", M::ContainerPadding(0, 0, 12)),
        ("container-padding 0 0 10", M::ContainerPadding(0, 0, 10)),
        ("workspace-padding 0 0 12", M::WorkspacePadding(0, 0, 12)),
        ("workspace-padding 0 0 10", M::WorkspacePadding(0, 0, 10)),
        (
            "named-workspace-container-padding one 12",
            M::NamedWorkspaceContainerPadding("one".into(), 12),
        ),
        (
            "named-workspace-container-padding one 10",
            M::NamedWorkspaceContainerPadding("one".into(), 10),
        ),
        (
            "named-workspace-padding one 12",
            M::NamedWorkspacePadding("one".into(), 12),
        ),
        (
            "named-workspace-padding one 10",
            M::NamedWorkspacePadding("one".into(), 10),
        ),
        (
            "cross-monitor-move-behaviour insert",
            M::CrossMonitorMoveBehaviour(MoveBehaviour::Insert),
        ),
        (
            "cross-monitor-move-behaviour swap",
            M::CrossMonitorMoveBehaviour(MoveBehaviour::Swap),
        ),
        (
            "toggle-cross-monitor-move-behaviour",
            M::ToggleCrossMonitorMoveBehaviour,
        ),
        (
            "toggle-cross-monitor-move-behaviour",
            M::ToggleCrossMonitorMoveBehaviour,
        ),
        (
            "toggle-monocle-focus-behaviour",
            M::ToggleMonocleFocusBehaviour,
        ),
        (
            "toggle-monocle-focus-behaviour",
            M::ToggleMonocleFocusBehaviour,
        ),
        (
            "unmanaged-window-operation-behaviour op",
            M::UnmanagedWindowOperationBehaviour(OperationBehaviour::Op),
        ),
        (
            "unmanaged-window-operation-behaviour no-op",
            M::UnmanagedWindowOperationBehaviour(OperationBehaviour::NoOp),
        ),
        (
            "unmanaged-window-operation-behaviour op",
            M::UnmanagedWindowOperationBehaviour(OperationBehaviour::Op),
        ),
        ("session-float-rule", M::SessionFloatRule),
        ("clear-session-float-rules", M::ClearSessionFloatRules),
        ("toggle-float", M::ToggleFloat),
        ("enforce-workspace-rules", M::EnforceWorkspaceRules),
        ("clear-all-workspace-rules", M::ClearAllWorkspaceRules),
        ("manage", M::ManageFocusedWindow),
        ("retile", M::Retile),
        (
            "scrolling: change-layout scrolling",
            M::ChangeLayout(DefaultLayout::Scrolling),
        ),
        ("scrolling: consume left", M::Consume(Left)),
        ("scrolling: expel", M::Expel),
        ("scrolling: toggle-column-stack", M::ToggleColumnStack),
        (
            "scrolling: scrolling-layout-full-width",
            M::ScrollingLayoutFullWidth,
        ),
        ("scrolling: center-focus", M::CenterFocus),
        ("scrolling: expand", M::Expand),
        ("change-layout bsp", M::ChangeLayout(DefaultLayout::BSP)),
    ]
}

#[test]
fn no_command_of_the_sweep_breaks_the_daemon() {
    let (fake, mut wm) = desktop();

    let mut errors = Vec::new();
    for (step, (name, message)) in sweep().into_iter().enumerate() {
        let mut reply = Vec::new();
        let result = with_backend(fake.clone(), || wm.process_command(message, &mut reply));
        if let Err(error) = result {
            errors.push(format!("{step} {name}: {error}"));
        }

        for id in IDS {
            assert!(
                managed(&wm, id),
                "after {step} `{name}` the window {id} is not managed"
            );
        }

        let mut state = Vec::new();
        with_backend(fake.clone(), || wm.process_command(M::State, &mut state)).unwrap();
        serde_json::from_slice::<serde_json::Value>(&state)
            .unwrap_or_else(|error| panic!("after {step} `{name}` the state is not JSON: {error}"));
    }

    // a command may refuse (a direction with no window in it), which is not a broken daemon; any
    // other error is one the end-to-end scenario would call a command that was not understood
    let others: Vec<_> = errors
        .iter()
        .filter(|e| !e.ends_with("there is no container or monitor in this direction"))
        .collect();
    assert!(others.is_empty(), "commands that failed: {others:#?}");
}
