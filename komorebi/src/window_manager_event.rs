use std::fmt::Display;
use std::fmt::Formatter;

use serde::Deserialize;
use serde::Serialize;

use crate::OBJECT_NAME_CHANGE_ON_LAUNCH;
use crate::OBJECT_NAME_CHANGE_TITLE_IGNORE_LIST;
use crate::REGEX_IDENTIFIERS;
use crate::window::Window;
use crate::window::should_act;
use crate::winevent::WinEvent;

#[derive(Debug, Copy, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[serde(tag = "type", content = "content")]
pub enum WindowManagerEvent {
    Destroy(WinEvent, Window),
    FocusChange(WinEvent, Window),
    Hide(WinEvent, Window),
    Cloak(WinEvent, Window),
    Minimize(WinEvent, Window),
    Show(WinEvent, Window),
    Uncloak(WinEvent, Window),
    MoveResizeStart(WinEvent, Window),
    MoveResizeEnd(WinEvent, Window),
    MouseCapture(WinEvent, Window),
    Manage(Window),
    Unmanage(Window),
    Raise(Window),
    TitleUpdate(WinEvent, Window),
}

impl Display for WindowManagerEvent {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Manage(window) => {
                write!(f, "Manage (Window: {window})")
            }
            Self::Unmanage(window) => {
                write!(f, "Unmanage (Window: {window})")
            }
            Self::Destroy(winevent, window) => {
                write!(f, "Destroy (WinEvent: {winevent}, Window: {window})")
            }
            Self::FocusChange(winevent, window) => {
                write!(f, "FocusChange (WinEvent: {winevent}, Window: {window})",)
            }
            Self::Hide(winevent, window) => {
                write!(f, "Hide (WinEvent: {winevent}, Window: {window})")
            }
            Self::Cloak(winevent, window) => {
                write!(f, "Cloak (WinEvent: {winevent}, Window: {window})")
            }
            Self::Minimize(winevent, window) => {
                write!(f, "Minimize (WinEvent: {winevent}, Window: {window})")
            }
            Self::Show(winevent, window) => {
                write!(f, "Show (WinEvent: {winevent}, Window: {window})")
            }
            Self::Uncloak(winevent, window) => {
                write!(f, "Uncloak (WinEvent: {winevent}, Window: {window})")
            }
            Self::MoveResizeStart(winevent, window) => {
                write!(
                    f,
                    "MoveResizeStart (WinEvent: {winevent}, Window: {window})",
                )
            }
            Self::MoveResizeEnd(winevent, window) => {
                write!(f, "MoveResizeEnd (WinEvent: {winevent}, Window: {window})",)
            }
            Self::MouseCapture(winevent, window) => {
                write!(f, "MouseCapture (WinEvent: {winevent}, Window: {window})",)
            }
            Self::Raise(window) => {
                write!(f, "Raise (Window: {window})")
            }
            Self::TitleUpdate(winevent, window) => {
                write!(f, "TitleUpdate (WinEvent: {winevent}, Window: {window})")
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
            | Self::Cloak(_, window)
            | Self::Minimize(_, window)
            | Self::Show(_, window)
            | Self::Uncloak(_, window)
            | Self::MoveResizeStart(_, window)
            | Self::MoveResizeEnd(_, window)
            | Self::MouseCapture(_, window)
            | Self::Raise(window)
            | Self::Manage(window)
            | Self::Unmanage(window)
            | Self::TitleUpdate(_, window) => window,
        }
    }

    pub const fn hwnd(self) -> isize {
        self.window().hwnd
    }

    pub const fn title(self) -> &'static str {
        match self {
            WindowManagerEvent::Destroy(_, _) => "Destroy",
            WindowManagerEvent::FocusChange(_, _) => "FocusChange",
            WindowManagerEvent::Hide(_, _) => "Hide",
            WindowManagerEvent::Cloak(_, _) => "Cloak",
            WindowManagerEvent::Minimize(_, _) => "Minimize",
            WindowManagerEvent::Show(_, _) => "Show",
            WindowManagerEvent::Uncloak(_, _) => "Uncloak",
            WindowManagerEvent::MoveResizeStart(_, _) => "MoveResizeStart",
            WindowManagerEvent::MoveResizeEnd(_, _) => "MoveResizeEnd",
            WindowManagerEvent::MouseCapture(_, _) => "MouseCapture",
            WindowManagerEvent::Manage(_) => "Manage",
            WindowManagerEvent::Unmanage(_) => "Unmanage",
            WindowManagerEvent::Raise(_) => "Raise",
            WindowManagerEvent::TitleUpdate(_, _) => "TitleUpdate",
        }
    }

    pub fn winevent(self) -> Option<String> {
        match self {
            WindowManagerEvent::Destroy(event, _)
            | WindowManagerEvent::FocusChange(event, _)
            | WindowManagerEvent::Hide(event, _)
            | WindowManagerEvent::Cloak(event, _)
            | WindowManagerEvent::Minimize(event, _)
            | WindowManagerEvent::Show(event, _)
            | WindowManagerEvent::Uncloak(event, _)
            | WindowManagerEvent::MoveResizeStart(event, _)
            | WindowManagerEvent::MoveResizeEnd(event, _)
            | WindowManagerEvent::MouseCapture(event, _)
            | WindowManagerEvent::TitleUpdate(event, _) => Some(event.to_string()),
            WindowManagerEvent::Manage(_)
            | WindowManagerEvent::Unmanage(_)
            | WindowManagerEvent::Raise(_) => None,
        }
    }

    pub fn from_win_event(winevent: WinEvent, window: Window) -> Option<Self> {
        match winevent {
            WinEvent::ObjectDestroy => Option::from(Self::Destroy(winevent, window)),

            WinEvent::ObjectHide => Option::from(Self::Hide(winevent, window)),
            WinEvent::ObjectCloaked => Option::from(Self::Cloak(winevent, window)),

            WinEvent::SystemMinimizeStart => Option::from(Self::Minimize(winevent, window)),

            WinEvent::ObjectShow | WinEvent::SystemMinimizeEnd => {
                Option::from(Self::Show(winevent, window))
            }

            WinEvent::ObjectUncloaked => Option::from(Self::Uncloak(winevent, window)),

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
                let object_name_change_title_ignore_list =
                    OBJECT_NAME_CHANGE_TITLE_IGNORE_LIST.lock();
                let regex_identifiers = REGEX_IDENTIFIERS.lock();

                let title = &window.title().ok()?;
                let exe_name = &window.exe().ok()?;
                let class = &window.class().ok()?;
                let path = &window.path().ok()?;

                let mut should_trigger_show = should_act(
                    title,
                    exe_name,
                    class,
                    path,
                    &object_name_change_on_launch,
                    &regex_identifiers,
                )
                .is_some();

                if should_trigger_show {
                    for r in &*object_name_change_title_ignore_list {
                        if r.is_match(title) {
                            should_trigger_show = false;
                        }
                    }
                }

                // should not trigger show on minimized windows, for example when firefox sends
                // this message due to youtube autoplay changing the window title
                // https://github.com/LGUG2Z/komorebi/issues/941
                if should_trigger_show && !window.is_miminized() {
                    Option::from(Self::Show(winevent, window))
                } else {
                    Option::from(Self::TitleUpdate(winevent, window))
                }
            }
            _ => None,
        }
    }
}
