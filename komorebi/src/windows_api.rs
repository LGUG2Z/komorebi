use std::collections::VecDeque;
use std::convert::TryFrom;
use std::ffi::c_void;
use std::mem::size_of;

use color_eyre::eyre::anyhow;
use color_eyre::eyre::bail;
use color_eyre::eyre::Error;
use color_eyre::Result;
use windows::core::Result as WindowsCrateResult;
use windows::core::PCWSTR;
use windows::core::PWSTR;
use windows::Win32::Foundation::CloseHandle;
use windows::Win32::Foundation::BOOL;
use windows::Win32::Foundation::COLORREF;
use windows::Win32::Foundation::HANDLE;
use windows::Win32::Foundation::HMODULE;
use windows::Win32::Foundation::HWND;
use windows::Win32::Foundation::LPARAM;
use windows::Win32::Foundation::POINT;
use windows::Win32::Foundation::WPARAM;
use windows::Win32::Graphics::Dwm::DwmGetWindowAttribute;
use windows::Win32::Graphics::Dwm::DwmSetWindowAttribute;
use windows::Win32::Graphics::Dwm::DWMWA_BORDER_COLOR;
use windows::Win32::Graphics::Dwm::DWMWA_CLOAKED;
use windows::Win32::Graphics::Dwm::DWMWA_COLOR_NONE;
use windows::Win32::Graphics::Dwm::DWMWA_EXTENDED_FRAME_BOUNDS;
use windows::Win32::Graphics::Dwm::DWMWA_WINDOW_CORNER_PREFERENCE;
use windows::Win32::Graphics::Dwm::DWMWCP_ROUND;
use windows::Win32::Graphics::Dwm::DWMWINDOWATTRIBUTE;
use windows::Win32::Graphics::Dwm::DWM_CLOAKED_APP;
use windows::Win32::Graphics::Dwm::DWM_CLOAKED_INHERITED;
use windows::Win32::Graphics::Dwm::DWM_CLOAKED_SHELL;
use windows::Win32::Graphics::Gdi::CreateSolidBrush;
use windows::Win32::Graphics::Gdi::EnumDisplayMonitors;
use windows::Win32::Graphics::Gdi::GetMonitorInfoW;
use windows::Win32::Graphics::Gdi::MonitorFromPoint;
use windows::Win32::Graphics::Gdi::MonitorFromWindow;
use windows::Win32::Graphics::Gdi::Rectangle;
use windows::Win32::Graphics::Gdi::RoundRect;
use windows::Win32::Graphics::Gdi::HBRUSH;
use windows::Win32::Graphics::Gdi::HDC;
use windows::Win32::Graphics::Gdi::HMONITOR;
use windows::Win32::Graphics::Gdi::MONITORENUMPROC;
use windows::Win32::Graphics::Gdi::MONITORINFOEXW;
use windows::Win32::Graphics::Gdi::MONITOR_DEFAULTTONEAREST;
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::System::RemoteDesktop::ProcessIdToSessionId;
use windows::Win32::System::RemoteDesktop::WTSRegisterSessionNotification;
use windows::Win32::System::Threading::GetCurrentProcessId;
use windows::Win32::System::Threading::OpenProcess;
use windows::Win32::System::Threading::QueryFullProcessImageNameW;
use windows::Win32::System::Threading::PROCESS_ACCESS_RIGHTS;
use windows::Win32::System::Threading::PROCESS_NAME_WIN32;
use windows::Win32::System::Threading::PROCESS_QUERY_INFORMATION;
use windows::Win32::UI::HiDpi::GetDpiForMonitor;
use windows::Win32::UI::HiDpi::SetProcessDpiAwarenessContext;
use windows::Win32::UI::HiDpi::DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2;
use windows::Win32::UI::HiDpi::MDT_EFFECTIVE_DPI;
use windows::Win32::UI::Input::KeyboardAndMouse::GetKeyState;
use windows::Win32::UI::Input::KeyboardAndMouse::SendInput;
use windows::Win32::UI::Input::KeyboardAndMouse::INPUT;
use windows::Win32::UI::Input::KeyboardAndMouse::INPUT_0;
use windows::Win32::UI::Input::KeyboardAndMouse::INPUT_MOUSE;
use windows::Win32::UI::Input::KeyboardAndMouse::MOUSEEVENTF_LEFTDOWN;
use windows::Win32::UI::Input::KeyboardAndMouse::MOUSEEVENTF_LEFTUP;
use windows::Win32::UI::Input::KeyboardAndMouse::MOUSEINPUT;
use windows::Win32::UI::Input::KeyboardAndMouse::VK_LBUTTON;
use windows::Win32::UI::Input::KeyboardAndMouse::VK_MENU;
use windows::Win32::UI::WindowsAndMessaging::AllowSetForegroundWindow;
use windows::Win32::UI::WindowsAndMessaging::BringWindowToTop;
use windows::Win32::UI::WindowsAndMessaging::CreateWindowExW;
use windows::Win32::UI::WindowsAndMessaging::EnumWindows;
use windows::Win32::UI::WindowsAndMessaging::GetCursorPos;
use windows::Win32::UI::WindowsAndMessaging::GetDesktopWindow;
use windows::Win32::UI::WindowsAndMessaging::GetForegroundWindow;
use windows::Win32::UI::WindowsAndMessaging::GetTopWindow;
use windows::Win32::UI::WindowsAndMessaging::GetWindow;
use windows::Win32::UI::WindowsAndMessaging::GetWindowLongPtrW;
use windows::Win32::UI::WindowsAndMessaging::GetWindowRect;
use windows::Win32::UI::WindowsAndMessaging::GetWindowTextW;
use windows::Win32::UI::WindowsAndMessaging::GetWindowThreadProcessId;
use windows::Win32::UI::WindowsAndMessaging::IsIconic;
use windows::Win32::UI::WindowsAndMessaging::IsWindow;
use windows::Win32::UI::WindowsAndMessaging::IsWindowVisible;
use windows::Win32::UI::WindowsAndMessaging::IsZoomed;
use windows::Win32::UI::WindowsAndMessaging::PostMessageW;
use windows::Win32::UI::WindowsAndMessaging::RealGetWindowClassW;
use windows::Win32::UI::WindowsAndMessaging::RegisterClassW;
use windows::Win32::UI::WindowsAndMessaging::SetCursorPos;
use windows::Win32::UI::WindowsAndMessaging::SetForegroundWindow;
use windows::Win32::UI::WindowsAndMessaging::SetLayeredWindowAttributes;
use windows::Win32::UI::WindowsAndMessaging::SetWindowLongPtrW;
use windows::Win32::UI::WindowsAndMessaging::SetWindowPos;
use windows::Win32::UI::WindowsAndMessaging::ShowWindow;
use windows::Win32::UI::WindowsAndMessaging::SystemParametersInfoW;
use windows::Win32::UI::WindowsAndMessaging::WindowFromPoint;
use windows::Win32::UI::WindowsAndMessaging::CW_USEDEFAULT;
use windows::Win32::UI::WindowsAndMessaging::GWL_EXSTYLE;
use windows::Win32::UI::WindowsAndMessaging::GWL_STYLE;
use windows::Win32::UI::WindowsAndMessaging::GW_HWNDNEXT;
use windows::Win32::UI::WindowsAndMessaging::HWND_TOP;
use windows::Win32::UI::WindowsAndMessaging::LWA_ALPHA;
use windows::Win32::UI::WindowsAndMessaging::LWA_COLORKEY;
use windows::Win32::UI::WindowsAndMessaging::SET_WINDOW_POS_FLAGS;
use windows::Win32::UI::WindowsAndMessaging::SHOW_WINDOW_CMD;
use windows::Win32::UI::WindowsAndMessaging::SPIF_SENDCHANGE;
use windows::Win32::UI::WindowsAndMessaging::SPI_GETACTIVEWINDOWTRACKING;
use windows::Win32::UI::WindowsAndMessaging::SPI_GETFOREGROUNDLOCKTIMEOUT;
use windows::Win32::UI::WindowsAndMessaging::SPI_SETACTIVEWINDOWTRACKING;
use windows::Win32::UI::WindowsAndMessaging::SPI_SETFOREGROUNDLOCKTIMEOUT;
use windows::Win32::UI::WindowsAndMessaging::SWP_NOMOVE;
use windows::Win32::UI::WindowsAndMessaging::SWP_NOSIZE;
use windows::Win32::UI::WindowsAndMessaging::SWP_SHOWWINDOW;
use windows::Win32::UI::WindowsAndMessaging::SW_HIDE;
use windows::Win32::UI::WindowsAndMessaging::SW_MAXIMIZE;
use windows::Win32::UI::WindowsAndMessaging::SW_MINIMIZE;
use windows::Win32::UI::WindowsAndMessaging::SW_NORMAL;
use windows::Win32::UI::WindowsAndMessaging::SW_SHOWNOACTIVATE;
use windows::Win32::UI::WindowsAndMessaging::SYSTEM_PARAMETERS_INFO_ACTION;
use windows::Win32::UI::WindowsAndMessaging::SYSTEM_PARAMETERS_INFO_UPDATE_FLAGS;
use windows::Win32::UI::WindowsAndMessaging::WINDOW_LONG_PTR_INDEX;
use windows::Win32::UI::WindowsAndMessaging::WM_CLOSE;
use windows::Win32::UI::WindowsAndMessaging::WNDCLASSW;
use windows::Win32::UI::WindowsAndMessaging::WNDENUMPROC;
use windows::Win32::UI::WindowsAndMessaging::WS_DISABLED;
use windows::Win32::UI::WindowsAndMessaging::WS_EX_LAYERED;
use windows::Win32::UI::WindowsAndMessaging::WS_EX_NOACTIVATE;
use windows::Win32::UI::WindowsAndMessaging::WS_EX_TOOLWINDOW;
use windows::Win32::UI::WindowsAndMessaging::WS_EX_TOPMOST;
use windows::Win32::UI::WindowsAndMessaging::WS_POPUP;
use windows::Win32::UI::WindowsAndMessaging::WS_SYSMENU;

