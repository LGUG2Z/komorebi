use std::num::NonZeroUsize;

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
pub enum Layout {
    BSP,
    Columns,
    Rows,
    VerticalStack,
    HorizontalStack,
    UltrawideVerticalStack,
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize, Display, EnumString, ArgEnum)]
#[strum(serialize_all = "snake_case")]
pub enum Flip {
    Horizontal,
    Vertical,
    HorizontalAndVertical,
}

impl Layout {
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

    #[must_use]
    #[allow(clippy::too_many_lines)]
    pub fn calculate(
        &self,
        area: &Rect,
        len: NonZeroUsize,
        container_padding: Option<i32>,
        layout_flip: Option<Flip>,
        resize_dimensions: &[Option<Rect>],
    ) -> Vec<Rect> {
        let len = usize::from(len);
        let mut dimensions = match self {
            Layout::BSP => recursive_fibonacci(
                0,
                len,
                area,
                layout_flip,
                calculate_resize_adjustments(resize_dimensions),
            ),
            Layout::Columns => columns(area, len),
            Layout::Rows => rows(area, len),
            Layout::VerticalStack => {
                let mut layouts: Vec<Rect> = vec![];

                let primary_right = match len {
                    1 => area.right,
                    _ => area.right / 2,
                };

                let mut main_left = area.left;
                let mut stack_left = area.left + primary_right;

                match layout_flip {
                    Some(Flip::Horizontal | Flip::HorizontalAndVertical) if len > 1 => {
                        main_left = main_left + area.right - primary_right;
                        stack_left = area.left;
                    }
                    _ => {}
                }

                if len >= 1 {
                    layouts.push(Rect {
                        left: main_left,
                        top: area.top,
                        right: primary_right,
                        bottom: area.bottom,
                    });

                    if len > 1 {
                        layouts.append(&mut rows(
                            &Rect {
                                left: stack_left,
                                top: area.top,
                                right: area.right - primary_right,
                                bottom: area.bottom,
                            },
                            len - 1,
                        ));
                    }
                }

                layouts
            }
            Layout::HorizontalStack => {
                let mut layouts: Vec<Rect> = vec![];

                let bottom = match len {
                    1 => area.bottom,
                    _ => area.bottom / 2,
                };

                let mut main_top = area.top;
                let mut stack_top = area.top + bottom;

                match layout_flip {
                    Some(Flip::Vertical | Flip::HorizontalAndVertical) if len > 1 => {
                        main_top = main_top + area.bottom - bottom;
                        stack_top = area.top;
                    }
                    _ => {}
                }

                if len >= 1 {
                    layouts.push(Rect {
                        left: area.left,
                        top: main_top,
                        right: area.right,
                        bottom,
                    });

                    if len > 1 {
                        layouts.append(&mut columns(
                            &Rect {
                                left: area.left,
                                top: stack_top,
                                right: area.right,
                                bottom: area.bottom - bottom,
                            },
                            len - 1,
                        ));
                    }
                }

                layouts
            }
            Layout::UltrawideVerticalStack => {
                let mut layouts: Vec<Rect> = vec![];

                let primary_right = match len {
                    1 => area.right,
                    _ => area.right / 2,
                };

                let secondary_right = match len {
                    1 => 0,
                    2 => area.right - primary_right,
                    _ => (area.right - primary_right) / 2,
                };

                let (primary_left, secondary_left, stack_left) = match len {
                    1 => (area.left, 0, 0),
                    2 => {
                        let mut primary = area.left + secondary_right;
                        let mut secondary = area.left;

                        match layout_flip {
                            Some(Flip::Horizontal | Flip::HorizontalAndVertical) if len > 1 => {
                                primary = area.left;
                                secondary = area.left + primary_right;
                            }
                            _ => {}
                        }

                        (primary, secondary, 0)
                    }
                    _ => {
                        let primary = area.left + secondary_right;
                        let mut secondary = area.left;
                        let mut stack = area.left + primary_right + secondary_right;

                        match layout_flip {
                            Some(Flip::Horizontal | Flip::HorizontalAndVertical) if len > 1 => {
                                secondary = area.left + primary_right + secondary_right;
                                stack = area.left;
                            }
                            _ => {}
                        }

                        (primary, secondary, stack)
                    }
                };

                if len >= 1 {
                    layouts.push(Rect {
                        left: primary_left,
                        top: area.top,
                        right: primary_right,
                        bottom: area.bottom,
                    });

                    if len >= 2 {
                        layouts.push(Rect {
                            left: secondary_left,
                            top: area.top,
                            right: secondary_right,
                            bottom: area.bottom,
                        });

                        if len > 2 {
                            layouts.append(&mut rows(
                                &Rect {
                                    left: stack_left,
                                    top: area.top,
                                    right: secondary_right,
                                    bottom: area.bottom,
                                },
                                len - 2,
                            ));
                        }
                    }
                }

                layouts
            }
        };

        dimensions
            .iter_mut()
            .for_each(|l| l.add_padding(container_padding));

        dimensions
    }
}

fn columns(area: &Rect, len: usize) -> Vec<Rect> {
    #[allow(clippy::cast_possible_wrap, clippy::cast_possible_truncation)]
    let right = area.right / len as i32;
    let mut left = 0;

    let mut layouts: Vec<Rect> = vec![];
    for _ in 0..len {
        layouts.push(Rect {
            left: area.left + left,
            top: area.top,
            right,
            bottom: area.bottom,
        });

        left += right;
    }

    layouts
}

