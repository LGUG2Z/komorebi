//! Linux, as KDE Plasma: the host that records komorebi for KDE.
//!
//! The master clock, the child processes and the shell are Linux's own. The rest of the host
//! (`apps`, `capture`, `power`, `session`, `trace`) is still the `unsupported` host's, and is
//! replaced one module at a time as each is written (see `docs/PLATFORM.md`).

// the pieces of the fallback that Linux has its own version of are not used
#[allow(dead_code)]
#[path = "../unsupported.rs"]
mod fallback;

pub mod clock;
pub mod process;
pub mod shell;

pub use fallback::apps;
pub use fallback::capture;
pub use fallback::power;
pub use fallback::session;
pub use fallback::trace;

/// Nothing to do first.
pub fn init() {}
