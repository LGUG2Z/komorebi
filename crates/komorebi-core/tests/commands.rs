//! The shared commands of `komorebi_core::command`, on a recording host.
//!
//! Each test names the e2e check of `commands.py` (SWEEP) it mirrors: the e2e sends the command to a
//! running daemon and checks that it is answered and the daemon survives; here the effect on the
//! model is checked, for all three daemons at once.

mod common;

use color_eyre::eyre;
use common::*;
use komorebi_core::CommandHost;
use komorebi_core::Monitor;
use komorebi_layouts::CycleDirection;
use komorebi_layouts::OperationDirection;
use std::collections::VecDeque;

struct Host {
    monitors: VecDeque<Monitor<StubPlatform>>,
    focused: usize,
    pointer_on: Option<usize>,
    calls: Vec<String>,
}

fn monitor(workspaces: usize) -> Monitor<StubPlatform> {
    let mut monitor: Monitor<StubPlatform> = serde_json::from_value(serde_json::json!({
        "id": 7,
        "device": "Virtual-0",
        "size": { "left": 0, "top": 0, "right": 1920, "bottom": 1080 },
        "work_area_size": { "left": 0, "top": 0, "right": 1920, "bottom": 1040 },
        "work_area_offset": null,
        "window_based_work_area_offset": null,
        "window_based_work_area_offset_limit": 1,
        "workspaces": { "elements": [], "focused": 0 },
        "container_padding": null,
        "workspace_padding": null,
        "wallpaper": null,
        "floating_layer_behaviour": null
    }))
    .unwrap();
    monitor.ensure_workspace_count(workspaces);
    monitor
}

impl Host {
    fn new(workspaces: usize) -> Self {
        fresh();
        Self {
            monitors: VecDeque::from([monitor(workspaces)]),
            focused: 0,
            pointer_on: None,
            calls: vec![],
        }
    }

    fn shown(&self) -> usize {
        self.monitors[self.focused].focused_workspace_idx()
    }

    /// puts a window on workspace `idx` of the focused monitor
    fn occupy(&mut self, idx: usize, window: u32) {
        self.monitors[self.focused].workspaces_mut()[idx]
            .new_container_for_window(StubWindow { id: window })
            .unwrap();
    }
}

impl CommandHost for Host {
    type P = StubPlatform;

    fn monitors(&self) -> &VecDeque<Monitor<StubPlatform>> {
        &self.monitors
    }

    fn focused_monitor(&self) -> Option<&Monitor<StubPlatform>> {
        self.monitors.get(self.focused)
    }

    fn focused_monitor_mut(&mut self) -> Option<&mut Monitor<StubPlatform>> {
        self.monitors.get_mut(self.focused)
    }

    fn focused_monitor_idx(&self) -> usize {
        self.focused
    }

    fn monitor_idx_from_current_pos(&mut self) -> Option<usize> {
        self.pointer_on
    }

    fn focus_monitor(&mut self, idx: usize) -> eyre::Result<()> {
        self.calls.push(format!("focus_monitor {idx}"));
        self.focused = idx;
        Ok(())
    }

    fn focus_workspace(&mut self, idx: usize) -> eyre::Result<()> {
        self.calls.push(format!("focus_workspace {idx}"));
        self.monitors[self.focused].focus_workspace(idx)
    }

    fn show_workspace_on(&mut self, monitor_idx: usize, workspace_idx: usize) -> eyre::Result<()> {
        self.calls
            .push(format!("show_workspace_on {monitor_idx} {workspace_idx}"));
        self.monitors[monitor_idx].focus_workspace(workspace_idx)
    }

    fn move_container_to_workspace(
        &mut self,
        idx: usize,
        follow: bool,
        _: Option<OperationDirection>,
    ) -> eyre::Result<()> {
        self.calls.push(format!("move_container {idx} {follow}"));
        Ok(())
    }
}

