use crate::core_graphics::error::CoreGraphicsError;
use crate::monitor_reconciliator;
use crate::monitor_reconciliator::MonitorNotification;
use objc2_core_graphics::CGDirectDisplayID;
use objc2_core_graphics::CGDisplayChangeSummaryFlags;
use objc2_core_graphics::CGDisplayRegisterReconfigurationCallback;
use objc2_core_graphics::CGDisplayRemoveReconfigurationCallback;
use std::ffi::c_void;

pub struct DisplayReconfigurationListener {}

unsafe extern "C-unwind" fn callback(
    display_id: CGDirectDisplayID,
    flags: CGDisplayChangeSummaryFlags,
    _user_info: *mut c_void,
) {
    if flags.contains(CGDisplayChangeSummaryFlags::DesktopShapeChangedFlag) {
        tracing::debug!("display: {display_id} resized");
        monitor_reconciliator::send_notification(MonitorNotification::Resize(display_id));
    }
    if flags.contains(CGDisplayChangeSummaryFlags::AddFlag) {
        tracing::debug!("display: {display_id} added");
        monitor_reconciliator::send_notification(MonitorNotification::DisplayConnectionChange(
            display_id,
        ));
    }

    if flags.contains(CGDisplayChangeSummaryFlags::RemoveFlag) {
        tracing::debug!("display: {display_id} removed");
        monitor_reconciliator::send_notification(MonitorNotification::DisplayConnectionChange(
            display_id,
        ));
    }

    // if flags.contains(CGDisplayChangeSummaryFlags::EnabledFlag) {
    //     tracing::debug!("display: {display_id} enabled");
    //     monitor_reconciliator::send_notification(MonitorNotification::DisplayConnectionChange(
    //         display_id,
    //     ));
    // }
    //
    // if flags.contains(CGDisplayChangeSummaryFlags::DisabledFlag) {
    //     tracing::debug!("display: {display_id} disabled");
    //     monitor_reconciliator::send_notification(MonitorNotification::DisplayConnectionChange(
    //         display_id,
    //     ));
    // }
    //
    // if flags.contains(CGDisplayChangeSummaryFlags::MovedFlag) {
    //     tracing::debug!("display: {display_id} moved");
    //     monitor_reconciliator::send_notification(MonitorNotification::DisplayConnectionChange(
    //         display_id,
    //     ));
    // }
    //
    // if flags.contains(CGDisplayChangeSummaryFlags::BeginConfigurationFlag) {
    //     tracing::debug!("display: {display_id} configured");
    // }
    //
    // if flags.contains(CGDisplayChangeSummaryFlags::MirrorFlag) {
    //     tracing::debug!("display: {display_id} mirrored");
    // }
    //
    // if flags.contains(CGDisplayChangeSummaryFlags::UnMirrorFlag) {
    //     tracing::debug!("display: {display_id} unmirrored");
    // }
    //
    // tracing::trace!("display: {display_id}, flags: {flags:?}");
}

impl DisplayReconfigurationListener {
    pub fn init() -> Result<(), CoreGraphicsError> {
        tracing::info!("registering display reconfiguration listener callback");

        unsafe {
            match CoreGraphicsError::from(CGDisplayRegisterReconfigurationCallback(
                Some(callback),
                std::ptr::null_mut(),
            )) {
                CoreGraphicsError::Success => Ok(()),
                err => Err(err),
            }
        }
    }
}

impl Drop for DisplayReconfigurationListener {
    fn drop(&mut self) {
        tracing::info!("removing display reconfiguration listener callback");

        unsafe {
            match CoreGraphicsError::from(CGDisplayRemoveReconfigurationCallback(
                Some(callback),
                std::ptr::null_mut(),
            )) {
                CoreGraphicsError::Success => {}
                error => {
                    tracing::error!(
                        "failed to remove display reconfiguration listener callback {error}"
                    )
                }
            }
        }
    }
}
