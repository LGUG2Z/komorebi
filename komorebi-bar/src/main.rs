//! `komorebi-bar`, the status bar of komorebi. The code of each platform is in a module of its own.

mod windows;

fn main() -> color_eyre::Result<()> {
    windows::main()
}
