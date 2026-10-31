//! Characterization of `Workspace<P>`: the ring of containers, the index and focus invariants of
//! adding, removing, stacking and moving them, and the resize adjustments that follow them.

mod common;

use common::*;
use komorebi_config::placement::InitialWindowPlacementRules;
use komorebi_config::placement::PlacementTarget;
use komorebi_config::placement::WindowPlacement;
use komorebi_core::Container;
use komorebi_core::Lockable;
use komorebi_core::Workspace;
use komorebi_layouts::CycleDirection;
use komorebi_layouts::DefaultLayout;
use komorebi_layouts::Layout;
use komorebi_layouts::OperationDirection;
use komorebi_layouts::Rect;

fn w(id: u32) -> StubWindow {
    StubWindow { id }
}

fn mark(workspace: &mut Workspace<StubPlatform>, n: i32) {
    workspace.resize_dimensions = (0..n)
        .map(|i| {
            Some(Rect {
                left: i + 1,
                top: i + 1,
                right: i + 1,
                bottom: i + 1,
            })
        })
        .collect();
}

#[test]
fn an_empty_workspace_has_nothing_in_it() {
    fresh();
    let workspace = Workspace::<StubPlatform>::default();

    assert!(workspace.is_empty());
    assert_eq!(workspace.focused_container_idx(), 0);
    assert!(workspace.focused_container().is_none());
    assert!(workspace.visible_windows().iter().all(Option::is_none));
    assert!(!workspace.contains_window(1));
    assert_eq!(workspace.container_idx_for_window(1), None);
}

#[test]
fn the_first_window_goes_to_index_zero_and_each_next_after_the_focused_container() {
    fresh();
    let mut workspace = workspace_of(&[1]);
    assert_eq!(layout_of(&workspace), [[1]]);

    workspace.new_container_for_window(w(2)).unwrap();
    workspace.focus_container(0);
    workspace.new_container_for_window(w(3)).unwrap();

    assert_eq!(layout_of(&workspace), [vec![1], vec![3], vec![2]]);
    assert_eq!(workspace.focused_container_idx(), 1);
}

#[test]
fn a_new_container_is_focused_and_gets_an_empty_resize_slot_at_its_index() {
    fresh();
    let mut workspace = workspace_of(&[1, 2]);
    mark(&mut workspace, 2);
    workspace.focus_container(0);

    workspace.new_container_for_window(w(3)).unwrap();

    assert_eq!(workspace.focused_container_idx(), 1);
    assert_eq!(workspace.resize_dimensions.len(), 3);
    assert!(workspace.resize_dimensions[1].is_none());
    assert!(workspace.resize_dimensions[0].is_some());
    assert!(workspace.resize_dimensions[2].is_some());
}

#[test]
fn the_scrolling_hook_chooses_the_index_of_a_new_container_in_a_non_empty_workspace() {
    fresh();
    let mut workspace = workspace_of(&[1, 2, 3]);
    set_scrolling_insertion_idx(Some(0));

    workspace.new_container_for_window(w(4)).unwrap();

    assert_eq!(layout_of(&workspace)[0], [4]);
}

#[test]
fn a_new_window_takes_the_container_a_preselection_reserved() {
    fresh();
    let mut workspace = workspace_of(&[1, 2]);
    workspace.preselect_container_idx(1);
    assert_eq!(workspace.containers().len(), 3);
    assert!(workspace.containers()[1].is_preselect());
    assert_eq!(workspace.preselected_container_idx, Some(1));

    workspace.new_container_for_window(w(3)).unwrap();

    assert_eq!(layout_of(&workspace), [vec![1], vec![3], vec![2]]);
    assert_eq!(workspace.preselected_container_idx, None);
    assert!(!workspace.containers().iter().any(Container::is_preselect));
}

#[test]
fn cancelling_a_preselection_removes_its_container_and_does_nothing_without_one() {
    fresh();
    let mut workspace = workspace_of(&[1, 2]);

    workspace.cancel_preselect();
    assert_eq!(workspace.containers().len(), 2);

    workspace.preselect_container_idx(0);
    workspace.cancel_preselect();
    assert_eq!(layout_of(&workspace), [vec![1], vec![2]]);
    assert_eq!(workspace.preselected_container_idx, None);
}

