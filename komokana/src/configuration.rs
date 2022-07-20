#![allow(clippy::use_self)]

use serde::Deserialize;
use serde::Serialize;

pub type Configuration = Vec<Entry>;

#[derive(Default, Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Entry {
    pub exe: String,
    pub target_layer: String,
    pub title_overrides: Option<Vec<TitleOverride>>,
    pub virtual_key_overrides: Option<Vec<VirtualKeyOverride>>,
    pub virtual_key_ignores: Option<Vec<i32>>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TitleOverride {
    pub title: String,
    pub strategy: Strategy,
    pub target_layer: String,
}

#[derive(Default, Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct VirtualKeyOverride {
    pub virtual_key_code: i32,
    pub targer_layer: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Strategy {
    StartsWith,
    EndsWith,
    Contains,
    Equals,
}