use komorebi_core::Rect;

use crate::container::Container;
use crate::monitor;
use crate::monitor::Monitor;
use crate::ring::Ring;
use crate::set_window_position::SetWindowPosition;
use crate::windows_callbacks;
use crate::Window;
use crate::DISPLAY_INDEX_PREFERENCES;
use crate::MONITOR_INDEX_PREFERENCES;

pub enum WindowsResult<T, E> {
    Err(E),
    Ok(T),
}

macro_rules! impl_from_integer_for_windows_result {
    ( $( $integer_type:ty ),+ ) => {
        $(
            impl From<$integer_type> for WindowsResult<$integer_type, Error> {
                fn from(return_value: $integer_type) -> Self {
                    match return_value {
                        0 => Self::Err(std::io::Error::last_os_error().into()),
                        _ => Self::Ok(return_value),
                    }
                }
            }
        )+
    };
}

impl_from_integer_for_windows_result!(usize, isize, u16, u32, i32);

impl<T, E> From<WindowsResult<T, E>> for Result<T, E> {
    fn from(result: WindowsResult<T, E>) -> Self {
        match result {
            WindowsResult::Err(error) => Err(error),
            WindowsResult::Ok(ok) => Ok(ok),
        }
    }
}

pub trait ProcessWindowsCrateResult<T> {
    fn process(self) -> Result<T>;
}

