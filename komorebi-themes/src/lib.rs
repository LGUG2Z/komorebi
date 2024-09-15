#![warn(clippy::all)]
#![allow(clippy::missing_errors_doc)]

use schemars::JsonSchema;
use serde::Deserialize;
use serde::Serialize;

pub use base16_egui_themes::Base16;
pub use catppuccin_egui;
pub use egui::Color32;

#[derive(Copy, Clone, Debug, Serialize, Deserialize, JsonSchema)]
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
}

#[derive(Copy, Clone, Debug, Default, Serialize, Deserialize, JsonSchema)]
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

impl Base16Value {
    pub fn color32(&self, theme: Base16) -> Color32 {
        match self {
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
        }
    }
}

#[derive(Copy, Clone, Debug, Serialize, Deserialize, JsonSchema)]
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

#[derive(Copy, Clone, Debug, Default, Serialize, Deserialize, JsonSchema)]
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

impl CatppuccinValue {
    pub fn color32(&self, theme: catppuccin_egui::Theme) -> Color32 {
        match self {
            CatppuccinValue::Rosewater => theme.rosewater,
            CatppuccinValue::Flamingo => theme.flamingo,
            CatppuccinValue::Pink => theme.pink,
            CatppuccinValue::Mauve => theme.mauve,
            CatppuccinValue::Red => theme.red,
            CatppuccinValue::Maroon => theme.maroon,
            CatppuccinValue::Peach => theme.peach,
            CatppuccinValue::Yellow => theme.yellow,
            CatppuccinValue::Green => theme.green,
            CatppuccinValue::Teal => theme.teal,
            CatppuccinValue::Sky => theme.sky,
            CatppuccinValue::Sapphire => theme.sapphire,
            CatppuccinValue::Blue => theme.blue,
            CatppuccinValue::Lavender => theme.lavender,
            CatppuccinValue::Text => theme.text,
            CatppuccinValue::Subtext1 => theme.subtext1,
            CatppuccinValue::Subtext0 => theme.subtext0,
            CatppuccinValue::Overlay2 => theme.overlay2,
            CatppuccinValue::Overlay1 => theme.overlay1,
            CatppuccinValue::Overlay0 => theme.overlay0,
            CatppuccinValue::Surface2 => theme.surface2,
            CatppuccinValue::Surface1 => theme.surface1,
            CatppuccinValue::Surface0 => theme.surface0,
            CatppuccinValue::Base => theme.base,
            CatppuccinValue::Mantle => theme.mantle,
            CatppuccinValue::Crust => theme.crust,
        }
    }
}
