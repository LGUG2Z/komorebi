//! The `komorebi.json` data types and the generation of their JSON Schema.
//!
//! The crate holds no window manager code: it knows what a configuration file may say, not
//! how a daemon applies it. The types are moved here from the three daemons one family at a
//! time; the schema of each tree is checked in (`schema.json`, `schema.asc.json`) and
//! `tests/schemas.rs` compares what this crate generates with those files.
#![warn(clippy::all)]
#![allow(clippy::missing_errors_doc, clippy::use_self, clippy::doc_markdown)]

#[cfg(feature = "schemars")]
pub mod schema;

pub mod animation;
pub mod application_configuration;
pub mod asc;
pub mod aspect_ratio;
pub mod behaviour;
pub mod changes;
pub mod defaults;
pub mod matching;
pub mod placement;
#[cfg(feature = "schemars")]
pub mod platforms;
pub mod static_config;
