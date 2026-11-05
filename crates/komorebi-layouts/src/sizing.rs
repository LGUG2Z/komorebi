use clap::ValueEnum;
use serde::Deserialize;
use serde::Serialize;
use strum::Display;
use strum::EnumString;

#[derive(Clone, Copy, Debug, Serialize, Deserialize, Display, EnumString, ValueEnum)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
/// Sizing
pub enum Sizing {
    /// Increase
    Increase,
    /// Decrease
    Decrease,
}

impl Sizing {
    #[must_use]
    pub const fn adjust_by(&self, value: i32, adjustment: i32) -> i32 {
        match self {
            Self::Increase => value + adjustment,
            Self::Decrease => {
                if value > 0 && value - adjustment >= 0 {
                    value - adjustment
                } else {
                    value
                }
            }
        }
    }
}
