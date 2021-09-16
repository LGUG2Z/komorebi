use std::collections::VecDeque;

use bindings::Windows::Win32::Foundation::BOOL;
use bindings::Windows::Win32::Foundation::HWND;
use bindings::Windows::Win32::Foundation::LPARAM;
use bindings::Windows::Win32::Foundation::RECT;
use bindings::Windows::Win32::Graphics::Gdi::HDC;
use bindings::Windows::Win32::Graphics::Gdi::HMONITOR;
use bindings::Windows::Win32::UI::Accessibility::HWINEVENTHOOK;

use crate::container::Container;
use crate::monitor::Monitor;
use crate::ring::Ring;
use crate::window::Window;
use crate::window_manager_event::WindowManagerEvent;
use crate::windows_api::WindowsApi;
use crate::winevent_listener::WINEVENT_CALLBACK_CHANNEL;

pub extern "system" fn valid_display_monitors(
    hmonitor: HMONITOR,
    _: HDC,
    _: *mut RECT,
    lparam: LPARAM,
) -> BOOL {
    let monitors = unsafe { &mut *(lparam.0 as *mut Vec<isize>) };
    monitors.push(hmonitor.0);
    true.into()
}

pub extern "system" fn enum_display_monitor(
    hmonitor: HMONITOR,
    _: HDC,
    _: *mut RECT,
    lparam: LPARAM,
) -> BOOL {
    let monitors = unsafe { &mut *(lparam.0 as *mut Ring<Monitor>) };

    // Don't duplicate a monitor that is already being managed
    for monitor in monitors.elements() {
        if monitor.id() == hmonitor.0 {
            return true.into();
        }
    }

    if let Ok(m) = WindowsApi::monitor(hmonitor) {
        monitors.elements_mut().push_back(m);
    }

    true.into()
}

#[allow(dead_code)]
pub extern "system" fn valid_hwnds(hwnd: HWND, lparam: LPARAM) -> BOOL {
    let hwnds = unsafe { &mut *(lparam.0 as *mut Vec<isize>) };
    hwnds.push(hwnd.0);
    true.into()
}

pub extern "system" fn enum_window(hwnd: HWND, lparam: LPARAM) -> BOOL {
    let containers = unsafe { &mut *(lparam.0 as *mut VecDeque<Container>) };

    let is_visible = WindowsApi::is_window_visible(hwnd);
    let is_window = WindowsApi::is_window(hwnd);
    let is_minimized = WindowsApi::is_iconic(hwnd);

    if is_visible && is_window && !is_minimized {
        let window = Window { hwnd: hwnd.0 };

        if let Ok(should_manage) = window.should_manage(None) {
            if should_manage {
                let mut container = Container::default();
                container.windows_mut().push_back(window);
                containers.push_back(container);
            }
        }
    }

    true.into()
}

pub extern "system" fn win_event_hook(
    _h_win_event_hook: HWINEVENTHOOK,
    event: u32,
    hwnd: HWND,
    id_object: i32,
    _id_child: i32,
    _id_event_thread: u32,
    _dwms_event_time: u32,
) {
    // OBJID_WINDOW
    if id_object != 0 {
        return;
    }

    let window = Window { hwnd: hwnd.0 };

    let winevent = unsafe { ::std::mem::transmute(event) };
    let event_type = match WindowManagerEvent::from_win_event(winevent, window) {
        None => return,
        Some(event) => event,
    };

    if let Ok(should_manage) = window.should_manage(Option::from(event_type)) {
        if should_manage {
            WINEVENT_CALLBACK_CHANNEL
                .lock()
                .0
                .send(event_type)
                .expect("could not send message on WINEVENT_CALLBACK_CHANNEL");
        }
    }
}
