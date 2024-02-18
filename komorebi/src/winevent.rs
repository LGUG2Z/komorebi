#![allow(clippy::use_self)]

use schemars::JsonSchema;
use serde::Deserialize;
use serde::Serialize;
use strum::Display;
use windows::Win32::UI::WindowsAndMessaging::EVENT_AIA_END;
use windows::Win32::UI::WindowsAndMessaging::EVENT_AIA_START;
use windows::Win32::UI::WindowsAndMessaging::EVENT_CONSOLE_CARET;
use windows::Win32::UI::WindowsAndMessaging::EVENT_CONSOLE_END;
use windows::Win32::UI::WindowsAndMessaging::EVENT_CONSOLE_END_APPLICATION;
use windows::Win32::UI::WindowsAndMessaging::EVENT_CONSOLE_LAYOUT;
use windows::Win32::UI::WindowsAndMessaging::EVENT_CONSOLE_START_APPLICATION;
use windows::Win32::UI::WindowsAndMessaging::EVENT_CONSOLE_UPDATE_REGION;
use windows::Win32::UI::WindowsAndMessaging::EVENT_CONSOLE_UPDATE_SCROLL;
use windows::Win32::UI::WindowsAndMessaging::EVENT_CONSOLE_UPDATE_SIMPLE;
use windows::Win32::UI::WindowsAndMessaging::EVENT_OBJECT_ACCELERATORCHANGE;
use windows::Win32::UI::WindowsAndMessaging::EVENT_OBJECT_CLOAKED;
use windows::Win32::UI::WindowsAndMessaging::EVENT_OBJECT_CONTENTSCROLLED;
use windows::Win32::UI::WindowsAndMessaging::EVENT_OBJECT_CREATE;
use windows::Win32::UI::WindowsAndMessaging::EVENT_OBJECT_DEFACTIONCHANGE;
use windows::Win32::UI::WindowsAndMessaging::EVENT_OBJECT_DESCRIPTIONCHANGE;
use windows::Win32::UI::WindowsAndMessaging::EVENT_OBJECT_DESTROY;
use windows::Win32::UI::WindowsAndMessaging::EVENT_OBJECT_DRAGCANCEL;
use windows::Win32::UI::WindowsAndMessaging::EVENT_OBJECT_DRAGCOMPLETE;
use windows::Win32::UI::WindowsAndMessaging::EVENT_OBJECT_DRAGDROPPED;
use windows::Win32::UI::WindowsAndMessaging::EVENT_OBJECT_DRAGENTER;
use windows::Win32::UI::WindowsAndMessaging::EVENT_OBJECT_DRAGLEAVE;
use windows::Win32::UI::WindowsAndMessaging::EVENT_OBJECT_DRAGSTART;
use windows::Win32::UI::WindowsAndMessaging::EVENT_OBJECT_END;
use windows::Win32::UI::WindowsAndMessaging::EVENT_OBJECT_FOCUS;
use windows::Win32::UI::WindowsAndMessaging::EVENT_OBJECT_HELPCHANGE;
use windows::Win32::UI::WindowsAndMessaging::EVENT_OBJECT_HIDE;
use windows::Win32::UI::WindowsAndMessaging::EVENT_OBJECT_HOSTEDOBJECTSINVALIDATED;
use windows::Win32::UI::WindowsAndMessaging::EVENT_OBJECT_IME_CHANGE;
use windows::Win32::UI::WindowsAndMessaging::EVENT_OBJECT_IME_HIDE;
use windows::Win32::UI::WindowsAndMessaging::EVENT_OBJECT_IME_SHOW;
use windows::Win32::UI::WindowsAndMessaging::EVENT_OBJECT_INVOKED;
use windows::Win32::UI::WindowsAndMessaging::EVENT_OBJECT_LIVEREGIONCHANGED;
use windows::Win32::UI::WindowsAndMessaging::EVENT_OBJECT_LOCATIONCHANGE;
use windows::Win32::UI::WindowsAndMessaging::EVENT_OBJECT_NAMECHANGE;
use windows::Win32::UI::WindowsAndMessaging::EVENT_OBJECT_PARENTCHANGE;
use windows::Win32::UI::WindowsAndMessaging::EVENT_OBJECT_REORDER;
use windows::Win32::UI::WindowsAndMessaging::EVENT_OBJECT_SELECTION;
use windows::Win32::UI::WindowsAndMessaging::EVENT_OBJECT_SELECTIONADD;
use windows::Win32::UI::WindowsAndMessaging::EVENT_OBJECT_SELECTIONREMOVE;
use windows::Win32::UI::WindowsAndMessaging::EVENT_OBJECT_SELECTIONWITHIN;
use windows::Win32::UI::WindowsAndMessaging::EVENT_OBJECT_SHOW;
use windows::Win32::UI::WindowsAndMessaging::EVENT_OBJECT_STATECHANGE;
use windows::Win32::UI::WindowsAndMessaging::EVENT_OBJECT_TEXTEDIT_CONVERSIONTARGETCHANGED;
use windows::Win32::UI::WindowsAndMessaging::EVENT_OBJECT_TEXTSELECTIONCHANGED;
use windows::Win32::UI::WindowsAndMessaging::EVENT_OBJECT_UNCLOAKED;
use windows::Win32::UI::WindowsAndMessaging::EVENT_OBJECT_VALUECHANGE;
use windows::Win32::UI::WindowsAndMessaging::EVENT_OEM_DEFINED_END;
use windows::Win32::UI::WindowsAndMessaging::EVENT_OEM_DEFINED_START;
use windows::Win32::UI::WindowsAndMessaging::EVENT_SYSTEM_ALERT;
use windows::Win32::UI::WindowsAndMessaging::EVENT_SYSTEM_ARRANGMENTPREVIEW;
use windows::Win32::UI::WindowsAndMessaging::EVENT_SYSTEM_CAPTUREEND;
use windows::Win32::UI::WindowsAndMessaging::EVENT_SYSTEM_CAPTURESTART;
use windows::Win32::UI::WindowsAndMessaging::EVENT_SYSTEM_CONTEXTHELPEND;
use windows::Win32::UI::WindowsAndMessaging::EVENT_SYSTEM_CONTEXTHELPSTART;
use windows::Win32::UI::WindowsAndMessaging::EVENT_SYSTEM_DESKTOPSWITCH;
use windows::Win32::UI::WindowsAndMessaging::EVENT_SYSTEM_DIALOGEND;
use windows::Win32::UI::WindowsAndMessaging::EVENT_SYSTEM_DIALOGSTART;
use windows::Win32::UI::WindowsAndMessaging::EVENT_SYSTEM_DRAGDROPEND;
use windows::Win32::UI::WindowsAndMessaging::EVENT_SYSTEM_DRAGDROPSTART;
use windows::Win32::UI::WindowsAndMessaging::EVENT_SYSTEM_END;
use windows::Win32::UI::WindowsAndMessaging::EVENT_SYSTEM_FOREGROUND;
use windows::Win32::UI::WindowsAndMessaging::EVENT_SYSTEM_IME_KEY_NOTIFICATION;
use windows::Win32::UI::WindowsAndMessaging::EVENT_SYSTEM_MENUEND;
use windows::Win32::UI::WindowsAndMessaging::EVENT_SYSTEM_MENUPOPUPEND;
use windows::Win32::UI::WindowsAndMessaging::EVENT_SYSTEM_MENUPOPUPSTART;
use windows::Win32::UI::WindowsAndMessaging::EVENT_SYSTEM_MENUSTART;
use windows::Win32::UI::WindowsAndMessaging::EVENT_SYSTEM_MINIMIZEEND;
use windows::Win32::UI::WindowsAndMessaging::EVENT_SYSTEM_MINIMIZESTART;
use windows::Win32::UI::WindowsAndMessaging::EVENT_SYSTEM_MOVESIZEEND;
use windows::Win32::UI::WindowsAndMessaging::EVENT_SYSTEM_MOVESIZESTART;
use windows::Win32::UI::WindowsAndMessaging::EVENT_SYSTEM_SCROLLINGEND;
use windows::Win32::UI::WindowsAndMessaging::EVENT_SYSTEM_SCROLLINGSTART;
use windows::Win32::UI::WindowsAndMessaging::EVENT_SYSTEM_SOUND;
use windows::Win32::UI::WindowsAndMessaging::EVENT_SYSTEM_SWITCHEND;
use windows::Win32::UI::WindowsAndMessaging::EVENT_SYSTEM_SWITCHER_APPDROPPED;
use windows::Win32::UI::WindowsAndMessaging::EVENT_SYSTEM_SWITCHER_APPGRABBED;
use windows::Win32::UI::WindowsAndMessaging::EVENT_SYSTEM_SWITCHER_APPOVERTARGET;
use windows::Win32::UI::WindowsAndMessaging::EVENT_SYSTEM_SWITCHER_CANCELLED;
use windows::Win32::UI::WindowsAndMessaging::EVENT_SYSTEM_SWITCHSTART;
use windows::Win32::UI::WindowsAndMessaging::EVENT_UIA_EVENTID_END;
use windows::Win32::UI::WindowsAndMessaging::EVENT_UIA_EVENTID_START;
use windows::Win32::UI::WindowsAndMessaging::EVENT_UIA_PROPID_END;
use windows::Win32::UI::WindowsAndMessaging::EVENT_UIA_PROPID_START;

