use crate::config::Color32Ext;
use crate::BACKGROUND_COLOR;
use eframe::egui::Color32;
use eframe::egui::Frame;
use eframe::egui::InnerResponse;
use eframe::egui::Margin;
use eframe::egui::Rounding;
use eframe::egui::Shadow;
use eframe::egui::Ui;
use eframe::egui::Vec2;
use schemars::JsonSchema;
use serde::Deserialize;
use serde::Serialize;
use std::sync::atomic::Ordering;

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
            Self::Side(_) => Self::default_response(ui, add_contents),
            Self::Widget(_) => Self::default_response(ui, add_contents),
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
            Self::Side(config) => Self::define_frame(ui, config).show(ui, add_contents),
            Self::Widget(_) => Self::default_response(ui, add_contents),
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
            Self::Side(_) => Self::default_response(ui, add_contents),
            Self::Widget(config) => Self::define_frame(ui, config).show(ui, add_contents),
            Self::None => Self::default_response(ui, add_contents),
        }
    }

    fn define_frame(ui: &mut Ui, config: &mut GroupingConfig) -> Frame {
        Frame::none()
            .outer_margin(Margin::same(0.0))
            .inner_margin(Margin::symmetric(3.0, 3.0))
            .stroke(ui.style().visuals.widgets.noninteractive.bg_stroke)
            .rounding(match config.rounding {
                Some(rounding) => rounding.into(),
                None => ui.style().visuals.widgets.noninteractive.rounding,
            })
            .fill(
                Color32::from_u32(BACKGROUND_COLOR.load(Ordering::SeqCst))
                    .try_apply_alpha(config.transparency_alpha),
            )
            .shadow(match config.style {
                Some(style) => match style {
                    // new styles can be added if needed
                    GroupingStyle::Default => Shadow::NONE,
                    GroupingStyle::DefaultWithShadow => Shadow {
                        blur: 4.0,
                        offset: Vec2::new(1.0, 1.0),
                        spread: 3.0,
                        color: Color32::BLACK.try_apply_alpha(config.transparency_alpha),
                    },
                },
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
}

#[derive(Copy, Clone, Debug, Serialize, Deserialize, JsonSchema)]
pub struct GroupingConfig {
    pub style: Option<GroupingStyle>,
    pub transparency_alpha: Option<u8>,
    pub rounding: Option<RoundingConfig>,
}

#[derive(Copy, Clone, Debug, Serialize, Deserialize, JsonSchema)]
pub enum GroupingStyle {
    Default,
    DefaultWithShadow,
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
