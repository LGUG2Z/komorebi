use std::num::NonZeroUsize;

use clap::ValueEnum;
use schemars::JsonSchema;
use serde::Deserialize;
use serde::Serialize;
use strum::Display;
use strum::EnumString;

use crate::custom_layout::Column;
use crate::custom_layout::ColumnSplit;
use crate::custom_layout::ColumnSplitWithCapacity;
use crate::CustomLayout;
use crate::DefaultLayout;
use crate::Rect;

pub trait Arrangement {
    fn calculate(
        &self,
        area: &Rect,
        len: NonZeroUsize,
        container_padding: Option<i32>,
        layout_flip: Option<Axis>,
        resize_dimensions: &[Option<Rect>],
    ) -> Vec<Rect>;
}

impl Arrangement for DefaultLayout {
    #[allow(clippy::too_many_lines)]
    fn calculate(
        &self,
        area: &Rect,
        len: NonZeroUsize,
        container_padding: Option<i32>,
        layout_flip: Option<Axis>,
        resize_dimensions: &[Option<Rect>],
    ) -> Vec<Rect> {
        let len = usize::from(len);
        let mut dimensions = match self {
            Self::BSP => recursive_fibonacci(
                0,
                len,
                area,
                layout_flip,
                calculate_resize_adjustments(resize_dimensions),
            ),
            Self::Columns => columns(area, len),
            Self::Rows => rows(area, len),
            Self::VerticalStack => {
                let mut layouts: Vec<Rect> = vec![];

                let primary_right = match len {
                    1 => area.right,
                    _ => area.right / 2,
                };

                let mut main_left = area.left;
                let mut stack_left = area.left + primary_right;

                match layout_flip {
                    Some(Axis::Horizontal | Axis::HorizontalAndVertical) if len > 1 => {
                        main_left = main_left + area.right - primary_right;
                        stack_left = area.left;
                    }
                    _ => {}
                }

                if len >= 1 {
                    layouts.push(Rect {
                        left: main_left,
                        top: area.top,
                        right: primary_right,
                        bottom: area.bottom,
                    });

                    if len > 1 {
                        layouts.append(&mut rows(
                            &Rect {
                                left: stack_left,
                                top: area.top,
                                right: area.right - primary_right,
                                bottom: area.bottom,
                            },
                            len - 1,
                        ));
                    }
                }

                layouts
            }
            Self::HorizontalStack => {
                let mut layouts: Vec<Rect> = vec![];

                let bottom = match len {
                    1 => area.bottom,
                    _ => area.bottom / 2,
                };

                let mut main_top = area.top;
                let mut stack_top = area.top + bottom;

                match layout_flip {
                    Some(Axis::Vertical | Axis::HorizontalAndVertical) if len > 1 => {
                        main_top = main_top + area.bottom - bottom;
                        stack_top = area.top;
                    }
                    _ => {}
                }

                if len >= 1 {
                    layouts.push(Rect {
                        left: area.left,
                        top: main_top,
                        right: area.right,
                        bottom,
                    });

                    if len > 1 {
                        layouts.append(&mut columns(
                            &Rect {
                                left: area.left,
                                top: stack_top,
                                right: area.right,
                                bottom: area.bottom - bottom,
                            },
                            len - 1,
                        ));
                    }
                }

                layouts
            }
            Self::UltrawideVerticalStack => ultrawide(area, len, layout_flip, resize_dimensions),
            #[allow(
                clippy::cast_precision_loss,
                clippy::cast_possible_truncation,
                clippy::cast_possible_wrap
            )]
            Self::Grid => {
                // Shamelessly lifted from LeftWM
                // https://github.com/leftwm/leftwm/blob/18675067b8450e520ef75db2ebbb0d973aa1199e/leftwm-core/src/layouts/grid_horizontal.rs
                let mut layouts: Vec<Rect> = vec![];
                layouts.resize(len, Rect::default());

                let len = len as i32;

                let num_cols = (len as f32).sqrt().ceil() as i32;
                let mut iter = layouts.iter_mut().enumerate().peekable();

                for col in 0..num_cols {
                    let iter_peek = iter.peek().map(|x| x.0).unwrap_or_default() as i32;
                    let remaining_windows = len - iter_peek;
                    let remaining_columns = num_cols - col;
                    let num_rows_in_this_col = remaining_windows / remaining_columns;

                    let win_height = area.bottom / num_rows_in_this_col;
                    let win_width = area.right / num_cols;

                    for row in 0..num_rows_in_this_col {
                        if let Some((_idx, win)) = iter.next() {
                            let mut left = area.left + win_width * col;
                            let mut top = area.top + win_height * row;

                            match layout_flip {
                                Some(Axis::Horizontal) => {
                                    left = area.right - win_width * (col + 1) + area.left;
                                }
                                Some(Axis::Vertical) => {
                                    top = area.bottom - win_height * (row + 1) + area.top;
                                }
                                Some(Axis::HorizontalAndVertical) => {
                                    left = area.right - win_width * (col + 1) + area.left;
                                    top = area.bottom - win_height * (row + 1) + area.top;
                                }
                                None => {} // No flip
                            }

                            win.bottom = win_height;
                            win.right = win_width;
                            win.left = left;
                            win.top = top;
                        }
                    }
                }

                layouts
            }
        };

        dimensions
            .iter_mut()
            .for_each(|l| l.add_padding(container_padding.unwrap_or_default()));

        dimensions
    }
}

