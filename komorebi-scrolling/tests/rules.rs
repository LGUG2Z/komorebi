//! The rules that the crate documentation promises, one test each, written as specifications.
//!
//! Unless a test says otherwise, the viewport is 1200 by 800, there are no gaps, and columns are
//! a quarter of the viewport wide (300), so four of them fit.

mod common;

use common::*;
use komorebi_scrolling::CenterFocusedColumn;
use komorebi_scrolling::ColumnWidth;
use komorebi_scrolling::Options;
use komorebi_scrolling::Rect;
use komorebi_scrolling::ScrollingSpace;
use komorebi_scrolling::Viewport;
use komorebi_scrolling::Visibility;

// Rule 1: a column's width is its own

#[test]
fn opening_columns_never_changes_the_width_of_the_others() {
    let mut space = ScrollingSpace::new(options(0.25), VIEW);

    for id in 0..9 {
        space.add_column(id, None, None, true);

        for (_, width) in widths(&space) {
            assert_eq!(width, 300, "after opening column {id}");
        }
    }
}

#[test]
fn closing_columns_never_changes_the_width_of_the_others() {
    let mut space = strip(options(0.25), 6);
    space.set_width(&2, ColumnWidth::Fixed(500));
    let before = widths(&space);

    space.remove_column(&0);
    space.remove_column(&5);

    let expected: Vec<_> = before
        .into_iter()
        .filter(|(id, _)| *id != 0 && *id != 5)
        .collect();
    assert_eq!(widths(&space), expected);
}

#[test]
fn moving_focus_never_changes_a_width() {
    let mut space = strip(options(0.25), 6);
    space.set_width(&1, ColumnWidth::Fixed(555));
    let before = widths(&space);

    for id in [0, 3, 5, 1, 4] {
        space.focus(&id);
        assert_eq!(widths(&space), before);
    }
}

#[test]
fn a_strip_with_more_columns_than_fit_is_wider_than_the_viewport() {
    let space = strip(options(0.5), 6);
    let layout = space.layout();

    let start = layout.first().unwrap().rect.x;
    let end = layout.last().unwrap().rect.right();
    assert_eq!(end - start, 6 * 600);
}

#[test]
fn a_lone_column_is_not_stretched_to_fill_the_viewport() {
    let space = strip(options(0.5), 1);

    assert_eq!(
        rect(&space, 0),
        Rect {
            x: 0,
            y: 0,
            width: 600,
            height: 800
        }
    );
}

// Rule 2: the focused column does not move by itself

/// A strip of eight columns with column 5 in focus, and what column 5's place on screen is.
fn strip_with_a_focused_column() -> (ScrollingSpace<u32>, i32) {
    let mut space = strip(options(0.25), 8);
    space.focus(&5);

    let x = x_of(&space, 5);
    (space, x)
}

#[test]
fn nothing_that_happens_to_other_columns_moves_the_focused_one() {
    type Change = Box<dyn Fn(&mut ScrollingSpace<u32>)>;

    let changes: Vec<(&str, Change)> = vec![
        (
            "opening a column at the far left",
            Box::new(|s| s.add_column(100, Some(0), None, false)),
        ),
        (
            "opening a column directly to the left of it",
            Box::new(|s| s.add_column(100, Some(5), None, false)),
        ),
        (
            "opening a column directly to the right of it",
            Box::new(|s| s.add_column(100, None, None, false)),
        ),
        (
            "opening a column at the far right",
            Box::new(|s| s.add_column(100, Some(99), None, false)),
        ),
        (
            "closing a column to the left",
            Box::new(|s| assert!(s.remove_column(&1))),
        ),
        (
            "closing the column directly to the left",
            Box::new(|s| assert!(s.remove_column(&4))),
        ),
        (
            "closing a column to the right",
            Box::new(|s| assert!(s.remove_column(&7))),
        ),
        (
            "widening a column to the left",
            Box::new(|s| s.set_width(&1, ColumnWidth::Fixed(900))),
        ),
        (
            "narrowing a column to the left",
            Box::new(|s| s.set_width(&4, ColumnWidth::Fixed(100))),
        ),
        (
            "widening a column to the right",
            Box::new(|s| s.set_width(&6, ColumnWidth::Fixed(900))),
        ),
        (
            "cycling the preset of a column to the left",
            Box::new(|s| s.cycle_preset_width(&2, true)),
        ),
        (
            "making a column to the left full width",
            Box::new(|s| s.toggle_full_width(&3)),
        ),
        (
            "a column to the left being found to have a minimum width",
            Box::new(|s| s.set_width_limits(&4, Some(800), None)),
        ),
        (
            "a column to the left going fullscreen",
            Box::new(|s| s.set_fullscreen(&2, true)),
        ),
        (
            "moving a column from the left to further left",
            Box::new(|s| s.move_column(&3, 0)),
        ),
        (
            "moving a column from the left to the right of the focused one",
            Box::new(|s| s.move_column(&1, 7)),
        ),
        (
            "moving a column from the right to the left of the focused one",
            Box::new(|s| s.move_column(&7, 1)),
        ),
    ];

    for (what, change) in changes {
        let (mut space, x) = strip_with_a_focused_column();

        change(&mut space);

        assert_eq!(space.active_id(), Some(&5), "{what} changed what has focus");
        assert_eq!(x_of(&space, 5), x, "{what} moved the focused column");
    }
}

