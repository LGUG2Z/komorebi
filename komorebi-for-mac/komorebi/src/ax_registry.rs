//! The id -> {AX element, pid, observer} table of step 11(ii): what lets anything act on a
//! window from nothing but its `WindowId`.
//!
//! It is `komorebi_backend::Registry` (whose rules are tested natively: thread-safe, nothing of
//! the platform runs under its lock, stale ids are `Error::WindowGone`) instantiated with
//! [`AxEntry`].
//!
//! # Lifetime
//!
//! An entry is created by `Window::new` and by [`sync`] when a window's element or id changes
//! (a native tab). It ends at every place the daemon learns of a death:
//!
//! - `Window::drop`, when the window is no longer alive ([`forget`]);
//! - the reaper, when it reaps a window ([`forget`]);
//! - an application terminating ([`forget_pid`], from `WindowManagerEvent::Destroy`).
//!
//! A window that is alive and never managed stays registered until it dies or its application
//! quits: the entry only holds retained references that the `Window` clones held anyway.
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
