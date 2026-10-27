//! The macOS host: what the [`platform`](crate::platform) facade is made of on a Mac.
//!
//! | module | built on |
//! |---|---|
//! | `apps` | the window server's window list (`CGWindowListCopyWindowInfo`) |
//! | `capture` | `ScreenCaptureKit` and `AVFoundation` (in `crate::capture::sck`) |
//! | `clock` | `mach_absolute_time` |
//! | `power` | nothing: a Mac's sleep is found from the clocks, so there is no watcher |
//! | `process` | process groups and `SIGTERM` |
//! | `session` | the CoreGraphics session dictionary |
//! | `shell` | `open`, `osascript` and `sw_vers` |
//! | `trace` | nothing: the OS trace is `log stream` children (`crate::sources::oslog`), not taps |
//!
//! Nothing here is public API except through the facade: this module is private, and the facade
//! re-exports its submodules under the same names on every host.

pub mod apps;
pub mod clock;
pub mod process;
pub mod shell;
pub mod trace;

/// Does nothing: a Mac needs nothing done once at the start of `main`. (Windows makes the process
/// DPI aware.)
pub fn init() {}
