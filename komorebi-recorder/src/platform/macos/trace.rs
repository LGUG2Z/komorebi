//! The OS-trace taps: none on macOS.
//!
//! macOS records its OS trace with `log stream` children (see [`crate::sources::oslog`]), not with
//! taps of its own, so there is nothing to start here and [`start`] starts none. On Windows the
//! taps are a `WinEvent` hook and Event Log subscriptions, which feed the two files of
//! [`crate::sources::wintrace`].

use crate::sources::wintrace::TraceStream;
use std::sync::Arc;

/// The running taps, which listen until dropped. Never constructed on this host: [`start`]
/// returns none.
pub struct Taps;

/// Starts the OS-trace taps, writing into `system` and `event_log`. This host starts none and
/// returns `None` with one problem in words, saying that the OS trace is recorded another way.
///
/// The second element of the result lists why a tap could not be started, one entry each; the
/// recording goes on without it. On Windows the taps are always returned, with an entry for each
/// that failed.
#[must_use]
pub fn start(
    _system: Arc<TraceStream>,
    _event_log: Arc<TraceStream>,
) -> (Option<Taps>, Vec<String>) {
    (
        None,
        vec!["winevents: this host records its OS trace another way".to_string()],
    )
}
