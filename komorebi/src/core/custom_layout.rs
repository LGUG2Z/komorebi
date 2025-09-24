use std::collections::HashMap;
use std::fs::File;
use std::io::BufReader;
use std::ops::Deref;
use std::ops::DerefMut;
use std::path::Path;

use color_eyre::Result;
use color_eyre::eyre::bail;
use serde::Deserialize;
use serde::Serialize;

use super::Rect;

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
pub struct CustomLayout(Vec<Column>);

impl Deref for CustomLayout {
    type Target = Vec<Column>;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl DerefMut for CustomLayout {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.0
    }
}

impl CustomLayout {
    pub fn from_path<P: AsRef<Path>>(path: P) -> Result<Self> {
        let path = path.as_ref();
        let layout: Self = match path.extension() {
            Some(extension) if extension == "yaml" || extension == "yml" => {
                serde_json::from_reader(BufReader::new(File::open(path)?))?
            }
            Some(extension) if extension == "json" => {
                serde_json::from_reader(BufReader::new(File::open(path)?))?
            }
            _ => bail!("custom layouts must be json or yaml files"),
        };

        if !layout.is_valid() {
            bail!("the layout file provided was invalid");
        }

        Ok(layout)
    }

    #[must_use]
    pub fn column_with_idx(&self, idx: usize) -> (usize, Option<&Column>) {
        let column_idx = self.column_for_container_idx(idx);
        let column = self.get(column_idx);
        (column_idx, column)
    }

    #[must_use]
    pub fn primary_idx(&self) -> Option<usize> {
        for (i, column) in self.iter().enumerate() {
            if let Column::Primary(_) = column {
                return Option::from(i);
            }
        }

        None
    }

    #[must_use]
    pub fn primary_width_percentage(&self) -> Option<f32> {
        for column in self.iter() {
            if let Column::Primary(Option::Some(ColumnWidth::WidthPercentage(percentage))) = column
            {
                return Option::from(*percentage);
            }
        }

        None
    }

    pub fn set_primary_width_percentage(&mut self, percentage: f32) {
        for column in self.iter_mut() {
            if let Column::Primary(Option::Some(ColumnWidth::WidthPercentage(current))) = column {
                *current = percentage;
            }
        }
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
                Column::Primary(_) => primaries += 1,
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
                Column::Primary(_) | Column::Secondary(None) => {
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

    #[must_use]
    pub fn column_area_with_last(
        len: usize,
        work_area: &Rect,
        primary_right: i32,
        last_column: Option<Rect>,
        offset: Option<usize>,
    ) -> Rect {
        let divisor = offset.map_or_else(|| len - 1, |offset| len - offset - 1);

        #[allow(clippy::cast_possible_wrap, clippy::cast_possible_truncation)]
        let equal_width = (work_area.right - primary_right) / divisor as i32;
        let left = last_column.map_or(work_area.left, |last| last.left + last.right);
        let right = equal_width;

        Rect {
            left,
            top: work_area.top,
            right,
            bottom: work_area.bottom,
        }
    }

    #[must_use]
    pub fn main_column_area(
        work_area: &Rect,
        primary_right: i32,
        last_column: Option<Rect>,
    ) -> Rect {
        let left = last_column.map_or(work_area.left, |last| last.left + last.right);

        Rect {
            left,
            top: work_area.top,
            right: primary_right,
            bottom: work_area.bottom,
        }
    }
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[serde(tag = "column", content = "configuration")]
pub enum Column {
    Primary(Option<ColumnWidth>),
    Secondary(Option<ColumnSplitWithCapacity>),
    Tertiary(ColumnSplit),
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
pub enum ColumnWidth {
    WidthPercentage(f32),
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
pub enum ColumnSplit {
    Horizontal,
    Vertical,
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
pub enum ColumnSplitWithCapacity {
    Horizontal(usize),
    Vertical(usize),
}
