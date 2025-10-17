use super::DefaultLayout;
use super::OperationDirection;
use super::custom_layout::Column;
use super::custom_layout::ColumnSplit;
use super::custom_layout::ColumnSplitWithCapacity;
use super::custom_layout::CustomLayout;
use crate::default_layout::LayoutOptions;

pub trait Direction {
    fn index_in_direction(
        &self,
        op_direction: OperationDirection,
        idx: usize,
        count: usize,
        layout_options: Option<LayoutOptions>,
    ) -> Option<usize>;

    fn is_valid_direction(
        &self,
        op_direction: OperationDirection,
        idx: usize,
        count: usize,
        layout_options: Option<LayoutOptions>,
    ) -> bool;
    fn up_index(
        &self,
        op_direction: Option<OperationDirection>,
        idx: usize,
        count: Option<usize>,
        layout_options: Option<LayoutOptions>,
    ) -> usize;
    fn down_index(
        &self,
        op_direction: Option<OperationDirection>,
        idx: usize,
        count: Option<usize>,
        layout_options: Option<LayoutOptions>,
    ) -> usize;
    fn left_index(
        &self,
        op_direction: Option<OperationDirection>,
        idx: usize,
        count: Option<usize>,
        layout_options: Option<LayoutOptions>,
    ) -> usize;
    fn right_index(
        &self,
        op_direction: Option<OperationDirection>,
        idx: usize,
        count: Option<usize>,
        layout_options: Option<LayoutOptions>,
    ) -> usize;
}

impl Direction for DefaultLayout {
    fn index_in_direction(
        &self,
        op_direction: OperationDirection,
        idx: usize,
        count: usize,
        layout_options: Option<LayoutOptions>,
    ) -> Option<usize> {
        match op_direction {
            OperationDirection::Left => {
                if self.is_valid_direction(op_direction, idx, count, layout_options) {
                    Option::from(self.left_index(
                        Some(op_direction),
                        idx,
                        Some(count),
                        layout_options,
                    ))
                } else {
                    None
                }
            }
            OperationDirection::Right => {
                if self.is_valid_direction(op_direction, idx, count, layout_options) {
                    Option::from(self.right_index(
                        Some(op_direction),
                        idx,
                        Some(count),
                        layout_options,
                    ))
                } else {
                    None
                }
            }
            OperationDirection::Up => {
                if self.is_valid_direction(op_direction, idx, count, layout_options) {
                    Option::from(self.up_index(
                        Some(op_direction),
                        idx,
                        Some(count),
                        layout_options,
                    ))
                } else {
                    None
                }
            }
            OperationDirection::Down => {
                if self.is_valid_direction(op_direction, idx, count, layout_options) {
                    Option::from(self.down_index(
                        Some(op_direction),
                        idx,
                        Some(count),
                        layout_options,
                    ))
                } else {
                    None
                }
            }
        }
    }

    fn is_valid_direction(
        &self,
        op_direction: OperationDirection,
        idx: usize,
        count: usize,
        layout_options: Option<LayoutOptions>,
    ) -> bool {
        if count < 2 {
            return false;
        }

        match op_direction {
            OperationDirection::Up => match self {
                Self::BSP => idx != 0 && idx != 1,
                Self::Columns => false,
                Self::Rows | Self::HorizontalStack => idx != 0,
                Self::VerticalStack | Self::RightMainVerticalStack => idx != 0 && idx != 1,
                Self::UltrawideVerticalStack => idx > 2,
                Self::Grid => !is_grid_edge(op_direction, idx, count, layout_options),
                Self::Scrolling => false,
            },
            OperationDirection::Down => match self {
                Self::BSP => idx != count - 1 && !idx.is_multiple_of(2),
                Self::Columns => false,
                Self::Rows => idx != count - 1,
                Self::VerticalStack | Self::RightMainVerticalStack => idx != 0 && idx != count - 1,
                Self::HorizontalStack => idx == 0,
                Self::UltrawideVerticalStack => idx > 1 && idx != count - 1,
                Self::Grid => !is_grid_edge(op_direction, idx, count, layout_options),
                Self::Scrolling => false,
            },
            OperationDirection::Left => match self {
                Self::BSP => idx != 0,
                Self::Columns | Self::VerticalStack => idx != 0,
                Self::RightMainVerticalStack => idx == 0,
                Self::Rows => false,
                Self::HorizontalStack => idx != 0 && idx != 1,
                Self::UltrawideVerticalStack => idx != 1,
                Self::Grid => !is_grid_edge(op_direction, idx, count, layout_options),
                Self::Scrolling => idx != 0,
            },
            OperationDirection::Right => match self {
                Self::BSP => idx.is_multiple_of(2) && idx != count - 1,
                Self::Columns => idx != count - 1,
                Self::Rows => false,
                Self::VerticalStack => idx == 0,
                Self::RightMainVerticalStack => idx != 0,
                Self::HorizontalStack => idx != 0 && idx != count - 1,
                Self::UltrawideVerticalStack => match count {
                    2 => idx != 0,
                    _ => idx < 2,
                },
                Self::Grid => !is_grid_edge(op_direction, idx, count, layout_options),
                Self::Scrolling => idx != count - 1,
            },
        }
    }

