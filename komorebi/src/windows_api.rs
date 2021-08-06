use std::collections::VecDeque;
use std::convert::TryFrom;
use std::convert::TryInto;

use color_eyre::eyre::ContextCompat;
use color_eyre::Result;
use eyre::Error;

use bindings::Windows::Win32::Foundation::BOOL;
use bindings::Windows::Win32::Foundation::HANDLE;
use bindings::Windows::Win32::Foundation::HWND;
use bindings::Windows::Win32::Foundation::LPARAM;
use bindings::Windows::Win32::Foundation::POINT;
use bindings::Windows::Win32::Foundation::PWSTR;
use bindings::Windows::Win32::Foundation::RECT;
use bindings::Windows::Win32::Graphics::Dwm::DwmGetWindowAttribute;
use bindings::Windows::Win32::Graphics::Dwm::DWMWA_CLOAKED;
use bindings::Windows::Win32::Graphics::Dwm::DWMWA_EXTENDED_FRAME_BOUNDS;
use bindings::Windows::Win32::Graphics::Dwm::DWMWINDOWATTRIBUTE;
use bindings::Windows::Win32::Graphics::Dwm::DWM_CLOAKED_APP;
use bindings::Windows::Win32::Graphics::Dwm::DWM_CLOAKED_INHERITED;
use bindings::Windows::Win32::Graphics::Dwm::DWM_CLOAKED_SHELL;
use bindings::Windows::Win32::Graphics::Gdi::EnumDisplayMonitors;
use bindings::Windows::Win32::Graphics::Gdi::GetMonitorInfoW;
use bindings::Windows::Win32::Graphics::Gdi::MonitorFromWindow;
use bindings::Windows::Win32::Graphics::Gdi::HDC;
use bindings::Windows::Win32::Graphics::Gdi::HMONITOR;
use bindings::Windows::Win32::Graphics::Gdi::MONITORENUMPROC;
use bindings::Windows::Win32::Graphics::Gdi::MONITORINFO;
use bindings::Windows::Win32::Graphics::Gdi::MONITOR_DEFAULTTONEAREST;
use bindings::Windows::Win32::System::Threading::AttachThreadInput;
use bindings::Windows::Win32::System::Threading::GetCurrentProcessId;
use bindings::Windows::Win32::System::Threading::GetCurrentThreadId;
use bindings::Windows::Win32::System::Threading::OpenProcess;
use bindings::Windows::Win32::System::Threading::QueryFullProcessImageNameW;
use bindings::Windows::Win32::System::Threading::PROCESS_ACCESS_RIGHTS;
use bindings::Windows::Win32::System::Threading::PROCESS_NAME_FORMAT;
use bindings::Windows::Win32::System::Threading::PROCESS_QUERY_INFORMATION;
use bindings::Windows::Win32::UI::KeyboardAndMouseInput::SetFocus;
use bindings::Windows::Win32::UI::WindowsAndMessaging::AllowSetForegroundWindow;
use bindings::Windows::Win32::UI::WindowsAndMessaging::EnumWindows;
use bindings::Windows::Win32::UI::WindowsAndMessaging::GetCursorPos;
use bindings::Windows::Win32::UI::WindowsAndMessaging::GetDesktopWindow;
use bindings::Windows::Win32::UI::WindowsAndMessaging::GetTopWindow;
use bindings::Windows::Win32::UI::WindowsAndMessaging::GetWindow;
use bindings::Windows::Win32::UI::WindowsAndMessaging::GetWindowLongPtrW;
use bindings::Windows::Win32::UI::WindowsAndMessaging::GetWindowRect;
use bindings::Windows::Win32::UI::WindowsAndMessaging::GetWindowTextW;
use bindings::Windows::Win32::UI::WindowsAndMessaging::GetWindowThreadProcessId;
use bindings::Windows::Win32::UI::WindowsAndMessaging::IsIconic;
use bindings::Windows::Win32::UI::WindowsAndMessaging::IsWindow;
use bindings::Windows::Win32::UI::WindowsAndMessaging::IsWindowVisible;
use bindings::Windows::Win32::UI::WindowsAndMessaging::RealGetWindowClassW;
use bindings::Windows::Win32::UI::WindowsAndMessaging::SetCursorPos;
use bindings::Windows::Win32::UI::WindowsAndMessaging::SetForegroundWindow;
use bindings::Windows::Win32::UI::WindowsAndMessaging::SetWindowLongPtrW;
use bindings::Windows::Win32::UI::WindowsAndMessaging::SetWindowPos;
use bindings::Windows::Win32::UI::WindowsAndMessaging::ShowWindow;
use bindings::Windows::Win32::UI::WindowsAndMessaging::GWL_EXSTYLE;
use bindings::Windows::Win32::UI::WindowsAndMessaging::GWL_STYLE;
use bindings::Windows::Win32::UI::WindowsAndMessaging::GW_HWNDNEXT;
use bindings::Windows::Win32::UI::WindowsAndMessaging::HWND_NOTOPMOST;
use bindings::Windows::Win32::UI::WindowsAndMessaging::HWND_TOPMOST;
use bindings::Windows::Win32::UI::WindowsAndMessaging::SET_WINDOW_POS_FLAGS;
use bindings::Windows::Win32::UI::WindowsAndMessaging::SHOW_WINDOW_CMD;
use bindings::Windows::Win32::UI::WindowsAndMessaging::SW_HIDE;
use bindings::Windows::Win32::UI::WindowsAndMessaging::SW_RESTORE;
use bindings::Windows::Win32::UI::WindowsAndMessaging::WINDOW_LONG_PTR_INDEX;
use bindings::Windows::Win32::UI::WindowsAndMessaging::WNDENUMPROC;
use komorebi_core::Rect;