impl Arrangement for CustomLayout {
    fn calculate(
        &self,
        area: &Rect,
        len: NonZeroUsize,
        container_padding: Option<i32>,
        _layout_flip: Option<Axis>,
        _resize_dimensions: &[Option<Rect>],
    ) -> Vec<Rect> {
        let mut dimensions = vec![];
        let container_count = len.get();

        if container_count < self.len() {
            let mut layouts = columns(area, container_count);
            dimensions.append(&mut layouts);
        } else {
            let count_map = self.column_container_counts();

            // If there are not enough windows to trigger the final tertiary
            // column in the custom layout, use an offset to reduce the number of
            // columns to calculate each column's area by, so that we don't have
            // an empty ghost tertiary column and the screen space can be maximised
            // until there are enough windows to create it
            let mut tertiary_trigger_threshold = 0;

            // always -1 because we don't insert the tertiary column in the count_map
            for i in 0..self.len() - 1 {
                tertiary_trigger_threshold += count_map.get(&i).unwrap();
            }

            let enable_tertiary_column = len.get() > tertiary_trigger_threshold;

            let offset = if enable_tertiary_column {
                None
            } else {
                Option::from(1)
            };

            #[allow(clippy::cast_possible_truncation, clippy::cast_possible_wrap)]
            let primary_right = self.primary_width_percentage().map_or_else(
                || area.right / self.len() as i32,
                |percentage| (area.right / 100) * percentage as i32,
            );

            for (idx, column) in self.iter().enumerate() {
                // If we are offsetting a tertiary column for which the threshold
                // has not yet been met, this loop should not run for that final
                // tertiary column
                if idx < self.len() - offset.unwrap_or(0) {
                    let column_area = if idx == 0 {
                        Self::column_area_with_last(self.len(), area, primary_right, None, offset)
                    } else {
                        Self::column_area_with_last(
                            self.len(),
                            area,
                            primary_right,
                            Option::from(dimensions[self.first_container_idx(idx - 1)]),
                            offset,
                        )
                    };

                    match column {
                        Column::Primary(Option::Some(_)) => {
                            let main_column_area = if idx == 0 {
                                Self::main_column_area(area, primary_right, None)
                            } else {
                                Self::main_column_area(
                                    area,
                                    primary_right,
                                    Option::from(dimensions[self.first_container_idx(idx - 1)]),
                                )
                            };

                            dimensions.push(main_column_area);
                        }
                        Column::Primary(None) | Column::Secondary(None) => {
                            dimensions.push(column_area);
                        }
                        Column::Secondary(Some(split)) => match split {
                            ColumnSplitWithCapacity::Horizontal(capacity) => {
                                let mut rows = rows(&column_area, *capacity);
                                dimensions.append(&mut rows);
                            }
                            ColumnSplitWithCapacity::Vertical(capacity) => {
                                let mut columns = columns(&column_area, *capacity);
                                dimensions.append(&mut columns);
                            }
                        },
                        Column::Tertiary(split) => {
                            let column_area = Self::column_area_with_last(
                                self.len(),
                                area,
                                primary_right,
                                Option::from(dimensions[self.first_container_idx(idx - 1)]),
                                offset,
                            );

                            let remaining = container_count - tertiary_trigger_threshold;

                            match split {
                                ColumnSplit::Horizontal => {
                                    let mut rows = rows(&column_area, remaining);
                                    dimensions.append(&mut rows);
                                }
                                ColumnSplit::Vertical => {
                                    let mut columns = columns(&column_area, remaining);
                                    dimensions.append(&mut columns);
                                }
                            }
                        }
                    }
                }
            }
        }

        dimensions
            .iter_mut()
            .for_each(|l| l.add_padding(container_padding.unwrap_or_default()));

        dimensions
    }
}

