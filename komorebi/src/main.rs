//! `komorebi`, the tiling window manager daemon, for Windows, KDE Plasma and macOS: the daemon of the target
//! OS is a library of its own and this binary runs it.

fn main() -> color_eyre::eyre::Result<()> {
    cfg_select! {
        target_os = "windows" => komorebi_windows::daemon::run(),
        target_os = "linux" => komorebi_linux::daemon::run(),
        target_os = "macos" => komorebi_macos::daemon::run(),
    }
}
