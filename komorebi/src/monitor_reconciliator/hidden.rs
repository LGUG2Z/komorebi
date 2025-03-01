use std::sync::mpsc;
use std::time::Duration;

use windows::Win32::Devices::Display::GUID_DEVINTERFACE_DISPLAY_ADAPTER;
use windows::Win32::Devices::Display::GUID_DEVINTERFACE_MONITOR;
use windows::Win32::Devices::Display::GUID_DEVINTERFACE_VIDEO_OUTPUT_ARRIVAL;
use windows::Win32::Foundation::HWND;
use windows::Win32::Foundation::LPARAM;
use windows::Win32::Foundation::LRESULT;
use windows::Win32::Foundation::WPARAM;
use windows::Win32::System::Power::POWERBROADCAST_SETTING;
use windows::Win32::System::SystemServices::GUID_LIDSWITCH_STATE_CHANGE;
use windows::Win32::UI::WindowsAndMessaging::CS_HREDRAW;
use windows::Win32::UI::WindowsAndMessaging::CS_VREDRAW;
use windows::Win32::UI::WindowsAndMessaging::DBT_CONFIGCHANGED;
use windows::Win32::UI::WindowsAndMessaging::DBT_DEVICEARRIVAL;
use windows::Win32::UI::WindowsAndMessaging::DBT_DEVICEREMOVECOMPLETE;
use windows::Win32::UI::WindowsAndMessaging::DBT_DEVNODES_CHANGED;
use windows::Win32::UI::WindowsAndMessaging::DBT_DEVTYP_DEVICEINTERFACE;
use windows::Win32::UI::WindowsAndMessaging::DEV_BROADCAST_DEVICEINTERFACE_W;
use windows::Win32::UI::WindowsAndMessaging::DefWindowProcW;
use windows::Win32::UI::WindowsAndMessaging::DispatchMessageW;
use windows::Win32::UI::WindowsAndMessaging::GetMessageW;
use windows::Win32::UI::WindowsAndMessaging::MSG;
use windows::Win32::UI::WindowsAndMessaging::PBT_APMRESUMEAUTOMATIC;
use windows::Win32::UI::WindowsAndMessaging::PBT_APMRESUMESUSPEND;
use windows::Win32::UI::WindowsAndMessaging::PBT_APMSUSPEND;
use windows::Win32::UI::WindowsAndMessaging::PBT_POWERSETTINGCHANGE;
use windows::Win32::UI::WindowsAndMessaging::REGISTER_NOTIFICATION_FLAGS;
use windows::Win32::UI::WindowsAndMessaging::SPI_SETWORKAREA;
use windows::Win32::UI::WindowsAndMessaging::TranslateMessage;
use windows::Win32::UI::WindowsAndMessaging::WM_DEVICECHANGE;
use windows::Win32::UI::WindowsAndMessaging::WM_DISPLAYCHANGE;
use windows::Win32::UI::WindowsAndMessaging::WM_POWERBROADCAST;
use windows::Win32::UI::WindowsAndMessaging::WM_SETTINGCHANGE;
use windows::Win32::UI::WindowsAndMessaging::WM_WTSSESSION_CHANGE;
use windows::Win32::UI::WindowsAndMessaging::WNDCLASSW;
use windows::Win32::UI::WindowsAndMessaging::WTS_SESSION_LOCK;
use windows::Win32::UI::WindowsAndMessaging::WTS_SESSION_UNLOCK;
use windows::core::PCWSTR;

use crate::WindowsApi;
use crate::monitor_reconciliator;
use crate::windows_api;

// This is a hidden window specifically spawned to listen to system-wide events related to monitors
#[derive(Debug, Clone, Copy)]
pub struct Hidden {
    pub hwnd: isize,
}

impl From<isize> for Hidden {
    fn from(hwnd: isize) -> Self {
        Self { hwnd }
    }
}

impl Hidden {
    pub const fn hwnd(self) -> HWND {
        HWND(windows_api::as_ptr!(self.hwnd))
    }

