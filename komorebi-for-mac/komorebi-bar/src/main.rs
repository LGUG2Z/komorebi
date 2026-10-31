//! `komorebi-bar-macos`: the macOS bar of the unified `komorebi-bar` package, kept under its old package
//! name until the cutover. The code is in `komorebi-bar/src/macos`.

#[path = "../../../komorebi-bar/src/macos/mod.rs"]
mod macos;

fn main() -> color_eyre::Result<()> {
    macos::main()
}
