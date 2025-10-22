use crate::Base16ColourPalette;
use crate::colour::Colour;
use crate::colour::Hex;
use hex_color::HexColor;
use std::collections::VecDeque;
use std::fmt::Display;
use std::fmt::Formatter;
use std::path::Path;

use serde::Deserialize;
use serde::Serialize;

#[derive(Debug, Default, Copy, Clone, Serialize, Deserialize, PartialEq)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
pub enum ThemeVariant {
    #[default]
    Dark,
    Light,
}

impl Display for ThemeVariant {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            ThemeVariant::Dark => write!(f, "dark"),
            ThemeVariant::Light => write!(f, "light"),
        }
    }
}

impl From<ThemeVariant> for flavours::operations::generate::Mode {
    fn from(value: ThemeVariant) -> Self {
        match value {
            ThemeVariant::Dark => Self::Dark,
            ThemeVariant::Light => Self::Light,
        }
    }
}

pub fn generate_base16_palette(
    image_path: &Path,
    variant: ThemeVariant,
) -> Result<Base16ColourPalette, hex_color::ParseHexColorError> {
    Base16ColourPalette::try_from(
        &flavours::operations::generate::generate(image_path, variant.into(), false)
            .unwrap_or_default(),
    )
}

impl TryFrom<&VecDeque<String>> for Base16ColourPalette {
    type Error = hex_color::ParseHexColorError;

    fn try_from(value: &VecDeque<String>) -> Result<Self, Self::Error> {
        let fixed = value.iter().map(|s| format!("#{s}")).collect::<Vec<_>>();
        if fixed.len() != 16 {
            return Err(hex_color::ParseHexColorError::Empty);
        }

        Ok(Self {
            base_00: Colour::Hex(Hex(HexColor::parse(&fixed[0])?)),
            base_01: Colour::Hex(Hex(HexColor::parse(&fixed[1])?)),
            base_02: Colour::Hex(Hex(HexColor::parse(&fixed[2])?)),
            base_03: Colour::Hex(Hex(HexColor::parse(&fixed[3])?)),
            base_04: Colour::Hex(Hex(HexColor::parse(&fixed[4])?)),
            base_05: Colour::Hex(Hex(HexColor::parse(&fixed[5])?)),
            base_06: Colour::Hex(Hex(HexColor::parse(&fixed[6])?)),
            base_07: Colour::Hex(Hex(HexColor::parse(&fixed[7])?)),
            base_08: Colour::Hex(Hex(HexColor::parse(&fixed[8])?)),
            base_09: Colour::Hex(Hex(HexColor::parse(&fixed[9])?)),
            base_0a: Colour::Hex(Hex(HexColor::parse(&fixed[10])?)),
            base_0b: Colour::Hex(Hex(HexColor::parse(&fixed[11])?)),
            base_0c: Colour::Hex(Hex(HexColor::parse(&fixed[12])?)),
            base_0d: Colour::Hex(Hex(HexColor::parse(&fixed[13])?)),
            base_0e: Colour::Hex(Hex(HexColor::parse(&fixed[14])?)),
            base_0f: Colour::Hex(Hex(HexColor::parse(&fixed[15])?)),
        })
    }
}