#[test]
fn widening_the_focused_column_keeps_its_left_edge_until_it_no_longer_fits() {
    let mut space = strip(options(0.25), 8);
    space.focus(&5);
    let x = x_of(&space, 5);
    assert_eq!(x, 300);

    space.set_width(&5, ColumnWidth::Fixed(600));
    assert_eq!(x_of(&space, 5), x, "it fits, so it stays");

    space.set_width(&5, ColumnWidth::Fixed(1000));
    assert_eq!(
        rect(&space, 5).right(),
        1200,
        "it does not fit, so it scrolls just far enough to"
    );
}

// Rule 3: scrolling is as little as it can be

#[test]
fn focusing_a_column_that_is_in_view_does_not_scroll() {
    let mut space = strip(options(0.25), 8);
    let before = rects(&space);

    // columns 4 to 7 are in view
    for id in [6, 4, 5, 7, 4] {
        space.focus(&id);
        assert_eq!(rects(&space), before, "focusing column {id} scrolled");
    }
}

#[test]
fn focusing_a_column_off_the_left_edge_brings_it_in_at_the_left_edge() {
    let mut space = strip(options(0.25), 8);

    space.focus(&3);

    assert_eq!(rect(&space, 3).x, 0);
    assert_eq!(placement(&space, 3).visibility, Visibility::Visible);
    // and the strip moved by exactly that much, no more
    assert_eq!(rect(&space, 4).x, 300);
    assert_eq!(rect(&space, 6).x, 900);
}

#[test]
fn focusing_a_column_off_the_right_edge_brings_it_in_at_the_right_edge() {
    let mut space = strip(options(0.25), 8);
    space.focus(&1);
    assert_eq!(rect(&space, 1).x, 0);

    space.focus(&6);

    assert_eq!(rect(&space, 6).right(), 1200);
    assert_eq!(rect(&space, 5).x, 600);
}

#[test]
fn a_column_that_is_partly_in_view_counts_as_out_of_view() {
    let mut space = strip(options(0.25), 8);
    space.focus(&3);
    space.set_width(&3, ColumnWidth::Fixed(700));
    space.focus(&1);
    // column 1 is at the left edge and 2 follows it, so column 3 starts at 600 and ends at 1300,
    // and is cut off by the right edge
    assert_eq!(rect(&space, 3).x, 600);
    assert_eq!(placement(&space, 3).visibility, Visibility::Clipped);

    space.focus(&3);

    assert_eq!(placement(&space, 3).visibility, Visibility::Visible);
    assert_eq!(rect(&space, 3).right(), 1200);
}

#[test]
fn gaps_are_left_between_a_column_that_is_brought_in_and_the_edge() {
    let options = Options {
        gaps: 16,
        ..options(0.25)
    };
    let mut space = strip(options, 8);

    // each column is 1184 * 0.25 - 16 = 280 wide
    assert_eq!(width_of(&space, 0), 280);
    assert_eq!(
        rect(&space, 7).right(),
        1200 - 16,
        "brought in from the right"
    );

    space.focus(&3);
    assert_eq!(rect(&space, 3).x, 16, "brought in from the left");
}

// Rule 4: temporary changes are undone

#[test]
fn closing_a_column_that_was_just_opened_returns_to_the_one_before_it() {
    let mut space = strip(options(0.25), 8);
    space.focus(&3);
    space.focus(&6);
    assert_eq!(
        rect(&space, 6).x,
        900,
        "column 6 is at the right edge, with column 7 beyond"
    );
    let before = rects(&space);

    space.add_column(100, None, None, true);
    assert_eq!(space.active_id(), Some(&100));
    assert_ne!(rects(&space), before);

    space.remove_column(&100);

    assert_eq!(space.active_id(), Some(&6));
    assert_eq!(rects(&space), before, "the view is back where it was");
}

#[test]
fn once_focus_has_moved_closing_a_column_does_not_return() {
    let mut space = strip(options(0.25), 8);
    space.focus(&3);
    space.focus(&6);

    space.add_column(100, None, None, true);
    space.focus(&5);
    space.focus(&100);
    space.remove_column(&100);

    // there is nothing to return to, so the column that took the closed column's place has focus
    assert_eq!(space.active_id(), Some(&7));
}

#[test]
fn closing_the_column_to_return_to_cancels_the_return() {
    let mut space = strip(options(0.25), 8);
    space.focus(&3);
    space.focus(&6);

    space.add_column(100, None, None, true);
    space.remove_column(&0);
    space.remove_column(&100);

    assert_eq!(space.active_id(), Some(&7));
}

