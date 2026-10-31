//! Characterization of the windows a workspace keeps outside its tiles (the monocle container, the
//! maximized window, the floating windows), the layer the focus commands act on, and hiding.

mod common;

use common::*;
use komorebi_core::Container;
use komorebi_core::Workspace;
use komorebi_core::WorkspaceLayer;
use komorebi_layouts::CycleDirection;

fn w(id: u32) -> StubWindow {
    StubWindow { id }
}

// ---- monocle ----------------------------------------------------------------------------------

#[test]
fn toggling_monocle_takes_the_focused_container_out_and_remembers_its_index() {
    fresh();
    let mut workspace = workspace_of(&[1, 2, 3]);
    workspace.focus_container(1);

    workspace.new_monocle_container().unwrap();

    assert_eq!(layout_of(&workspace), [vec![1], vec![3]]);
    assert_eq!(
        workspace.monocle_container.as_ref().unwrap().windows()[0].id,
        2
    );
    assert_eq!(workspace.monocle_container_restore_idx, Some(1));
    // the focus goes to the container before the index it was taken from
    assert_eq!(workspace.focused_container_idx(), 0);
}

#[test]
fn toggling_monocle_loads_the_stack_it_took_at_the_hide_spot() {
    fresh();
    let mut workspace = workspace_of(&[1, 2]);
    workspace.state.spot = 8;
    workspace.containers_mut()[1].add_window(w(3), 8).unwrap();
    log();

    workspace.new_monocle_container().unwrap();

    assert_eq!(log(), ["hide_stacked 2 @8", "restore_stacked 3"]);
}

#[test]
fn monocle_on_an_empty_workspace_is_an_error_and_sets_nothing() {
    fresh();
    let mut workspace = Workspace::<StubPlatform>::default();

    assert!(workspace.new_monocle_container().is_err());
    assert!(workspace.monocle_container.is_none());
    assert_eq!(workspace.monocle_container_restore_idx, None);
}

#[test]
fn monocle_leaves_the_resize_adjustments_alone_so_the_container_returns_at_its_size() {
    fresh();
    let mut workspace = workspace_of(&[1, 2]);
    workspace.resize_dimensions = vec![None, None];

    workspace.new_monocle_container().unwrap();

    assert_eq!(workspace.resize_dimensions.len(), 2);
}

#[test]
fn reintegrating_monocle_puts_the_container_back_at_its_index_and_focuses_it() {
    fresh();
    let mut workspace = workspace_of(&[1, 2, 3]);
    workspace.focus_container(1);
    workspace.new_monocle_container().unwrap();
    log();

    workspace.reintegrate_monocle_container().unwrap();

    assert_eq!(layout_of(&workspace), [vec![1], vec![2], vec![3]]);
    assert_eq!(workspace.focused_container_idx(), 1);
    assert!(workspace.monocle_container.is_none());
    assert_eq!(workspace.monocle_container_restore_idx, None);
    assert_eq!(log(), ["restore_stacked 2"]);
}

#[test]
fn reintegrating_without_monocle_is_an_error() {
    fresh();
    let mut workspace = workspace_of(&[1, 2]);

    assert!(workspace.reintegrate_monocle_container().is_err());
    assert_eq!(workspace.containers().len(), 2);
}

#[test]
fn reintegrating_at_an_index_past_the_end_pads_with_empty_containers() {
    fresh();
    let mut workspace = workspace_of(&[1, 2, 3]);
    workspace.focus_container(2);
    workspace.new_monocle_container().unwrap();
    // the other containers went away while it was monocle
    workspace.containers_mut().truncate(1);

    workspace.reintegrate_monocle_container().unwrap();

    // restore index 2 with one container: one empty container pads the gap
    assert_eq!(layout_of(&workspace), [vec![1], vec![], vec![3]]);
    assert_eq!(workspace.focused_container_idx(), 2);
}

#[test]
fn cycling_monocle_hands_the_monocle_to_the_next_container_and_wraps() {
    fresh();
    let mut workspace = workspace_of(&[1, 2, 3]);
    workspace.focus_container(2);
    workspace.new_monocle_container().unwrap();

    workspace
        .cycle_monocle_container(CycleDirection::Next)
        .unwrap();

    // 3 went back at index 2 and focus moved to the next after it, wrapping to 0
    assert_eq!(
        workspace.monocle_container.as_ref().unwrap().windows()[0].id,
        1
    );
    assert_eq!(layout_of(&workspace), [vec![2], vec![3]]);
    assert_eq!(workspace.monocle_container_restore_idx, Some(0));
}

