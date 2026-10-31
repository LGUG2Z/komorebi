#![allow(non_upper_case_globals)]

use crate::accessibility::notification_constants::AccessibilityNotification;
use crate::app_kit_notification_constants::AppKitWorkspaceNotification;
use crate::macos_api::MacosApi;
use serde::Deserialize;
use serde::Serialize;
use strum::Display;

#[derive(Debug, Copy, Clone, Serialize, Deserialize, Display)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[serde(tag = "type", content = "content")]
pub enum SystemNotification {
    Accessibility(AccessibilityNotification),
    AppKitWorkspace(AppKitWorkspaceNotification),
    Manual(ManualNotification),
}

#[derive(Debug, Copy, Clone, Serialize, Deserialize, Display)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
pub enum ManualNotification {
    ShowOnInputEvent,
    ShowOnFocusChangeFirstTabDestroyed,
    ShowOnFocusChangeWindowlessAppRestored,
    Manage,
    Unmanage,
    MoveEnd,
    ResizeEnd,
}

#[derive(Debug, Copy, Clone, Serialize, Deserialize, Display)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[serde(tag = "type", content = "content")]
pub enum WindowManagerEvent {
    FocusChange(SystemNotification, i32, Option<u32>),
    Show(SystemNotification, i32),
    Destroy(SystemNotification, i32),
    Minimize(SystemNotification, i32, u32),
    Restore(SystemNotification, i32, u32),
    Manage(SystemNotification, i32, u32),
    Unmanage(SystemNotification, i32, u32),
    MoveStart(SystemNotification, i32, u32),
    MoveEnd(SystemNotification, i32, u32),
    ResizeStart(SystemNotification, i32, u32),
    /// The window that is being dragged by an edge has moved, which is not a report of
    /// where it is, only that it has: the handler looks
    ResizeStep(SystemNotification, i32, u32),
    ResizeEnd(SystemNotification, i32, u32),
    SpaceChange(SystemNotification, i32),
    ScreenLock(SystemNotification, i32),
    ScreenUnlock(SystemNotification, i32),
}

impl WindowManagerEvent {
    pub fn from_system_notification(
        notification: SystemNotification,
        process_id: i32,
        window_id: Option<u32>,
    ) -> Option<Self> {
        match notification {
            SystemNotification::Accessibility(AccessibilityNotification::AXMainWindowChanged)
            | SystemNotification::Accessibility(
                AccessibilityNotification::AXApplicationActivated,
            )
            | SystemNotification::AppKitWorkspace(
                AppKitWorkspaceNotification::NSWorkspaceDidActivateApplicationNotification,
            ) => Some(WindowManagerEvent::FocusChange(
                notification,
                process_id,
                window_id,
            )),
            SystemNotification::Accessibility(AccessibilityNotification::AXWindowCreated)
            | SystemNotification::Accessibility(AccessibilityNotification::AXApplicationShown)
            | SystemNotification::AppKitWorkspace(
                AppKitWorkspaceNotification::NSWorkspaceDidLaunchApplicationNotification,
            )
            | SystemNotification::Manual(
                ManualNotification::ShowOnFocusChangeWindowlessAppRestored,
            )
            | SystemNotification::Manual(ManualNotification::ShowOnFocusChangeFirstTabDestroyed)
            | SystemNotification::Manual(ManualNotification::ShowOnInputEvent) => {
                Some(WindowManagerEvent::Show(notification, process_id))
            }
            SystemNotification::Accessibility(AccessibilityNotification::AXUIElementDestroyed)
            | SystemNotification::AppKitWorkspace(
                AppKitWorkspaceNotification::NSWorkspaceDidTerminateApplicationNotification,
            )
            | SystemNotification::Accessibility(AccessibilityNotification::AXApplicationHidden) => {
                Some(WindowManagerEvent::Destroy(notification, process_id))
            }
            // TODO: figure out if we wanna handle the hide/unhide notifications separately
            // TODO: maybe turn window id into a vec of window IDs for hidden apps
            SystemNotification::Accessibility(AccessibilityNotification::AXWindowMiniaturized) => {
                window_id.map(|window_id| {
                    WindowManagerEvent::Minimize(notification, process_id, window_id)
                })
            }
            SystemNotification::Accessibility(
                AccessibilityNotification::AXWindowDeminiaturized,
            ) => window_id
                .map(|window_id| WindowManagerEvent::Restore(notification, process_id, window_id)),
            SystemNotification::Manual(ManualNotification::Manage) => window_id
                .map(|window_id| WindowManagerEvent::Manage(notification, process_id, window_id)),
            SystemNotification::Manual(ManualNotification::Unmanage) => window_id
                .map(|window_id| WindowManagerEvent::Unmanage(notification, process_id, window_id)),
            SystemNotification::Manual(ManualNotification::MoveEnd) => window_id
                .map(|window_id| WindowManagerEvent::MoveEnd(notification, process_id, window_id)),
            SystemNotification::Manual(ManualNotification::ResizeEnd) => {
                window_id.map(|window_id| {
                    WindowManagerEvent::ResizeEnd(notification, process_id, window_id)
                })
            }
            SystemNotification::Accessibility(AccessibilityNotification::AXWindowMoved) => {
                if MacosApi::left_mouse_button_is_pressed() {
                    window_id.map(|window_id| {
                        WindowManagerEvent::MoveStart(notification, process_id, window_id)
                    })
                } else {
                    window_id.map(|window_id| {
                        WindowManagerEvent::MoveEnd(notification, process_id, window_id)
                    })
                }
            }
            SystemNotification::Accessibility(AccessibilityNotification::AXWindowResized) => {
                if MacosApi::left_mouse_button_is_pressed() {
                    window_id.map(|window_id| {
                        WindowManagerEvent::ResizeStart(notification, process_id, window_id)
                    })
                } else {
                    window_id.map(|window_id| {
                        WindowManagerEvent::ResizeEnd(notification, process_id, window_id)
                    })
                }
            }
            notification => {
                let notification_string = match notification {
                    SystemNotification::Accessibility(n) => n.to_string(),
                    SystemNotification::AppKitWorkspace(n) => n.to_string(),
                    SystemNotification::Manual(n) => n.to_string(),
                };

                tracing::trace!(
                    "ignoring system notification: {notification_string} (pid: {process_id}, window id: {window_id:?})"
                );
                None
            }
        }
    }

