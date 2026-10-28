//! `view`: the arguments of the viewer window.
//!
//! The command table runs the command with `crate::viewer::run`; this file holds only its arguments.

use clap::Args;
use std::path::PathBuf;

/// The arguments of `view`: the sessions to open, if any are given.
#[derive(Args, Debug)]
pub struct ViewArgs {
    /// The recordings' folders, each opened as a tab of the one viewer window [default: choose
    /// one in the window]
    pub session: Vec<PathBuf>,
    /// Open a window of its own, instead of a tab of the viewer that is already running
    #[arg(long)]
    pub new_window: bool,
}
