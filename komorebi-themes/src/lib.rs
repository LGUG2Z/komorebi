#![warn(clippy::all)]
#![allow(clippy::missing_errors_doc)]

pub mod colour;
mod generator;

pub use generator::ThemeVariant;
pub use generator::generate_base16_palette;

use schemars::JsonSchema;
use serde::Deserialize;
use serde::Serialize;
use strum::Display;
use strum::IntoEnumIterator;

use crate::colour::Colour;
pub use base16_egui_themes::Base16;
pub use catppuccin_egui;
pub use eframe::egui::Color32;
use eframe::egui::Shadow;
use eframe::egui::Stroke;
use eframe::egui::Style;
use eframe::egui::Visuals;
use eframe::egui::style::Selection;
use eframe::egui::style::WidgetVisuals;
use eframe::egui::style::Widgets;
use serde_variant::to_variant_name;

#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema, PartialEq)]
#[serde(tag = "type")]
pub enum Theme {
    /// A theme from catppuccin-egui
    Catppuccin {
        name: Catppuccin,
        accent: Option<CatppuccinValue>,
    },
    /// A theme from base16-egui-themes
    Base16 {
        name: Base16,
        accent: Option<Base16Value>,
    },
    /// A custom base16 palette
    Custom {
        palette: Box<Base16ColourPalette>,
        accent: Option<Base16Value>,
    },
}

#[derive(Copy, Clone, Debug, Serialize, Deserialize, JsonSchema, PartialEq)]
pub struct Base16ColourPalette {
    pub base_00: Colour,
    pub base_01: Colour,
    pub base_02: Colour,
    pub base_03: Colour,
    pub base_04: Colour,
    pub base_05: Colour,
    pub base_06: Colour,
    pub base_07: Colour,
    pub base_08: Colour,
    pub base_09: Colour,
    pub base_0a: Colour,
    pub base_0b: Colour,
    pub base_0c: Colour,
    pub base_0d: Colour,
    pub base_0e: Colour,
    pub base_0f: Colour,
}

impl Base16ColourPalette {
    pub fn background(self) -> Color32 {
        self.base_01.into()
    }
    pub fn style(self) -> Style {
        let original = Style::default();
        Style {
            visuals: Visuals {
                widgets: Widgets {
                    noninteractive: WidgetVisuals {
                        bg_fill: self.base_01.into(),
                        weak_bg_fill: self.base_01.into(),
                        bg_stroke: Stroke {
                            color: self.base_02.into(),
                            ..original.visuals.widgets.noninteractive.bg_stroke
                        },
                        fg_stroke: Stroke {
                            color: self.base_05.into(),
                            ..original.visuals.widgets.noninteractive.fg_stroke
                        },
                        ..original.visuals.widgets.noninteractive
                    },
                    inactive: WidgetVisuals {
                        bg_fill: self.base_02.into(),
                        weak_bg_fill: self.base_02.into(),
                        bg_stroke: Stroke {
                            color: Color32::from_rgba_premultiplied(0, 0, 0, 0),
                            ..original.visuals.widgets.inactive.bg_stroke
                        },
                        fg_stroke: Stroke {
                            color: self.base_05.into(),
                            ..original.visuals.widgets.inactive.fg_stroke
                        },
                        ..original.visuals.widgets.inactive
                    },
                    hovered: WidgetVisuals {
                        bg_fill: self.base_02.into(),
                        weak_bg_fill: self.base_02.into(),
                        bg_stroke: Stroke {
                            color: self.base_03.into(),
                            ..original.visuals.widgets.hovered.bg_stroke
                        },
                        fg_stroke: Stroke {
                            color: self.base_06.into(),
                            ..original.visuals.widgets.hovered.fg_stroke
                        },
                        ..original.visuals.widgets.hovered
                    },
                    active: WidgetVisuals {
                        bg_fill: self.base_02.into(),
                        weak_bg_fill: self.base_02.into(),
                        bg_stroke: Stroke {
                            color: self.base_03.into(),
                            ..original.visuals.widgets.hovered.bg_stroke
                        },
                        fg_stroke: Stroke {
                            color: self.base_06.into(),
                            ..original.visuals.widgets.hovered.fg_stroke
                        },
                        ..original.visuals.widgets.active
                    },
                    open: WidgetVisuals {
                        bg_fill: self.base_01.into(),
                        weak_bg_fill: self.base_01.into(),
                        bg_stroke: Stroke {
                            color: self.base_02.into(),
                            ..original.visuals.widgets.open.bg_stroke
                        },
                        fg_stroke: Stroke {
                            color: self.base_06.into(),
                            ..original.visuals.widgets.open.fg_stroke
                        },
                        ..original.visuals.widgets.open
                    },
                },
                selection: Selection {
                    bg_fill: self.base_02.into(),
                    stroke: Stroke {
                        color: self.base_06.into(),
                        ..original.visuals.selection.stroke
                    },
                },
                hyperlink_color: self.base_08.into(),
                faint_bg_color: Color32::from_rgba_premultiplied(0, 0, 0, 0),
                extreme_bg_color: self.base_00.into(),
                code_bg_color: self.base_02.into(),
                warn_fg_color: self.base_0c.into(),
                error_fg_color: self.base_0b.into(),
                window_shadow: Shadow {
                    color: Color32::from_rgba_premultiplied(0, 0, 0, 96),
                    ..original.visuals.window_shadow
                },
                window_fill: self.base_01.into(),
                window_stroke: Stroke {
                    color: self.base_02.into(),
                    ..original.visuals.window_stroke
                },
                panel_fill: self.base_01.into(),
                popup_shadow: Shadow {
                    color: Color32::from_rgba_premultiplied(0, 0, 0, 96),
                    ..original.visuals.popup_shadow
                },
                ..original.visuals
            },
            ..original
        }
    }
}

