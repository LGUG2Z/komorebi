//! Columns of more than one tile, which sit one above the other and share the height.
//!
//! The viewport is 1200 by 800, there are no gaps unless a test says so, and columns are a quarter
//! of the viewport wide (300). A strip of four is keyed 0, 1, 2 and 3 in reading order.

mod common;

use common::*;
use komorebi_scrolling::ColumnWidth;
use komorebi_scrolling::Options;
use komorebi_scrolling::Rect;
use komorebi_scrolling::ScrollingSpace;

fn ids(space: &ScrollingSpace<u32>) -> Vec<u32> {
    space.ids()
}

/// The columns, each as the keys of its tiles from the top.
fn columns(space: &ScrollingSpace<u32>) -> Vec<Vec<u32>> {
    let mut seen = vec![];
    let mut columns = vec![];

    for id in space.ids() {
        if seen.contains(&id) {
            continue;
        }

        let column = space.column_of(&id).unwrap();
        seen.extend(column.iter().copied());
        columns.push(column);
    }

    columns
}

// Making columns of tiles

#[test]
fn consuming_to_the_right_takes_the_next_columns_tile_to_the_bottom_of_this_one() {
    let mut space = strip(options(0.25), 4);
    space.focus(&0);

    assert!(space.consume_right());

    assert_eq!(columns(&space), [vec![0, 1], vec![2], vec![3]]);
    // the order of the tiles is the order that they were in
    assert_eq!(ids(&space), [0, 1, 2, 3]);
    assert_eq!(space.len(), 3);
    assert_eq!(space.tile_count(), 4);

    // one above the other, in the column that was 0's, sharing the height
    assert_eq!(
        rect(&space, 0),
        Rect {
            x: 0,
            y: 0,
            width: 300,
            height: 400
        }
    );
    assert_eq!(
        rect(&space, 1),
        Rect {
            x: 0,
            y: 400,
            width: 300,
            height: 400
        }
    );

    // and the columns after it have moved up to where the one that went was
    assert_eq!(x_of(&space, 2), 300);
    assert_eq!(x_of(&space, 3), 600);
}

#[test]
fn consuming_takes_only_the_first_tile_of_a_column_that_has_others() {
    let mut space = strip(options(0.25), 4);
    space.focus(&1);
    space.consume_right();
    space.set_width(&1, ColumnWidth::Fixed(500));
    assert_eq!(columns(&space), [vec![0], vec![1, 2], vec![3]]);

    space.focus(&0);
    assert!(space.consume_right());

    // 1 left its column, and 2 is a column of its own that has the width that the column had
    assert_eq!(columns(&space), [vec![0, 1], vec![2], vec![3]]);
    assert_eq!(width_of(&space, 2), 500);
    assert_eq!(width_of(&space, 1), 300);
}

#[test]
fn there_is_nothing_to_consume_after_the_last_column_or_before_the_first() {
    let mut space = strip(options(0.25), 3);
    space.focus(&2);
    assert!(!space.consume_right());

    space.focus(&0);
    assert!(!space.consume_left());

    assert_eq!(columns(&space), [vec![0], vec![1], vec![2]]);
}

#[test]
fn consuming_to_the_left_takes_the_last_tile_of_the_column_before_to_the_top_of_this_one() {
    let mut space = strip(options(0.25), 4);
    space.focus(&1);

    assert!(space.consume_left());

    assert_eq!(columns(&space), [vec![0, 1], vec![2], vec![3]]);
    assert_eq!(ids(&space), [0, 1, 2, 3]);
    assert_eq!(space.active_id(), Some(&1), "focus stays on the same tile");
    assert_eq!(space.active_tile_index(), Some(1));
}

#[test]
fn consuming_to_the_left_leaves_the_tiles_above_the_one_that_goes() {
    let mut space = strip(options(0.25), 4);
    space.focus(&0);
    space.consume_right();
    space.focus(&2);

    assert!(space.consume_left());

    assert_eq!(columns(&space), [vec![0], vec![1, 2], vec![3]]);
    assert_eq!(space.active_id(), Some(&2));
}

