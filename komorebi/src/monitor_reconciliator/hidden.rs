use std::sync::mpsc;
use std::time::Duration;

use windows::core::PCWSTR;
use windows::Win32::Foundation::HWND;
use windows::Win32::Foundation::LPARAM;
use windows::Win32::Foundation::LRESULT;
use windows::Win32::Foundation::WPARAM;
use windows::Win32::UI::WindowsAndMessaging::DefWindowProcW;
use windows::Win32::UI::WindowsAndMessaging::DispatchMessageW;
use windows::Win32::UI::WindowsAndMessaging::GetMessageW;
use windows::Win32::UI::WindowsAndMessaging::TranslateMessage;
use windows::Win32::UI::WindowsAndMessaging::CS_HREDRAW;
use windows::Win32::UI::WindowsAndMessaging::CS_VREDRAW;
use windows::Win32::UI::WindowsAndMessaging::DBT_DEVNODES_CHANGED;
use windows::Win32::UI::WindowsAndMessaging::MSG;
use windows::Win32::UI::WindowsAndMessaging::PBT_APMRESUMEAUTOMATIC;
use windows::Win32::UI::WindowsAndMessaging::PBT_APMRESUMESUSPEND;
use windows::Win32::UI::WindowsAndMessaging::PBT_APMSUSPEND;
use windows::Win32::UI::WindowsAndMessaging::SPI_SETWORKAREA;
use windows::Win32::UI::WindowsAndMessaging::WM_DEVICECHANGE;
use windows::Win32::UI::WindowsAndMessaging::WM_DISPLAYCHANGE;
use windows::Win32::UI::WindowsAndMessaging::WM_POWERBROADCAST;
use windows::Win32::UI::WindowsAndMessaging::WM_SETTINGCHANGE;
use windows::Win32::UI::WindowsAndMessaging::WM_WTSSESSION_CHANGE;
use windows::Win32::UI::WindowsAndMessaging::WNDCLASSW;
use windows::Win32::UI::WindowsAndMessaging::WTS_SESSION_LOCK;
use windows::Win32::UI::WindowsAndMessaging::WTS_SESSION_UNLOCK;

use crate::monitor_reconciliator;
use crate::WindowsApi;

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
        HWND(self.hwnd)
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

        std::thread::spawn(move || -> color_eyre::Result<()> {
            let hwnd = WindowsApi::create_hidden_window(PCWSTR(name.as_ptr()), h_module)?;
            hwnd_sender.send(hwnd)?;

            let mut msg: MSG = MSG::default();

            loop {
                unsafe {
                    if !GetMessageW(&mut msg, HWND::default(), 0, 0).as_bool() {
                        tracing::debug!("hidden window event processing thread shutdown");
                        break;
                    };
                    TranslateMessage(&msg);
                    DispatchMessageW(&msg);
                }

                std::thread::sleep(Duration::from_millis(10))
            }

            Ok(())
        });

        let hwnd = hwnd_receiver.recv()?;

        WindowsApi::wts_register_session_notification(hwnd)?;

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
                                monitor_reconciliator::Notification::ResumingFromSuspendedState,
                            );
                            LRESULT(0)
                        }
                        // Computer is entering a suspended state
                        PBT_APMSUSPEND => {
                            tracing::debug!(
                                "WM_POWERBROADCAST event received - entering suspended state"
                            );
                            monitor_reconciliator::send_notification(
                                monitor_reconciliator::Notification::EnteringSuspendedState,
                            );
                            LRESULT(0)
                        }
                        _ => LRESULT(0),
                    }
                }
                WM_WTSSESSION_CHANGE => {
                    match wparam.0 as u32 {
                        WTS_SESSION_LOCK => {
                            tracing::debug!("WM_WTSSESSION_CHANGE event received with WTS_SESSION_LOCK - screen locked");

                            monitor_reconciliator::send_notification(
                                monitor_reconciliator::Notification::SessionLocked,
                            );
                        }
                        WTS_SESSION_UNLOCK => {
                            tracing::debug!("WM_WTSSESSION_CHANGE event received with WTS_SESSION_UNLOCK - screen unlocked");

                            monitor_reconciliator::send_notification(
                                monitor_reconciliator::Notification::SessionUnlocked,
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
                        "WM_DISPLAYCHANGE event received with wparam: {}- work area or display resolution changed", wparam.0
                    );

                    monitor_reconciliator::send_notification(
                        monitor_reconciliator::Notification::ResolutionScalingChanged,
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
                            monitor_reconciliator::Notification::WorkAreaChanged,
                        );
                    }
                    LRESULT(0)
                }
                // This event + wparam combo is sent 4 times when a monitor is added based on my testing on win11
                // Original idea from https://stackoverflow.com/a/33762334
                WM_DEVICECHANGE => {
                    #[allow(clippy::cast_possible_truncation)]
                    if wparam.0 as u32 == DBT_DEVNODES_CHANGED {
                        tracing::debug!(
                                "WM_DEVICECHANGE event received with DBT_DEVNODES_CHANGED - display added or removed"
                            );
                        monitor_reconciliator::send_notification(
                            monitor_reconciliator::Notification::DisplayConnectionChange,
                        );
                    }

                    LRESULT(0)
                }
                _ => DefWindowProcW(window, message, wparam, lparam),
            }
        }
    }
}