#[test]
fn cycle_workspace_next_and_previous_wrap_around() {
    // commands.py: "cycle-workspace next" / "cycle-workspace previous"
    let mut host = Host::new(3);

    komorebi_core::cycle_focus_workspace(&mut host, CycleDirection::Next).unwrap();
    assert_eq!(host.shown(), 1);
    komorebi_core::cycle_focus_workspace(&mut host, CycleDirection::Next).unwrap();
    komorebi_core::cycle_focus_workspace(&mut host, CycleDirection::Next).unwrap();
    assert_eq!(
        host.shown(),
        0,
        "next wraps from the last workspace to the first"
    );

    komorebi_core::cycle_focus_workspace(&mut host, CycleDirection::Previous).unwrap();
    assert_eq!(host.shown(), 2, "previous wraps from the first to the last");
}

#[test]
fn cycling_focuses_the_monitor_under_the_pointer_when_its_workspace_is_empty() {
    // the preamble of cycle-workspace on a second monitor
    let mut host = Host::new(2);
    host.monitors.push_back(monitor(2));
    host.pointer_on = Some(1);

    komorebi_core::cycle_focus_workspace(&mut host, CycleDirection::Next).unwrap();

    assert_eq!(host.calls[0], "focus_monitor 1");
    assert_eq!(host.focused, 1);
}

#[test]
fn cycle_empty_workspace_skips_the_ones_with_windows() {
    // commands.py: "cycle-empty-workspace next"
    let mut host = Host::new(4);
    host.occupy(0, 1);
    host.occupy(1, 2);

    komorebi_core::cycle_focus_empty_workspace(&mut host, CycleDirection::Next).unwrap();

    assert_eq!(host.shown(), 2);
}

#[test]
fn cycle_empty_workspace_does_nothing_when_none_is_empty() {
    let mut host = Host::new(2);
    host.occupy(0, 1);
    host.occupy(1, 2);

    komorebi_core::cycle_focus_empty_workspace(&mut host, CycleDirection::Next).unwrap();

    assert!(host.calls.is_empty());
}

#[test]
fn close_workspace_removes_an_empty_unnamed_one_and_focuses_the_one_before() {
    // commands.py: "close-workspace"
    let mut host = Host::new(3);
    host.monitors[0].focus_workspace(2).unwrap();

    komorebi_core::close_workspace(&mut host).unwrap();

    assert_eq!(host.monitors[0].workspaces().len(), 2);
    assert_eq!(host.calls, ["focus_workspace 1"]);
}

#[test]
fn close_workspace_keeps_a_workspace_with_a_window_a_name_or_the_only_one() {
    let mut host = Host::new(2);
    host.monitors[0].focus_workspace(1).unwrap();
    host.occupy(1, 1);
    komorebi_core::close_workspace(&mut host).unwrap();
    assert_eq!(host.monitors[0].workspaces().len(), 2, "a window keeps it");

    let mut host = Host::new(2);
    host.monitors[0].focus_workspace(1).unwrap();
    host.monitors[0].workspaces_mut()[1].name = Some("web".to_string());
    komorebi_core::close_workspace(&mut host).unwrap();
    assert_eq!(host.monitors[0].workspaces().len(), 2, "a name keeps it");

    let mut host = Host::new(1);
    komorebi_core::close_workspace(&mut host).unwrap();
    assert_eq!(host.monitors[0].workspaces().len(), 1, "the only one stays");
}

#[test]
fn focus_last_workspace_goes_back_and_forth() {
    // commands.py: "focus-last-workspace" twice
    let mut host = Host::new(3);
    host.monitors[0].focus_workspace(2).unwrap();

    komorebi_core::focus_last_workspace(&mut host).unwrap();
    assert_eq!(host.shown(), 0, "the workspace shown before this one");

    komorebi_core::focus_last_workspace(&mut host).unwrap();
    assert_eq!(host.shown(), 2, "and back again");
}

