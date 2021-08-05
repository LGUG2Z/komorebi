use std::fmt::Display;
use std::fmt::Formatter;

use crate::window::Window;
use crate::winevent::WinEvent;

#[derive(Debug, Copy, Clone)]
pub enum WindowManagerEvent {
    Destroy(WinEvent, Window),
    FocusChange(WinEvent, Window),
    Hide(WinEvent, Window),
    Minimize(WinEvent, Window),
    Show(WinEvent, Window),
    MoveResizeStart(WinEvent, Window),
    MoveResizeEnd(WinEvent, Window),
    MouseCapture(WinEvent, Window),
}

impl Display for WindowManagerEvent {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
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
            | WindowManagerEvent::MouseCapture(_, window) => window,
        }
    }

    pub const fn from_win_event(winevent: WinEvent, window: Window) -> Option<Self> {
        match winevent {
            WinEvent::ObjectDestroy => Some(Self::Destroy(winevent, window)),

            WinEvent::ObjectCloaked | WinEvent::ObjectHide => Some(Self::Hide(winevent, window)),

            WinEvent::SystemMinimizeStart => Some(Self::Minimize(winevent, window)),

            WinEvent::ObjectShow | WinEvent::ObjectUncloaked | WinEvent::SystemMinimizeEnd => {
                Some(Self::Show(winevent, window))
            }

            WinEvent::ObjectFocus | WinEvent::SystemForeground => {
                Some(Self::FocusChange(winevent, window))
            }
            WinEvent::SystemMoveSizeStart => Some(Self::MoveResizeStart(winevent, window)),
            WinEvent::SystemMoveSizeEnd => Some(Self::MoveResizeEnd(winevent, window)),
            WinEvent::SystemCaptureStart | WinEvent::SystemCaptureEnd => {
                Some(Self::MouseCapture(winevent, window))
            }
            _ => None,
        }
    }
}
