//! The Windows host: what the [`platform`](crate::platform) facade is made of on Windows.
//!
//! | module | built on |
//! |---|---|
//! | `apps` | `EnumWindows` and `GetWindowThreadProcessId` |
//! | `capture` | monitor enumeration and per-monitor DPI; the capture itself is Windows.Graphics.Capture (in `crate::capture::wgc`) |
//! | `clock` | `QueryPerformanceCounter` |
//! | `media` | Windows Runtime and Media Foundation start-up, shared by the capture and the decoder |
//! | `power` | a hidden window that is sent suspend, resume, lid, display and session messages |
//! | `process` | process creation flags and `TerminateProcess` |
//! | `session` | whether the input desktop can be opened |
//! | `shell` | Explorer, the folder dialog and `RtlGetVersion` |
//! | `trace` | a `WinEvent` hook and Event Log subscriptions (see `crate::sources::wintrace`) |
//! | `window` | this program's own windows and the reads about any window |
//!
//! Nothing here is public API except through the facade: this module is private, and the facade
//! re-exports its submodules (all but `window`, which the shared `platform::window` calls) under the
//! same names as on the other hosts.
//!
//! This code is compiled only for Windows. From another host it is type-checked and linted with
//! `just check-windows`, and the checklist for running it on a real machine is
//! `docs/dev/windows-checkpoints.md`.

pub mod apps;
pub mod clock;
pub mod process;
pub mod shell;

/// Does what Windows needs once, first thing in `main`: makes the process per-monitor DPI aware
/// ([`capture::become_per_monitor_dpi_aware`]), so that every size and position it reads is in
/// physical pixels, as komorebi's are. A Mac needs nothing here.
pub fn init() {
    // Without this Windows lies to us about sizes: a display scaled to 150 % reads as smaller than
    // it is, and komorebi's rects (which are physical pixels) would not match ours.
}
