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
use std::sync::atomic::AtomicUsize;
use std::sync::atomic::Ordering;

static SHOW_KOMOREBI_LAYOUT_OPTIONS: AtomicUsize = AtomicUsize::new(0);

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
    /// Komorebi monitor index of the monitor on which to render the bar
    pub monitor_idx: usize,
    /// Spacing between widgets
    pub spacing: f32,
    /// Sets how widgets are grouped
    pub grouping: Grouping,
    /// Background color
    pub background_color: Color32,
    /// Alignment of the widgets
    pub alignment: Option<Alignment>,
    /// Add more inner margin when adding a widget group
    pub more_inner_margin: bool,
    /// Set to true after the first time the apply_on_widget was called on an alignment
    pub applied_on_widget: bool,
}

pub trait RenderExt {
    fn new_renderconfig(&self, background_color: Color32) -> RenderConfig;
}

impl RenderExt for &KomobarConfig {
    fn new_renderconfig(&self, background_color: Color32) -> RenderConfig {
        RenderConfig {
            monitor_idx: self.monitor.index,
            spacing: self.widget_spacing.unwrap_or(10.0),
            grouping: self.grouping.unwrap_or(Grouping::None),
            background_color,
            alignment: None,
            more_inner_margin: false,
            applied_on_widget: false,
        }
    }
}

impl RenderConfig {
    pub fn load_show_komorebi_layout_options() -> bool {
        SHOW_KOMOREBI_LAYOUT_OPTIONS.load(Ordering::SeqCst) != 0
    }

    pub fn store_show_komorebi_layout_options(show: bool) {
        SHOW_KOMOREBI_LAYOUT_OPTIONS.store(show as usize, Ordering::SeqCst);
    }

    pub fn new() -> Self {
        Self {
            monitor_idx: 0,
            spacing: 0.0,
            grouping: Grouping::None,
            background_color: Color32::BLACK,
            alignment: None,
            more_inner_margin: false,
            applied_on_widget: false,
        }
    }

    pub fn apply_on_bar<R>(
        &mut self,
        ui: &mut Ui,
        add_contents: impl FnOnce(&mut Ui) -> R,
    ) -> InnerResponse<R> {
        self.alignment = None;

        if let Grouping::Bar(config) = self.grouping {
            return self.define_group(None, config, ui, add_contents);
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
            return self.define_group(None, config, ui, add_contents);
        }

        Self::fallback_group(ui, add_contents)
    }

    pub fn apply_on_widget<R>(
        &mut self,
        more_inner_margin: bool,
        ui: &mut Ui,
        add_contents: impl FnOnce(&mut Ui) -> R,
    ) -> InnerResponse<R> {
        self.more_inner_margin = more_inner_margin;
        let outer_margin = self.widget_outer_margin(ui);

        if let Grouping::Widget(config) = self.grouping {
            return self.define_group(Some(outer_margin), config, ui, add_contents);
        }

        self.fallback_widget_group(Some(outer_margin), ui, add_contents)
    }

    fn fallback_group<R>(ui: &mut Ui, add_contents: impl FnOnce(&mut Ui) -> R) -> InnerResponse<R> {
        InnerResponse {
            inner: add_contents(ui),
            response: ui.response().clone(),
        }
    }

    fn fallback_widget_group<R>(
        &mut self,
        outer_margin: Option<Margin>,
        ui: &mut Ui,
        add_contents: impl FnOnce(&mut Ui) -> R,
    ) -> InnerResponse<R> {
        Frame::none()
            .outer_margin(outer_margin.unwrap_or(Margin::ZERO))
            .inner_margin(match self.more_inner_margin {
                true => Margin::symmetric(5.0, 0.0),
                false => Margin::same(0.0),
            })
            .show(ui, add_contents)
    }

    fn define_group<R>(
        &mut self,
        outer_margin: Option<Margin>,
        config: GroupingConfig,
        ui: &mut Ui,
        add_contents: impl FnOnce(&mut Ui) -> R,
    ) -> InnerResponse<R> {
        Frame::group(ui.style_mut())
            .outer_margin(outer_margin.unwrap_or(Margin::ZERO))
            .inner_margin(match self.more_inner_margin {
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

    fn widget_outer_margin(&mut self, ui: &mut Ui) -> Margin {
        let spacing = if self.applied_on_widget {
            // Remove the default item spacing from the margin
            self.spacing - ui.spacing().item_spacing.x
        } else {
            0.0
        };

        if !self.applied_on_widget {
            self.applied_on_widget = true;
        }

        Margin {
            left: match self.alignment {
                Some(align) => match align {
                    Alignment::Left => spacing,
                    Alignment::Center => spacing,
                    Alignment::Right => 0.0,
                },
                None => 0.0,
            },
            right: match self.alignment {
                Some(align) => match align {
                    Alignment::Left => 0.0,
                    Alignment::Center => 0.0,
                    Alignment::Right => spacing,
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
    /// Styles for the grouping
    pub style: Option<GroupingStyle>,
    /// Alpha value for the color transparency [[0-255]] (default: 200)
    pub transparency_alpha: Option<u8>,
    /// Rounding values for the 4 corners. Can be a single or 4 values.
    pub rounding: Option<RoundingConfig>,
}

#[derive(Copy, Clone, Debug, Serialize, Deserialize, JsonSchema)]
pub enum GroupingStyle {
    #[serde(alias = "CtByte")]
    Default,
    /// A black shadow is added under the default group
    #[serde(alias = "CtByteWithShadow")]
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
