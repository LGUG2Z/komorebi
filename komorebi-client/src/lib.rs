#![warn(clippy::all)]
#![allow(clippy::missing_errors_doc)]

//! The client of the komorebi daemon, for Windows, KDE Plasma and macOS: one crate, with the module
//! of the target OS re-exported at its root. The functions that talk to the daemon's socket and the
//! types they send are the same in the three modules; what differs is the daemon crate they
//! re-export from and the socket type (`uds_windows` on Windows, `std::os::unix` elsewhere).

cfg_select! {
    target_os = "windows" => {
        mod windows;
        pub use windows::*;
    }
    target_os = "linux" => {
        extern crate komorebi_linux;
        mod linux;
        pub use linux::*;
    }
    target_os = "macos" => {
        extern crate komorebi_macos;
        mod macos;
        pub use macos::*;
    }
}
