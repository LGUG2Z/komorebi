//! Moving the strip by hand: a pan is a drag that the user starts, moves and lets go of, perhaps
//! still moving, and the strip comes to rest with a column aligned to an edge.
//!
//! The viewport is 1200 by 800 and there are no gaps unless a test says so. Eight columns of 300
//! are 2400 wide, so the viewport's left edge can be anywhere from 0 to 1200 in it.

mod common;

use common::*;
use komorebi_scrolling::CenterFocusedColumn;
use komorebi_scrolling::ColumnWidth;
use komorebi_scrolling::Options;
use komorebi_scrolling::ScrollingSpace;
use komorebi_scrolling::Visibility;

fn visible_ids(space: &ScrollingSpace<u32>) -> Vec<u32> {
    space
        .layout()
        .into_iter()
        .filter(|placement| placement.visibility == Visibility::Visible)
        .map(|placement| placement.id)
        .collect()
}

#[test]
fn a_pan_moves_the_strip_by_exactly_what_it_is_given_and_nothing_else() {
    let mut space = strip(options(0.25), 8);
    assert_eq!(space.view_offset(), 1200.0);
    let widths_before = widths(&space);

    space.begin_pan();
    space.pan_by(100.0);

    // the strip moved to the right on screen, so the viewport is further left in it
    assert_eq!(space.view_offset(), 1100.0);
    assert_eq!(space.active_x(), Some(1000.0));
    assert_eq!(x_of(&space, 7), 1000);
    assert_eq!(widths(&space), widths_before);
    assert_eq!(
        space.active_id(),
        Some(&7),
        "the active column is the user's to move"
    );

    space.pan_by(-350.0);
    assert_eq!(space.view_offset(), 1450.0);
}

#[test]
fn letting_go_standing_still_rests_at_the_nearest_alignment() {
    let mut space = strip(options(0.25), 8);

    space.begin_pan();
    space.pan_by(500.0);
    let snap = space.pan_end(0.0).unwrap();

    // from 1200 to 700, which is nearer 600 than it is 900
    assert_eq!(snap.from, 700.0);
    assert_eq!(snap.to, 600.0);
    assert_eq!(space.view_offset(), 600.0);

    // columns 2 to 5 are in view, and focus moved to the nearest of them to where it was
    assert_eq!(visible_ids(&space), [2, 3, 4, 5]);
    assert_eq!(snap.active, 5);
    assert_eq!(space.active_id(), Some(&5));
    assert_eq!(x_of(&space, 5), 900);
}

#[test]
fn a_flick_carries_on_in_the_direction_it_was_going() {
    let mut space = strip(options(0.25), 8);
    space.focus(&0);
    assert_eq!(space.view_offset(), 0.0);

    // the finger moved left by 100, so the viewport is further right in the strip, and it was
    // going on at 1500 pixels a second when it was let go, which is another 500 or so
    space.begin_pan();
    space.pan_by(-100.0);
    let snap = space.pan_end(-1500.0).unwrap();

    assert_eq!(snap.to, 600.0);
    assert_eq!(visible_ids(&space), [2, 3, 4, 5]);
}

#[test]
fn the_strip_never_comes_to_rest_past_either_end_however_hard_it_was_flicked() {
    let mut space = strip(options(0.25), 8);
    space.focus(&4);

    space.begin_pan();
    let snap = space.pan_end(-1_000_000.0).unwrap();
    assert_eq!(snap.to, 1200.0);
    assert_eq!(x_of(&space, 7), 900);

    space.begin_pan();
    let snap = space.pan_end(1_000_000.0).unwrap();
    assert_eq!(snap.to, 0.0);
    assert_eq!(x_of(&space, 0), 0);
}

#[test]
fn a_pan_that_goes_past_the_end_and_is_let_go_comes_back() {
    let mut space = strip(options(0.25), 8);

    space.begin_pan();
    space.pan_by(-500.0);
    // 500 past the last column, and it is just as far as it was told
    assert_eq!(space.view_offset(), 1700.0);

    let snap = space.pan_end(0.0).unwrap();

    assert_eq!(snap.from, 1700.0);
    assert_eq!(snap.to, 1200.0);
}

#[test]
fn how_far_past_the_end_a_pan_can_go_is_limited() {
    let mut space = strip(options(0.25), 8);

    space.begin_pan();
    space.pan_by(-1_000_000.0);

    // by about the width of the viewport
    assert_eq!(space.view_offset(), 2400.0);
    space.pan_by(f64::NAN);
    assert_eq!(space.view_offset(), 2400.0);
}

#[test]
fn focus_stays_where_it_is_when_it_is_still_in_view_after_the_pan() {
    let mut space = strip(options(0.25), 8);
    space.focus(&3);
    assert_eq!(space.view_offset(), 900.0);

    space.begin_pan();
    space.pan_by(300.0);
    let snap = space.pan_end(0.0).unwrap();

    // column 3 is at the left edge and the pan took it a column to the right
    assert_eq!(snap.to, 600.0);
    assert_eq!(snap.active, 3);
    assert_eq!(space.active_id(), Some(&3));
    assert_eq!(x_of(&space, 3), 300);
}

