//! What the `layouts`, `flipping`, `layout_flip`, `padding` and `tiling` end-to-end scenarios
//! assert about where the layout puts the windows, asserted on the arithmetic alone: every layout,
//! with every count of windows the scenarios use (and more), is inside the work area, with no
//! window over another, and mirrored by a flip.
//!
//! Not covered here, and so still end-to-end: that a daemon takes the layout's rectangles to the
//! operating system and that the operating system's frames agree with them (border widths, client
//! rounding, timing).

use komorebi_layouts::Arrangement;
use komorebi_layouts::Axis;
use komorebi_layouts::DefaultLayout;
use komorebi_layouts::Rect;
use std::num::NonZeroUsize;

/// The layouts the `layouts` scenario goes through (its `LAYOUTS`).
const LAYOUTS: [DefaultLayout; 8] = [
    DefaultLayout::BSP,
    DefaultLayout::Columns,
    DefaultLayout::Rows,
    DefaultLayout::VerticalStack,
    DefaultLayout::HorizontalStack,
    DefaultLayout::UltrawideVerticalStack,
    DefaultLayout::Grid,
    DefaultLayout::RightMainVerticalStack,
];

/// The layouts that tile the whole of the work area (`Grid` leaves a gap with a count it cannot
/// fill with whole cells).
fn fills_the_area(layout: DefaultLayout) -> bool {
    layout != DefaultLayout::Grid
}

const AREAS: [Rect; 3] = [
    Rect {
        left: 0,
        top: 0,
        right: 1920,
        bottom: 1080,
    },
    // a work area that is not at the origin, as one of a second monitor is
    Rect {
        left: 1920,
        top: 40,
        right: 1280,
        bottom: 984,
    },
    // one that is not a nice number
    Rect {
        left: -1366,
        top: 0,
        right: 1366,
        bottom: 728,
    },
];

fn tiles(
    layout: DefaultLayout,
    area: &Rect,
    count: usize,
    padding: Option<i32>,
    flip: Option<Axis>,
) -> Vec<Rect> {
    layout.calculate(
        area,
        NonZeroUsize::new(count).unwrap(),
        padding,
        flip,
        &[],
        0,
        None,
        &[],
    )
}

fn inside(r: &Rect, area: &Rect) -> bool {
    r.left >= area.left
        && r.top >= area.top
        && r.left + r.right <= area.left + area.right
        && r.top + r.bottom <= area.top + area.bottom
}

fn overlap(a: &Rect, b: &Rect) -> bool {
    a.left < b.left + b.right
        && b.left < a.left + a.right
        && a.top < b.top + b.bottom
        && b.top < a.top + a.bottom
}

fn area_of(r: &Rect) -> i64 {
    i64::from(r.right) * i64::from(r.bottom)
}

/// `r` mirrored in `area` along `axis`.
fn mirrored(r: &Rect, area: &Rect, axis: Axis) -> Rect {
    let mut m = *r;
    if matches!(axis, Axis::Horizontal | Axis::HorizontalAndVertical) {
        m.left = area.left + (area.left + area.right) - (r.left + r.right);
    }
    if matches!(axis, Axis::Vertical | Axis::HorizontalAndVertical) {
        m.top = area.top + (area.top + area.bottom) - (r.top + r.bottom);
    }
    m
}

fn sorted(mut v: Vec<Rect>) -> Vec<Rect> {
    v.sort_by_key(|r| (r.left, r.top, r.right, r.bottom));
    v
}

// mirrors: layouts / <layout>, <n> windows: every window is where the layout says
#[test]
fn every_layout_gives_one_tile_per_window() {
    for layout in LAYOUTS {
        for area in &AREAS {
            for count in 1..=12 {
                assert_eq!(
                    tiles(layout, area, count, None, None).len(),
                    count,
                    "{layout}, {count} windows"
                );
            }
        }
    }
}

// mirrors: layouts / <layout>, <n> windows: ... inside the work area
// mirrors: layouts / <layout>, <n> windows: ... and not on each other
// mirrors: tiling / <title> is inside the work area; <a> and <b> do not overlap
#[test]
fn every_layout_keeps_every_tile_inside_the_work_area_and_off_the_others() {
    for layout in LAYOUTS {
        for area in &AREAS {
            for count in 1..=12 {
                for padding in [None, Some(0), Some(10), Some(30)] {
                    let t = tiles(layout, area, count, padding, None);
                    for (i, r) in t.iter().enumerate() {
                        assert!(
                            inside(r, area),
                            "{layout}, {count} windows, padding {padding:?}: tile {i} {r:?} is outside {area:?}"
                        );
                        // BSP halves a tile per window: past eight windows, with padding on each
                        // side, a tile of a 1080 px high area is legitimately nothing
                        assert!(
                            count > 8 || r.right > 0 && r.bottom > 0,
                            "{layout}, {count} windows, padding {padding:?}: tile {i} {r:?} is empty"
                        );
                        for (j, o) in t.iter().enumerate().skip(i + 1) {
                            assert!(
                                !overlap(r, o),
                                "{layout}, {count} windows, padding {padding:?}: tiles {i} {r:?} and {j} {o:?} overlap"
                            );
                        }
                    }
                }
            }
        }
    }
}

