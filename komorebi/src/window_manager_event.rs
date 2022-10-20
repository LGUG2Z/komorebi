use std::fmt::Display;
use std::fmt::Formatter;

use schemars::JsonSchema;
use serde::Serialize;

use crate::window::Window;
use crate::winevent::WinEvent;
use crate::OBJECT_NAME_CHANGE_ON_LAUNCH;

#[derive(Debug, Copy, Clone, Serialize, JsonSchema)]
#[serde(tag = "type", content = "content")]
pub enum WindowManagerEvent {
    Destroy(WinEvent, Window),
    FocusChange(WinEvent, Window),
    Hide(WinEvent, Window),
    Minimize(WinEvent, Window),
    Show(WinEvent, Window),
    MoveResizeStart(WinEvent, Window),
    MoveResizeEnd(WinEvent, Window),
    MouseCapture(WinEvent, Window),
    Manage(Window),
    Unmanage(Window),
    Raise(Window),
    DisplayChange(Window),
}

impl Display for WindowManagerEvent {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Manage(window) => {
                write!(f, "Manage (Window: {})", window)
            }
            Self::Unmanage(window) => {
                write!(f, "Unmanage (Window: {})", window)
            }
            Self::Destroy(winevent, window) => {
                write!(f, "Destroy (WinEvent: {}, Window: {})", winevent, window)
            }
            Self::FocusChange(winevent, window) => {
                write!(
                    f,
                    "FocusChange (WinEvent: {}, Window: {})",
                    winevent, window
                )
            }
            Self::Hide(winevent, window) => {
                write!(f, "Hide (WinEvent: {}, Window: {})", winevent, window)
            }
            Self::Minimize(winevent, window) => {
                write!(f, "Minimize (WinEvent: {}, Window: {})", winevent, window)
            }
            Self::Show(winevent, window) => {
                write!(f, "Show (WinEvent: {}, Window: {})", winevent, window)
            }
            Self::MoveResizeStart(winevent, window) => {
                write!(
                    f,
                    "MoveResizeStart (WinEvent: {}, Window: {})",
                    winevent, window
                )
            }
            Self::MoveResizeEnd(winevent, window) => {
                write!(
                    f,
                    "MoveResizeEnd (WinEvent: {}, Window: {})",
                    winevent, window
                )
            }
            Self::MouseCapture(winevent, window) => {
                write!(
                    f,
                    "MouseCapture (WinEvent: {}, Window: {})",
                    winevent, window
                )
            }
            Self::Raise(window) => {
                write!(f, "Raise (Window: {})", window)
            }
            Self::DisplayChange(window) => {
                write!(f, "DisplayChange (Window: {})", window)
            }
        }
    }
}

impl WindowManagerEvent {
    pub const fn window(self) -> Window {
        match self {
            Self::Destroy(_, window)
            | Self::FocusChange(_, window)
            | Self::Hide(_, window)
            | Self::Minimize(_, window)
            | Self::Show(_, window)
            | Self::MoveResizeStart(_, window)
            | Self::MoveResizeEnd(_, window)
            | Self::MouseCapture(_, window)
            | Self::Raise(window)
            | Self::Manage(window)
            | Self::DisplayChange(window)
            | Self::Unmanage(window) => window,
        }
    }

    pub fn from_win_event(winevent: WinEvent, window: Window) -> Option<Self> {
        match winevent {
            WinEvent::ObjectDestroy => Option::from(Self::Destroy(winevent, window)),

            WinEvent::ObjectCloaked | WinEvent::ObjectHide => {
                Option::from(Self::Hide(winevent, window))
            }

            WinEvent::SystemMinimizeStart => Option::from(Self::Minimize(winevent, window)),

            WinEvent::ObjectShow | WinEvent::ObjectUncloaked | WinEvent::SystemMinimizeEnd => {
                Option::from(Self::Show(winevent, window))
            }

            WinEvent::ObjectFocus | WinEvent::SystemForeground => {
                Option::from(Self::FocusChange(winevent, window))
            }
            WinEvent::SystemMoveSizeStart => Option::from(Self::MoveResizeStart(winevent, window)),
            WinEvent::SystemMoveSizeEnd => Option::from(Self::MoveResizeEnd(winevent, window)),
            WinEvent::SystemCaptureStart | WinEvent::SystemCaptureEnd => {
                Option::from(Self::MouseCapture(winevent, window))
            }
            WinEvent::ObjectNameChange => {
                // Some apps like Firefox don't send ObjectCreate or ObjectShow on launch
                // This spams the message queue, but I don't know what else to do. On launch
                // it only sends the following WinEvents :/
                //
                // [yatta\src\windows_event.rs:110] event = 32780 ObjectNameChange
                // [yatta\src\windows_event.rs:110] event = 32779 ObjectLocationChange

                let object_name_change_on_launch = OBJECT_NAME_CHANGE_ON_LAUNCH.lock();

                if object_name_change_on_launch.contains(&window.exe().ok()?)
                    || object_name_change_on_launch.contains(&window.class().ok()?)
                    || object_name_change_on_launch.contains(&window.title().ok()?)
                {
                    Option::from(Self::Show(winevent, window))
                } else {
                    None
                }
            }
            _ => None,
        }
    }
}