#[test]
fn cycling_monocle_with_no_other_container_leaves_the_workspace_as_it_is() {
    fresh();
    let mut workspace = workspace_of(&[1]);
    workspace.new_monocle_container().unwrap();

    workspace
        .cycle_monocle_container(CycleDirection::Next)
        .unwrap();

    assert!(workspace.monocle_container.is_some());
    assert!(workspace.containers().is_empty());
}

#[test]
fn monocle_and_maximized_windows_are_reported_for_the_focused_window() {
    fresh();
    let mut workspace = workspace_of(&[1, 2]);

    assert!(workspace.is_focused_window_monocle_or_maximized().is_err());
    set_foreground(Some(1));
    assert!(!workspace.is_focused_window_monocle_or_maximized().unwrap());

    workspace.monocle_container = Some(container_of(&[1]));
    assert!(workspace.is_focused_window_monocle_or_maximized().unwrap());

    workspace.monocle_container = None;
    workspace.maximized_window = Some(w(1));
    assert!(workspace.is_focused_window_monocle_or_maximized().unwrap());
}

// ---- floating windows -------------------------------------------------------------------------

#[test]
fn floating_the_focused_window_of_a_stack_leaves_the_container_with_its_other_windows() {
    fresh();
    let mut workspace = workspace_of(&[1, 2]);
    workspace.state.spot = 2;
    workspace.containers_mut()[1].add_window(w(3), 2).unwrap();
    log();

    workspace.new_floating_window().unwrap();

    assert_eq!(layout_of(&workspace), [vec![1], vec![2]]);
    assert_eq!(workspace.floating_windows().len(), 1);
    assert_eq!(workspace.floating_windows()[0].id, 3);
    assert_eq!(log(), ["restore_stacked 2"]);
}

#[test]
fn floating_the_only_window_of_the_last_container_focuses_the_new_last_container() {
    fresh();
    let mut workspace = workspace_of(&[1, 2, 3]);

    workspace.new_floating_window().unwrap();

    assert_eq!(layout_of(&workspace), [vec![1], vec![2]]);
    assert_eq!(workspace.focused_container_idx(), 1);
}

#[test]
fn floating_the_only_window_of_a_middle_container_keeps_the_focus_index() {
    fresh();
    let mut workspace = workspace_of(&[1, 2, 3]);
    workspace.focus_container(1);

    workspace.new_floating_window().unwrap();

    assert_eq!(layout_of(&workspace), [vec![1], vec![3]]);
    assert_eq!(workspace.focused_container_idx(), 1);
    assert_eq!(log(), ["note_scrolling_departure 1"]);
}

#[test]
fn floating_the_maximized_window_unmaximizes_it_first_and_clears_the_maximized_state() {
    fresh();
    let mut workspace = workspace_of(&[1]);
    workspace.maximized_window = Some(w(4));
    workspace.maximized_window_restore_idx = Some(0);

    workspace.new_floating_window().unwrap();

    assert_eq!(log(), ["unmaximize 4"]);
    assert!(workspace.maximized_window.is_none());
    assert_eq!(workspace.maximized_window_restore_idx, None);
    assert_eq!(workspace.floating_windows()[0].id, 4);
    assert_eq!(workspace.containers().len(), 1);
}

#[test]
fn a_maximized_window_that_cannot_be_unmaximized_stays_maximized() {
    fresh();
    let mut workspace = workspace_of(&[1]);
    workspace.maximized_window = Some(w(4));
    fail("unmaximize 4");

    assert!(workspace.new_floating_window().is_err());

    assert!(workspace.maximized_window.is_some());
    assert!(workspace.floating_windows().is_empty());
}

#[test]
fn floating_the_monocle_window_drops_an_emptied_monocle_container() {
    fresh();
    let mut workspace = workspace_of(&[1]);
    workspace.monocle_container = Some(container_of(&[5]));
    workspace.monocle_container_restore_idx = Some(0);

    workspace.new_floating_window().unwrap();

    assert!(workspace.monocle_container.is_none());
    assert_eq!(workspace.monocle_container_restore_idx, None);
    assert_eq!(workspace.floating_windows()[0].id, 5);
}