    fn up_index(
        &self,
        op_direction: Option<OperationDirection>,
        idx: usize,
        count: Option<usize>,
        layout_options: Option<LayoutOptions>,
    ) -> usize {
        match self {
            Self::BSP => {
                if idx.is_multiple_of(2) {
                    idx - 1
                } else {
                    idx - 2
                }
            }
            Self::Columns => unreachable!(),
            Self::Rows
            | Self::VerticalStack
            | Self::UltrawideVerticalStack
            | Self::RightMainVerticalStack => idx - 1,
            Self::HorizontalStack => 0,
            Self::Grid => grid_neighbor(op_direction, idx, count, layout_options),
            Self::Scrolling => unreachable!(),
        }
    }

    fn down_index(
        &self,
        op_direction: Option<OperationDirection>,
        idx: usize,
        count: Option<usize>,
        layout_options: Option<LayoutOptions>,
    ) -> usize {
        match self {
            Self::BSP
            | Self::Rows
            | Self::VerticalStack
            | Self::UltrawideVerticalStack
            | Self::RightMainVerticalStack => idx + 1,
            Self::Columns => unreachable!(),
            Self::HorizontalStack => 1,
            Self::Grid => grid_neighbor(op_direction, idx, count, layout_options),
            Self::Scrolling => unreachable!(),
        }
    }

    fn left_index(
        &self,
        op_direction: Option<OperationDirection>,
        idx: usize,
        count: Option<usize>,
        layout_options: Option<LayoutOptions>,
    ) -> usize {
        match self {
            Self::BSP => {
                if idx.is_multiple_of(2) {
                    idx - 2
                } else {
                    idx - 1
                }
            }
            Self::Columns | Self::HorizontalStack => idx - 1,
            Self::Rows => unreachable!(),
            Self::VerticalStack => 0,
            Self::RightMainVerticalStack => 1,
            Self::UltrawideVerticalStack => match idx {
                0 => 1,
                1 => unreachable!(),
                _ => 0,
            },
            Self::Grid => grid_neighbor(op_direction, idx, count, layout_options),
            Self::Scrolling => idx - 1,
        }
    }

    fn right_index(
        &self,
        op_direction: Option<OperationDirection>,
        idx: usize,
        count: Option<usize>,
        layout_options: Option<LayoutOptions>,
    ) -> usize {
        match self {
            Self::BSP | Self::Columns | Self::HorizontalStack => idx + 1,
            Self::Rows => unreachable!(),
            Self::VerticalStack => 1,
            Self::RightMainVerticalStack => 0,
            Self::UltrawideVerticalStack => match idx {
                1 => 0,
                0 => 2,
                _ => unreachable!(),
            },
            Self::Grid => grid_neighbor(op_direction, idx, count, layout_options),
            Self::Scrolling => idx + 1,
        }
    }
}

struct GridItem {
    state: GridItemState,
    row: usize,
    num_rows: usize,
    touching_edges: GridTouchingEdges,
}

enum GridItemState {
    Valid,
    Invalid,
}

#[allow(clippy::struct_excessive_bools)]
struct GridTouchingEdges {
    left: bool,
    right: bool,
    up: bool,
    down: bool,
}

#[allow(
    clippy::cast_possible_truncation,
    clippy::cast_precision_loss,
    clippy::cast_sign_loss
)]
fn get_grid_item(idx: usize, count: usize, layout_options: Option<LayoutOptions>) -> GridItem {
    let row_constraint = layout_options.and_then(|o| o.grid.map(|g| g.rows));
    let num_cols = if let Some(rows) = row_constraint {
        ((count as f32) / (rows as f32)).ceil() as i32
    } else {
        (count as f32).sqrt().ceil() as i32
    };

    let mut iter = 0;

    for col in 0..num_cols {
        let remaining_windows = (count - iter) as i32;
        let remaining_columns = num_cols - col;

        let num_rows_in_this_col = if let Some(rows) = row_constraint {
            (remaining_windows / remaining_columns).min(rows as i32)
        } else {
            remaining_windows / remaining_columns
        };

        for row in 0..num_rows_in_this_col {
            if iter == idx {
                return GridItem {
                    state: GridItemState::Valid,
                    row: (row + 1) as usize,
                    num_rows: num_rows_in_this_col as usize,
                    touching_edges: GridTouchingEdges {
                        left: col == 0,
                        right: col == num_cols - 1,
                        up: row == 0,
                        down: row == num_rows_in_this_col - 1,
                    },
                };
            }

            iter += 1;
        }
    }

    GridItem {
        state: GridItemState::Invalid,
        row: 0,
        num_rows: 0,
        touching_edges: GridTouchingEdges {
            left: true,
            right: true,
            up: true,
            down: true,
        },
    }
}

fn is_grid_edge(
    op_direction: OperationDirection,
    idx: usize,
    count: usize,
    layout_options: Option<LayoutOptions>,
) -> bool {
    let item = get_grid_item(idx, count, layout_options);

    match item.state {
        GridItemState::Invalid => false,
        GridItemState::Valid => match op_direction {
            OperationDirection::Left => item.touching_edges.left,
            OperationDirection::Right => item.touching_edges.right,
            OperationDirection::Up => item.touching_edges.up,
            OperationDirection::Down => item.touching_edges.down,
        },
    }
}

