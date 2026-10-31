//! `komorebi-client-macos`: the client of the unified `komorebi-client` package, kept under its old
//! package name until the cutover. The code is in `komorebi-client/src/macos.rs`.

extern crate komorebi as komorebi_macos;

#[path = "../../../komorebi-client/src/macos.rs"]
mod platform;

pub use platform::*;