#[test]
fn opening_a_column_between_the_two_cancels_the_return() {
    let mut space = strip(options(0.25), 8);
    space.focus(&3);
    space.focus(&6);

    space.add_column(100, None, None, true);
    // a column now sits between 6 and 100, so 6 is no longer the one that 100 was opened next to
    let at = space.position(&100).unwrap();
    space.add_column(101, Some(at), None, false);
    assert_eq!(space.ids()[6..9], [6, 101, 100]);
    space.remove_column(&100);

    assert_eq!(
        space.active_id(),
        Some(&7),
        "the column after the closed one has focus"
    );
}

#[test]
fn a_column_opened_anywhere_else_is_not_returned_from() {
    let mut space = strip(options(0.25), 8);
    space.focus(&6);

    space.add_column(100, Some(0), None, true);
    space.remove_column(&100);

    // it was on the far left, so it was not next to the old column and nothing is remembered
    assert_eq!(space.active_id(), Some(&0));
}

#[test]
fn closing_the_focused_column_otherwise_focuses_the_one_that_takes_its_place() {
    let mut space = strip(options(0.25), 8);
    space.focus(&4);
    let view_pos = space.view_offset();

    space.remove_column(&4);

    assert_eq!(space.active_id(), Some(&5));
    assert_eq!(
        space.view_offset(),
        view_pos,
        "the viewport stays put when it can"
    );
}

#[test]
fn closing_the_last_column_focuses_the_one_before_it() {
    let mut space = strip(options(0.25), 8);

    space.remove_column(&7);

    assert_eq!(space.active_id(), Some(&6));
    assert_eq!(placement(&space, 6).visibility, Visibility::Visible);
}

#[test]
fn closing_every_column_leaves_an_empty_strip_that_can_be_used_again() {
    let mut space = strip(options(0.5), 3);

    for id in 0..3 {
        assert!(space.remove_column(&id));
    }
    assert!(space.is_empty());
    assert!(space.layout().is_empty());
    assert_eq!(space.active_id(), None);

    space.add_column(7, None, None, true);
    assert_eq!(rect(&space, 7).x, 0);
    assert_eq!(space.active_id(), Some(&7));
}

#[test]
fn leaving_fullscreen_puts_the_viewport_back() {
    let mut space = strip(options(0.25), 8);
    space.focus(&6);
    let before = rects(&space);

    space.set_fullscreen(&6, true);
    assert_eq!(
        rect(&space, 6),
        Rect {
            x: 0,
            y: 0,
            width: 1200,
            height: 800
        }
    );

    space.set_fullscreen(&6, false);
    assert_eq!(rects(&space), before);
}

#[test]
fn the_viewport_is_not_put_back_after_focus_has_been_somewhere_else() {
    let mut space = strip(options(0.25), 8);
    space.focus(&6);

    space.set_fullscreen(&6, true);
    space.focus(&2);
    space.focus(&6);
    space.set_fullscreen(&6, false);

    // it is just brought into view, like any column
    assert_eq!(placement(&space, 6).visibility, Visibility::Visible);
}

// Rule 5: a column wider than the viewport starts at its left edge

#[test]
fn a_column_wider_than_the_viewport_is_aligned_with_the_left_edge() {
    let mut space = strip(options(0.25), 4);

    space.set_width(&2, ColumnWidth::Fixed(2000));
    space.focus(&2);
    assert_eq!(rect(&space, 2).x, 0);
    assert_eq!(rect(&space, 2).width, 2000);

    // and still is after going away and coming back
    space.focus(&0);
    space.focus(&2);
    assert_eq!(rect(&space, 2).x, 0);
}

#[test]
fn a_column_as_wide_as_the_viewport_is_aligned_with_the_left_edge_too() {
    let mut space = strip(options(0.25), 3);

    space.set_width(&1, ColumnWidth::Fixed(1200));
    space.focus(&1);

    assert_eq!(rect(&space, 1).x, 0);
}

// Rule 6: proportions include the gaps

#[test]
fn columns_that_add_up_to_the_viewport_tile_it_whatever_the_gaps() {
    for gaps in [0, 1, 7, 16, 25] {
        for count in 1..=8_u32 {
            let options = Options {
                gaps,
                ..options(1.0 / f64::from(count))
            };
            let space = strip(options, count);
            let layout = space.layout();

            assert_eq!(
                layout.first().unwrap().rect.x,
                gaps,
                "gaps {gaps}, columns {count}"
            );
            assert_eq!(
                layout.last().unwrap().rect.right(),
                1200 - gaps,
                "gaps {gaps}, columns {count}"
            );

            for pair in layout.windows(2) {
                let gap = pair[1].rect.x - pair[0].rect.right();
                assert_eq!(
                    gap, gaps,
                    "gaps {gaps}, columns {count}, after column {}",
                    pair[0].id
                );
            }
        }
    }
}

#[test]
fn a_fixed_width_does_not_depend_on_the_gaps() {
    let options = Options {
        gaps: 40,
        default_width: ColumnWidth::Fixed(500),
        ..Options::default()
    };
    let space = strip(options, 3);

    for (_, width) in widths(&space) {
        assert_eq!(width, 500);
    }
}

#[test]
fn columns_leave_a_gap_at_the_top_and_bottom_too() {
    let options = Options {
        gaps: 16,
        ..options(0.5)
    };
    let space = strip(options, 1);

    let rect = rect(&space, 0);
    assert_eq!((rect.y, rect.bottom()), (16, 800 - 16));
}

