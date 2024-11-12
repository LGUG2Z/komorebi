use crate::bar::Alignment;
use crate::config::Color32Ext;
use crate::widget::RenderConfig;
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
    /// Widgets are grouped as a whole
    Bar(GroupingConfig),
    /// Widgets are grouped by alignment
    Alignment(GroupingConfig),
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
            Self::Bar(config) => Self::define_group(false, None, ui, add_contents, config),
            Self::Alignment(_) => Self::no_group(ui, add_contents),
            Self::Widget(_) => Self::no_group(ui, add_contents),
            Self::None => Self::no_group(ui, add_contents),
        }
    }

    pub fn apply_on_alignment<R>(
        &mut self,
        ui: &mut Ui,
        add_contents: impl FnOnce(&mut Ui) -> R,
    ) -> InnerResponse<R> {
        match self {
            Self::Bar(_) => Self::no_group(ui, add_contents),
            Self::Alignment(config) => Self::define_group(false, None, ui, add_contents, config),
            Self::Widget(_) => Self::no_group(ui, add_contents),
            Self::None => Self::no_group(ui, add_contents),
        }
    }

    pub fn apply_on_widget<R>(
        &mut self,
        use_spacing: bool,
        render_config: RenderConfig,
        ui: &mut Ui,
        add_contents: impl FnOnce(&mut Ui) -> R,
    ) -> InnerResponse<R> {
        match self {
            Self::Bar(_) => Self::widget_group(use_spacing, render_config, ui, add_contents),
            Self::Alignment(_) => Self::widget_group(use_spacing, render_config, ui, add_contents),
            Self::Widget(config) => {
                Self::define_group(use_spacing, Some(render_config), ui, add_contents, config)
            }
            Self::None => Self::widget_group(use_spacing, render_config, ui, add_contents),
        }
    }

    fn define_group<R>(
        use_spacing: bool,
        render_config: Option<RenderConfig>,
        ui: &mut Ui,
        add_contents: impl FnOnce(&mut Ui) -> R,
        config: &mut GroupingConfig,
    ) -> InnerResponse<R> {
        Frame::none()
            .outer_margin(Self::widget_outer_margin(render_config))
            .inner_margin(match use_spacing {
                true => Margin::symmetric(5.0 + 3.0, 3.0),
                false => Margin::symmetric(3.0, 3.0),
            })
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
            .show(ui, add_contents)
    }

    fn widget_group<R>(
        use_spacing: bool,
        render_config: RenderConfig,
        ui: &mut Ui,
        add_contents: impl FnOnce(&mut Ui) -> R,
    ) -> InnerResponse<R> {
        Frame::none()
            .outer_margin(Self::widget_outer_margin(Some(render_config)))
            .inner_margin(match use_spacing {
                true => Margin::symmetric(5.0, 0.0),
                false => Margin::same(0.0),
            })
            .show(ui, add_contents)
    }

    fn no_group<R>(ui: &mut Ui, add_contents: impl FnOnce(&mut Ui) -> R) -> InnerResponse<R> {
        InnerResponse {
            inner: add_contents(ui),
            response: ui.response().clone(),
        }
    }

    fn widget_outer_margin(render_config: Option<RenderConfig>) -> Margin {
        Margin {
            left: match render_config {
                Some(config) => match config.alignment {
                    Some(align) => match align {
                        Alignment::Left => 0.0,
                        Alignment::Right => config.spacing,
                    },
                    None => 0.0,
                },
                None => 0.0,
            },
            right: match render_config {
                Some(config) => match config.alignment {
                    Some(align) => match align {
                        Alignment::Left => config.spacing,
                        Alignment::Right => 0.0,
                    },
                    None => 0.0,
                },
                None => 0.0,
            },
            top: 0.0,
            bottom: 0.0,
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