#[test]
fn consuming_does_not_move_the_focused_column_on_screen() {
    let mut space = strip(options(0.25), 8);
    space.focus(&5);
    let x = space.active_x();

    space.consume_left();
    assert_eq!(space.active_x(), x);
    space.consume_right();
    assert_eq!(space.active_x(), x);
    assert_eq!(space.active_id(), Some(&5));
}

#[test]
fn consuming_does_not_change_the_width_of_the_focused_column() {
    let mut space = strip(options(0.25), 4);
    space.set_width(&1, ColumnWidth::Fixed(450));
    space.focus(&1);

    space.consume_right();
    assert_eq!(width_of(&space, 1), 450);
    assert_eq!(
        width_of(&space, 2),
        450,
        "the tile has the width of its column"
    );
    space.consume_left();
    assert_eq!(width_of(&space, 1), 450);
}

// Taking tiles out of a column

#[test]
fn expelling_the_last_tile_puts_it_in_a_column_of_its_own_on_the_right() {
    let mut space = strip(options(0.25), 3);
    space.focus(&0);
    space.consume_right();
    space.set_width(&0, ColumnWidth::Fixed(500));
    space.focus(&1);

    assert!(space.expel());

    assert_eq!(columns(&space), [vec![0], vec![1], vec![2]]);
    assert_eq!(
        ids(&space),
        [0, 1, 2],
        "it was last, so it did not move in the order"
    );
    // 0 keeps what it was given and the new column has the default
    assert_eq!(width_of(&space, 0), 500);
    assert_eq!(width_of(&space, 1), 300);
    // and focus goes with it
    assert_eq!(space.active_id(), Some(&1));
    assert_eq!(space.active_index(), Some(1));
}

#[test]
fn expelling_a_tile_that_has_others_below_it_moves_it_past_them() {
    let mut space = strip(options(0.25), 3);
    space.focus(&0);
    space.consume_right();
    space.consume_right();
    assert_eq!(columns(&space), [vec![0, 1, 2]]);

    space.focus_down();
    assert_eq!(space.active_id(), Some(&1));
    assert!(space.expel());

    assert_eq!(columns(&space), [vec![0, 2], vec![1]]);
    assert_eq!(
        ids(&space),
        [0, 2, 1],
        "the host has to change its list to match"
    );
    assert_eq!(space.active_id(), Some(&1));
}

#[test]
fn a_tile_that_is_alone_cannot_be_expelled() {
    let mut space = strip(options(0.25), 3);

    assert!(!space.expel());
    assert_eq!(columns(&space), [vec![0], vec![1], vec![2]]);
}

#[test]
fn expelling_scrolls_only_if_the_new_column_is_out_of_view() {
    let mut space = strip(options(0.25), 4);
    space.focus(&3);
    space.consume_left();
    space.set_width(&3, ColumnWidth::Fixed(900));
    let before = x_of(&space, 3);

    space.expel();

    // 3 was 900 wide and had the right of the screen. The new column is 300 wide and fits
    assert_eq!(space.active_id(), Some(&3));
    assert!(placement(&space, 3).rect.right() <= 1200);
    assert!(x_of(&space, 3) >= 0, "{before}");
}

// Moving around

#[test]
fn focus_moves_between_the_tiles_of_a_column_and_stops_at_the_ends() {
    let mut space = strip(options(0.25), 3);
    space.focus(&0);
    space.consume_right();
    space.consume_right();
    assert_eq!(space.active_id(), Some(&0));

    assert!(!space.focus_up());
    assert!(space.focus_down());
    assert_eq!(space.active_id(), Some(&1));
    assert!(space.focus_down());
    assert_eq!(space.active_id(), Some(&2));
    assert!(!space.focus_down(), "and it does not go round");
    assert!(space.focus_up());
    assert_eq!(space.active_id(), Some(&1));

    // and nothing in the strip moved
    assert_eq!(x_of(&space, 0), 0);
    assert!(placement(&space, 1).active);
}

