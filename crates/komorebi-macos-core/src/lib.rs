//! The parts of `komorebi-macos` that need nothing from macOS.
//!
//! The animation maths, the record of what windows turned out to be, and the rules for the
//! accessibility attribute that makes apps animate on their own live here so that their tests run on
//! every platform (a Linux CI runner included), not only on a Mac: `komorebi-macos` pulls in the
//! Objective-C runtime crates, which refuse to compile anywhere else.
//!
//! The rule of the crate is that it has no dependency on `objc2`, `block2`, `dispatch2` or any
//! binding of an Apple framework, so a module that would need one does not belong in it.
//! `komorebi-macos` re-exports each module at the path it had before, so nothing that uses them
//! changed.

pub mod animation;
pub mod arrival;
pub mod enhanced_ui;
