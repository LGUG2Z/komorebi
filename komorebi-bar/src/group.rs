use crate::config::AlphaColour;
use eframe::egui::Color32;
use eframe::egui::Frame;
use eframe::egui::InnerResponse;
use eframe::egui::Margin;
use eframe::egui::Rounding;
use eframe::egui::Stroke;
use eframe::egui::Ui;
use komorebi_client::Colour;
use komorebi_client::Rect;
use komorebi_client::Rgb;
use schemars::JsonSchema;
use serde::Deserialize;
use serde::Serialize;

#[derive(Copy, Clone, Debug, Serialize, Deserialize, JsonSchema)]
pub enum Grouping {
    /// No grouping is applied
    None,
    /// Widgets are grouped individually
    Widget(GroupingConfig),
    /// Widgets are grouped on each side
    Side(GroupingConfig),
}

impl Grouping {
    pub fn apply_on_widget<R>(
        &mut self,
        ui: &mut Ui,
        add_contents: impl FnOnce(&mut Ui) -> R,
    ) -> InnerResponse<R> {
        match self {
            Self::Widget(config) => Frame::none()
                .fill(match config.fill {
                    Some(color) => color.into(),
                    None => Color32::TRANSPARENT,
                })
                .outer_margin(match config.outer_margin {
                    Some(margin) => Self::rect_to_margin(margin),
                    None => Margin::symmetric(0.0, 0.0),
                })
                .inner_margin(match config.inner_margin {
                    Some(margin) => Self::rect_to_margin(margin),
                    None => Margin::symmetric(7.0, 2.0),
                })
                .rounding(match config.rounding {
                    Some(rounding) => rounding.into(),
                    None => Rounding::same(10.0),
                })
                .stroke(match config.stroke {
                    Some(line) => line.into(),
                    None => ui.style().visuals.widgets.noninteractive.bg_stroke,
                })
                .show(ui, add_contents),
            Self::Side(_config) => InnerResponse {
                inner: add_contents(ui),
                response: ui.response().clone(),
            },
            Self::None => InnerResponse {
                inner: add_contents(ui),
                response: ui.response().clone(),
            },
        }
    }

    pub fn apply_on_side<R>(
        &mut self,
        ui: &mut Ui,
        add_contents: impl FnOnce(&mut Ui) -> R,
    ) -> InnerResponse<R> {
        match self {
            Self::Widget(_config) => InnerResponse {
                inner: add_contents(ui),
                response: ui.response().clone(),
            },
            Self::Side(config) => Frame::none()
                .fill(match config.fill {
                    Some(color) => color.into(),
                    None => Color32::TRANSPARENT,
                })
                .outer_margin(match config.outer_margin {
                    Some(margin) => Self::rect_to_margin(margin),
                    None => Margin::symmetric(0.0, 0.0),
                })
                .inner_margin(match config.inner_margin {
                    Some(margin) => Self::rect_to_margin(margin),
                    None => Margin::symmetric(7.0, 2.0),
                })
                .rounding(match config.rounding {
                    Some(rounding) => rounding.into(),
                    None => Rounding::same(10.0),
                })
                .stroke(match config.stroke {
                    Some(line) => line.into(),
                    None => ui.style().visuals.widgets.noninteractive.bg_stroke,
                })
                .show(ui, add_contents),
            Self::None => InnerResponse {
                inner: add_contents(ui),
                response: ui.response().clone(),
            },
        }
    }

    fn rect_to_margin(rect: Rect) -> Margin {
        Margin {
            left: rect.left as f32,
            right: rect.right as f32,
            top: rect.top as f32,
            bottom: rect.bottom as f32,
        }
    }

    // NOTE: this is also in the komorebi_gui. Should be moved to the "komorebi colour"
    pub fn colour_to_color32(colour: Option<Colour>) -> Color32 {
        match colour {
            Some(Colour::Rgb(rgb)) => Color32::from_rgb(rgb.r as u8, rgb.g as u8, rgb.b as u8),
            Some(Colour::Hex(hex)) => {
                let rgb = Rgb::from(hex);
                Color32::from_rgb(rgb.r as u8, rgb.g as u8, rgb.b as u8)
            }
            //None => Color32::from_rgb(0, 0, 0),
            None => Color32::TRANSPARENT,
        }
    }
}

#[derive(Copy, Clone, Debug, Serialize, Deserialize, JsonSchema)]
pub struct GroupingConfig {
    pub fill: Option<AlphaColour>,
    pub rounding: Option<BorderRadius>,
    pub outer_margin: Option<Rect>,
    pub inner_margin: Option<Rect>,
    pub stroke: Option<Line>,
}

#[derive(Copy, Clone, Debug, Serialize, Deserialize, JsonSchema)]
pub struct Line {
    pub width: f32,
    pub color: Option<Colour>,
}

impl From<Line> for Stroke {
    fn from(value: Line) -> Self {
        Self {
            width: value.width,
            color: Grouping::colour_to_color32(value.color),
        }
    }
}

#[derive(Copy, Clone, Debug, Serialize, Deserialize, JsonSchema)]
pub struct BorderRadius {
    /// Radius of the rounding of the North-West (left top) corner.
    pub nw: f32,
    /// Radius of the rounding of the North-East (right top) corner.
    pub ne: f32,
    /// Radius of the rounding of the South-West (left bottom) corner.
    pub sw: f32,
    /// Radius of the rounding of the South-East (right bottom) corner.
    pub se: f32,
}

impl From<BorderRadius> for Rounding {
    fn from(value: BorderRadius) -> Self {
        Self {
            nw: value.nw,
            ne: value.ne,
            sw: value.sw,
            se: value.se,
        }
    }
}
