//! Characterization of `Workspace::update_shared`: the order of the hook calls and their
//! arguments for every kind of workspace, and what each hook's failure does to the update.
//!
//! The work area is 1000 x 800 with a workspace padding of 20 and a container padding of 10 unless
//! a test says otherwise, so a BSP workspace of two containers has tiles
//! `[30 30 460 740]` and `[510 30 460 740]` (rects are left, top, width, height).

mod common;

use common::*;
use komorebi_config::behaviour::WindowContainerBehaviour;
use komorebi_core::Container;
use komorebi_core::Pass;
use komorebi_core::Workspace;
use komorebi_layouts::DefaultLayout;
use komorebi_layouts::Layout;
use komorebi_layouts::Rect;

fn w(id: u32) -> StubWindow {
    StubWindow { id }
}

fn on_screen(ids: &[u32]) -> Workspace<StubPlatform> {
    let mut workspace = workspace_of(ids);
    workspace.globals.work_area = work_area(1000, 800);
    workspace.globals.container_padding = Some(10);
    workspace.globals.workspace_padding = Some(20);
    workspace
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
fn a_tiling_workspace_calls_the_hooks_in_order_and_holds_the_guard_to_the_end() {
    fresh();
    let mut workspace = on_screen(&[1, 2]);

    workspace.update_shared().unwrap();

    assert_eq!(
        log(),
        [
            "begin_update",
            "containers_pruned focused_before=1",
            "adjust_layout (1) [30 30 460 740] scrolling=false maximized=false resize_empty=false",
            "place_container (1) [30 30 460 740] hidden=false padding=10 border=0+0",
            "adjust_layout (2) [510 30 460 740] scrolling=false maximized=false resize_empty=false",
            "place_container (2) [510 30 460 740] hidden=false padding=10 border=0+0",
            "end_update pass=0 around=10 viewport=960",
            "guard dropped",
        ]
    );
}

#[test]
fn the_borders_are_in_the_placement_and_in_the_around_window_width() {
    fresh();
    let mut workspace = on_screen(&[1]);
    workspace.globals.border_offset = 3;
    workspace.globals.border_width = 2;

    workspace.update_shared().unwrap();

    let log = log();
    assert!(log.contains(
        &"place_container (1) [30 30 940 740] hidden=false padding=10 border=3+2".to_string()
    ));
    assert!(log.contains(&"end_update pass=0 around=15 viewport=960".to_string()));
}

#[test]
fn the_layout_the_hooks_got_is_kept_as_the_latest_layout_after_the_adjustment() {
    fresh();
    let mut workspace = on_screen(&[1, 2]);
    set_adjust(2);

    workspace.update_shared().unwrap();

    assert_eq!(
        workspace.latest_layout,
        [rect(32, 32, 456, 736), rect(512, 32, 456, 736)]
    );
    let log = log();
    assert!(
        log.iter()
            .any(|l| l.starts_with("place_container (1) [32 32 456 736]"))
    );
}

#[test]
fn a_monocle_container_gets_the_work_area_less_every_padding_and_nothing_else_is_placed() {
    fresh();
    let mut workspace = on_screen(&[1, 2]);
    workspace.globals.border_offset = 3;
    workspace.globals.border_width = 2;
    workspace.new_monocle_container().unwrap();
    log();

    workspace.update_shared().unwrap();

    // 20 workspace padding, then 10 + 3 + 2 more on each side
    assert_eq!(
        log(),
        [
            "begin_update",
            "containers_pruned focused_before=0",
            "place_monocle 2 [35 35 930 730]",
            "guard dropped",
        ]
    );
}

#[test]
fn a_maximized_window_is_laid_out_by_the_platform_and_the_containers_are_left_alone() {
    fresh();
    let mut workspace = on_screen(&[1, 2]);
    workspace.maximized_window = Some(w(9));

    workspace.update_shared().unwrap();

    assert_eq!(
        log(),
        [
            "begin_update",
            "containers_pruned focused_before=1",
            "maximize_in_layout 9",
            "guard dropped",
        ]
    );
    assert!(workspace.latest_layout.is_empty());
}

#[test]
fn the_monocle_container_wins_over_a_maximized_window() {
    fresh();
    let mut workspace = on_screen(&[1, 2]);
    workspace.new_monocle_container().unwrap();
    workspace.maximized_window = Some(w(9));
    log();

    workspace.update_shared().unwrap();

    let log = log();
    assert!(log.iter().any(|l| l.starts_with("place_monocle 2")));
    assert!(!log.iter().any(|l| l.starts_with("maximize_in_layout")));
}

#[test]
fn an_empty_workspace_prunes_and_places_nothing() {
    fresh();
    let mut workspace = on_screen(&[]);

    workspace.update_shared().unwrap();

    assert_eq!(
        log(),
        [
            "begin_update",
            "containers_pruned focused_before=0",
            "guard dropped"
        ]
    );
    assert!(workspace.resize_dimensions.is_empty());
}

#[test]
fn a_workspace_that_is_not_tiling_prunes_and_places_nothing() {
    fresh();
    let mut workspace = on_screen(&[1, 2]);
    workspace.tile = false;
    workspace.resize_dimensions = vec![None; 5];

    workspace.update_shared().unwrap();

    assert_eq!(
        log(),
        [
            "begin_update",
            "containers_pruned focused_before=1",
            "guard dropped"
        ]
    );
    // the resize adjustments are still brought to the number of containers
    assert_eq!(workspace.resize_dimensions.len(), 2);
}

#[test]
fn a_skipped_update_calls_nothing_after_begin_update_and_changes_nothing() {
    fresh();
    let mut workspace = on_screen(&[1, 2]);
    workspace.resize_dimensions = vec![None; 5];
    skip_updates();

    workspace.update_shared().unwrap();

    assert_eq!(log(), ["begin_update"]);
    assert_eq!(workspace.resize_dimensions.len(), 5);
}

#[test]
fn a_failing_begin_update_is_the_error_of_the_update_and_nothing_else_runs() {
    fresh();
    let mut workspace = on_screen(&[1, 2]);
    fail("begin_update");

    assert!(workspace.update_shared().is_err());

    assert_eq!(log(), ["begin_update"]);
}

#[test]
fn empty_containers_are_dropped_but_a_preselected_one_stays() {
    fresh();
    let mut workspace = on_screen(&[1, 3]);
    workspace.containers_mut().insert(1, Container::default());
    workspace.focus_container(2);
    workspace.containers_mut().push_back(Container::preselect());
    workspace.update_shared().unwrap();

    let log = log();
    assert!(log.contains(&"containers_pruned focused_before=2".to_string()));
    assert_eq!(workspace.containers().len(), 3);
    assert!(workspace.containers()[2].is_preselect());
    // the shared code leaves the focus where it was: the hook is for the platform to fix it
    assert_eq!(workspace.focused_container_idx(), 2);
}

#[test]
fn end_update_asking_again_lays_the_workspace_out_and_places_the_windows_once_more() {
    fresh();
    let mut workspace = on_screen(&[1, 2]);
    script_passes(&[Pass::Again, Pass::Again, Pass::Done]);

    workspace.update_shared().unwrap();

    let log = log();
    let ends: Vec<&String> = log.iter().filter(|l| l.starts_with("end_update")).collect();
    assert_eq!(
        ends,
        [
            "end_update pass=0 around=10 viewport=960",
            "end_update pass=1 around=10 viewport=960",
            "end_update pass=2 around=10 viewport=960",
        ]
    );
    assert_eq!(
        log.iter()
            .filter(|l| l.starts_with("place_container"))
            .count(),
        6
    );
    assert_eq!(
        log.iter()
            .filter(|l| l.starts_with("adjust_layout"))
            .count(),
        6
    );
    // begin_update and the pruning run once, however many passes there are
    assert_eq!(log.iter().filter(|l| *l == "begin_update").count(), 1);
    assert_eq!(
        log.iter()
            .filter(|l| l.starts_with("containers_pruned"))
            .count(),
        1
    );
    assert_eq!(log.last().unwrap(), "guard dropped");
}

#[test]
fn a_failing_place_container_stops_the_update_before_end_update_and_the_bookkeeping() {
    fresh();
    let mut workspace = on_screen(&[1, 2]);
    workspace.resize_dimensions = vec![None; 5];
    fail("place_container");

    assert!(workspace.update_shared().is_err());

    // the first container failed: the second was not tried, end_update was not called, the
    // guard was still dropped, and neither the latest layout nor the resize vector was set
    assert_eq!(
        log(),
        [
            "begin_update",
            "containers_pruned focused_before=1",
            "adjust_layout (1) [30 30 460 740] scrolling=false maximized=false resize_empty=false",
            "place_container (1) [30 30 460 740] hidden=false padding=10 border=0+0",
            "guard dropped",
        ]
    );
    assert!(workspace.latest_layout.is_empty());
    assert_eq!(workspace.resize_dimensions.len(), 5);
}

#[test]
fn a_failing_place_monocle_is_the_error_of_the_update() {
    fresh();
    let mut workspace = on_screen(&[1, 2]);
    workspace.new_monocle_container().unwrap();
    workspace.resize_dimensions = vec![None; 5];
    fail("place_monocle");

    assert!(workspace.update_shared().is_err());

    assert_eq!(workspace.resize_dimensions.len(), 5);
    assert_eq!(log().last().unwrap(), "guard dropped");
}

#[test]
fn a_failing_maximize_in_layout_is_the_error_of_the_update() {
    fresh();
    let mut workspace = on_screen(&[1]);
    workspace.maximized_window = Some(w(9));
    fail("maximize_in_layout");

    assert!(workspace.update_shared().is_err());
}

#[test]
fn a_failing_end_update_is_the_error_of_the_update_after_the_layout_was_kept() {
    fresh();
    let mut workspace = on_screen(&[1, 2]);
    workspace.resize_dimensions = vec![None; 5];
    fail("end_update");

    assert!(workspace.update_shared().is_err());

    assert_eq!(workspace.latest_layout.len(), 2);
    assert_eq!(workspace.resize_dimensions.len(), 5);
}

#[test]
fn a_scrolling_workspace_asks_the_platform_for_its_tiles_and_which_rest_hidden() {
    fresh();
    let mut workspace = on_screen(&[1, 2]);
    workspace.layout = Layout::Default(DefaultLayout::Scrolling);
    set_scrolling_hidden(&[false, true]);

    workspace.update_shared().unwrap();

    assert_eq!(
        log(),
        [
            "begin_update",
            "containers_pruned focused_before=1",
            "adopt_widths_before_layout",
            "scrolling_layouts [20 20 960 760] padding=10",
            "adjust_layout (1) [20 20 480 760] scrolling=true maximized=false resize_empty=false",
            "place_container (1) [20 20 480 760] hidden=false padding=10 border=0+0",
            "adjust_layout (2) [500 20 480 760] scrolling=true maximized=false resize_empty=false",
            "place_container (2) [500 20 480 760] hidden=true padding=10 border=0+0",
            "end_update pass=0 around=10 viewport=960",
            "guard dropped",
        ]
    );
}

#[test]
fn a_scrolling_workspace_adopts_the_widths_the_windows_have_when_the_platform_says_so() {
    fresh();
    let mut workspace = on_screen(&[1]);
    workspace.layout = Layout::Default(DefaultLayout::Scrolling);
    adopt_widths(true);

    workspace.update_shared().unwrap();

    let log = log();
    let at = |prefix: &str| log.iter().position(|l| l.starts_with(prefix)).unwrap();
    assert_eq!(
        log[at("adopt_actual_widths")],
        "adopt_actual_widths around=10 viewport=960"
    );
    assert!(at("adopt_widths_before_layout") < at("adopt_actual_widths"));
    assert!(at("adopt_actual_widths") < at("scrolling_layouts"));
}

#[test]
fn a_tiling_workspace_never_asks_about_the_widths_or_the_scrolling_tiles() {
    fresh();
    let mut workspace = on_screen(&[1]);
    adopt_widths(true);

    workspace.update_shared().unwrap();

    let log = log();
    assert!(
        !log.iter()
            .any(|l| l.contains("adopt_") || l.starts_with("scrolling_layouts"))
    );
}

#[test]
fn a_scrolling_workspace_that_asks_again_asks_for_its_tiles_again() {
    fresh();
    let mut workspace = on_screen(&[1, 2]);
    workspace.layout = Layout::Default(DefaultLayout::Scrolling);
    script_passes(&[Pass::Again, Pass::Done]);

    workspace.update_shared().unwrap();

    let log = log();
    assert_eq!(
        log.iter()
            .filter(|l| l.starts_with("scrolling_layouts"))
            .count(),
        2
    );
    // the widths are adopted once, before the first pass
    assert_eq!(
        log.iter()
            .filter(|l| *l == "adopt_widths_before_layout")
            .count(),
        1
    );
}

#[test]
fn the_layout_rule_with_the_highest_threshold_reached_replaces_the_layout_for_good() {
    fresh();
    let mut workspace = on_screen(&[1, 2, 3]);
    workspace.layout_rules = vec![
        (2, Layout::Default(DefaultLayout::Columns)),
        (3, Layout::Default(DefaultLayout::Rows)),
        (4, Layout::Default(DefaultLayout::Grid)),
    ];

    workspace.update_shared().unwrap();
    assert_eq!(workspace.layout, Layout::Default(DefaultLayout::Rows));

    // a rule that is no longer reached does not put the old layout back
    workspace.containers_mut().pop_back();
    workspace.layout_rules = vec![(5, Layout::Default(DefaultLayout::Grid))];
    workspace.update_shared().unwrap();
    assert_eq!(workspace.layout, Layout::Default(DefaultLayout::Rows));
}

#[test]
fn the_layout_in_force_decides_the_tiles_of_the_update() {
    fresh();
    let mut workspace = on_screen(&[1, 2]);
    workspace.layout = Layout::Default(DefaultLayout::Rows);

    workspace.update_shared().unwrap();

    assert_eq!(
        workspace.latest_layout,
        [rect(30, 30, 940, 360), rect(30, 410, 940, 360)]
    );
}

#[test]
fn the_container_behaviour_rules_set_the_behaviour_and_none_reached_clears_it() {
    fresh();
    let mut workspace = on_screen(&[1, 2]);
    workspace.window_container_behaviour_rules = Some(vec![
        (1, WindowContainerBehaviour::Append),
        (2, WindowContainerBehaviour::Create),
    ]);
    workspace.update_shared().unwrap();
    assert_eq!(
        workspace.window_container_behaviour,
        Some(WindowContainerBehaviour::Create)
    );

    workspace.window_container_behaviour_rules = Some(vec![(3, WindowContainerBehaviour::Append)]);
    workspace.update_shared().unwrap();
    assert_eq!(workspace.window_container_behaviour, None);
}

#[test]
fn without_rules_the_container_behaviour_is_left_as_it_was() {
    fresh();
    let mut workspace = on_screen(&[1]);
    workspace.window_container_behaviour = Some(WindowContainerBehaviour::Append);

    workspace.update_shared().unwrap();

    assert_eq!(
        workspace.window_container_behaviour,
        Some(WindowContainerBehaviour::Append)
    );
}

fn viewport_of(workspace: &mut Workspace<StubPlatform>) -> String {
    log();
    workspace.update_shared().unwrap();
    log()
        .iter()
        .find_map(|l| l.split("viewport=").nth(1).map(str::to_string))
        .unwrap()
}

#[test]
fn the_padding_of_the_workspace_wins_over_the_monitors_even_when_it_is_zero() {
    fresh();
    let mut workspace = on_screen(&[1]);
    workspace.workspace_padding = Some(0);
    workspace.container_padding = Some(0);

    workspace.update_shared().unwrap();

    assert_eq!(workspace.latest_layout, [rect(0, 0, 1000, 800)]);
}

#[test]
fn no_padding_anywhere_is_none_at_all() {
    fresh();
    let mut workspace = on_screen(&[1]);
    workspace.globals.workspace_padding = None;
    workspace.globals.container_padding = None;

    workspace.update_shared().unwrap();

    assert_eq!(workspace.latest_layout, [rect(0, 0, 1000, 800)]);
}

#[test]
fn the_work_area_offset_comes_from_the_rule_then_the_workspace_then_the_monitor() {
    fresh();
    let mut workspace = on_screen(&[1, 2]);
    workspace.globals.workspace_padding = None;
    workspace.globals.work_area_offset = Some(rect(1, 0, 10, 0));
    assert_eq!(viewport_of(&mut workspace), "990");

    workspace.work_area_offset = Some(rect(2, 0, 20, 0));
    assert_eq!(viewport_of(&mut workspace), "980");

    workspace.work_area_offset_rules = vec![(1, rect(3, 0, 30, 0)), (2, rect(4, 0, 40, 0))];
    assert_eq!(viewport_of(&mut workspace), "960");

    // a rule that is not reached is not used
    workspace.work_area_offset_rules = vec![(3, rect(4, 0, 40, 0))];
    assert_eq!(viewport_of(&mut workspace), "980");
}

#[test]
fn a_monocle_container_counts_as_one_container_for_the_work_area_offset_rules() {
    fresh();
    let mut workspace = on_screen(&[1, 2, 3]);
    workspace.globals.workspace_padding = None;
    workspace.globals.container_padding = None;
    workspace.work_area_offset_rules = vec![(1, rect(0, 0, 100, 0)), (3, rect(0, 0, 300, 0))];
    workspace.new_monocle_container().unwrap();
    log();

    workspace.update_shared().unwrap();

    // two containers are left beside the monocle one: the rule for three is not reached, and the
    // one for the monocle's count of one is
    assert_eq!(log()[2], "place_monocle 3 [0 0 900 800]");
}

#[test]
fn the_window_based_offset_applies_up_to_its_limit_and_when_the_workspace_takes_it() {
    fresh();
    let mut workspace = on_screen(&[1, 2]);
    workspace.globals.workspace_padding = None;
    workspace.globals.window_based_work_area_offset = Some(rect(0, 0, 50, 0));
    workspace.globals.window_based_work_area_offset_limit = 1;
    assert_eq!(viewport_of(&mut workspace), "1000");

    workspace.globals.window_based_work_area_offset_limit = 2;
    assert_eq!(viewport_of(&mut workspace), "950");

    workspace.apply_window_based_work_area_offset = false;
    assert_eq!(viewport_of(&mut workspace), "1000");
}

#[test]
fn the_window_based_offset_applies_to_a_monocle_container_whatever_the_count_when_the_limit_is_set()
{
    fresh();
    let mut workspace = on_screen(&[1, 2, 3]);
    workspace.globals.workspace_padding = None;
    workspace.globals.container_padding = None;
    workspace.globals.window_based_work_area_offset = Some(rect(0, 0, 50, 0));
    workspace.globals.window_based_work_area_offset_limit = 1;
    workspace.new_monocle_container().unwrap();
    log();

    workspace.update_shared().unwrap();

    assert_eq!(log()[2], "place_monocle 3 [0 0 950 800]");
}

#[test]
fn the_resize_adjustments_are_brought_to_the_number_of_containers() {
    fresh();
    let mut workspace = on_screen(&[1, 2, 3]);
    workspace.resize_dimensions = vec![];

    workspace.update_shared().unwrap();
    assert_eq!(workspace.resize_dimensions, [None, None, None]);

    workspace.resize_dimensions = vec![None; 7];
    workspace.update_shared().unwrap();
    assert_eq!(workspace.resize_dimensions.len(), 3);
}

#[test]
fn the_resize_adjustments_are_not_cut_while_a_container_is_the_monocle() {
    fresh();
    let mut workspace = on_screen(&[1, 2]);
    workspace.resize_dimensions = vec![Some(rect(1, 1, 1, 1)); 2];
    workspace.new_monocle_container().unwrap();

    workspace.update_shared().unwrap();

    assert_eq!(workspace.resize_dimensions.len(), 2);
}

#[test]
fn the_resize_constraints_are_enforced_before_the_layout_is_calculated() {
    fresh();
    let mut workspace = on_screen(&[1, 2]);
    workspace.resize_dimensions = vec![Some(rect(5, 5, 5, 5)); 2];

    workspace.update_shared().unwrap();

    assert_eq!(workspace.resize_dimensions[0], Some(rect(0, 0, 5, 0)));
    assert_eq!(workspace.resize_dimensions[1], Some(rect(5, 5, 0, 0)));
    // an adjustment shows in the placement
    assert!(log().iter().any(|l| l.contains("resize_empty=false")));
}

#[test]
fn the_placement_says_when_there_are_no_resize_adjustments() {
    fresh();
    let mut workspace = on_screen(&[1]);
    workspace.resize_dimensions = vec![];

    workspace.update_shared().unwrap();

    assert!(log().iter().any(|l| l.contains("resize_empty=true")));
}
