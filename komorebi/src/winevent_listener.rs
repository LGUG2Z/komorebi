use std::sync::OnceLock;

use crossbeam_channel::Receiver;
use crossbeam_channel::Sender;
use windows::Win32::Foundation::HWND;
use windows::Win32::UI::Accessibility::SetWinEventHook;
use windows::Win32::UI::WindowsAndMessaging::DispatchMessageW;
use windows::Win32::UI::WindowsAndMessaging::GetMessageW;
use windows::Win32::UI::WindowsAndMessaging::TranslateMessage;
use windows::Win32::UI::WindowsAndMessaging::EVENT_MAX;
use windows::Win32::UI::WindowsAndMessaging::EVENT_MIN;
use windows::Win32::UI::WindowsAndMessaging::MSG;

use crate::window_manager_event::WindowManagerEvent;
use crate::windows_callbacks;

static CHANNEL: OnceLock<(Sender<WindowManagerEvent>, Receiver<WindowManagerEvent>)> =
    OnceLock::new();

static EVENT_PUMP: OnceLock<std::thread::JoinHandle<()>> = OnceLock::new();

pub fn start() {
    EVENT_PUMP.get_or_init(|| {
        std::thread::spawn(move || {
            unsafe {
                SetWinEventHook(
                    EVENT_MIN,
                    EVENT_MAX,
                    None,
                    Some(windows_callbacks::win_event_hook),
                    0,
                    0,
                    0,
                )
            };

            loop {
                let mut msg: MSG = MSG::default();
                unsafe {
                    if !GetMessageW(&mut msg, HWND(0), 0, 0).as_bool() {
                        tracing::info!("windows event processing shutdown");
                        break;
                    };
                    TranslateMessage(&msg);
                    DispatchMessageW(&msg);
                }
            }
        })
    });
}

fn channel() -> &'static (Sender<WindowManagerEvent>, Receiver<WindowManagerEvent>) {
    CHANNEL.get_or_init(crossbeam_channel::unbounded)
}

pub fn event_tx() -> Sender<WindowManagerEvent> {
    channel().0.clone()
}

pub fn event_rx() -> Receiver<WindowManagerEvent> {
    channel().1.clone()
}
