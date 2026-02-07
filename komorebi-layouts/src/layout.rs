use serde::Deserialize;
use serde::Serialize;

use super::Arrangement;
#[cfg(feature = "win32")]
use super::CustomLayout;
use super::DefaultLayout;
use super::Direction;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
pub enum Layout {
    Default(DefaultLayout),
    #[cfg(feature = "win32")]
    Custom(CustomLayout),
}

impl Layout {
    #[must_use]
    pub fn as_boxed_direction(&self) -> Box<dyn Direction> {
        match self {
            Layout::Default(layout) => Box::new(*layout),
            #[cfg(feature = "win32")]
            Layout::Custom(layout) => Box::new(layout.clone()),
        }
    }

    #[must_use]
    pub fn as_boxed_arrangement(&self) -> Box<dyn Arrangement> {
        match self {
            Layout::Default(layout) => Box::new(*layout),
            #[cfg(feature = "win32")]
            Layout::Custom(layout) => Box::new(layout.clone()),
        }
    }
}
