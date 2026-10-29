//! Linux, as KDE Plasma: the host that records komorebi for KDE.
//!
//! The master clock is `CLOCK_MONOTONIC`; the screen is the compositor's screencast, read through
//! PipeWire and written by GStreamer; sleep and the lock are heard on D-Bus; the windows are
//! komorebi for KDE's own; and the OS trace is the journal. What each does, and what it cannot,
//! is in `docs/PLATFORM.md`.

pub mod apps;
pub mod capture;
pub mod clock;
pub mod exclude;
pub mod power;
pub mod process;
mod screencast;
pub mod session;
pub mod shell;
pub mod trace;
pub mod wayland;

/// Nothing to do first.
pub fn init() {}
