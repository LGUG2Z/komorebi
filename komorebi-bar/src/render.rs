use crate::bar::Alignment;
use crate::config::KomobarConfig;
use crate::config::MonitorConfigOrIndex;
use crate::AUTO_SELECT_FILL_COLOUR;
use crate::AUTO_SELECT_TEXT_COLOUR;
use eframe::egui::Color32;
use eframe::egui::Context;
use eframe::egui::CornerRadius;
use eframe::egui::FontId;
use eframe::egui::Frame;
use eframe::egui::InnerResponse;
use eframe::egui::Margin;
use eframe::egui::Shadow;
use eframe::egui::TextStyle;
use eframe::egui::Ui;
use komorebi_client::Colour;
use komorebi_client::Rgb;
use serde::Deserialize;
use serde::Serialize;
use std::num::NonZeroU32;
use std::sync::atomic::AtomicUsize;
use std::sync::atomic::Ordering;
use std::sync::Arc;

static SHOW_KOMOREBI_LAYOUT_OPTIONS: AtomicUsize = AtomicUsize::new(0);

#[derive(Copy, Clone, Debug, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
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

#[derive(Clone)]
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
    /// FontId for text
    pub text_font_id: FontId,
    /// FontId for icon (based on scaling the text font id)
    pub icon_font_id: FontId,
    /// Show all icons on the workspace section of the Komorebi widget
    pub show_all_icons: bool,
    /// Background color of the selected frame
    pub auto_select_fill: Option<Color32>,
    /// Text color of the selected frame
    pub auto_select_text: Option<Color32>,
}

pub trait RenderExt {
    fn new_renderconfig(
        &self,
        ctx: &Context,
        background_color: Color32,
        icon_scale: Option<f32>,
    ) -> RenderConfig;
}

impl RenderExt for &KomobarConfig {
    fn new_renderconfig(
        &self,
        ctx: &Context,
        background_color: Color32,
        icon_scale: Option<f32>,
    ) -> RenderConfig {
        let text_font_id = ctx
            .style()
            .text_styles
            .get(&TextStyle::Body)
            .cloned()
            .unwrap_or_else(FontId::default);

        let mut icon_font_id = text_font_id.clone();
        icon_font_id.size *= icon_scale.unwrap_or(1.4).clamp(1.0, 2.0);

        let monitor_idx = match &self.monitor {
            MonitorConfigOrIndex::MonitorConfig(monitor_config) => monitor_config.index,
            MonitorConfigOrIndex::Index(idx) => *idx,
        };

        // check if any of the alignments have a komorebi widget with the workspace set to show all icons
        let show_all_icons =
            KomobarConfig::show_all_icons_on_komorebi_workspace(&self.left_widgets)
                || self
                    .center_widgets
                    .as_ref()
                    .is_some_and(|list| KomobarConfig::show_all_icons_on_komorebi_workspace(list))
                || KomobarConfig::show_all_icons_on_komorebi_workspace(&self.right_widgets);

        RenderConfig {
            monitor_idx,
            spacing: self.widget_spacing.unwrap_or(10.0),
            grouping: self.grouping.unwrap_or(Grouping::None),
            background_color,
            alignment: None,
            more_inner_margin: false,
            applied_on_widget: false,
            text_font_id,
            icon_font_id,
            show_all_icons,
            auto_select_fill: NonZeroU32::new(AUTO_SELECT_FILL_COLOUR.load(Ordering::SeqCst))
                .map(|c| Colour::Rgb(Rgb::from(c.get())).into()),
            auto_select_text: NonZeroU32::new(AUTO_SELECT_TEXT_COLOUR.load(Ordering::SeqCst))
                .map(|c| Colour::Rgb(Rgb::from(c.get())).into()),
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
            text_font_id: FontId::default(),
            icon_font_id: FontId::default(),
            show_all_icons: false,
            auto_select_fill: None,
            auto_select_text: None,
        }
    }