// Rule 7: fullscreen is a column like any other

#[test]
fn a_fullscreen_column_can_be_scrolled_away_from_and_back_to() {
    let mut space = strip(options(0.5), 3);
    space.focus(&1);
    space.set_fullscreen(&1, true);

    assert_eq!(placement(&space, 0).visibility, Visibility::OffLeft);
    assert_eq!(placement(&space, 2).visibility, Visibility::OffRight);

    // focus moves on to the next column, which is brought in from the right
    space.focus(&2);
    assert_eq!(placement(&space, 2).visibility, Visibility::Visible);
    assert_eq!(rect(&space, 2).right(), 1200);

    // the fullscreen column is still fullscreen, and still where it was in the strip: it is as
    // wide as the viewport, so with the 600 of column 2 in view, half of it is still showing
    let fullscreen = placement(&space, 1);
    assert_eq!(fullscreen.rect.width, 1200);
    assert_eq!(fullscreen.rect.height, 800);
    assert_eq!(fullscreen.rect.x, -600);
    assert_eq!(fullscreen.visibility, Visibility::Clipped);

    space.focus(&1);
    assert_eq!(
        rect(&space, 1),
        Rect {
            x: 0,
            y: 0,
            width: 1200,
            height: 800
        }
    );
}

#[test]
fn a_fullscreen_column_ignores_the_gaps() {
    let options = Options {
        gaps: 16,
        ..options(0.5)
    };
    let mut space = strip(options, 2);

    space.set_fullscreen(&1, true);

    assert_eq!(
        rect(&space, 1),
        Rect {
            x: 0,
            y: 0,
            width: 1200,
            height: 800
        }
    );
}

// Centring

fn centered(mode: CenterFocusedColumn) -> Options {
    Options {
        center_focused_column: mode,
        ..options(0.25)
    }
}

#[test]
fn with_always_the_focused_column_is_always_centred() {
    let mut space = strip(centered(CenterFocusedColumn::Always), 7);

    for id in [3, 0, 6, 2, 4] {
        space.focus(&id);
        assert_eq!(rect(&space, id).x, 450, "column {id}");
    }
}

#[test]
fn a_column_that_is_too_wide_to_centre_is_aligned_with_the_left_edge() {
    let mut space = strip(centered(CenterFocusedColumn::Always), 3);
    space.set_width(&1, ColumnWidth::Fixed(1500));

    space.focus(&1);

    assert_eq!(rect(&space, 1).x, 0);
}

#[test]
fn with_never_a_column_in_view_is_not_centred() {
    let mut space = strip(centered(CenterFocusedColumn::Never), 3);
    assert_eq!(rect(&space, 0).x, 0);

    space.focus(&0);

    assert_eq!(rect(&space, 0).x, 0);
}

#[test]
fn on_overflow_a_column_that_fits_beside_the_last_one_is_just_brought_in() {
    // 480 wide: two of them are 960, which fits
    let options = Options {
        center_focused_column: CenterFocusedColumn::OnOverflow,
        ..options(0.4)
    };
    let mut space = strip(options, 4);
    space.focus(&0);
    assert_eq!(rect(&space, 0).x, 0);

    space.focus(&1);
    assert_eq!(rect(&space, 1).x, 480, "in view, so it does not move");

    space.focus(&2);
    assert_eq!(rect(&space, 2).right(), 1200, "brought in from the right");
}

#[test]
fn on_overflow_a_column_that_does_not_fit_beside_the_last_one_is_centred() {
    // 720 wide: two of them are 1440, which does not
    let options = Options {
        center_focused_column: CenterFocusedColumn::OnOverflow,
        ..options(0.6)
    };
    let mut space = strip(options, 4);
    space.focus(&0);

    space.focus(&1);
    assert_eq!(rect(&space, 1).x, 240, "(1200 - 720) / 2");

    space.focus(&0);
    assert_eq!(rect(&space, 0).x, 240);
}

#[test]
fn a_lone_column_can_be_kept_centred_whatever_the_mode() {
    let options = Options {
        always_center_single_column: true,
        ..options(0.5)
    };
    let mut space = strip(options, 1);
    assert_eq!(rect(&space, 0).x, 300);

    space.add_column(1, None, None, true);
    assert_ne!(
        rect(&space, 1).x,
        300,
        "with two columns it is the usual rule again"
    );

    space.remove_column(&1);
    assert_eq!(rect(&space, 0).x, 300);
}

#[test]
fn centre_column_centres_the_focused_column_once() {
    let mut space = strip(options(0.25), 8);
    space.focus(&6);

    space.center_column();
    assert_eq!(rect(&space, 6).x, 450);

    // it is not sticky: the next column that is in view does not move the strip
    space.focus(&7);
    assert_eq!(rect(&space, 6).x, 450);
}

#[test]
fn centre_visible_columns_centres_the_group_that_is_in_view() {
    let mut space = strip(options(0.25), 2);
    assert_eq!((x_of(&space, 0), x_of(&space, 1)), (0, 300));

    space.center_visible_columns();

    assert_eq!((x_of(&space, 0), x_of(&space, 1)), (300, 600));
}

