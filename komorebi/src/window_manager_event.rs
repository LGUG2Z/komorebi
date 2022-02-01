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
    MonitorPoll(WinEvent, Window),
}

impl Display for WindowManagerEvent {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            WindowManagerEvent::Manage(window) => {
                write!(f, "Manage (Window: {})", window)
            }
            WindowManagerEvent::Unmanage(window) => {
                write!(f, "Unmanage (Window: {})", window)
            }
            WindowManagerEvent::Destroy(winevent, window) => {
                write!(f, "Destroy (WinEvent: {}, Window: {})", winevent, window)
            }
            WindowManagerEvent::FocusChange(winevent, window) => {
                write!(
                    f,
                    "FocusChange (WinEvent: {}, Window: {})",
                    winevent, window
                )
            }
            WindowManagerEvent::Hide(winevent, window) => {
                write!(f, "Hide (WinEvent: {}, Window: {})", winevent, window)
            }
            WindowManagerEvent::Minimize(winevent, window) => {
                write!(f, "Minimize (WinEvent: {}, Window: {})", winevent, window)
            }
            WindowManagerEvent::Show(winevent, window) => {
                write!(f, "Show (WinEvent: {}, Window: {})", winevent, window)
            }
            WindowManagerEvent::MoveResizeStart(winevent, window) => {
                write!(
                    f,
                    "MoveResizeStart (WinEvent: {}, Window: {})",
                    winevent, window
                )
            }
            WindowManagerEvent::MoveResizeEnd(winevent, window) => {
                write!(
                    f,
                    "MoveResizeEnd (WinEvent: {}, Window: {})",
                    winevent, window
                )
            }
            WindowManagerEvent::MouseCapture(winevent, window) => {
                write!(
                    f,
                    "MouseCapture (WinEvent: {}, Window: {})",
                    winevent, window
                )
            }
            WindowManagerEvent::Raise(window) => {
                write!(f, "Raise (Window: {})", window)
            }
            WindowManagerEvent::MonitorPoll(winevent, window) => {
                write!(
                    f,
                    "MonitorPoll (WinEvent: {}, Window: {})",
                    winevent, window
                )
            }
        }
    }
}

impl WindowManagerEvent {
    pub const fn window(self) -> Window {
        match self {
            WindowManagerEvent::Destroy(_, window)
            | WindowManagerEvent::FocusChange(_, window)
            | WindowManagerEvent::Hide(_, window)
            | WindowManagerEvent::Minimize(_, window)
            | WindowManagerEvent::Show(_, window)
            | WindowManagerEvent::MoveResizeStart(_, window)
            | WindowManagerEvent::MoveResizeEnd(_, window)
            | WindowManagerEvent::MouseCapture(_, window)
            | WindowManagerEvent::MonitorPoll(_, window)
            | WindowManagerEvent::Raise(window)
            | WindowManagerEvent::Manage(window)
            | WindowManagerEvent::Unmanage(window) => window,
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

                if object_name_change_on_launch.contains(&window.exe().ok()?) {
                    Option::from(Self::Show(winevent, window))
                } else {
                    None
                }
            }
            WinEvent::ObjectCreate => {
                if let Ok(title) = window.title() {
                    // Hidden COM support mechanism window that fires this event on both DPI/scaling
                    // changes and resolution changes, a good candidate for polling
                    if title == "OLEChannelWnd" {
                        return Option::from(Self::MonitorPoll(winevent, window));
                    }
                }

                None
            }
            _ => None,
        }
    }
}
