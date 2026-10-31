//! The command transport shared by every komorebi daemon.
//!
//! The crate holds no window-manager code: the daemons plug their own window manager in through
//! small traits.
#![warn(clippy::all)]

mod pause;

pub use pause::allowed_while_paused;