#[test]
fn a_column_remembers_which_of_its_tiles_had_focus() {
    let mut space = strip(options(0.25), 4);
    space.focus(&0);
    space.consume_right();
    space.focus(&1);

    space.focus(&2);
    assert_eq!(space.active_id(), Some(&2));

    space.focus_left();
    assert_eq!(space.active_id(), Some(&1), "it was the one that had it");

    space.focus_right();
    space.focus_right();
    space.focus_left();
    space.focus_left();
    assert_eq!(space.active_id(), Some(&1));
}

#[test]
fn focusing_a_tile_in_another_column_goes_to_that_column_and_that_tile() {
    let mut space = strip(options(0.25), 6);
    space.focus(&3);
    space.consume_right();
    space.focus(&0);

    space.focus(&4);

    assert_eq!(space.active_id(), Some(&4));
    assert_eq!(space.active_index(), Some(3), "columns, not tiles");
    assert_eq!(
        space.active_tile_index(),
        Some(4),
        "tiles, in reading order"
    );
}

#[test]
fn moving_a_tile_swaps_it_with_its_neighbour_and_keeps_focus_on_it() {
    let mut space = strip(options(0.25), 3);
    space.focus(&0);
    space.consume_right();
    space.consume_right();

    assert!(!space.move_tile_up(), "it is at the top");
    assert!(space.move_tile_down());
    assert_eq!(ids(&space), [1, 0, 2]);
    assert_eq!(space.active_id(), Some(&0));
    assert_eq!(columns(&space), [vec![1, 0, 2]]);

    assert!(space.move_tile_down());
    assert!(!space.move_tile_down(), "it is at the bottom");
    assert_eq!(ids(&space), [1, 2, 0]);

    assert!(space.move_tile_up());
    assert_eq!(ids(&space), [1, 0, 2]);
    assert_eq!(space.active_id(), Some(&0));
}

#[test]
fn moving_a_column_moves_all_of_its_tiles() {
    let mut space = strip(options(0.25), 4);
    space.focus(&1);
    space.consume_right();
    assert_eq!(columns(&space), [vec![0], vec![1, 2], vec![3]]);

    space.move_column(&1, 2);

    assert_eq!(columns(&space), [vec![0], vec![3], vec![1, 2]]);
    assert_eq!(ids(&space), [0, 3, 1, 2]);
}

// What a column of tiles looks like

#[test]
fn tiles_share_the_height_of_the_column_with_a_gap_between_them() {
    let options = Options {
        gaps: 10,
        ..options(0.25)
    };
    let mut space = strip(options, 3);
    space.focus(&0);
    space.consume_right();
    space.consume_right();

    // 800 less a gap above, a gap below and a gap between each of the three: 800 - 40 = 760
    let tiles: Vec<_> = [0, 1, 2].iter().map(|id| rect(&space, *id)).collect();
    let heights: Vec<_> = tiles.iter().map(|tile| tile.height).collect();
    assert_eq!(tiles[0].y, 10);
    assert_eq!(tiles[2].bottom(), 790);
    for pair in tiles.windows(2) {
        assert_eq!(pair[1].y - pair[0].bottom(), 10);
    }
    assert!(heights.iter().all(|h| (*h - 253).abs() <= 1), "{heights:?}");
}

#[test]
fn only_one_tile_in_the_strip_is_active() {
    let mut space = strip(options(0.25), 3);
    space.focus(&0);
    space.consume_right();
    space.focus(&1);

    let active: Vec<_> = space
        .layout()
        .into_iter()
        .filter(|placement| placement.active)
        .map(|placement| placement.id)
        .collect();
    assert_eq!(active, [1]);
}

