use clap::ArgEnum;
use serde::Deserialize;
use serde::Serialize;
use strum::Display;
use strum::EnumString;

#[derive(Clone, Copy, Debug, Serialize, Deserialize, Display, EnumString, ArgEnum)]
#[strum(serialize_all = "snake_case")]
pub enum CycleDirection {
    Previous,
    Next,
}

impl CycleDirection {
    pub fn next_idx(&self, idx: usize, len: usize) -> usize {
        match self {
            CycleDirection::Previous => {
                if idx == 0 {
                    len - 1
                } else {
                    idx - 1
                }
            }
            CycleDirection::Next => {
                if idx == len - 1 {
                    0
                } else {
                    idx + 1
                }
            }
        }
    }
}
