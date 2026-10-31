//! The id -> {AX element, pid, observer} table of step 11(ii): what lets anything act on a
//! window from nothing but its `WindowId`.
//!
//! It is `komorebi_backend::Registry` (whose rules are tested natively: thread-safe, nothing of
//! the platform runs under its lock, stale ids are `Error::WindowGone`) instantiated with
//! [`AxEntry`].
//!
//! # Lifetime
//!
//! An entry is created by `Window::observe`, when the window's observer has been attached to the run
//! loop (a `Window` made only to ask an application about a window is never observed, so never
//! registered, and the observer in the table is the attached one), and by [`sync`] when a window's
//! element or id changes (a native tab). The entry holds an *inert* copy of the application
//! ([`crate::application::Application::inert`]): dropping a normal copy asks the application
//! something over Accessibility and may invalidate the shared observer, which must never happen
//! inside a table operation.
//!
//! It ends where the daemon knows a window is gone:
//!
//! - the reaper, when it reaps a window ([`forget`]), and the application reaper
//!   (`reap_invalid_windows_for_application`, [`forget`]);
//! - an application terminating ([`forget_pid`], the first thing `process_event` does for
//!   `NSWorkspaceDidTerminateApplication`, ahead of the pause and other-space returns).
//!
//! It does not end in `Window::drop`: that runs for every clone, `alive()` is false for any failed
//! Accessibility call, and a transient failure must not deregister a live window.
//!
//! Not enforced: an observed window that closes without passing through the reapers keeps its entry
//! until its application quits (`WindowManagerEvent::Destroy` carries no window id). The table is
//! bounded by the windows observed per running application; a sweep would cost one Accessibility call
//! per entry under the window manager lock. What an unattached observer costs (a Mach port?) is not
//! verified (hand check H4).
//!
//! # Threads
//!
//! Callable from any thread. Nothing here dispatches to the main queue or takes the window
//! manager's lock (`docs/dev/seam.md` section 3.3); a closure that runs on the main thread may
//! call it. Handles removed from the table are dropped after the table's lock is released.

use crate::AccessibilityObserver;
use crate::AccessibilityUiElement;
use crate::application::Application;
use crate::window::Window;
use komorebi_backend::Registry;
use komorebi_backend::Result;
use komorebi_backend::WindowId;
use std::sync::LazyLock;

/// What it takes to act on one window: its element and observer, and its application (whose
/// element and observer are the application's, and whose `process_id` is the pid).
#[derive(Debug, Clone)]
pub struct AxEntry {
    /// The window's Accessibility element
    pub element: AccessibilityUiElement,
    /// The observer that reports the window's events
    pub observer: AccessibilityObserver,
    /// The application that owns the window
    pub application: Application,
}

impl AxEntry {
    /// The pid of the owning application
    pub fn pid(&self) -> i32 {
        self.application.process_id
    }
}

static WINDOWS: LazyLock<Registry<AxEntry>> = LazyLock::new(Registry::new);

/// Registers the window under its id, replacing what was registered under it.
pub fn register(window: &Window) {
    let entry = window.entry();
    let pid = entry.pid();
    // the replaced entry is dropped here, with the table's lock released
    drop(WINDOWS.insert(WindowId::from_u32(window.id), pid, entry));
}

/// A window changed its id or its element in place (a native tab that is now shown by another
/// element): forget the old id and register the window as it is.
pub fn sync(old_id: u32, window: &Window) {
    if old_id != window.id {
        forget(old_id);
    }

    register(window);
}

/// Forgets a window that is gone. A second call is not an error.
pub fn forget(id: u32) {
    drop(WINDOWS.remove(WindowId::from_u32(id)));
}

/// Forgets every window of a process that quit.
pub fn forget_pid(pid: i32) {
    drop(WINDOWS.remove_pid(pid));
}

/// The entry of a window, or `Error::WindowGone` if the id is stale.
pub fn get(id: WindowId) -> Result<AxEntry> {
    WINDOWS.get(id)
}

/// A `Window` made from the entry of the id, or `Error::WindowGone` if the id is stale.
pub fn window(id: WindowId) -> Result<Window> {
    let entry = WINDOWS.get(id)?;
    // an id that does not fit a `CGWindowID` cannot have been registered
    let raw = id.to_u32().ok_or(komorebi_backend::Error::WindowGone(id))?;
    Ok(Window::from_entry(raw, entry))
}

/// Every registered window, by id.
pub fn windows() -> Vec<Window> {
    WINDOWS
        .snapshot()
        .into_iter()
        .filter_map(|(id, entry)| Some(Window::from_entry(id.to_u32()?, entry)))
        .collect()
}

/// The `CGWindowID` of the window that an Accessibility element is, as handed over by the system
/// (an observer callback, the event tap, the focused window of an application). This is the one
/// way to turn an element into the key of the table; a window is registered under what it
/// returns.
pub fn identify(
    element: &objc2_application_services::AXUIElement,
) -> std::result::Result<u32, crate::accessibility::error::AccessibilityError> {
    crate::accessibility::AccessibilityApi::window_id(element)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn window_of(id: u32, pid: i32) -> Window {
        let mut window = Window::from(id);
        window.application.process_id = pid;
        window
    }

    fn id(raw: u32) -> WindowId {
        WindowId::from_u32(raw)
    }

    /// `Application::drop` asks the application whether it is still there (a blocking
    /// Accessibility round trip) when `is_observable`; the table must hold copies that skip it.
    #[test]
    fn a_registered_entry_does_not_ask_the_app_when_it_is_dropped() {
        let mut window = window_of(0x5101, 6_100_001);
        window.application.is_observable = true;

        assert!(!window.entry().application.is_observable);

        register(&window);
        assert!(!get(id(0x5101)).unwrap().application.is_observable);

        forget(0x5101);
    }

    #[test]
    fn a_window_made_from_an_entry_is_inert_too() {
        let mut window = window_of(0x5102, 6_100_002);
        window.application.is_observable = true;
        register(&window);

        assert!(!super::window(id(0x5102)).unwrap().application.is_observable);

        forget(0x5102);
    }
}

#[cfg(test)]
mod drop_tests {
    use super::*;

    /// `State::from` clones every window on every command and event notification; whatever a
    /// clone's drop finds out about the window (the stand-in element answers differently with and
    /// without Accessibility permission), the entry of a window that was not reaped stays.
    #[test]
    fn dropping_a_clone_does_not_deregister_a_window() {
        let mut window = Window::from(0x5201);
        window.application.process_id = 6_200_001;
        register(&window);

        drop(window.clone());

        assert!(get(WindowId::from_u32(0x5201)).is_ok());

        forget(0x5201);
    }
}

#[cfg(test)]
mod pid_tests {
    use super::*;

    #[test]
    fn an_application_quitting_takes_its_windows_and_no_other() {
        let mut quitter = Window::from(0x5301);
        quitter.application.process_id = 6_300_001;
        let mut stayer = Window::from(0x5302);
        stayer.application.process_id = 6_300_002;
        register(&quitter);
        register(&stayer);

        forget_pid(6_300_001);

        assert!(get(WindowId::from_u32(0x5301)).is_err());
        assert!(get(WindowId::from_u32(0x5302)).is_ok());

        forget(0x5302);
    }
}