#[test]
fn a_fullscreen_column_gives_its_tiles_the_whole_height() {
    let options = Options {
        gaps: 10,
        ..options(0.25)
    };
    let mut space = strip(options, 2);
    space.focus(&0);
    space.consume_right();

    space.set_fullscreen(&0, true);

    assert_eq!(rect(&space, 0).y, 0);
    assert_eq!(rect(&space, 1).bottom(), 800);
    assert_eq!(rect(&space, 1).y, rect(&space, 0).bottom());
}

// Closing tiles

#[test]
fn closing_a_tile_leaves_the_column_with_its_width() {
    let mut space = strip(options(0.25), 3);
    space.focus(&0);
    space.consume_right();
    space.set_width(&0, ColumnWidth::Fixed(500));

    assert!(space.remove_column(&1));

    assert_eq!(columns(&space), [vec![0], vec![2]]);
    assert_eq!(width_of(&space, 0), 500);
    assert_eq!(rect(&space, 0).height, 800, "and it has the height again");
}

#[test]
fn closing_the_first_tile_of_a_column_leaves_the_others_as_the_column() {
    let mut space = strip(options(0.25), 3);
    space.focus(&0);
    space.consume_right();
    space.consume_right();
    space.set_width(&0, ColumnWidth::Fixed(500));

    space.remove_column(&0);

    assert_eq!(columns(&space), [vec![1, 2]]);
    assert_eq!(width_of(&space, 1), 500);
    assert_eq!(width_of(&space, 2), 500);
}

#[test]
fn closing_the_focused_tile_gives_focus_to_the_one_that_takes_its_place() {
    let mut space = strip(options(0.25), 3);
    space.focus(&0);
    space.consume_right();
    space.consume_right();
    space.focus(&1);

    space.remove_column(&1);
    assert_eq!(space.active_id(), Some(&2), "the one below");

    space.remove_column(&2);
    assert_eq!(
        space.active_id(),
        Some(&0),
        "and the one above if there is none"
    );
}

#[test]
fn closing_a_tile_that_does_not_have_focus_leaves_focus_where_it_is() {
    let mut space = strip(options(0.25), 3);
    space.focus(&0);
    space.consume_right();
    space.consume_right();
    space.focus(&2);

    space.remove_column(&0);
    assert_eq!(space.active_id(), Some(&2));

    let mut space = strip(options(0.25), 3);
    space.focus(&0);
    space.consume_right();
    space.consume_right();
    space.focus(&1);
    space.remove_column(&2);
    assert_eq!(space.active_id(), Some(&1));
}

// A host that keeps a list

#[test]
fn saying_what_is_there_changes_nothing_with_columns_of_tiles() {
    let mut space = strip(options(0.25), 6);
    space.focus(&1);
    space.consume_right();
    space.focus(&4);
    space.consume_left();
    space.set_width(&1, ColumnWidth::Fixed(450));
    let before = space.clone();

    let all = space.ids();
    let focused = space.active_id().copied();
    space.reconcile(&all, focused.as_ref());

    assert_eq!(space, before);
}

#[test]
fn a_tile_that_is_new_starts_a_column_of_its_own() {
    let mut space = strip(options(0.25), 3);
    space.focus(&0);
    space.consume_right();

    // after the last tile of the column, which is where the host puts a window that is opened
    space.reconcile(&[0, 1, 9, 2], Some(&9));

    assert_eq!(columns(&space), [vec![0, 1], vec![9], vec![2]]);
    assert_eq!(width_of(&space, 9), 300);
}

#[test]
fn a_tile_that_is_put_in_the_middle_of_a_column_splits_it() {
    // the list wins over what the model had: this is the reason that a host puts a window that
    // is opened after the last tile of the column that has focus, and not after the focused tile
    let mut space = strip(options(0.25), 3);
    space.focus(&0);
    space.consume_right();
    space.consume_right();

    space.reconcile(&[0, 9, 1, 2], Some(&9));

    assert_eq!(columns(&space), [vec![0], vec![9, 1, 2]]);
}