#[test]
fn move_to_last_workspace_moves_the_container_and_remembers_where_it_came_from() {
    // commands.py: "move-to-last-workspace" and "send-to-last-workspace"
    let mut host = Host::new(3);
    host.monitors[0].focus_workspace(2).unwrap();

    komorebi_core::move_container_to_last_workspace(&mut host, true).unwrap();
    assert_eq!(host.calls, ["move_container 0 true"]);
    assert_eq!(host.monitors[0].last_focused_workspace, Some(2));

    host.calls.clear();
    komorebi_core::move_container_to_last_workspace(&mut host, false).unwrap();
    assert_eq!(
        host.calls,
        ["move_container 2 false"],
        "send does not follow"
    );
}

impl komorebi_core::MonitorCommandHost for Host {
    fn mouse_follows_focus(&self) -> bool {
        true
    }

    fn update_focused_workspace(
        &mut self,
        follow_focus: bool,
        trigger_focus: bool,
    ) -> eyre::Result<()> {
        self.calls.push(format!(
            "update_focused_workspace {follow_focus} {trigger_focus}"
        ));
        Ok(())
    }

    fn direction_from_monitor_idx(&self, idx: usize) -> Option<OperationDirection> {
        (idx != self.focused).then_some(OperationDirection::Right)
    }

    fn move_container_to_monitor(
        &mut self,
        idx: usize,
        workspace_idx: Option<usize>,
        follow: bool,
        direction: Option<OperationDirection>,
    ) -> eyre::Result<()> {
        self.calls.push(format!(
            "move_to_monitor {idx} {workspace_idx:?} {follow} {}",
            direction.is_some()
        ));
        Ok(())
    }

    fn focused_workspace_idx(&self) -> eyre::Result<usize> {
        Ok(self.shown())
    }
}

fn two_monitors() -> Host {
    let mut host = Host::new(3);
    host.monitors.push_back(monitor(3));
    host
}

#[test]
fn cycle_move_to_workspace_moves_and_send_to_workspace_does_not_follow() {
    // commands.py: "cycle-move-to-workspace previous" and "cycle-send-to-workspace next"
    let mut host = Host::new(3);

    komorebi_core::cycle_move_container_to_workspace(&mut host, CycleDirection::Previous, true)
        .unwrap();
    komorebi_core::cycle_move_container_to_workspace(&mut host, CycleDirection::Next, false)
        .unwrap();

    assert_eq!(
        host.calls,
        ["move_container 2 true", "move_container 1 false"]
    );
}

#[test]
fn cycle_monitor_focuses_the_next_monitor_and_lays_it_out() {
    // commands.py: "cycle-monitor next"
    let mut host = two_monitors();

    komorebi_core::cycle_focus_monitor(&mut host, CycleDirection::Next).unwrap();

    assert_eq!(host.focused, 1);
    assert_eq!(
        host.calls,
        ["focus_monitor 1", "update_focused_workspace true true"]
    );

    komorebi_core::cycle_focus_monitor(&mut host, CycleDirection::Next).unwrap();
    assert_eq!(host.focused, 0, "it wraps");
}

#[test]
fn cycle_move_and_send_to_monitor_go_in_the_direction_of_the_target() {
    // commands.py: "cycle-move-to-monitor next" and "cycle-send-to-monitor next"
    let mut host = two_monitors();

    komorebi_core::cycle_move_container_to_monitor(&mut host, CycleDirection::Next, true).unwrap();
    komorebi_core::cycle_move_container_to_monitor(&mut host, CycleDirection::Next, false).unwrap();

    assert_eq!(
        host.calls,
        [
            "move_to_monitor 1 None true true",
            "move_to_monitor 1 None false true"
        ]
    );
}

#[test]
fn focus_monitor_workspace_does_nothing_when_that_is_already_in_front() {
    // commands.py: "focus-monitor-workspace 0 1"
    let mut host = two_monitors();

    komorebi_core::focus_monitor_workspace_number(&mut host, 0, 0).unwrap();
    assert!(host.calls.is_empty());

    komorebi_core::focus_monitor_workspace_number(&mut host, 1, 2).unwrap();
    assert_eq!(host.calls, ["focus_monitor 1", "focus_workspace 2"]);
}

