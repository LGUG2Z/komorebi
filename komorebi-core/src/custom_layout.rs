use std::collections::HashMap;
use std::num::NonZeroUsize;

use clap::ArgEnum;
use serde::Deserialize;
use serde::Serialize;
use strum::Display;
use strum::EnumString;

use crate::layout::columns;
use crate::layout::rows;
use crate::layout::Dimensions;
use crate::Flip;
use crate::Rect;

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct CustomLayout {
    pub columns: Vec<Column>,
    pub primary_index: usize,
}

// For example:
//
//     CustomLayout {
//         columns: vec![
//             Column::Secondary(Option::from(ColumnSplitWithCapacity::Horizontal(3))),
//             Column::Secondary(None),
//             Column::Primary,
//             Column::Tertiary(ColumnSplit::Horizontal),
//         ],
//         primary_index: 2,
//     };

impl CustomLayout {
    #[must_use]
    pub fn is_valid(&self) -> bool {
        // A valid layout must have at least one column
        if self.columns.is_empty() {
            return false;
        };

        // The final column must not have a fixed capacity
        match self.columns.last() {
            Some(Column::Tertiary(_)) => {}
            _ => return false,
        }

        let mut primaries = 0;
        let mut tertiaries = 0;

        for column in &self.columns {
            match column {
                Column::Primary => primaries += 1,
                Column::Tertiary(_) => tertiaries += 1,
                _ => {}
            }
        }

        // There must only be one primary and one tertiary column
        matches!(primaries, 1) && matches!(tertiaries, 1)
    }

    #[must_use]
    pub fn area(&self, work_area: &Rect, idx: usize, offset: Option<usize>) -> Rect {
        let divisor =
            offset.map_or_else(|| self.columns.len(), |offset| self.columns.len() - offset);

        #[allow(clippy::cast_possible_wrap, clippy::cast_possible_truncation)]
        let equal_width = work_area.right / divisor as i32;
        let mut left = work_area.left;
        let right = equal_width;

        for _ in 0..idx {
            left += right;
        }

        Rect {
            left,
            top: work_area.top,
            right,
            bottom: work_area.bottom,
        }
    }
}

impl Dimensions for CustomLayout {
    fn calculate(
        &self,
        area: &Rect,
        len: NonZeroUsize,
        container_padding: Option<i32>,
        _layout_flip: Option<Flip>,
        _resize_dimensions: &[Option<Rect>],
    ) -> Vec<Rect> {
        let mut dimensions = vec![];

        match len.get() {
            0 => {}
            // One window takes up the whole area
            1 => dimensions.push(*area),
            // If there number of windows is less than or equal to the number of
            // columns in the custom layout, just use a regular columnar layout
            // until there are enough windows to start really applying the layout
            i if i <= self.columns.len() => {
                let mut layouts = columns(area, i);
                dimensions.append(&mut layouts);
            }
            container_count => {
                let mut count_map: HashMap<usize, usize> = HashMap::new();

                for (idx, column) in self.columns.iter().enumerate() {
                    match column {
                        Column::Primary | Column::Secondary(None) => {
                            count_map.insert(idx, 1);
                        }
                        Column::Secondary(Some(split)) => {
                            count_map.insert(
                                idx,
                                match split {
                                    ColumnSplitWithCapacity::Vertical(n)
                                    | ColumnSplitWithCapacity::Horizontal(n) => *n,
                                },
                            );
                        }
                        Column::Tertiary(_) => {}
                    }
                }

                // If there are not enough windows to trigger the final tertiary
                // column in the custom layout, use an offset to reduce the number of
                // columns to calculate each column's area by, so that we don't have
                // an empty ghost tertiary column and the screen space can be maximised
                // until there are enough windows to create it
                let mut tertiary_trigger_threshold = 0;

                // always -1 because we don't insert the tertiary column in the count_map
                for i in 0..self.columns.len() - 1 {
                    tertiary_trigger_threshold += count_map.get(&i).unwrap();
                }

                let enable_tertiary_column = len.get() > tertiary_trigger_threshold;

                let offset = if enable_tertiary_column {
                    None
                } else {
                    Option::from(1)
                };

                for (idx, column) in self.columns.iter().enumerate() {
                    // If we are offsetting a tertiary column for which the threshold
                    // has not yet been met, this loop should not run for that final
                    // tertiary column
                    if idx < self.columns.len() - offset.unwrap_or(0) {
                        let column_area = self.area(area, idx, offset);

                        match column {
                            Column::Primary | Column::Secondary(None) => {
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
        }

        dimensions
            .iter_mut()
            .for_each(|l| l.add_padding(container_padding));

        dimensions
    }
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize, Display, EnumString, ArgEnum)]
#[strum(serialize_all = "snake_case")]
pub enum Column {
    Primary,
    Secondary(Option<ColumnSplitWithCapacity>),
    Tertiary(ColumnSplit),
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize, Display, EnumString, ArgEnum)]
#[strum(serialize_all = "snake_case")]
pub enum ColumnSplitWithCapacity {
    Vertical(usize),
    Horizontal(usize),
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize, Display, EnumString, ArgEnum)]
#[strum(serialize_all = "snake_case")]
pub enum ColumnSplit {
    Horizontal,
    Vertical,
}

impl Default for ColumnSplit {
    fn default() -> Self {
        Self::Horizontal
    }
}
