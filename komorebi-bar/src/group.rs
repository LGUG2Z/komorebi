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
}

#[derive(Copy, Clone, Debug, Serialize, Deserialize, JsonSchema)]
pub struct GroupingConfig {
    pub fill: Option<AlphaColour>,
    pub rounding: Option<RoundingConfig>,
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
            color: match value.color {
                Some(color) => color.into(),
                None => Color32::from_rgb(0, 0, 0),
            },
        }
    }
}

#[derive(Copy, Clone, Debug, Serialize, Deserialize, JsonSchema)]
pub enum RoundingConfig {
    /// All 4 corners are the same    
    Same(f32),
    /// All 4 corners are custom. Order: NW, NE, SW, SE
    Individual([f32; 4]),
}

impl From<RoundingConfig> for Rounding {
    fn from(value: RoundingConfig) -> Self {
        match value {
            RoundingConfig::Same(value) => Rounding::same(value),
            RoundingConfig::Individual(values) => Self {
                nw: values[0],
                ne: values[1],
                sw: values[2],
                se: values[3],
            },
        }
    }
}
