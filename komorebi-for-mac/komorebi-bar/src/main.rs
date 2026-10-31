//! `komorebi-bar` for macOS. The code is in the `macos` module.

mod macos;

fn main() -> color_eyre::Result<()> {
    macos::main()
}
