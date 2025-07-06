use crate::container::ContainerIdx;
use crate::ring::RingIndex;

use super::custom_layout::Column;
use super::custom_layout::ColumnSplit;
use super::custom_layout::ColumnSplitWithCapacity;
use super::custom_layout::CustomLayout;
use super::DefaultLayout;
use super::OperationDirection;

pub trait Direction {
    fn index_in_direction(
        &self,
        op_direction: OperationDirection,
        idx: ContainerIdx,
        count: usize,
    ) -> Option<ContainerIdx>;

    fn is_valid_direction(
        &self,
        op_direction: OperationDirection,
        idx: ContainerIdx,
        count: usize,
    ) -> bool;
    fn up_index(
        &self,
        op_direction: Option<OperationDirection>,
        idx: ContainerIdx,
        count: Option<usize>,
    ) -> ContainerIdx;
    fn down_index(
        &self,
        op_direction: Option<OperationDirection>,
        idx: ContainerIdx,
        count: Option<usize>,
    ) -> ContainerIdx;
    fn left_index(
        &self,
        op_direction: Option<OperationDirection>,
        idx: ContainerIdx,
        count: Option<usize>,
    ) -> ContainerIdx;
    fn right_index(
        &self,
        op_direction: Option<OperationDirection>,
        idx: ContainerIdx,
        count: Option<usize>,
    ) -> ContainerIdx;
}

impl Direction for DefaultLayout {
    fn index_in_direction(
        &self,
        op_direction: OperationDirection,
        idx: ContainerIdx,
        count: usize,
    ) -> Option<ContainerIdx> {
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
        idx: ContainerIdx,
        count: usize,
    ) -> bool {
        if count < 2 {
            return false;
        }

        let last_index = ContainerIdx::from_usize(count - 1);
        match op_direction {
            OperationDirection::Up => match self {
                Self::BSP => idx != ContainerIdx::default() && idx != ContainerIdx::from_usize(1),
                Self::Columns => false,
                Self::Rows | Self::HorizontalStack => idx != ContainerIdx::default(),
                Self::VerticalStack | Self::RightMainVerticalStack => {
                    idx != ContainerIdx::default() && idx != ContainerIdx::from_usize(1)
                }
                Self::UltrawideVerticalStack => idx > ContainerIdx::from_usize(2),
                Self::Grid => !is_grid_edge(op_direction, idx, count),
                Self::Scrolling => false,
            },
            OperationDirection::Down => match self {
                Self::BSP => idx != last_index && !idx.even(),
                Self::Columns => false,
                Self::Rows => idx != last_index,
                Self::VerticalStack | Self::RightMainVerticalStack => {
                    idx != ContainerIdx::default() && idx != last_index
                }
                Self::HorizontalStack => idx == ContainerIdx::default(),
                Self::UltrawideVerticalStack => {
                    idx > ContainerIdx::from_usize(1) && idx != last_index
                }
                Self::Grid => !is_grid_edge(op_direction, idx, count),
                Self::Scrolling => false,
            },
            OperationDirection::Left => match self {
                Self::BSP => idx != ContainerIdx::default(),
                Self::Columns | Self::VerticalStack => idx != ContainerIdx::default(),
                Self::RightMainVerticalStack => idx == ContainerIdx::default(),
                Self::Rows => false,
                Self::HorizontalStack => {
                    idx != ContainerIdx::default() && idx != ContainerIdx::from_usize(1)
                }
                Self::UltrawideVerticalStack => idx != ContainerIdx::from_usize(1),
                Self::Grid => !is_grid_edge(op_direction, idx, count),
                Self::Scrolling => idx != ContainerIdx::default(),
            },
            OperationDirection::Right => match self {
                Self::BSP => idx.even() && idx != last_index,
                Self::Columns => idx != last_index,
                Self::Rows => false,
                Self::VerticalStack => idx == ContainerIdx::default(),
                Self::RightMainVerticalStack => idx != ContainerIdx::default(),
                Self::HorizontalStack => idx != ContainerIdx::default() && idx != last_index,
                Self::UltrawideVerticalStack => match count {
                    2 => idx != ContainerIdx::default(),
                    _ => idx < ContainerIdx::from_usize(2),
                },
                Self::Grid => !is_grid_edge(op_direction, idx, count),
                Self::Scrolling => idx != last_index,
            },
        }
    }

