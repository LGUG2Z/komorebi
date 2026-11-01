use crate::accessibility::private::_AXUIElementGetWindow;
use error::AccessibilityApiError;
use error::AccessibilityCustomError;
use error::AccessibilityError;
use objc2_application_services::AXObserver;
use objc2_application_services::AXObserverCallback;
use objc2_application_services::AXUIElement;
use objc2_application_services::AXValue;
use objc2_application_services::AXValueType;
use objc2_core_foundation::CFArray;
use objc2_core_foundation::CFRetained;
use objc2_core_foundation::CFRunLoop;
use objc2_core_foundation::CFRunLoopSource;
use objc2_core_foundation::CFString;
use objc2_core_foundation::CFType;
use objc2_core_foundation::kCFRunLoopDefaultMode;
use objc2_core_graphics::CGWindowID;
use std::ffi::c_void;
use std::ptr::NonNull;

pub mod action_constants;
pub mod attribute_constants;
pub mod enhanced_ui;
pub mod error;
pub mod notification_constants;
pub mod private;

pub struct AccessibilityApi;

impl AccessibilityApi {
    pub fn window_id(element: &AXUIElement) -> Result<u32, AccessibilityError> {
        let mut window_id = CGWindowID::default();
        unsafe {
            match AccessibilityError::from(_AXUIElementGetWindow(element, &mut window_id)) {
                AccessibilityError::Api(AccessibilityApiError::Success) => Ok(window_id),
                error => Err(error),
            }
        }
    }

    pub fn copy_attribute_value<T: objc2_core_foundation::Type>(
        element: &AXUIElement,
        attribute: &'static str,
    ) -> Option<CFRetained<T>> {
        let mut receiver = std::ptr::null();

        unsafe {
            element.copy_attribute_value(
                &CFString::from_static_str(attribute),
                NonNull::from(&mut receiver),
            );

            NonNull::new(receiver.cast::<T>().cast_mut())
                .map(|cf_type| CFRetained::from_raw(cf_type))
        }
    }

    pub fn copy_attribute_names(element: &AXUIElement) -> Option<CFRetained<CFArray>> {
        let mut receiver = std::ptr::null();

        unsafe {
            element.copy_attribute_names(NonNull::from(&mut receiver));

            NonNull::new(receiver.cast::<CFArray>().cast_mut())
                .map(|cf_type| CFRetained::from_raw(cf_type))
        }
    }

    pub fn set_attribute_ax_value<T>(
        element: &AXUIElement,
        attribute: &'static str,
        value_type: AXValueType,
        mut value: T,
    ) -> Result<(), AccessibilityError> {
        let pointer = NonNull::from_mut(&mut value).cast::<std::ffi::c_void>();

        unsafe {
            let value = AXValue::new(value_type, pointer).ok_or(AccessibilityError::Custom(
                AccessibilityCustomError::AxValueCreate,
            ))?;

            match AccessibilityError::from(
                element.set_attribute_value(&CFString::from_static_str(attribute), &value),
            ) {
                AccessibilityError::Api(AccessibilityApiError::Success) => Ok(()),
                error => Err(error),
            }
        }
    }

    pub fn set_attribute_cf_value(
        element: &AXUIElement,
        attribute: &'static str,
        value: &CFType,
    ) -> Result<(), AccessibilityError> {
        unsafe {
            match AccessibilityError::from(
                element.set_attribute_value(&CFString::from_static_str(attribute), value),
            ) {
                AccessibilityError::Api(AccessibilityApiError::Success) => Ok(()),
                error => Err(error),
            }
        }
    }

    pub fn perform_action(
        element: &AXUIElement,
        action: &'static str,
    ) -> Result<(), AccessibilityError> {
        unsafe {
            match AccessibilityError::from(
                element.perform_action(&CFString::from_static_str(action)),
            ) {
                AccessibilityError::Api(AccessibilityApiError::Success) => Ok(()),
                error => Err(error),
            }
        }
    }

    /// Stop waiting on an app that does not answer after `seconds`
    ///
    /// Every AX call is a synchronous round trip to the app that owns the element, and by default
    /// an app that has hung can hold the caller for several seconds.
    pub fn set_messaging_timeout(
        element: &AXUIElement,
        seconds: f32,
    ) -> Result<(), AccessibilityError> {
        unsafe {
            match AccessibilityError::from(element.set_messaging_timeout(seconds)) {
                AccessibilityError::Api(AccessibilityApiError::Success) => Ok(()),
                error => Err(error),
            }
        }
    }