#[test]
fn centre_visible_columns_does_nothing_if_the_focused_column_is_not_fully_in_view() {
    let mut space = strip(options(0.25), 8);
    space.set_width(&7, ColumnWidth::Fixed(2000));
    let before = rects(&space);

    space.center_visible_columns();

    assert_eq!(rects(&space), before);
}

#[test]
fn expand_fills_what_the_columns_in_view_leave_free() {
    let mut space = strip(options(0.25), 2);
    space.focus(&0);

    space.expand_to_available_width();

    assert_eq!(width_of(&space, 0), 900);
    assert_eq!(x_of(&space, 0), 0);
    assert_eq!(x_of(&space, 1), 900);
    assert_eq!(rect(&space, 1).right(), 1200);
}

#[test]
fn expand_on_the_only_column_in_view_makes_it_full_width_which_can_be_undone() {
    let mut space = strip(options(0.25), 1);

    space.expand_to_available_width();
    assert_eq!(width_of(&space, 0), 1200);

    space.toggle_full_width(&0);
    assert_eq!(width_of(&space, 0), 300);
}

#[test]
fn expand_again_does_not_make_the_column_full_width_when_the_columns_add_up_to_a_fraction_over() {
    // thirds of 5092 are 1697.33 wide, so what is left after one that was made 200 wider does
    // not come to a whole number of pixels
    let mut space = ScrollingSpace::new(
        options(1.0 / 3.0),
        Viewport {
            width: 5092,
            height: 800,
        },
    );
    space.add_column(0, None, None, true);
    space.add_column(1, None, None, true);
    space.set_width(&1, ColumnWidth::Fixed(1897));

    space.expand_to_available_width();
    let once = rects(&space);
    assert_eq!(width_of(&space, 1), 3394);

    for _ in 0..3 {
        space.expand_to_available_width();
        assert_eq!(rects(&space), once);
    }
}

// Widths

#[test]
fn presets_are_stepped_through_in_order_and_wrap() {
    let mut space = strip(options(0.5), 1);
    // 1/3, 1/2 and 2/3 are the default presets
    let mut seen = vec![];

    for _ in 0..4 {
        space.cycle_preset_width(&0, true);
        seen.push(width_of(&space, 0));
    }

    // a column at 1/2 is not at a preset yet, so the first step goes to the next one up
    assert_eq!(seen, [800, 400, 600, 800]);
}

#[test]
fn presets_can_be_stepped_through_backwards() {
    let mut space = strip(options(0.5), 1);
    let mut seen = vec![];

    for _ in 0..4 {
        space.cycle_preset_width(&0, false);
        seen.push(width_of(&space, 0));
    }

    assert_eq!(seen, [400, 800, 600, 400]);
}

#[test]
fn a_column_narrower_than_every_preset_starts_at_the_first_one() {
    let mut space = strip(options(0.25), 1);

    space.cycle_preset_width(&0, true);

    assert_eq!(width_of(&space, 0), 400);
}

#[test]
fn setting_a_width_by_hand_forgets_the_preset() {
    let mut space = strip(options(0.5), 1);
    space.cycle_preset_width(&0, true);
    assert_eq!(width_of(&space, 0), 800);

    space.set_width(&0, ColumnWidth::Proportion(0.5));
    space.cycle_preset_width(&0, true);

    // from 600 the next one up is 800 again, not the one after 800
    assert_eq!(width_of(&space, 0), 800);
}

#[test]
fn full_width_takes_the_whole_viewport_less_the_gaps_and_gives_the_width_back() {
    // with gaps of 8 a quarter is (1200 - 8) / 4 - 8 = 290, which is a whole number of pixels
    let options = Options {
        gaps: 8,
        ..options(0.25)
    };
    let mut space = strip(options, 2);
    assert_eq!(width_of(&space, 1), 290);

    space.toggle_full_width(&1);
    assert_eq!(width_of(&space, 1), 1200 - 16);
    assert_eq!(x_of(&space, 1), 8);

    space.toggle_full_width(&1);
    assert_eq!(width_of(&space, 1), 290);
}

#[test]
fn a_width_limit_that_is_found_out_is_respected_and_the_columns_after_it_make_room() {
    let mut space = strip(options(0.25), 3);
    space.focus(&0);
    let next = x_of(&space, 1);

    space.set_width_limits(&0, Some(700), None);

    assert_eq!(width_of(&space, 0), 700);
    assert_eq!(x_of(&space, 1), next + 400);

    space.set_width_limits(&0, None, None);
    assert_eq!(width_of(&space, 0), 300);
    assert_eq!(x_of(&space, 1), next);
}

#[test]
fn the_limits_that_a_column_was_given_can_be_read_back() {
    let mut space = strip(options(0.25), 2);
    assert_eq!(space.width_limits(&0), Some((None, None)));
    assert_eq!(space.width_limits(&9), None);

    space.set_width_limits(&0, Some(700), None);
    space.set_width_limits(&1, None, Some(200));

    assert_eq!(space.width_limits(&0), Some((Some(700), None)));
    assert_eq!(space.width_limits(&1), Some((None, Some(200))));

    // a column that goes takes its limits with it
    space.remove_column(&0);
    assert_eq!(space.width_limits(&0), None);
}

