//! Characterization of the resize constraints: which sides of each tile of each layout may be
//! given a resize adjustment.

mod common;

use common::*;
use komorebi_core::Workspace;
use komorebi_layouts::DefaultLayout;
use komorebi_layouts::Layout;
use komorebi_layouts::Rect;

/// A workspace of `n` containers whose adjustments are all 5 on every side, with the layout.
fn workspace_with(layout: DefaultLayout, n: usize) -> Workspace<StubPlatform> {
    Workspace {
        layout: Layout::Default(layout),
        resize_dimensions: vec![
            Some(Rect {
                left: 5,
                top: 5,
                right: 5,
                bottom: 5
            });
            n
        ],
        ..Default::default()
    }
}

/// The sides that are still adjustable, per tile, as the letters of "ltrb".
fn sides(workspace: &Workspace<StubPlatform>) -> Vec<String> {
    workspace
        .resize_dimensions
        .iter()
        .map(|rect| {
            let Some(r) = rect else {
                return "-".to_string();
            };
            let mut s = String::new();
            for (letter, value) in [('l', r.left), ('t', r.top), ('r', r.right), ('b', r.bottom)] {
                if value != 0 {
                    s.push(letter);
                }
            }
            s
        })
        .collect()
}

fn constrained(layout: DefaultLayout, n: usize) -> Vec<String> {
    fresh();
    let mut workspace = workspace_with(layout, n);
    workspace.enforce_resize_constraints();
    sides(&workspace)
}

fn check(layout: DefaultLayout, expected: &[&[&str]]) {
    for tiles in expected {
        assert_eq!(
            constrained(layout, tiles.len()),
            *tiles,
            "{layout:?} with {} containers",
            tiles.len()
        );
    }
}

#[test]
fn bsp_alternates_the_closed_side_and_closes_the_outer_edges() {
    check(
        DefaultLayout::BSP,
        &[&[""], &["r", "lt"], &["r", "ltb", "lt"]],
    );
}

#[test]
fn columns_only_resize_sideways_and_not_beyond_the_outer_columns() {
    check(
        DefaultLayout::Columns,
        &[&[""], &["r", "l"], &["r", "lr", "l"]],
    );
}

#[test]
fn rows_only_resize_up_and_down_and_not_beyond_the_outer_rows() {
    check(
        DefaultLayout::Rows,
        &[&[""], &["b", "t"], &["b", "tb", "t"]],
    );
}

#[test]
fn the_vertical_stack_has_its_main_column_on_the_left() {
    check(
        DefaultLayout::VerticalStack,
        &[
            &[""],
            &["r", "lb"],
            &["r", "lb", "lt"],
            &["r", "lb", "ltb", "lt"],
        ],
    );
}

#[test]
fn the_horizontal_stack_has_its_main_row_on_top() {
    check(
        DefaultLayout::HorizontalStack,
        &[
            &[""],
            &["b", "t"],
            &["b", "tr", "lt"],
            &["b", "tr", "ltr", "lt"],
        ],
    );
}

#[test]
fn the_ultrawide_stack_has_its_main_column_in_the_middle() {
    check(
        DefaultLayout::UltrawideVerticalStack,
        &[
            &[""],
            &["l", "r"],
            &["lr", "r", "lb"],
            &["lr", "r", "lb", "lt"],
        ],
    );
}

#[test]
fn the_right_main_vertical_stack_constrains_index_one_and_leaves_index_zero_free() {
    // characterization of what the code does: it follows the comment "Zero is actually on the
    // right" but indexes the second container, so the first (the main one) is never constrained
    // and the second may not be resized at all; the ignored test below says what the comment asks
    check(
        DefaultLayout::RightMainVerticalStack,
        &[
            &[""],
            &["ltrb", ""],
            &["ltrb", "", "tr"],
            &["ltrb", "", "trb", "tr"],
        ],
    );
}

#[test]
#[ignore = "bug: enforce_resize_for_right_vertical_stack indexes resize_dimensions[1] where its \
            comment says zero is on the right; inherited unchanged from the Windows tree"]
fn the_right_main_vertical_stack_constrains_its_main_container_at_index_zero() {
    fresh();
    let mut workspace = workspace_with(DefaultLayout::RightMainVerticalStack, 3);

    workspace.enforce_resize_constraints();

    // the main container is on the right: it cannot go up, down or right
    assert_eq!(sides(&workspace)[0], "l");
}

#[test]
fn layouts_without_a_resize_rule_take_every_adjustment_away() {
    for layout in [DefaultLayout::Grid, DefaultLayout::Scrolling] {
        assert_eq!(constrained(layout, 3), ["", "", ""], "{layout:?}");
    }
}

#[test]
fn a_tile_without_an_adjustment_stays_without_one() {
    fresh();
    let mut workspace = workspace_with(DefaultLayout::Columns, 3);
    workspace.resize_dimensions[1] = None;

    workspace.enforce_resize_constraints();

    assert_eq!(sides(&workspace), ["r", "-", "l"]);
}

#[test]
fn no_adjustments_at_all_is_fine_for_every_layout() {
    for layout in [
        DefaultLayout::BSP,
        DefaultLayout::Columns,
        DefaultLayout::Rows,
        DefaultLayout::VerticalStack,
        DefaultLayout::RightMainVerticalStack,
        DefaultLayout::HorizontalStack,
        DefaultLayout::UltrawideVerticalStack,
        DefaultLayout::Grid,
    ] {
        assert!(constrained(layout, 0).is_empty());
    }
}