#[test]
fn tiles_that_go_are_taken_out_of_their_columns() {
    let mut space = strip(options(0.25), 4);
    space.focus(&0);
    space.consume_right();
    space.consume_right();
    space.set_width(&0, ColumnWidth::Fixed(500));

    space.reconcile(&[0, 2, 3], Some(&0));

    assert_eq!(columns(&space), [vec![0, 2], vec![3]]);
    assert_eq!(width_of(&space, 0), 500);
}

#[test]
fn a_list_that_swaps_two_columns_moves_the_tiles_with_them() {
    let mut space = strip(options(0.25), 4);
    space.focus(&1);
    space.consume_right();
    space.set_width(&1, ColumnWidth::Fixed(450));

    space.reconcile(&[3, 1, 2, 0], Some(&1));

    assert_eq!(columns(&space), [vec![3], vec![1, 2], vec![0]]);
    assert_eq!(width_of(&space, 1), 450);
}

#[test]
fn a_list_that_moves_a_tile_into_another_column_is_followed() {
    let mut space = strip(options(0.25), 3);
    space.focus(&0);
    space.consume_right();

    // the tile that was below 0 is now below 2, which is in a column of its own
    space.reconcile(&[0, 2, 1], Some(&0));

    assert_eq!(columns(&space), [vec![0], vec![2, 1]]);
}

#[test]
fn a_column_that_does_not_have_focus_keeps_its_focused_tile_when_the_list_reorders_its_tiles() {
    let mut space = strip(options(0.25), 4);
    space.focus(&1);
    space.consume_right();
    space.consume_right();
    assert_eq!(columns(&space), [vec![0], vec![1, 2, 3]]);
    space.focus(&3);
    space.focus(&0);

    // the host swapped the last two tiles of the column on the right
    space.reconcile(&[0, 1, 3, 2], Some(&0));
    space.focus_right();

    assert_eq!(
        space.active_id(),
        Some(&3),
        "it is still the one that had focus"
    );
    assert_eq!(space.active_tile_index(), Some(2));
}

#[test]
fn a_column_that_loses_its_first_tile_to_consuming_keeps_its_focused_tile() {
    let mut space = strip(options(0.25), 4);
    space.focus(&1);
    space.consume_right();
    space.consume_right();
    space.focus(&3);
    space.focus(&0);
    assert_eq!(columns(&space), [vec![0], vec![1, 2, 3]]);

    assert!(space.consume_right());
    space.focus_right();

    assert_eq!(columns(&space), [vec![0, 1], vec![2, 3]]);
    assert_eq!(space.active_id(), Some(&3));
}

#[test]
fn consuming_to_the_left_forgets_which_column_a_new_window_was_opened_next_to() {
    let mut space = strip(options(0.25), 3);
    space.focus(&1);
    // 9 is opened next to 1, so closing it would go back to 1
    space.add_column(9, None, None, true);
    assert_eq!(ids(&space), [0, 1, 9, 2]);

    // but 1 is now in a column with it, and 1 is not what is to the left of 9 any more
    space.consume_left();
    assert_eq!(columns(&space), [vec![0], vec![1, 9], vec![2]]);
    space.remove_column(&9);
    space.remove_column(&1);

    // so the column that takes its place is the one on the right, as when anything else is closed
    assert_eq!(space.active_id(), Some(&2));
}

// Going to another strip

#[test]
fn a_tile_that_comes_from_another_strip_is_as_wide_as_it_was_there() {
    let mut there = strip(options(0.25), 3);
    there.set_width(&1, ColumnWidth::Fixed(555));
    there.set_width_limits(&1, Some(500), None);
    let record = there.column_record(&1).unwrap();

    let mut here = strip(options(0.25), 2);
    here.arrive(9, record);
    here.reconcile(&[0, 1, 9], Some(&9));

    assert_eq!(width_of(&here, 9), 555);
    assert_eq!(here.width_limits(&9), Some((Some(500), None)));
    assert_eq!(width_of(&here, 0), 300, "and nothing else changed");
}