    fn up_index(
        &self,
        op_direction: Option<OperationDirection>,
        idx: ContainerIdx,
        count: Option<usize>,
    ) -> ContainerIdx {
        match self {
            Self::BSP => {
                if idx.even() {
                    idx.previous()
                } else {
                    ContainerIdx::from_usize(idx.into_usize() - 2)
                }
            }
            Self::Columns => unreachable!(),
            Self::Rows
            | Self::VerticalStack
            | Self::UltrawideVerticalStack
            | Self::RightMainVerticalStack => idx.previous(),
            Self::HorizontalStack => ContainerIdx::default(),
            Self::Grid => grid_neighbor(op_direction, idx, count),
            Self::Scrolling => unreachable!(),
        }
    }

    fn down_index(
        &self,
        op_direction: Option<OperationDirection>,
        idx: ContainerIdx,
        count: Option<usize>,
    ) -> ContainerIdx {
        match self {
            Self::BSP
            | Self::Rows
            | Self::VerticalStack
            | Self::UltrawideVerticalStack
            | Self::RightMainVerticalStack => idx.next(),
            Self::Columns => unreachable!(),
            Self::HorizontalStack => ContainerIdx::from_usize(1),
            Self::Grid => grid_neighbor(op_direction, idx, count),
            Self::Scrolling => unreachable!(),
        }
    }

    fn left_index(
        &self,
        op_direction: Option<OperationDirection>,
        idx: ContainerIdx,
        count: Option<usize>,
    ) -> ContainerIdx {
        match self {
            Self::BSP => {
                if idx.even() {
                    ContainerIdx::from_usize(idx.into_usize() - 2)
                } else {
                    idx.previous()
                }
            }
            Self::Columns | Self::HorizontalStack => idx.previous(),
            Self::Rows => unreachable!(),
            Self::VerticalStack => ContainerIdx::default(),
            Self::RightMainVerticalStack => ContainerIdx::from_usize(1),
            Self::UltrawideVerticalStack => match idx.into_usize() {
                0 => ContainerIdx::from_usize(1),
                1 => unreachable!(),
                _ => ContainerIdx::default(),
            },
            Self::Grid => grid_neighbor(op_direction, idx, count),
            Self::Scrolling => idx.previous(),
        }
    }