impl Theme {
    pub fn variant_names(&self) -> Vec<String> {
        match self {
            Theme::Catppuccin { .. } => {
                vec![
                    "Frappe".to_string(),
                    "Latte".to_string(),
                    "Macchiato".to_string(),
                    "Mocha".to_string(),
                ]
            }
            Theme::Base16 { .. } => Base16::iter()
                .map(|variant| {
                    to_variant_name(&variant)
                        .expect("could not convert to variant name")
                        .to_string()
                })
                .collect(),
            Theme::Custom { .. } => vec!["Custom".to_string()],
        }
    }
}

#[derive(Copy, Clone, Debug, Default, Serialize, Deserialize, JsonSchema, Display, PartialEq)]
pub enum Base16Value {
    Base00,
    Base01,
    Base02,
    Base03,
    Base04,
    Base05,
    #[default]
    Base06,
    Base07,
    Base08,
    Base09,
    Base0A,
    Base0B,
    Base0C,
    Base0D,
    Base0E,
    Base0F,
}

pub enum Base16Wrapper {
    Base16(Base16),
    Custom(Box<Base16ColourPalette>),
}

impl Base16Value {
    pub fn color32(&self, theme: Base16Wrapper) -> Color32 {
        match theme {
            Base16Wrapper::Base16(theme) => match self {
                Base16Value::Base00 => theme.base00(),
                Base16Value::Base01 => theme.base01(),
                Base16Value::Base02 => theme.base02(),
                Base16Value::Base03 => theme.base03(),
                Base16Value::Base04 => theme.base04(),
                Base16Value::Base05 => theme.base05(),
                Base16Value::Base06 => theme.base06(),
                Base16Value::Base07 => theme.base07(),
                Base16Value::Base08 => theme.base08(),
                Base16Value::Base09 => theme.base09(),
                Base16Value::Base0A => theme.base0a(),
                Base16Value::Base0B => theme.base0b(),
                Base16Value::Base0C => theme.base0c(),
                Base16Value::Base0D => theme.base0d(),
                Base16Value::Base0E => theme.base0e(),
                Base16Value::Base0F => theme.base0f(),
            },
            Base16Wrapper::Custom(colours) => match self {
                Base16Value::Base00 => colours.base_00.into(),
                Base16Value::Base01 => colours.base_01.into(),
                Base16Value::Base02 => colours.base_02.into(),
                Base16Value::Base03 => colours.base_03.into(),
                Base16Value::Base04 => colours.base_04.into(),
                Base16Value::Base05 => colours.base_05.into(),
                Base16Value::Base06 => colours.base_06.into(),
                Base16Value::Base07 => colours.base_07.into(),
                Base16Value::Base08 => colours.base_08.into(),
                Base16Value::Base09 => colours.base_09.into(),
                Base16Value::Base0A => colours.base_0a.into(),
                Base16Value::Base0B => colours.base_0b.into(),
                Base16Value::Base0C => colours.base_0c.into(),
                Base16Value::Base0D => colours.base_0d.into(),
                Base16Value::Base0E => colours.base_0e.into(),
                Base16Value::Base0F => colours.base_0f.into(),
            },
        }
    }
}

