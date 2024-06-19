use chrono::DateTime;
use chrono::Utc;
use std::fmt::Display;
use std::fmt::Formatter;

use schemars::JsonSchema;
use serde::Deserialize;
use serde::Serialize;

use crate::window::should_act;
use crate::window::Window;
use crate::winevent::WinEvent;
use crate::OBJECT_NAME_CHANGE_ON_LAUNCH;
use crate::REGEX_IDENTIFIERS;

#[derive(Debug, Copy, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "type", content = "content")]
pub enum WindowManagerEvent {
    Destroy(WinEvent, Window, DateTime<Utc>),
    FocusChange(WinEvent, Window, DateTime<Utc>),
    Hide(WinEvent, Window, DateTime<Utc>),
    Cloak(WinEvent, Window, DateTime<Utc>),
    Minimize(WinEvent, Window, DateTime<Utc>),
    Show(WinEvent, Window, DateTime<Utc>),
    Uncloak(WinEvent, Window, DateTime<Utc>),
    MoveResizeStart(WinEvent, Window, DateTime<Utc>),
    MoveResizeEnd(WinEvent, Window, DateTime<Utc>),
    MouseCapture(WinEvent, Window, DateTime<Utc>),
    Manage(Window, DateTime<Utc>),
    Unmanage(Window, DateTime<Utc>),
    Raise(Window, DateTime<Utc>),
    TitleUpdate(WinEvent, Window, DateTime<Utc>),
}

impl Display for WindowManagerEvent {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Manage(window, _) => {
                write!(f, "Manage (Window: {window})")
            }
            Self::Unmanage(window, _) => {
                write!(f, "Unmanage (Window: {window})")
            }
            Self::Destroy(winevent, window, _) => {
                write!(f, "Destroy (WinEvent: {winevent}, Window: {window})")
            }
            Self::FocusChange(winevent, window, _) => {
                write!(f, "FocusChange (WinEvent: {winevent}, Window: {window})",)
            }
            Self::Hide(winevent, window, _) => {
                write!(f, "Hide (WinEvent: {winevent}, Window: {window})")
            }
            Self::Cloak(winevent, window, _) => {
                write!(f, "Cloak (WinEvent: {winevent}, Window: {window})")
            }
            Self::Minimize(winevent, window, _) => {
                write!(f, "Minimize (WinEvent: {winevent}, Window: {window})")
            }
            Self::Show(winevent, window, _) => {
                write!(f, "Show (WinEvent: {winevent}, Window: {window})")
            }
            Self::Uncloak(winevent, window, _) => {
                write!(f, "Uncloak (WinEvent: {winevent}, Window: {window})")
            }
            Self::MoveResizeStart(winevent, window, _) => {
                write!(
                    f,
                    "MoveResizeStart (WinEvent: {winevent}, Window: {window})",
                )
            }
            Self::MoveResizeEnd(winevent, window, _) => {
                write!(f, "MoveResizeEnd (WinEvent: {winevent}, Window: {window})",)
            }
            Self::MouseCapture(winevent, window, _) => {
                write!(f, "MouseCapture (WinEvent: {winevent}, Window: {window})",)
            }
            Self::Raise(window, _) => {
                write!(f, "Raise (Window: {window})")
            }
            Self::TitleUpdate(winevent, window, _) => {
                write!(f, "TitleUpdate (WinEvent: {winevent}, Window: {window})")
            }
        }
    }
}