macro_rules! impl_process_windows_crate_integer_wrapper_result {
    ( $($input:ty => $deref:ty),+ $(,)? ) => (
        paste::paste! {
            $(
                impl ProcessWindowsCrateResult<$deref> for $input {
                    fn process(self) -> Result<$deref> {
                        if self == $input(0) {
                            Err(std::io::Error::last_os_error().into())
                        } else {
                            Ok(self.0)
                        }
                    }
                }
            )+
        }
    );
}

impl_process_windows_crate_integer_wrapper_result!(
    HWND => isize,
);

impl<T> ProcessWindowsCrateResult<T> for WindowsCrateResult<T> {
    fn process(self) -> Result<T> {
        match self {
            Ok(value) => Ok(value),
            Err(error) => Err(error.into()),
        }
    }
}

pub struct WindowsApi;

impl WindowsApi {
    pub fn enum_display_monitors(
        callback: MONITORENUMPROC,
        callback_data_address: isize,
    ) -> Result<()> {
        unsafe { EnumDisplayMonitors(HDC(0), None, callback, LPARAM(callback_data_address)) }
            .ok()
            .process()
    }

    pub fn valid_hmonitors() -> Result<Vec<(String, isize)>> {
        Ok(win32_display_data::connected_displays_all()
            .flatten()
            .map(|d| {
                let name = d.device_name.trim_start_matches(r"\\.\").to_string();
                let name = name.split('\\').collect::<Vec<_>>()[0].to_string();

                (name, d.hmonitor)
            })
            .collect::<Vec<_>>())
    }

    pub fn load_monitor_information(monitors: &mut Ring<Monitor>) -> Result<()> {
        'read: for display in win32_display_data::connected_displays_all().flatten() {
            let path = display.device_path.clone();
            let mut split: Vec<_> = path.split('#').collect();
            split.remove(0);
            split.remove(split.len() - 1);
            let device = split[0].to_string();
            let device_id = split.join("-");

            let name = display.device_name.trim_start_matches(r"\\.\").to_string();
            let name = name.split('\\').collect::<Vec<_>>()[0].to_string();

            for monitor in monitors.elements() {
                if device_id.eq(monitor.device_id()) {
                    continue 'read;
                }
            }

            let m = monitor::new(
                display.hmonitor,
                display.size.into(),
                display.work_area_size.into(),
                name,
                device,
                device_id,
            );

            let mut index_preference = None;
            let monitor_index_preferences = MONITOR_INDEX_PREFERENCES.lock();
            for (index, monitor_size) in &*monitor_index_preferences {
                if m.size() == monitor_size {
                    index_preference = Option::from(index);
                }
            }

            let display_index_preferences = DISPLAY_INDEX_PREFERENCES.lock();
            for (index, id) in &*display_index_preferences {
                if id.eq(m.device_id()) {
                    index_preference = Option::from(index);
                }
            }

            if monitors.elements().is_empty() {
                monitors.elements_mut().push_back(m);
            } else if let Some(preference) = index_preference {
                while *preference > monitors.elements().len() {
                    monitors.elements_mut().reserve(1);
                }

                monitors.elements_mut().insert(*preference, m);
            } else {
                monitors.elements_mut().push_back(m);
            }
        }

        Ok(())
    }

    pub fn enum_windows(callback: WNDENUMPROC, callback_data_address: isize) -> Result<()> {
        unsafe { EnumWindows(callback, LPARAM(callback_data_address)) }.process()
    }

    pub fn load_workspace_information(monitors: &mut Ring<Monitor>) -> Result<()> {
        for monitor in monitors.elements_mut() {
            let monitor_name = monitor.name().clone();
            if let Some(workspace) = monitor.workspaces_mut().front_mut() {
                // EnumWindows will enumerate through windows on all monitors
                Self::enum_windows(
                    Some(windows_callbacks::enum_window),
                    workspace.containers_mut() as *mut VecDeque<Container> as isize,
                )?;

                // Ensure that the resize_dimensions Vec length matches the number of containers for
                // the potential later calls to workspace.remove_window later in this fn
                let len = workspace.containers().len();
                workspace.resize_dimensions_mut().resize(len, None);

                // We have to prune each monitor's primary workspace of undesired windows here
                let mut windows_on_other_monitors = vec![];

                for container in workspace.containers_mut() {
                    for window in container.windows() {
                        if Self::monitor_name_from_window(window.hwnd())? != monitor_name {
                            windows_on_other_monitors.push(window.hwnd().0);
                        }
                    }
                }

                for hwnd in windows_on_other_monitors {
                    workspace.remove_window(hwnd)?;
                }
            }
        }

        Ok(())
    }

    pub fn allow_set_foreground_window(process_id: u32) -> Result<()> {
        unsafe { AllowSetForegroundWindow(process_id) }.process()
    }

    pub fn monitor_from_window(hwnd: HWND) -> isize {
        // MONITOR_DEFAULTTONEAREST ensures that the return value will never be NULL
        // https://docs.microsoft.com/en-us/windows/win32/api/winuser/nf-winuser-monitorfromwindow
        unsafe { MonitorFromWindow(hwnd, MONITOR_DEFAULTTONEAREST) }.0
    }

    pub fn monitor_name_from_window(hwnd: HWND) -> Result<String> {
        // MONITOR_DEFAULTTONEAREST ensures that the return value will never be NULL
        // https://docs.microsoft.com/en-us/windows/win32/api/winuser/nf-winuser-monitorfromwindow
        Ok(
            Self::monitor(unsafe { MonitorFromWindow(hwnd, MONITOR_DEFAULTTONEAREST) }.0)?
                .name()
                .to_string(),
        )
    }

    pub fn monitor_from_point(point: POINT) -> isize {
        // MONITOR_DEFAULTTONEAREST ensures that the return value will never be NULL
        // https://docs.microsoft.com/en-us/windows/win32/api/winuser/nf-winuser-monitorfromwindow
        unsafe { MonitorFromPoint(point, MONITOR_DEFAULTTONEAREST) }.0
    }

    /// position window resizes the target window to the given layout, adjusting
    /// the layout to account for any window shadow borders (the window painted
    /// region will match layout on completion).
    pub fn position_window(hwnd: HWND, layout: &Rect, top: bool) -> Result<()> {
        let mut flags = SetWindowPosition::NO_ACTIVATE
            | SetWindowPosition::NO_SEND_CHANGING
            | SetWindowPosition::NO_COPY_BITS
            | SetWindowPosition::FRAME_CHANGED;

        // If the request is to place the window on top, then HWND_TOP will take
        // effect, otherwise pass NO_Z_ORDER that will cause set_window_pos to
        // ignore the z-order paramter.
        if !top {
            flags |= SetWindowPosition::NO_Z_ORDER;
        }

        let shadow_rect = Self::shadow_rect(hwnd).unwrap_or_default();
        let rect = Rect {
            left: layout.left + shadow_rect.left,
            top: layout.top + shadow_rect.top,
            right: layout.right + shadow_rect.right,
            bottom: layout.bottom + shadow_rect.bottom,
        };

        // Note: earlier code had set HWND_TOPMOST here, but we should not do
        // that. HWND_TOPMOST is a sticky z-order change, rather than a regular
        // z-order reordering. Programs will use TOPMOST themselves to do things
        // such as making sure that their tool windows or dialog pop-ups are
        // above their main window. If any such windows are unmanaged, they must
        // still remian topmost, so we set HWND_TOP here, which will cause the
        // managed window to come to the front, but if the managed window has a
        // child that is TOPMOST it will still be rendered above, in the proper
        // order expected by the application. It's also important to understand
        // that TOPMOST is somewhat viral, in that when you set a window to
        // TOPMOST all of its owned windows are also made TOPMOST.
        // See https://learn.microsoft.com/en-us/windows/win32/api/winuser/nf-winuser-setwindowpos#remarks
        Self::set_window_pos(hwnd, &rect, HWND_TOP, flags.bits())
    }

    pub fn bring_window_to_top(hwnd: HWND) -> Result<()> {
        unsafe { BringWindowToTop(hwnd) }.process()
    }

    // Raise the window to the top of the Z order, but do not activate or focus
    // it. Use raise_and_focus_window to activate and focus a window.
    pub fn raise_window(hwnd: HWND) -> Result<()> {
        let flags = SetWindowPosition::NO_MOVE | SetWindowPosition::NO_ACTIVATE;

        let position = HWND_TOP;
        Self::set_window_pos(hwnd, &Rect::default(), position, flags.bits())
    }

    pub fn set_border_pos(hwnd: HWND, layout: &Rect, position: HWND) -> Result<()> {
        let flags = { SetWindowPosition::SHOW_WINDOW | SetWindowPosition::NO_ACTIVATE };
        Self::set_window_pos(hwnd, layout, position, flags.bits())
    }

    /// set_window_pos calls SetWindowPos without any accounting for Window decorations.
    fn set_window_pos(hwnd: HWND, layout: &Rect, position: HWND, flags: u32) -> Result<()> {
        unsafe {
            SetWindowPos(
                hwnd,
                position,
                layout.left,
                layout.top,
                layout.right,
                layout.bottom,
                SET_WINDOW_POS_FLAGS(flags),
            )
        }
        .process()
    }

    pub fn show_window(hwnd: HWND, command: SHOW_WINDOW_CMD) {
        // BOOL is returned but does not signify whether or not the operation was succesful
        // https://docs.microsoft.com/en-us/windows/win32/api/winuser/nf-winuser-showwindow
        unsafe { ShowWindow(hwnd, command) };
    }

    pub fn minimize_window(hwnd: HWND) {
        Self::show_window(hwnd, SW_MINIMIZE);
    }

    fn post_message(hwnd: HWND, message: u32, wparam: WPARAM, lparam: LPARAM) -> Result<()> {
        unsafe { PostMessageW(hwnd, message, wparam, lparam) }.process()
    }

    pub fn close_window(hwnd: HWND) -> Result<()> {
        match Self::post_message(hwnd, WM_CLOSE, WPARAM(0), LPARAM(0)) {
            Ok(()) => Ok(()),
            Err(_) => Err(anyhow!("could not close window")),
        }
    }

    pub fn hide_window(hwnd: HWND) {
        Self::show_window(hwnd, SW_HIDE);
    }

    pub fn restore_window(hwnd: HWND) {
        Self::show_window(hwnd, SW_SHOWNOACTIVATE);
    }

    pub fn unmaximize_window(hwnd: HWND) {
        Self::show_window(hwnd, SW_NORMAL);
    }

    pub fn maximize_window(hwnd: HWND) {
        Self::show_window(hwnd, SW_MAXIMIZE);
    }

    pub fn foreground_window() -> Result<isize> {
        unsafe { GetForegroundWindow() }.process()
    }

    pub fn raise_and_focus_window(hwnd: HWND) -> Result<()> {
        let event = [INPUT {
            r#type: INPUT_MOUSE,
            ..Default::default()
        }];

        unsafe {
            // Send an input event to our own process first so that we pass the
            // foreground lock check
            SendInput(&event, size_of::<INPUT>() as i32);
            // Error ignored, as the operation is not always necessary.
            let _ = SetWindowPos(
                hwnd,
                HWND_TOP,
                0,
                0,
                0,
                0,
                SWP_NOMOVE | SWP_NOSIZE | SWP_SHOWWINDOW,
            )
            .process();
            SetForegroundWindow(hwnd)
        }
        .ok()
        .process()
    }

    #[allow(dead_code)]
    pub fn top_window() -> Result<isize> {
        unsafe { GetTopWindow(HWND::default()) }.process()
    }

    pub fn desktop_window() -> Result<isize> {
        unsafe { GetDesktopWindow() }.process()
    }

    #[allow(dead_code)]
    pub fn next_window(hwnd: HWND) -> Result<isize> {
        unsafe { GetWindow(hwnd, GW_HWNDNEXT) }.process()
    }

    pub fn alt_tab_windows() -> Result<Vec<Window>> {
        let mut hwnds = vec![];
        Self::enum_windows(
            Some(windows_callbacks::alt_tab_windows),
            &mut hwnds as *mut Vec<Window> as isize,
        )?;

        Ok(hwnds)
    }

    #[allow(dead_code)]
    pub fn top_visible_window() -> Result<isize> {
        let hwnd = Self::top_window()?;
        let mut next_hwnd = hwnd;

        while next_hwnd != 0 {
            if Self::is_window_visible(HWND(next_hwnd)) {
                return Ok(next_hwnd);
            }

            next_hwnd = Self::next_window(HWND(next_hwnd))?;
        }

        Err(anyhow!("could not find next window"))
    }

    pub fn window_rect(hwnd: HWND) -> Result<Rect> {
        let mut rect = unsafe { std::mem::zeroed() };

        if Self::dwm_get_window_attribute(hwnd, DWMWA_EXTENDED_FRAME_BOUNDS, &mut rect).is_ok() {
            // TODO(raggi): once we declare DPI awareness, we will need to scale the rect.
            // let window_scale = unsafe { GetDpiForWindow(hwnd) };
            // let system_scale = unsafe { GetDpiForSystem() };
            // Ok(Rect::from(rect).scale(system_scale.try_into()?, window_scale.try_into()?))
            Ok(Rect::from(rect))
        } else {
            unsafe { GetWindowRect(hwnd, &mut rect) }.process()?;
            Ok(Rect::from(rect))
        }
    }

    /// shadow_rect computes the offset of the shadow position of the window to
    /// the window painted region. The four values in the returned Rect can be
    /// added to a position rect to compute a size for set_window_pos that will
    /// fill the target area, ignoring shadows.
    fn shadow_rect(hwnd: HWND) -> Result<Rect> {
        let window_rect = Self::window_rect(hwnd)?;

        let mut srect = Default::default();
        unsafe { GetWindowRect(hwnd, &mut srect) }.process()?;
        let shadow_rect = Rect::from(srect);

        Ok(Rect {
            left: shadow_rect.left - window_rect.left,
            top: shadow_rect.top - window_rect.top,
            right: shadow_rect.right - window_rect.right,
            bottom: shadow_rect.bottom - window_rect.bottom,
        })
    }

    pub fn round_rect(hdc: HDC, rect: &Rect, border_radius: i32) {
        unsafe {
            RoundRect(
                hdc,
                rect.left,
                rect.top,
                rect.right,
                rect.bottom,
                border_radius,
                border_radius,
            );
        }
    }
    pub fn rectangle(hdc: HDC, rect: &Rect) {
        unsafe {
            Rectangle(hdc, rect.left, rect.top, rect.right, rect.bottom);
        }
    }
    fn set_cursor_pos(x: i32, y: i32) -> Result<()> {
        unsafe { SetCursorPos(x, y) }.process()
    }

    pub fn cursor_pos() -> Result<POINT> {
        let mut cursor_pos = POINT::default();
        unsafe { GetCursorPos(&mut cursor_pos) }.process()?;

        Ok(cursor_pos)
    }

    pub fn window_from_point(point: POINT) -> Result<isize> {
        unsafe { WindowFromPoint(point) }.process()
    }

    pub fn window_at_cursor_pos() -> Result<isize> {
        Self::window_from_point(Self::cursor_pos()?)
    }

    pub fn center_cursor_in_rect(rect: &Rect) -> Result<()> {
        Self::set_cursor_pos(rect.left + (rect.right / 2), rect.top + (rect.bottom / 2))
    }

    pub fn window_thread_process_id(hwnd: HWND) -> (u32, u32) {
        let mut process_id: u32 = 0;

        // Behaviour is undefined if an invalid HWND is given
        // https://docs.microsoft.com/en-us/windows/win32/api/winuser/nf-winuser-getwindowthreadprocessid
        let thread_id = unsafe {
            GetWindowThreadProcessId(hwnd, Option::from(std::ptr::addr_of_mut!(process_id)))
        };

        (process_id, thread_id)
    }

    pub fn current_process_id() -> u32 {
        unsafe { GetCurrentProcessId() }
    }

    pub fn process_id_to_session_id() -> Result<u32> {
        let process_id = Self::current_process_id();
        let mut session_id = 0;

        unsafe {
            if ProcessIdToSessionId(process_id, &mut session_id).is_ok() {
                Ok(session_id)
            } else {
                Err(anyhow!("could not determine current session id"))
            }
        }
    }

    #[allow(dead_code)]
    fn set_window_long_ptr_w(
        hwnd: HWND,
        index: WINDOW_LONG_PTR_INDEX,
        new_value: isize,
    ) -> Result<()> {
        Result::from(WindowsResult::from(unsafe {
            SetWindowLongPtrW(hwnd, index, new_value)
        }))
        .map(|_| {})
    }

    pub fn gwl_style(hwnd: HWND) -> Result<isize> {
        Self::window_long_ptr_w(hwnd, GWL_STYLE)
    }

    pub fn gwl_ex_style(hwnd: HWND) -> Result<isize> {
        Self::window_long_ptr_w(hwnd, GWL_EXSTYLE)
    }

    fn window_long_ptr_w(hwnd: HWND, index: WINDOW_LONG_PTR_INDEX) -> Result<isize> {
        // Can return 0, which does not always mean that an error has occurred
        // https://docs.microsoft.com/en-us/windows/win32/api/winuser/nf-winuser-getwindowlongptrw
        Result::from(WindowsResult::from(unsafe {
            GetWindowLongPtrW(hwnd, index)
        }))
    }

    #[allow(dead_code)]
    pub fn update_style(hwnd: HWND, new_value: isize) -> Result<()> {
        Self::set_window_long_ptr_w(hwnd, GWL_STYLE, new_value)
    }

    #[allow(dead_code)]
    pub fn update_ex_style(hwnd: HWND, new_value: isize) -> Result<()> {
        Self::set_window_long_ptr_w(hwnd, GWL_EXSTYLE, new_value)
    }

    pub fn window_text_w(hwnd: HWND) -> Result<String> {
        let mut text: [u16; 512] = [0; 512];
        match WindowsResult::from(unsafe { GetWindowTextW(hwnd, &mut text) }) {
            WindowsResult::Ok(len) => {
                let length = usize::try_from(len)?;
                Ok(String::from_utf16(&text[..length])?)
            }
            WindowsResult::Err(error) => Err(error),
        }
    }

    fn open_process(
        access_rights: PROCESS_ACCESS_RIGHTS,
        inherit_handle: bool,
        process_id: u32,
    ) -> Result<HANDLE> {
        unsafe { OpenProcess(access_rights, inherit_handle, process_id) }.process()
    }

    pub fn close_process(handle: HANDLE) -> Result<()> {
        unsafe { CloseHandle(handle) }.process()
    }

    pub fn process_handle(process_id: u32) -> Result<HANDLE> {
        Self::open_process(PROCESS_QUERY_INFORMATION, false, process_id)
    }

    pub fn exe_path(handle: HANDLE) -> Result<String> {
        let mut len = 260_u32;
        let mut path: Vec<u16> = vec![0; len as usize];
        let text_ptr = path.as_mut_ptr();

        unsafe {
            QueryFullProcessImageNameW(handle, PROCESS_NAME_WIN32, PWSTR(text_ptr), &mut len)
        }
        .process()?;

        Ok(String::from_utf16(&path[..len as usize])?)
    }

    pub fn exe(handle: HANDLE) -> Result<String> {
        Ok(Self::exe_path(handle)?
            .split('\\')
            .last()
            .ok_or_else(|| anyhow!("there is no last element"))?
            .to_string())
    }

    pub fn real_window_class_w(hwnd: HWND) -> Result<String> {
        const BUF_SIZE: usize = 512;
        let mut class: [u16; BUF_SIZE] = [0; BUF_SIZE];

        let len = Result::from(WindowsResult::from(unsafe {
            RealGetWindowClassW(hwnd, &mut class)
        }))?;

        Ok(String::from_utf16(&class[0..len as usize])?)
    }

    pub fn dwm_get_window_attribute<T>(
        hwnd: HWND,
        attribute: DWMWINDOWATTRIBUTE,
        value: &mut T,
    ) -> Result<()> {
        unsafe {
            DwmGetWindowAttribute(
                hwnd,
                attribute,
                (value as *mut T).cast(),
                u32::try_from(std::mem::size_of::<T>())?,
            )?;
        }

        Ok(())
    }

    pub fn is_window_cloaked(hwnd: HWND) -> Result<bool> {
        let mut cloaked: u32 = 0;
        Self::dwm_get_window_attribute(hwnd, DWMWA_CLOAKED, &mut cloaked)?;

        Ok(matches!(
            cloaked,
            DWM_CLOAKED_APP | DWM_CLOAKED_SHELL | DWM_CLOAKED_INHERITED
        ))
    }

    pub fn is_window(hwnd: HWND) -> bool {
        unsafe { IsWindow(hwnd) }.into()
    }

    pub fn is_window_visible(hwnd: HWND) -> bool {
        unsafe { IsWindowVisible(hwnd) }.into()
    }

    pub fn is_iconic(hwnd: HWND) -> bool {
        unsafe { IsIconic(hwnd) }.into()
    }

    pub fn is_zoomed(hwnd: HWND) -> bool {
        unsafe { IsZoomed(hwnd) }.into()
    }

    pub fn monitor_info_w(hmonitor: HMONITOR) -> Result<MONITORINFOEXW> {
        let mut ex_info = MONITORINFOEXW::default();
        ex_info.monitorInfo.cbSize = u32::try_from(std::mem::size_of::<MONITORINFOEXW>())?;
        unsafe { GetMonitorInfoW(hmonitor, &mut ex_info.monitorInfo) }
            .ok()
            .process()?;

        Ok(ex_info)
    }

    pub fn monitor(hmonitor: isize) -> Result<Monitor> {
        for display in win32_display_data::connected_displays_all().flatten() {
            if display.hmonitor == hmonitor {
                let path = display.device_path;
                let mut split: Vec<_> = path.split('#').collect();
                split.remove(0);
                split.remove(split.len() - 1);
                let device = split[0].to_string();
                let device_id = split.join("-");

                let name = display.device_name.trim_start_matches(r"\\.\").to_string();
                let name = name.split('\\').collect::<Vec<_>>()[0].to_string();

                let monitor = monitor::new(
                    hmonitor,
                    display.size.into(),
                    display.work_area_size.into(),
                    name,
                    device,
                    device_id,
                );

                return Ok(monitor);
            }
        }

        bail!("could not find device_id for hmonitor: {hmonitor}");
    }

    pub fn set_process_dpi_awareness_context() -> Result<()> {
        unsafe { SetProcessDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2) }
            .process()
    }

    #[allow(dead_code)]
    pub fn system_parameters_info_w(
        action: SYSTEM_PARAMETERS_INFO_ACTION,
        ui_param: u32,
        pv_param: *mut c_void,
        update_flags: SYSTEM_PARAMETERS_INFO_UPDATE_FLAGS,
    ) -> Result<()> {
        unsafe { SystemParametersInfoW(action, ui_param, Option::from(pv_param), update_flags) }
            .process()
    }

    #[tracing::instrument]
    pub fn foreground_lock_timeout() -> Result<()> {
        let mut value: u32 = 0;

        Self::system_parameters_info_w(
            SPI_GETFOREGROUNDLOCKTIMEOUT,
            0,
            std::ptr::addr_of_mut!(value).cast(),
            SPIF_SENDCHANGE,
        )?;

        tracing::info!("current value of ForegroundLockTimeout is {value}");

        if value != 0 {
            tracing::info!("updating value of ForegroundLockTimeout to {value} in order to enable keyboard-driven focus updating");

            Self::system_parameters_info_w(
                SPI_SETFOREGROUNDLOCKTIMEOUT,
                0,
                std::ptr::null_mut::<c_void>(),
                SPIF_SENDCHANGE,
            )?;

            Self::system_parameters_info_w(
                SPI_GETFOREGROUNDLOCKTIMEOUT,
                0,
                std::ptr::addr_of_mut!(value).cast(),
                SPIF_SENDCHANGE,
            )?;

            tracing::info!("updated value of ForegroundLockTimeout is now {value}");
        }

        Ok(())
    }

    #[allow(dead_code)]
    pub fn focus_follows_mouse() -> Result<bool> {
        let mut is_enabled: BOOL = unsafe { std::mem::zeroed() };

        Self::system_parameters_info_w(
            SPI_GETACTIVEWINDOWTRACKING,
            0,
            std::ptr::addr_of_mut!(is_enabled).cast(),
            SPIF_SENDCHANGE,
        )?;

        Ok(is_enabled.into())
    }

    #[allow(dead_code)]
    pub fn enable_focus_follows_mouse() -> Result<()> {
        Self::system_parameters_info_w(
            SPI_SETACTIVEWINDOWTRACKING,
            0,
            1 as *mut c_void,
            SPIF_SENDCHANGE,
        )
    }

    #[allow(dead_code)]
    pub fn disable_focus_follows_mouse() -> Result<()> {
        Self::system_parameters_info_w(
            SPI_SETACTIVEWINDOWTRACKING,
            0,
            std::ptr::null_mut::<c_void>(),
            SPIF_SENDCHANGE,
        )
    }

    pub fn module_handle_w() -> Result<HMODULE> {
        unsafe { GetModuleHandleW(None) }.process()
    }

    pub fn create_solid_brush(colour: u32) -> HBRUSH {
        unsafe { CreateSolidBrush(COLORREF(colour)) }
    }

    pub fn register_class_w(window_class: &WNDCLASSW) -> Result<u16> {
        Result::from(WindowsResult::from(unsafe { RegisterClassW(window_class) }))
    }

    pub fn dpi_for_monitor(hmonitor: isize) -> Result<f32> {
        let mut dpi_x = u32::default();
        let mut dpi_y = u32::default();

        unsafe {
            GetDpiForMonitor(
                HMONITOR(hmonitor),
                MDT_EFFECTIVE_DPI,
                std::ptr::addr_of_mut!(dpi_x),
                std::ptr::addr_of_mut!(dpi_y),
            )
        }
        .process()?;

        #[allow(clippy::cast_precision_loss)]
        Ok(dpi_y as f32 / 96.0)
    }

    pub fn monitors_have_same_dpi(hmonitor_a: isize, hmonitor_b: isize) -> Result<bool> {
        let dpi_a = Self::dpi_for_monitor(hmonitor_a)?;
        let dpi_b = Self::dpi_for_monitor(hmonitor_b)?;

        Ok((dpi_a - dpi_b).abs() < f32::EPSILON)
    }

    pub fn round_corners(hwnd: isize) -> Result<()> {
        let round = DWMWCP_ROUND;

        unsafe {
            DwmSetWindowAttribute(
                HWND(hwnd),
                DWMWA_WINDOW_CORNER_PREFERENCE,
                std::ptr::addr_of!(round).cast(),
                4,
            )
        }
        .process()
    }

    pub fn set_window_accent(hwnd: isize, color: Option<u32>) -> Result<()> {
        let col_ref = COLORREF(color.unwrap_or(DWMWA_COLOR_NONE));
        unsafe {
            DwmSetWindowAttribute(
                HWND(hwnd),
                DWMWA_BORDER_COLOR,
                std::ptr::addr_of!(col_ref).cast(),
                4,
            )
        }
        .process()
    }

    pub fn create_border_window(name: PCWSTR, instance: HMODULE) -> Result<isize> {
        unsafe {
            let hwnd = CreateWindowExW(
                WS_EX_TOOLWINDOW | WS_EX_LAYERED | WS_EX_TOPMOST | WS_EX_NOACTIVATE,
                name,
                name,
                WS_POPUP | WS_SYSMENU,
                CW_USEDEFAULT,
                CW_USEDEFAULT,
                CW_USEDEFAULT,
                CW_USEDEFAULT,
                None,
                None,
                instance,
                None,
            );

            SetLayeredWindowAttributes(hwnd, COLORREF(0), 0, LWA_COLORKEY)?;

            hwnd
        }
        .process()
    }

    pub fn set_transparent(hwnd: HWND, alpha: u8) -> Result<()> {
        unsafe {
            #[allow(clippy::cast_sign_loss)]
            SetLayeredWindowAttributes(hwnd, COLORREF(-1i32 as u32), alpha, LWA_ALPHA)?;
        }

        Ok(())
    }

    pub fn create_hidden_window(name: PCWSTR, instance: HMODULE) -> Result<isize> {
        unsafe {
            CreateWindowExW(
                WS_EX_NOACTIVATE,
                name,
                name,
                WS_DISABLED,
                CW_USEDEFAULT,
                CW_USEDEFAULT,
                CW_USEDEFAULT,
                CW_USEDEFAULT,
                None,
                None,
                instance,
                None,
            )
        }
        .process()
    }

    pub fn alt_is_pressed() -> bool {
        let state = unsafe { GetKeyState(i32::from(VK_MENU.0)) };
        #[allow(clippy::cast_sign_loss)]
        let actual = (state as u16) & 0x8000;
        actual != 0
    }

    pub fn lbutton_is_pressed() -> bool {
        let state = unsafe { GetKeyState(i32::from(VK_LBUTTON.0)) };
        #[allow(clippy::cast_sign_loss)]
        let actual = (state as u16) & 0x8000;
        actual != 0
    }

    pub fn left_click() -> u32 {
        let inputs = [
            INPUT {
                r#type: INPUT_MOUSE,
                Anonymous: INPUT_0 {
                    mi: MOUSEINPUT {
                        dx: 0,
                        dy: 0,
                        mouseData: 0,
                        dwFlags: MOUSEEVENTF_LEFTDOWN,
                        time: 0,
                        dwExtraInfo: 0,
                    },
                },
            },
            INPUT {
                r#type: INPUT_MOUSE,
                Anonymous: INPUT_0 {
                    mi: MOUSEINPUT {
                        dx: 0,
                        dy: 0,
                        mouseData: 0,
                        dwFlags: MOUSEEVENTF_LEFTUP,
                        time: 0,
                        dwExtraInfo: 0,
                    },
                },
            },
        ];

        #[allow(clippy::cast_possible_wrap, clippy::cast_possible_truncation)]
        unsafe {
            SendInput(&inputs, std::mem::size_of::<INPUT>() as i32)
        }
    }

    pub fn wts_register_session_notification(hwnd: isize) -> Result<()> {
        unsafe { WTSRegisterSessionNotification(HWND(hwnd), 1) }.process()
    }
}