#[test]
fn initial_placement_rules_choose_where_a_new_window_goes() {
    fresh();
    let cases = [
        (WindowPlacement::AfterFocused, vec![1, 2, 9, 3]),
        (WindowPlacement::BeforeFocused, vec![1, 9, 2, 3]),
        (WindowPlacement::Last, vec![1, 2, 3, 9]),
        (WindowPlacement::Primary, vec![9, 1, 2, 3]),
    ];

    for (placement, order) in cases {
        let mut workspace = workspace_of(&[1, 2, 3]);
        workspace.focus_container(1);
        workspace.initial_window_placement_rules = Some(InitialWindowPlacementRules::Target(
            PlacementTarget::Placement(placement),
        ));

        workspace.new_container_for_window(w(9)).unwrap();

        let firsts: Vec<u32> = layout_of(&workspace).iter().map(|c| c[0]).collect();
        assert_eq!(firsts, order, "{placement:?}");
    }
}

#[test]
fn a_one_based_placement_index_is_zero_based_and_out_of_range_falls_back_after_the_focused() {
    fresh();
    let mut workspace = workspace_of(&[1, 2, 3]);
    workspace.focus_container(0);
    workspace.initial_window_placement_rules = Some(InitialWindowPlacementRules::Target(
        PlacementTarget::Index(3),
    ));
    assert_eq!(workspace.resolve_placement_index(&w(9)), 2);

    // index 0 saturates to the first place
    workspace.initial_window_placement_rules = Some(InitialWindowPlacementRules::Target(
        PlacementTarget::Index(0),
    ));
    assert_eq!(workspace.resolve_placement_index(&w(9)), 0);

    // one more than the containers is the end, two more falls back to after the focused one
    workspace.initial_window_placement_rules = Some(InitialWindowPlacementRules::Target(
        PlacementTarget::Index(4),
    ));
    assert_eq!(workspace.resolve_placement_index(&w(9)), 3);
    workspace.initial_window_placement_rules = Some(InitialWindowPlacementRules::Target(
        PlacementTarget::Index(5),
    ));
    assert_eq!(workspace.resolve_placement_index(&w(9)), 1);
}

#[test]
fn with_no_rules_a_window_goes_after_the_focused_container() {
    fresh();
    let mut workspace = workspace_of(&[1, 2, 3]);
    workspace.focus_container(0);

    assert_eq!(workspace.resolve_placement_index(&w(9)), 1);
}

#[test]
fn inserting_past_the_end_appends_and_pushes_a_resize_slot() {
    fresh();
    let mut workspace = workspace_of(&[1, 2]);
    mark(&mut workspace, 2);

    let idx = workspace.insert_container_at_idx(9, container_of(&[3]));

    assert_eq!(idx, 2);
    assert_eq!(workspace.focused_container_idx(), 2);
    assert_eq!(workspace.resize_dimensions.len(), 3);
    assert!(workspace.resize_dimensions[2].is_none());
}

#[test]
fn a_locked_container_keeps_its_index_when_another_is_inserted_before_it() {
    fresh();
    let mut workspace = workspace_of(&[1, 2, 3]);
    workspace.containers_mut()[1].set_locked(true);

    let idx = workspace.insert_container_at_idx(0, container_of(&[9]));

    assert_eq!(layout_of(&workspace), [vec![9], vec![2], vec![1], vec![3]]);
    assert_eq!(idx, 0);
}

#[test]
fn removing_a_container_tells_the_scrolling_hook_first_and_drops_its_resize_slot() {
    fresh();
    let mut workspace = workspace_of(&[1, 2, 3]);
    mark(&mut workspace, 3);

    let removed = workspace.remove_container_by_idx(1).unwrap();

    assert_eq!(removed.windows()[0].id, 2);
    assert_eq!(log(), ["note_scrolling_departure 1"]);
    assert_eq!(workspace.resize_dimensions.len(), 2);
    assert_eq!(workspace.resize_dimensions[1].unwrap().left, 3);
}

#[test]
fn removing_past_the_end_removes_nothing() {
    fresh();
    let mut workspace = workspace_of(&[1, 2]);
    mark(&mut workspace, 2);

    assert!(workspace.remove_container_by_idx(7).is_none());

    assert_eq!(workspace.containers().len(), 2);
    assert_eq!(workspace.resize_dimensions.len(), 2);
}

