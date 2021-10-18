use std::collections::HashMap;
use std::ops::Deref;

use serde::Deserialize;
use serde::Serialize;

use crate::Rect;

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct CustomLayout(Vec<Column>);

impl Deref for CustomLayout {
    type Target = Vec<Column>;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl CustomLayout {
    #[must_use]
    pub fn column_with_idx(&self, idx: usize) -> (usize, Option<&Column>) {
        let column_idx = self.column_for_container_idx(idx);
        let column = self.get(column_idx);
        (column_idx, column)
    }

    #[must_use]
    pub fn primary_idx(&self) -> Option<usize> {
        for (i, column) in self.iter().enumerate() {
            if let Column::Primary = column {
                return Option::from(i);
            }
        }

        None
    }

    #[must_use]
    pub fn is_valid(&self) -> bool {
        // A valid layout must have at least one column
        if self.is_empty() {
            return false;
        };

        // Vertical column splits aren't supported at the moment
        for column in self.iter() {
            match column {
                Column::Tertiary(ColumnSplit::Vertical)
                | Column::Secondary(Some(ColumnSplitWithCapacity::Vertical(_))) => return false,
                _ => {}
            }
        }

        // The final column must not have a fixed capacity
        match self.last() {
            Some(Column::Tertiary(_)) => {}
            _ => return false,
        }

        let mut primaries = 0;
        let mut tertiaries = 0;

        for column in self.iter() {
            match column {
                Column::Primary => primaries += 1,
                Column::Tertiary(_) => tertiaries += 1,
                Column::Secondary(_) => {}
            }
        }

        // There must only be one primary and one tertiary column
        matches!(primaries, 1) && matches!(tertiaries, 1)
    }

    pub(crate) fn column_container_counts(&self) -> HashMap<usize, usize> {
        let mut count_map = HashMap::new();

        for (idx, column) in self.iter().enumerate() {
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

        count_map
    }

    #[must_use]
    pub fn first_container_idx(&self, col_idx: usize) -> usize {
        let count_map = self.column_container_counts();
        let mut container_idx_accumulator = 0;

        for i in 0..col_idx {
            if let Some(n) = count_map.get(&i) {
                container_idx_accumulator += n;
            }
        }

        container_idx_accumulator
    }

    #[must_use]
    pub fn column_for_container_idx(&self, idx: usize) -> usize {
        let count_map = self.column_container_counts();
        let mut container_idx_accumulator = 0;

        // always -1 because we don't insert the tertiary column in the count_map
        for i in 0..self.len() - 1 {
            if let Some(n) = count_map.get(&i) {
                container_idx_accumulator += n;

                // The accumulator becomes greater than the window container index
                // for the first time when we reach a column that contains that
                // window container index
                if container_idx_accumulator > idx {
                    return i;
                }
            }
        }

        // If the accumulator never reaches a point where it is greater than the
        // window container index, then the only remaining possibility is the
        // final tertiary column
        self.len() - 1
    }

    #[must_use]
    pub fn column_area(&self, work_area: &Rect, idx: usize, offset: Option<usize>) -> Rect {
        let divisor = offset.map_or_else(|| self.len(), |offset| self.len() - offset);

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

#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
#[serde(tag = "column", content = "configuration")]
pub enum Column {
    Primary,
    Secondary(Option<ColumnSplitWithCapacity>),
    Tertiary(ColumnSplit),
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
pub enum ColumnSplit {
    Horizontal,
    Vertical,
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
pub enum ColumnSplitWithCapacity {
    Horizontal(usize),
    Vertical(usize),
}
