//! Whether the session is locked, from the screen saver on the session bus.
//!
//! KDE's screen locker answers `org.freedesktop.ScreenSaver.GetActive`: `true` while the lock
//! screen is up. The connection to the bus is made once. A session with no bus, or no screen
//! saver, is read as unlocked: that is also what [`SessionState::default`] is, and the recorder
//! then finds a lock the other way, by a stream that stops.

use crate::system_state::SessionState;
use std::sync::OnceLock;
use zbus::blocking::Connection;

fn locked() -> Option<bool> {
    static BUS: OnceLock<Option<Connection>> = OnceLock::new();
    let bus = BUS.get_or_init(|| Connection::session().ok()).as_ref()?;
    bus.call_method(
        Some("org.freedesktop.ScreenSaver"),
        "/org/freedesktop/ScreenSaver",
        Some("org.freedesktop.ScreenSaver"),
        "GetActive",
        &(),
    )
    .ok()?
    .body()
    .deserialize::<bool>()
    .ok()
}

/// Returns the current session state: locked as the screen saver says, and on the console (this
/// host does not tell otherwise).
#[must_use]
pub fn read() -> SessionState {
    SessionState {
        locked: locked().unwrap_or(false),
        on_console: true,
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn reading_the_session_never_fails_with_or_without_a_bus() {
        // the answer is the machine's, and not asserted: only that one is given
        let _ = super::read();
    }
}