#[test]
fn focus_goes_to_the_nearest_column_in_view_when_the_active_one_is_not() {
    let mut space = strip(options(0.25), 8);
    space.focus(&0);

    space.begin_pan();
    space.pan_by(-900.0);
    let snap = space.pan_end(0.0).unwrap();

    assert_eq!(snap.to, 900.0);
    assert_eq!(visible_ids(&space), [3, 4, 5, 6]);
    assert_eq!(snap.active, 3, "the one of them that is nearest to 0");
}

#[test]
fn the_tile_that_has_focus_in_a_column_is_the_one_that_gets_it() {
    let mut space = strip(options(0.25), 8);
    space.focus(&5);
    space.consume_right();
    space.focus(&6);
    space.focus(&7);
    // seven columns of 300, one of which has two tiles, with 7 in the last of them
    assert_eq!(space.view_offset(), 1200.0);

    space.begin_pan();
    space.pan_by(600.0);
    let snap = space.pan_end(0.0).unwrap();

    // 7 is out of view, and the nearest column that is in view is the one with 5 and 6, which
    // remembers that 6 had focus
    assert_eq!(snap.to, 600.0);
    assert_eq!(snap.active, 6);
}

#[test]
fn gaps_are_left_between_the_strip_and_the_edges() {
    let options = Options {
        gaps: 16,
        ..options(0.25)
    };
    let mut space = strip(options, 8);
    space.focus(&0);

    space.begin_pan();
    space.pan_by(500.0);
    space.pan_end(0.0);
    assert_eq!(
        x_of(&space, 0),
        16,
        "the first column is a gap from the edge"
    );

    space.begin_pan();
    space.pan_end(-1_000_000.0);
    let last = placement(&space, 7).rect;
    assert_eq!(last.right(), 1200 - 16);
}

#[test]
fn a_strip_that_fits_in_the_viewport_stays_where_it_starts() {
    let mut space = strip(options(0.25), 2);

    space.begin_pan();
    space.pan_by(400.0);
    let snap = space.pan_end(5000.0).unwrap();

    assert_eq!(snap.to, 0.0);
    assert_eq!(x_of(&space, 0), 0);
    assert_eq!(x_of(&space, 1), 300);
}

#[test]
fn columns_wider_than_the_viewport_rest_at_their_left_edge() {
    let mut space = strip(options(0.25), 3);
    for id in 0..3 {
        space.set_width(&id, ColumnWidth::Fixed(1500));
    }

    space.begin_pan();
    space.pan_by(700.0);
    let snap = space.pan_end(0.0).unwrap();

    // none of them fits, so the nearest to the middle is aligned, and is the active one
    let active = placement(&space, snap.active);
    assert_eq!(active.rect.x, 0);
    assert_eq!(active.visibility, Visibility::Clipped);
}

#[test]
fn when_columns_are_centred_the_pan_rests_with_one_in_the_middle() {
    let options = Options {
        center_focused_column: CenterFocusedColumn::Always,
        ..options(0.25)
    };
    let mut space = strip(options, 8);
    space.focus(&3);

    space.begin_pan();
    space.pan_by(130.0);
    let snap = space.pan_end(0.0).unwrap();

    // 450 is where the left edge of the column would be if it were centred in 1200
    assert_eq!(x_of(&space, snap.active), 450);
}

#[test]
fn a_pan_forgets_the_view_that_was_to_be_put_back() {
    let mut space = strip(options(0.25), 3);
    space.focus(&0);
    // 9 is opened next to 0, so closing it would put focus back on 0
    space.add_column(9, None, None, true);

    space.begin_pan();
    space.remove_column(&9);

    // the user moved the strip since, so it is a column that takes the place of the one that went
    assert_eq!(space.active_id(), Some(&1));
}

#[test]
fn nothing_in_the_strip_means_nothing_to_pan() {
    let mut space = ScrollingSpace::<u32>::new(options(0.25), VIEW);

    space.begin_pan();
    space.pan_by(100.0);

    assert_eq!(space.pan_end(100.0), None);
}

#[test]
fn a_pan_that_is_let_go_is_laid_out_like_any_other_strip() {
    let mut space = strip(options(0.25), 8);

    space.begin_pan();
    space.pan_by(333.0);
    space.pan_end(250.0);

    // the placements say which columns are where, and the active one is fully in view
    let layout = space.layout();
    let active = layout.iter().find(|placement| placement.active).unwrap();
    assert_eq!(active.visibility, Visibility::Visible);
    for pair in layout.windows(2) {
        assert_eq!(pair[1].rect.x - pair[0].rect.right(), 0);
    }
}
