use clap::ValueEnum;
use serde::Deserialize;
use serde::Serialize;
use strum::Display;
use strum::EnumString;

use super::OperationDirection;
use super::Rect;
use super::Sizing;

#[derive(
    Clone, Copy, Debug, Serialize, Deserialize, Eq, PartialEq, Display, EnumString, ValueEnum,
)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
/// A predefined komorebi layout
pub enum DefaultLayout {
    /// BSP Layout
    ///
    /// ```
    /// +-------+-----+
    /// |       |     |
    /// |       +--+--+
    /// |       |  |--|
    /// +-------+--+--+
    /// ```
    BSP,
    /// Columns Layout
    ///
    /// ```
    /// +--+--+--+--+
    /// |  |  |  |  |
    /// |  |  |  |  |
    /// |  |  |  |  |
    /// +--+--+--+--+
    /// ```
    Columns,
    /// Rows Layout
    ///
    /// ```
    /// +-----------+
    /// |-----------|
    /// |-----------|
    /// |-----------|
    /// +-----------+
    /// ```
    Rows,
    /// Vertical Stack Layout
    ///
    /// ```
    /// +-------+-----+
    /// |       |     |
    /// |       +-----+
    /// |       |     |
    /// +-------+-----+
    /// ```
    VerticalStack,
    /// Horizontal Stack Layout
    ///
    /// ```
    /// +------+------+
    /// |             |
    /// |------+------+
    /// |      |      |
    /// +------+------+
    /// ```
    HorizontalStack,
    /// Ultrawide Vertical Stack Layout
    ///
    /// ```
    /// +-----+-----------+-----+
    /// |     |           |     |
    /// |     |           +-----+
    /// |     |           |     |
    /// |     |           +-----+
    /// |     |           |     |
    /// +-----+-----------+-----+
    /// ```
    UltrawideVerticalStack,
    /// Grid Layout
    ///
    /// ```
    /// +-----+-----+   +---+---+---+   +---+---+---+   +---+---+---+
    /// |     |     |   |   |   |   |   |   |   |   |   |   |   |   |
    /// |     |     |   |   |   |   |   |   |   |   |   |   |   +---+
    /// +-----+-----+   |   +---+---+   +---+---+---+   +---+---|   |
    /// |     |     |   |   |   |   |   |   |   |   |   |   |   +---+
    /// |     |     |   |   |   |   |   |   |   |   |   |   |   |   |
    /// +-----+-----+   +---+---+---+   +---+---+---+   +---+---+---+
    ///   4 windows       5 windows       6 windows       7 windows
    ///```
    Grid,
    /// Right Main Vertical Stack Layout
    ///
    /// ```
    /// +-----+-------+
    /// |     |       |
    /// +-----+       |
    /// |     |       |
    /// +-----+-------+
    /// ```
    RightMainVerticalStack,
    /// Scrolling Layout
    ///
    /// ```
    /// +--+--+--+--+--+--+
    /// |     |     |     |
    /// |     |     |     |
    /// |     |     |     |
    /// +--+--+--+--+--+--+
    /// ```
    Scrolling,
    // NOTE: If any new layout is added, please make sure to register the same in `DefaultLayout::cycle`
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize, Eq, PartialEq)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
/// Options for specific layouts
pub struct LayoutOptions {
    /// Options related to the Scrolling layout
    pub scrolling: Option<ScrollingLayoutOptions>,
    /// Options related to the Grid layout
    pub grid: Option<GridLayoutOptions>,
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize, Eq, PartialEq)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
/// Options for the Scrolling layout
pub struct ScrollingLayoutOptions {
    /// Desired number of visible columns (default: 3)
    pub columns: usize,
    /// With an odd number of visible columns, keep the focused window column centered
    pub center_focused_column: Option<bool>,
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize, Eq, PartialEq)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
/// Options for the Grid layout
pub struct GridLayoutOptions {
    /// Maximum number of rows per grid column
    pub rows: usize,
}

impl DefaultLayout {
    pub fn leftmost_index(&self, len: usize) -> usize {
        match self {
            Self::UltrawideVerticalStack | Self::RightMainVerticalStack => match len {
                n if n > 1 => 1,
                _ => 0,
            },
            Self::Scrolling => 0,
            DefaultLayout::BSP
            | DefaultLayout::Columns
            | DefaultLayout::Rows
            | DefaultLayout::VerticalStack
            | DefaultLayout::HorizontalStack
            | DefaultLayout::Grid => 0,
        }
    }

