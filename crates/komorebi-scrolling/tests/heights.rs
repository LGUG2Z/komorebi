//! How tall the tiles of a column are: equal by default, and one of them can be given a height,
//! with the others taking up what is left.
//!
//! The viewport is 1200 by 800 and there are no gaps unless a test says so.

mod common;

use common::*;
use komorebi_scrolling::Options;
use komorebi_scrolling::ScrollingSpace;
use komorebi_scrolling::TileHeight;

/// A strip of `tiles` tiles in one column, with 0 in focus, and then one more column
fn column_of(options: Options, tiles: u32) -> ScrollingSpace<u32> {
    let mut space = strip(options, tiles + 1);
    space.focus(&0);
    for _ in 1..tiles {
        space.consume_right();
    }

    space
}

fn heights_of(space: &ScrollingSpace<u32>, tiles: u32) -> Vec<i32> {
    (0..tiles).map(|id| rect(space, id).height).collect()
}

#[test]
fn tiles_share_the_height_equally_until_one_is_given_a_height() {
    let space = column_of(options(0.25), 3);

    let heights = heights_of(&space, 3);
    assert!(heights.iter().all(|h| (h - 267).abs() <= 1), "{heights:?}");
    assert_eq!(heights.iter().sum::<i32>(), 800);
}

#[test]
fn a_tile_that_is_given_a_height_has_it_and_the_others_share_the_rest() {
    let mut space = column_of(options(0.25), 3);

    space.set_tile_height(&1, TileHeight::Fixed(500));

    assert_eq!(heights_of(&space, 3), [150, 500, 150]);
    assert_eq!(space.tile_height(&1), Some(500.0));
}

#[test]
fn a_height_can_be_a_share_of_the_column() {
    let mut space = column_of(options(0.25), 2);

    space.set_tile_height(&0, TileHeight::Proportion(0.25));

    assert_eq!(heights_of(&space, 2), [200, 600]);
}

#[test]
fn a_share_of_the_column_takes_the_gaps_between_the_tiles_out_first() {
    let options = Options {
        gaps: 10,
        ..options(0.25)
    };
    let mut space = column_of(options, 2);

    // 800 less 10 above, 10 below and 10 between is 770
    space.set_tile_height(&0, TileHeight::Proportion(0.5));

    assert_eq!(heights_of(&space, 2), [385, 385]);
    assert_eq!(rect(&space, 0).y, 10);
    assert_eq!(rect(&space, 1).bottom(), 790);
}

#[test]
fn giving_a_tile_a_height_gives_the_others_back_an_equal_share() {
    let mut space = column_of(options(0.25), 3);
    space.set_tile_height(&0, TileHeight::Fixed(500));

    space.set_tile_height(&2, TileHeight::Fixed(300));

    // 0 is no longer as tall as it was given, and shares what 2 leaves with 1
    assert_eq!(heights_of(&space, 3), [250, 250, 300]);
}

#[test]
fn a_tile_can_be_given_back_an_equal_share_by_itself() {
    let mut space = column_of(options(0.25), 3);
    space.set_tile_height(&1, TileHeight::Fixed(500));

    space.set_tile_height(&1, TileHeight::Auto);

    let heights = heights_of(&space, 3);
    assert!(heights.iter().all(|h| (h - 267).abs() <= 1), "{heights:?}");
}

#[test]
fn a_tile_that_is_alone_fills_its_column_whatever_it_is_given() {
    let mut space = strip(options(0.25), 2);

    space.set_tile_height(&0, TileHeight::Fixed(100));
    space.resize_tile_height(&0, -50.0);

    assert_eq!(rect(&space, 0).height, 800);
}

#[test]
fn what_one_tile_is_given_cannot_leave_the_others_less_than_the_minimum() {
    let options = Options {
        minimum_tile_height: 200,
        ..options(0.25)
    };
    let mut space = column_of(options, 3);

    space.set_tile_height(&1, TileHeight::Fixed(700));

    // 800 less two tiles of at least 200
    assert_eq!(heights_of(&space, 3), [200, 400, 200]);
}

#[test]
fn a_tile_is_not_given_less_than_the_minimum() {
    let options = Options {
        minimum_tile_height: 200,
        ..options(0.25)
    };
    let mut space = column_of(options, 2);

    space.set_tile_height(&0, TileHeight::Fixed(50));

    assert_eq!(heights_of(&space, 2), [200, 600]);
}

#[test]
fn a_column_without_room_for_the_minimum_shares_equally() {
    let options = Options {
        minimum_tile_height: 300,
        ..options(0.25)
    };
    let mut space = column_of(options, 3);
    space.set_tile_height(&0, TileHeight::Fixed(500));

    let heights = heights_of(&space, 3);
    assert!(heights.iter().all(|h| (h - 267).abs() <= 1), "{heights:?}");
}

