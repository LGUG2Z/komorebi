//! `komorebi-bar`, the status bar of komorebi, for Windows and macOS: the module of the target OS is
//! the bar, the code both share is in `komorebi-bar-core`.

cfg_select! {
    target_os = "windows" => {
        mod windows;
        use windows as platform;
    }
    target_os = "macos" => {
        mod macos;
        use macos as platform;
    }
}

fn main() -> color_eyre::Result<()> {
    platform::main()
}