    fn right_index(
        &self,
        op_direction: Option<OperationDirection>,
        idx: ContainerIdx,
        count: Option<usize>,
    ) -> ContainerIdx {
        match self {
            Self::BSP | Self::Columns | Self::HorizontalStack => idx.next(),
            Self::Rows => unreachable!(),
            Self::VerticalStack => ContainerIdx::from_usize(1),
            Self::RightMainVerticalStack => ContainerIdx::default(),
            Self::UltrawideVerticalStack => match idx.into_usize() {
                1 => ContainerIdx::default(),
                0 => ContainerIdx::from_usize(2),
                _ => unreachable!(),
            },
            Self::Grid => grid_neighbor(op_direction, idx, count),
            Self::Scrolling => idx.next(),
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
fn get_grid_item(idx: ContainerIdx, count: usize) -> GridItem {
    let num_cols = (count as f32).sqrt().ceil() as usize;
    let mut current_idx = ContainerIdx::default();

    for col in 0..num_cols {
        let remaining_windows = count - current_idx.into_usize();
        let remaining_columns = num_cols - col;
        let num_rows_in_this_col = remaining_windows / remaining_columns;

        for row in 0..num_rows_in_this_col {
            if current_idx == idx {
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

            current_idx = current_idx.next();
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

fn is_grid_edge(op_direction: OperationDirection, idx: ContainerIdx, count: usize) -> bool {
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
    idx: ContainerIdx,
    count: Option<usize>,
) -> ContainerIdx {
    let Some(op_direction) = op_direction else {
        return ContainerIdx::default();
    };

    let Some(count) = count else {
        return ContainerIdx::default();
    };

    let item = get_grid_item(idx, count);

    let idx = idx.into_usize();
    let idx = match op_direction {
        OperationDirection::Left => {
            let item_from_prev_col = get_grid_item(ContainerIdx::from_usize(idx - item.row), count);

            if (item.touching_edges.up && item.num_rows != item_from_prev_col.num_rows)
                || (item.num_rows != item_from_prev_col.num_rows && !item.touching_edges.down)
            {
                idx - (item.num_rows - 1)
            } else {
                idx - item.num_rows
            }
        }
        OperationDirection::Right => idx + item.num_rows,
        OperationDirection::Up => idx - 1,
        OperationDirection::Down => idx + 1,
    };
    ContainerIdx::from_usize(idx)
}

impl Direction for CustomLayout {
    fn index_in_direction(
        &self,
        op_direction: OperationDirection,
        idx: ContainerIdx,
        count: usize,
    ) -> Option<ContainerIdx> {
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
        idx: ContainerIdx,
        count: usize,
    ) -> bool {
        if count <= self.len() {
            return DefaultLayout::Columns.is_valid_direction(op_direction, idx, count);
        }

        let last_index = ContainerIdx::from_usize(count - 1);
        match op_direction {
            OperationDirection::Left => {
                idx != ContainerIdx::default()
                    && self.column_for_container_idx(idx) != ContainerIdx::default()
            }
            OperationDirection::Right => {
                idx != last_index
                    && self.column_for_container_idx(idx)
                        != ContainerIdx::from_usize(self.len() - 1)
            }
            OperationDirection::Up => {
                if idx == ContainerIdx::default() {
                    return false;
                }

                let (column_idx, column) = self.column_with_idx(idx);
                column.is_some_and(|column| match column {
                    Column::Secondary(Some(ColumnSplitWithCapacity::Horizontal(_)))
                    | Column::Tertiary(ColumnSplit::Horizontal) => {
                        self.column_for_container_idx(idx.previous()) == column_idx
                    }
                    _ => false,
                })
            }
            OperationDirection::Down => {
                if idx == last_index {
                    return false;
                }

                let (column_idx, column) = self.column_with_idx(idx);
                column.is_some_and(|column| match column {
                    Column::Secondary(Some(ColumnSplitWithCapacity::Horizontal(_)))
                    | Column::Tertiary(ColumnSplit::Horizontal) => {
                        self.column_for_container_idx(idx.next()) == column_idx
                    }
                    _ => false,
                })
            }
        }
    }

    fn up_index(
        &self,
        _op_direction: Option<OperationDirection>,
        idx: ContainerIdx,
        _count: Option<usize>,
    ) -> ContainerIdx {
        idx.previous()
    }

    fn down_index(
        &self,
        _op_direction: Option<OperationDirection>,
        idx: ContainerIdx,
        _count: Option<usize>,
    ) -> ContainerIdx {
        idx.next()
    }

    fn left_index(
        &self,
        _op_direction: Option<OperationDirection>,
        idx: ContainerIdx,
        _count: Option<usize>,
    ) -> ContainerIdx {
        let column_idx = self.column_for_container_idx(idx);
        if column_idx.previous() == ContainerIdx::default() {
            ContainerIdx::default()
        } else {
            self.first_container_idx(column_idx.previous())
        }
    }

    fn right_index(
        &self,
        _op_direction: Option<OperationDirection>,
        idx: ContainerIdx,
        _count: Option<usize>,
    ) -> ContainerIdx {
        let column_idx = self.column_for_container_idx(idx);
        self.first_container_idx(column_idx.next())
    }
}
