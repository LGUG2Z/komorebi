//! The `komorebi-recorder` command: it reads the command line and runs it (see [`komorebi_recorder::cli`]).

use color_eyre::eyre;
use std::process::ExitCode;

fn main() -> eyre::Result<ExitCode> {
    komorebi_recorder::cli::main()
}
