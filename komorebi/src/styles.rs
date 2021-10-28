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
        const BORDER = WS_BORDER.0;
        const CAPTION = WS_CAPTION.0;
        const CHILD = WS_CHILD.0;
        const CHILDWINDOW = WS_CHILDWINDOW.0;
        const CLIPCHILDREN = WS_CLIPCHILDREN.0;
        const CLIPSIBLINGS = WS_CLIPSIBLINGS.0;
        const DISABLED = WS_DISABLED.0;
        const DLGFRAME = WS_DLGFRAME.0;
        const GROUP = WS_GROUP.0;
        const HSCROLL = WS_HSCROLL.0;
        const ICONIC = WS_ICONIC.0;
        const MAXIMIZE = WS_MAXIMIZE.0;
        const MAXIMIZEBOX = WS_MAXIMIZEBOX.0;
        const MINIMIZE = WS_MINIMIZE.0;
        const MINIMIZEBOX = WS_MINIMIZEBOX.0;
        const OVERLAPPED = WS_OVERLAPPED.0;
        const OVERLAPPEDWINDOW = WS_OVERLAPPEDWINDOW.0;
        const POPUP = WS_POPUP.0;
        const POPUPWINDOW = WS_POPUPWINDOW.0;
        const SIZEBOX = WS_SIZEBOX.0;
        const SYSMENU = WS_SYSMENU.0;
        const TABSTOP = WS_TABSTOP.0;
        const THICKFRAME = WS_THICKFRAME.0;
        const TILED = WS_TILED.0;
        const TILEDWINDOW = WS_TILEDWINDOW.0;
        const VISIBLE = WS_VISIBLE.0;
        const VSCROLL = WS_VSCROLL.0;
    }
}

// https://docs.microsoft.com/en-us/windows/win32/winmsg/extended-window-styles
bitflags! {
    #[derive(Default)]
    pub struct ExtendedWindowStyle: u32 {
        const ACCEPTFILES = WS_EX_ACCEPTFILES.0;
        const APPWINDOW = WS_EX_APPWINDOW.0;
        const CLIENTEDGE = WS_EX_CLIENTEDGE.0;
        const COMPOSITED = WS_EX_COMPOSITED.0;
        const CONTEXTHELP = WS_EX_CONTEXTHELP.0;
        const CONTROLPARENT = WS_EX_CONTROLPARENT.0;
        const DLGMODALFRAME = WS_EX_DLGMODALFRAME.0;
        const LAYERED = WS_EX_LAYERED.0;
        const LAYOUTRTL = WS_EX_LAYOUTRTL.0;
        const LEFT = WS_EX_LEFT.0;
        const LEFTSCROLLBAR = WS_EX_LEFTSCROLLBAR.0;
        const LTRREADING = WS_EX_LTRREADING.0;
        const MDICHILD = WS_EX_MDICHILD.0;
        const NOACTIVATE = WS_EX_NOACTIVATE.0;
        const NOINHERITLAYOUT = WS_EX_NOINHERITLAYOUT.0;
        const NOPARENTNOTIFY = WS_EX_NOPARENTNOTIFY.0;
        const NOREDIRECTIONBITMAP = WS_EX_NOREDIRECTIONBITMAP.0;
        const OVERLAPPEDWINDOW = WS_EX_OVERLAPPEDWINDOW.0;
        const PALETTEWINDOW = WS_EX_PALETTEWINDOW.0;
        const RIGHT = WS_EX_RIGHT.0;
        const RIGHTSCROLLBAR = WS_EX_RIGHTSCROLLBAR.0;
        const RTLREADING = WS_EX_RTLREADING.0;
        const STATICEDGE = WS_EX_STATICEDGE.0;
        const TOOLWINDOW = WS_EX_TOOLWINDOW.0;
        const TOPMOST = WS_EX_TOPMOST.0;
        const TRANSPARENT = WS_EX_TRANSPARENT.0;
        const WINDOWEDGE = WS_EX_WINDOWEDGE.0;
    }
}