#[derive(
    Clone, Copy, Debug, Serialize, Deserialize, Display, EnumString, ValueEnum, JsonSchema,
)]
#[strum(serialize_all = "snake_case")]
pub enum Axis {
    Horizontal,
    Vertical,
    HorizontalAndVertical,
}

#[must_use]
fn columns(area: &Rect, len: usize) -> Vec<Rect> {
    #[allow(clippy::cast_possible_wrap, clippy::cast_possible_truncation)]
    let right = area.right / len as i32;
    let mut left = 0;

    let mut layouts: Vec<Rect> = vec![];
    for _ in 0..len {
        layouts.push(Rect {
            left: area.left + left,
            top: area.top,
            right,
            bottom: area.bottom,
        });

        left += right;
    }

    layouts
}

#[must_use]
fn rows(area: &Rect, len: usize) -> Vec<Rect> {
    #[allow(clippy::cast_possible_wrap, clippy::cast_possible_truncation)]
    let bottom = area.bottom / len as i32;
    let mut top = 0;

    let mut layouts: Vec<Rect> = vec![];
    for _ in 0..len {
        layouts.push(Rect {
            left: area.left,
            top: area.top + top,
            right: area.right,
            bottom,
        });

        top += bottom;
    }

    layouts
}

fn calculate_resize_adjustments(resize_dimensions: &[Option<Rect>]) -> Vec<Option<Rect>> {
    let mut resize_adjustments = resize_dimensions.to_vec();

    // This needs to be aware of layout flips
    for (i, opt) in resize_dimensions.iter().enumerate() {
        if let Some(resize_ref) = opt {
            if i > 0 {
                if resize_ref.left != 0 {
                    #[allow(clippy::if_not_else)]
                    let range = if i == 1 {
                        0..1
                    } else if i & 1 != 0 {
                        i - 1..i
                    } else {
                        i - 2..i
                    };

                    for n in range {
                        let should_adjust = n % 2 == 0;
                        if should_adjust {
                            if let Some(Some(adjacent_resize)) = resize_adjustments.get_mut(n) {
                                adjacent_resize.right += resize_ref.left;
                            } else {
                                resize_adjustments[n] = Option::from(Rect {
                                    left: 0,
                                    top: 0,
                                    right: resize_ref.left,
                                    bottom: 0,
                                });
                            }
                        }
                    }

                    if let Some(rr) = resize_adjustments[i].as_mut() {
                        rr.left = 0;
                    }
                }

                if resize_ref.top != 0 {
                    let range = if i == 1 {
                        0..1
                    } else if i & 1 == 0 {
                        i - 1..i
                    } else {
                        i - 2..i
                    };

                    for n in range {
                        let should_adjust = n % 2 != 0;
                        if should_adjust {
                            if let Some(Some(adjacent_resize)) = resize_adjustments.get_mut(n) {
                                adjacent_resize.bottom += resize_ref.top;
                            } else {
                                resize_adjustments[n] = Option::from(Rect {
                                    left: 0,
                                    top: 0,
                                    right: 0,
                                    bottom: resize_ref.top,
                                });
                            }
                        }
                    }

                    if let Some(Some(resize)) = resize_adjustments.get_mut(i) {
                        resize.top = 0;
                    }
                }
            }
        }
    }

    let cleaned_resize_adjustments: Vec<_> = resize_adjustments
        .iter()
        .map(|adjustment| match adjustment {
            None => None,
            Some(rect) if rect.eq(&Rect::default()) => None,
            Some(_) => *adjustment,
        })
        .collect();

    cleaned_resize_adjustments
}

