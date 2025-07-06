use clap::ValueEnum;
use serde::Deserialize;
use serde::Serialize;
use strum::Display;
use strum::EnumString;

use crate::ring::Ring;
use crate::ring::RingElement;
use crate::ring::RingIndex;

#[derive(Clone, Copy, Debug, Serialize, Deserialize, Display, EnumString, ValueEnum)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
pub enum CycleDirection {
    Previous,
    Next,
}

impl CycleDirection {
    #[must_use]
    pub fn next_idx<I: RingIndex + Default, RE>(&self, idx: I, ring: &Ring<RE>) -> Option<I>
    where
        RE: RingElement<Index = I>,
    {
        if ring.is_empty() {
            return None;
        }

        match self {
            Self::Previous => {
                if idx == I::default() {
                    Some(ring.last_index())
                } else {
                    Some(idx.previous())
                }
            }
            Self::Next => {
                if idx == ring.last_index() {
                    Some(I::default())
                } else {
                    Some(idx.next())
                }
            }
        }
    }
}