#[test]
fn floating_the_monocle_window_of_a_stack_keeps_the_monocle_and_shows_the_next() {
    fresh();
    let mut workspace = workspace_of(&[1]);
    workspace.monocle_container = Some(container_of(&[5, 6]));
    log();

    workspace.new_floating_window().unwrap();

    assert_eq!(
        workspace
            .monocle_container
            .as_ref()
            .unwrap()
            .windows()
            .len(),
        1
    );
    assert_eq!(log(), ["restore_stacked 5"]);
}

#[test]
fn floating_with_nothing_to_float_is_an_error() {
    fresh();
    let mut workspace = Workspace::<StubPlatform>::default();

    assert!(workspace.new_floating_window().is_err());
}

#[test]
fn a_floating_window_is_removed_by_id_and_the_hook_runs_before_it_goes() {
    fresh();
    let mut workspace = workspace_of(&[1]);
    workspace.floating_windows_mut().push_back(w(7));
    workspace.floating_windows_mut().push_back(w(8));

    let removed = workspace.remove_floating_window(8).unwrap();

    assert_eq!(removed.id, 8);
    assert_eq!(log(), ["before_floating_removed len=2 focus=0"]);
    assert!(workspace.remove_floating_window(8).is_none());
    // nothing is logged for a window that is not there
    assert!(log().is_empty());
}

#[test]
fn the_focused_floating_window_is_the_one_the_platform_says_has_the_focus() {
    fresh();
    let mut workspace = workspace_of(&[1]);
    workspace.floating_windows_mut().push_back(w(7));

    assert!(workspace.remove_focused_floating_window().is_none());
    set_foreground(Some(7));
    assert_eq!(workspace.remove_focused_floating_window().unwrap().id, 7);
}

#[test]
fn a_floating_window_returns_to_the_layout_in_a_container_at_the_focused_index() {
    fresh();
    let mut workspace = workspace_of(&[1, 2]);
    workspace.focus_container(0);
    workspace.floating_windows_mut().push_back(w(7));
    set_foreground(Some(7));

    workspace.new_container_for_floating_window().unwrap();

    assert_eq!(layout_of(&workspace), [vec![7], vec![1], vec![2]]);
    assert_eq!(workspace.focused_container_idx(), 0);
    assert!(workspace.floating_windows().is_empty());
}

#[test]
fn a_window_that_is_not_floating_cannot_return_to_the_layout() {
    fresh();
    let mut workspace = workspace_of(&[1, 2]);
    set_foreground(Some(1));

    assert!(workspace.new_container_for_floating_window().is_err());
    assert_eq!(workspace.containers().len(), 2);
}

#[test]
fn the_floating_focus_moves_to_the_previous_window_and_stops_at_the_first() {
    fresh();
    let mut workspace = Workspace::<StubPlatform>::default();
    for id in [1, 2, 3] {
        workspace.floating_windows_mut().push_back(w(id));
    }
    workspace.focus_floating_window(2);

    workspace.focus_previous_floating_window();
    assert_eq!(workspace.focused_floating_window_idx(), 1);
    workspace.focus_previous_floating_window();
    workspace.focus_previous_floating_window();
    assert_eq!(workspace.focused_floating_window_idx(), 0);
    assert_eq!(workspace.focused_floating_window().map(|w| w.id), Some(1));
}

// ---- layer ------------------------------------------------------------------------------------

#[test]
fn a_workspace_starts_on_its_tiling_layer() {
    let workspace = Workspace::<StubPlatform>::default();

    assert_eq!(workspace.layer, WorkspaceLayer::Tiling);
}

#[test]
fn the_layers_are_named_tiling_and_floating_in_text_and_json() {
    assert_eq!(WorkspaceLayer::Tiling.to_string(), "Tiling");
    assert_eq!(WorkspaceLayer::Floating.to_string(), "Floating");
    assert_eq!(
        serde_json::to_string(&WorkspaceLayer::Floating).unwrap(),
        "\"Floating\""
    );
    assert_eq!(
        serde_json::from_str::<WorkspaceLayer>("\"Tiling\"").unwrap(),
        WorkspaceLayer::Tiling
    );
}

#[test]
fn the_layer_is_a_field_of_the_workspace_that_the_shared_code_never_changes() {
    fresh();
    let mut workspace = workspace_of(&[1, 2]);
    workspace.layer = WorkspaceLayer::Floating;

    workspace.new_floating_window().unwrap();
    workspace.new_container_for_window(w(9)).unwrap();
    workspace.new_monocle_container().unwrap();
    workspace.reintegrate_monocle_container().unwrap();

    assert_eq!(workspace.layer, WorkspaceLayer::Floating);
}

// ---- hiding -----------------------------------------------------------------------------------

