use bitflags::bitflags;
use windows::Win32::UI::WindowsAndMessaging::WS_BORDER;
use windows::Win32::UI::WindowsAndMessaging::WS_CAPTION;
use windows::Win32::UI::WindowsAndMessaging::WS_CHILD;
use windows::Win32::UI::WindowsAndMessaging::WS_CHILDWINDOW;
use windows::Win32::UI::WindowsAndMessaging::WS_CLIPCHILDREN;
use windows::Win32::UI::WindowsAndMessaging::WS_CLIPSIBLINGS;
use windows::Win32::UI::WindowsAndMessaging::WS_DISABLED;
use windows::Win32::UI::WindowsAndMessaging::WS_DLGFRAME;
use windows::Win32::UI::WindowsAndMessaging::WS_EX_ACCEPTFILES;
use windows::Win32::UI::WindowsAndMessaging::WS_EX_APPWINDOW;
use windows::Win32::UI::WindowsAndMessaging::WS_EX_CLIENTEDGE;
use windows::Win32::UI::WindowsAndMessaging::WS_EX_COMPOSITED;
use windows::Win32::UI::WindowsAndMessaging::WS_EX_CONTEXTHELP;
use windows::Win32::UI::WindowsAndMessaging::WS_EX_CONTROLPARENT;
use windows::Win32::UI::WindowsAndMessaging::WS_EX_DLGMODALFRAME;
use windows::Win32::UI::WindowsAndMessaging::WS_EX_LAYERED;
use windows::Win32::UI::WindowsAndMessaging::WS_EX_LAYOUTRTL;
use windows::Win32::UI::WindowsAndMessaging::WS_EX_LEFT;
use windows::Win32::UI::WindowsAndMessaging::WS_EX_LEFTSCROLLBAR;
use windows::Win32::UI::WindowsAndMessaging::WS_EX_LTRREADING;
use windows::Win32::UI::WindowsAndMessaging::WS_EX_MDICHILD;
use windows::Win32::UI::WindowsAndMessaging::WS_EX_NOACTIVATE;
use windows::Win32::UI::WindowsAndMessaging::WS_EX_NOINHERITLAYOUT;
use windows::Win32::UI::WindowsAndMessaging::WS_EX_NOPARENTNOTIFY;
use windows::Win32::UI::WindowsAndMessaging::WS_EX_NOREDIRECTIONBITMAP;
use windows::Win32::UI::WindowsAndMessaging::WS_EX_OVERLAPPEDWINDOW;
use windows::Win32::UI::WindowsAndMessaging::WS_EX_PALETTEWINDOW;
use windows::Win32::UI::WindowsAndMessaging::WS_EX_RIGHT;
use windows::Win32::UI::WindowsAndMessaging::WS_EX_RIGHTSCROLLBAR;
use windows::Win32::UI::WindowsAndMessaging::WS_EX_RTLREADING;
use windows::Win32::UI::WindowsAndMessaging::WS_EX_STATICEDGE;
use windows::Win32::UI::WindowsAndMessaging::WS_EX_TOOLWINDOW;
use windows::Win32::UI::WindowsAndMessaging::WS_EX_TOPMOST;
use windows::Win32::UI::WindowsAndMessaging::WS_EX_TRANSPARENT;
use windows::Win32::UI::WindowsAndMessaging::WS_EX_WINDOWEDGE;
use windows::Win32::UI::WindowsAndMessaging::WS_GROUP;
use windows::Win32::UI::WindowsAndMessaging::WS_HSCROLL;
use windows::Win32::UI::WindowsAndMessaging::WS_ICONIC;
use windows::Win32::UI::WindowsAndMessaging::WS_MAXIMIZE;
use windows::Win32::UI::WindowsAndMessaging::WS_MAXIMIZEBOX;
use windows::Win32::UI::WindowsAndMessaging::WS_MINIMIZE;
use windows::Win32::UI::WindowsAndMessaging::WS_MINIMIZEBOX;
use windows::Win32::UI::WindowsAndMessaging::WS_OVERLAPPED;
use windows::Win32::UI::WindowsAndMessaging::WS_OVERLAPPEDWINDOW;
use windows::Win32::UI::WindowsAndMessaging::WS_POPUP;
use windows::Win32::UI::WindowsAndMessaging::WS_POPUPWINDOW;
use windows::Win32::UI::WindowsAndMessaging::WS_SIZEBOX;
use windows::Win32::UI::WindowsAndMessaging::WS_SYSMENU;
use windows::Win32::UI::WindowsAndMessaging::WS_TABSTOP;
use windows::Win32::UI::WindowsAndMessaging::WS_THICKFRAME;
use windows::Win32::UI::WindowsAndMessaging::WS_TILED;
use windows::Win32::UI::WindowsAndMessaging::WS_TILEDWINDOW;
use windows::Win32::UI::WindowsAndMessaging::WS_VISIBLE;
use windows::Win32::UI::WindowsAndMessaging::WS_VSCROLL;

