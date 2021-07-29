fn main() {
    windows::build!(
        Windows::Win32::Foundation::{
            POINT,
            RECT,
            BOOL,
            PWSTR,
            HWND,
            LPARAM,
        },
        // error: `Windows.Win32.Graphics.Dwm.DWMWA_CLOAKED` not found in metadata
        Windows::Win32::Graphics::Dwm::*,
        // error: `Windows.Win32.Graphics.Gdi.MONITOR_DEFAULTTONEAREST` not found in metadata
        Windows::Win32::Graphics::Gdi::*,
        Windows::Win32::System::Threading::{
            PROCESS_ACCESS_RIGHTS,
            PROCESS_NAME_FORMAT,
            OpenProcess,
            QueryFullProcessImageNameW,
            GetCurrentThreadId,
            AttachThreadInput,
            GetCurrentProcessId
        },
        Windows::Win32::UI::KeyboardAndMouseInput::SetFocus,
        Windows::Win32::UI::Accessibility::{SetWinEventHook, HWINEVENTHOOK},
        // error: `Windows.Win32.UI.WindowsAndMessaging.GWL_EXSTYLE` not found in metadata
        Windows::Win32::UI::WindowsAndMessaging::*,
    );
}
