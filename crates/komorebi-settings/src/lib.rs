//! The settings a daemon reads while it runs.
//!
//! Today each daemon keeps them in dozens of process-wide statics that `apply_globals` and the
//! `SocketMessage` handlers write one at a time while the border, animator and reaper threads
//! read them (see `docs/dev/settings.md`). This crate holds the replacement and nothing else: no
//! daemon uses it yet.
//!
//! * [`Settings`] is a value: a [`Shared`] part that is the same struct on every platform and a
//!   platform part `P` that each daemon defines for its own subsystems.
//! * [`Handle`] holds the current value. A reader takes a [`Handle::snapshot`] once per unit of
//!   work and sees one consistent configuration for all of it. A writer uses [`Handle::update`],
//!   which serialises writers, so two writers never lose each other's change, and publishes the
//!   result once.
//! * [`Handle::scoped`] replaces the "store a value, do work, store the old value back" pattern
//!   without clobbering a change made in between.
#![warn(clippy::all)]
#![allow(clippy::missing_errors_doc, clippy::use_self, clippy::doc_markdown)]

mod handle;
pub mod shared;

pub use handle::Handle;
pub use handle::Scoped;
pub use shared::Shared;

/// Every setting of one running daemon.
///
/// `P` is the platform part: a plain type the daemon defines (the Windows stackbar and hiding
/// behaviour, the macOS tabbed-application list, ...). Shared code never looks inside it, so it is
/// a type parameter and not a trait; it needs to be `Clone` so that an update can copy the value.
#[derive(Clone, Debug, PartialEq)]
pub struct Settings<P> {
    pub shared: Shared,
    pub platform: P,
}

impl<P> Settings<P> {
    pub fn new(shared: Shared, platform: P) -> Self {
        Self { shared, platform }
    }
}
