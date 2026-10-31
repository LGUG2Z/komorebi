//! `komorebic-macos`: the macOS command line of the unified `komorebic` package, kept under its old
//! package name until the cutover. The code is in `komorebic/src/macos`.

#[path = "../../../komorebic/src/macos/mod.rs"]
mod macos;

fn main() -> color_eyre::eyre::Result<()> {
    macos::main()
}
