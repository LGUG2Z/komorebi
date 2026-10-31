use crate::app_kit_notification_constants::AppKitWorkspaceNotification;
use crate::application::Application;
use crate::window_manager_event::SystemNotification;
use crate::window_manager_event::WindowManagerEvent;
use crate::window_manager_event_listener;
use objc2::AnyThread;
use objc2::define_class;
use objc2::msg_send;
use objc2::rc::Retained;
use objc2::runtime::NSObject;
use objc2::sel;
use objc2_app_kit::NSApplication;
use objc2_app_kit::NSRunningApplication;
use objc2_app_kit::NSWorkspace;
use objc2_foundation::NSDistributedNotificationCenter;
use objc2_foundation::NSNotification;
use objc2_foundation::NSNotificationCenter;
use objc2_foundation::NSNotificationName;
use objc2_foundation::NSString;
use std::str::FromStr;

#[derive(Debug, Clone)]
pub struct Ivars {}

// this is gross, auto formatting doesn't work inside here
define_class! {
    #[unsafe(super(NSObject))]
    #[ivars = Ivars]
    #[derive(Debug)]
    pub struct NotificationCenterListenerNsObject;

    impl NotificationCenterListenerNsObject {
        #[unsafe(method(handleNotification:))]
        fn handle_notification(&self, notif: &NSNotification) {
            let mut process_id = None;
            let mut window_id = None;
            let mut valid_keys = vec![];
            let mut application = None;

            if let Some(user_info) = notif.userInfo() {
                let all_keys = user_info.allKeys();
                for k in all_keys {
                    if let Some(key) = k.downcast_ref::<NSString>()  {
                        valid_keys.push(key.to_string());
                    }
                }

                if let Some(application_key) =
                    user_info.valueForKey(&NSString::from_str("NSWorkspaceApplicationKey"))
                    && let Some(running_application) = application_key.downcast_ref::<NSRunningApplication>()
                {
                    process_id = Some(running_application.processIdentifier());
                    if let Ok(app) = Application::new(running_application.processIdentifier()) {
                        window_id = app.main_window_id();
                        application = Some(app);
                    }
                }
            }

            tracing::trace!("received {} with keys {}", notif.name(), valid_keys.join(", "));

            match process_id {
                None => {
                    if let Ok(notification) =
                        AppKitWorkspaceNotification::from_str(&notif.name().to_string()) {
                        if matches!(notification, AppKitWorkspaceNotification::NSWorkspaceActiveSpaceDidChangeNotification) {
                            // TODO: this is really, really stupid
                            window_manager_event_listener::send_notification(WindowManagerEvent::SpaceChange(SystemNotification::AppKitWorkspace(notification), 0));
                        }
                    } else {
                        tracing::debug!(
                            "notification: {}, skipping as there is no associated process id",
                            notif.name()
                        );
                    }
                }
                Some(process_id) => {
                    tracing::debug!("notification: {}, process: {process_id}", notif.name());
                    match AppKitWorkspaceNotification::from_str(&notif.name().to_string()) {
                        Ok(AppKitWorkspaceNotification::NSWorkspaceDidActivateApplicationNotification) => {
                            if let Some(app) = &application {
                                match app.name().unwrap_or_default().as_str() {
                                    "loginwindow" => {
                                        window_manager_event_listener::send_notification(
                                            WindowManagerEvent::ScreenLock(
                                                SystemNotification::AppKitWorkspace(AppKitWorkspaceNotification::NSWorkspaceDidActivateApplicationNotification),
                                                process_id
                                            )
                                        );
                                    }
                                    app if !app.is_empty() => {
                                        if let Some(event) = WindowManagerEvent::from_system_notification(
                                            SystemNotification::AppKitWorkspace(AppKitWorkspaceNotification::NSWorkspaceDidActivateApplicationNotification),
                                            process_id,
                                            window_id,
                                        ) {
                                            window_manager_event_listener::send_notification(event);
                                        }
                                    }
                                    _ => {},
                                }
                            }
                        }
                        Ok(AppKitWorkspaceNotification::NSWorkspaceDidDeactivateApplicationNotification) => {
                            if let Some(app) = &application {
                                match app.name().unwrap_or_default().as_str() {
                                    "loginwindow" => {
                                        window_manager_event_listener::send_notification(
                                            WindowManagerEvent::ScreenUnlock(
                                                SystemNotification::AppKitWorkspace(AppKitWorkspaceNotification::NSWorkspaceDidDeactivateApplicationNotification),
                                                process_id
                                            )
                                        );
                                    }
                                    app if !app.is_empty() => {
                                        if let Some(event) = WindowManagerEvent::from_system_notification(
                                            SystemNotification::AppKitWorkspace(AppKitWorkspaceNotification::NSWorkspaceDidDeactivateApplicationNotification),
                                            process_id,
                                            window_id,
                                        ) {
                                            window_manager_event_listener::send_notification(event);
                                        }
                                    }
                                    _ => {},
                                }
                            }
                        }
                        Ok(notification) => {
                            if let Some(event) = WindowManagerEvent::from_system_notification(
                                SystemNotification::AppKitWorkspace(notification),
                                process_id,
                                window_id,
                            ) {
                                window_manager_event_listener::send_notification(event);
                            }
                        }
                        _ => {}
                    }
                }
            }
        }
    }
}