#[test]
fn removing_a_container_focuses_the_one_before_the_focused_container_not_before_the_removed_one() {
    fresh();
    // the focus was on the last of four containers: whichever is removed, it moves to the index
    // before the old focus, which can name a different container than the one that was focused
    for removed in 0..4 {
        let mut workspace = workspace_of(&[1, 2, 3, 4]);

        workspace.remove_container(removed).unwrap();

        assert_eq!(workspace.focused_container_idx(), 2, "removed {removed}");
    }

    // with the focus on the first, it stays there
    let mut workspace = workspace_of(&[1, 2, 3, 4]);
    workspace.focus_container(0);
    workspace.remove_container(2).unwrap();
    assert_eq!(workspace.focused_container_idx(), 0);
}

#[test]
fn removing_the_focused_container_focuses_the_one_before_it_and_the_last_leaves_none() {
    fresh();
    let mut workspace = workspace_of(&[1, 2, 3]);
    workspace.focus_container(1);

    let removed = workspace.remove_focused_container().unwrap();
    assert_eq!(removed.windows()[0].id, 2);
    assert_eq!(workspace.focused_container_idx(), 0);

    workspace.remove_focused_container();
    workspace.remove_focused_container();
    assert!(workspace.containers().is_empty());
    assert_eq!(workspace.focused_container_idx(), 0);
    assert!(workspace.remove_focused_container().is_none());
}

#[test]
fn a_container_added_to_the_back_or_the_front_takes_the_focus() {
    fresh();
    let mut workspace = workspace_of(&[1, 2]);

    workspace.add_container_to_back(container_of(&[3]));
    assert_eq!(workspace.focused_container_idx(), 2);
    workspace.add_container_to_front(container_of(&[4]));
    assert_eq!(workspace.focused_container_idx(), 0);

    assert_eq!(layout_of(&workspace), [vec![4], vec![1], vec![2], vec![3]]);
}

#[test]
fn focusing_the_first_last_and_previous_container_on_an_empty_workspace_stays_at_zero() {
    fresh();
    let mut workspace = Workspace::<StubPlatform>::default();

    workspace.focus_last_container();
    assert_eq!(workspace.focused_container_idx(), 0);
    workspace.focus_previous_container();
    assert_eq!(workspace.focused_container_idx(), 0);
    workspace.focus_first_container();
    assert_eq!(workspace.focused_container_idx(), 0);
}

#[test]
fn swapping_containers_focuses_the_second_and_the_lock_stays_with_the_position() {
    fresh();
    let mut workspace = workspace_of(&[1, 2, 3]);
    workspace.containers_mut()[0].set_locked(true);

    workspace.swap_containers(0, 2);

    assert_eq!(layout_of(&workspace), [vec![3], vec![2], vec![1]]);
    assert_eq!(workspace.focused_container_idx(), 2);
    assert!(workspace.containers()[0].locked);
    assert!(!workspace.containers()[2].locked);
}

#[test]
fn cycling_wraps_around_and_an_empty_workspace_has_nowhere_to_cycle() {
    fresh();
    let mut workspace = workspace_of(&[1, 2, 3]);

    assert_eq!(
        workspace.new_idx_for_cycle_direction(CycleDirection::Next),
        Some(0)
    );
    workspace.focus_container(0);
    assert_eq!(
        workspace.new_idx_for_cycle_direction(CycleDirection::Previous),
        Some(2)
    );
    assert_eq!(
        Workspace::<StubPlatform>::default().new_idx_for_cycle_direction(CycleDirection::Next),
        None
    );
}

#[test]
fn directions_follow_the_layout_and_the_flip() {
    fresh();
    let mut workspace = workspace_of(&[1, 2]);
    workspace.focus_container(0);
    assert_eq!(
        workspace.new_idx_for_direction(OperationDirection::Right),
        Some(1)
    );
    assert_eq!(
        workspace.new_idx_for_direction(OperationDirection::Left),
        None
    );

    workspace.layout_flip = Some(komorebi_layouts::Axis::Horizontal);
    assert_eq!(
        workspace.new_idx_for_direction(OperationDirection::Left),
        Some(1)
    );
    assert_eq!(
        workspace.new_idx_for_direction(OperationDirection::Right),
        None
    );

    assert_eq!(
        Workspace::<StubPlatform>::default().new_idx_for_direction(OperationDirection::Left),
        None
    );
}

