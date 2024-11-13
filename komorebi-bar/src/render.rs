use crate::bar::Alignment;
use crate::config::KomobarConfig;
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

#[derive(Copy, Clone)]
pub struct RenderConfig {
    /// Spacing between widgets
    pub spacing: f32,
    /// Sets how widgets are grouped
    pub grouping: Grouping,
    /// Background color
    pub background_color: Color32,
    /// Alignment of the widgets
    pub alignment: Option<Alignment>,
    /// Remove spacing if true
    pub no_spacing: Option<bool>,
}

pub trait RenderExt {
    fn new_renderconfig(&self, background_color: Color32) -> RenderConfig;
}

impl RenderExt for &KomobarConfig {
    fn new_renderconfig(&self, background_color: Color32) -> RenderConfig {
        RenderConfig {
            spacing: self.widget_spacing.unwrap_or(10.0),
            grouping: self.grouping.unwrap_or(Grouping::None),
            background_color,
            alignment: None,
            no_spacing: None,
        }
    }
}

impl RenderConfig {
    pub fn apply_on_bar<R>(
        &mut self,
        ui: &mut Ui,
        add_contents: impl FnOnce(&mut Ui) -> R,
    ) -> InnerResponse<R> {
        self.alignment = None;

        if let Grouping::Bar(config) = self.grouping {
            return self.define_group(false, config, ui, add_contents);
        }

        Self::fallback_group(ui, add_contents)
    }

    pub fn apply_on_alignment<R>(
        &mut self,
        ui: &mut Ui,
        add_contents: impl FnOnce(&mut Ui) -> R,
    ) -> InnerResponse<R> {
        self.alignment = None;

        if let Grouping::Alignment(config) = self.grouping {
            return self.define_group(false, config, ui, add_contents);
        }

        Self::fallback_group(ui, add_contents)
    }

    pub fn apply_on_widget<R>(
        &mut self,
        use_spacing: bool,
        // TODO: this should remove the margin on the last widget on the left side and the first widget on the right side
        // This is complex, since the last/first widget can have multiple "sections", like komorebi, network, ...
        // This and the same setting on RenderConfig needs to be combined.
        //_first_or_last: Option<bool>,
        ui: &mut Ui,
        add_contents: impl FnOnce(&mut Ui) -> R,
    ) -> InnerResponse<R> {
        if let Grouping::Widget(config) = self.grouping {
            return self.define_group(use_spacing, config, ui, add_contents);
        }

        self.fallback_widget_group(use_spacing, ui, add_contents)
    }

    fn fallback_group<R>(ui: &mut Ui, add_contents: impl FnOnce(&mut Ui) -> R) -> InnerResponse<R> {
        InnerResponse {
            inner: add_contents(ui),
            response: ui.response().clone(),
        }
    }

    fn fallback_widget_group<R>(
        &mut self,
        use_spacing: bool,
        ui: &mut Ui,
        add_contents: impl FnOnce(&mut Ui) -> R,
    ) -> InnerResponse<R> {
        Frame::none()
            .outer_margin(self.widget_outer_margin())
            .inner_margin(match use_spacing {
                true => Margin::symmetric(5.0, 0.0),
                false => Margin::same(0.0),
            })
            .show(ui, add_contents)
    }

    fn define_group<R>(
        &mut self,
        use_spacing: bool,
        config: GroupingConfig,
        ui: &mut Ui,
        add_contents: impl FnOnce(&mut Ui) -> R,
    ) -> InnerResponse<R> {
        Frame::none()
            .outer_margin(self.widget_outer_margin())
            .inner_margin(match use_spacing {
                true => Margin::symmetric(8.0, 3.0),
                false => Margin::symmetric(3.0, 3.0),
            })
            .stroke(ui.style().visuals.widgets.noninteractive.bg_stroke)
            .rounding(match config.rounding {
                Some(rounding) => rounding.into(),
                None => ui.style().visuals.widgets.noninteractive.rounding,
            })
            .fill(
                self.background_color
                    .try_apply_alpha(config.transparency_alpha),
            )
            .shadow(match config.style {
                Some(style) => match style {
                    // new styles can be added if needed here
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

    fn widget_outer_margin(&self) -> Margin {
        Margin {
            left: match self.alignment {
                Some(align) => match align {
                    Alignment::Left => 0.0,
                    Alignment::Right => {
                        if self.no_spacing.is_some_and(|v| v) {
                            0.0
                        } else {
                            self.spacing
                        }
                    }
                },
                None => 0.0,
            },
            right: match self.alignment {
                Some(align) => match align {
                    Alignment::Left => {
                        if self.no_spacing.is_some_and(|v| v) {
                            0.0
                        } else {
                            self.spacing
                        }
                    }
                    Alignment::Right => 0.0,
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

pub trait Color32Ext {
    fn try_apply_alpha(self, transparency_alpha: Option<u8>) -> Self;
}

impl Color32Ext for Color32 {
    /// Tries to apply the alpha value to the Color32
    fn try_apply_alpha(self, transparency_alpha: Option<u8>) -> Self {
        if let Some(alpha) = transparency_alpha {
            return Color32::from_rgba_unmultiplied(self.r(), self.g(), self.b(), alpha);
        }

        self
    }
}
