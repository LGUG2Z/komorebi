//! The `komorebi-recorder` command.

use clap::Parser;

#[derive(Parser)]
#[command(
    author,
    version,
    about = "Synchronized screen, log and trace capture for komorebi bug reproduction"
)]
struct Opts;

fn main() {
    Opts::parse();
}
