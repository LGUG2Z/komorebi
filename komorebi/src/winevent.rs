use bindings::Windows::Win32::UI::WindowsAndMessaging::EVENT_AIA_END;
use bindings::Windows::Win32::UI::WindowsAndMessaging::EVENT_AIA_START;
use bindings::Windows::Win32::UI::WindowsAndMessaging::EVENT_CONSOLE_CARET;
use bindings::Windows::Win32::UI::WindowsAndMessaging::EVENT_CONSOLE_END;
use bindings::Windows::Win32::UI::WindowsAndMessaging::EVENT_CONSOLE_END_APPLICATION;
use bindings::Windows::Win32::UI::WindowsAndMessaging::EVENT_CONSOLE_LAYOUT;
use bindings::Windows::Win32::UI::WindowsAndMessaging::EVENT_CONSOLE_START_APPLICATION;
use bindings::Windows::Win32::UI::WindowsAndMessaging::EVENT_CONSOLE_UPDATE_REGION;
use bindings::Windows::Win32::UI::WindowsAndMessaging::EVENT_CONSOLE_UPDATE_SCROLL;
use bindings::Windows::Win32::UI::WindowsAndMessaging::EVENT_CONSOLE_UPDATE_SIMPLE;
use bindings::Windows::Win32::UI::WindowsAndMessaging::EVENT_OBJECT_ACCELERATORCHANGE;
use bindings::Windows::Win32::UI::WindowsAndMessaging::EVENT_OBJECT_CLOAKED;
use bindings::Windows::Win32::UI::WindowsAndMessaging::EVENT_OBJECT_CONTENTSCROLLED;
use bindings::Windows::Win32::UI::WindowsAndMessaging::EVENT_OBJECT_CREATE;
use bindings::Windows::Win32::UI::WindowsAndMessaging::EVENT_OBJECT_DEFACTIONCHANGE;
use bindings::Windows::Win32::UI::WindowsAndMessaging::EVENT_OBJECT_DESCRIPTIONCHANGE;
use bindings::Windows::Win32::UI::WindowsAndMessaging::EVENT_OBJECT_DESTROY;
use bindings::Windows::Win32::UI::WindowsAndMessaging::EVENT_OBJECT_DRAGCANCEL;
use bindings::Windows::Win32::UI::WindowsAndMessaging::EVENT_OBJECT_DRAGCOMPLETE;
use bindings::Windows::Win32::UI::WindowsAndMessaging::EVENT_OBJECT_DRAGDROPPED;
use bindings::Windows::Win32::UI::WindowsAndMessaging::EVENT_OBJECT_DRAGENTER;
use bindings::Windows::Win32::UI::WindowsAndMessaging::EVENT_OBJECT_DRAGLEAVE;
use bindings::Windows::Win32::UI::WindowsAndMessaging::EVENT_OBJECT_DRAGSTART;
use bindings::Windows::Win32::UI::WindowsAndMessaging::EVENT_OBJECT_END;
use bindings::Windows::Win32::UI::WindowsAndMessaging::EVENT_OBJECT_FOCUS;
use bindings::Windows::Win32::UI::WindowsAndMessaging::EVENT_OBJECT_HELPCHANGE;
use bindings::Windows::Win32::UI::WindowsAndMessaging::EVENT_OBJECT_HIDE;
use bindings::Windows::Win32::UI::WindowsAndMessaging::EVENT_OBJECT_HOSTEDOBJECTSINVALIDATED;
use bindings::Windows::Win32::UI::WindowsAndMessaging::EVENT_OBJECT_IME_CHANGE;
use bindings::Windows::Win32::UI::WindowsAndMessaging::EVENT_OBJECT_IME_HIDE;
use bindings::Windows::Win32::UI::WindowsAndMessaging::EVENT_OBJECT_IME_SHOW;
use bindings::Windows::Win32::UI::WindowsAndMessaging::EVENT_OBJECT_INVOKED;
use bindings::Windows::Win32::UI::WindowsAndMessaging::EVENT_OBJECT_LIVEREGIONCHANGED;
use bindings::Windows::Win32::UI::WindowsAndMessaging::EVENT_OBJECT_LOCATIONCHANGE;
use bindings::Windows::Win32::UI::WindowsAndMessaging::EVENT_OBJECT_NAMECHANGE;
use bindings::Windows::Win32::UI::WindowsAndMessaging::EVENT_OBJECT_PARENTCHANGE;
use bindings::Windows::Win32::UI::WindowsAndMessaging::EVENT_OBJECT_REORDER;
use bindings::Windows::Win32::UI::WindowsAndMessaging::EVENT_OBJECT_SELECTION;
use bindings::Windows::Win32::UI::WindowsAndMessaging::EVENT_OBJECT_SELECTIONADD;
use bindings::Windows::Win32::UI::WindowsAndMessaging::EVENT_OBJECT_SELECTIONREMOVE;
use bindings::Windows::Win32::UI::WindowsAndMessaging::EVENT_OBJECT_SELECTIONWITHIN;
use bindings::Windows::Win32::UI::WindowsAndMessaging::EVENT_OBJECT_SHOW;
use bindings::Windows::Win32::UI::WindowsAndMessaging::EVENT_OBJECT_STATECHANGE;
use bindings::Windows::Win32::UI::WindowsAndMessaging::EVENT_OBJECT_TEXTEDIT_CONVERSIONTARGETCHANGED;
use bindings::Windows::Win32::UI::WindowsAndMessaging::EVENT_OBJECT_TEXTSELECTIONCHANGED;
use bindings::Windows::Win32::UI::WindowsAndMessaging::EVENT_OBJECT_UNCLOAKED;
use bindings::Windows::Win32::UI::WindowsAndMessaging::EVENT_OBJECT_VALUECHANGE;
use bindings::Windows::Win32::UI::WindowsAndMessaging::EVENT_OEM_DEFINED_END;
use bindings::Windows::Win32::UI::WindowsAndMessaging::EVENT_OEM_DEFINED_START;
use bindings::Windows::Win32::UI::WindowsAndMessaging::EVENT_SYSTEM_ALERT;
use bindings::Windows::Win32::UI::WindowsAndMessaging::EVENT_SYSTEM_ARRANGMENTPREVIEW;
use bindings::Windows::Win32::UI::WindowsAndMessaging::EVENT_SYSTEM_CAPTUREEND;
use bindings::Windows::Win32::UI::WindowsAndMessaging::EVENT_SYSTEM_CAPTURESTART;
use bindings::Windows::Win32::UI::WindowsAndMessaging::EVENT_SYSTEM_CONTEXTHELPEND;
use bindings::Windows::Win32::UI::WindowsAndMessaging::EVENT_SYSTEM_CONTEXTHELPSTART;
use bindings::Windows::Win32::UI::WindowsAndMessaging::EVENT_SYSTEM_DESKTOPSWITCH;
use bindings::Windows::Win32::UI::WindowsAndMessaging::EVENT_SYSTEM_DIALOGEND;
use bindings::Windows::Win32::UI::WindowsAndMessaging::EVENT_SYSTEM_DIALOGSTART;
use bindings::Windows::Win32::UI::WindowsAndMessaging::EVENT_SYSTEM_DRAGDROPEND;
use bindings::Windows::Win32::UI::WindowsAndMessaging::EVENT_SYSTEM_DRAGDROPSTART;
use bindings::Windows::Win32::UI::WindowsAndMessaging::EVENT_SYSTEM_END;
use bindings::Windows::Win32::UI::WindowsAndMessaging::EVENT_SYSTEM_FOREGROUND;
use bindings::Windows::Win32::UI::WindowsAndMessaging::EVENT_SYSTEM_IME_KEY_NOTIFICATION;
use bindings::Windows::Win32::UI::WindowsAndMessaging::EVENT_SYSTEM_MENUEND;
use bindings::Windows::Win32::UI::WindowsAndMessaging::EVENT_SYSTEM_MENUPOPUPEND;
use bindings::Windows::Win32::UI::WindowsAndMessaging::EVENT_SYSTEM_MENUPOPUPSTART;
use bindings::Windows::Win32::UI::WindowsAndMessaging::EVENT_SYSTEM_MENUSTART;
use bindings::Windows::Win32::UI::WindowsAndMessaging::EVENT_SYSTEM_MINIMIZEEND;
use bindings::Windows::Win32::UI::WindowsAndMessaging::EVENT_SYSTEM_MINIMIZESTART;
use bindings::Windows::Win32::UI::WindowsAndMessaging::EVENT_SYSTEM_MOVESIZEEND;
use bindings::Windows::Win32::UI::WindowsAndMessaging::EVENT_SYSTEM_MOVESIZESTART;
use bindings::Windows::Win32::UI::WindowsAndMessaging::EVENT_SYSTEM_SCROLLINGEND;
use bindings::Windows::Win32::UI::WindowsAndMessaging::EVENT_SYSTEM_SCROLLINGSTART;
use bindings::Windows::Win32::UI::WindowsAndMessaging::EVENT_SYSTEM_SOUND;
use bindings::Windows::Win32::UI::WindowsAndMessaging::EVENT_SYSTEM_SWITCHEND;
use bindings::Windows::Win32::UI::WindowsAndMessaging::EVENT_SYSTEM_SWITCHER_APPDROPPED;
use bindings::Windows::Win32::UI::WindowsAndMessaging::EVENT_SYSTEM_SWITCHER_APPGRABBED;
use bindings::Windows::Win32::UI::WindowsAndMessaging::EVENT_SYSTEM_SWITCHER_APPOVERTARGET;
use bindings::Windows::Win32::UI::WindowsAndMessaging::EVENT_SYSTEM_SWITCHER_CANCELLED;
use bindings::Windows::Win32::UI::WindowsAndMessaging::EVENT_SYSTEM_SWITCHSTART;
use bindings::Windows::Win32::UI::WindowsAndMessaging::EVENT_UIA_EVENTID_END;
use bindings::Windows::Win32::UI::WindowsAndMessaging::EVENT_UIA_EVENTID_START;
use bindings::Windows::Win32::UI::WindowsAndMessaging::EVENT_UIA_PROPID_END;
use bindings::Windows::Win32::UI::WindowsAndMessaging::EVENT_UIA_PROPID_START;

#[derive(Clone, Copy, PartialEq, Debug, strum::Display)]
#[repr(u32)]
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