#[test]
fn move_and_send_to_monitor_workspace_name_the_workspace() {
    // commands.py: "move-to-monitor-workspace 0 1" and "send-to-monitor-workspace 0 1"
    let mut host = two_monitors();

    komorebi_core::move_container_to_monitor_workspace_number(&mut host, 1, 2, true).unwrap();
    komorebi_core::move_container_to_monitor_workspace_number(&mut host, 1, 2, false).unwrap();

    assert_eq!(
        host.calls,
        [
            "move_to_monitor 1 Some(2) true true",
            "move_to_monitor 1 Some(2) false true"
        ]
    );
}

impl komorebi_core::ContainerCommandHost for Host {
    fn monitors_mut(&mut self) -> &mut VecDeque<Monitor<StubPlatform>> {
        &mut self.monitors
    }
}

#[test]
fn lock_and_unlock_a_container_by_index() {
    // commands.py has toggle-lock; the by-index forms (lock/unlock-monitor-workspace-container) are in the KDE command_sweep
    let mut host = Host::new(2);
    host.occupy(0, 1);

    komorebi_core::set_container_locked(&mut host, 0, 0, 0, true).unwrap();
    assert!(host.monitors[0].workspaces()[0].containers()[0].locked);

    komorebi_core::set_container_locked(&mut host, 0, 0, 0, false).unwrap();
    assert!(!host.monitors[0].workspaces()[0].containers()[0].locked);
}

#[test]
fn locking_a_container_that_is_not_there_is_ignored_but_a_missing_monitor_or_workspace_is_not() {
    let mut host = Host::new(1);

    assert!(komorebi_core::set_container_locked(&mut host, 0, 0, 5, true).is_ok());
    assert!(komorebi_core::set_container_locked(&mut host, 3, 0, 0, true).is_err());
    assert!(komorebi_core::set_container_locked(&mut host, 0, 9, 0, true).is_err());
}

fn pointer_on_the_second_monitor() -> Host {
    let mut host = two_monitors();
    host.pointer_on = Some(1);
    host
}

#[test]
fn the_pointer_picks_the_monitor_for_the_empty_close_last_cycle_and_number_commands_on_all_platforms()
 {
    // commands.py: close-workspace, cycle-empty-workspace, focus-last-workspace, focus-workspace,
    // focus-workspaces; the monitor under the pointer is focused first when its workspace is empty
    for command in 0..5 {
        let mut host = pointer_on_the_second_monitor();
        match command {
            0 => komorebi_core::close_workspace(&mut host).unwrap(),
            1 => komorebi_core::focus_last_workspace(&mut host).unwrap(),
            2 => {
                komorebi_core::cycle_focus_empty_workspace(&mut host, CycleDirection::Next).unwrap()
            }
            3 => komorebi_core::focus_workspace_number(&mut host, 1).unwrap(),
            _ => komorebi_core::focus_workspace_numbers(&mut host, 1).unwrap(),
        }
        assert_eq!(host.calls[0], "focus_monitor 1", "command {command}");
    }
}

#[test]
fn focus_workspace_number_only_switches_when_it_is_not_in_front() {
    // commands.py: "focus-workspace 1", "focus-workspace 0"
    let mut host = Host::new(3);

    komorebi_core::focus_workspace_number(&mut host, 0).unwrap();
    assert!(host.calls.is_empty());

    komorebi_core::focus_workspace_number(&mut host, 2).unwrap();
    assert_eq!(host.calls, ["focus_workspace 2"]);
}

#[test]
fn focus_workspaces_shows_the_workspace_on_every_other_monitor_then_on_the_focused_one() {
    // commands.py: "focus-workspaces 1", "focus-workspaces 0"
    let mut host = two_monitors();
    host.monitors.push_back(monitor(3));

    komorebi_core::focus_workspace_numbers(&mut host, 2).unwrap();

    assert_eq!(
        host.calls,
        [
            "show_workspace_on 1 2",
            "show_workspace_on 2 2",
            "focus_workspace 2"
        ]
    );
}

