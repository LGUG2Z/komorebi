//! [`WindowManagerEvent`], an event the compositor reported about a window, in the form the
//! window manager handles it
//! ([`WindowManager::process_event`](crate::window_manager::WindowManager::process_event)).
//!
//! An event carries the compositor signal it came from ([`SystemNotification`]), which is only for
//! logging. Every event but one is about a window, and carries it: a
//! [`WindowManagerEvent::WorkspaceChange`] is about a monitor that shows another workspace. Two
//! events come from komorebi itself rather than from the compositor:
//! [`WindowManagerEvent::Manage`] and [`WindowManagerEvent::Unmanage`], which the commands that
//! manage and unmanage the focused window raise.

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
    /// `workspace.currentDesktopChanged`: a monitor now shows another desktop
    WorkspaceChanged,
    /// `window.desktopsChanged`: a window is on another desktop
    WindowMovedToDesktop,
    /// `window.minimizedChanged` to true
    WindowMinimized,
    /// `window.minimizedChanged` to false
    WindowRestored,
    /// `window.maximizedChanged`
    WindowMaximizedChanged,
    /// `window.outputChanged`
    WindowOutputChanged,
    /// `window.captionChanged`
    CaptionChanged,
    /// `window.interactiveMoveResizeStarted` for a move
    InteractiveMoveStarted,
    /// `window.interactiveMoveResizeFinished` after a move
    InteractiveMoveFinished,
    /// `window.interactiveMoveResizeStarted` for a resize
    InteractiveResizeStarted,
    /// `window.interactiveMoveResizeFinished` after a resize
    InteractiveResizeFinished,
    /// `window.interactiveMoveResizeStepped` of a resize: where the user has the edges now
    InteractiveResizeStepped,
}

/// Events komorebi raises itself, for example from a command
#[derive(Debug, Copy, Clone, Serialize, Deserialize, Display, PartialEq, Eq)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
pub enum ManualNotification {
    Manage,
    Unmanage,
}

/// Window events: each carries the window it is about, but for a workspace change.
#[derive(Debug, Copy, Clone, Serialize, Deserialize, Display)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[serde(tag = "type", content = "content")]
pub enum WindowManagerEvent {
    FocusChange(SystemNotification, Window),
    Show(SystemNotification, Window),
    Destroy(SystemNotification, Window),
    Minimize(SystemNotification, Window),
    Restore(SystemNotification, Window),
    /// The window's title changed
    TitleUpdate(SystemNotification, Window),
    /// A window is on another output than it was: the user moved it (a shortcut), or KWin did when
    /// an output went away or came back. Where it is now is what the window reports.
    OutputChange(SystemNotification, Window),
    /// A window was maximized (`true`) or restored (`false`) by the compositor: komorebi's own
    /// `toggle-maximize` does this too and finds the window already as it asked, but the user's
    /// title bar does it without komorebi's knowing
    MaximizeChange(SystemNotification, Window, bool),
    /// The user asked komorebi to manage the window (`ManageFocusedWindow`)
    Manage(Window),
    /// The user asked komorebi to stop managing the window (`UnmanageFocusedWindow`)
    Unmanage(Window),
    /// The user took hold of a window to move it with the pointer
    MoveStart(SystemNotification, Window),
    /// The user let go of a window they were moving: where it is now is what the window reports,
    /// and the pointer is where they let go
    MoveEnd(SystemNotification, Window),
    /// The user took hold of an edge or a corner of a window to resize it with the pointer
    ResizeStart(SystemNotification, Window),
    /// The user let go of the edge or the corner they were dragging: where they took it is what the
    /// window reports
    ResizeEnd(SystemNotification, Window),
    /// The user is part way through resizing a window: where they have its edges now is what the
    /// window reports (`Window::rect`). Only the newest of these is ever waiting
    /// (`window_manager_event_listener`), and they make no notification.
    ResizeStep(SystemNotification, Window),
    /// A monitor (by id) now shows another workspace: komorebi did that itself, or a pager, a
    /// shortcut or an app raising a window did. Not about any one window.
    WorkspaceChange(SystemNotification, u32, usize),
    /// A window was put on another workspace's desktop by something other than komorebi (the
    /// window menu's "Move to Desktop", a drag in the pager)
    WorkspaceMove(SystemNotification, Window, usize),
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
            | WindowManagerEvent::TitleUpdate(_, window)
            | WindowManagerEvent::MaximizeChange(_, window, _)
            | WindowManagerEvent::OutputChange(_, window)
            | WindowManagerEvent::Manage(window)
            | WindowManagerEvent::Unmanage(window)
            | WindowManagerEvent::MoveStart(_, window)
            | WindowManagerEvent::MoveEnd(_, window)
            | WindowManagerEvent::ResizeStart(_, window)
            | WindowManagerEvent::ResizeEnd(_, window)
            | WindowManagerEvent::ResizeStep(_, window)
            | WindowManagerEvent::WorkspaceMove(_, window, _) => *window,
            WindowManagerEvent::WorkspaceChange(..) => return None,
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
            | WindowManagerEvent::Restore(n, _)
            | WindowManagerEvent::TitleUpdate(n, _)
            | WindowManagerEvent::MaximizeChange(n, _, _)
            | WindowManagerEvent::OutputChange(n, _)
            | WindowManagerEvent::MoveStart(n, _)
            | WindowManagerEvent::MoveEnd(n, _)
            | WindowManagerEvent::ResizeStart(n, _)
            | WindowManagerEvent::ResizeEnd(n, _)
            | WindowManagerEvent::ResizeStep(n, _)
            | WindowManagerEvent::WorkspaceChange(n, _, _)
            | WindowManagerEvent::WorkspaceMove(n, _, _) => match n {
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

    #[test]
    fn an_output_change_names_its_window_and_the_signal_it_came_from() {
        let event = WindowManagerEvent::OutputChange(
            SystemNotification::KWin(KWinNotification::WindowOutputChanged),
            Window::from(302),
        );

        assert_eq!(event.window(), Some(Window::from(302)));
        assert_eq!(event.notification(), "WindowOutputChanged");
        assert_eq!(event.to_string(), "OutputChange");
    }

    #[test]
    fn a_workspace_change_is_about_a_monitor_and_no_window() {
        let event = WindowManagerEvent::WorkspaceChange(
            SystemNotification::KWin(KWinNotification::WorkspaceChanged),
            7,
            2,
        );

        assert_eq!(event.window(), None);
        assert_eq!(event.window_id(), None);
        assert_eq!(event.notification(), "WorkspaceChanged");
    }

    #[test]
    fn a_maximize_change_says_whether_the_window_was_maximized_or_restored() {
        let event = WindowManagerEvent::MaximizeChange(
            SystemNotification::KWin(KWinNotification::WindowMaximizedChanged),
            Window::from(302),
            true,
        );

        assert_eq!(event.window_id(), Some(302));
        assert_eq!(event.notification(), "WindowMaximizedChanged");
        assert_eq!(event.to_string(), "MaximizeChange");
        assert!(matches!(
            event,
            WindowManagerEvent::MaximizeChange(_, _, true)
        ));
    }
}