use crate::container::Container;
use crate::monitor;
use crate::monitor::Monitor;
use crate::ring::Ring;
use crate::set_window_position::SetWindowPosition;
use crate::windows_callbacks;
use crate::workspace::Workspace;

pub enum WindowsResult<T, E> {
    Err(E),
    Ok(T),
}

impl From<BOOL> for WindowsResult<(), Error> {
    fn from(return_value: BOOL) -> Self {
        if return_value.as_bool() {
            Self::Ok(())
        } else {
            Self::Err(std::io::Error::last_os_error().into())
        }
    }
}

impl From<HWND> for WindowsResult<isize, Error> {
    fn from(return_value: HWND) -> Self {
        if return_value.is_null() {
            Self::Err(std::io::Error::last_os_error().into())
        } else {
            Self::Ok(return_value.0)
        }
    }
}

impl From<HANDLE> for WindowsResult<HANDLE, Error> {
    fn from(return_value: HANDLE) -> Self {
        if return_value.is_null() {
            Self::Err(std::io::Error::last_os_error().into())
        } else {
            Self::Ok(return_value)
        }
    }
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

impl_from_integer_for_windows_result!(isize, u32, i32);

impl<T, E> From<WindowsResult<T, E>> for Result<T, E> {
    fn from(result: WindowsResult<T, E>) -> Self {
        match result {
            WindowsResult::Err(error) => Self::Err(error),
            WindowsResult::Ok(ok) => Self::Ok(ok),
        }
    }
}

pub struct WindowsApi;

impl WindowsApi {
    pub fn enum_display_monitors(
        callback: MONITORENUMPROC,
        callback_data_address: isize,
    ) -> Result<()> {
        Result::from(WindowsResult::from(unsafe {
            EnumDisplayMonitors(
                HDC(0),
                std::ptr::null_mut(),
                Option::from(callback),
                LPARAM(callback_data_address),
            )
        }))
    }

    pub fn load_monitor_information(monitors: &mut Ring<Monitor>) -> Result<()> {
        Self::enum_display_monitors(
            windows_callbacks::enum_display_monitor,
            monitors as *mut Ring<Monitor> as isize,
        )
    }

    pub fn enum_windows(callback: WNDENUMPROC, callback_data_address: isize) -> Result<()> {
        Result::from(WindowsResult::from(unsafe {
            EnumWindows(Option::from(callback), LPARAM(callback_data_address))
        }))
    }
    pub fn load_workspace_information(monitors: &mut Ring<Monitor>) -> Result<()> {
        for monitor in monitors.elements_mut() {
            if monitor.workspaces().is_empty() {
                let mut workspace = Workspace::default();

                // EnumWindows will enumerate through windows on all monitors
                Self::enum_windows(
                    windows_callbacks::enum_window,
                    workspace.containers_mut() as *mut VecDeque<Container> as isize,
                )?;

                // So we have to prune each monitor's primary workspace of undesired windows here
                let mut windows_on_other_monitors = vec![];

                for container in workspace.containers_mut() {
                    for window in container.windows() {
                        if Self::monitor_from_window(window.hwnd()) != monitor.id() {
                            windows_on_other_monitors.push(window.hwnd().0);
                        }
                    }
                }

                for hwnd in windows_on_other_monitors {
                    workspace.remove_window(hwnd)?;
                }

                monitor.workspaces_mut().push_back(workspace);
            }
        }

        Ok(())
    }

