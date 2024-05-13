use std::collections::VecDeque;
use widestring::U16CStr;

use windows::Win32::Foundation::BOOL;
use windows::Win32::Foundation::HWND;
use windows::Win32::Foundation::LPARAM;
use windows::Win32::Foundation::LRESULT;
use windows::Win32::Foundation::RECT;
use windows::Win32::Foundation::WPARAM;
use windows::Win32::Graphics::Gdi::HDC;
use windows::Win32::Graphics::Gdi::HMONITOR;
use windows::Win32::UI::Accessibility::HWINEVENTHOOK;
use windows::Win32::UI::WindowsAndMessaging::DefWindowProcW;
use windows::Win32::UI::WindowsAndMessaging::DBT_DEVNODES_CHANGED;
use windows::Win32::UI::WindowsAndMessaging::SPI_ICONVERTICALSPACING;
use windows::Win32::UI::WindowsAndMessaging::SPI_SETWORKAREA;
use windows::Win32::UI::WindowsAndMessaging::WM_DEVICECHANGE;
use windows::Win32::UI::WindowsAndMessaging::WM_DISPLAYCHANGE;
use windows::Win32::UI::WindowsAndMessaging::WM_SETTINGCHANGE;

use crate::container::Container;
use crate::monitor::Monitor;
use crate::ring::Ring;
use crate::window::RuleDebug;
use crate::window::Window;
use crate::window_manager_event::WindowManagerEvent;
use crate::windows_api::WindowsApi;
use crate::winevent::WinEvent;
use crate::winevent_listener;
use crate::DISPLAY_INDEX_PREFERENCES;
use crate::MONITOR_INDEX_PREFERENCES;

pub extern "system" fn valid_display_monitors(
    hmonitor: HMONITOR,
    _: HDC,
    _: *mut RECT,
    lparam: LPARAM,
) -> BOOL {
    let monitors = unsafe { &mut *(lparam.0 as *mut Vec<(String, isize)>) };
    if let Ok(m) = WindowsApi::monitor(hmonitor.0) {
        monitors.push((m.name().to_string(), hmonitor.0));
    }

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

    let current_index = monitors.elements().len();

    if let Ok(mut m) = WindowsApi::monitor(hmonitor.0) {
        #[allow(clippy::cast_possible_truncation)]
        if let Ok(d) = WindowsApi::enum_display_devices(current_index as u32, None) {
            let name = U16CStr::from_slice_truncate(d.DeviceName.as_ref())
                .expect("display device name was not a valid u16 c string")
                .to_ustring()
                .to_string_lossy()
                .trim_start_matches(r"\\.\")
                .to_string();

            if name.eq(m.name()) {
                if let Ok(device) = WindowsApi::enum_display_devices(0, Some(d.DeviceName.as_ptr()))
                {
                    let id = U16CStr::from_slice_truncate(device.DeviceID.as_ref())
                        .expect("display device id was not a valid u16 c string")
                        .to_ustring()
                        .to_string_lossy()
                        .trim_start_matches(r"\\?\")
                        .to_string();

                    let mut split: Vec<_> = id.split('#').collect();
                    split.remove(0);
                    split.remove(split.len() - 1);

                    m.set_device(Option::from(split[0].to_string()));
                    m.set_device_id(Option::from(split.join("-")));
                }
            }
        }

        let monitor_index_preferences = MONITOR_INDEX_PREFERENCES.lock();
        let mut index_preference = None;
        for (index, monitor_size) in &*monitor_index_preferences {
            if m.size() == monitor_size {
                index_preference = Option::from(index);
            }
        }

        let display_index_preferences = DISPLAY_INDEX_PREFERENCES.lock();
        for (index, device) in &*display_index_preferences {
            if let Some(known_device) = m.device_id() {
                if device == known_device {
                    index_preference = Option::from(index);
                }
            }
        }

        if monitors.elements().is_empty() {
            monitors.elements_mut().push_back(m);
        } else if let Some(preference) = index_preference {
            let current_len = monitors.elements().len();
            if *preference > current_len {
                monitors.elements_mut().reserve(1);
            }

            monitors.elements_mut().insert(*preference, m);
        } else {
            monitors.elements_mut().push_back(m);
        }
    }

    true.into()
}

pub extern "system" fn enum_window(hwnd: HWND, lparam: LPARAM) -> BOOL {
    let containers = unsafe { &mut *(lparam.0 as *mut VecDeque<Container>) };

    let is_visible = WindowsApi::is_window_visible(hwnd);
    let is_window = WindowsApi::is_window(hwnd);
    let is_minimized = WindowsApi::is_iconic(hwnd);
    let is_maximized = WindowsApi::is_zoomed(hwnd);

    if is_visible && is_window && !is_minimized {
        let window = Window { hwnd: hwnd.0 };

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
        let window = Window { hwnd: hwnd.0 };

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
    _dwms_event_time: u32,
) {
    // OBJID_WINDOW
    if id_object != 0 {
        return;
    }

    let window = Window { hwnd: hwnd.0 };

    let winevent = match WinEvent::try_from(event) {
        Ok(event) => event,
        Err(_) => return,
    };
    let event_type = match WindowManagerEvent::from_win_event(winevent, window) {
        None => return,
        Some(event) => event,
    };

    winevent_listener::event_tx()
        .send(event_type)
        .expect("could not send message on winevent_listener::event_tx");
}

pub extern "system" fn hidden_window(
    window: HWND,
    message: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    unsafe {
        match message {
            WM_DISPLAYCHANGE => {
                let event_type = WindowManagerEvent::DisplayChange(Window { hwnd: window.0 });
                winevent_listener::event_tx()
                    .send(event_type)
                    .expect("could not send message on winevent_listener::event_tx");

                LRESULT(0)
            }
            // Added based on this https://stackoverflow.com/a/33762334
            WM_SETTINGCHANGE => {
                #[allow(clippy::cast_possible_truncation)]
                if wparam.0 as u32 == SPI_SETWORKAREA.0
                    || wparam.0 as u32 == SPI_ICONVERTICALSPACING.0
                {
                    let event_type = WindowManagerEvent::DisplayChange(Window { hwnd: window.0 });
                    winevent_listener::event_tx()
                        .send(event_type)
                        .expect("could not send message on winevent_listener::event_tx");
                }
                LRESULT(0)
            }
            // Added based on this https://stackoverflow.com/a/33762334
            WM_DEVICECHANGE => {
                #[allow(clippy::cast_possible_truncation)]
                if wparam.0 as u32 == DBT_DEVNODES_CHANGED {
                    let event_type = WindowManagerEvent::DisplayChange(Window { hwnd: window.0 });
                    winevent_listener::event_tx()
                        .send(event_type)
                        .expect("could not send message on winevent_listener::event_tx");
                }
                LRESULT(0)
            }
            _ => DefWindowProcW(window, message, wparam, lparam),
        }
    }
}