    pub fn create(name: &str) -> color_eyre::Result<Self> {
        let name: Vec<u16> = format!("{name}\0").encode_utf16().collect();
        let class_name = PCWSTR(name.as_ptr());

        let h_module = WindowsApi::module_handle_w()?;
        let window_class = WNDCLASSW {
            hInstance: h_module.into(),
            lpszClassName: class_name,
            style: CS_HREDRAW | CS_VREDRAW,
            lpfnWndProc: Some(Self::callback),
            hbrBackground: WindowsApi::create_solid_brush(0),
            ..Default::default()
        };

        let _ = WindowsApi::register_class_w(&window_class)?;

        let (hwnd_sender, hwnd_receiver) = mpsc::channel();

        let instance = h_module.0 as isize;
        std::thread::spawn(move || -> color_eyre::Result<()> {
            let hwnd = WindowsApi::create_hidden_window(PCWSTR(name.as_ptr()), instance)?;
            hwnd_sender.send(hwnd)?;

            let mut msg: MSG = MSG::default();

            loop {
                unsafe {
                    if !GetMessageW(&mut msg, None, 0, 0).as_bool() {
                        tracing::debug!("hidden window event processing thread shutdown");
                        break;
                    };
                    // TODO: error handling
                    let _ = TranslateMessage(&msg);
                    DispatchMessageW(&msg);
                }

                std::thread::sleep(Duration::from_millis(10))
            }

            Ok(())
        });

        let hwnd = hwnd_receiver.recv()?;

        // Register Session Lock/Unlock events
        WindowsApi::wts_register_session_notification(hwnd)?;

        // Register Laptop lid open/close events
        WindowsApi::register_power_setting_notification(
            hwnd,
            &GUID_LIDSWITCH_STATE_CHANGE,
            REGISTER_NOTIFICATION_FLAGS(0),
        )?;

        // Register device interface events for multiple display related devices. Some of this
        // device interfaces might not be needed but it doesn't hurt to have them in case some user
        // uses some output device as monitor that falls into one of these device interface class
        // GUID.
        let monitor_filter = DEV_BROADCAST_DEVICEINTERFACE_W {
            dbcc_size: std::mem::size_of::<DEV_BROADCAST_DEVICEINTERFACE_W>() as u32,
            dbcc_devicetype: DBT_DEVTYP_DEVICEINTERFACE.0,
            dbcc_reserved: 0,
            dbcc_classguid: GUID_DEVINTERFACE_MONITOR,
            dbcc_name: [0; 1],
        };
        let display_adapter_filter = DEV_BROADCAST_DEVICEINTERFACE_W {
            dbcc_size: std::mem::size_of::<DEV_BROADCAST_DEVICEINTERFACE_W>() as u32,
            dbcc_devicetype: DBT_DEVTYP_DEVICEINTERFACE.0,
            dbcc_reserved: 0,
            dbcc_classguid: GUID_DEVINTERFACE_DISPLAY_ADAPTER,
            dbcc_name: [0; 1],
        };
        let video_output_filter = DEV_BROADCAST_DEVICEINTERFACE_W {
            dbcc_size: std::mem::size_of::<DEV_BROADCAST_DEVICEINTERFACE_W>() as u32,
            dbcc_devicetype: DBT_DEVTYP_DEVICEINTERFACE.0,
            dbcc_reserved: 0,
            dbcc_classguid: GUID_DEVINTERFACE_VIDEO_OUTPUT_ARRIVAL,
            dbcc_name: [0; 1],
        };
        WindowsApi::register_device_notification(
            hwnd,
            monitor_filter,
            REGISTER_NOTIFICATION_FLAGS(0),
        )?;
        WindowsApi::register_device_notification(
            hwnd,
            display_adapter_filter,
            REGISTER_NOTIFICATION_FLAGS(0),
        )?;
        WindowsApi::register_device_notification(
            hwnd,
            video_output_filter,
            REGISTER_NOTIFICATION_FLAGS(0),
        )?;

        Ok(Self { hwnd })
    }