// mirrors: tiling / the windows share the work area: none takes all of its width
// mirrors: closing / the windows that are left take more room between them
#[test]
fn without_padding_the_tiles_fill_the_work_area_and_fewer_windows_get_more_room() {
    for layout in LAYOUTS.into_iter().filter(|l| fills_the_area(*l)) {
        for area in &AREAS {
            for count in 1..=12 {
                let t = tiles(layout, area, count, None, None);
                let total: i64 = t.iter().map(area_of).sum();
                assert_eq!(
                    total,
                    area_of(area),
                    "{layout}, {count} windows leave a gap in {area:?}: {t:?}"
                );
            }
            // one window fewer: each of the others has at least as much room on average
            for count in 2..=12 {
                let more = tiles(layout, area, count - 1, None, None);
                let less = tiles(layout, area, count, None, None);
                let biggest = |v: &[Rect]| v.iter().map(area_of).max().unwrap();
                assert!(
                    biggest(&more) >= biggest(&less),
                    "{layout}: {count} windows has a bigger tile than {} windows",
                    count - 1
                );
            }
        }
    }
}

// mirrors: layouts / <layout>, 1 windows: ... (a single window has the whole work area, less
// padding)
#[test]
fn a_single_window_has_the_work_area_less_the_container_padding() {
    for layout in LAYOUTS {
        for area in &AREAS {
            assert_eq!(tiles(layout, area, 1, None, None), vec![*area], "{layout}");
            let p = 10;
            let t = tiles(layout, area, 1, Some(p), None);
            assert_eq!(
                t,
                vec![Rect {
                    left: area.left + p,
                    top: area.top + p,
                    right: area.right - 2 * p,
                    bottom: area.bottom - 2 * p,
                }],
                "{layout}"
            );
        }
    }
}

// mirrors: padding / a container padding 30 px more leaves the windows less room
// mirrors: padding / with the padding as it was, the windows ... have the room they had
#[test]
fn container_padding_takes_room_from_every_tile_and_taking_it_away_gives_it_back() {
    for layout in LAYOUTS {
        let area = &AREAS[0];
        for count in 1..=8 {
            let plain = tiles(layout, area, count, None, None);
            let padded = tiles(layout, area, count, Some(30), None);
            for (i, (a, b)) in plain.iter().zip(&padded).enumerate() {
                assert!(
                    area_of(b) < area_of(a),
                    "{layout}, {count} windows: tile {i} did not shrink: {a:?} -> {b:?}"
                );
            }
            assert_eq!(
                plain,
                tiles(layout, area, count, None, None),
                "{layout}: deterministic"
            );
            assert_eq!(
                plain,
                tiles(layout, area, count, Some(0), None),
                "{layout}, {count} windows: padding 0 is no padding"
            );
        }
    }
}

// mirrors: flipping / after 'flip-layout horizontal' every window is where the mirror of its place is
// mirrors: flipping / flipped again, every window is where it was
// mirrors: layout_flip / <label>: flipped along both axes
#[test]
fn a_flipped_layout_is_the_layout_mirrored_as_a_set_of_tiles() {
    for layout in LAYOUTS {
        for area in &AREAS {
            for count in 1..=8 {
                for axis in [
                    Axis::Horizontal,
                    Axis::Vertical,
                    Axis::HorizontalAndVertical,
                ] {
                    let plain = tiles(layout, area, count, None, None);
                    let flipped = tiles(layout, area, count, None, Some(axis));
                    let want: Vec<Rect> = plain.iter().map(|r| mirrored(r, area, axis)).collect();
                    assert_eq!(
                        sorted(flipped),
                        sorted(want),
                        "{layout}, {count} windows, flip {axis}"
                    );
                }
            }
        }
    }
}

// mirrors: layout_flip / <label>: ... the windows have only gone from top to bottom
#[test]
fn a_vertical_flip_only_moves_a_tile_up_or_down() {
    for layout in LAYOUTS {
        let area = &AREAS[0];
        for count in 1..=8 {
            let plain = tiles(layout, area, count, None, None);
            let flipped = tiles(layout, area, count, None, Some(Axis::Vertical));
            // the same widths and the same columns: each tile has a partner at the same left and width
            let columns = |v: &[Rect]| {
                sorted(
                    v.iter()
                        .map(|r| Rect {
                            left: r.left,
                            top: 0,
                            right: r.right,
                            bottom: 0,
                        })
                        .collect(),
                )
            };
            assert_eq!(
                columns(&plain),
                columns(&flipped),
                "{layout}, {count} windows"
            );
        }
    }
}

// mirrors: layouts / <layout>, <n> windows: every window is where the layout says (the layout
// is the same whenever it is asked for, so the daemon can compare with the OS)
#[test]
fn the_layout_is_a_function_of_its_arguments() {
    for layout in LAYOUTS {
        for count in 1..=8 {
            let a = tiles(layout, &AREAS[1], count, Some(10), Some(Axis::Horizontal));
            let b = tiles(layout, &AREAS[1], count, Some(10), Some(Axis::Horizontal));
            assert_eq!(a, b, "{layout}");
        }
    }
}
