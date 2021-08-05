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
use crate::winevent::WinEvent;
use crate::winevent_listener::WINEVENT_CALLBACK_CHANNEL;

pub extern "system" fn enum_display_monitor(
    hmonitor: HMONITOR,
    _: HDC,
    _: *mut RECT,
    lparam: LPARAM,
) -> BOOL {
    let monitors = unsafe { &mut *(lparam.0 as *mut Ring<Monitor>) };
    if let Ok(m) = WindowsApi::monitor(hmonitor) {
        monitors.elements_mut().push_back(m);
    }

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
    let event_type = if let Some(event) = WindowManagerEvent::from_win_event(winevent, window) {
        event
    } else {
        // Some apps like Firefox don't send ObjectCreate or ObjectShow on launch
        // This spams the message queue, but I don't know what else to do. On launch
        // it only sends the following WinEvents :/
        //
        // [yatta\src\windows_event.rs:110] event = 32780 ObjectNameChange
        // [yatta\src\windows_event.rs:110] event = 32779 ObjectLocationChange
        let object_name_change_on_launch =
            vec!["firefox.exe".to_string(), "idea64.exe".to_string()];

        if let Ok(exe) = window.exe() {
            if winevent == WinEvent::ObjectNameChange {
                if object_name_change_on_launch.contains(&exe) {
                    WindowManagerEvent::Show(winevent, window)
                } else {
                    return;
                }
            } else {
                return;
            }
        } else {
            return;
        }
    };

    if let Ok(should_manage) = window.should_manage(Option::from(event_type)) {
        if should_manage {
            WINEVENT_CALLBACK_CHANNEL
                .lock()
                .unwrap()
                .0
                .send(event_type)
                .expect("could not send message on WINEVENT_CALLBACK_CHANNEL");
        }
    }
}
