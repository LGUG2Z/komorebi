//! `view`: the arguments of the viewer window.
//!
//! The command table runs the command with `crate::viewer::run`; this file holds only its arguments.

use clap::Args;
use std::path::PathBuf;

/// The arguments of `view`: the session to open, if one is given.
#[derive(Args, Debug)]
pub struct ViewArgs {
    /// The recording's folder [default: choose one in the window]
    pub session: Option<PathBuf>,
}
