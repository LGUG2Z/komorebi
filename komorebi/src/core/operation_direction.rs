use std::num::NonZeroUsize;

use clap::ValueEnum;
use serde::Deserialize;
use serde::Serialize;
use strum::Display;
use strum::EnumString;

use super::Axis;
use super::direction::Direction;

#[derive(Clone, Copy, Debug, Serialize, Deserialize, Display, EnumString, ValueEnum)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
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

    fn flip(self, layout_flip: Option<Axis>) -> Self {
        layout_flip.map_or(self, |flip| match self {
            Self::Left => match flip {
                Axis::Horizontal | Axis::HorizontalAndVertical => Self::Right,
                Axis::Vertical => self,
            },
            Self::Right => match flip {
                Axis::Horizontal | Axis::HorizontalAndVertical => Self::Left,
                Axis::Vertical => self,
            },
            Self::Up => match flip {
                Axis::Vertical | Axis::HorizontalAndVertical => Self::Down,
                Axis::Horizontal => self,
            },
            Self::Down => match flip {
                Axis::Vertical | Axis::HorizontalAndVertical => Self::Up,
                Axis::Horizontal => self,
            },
        })
    }

    #[must_use]
    pub fn destination(
        self,
        layout: &dyn Direction,
        layout_flip: Option<Axis>,
        idx: usize,
        len: NonZeroUsize,
    ) -> Option<usize> {
        layout.index_in_direction(self.flip(layout_flip), idx, len.get())
    }
}
