use crate::config::AlphaColour;
use crate::config::Position;
use eframe::egui::Color32;
use eframe::egui::Frame;
use eframe::egui::InnerResponse;
use eframe::egui::Margin;
use eframe::egui::Rounding;
use eframe::egui::Shadow;
use eframe::egui::Stroke;
use eframe::egui::Ui;
use eframe::egui::Vec2;
use komorebi_client::Colour;
use komorebi_client::Rect;
use schemars::JsonSchema;
use serde::Deserialize;
use serde::Serialize;

#[derive(Copy, Clone, Debug, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "kind")]
pub enum Grouping {
    /// No grouping is applied
    None,
    /// Widgets are grouped on the bar
    Bar(GroupingConfig),
    /// Widgets are grouped on each side
    Side(GroupingConfig),
    /// Widgets are grouped individually
    Widget(GroupingConfig),
}

impl Grouping {
    pub fn apply_on_bar<R>(
        &mut self,
        ui: &mut Ui,
        add_contents: impl FnOnce(&mut Ui) -> R,
    ) -> InnerResponse<R> {
        match self {
            Self::Bar(config) => Self::define_frame(ui, config).show(ui, add_contents),
            Self::Widget(_) => Self::default_response(ui, add_contents),
            Self::Side(_) => Self::default_response(ui, add_contents),
            Self::None => Self::default_response(ui, add_contents),
        }
    }

    pub fn apply_on_widget<R>(
        &mut self,
        ui: &mut Ui,
        add_contents: impl FnOnce(&mut Ui) -> R,
    ) -> InnerResponse<R> {
        match self {
            Self::Bar(_) => Self::default_response(ui, add_contents),
            Self::Widget(config) => Self::define_frame(ui, config).show(ui, add_contents),
            Self::Side(_) => Self::default_response(ui, add_contents),
            Self::None => Self::default_response(ui, add_contents),
        }
    }

    pub fn apply_on_side<R>(
        &mut self,
        ui: &mut Ui,
        add_contents: impl FnOnce(&mut Ui) -> R,
    ) -> InnerResponse<R> {
        match self {
            Self::Bar(_) => Self::default_response(ui, add_contents),
            Self::Widget(_) => Self::default_response(ui, add_contents),
            Self::Side(config) => Self::define_frame(ui, config).show(ui, add_contents),
            Self::None => Self::default_response(ui, add_contents),
        }
    }

    fn define_frame(ui: &mut Ui, config: &mut GroupingConfig) -> Frame {
        Frame::none()
            .fill(match config.fill {
                Some(color) => color.to_color32_or(None),
                None => Color32::TRANSPARENT,
            })
            .outer_margin(match config.outer_margin {
                Some(margin) => Self::rect_to_margin(margin),
                None => Margin::symmetric(0.0, 0.0),
            })
            .inner_margin(match config.inner_margin {
                Some(margin) => Self::rect_to_margin(margin),
                None => Margin::symmetric(5.0, 2.0),
            })
            .rounding(match config.rounding {
                Some(rounding) => rounding.into(),
                None => Rounding::same(5.0),
            })
            .stroke(match config.stroke {
                Some(line) => line.into(),
                None => ui.style().visuals.widgets.noninteractive.bg_stroke,
            })
            .shadow(match config.shadow {
                Some(shadow) => shadow.into(),
                None => Shadow::NONE,
            })
    }

    fn default_response<R>(
        ui: &mut Ui,
        add_contents: impl FnOnce(&mut Ui) -> R,
    ) -> InnerResponse<R> {
        InnerResponse {
            inner: add_contents(ui),
            response: ui.response().clone(),
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
    pub shadow: Option<BoxShadow>,
}

#[derive(Copy, Clone, Debug, Serialize, Deserialize, JsonSchema)]
pub struct Line {
    pub width: Option<f32>,
    pub color: Option<Colour>,
}

impl From<Line> for Stroke {
    fn from(value: Line) -> Self {
        Self {
            width: value.width.unwrap_or(1.0),
            color: match value.color {
                Some(color) => color.into(),
                None => Color32::from_rgb(0, 0, 0),
            },
        }
    }
}

#[derive(Copy, Clone, Debug, Serialize, Deserialize, JsonSchema)]
#[serde(untagged)]
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

#[derive(Copy, Clone, Debug, Serialize, Deserialize, JsonSchema)]
pub struct BoxShadow {
    /// Move the shadow by this much.
    ///
    /// For instance, a value of `[1.0, 2.0]` will move the shadow 1 point to the right and 2 points down,
    /// causing a drop-shadow effect.
    pub offset: Option<Position>,

    /// The width of the blur, i.e. the width of the fuzzy penumbra.
    ///
    /// A value of 0.0 means a sharp shadow.
    pub blur: Option<f32>,

    /// Expand the shadow in all directions by this much.
    pub spread: Option<f32>,

    /// Color of the opaque center of the shadow.
    pub color: Option<AlphaColour>,
}

impl From<BoxShadow> for Shadow {
    fn from(value: BoxShadow) -> Self {
        Shadow {
            offset: match value.offset {
                Some(offset) => offset.into(),
                None => Vec2::ZERO,
            },
            blur: value.blur.unwrap_or(0.0),
            spread: value.spread.unwrap_or(0.0),
            color: match value.color {
                Some(color) => color.to_color32_or(None),
                None => Color32::TRANSPARENT,
            },
        }
    }
}
