use hex_color::HexColor;
use schemars::gen::SchemaGenerator;
use schemars::schema::InstanceType;
use schemars::schema::Schema;
use schemars::schema::SchemaObject;
use schemars::JsonSchema;
use serde::Deserialize;
use serde::Serialize;

#[derive(Debug, Copy, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(untagged)]
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

#[derive(Debug, Copy, Clone, Serialize, Deserialize)]
pub struct Hex(HexColor);

impl JsonSchema for Hex {
    fn schema_name() -> String {
        String::from("Hex")
    }

    fn json_schema(_: &mut SchemaGenerator) -> Schema {
        SchemaObject {
            instance_type: Some(InstanceType::String.into()),
            ..Default::default()
        }
        .into()
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

#[derive(Debug, Copy, Clone, Serialize, Deserialize, JsonSchema)]
pub struct Rgb {
    /// Red
    pub r: u32,
    /// Green
    pub g: u32,
    /// Blue
    pub b: u32,
}

impl Rgb {
    pub fn new(r: u32, g: u32, b: u32) -> Self {
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
            g: value >> 8 & 0xff,
            b: value >> 16 & 0xff,
        }
    }
}
