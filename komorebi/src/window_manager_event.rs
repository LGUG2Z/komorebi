//! [`WindowManagerEvent`], an event the compositor reported about a window, in the form the
//! window manager handles it
//! ([`WindowManager::process_event`](crate::window_manager::WindowManager::process_event)).
//!
//! An event is about one window and carries it, and the compositor signal it came from
//! ([`SystemNotification`]), which is only for logging. Two events come from komorebi itself
//! rather than from the compositor: [`WindowManagerEvent::Manage`] and
//! [`WindowManagerEvent::Unmanage`], which the commands that manage and unmanage the focused window
//! raise.

use crate::window::Window;
use serde::Deserialize;
use serde::Serialize;
use strum::Display;

/// Where a [`WindowManagerEvent`] came from: a compositor signal, or komorebi itself
#[derive(Debug, Copy, Clone, Serialize, Deserialize, Display, PartialEq, Eq)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[serde(tag = "type", content = "content")]
pub enum SystemNotification {
    KWin(KWinNotification),
    Manual(ManualNotification),
}

/// The KWin script signals the bridge listens to
#[derive(Debug, Copy, Clone, Serialize, Deserialize, Display, PartialEq, Eq)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
pub enum KWinNotification {
    /// `workspace.windowAdded`
    WindowAdded,
    /// `workspace.windowRemoved`
    WindowRemoved,
    /// `workspace.windowActivated`
    WindowActivated,
    /// `window.minimizedChanged` to true
    WindowMinimized,
    /// `window.minimizedChanged` to false
    WindowRestored,
}

/// Events komorebi raises itself, for example from a command
#[derive(Debug, Copy, Clone, Serialize, Deserialize, Display, PartialEq, Eq)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
pub enum ManualNotification {
    Manage,
    Unmanage,
}

/// Window events: each carries the window it is about.
#[derive(Debug, Copy, Clone, Serialize, Deserialize, Display)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[serde(tag = "type", content = "content")]
pub enum WindowManagerEvent {
    FocusChange(SystemNotification, Window),
    Show(SystemNotification, Window),
    Destroy(SystemNotification, Window),
    Minimize(SystemNotification, Window),
    Restore(SystemNotification, Window),
    /// The user asked komorebi to manage the window (`ManageFocusedWindow`)
    Manage(Window),
    /// The user asked komorebi to stop managing the window (`UnmanageFocusedWindow`)
    Unmanage(Window),
}

impl WindowManagerEvent {
    /// The window this event is about, if it is about one
    pub fn window(&self) -> Option<Window> {
        Some(match self {
            WindowManagerEvent::FocusChange(_, window)
            | WindowManagerEvent::Show(_, window)
            | WindowManagerEvent::Destroy(_, window)
            | WindowManagerEvent::Minimize(_, window)
            | WindowManagerEvent::Restore(_, window)
            | WindowManagerEvent::Manage(window)
            | WindowManagerEvent::Unmanage(window) => *window,
        })
    }

    pub fn window_id(&self) -> Option<u32> {
        self.window().map(|w| w.id)
    }

    pub fn process_id(&self) -> Option<i32> {
        self.window().map(|w| w.process_id())
    }

    /// The notification that caused this event, for logging
    pub fn notification(&self) -> String {
        match self {
            WindowManagerEvent::FocusChange(n, _)
            | WindowManagerEvent::Show(n, _)
            | WindowManagerEvent::Destroy(n, _)
            | WindowManagerEvent::Minimize(n, _)
            | WindowManagerEvent::Restore(n, _) => match n {
                SystemNotification::KWin(k) => k.to_string(),
                SystemNotification::Manual(m) => m.to_string(),
            },
            WindowManagerEvent::Manage(_) => ManualNotification::Manage.to_string(),
            WindowManagerEvent::Unmanage(_) => ManualNotification::Unmanage.to_string(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_event_names_its_window_and_the_signal_it_came_from() {
        let event = WindowManagerEvent::Minimize(
            SystemNotification::KWin(KWinNotification::WindowMinimized),
            Window::from(301),
        );

        assert_eq!(event.window(), Some(Window::from(301)));
        assert_eq!(event.window_id(), Some(301));
        // the crate's unit tests get a permissive fake, which gives a window the pid id + 1000
        assert_eq!(event.process_id(), Some(1301));
        assert_eq!(event.notification(), "WindowMinimized");
        assert_eq!(event.to_string(), "Minimize");
    }
}
