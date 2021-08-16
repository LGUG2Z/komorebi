fn main() {
    windows::build!(
        Windows::Win32::Foundation::RECT,
        Windows::Win32::Foundation::POINT,
        Windows::Win32::Foundation::BOOL,
        Windows::Win32::Foundation::PWSTR,
        Windows::Win32::Foundation::HWND,
        Windows::Win32::Foundation::LPARAM,
        // error: `Windows.Win32.Graphics.Dwm.DWMWA_CLOAKED` not found in metadata
        Windows::Win32::Graphics::Dwm::*,
        // error: `Windows.Win32.Graphics.Gdi.MONITOR_DEFAULTTONEAREST` not found in metadata
        Windows::Win32::Graphics::Gdi::*,
        Windows::Win32::System::Threading::PROCESS_ACCESS_RIGHTS,
        Windows::Win32::System::Threading::PROCESS_NAME_FORMAT,
        Windows::Win32::System::Threading::OpenProcess,
        Windows::Win32::System::Threading::QueryFullProcessImageNameW,
        Windows::Win32::System::Threading::GetCurrentThreadId,
        Windows::Win32::System::Threading::AttachThreadInput,
        Windows::Win32::System::Threading::GetCurrentProcessId,
        Windows::Win32::UI::KeyboardAndMouseInput::SetFocus,
        Windows::Win32::UI::Accessibility::{SetWinEventHook, HWINEVENTHOOK},
        // error: `Windows.Win32.UI.WindowsAndMessaging.GWL_EXSTYLE` not found in metadata
        Windows::Win32::UI::WindowsAndMessaging::*,
    );
}