    pub fn process_id(&self) -> i32 {
        match self {
            WindowManagerEvent::FocusChange(_, process_id, _)
            | WindowManagerEvent::Show(_, process_id)
            | WindowManagerEvent::Destroy(_, process_id)
            | WindowManagerEvent::Minimize(_, process_id, _)
            | WindowManagerEvent::Manage(_, process_id, _)
            | WindowManagerEvent::Unmanage(_, process_id, _)
            | WindowManagerEvent::MoveStart(_, process_id, _)
            | WindowManagerEvent::MoveEnd(_, process_id, _)
            | WindowManagerEvent::ResizeStart(_, process_id, _)
            | WindowManagerEvent::ResizeStep(_, process_id, _)
            | WindowManagerEvent::ResizeEnd(_, process_id, _)
            | WindowManagerEvent::SpaceChange(_, process_id)
            | WindowManagerEvent::ScreenLock(_, process_id)
            | WindowManagerEvent::ScreenUnlock(_, process_id)
            | WindowManagerEvent::Restore(_, process_id, _) => *process_id,
        }
    }

    pub fn notification(&self) -> String {
        match self {
            WindowManagerEvent::FocusChange(n, _, _)
            | WindowManagerEvent::Show(n, _)
            | WindowManagerEvent::Destroy(n, _)
            | WindowManagerEvent::Minimize(n, _, _)
            | WindowManagerEvent::Manage(n, _, _)
            | WindowManagerEvent::MoveStart(n, _, _)
            | WindowManagerEvent::MoveEnd(n, _, _)
            | WindowManagerEvent::ResizeStart(n, _, _)
            | WindowManagerEvent::ResizeStep(n, _, _)
            | WindowManagerEvent::ResizeEnd(n, _, _)
            | WindowManagerEvent::Unmanage(n, _, _)
            | WindowManagerEvent::SpaceChange(n, _)
            | WindowManagerEvent::ScreenLock(n, _)
            | WindowManagerEvent::ScreenUnlock(n, _)
            | WindowManagerEvent::Restore(n, _, _) => match n {
                SystemNotification::Accessibility(a) => a.to_string(),
                SystemNotification::AppKitWorkspace(a) => a.to_string(),
                SystemNotification::Manual(m) => m.to_string(),
            },
        }
    }

    pub fn window_id(&self) -> Option<u32> {
        match self {
            WindowManagerEvent::FocusChange(_, _, window_id) => *window_id,
            WindowManagerEvent::Show(_, _)
            | WindowManagerEvent::Destroy(_, _)
            | WindowManagerEvent::ScreenLock(_, _)
            | WindowManagerEvent::ScreenUnlock(_, _)
            | WindowManagerEvent::SpaceChange(_, _) => None,
            WindowManagerEvent::Minimize(_, _, window_id)
            | WindowManagerEvent::Manage(_, _, window_id)
            | WindowManagerEvent::Unmanage(_, _, window_id)
            | WindowManagerEvent::MoveStart(_, _, window_id)
            | WindowManagerEvent::MoveEnd(_, _, window_id)
            | WindowManagerEvent::ResizeStart(_, _, window_id)
            | WindowManagerEvent::ResizeStep(_, _, window_id)
            | WindowManagerEvent::ResizeEnd(_, _, window_id)
            | WindowManagerEvent::Restore(_, _, window_id) => Some(*window_id),
        }
    }
}
