//! Linux, as KDE Plasma: the host that records komorebi for KDE.
//!
//! The master clock, the child processes, the shell, the screen capture, power, the session lock
//! and the window list are Linux's own. The OS trace is still the `unsupported` host's, and is
//! replaced when it is written (see `docs/PLATFORM.md`).

// the pieces of the fallback that Linux has its own version of are not used
#[allow(dead_code)]
#[path = "../unsupported.rs"]
mod fallback;

pub mod apps;
pub mod capture;
pub mod clock;
pub mod power;
pub mod process;
mod screencast;
pub mod session;
pub mod shell;
pub mod wayland;

pub use fallback::trace;

/// Nothing to do first.
pub fn init() {}
