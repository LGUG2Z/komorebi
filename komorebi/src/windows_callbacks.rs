use chrono::TimeZone;
use chrono::Utc;
use std::collections::VecDeque;
use std::ops::Sub;
use std::time::Duration;
use std::time::UNIX_EPOCH;

use windows::Win32::Foundation::BOOL;
use windows::Win32::Foundation::HWND;
use windows::Win32::Foundation::LPARAM;
use windows::Win32::UI::Accessibility::HWINEVENTHOOK;

use crate::container::Container;
use crate::window::RuleDebug;
use crate::window::Window;
use crate::window_manager_event::WindowManagerEvent;
use crate::windows_api::WindowsApi;
use crate::winevent::WinEvent;
use crate::winevent_listener;

pub extern "system" fn enum_window(hwnd: HWND, lparam: LPARAM) -> BOOL {
    let containers = unsafe { &mut *(lparam.0 as *mut VecDeque<Container>) };

    let is_visible = WindowsApi::is_window_visible(hwnd);
    let is_window = WindowsApi::is_window(hwnd);
    let is_minimized = WindowsApi::is_iconic(hwnd);
    let is_maximized = WindowsApi::is_zoomed(hwnd);

    if is_visible && is_window && !is_minimized {
        let window = Window::from(hwnd);

        if let Ok(should_manage) = window.should_manage(None, &mut RuleDebug::default()) {
            if should_manage {
                if is_maximized {
                    WindowsApi::restore_window(hwnd);
                }

                let mut container = Container::default();
                container.windows_mut().push_back(window);
                containers.push_back(container);
            }
        }
    }

    true.into()
}

pub extern "system" fn alt_tab_windows(hwnd: HWND, lparam: LPARAM) -> BOOL {
    let windows = unsafe { &mut *(lparam.0 as *mut Vec<Window>) };

    let is_visible = WindowsApi::is_window_visible(hwnd);
    let is_window = WindowsApi::is_window(hwnd);
    let is_minimized = WindowsApi::is_iconic(hwnd);

    if is_visible && is_window && !is_minimized {
        let window = Window::from(hwnd);

        if let Ok(should_manage) = window.should_manage(None, &mut RuleDebug::default()) {
            if should_manage {
                windows.push(window);
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
    dwms_event_time: u32,
) {
    // OBJID_WINDOW
    if id_object != 0 {
        return;
    }

    let millis_since_boot = WindowsApi::tick_count();
    let system_time_now = std::time::SystemTime::now();

    let boot_time = system_time_now
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .sub(Duration::from_millis(millis_since_boot))
        .as_secs();

    let boot_time_utc = Utc.timestamp_opt(boot_time as i64, 0).unwrap();
    let timestamp = boot_time_utc + Duration::from_millis(dwms_event_time as u64);

    let window = Window::from(hwnd);

    let winevent = match WinEvent::try_from(event) {
        Ok(event) => event,
        Err(_) => return,
    };

    let event_type = match WindowManagerEvent::from_win_event(winevent, window, timestamp) {
        None => {
            tracing::trace!(
                "Unhandled WinEvent: {winevent} (hwnd: {}, exe: {}, title: {}, class: {})",
                window.hwnd,
                window.exe().unwrap_or_default(),
                window.title().unwrap_or_default(),
                window.class().unwrap_or_default()
            );

            return;
        }
        Some(event) => event,
    };

    winevent_listener::event_tx()
        .send(event_type)
        .expect("could not send message on winevent_listener::event_tx");
}