#[test]
fn a_maximum_width_limits_a_column() {
    let mut space = strip(options(0.5), 1);

    space.set_width_limits(&0, None, Some(400));

    assert_eq!(width_of(&space, 0), 400);
}

#[test]
fn resetting_the_widths_puts_every_column_back_to_the_default() {
    let mut space = strip(options(0.25), 5);
    space.set_width(&0, ColumnWidth::Fixed(555));
    space.cycle_preset_width(&1, true);
    space.toggle_full_width(&2);
    space.set_width(&3, ColumnWidth::Proportion(0.75));
    space.add_column(9, None, Some(ColumnWidth::Proportion(0.4)), false);

    space.reset_widths();

    assert!(widths(&space).iter().all(|(_, width)| *width == 300));
}

#[test]
fn columns_that_were_reset_follow_the_default_again() {
    let mut space = strip(options(0.25), 3);
    space.set_width(&1, ColumnWidth::Fixed(500));

    space.reset_widths();
    space.set_options(options(0.5));

    assert!(widths(&space).iter().all(|(_, width)| *width == 600));
}

#[test]
fn resetting_the_widths_forgets_the_preset_cycling_was_at() {
    let mut space = strip(options(0.5), 1);
    space.cycle_preset_width(&0, true);
    space.cycle_preset_width(&0, true);
    assert_eq!(width_of(&space, 0), 400);

    space.reset_widths();
    space.cycle_preset_width(&0, true);

    // from 600 the first step up is 800, not the preset after 400
    assert_eq!(width_of(&space, 0), 800);
}

#[test]
fn resetting_the_widths_keeps_the_left_edge_of_the_active_column() {
    let mut space = strip(options(0.25), 6);
    space.focus(&3);
    space.set_width(&1, ColumnWidth::Fixed(500));
    space.set_width(&2, ColumnWidth::Fixed(200));
    let edge = x_of(&space, 3);

    space.reset_widths();

    // the columns before it are wider or narrower, and it has not moved
    assert_eq!(x_of(&space, 3), edge);
    assert_eq!(space.active_id(), Some(&3));
}

#[test]
fn resetting_the_widths_scrolls_the_active_column_only_as_far_as_it_has_to() {
    let mut space = strip(options(0.25), 3);
    space.set_width(&2, ColumnWidth::Fixed(1000));
    assert_eq!(x_of(&space, 2), 200);

    space.reset_widths();

    // the column is a quarter of the viewport again, and its left edge is where it was
    assert_eq!(width_of(&space, 2), 300);
    assert_eq!(x_of(&space, 2), 200);
}

#[test]
fn resetting_the_widths_brings_the_active_column_back_in_when_it_grows_past_the_edge() {
    let mut space = strip(options(0.25), 6);
    space.set_width(&5, ColumnWidth::Fixed(100));
    assert_eq!(x_of(&space, 5), 900);

    // the default is wider than the column was, and it would end past the right edge
    space.set_options(options(0.5));
    space.reset_widths();

    assert_eq!(width_of(&space, 5), 600);
    assert_eq!(x_of(&space, 5), 600);
}

#[test]
fn resetting_the_widths_keeps_what_was_found_out_about_a_window() {
    let mut space = strip(options(0.25), 3);
    space.set_width_limits(&1, Some(700), None);
    space.set_width_limits(&2, None, Some(200));
    space.set_width(&1, ColumnWidth::Fixed(500));

    space.reset_widths();

    assert_eq!(widths(&space), [(0, 300), (1, 700), (2, 200)]);
}

#[test]
fn resetting_the_widths_leaves_fullscreen_alone() {
    let mut space = strip(options(0.25), 3);
    space.focus(&1);
    space.set_fullscreen(&1, true);
    space.set_width(&2, ColumnWidth::Fixed(500));

    space.reset_widths();

    assert_eq!(width_of(&space, 1), 1200);
    assert_eq!(width_of(&space, 2), 300);

    space.set_fullscreen(&1, false);
    assert_eq!(width_of(&space, 1), 300);
}

#[test]
fn resetting_the_widths_of_an_empty_strip_does_nothing() {
    let mut space = ScrollingSpace::<u32>::new(options(0.25), VIEW);

    space.reset_widths();

    assert!(space.is_empty());
}

// The viewport and the options

#[test]
fn proportions_follow_the_viewport() {
    let mut space = strip(options(0.5), 2);

    space.set_viewport(Viewport {
        width: 1000,
        height: 600,
    });

    assert_eq!(widths(&space), [(0, 500), (1, 500)]);
    assert_eq!(rect(&space, 0).height, 600);
    assert_eq!(placement(&space, 1).visibility, Visibility::Visible);
}

