use hex_color::HexColor;
#[cfg(feature = "schemars")]
use schemars::Schema;
#[cfg(feature = "schemars")]
use schemars::SchemaGenerator;

use crate::Color32;
use serde::Deserialize;
use serde::Serialize;

#[derive(Debug, Copy, Clone, Serialize, Deserialize, PartialEq)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[serde(untagged)]
/// Colour representation
pub enum Colour {
    /// Colour represented as RGB
    Rgb(Rgb),
    /// Colour represented as Hex
    Hex(Hex),
}

impl From<Rgb> for Colour {
    fn from(value: Rgb) -> Self {
        Self::Rgb(value)
    }
}

impl From<u32> for Colour {
    fn from(value: u32) -> Self {
        Self::Rgb(Rgb::from(value))
    }
}

impl From<Color32> for Colour {
    fn from(value: Color32) -> Self {
        Colour::Rgb(Rgb::new(
            value.r() as u32,
            value.g() as u32,
            value.b() as u32,
        ))
    }
}

impl From<Colour> for Color32 {
    fn from(value: Colour) -> Self {
        match value {
            Colour::Rgb(rgb) => Color32::from_rgb(rgb.r as u8, rgb.g as u8, rgb.b as u8),
            Colour::Hex(hex) => {
                let rgb = Rgb::from(hex);
                Color32::from_rgb(rgb.r as u8, rgb.g as u8, rgb.b as u8)
            }
        }
    }
}

/// Colour represented as a Hex string
#[derive(Debug, Copy, Clone, Serialize, Deserialize, PartialEq)]
pub struct Hex(pub HexColor);

#[cfg(feature = "schemars")]
impl schemars::JsonSchema for Hex {
    fn schema_name() -> std::borrow::Cow<'static, str> {
        std::borrow::Cow::Borrowed("Hex")
    }

    fn json_schema(_: &mut SchemaGenerator) -> Schema {
        schemars::json_schema!({
            "type": "string",
            "format": "color-hex",
            "description": "Colour represented as a Hex string"
        })
    }
}

impl From<Colour> for u32 {
    fn from(value: Colour) -> Self {
        match value {
            Colour::Rgb(val) => val.into(),
            Colour::Hex(val) => (Rgb::from(val)).into(),
        }
    }
}

#[derive(Debug, Copy, Clone, Serialize, Deserialize, PartialEq)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
/// Colour represented as RGB
pub struct Rgb {
    /// Red
    pub r: u32,
    /// Green
    pub g: u32,
    /// Blue
    pub b: u32,
}

impl Rgb {
    pub const fn new(r: u32, g: u32, b: u32) -> Self {
        Self { r, g, b }
    }
}

impl From<Hex> for Rgb {
    fn from(value: Hex) -> Self {
        value.0.into()
    }
}

impl From<HexColor> for Rgb {
    fn from(value: HexColor) -> Self {
        Self {
            r: value.r as u32,
            g: value.g as u32,
            b: value.b as u32,
        }
    }
}

impl From<Rgb> for u32 {
    fn from(value: Rgb) -> Self {
        value.r | (value.g << 8) | (value.b << 16)
    }
}

impl From<u32> for Rgb {
    fn from(value: u32) -> Self {
        Self {
            r: value & 0xff,
            g: (value >> 8) & 0xff,
            b: (value >> 16) & 0xff,
        }
    }
}