    pub fn change_frame_on_bar(
        &mut self,
        frame: Frame,
        ui_style: &Arc<eframe::egui::Style>,
    ) -> Frame {
        self.alignment = None;

        if let Grouping::Bar(config) = self.grouping {
            return self.define_group_frame(
                //TODO: this outer margin can be a config
                Some(Margin {
                    left: 10,
                    right: 10,
                    top: 6,
                    bottom: 6,
                }),
                config,
                ui_style,
            );
        }

        frame
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
        Frame::NONE
            .outer_margin(outer_margin.unwrap_or(Margin::ZERO))
            .inner_margin(match self.more_inner_margin {
                true => Margin::symmetric(5, 0),
                false => Margin::same(0),
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
        self.define_group_frame(outer_margin, config, ui.style())
            .show(ui, add_contents)
    }

    pub fn define_group_frame(
        &mut self,
        outer_margin: Option<Margin>,
        config: GroupingConfig,
        ui_style: &Arc<eframe::egui::Style>,
    ) -> Frame {
        Frame::group(ui_style)
            .outer_margin(outer_margin.unwrap_or(Margin::ZERO))
            .inner_margin(match self.more_inner_margin {
                true => Margin::symmetric(6, 1),
                false => Margin::symmetric(1, 1),
            })
            .stroke(ui_style.visuals.widgets.noninteractive.bg_stroke)
            .corner_radius(match config.rounding {
                Some(rounding) => rounding.into(),
                None => ui_style.visuals.widgets.noninteractive.corner_radius,
            })
            .fill(
                self.background_color
                    .try_apply_alpha(config.transparency_alpha),
            )
            .shadow(match config.style {
                Some(style) => match style {
                    // new styles can be added if needed here
                    GroupingStyle::Default => Shadow::NONE,
                    GroupingStyle::DefaultWithShadowB4O1S3 => Shadow {
                        blur: 4,
                        offset: [1, 1],
                        spread: 3,
                        color: Color32::BLACK.try_apply_alpha(config.transparency_alpha),
                    },
                    GroupingStyle::DefaultWithShadowB4O0S3 => Shadow {
                        blur: 4,
                        offset: [0, 0],
                        spread: 3,
                        color: Color32::BLACK.try_apply_alpha(config.transparency_alpha),
                    },
                    GroupingStyle::DefaultWithShadowB0O1S3 => Shadow {
                        blur: 0,
                        offset: [1, 1],
                        spread: 3,
                        color: Color32::BLACK.try_apply_alpha(config.transparency_alpha),
                    },
                    GroupingStyle::DefaultWithGlowB3O1S2 => Shadow {
                        blur: 3,
                        offset: [1, 1],
                        spread: 2,
                        color: ui_style
                            .visuals
                            .selection
                            .stroke
                            .color
                            .try_apply_alpha(config.transparency_alpha),
                    },
                    GroupingStyle::DefaultWithGlowB3O0S2 => Shadow {
                        blur: 3,
                        offset: [0, 0],
                        spread: 2,
                        color: ui_style
                            .visuals
                            .selection
                            .stroke
                            .color
                            .try_apply_alpha(config.transparency_alpha),
                    },
                    GroupingStyle::DefaultWithGlowB0O1S2 => Shadow {
                        blur: 0,
                        offset: [1, 1],
                        spread: 2,
                        color: ui_style
                            .visuals
                            .selection
                            .stroke
                            .color
                            .try_apply_alpha(config.transparency_alpha),
                    },
                },
                None => Shadow::NONE,
            })
    }

    fn widget_outer_margin(&mut self, ui: &mut Ui) -> Margin {
        let spacing = if self.applied_on_widget {
            // Remove the default item spacing from the margin
            (self.spacing - ui.spacing().item_spacing.x) as i8
        } else {
            0
        };

        if !self.applied_on_widget {
            self.applied_on_widget = true;
        }

        Margin {
            left: match self.alignment {
                Some(align) => match align {
                    Alignment::Left => spacing,
                    Alignment::Center => spacing,
                    Alignment::Right => 0,
                },
                None => 0,
            },
            right: match self.alignment {
                Some(align) => match align {
                    Alignment::Left => 0,
                    Alignment::Center => 0,
                    Alignment::Right => spacing,
                },
                None => 0,
            },
            top: 0,
            bottom: 0,
        }
    }
}

#[derive(Copy, Clone, Debug, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
pub struct GroupingConfig {
    /// Styles for the grouping
    pub style: Option<GroupingStyle>,
    /// Alpha value for the color transparency [[0-255]] (default: 200)
    pub transparency_alpha: Option<u8>,
    /// Rounding values for the 4 corners. Can be a single or 4 values.
    pub rounding: Option<RoundingConfig>,
}

#[derive(Copy, Clone, Debug, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
pub enum GroupingStyle {
    #[serde(alias = "CtByte")]
    Default,
    /// A shadow is added under the default group. (blur: 4, offset: x-1 y-1, spread: 3)
    #[serde(alias = "CtByteWithShadow")]
    #[serde(alias = "DefaultWithShadow")]
    DefaultWithShadowB4O1S3,
    /// A shadow is added under the default group. (blur: 4, offset: x-0 y-0, spread: 3)
    DefaultWithShadowB4O0S3,
    /// A shadow is added under the default group. (blur: 0, offset: x-1 y-1, spread: 3)
    DefaultWithShadowB0O1S3,
    /// A glow is added under the default group. (blur: 3, offset: x-1 y-1, spread: 2)
    DefaultWithGlowB3O1S2,
    /// A glow is added under the default group. (blur: 3, offset: x-0 y-0, spread: 2)
    DefaultWithGlowB3O0S2,
    /// A glow is added under the default group. (blur: 0, offset: x-1 y-1, spread: 2)
    DefaultWithGlowB0O1S2,
}

#[derive(Copy, Clone, Debug, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[serde(untagged)]
pub enum RoundingConfig {
    /// All 4 corners are the same    
    Same(f32),
    /// All 4 corners are custom. Order: NW, NE, SW, SE
    Individual([f32; 4]),
}

impl From<RoundingConfig> for CornerRadius {
    fn from(value: RoundingConfig) -> Self {
        match value {
            RoundingConfig::Same(value) => Self::same(value as u8),
            RoundingConfig::Individual(values) => {
                let values = values.map(|f| f as u8);
                Self {
                    nw: values[0],
                    ne: values[1],
                    sw: values[2],
                    se: values[3],
                }
            }
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