#[test]
fn a_tile_that_comes_from_a_column_with_a_preset_carries_on_from_it() {
    let mut there = strip(options(0.25), 2);
    there.cycle_preset_width(&0, true);
    there.cycle_preset_width(&0, true);
    assert_eq!(width_of(&there, 0), 600);
    let record = there.column_record(&0).unwrap();

    let mut here = strip(options(0.25), 1);
    here.arrive(9, record);
    here.reconcile(&[0, 9], Some(&9));
    here.cycle_preset_width(&9, true);

    // 600 is the second preset, so the next is the third
    assert_eq!(width_of(&here, 9), 800);
}

#[test]
fn the_width_of_a_full_width_column_is_carried_with_it() {
    let mut there = strip(options(0.25), 2);
    there.toggle_full_width(&0);
    let record = there.column_record(&0).unwrap();

    let mut here = strip(options(0.25), 1);
    here.arrive(9, record);
    here.reconcile(&[0, 9], Some(&9));

    assert_eq!(width_of(&here, 9), 1200);
}

#[test]
fn a_tile_that_was_in_a_column_of_several_arrives_as_a_column_of_its_own() {
    let mut there = strip(options(0.25), 3);
    there.focus(&0);
    there.consume_right();
    there.set_width(&0, ColumnWidth::Fixed(450));
    there.set_tile_height(&1, komorebi_scrolling::TileHeight::Fixed(600));
    let record = there.column_record(&1).unwrap();

    let mut here = strip(options(0.25), 1);
    here.arrive(1, record);
    here.reconcile(&[0, 1], Some(&1));

    // it has the width of the column that it left, and the height of a column of its own
    assert_eq!(width_of(&here, 1), 450);
    assert_eq!(rect(&here, 1).height, 800);
    assert_eq!(here.column_of(&1), Some(vec![1]));
}

#[test]
fn what_arrives_is_used_once_and_only_by_a_tile_that_is_added() {
    let mut there = strip(options(0.25), 2);
    there.set_width(&0, ColumnWidth::Fixed(555));
    let record = there.column_record(&0).unwrap();

    let mut here = strip(options(0.25), 2);
    // 1 is there already, so there is nothing to be told about it
    here.arrive(1, record.clone());
    here.arrive(9, record);

    here.reconcile(&[0, 1, 9], Some(&9));
    assert_eq!(width_of(&here, 1), 300);
    assert_eq!(width_of(&here, 9), 555);

    // and it is not remembered: a tile that goes and comes back is the default
    here.remove_column(&9);
    here.reconcile(&[0, 1, 9], Some(&9));
    assert_eq!(width_of(&here, 9), 300);
}

#[test]
fn a_tile_that_is_added_with_a_width_of_its_own_has_it() {
    let mut there = strip(options(0.25), 2);
    there.set_width(&0, ColumnWidth::Fixed(555));
    let record = there.column_record(&0).unwrap();

    let mut here = strip(options(0.25), 1);
    here.arrive(9, record);
    here.add_column(9, None, Some(ColumnWidth::Fixed(222)), true);

    assert_eq!(width_of(&here, 9), 222);
}

#[test]
fn a_strip_does_not_wait_for_ever_for_tiles_that_never_come() {
    let mut there = strip(options(0.25), 2);
    there.set_width(&0, ColumnWidth::Fixed(555));
    let record = there.column_record(&0).unwrap();

    let mut here = strip(options(0.25), 1);
    for id in 100..140 {
        here.arrive(id, record.clone());
    }

    // the oldest were forgotten, and the newest are there
    here.reconcile(&[0, 100, 139], Some(&0));
    assert_eq!(width_of(&here, 100), 300);
    assert_eq!(width_of(&here, 139), 555);
}

#[test]
fn nothing_is_known_about_a_tile_that_is_not_there() {
    let space = strip(options(0.25), 2);

    assert_eq!(space.column_record(&9), None);
}
