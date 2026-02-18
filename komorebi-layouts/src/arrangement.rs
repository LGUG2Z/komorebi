use std::num::NonZeroUsize;

use clap::ValueEnum;
use serde::Deserialize;
use serde::Serialize;
use strum::Display;
use strum::EnumString;

#[cfg(feature = "win32")]
use super::CustomLayout;
use super::DefaultLayout;
use super::Rect;
#[cfg(feature = "win32")]
use super::custom_layout::Column;
#[cfg(feature = "win32")]
use super::custom_layout::ColumnSplit;
#[cfg(feature = "win32")]
use super::custom_layout::ColumnSplitWithCapacity;
use crate::default_layout::DEFAULT_RATIO;
use crate::default_layout::DEFAULT_SECONDARY_RATIO;
use crate::default_layout::LayoutOptions;
use crate::default_layout::MAX_RATIO;
use crate::default_layout::MAX_RATIOS;
use crate::default_layout::MIN_RATIO;

pub trait Arrangement {
    #[allow(clippy::too_many_arguments)]
    fn calculate(
        &self,
        area: &Rect,
        len: NonZeroUsize,
        container_padding: Option<i32>,
        layout_flip: Option<Axis>,
        resize_dimensions: &[Option<Rect>],
        focused_idx: usize,
        layout_options: Option<LayoutOptions>,
        latest_layout: &[Rect],
    ) -> Vec<Rect>;
}

