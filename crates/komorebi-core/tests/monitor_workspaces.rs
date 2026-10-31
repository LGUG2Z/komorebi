//! Characterization of `Monitor<P>`: the ring of workspaces, how it grows and shrinks, which
//! workspace has the focus, and the name a new workspace takes from what the platform knew.

mod common;

use common::*;
use komorebi_core::Monitor;
use komorebi_core::Workspace;
use komorebi_layouts::DefaultLayout;
use komorebi_layouts::Layout;

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

fn named(monitor: &mut Monitor<StubPlatform>, names: &[&str]) {
    for (workspace, name) in monitor.workspaces_mut().iter_mut().zip(names) {
        workspace.name = Some(name.to_string());
    }
}

fn names(monitor: &Monitor<StubPlatform>) -> Vec<Option<String>> {
    monitor
        .workspaces()
        .iter()
        .map(|w| w.name.clone())
        .collect()
}

#[test]
fn a_monitor_read_without_a_serial_number_has_the_platforms_default() {
    let monitor = monitor(0);

    assert_eq!(monitor.serial_number_id, "");
    assert_eq!(monitor.id, 7);
    assert!(monitor.last_focused_workspace.is_none());
}

#[test]
fn ensuring_a_count_adds_default_workspaces_and_never_removes_any() {
    fresh();
    let mut monitor = monitor(0);

    monitor.ensure_workspace_count(3);
    assert_eq!(monitor.workspaces().len(), 3);
    assert_eq!(monitor.new_workspace_idx(), 3);

    named(&mut monitor, &["a", "b", "c"]);
    monitor.ensure_workspace_count(2);
    monitor.ensure_workspace_count(0);
    assert_eq!(monitor.workspaces().len(), 3);

    monitor.ensure_workspace_count(4);
    assert_eq!(
        names(&monitor),
        [Some("a".into()), Some("b".into()), Some("c".into()), None]
    );
}

#[test]
fn ensuring_a_count_leaves_the_focus_alone() {
    fresh();
    let mut monitor = monitor(2);
    monitor.focus_workspace(1).unwrap();

    monitor.ensure_workspace_count(5);

    assert_eq!(monitor.focused_workspace_idx(), 1);
}

#[test]
fn a_new_workspace_is_a_default_workspace() {
    fresh();
    let monitor = monitor(1);

    assert_eq!(monitor.workspaces()[0], Workspace::default());
}

#[test]
fn focusing_a_workspace_remembers_the_one_that_had_the_focus() {
    fresh();
    let mut monitor = monitor(3);

    monitor.focus_workspace(2).unwrap();
    assert_eq!(monitor.focused_workspace_idx(), 2);
    assert_eq!(monitor.last_focused_workspace, Some(0));

    monitor.focus_workspace(1).unwrap();
    assert_eq!(monitor.last_focused_workspace, Some(2));
}

#[test]
fn focusing_the_workspace_that_has_the_focus_makes_it_its_own_last_focused() {
    fresh();
    let mut monitor = monitor(2);
    monitor.focus_workspace(1).unwrap();

    monitor.focus_workspace(1).unwrap();

    assert_eq!(monitor.last_focused_workspace, Some(1));
}

#[test]
fn focusing_past_the_end_creates_every_workspace_up_to_it() {
    fresh();
    let mut monitor = monitor(1);

    monitor.focus_workspace(3).unwrap();

    assert_eq!(monitor.workspaces().len(), 4);
    assert_eq!(monitor.focused_workspace_idx(), 3);
    assert_eq!(monitor.last_focused_workspace, Some(0));
}

#[test]
fn focusing_in_a_monitor_with_no_workspace_creates_it() {
    fresh();
    let mut monitor = monitor(0);

    monitor.focus_workspace(0).unwrap();

    assert_eq!(monitor.workspaces().len(), 1);
    assert!(monitor.focused_workspace().is_some());
}

