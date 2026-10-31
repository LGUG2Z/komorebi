use crate::DISPLAY_INDEX_PREFERENCES;
use crate::LibraryError;
use crate::accessibility::AccessibilityApi;
use crate::accessibility::attribute_constants::kAXFocusedApplicationAttribute;
use crate::accessibility::attribute_constants::kAXFocusedWindowAttribute;
use crate::accessibility::attribute_constants::kAXPositionAttribute;
use crate::accessibility::attribute_constants::kAXSizeAttribute;
use crate::accessibility::error::AccessibilityApiError;
use crate::accessibility::error::AccessibilityError;
use crate::application::Application;
use crate::cf_array_as;
use crate::container::Container;
use crate::core::Rect;
use crate::core::rect_ext::RectExt;
use crate::core_graphics::CoreGraphicsApi;
use crate::ioreg::IoReg;
use crate::monitor::Monitor;
use crate::monitor::MonitorInfo;
use crate::window::RuleDebug;
use crate::window::WindowInfo;
use crate::window_manager::WindowManager;
use color_eyre::eyre;
use color_eyre::eyre::OptionExt;
use dispatch2::DispatchQueue;
use objc2::MainThreadMarker;
use objc2_app_kit::NSDeviceDescriptionKey;
use objc2_app_kit::NSEvent;
use objc2_app_kit::NSScreen;
use objc2_app_kit::NSWorkspace;
use objc2_application_services::AXUIElement;
use objc2_application_services::AXValue;
use objc2_application_services::AXValueType;
use objc2_core_foundation::CFDictionary;
use objc2_core_foundation::CFRetained;
use objc2_core_foundation::CFString;
use objc2_core_foundation::CGPoint;
use objc2_core_foundation::CGRect;
use objc2_core_foundation::CGSize;
use objc2_core_graphics::CGEvent;
use objc2_core_graphics::CGEventSource;
use objc2_core_graphics::CGEventSourceStateID;
use objc2_core_graphics::CGMainDisplayID;
use objc2_foundation::NSDictionary;
use objc2_foundation::NSNumber;
use objc2_foundation::NSString;
use objc2_foundation::NSURL;
use std::collections::HashMap;
use std::collections::hash_map::Entry;
use std::ffi::c_void;
use std::path::Path;
use std::ptr::NonNull;

pub struct MacosApi;

impl MacosApi {
    pub fn set_wallpaper(path: &Path, display_id: u32) -> eyre::Result<()> {
        let path_str = path
            .to_str()
            .ok_or_eyre("Invalid path encoding")?
            .to_string();

        DispatchQueue::main().exec_async(move || {
            let ns_path = NSString::from_str(&path_str);
            let url = NSURL::fileURLWithPath(&ns_path);

            let mtm = unsafe { MainThreadMarker::new_unchecked() };
            let screens = NSScreen::screens(mtm);

            for screen in &screens {
                if let Some(did) = screen
                    .deviceDescription()
                    .objectForKey(&NSDeviceDescriptionKey::from_str("NSScreenNumber"))
                    && let Ok(did) = did.downcast::<NSNumber>()
                    && did.as_u32() == display_id
                {
                    let workspace = NSWorkspace::sharedWorkspace();
                    unsafe {
                        if let Err(error) = workspace.setDesktopImageURL_forScreen_options_error(
                            &url,
                            &screen,
                            &NSDictionary::new(),
                        ) {
                            tracing::warn!("failed to set wallpaper: {error}")
                        }
                    }
                }
            }
        });

        Ok(())
    }
}

impl MacosApi {
    #[tracing::instrument(skip_all)]
    pub fn load_monitor_information(wm: &mut WindowManager) -> Result<(), LibraryError> {
        let all_devices = Self::latest_monitor_information()?;

        let monitors = &mut wm.monitors;
        let monitor_usr_idx_map = &mut wm.monitor_usr_idx_map;

        'read: for device in all_devices {
            for monitor in monitors.elements() {
                if device.serial_number_id.eq(&monitor.serial_number_id) {
                    continue 'read;
                }
            }

            let m = device.clone();

            let mut index_preference = None;
            let display_index_preferences = DISPLAY_INDEX_PREFERENCES.read();
            for (index, id) in &*display_index_preferences {
                if m.serial_number_id.eq(id) {
                    index_preference = Option::from(index);
                }
            }

            if let Some(preference) = index_preference {
                while *preference >= monitors.elements().len() {
                    monitors.elements_mut().push_back(Monitor::placeholder());
                }

                let current_serial_id = monitors
                    .elements_mut()
                    .get(*preference)
                    .map_or("", |m| &m.serial_number_id);
                if current_serial_id == "PLACEHOLDER" {
                    let _ = monitors.elements_mut().remove(*preference);
                    monitors.elements_mut().insert(*preference, m);
                } else {
                    monitors.elements_mut().insert(*preference, m);
                }
            } else {
                monitors.elements_mut().push_back(m);
            }
        }

