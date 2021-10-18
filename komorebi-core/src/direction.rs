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
    fn up_index(&self, idx: usize) -> usize;
    fn down_index(&self, idx: usize) -> usize;
    fn left_index(&self, idx: usize) -> usize;
    fn right_index(&self, idx: usize) -> usize;
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
                    Option::from(self.left_index(idx))
                } else {
                    None
                }
            }
            OperationDirection::Right => {
                if self.is_valid_direction(op_direction, idx, count) {
                    Option::from(self.right_index(idx))
                } else {
                    None
                }
            }
            OperationDirection::Up => {
                if self.is_valid_direction(op_direction, idx, count) {
                    Option::from(self.up_index(idx))
                } else {
                    None
                }
            }
            OperationDirection::Down => {
                if self.is_valid_direction(op_direction, idx, count) {
                    Option::from(self.down_index(idx))
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
                DefaultLayout::BSP => count > 2 && idx != 0 && idx != 1,
                DefaultLayout::Columns => false,
                DefaultLayout::Rows | DefaultLayout::HorizontalStack => idx != 0,
                DefaultLayout::VerticalStack => idx != 0 && idx != 1,
                DefaultLayout::UltrawideVerticalStack => idx > 2,
            },
            OperationDirection::Down => match self {
                DefaultLayout::BSP => count > 2 && idx != count - 1 && idx % 2 != 0,
                DefaultLayout::Columns => false,
                DefaultLayout::Rows => idx != count - 1,
                DefaultLayout::VerticalStack => idx != 0 && idx != count - 1,
                DefaultLayout::HorizontalStack => idx == 0,
                DefaultLayout::UltrawideVerticalStack => idx > 1 && idx != count - 1,
            },
            OperationDirection::Left => match self {
                DefaultLayout::BSP => count > 1 && idx != 0,
                DefaultLayout::Columns | DefaultLayout::VerticalStack => idx != 0,
                DefaultLayout::Rows => false,
                DefaultLayout::HorizontalStack => idx != 0 && idx != 1,
                DefaultLayout::UltrawideVerticalStack => count > 1 && idx != 1,
            },
            OperationDirection::Right => match self {
                DefaultLayout::BSP => count > 1 && idx % 2 == 0 && idx != count - 1,
                DefaultLayout::Columns => idx != count - 1,
                DefaultLayout::Rows => false,
                DefaultLayout::VerticalStack => idx == 0,
                DefaultLayout::HorizontalStack => idx != 0 && idx != count - 1,
                DefaultLayout::UltrawideVerticalStack => match count {
                    0 | 1 => false,
                    2 => idx != 0,
                    _ => idx < 2,
                },
            },
        }
    }

    fn up_index(&self, idx: usize) -> usize {
        match self {
            DefaultLayout::BSP => {
                if idx % 2 == 0 {
                    idx - 1
                } else {
                    idx - 2
                }
            }
            DefaultLayout::Columns => unreachable!(),
            DefaultLayout::Rows
            | DefaultLayout::VerticalStack
            | DefaultLayout::UltrawideVerticalStack => idx - 1,
            DefaultLayout::HorizontalStack => 0,
        }
    }

    fn down_index(&self, idx: usize) -> usize {
        match self {
            DefaultLayout::BSP
            | DefaultLayout::Rows
            | DefaultLayout::VerticalStack
            | DefaultLayout::UltrawideVerticalStack => idx + 1,
            DefaultLayout::Columns => unreachable!(),
            DefaultLayout::HorizontalStack => 1,
        }
    }

    fn left_index(&self, idx: usize) -> usize {
        match self {
            DefaultLayout::BSP => {
                if idx % 2 == 0 {
                    idx - 2
                } else {
                    idx - 1
                }
            }
            DefaultLayout::Columns | DefaultLayout::HorizontalStack => idx - 1,
            DefaultLayout::Rows => unreachable!(),
            DefaultLayout::VerticalStack => 0,
            DefaultLayout::UltrawideVerticalStack => match idx {
                0 => 1,
                1 => unreachable!(),
                _ => 0,
            },
        }
    }

    fn right_index(&self, idx: usize) -> usize {
        match self {
            DefaultLayout::BSP | DefaultLayout::Columns | DefaultLayout::HorizontalStack => idx + 1,
            DefaultLayout::Rows => unreachable!(),
            DefaultLayout::VerticalStack => 1,
            DefaultLayout::UltrawideVerticalStack => match idx {
                1 => 0,
                0 => 2,
                _ => unreachable!(),
            },
        }
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
                    Option::from(self.left_index(idx))
                } else {
                    None
                }
            }
            OperationDirection::Right => {
                if self.is_valid_direction(op_direction, idx, count) {
                    Option::from(self.right_index(idx))
                } else {
                    None
                }
            }
            OperationDirection::Up => {
                if self.is_valid_direction(op_direction, idx, count) {
                    Option::from(self.up_index(idx))
                } else {
                    None
                }
            }
            OperationDirection::Down => {
                if self.is_valid_direction(op_direction, idx, count) {
                    Option::from(self.down_index(idx))
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
                match column {
                    None => false,
                    Some(column) => match column {
                        Column::Secondary(Some(ColumnSplitWithCapacity::Horizontal(_)))
                        | Column::Tertiary(ColumnSplit::Horizontal) => {
                            self.column_for_container_idx(idx - 1) == column_idx
                        }
                        _ => false,
                    },
                }
            }
            OperationDirection::Down => {
                if idx == count - 1 {
                    return false;
                }

                let (column_idx, column) = self.column_with_idx(idx);
                match column {
                    None => false,
                    Some(column) => match column {
                        Column::Secondary(Some(ColumnSplitWithCapacity::Horizontal(_)))
                        | Column::Tertiary(ColumnSplit::Horizontal) => {
                            self.column_for_container_idx(idx + 1) == column_idx
                        }
                        _ => false,
                    },
                }
            }
        }
    }

    fn up_index(&self, idx: usize) -> usize {
        idx - 1
    }

    fn down_index(&self, idx: usize) -> usize {
        idx + 1
    }

    fn left_index(&self, idx: usize) -> usize {
        let column_idx = self.column_for_container_idx(idx);
        if column_idx - 1 == 0 {
            0
        } else {
            self.first_container_idx(column_idx - 1)
        }
    }

    fn right_index(&self, idx: usize) -> usize {
        let column_idx = self.column_for_container_idx(idx);
        self.first_container_idx(column_idx + 1)
    }
}