#[derive(Copy, Clone, Debug, Serialize, Deserialize, JsonSchema, Display, PartialEq)]
pub enum Catppuccin {
    Frappe,
    Latte,
    Macchiato,
    Mocha,
}

impl Catppuccin {
    pub fn as_theme(self) -> catppuccin_egui::Theme {
        self.into()
    }
}

impl From<Catppuccin> for catppuccin_egui::Theme {
    fn from(val: Catppuccin) -> Self {
        match val {
            Catppuccin::Frappe => catppuccin_egui::FRAPPE,
            Catppuccin::Latte => catppuccin_egui::LATTE,
            Catppuccin::Macchiato => catppuccin_egui::MACCHIATO,
            Catppuccin::Mocha => catppuccin_egui::MOCHA,
        }
    }
}

#[derive(Copy, Clone, Debug, Default, Serialize, Deserialize, JsonSchema, Display, PartialEq)]
pub enum CatppuccinValue {
    Rosewater,
    Flamingo,
    Pink,
    Mauve,
    Red,
    Maroon,
    Peach,
    Yellow,
    Green,
    Teal,
    Sky,
    Sapphire,
    Blue,
    Lavender,
    #[default]
    Text,
    Subtext1,
    Subtext0,
    Overlay2,
    Overlay1,
    Overlay0,
    Surface2,
    Surface1,
    Surface0,
    Base,
    Mantle,
    Crust,
}

pub fn color32_compat(rgba: [u8; 4]) -> Color32 {
    Color32::from_rgba_unmultiplied(rgba[0], rgba[1], rgba[2], rgba[3])
}

impl CatppuccinValue {
    pub fn color32(&self, theme: catppuccin_egui::Theme) -> Color32 {
        match self {
            CatppuccinValue::Rosewater => color32_compat(theme.rosewater.to_srgba_unmultiplied()),
            CatppuccinValue::Flamingo => color32_compat(theme.flamingo.to_srgba_unmultiplied()),
            CatppuccinValue::Pink => color32_compat(theme.pink.to_srgba_unmultiplied()),
            CatppuccinValue::Mauve => color32_compat(theme.mauve.to_srgba_unmultiplied()),
            CatppuccinValue::Red => color32_compat(theme.red.to_srgba_unmultiplied()),
            CatppuccinValue::Maroon => color32_compat(theme.maroon.to_srgba_unmultiplied()),
            CatppuccinValue::Peach => color32_compat(theme.peach.to_srgba_unmultiplied()),
            CatppuccinValue::Yellow => color32_compat(theme.yellow.to_srgba_unmultiplied()),
            CatppuccinValue::Green => color32_compat(theme.green.to_srgba_unmultiplied()),
            CatppuccinValue::Teal => color32_compat(theme.teal.to_srgba_unmultiplied()),
            CatppuccinValue::Sky => color32_compat(theme.sky.to_srgba_unmultiplied()),
            CatppuccinValue::Sapphire => color32_compat(theme.sapphire.to_srgba_unmultiplied()),
            CatppuccinValue::Blue => color32_compat(theme.blue.to_srgba_unmultiplied()),
            CatppuccinValue::Lavender => color32_compat(theme.lavender.to_srgba_unmultiplied()),
            CatppuccinValue::Text => color32_compat(theme.text.to_srgba_unmultiplied()),
            CatppuccinValue::Subtext1 => color32_compat(theme.subtext1.to_srgba_unmultiplied()),
            CatppuccinValue::Subtext0 => color32_compat(theme.subtext0.to_srgba_unmultiplied()),
            CatppuccinValue::Overlay2 => color32_compat(theme.overlay2.to_srgba_unmultiplied()),
            CatppuccinValue::Overlay1 => color32_compat(theme.overlay1.to_srgba_unmultiplied()),
            CatppuccinValue::Overlay0 => color32_compat(theme.overlay0.to_srgba_unmultiplied()),
            CatppuccinValue::Surface2 => color32_compat(theme.surface2.to_srgba_unmultiplied()),
            CatppuccinValue::Surface1 => color32_compat(theme.surface1.to_srgba_unmultiplied()),
            CatppuccinValue::Surface0 => color32_compat(theme.surface0.to_srgba_unmultiplied()),
            CatppuccinValue::Base => color32_compat(theme.base.to_srgba_unmultiplied()),
            CatppuccinValue::Mantle => color32_compat(theme.mantle.to_srgba_unmultiplied()),
            CatppuccinValue::Crust => color32_compat(theme.crust.to_srgba_unmultiplied()),
        }
    }
}