#[allow(clippy::only_used_in_recursion)]
fn recursive_fibonacci(
    idx: usize,
    count: usize,
    area: &Rect,
    layout_flip: Option<Axis>,
    resize_adjustments: Vec<Option<Rect>>,
) -> Vec<Rect> {
    let mut a = *area;

    let resized = if let Some(Some(r)) = resize_adjustments.get(idx) {
        a.left += r.left;
        a.top += r.top;
        a.right += r.right;
        a.bottom += r.bottom;
        a
    } else {
        *area
    };

    let half_width = area.right / 2;
    let half_height = area.bottom / 2;
    let half_resized_width = resized.right / 2;
    let half_resized_height = resized.bottom / 2;

    let (main_x, alt_x, alt_y, main_y);

    if let Some(flip) = layout_flip {
        match flip {
            Axis::Horizontal => {
                main_x = resized.left + half_width + (half_width - half_resized_width);
                alt_x = resized.left;

                alt_y = resized.top + half_resized_height;
                main_y = resized.top;
            }
            Axis::Vertical => {
                main_y = resized.top + half_height + (half_height - half_resized_height);
                alt_y = resized.top;

                main_x = resized.left;
                alt_x = resized.left + half_resized_width;
            }
            Axis::HorizontalAndVertical => {
                main_x = resized.left + half_width + (half_width - half_resized_width);
                alt_x = resized.left;
                main_y = resized.top + half_height + (half_height - half_resized_height);
                alt_y = resized.top;
            }
        }
    } else {
        main_x = resized.left;
        alt_x = resized.left + half_resized_width;
        main_y = resized.top;
        alt_y = resized.top + half_resized_height;
    }

    #[allow(clippy::if_not_else)]
    if count == 0 {
        vec![]
    } else if count == 1 {
        vec![Rect {
            left: resized.left,
            top: resized.top,
            right: resized.right,
            bottom: resized.bottom,
        }]
    } else if idx % 2 != 0 {
        let mut res = vec![Rect {
            left: resized.left,
            top: main_y,
            right: resized.right,
            bottom: half_resized_height,
        }];
        res.append(&mut recursive_fibonacci(
            idx + 1,
            count - 1,
            &Rect {
                left: area.left,
                top: alt_y,
                right: area.right,
                bottom: area.bottom - half_resized_height,
            },
            layout_flip,
            resize_adjustments,
        ));
        res
    } else {
        let mut res = vec![Rect {
            left: main_x,
            top: resized.top,
            right: half_resized_width,
            bottom: resized.bottom,
        }];
        res.append(&mut recursive_fibonacci(
            idx + 1,
            count - 1,
            &Rect {
                left: alt_x,
                top: area.top,
                right: area.right - half_resized_width,
                bottom: area.bottom,
            },
            layout_flip,
            resize_adjustments,
        ));
        res
    }
}