/// A host of the window commands: records which of the daemon's functions the shared command called
struct Windows {
    host: Host,
    container_behaviour: komorebi_config::behaviour::WindowContainerBehaviour,
    move_behaviour: komorebi_protocol::types::MoveBehaviour,
    monocle_behaviour: komorebi_protocol::types::MonocleFocusBehaviour,
}

impl Windows {
    fn new() -> Self {
        Self {
            host: Host::new(3),
            container_behaviour: Default::default(),
            move_behaviour: komorebi_protocol::types::MoveBehaviour::Swap,
            monocle_behaviour: komorebi_protocol::types::MonocleFocusBehaviour::Cycle,
        }
    }

    fn floating() -> Self {
        let mut windows = Self::new();
        windows.host.monitors[0]
            .focused_workspace_mut()
            .unwrap()
            .layer = komorebi_core::WorkspaceLayer::Floating;
        windows
    }

    fn record(&mut self, call: impl Into<String>) -> eyre::Result<()> {
        self.host.calls.push(call.into());
        Ok(())
    }
}

impl CommandHost for Windows {
    type P = StubPlatform;
    fn monitors(&self) -> &VecDeque<Monitor<StubPlatform>> {
        self.host.monitors()
    }
    fn focused_monitor(&self) -> Option<&Monitor<StubPlatform>> {
        self.host.focused_monitor()
    }
    fn focused_monitor_mut(&mut self) -> Option<&mut Monitor<StubPlatform>> {
        self.host.focused_monitor_mut()
    }
    fn focused_monitor_idx(&self) -> usize {
        self.host.focused_monitor_idx()
    }
    fn monitor_idx_from_current_pos(&mut self) -> Option<usize> {
        self.host.monitor_idx_from_current_pos()
    }
    fn focus_monitor(&mut self, idx: usize) -> eyre::Result<()> {
        self.host.focus_monitor(idx)
    }
    fn focus_workspace(&mut self, idx: usize) -> eyre::Result<()> {
        self.host.focus_workspace(idx)
    }
    fn show_workspace_on(&mut self, monitor_idx: usize, workspace_idx: usize) -> eyre::Result<()> {
        self.host.show_workspace_on(monitor_idx, workspace_idx)
    }
    fn move_container_to_workspace(
        &mut self,
        idx: usize,
        follow: bool,
        direction: Option<OperationDirection>,
    ) -> eyre::Result<()> {
        self.host
            .move_container_to_workspace(idx, follow, direction)
    }
}

impl komorebi_core::MonitorCommandHost for Windows {
    fn mouse_follows_focus(&self) -> bool {
        self.host.mouse_follows_focus()
    }
    fn update_focused_workspace(&mut self, a: bool, b: bool) -> eyre::Result<()> {
        self.host.update_focused_workspace(a, b)
    }
    fn direction_from_monitor_idx(&self, idx: usize) -> Option<OperationDirection> {
        self.host.direction_from_monitor_idx(idx)
    }
    fn move_container_to_monitor(
        &mut self,
        idx: usize,
        workspace_idx: Option<usize>,
        follow: bool,
        direction: Option<OperationDirection>,
    ) -> eyre::Result<()> {
        self.host
            .move_container_to_monitor(idx, workspace_idx, follow, direction)
    }
    fn focused_workspace_idx(&self) -> eyre::Result<usize> {
        self.host.focused_workspace_idx()
    }
}

