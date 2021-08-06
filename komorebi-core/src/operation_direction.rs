use clap::Clap;
use serde::Deserialize;
use serde::Serialize;
use strum::Display;
use strum::EnumString;

use crate::Layout;
use crate::LayoutFlip;

#[derive(Clone, Copy, Debug, Serialize, Deserialize, Display, EnumString)]
#[strum(serialize_all = "snake_case")]
#[derive(Clap)]
pub enum OperationDirection {
    Left,
    Right,
    Up,
    Down,
}

impl OperationDirection {
    pub fn can_resize(&self, layout: Layout, idx: usize, len: usize) -> bool {
        match layout {
            Layout::BSP => match self {
                Self::Left => len != 0 && idx != 0,
                Self::Up => len > 2 && idx != 0 && idx != 1,
                Self::Right => len > 1 && idx % 2 == 0 && idx != len - 1,
                Self::Down => len > 2 && idx != len - 1 && idx % 2 != 0,
            },
            _ => false,
        }
    }

    fn flip_direction(
        direction: &OperationDirection,
        layout_flip: Option<LayoutFlip>,
    ) -> OperationDirection {
        if let Some(flip) = layout_flip {
            match direction {
                OperationDirection::Left => match flip {
                    LayoutFlip::Horizontal | LayoutFlip::HorizontalAndVertical => {
                        OperationDirection::Right
                    }
                    _ => *direction,
                },
                OperationDirection::Right => match flip {
                    LayoutFlip::Horizontal | LayoutFlip::HorizontalAndVertical => {
                        OperationDirection::Left
                    }
                    _ => *direction,
                },
                OperationDirection::Up => match flip {
                    LayoutFlip::Vertical | LayoutFlip::HorizontalAndVertical => {
                        OperationDirection::Down
                    }
                    _ => *direction,
                },
                OperationDirection::Down => match flip {
                    LayoutFlip::Vertical | LayoutFlip::HorizontalAndVertical => {
                        OperationDirection::Up
                    }
                    _ => *direction,
                },
            }
        } else {
            *direction
        }
    }

    pub fn is_valid(
        &self,
        layout: Layout,
        layout_flip: Option<LayoutFlip>,
        idx: usize,
        len: usize,
    ) -> bool {
        match OperationDirection::flip_direction(self, layout_flip) {
            OperationDirection::Up => match layout {
                Layout::BSP => len > 2 && idx != 0 && idx != 1,
                Layout::Columns => false,
                Layout::Rows => idx != 0,
            },
            OperationDirection::Down => match layout {
                Layout::BSP => len > 2 && idx != len - 1 && idx % 2 != 0,
                Layout::Columns => false,
                Layout::Rows => idx != len - 1,
            },
            OperationDirection::Left => match layout {
                Layout::BSP => len > 1 && idx != 0,
                Layout::Columns => idx != 0,
                Layout::Rows => false,
            },
            OperationDirection::Right => match layout {
                Layout::BSP => len > 1 && idx % 2 == 0,
                Layout::Columns => idx != len - 1,
                Layout::Rows => false,
            },
        }
    }

    pub fn new_idx(&self, layout: Layout, layout_flip: Option<LayoutFlip>, idx: usize) -> usize {
        match OperationDirection::flip_direction(self, layout_flip) {
            OperationDirection::Up => match layout {
                Layout::BSP => {
                    if idx % 2 == 0 {
                        idx - 1
                    } else {
                        idx - 2
                    }
                }
                Layout::Columns => unreachable!(),
                Layout::Rows => idx - 1,
            },
            OperationDirection::Down => match layout {
                Layout::BSP | Layout::Rows => idx + 1,
                Layout::Columns => unreachable!(),
            },
            OperationDirection::Left => match layout {
                Layout::BSP => {
                    if idx % 2 == 0 {
                        idx - 2
                    } else {
                        idx - 1
                    }
                }
                Layout::Columns => idx - 1,
                Layout::Rows => unreachable!(),
            },
            OperationDirection::Right => match layout {
                Layout::BSP | Layout::Columns => idx + 1,
                Layout::Rows => unreachable!(),
            },
        }
    }
}
