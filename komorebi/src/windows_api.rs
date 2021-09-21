use std::collections::VecDeque;
use std::convert::TryFrom;
use std::convert::TryInto;
use std::ffi::c_void;

use color_eyre::eyre::anyhow;
use color_eyre::eyre::Error;
use color_eyre::Result;

use bindings::Handle;
use bindings::Result as WindowsCrateResult;
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
use bindings::Windows::Win32::Graphics::Gdi::MonitorFromPoint;
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
use bindings::Windows::Win32::UI::WindowsAndMessaging::GetForegroundWindow;
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
use bindings::Windows::Win32::UI::WindowsAndMessaging::SystemParametersInfoW;
use bindings::Windows::Win32::UI::WindowsAndMessaging::WindowFromPoint;
use bindings::Windows::Win32::UI::WindowsAndMessaging::GWL_EXSTYLE;
use bindings::Windows::Win32::UI::WindowsAndMessaging::GWL_STYLE;
use bindings::Windows::Win32::UI::WindowsAndMessaging::GW_HWNDNEXT;
use bindings::Windows::Win32::UI::WindowsAndMessaging::HWND_NOTOPMOST;
use bindings::Windows::Win32::UI::WindowsAndMessaging::HWND_TOPMOST;
use bindings::Windows::Win32::UI::WindowsAndMessaging::SET_WINDOW_POS_FLAGS;
use bindings::Windows::Win32::UI::WindowsAndMessaging::SHOW_WINDOW_CMD;
use bindings::Windows::Win32::UI::WindowsAndMessaging::SPIF_SENDCHANGE;
use bindings::Windows::Win32::UI::WindowsAndMessaging::SPI_GETACTIVEWINDOWTRACKING;
use bindings::Windows::Win32::UI::WindowsAndMessaging::SPI_SETACTIVEWINDOWTRACKING;
use bindings::Windows::Win32::UI::WindowsAndMessaging::SW_HIDE;
use bindings::Windows::Win32::UI::WindowsAndMessaging::SW_MAXIMIZE;
use bindings::Windows::Win32::UI::WindowsAndMessaging::SW_RESTORE;
use bindings::Windows::Win32::UI::WindowsAndMessaging::SYSTEM_PARAMETERS_INFO_ACTION;
use bindings::Windows::Win32::UI::WindowsAndMessaging::SYSTEM_PARAMETERS_INFO_UPDATE_FLAGS;
use bindings::Windows::Win32::UI::WindowsAndMessaging::WINDOW_LONG_PTR_INDEX;
use bindings::Windows::Win32::UI::WindowsAndMessaging::WNDENUMPROC;
use komorebi_core::Rect;

use crate::container::Container;
use crate::monitor;
use crate::monitor::Monitor;
use crate::ring::Ring;
use crate::set_window_position::SetWindowPosition;
use crate::windows_callbacks;

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

impl_from_integer_for_windows_result!(isize, u32, i32);

impl<T, E> From<WindowsResult<T, E>> for Result<T, E> {
    fn from(result: WindowsResult<T, E>) -> Self {
        match result {
            WindowsResult::Err(error) => Self::Err(error),
            WindowsResult::Ok(ok) => Self::Ok(ok),
        }
    }
}

pub trait ProcessWindowsCrateResult<T> {
    fn process(self) -> Result<T>;
}

macro_rules! impl_process_windows_crate_result {
    ( $($input:ty => $deref:ty),+ $(,)? ) => (
        paste::paste! {
            $(
                impl ProcessWindowsCrateResult<$deref> for WindowsCrateResult<$input> {
                    fn process(self) -> Result<$deref> {
                        match self {
                            Ok(value) => Ok(value.0),
                            Err(error) => Err(error.into()),
                        }
                    }
                }
            )+
        }
    );
}

