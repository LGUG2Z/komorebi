//! The komorebi daemon's library: the vocabulary the window manager is written in and the data
//! structures it is built from.

#[macro_use]
pub mod ring;

pub mod backend;
pub mod core;
pub mod lockable_sequence;
