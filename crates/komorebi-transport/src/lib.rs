//! The command transport shared by every komorebi daemon.
//!
//! The crate holds no window-manager code: a daemon plugs its window manager into the listener
//! with [`CommandTarget`], and its platform's connection quirks (the read timeouts of the Windows
//! daemon) with [`ConnectionPolicy`].
#![warn(clippy::all)]

mod listener;
mod pause;

pub use listener::CommandListener;
pub use listener::CommandStream;
pub use listener::CommandTarget;
pub use listener::ConnectionPolicy;
pub use listener::NoPolicy;
pub use listener::listen_for_commands;
pub use listener::read_commands;
pub use pause::allowed_while_paused;

/// The `tracing` target of what this crate logs
///
/// The daemons for KDE and macOS filter by `komorebi=<level>`, which only matches targets that
/// are `komorebi` or start with `komorebi::`, so the crate does not log under its own module path.
pub(crate) const LOG_TARGET: &str = "komorebi::transport";
