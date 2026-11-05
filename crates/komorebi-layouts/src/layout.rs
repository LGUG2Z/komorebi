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

    /// Returns the container index of the primary (largest) pane for this layout.
    #[must_use]
    pub fn primary_index(&self) -> usize {
        match self {
            Layout::Default(layout) => layout.primary_index(),
            #[cfg(feature = "win32")]
            Layout::Custom(layout) => layout.primary_container_index().unwrap_or(0),
        }
    }

    /// Returns the container index of the secondary pane for this layout,
    /// if there are enough containers.
    #[must_use]
    pub fn secondary_index(&self, container_count: usize) -> Option<usize> {
        match self {
            Layout::Default(layout) => layout.secondary_index(container_count),
            #[cfg(feature = "win32")]
            Layout::Custom(layout) => layout.secondary_container_index(container_count),
        }
    }
}