#[test]
fn shrinking_the_viewport_keeps_the_focused_column_in_view() {
    let mut space = strip(
        Options {
            default_width: ColumnWidth::Fixed(500),
            ..Options::default()
        },
        3,
    );
    assert_eq!(placement(&space, 2).visibility, Visibility::Visible);

    space.set_viewport(Viewport {
        width: 700,
        height: 800,
    });

    assert_eq!(placement(&space, 2).visibility, Visibility::Visible);
}

#[test]
fn changing_the_options_applies_them_to_the_columns_there_are() {
    let mut space = strip(options(0.25), 3);

    space.set_options(Options {
        gaps: 10,
        center_focused_column: CenterFocusedColumn::Always,
        ..options(0.25)
    });

    // each is 1190 * 0.25 - 10 = 287.5 wide, and the focused one is centred
    assert_eq!(placement(&space, 2).rect.x, (1200 - 287) / 2);
}

#[test]
fn a_negative_gap_is_no_gap() {
    let space = strip(
        Options {
            gaps: -5,
            ..options(0.5)
        },
        2,
    );

    assert_eq!(space.options().gaps, 0);
    assert_eq!(widths(&space), [(0, 600), (1, 600)]);
}

// Visibility

#[test]
fn every_column_says_how_much_of_it_is_in_view() {
    let mut space = strip(options(0.25), 8);
    space.focus(&3);
    space.set_width(&3, ColumnWidth::Fixed(450));

    let visibility: Vec<_> = space
        .layout()
        .into_iter()
        .map(|placement| placement.visibility)
        .collect();

    // column 3 is at the left edge and is 450 wide, so column 4 starts at 450 and column 5 at
    // 750, and 6 at 1050, which ends at 1350: past the right edge
    assert_eq!(
        visibility,
        [
            Visibility::OffLeft,
            Visibility::OffLeft,
            Visibility::OffLeft,
            Visibility::Visible,
            Visibility::Visible,
            Visibility::Visible,
            Visibility::Clipped,
            Visibility::OffRight,
        ]
    );
}

#[test]
fn a_column_that_ends_exactly_at_the_left_edge_is_off_it() {
    let mut space = strip(options(0.25), 8);
    space.focus(&4);

    assert_eq!(rect(&space, 3).right(), 0);
    assert_eq!(placement(&space, 3).visibility, Visibility::OffLeft);
}

#[test]
fn exactly_one_column_is_active() {
    let mut space = strip(options(0.25), 5);
    space.focus(&2);

    let active: Vec<_> = space
        .layout()
        .into_iter()
        .filter(|placement| placement.active)
        .map(|placement| placement.id)
        .collect();

    assert_eq!(active, [2]);
    assert_eq!(space.active_index(), Some(2));
}

// Reconciling with a list that the host keeps

#[test]
fn reconcile_gives_the_same_strip_as_making_each_change_through_the_model() {
    // a window opens (and takes focus), another opens, the first one closes, focus moves, and a
    // window moves one place to the left
    let steps: [(&[u32], u32); 5] = [
        (&[0, 1, 2, 3, 4, 5], 5),
        (&[0, 1, 2, 3, 4, 5, 6], 6),
        (&[0, 2, 3, 4, 5, 6], 2),
        (&[0, 2, 3, 4, 5, 6], 5),
        (&[0, 2, 3, 5, 4, 6], 5),
    ];

    let mut reconciled = ScrollingSpace::new(options(0.25), VIEW);
    let mut direct = ScrollingSpace::new(options(0.25), VIEW);

    for id in 0..6 {
        direct.add_column(id, None, None, true);
    }
    reconciled.reconcile(steps[0].0, Some(&steps[0].1));
    assert_eq!(rects(&reconciled), rects(&direct));

    direct.add_column(6, None, None, true);
    reconciled.reconcile(steps[1].0, Some(&steps[1].1));
    assert_eq!(rects(&reconciled), rects(&direct));

    direct.remove_column(&1);
    direct.focus(&2);
    reconciled.reconcile(steps[2].0, Some(&steps[2].1));
    assert_eq!(rects(&reconciled), rects(&direct));

    direct.focus(&5);
    reconciled.reconcile(steps[3].0, Some(&steps[3].1));
    assert_eq!(rects(&reconciled), rects(&direct));

    direct.move_column(&5, 3);
    reconciled.reconcile(steps[4].0, Some(&steps[4].1));
    assert_eq!(rects(&reconciled), rects(&direct));
    assert_eq!(reconciled.ids(), [0, 2, 3, 5, 4, 6]);
}

#[test]
fn reconcile_keeps_the_width_of_columns_that_were_already_there() {
    let mut space = strip(options(0.25), 3);
    space.set_width(&1, ColumnWidth::Fixed(777));

    space.reconcile(&[0, 1, 2, 3], Some(&3));

    assert_eq!(width_of(&space, 1), 777);
    assert_eq!(width_of(&space, 3), 300);
}

#[test]
fn reconcile_ignores_a_key_that_is_listed_twice() {
    let mut space = ScrollingSpace::new(options(0.25), VIEW);

    space.reconcile(&[1, 2, 2, 3, 1], Some(&3));

    assert_eq!(space.ids(), [1, 2, 3]);
}