    pub fn allow_set_foreground_window(process_id: u32) -> Result<()> {
        Result::from(WindowsResult::from(unsafe {
            AllowSetForegroundWindow(process_id)
        }))
    }

    pub fn monitor_from_window(hwnd: HWND) -> isize {
        // MONITOR_DEFAULTTONEAREST ensures that the return value will never be NULL
        // https://docs.microsoft.com/en-us/windows/win32/api/winuser/nf-winuser-monitorfromwindow
        unsafe { MonitorFromWindow(hwnd, MONITOR_DEFAULTTONEAREST) }.0
    }

    pub fn position_window(hwnd: HWND, layout: &Rect, top: bool) -> Result<()> {
        let flags = SetWindowPosition::NO_ACTIVATE;

        let position = if top { HWND_TOPMOST } else { HWND_NOTOPMOST };
        Self::set_window_pos(hwnd, layout, position, flags.bits())
    }

    pub fn set_window_pos(hwnd: HWND, layout: &Rect, position: HWND, flags: u32) -> Result<()> {
        Result::from(WindowsResult::from(unsafe {
            SetWindowPos(
                hwnd,
                position,
                layout.left,
                layout.top,
                layout.right,
                layout.bottom,
                SET_WINDOW_POS_FLAGS(flags),
            )
        }))
    }

    fn show_window(hwnd: HWND, command: SHOW_WINDOW_CMD) {
        // BOOL is returned but does not signify whether or not the operation was succesful
        // https://docs.microsoft.com/en-us/windows/win32/api/winuser/nf-winuser-showwindow
        unsafe { ShowWindow(hwnd, command) };
    }

    pub fn hide_window(hwnd: HWND) {
        Self::show_window(hwnd, SW_HIDE);
    }

    pub fn restore_window(hwnd: HWND) {
        Self::show_window(hwnd, SW_RESTORE);
    }

    pub fn set_foreground_window(hwnd: HWND) -> Result<()> {
        match WindowsResult::from(unsafe { SetForegroundWindow(hwnd) }) {
            WindowsResult::Ok(_) => Ok(()),
            WindowsResult::Err(error) => {
                // TODO: Figure out the odd behaviour here, docs state that a zero value means
                // TODO: that the window was not brought to the foreground, but this contradicts
                // TODO: the behaviour that I have observed which resulted in this check
                if error.to_string() == "The operation completed successfully. (os error 0)" {
                    Ok(())
                } else {
                    Err(error)
                }
            }
        }
    }

    pub fn top_window() -> Result<isize> {
        Result::from(WindowsResult::from(unsafe { GetTopWindow(HWND::NULL).0 }))
    }

    pub fn desktop_window() -> Result<isize> {
        Result::from(WindowsResult::from(unsafe { GetDesktopWindow() }))
    }

    pub fn next_window(hwnd: HWND) -> Result<isize> {
        Result::from(WindowsResult::from(unsafe {
            GetWindow(hwnd, GW_HWNDNEXT).0
        }))
    }

    pub fn top_visible_window() -> Result<isize> {
        let hwnd = Self::top_window()?;
        let mut next_hwnd = hwnd;

        while next_hwnd != 0 {
            if Self::is_window_visible(HWND(next_hwnd)) {
                return Ok(next_hwnd);
            }

            next_hwnd = Self::next_window(HWND(next_hwnd))?;
        }

        Err(eyre::anyhow!("could not find next window"))
    }

    pub fn window_rect(hwnd: HWND) -> Result<Rect> {
        let mut rect = unsafe { std::mem::zeroed() };

        Result::from(WindowsResult::from(unsafe {
            GetWindowRect(hwnd, &mut rect)
        }))?;

        Ok(Rect::from(rect))
    }

    fn set_cursor_pos(x: i32, y: i32) -> Result<()> {
        Result::from(WindowsResult::from(unsafe { SetCursorPos(x, y) }))
    }

    pub fn cursor_pos() -> Result<POINT> {
        let mut cursor_pos: POINT = unsafe { std::mem::zeroed() };

        Result::from(WindowsResult::from(unsafe {
            GetCursorPos(&mut cursor_pos)
        }))?;

        Ok(cursor_pos)
    }

    pub fn center_cursor_in_rect(rect: &Rect) -> Result<()> {
        Self::set_cursor_pos(rect.left + (rect.right / 2), rect.top + (rect.bottom / 2))
    }

    pub fn window_thread_process_id(hwnd: HWND) -> (u32, u32) {
        let mut process_id: u32 = 0;

        // Behaviour is undefined if an invalid HWND is given
        // https://docs.microsoft.com/en-us/windows/win32/api/winuser/nf-winuser-getwindowthreadprocessid
        let thread_id = unsafe { GetWindowThreadProcessId(hwnd, &mut process_id) };

        (process_id, thread_id)
    }