// https://docs.microsoft.com/en-us/windows/win32/winmsg/window-styles
bitflags! {
    #[derive(Default)]
    pub struct WindowStyle: u32 {
        const BORDER = WS_BORDER;
        const CAPTION = WS_CAPTION;
        const CHILD = WS_CHILD;
        const CHILDWINDOW = WS_CHILDWINDOW;
        const CLIPCHILDREN = WS_CLIPCHILDREN;
        const CLIPSIBLINGS = WS_CLIPSIBLINGS;
        const DISABLED = WS_DISABLED;
        const DLGFRAME = WS_DLGFRAME;
        const GROUP = WS_GROUP;
        const HSCROLL = WS_HSCROLL;
        const ICONIC = WS_ICONIC;
        const MAXIMIZE = WS_MAXIMIZE;
        const MAXIMIZEBOX = WS_MAXIMIZEBOX;
        const MINIMIZE = WS_MINIMIZE;
        const MINIMIZEBOX = WS_MINIMIZEBOX;
        const OVERLAPPED = WS_OVERLAPPED;
        const OVERLAPPEDWINDOW = WS_OVERLAPPEDWINDOW;
        const POPUP = WS_POPUP;
        const POPUPWINDOW = WS_POPUPWINDOW;
        const SIZEBOX = WS_SIZEBOX;
        const SYSMENU = WS_SYSMENU;
        const TABSTOP = WS_TABSTOP;
        const THICKFRAME = WS_THICKFRAME;
        const TILED = WS_TILED;
        const TILEDWINDOW = WS_TILEDWINDOW;
        const VISIBLE = WS_VISIBLE;
        const VSCROLL = WS_VSCROLL;
    }
}

// https://docs.microsoft.com/en-us/windows/win32/winmsg/extended-window-styles
bitflags! {
    #[derive(Default)]
    pub struct ExtendedWindowStyle: u32 {
        const ACCEPTFILES = WS_EX_ACCEPTFILES;
        const APPWINDOW = WS_EX_APPWINDOW;
        const CLIENTEDGE = WS_EX_CLIENTEDGE;
        const COMPOSITED = WS_EX_COMPOSITED;
        const CONTEXTHELP = WS_EX_CONTEXTHELP;
        const CONTROLPARENT = WS_EX_CONTROLPARENT;
        const DLGMODALFRAME = WS_EX_DLGMODALFRAME;
        const LAYERED = WS_EX_LAYERED;
        const LAYOUTRTL = WS_EX_LAYOUTRTL;
        const LEFT = WS_EX_LEFT;
        const LEFTSCROLLBAR = WS_EX_LEFTSCROLLBAR;
        const LTRREADING = WS_EX_LTRREADING;
        const MDICHILD = WS_EX_MDICHILD;
        const NOACTIVATE = WS_EX_NOACTIVATE;
        const NOINHERITLAYOUT = WS_EX_NOINHERITLAYOUT;
        const NOPARENTNOTIFY = WS_EX_NOPARENTNOTIFY;
        const NOREDIRECTIONBITMAP = WS_EX_NOREDIRECTIONBITMAP;
        const OVERLAPPEDWINDOW = WS_EX_OVERLAPPEDWINDOW;
        const PALETTEWINDOW = WS_EX_PALETTEWINDOW;
        const RIGHT = WS_EX_RIGHT;
        const RIGHTSCROLLBAR = WS_EX_RIGHTSCROLLBAR;
        const RTLREADING = WS_EX_RTLREADING;
        const STATICEDGE = WS_EX_STATICEDGE;
        const TOOLWINDOW = WS_EX_TOOLWINDOW;
        const TOPMOST = WS_EX_TOPMOST;
        const TRANSPARENT = WS_EX_TRANSPARENT;
        const WINDOWEDGE = WS_EX_WINDOWEDGE;
    }
}
