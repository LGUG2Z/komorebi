use crate::custom_layout::Column;
use crate::custom_layout::ColumnSplit;
use crate::custom_layout::ColumnSplitWithCapacity;
use crate::custom_layout::CustomLayout;
use crate::DefaultLayout;
use crate::OperationDirection;

pub trait Direction {
    fn index_in_direction(
        &self,
        op_direction: OperationDirection,
        idx: usize,
        count: usize,
    ) -> Option<usize>;

    fn is_valid_direction(
        &self,
        op_direction: OperationDirection,
        idx: usize,
        count: usize,
    ) -> bool;
    fn up_index(
        &self,
        op_direction: Option<OperationDirection>,
        idx: usize,
        count: Option<usize>,
    ) -> usize;
    fn down_index(
        &self,
        op_direction: Option<OperationDirection>,
        idx: usize,
        count: Option<usize>,
    ) -> usize;
    fn left_index(
        &self,
        op_direction: Option<OperationDirection>,
        idx: usize,
        count: Option<usize>,
    ) -> usize;
    fn right_index(
        &self,
        op_direction: Option<OperationDirection>,
        idx: usize,
        count: Option<usize>,
    ) -> usize;
}

impl Direction for DefaultLayout {
    fn index_in_direction(
        &self,
        op_direction: OperationDirection,
        idx: usize,
        count: usize,
    ) -> Option<usize> {
        match op_direction {
            OperationDirection::Left => {
                if self.is_valid_direction(op_direction, idx, count) {
                    Option::from(self.left_index(Some(op_direction), idx, Some(count)))
                } else {
                    None
                }
            }
            OperationDirection::Right => {
                if self.is_valid_direction(op_direction, idx, count) {
                    Option::from(self.right_index(Some(op_direction), idx, Some(count)))
                } else {
                    None
                }
            }
            OperationDirection::Up => {
                if self.is_valid_direction(op_direction, idx, count) {
                    Option::from(self.up_index(Some(op_direction), idx, Some(count)))
                } else {
                    None
                }
            }
            OperationDirection::Down => {
                if self.is_valid_direction(op_direction, idx, count) {
                    Option::from(self.down_index(Some(op_direction), idx, Some(count)))
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
    ) -> bool {
        match op_direction {
            OperationDirection::Up => match self {
                Self::BSP => count > 2 && idx != 0 && idx != 1,
                Self::Columns => false,
                Self::Rows | Self::HorizontalStack => idx != 0,
                Self::VerticalStack => idx != 0 && idx != 1,
                Self::UltrawideVerticalStack => idx > 2,
                Self::Grid => !is_grid_edge(op_direction, idx, count),
            },
            OperationDirection::Down => match self {
                Self::BSP => count > 2 && idx != count - 1 && idx % 2 != 0,
                Self::Columns => false,
                Self::Rows => idx != count - 1,
                Self::VerticalStack => idx != 0 && idx != count - 1,
                Self::HorizontalStack => idx == 0,
                Self::UltrawideVerticalStack => idx > 1 && idx != count - 1,
                Self::Grid => !is_grid_edge(op_direction, idx, count),
            },
            OperationDirection::Left => match self {
                Self::BSP => count > 1 && idx != 0,
                Self::Columns | Self::VerticalStack => idx != 0,
                Self::Rows => false,
                Self::HorizontalStack => idx != 0 && idx != 1,
                Self::UltrawideVerticalStack => count > 1 && idx != 1,
                Self::Grid => !is_grid_edge(op_direction, idx, count),
            },
            OperationDirection::Right => match self {
                Self::BSP => count > 1 && idx % 2 == 0 && idx != count - 1,
                Self::Columns => idx != count - 1,
                Self::Rows => false,
                Self::VerticalStack => idx == 0,
                Self::HorizontalStack => idx != 0 && idx != count - 1,
                Self::UltrawideVerticalStack => match count {
                    0 | 1 => false,
                    2 => idx != 0,
                    _ => idx < 2,
                },
                Self::Grid => !is_grid_edge(op_direction, idx, count),
            },
        }
    }

    fn up_index(
        &self,
        op_direction: Option<OperationDirection>,
        idx: usize,
        count: Option<usize>,
    ) -> usize {
        match self {
            Self::BSP => {
                if idx % 2 == 0 {
                    idx - 1
                } else {
                    idx - 2
                }
            }
            Self::Columns => unreachable!(),
            Self::Rows | Self::VerticalStack | Self::UltrawideVerticalStack => idx - 1,
            Self::HorizontalStack => 0,
            Self::Grid => grid_neighbor(op_direction, idx, count),
        }
    }

    fn down_index(
        &self,
        op_direction: Option<OperationDirection>,
        idx: usize,
        count: Option<usize>,
    ) -> usize {
        match self {
            Self::BSP | Self::Rows | Self::VerticalStack | Self::UltrawideVerticalStack => idx + 1,
            Self::Columns => unreachable!(),
            Self::HorizontalStack => 1,
            Self::Grid => grid_neighbor(op_direction, idx, count),
        }
    }

    fn left_index(
        &self,
        op_direction: Option<OperationDirection>,
        idx: usize,
        count: Option<usize>,
    ) -> usize {
        match self {
            Self::BSP => {
                if idx % 2 == 0 {
                    idx - 2
                } else {
                    idx - 1
                }
            }
            Self::Columns | Self::HorizontalStack => idx - 1,
            Self::Rows => unreachable!(),
            Self::VerticalStack => 0,
            Self::UltrawideVerticalStack => match idx {
                0 => 1,
                1 => unreachable!(),
                _ => 0,
            },
            Self::Grid => grid_neighbor(op_direction, idx, count),
        }
    }

    fn right_index(
        &self,
        op_direction: Option<OperationDirection>,
        idx: usize,
        count: Option<usize>,
    ) -> usize {
        match self {
            Self::BSP | Self::Columns | Self::HorizontalStack => idx + 1,
            Self::Rows => unreachable!(),
            Self::VerticalStack => 1,
            Self::UltrawideVerticalStack => match idx {
                1 => 0,
                0 => 2,
                _ => unreachable!(),
            },
            Self::Grid => grid_neighbor(op_direction, idx, count),
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
fn get_grid_item(idx: usize, count: usize) -> GridItem {
    let num_cols = (count as f32).sqrt().ceil() as usize;
    let mut iter = 0;

    for col in 0..num_cols {
        let remaining_windows = count - iter;
        let remaining_columns = num_cols - col;
        let num_rows_in_this_col = remaining_windows / remaining_columns;

        for row in 0..num_rows_in_this_col {
            if iter == idx {
                return GridItem {
                    state: GridItemState::Valid,
                    row: row + 1,
                    num_rows: num_rows_in_this_col,
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

fn is_grid_edge(op_direction: OperationDirection, idx: usize, count: usize) -> bool {
    let item = get_grid_item(idx, count);

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
) -> usize {
    let Some(op_direction) = op_direction else {
        return 0;
    };

    let Some(count) = count else {
        return 0;
    };

    let item = get_grid_item(idx, count);

    match op_direction {
        OperationDirection::Left => {
            let item_from_prev_col = get_grid_item(idx - item.row, count);

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
    ) -> Option<usize> {
        if count <= self.len() {
            return DefaultLayout::Columns.index_in_direction(op_direction, idx, count);
        }

        match op_direction {
            OperationDirection::Left => {
                if self.is_valid_direction(op_direction, idx, count) {
                    Option::from(self.left_index(None, idx, None))
                } else {
                    None
                }
            }
            OperationDirection::Right => {
                if self.is_valid_direction(op_direction, idx, count) {
                    Option::from(self.right_index(None, idx, None))
                } else {
                    None
                }
            }
            OperationDirection::Up => {
                if self.is_valid_direction(op_direction, idx, count) {
                    Option::from(self.up_index(None, idx, None))
                } else {
                    None
                }
            }
            OperationDirection::Down => {
                if self.is_valid_direction(op_direction, idx, count) {
                    Option::from(self.down_index(None, idx, None))
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
    ) -> bool {
        if count <= self.len() {
            return DefaultLayout::Columns.is_valid_direction(op_direction, idx, count);
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
                column.map_or(false, |column| match column {
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
                column.map_or(false, |column| match column {
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
    ) -> usize {
        idx - 1
    }

    fn down_index(
        &self,
        _op_direction: Option<OperationDirection>,
        idx: usize,
        _count: Option<usize>,
    ) -> usize {
        idx + 1
    }

    fn left_index(
        &self,
        _op_direction: Option<OperationDirection>,
        idx: usize,
        _count: Option<usize>,
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
    ) -> usize {
        let column_idx = self.column_for_container_idx(idx);
        self.first_container_idx(column_idx + 1)
    }
}
