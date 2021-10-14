use clap::ArgEnum;
use serde::Deserialize;
use serde::Serialize;
use strum::Display;
use strum::EnumString;

use crate::Flip;
use crate::Layout;

#[derive(Clone, Copy, Debug, Serialize, Deserialize, Display, EnumString, ArgEnum)]
#[strum(serialize_all = "snake_case")]
pub enum OperationDirection {
    Left,
    Right,
    Up,
    Down,
}

impl OperationDirection {
    #[must_use]
    pub const fn opposite(self) -> Self {
        match self {
            Self::Left => Self::Right,
            Self::Right => Self::Left,
            Self::Up => Self::Down,
            Self::Down => Self::Up,
        }
    }

    fn flip_direction(direction: Self, layout_flip: Option<Flip>) -> Self {
        layout_flip.map_or(direction, |flip| match direction {
            Self::Left => match flip {
                Flip::Horizontal | Flip::HorizontalAndVertical => Self::Right,
                Flip::Vertical => direction,
            },
            Self::Right => match flip {
                Flip::Horizontal | Flip::HorizontalAndVertical => Self::Left,
                Flip::Vertical => direction,
            },
            Self::Up => match flip {
                Flip::Vertical | Flip::HorizontalAndVertical => Self::Down,
                Flip::Horizontal => direction,
            },
            Self::Down => match flip {
                Flip::Vertical | Flip::HorizontalAndVertical => Self::Up,
                Flip::Horizontal => direction,
            },
        })
    }

    #[must_use]
    pub fn is_valid(
        self,
        layout: Layout,
        layout_flip: Option<Flip>,
        idx: usize,
        len: usize,
    ) -> bool {
        match Self::flip_direction(self, layout_flip) {
            Self::Up => match layout {
                Layout::BSP => len > 2 && idx != 0 && idx != 1,
                Layout::Columns => false,
                Layout::Rows | Layout::HorizontalStack => idx != 0,
                Layout::VerticalStack => idx != 0 && idx != 1,
                Layout::UltrawideVerticalStack => idx > 2,
            },
            Self::Down => match layout {
                Layout::BSP => len > 2 && idx != len - 1 && idx % 2 != 0,
                Layout::Columns => false,
                Layout::Rows => idx != len - 1,
                Layout::VerticalStack => idx != 0 && idx != len - 1,
                Layout::HorizontalStack => idx == 0,
                Layout::UltrawideVerticalStack => idx > 1 && idx != len - 1,
            },
            Self::Left => match layout {
                Layout::BSP => len > 1 && idx != 0,
                Layout::Columns | Layout::VerticalStack => idx != 0,
                Layout::Rows => false,
                Layout::HorizontalStack => idx != 0 && idx != 1,
                Layout::UltrawideVerticalStack => len > 1 && idx != 1,
            },
            Self::Right => match layout {
                Layout::BSP => len > 1 && idx % 2 == 0 && idx != len - 1,
                Layout::Columns => idx != len - 1,
                Layout::Rows => false,
                Layout::VerticalStack => idx == 0,
                Layout::HorizontalStack => idx != 0 && idx != len - 1,
                Layout::UltrawideVerticalStack => match len {
                    0 | 1 => false,
                    2 => idx != 0,
                    _ => idx < 2,
                },
            },
        }
    }

    #[must_use]
    pub fn new_idx(self, layout: Layout, layout_flip: Option<Flip>, idx: usize) -> usize {
        match Self::flip_direction(self, layout_flip) {
            Self::Up => match layout {
                Layout::BSP => {
                    if idx % 2 == 0 {
                        idx - 1
                    } else {
                        idx - 2
                    }
                }
                Layout::Columns => unreachable!(),
                Layout::Rows | Layout::VerticalStack | Layout::UltrawideVerticalStack => idx - 1,
                Layout::HorizontalStack => 0,
            },
            Self::Down => match layout {
                Layout::BSP
                | Layout::Rows
                | Layout::VerticalStack
                | Layout::UltrawideVerticalStack => idx + 1,
                Layout::Columns => unreachable!(),
                Layout::HorizontalStack => 1,
            },
            Self::Left => match layout {
                Layout::BSP => {
                    if idx % 2 == 0 {
                        idx - 2
                    } else {
                        idx - 1
                    }
                }
                Layout::Columns | Layout::HorizontalStack => idx - 1,
                Layout::Rows => unreachable!(),
                Layout::VerticalStack => 0,
                Layout::UltrawideVerticalStack => match idx {
                    0 => 1,
                    1 => unreachable!(),
                    _ => 0,
                },
            },
            Self::Right => match layout {
                Layout::BSP | Layout::Columns | Layout::HorizontalStack => idx + 1,
                Layout::Rows => unreachable!(),
                Layout::VerticalStack => 1,
                Layout::UltrawideVerticalStack => match idx {
                    1 => 0,
                    0 => 2,
                    _ => unreachable!(),
                },
            },
        }
    }
}