#[test]
fn hiding_a_workspace_goes_floating_then_containers_then_maximized_then_monocle() {
    fresh();
    let mut workspace = workspace_of(&[1, 2]);
    workspace.state.spot = 3;
    workspace.containers_mut()[0].add_window(w(5), 3).unwrap();
    workspace.floating_windows_mut().push_back(w(7));
    workspace.floating_windows_mut().push_back(w(8));
    workspace.maximized_window = Some(w(9));
    workspace.monocle_container = Some(container_of(&[10, 11]));
    log();

    workspace.hide(None).unwrap();

    // floating windows from the back at the window spot; the containers at the display spot, each
    // from its last window; the maximized window at the window spot; the monocle container at the
    // display spot
    assert_eq!(
        log(),
        [
            "hide 8 @3",
            "hide 7 @3",
            "before_hide_containers",
            "hide 5 @103",
            "hide 1 @103",
            "hide 2 @103",
            "hide 9 @3",
            "hide 11 @103",
            "hide 10 @103",
        ]
    );
}

#[test]
fn hiding_but_one_window_spares_it_in_a_container_the_floating_layer_and_the_monocle() {
    fresh();
    let mut workspace = workspace_of(&[1, 2]);
    workspace.floating_windows_mut().push_back(w(7));
    workspace.monocle_container = Some(container_of(&[10]));
    log();

    workspace.hide(Some(2)).unwrap();
    assert_eq!(
        log(),
        [
            "hide 7 @0",
            "before_hide_containers",
            "hide 1 @100",
            "hide 10 @100"
        ]
    );

    workspace.hide(Some(7)).unwrap();
    assert_eq!(
        log(),
        [
            "before_hide_containers",
            "hide 1 @100",
            "hide 2 @100",
            "hide 10 @100"
        ]
    );
}

#[test]
fn a_floating_window_that_will_not_hide_is_skipped_and_the_rest_are_hidden() {
    fresh();
    let mut workspace = workspace_of(&[1]);
    workspace.floating_windows_mut().push_back(w(7));
    fail("hide 7");
    log();

    assert!(workspace.hide(None).is_ok());

    assert_eq!(
        log(),
        ["hide 7 @0", "before_hide_containers", "hide 1 @100"]
    );
}

#[test]
fn a_container_window_that_will_not_hide_is_skipped_and_the_rest_are_hidden() {
    fresh();
    let mut workspace = workspace_of(&[1, 2]);
    fail("hide 2");
    log();

    assert!(workspace.hide(None).is_ok());

    assert_eq!(
        log(),
        ["before_hide_containers", "hide 1 @100", "hide 2 @100"]
    );
}

#[test]
fn a_maximized_window_that_will_not_hide_is_an_error_and_the_monocle_is_not_reached() {
    fresh();
    let mut workspace = workspace_of(&[1]);
    workspace.maximized_window = Some(w(9));
    workspace.monocle_container = Some(container_of(&[10]));
    fail("hide 9");
    log();

    assert!(workspace.hide(None).is_err());

    assert_eq!(
        log(),
        ["before_hide_containers", "hide 1 @100", "hide 9 @0"]
    );
}

#[test]
fn restoring_the_focused_container_shows_only_its_front_window() {
    fresh();
    let mut workspace = workspace_of(&[1, 2]);
    workspace.containers_mut()[1].add_window(w(3), 0).unwrap();
    log();

    workspace.focused_container().unwrap().restore().unwrap();

    assert_eq!(log(), ["restore 3"]);
}

#[test]
fn an_empty_container_among_the_containers_is_hidden_without_a_call() {
    fresh();
    let mut workspace = workspace_of(&[1]);
    workspace.containers_mut().push_back(Container::default());
    log();

    workspace.hide(None).unwrap();

    assert_eq!(log(), ["before_hide_containers", "hide 1 @100"]);
}

#[test]
fn the_floating_removal_hook_runs_before_the_window_is_taken_out() {
    fresh();
    let mut workspace = Workspace::<StubPlatform>::default();
    workspace.floating_windows_mut().push_back(w(1));
    workspace.floating_windows_mut().push_back(w(2));
    workspace.focus_floating_window(1);

    assert_eq!(workspace.remove_floating_window(2).map(|w| w.id), Some(2));

    // pins today's order (the KDE and macOS order): the hook sees both windows and the old
    // focus, although the trait doc says "after a floating window was taken out"
    assert_eq!(log(), ["before_floating_removed len=2 focus=1"]);
}