#[test]
fn stacking_moves_the_focused_window_in_front_of_the_target_and_hides_the_one_it_covers() {
    fresh();
    let mut workspace = workspace_of(&[1, 2, 3]);
    workspace.state.spot = 4;
    workspace.focus_container(2);

    workspace.move_window_to_container(0).unwrap();

    // container 2 is empty and goes; container 0 now holds 1 and 3, 3 in front
    assert_eq!(layout_of(&workspace), [vec![1, 3], vec![2]]);
    assert_eq!(workspace.focused_container_idx(), 0);
    assert_eq!(workspace.containers()[0].focused_window_idx(), 1);
    assert_eq!(
        log(),
        [
            "note_scrolling_departure 2",
            "hide 1 @4",
            "hide_stacked 1 @4",
            "restore_stacked 3",
        ]
    );
}

#[test]
fn stacking_onto_a_later_container_counts_its_index_after_the_emptied_one_went() {
    fresh();
    let mut workspace = workspace_of(&[1, 2, 3]);
    workspace.focus_container(0);

    // after container 0 is removed, the target 2 is the old container 3 at index 1
    workspace.move_window_to_container(2).unwrap();

    assert_eq!(layout_of(&workspace), [vec![2], vec![3, 1]]);
    assert_eq!(workspace.focused_container_idx(), 1);
}

#[test]
fn stacking_out_of_a_stack_shows_the_next_window_of_the_container_it_left() {
    fresh();
    let mut workspace = workspace_of(&[1, 2]);
    workspace.containers_mut()[0].add_window(w(5), 0).unwrap();
    workspace.focus_container(0);
    log();

    workspace.move_window_to_container(1).unwrap();

    assert_eq!(layout_of(&workspace), [vec![1], vec![2, 5]]);
    assert_eq!(
        log(),
        [
            "restore_stacked 1",
            "hide 2 @0",
            "hide_stacked 2 @0",
            "restore_stacked 5",
        ]
    );
}

#[test]
fn stacking_onto_a_container_that_does_not_exist_is_an_error_after_the_window_left() {
    fresh();
    let mut workspace = workspace_of(&[1, 2]);
    workspace.focus_container(1);

    let result = workspace.move_window_to_container(9);

    assert!(result.is_err());
    // the window is out of its container, which was removed: it is lost to the workspace
    assert_eq!(layout_of(&workspace), [vec![1]]);
}

#[test]
fn stacking_in_an_empty_workspace_is_an_error() {
    fresh();
    let mut workspace = Workspace::<StubPlatform>::default();

    assert!(workspace.move_window_to_container(0).is_err());
}

#[test]
fn unstacking_puts_the_focused_window_in_a_container_after_the_one_it_left() {
    fresh();
    let mut workspace = workspace_of(&[1, 2]);
    workspace.containers_mut()[0].add_window(w(5), 0).unwrap();
    workspace.focus_container(0);
    log();

    workspace.new_container_for_focused_window().unwrap();

    assert_eq!(layout_of(&workspace), [vec![1], vec![5], vec![2]]);
    assert_eq!(workspace.focused_container_idx(), 1);
    assert_eq!(log(), ["restore_stacked 1"]);
}

#[test]
fn unstacking_a_single_window_container_moves_nothing_but_still_succeeds() {
    fresh();
    let mut workspace = workspace_of(&[1, 2]);
    workspace.focus_container(0);

    workspace.new_container_for_focused_window().unwrap();

    // the container left empty is removed, the window gets a container after the focused one
    assert_eq!(workspace.containers().len(), 2);
    assert!(workspace.contains_window(1));
}

#[test]
fn promoting_puts_the_focused_container_first_and_focuses_it() {
    fresh();
    let mut workspace = workspace_of(&[1, 2, 3]);
    workspace.focus_container(1);

    workspace.promote_container().unwrap();

    assert_eq!(layout_of(&workspace), [vec![2], vec![1], vec![3]]);
    assert_eq!(workspace.focused_container_idx(), 0);
}

#[test]
fn promoting_with_no_container_is_an_error() {
    fresh();
    let mut workspace = Workspace::<StubPlatform>::default();

    assert!(workspace.promote_container().is_err());
}

#[test]
fn a_locked_container_keeps_its_place_when_another_is_promoted() {
    fresh();
    let mut workspace = workspace_of(&[1, 2, 3]);
    workspace.containers_mut()[0].set_locked(true);
    workspace.focus_container(2);

    workspace.promote_container().unwrap();

    assert_eq!(layout_of(&workspace)[0], [1]);
}

