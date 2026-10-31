//! `komorebic-linux`: the KDE command line of the unified `komorebic` package, kept under its old
//! package name until the cutover. The code is in `komorebic/src/linux`.

#[path = "../../../komorebic/src/linux/mod.rs"]
mod linux;

fn main() -> color_eyre::eyre::Result<()> {
    linux::main()
}