    pub fn create_application(process_id: i32) -> CFRetained<AXUIElement> {
        unsafe { AXUIElement::new_application(process_id) }
    }

    pub fn create_observer(
        process_id: i32,
        callback: AXObserverCallback,
    ) -> Result<CFRetained<AXObserver>, AccessibilityError> {
        let mut observer_ref = std::ptr::null_mut();

        match AccessibilityError::from(unsafe {
            AXObserver::create(process_id, callback, NonNull::from_mut(&mut observer_ref))
        }) {
            AccessibilityError::Api(AccessibilityApiError::Success) => unsafe {
                Ok(CFRetained::from_raw(
                    NonNull::new(observer_ref.cast::<AXObserver>()).unwrap(),
                ))
            },
            error => Err(error),
        }
    }

    fn add_notifications_to_observer(
        observer: &AXObserver,
        element: &AXUIElement,
        notifications: &[&'static str],
        refcon: Option<*mut c_void>,
    ) -> Result<(), AccessibilityError> {
        for notification in notifications {
            unsafe {
                match AccessibilityError::from(AXObserver::add_notification(
                    observer,
                    element,
                    &CFString::from_str(notification),
                    refcon.unwrap_or_default(),
                )) {
                    // we don't want to return until all the notifications have been added
                    AccessibilityError::Api(AccessibilityApiError::Success) => {}
                    // if the notification is already registered then we don't treat that as a true error
                    AccessibilityError::Api(
                        AccessibilityApiError::NotificationAlreadyRegistered,
                    ) => {
                        let mut pid = 0;
                        element.pid(NonNull::from_mut(&mut pid));
                        tracing::info!(
                            "{notification} already exists on observer for process id {pid}"
                        );
                    }
                    error => return Err(error),
                }
            }
        }

        Ok(())
    }

    pub fn add_observer_to_run_loop(
        observer: &AXObserver,
        element: &AXUIElement,
        notifications: &[&'static str],
        run_loop: &CFRunLoop,
        refcon: Option<*mut c_void>,
    ) -> Result<(), AccessibilityError> {
        AccessibilityApi::add_notifications_to_observer(observer, element, notifications, refcon)?;

        unsafe {
            CFRunLoop::add_source(
                run_loop,
                Some(&observer.run_loop_source()),
                kCFRunLoopDefaultMode,
            );
        }

        Ok(())
    }

    pub fn invalidate_observer(observer: &AXObserver) {
        unsafe {
            // invalidate means that this will get removed from all run loops by the system
            CFRunLoopSource::invalidate(&observer.run_loop_source());
        }
    }

    /// Add a single notification to an observer for a specific element.
    pub fn add_notification_to_observer(
        observer: &AXObserver,
        element: &AXUIElement,
        notification: &'static str,
        refcon: Option<*mut c_void>,
    ) -> Result<(), AccessibilityError> {
        unsafe {
            match AccessibilityError::from(AXObserver::add_notification(
                observer,
                element,
                &CFString::from_str(notification),
                refcon.unwrap_or_default(),
            )) {
                AccessibilityError::Api(AccessibilityApiError::Success) => Ok(()),
                AccessibilityError::Api(AccessibilityApiError::NotificationAlreadyRegistered) => {
                    // Already registered is not an error for our purposes
                    Ok(())
                }
                error => Err(error),
            }
        }
    }

    /// Remove a single notification from an observer for a specific element.
    pub fn remove_notification_from_observer(
        observer: &AXObserver,
        element: &AXUIElement,
        notification: &'static str,
    ) -> Result<(), AccessibilityError> {
        unsafe {
            match AccessibilityError::from(AXObserver::remove_notification(
                observer,
                element,
                &CFString::from_str(notification),
            )) {
                AccessibilityError::Api(AccessibilityApiError::Success) => Ok(()),
                AccessibilityError::Api(AccessibilityApiError::NotificationNotRegistered) => {
                    // Not registered is not an error for our purposes
                    Ok(())
                }
                error => Err(error),
            }
        }
    }
}
