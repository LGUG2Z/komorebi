use crate::accessibility::AccessibilityApi;
use crate::macos_api::MacosApi;
use crate::reaper;
use crate::reaper::ReaperNotification;
use crate::window_manager_event::ManualNotification;
use crate::window_manager_event::SystemNotification;
use crate::window_manager_event::WindowManagerEvent;
use crate::window_manager_event_listener;
use objc2_core_foundation::CFMachPort;
use objc2_core_foundation::CFRetained;
use objc2_core_foundation::CFRunLoop;
use objc2_core_foundation::kCFRunLoopDefaultMode;
use objc2_core_graphics::CGEvent;
use objc2_core_graphics::CGEventTapLocation;
use objc2_core_graphics::CGEventTapOptions;
use objc2_core_graphics::CGEventTapPlacement;
use objc2_core_graphics::CGEventTapProxy;
use objc2_core_graphics::CGEventType;
use std::ffi::c_void;
use std::ptr::NonNull;
use std::sync::atomic::AtomicBool;
use std::sync::atomic::Ordering;

/// Set by the callback when the system has turned the tap off, which it does to a tap that is
/// too slow to answer, or when the user asks it to. The callback cannot do anything about it from
/// where it is, so it says so here for the main loop, which can.
static TAP_DISABLED: AtomicBool = AtomicBool::new(false);

/// Whether the tap not being created has been said, so that it is said once and not every time
/// that another try is made.
static CREATION_FAILURE_LOGGED: AtomicBool = AtomicBool::new(false);

/// Whether the system has turned the tap off since this was last asked.
pub fn take_disabled() -> bool {
    TAP_DISABLED.swap(false, Ordering::Relaxed)
}

/// What to do about the tap, from what is known of it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TapAction {
    /// It is there and on.
    Nothing,
    /// It is there and off, which is something that the system does, and it only needs turning on.
    Enable,
    /// It is gone, which happens across sleep without a word, and has to be made again.
    Rebuild,
}

/// What to do about a tap that is `valid` or not and `enabled` or not.
#[must_use]
pub fn tap_action(valid: bool, enabled: bool) -> TapAction {
    match (valid, enabled) {
        (false, _) => TapAction::Rebuild,
        (true, false) => TapAction::Enable,
        (true, true) => TapAction::Nothing,
    }
}

pub struct InputEventListener {
    port: CFRetained<CFMachPort>,
}

extern "C-unwind" fn callback(
    _: CGEventTapProxy,
    event_type: CGEventType,
    mut event_ref: NonNull<CGEvent>,
    _listener: *mut c_void,
) -> *mut CGEvent {
    // these are not events that anyone did anything for, but the system saying that it has turned
    // the tap off, and what comes back from the callback is ignored. Anything slow done here is
    // what gets a tap turned off, so all that is done is to say so
    if event_type == CGEventType::TapDisabledByTimeout
        || event_type == CGEventType::TapDisabledByUserInput
    {
        TAP_DISABLED.store(true, Ordering::Relaxed);

        return event_ref.as_ptr();
    }

    // this should cover both clicking the close button and closing out something
    // by using cmd+w or cmd+q since some apps don't send events on close
    reaper::send_notification(ReaperNotification::MouseUpKeyUp);

    // this one is only really for when people "drag" a tab out of one window
    // to create another window - we wanna make sure it gets handled because
    // events don't get sent sometimes
    if event_type == CGEventType::LeftMouseUp
        && let Some(element) = MacosApi::foreground_window()
    {
        let mut pid = 0;
        unsafe {
            element.pid(NonNull::from_mut(&mut pid));
        }

        if pid != 0 {
            let window_id = AccessibilityApi::window_id(&element).ok();

            // Send MoveEnd to handle pending drag-to-swap operations
            // macOS doesn't reliably fire AXWindowMoved after mouse release
            if let Some(event) = WindowManagerEvent::from_system_notification(
                SystemNotification::Manual(ManualNotification::MoveEnd),
                pid,
                window_id,
            ) {
                window_manager_event_listener::send_notification(event);
            }

            // Send ResizeEnd to handle pending resize operations
            // macOS doesn't reliably fire AXWindowResized after mouse release
            if let Some(event) = WindowManagerEvent::from_system_notification(
                SystemNotification::Manual(ManualNotification::ResizeEnd),
                pid,
                window_id,
            ) {
                window_manager_event_listener::send_notification(event);
            }

            // Handle tab dragging to create new windows
            if let Some(event) = WindowManagerEvent::from_system_notification(
                SystemNotification::Manual(ManualNotification::ShowOnInputEvent),
                pid,
                window_id,
            ) {
                window_manager_event_listener::send_notification(event);
            }
        }
    }

    unsafe { event_ref.as_mut() }
}

