//! The guest half of the e2e suite.
//!
//! It runs inside the interactive session of the guest, and the host talks to it over TCP: the
//! protocol is the same on every platform (`protocol.rs`), and what a platform adds to it is the
//! windows and the displays of its own desktop (`windows.rs`, `macos.rs`).

// no console window on Windows: it would be a window for the daemon to tile
#![cfg_attr(windows, windows_subsystem = "windows")]

#[cfg(target_os = "macos")]
mod macos;
mod protocol;
#[cfg(windows)]
mod windows;

#[cfg(windows)]
fn main() -> std::io::Result<()> {
    windows::run()
}

#[cfg(target_os = "macos")]
fn main() -> std::io::Result<()> {
    macos::run()
}

#[cfg(not(any(windows, target_os = "macos")))]
fn main() {
    eprintln!("komorebi-e2e-agent runs on Windows and macOS only");
    std::process::exit(1);
}