#[test]
fn windows_are_found_in_containers_monocle_maximized_and_floating() {
    fresh();
    let mut workspace = workspace_of(&[1, 2]);
    workspace.monocle_container = Some(container_of(&[3]));
    workspace.maximized_window = Some(w(4));
    workspace.floating_windows_mut().push_back(w(5));

    for id in [1, 2, 3, 4, 5] {
        assert!(workspace.contains_window(id), "{id}");
    }
    // the floating window is not one that komorebi tiles
    for id in [1, 2, 3, 4] {
        assert!(workspace.contains_managed_window(id), "{id}");
    }
    assert!(!workspace.contains_managed_window(5));
    assert!(!workspace.contains_window(6));
    assert!(!workspace.is_empty());
}

#[test]
fn a_workspace_with_only_a_floating_window_is_not_empty() {
    fresh();
    let mut workspace = Workspace::<StubPlatform>::default();
    workspace.floating_windows_mut().push_back(w(5));

    assert!(!workspace.is_empty());
}

#[test]
fn the_location_of_an_executable_is_looked_for_in_the_order_of_the_layers() {
    use komorebi_core::WorkspaceWindowLocation as Location;
    fresh();
    let mut workspace = workspace_of(&[1, 2]);
    workspace.maximized_window = Some(w(4));
    workspace.monocle_container = Some(container_of(&[3]));
    workspace.floating_windows_mut().push_back(w(5));

    assert!(matches!(
        workspace.location_from_exe("exe2"),
        Some(Location::Container(1, 0))
    ));
    assert!(matches!(
        workspace.location_from_exe("exe4"),
        Some(Location::Maximized)
    ));
    assert!(matches!(
        workspace.location_from_exe("exe3"),
        Some(Location::Monocle(0))
    ));
    assert!(matches!(
        workspace.location_from_exe("exe5"),
        Some(Location::Floating(0))
    ));
    assert!(workspace.location_from_exe("nothing").is_none());
}

#[test]
fn the_last_container_that_has_a_window_is_the_one_found_for_it() {
    fresh();
    let mut workspace = workspace_of(&[1, 2]);
    workspace.containers_mut()[0].add_window(w(2), 0).unwrap();

    assert_eq!(workspace.container_idx_for_window(2), Some(1));
}

#[test]
fn focusing_by_window_brings_a_stacked_window_to_the_front_and_loads_the_stack() {
    fresh();
    let mut workspace = workspace_of(&[1, 2]);
    workspace.state.spot = 6;
    workspace.containers_mut()[0].add_window(w(5), 6).unwrap();
    workspace.containers_mut()[0].focus_window(1);
    log();

    workspace.focus_container_by_window(1).unwrap();

    assert_eq!(workspace.focused_container_idx(), 0);
    assert_eq!(workspace.containers()[0].focused_window_idx(), 0);
    assert_eq!(log(), ["restore_stacked 1", "hide_stacked 5 @6"]);
}

#[test]
fn focusing_by_a_window_already_in_front_loads_nothing() {
    fresh();
    let mut workspace = workspace_of(&[1, 2]);

    workspace.focus_container_by_window(1).unwrap();

    assert_eq!(workspace.focused_container_idx(), 0);
    assert!(log().is_empty());
}

#[test]
fn focusing_by_an_unknown_window_is_an_error_and_changes_nothing() {
    fresh();
    let mut workspace = workspace_of(&[1, 2]);

    assert!(workspace.focus_container_by_window(9).is_err());
    assert_eq!(workspace.focused_container_idx(), 1);
}

#[test]
fn the_pointer_finds_the_container_whose_last_laid_out_tile_it_is_in() {
    fresh();
    let mut workspace = workspace_of(&[1, 2]);
    workspace.latest_layout = vec![
        Rect {
            left: 0,
            top: 0,
            right: 100,
            bottom: 100,
        },
        Rect {
            left: 100,
            top: 0,
            right: 100,
            bottom: 100,
        },
    ];

    assert_eq!(workspace.container_idx_from_current_point(), None);
    set_cursor(Some((150, 50)));
    assert_eq!(workspace.container_idx_from_current_point(), Some(1));
    set_cursor(Some((50, 50)));
    assert_eq!(workspace.container_idx_from_current_point(), Some(0));
    set_cursor(Some((500, 50)));
    assert_eq!(workspace.container_idx_from_current_point(), None);
}

