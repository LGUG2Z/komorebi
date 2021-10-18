use std::num::NonZeroUsize;

use clap::ArgEnum;
use serde::Deserialize;
use serde::Serialize;
use strum::Display;
use strum::EnumString;

use crate::direction::Direction;
use crate::Flip;

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

    fn flip(self, layout_flip: Option<Flip>) -> Self {
        layout_flip.map_or(self, |flip| match self {
            Self::Left => match flip {
                Flip::Horizontal | Flip::HorizontalAndVertical => Self::Right,
                Flip::Vertical => self,
            },
            Self::Right => match flip {
                Flip::Horizontal | Flip::HorizontalAndVertical => Self::Left,
                Flip::Vertical => self,
            },
            Self::Up => match flip {
                Flip::Vertical | Flip::HorizontalAndVertical => Self::Down,
                Flip::Horizontal => self,
            },
            Self::Down => match flip {
                Flip::Vertical | Flip::HorizontalAndVertical => Self::Up,
                Flip::Horizontal => self,
            },
        })
    }

    #[must_use]
    pub fn destination(
        self,
        layout: &dyn Direction,
        layout_flip: Option<Flip>,
        idx: usize,
        len: NonZeroUsize,
    ) -> Option<usize> {
        layout.index_in_direction(self.flip(layout_flip), idx, len.get())
    }
}
