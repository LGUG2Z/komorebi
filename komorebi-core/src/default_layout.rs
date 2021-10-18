use clap::ArgEnum;
use serde::Deserialize;
use serde::Serialize;
use strum::Display;
use strum::EnumString;

use crate::OperationDirection;
use crate::Rect;
use crate::Sizing;

#[derive(Clone, Copy, Debug, Serialize, Deserialize, Display, EnumString, ArgEnum)]
#[strum(serialize_all = "snake_case")]
pub enum DefaultLayout {
    BSP,
    Columns,
    Rows,
    VerticalStack,
    HorizontalStack,
    UltrawideVerticalStack,
}

impl DefaultLayout {
    #[must_use]
    #[allow(clippy::cast_precision_loss)]
    pub fn resize(
        &self,
        unaltered: &Rect,
        resize: &Option<Rect>,
        edge: OperationDirection,
        sizing: Sizing,
        step: Option<i32>,
    ) -> Option<Rect> {
        if !matches!(self, Self::BSP) {
            return None;
        };

        let max_divisor = 1.005;
        let mut r = resize.unwrap_or_default();

        let resize_step = step.unwrap_or(50);

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
                    let diff = ((r.left + -resize_step) as f32).abs();
                    let max = unaltered.right as f32 / max_divisor;
                    if diff < max {
                        r.left += -resize_step;
                    }
                }
                Sizing::Decrease => {
                    let diff = ((r.left - -resize_step) as f32).abs();
                    let max = unaltered.right as f32 / max_divisor;
                    if diff < max {
                        r.left -= -resize_step;
                    }
                }
            },
            OperationDirection::Up => match sizing {
                Sizing::Increase => {
                    let diff = ((r.top + resize_step) as f32).abs();
                    let max = unaltered.bottom as f32 / max_divisor;
                    if diff < max {
                        r.top += -resize_step;
                    }
                }
                Sizing::Decrease => {
                    let diff = ((r.top - resize_step) as f32).abs();
                    let max = unaltered.bottom as f32 / max_divisor;
                    if diff < max {
                        r.top -= -resize_step;
                    }
                }
            },
            OperationDirection::Right => match sizing {
                Sizing::Increase => {
                    let diff = ((r.right + resize_step) as f32).abs();
                    let max = unaltered.right as f32 / max_divisor;
                    if diff < max {
                        r.right += resize_step;
                    }
                }
                Sizing::Decrease => {
                    let diff = ((r.right - resize_step) as f32).abs();
                    let max = unaltered.right as f32 / max_divisor;
                    if diff < max {
                        r.right -= resize_step;
                    }
                }
            },
            OperationDirection::Down => match sizing {
                Sizing::Increase => {
                    let diff = ((r.bottom + resize_step) as f32).abs();
                    let max = unaltered.bottom as f32 / max_divisor;
                    if diff < max {
                        r.bottom += resize_step;
                    }
                }
                Sizing::Decrease => {
                    let diff = ((r.bottom - resize_step) as f32).abs();
                    let max = unaltered.bottom as f32 / max_divisor;
                    if diff < max {
                        r.bottom -= resize_step;
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
}