        monitors
            .elements_mut()
            .retain(|m| m.serial_number_id.ne("PLACEHOLDER"));

        // Rebuild monitor index map
        *monitor_usr_idx_map = HashMap::new();
        let mut added_monitor_idxs = Vec::new();
        for (index, id) in &*DISPLAY_INDEX_PREFERENCES.read() {
            if let Some(m_idx) = monitors
                .elements()
                .iter()
                .position(|m| m.serial_number_id.eq(id))
            {
                monitor_usr_idx_map.insert(*index, m_idx);
                added_monitor_idxs.push(m_idx);
            }
        }

        let max_usr_idx = monitors
            .elements()
            .len()
            .max(monitor_usr_idx_map.keys().max().map_or(0, |v| *v));

        let mut available_usr_idxs = (0..max_usr_idx)
            .filter(|i| !monitor_usr_idx_map.contains_key(i))
            .collect::<Vec<_>>();

        let not_added_monitor_idxs = (0..monitors.elements().len())
            .filter(|i| !added_monitor_idxs.contains(i))
            .collect::<Vec<_>>();

        for i in not_added_monitor_idxs {
            if let Some(next_usr_idx) = available_usr_idxs.first() {
                monitor_usr_idx_map.insert(*next_usr_idx, i);
                available_usr_idxs.remove(0);
            } else if let Some(idx) = monitor_usr_idx_map.keys().max() {
                monitor_usr_idx_map.insert(*idx, i);
            }
        }