impl Arrangement for DefaultLayout {
    #[allow(clippy::too_many_lines, clippy::cognitive_complexity)]
    fn calculate(
        &self,
        area: &Rect,
        len: NonZeroUsize,
        container_padding: Option<i32>,
        layout_flip: Option<Axis>,
        resize_dimensions: &[Option<Rect>],
        focused_idx: usize,
        layout_options: Option<LayoutOptions>,
        latest_layout: &[Rect],
    ) -> Vec<Rect> {
        // Trace layout_options for debugging
        if let Some(ref opts) = layout_options {
            tracing::debug!(
                "Layout {:?} - layout_options received: column_ratios={:?}, row_ratios={:?}",
                self,
                opts.column_ratios,
                opts.row_ratios
            );
        } else {
            tracing::debug!("Layout {:?} - no layout_options provided", self);
        }

        let len = usize::from(len);
        let mut dimensions = match self {
            Self::Scrolling => {
                let column_count = layout_options
                    .as_ref()
                    .and_then(|o| o.scrolling.map(|s| s.columns))
                    .unwrap_or(3);

                let column_width = area.right / column_count.min(len) as i32;
                let mut layouts = Vec::with_capacity(len);

                let visible_columns = area.right / column_width;
                let keep_centered = layout_options
                    .as_ref()
                    .and_then(|o| {
                        o.scrolling
                            .map(|s| s.center_focused_column.unwrap_or_default())
                    })
                    .unwrap_or(false);

                let first_visible: isize = if focused_idx == 0 {
                    // if focused idx is 0, we are at the beginning of the scrolling strip
                    0
                } else {
                    let previous_first_visible = if latest_layout.is_empty() {
                        0
                    } else {
                        // previous first_visible based on the left position of the first visible window
                        let left_edge = area.left;
                        latest_layout
                            .iter()
                            .position(|rect| rect.left >= left_edge)
                            .unwrap_or(0) as isize
                    };

                    let focused_idx = focused_idx as isize;

                    // if center_focused_column is enabled, and we have an odd number of visible columns,
                    // center the focused window column
                    if keep_centered && visible_columns % 2 == 1 {
                        let center_offset = visible_columns as isize / 2;
                        (focused_idx - center_offset).max(0).min(
                            (len as isize)
                                .saturating_sub(visible_columns as isize)
                                .max(0),
                        )
                    } else {
                        if focused_idx < previous_first_visible {
                            // focused window is off the left edge, we need to scroll left
                            focused_idx
                        } else if focused_idx >= previous_first_visible + visible_columns as isize {
                            // focused window is off the right edge, we need to scroll right
                            // and make sure it's the last visible window
                            (focused_idx + 1 - visible_columns as isize).max(0)
                        } else {
                            // focused window is already visible, we don't need to scroll
                            previous_first_visible
                        }
                        .min(
                            (len as isize)
                                .saturating_sub(visible_columns as isize)
                                .max(0),
                        )
                    }
                };

                for i in 0..len {
                    let position = (i as isize) - first_visible;
                    let left = area.left + (position as i32 * column_width);

                    layouts.push(Rect {
                        left,
                        top: area.top,
                        right: column_width,
                        bottom: area.bottom,
                    });
                }

                // Last visible column absorbs any remainder from integer division
                // so that visible columns tile the full area width without gaps
                let width_remainder = area.right - column_width * visible_columns;
                if width_remainder > 0 {
                    let last_visible_idx =
                        (first_visible as usize + visible_columns as usize - 1).min(len - 1);
                    layouts[last_visible_idx].right += width_remainder;
                }

                let adjustment = calculate_scrolling_adjustment(resize_dimensions);
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
            Self::BSP => {
                let column_split_ratio = layout_options
                    .and_then(|o| o.column_ratios)
                    .and_then(|r| r[0])
                    .unwrap_or(DEFAULT_RATIO)
                    .clamp(MIN_RATIO, MAX_RATIO);
                let row_split_ratio = layout_options
                    .and_then(|o| o.row_ratios)
                    .and_then(|r| r[0])
                    .unwrap_or(DEFAULT_RATIO)
                    .clamp(MIN_RATIO, MAX_RATIO);
                recursive_fibonacci(
                    0,
                    len,
                    area,
                    layout_flip,
                    calculate_resize_adjustments(resize_dimensions),
                    column_split_ratio,
                    row_split_ratio,
                )
            }
            Self::Columns => {
                let ratios = layout_options.and_then(|o| o.column_ratios);
                let mut layouts = columns_with_ratios(area, len, ratios);

                let adjustment = calculate_columns_adjustment(resize_dimensions);
                layouts
                    .iter_mut()
                    .zip(adjustment.iter())
                    .for_each(|(layout, adjustment)| {
                        layout.top += adjustment.top;
                        layout.bottom += adjustment.bottom;
                        layout.left += adjustment.left;
                        layout.right += adjustment.right;
                    });

                if matches!(
                    layout_flip,
                    Some(Axis::Horizontal | Axis::HorizontalAndVertical)
                ) && let 2.. = len
                {
                    columns_reverse(&mut layouts);
                }

                layouts
            }
            Self::Rows => {
                let ratios = layout_options.and_then(|o| o.row_ratios);
                let mut layouts = rows_with_ratios(area, len, ratios);

                let adjustment = calculate_rows_adjustment(resize_dimensions);
                layouts
                    .iter_mut()
                    .zip(adjustment.iter())
                    .for_each(|(layout, adjustment)| {
                        layout.top += adjustment.top;
                        layout.bottom += adjustment.bottom;
                        layout.left += adjustment.left;
                        layout.right += adjustment.right;
                    });

                if matches!(
                    layout_flip,
                    Some(Axis::Vertical | Axis::HorizontalAndVertical)
                ) && let 2.. = len
                {
                    rows_reverse(&mut layouts);
                }

                layouts
            }
            Self::VerticalStack => {
                let mut layouts: Vec<Rect> = vec![];

                #[allow(clippy::cast_possible_truncation)]
                let primary_right = match len {
                    1 => area.right,
                    _ => {
                        let ratio = layout_options
                            .and_then(|o| o.column_ratios)
                            .and_then(|r| r[0])
                            .unwrap_or(DEFAULT_RATIO)
                            .clamp(MIN_RATIO, MAX_RATIO);
                        (area.right as f32 * ratio) as i32
                    }
                };

                let main_left = area.left;
                let stack_left = area.left + primary_right;

                if len >= 1 {
                    layouts.push(Rect {
                        left: main_left,
                        top: area.top,
                        right: primary_right,
                        bottom: area.bottom,
                    });

                    if len > 1 {
                        let row_ratios = layout_options.and_then(|o| o.row_ratios);
                        layouts.append(&mut rows_with_ratios(
                            &Rect {
                                left: stack_left,
                                top: area.top,
                                right: area.right - primary_right,
                                bottom: area.bottom,
                            },
                            len - 1,
                            row_ratios,
                        ));
                    }
                }

                let adjustment = calculate_vertical_stack_adjustment(resize_dimensions);
                layouts
                    .iter_mut()
                    .zip(adjustment.iter())
                    .for_each(|(layout, adjustment)| {
                        layout.top += adjustment.top;
                        layout.bottom += adjustment.bottom;
                        layout.left += adjustment.left;
                        layout.right += adjustment.right;
                    });

                if matches!(
                    layout_flip,
                    Some(Axis::Horizontal | Axis::HorizontalAndVertical)
                ) && let 2.. = len
                {
                    let (primary, rest) = layouts.split_at_mut(1);
                    let primary = &mut primary[0];

                    for rect in rest.iter_mut() {
                        rect.left = primary.left;
                    }
                    primary.left = rest[0].left + rest[0].right;
                }

                if matches!(
                    layout_flip,
                    Some(Axis::Vertical | Axis::HorizontalAndVertical)
                ) && let 3.. = len
                {
                    rows_reverse(&mut layouts[1..]);
                }

                layouts
            }
            Self::RightMainVerticalStack => {
                // Shamelessly borrowed from LeftWM: https://github.com/leftwm/leftwm/commit/f673851745295ae7584a102535566f559d96a941
                let mut layouts: Vec<Rect> = vec![];

                #[allow(clippy::cast_possible_truncation)]
                let primary_width = match len {
                    1 => area.right,
                    _ => {
                        let ratio = layout_options
                            .and_then(|o| o.column_ratios)
                            .and_then(|r| r[0])
                            .unwrap_or(DEFAULT_RATIO)
                            .clamp(MIN_RATIO, MAX_RATIO);
                        (area.right as f32 * ratio) as i32
                    }
                };

                let primary_left = match len {
                    1 => 0,
                    _ => area.right - primary_width,
                };

                if len >= 1 {
                    layouts.push(Rect {
                        left: area.left + primary_left,
                        top: area.top,
                        right: primary_width,
                        bottom: area.bottom,
                    });

                    if len > 1 {
                        let row_ratios = layout_options.and_then(|o| o.row_ratios);
                        layouts.append(&mut rows_with_ratios(
                            &Rect {
                                left: area.left,
                                top: area.top,
                                right: primary_left,
                                bottom: area.bottom,
                            },
                            len - 1,
                            row_ratios,
                        ));
                    }
                }

                let adjustment = calculate_right_vertical_stack_adjustment(resize_dimensions);
                layouts
                    .iter_mut()
                    .zip(adjustment.iter())
                    .for_each(|(layout, adjustment)| {
                        layout.top += adjustment.top;
                        layout.bottom += adjustment.bottom;
                        layout.left += adjustment.left;
                        layout.right += adjustment.right;
                    });

                if matches!(
                    layout_flip,
                    Some(Axis::Horizontal | Axis::HorizontalAndVertical)
                ) && let 2.. = len
                {
                    let (primary, rest) = layouts.split_at_mut(1);
                    let primary = &mut primary[0];

                    primary.left = rest[0].left;
                    for rect in rest.iter_mut() {
                        rect.left = primary.left + primary.right;
                    }
                }

                if matches!(
                    layout_flip,
                    Some(Axis::Vertical | Axis::HorizontalAndVertical)
                ) && let 3.. = len
                {
                    rows_reverse(&mut layouts[1..]);
                }

                layouts
            }
            Self::HorizontalStack => {
                let mut layouts: Vec<Rect> = vec![];

                #[allow(clippy::cast_possible_truncation)]
                let bottom = match len {
                    1 => area.bottom,
                    _ => {
                        let ratio = layout_options
                            .and_then(|o| o.row_ratios)
                            .and_then(|r| r[0])
                            .unwrap_or(DEFAULT_RATIO)
                            .clamp(MIN_RATIO, MAX_RATIO);
                        (area.bottom as f32 * ratio) as i32
                    }
                };

                let main_top = area.top;
                let stack_top = area.top + bottom;

                if len >= 1 {
                    layouts.push(Rect {
                        left: area.left,
                        top: main_top,
                        right: area.right,
                        bottom,
                    });

                    if len > 1 {
                        let col_ratios = layout_options.and_then(|o| o.column_ratios);
                        layouts.append(&mut columns_with_ratios(
                            &Rect {
                                left: area.left,
                                top: stack_top,
                                right: area.right,
                                bottom: area.bottom - bottom,
                            },
                            len - 1,
                            col_ratios,
                        ));
                    }
                }

                let adjustment = calculate_horizontal_stack_adjustment(resize_dimensions);
                layouts
                    .iter_mut()
                    .zip(adjustment.iter())
                    .for_each(|(layout, adjustment)| {
                        layout.top += adjustment.top;
                        layout.bottom += adjustment.bottom;
                        layout.left += adjustment.left;
                        layout.right += adjustment.right;
                    });

                if matches!(
                    layout_flip,
                    Some(Axis::Vertical | Axis::HorizontalAndVertical)
                ) && let 2.. = len
                {
                    let (primary, rest) = layouts.split_at_mut(1);
                    let primary = &mut primary[0];

                    for rect in rest.iter_mut() {
                        rect.top = primary.top;
                    }
                    primary.top = rest[0].top + rest[0].bottom;
                }

                if matches!(
                    layout_flip,
                    Some(Axis::Horizontal | Axis::HorizontalAndVertical)
                ) && let 3.. = len
                {
                    columns_reverse(&mut layouts[1..]);
                }

                layouts
            }
            Self::UltrawideVerticalStack => {
                let mut layouts: Vec<Rect> = vec![];

                // Get ratios: [0] = primary (center), [1] = secondary (left), remainder = tertiary (right)
                let ratios = layout_options.and_then(|o| o.column_ratios);
                let primary_ratio = ratios
                    .and_then(|r| r[0])
                    .unwrap_or(DEFAULT_RATIO)
                    .clamp(MIN_RATIO, MAX_RATIO);
                let secondary_ratio = ratios
                    .and_then(|r| r[1])
                    .unwrap_or(DEFAULT_SECONDARY_RATIO)
                    .clamp(MIN_RATIO, MAX_RATIO);

                #[allow(clippy::cast_possible_truncation)]
                let primary_right = match len {
                    1 => area.right,
                    _ => (area.right as f32 * primary_ratio) as i32,
                };

                #[allow(clippy::cast_possible_truncation)]
                let secondary_right = match len {
                    1 => 0,
                    2 => area.right - primary_right,
                    _ => (area.right as f32 * secondary_ratio) as i32,
                };

                let (primary_left, secondary_left, stack_left) = match len {
                    1 => (area.left, 0, 0),
                    2 => {
                        let primary = area.left + secondary_right;
                        let secondary = area.left;

                        (primary, secondary, 0)
                    }
                    _ => {
                        let primary = area.left + secondary_right;
                        let secondary = area.left;
                        let stack = area.left + primary_right + secondary_right;

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
                            // Tertiary column gets remaining space after primary and secondary
                            let tertiary_right = area.right - primary_right - secondary_right;
                            let row_ratios = layout_options.and_then(|o| o.row_ratios);
                            layouts.append(&mut rows_with_ratios(
                                &Rect {
                                    left: stack_left,
                                    top: area.top,
                                    right: tertiary_right,
                                    bottom: area.bottom,
                                },
                                len - 2,
                                row_ratios,
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

                if matches!(
                    layout_flip,
                    Some(Axis::Horizontal | Axis::HorizontalAndVertical)
                ) {
                    match len {
                        2 => {
                            let (primary, secondary) = layouts.split_at_mut(1);
                            let primary = &mut primary[0];
                            let secondary = &mut secondary[0];

                            primary.left = secondary.left;
                            secondary.left = primary.left + primary.right;
                        }
                        3.. => {
                            let (primary, rest) = layouts.split_at_mut(1);
                            let (secondary, tertiary) = rest.split_at_mut(1);
                            let primary = &mut primary[0];
                            let secondary = &mut secondary[0];

                            for rect in tertiary.iter_mut() {
                                rect.left = secondary.left;
                            }
                            primary.left = tertiary[0].left + tertiary[0].right;
                            secondary.left = primary.left + primary.right;
                        }
                        _ => {}
                    }
                }

                if matches!(
                    layout_flip,
                    Some(Axis::Vertical | Axis::HorizontalAndVertical)
                ) && let 4.. = len
                {
                    rows_reverse(&mut layouts[2..]);
                }

                layouts
            }
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

                let row_constraint = layout_options.as_ref().and_then(|o| o.grid.map(|g| g.rows));
                let column_ratios = layout_options.and_then(|o| o.column_ratios);

                // Count defined column ratios (already validated at deserialization to sum < 1.0)
                let defined_ratios = column_ratios
                    .as_ref()
                    .map(|r| r.iter().filter(|x| x.is_some()).count())
                    .unwrap_or(0);

                let num_cols = if let Some(rows) = row_constraint {
                    ((len as f32) / (rows as f32)).ceil() as i32
                } else {
                    (len as f32).sqrt().ceil() as i32
                };

                // Pre-calculate column widths and left positions using same logic as columns_with_ratios
                let mut col_widths: Vec<i32> = Vec::with_capacity(num_cols as usize);
                let mut col_lefts: Vec<i32> = Vec::with_capacity(num_cols as usize);
                let mut current_left = area.left;

                for col in 0..num_cols {
                    let col_idx = col as usize;
                    let width = if let Some(ref ratios) = column_ratios {
                        // Only apply ratio if there's at least one more column after this
                        // The last column always gets the remaining space
                        let should_apply_ratio =
                            col_idx < MAX_RATIOS && col_idx < defined_ratios && col < num_cols - 1;

                        if should_apply_ratio {
                            if let Some(ratio) = ratios[col_idx] {
                                (area.right as f32 * ratio) as i32
                            } else {
                                let used: f32 = (0..col_idx).filter_map(|j| ratios[j]).sum();
                                let remaining_space =
                                    area.right - (area.right as f32 * used) as i32;
                                let remaining_cols = num_cols - col;
                                remaining_space / remaining_cols
                            }
                        } else {
                            // Beyond defined ratios or last column - split remaining space equally
                            // Only count ratios that were actually applied (up to defined_ratios, but not beyond num_cols - 1)
                            let ratios_applied = defined_ratios.min((num_cols - 1) as usize);
                            let used: f32 = (0..ratios_applied).filter_map(|j| ratios[j]).sum();
                            let remaining_space = area.right - (area.right as f32 * used) as i32;
                            let remaining_cols = (num_cols as usize - ratios_applied) as i32;
                            if remaining_cols > 0 {
                                remaining_space / remaining_cols
                            } else {
                                remaining_space
                            }
                        }
                    } else {
                        area.right / num_cols
                    };

                    col_lefts.push(current_left);
                    col_widths.push(width);
                    current_left += width;
                }

                // Last column absorbs any remainder from integer division
                // so that columns tile the full area width without gaps
                let total_width: i32 = col_widths.iter().sum();
                let width_remainder = area.right - total_width;
                if width_remainder > 0
                    && let Some(last) = col_widths.last_mut()
                {
                    *last += width_remainder;
                }

                // Pre-calculate flipped column positions: same widths laid out
                // in reverse order so that the last column sits at area.left
                let flipped_col_lefts = if matches!(
                    layout_flip,
                    Some(Axis::Horizontal | Axis::HorizontalAndVertical)
                ) {
                    let n = num_cols as usize;
                    let mut flipped = vec![0i32; n];
                    let mut fl = area.left;
                    for i in (0..n).rev() {
                        flipped[i] = fl;
                        fl += col_widths[i];
                    }
                    flipped
                } else {
                    vec![]
                };

                let mut iter = layouts.iter_mut().enumerate().peekable();

                for col in 0..num_cols {
                    let iter_peek = iter.peek().map(|x| x.0).unwrap_or_default() as i32;
                    let remaining_windows = len - iter_peek;
                    let remaining_columns = num_cols - col;

                    let num_rows_in_this_col = if let Some(rows) = row_constraint {
                        (remaining_windows / remaining_columns).min(rows as i32)
                    } else {
                        remaining_windows / remaining_columns
                    };

                    // Rows within each column: base height from integer division,
                    // last row absorbs any remainder to cover the full area height
                    let base_height = area.bottom / num_rows_in_this_col;
                    let height_remainder = area.bottom - base_height * num_rows_in_this_col;

                    let col_idx = col as usize;
                    let win_width = col_widths[col_idx];
                    let col_left = col_lefts[col_idx];

                    for row in 0..num_rows_in_this_col {
                        if let Some((_idx, win)) = iter.next() {
                            let is_last_row = row == num_rows_in_this_col - 1;
                            let win_height = if is_last_row {
                                base_height + height_remainder
                            } else {
                                base_height
                            };

                            let mut left = col_left;
                            let mut top = area.top + base_height * row;

                            match layout_flip {
                                Some(Axis::Horizontal) => {
                                    left = flipped_col_lefts[col_idx];
                                }
                                Some(Axis::Vertical) => {
                                    top = if is_last_row {
                                        area.top
                                    } else {
                                        area.top + area.bottom - base_height * (row + 1)
                                    };
                                }
                                Some(Axis::HorizontalAndVertical) => {
                                    left = flipped_col_lefts[col_idx];
                                    top = if is_last_row {
                                        area.top
                                    } else {
                                        area.top + area.bottom - base_height * (row + 1)
                                    };
                                }
                                None => {}
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

#[cfg(feature = "win32")]
impl Arrangement for CustomLayout {
    fn calculate(
        &self,
        area: &Rect,
        len: NonZeroUsize,
        container_padding: Option<i32>,
        _layout_flip: Option<Axis>,
        _resize_dimensions: &[Option<Rect>],
        _focused_idx: usize,
        _layout_options: Option<LayoutOptions>,
        _latest_layout: &[Rect],
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
                        Column::Primary(Some(_)) => {
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

#[derive(Clone, Copy, Debug, Serialize, Deserialize, Display, EnumString, ValueEnum, PartialEq)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
/// Axis on which to perform an operation
pub enum Axis {
    /// Horizontal axis
    Horizontal,
    /// Vertical axis
    Vertical,
    /// Both horizontal and vertical axes
    HorizontalAndVertical,
}

#[cfg(feature = "win32")]
#[must_use]
fn columns(area: &Rect, len: usize) -> Vec<Rect> {
    columns_with_ratios(area, len, None)
}

#[must_use]
fn columns_with_ratios(
    area: &Rect,
    len: usize,
    ratios: Option<[Option<f32>; MAX_RATIOS]>,
) -> Vec<Rect> {
    tracing::debug!(
        "columns_with_ratios called: len={}, ratios={:?}",
        len,
        ratios
    );
    let mut layouts: Vec<Rect> = vec![];
    let mut left = 0;

    // Count how many ratios are defined (already validated at deserialization to sum < 1.0)
    let defined_ratios = ratios
        .as_ref()
        .map(|r| r.iter().filter(|x| x.is_some()).count())
        .unwrap_or(0);

    for i in 0..len {
        #[allow(clippy::cast_possible_truncation)]
        let right = if let Some(ref r) = ratios {
            // Only apply ratio[i] if there's at least one more column after this (i < len - 1)
            // The last column always gets the remaining space
            let should_apply_ratio = i < MAX_RATIOS && i < defined_ratios && i < len - 1;

            if should_apply_ratio {
                if let Some(ratio) = r[i] {
                    (area.right as f32 * ratio) as i32
                } else {
                    let used: f32 = (0..i).filter_map(|j| r[j]).sum();
                    let remaining_space = area.right - (area.right as f32 * used) as i32;
                    let remaining_columns = len - i;
                    remaining_space / remaining_columns as i32
                }
            } else {
                // Last column or beyond defined ratios - split remaining space equally
                let ratios_applied = i.min(defined_ratios).min(len.saturating_sub(1));
                let used: f32 = (0..ratios_applied).filter_map(|j| r[j]).sum();
                let remaining_space = area.right - (area.right as f32 * used) as i32;
                let remaining_columns = len - ratios_applied;
                if remaining_columns > 0 {
                    remaining_space / remaining_columns as i32
                } else {
                    remaining_space
                }
            }
        } else {
            // Equal width columns (original behavior)
            #[allow(clippy::cast_possible_wrap)]
            {
                area.right / len as i32
            }
        };

        layouts.push(Rect {
            left: area.left + left,
            top: area.top,
            right,
            bottom: area.bottom,
        });

        left += right;
    }

    // Last column absorbs any remainder from integer division
    // so that columns tile the full area width without gaps
    let total_width: i32 = layouts.iter().map(|r| r.right).sum();
    let remainder = area.right - total_width;
    if remainder > 0
        && let Some(last) = layouts.last_mut()
    {
        last.right += remainder;
    }

    layouts
}

#[cfg(feature = "win32")]
#[must_use]
fn rows(area: &Rect, len: usize) -> Vec<Rect> {
    rows_with_ratios(area, len, None)
}

#[must_use]
fn rows_with_ratios(
    area: &Rect,
    len: usize,
    ratios: Option<[Option<f32>; MAX_RATIOS]>,
) -> Vec<Rect> {
    tracing::debug!("rows_with_ratios called: len={}, ratios={:?}", len, ratios);
    let mut layouts: Vec<Rect> = vec![];
    let mut top = 0;

    // Count how many ratios are defined (already validated at deserialization to sum < 1.0)
    let defined_ratios = ratios
        .as_ref()
        .map(|r| r.iter().filter(|x| x.is_some()).count())
        .unwrap_or(0);

    for i in 0..len {
        #[allow(clippy::cast_possible_truncation)]
        let bottom = if let Some(ref r) = ratios {
            // Only apply ratio[i] if there's at least one more row after this (i < len - 1)
            // The last row always gets the remaining space
            let should_apply_ratio = i < MAX_RATIOS && i < defined_ratios && i < len - 1;

            if should_apply_ratio {
                if let Some(ratio) = r[i] {
                    (area.bottom as f32 * ratio) as i32
                } else {
                    let used: f32 = (0..i).filter_map(|j| r[j]).sum();
                    let remaining_space = area.bottom - (area.bottom as f32 * used) as i32;
                    let remaining_rows = len - i;
                    remaining_space / remaining_rows as i32
                }
            } else {
                // Last row or beyond defined ratios - split remaining space equally
                let ratios_applied = i.min(defined_ratios).min(len.saturating_sub(1));
                let used: f32 = (0..ratios_applied).filter_map(|j| r[j]).sum();
                let remaining_space = area.bottom - (area.bottom as f32 * used) as i32;
                let remaining_rows = len - ratios_applied;
                if remaining_rows > 0 {
                    remaining_space / remaining_rows as i32
                } else {
                    remaining_space
                }
            }
        } else {
            // Equal height rows (original behavior)
            #[allow(clippy::cast_possible_wrap)]
            {
                area.bottom / len as i32
            }
        };

        layouts.push(Rect {
            left: area.left,
            top: area.top + top,
            right: area.right,
            bottom,
        });

        top += bottom;
    }

    // Last row absorbs any remainder from integer division
    // so that rows tile the full area height without gaps
    let total_height: i32 = layouts.iter().map(|r| r.bottom).sum();
    let remainder = area.bottom - total_height;
    if remainder > 0
        && let Some(last) = layouts.last_mut()
    {
        last.bottom += remainder;
    }

    layouts
}

fn columns_reverse(columns: &mut [Rect]) {
    let len = columns.len();
    columns[len - 1].left = columns[0].left;
    for i in (0..len - 1).rev() {
        columns[i].left = columns[i + 1].left + columns[i + 1].right;
    }
}

fn rows_reverse(rows: &mut [Rect]) {
    let len = rows.len();
    rows[len - 1].top = rows[0].top;
    for i in (0..len - 1).rev() {
        rows[i].top = rows[i + 1].top + rows[i + 1].bottom;
    }
}

fn calculate_resize_adjustments(resize_dimensions: &[Option<Rect>]) -> Vec<Option<Rect>> {
    let mut resize_adjustments = resize_dimensions.to_vec();

    // This needs to be aware of layout flips
    for (i, opt) in resize_dimensions.iter().enumerate() {
        if let Some(resize_ref) = opt
            && i > 0
        {
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
    column_split_ratio: f32,
    row_split_ratio: f32,
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

    #[allow(clippy::cast_possible_truncation)]
    let primary_resized_width = (resized.right as f32 * column_split_ratio) as i32;
    #[allow(clippy::cast_possible_truncation)]
    let primary_resized_height = (resized.bottom as f32 * row_split_ratio) as i32;

    let (main_x, alt_x, alt_y, main_y);

    if let Some(flip) = layout_flip {
        match flip {
            Axis::Horizontal => {
                main_x = resized.left + (area.right - primary_resized_width);
                alt_x = resized.left;

                alt_y = resized.top + primary_resized_height;
                main_y = resized.top;
            }
            Axis::Vertical => {
                main_y = resized.top + (area.bottom - primary_resized_height);
                alt_y = resized.top;

                main_x = resized.left;
                alt_x = resized.left + primary_resized_width;
            }
            Axis::HorizontalAndVertical => {
                main_x = resized.left + (area.right - primary_resized_width);
                alt_x = resized.left;
                main_y = resized.top + (area.bottom - primary_resized_height);
                alt_y = resized.top;
            }
        }
    } else {
        main_x = resized.left;
        alt_x = resized.left + primary_resized_width;
        main_y = resized.top;
        alt_y = resized.top + primary_resized_height;
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
    } else if !idx.is_multiple_of(2) {
        let mut res = vec![Rect {
            left: resized.left,
            top: main_y,
            right: resized.right,
            bottom: primary_resized_height,
        }];
        res.append(&mut recursive_fibonacci(
            idx + 1,
            count - 1,
            &Rect {
                left: area.left,
                top: alt_y,
                right: area.right,
                bottom: area.bottom - primary_resized_height,
            },
            layout_flip,
            resize_adjustments,
            column_split_ratio,
            row_split_ratio,
        ));
        res
    } else {
        let mut res = vec![Rect {
            left: main_x,
            top: resized.top,
            right: primary_resized_width,
            bottom: resized.bottom,
        }];
        res.append(&mut recursive_fibonacci(
            idx + 1,
            count - 1,
            &Rect {
                left: alt_x,
                top: area.top,
                right: area.right - primary_resized_width,
                bottom: area.bottom,
            },
            layout_flip,
            resize_adjustments,
            column_split_ratio,
            row_split_ratio,
        ));
        res
    }
}

fn calculate_columns_adjustment(resize_dimensions: &[Option<Rect>]) -> Vec<Rect> {
    let len = resize_dimensions.len();
    let mut result = vec![Rect::default(); len];
    match len {
        0 | 1 => (),
        _ => {
            for (i, rect) in resize_dimensions.iter().enumerate() {
                if let Some(rect) = rect {
                    if i != 0 {
                        resize_right(&mut result[i - 1], rect.left);
                        resize_left(&mut result[i], rect.left);
                    }

                    if i != len - 1 {
                        resize_right(&mut result[i], rect.right);
                        resize_left(&mut result[i + 1], rect.right);
                    }
                }
            }
        }
    };

    result
}

fn calculate_rows_adjustment(resize_dimensions: &[Option<Rect>]) -> Vec<Rect> {
    let len = resize_dimensions.len();
    let mut result = vec![Rect::default(); len];
    match len {
        0 | 1 => (),
        _ => {
            for (i, rect) in resize_dimensions.iter().enumerate() {
                if let Some(rect) = rect {
                    if i != 0 {
                        resize_bottom(&mut result[i - 1], rect.top);
                        resize_top(&mut result[i], rect.top);
                    }

                    if i != len - 1 {
                        resize_bottom(&mut result[i], rect.bottom);
                        resize_top(&mut result[i + 1], rect.bottom);
                    }
                }
            }
        }
    };

    result
}

fn calculate_vertical_stack_adjustment(resize_dimensions: &[Option<Rect>]) -> Vec<Rect> {
    let len = resize_dimensions.len();
    let mut result = vec![Rect::default(); len];
    match len {
        // One container can't be resized
        0 | 1 => (),
        _ => {
            let (master, stack) = result.split_at_mut(1);
            let primary = &mut master[0];

            if let Some(resize) = resize_dimensions[0] {
                resize_right(primary, resize.right);
                for s in &mut *stack {
                    resize_left(s, resize.right);
                }
            }

            // Handle stack on the right
            for (i, rect) in resize_dimensions[1..].iter().enumerate() {
                if let Some(rect) = rect {
                    resize_right(primary, rect.left);
                    stack
                        .iter_mut()
                        .for_each(|vertical_element| resize_left(vertical_element, rect.left));

                    // Containers in stack except first can be resized up displacing container
                    // above them
                    if i != 0 {
                        resize_bottom(&mut stack[i - 1], rect.top);
                        resize_top(&mut stack[i], rect.top);
                    }

                    // Containers in stack except last can be resized down displacing container
                    // below them
                    if i != stack.len() - 1 {
                        resize_bottom(&mut stack[i], rect.bottom);
                        resize_top(&mut stack[i + 1], rect.bottom);
                    }
                }
            }
        }
    };

    result
}

fn calculate_right_vertical_stack_adjustment(resize_dimensions: &[Option<Rect>]) -> Vec<Rect> {
    let len = resize_dimensions.len();
    let mut result = vec![Rect::default(); len];
    match len {
        // One container can't be resized
        0 | 1 => (),
        _ => {
            let (master, stack) = result.split_at_mut(1);
            let primary = &mut master[0];

            if let Some(resize) = resize_dimensions[0] {
                resize_left(primary, resize.left);
                for s in &mut *stack {
                    resize_right(s, resize.left);
                }
            }

            // Handle stack on the left
            for (i, rect) in resize_dimensions[1..].iter().enumerate() {
                if let Some(rect) = rect {
                    resize_left(primary, rect.right);
                    stack
                        .iter_mut()
                        .for_each(|vertical_element| resize_right(vertical_element, rect.right));

                    // Containers in stack except first can be resized up displacing container
                    // above them
                    if i != 0 {
                        resize_bottom(&mut stack[i - 1], rect.top);
                        resize_top(&mut stack[i], rect.top);
                    }

                    // Containers in stack except last can be resized down displacing container
                    // below them
                    if i != stack.len() - 1 {
                        resize_bottom(&mut stack[i], rect.bottom);
                        resize_top(&mut stack[i + 1], rect.bottom);
                    }
                }
            }
        }
    };

    result
}

fn calculate_horizontal_stack_adjustment(resize_dimensions: &[Option<Rect>]) -> Vec<Rect> {
    let len = resize_dimensions.len();
    let mut result = vec![Rect::default(); len];
    match len {
        0 | 1 => (),
        _ => {
            let (primary, rest) = result.split_at_mut(1);
            let primary = &mut primary[0];
            if let Some(resize_primary) = resize_dimensions[0] {
                resize_bottom(primary, resize_primary.bottom);

                for horizontal_element in &mut *rest {
                    resize_top(horizontal_element, resize_primary.bottom);
                }
            }

            for (i, rect) in resize_dimensions[1..].iter().enumerate() {
                if let Some(rect) = rect {
                    resize_bottom(primary, rect.top);
                    rest.iter_mut()
                        .for_each(|vertical_element| resize_top(vertical_element, rect.top));

                    if i != 0 {
                        resize_right(&mut rest[i - 1], rect.left);
                        resize_left(&mut rest[i], rect.left);
                    }

                    if i != rest.len() - 1 {
                        resize_right(&mut rest[i], rect.right);
                        resize_left(&mut rest[i + 1], rect.right);
                    }
                }
            }
        }
    };

    result
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

fn calculate_scrolling_adjustment(resize_dimensions: &[Option<Rect>]) -> Vec<Rect> {
    let len = resize_dimensions.len();
    let mut result = vec![Rect::default(); len];

    if len <= 1 {
        return result;
    }

    for (i, rect) in resize_dimensions.iter().enumerate() {
        if let Some(rect) = rect {
            let is_leftmost = i == 0;
            let is_rightmost = i == len - 1;

            resize_left(&mut result[i], rect.left);
            resize_right(&mut result[i], rect.right);
            resize_top(&mut result[i], rect.top);
            resize_bottom(&mut result[i], rect.bottom);

            if !is_leftmost && rect.left != 0 {
                resize_right(&mut result[i - 1], rect.left);
            }

            if !is_rightmost && rect.right != 0 {
                resize_left(&mut result[i + 1], rect.right);
            }
        }
    }

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

#[cfg(test)]
#[path = "arrangement_tests.rs"]
mod tests;
