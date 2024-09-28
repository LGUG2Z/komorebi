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
}

pub fn new_animation_key(prefix: AnimationPrefix, key: String) -> String {
    match prefix {
        AnimationPrefix::WindowMove => format!("window_move:{}", key),
    }
}
