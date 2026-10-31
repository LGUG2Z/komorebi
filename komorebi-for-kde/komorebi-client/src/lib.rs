//! `komorebi-client-linux`: the client of the unified `komorebi-client` package, kept under its old
//! package name until the cutover. The code is in `komorebi-client/src/linux.rs`.

extern crate komorebi as komorebi_linux;

#[path = "../../../komorebi-client/src/linux.rs"]
mod platform;

pub use platform::*;
