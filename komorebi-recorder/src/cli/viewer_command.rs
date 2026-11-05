//! `gui` and `view`: the arguments of the one window.
//!
//! The command table runs `gui` with `crate::app::run` and `view` with `crate::viewer::run`; this
//! file holds only their arguments.

use super::RecordArgs;
use clap::Args;
use std::path::PathBuf;

/// The arguments of `gui` (and of running with no command): the options the Record tab starts
/// from, which are those of `record`.
///
/// If a window is already running they are ignored: the second launch only raises it and shows its
/// Record tab.
#[derive(Args, Clone, Debug, Default)]
pub struct GuiArgs {
    /// The options of `record`, which each recording started from the window uses
    #[command(flatten)]
    pub record: RecordArgs,
    /// Open a window of its own, instead of raising the one that is already running
    #[arg(long)]
    pub new_window: bool,
}

/// The arguments of `view`: the sessions to open, if any are given.
#[derive(Args, Debug)]
pub struct ViewArgs {
    /// The recordings' folders, each opened as a tab of the one window [default: choose
    /// one in the window]
    pub session: Vec<PathBuf>,
    /// Open a window of its own, instead of a tab of the window that is already running
    #[arg(long)]
    pub new_window: bool,
}
