//! The code the Windows and the macOS `komorebi-bar` share.
//!
//! The crate is being filled in by pure moves, one file at a time, each held to the schema
//! oracle in `tests/schema_oracle.rs`: the checked-in `schema.bar.json` of both bars must not
//! change unless a reviewed revision says so.
#![warn(clippy::all)]

pub mod selected_frame;
pub mod ui;

pub mod alignment;
pub mod statics;

pub mod config;

pub mod render;

pub mod widgets;

pub mod platform;

pub mod schema;
