use bitflags::bitflags;

use bindings::Windows::Win32::UI::WindowsAndMessaging::SWP_ASYNCWINDOWPOS;
use bindings::Windows::Win32::UI::WindowsAndMessaging::SWP_DEFERERASE;
use bindings::Windows::Win32::UI::WindowsAndMessaging::SWP_DRAWFRAME;
use bindings::Windows::Win32::UI::WindowsAndMessaging::SWP_FRAMECHANGED;
use bindings::Windows::Win32::UI::WindowsAndMessaging::SWP_HIDEWINDOW;
use bindings::Windows::Win32::UI::WindowsAndMessaging::SWP_NOACTIVATE;
use bindings::Windows::Win32::UI::WindowsAndMessaging::SWP_NOCOPYBITS;
use bindings::Windows::Win32::UI::WindowsAndMessaging::SWP_NOMOVE;
use bindings::Windows::Win32::UI::WindowsAndMessaging::SWP_NOOWNERZORDER;
use bindings::Windows::Win32::UI::WindowsAndMessaging::SWP_NOREDRAW;
use bindings::Windows::Win32::UI::WindowsAndMessaging::SWP_NOREPOSITION;
use bindings::Windows::Win32::UI::WindowsAndMessaging::SWP_NOSENDCHANGING;
use bindings::Windows::Win32::UI::WindowsAndMessaging::SWP_NOSIZE;
use bindings::Windows::Win32::UI::WindowsAndMessaging::SWP_NOZORDER;
use bindings::Windows::Win32::UI::WindowsAndMessaging::SWP_SHOWWINDOW;

bitflags! {
    #[derive(Default)]
    pub struct SetWindowPosition: u32 {
        const ASYNC_WINDOW_POS = SWP_ASYNCWINDOWPOS.0;
        const DEFER_ERASE = SWP_DEFERERASE.0;
        const DRAW_FRAME = SWP_DRAWFRAME.0;
        const FRAME_CHANGED = SWP_FRAMECHANGED.0;
        const HIDE_WINDOW = SWP_HIDEWINDOW.0;
        const NO_ACTIVATE = SWP_NOACTIVATE.0;
        const NO_COPY_BITS = SWP_NOCOPYBITS.0;
        const NO_MOVE = SWP_NOMOVE.0;
        const NO_OWNER_Z_ORDER = SWP_NOOWNERZORDER.0;
        const NO_REDRAW = SWP_NOREDRAW.0;
        const NO_REPOSITION = SWP_NOREPOSITION.0;
        const NO_SEND_CHANGING = SWP_NOSENDCHANGING.0;
        const NO_SIZE = SWP_NOSIZE.0;
        const NO_Z_ORDER = SWP_NOZORDER.0;
        const SHOW_WINDOW = SWP_SHOWWINDOW.0;
    }
}