#[derive(Debug)]
pub struct NotificationCenterListener {
    pub inner: Retained<NotificationCenterListenerNsObject>,
}

impl Drop for NotificationCenterListener {
    fn drop(&mut self) {
        tracing::info!("removing notification center observer");
        unsafe {
            NSWorkspace::sharedWorkspace()
                .notificationCenter()
                .removeObserver(&self.inner);
            NSNotificationCenter::defaultCenter().removeObserver(&self.inner);
            NSDistributedNotificationCenter::defaultCenter().removeObserver(&self.inner);
        }
    }
}

impl NotificationCenterListenerNsObject {
    fn new() -> Retained<Self> {
        // if we don't set ivars here this fails to compile
        let this = Self::alloc().set_ivars(Ivars {});
        unsafe { msg_send![super(this), init] }
    }
}

impl NotificationCenterListener {
    pub fn init() -> Self {
        let observer = NotificationCenterListener {
            inner: NotificationCenterListenerNsObject::new(),
        };

        unsafe {
            // this is needed to get the spice flowing??
            if !NSApplication::load() {
                panic!("NSApplicationLoad failed");
            }

            let shared_workspace = NSWorkspace::sharedWorkspace();
            let notification_center = shared_workspace.notificationCenter();

            // https://developer.apple.com/documentation/appkit/nsworkspace/willlaunchapplicationnotification?language=objc
            for notification in [
                // A notification that the workspace posts when a new app starts up.
                AppKitWorkspaceNotification::NSWorkspaceDidLaunchApplicationNotification,
                // A notification that the workspace posts when an app finishes executing.
                AppKitWorkspaceNotification::NSWorkspaceDidTerminateApplicationNotification,
                // A notification that the workspace posts after a user session switches in.
                AppKitWorkspaceNotification::NSWorkspaceSessionDidBecomeActiveNotification,
                // A notification that the workspace posts before a user session switches out.
                AppKitWorkspaceNotification::NSWorkspaceSessionDidResignActiveNotification,
                // A notification that the workspace posts when the Finder hides an app.
                AppKitWorkspaceNotification::NSWorkspaceDidHideApplicationNotification,
                // A notification that the workspace posts when the Finder unhides an app.
                AppKitWorkspaceNotification::NSWorkspaceDidUnhideApplicationNotification,
                // A notification that the workspace posts when the Finder is about to activate an app.
                AppKitWorkspaceNotification::NSWorkspaceDidActivateApplicationNotification,
                // A notification that the workspace posts when the Finder deactivates an app.
                AppKitWorkspaceNotification::NSWorkspaceDidDeactivateApplicationNotification,
                // A notification that the workspace posts when a volume changes its name or mount path.
                AppKitWorkspaceNotification::NSWorkspaceDidRenameVolumeNotification,
                // A notification that the workspace posts when a new device mounts.
                AppKitWorkspaceNotification::NSWorkspaceDidMountNotification,
                // A notification that the workspace posts when the Finder is about to unmount a device.
                AppKitWorkspaceNotification::NSWorkspaceWillUnmountNotification,
                // A notification that the workspace posts when the Finder unmounts a device.
                AppKitWorkspaceNotification::NSWorkspaceDidUnmountNotification,
                // A notification that the workspace posts when the Finder file labels or colors change.
                AppKitWorkspaceNotification::NSWorkspaceDidChangeFileLabelsNotification,
                // A notification that the workspace posts when a Spaces change occurs.
                AppKitWorkspaceNotification::NSWorkspaceActiveSpaceDidChangeNotification,
                // A notification that the workspace posts when the device wakes from sleep.
                AppKitWorkspaceNotification::NSWorkspaceDidWakeNotification,
            ] {
                let notification_name: Retained<NSNotificationName> = notification.into();

                // thanks, I hate it
                notification_center.addObserver_selector_name_object(
                    &observer.inner,
                    sel!(handleNotification:),
                    Some(&notification_name),
                    None,
                );
            }

            observer
        }
    }
}
