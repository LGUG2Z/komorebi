use clap::ValueEnum;
use schemars::JsonSchema;
use serde::Deserialize;
use serde::Serialize;
use strum::Display;
use strum::EnumString;

#[derive(
    Copy, Clone, Debug, Serialize, Deserialize, Display, EnumString, ValueEnum, JsonSchema,
)]
pub enum AnimationPrefix {
    WindowMove,
    WindowTransparency,
}

pub fn new_animation_key(prefix: AnimationPrefix, key: String) -> String {
    format!("{}:{}", prefix, key)
}
