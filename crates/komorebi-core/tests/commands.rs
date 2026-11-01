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
    pointer_for_all: bool,
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
            pointer_for_all: false,
        }
    }

    fn shown(&self) -> usize {
        self.monitors[self.focused].focused_workspace_idx()
    }

    /// puts a window on workspace `idx` of the focused monitor
    fn occupy(&mut self, idx: usize, window: u32) {
        self.monitors[self.focused].workspaces_mut()[idx]
            .new_container_for_window(StubWindow { id: window });
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

#[test]
fn only_windows_lets_the_pointer_pick_the_monitor_for_the_empty_and_close_commands() {
    // the platform switch of CommandHost::POINTER_PICKS_THE_MONITOR_FOR_ALL (default false: KDE and
    // macOS); a host that says true gets the preamble for close and cycle-empty too
    struct WindowsLike(Host);

    impl CommandHost for WindowsLike {
        type P = StubPlatform;
        const POINTER_PICKS_THE_MONITOR_FOR_ALL: bool = true;

        fn monitors(&self) -> &VecDeque<Monitor<StubPlatform>> {
            self.0.monitors()
        }
        fn focused_monitor(&self) -> Option<&Monitor<StubPlatform>> {
            self.0.focused_monitor()
        }
        fn focused_monitor_mut(&mut self) -> Option<&mut Monitor<StubPlatform>> {
            self.0.focused_monitor_mut()
        }
        fn focused_monitor_idx(&self) -> usize {
            self.0.focused_monitor_idx()
        }
        fn monitor_idx_from_current_pos(&mut self) -> Option<usize> {
            self.0.monitor_idx_from_current_pos()
        }
        fn focus_monitor(&mut self, idx: usize) -> eyre::Result<()> {
            self.0.focus_monitor(idx)
        }
        fn focus_workspace(&mut self, idx: usize) -> eyre::Result<()> {
            self.0.focus_workspace(idx)
        }
        fn move_container_to_workspace(
            &mut self,
            idx: usize,
            follow: bool,
            direction: Option<OperationDirection>,
        ) -> eyre::Result<()> {
            self.0.move_container_to_workspace(idx, follow, direction)
        }
    }

    let mut other = Host::new(2);
    other.monitors.push_back(monitor(2));
    other.pointer_on = Some(1);
    komorebi_core::close_workspace(&mut other).unwrap();
    assert!(
        !other.calls.iter().any(|call| call.starts_with("focus_monitor")),
        "KDE and macOS do not look at the pointer here"
    );

    let mut host = WindowsLike({
        let mut host = Host::new(2);
        host.monitors.push_back(monitor(2));
        host.pointer_on = Some(1);
        host
    });
    komorebi_core::close_workspace(&mut host).unwrap();
    assert_eq!(host.0.calls[0], "focus_monitor 1");
}
