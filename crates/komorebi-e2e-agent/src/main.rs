//! The guest half of the e2e suite.
//!
//! It runs inside the interactive session of the guest, and the host talks to it over TCP: the
//! protocol is the same on every platform (`protocol.rs`), and what a platform adds to it is the
//! windows and the displays of its own desktop (`windows.rs`, `macos.rs`).

// no console window on Windows: it would be a window for the daemon to tile
#![cfg_attr(windows, windows_subsystem = "windows")]

mod protocol;
#[cfg(windows)]
mod windows;

#[cfg(windows)]
fn main() -> std::io::Result<()> {
    windows::run()
}

#[cfg(not(windows))]
fn main() {
    eprintln!("komorebi-e2e-agent runs on Windows only");
    std::process::exit(1);
}
