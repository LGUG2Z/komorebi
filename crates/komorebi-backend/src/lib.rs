//! What every komorebi daemon asks of its platform.
//!
//! The window manager logic should talk to a [`Backend`], never to KWin, Win32 or the macOS
//! Accessibility API. This crate holds the vocabulary of that conversation and nothing that
//! belongs to one platform:
//!
//! - [`WindowId`] and [`MonitorId`], the names a backend gives windows and monitors;
//! - [`WindowInfo`] and [`MonitorInfo`], plain data, with an [`Option`] for everything that is
//!   not available on every platform;
//! - [`Error`], which can say that "the window is gone" and that "this platform cannot do that";
//! - the [`Backend`] trait, with only the operations that at least two platforms have
//!   (`docs/dev/seam.md`, section 2.1), a [`Capability`] query for the ones only some platforms
//!   have, and a [`Batch`] scope for writes that should appear together;
//! - [`fake::FakeBackend`], an in-memory desktop with failure injection and frame offsets,
//!   and [`contract`], the checks that every backend has to pass.
//!
//! The evidence for each choice, and the status of every claim (confirmed, corrected or target),
//! is in `docs/dev/seam.md`. The doc comment of each item says which platforms implement it.
//!
//! # Platform-specific operations
//!
//! Borders and their colours, the look, the wallpaper, drag origin and end frames, stackbars,
//! observers and event taps are not here: they are one platform's business and live in that
//! platform's own trait or crate (for KDE, `komorebi::backend::Backend` still has them).

#![deny(missing_docs)]

mod backend;
pub mod contract;
mod error;
pub mod fake;
mod id;
mod info;
mod registry;

pub use backend::Backend;
pub use backend::Batch;
pub use backend::Capability;
pub use error::Error;
pub use error::Result;
pub use id::MonitorId;
pub use id::WindowId;
pub use info::MonitorInfo;
pub use info::WindowInfo;
pub use komorebi_layouts::Rect;
pub use registry::Registry;