impl InputEventListener {
    pub fn init(run_loop: &CFRunLoop) -> Option<Self> {
        let mouse_event_mask = (1 << CGEventType::LeftMouseUp.0) | (1 << CGEventType::KeyUp.0);
        let mut port = None;

        unsafe {
            let tap_port = CGEvent::tap_create(
                CGEventTapLocation::HIDEventTap,
                CGEventTapPlacement::HeadInsertEventTap,
                CGEventTapOptions::Default,
                mouse_event_mask,
                Some(callback),
                std::ptr::null_mut(),
            );

            if let Some(tap_port) = &tap_port {
                port = Some(tap_port.clone());
            }

            match CFMachPort::new_run_loop_source(None, tap_port.as_deref(), 0) {
                None => {}
                Some(source) => {
                    CFRunLoop::add_source(run_loop, Some(&source), kCFRunLoopDefaultMode);
                }
            }
        }

        if port.is_none() && !CREATION_FAILURE_LOGGED.swap(true, Ordering::Relaxed) {
            tracing::error!(
                "could not create the event tap, so clicks and key presses will not be seen: is \
                 the Accessibility permission granted? It is tried again every few seconds"
            );
        }

        port.map(|port| Self { port })
    }

    /// Look after the tap: turn it on if it has been turned off, and make it again if it has gone.
    ///
    /// The system turns a tap off when it is too slow, and a tap does not survive sleep without
    /// telling anyone, so this is for the main loop to do now and again, and whenever
    /// [`take_disabled`] says that it has been turned off.
    pub fn ensure_enabled(&mut self, run_loop: &CFRunLoop) {
        match tap_action(self.port.is_valid(), CGEvent::tap_is_enabled(&self.port)) {
            TapAction::Nothing => {}
            TapAction::Enable => {
                tracing::warn!("the event tap was turned off, turning it on again");
                CGEvent::tap_enable(&self.port, true);
            }
            TapAction::Rebuild => {
                tracing::warn!("the event tap is gone, making it again");

                if let Some(listener) = Self::init(run_loop) {
                    // the old one is taken down as it goes
                    *self = listener;
                }
            }
        }
    }
}

impl Drop for InputEventListener {
    fn drop(&mut self) {
        CFMachPort::invalidate(&self.port);
        CGEvent::tap_enable(&self.port, false);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_tap_that_is_there_and_on_is_left_alone() {
        assert_eq!(tap_action(true, true), TapAction::Nothing);
    }

    #[test]
    fn a_tap_that_was_turned_off_is_turned_on() {
        assert_eq!(tap_action(true, false), TapAction::Enable);
    }

    #[test]
    fn a_tap_that_is_gone_is_made_again_whatever_it_says_about_being_on() {
        assert_eq!(tap_action(false, false), TapAction::Rebuild);
        assert_eq!(tap_action(false, true), TapAction::Rebuild);
    }

    #[test]
    fn being_turned_off_is_noticed_once() {
        TAP_DISABLED.store(true, Ordering::Relaxed);

        assert!(take_disabled());
        assert!(!take_disabled());
    }
}
