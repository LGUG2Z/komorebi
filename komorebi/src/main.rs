//! The komorebi daemon: a tiling window manager for KDE Plasma.
//!
//! The daemon owns the layout of every window and asks KWin, the window
//! manager of Plasma, to put each one where the layout says.

use clap::Parser;

/// A tiling window manager for KDE Plasma.
#[derive(Parser)]
#[clap(author, about, version = version::LONG_VERSION)]
struct Opts {}

fn main() {
    Opts::parse();
}
