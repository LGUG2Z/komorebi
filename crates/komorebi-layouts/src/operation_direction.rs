use std::num::NonZeroUsize;

use super::Axis;
use super::direction::Direction;
use crate::default_layout::DefaultLayout;
use crate::default_layout::LayoutOptions;
use clap::ValueEnum;
use serde::Deserialize;
use serde::Serialize;
use strum::Display;
use strum::EnumString;

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
        layout_options: Option<LayoutOptions>,
    ) -> Option<usize> {
        layout.index_in_direction(self.flip(layout_flip), idx, len.get(), layout_options)
    }

    /// Index of the container to focus when crossing a workspace or monitor
    /// boundary by moving in `self` direction into `layout`.
    ///
    /// `layout_flip` mirrors a layout's geometry without reordering its
    /// containers, so the structural [`DefaultLayout::leftmost_index`] /
    /// [`DefaultLayout::rightmost_index`] must be selected against the *flipped*
    /// direction to match the adjustment [`OperationDirection::destination`] makes
    /// for intra-workspace focus. Otherwise, focus crossing a boundary into a
    /// flipped workspace lands on the container at the far edge instead of the
    /// one the user crossed toward.
    #[must_use]
    pub fn cross_boundary_edge_index(
        self,
        layout: DefaultLayout,
        len: usize,
        layout_flip: Option<Axis>,
    ) -> usize {
        match self.flip(layout_flip) {
            Self::Left => layout.rightmost_index(len),
            Self::Right => layout.leftmost_index(len),
            Self::Up | Self::Down => {
                unreachable!("only called for horizontal Left/Right crossings")
            }
        }
    }
}

#[cfg(test)]
#[path = "operation_direction_tests.rs"]
mod tests;