fn calculate_ultrawide_adjustment(resize_dimensions: &[Option<Rect>]) -> Vec<Rect> {
    let len = resize_dimensions.len();
    let mut result = vec![Rect::default(); len];
    match len {
        // One container can't be resized
        0 | 1 => (),
        2 => {
            let (primary, secondary) = result.split_at_mut(1);
            let primary = &mut primary[0];
            let secondary = &mut secondary[0];
            // With two containers on screen container 0 is on the right
            if let Some(resize_primary) = resize_dimensions[0] {
                resize_left(primary, resize_primary.left);
                resize_right(secondary, resize_primary.left);
            }

            if let Some(resize_secondary) = resize_dimensions[1] {
                resize_left(primary, resize_secondary.right);
                resize_right(secondary, resize_secondary.right);
            }
        }
        _ => {
            let (primary, rest) = result.split_at_mut(1);
            let (secondary, tertiary) = rest.split_at_mut(1);
            let primary = &mut primary[0];
            let secondary = &mut secondary[0];
            // With three or more containers container 0 is in the center
            if let Some(resize_primary) = resize_dimensions[0] {
                resize_left(primary, resize_primary.left);
                resize_right(primary, resize_primary.right);

                resize_right(secondary, resize_primary.left);

                for vertical_element in &mut *tertiary {
                    resize_left(vertical_element, resize_primary.right);
                }
            }

            // Container 1 is on the left
            if let Some(resize_secondary) = resize_dimensions[1] {
                resize_left(primary, resize_secondary.right);
                resize_right(secondary, resize_secondary.right);
            }

            // Handle stack on the right
            for (i, rect) in resize_dimensions[2..].iter().enumerate() {
                if let Some(rect) = rect {
                    resize_right(primary, rect.left);
                    tertiary
                        .iter_mut()
                        .for_each(|vertical_element| resize_left(vertical_element, rect.left));

                    // Containers in stack except first can be resized up displacing container
                    // above them
                    if i != 0 {
                        resize_bottom(&mut tertiary[i - 1], rect.top);
                        resize_top(&mut tertiary[i], rect.top);
                    }

                    // Containers in stack except last can be resized down displacing container
                    // below them
                    if i != tertiary.len() - 1 {
                        resize_bottom(&mut tertiary[i], rect.bottom);
                        resize_top(&mut tertiary[i + 1], rect.bottom);
                    }
                }
            }
        }
    };

    result
}

fn resize_left(rect: &mut Rect, resize: i32) {
    rect.left += resize / 2;
    rect.right += -resize / 2;
}

fn resize_right(rect: &mut Rect, resize: i32) {
    rect.right += resize / 2;
}

fn resize_top(rect: &mut Rect, resize: i32) {
    rect.top += resize / 2;
    rect.bottom += -resize / 2;
}

fn resize_bottom(rect: &mut Rect, resize: i32) {
    rect.bottom += resize / 2;
}

fn ultrawide(
    area: &Rect,
    len: usize,
    layout_flip: Option<Axis>,
    resize_dimensions: &[Option<Rect>],
) -> Vec<Rect> {
    let mut layouts: Vec<Rect> = vec![];

    let primary_right = match len {
        1 => area.right,
        _ => area.right / 2,
    };

    let secondary_right = match len {
        1 => 0,
        2 => area.right - primary_right,
        _ => (area.right - primary_right) / 2,
    };

    let (primary_left, secondary_left, stack_left) = match len {
        1 => (area.left, 0, 0),
        2 => {
            let mut primary = area.left + secondary_right;
            let mut secondary = area.left;

            match layout_flip {
                Some(Axis::Horizontal | Axis::HorizontalAndVertical) if len > 1 => {
                    primary = area.left;
                    secondary = area.left + primary_right;
                }
                _ => {}
            }

            (primary, secondary, 0)
        }
        _ => {
            let primary = area.left + secondary_right;
            let mut secondary = area.left;
            let mut stack = area.left + primary_right + secondary_right;

            match layout_flip {
                Some(Axis::Horizontal | Axis::HorizontalAndVertical) if len > 1 => {
                    secondary = area.left + primary_right + secondary_right;
                    stack = area.left;
                }
                _ => {}
            }

            (primary, secondary, stack)
        }
    };

    if len >= 1 {
        layouts.push(Rect {
            left: primary_left,
            top: area.top,
            right: primary_right,
            bottom: area.bottom,
        });

        if len >= 2 {
            layouts.push(Rect {
                left: secondary_left,
                top: area.top,
                right: secondary_right,
                bottom: area.bottom,
            });

            if len > 2 {
                layouts.append(&mut rows(
                    &Rect {
                        left: stack_left,
                        top: area.top,
                        right: secondary_right,
                        bottom: area.bottom,
                    },
                    len - 2,
                ));
            }
        }
    }

    let adjustment = calculate_ultrawide_adjustment(resize_dimensions);
    layouts
        .iter_mut()
        .zip(adjustment.iter())
        .for_each(|(layout, adjustment)| {
            layout.top += adjustment.top;
            layout.bottom += adjustment.bottom;
            layout.left += adjustment.left;
            layout.right += adjustment.right;
        });

    layouts
}
