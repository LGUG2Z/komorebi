use schemars::JsonSchema;
use serde::Deserialize;
use serde::Serialize;

use crate::Arrangement;
use crate::CustomLayout;
use crate::DefaultLayout;
use crate::Direction;

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq)]
pub enum Layout {
    Default(DefaultLayout),
    Custom(CustomLayout),
}

impl Layout {
    #[must_use]
    pub fn as_boxed_direction(&self) -> Box<dyn Direction> {
        match self {
            Layout::Default(layout) => Box::new(*layout),
            Layout::Custom(layout) => Box::new(layout.clone()),
        }
    }

    #[must_use]
    pub fn as_boxed_arrangement(&self) -> Box<dyn Arrangement> {
        match self {
            Layout::Default(layout) => Box::new(*layout),
            Layout::Custom(layout) => Box::new(layout.clone()),
        }
    }
}
