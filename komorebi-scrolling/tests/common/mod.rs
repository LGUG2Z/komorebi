#![allow(dead_code)]

use komorebi_scrolling::ColumnWidth;
use komorebi_scrolling::Options;
use komorebi_scrolling::Placement;
use komorebi_scrolling::Rect;
use komorebi_scrolling::ScrollingSpace;
use komorebi_scrolling::Viewport;

pub const VIEW: Viewport = Viewport {
    width: 1200,
    height: 800,
};

/// Options where every new column is `proportion` of the viewport wide, and there are no gaps.
pub fn options(proportion: f64) -> Options {
    Options {
        default_width: ColumnWidth::Proportion(proportion),
        ..Options::default()
    }
}

/// A strip of `count` columns, keyed 0, 1, 2..., each opened next to the one before and in focus.
pub fn strip(options: Options, count: u32) -> ScrollingSpace<u32> {
    let mut space = ScrollingSpace::new(options, VIEW);
    for id in 0..count {
        space.add_column(id, None, None, true);
    }

    space
}

pub fn placement(space: &ScrollingSpace<u32>, id: u32) -> Placement<u32> {
    space
        .layout()
        .into_iter()
        .find(|placement| placement.id == id)
        .unwrap_or_else(|| panic!("there is no column {id}"))
}

pub fn rect(space: &ScrollingSpace<u32>, id: u32) -> Rect {
    placement(space, id).rect
}

pub fn x_of(space: &ScrollingSpace<u32>, id: u32) -> i32 {
    rect(space, id).x
}

pub fn width_of(space: &ScrollingSpace<u32>, id: u32) -> i32 {
    rect(space, id).width
}

pub fn widths(space: &ScrollingSpace<u32>) -> Vec<(u32, i32)> {
    space
        .layout()
        .into_iter()
        .map(|placement| (placement.id, placement.rect.width))
        .collect()
}

pub fn rects(space: &ScrollingSpace<u32>) -> Vec<(u32, Rect)> {
    space
        .layout()
        .into_iter()
        .map(|placement| (placement.id, placement.rect))
        .collect()
}
