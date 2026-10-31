use crate::AccessibilityObserver;
use crate::AccessibilityUiElement;
use crate::accessibility::AccessibilityApi;
use crate::accessibility::attribute_constants::kAXMainWindowAttribute;
use crate::accessibility::attribute_constants::kAXTitleAttribute;
use crate::accessibility::attribute_constants::kAXWindowsAttribute;
use crate::accessibility::error::AccessibilityError;
use crate::accessibility::notification_constants::AccessibilityNotification;
use crate::accessibility::notification_constants::kAXApplicationActivatedNotification;
use crate::accessibility::notification_constants::kAXApplicationDeactivatedNotification;
use crate::accessibility::notification_constants::kAXApplicationHiddenNotification;
use crate::accessibility::notification_constants::kAXApplicationShownNotification;
use crate::accessibility::notification_constants::kAXMainWindowChangedNotification;
use crate::accessibility::notification_constants::kAXUIElementDestroyedNotification;
use crate::accessibility::notification_constants::kAXWindowCreatedNotification;
use crate::window::Window;
use crate::window_manager_event::SystemNotification;
use crate::window_manager_event::WindowManagerEvent;
use crate::window_manager_event_listener;
use objc2_application_services::AXObserver;
use objc2_application_services::AXUIElement;
use objc2_core_foundation::CFArray;
use objc2_core_foundation::CFRetained;
use objc2_core_foundation::CFRunLoop;
use objc2_core_foundation::CFString;
use std::ffi::c_void;
use std::ptr::NonNull;
use std::str::FromStr;
use tracing::instrument;

const NOTIFICATIONS: &[&str] = &[
    kAXApplicationActivatedNotification,
    kAXApplicationDeactivatedNotification,
    kAXApplicationHiddenNotification,
    kAXApplicationShownNotification,
    // this is when we change focus between two windows of the same app
    kAXMainWindowChangedNotification,
    // this is when the same app has a new window opened
    kAXWindowCreatedNotification,
    // this is when a window of an application is destroyed / closed
    // when this fires, the app owner name won't be found, but the can be matched via PID
    kAXUIElementDestroyedNotification,
];

#[derive(Debug, Default, Clone, PartialEq)]
pub struct Application {
    element: AccessibilityUiElement,
    pub process_id: i32,
    pub observer: AccessibilityObserver,
    pub is_observable: bool,
}

#[instrument(skip_all)]
unsafe extern "C-unwind" fn application_observer_callback(
    _observer: NonNull<AXObserver>,
    element: NonNull<AXUIElement>,
    notification: NonNull<CFString>,
    _context: *mut c_void,
) {
    unsafe {
        let notification_str = notification.as_ref().to_string();
        let name =
            AccessibilityApi::copy_attribute_value::<CFString>(element.as_ref(), kAXTitleAttribute)
                .map(|s| s.to_string());

        // AXUIElementDestroyed fires when the element is already gone,
        // so we can't get the title - but we still need to process it
        let is_destroyed = matches!(
            AccessibilityNotification::from_str(&notification_str),
            Ok(AccessibilityNotification::AXUIElementDestroyed)
        );

        if is_destroyed || name.as_ref().is_some_and(|n| !n.is_empty()) {
            let mut process_id = 0;
            element.as_ref().pid(NonNull::from_mut(&mut process_id));

            if let Ok(notification) = AccessibilityNotification::from_str(&notification_str)
                && let Some(event) = WindowManagerEvent::from_system_notification(
                    SystemNotification::Accessibility(notification),
                    process_id,
                    None,
                )
            {
                tracing::debug!(
                    "notification: {notification}, process: {process_id}, name: \"{}\"",
                    name.as_deref().unwrap_or("<destroyed>")
                );

                window_manager_event_listener::send_notification(event);
            }
        }
    }
}

impl Drop for Application {
    fn drop(&mut self) {
        // this gets called when an Application clone on a Window is dropped, so we need
        // to make sure it only invalidates the observer if the Application is no longer
        // running
        if self.is_observable && !self.is_valid() {
            tracing::info!(
                "invalidating application observer for process id {}",
                self.process_id
            );
            // make sure the observer gets removed from any run loops
            AccessibilityApi::invalidate_observer(&self.observer);
        }
    }
}

impl Application {
    pub fn new(process_id: i32) -> Result<Self, AccessibilityError> {
        Ok(Self {
            element: AccessibilityUiElement(AccessibilityApi::create_application(process_id)),
            process_id,
            observer: AccessibilityObserver(Some(AccessibilityApi::create_observer(
                process_id,
                Some(application_observer_callback),
            )?)),
            is_observable: true,
        })
    }

    /// The AX element of the application itself, as opposed to one of its windows
    pub fn element(&self) -> &AccessibilityUiElement {
        &self.element
    }

    pub fn name(&self) -> Option<String> {
        AccessibilityApi::copy_attribute_value::<CFString>(&self.element, kAXTitleAttribute)
            .map(|s| s.to_string())
    }

    #[tracing::instrument(skip_all)]
    pub fn observe(&mut self, run_loop: &CFRunLoop, refcon: Option<*mut c_void>) {
        tracing::info!(
            "registering observer for process: {}, name: {}",
            self.process_id,
            self.name()
                .unwrap_or_else(|| String::from("<NO NAME FOUND>"))
        );

        let mut retries = 5;

        while retries > 0 {
            match AccessibilityApi::add_observer_to_run_loop(
                &self.observer,
                &self.element,
                NOTIFICATIONS,
                run_loop,
                refcon,
            ) {
                Ok(_) => {
                    self.is_observable = true;
                    break;
                }
                Err(error) => {
                    // Chromium apps are still gross on macOS too
                    retries -= 1;
                    std::thread::sleep(std::time::Duration::from_millis(100));
                    tracing::warn!(
                        "failed to register observer for process {} ({}): {error}, {retries} retries left",
                        self.process_id,
                        self.name()
                            .unwrap_or_else(|| String::from("<NO NAME FOUND>"))
                    );
                }
            }
        }
    }

    pub fn is_valid(&self) -> bool {
        AccessibilityApi::copy_attribute_names(&self.element).is_some()
    }

    pub fn window_elements(&self) -> Option<CFRetained<CFArray<AXUIElement>>> {
        AccessibilityApi::copy_attribute_value::<CFArray<AXUIElement>>(
            &self.element,
            kAXWindowsAttribute,
        )
    }

    pub fn main_window(&self) -> Option<CFRetained<AXUIElement>> {
        AccessibilityApi::copy_attribute_value::<AXUIElement>(&self.element, kAXMainWindowAttribute)
    }

    pub fn main_window_id(&self) -> Option<u32> {
        let window = AccessibilityApi::copy_attribute_value::<AXUIElement>(
            &self.element,
            kAXMainWindowAttribute,
        )?;
        AccessibilityApi::window_id(window.as_ref()).ok()
    }

    pub fn window_by_title(&self, title: &str) -> Option<Window> {
        let mut target = None;

        for element in self.window_elements()? {
            let window = Window::new(element, self.clone()).ok()?;

            if let Some(window_title) = window.title()
                && (window_title.eq(title)
                    // a hack for handling already-launched apps with inconsistent titles when
                    // komorebi launches, such as Activity Monitor, Google Chrome, and counting...
                    || window_title.contains(title))
            {
                target = Some(window);
            }
        }

        target
    }
}