#[derive(Clone, Copy, PartialEq, Eq, Debug, Serialize, Deserialize, Display, JsonSchema)]
#[repr(u32)]
#[allow(dead_code)]
pub enum WinEvent {
    AiaEnd = EVENT_AIA_END,
    AiaStart = EVENT_AIA_START,
    ConsoleCaret = EVENT_CONSOLE_CARET,
    ConsoleEnd = EVENT_CONSOLE_END,
    ConsoleEndApplication = EVENT_CONSOLE_END_APPLICATION,
    ConsoleLayout = EVENT_CONSOLE_LAYOUT,
    ConsoleStartApplication = EVENT_CONSOLE_START_APPLICATION,
    ConsoleUpdateRegion = EVENT_CONSOLE_UPDATE_REGION,
    ConsoleUpdateScroll = EVENT_CONSOLE_UPDATE_SCROLL,
    ConsoleUpdateSimple = EVENT_CONSOLE_UPDATE_SIMPLE,
    ObjectAcceleratorChange = EVENT_OBJECT_ACCELERATORCHANGE,
    ObjectCloaked = EVENT_OBJECT_CLOAKED,
    ObjectContentScrolled = EVENT_OBJECT_CONTENTSCROLLED,
    ObjectCreate = EVENT_OBJECT_CREATE,
    ObjectDefActionChange = EVENT_OBJECT_DEFACTIONCHANGE,
    ObjectDescriptionChange = EVENT_OBJECT_DESCRIPTIONCHANGE,
    ObjectDestroy = EVENT_OBJECT_DESTROY,
    ObjectDragCancel = EVENT_OBJECT_DRAGCANCEL,
    ObjectDragComplete = EVENT_OBJECT_DRAGCOMPLETE,
    ObjectDragDropped = EVENT_OBJECT_DRAGDROPPED,
    ObjectDragEnter = EVENT_OBJECT_DRAGENTER,
    ObjectDragLeave = EVENT_OBJECT_DRAGLEAVE,
    ObjectDragStart = EVENT_OBJECT_DRAGSTART,
    ObjectEnd = EVENT_OBJECT_END,
    ObjectFocus = EVENT_OBJECT_FOCUS,
    ObjectHelpChange = EVENT_OBJECT_HELPCHANGE,
    ObjectHide = EVENT_OBJECT_HIDE,
    ObjectHostedObjectsInvalidated = EVENT_OBJECT_HOSTEDOBJECTSINVALIDATED,
    ObjectImeChange = EVENT_OBJECT_IME_CHANGE,
    ObjectImeHide = EVENT_OBJECT_IME_HIDE,
    ObjectImeShow = EVENT_OBJECT_IME_SHOW,
    ObjectInvoked = EVENT_OBJECT_INVOKED,
    ObjectLiveRegionChanged = EVENT_OBJECT_LIVEREGIONCHANGED,
    ObjectLocationChange = EVENT_OBJECT_LOCATIONCHANGE,
    ObjectNameChange = EVENT_OBJECT_NAMECHANGE,
    ObjectParentChange = EVENT_OBJECT_PARENTCHANGE,
    ObjectReorder = EVENT_OBJECT_REORDER,
    ObjectSelection = EVENT_OBJECT_SELECTION,
    ObjectSelectionAdd = EVENT_OBJECT_SELECTIONADD,
    ObjectSelectionRemove = EVENT_OBJECT_SELECTIONREMOVE,
    ObjectSelectionWithin = EVENT_OBJECT_SELECTIONWITHIN,
    ObjectShow = EVENT_OBJECT_SHOW,
    ObjectStateChange = EVENT_OBJECT_STATECHANGE,
    ObjectTextEditConversionTargetChanged = EVENT_OBJECT_TEXTEDIT_CONVERSIONTARGETCHANGED,
    ObjectTextSelectionChanged = EVENT_OBJECT_TEXTSELECTIONCHANGED,
    ObjectUncloaked = EVENT_OBJECT_UNCLOAKED,
    ObjectValueChange = EVENT_OBJECT_VALUECHANGE,
    OemDefinedEnd = EVENT_OEM_DEFINED_END,
    OemDefinedStart = EVENT_OEM_DEFINED_START,
    SystemAlert = EVENT_SYSTEM_ALERT,
    SystemArrangementPreview = EVENT_SYSTEM_ARRANGMENTPREVIEW,
    SystemCaptureEnd = EVENT_SYSTEM_CAPTUREEND,
    SystemCaptureStart = EVENT_SYSTEM_CAPTURESTART,
    SystemContextHelpEnd = EVENT_SYSTEM_CONTEXTHELPEND,
    SystemContextHelpStart = EVENT_SYSTEM_CONTEXTHELPSTART,
    SystemDesktopSwitch = EVENT_SYSTEM_DESKTOPSWITCH,
    SystemDialogEnd = EVENT_SYSTEM_DIALOGEND,
    SystemDialogStart = EVENT_SYSTEM_DIALOGSTART,
    SystemDragDropEnd = EVENT_SYSTEM_DRAGDROPEND,
    SystemDragDropStart = EVENT_SYSTEM_DRAGDROPSTART,
    SystemEnd = EVENT_SYSTEM_END,
    SystemForeground = EVENT_SYSTEM_FOREGROUND,
    SystemImeKeyNotification = EVENT_SYSTEM_IME_KEY_NOTIFICATION,
    SystemMenuEnd = EVENT_SYSTEM_MENUEND,
    SystemMenuPopupEnd = EVENT_SYSTEM_MENUPOPUPEND,
    SystemMenuPopupStart = EVENT_SYSTEM_MENUPOPUPSTART,
    SystemMenuStart = EVENT_SYSTEM_MENUSTART,
    SystemMinimizeEnd = EVENT_SYSTEM_MINIMIZEEND,
    SystemMinimizeStart = EVENT_SYSTEM_MINIMIZESTART,
    SystemMoveSizeEnd = EVENT_SYSTEM_MOVESIZEEND,
    SystemMoveSizeStart = EVENT_SYSTEM_MOVESIZESTART,
    SystemScrollingEnd = EVENT_SYSTEM_SCROLLINGEND,
    SystemScrollingStart = EVENT_SYSTEM_SCROLLINGSTART,
    SystemSound = EVENT_SYSTEM_SOUND,
    SystemSwitchEnd = EVENT_SYSTEM_SWITCHEND,
    SystemSwitchStart = EVENT_SYSTEM_SWITCHSTART,
    SystemSwitcherAppDropped = EVENT_SYSTEM_SWITCHER_APPDROPPED,
    SystemSwitcherAppGrabbed = EVENT_SYSTEM_SWITCHER_APPGRABBED,
    SystemSwitcherAppOverTarget = EVENT_SYSTEM_SWITCHER_APPOVERTARGET,
    SystemSwitcherCancelled = EVENT_SYSTEM_SWITCHER_CANCELLED,
    UiaEventIdSEnd = EVENT_UIA_EVENTID_END,
    UiaEventIdStart = EVENT_UIA_EVENTID_START,
    UiaPropIdSEnd = EVENT_UIA_PROPID_END,
    UiaPropIdStart = EVENT_UIA_PROPID_START,
}