fn rows(area: &Rect, len: usize) -> Vec<Rect> {
    #[allow(clippy::cast_possible_wrap, clippy::cast_possible_truncation)]
    let bottom = area.bottom / len as i32;
    let mut top = 0;

    let mut layouts: Vec<Rect> = vec![];
    for _ in 0..len {
        layouts.push(Rect {
            left: area.left,
            top: area.top + top,
            right: area.right,
            bottom,
        });

        top += bottom;
    }

    layouts
}

fn calculate_resize_adjustments(resize_dimensions: &[Option<Rect>]) -> Vec<Option<Rect>> {
    let mut resize_adjustments = resize_dimensions.to_vec();

    // This needs to be aware of layout flips
    for (i, opt) in resize_dimensions.iter().enumerate() {
        if let Some(resize_ref) = opt {
            if i > 0 {
                if resize_ref.left != 0 {
                    #[allow(clippy::if_not_else)]
                    let range = if i == 1 {
                        0..1
                    } else if i & 1 != 0 {
                        i - 1..i
                    } else {
                        i - 2..i
                    };

                    for n in range {
                        let should_adjust = n % 2 == 0;
                        if should_adjust {
                            if let Some(Some(adjacent_resize)) = resize_adjustments.get_mut(n) {
                                adjacent_resize.right += resize_ref.left;
                            } else {
                                resize_adjustments[n] = Option::from(Rect {
                                    left: 0,
                                    top: 0,
                                    right: resize_ref.left,
                                    bottom: 0,
                                });
                            }
                        }
                    }

                    if let Some(rr) = resize_adjustments[i].as_mut() {
                        rr.left = 0;
                    }
                }

                if resize_ref.top != 0 {
                    let range = if i == 1 {
                        0..1
                    } else if i & 1 == 0 {
                        i - 1..i
                    } else {
                        i - 2..i
                    };

                    for n in range {
                        let should_adjust = n % 2 != 0;
                        if should_adjust {
                            if let Some(Some(adjacent_resize)) = resize_adjustments.get_mut(n) {
                                adjacent_resize.bottom += resize_ref.top;
                            } else {
                                resize_adjustments[n] = Option::from(Rect {
                                    left: 0,
                                    top: 0,
                                    right: 0,
                                    bottom: resize_ref.top,
                                });
                            }
                        }
                    }

                    if let Some(Some(resize)) = resize_adjustments.get_mut(i) {
                        resize.top = 0;
                    }
                }
            }
        }
    }

    let cleaned_resize_adjustments: Vec<_> = resize_adjustments
        .iter()
        .map(|adjustment| match adjustment {
            None => None,
            Some(rect) if rect.eq(&Rect::default()) => None,
            Some(_) => *adjustment,
        })
        .collect();

    cleaned_resize_adjustments
}

fn recursive_fibonacci(
    idx: usize,
    count: usize,
    area: &Rect,
    layout_flip: Option<Flip>,
    resize_adjustments: Vec<Option<Rect>>,
) -> Vec<Rect> {
    let mut a = *area;

    let resized = if let Some(Some(r)) = resize_adjustments.get(idx) {
        a.left += r.left;
        a.top += r.top;
        a.right += r.right;
        a.bottom += r.bottom;
        a
    } else {
        *area
    };

    let half_width = area.right / 2;
    let half_height = area.bottom / 2;
    let half_resized_width = resized.right / 2;
    let half_resized_height = resized.bottom / 2;

    let (main_x, alt_x, alt_y, main_y);

    if let Some(flip) = layout_flip {
        match flip {
            Flip::Horizontal => {
                main_x = resized.left + half_width + (half_width - half_resized_width);
                alt_x = resized.left;

                alt_y = resized.top + half_resized_height;
                main_y = resized.top;
            }
            Flip::Vertical => {
                main_y = resized.top + half_height + (half_height - half_resized_height);
                alt_y = resized.top;

                main_x = resized.left;
                alt_x = resized.left + half_resized_width;
            }
            Flip::HorizontalAndVertical => {
                main_x = resized.left + half_width + (half_width - half_resized_width);
                alt_x = resized.left;
                main_y = resized.top + half_height + (half_height - half_resized_height);
                alt_y = resized.top;
            }
        }
    } else {
        main_x = resized.left;
        alt_x = resized.left + half_resized_width;
        main_y = resized.top;
        alt_y = resized.top + half_resized_height;
    }

    #[allow(clippy::if_not_else)]
    if count == 0 {
        vec![]
    } else if count == 1 {
        vec![Rect {
            left: resized.left,
            top: resized.top,
            right: resized.right,
            bottom: resized.bottom,
        }]
    } else if idx % 2 != 0 {
        let mut res = vec![Rect {
            left: resized.left,
            top: main_y,
            right: resized.right,
            bottom: half_resized_height,
        }];
        res.append(&mut recursive_fibonacci(
            idx + 1,
            count - 1,
            &Rect {
                left: area.left,
                top: alt_y,
                right: area.right,
                bottom: area.bottom - half_resized_height,
            },
            layout_flip,
            resize_adjustments,
        ));
        res
    } else {
        let mut res = vec![Rect {
            left: main_x,
            top: resized.top,
            right: half_resized_width,
            bottom: resized.bottom,
        }];
        res.append(&mut recursive_fibonacci(
            idx + 1,
            count - 1,
            &Rect {
                left: alt_x,
                top: area.top,
                right: area.right - half_resized_width,
                bottom: area.bottom,
            },
            layout_flip,
            resize_adjustments,
        ));
        res
    }
}