impl_process_windows_crate_result!(
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
        unsafe {
            EnumDisplayMonitors(
                HDC(0),
                std::ptr::null_mut(),
                Option::from(callback),
                LPARAM(callback_data_address),
            )
        }
        .ok()
        .process()
    }

    pub fn valid_hmonitors() -> Result<Vec<isize>> {
        let mut monitors: Vec<isize> = vec![];
        let monitors_ref: &mut Vec<isize> = monitors.as_mut();
        Self::enum_display_monitors(
            windows_callbacks::valid_display_monitors,
            monitors_ref as *mut Vec<isize> as isize,
        )?;

        Ok(monitors)
    }

    pub fn load_monitor_information(monitors: &mut Ring<Monitor>) -> Result<()> {
        Self::enum_display_monitors(
            windows_callbacks::enum_display_monitor,
            monitors as *mut Ring<Monitor> as isize,
        )
    }

    pub fn enum_windows(callback: WNDENUMPROC, callback_data_address: isize) -> Result<()> {
        unsafe { EnumWindows(Option::from(callback), LPARAM(callback_data_address)) }
            .ok()
            .process()
    }

    pub fn load_workspace_information(monitors: &mut Ring<Monitor>) -> Result<()> {
        for monitor in monitors.elements_mut() {
            let monitor_id = monitor.id();
            if let Some(workspace) = monitor.workspaces_mut().front_mut() {
                // EnumWindows will enumerate through windows on all monitors
                Self::enum_windows(
                    windows_callbacks::enum_window,
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
                        if Self::monitor_from_window(window.hwnd()) != monitor_id {
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
        unsafe { AllowSetForegroundWindow(process_id) }
            .ok()
            .process()
    }

    pub fn monitor_from_window(hwnd: HWND) -> isize {
        // MONITOR_DEFAULTTONEAREST ensures that the return value will never be NULL
        // https://docs.microsoft.com/en-us/windows/win32/api/winuser/nf-winuser-monitorfromwindow
        unsafe { MonitorFromWindow(hwnd, MONITOR_DEFAULTTONEAREST) }.0
    }

    pub fn monitor_from_point(point: POINT) -> isize {
        // MONITOR_DEFAULTTONEAREST ensures that the return value will never be NULL
        // https://docs.microsoft.com/en-us/windows/win32/api/winuser/nf-winuser-monitorfromwindow
        unsafe { MonitorFromPoint(point, MONITOR_DEFAULTTONEAREST) }.0
    }

    pub fn position_window(hwnd: HWND, layout: &Rect, top: bool) -> Result<()> {
        let flags = SetWindowPosition::NO_ACTIVATE;

        let position = if top { HWND_TOPMOST } else { HWND_NOTOPMOST };
        Self::set_window_pos(hwnd, layout, position, flags.bits())
    }

    pub fn set_window_pos(hwnd: HWND, layout: &Rect, position: HWND, flags: u32) -> Result<()> {
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
        .ok()
        .process()
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

    pub fn maximize_window(hwnd: HWND) {
        Self::show_window(hwnd, SW_MAXIMIZE);
    }

    pub fn foreground_window() -> Result<isize> {
        unsafe { GetForegroundWindow() }.ok().process()
    }

    pub fn set_foreground_window(hwnd: HWND) -> Result<()> {
        unsafe { SetForegroundWindow(hwnd) }.ok().process()
    }

    #[allow(dead_code)]
    pub fn top_window() -> Result<isize> {
        unsafe { GetTopWindow(HWND::default()) }.ok().process()
    }

    pub fn desktop_window() -> Result<isize> {
        unsafe { GetDesktopWindow() }.ok().process()
    }

    #[allow(dead_code)]
    pub fn next_window(hwnd: HWND) -> Result<isize> {
        unsafe { GetWindow(hwnd, GW_HWNDNEXT) }.ok().process()
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
        unsafe { GetWindowRect(hwnd, &mut rect) }.ok().process()?;

        Ok(Rect::from(rect))
    }

    fn set_cursor_pos(x: i32, y: i32) -> Result<()> {
        unsafe { SetCursorPos(x, y) }.ok().process()
    }

    pub fn cursor_pos() -> Result<POINT> {
        let mut cursor_pos = POINT::default();
        unsafe { GetCursorPos(&mut cursor_pos) }.ok().process()?;

        Ok(cursor_pos)
    }

    pub fn window_from_point(point: POINT) -> Result<isize> {
        unsafe { WindowFromPoint(point) }.ok().process()
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
        unsafe { AttachThreadInput(thread_id, target_thread_id, attach) }
            .ok()
            .process()
    }

    pub fn set_focus(hwnd: HWND) -> Result<()> {
        unsafe { SetFocus(hwnd) }.ok().map(|_| ()).process()
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
        Result::from(WindowsResult::from(unsafe {
            GetWindowLongPtrW(hwnd, index)
        }))
    }

    #[allow(dead_code)]
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
        unsafe { OpenProcess(access_rights, inherit_handle, process_id) }
            .ok()
            .process()
    }

    pub fn process_handle(process_id: u32) -> Result<HANDLE> {
        Self::open_process(PROCESS_QUERY_INFORMATION, false, process_id)
    }

    pub fn exe_path(handle: HANDLE) -> Result<String> {
        let mut len = 260_u32;
        let mut path: Vec<u16> = vec![0; len as usize];
        let text_ptr = path.as_mut_ptr();

        unsafe {
            QueryFullProcessImageNameW(
                handle,
                PROCESS_NAME_FORMAT(0),
                PWSTR(text_ptr),
                &mut len as *mut u32,
            )
        }
        .ok()
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

        unsafe { GetMonitorInfoW(hmonitor, (&mut monitor_info as *mut MONITORINFO).cast()) }
            .ok()
            .process()?;

        Ok(monitor_info)
    }

    pub fn monitor(hmonitor: isize) -> Result<Monitor> {
        let monitor_info = Self::monitor_info_w(HMONITOR(hmonitor))?;

        Ok(monitor::new(
            hmonitor,
            monitor_info.rcMonitor.into(),
            monitor_info.rcWork.into(),
        ))
    }

    #[allow(dead_code)]
    pub fn system_parameters_info_w(
        action: SYSTEM_PARAMETERS_INFO_ACTION,
        ui_param: u32,
        pv_param: *mut c_void,
        update_flags: SYSTEM_PARAMETERS_INFO_UPDATE_FLAGS,
    ) -> Result<()> {
        unsafe { SystemParametersInfoW(action, ui_param, pv_param, update_flags) }
            .ok()
            .process()
    }

    #[allow(dead_code)]
    pub fn focus_follows_mouse() -> Result<bool> {
        let mut is_enabled: BOOL = unsafe { std::mem::zeroed() };

        Self::system_parameters_info_w(
            SPI_GETACTIVEWINDOWTRACKING,
            0,
            (&mut is_enabled as *mut BOOL).cast(),
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
}