    pub fn current_thread_id() -> u32 {
        unsafe { GetCurrentThreadId() }
    }

    pub fn current_process_id() -> u32 {
        unsafe { GetCurrentProcessId() }
    }

    pub fn attach_thread_input(thread_id: u32, target_thread_id: u32, attach: bool) -> Result<()> {
        Result::from(WindowsResult::from(unsafe {
            AttachThreadInput(thread_id, target_thread_id, attach)
        }))
    }

    pub fn set_focus(hwnd: HWND) -> Result<()> {
        match WindowsResult::from(unsafe { SetFocus(hwnd) }) {
            WindowsResult::Ok(_) => Ok(()),
            WindowsResult::Err(error) => {
                // If the window is not attached to the calling thread's message queue, the return value is NULL
                // https://docs.microsoft.com/en-us/windows/win32/api/winuser/nf-winuser-setfocus
                if error.to_string() == "The operation completed successfully. (os error 0)" {
                    Ok(())
                } else {
                    Err(error)
                }
            }
        }
    }

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
        Result::from(WindowsResult::from(unsafe {
            GetWindowLongPtrW(hwnd, index)
        }))
    }

    pub fn update_style(hwnd: HWND, new_value: isize) -> Result<()> {
        Self::set_window_long_ptr_w(hwnd, GWL_STYLE, new_value)
    }

    pub fn window_text_w(hwnd: HWND) -> Result<String> {
        let mut text: [u16; 512] = [0; 512];
        match WindowsResult::from(unsafe {
            GetWindowTextW(hwnd, PWSTR(text.as_mut_ptr()), text.len().try_into()?)
        }) {
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
        Result::from(WindowsResult::from(unsafe {
            OpenProcess(access_rights, inherit_handle, process_id)
        }))
    }

    pub fn process_handle(process_id: u32) -> Result<HANDLE> {
        Self::open_process(PROCESS_QUERY_INFORMATION, false, process_id)
    }

    pub fn exe_path(handle: HANDLE) -> Result<String> {
        let mut len = 260_u32;
        let mut path: Vec<u16> = vec![0; len as usize];
        let text_ptr = path.as_mut_ptr();

        Result::from(WindowsResult::from(unsafe {
            QueryFullProcessImageNameW(
                handle,
                PROCESS_NAME_FORMAT(0),
                PWSTR(text_ptr),
                &mut len as *mut u32,
            )
        }))?;

        Ok(String::from_utf16(&path[..len as usize])?)
    }

    pub fn exe(handle: HANDLE) -> Result<String> {
        Ok(Self::exe_path(handle)?
            .split('\\')
            .last()
            .context("there is no last element")?
            .to_string())
    }

    pub fn real_window_class_w(hwnd: HWND) -> Result<String> {
        const BUF_SIZE: usize = 512;
        let mut class: [u16; BUF_SIZE] = [0; BUF_SIZE];

        let len = Result::from(WindowsResult::from(unsafe {
            RealGetWindowClassW(hwnd, PWSTR(class.as_mut_ptr()), u32::try_from(BUF_SIZE)?)
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
                std::mem::transmute::<_, u32>(attribute),
                (value as *mut T).cast(),
                u32::try_from(std::mem::size_of::<T>())?,
            )?;
        }

        Ok(())
    }

    #[allow(dead_code)]
    pub fn window_rect_with_extended_frame_bounds(hwnd: HWND) -> Result<Rect> {
        let mut rect = RECT::default();
        Self::dwm_get_window_attribute(hwnd, DWMWA_EXTENDED_FRAME_BOUNDS, &mut rect)?;

        Ok(Rect::from(rect))
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

    pub fn monitor_info_w(hmonitor: HMONITOR) -> Result<MONITORINFO> {
        let mut monitor_info: MONITORINFO = unsafe { std::mem::zeroed() };
        monitor_info.cbSize = u32::try_from(std::mem::size_of::<MONITORINFO>())?;

        Result::from(WindowsResult::from(unsafe {
            GetMonitorInfoW(hmonitor, (&mut monitor_info as *mut MONITORINFO).cast())
        }))?;

        Ok(monitor_info)
    }

    pub fn monitor(hmonitor: HMONITOR) -> Result<Monitor> {
        let monitor_info = Self::monitor_info_w(hmonitor)?;

        Ok(monitor::new(
            hmonitor.0,
            monitor_info.rcMonitor.into(),
            monitor_info.rcWork.into(),
        ))
    }
}