#[test]
fn focusing_without_a_known_name_leaves_the_names_as_they_are() {
    fresh();
    let mut monitor = monitor(2);
    named(&mut monitor, &["a", "b"]);

    monitor.focus_workspace(1).unwrap();
    monitor.focus_workspace(4).unwrap();

    assert_eq!(
        names(&monitor),
        [Some("a".into()), Some("b".into()), None, None, None]
    );
}

#[test]
fn a_workspace_created_by_focusing_takes_the_name_the_platform_knew_for_it() {
    fresh();
    let mut monitor = monitor(1);
    set_known_workspace_name(2, "known");
    set_known_workspace_name(1, "skipped");

    monitor.focus_workspace(2).unwrap();

    // only the workspace that was focused takes the name, the one made on the way does not
    assert_eq!(names(&monitor), [None, None, Some("known".into())]);
}

#[test]
fn focusing_an_existing_workspace_overwrites_its_name_with_the_known_one() {
    fresh();
    let mut monitor = monitor(2);
    named(&mut monitor, &["mine", "other"]);
    set_known_workspace_name(1, "known");

    monitor.focus_workspace(1).unwrap();

    // the code comment says the name is set when the workspace is created, but it is set at
    // every focus: a name given at runtime is replaced by the known one
    assert_eq!(monitor.focused_workspace_name().as_deref(), Some("known"));
}

#[test]
fn removing_a_workspace_by_index_takes_it_out_and_does_not_move_the_focus_index() {
    fresh();
    let mut monitor = monitor(3);
    named(&mut monitor, &["a", "b", "c"]);
    monitor.focus_workspace(2).unwrap();

    let removed = monitor.remove_workspace_by_idx(0).unwrap();

    assert_eq!(removed.name.as_deref(), Some("a"));
    assert_eq!(names(&monitor), [Some("b".into()), Some("c".into())]);
    assert_eq!(monitor.focused_workspace_idx(), 2);
}

#[test]
fn removing_a_workspace_past_the_end_removes_nothing_and_focuses_the_one_before_it() {
    fresh();
    let mut monitor = monitor(2);

    assert!(monitor.remove_workspace_by_idx(5).is_none());

    // the focus went to index 4, creating the workspaces up to it
    assert_eq!(monitor.workspaces().len(), 5);
    assert_eq!(monitor.focused_workspace_idx(), 4);
}

#[test]
fn removing_the_first_workspace_of_an_empty_monitor_gives_it_a_default_one() {
    fresh();
    let mut monitor = monitor(0);

    assert!(monitor.remove_workspace_by_idx(0).is_none());

    assert_eq!(monitor.workspaces().len(), 1);
}

#[test]
fn removing_every_workspace_hands_them_over_in_order_and_leaves_the_focus_index() {
    fresh();
    let mut monitor = monitor(3);
    named(&mut monitor, &["a", "b", "c"]);
    monitor.focus_workspace(2).unwrap();

    let removed = monitor.remove_workspaces();

    let removed_names: Vec<_> = removed.iter().map(|w| w.name.clone().unwrap()).collect();
    assert_eq!(removed_names, ["a", "b", "c"]);
    assert!(monitor.workspaces().is_empty());
    assert!(monitor.focused_workspace().is_none());
    assert_eq!(monitor.focused_workspace_idx(), 2);
    assert_eq!(monitor.focused_workspace_name(), None);
}

#[test]
fn a_container_added_to_a_workspace_goes_to_its_back_and_takes_the_focus() {
    fresh();
    let mut monitor = monitor(2);
    monitor.focus_workspace(1).unwrap();
    monitor.workspaces_mut()[0].add_container_to_back(container_of(&[1]));

    monitor.add_container(container_of(&[2]), Some(0)).unwrap();

    assert_eq!(layout_of(&monitor.workspaces()[0]), [vec![1], vec![2]]);
    assert_eq!(monitor.workspaces()[0].focused_container_idx(), 1);
    assert!(monitor.workspaces()[1].containers().is_empty());
}