    pub fn rightmost_index(&self, len: usize) -> usize {
        match self {
            DefaultLayout::BSP
            | DefaultLayout::Columns
            | DefaultLayout::Rows
            | DefaultLayout::VerticalStack
            | DefaultLayout::HorizontalStack
            | DefaultLayout::Grid => len.saturating_sub(1),
            DefaultLayout::UltrawideVerticalStack => match len {
                2 => 0,
                _ => len.saturating_sub(1),
            },
            DefaultLayout::RightMainVerticalStack => 0,
            DefaultLayout::Scrolling => len.saturating_sub(1),
        }
    }

    #[must_use]
    #[allow(clippy::cast_precision_loss, clippy::only_used_in_recursion)]
    pub fn resize(
        &self,
        unaltered: &Rect,
        resize: &Option<Rect>,
        edge: OperationDirection,
        sizing: Sizing,
        delta: i32,
    ) -> Option<Rect> {
        if !matches!(
            self,
            Self::BSP
                | Self::Columns
                | Self::Rows
                | Self::VerticalStack
                | Self::RightMainVerticalStack
                | Self::HorizontalStack
                | Self::UltrawideVerticalStack
                | Self::Scrolling
        ) {
            return None;
        };

        let mut r = resize.unwrap_or_default();

        let resize_delta = delta;

        match edge {
            OperationDirection::Left => match sizing {
                Sizing::Increase => {
                    // Some final checks to make sure the user can't infinitely resize to
                    // the point of pushing other windows out of bounds

                    // Note: These checks cannot take into account the changes made to the
                    // edges of adjacent windows at operation time, so it is still possible
                    // to push windows out of bounds by maxing out an Increase Left on a
                    // Window with index 1, and then maxing out a Decrease Right on a Window
                    // with index 0. I don't think it's worth trying to defensively program
                    // against this; if people end up in this situation they are better off
                    // just hitting the retile command
                    let diff = ((r.left + -resize_delta) as f32).abs();
                    if diff < unaltered.right as f32 {
                        r.left += -resize_delta;
                    }
                }
                Sizing::Decrease => {
                    let diff = ((r.left - -resize_delta) as f32).abs();
                    if diff < unaltered.right as f32 {
                        r.left -= -resize_delta;
                    }
                }
            },
            OperationDirection::Up => match sizing {
                Sizing::Increase => {
                    let diff = ((r.top + resize_delta) as f32).abs();
                    if diff < unaltered.bottom as f32 {
                        r.top += -resize_delta;
                    }
                }
                Sizing::Decrease => {
                    let diff = ((r.top - resize_delta) as f32).abs();
                    if diff < unaltered.bottom as f32 {
                        r.top -= -resize_delta;
                    }
                }
            },
            OperationDirection::Right => match sizing {
                Sizing::Increase => {
                    let diff = ((r.right + resize_delta) as f32).abs();
                    if diff < unaltered.right as f32 {
                        r.right += resize_delta;
                    }
                }
                Sizing::Decrease => {
                    let diff = ((r.right - resize_delta) as f32).abs();
                    if diff < unaltered.right as f32 {
                        r.right -= resize_delta;
                    }
                }
            },
            OperationDirection::Down => match sizing {
                Sizing::Increase => {
                    let diff = ((r.bottom + resize_delta) as f32).abs();
                    if diff < unaltered.bottom as f32 {
                        r.bottom += resize_delta;
                    }
                }
                Sizing::Decrease => {
                    let diff = ((r.bottom - resize_delta) as f32).abs();
                    if diff < unaltered.bottom as f32 {
                        r.bottom -= resize_delta;
                    }
                }
            },
        };

        if r.eq(&Rect::default()) {
            None
        } else {
            Option::from(r)
        }
    }

    #[must_use]
    pub const fn cycle_next(self) -> Self {
        match self {
            Self::BSP => Self::Columns,
            Self::Columns => Self::Rows,
            Self::Rows => Self::VerticalStack,
            Self::VerticalStack => Self::HorizontalStack,
            Self::HorizontalStack => Self::UltrawideVerticalStack,
            Self::UltrawideVerticalStack => Self::Grid,
            Self::Grid => Self::RightMainVerticalStack,
            Self::RightMainVerticalStack => Self::Scrolling,
            Self::Scrolling => Self::BSP,
        }
    }

    #[must_use]
    pub const fn cycle_previous(self) -> Self {
        match self {
            Self::Scrolling => Self::RightMainVerticalStack,
            Self::RightMainVerticalStack => Self::Grid,
            Self::Grid => Self::UltrawideVerticalStack,
            Self::UltrawideVerticalStack => Self::HorizontalStack,
            Self::HorizontalStack => Self::VerticalStack,
            Self::VerticalStack => Self::Rows,
            Self::Rows => Self::Columns,
            Self::Columns => Self::BSP,
            Self::BSP => Self::RightMainVerticalStack,
        }
    }
}
