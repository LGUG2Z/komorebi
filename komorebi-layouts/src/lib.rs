#![warn(clippy::all)]
#![allow(clippy::missing_errors_doc, clippy::use_self, clippy::doc_markdown)]

//! Layout system for the komorebi window manager.
//!
//! This crate provides the core layout algorithms and types for arranging windows
//! in various configurations. It includes optional Windows-specific functionality
//! behind the `win32` feature flag.

pub mod arrangement;
#[cfg(feature = "win32")]
pub mod custom_layout;
pub mod cycle_direction;
pub mod default_layout;
pub mod direction;
pub mod layout;
pub mod operation_direction;
pub mod rect;
pub mod sizing;

pub use arrangement::*;
#[cfg(feature = "win32")]
pub use custom_layout::*;
pub use cycle_direction::*;
pub use default_layout::*;
pub use direction::*;
pub use layout::*;
pub use operation_direction::*;
pub use rect::*;
pub use sizing::*;