impl WindowManagerEvent {
    pub const fn timestamp(self) -> DateTime<Utc> {
        match self {
            WindowManagerEvent::Destroy(_, _, timestamp)
            | WindowManagerEvent::FocusChange(_, _, timestamp)
            | WindowManagerEvent::Hide(_, _, timestamp)
            | WindowManagerEvent::Cloak(_, _, timestamp)
            | WindowManagerEvent::Minimize(_, _, timestamp)
            | WindowManagerEvent::Show(_, _, timestamp)
            | WindowManagerEvent::Uncloak(_, _, timestamp)
            | WindowManagerEvent::MoveResizeStart(_, _, timestamp)
            | WindowManagerEvent::MoveResizeEnd(_, _, timestamp)
            | WindowManagerEvent::MouseCapture(_, _, timestamp)
            | WindowManagerEvent::Manage(_, timestamp)
            | WindowManagerEvent::Unmanage(_, timestamp)
            | WindowManagerEvent::Raise(_, timestamp)
            | WindowManagerEvent::TitleUpdate(_, _, timestamp) => timestamp,
        }
    }
    pub const fn window(self) -> Window {
        match self {
            Self::Destroy(_, window, _)
            | Self::FocusChange(_, window, _)
            | Self::Hide(_, window, _)
            | Self::Cloak(_, window, _)
            | Self::Minimize(_, window, _)
            | Self::Show(_, window, _)
            | Self::Uncloak(_, window, _)
            | Self::MoveResizeStart(_, window, _)
            | Self::MoveResizeEnd(_, window, _)
            | Self::MouseCapture(_, window, _)
            | Self::Raise(window, _)
            | Self::Manage(window, _)
            | Self::Unmanage(window, _)
            | Self::TitleUpdate(_, window, _) => window,
        }
    }

    pub fn from_win_event(
        winevent: WinEvent,
        window: Window,
        timestamp: DateTime<Utc>,
    ) -> Option<Self> {
        match winevent {
            WinEvent::ObjectDestroy => Option::from(Self::Destroy(winevent, window, timestamp)),

            WinEvent::ObjectHide => Option::from(Self::Hide(winevent, window, timestamp)),
            WinEvent::ObjectCloaked => Option::from(Self::Cloak(winevent, window, timestamp)),

            WinEvent::SystemMinimizeStart => {
                Option::from(Self::Minimize(winevent, window, timestamp))
            }

            WinEvent::ObjectShow | WinEvent::SystemMinimizeEnd => {
                Option::from(Self::Show(winevent, window, timestamp))
            }

            WinEvent::ObjectUncloaked => Option::from(Self::Uncloak(winevent, window, timestamp)),

            WinEvent::ObjectFocus | WinEvent::SystemForeground => {
                Option::from(Self::FocusChange(winevent, window, timestamp))
            }
            WinEvent::SystemMoveSizeStart => {
                Option::from(Self::MoveResizeStart(winevent, window, timestamp))
            }
            WinEvent::SystemMoveSizeEnd => {
                Option::from(Self::MoveResizeEnd(winevent, window, timestamp))
            }
            WinEvent::SystemCaptureStart | WinEvent::SystemCaptureEnd => {
                Option::from(Self::MouseCapture(winevent, window, timestamp))
            }
            WinEvent::ObjectNameChange => {
                // Some apps like Firefox don't send ObjectCreate or ObjectShow on launch
                // This spams the message queue, but I don't know what else to do. On launch
                // it only sends the following WinEvents :/
                //
                // [yatta\src\windows_event.rs:110] event = 32780 ObjectNameChange
                // [yatta\src\windows_event.rs:110] event = 32779 ObjectLocationChange

                let object_name_change_on_launch = OBJECT_NAME_CHANGE_ON_LAUNCH.lock();
                let regex_identifiers = REGEX_IDENTIFIERS.lock();

                let title = &window.title().ok()?;
                let exe_name = &window.exe().ok()?;
                let class = &window.class().ok()?;
                let path = &window.path().ok()?;

                let should_trigger_show = should_act(
                    title,
                    exe_name,
                    class,
                    path,
                    &object_name_change_on_launch,
                    &regex_identifiers,
                )
                .is_some();

                if should_trigger_show {
                    Option::from(Self::Show(winevent, window, timestamp))
                } else {
                    Option::from(Self::TitleUpdate(winevent, window, timestamp))
                }
            }
            _ => None,
        }
    }
}