    pub extern "system" fn callback(
        window: HWND,
        message: u32,
        wparam: WPARAM,
        lparam: LPARAM,
    ) -> LRESULT {
        unsafe {
            match message {
                WM_POWERBROADCAST => {
                    match wparam.0 as u32 {
                        // Automatic: System resumed itself from sleep or hibernation
                        // Suspend: User resumed system from sleep or hibernation
                        PBT_APMRESUMEAUTOMATIC | PBT_APMRESUMESUSPEND => {
                            tracing::debug!(
                                "WM_POWERBROADCAST event received - resume from suspend"
                            );
                            monitor_reconciliator::send_notification(
                                monitor_reconciliator::MonitorNotification::ResumingFromSuspendedState,
                            );
                            LRESULT(0)
                        }
                        // Computer is entering a suspended state
                        PBT_APMSUSPEND => {
                            tracing::debug!(
                                "WM_POWERBROADCAST event received - entering suspended state"
                            );
                            monitor_reconciliator::send_notification(
                                monitor_reconciliator::MonitorNotification::EnteringSuspendedState,
                            );
                            LRESULT(0)
                        }
                        // Monitor change power status
                        PBT_POWERSETTINGCHANGE => {
                            if let POWERBROADCAST_SETTING {
                                PowerSetting: GUID_LIDSWITCH_STATE_CHANGE,
                                DataLength: _,
                                Data: [0],
                            } = *(lparam.0 as *const POWERBROADCAST_SETTING)
                            {
                                tracing::debug!(
                                    "WM_POWERBROADCAST event received - laptop lid closed"
                                );
                                monitor_reconciliator::send_notification(
                                    monitor_reconciliator::MonitorNotification::DisplayConnectionChange,
                                );
                            } else if let POWERBROADCAST_SETTING {
                                PowerSetting: GUID_LIDSWITCH_STATE_CHANGE,
                                DataLength: _,
                                Data: [1],
                            } = *(lparam.0 as *const POWERBROADCAST_SETTING)
                            {
                                tracing::debug!(
                                    "WM_POWERBROADCAST event received - laptop lid opened"
                                );
                                monitor_reconciliator::send_notification(
                                    monitor_reconciliator::MonitorNotification::DisplayConnectionChange,
                                );
                            }
                            LRESULT(0)
                        }
                        _ => LRESULT(0),
                    }
                }
                WM_WTSSESSION_CHANGE => {
                    match wparam.0 as u32 {
                        WTS_SESSION_LOCK => {
                            tracing::debug!(
                                "WM_WTSSESSION_CHANGE event received with WTS_SESSION_LOCK - screen locked"
                            );

                            monitor_reconciliator::send_notification(
                                monitor_reconciliator::MonitorNotification::SessionLocked,
                            );
                        }
                        WTS_SESSION_UNLOCK => {
                            tracing::debug!(
                                "WM_WTSSESSION_CHANGE event received with WTS_SESSION_UNLOCK - screen unlocked"
                            );

                            monitor_reconciliator::send_notification(
                                monitor_reconciliator::MonitorNotification::SessionUnlocked,
                            );
                        }
                        _ => {}
                    }

                    LRESULT(0)
                }
                // This event gets sent when:
                // - The scaling factor on a display changes
                // - The resolution on a display changes
                // - A monitor is added
                // - A monitor is removed
                // Since WM_DEVICECHANGE also notifies on monitor changes, we only handle scaling
                // and resolution changes here
                WM_DISPLAYCHANGE => {
                    tracing::debug!(
                        "WM_DISPLAYCHANGE event received with wparam: {}- work area or display resolution changed",
                        wparam.0
                    );

                    monitor_reconciliator::send_notification(
                        monitor_reconciliator::MonitorNotification::ResolutionScalingChanged,
                    );
                    LRESULT(0)
                }
                // Unfortunately this is the event sent with ButteryTaskbar which I use a lot
                // Original idea from https://stackoverflow.com/a/33762334
                WM_SETTINGCHANGE => {
                    #[allow(clippy::cast_possible_truncation)]
                    if wparam.0 as u32 == SPI_SETWORKAREA.0 {
                        tracing::debug!(
                            "WM_SETTINGCHANGE event received with SPI_SETWORKAREA - work area changed (probably butterytaskbar or something similar)"
                        );

                        monitor_reconciliator::send_notification(
                            monitor_reconciliator::MonitorNotification::WorkAreaChanged,
                        );
                    }
                    LRESULT(0)
                }
                // This event + wparam combo is sent 4 times when a monitor is added based on my testing on win11
                // Original idea from https://stackoverflow.com/a/33762334
                WM_DEVICECHANGE => {
                    #[allow(clippy::cast_possible_truncation)]
                    let event = wparam.0 as u32;
                    if event == DBT_DEVNODES_CHANGED
                        || event == DBT_CONFIGCHANGED
                        || event == DBT_DEVICEARRIVAL
                        || event == DBT_DEVICEREMOVECOMPLETE
                    {
                        tracing::debug!(
                            "WM_DEVICECHANGE event received with one of [DBT_DEVNODES_CHANGED, DBT_CONFIGCHANGED, DBT_DEVICEARRIVAL, DBT_DEVICEREMOVECOMPLETE] - display added or removed"
                        );
                        monitor_reconciliator::send_notification(
                            monitor_reconciliator::MonitorNotification::DisplayConnectionChange,
                        );
                    }

                    LRESULT(0)
                }
                _ => DefWindowProcW(window, message, wparam, lparam),
            }
        }
    }
}
