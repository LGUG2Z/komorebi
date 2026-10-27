//! Hearing about sleep, wake, the lid, the displays and the session as they happen: not on macOS.
//!
//! macOS has no power watcher: a sleep is found by the host clock pausing while the wall clock does
//! not (see [`crate::clock::JumpDetector`]). [`start`] returns no [`Watcher`], and the recording
//! loop works from the clocks alone.
//!
//! On Windows a watcher listens to a hidden window and hands the recording loop a
//! [`PowerEvent`] for each thing it hears.

use crate::power::PowerEvent;

/// Listens for power events and holds what it heard until it is drained. Never constructed on this
/// host: [`start`] returns `None`.
pub struct Watcher;

impl Watcher {
    /// Returns what has been heard since the last call, oldest first. Always empty on this host.
    #[must_use]
    pub fn drain(&self) -> Vec<PowerEvent> {
        Vec::new()
    }
}

/// Starts listening for power events. There is nothing to listen to on a Mac, so this returns
/// `Ok(None)` and the recording loop finds a sleep from the clocks.
///
/// # Errors
///
/// Never fails on this host. (On Windows it fails if the watcher's window cannot be made.)
///
/// # Example
///
/// ```rust
/// use komorebi_recorder::platform::power;
///
/// // a Mac has nothing to listen to: the recording loop works from the clocks
/// assert!(power::start()?.is_none());
/// # Ok::<(), Box<dyn std::error::Error>>(())
/// ```
pub fn start() -> color_eyre::eyre::Result<Option<Watcher>> {
    Ok(None)
}