#[test]
fn the_visible_windows_are_maximized_then_monocle_then_each_container_then_floating() {
    fresh();
    let mut workspace = workspace_of(&[1, 2]);
    workspace.containers_mut()[0].add_window(w(5), 0).unwrap();
    workspace.maximized_window = Some(w(4));
    workspace.monocle_container = Some(container_of(&[3]));
    workspace.floating_windows_mut().push_back(w(6));

    let ids: Vec<Option<u32>> = workspace
        .visible_windows()
        .iter()
        .map(|w| w.map(|w| w.id))
        .collect();

    assert_eq!(ids, [Some(4), Some(3), Some(5), Some(2), Some(6)]);
}

#[test]
fn without_a_maximized_window_the_first_visible_entry_is_none() {
    fresh();
    let workspace = workspace_of(&[1]);

    let ids: Vec<Option<u32>> = workspace
        .visible_windows()
        .iter()
        .map(|w| w.map(|w| w.id))
        .collect();

    assert_eq!(ids, [None, Some(1)]);
}

#[test]
fn only_the_default_layouts_are_the_scrolling_layout_when_it_is_scrolling() {
    fresh();
    let mut workspace = workspace_of(&[1]);
    assert!(!workspace.is_scrolling());

    workspace.layout = Layout::Default(DefaultLayout::Scrolling);
    assert!(workspace.is_scrolling());
}

#[test]
fn a_placement_rule_that_matches_the_window_decides_where_it_goes() {
    use komorebi_config::placement::InitialWindowPlacementRules;
    fresh();
    let rules = |json: serde_json::Value| -> InitialWindowPlacementRules {
        serde_json::from_value(json).unwrap()
    };
    let single = serde_json::json!({ "Last": { "kind": "Exe", "id": "exe9" } });
    let many = serde_json::json!({ "Last": [{ "kind": "Exe", "id": "other" }, { "kind": "Exe", "id": "exe9" }] });
    let secondary = serde_json::json!({ "Secondary": { "kind": "Exe", "id": "exe9" } });

    for (json, expected) in [(single, 3), (many, 3), (secondary, 1)] {
        let mut workspace = workspace_of(&[1, 2, 3]);
        workspace.focus_container(0);
        workspace.initial_window_placement_rules = Some(rules(json.clone()));

        // no rule matches this window: after the focused container
        assert_eq!(workspace.resolve_placement_index(&w(9)), 1, "{json}");

        set_match_id("exe9");
        assert_eq!(workspace.resolve_placement_index(&w(9)), expected, "{json}");
        fresh();
    }
}

#[test]
fn the_hooks_a_platform_does_not_override_do_nothing_to_the_workspace() {
    fresh();
    let mut workspace = workspace_of(&[1, 2]);
    workspace.floating_windows_mut().push_back(w(5));
    workspace.floating_windows_mut().push_back(w(6));
    workspace.focus_floating_window(1);

    // the stub overrides before_floating_removed only to log: the focus index is the workspace's
    // own business, and the default platform of the other tests keeps it
    workspace.remove_floating_window(6);
    assert_eq!(workspace.focused_floating_window_idx(), 1);
}

#[test]
fn the_json_keys_of_a_default_workspace_are_the_ones_every_daemon_prints() {
    fresh();
    let value = serde_json::to_value(Workspace::<StubPlatform>::default()).unwrap();
    let mut keys: Vec<&str> = value
        .as_object()
        .unwrap()
        .keys()
        .map(String::as_str)
        .collect();
    keys.sort_unstable();

    assert_eq!(
        keys,
        [
            "apply_window_based_work_area_offset",
            "container_padding",
            "containers",
            "float_override",
            "floating_layer_behaviour",
            "floating_windows",
            "latest_layout",
            "layer",
            "layout",
            "layout_flip",
            "layout_options",
            "layout_options_rules",
            "layout_rules",
            "maximized_window",
            "monocle_container",
            "name",
            "resize_dimensions",
            "tile",
            "wallpaper",
            "window_container_behaviour",
            "window_container_behaviour_rules",
            "work_area_offset",
            "work_area_offset_rules",
            "workspace_padding",
        ]
    );
}