#[test]
fn reconcile_with_nothing_empties_the_strip() {
    let mut space = strip(options(0.25), 3);

    space.reconcile(&[], None);

    assert!(space.is_empty());
}

#[test]
fn reconcile_without_a_focused_key_leaves_focus_where_it_is() {
    let mut space = strip(options(0.25), 4);
    space.focus(&1);

    space.reconcile(&[0, 1, 2, 3], None);

    assert_eq!(space.active_id(), Some(&1));
}

#[test]
fn reconcile_when_the_focused_column_is_the_one_that_went_gives_focus_to_what_the_host_says() {
    let mut space = strip(options(0.25), 4);
    space.focus(&1);

    space.reconcile(&[0, 2, 3], Some(&3));

    assert_eq!(space.active_id(), Some(&3));
    assert_eq!(space.ids(), [0, 2, 3]);
}

#[test]
fn focus_movement_helpers_report_whether_they_moved() {
    let mut space = strip(options(0.25), 3);
    space.focus(&0);

    assert!(!space.focus_left());
    assert!(space.focus_right());
    assert!(space.focus_right());
    assert!(!space.focus_right());
    assert!(space.focus_left());
    assert!(!space.focus(&99));
    assert!(!space.focus_index(99));
    assert!(!space.remove_column(&99));
    assert_eq!(space.active_id(), Some(&1));

    let mut empty: ScrollingSpace<u32> = ScrollingSpace::new(options(0.25), VIEW);
    assert!(!empty.focus_left());
    assert!(!empty.focus_right());
}

#[test]
fn moving_the_focused_column_takes_focus_and_the_viewport_with_it() {
    let mut space = strip(options(0.25), 8);
    space.focus(&5);

    space.move_column(&5, 1);

    assert_eq!(space.active_id(), Some(&5));
    assert_eq!(space.active_index(), Some(1));
    assert_eq!(placement(&space, 5).visibility, Visibility::Visible);
    assert_eq!(rect(&space, 5).x, 0);
}

// Asking questions, and comparing

#[test]
fn a_column_can_be_asked_how_wide_it_is() {
    let mut space = strip(options(0.25), 3);
    assert_eq!(space.column_width(&1), Some(300.0));

    space.set_width(&1, ColumnWidth::Fixed(555));
    assert_eq!(space.column_width(&1), Some(555.0));

    space.toggle_full_width(&1);
    assert_eq!(space.column_width(&1), Some(1200.0));

    assert_eq!(space.column_width(&99), None);
}

#[test]
fn the_width_that_is_asked_for_is_not_rounded_to_pixels() {
    // with gaps of 10 a quarter is (1200 - 10) / 4 - 10 = 287.5 wide
    let options = Options {
        gaps: 10,
        ..options(0.25)
    };
    let space = strip(options, 2);

    assert_eq!(space.column_width(&0), Some(287.5));
}

// The default width

#[test]
fn columns_that_were_never_given_a_width_follow_the_default() {
    let mut space = strip(options(0.25), 4);
    space.set_width(&1, ColumnWidth::Fixed(500));
    space.add_column(9, Some(4), Some(ColumnWidth::Proportion(0.4)), false);

    space.set_options(options(0.5));

    // 1 was sized by hand and 9 was opened with a width of its own, so neither follows
    assert_eq!(
        widths(&space),
        [(0, 600), (1, 500), (2, 600), (3, 600), (9, 480)]
    );
}

#[test]
fn a_column_that_is_sized_stops_following_the_default() {
    let mut space = strip(options(0.25), 5);
    space.cycle_preset_width(&1, true);
    space.set_width(&2, ColumnWidth::Proportion(0.25));
    space.focus(&3);
    space.expand_to_available_width();
    let before = widths(&space);

    space.set_options(options(0.5));

    for id in [1, 2, 3] {
        let id_width = |widths: &[(u32, i32)]| widths.iter().find(|w| w.0 == id).unwrap().1;
        assert_eq!(id_width(&widths(&space)), id_width(&before), "column {id}");
    }
    // and 0 and 4 do
    assert_eq!(width_of(&space, 0), 600);
    assert_eq!(width_of(&space, 4), 600);
}

#[test]
fn a_column_that_is_toggled_to_full_width_and_back_follows_the_default_again() {
    let mut space = strip(options(0.25), 2);

    space.toggle_full_width(&1);
    space.set_options(options(0.5));
    space.toggle_full_width(&1);

    assert_eq!(width_of(&space, 1), 600);
}

#[test]
fn opening_a_column_never_resizes_the_others_even_though_they_follow_the_default() {
    let mut space = strip(options(0.25), 3);

    // the default is not touched by opening a column, so nothing follows anything anywhere
    space.add_column(9, None, None, true);

    assert!(widths(&space).iter().all(|(_, width)| *width == 300));
}

#[test]
fn a_strip_equals_one_that_was_made_the_same_way() {
    let mut one = strip(options(0.25), 4);
    let two = strip(options(0.25), 4);
    assert_eq!(one, two);

    one.focus(&0);
    assert_ne!(one, two, "the viewport is part of what is compared");

    let mut three = strip(options(0.25), 4);
    three.focus(&0);
    assert_eq!(one, three);
}