        Ok(())
    }

    pub fn update_monitor_work_areas(wm: &mut WindowManager) -> eyre::Result<()> {
        let all_devices = Self::latest_monitor_information()?;
        let mut updated = false;

        for device in all_devices {
            // Display ids change when a display is reconnected, the serial number does not
            for monitor in wm.monitors_mut() {
                if monitor.serial_number_id == device.serial_number_id {
                    monitor.id = device.id;
                    monitor.size = device.size;
                    monitor.work_area_size = device.work_area_size;
                    updated = true;

                    tracing::info!(
                        "updated monitor size and work area for monitor {}",
                        monitor.serial_number_id
                    );
                }
            }
        }

        // A display that komorebi does not know yet is the business of the reconciliator
        if updated {
            let focus_follows_mouse = wm.mouse_follows_focus;
            wm.update_focused_workspace(focus_follows_mouse, true)?;
        }

        Ok(())
    }

    pub fn latest_monitor_information() -> Result<Vec<Monitor>, LibraryError> {
        let screens = NSScreen::screens(MainThreadMarker::new().unwrap());
        let mut network_attached_screens = vec![];

        for display_id in CoreGraphicsApi::connected_display_ids()? {
            let serial_number = CoreGraphicsApi::display_serial_number(display_id);
            network_attached_screens.push((serial_number, display_id));
        }

        let monitor_info = IoReg::query_monitors()?;

        let mut monitor_info_map = HashMap::new();
        for m in monitor_info {
            monitor_info_map.insert(m.serial_number, m);
        }

        // total hackery for iPad second screen stuff over the network
        for info in monitor_info_map.values() {
            network_attached_screens
                .retain(|(serial_number, _)| serial_number != &info.serial_number);
        }

        for (serial_number, display_id) in network_attached_screens {
            for screen in &screens {
                if let Some(did) = screen
                    .deviceDescription()
                    .objectForKey(&NSDeviceDescriptionKey::from_str("NSScreenNumber"))
                    && let Ok(did) = did.downcast::<NSNumber>()
                    && did.as_u32() == display_id
                {
                    monitor_info_map.insert(
                        serial_number,
                        MonitorInfo {
                            alphanumeric_serial_number: serial_number.to_string(),
                            manufacturer_id: "".to_string(),
                            product_name: screen.localizedName().to_string(),
                            legacy_manufacturer_id: "".to_string(),
                            product_id: "".to_string(),
                            serial_number,
                            week_of_manufacture: "".to_string(),
                            year_of_manufacture: "".to_string(),
                        },
                    );
                }
            }
        }

        let mut all_devices = vec![];

        for display_id in CoreGraphicsApi::connected_display_ids()? {
            let serial_number = CoreGraphicsApi::display_serial_number(display_id);
            if let Some(info) = monitor_info_map.get(&serial_number) {
                for screen in &screens {
                    if let Some(did) = screen
                        .deviceDescription()
                        .objectForKey(&NSDeviceDescriptionKey::from_str("NSScreenNumber"))
                    {
                        let display_bounds = CoreGraphicsApi::display_bounds(display_id);
                        if let Ok(did) = did.downcast::<NSNumber>()
                            && did.as_u32() == display_id
                        {
                            let size = Rect::from(display_bounds);

                            let visible_frame = screen.visibleFrame();
                            let primary_display_height =
                                CoreGraphicsApi::display_bounds(CGMainDisplayID())
                                    .size
                                    .height;

                            let quartz_visible_frame = CGRect::new(
                                CGPoint::new(
                                    visible_frame.origin.x,
                                    primary_display_height
                                        - visible_frame.origin.y
                                        - visible_frame.size.height,
                                ),
                                CGSize::new(visible_frame.size.width, visible_frame.size.height),
                            );

                            let work_area_size = Rect::from(quartz_visible_frame);

                            let monitor = Monitor::new(
                                display_id,
                                size,
                                work_area_size,
                                &info.product_name,
                                &info.alphanumeric_serial_number,
                            );

                            all_devices.push(monitor);
                        }
                    }
                }
            }
        }

        Ok(all_devices)
    }

    #[tracing::instrument(skip_all)]
    pub fn load_workspace_information(wm: &mut WindowManager) -> Result<(), LibraryError> {
        let mut monitor_size_map = HashMap::new();
        let mut monitor_workspace_map = HashMap::new();
        let mut monitor_window_map = HashMap::new();
        let mut valid_window_count = 0;

        for (idx, monitor) in wm.monitors.elements_mut().iter_mut().enumerate() {
            monitor_size_map.insert(idx, monitor.size);
            monitor_workspace_map.insert(idx, monitor.focused_workspace_mut());
        }

        if let Some(window_list_info) = CoreGraphicsApi::window_list_info() {
            tracing::info!("{} windows found", window_list_info.len());

            for raw_window_info in cf_array_as::<CFDictionary>(&window_list_info) {
                if let Some(info) = WindowInfo::new(raw_window_info).validated() {
                    let window_rect = Rect::from(info.bounds);

                    for (monitor_idx, monitor_size) in &monitor_size_map {
                        if monitor_size.contains(&window_rect) {
                            let entry = monitor_window_map
                                .entry(monitor_idx)
                                .or_insert_with(Vec::new);

                            let application = match wm.applications.entry(info.owner_pid) {
                                Entry::Occupied(entry) => entry.into_mut(),
                                Entry::Vacant(vacant) => {
                                    let mut application = Application::new(info.owner_pid)?;
                                    // TODO: this ain't great, fix this OBS workaround
                                    application.observe(&wm.run_loop, None);
                                    vacant.insert(application)
                                }
                            };

                            if let Some(window) = application.window_by_title(&info.name) {
                                let mut rule_debug = RuleDebug::default();
                                if window.should_manage(None, &mut rule_debug)? {
                                    window.observe(&wm.run_loop, None)?;
                                    entry.push(window);
                                    valid_window_count += 1
                                }
                            }
                        }
                    }
                }
            }

            tracing::info!("{valid_window_count} valid windows identified");
        }

        for (monitor_idx, windows) in monitor_window_map {
            for window in windows {
                let mut container = Container::default();

                container.windows_mut().push_back(window);
                if let Some(Some(workspace)) = monitor_workspace_map.get_mut(monitor_idx) {
                    workspace.containers_mut().push_back(container);
                }
            }
        }

        Ok(())
    }

    pub fn center_cursor_in_rect(rect: &Rect) -> Result<(), LibraryError> {
        Ok(CoreGraphicsApi::warp_mouse_cursor_position(
            rect.left + (rect.right / 2),
            rect.top + (rect.bottom / 2),
        )?)
    }

    // #[track_caller] // use this to with std::panic::Location::caller() to debug failures
    #[tracing::instrument(skip_all)]
    pub fn window_rect(element: &AXUIElement) -> Result<CGRect, AccessibilityError> {
        let mut position_receiver = std::ptr::null();
        let mut size_receiver = std::ptr::null();

        unsafe {
            match AccessibilityApiError::from(element.copy_attribute_value(
                &CFString::from_static_str(kAXPositionAttribute),
                NonNull::from_mut(&mut position_receiver),
            )) {
                AccessibilityApiError::Success => {}
                error => {
                    tracing::error!("failed to get window position: {error}");
                    return Err(error.into());
                }
            };

            match AccessibilityApiError::from(element.copy_attribute_value(
                &CFString::from_static_str(kAXSizeAttribute),
                NonNull::from_mut(&mut size_receiver),
            )) {
                AccessibilityApiError::Success => {}
                error => {
                    tracing::error!("failed to get window size: {error}");
                    return Err(error.into());
                }
            };

            let mut rect = CGRect::default();

            AXValue::value(
                &*position_receiver.cast::<AXValue>(),
                AXValueType::CGPoint,
                NonNull::from_mut(&mut rect.origin).cast::<c_void>(),
            );
            AXValue::value(
                &*size_receiver.cast::<AXValue>(),
                AXValueType::CGSize,
                NonNull::from_mut(&mut rect.size).cast::<c_void>(),
            );

            Ok(rect)
        }
    }

    pub fn foreground_window_id() -> Option<u32> {
        unsafe {
            let syswide = AXUIElement::new_system_wide();
            let app = AccessibilityApi::copy_attribute_value::<AXUIElement>(
                &syswide,
                kAXFocusedApplicationAttribute,
            )?;

            let window = AccessibilityApi::copy_attribute_value::<AXUIElement>(
                &app,
                kAXFocusedWindowAttribute,
            )?;

            AccessibilityApi::window_id(&window).ok()
        }
    }

    pub fn foreground_window() -> Option<CFRetained<AXUIElement>> {
        unsafe {
            let syswide = AXUIElement::new_system_wide();
            let app = AccessibilityApi::copy_attribute_value::<AXUIElement>(
                &syswide,
                kAXFocusedApplicationAttribute,
            )?;

            AccessibilityApi::copy_attribute_value::<AXUIElement>(&app, kAXFocusedWindowAttribute)
        }
    }

    /// The process id of the application that is in front right now, as the accessibility API
    /// says, or `None` if it will not say
    pub fn frontmost_process_id() -> Option<i32> {
        unsafe {
            let syswide = AXUIElement::new_system_wide();
            let app = AccessibilityApi::copy_attribute_value::<AXUIElement>(
                &syswide,
                kAXFocusedApplicationAttribute,
            )?;

            let mut pid = 0;
            app.pid(NonNull::from_mut(&mut pid));

            (pid != 0).then_some(pid)
        }
    }

    pub fn cursor_pos() -> CGPoint {
        let source = CGEventSource::new(CGEventSourceStateID::HIDSystemState);
        let event = CGEvent::new(source.as_deref());
        CGEvent::location(event.as_deref())
    }

    pub fn monitor_from_point(point: CGPoint) -> Option<u32> {
        CoreGraphicsApi::display_with_point(point)
    }

    pub fn left_mouse_button_is_pressed() -> bool {
        NSEvent::pressedMouseButtons() & 0x1 != 0
    }

    pub fn activate_finder() {
        use objc2_app_kit::NSApplicationActivationOptions;

        DispatchQueue::main().exec_sync(move || {
            let workspace = NSWorkspace::sharedWorkspace();

            let apps = workspace.runningApplications();
            for app in apps.iter() {
                if let Some(bundle_id) = app.bundleIdentifier()
                    && bundle_id.to_string() == "com.apple.finder"
                {
                    // Activate without opening a new window
                    app.activateWithOptions(NSApplicationActivationOptions::empty());
                    return;
                }
            }

            tracing::warn!("could not find running Finder application");
        });
    }
}