impl komorebi_core::WindowCommandHost for Windows {
    fn move_container_in_direction(&mut self, d: OperationDirection) -> eyre::Result<()> {
        self.record(format!("move_container {d}"))
    }
    fn move_floating_window_in_direction(&mut self, d: OperationDirection) -> eyre::Result<()> {
        self.record(format!("move_floating {d}"))
    }
    fn focus_container_in_direction(&mut self, d: OperationDirection) -> eyre::Result<()> {
        self.record(format!("focus_container {d}"))
    }
    fn focus_floating_window_in_direction(&mut self, d: OperationDirection) -> eyre::Result<()> {
        self.record(format!("focus_floating {d}"))
    }
    fn focus_container_in_cycle_direction(&mut self, d: CycleDirection) -> eyre::Result<()> {
        self.record(format!("cycle_container {d}"))
    }
    fn focus_floating_window_in_cycle_direction(&mut self, d: CycleDirection) -> eyre::Result<()> {
        self.record(format!("cycle_floating {d}"))
    }
    fn preselect_container_in_direction(&mut self, d: OperationDirection) -> eyre::Result<()> {
        self.record(format!("preselect {d}"))
    }
    fn promote_container_to_front(&mut self) -> eyre::Result<()> {
        self.record("promote")
    }
    fn update_focused_workspace_layout(&mut self) -> eyre::Result<()> {
        self.record("update")
    }
}

impl komorebi_core::BehaviourCommandHost for Windows {
    fn window_container_behaviour(&self) -> komorebi_config::behaviour::WindowContainerBehaviour {
        self.container_behaviour
    }
    fn set_window_container_behaviour(
        &mut self,
        behaviour: komorebi_config::behaviour::WindowContainerBehaviour,
    ) {
        self.container_behaviour = behaviour;
    }
    fn cross_monitor_move_behaviour(&self) -> komorebi_protocol::types::MoveBehaviour {
        self.move_behaviour
    }
    fn set_cross_monitor_move_behaviour(&mut self, b: komorebi_protocol::types::MoveBehaviour) {
        self.move_behaviour = b;
    }
    fn monocle_focus_behaviour(&self) -> komorebi_protocol::types::MonocleFocusBehaviour {
        self.monocle_behaviour
    }
    fn set_monocle_focus_behaviour(&mut self, b: komorebi_protocol::types::MonocleFocusBehaviour) {
        self.monocle_behaviour = b;
    }
}

#[test]
fn the_window_commands_pick_the_tiling_or_the_floating_function_by_the_layer() {
    // mirrors: commands / "move left", "focus left", "cycle-focus next", "cycle-move next"
    let mut tiling = Windows::new();
    komorebi_core::move_window(&mut tiling, OperationDirection::Left).unwrap();
    komorebi_core::focus_window(&mut tiling, OperationDirection::Right).unwrap();
    komorebi_core::cycle_focus_window(&mut tiling, CycleDirection::Next).unwrap();
    assert_eq!(
        tiling.host.calls,
        [
            "move_container Left",
            "focus_container Right",
            "cycle_container Next"
        ]
    );

    let mut floating = Windows::floating();
    komorebi_core::move_window(&mut floating, OperationDirection::Left).unwrap();
    komorebi_core::focus_window(&mut floating, OperationDirection::Right).unwrap();
    komorebi_core::cycle_focus_window(&mut floating, CycleDirection::Previous).unwrap();
    assert_eq!(
        floating.host.calls,
        [
            "move_floating Left",
            "focus_floating Right",
            "cycle_floating Previous"
        ]
    );
}

#[test]
fn preselect_sets_a_direction_once_on_the_tiling_layer_and_lays_the_workspace_out() {
    // mirrors: commands / "preselect left", "cancel-preselect"
    let mut windows = Windows::new();
    komorebi_core::preselect_direction(&mut windows, OperationDirection::Left).unwrap();
    assert_eq!(windows.host.calls, ["preselect Left", "update"]);

    // a workspace that already has a preselect ignores the command
    windows.host.calls.clear();
    windows.host.monitors[0]
        .focused_workspace_mut()
        .unwrap()
        .preselected_container_idx = Some(0);
    komorebi_core::preselect_direction(&mut windows, OperationDirection::Up).unwrap();
    assert!(windows.host.calls.is_empty());

    // cancelling forgets it and lays the workspace out
    komorebi_core::cancel_preselect(&mut windows).unwrap();
    assert_eq!(windows.host.calls, ["update"]);
    assert_eq!(
        windows.host.monitors[0]
            .focused_workspace()
            .unwrap()
            .preselected_container_idx,
        None
    );

    // the floating layer has no preselect
    let mut floating = Windows::floating();
    komorebi_core::preselect_direction(&mut floating, OperationDirection::Left).unwrap();
    assert!(floating.host.calls.is_empty());
}

