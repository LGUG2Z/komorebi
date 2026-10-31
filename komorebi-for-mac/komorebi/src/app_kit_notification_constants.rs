#![allow(clippy::enum_variant_names)]

use objc2::rc::Retained;
use objc2_foundation::NSNotification;
use objc2_foundation::NSNotificationName;
use serde::Deserialize;
use serde::Serialize;
use std::str::FromStr;
use strum::Display;
use strum::EnumString;

#[derive(Debug, Copy, Clone, Serialize, Deserialize, Display, EnumString)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
pub enum AppKitWorkspaceNotification {
    // A notification that the workspace posts when any of the accessibility display options change.
    NSWorkspaceAccessibilityDisplayOptionsDidChangeNotification,
    // A notification that the workspace posts when a Spaces change occurs.
    NSWorkspaceActiveSpaceDidChangeNotification,
    // A notification that the workspace posts when the Finder is about to activate an app.
    NSWorkspaceDidActivateApplicationNotification,
    // A notification that the workspace posts when the Finder file labels or colors change.
    NSWorkspaceDidChangeFileLabelsNotification,
    // A notification that the workspace posts when the Finder deactivates an app.
    NSWorkspaceDidDeactivateApplicationNotification,
    // A notification that the workspace posts when the Finder hides an app.
    NSWorkspaceDidHideApplicationNotification,
    // A notification that the workspace posts when a new app starts up.
    NSWorkspaceDidLaunchApplicationNotification,
    // A notification that the workspace posts when a new device mounts.
    NSWorkspaceDidMountNotification,
    // Posted when a file operation has been performed in the receiving app.
    // #[deprecated]
    // NSWorkspaceDidPerformFileOperationNotification,
    // A notification that the workspace posts when a volume changes its name or mount path.
    NSWorkspaceDidRenameVolumeNotification,
    // A notification that the workspace posts when an app finishes executing.
    NSWorkspaceDidTerminateApplicationNotification,
    // A notification that the workspace posts when the Finder unhides an app.
    NSWorkspaceDidUnhideApplicationNotification,
    // A notification that the workspace posts when the Finder unmounts a device.
    NSWorkspaceDidUnmountNotification,
    // A notification that the workspace posts when the device wakes from sleep.
    NSWorkspaceDidWakeNotification,
    // A notification that the workspace posts when the device’s screen goes to sleep.
    NSWorkspaceScreensDidSleepNotification,
    // A notification that the workspace posts when the device’s screens wake.
    NSWorkspaceScreensDidWakeNotification,
    // A notification that the workspace posts after a user session switches in.
    NSWorkspaceSessionDidBecomeActiveNotification,
    // A notification that the workspace posts before a user session switches out.
    NSWorkspaceSessionDidResignActiveNotification,
    // A notification that the workspace posts when the Finder is about to launch an app.
    NSWorkspaceWillLaunchApplicationNotification,
    // A notification that the workspace posts when the user requests a logout or powers off the device.
    NSWorkspaceWillPowerOffNotification,
    // A notification that the workspace posts before the device goes to sleep.
    NSWorkspaceWillSleepNotification,
    // A notification that the workspace posts when the Finder is about to unmount a device.
    NSWorkspaceWillUnmountNotification,
}

impl From<AppKitWorkspaceNotification> for Retained<NSNotificationName> {
    fn from(value: AppKitWorkspaceNotification) -> Self {
        NSNotificationName::from_str(&value.to_string())
    }
}

impl TryFrom<NSNotification> for AppKitWorkspaceNotification {
    type Error = strum::ParseError;

    fn try_from(value: NSNotification) -> Result<Self, Self::Error> {
        let name = value.name().to_string();

        AppKitWorkspaceNotification::from_str(&name)
    }
}
