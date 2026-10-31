//! `komorebic`, the command line interface of komorebi, for Windows, KDE Plasma and macOS.
//!
//! The platform is the target OS: each of the three modules is the CLI of one platform, with the
//! subcommands that platform has, and `main` runs the one that was compiled in. The code they share
//! is `komorebic-core`. The Linux and macOS modules are still in their own trees and are included
//! from there.

cfg_select! {
    target_os = "windows" => {
        mod windows;
        use windows as platform;
    }
    target_os = "linux" => {
        extern crate komorebi_client_linux as komorebi_client;
        extern crate komorebi_linux as komorebi;
        extern crate version_linux as version;
        mod linux;
        use linux as platform;
    }
    target_os = "macos" => {
        extern crate komorebi_client_macos as komorebi_client;
        extern crate version_macos as version;
        mod macos;
        use macos as platform;
    }
}

fn main() -> color_eyre::eyre::Result<()> {
    platform::main()
}
