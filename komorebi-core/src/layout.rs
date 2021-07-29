use clap::Clap;
use serde::Deserialize;
use serde::Serialize;
use strum::Display;
use strum::EnumString;

use crate::Rect;

#[derive(Clone, Copy, Debug, Serialize, Deserialize, Display, EnumString)]
#[strum(serialize_all = "snake_case")]
#[derive(Clap)]
pub enum Layout {
    BSP,
    Columns,
    Rows,
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize, Display, EnumString)]
#[strum(serialize_all = "snake_case")]
#[derive(Clap)]
pub enum LayoutFlip {
    Horizontal,
    Vertical,
    HorizontalAndVertical,
}

impl Layout {
    pub fn calculate(
        &self,
        area: &Rect,
        count: usize,
        container_padding: Option<i32>,
        layout_flip: Option<LayoutFlip>,
    ) -> Vec<Rect> {
        let mut dimensions = match self {
            Layout::BSP => self.fibonacci(area, count, layout_flip),
            Layout::Columns => {
                let right = area.right / count as i32;
                let mut left = 0;

                let mut layouts: Vec<Rect> = vec![];
                for _ in 0..count {
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
            Layout::Rows => {
                let bottom = area.bottom / count as i32;
                let mut top = 0;

                let mut layouts: Vec<Rect> = vec![];
                for _ in 0..count {
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
        };

        dimensions
            .iter_mut()
            .for_each(|l| l.add_padding(container_padding));

        dimensions
    }

    pub fn fibonacci(
        &self,
        area: &Rect,
        count: usize,
        layout_flip: Option<LayoutFlip>,
    ) -> Vec<Rect> {
        let mut dimensions = vec![];

        for _ in 0..count {
            dimensions.push(Rect::default())
        }

        let mut left = area.left;
        let mut top = area.top;
        let mut bottom = area.bottom;
        let mut right = area.right;

        for i in 0..count {
            if i % 2 != 0 {
                continue;
            }

            let half_width = right / 2;
            let half_height = bottom / 2;

            let (main_x, alt_x, new_y, alt_y);

            match layout_flip {
                Some(flip) => match flip {
                    LayoutFlip::Horizontal => {
                        main_x = left + half_width;
                        alt_x = left;

                        new_y = top + half_height;
                        alt_y = top;
                    }
                    LayoutFlip::Vertical => {
                        new_y = top;
                        alt_y = top + half_height;

                        main_x = left;
                        alt_x = left + half_width;
                    }
                    LayoutFlip::HorizontalAndVertical => {
                        main_x = left + half_width;
                        alt_x = left;
                        new_y = top;
                        alt_y = top + half_height;
                    }
                },
                None => {
                    main_x = left;
                    alt_x = left + half_width;
                    new_y = top + half_height;
                    alt_y = top;
                }
            }

            match count - i {
                1 => {
                    set_dimensions(&mut dimensions[i], left, top, right, bottom);
                }
                2 => {
                    set_dimensions(&mut dimensions[i], main_x, top, half_width, bottom);
                    set_dimensions(&mut dimensions[i + 1], alt_x, top, half_width, bottom);
                }
                _ => {
                    set_dimensions(&mut dimensions[i], main_x, top, half_width, bottom);
                    set_dimensions(
                        &mut dimensions[i + 1],
                        alt_x,
                        alt_y,
                        half_width,
                        half_height,
                    );

                    left = alt_x;
                    top = new_y;
                    right = half_width;
                    bottom = half_height;
                }
            }
        }

        dimensions
    }
}

impl Layout {
    pub fn next(&mut self) {
        match self {
            Layout::BSP => *self = Layout::Columns,
            Layout::Columns => *self = Layout::Rows,
            Layout::Rows => *self = Layout::BSP,
        }
    }

    pub fn previous(&mut self) {
        match self {
            Layout::BSP => *self = Layout::Rows,
            Layout::Columns => *self = Layout::BSP,
            Layout::Rows => *self = Layout::Columns,
        }
    }
}

fn set_dimensions(rect: &mut Rect, left: i32, top: i32, right: i32, bottom: i32) {
    rect.bottom = bottom;
    rect.right = right;
    rect.left = left;
    rect.top = top;
}