#[test]
fn resizing_a_tile_starts_from_the_height_that_it_has() {
    let mut space = column_of(options(0.25), 2);
    assert_eq!(heights_of(&space, 2), [400, 400]);

    space.resize_tile_height(&0, 100.0);
    assert_eq!(heights_of(&space, 2), [500, 300]);

    space.resize_tile_height(&0, -250.0);
    assert_eq!(heights_of(&space, 2), [250, 550]);
}

#[test]
fn resizing_a_tile_stops_at_the_minimum_and_at_what_the_others_need() {
    let options = Options {
        minimum_tile_height: 150,
        ..options(0.25)
    };
    let mut space = column_of(options, 3);

    space.resize_tile_height(&1, 5000.0);
    assert_eq!(heights_of(&space, 3), [150, 500, 150]);

    space.resize_tile_height(&1, -5000.0);
    assert_eq!(rect(&space, 1).height, 150);
}

#[test]
fn preset_heights_are_stepped_through_in_order_and_wrap() {
    let mut space = column_of(options(0.25), 2);
    let mut seen = vec![];

    // a third, a half and two thirds of 800, and the tile starts at a half
    for _ in 0..4 {
        space.cycle_preset_height(&0, true);
        seen.push(rect(&space, 0).height);
    }
    assert_eq!(seen, [533, 267, 400, 533]);

    let mut seen = vec![];
    for _ in 0..3 {
        space.cycle_preset_height(&0, false);
        seen.push(rect(&space, 0).height);
    }
    assert_eq!(seen, [400, 267, 533]);
}

#[test]
fn resetting_the_heights_of_a_column_leaves_the_other_columns_alone() {
    let mut space = column_of(options(0.25), 2);
    space.consume_right();
    space.set_tile_height(&1, TileHeight::Fixed(600));
    assert_eq!(rect(&space, 1).height, 600);

    space.reset_tile_heights(&0);

    assert_eq!(heights_of(&space, 3).iter().sum::<i32>(), 800);
    assert!((rect(&space, 0).height - 267).abs() <= 1);
}

#[test]
fn every_height_can_be_forgotten() {
    let mut space = column_of(options(0.25), 2);
    space.set_tile_height(&0, TileHeight::Fixed(600));

    space.reset_heights();

    assert_eq!(heights_of(&space, 2), [400, 400]);
}

#[test]
fn a_height_stays_with_its_tile_when_the_tile_moves() {
    let mut space = column_of(options(0.25), 3);
    space.set_tile_height(&0, TileHeight::Fixed(500));

    space.move_tile_down();

    // 0 moved below 1, and it is 0 that is 500 tall wherever it is
    assert_eq!(space.ids()[..3], [1, 0, 2]);
    assert_eq!(heights_of(&space, 3), [500, 150, 150]);
    assert!(rect(&space, 1).y < rect(&space, 0).y);
}

#[test]
fn a_tile_that_changes_column_starts_with_an_equal_share() {
    let mut space = column_of(options(0.25), 2);
    space.set_tile_height(&0, TileHeight::Fixed(600));

    space.expel();

    // 0 is on its own now, in a column of its own, and 1 is too
    assert_eq!(rect(&space, 0).height, 800);

    space.consume_right();
    // and the tile that joined it is as tall as an equal share, as it is
    assert_eq!(space.column_of(&0), Some(vec![0, 2]));
    assert_eq!(rect(&space, 0).height, 400);
    assert_eq!(rect(&space, 2).height, 400);
}

#[test]
fn a_tile_that_is_closed_takes_its_height_with_it() {
    let mut space = column_of(options(0.25), 3);
    space.set_tile_height(&1, TileHeight::Fixed(500));

    space.remove_column(&1);

    assert_eq!(rect(&space, 0).height, 400);
    assert_eq!(rect(&space, 2).height, 400);
}

#[test]
fn heights_do_not_move_anything_sideways() {
    let mut space = column_of(options(0.25), 3);
    let x = space.active_x();
    let widths_before = widths(&space);

    space.resize_tile_height(&0, 100.0);
    space.cycle_preset_height(&0, true);

    assert_eq!(space.active_x(), x);
    assert_eq!(widths(&space), widths_before);
}

#[test]
fn saying_what_is_there_keeps_the_heights() {
    let mut space = column_of(options(0.25), 3);
    space.set_tile_height(&1, TileHeight::Fixed(500));
    let before = space.clone();

    let ids = space.ids();
    let focused = space.active_id().copied();
    space.reconcile(&ids, focused.as_ref());

    assert_eq!(space, before);
    assert_eq!(heights_of(&space, 3), [150, 500, 150]);
}