#[test]
fn a_container_added_without_a_workspace_goes_to_the_focused_one() {
    fresh();
    let mut monitor = monitor(2);
    monitor.focus_workspace(1).unwrap();

    monitor.add_container(container_of(&[3]), None).unwrap();

    assert_eq!(layout_of(&monitor.workspaces()[1]), [[3]]);
}

#[test]
fn adding_a_container_to_a_workspace_that_is_not_there_is_an_error() {
    fresh();
    let mut monitor = monitor(1);

    assert!(monitor.add_container(container_of(&[1]), Some(4)).is_err());
    assert!(monitor.workspaces()[0].containers().is_empty());
}

#[test]
fn adding_a_container_to_a_monitor_without_workspaces_is_an_error() {
    fresh();
    let mut monitor = monitor(0);

    assert!(monitor.add_container(container_of(&[1]), None).is_err());
}

#[test]
fn the_focused_workspace_layout_is_given_only_while_it_tiles() {
    fresh();
    let mut monitor = monitor(1);
    assert_eq!(
        monitor.focused_workspace_layout(),
        Some(Layout::Default(DefaultLayout::BSP))
    );

    monitor.focused_workspace_mut().unwrap().tile = false;
    assert_eq!(monitor.focused_workspace_layout(), None);

    monitor.remove_workspaces();
    assert_eq!(monitor.focused_workspace_layout(), None);
}

#[test]
fn a_monitor_writes_last_focused_workspace_only_once_a_workspace_was_focused() {
    fresh();
    let mut monitor = monitor(2);
    let keys = |m: &Monitor<StubPlatform>| -> Vec<String> {
        serde_json::to_value(m)
            .unwrap()
            .as_object()
            .unwrap()
            .keys()
            .cloned()
            .collect()
    };
    assert!(!keys(&monitor).contains(&"last_focused_workspace".to_string()));

    monitor.focus_workspace(1).unwrap();

    assert!(keys(&monitor).contains(&"last_focused_workspace".to_string()));
}

#[test]
fn a_monitor_with_workspaces_round_trips_through_json() {
    fresh();
    let mut monitor = monitor(2);
    monitor.focus_workspace(1).unwrap();
    monitor.add_container(container_of(&[1, 2]), None).unwrap();

    let json = serde_json::to_value(&monitor).unwrap();
    let back: Monitor<StubPlatform> = serde_json::from_value(json.clone()).unwrap();

    assert_eq!(back, monitor);
    assert_eq!(serde_json::to_value(&back).unwrap(), json);
}

#[test]
fn removing_the_workspace_at_the_length_focuses_the_last_one_and_removes_nothing() {
    fresh();
    let mut monitor = monitor(3);

    assert!(monitor.remove_workspace_by_idx(3).is_none());

    assert_eq!(monitor.workspaces().len(), 3);
    assert_eq!(monitor.focused_workspace_idx(), 2);
}

#[test]
fn an_old_monitor_file_with_workspaces_and_without_optional_keys_loads() {
    fresh();
    let workspace = serde_json::to_value(Workspace::<StubPlatform>::default()).unwrap();
    let mut workspace = workspace;
    for key in ["layout_options_rules", "name", "layout_flip", "wallpaper"] {
        workspace.as_object_mut().unwrap().remove(key);
    }
    let monitor: Monitor<StubPlatform> = serde_json::from_value(serde_json::json!({
        "id": 1, "device": "d",
        "size": { "left": 0, "top": 0, "right": 1, "bottom": 1 },
        "work_area_size": { "left": 0, "top": 0, "right": 1, "bottom": 1 },
        "work_area_offset": null, "window_based_work_area_offset": null,
        "window_based_work_area_offset_limit": 1,
        "workspaces": { "elements": [workspace.clone(), workspace], "focused": 1 },
        "container_padding": null, "workspace_padding": null,
        "wallpaper": null, "floating_layer_behaviour": null
    }))
    .unwrap();

    assert_eq!(monitor.workspaces().len(), 2);
    assert_eq!(monitor.focused_workspace_idx(), 1);
    assert!(monitor.workspaces()[0].layout_options_rules.is_empty());
}
