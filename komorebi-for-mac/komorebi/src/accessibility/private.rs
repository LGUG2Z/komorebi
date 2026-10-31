#![allow(non_upper_case_globals, unused)]

use crate::accessibility::AccessibilityApi;
use crate::accessibility::error::AccessibilityError;
use objc2_application_services::AXError;
use objc2_application_services::AXUIElement;
use objc2_core_foundation::CFBoolean;
use objc2_core_graphics::CGWindowID;

pub const kAXEnhancedUserInterface: &str = "AXEnhancedUserInterface";

// this is the only private API call Aerospace uses, so I think we're ok to use it too
// https://github.com/nikitabobko/AeroSpace?tab=readme-ov-file#project-values
unsafe extern "C" {
    /// Extract `window_id` from an AXUIElement.
    pub fn _AXUIElementGetWindow(elem: &AXUIElement, window_id: *mut CGWindowID) -> AXError;
}

/// Get the current state of Enhanced User Interface for an application element
///
/// Returns `None` if the application has no such attribute, or did not answer.
pub fn get_enhanced_user_interface_if_present(element: &AXUIElement) -> Option<bool> {
    AccessibilityApi::copy_attribute_value::<CFBoolean>(element, kAXEnhancedUserInterface)
        .map(|b| b.as_bool())
}

/// Set the Enhanced User Interface state for an application element
///
/// This belongs to the application, not to any one of its windows. See
/// [`crate::accessibility::enhanced_ui`] for how it is used while windows are being animated.
pub fn set_enhanced_user_interface(
    element: &AXUIElement,
    enabled: bool,
) -> Result<(), AccessibilityError> {
    let cf_boolean = CFBoolean::new(enabled);
    let value = &**cf_boolean;
    AccessibilityApi::set_attribute_cf_value(element, kAXEnhancedUserInterface, value)
}