fn grid_neighbor(
    op_direction: Option<OperationDirection>,
    idx: usize,
    count: Option<usize>,
    layout_options: Option<LayoutOptions>,
) -> usize {
    let Some(op_direction) = op_direction else {
        return 0;
    };

    let Some(count) = count else {
        return 0;
    };

    let item = get_grid_item(idx, count, layout_options);

    match op_direction {
        OperationDirection::Left => {
            let item_from_prev_col = get_grid_item(idx - item.row, count, layout_options);

            if item.touching_edges.up && item.num_rows != item_from_prev_col.num_rows {
                return idx - (item.num_rows - 1);
            }

            if item.num_rows != item_from_prev_col.num_rows && !item.touching_edges.down {
                return idx - (item.num_rows - 1);
            }

            idx - item.num_rows
        }
        OperationDirection::Right => idx + item.num_rows,
        OperationDirection::Up => idx - 1,
        OperationDirection::Down => idx + 1,
    }
}

impl Direction for CustomLayout {
    fn index_in_direction(
        &self,
        op_direction: OperationDirection,
        idx: usize,
        count: usize,
        layout_options: Option<LayoutOptions>,
    ) -> Option<usize> {
        if count <= self.len() {
            return DefaultLayout::Columns.index_in_direction(
                op_direction,
                idx,
                count,
                layout_options,
            );
        }

        match op_direction {
            OperationDirection::Left => {
                if self.is_valid_direction(op_direction, idx, count, layout_options) {
                    Option::from(self.left_index(None, idx, None, layout_options))
                } else {
                    None
                }
            }
            OperationDirection::Right => {
                if self.is_valid_direction(op_direction, idx, count, layout_options) {
                    Option::from(self.right_index(None, idx, None, layout_options))
                } else {
                    None
                }
            }
            OperationDirection::Up => {
                if self.is_valid_direction(op_direction, idx, count, layout_options) {
                    Option::from(self.up_index(None, idx, None, layout_options))
                } else {
                    None
                }
            }
            OperationDirection::Down => {
                if self.is_valid_direction(op_direction, idx, count, layout_options) {
                    Option::from(self.down_index(None, idx, None, layout_options))
                } else {
                    None
                }
            }
        }
    }

    fn is_valid_direction(
        &self,
        op_direction: OperationDirection,
        idx: usize,
        count: usize,
        layout_options: Option<LayoutOptions>,
    ) -> bool {
        if count <= self.len() {
            return DefaultLayout::Columns.is_valid_direction(
                op_direction,
                idx,
                count,
                layout_options,
            );
        }

        match op_direction {
            OperationDirection::Left => idx != 0 && self.column_for_container_idx(idx) != 0,
            OperationDirection::Right => {
                idx != count - 1 && self.column_for_container_idx(idx) != self.len() - 1
            }
            OperationDirection::Up => {
                if idx == 0 {
                    return false;
                }

                let (column_idx, column) = self.column_with_idx(idx);
                column.is_some_and(|column| match column {
                    Column::Secondary(Some(ColumnSplitWithCapacity::Horizontal(_)))
                    | Column::Tertiary(ColumnSplit::Horizontal) => {
                        self.column_for_container_idx(idx - 1) == column_idx
                    }
                    _ => false,
                })
            }
            OperationDirection::Down => {
                if idx == count - 1 {
                    return false;
                }

                let (column_idx, column) = self.column_with_idx(idx);
                column.is_some_and(|column| match column {
                    Column::Secondary(Some(ColumnSplitWithCapacity::Horizontal(_)))
                    | Column::Tertiary(ColumnSplit::Horizontal) => {
                        self.column_for_container_idx(idx + 1) == column_idx
                    }
                    _ => false,
                })
            }
        }
    }

    fn up_index(
        &self,
        _op_direction: Option<OperationDirection>,
        idx: usize,
        _count: Option<usize>,
        _layout_options: Option<LayoutOptions>,
    ) -> usize {
        idx - 1
    }

    fn down_index(
        &self,
        _op_direction: Option<OperationDirection>,
        idx: usize,
        _count: Option<usize>,
        _layout_options: Option<LayoutOptions>,
    ) -> usize {
        idx + 1
    }

    fn left_index(
        &self,
        _op_direction: Option<OperationDirection>,
        idx: usize,
        _count: Option<usize>,
        _layout_options: Option<LayoutOptions>,
    ) -> usize {
        let column_idx = self.column_for_container_idx(idx);
        if column_idx - 1 == 0 {
            0
        } else {
            self.first_container_idx(column_idx - 1)
        }
    }

    fn right_index(
        &self,
        _op_direction: Option<OperationDirection>,
        idx: usize,
        _count: Option<usize>,
        _layout_options: Option<LayoutOptions>,
    ) -> usize {
        let column_idx = self.column_for_container_idx(idx);
        self.first_container_idx(column_idx + 1)
    }
}
