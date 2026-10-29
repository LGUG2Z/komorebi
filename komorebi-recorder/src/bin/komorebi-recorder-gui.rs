//! The same program as `komorebi-recorder`, built for the Windows GUI subsystem: it opens no
//! console window, which is what a shortcut wants.
//! Its output goes nowhere, so use `komorebi-recorder` to run a command and read what it prints.
//! (On other systems there is no such distinction and this is the same binary.)
//!
//! `main` is [`komorebi_recorder::cli::main`], as it is for `komorebi-recorder`: it reads the
//! command line and runs the command, and with no command opens the one window on its Record tab
//! ([`komorebi_recorder::app::run`]). It shares the window's socket with `komorebi-recorder`, so
//! whichever of the two is started second raises the window the first one opened. A process that
//! wants a window to open a recording ([`komorebi_recorder::app::open_viewer`]) starts this
//! executable with `view` when it is next to the one that is running (on Windows), so that
//! opening a recording does not open a console window beside it.
#![cfg_attr(windows, windows_subsystem = "windows")]

use color_eyre::eyre;
use std::process::ExitCode;

fn main() -> eyre::Result<ExitCode> {
    komorebi_recorder::cli::main()
}