#[test]
fn promote_window_focuses_in_the_direction_then_promotes() {
    // mirrors: commands / "promote-window left"
    let mut windows = Windows::new();
    komorebi_core::promote_window(&mut windows, OperationDirection::Left).unwrap();
    assert_eq!(windows.host.calls, ["focus_container Left", "promote"]);
}

fn name_workspace(host: &mut Host, monitor: usize, workspace: usize, name: &str) {
    host.monitors[monitor].workspaces_mut()[workspace].name = Some(name.to_string());
}

#[test]
fn named_workspace_commands_find_the_workspace_on_any_monitor() {
    // mirrors: commands / "focus-named-workspace", "move-to-named-workspace",
    // "send-to-named-workspace" (the stub host has focused the monitor with the workspace by then,
    // so there is no direction to it)
    let mut windows = Windows::new();
    windows.host.monitors.push_back(monitor(3));
    name_workspace(&mut windows.host, 1, 2, "web");

    komorebi_core::focus_named_workspace(&mut windows, "web").unwrap();
    assert_eq!(windows.host.calls, ["focus_monitor 1", "focus_workspace 2"]);

    windows.host.calls.clear();
    komorebi_core::move_container_to_named_workspace(&mut windows, "web", true).unwrap();
    komorebi_core::move_container_to_named_workspace(&mut windows, "web", false).unwrap();
    assert_eq!(
        windows.host.calls,
        [
            "move_to_monitor 1 Some(2) true false",
            "move_to_monitor 1 Some(2) false false"
        ]
    );

    // a name that is nowhere does nothing
    windows.host.calls.clear();
    komorebi_core::focus_named_workspace(&mut windows, "nowhere").unwrap();
    komorebi_core::move_container_to_named_workspace(&mut windows, "nowhere", true).unwrap();
    assert!(windows.host.calls.is_empty());
}

#[test]
fn the_behaviour_toggles_flip_between_two_values_and_leave_noop_alone() {
    // mirrors: commands / "toggle-window-container-behaviour", "toggle-cross-monitor-move-behaviour",
    // "toggle-monocle-focus-behaviour"
    use komorebi_config::behaviour::WindowContainerBehaviour as Container;
    use komorebi_protocol::types::MonocleFocusBehaviour as Monocle;
    use komorebi_protocol::types::MoveBehaviour as Move;

    let mut windows = Windows::new();
    komorebi_core::toggle_window_container_behaviour(&mut windows);
    assert_eq!(windows.container_behaviour, Container::Append);
    komorebi_core::toggle_window_container_behaviour(&mut windows);
    assert_eq!(windows.container_behaviour, Container::Create);

    komorebi_core::toggle_cross_monitor_move_behaviour(&mut windows);
    assert_eq!(windows.move_behaviour, Move::Insert);
    komorebi_core::toggle_cross_monitor_move_behaviour(&mut windows);
    assert_eq!(windows.move_behaviour, Move::Swap);
    windows.move_behaviour = Move::NoOp;
    komorebi_core::toggle_cross_monitor_move_behaviour(&mut windows);
    assert_eq!(windows.move_behaviour, Move::NoOp);

    komorebi_core::toggle_monocle_focus_behaviour(&mut windows);
    assert_eq!(windows.monocle_behaviour, Monocle::NoOp);
    komorebi_core::toggle_monocle_focus_behaviour(&mut windows);
    assert_eq!(windows.monocle_behaviour, Monocle::Cycle);
}
