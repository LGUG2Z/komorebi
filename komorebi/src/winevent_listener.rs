use std::sync::atomic::AtomicIsize;
use std::sync::atomic::Ordering;
use std::sync::Arc;
use std::thread;
use std::time::Duration;

use crossbeam_channel::Receiver;
use crossbeam_channel::Sender;
use lazy_static::lazy_static;
use parking_lot::Mutex;
use windows::Win32::Foundation::HWND;
use windows::Win32::UI::Accessibility::SetWinEventHook;
use windows::Win32::UI::WindowsAndMessaging::DispatchMessageW;
use windows::Win32::UI::WindowsAndMessaging::PeekMessageW;
use windows::Win32::UI::WindowsAndMessaging::TranslateMessage;
use windows::Win32::UI::WindowsAndMessaging::EVENT_MAX;
use windows::Win32::UI::WindowsAndMessaging::EVENT_MIN;
use windows::Win32::UI::WindowsAndMessaging::MSG;
use windows::Win32::UI::WindowsAndMessaging::PM_REMOVE;

use crate::window_manager_event::WindowManagerEvent;
use crate::windows_callbacks;

lazy_static! {
    pub static ref WINEVENT_CALLBACK_CHANNEL: Arc<Mutex<(Sender<WindowManagerEvent>, Receiver<WindowManagerEvent>)>> =
        Arc::new(Mutex::new(crossbeam_channel::unbounded()));
}

#[derive(Debug, Clone)]
pub struct WinEventListener {
    hook: Arc<AtomicIsize>,
    outgoing_events: Arc<Mutex<Sender<WindowManagerEvent>>>,
}

impl WinEventListener {
    #[must_use]
    pub fn new(outgoing: Arc<Mutex<Sender<WindowManagerEvent>>>) -> Self {
        Self {
            hook: Arc::new(AtomicIsize::new(0)),
            outgoing_events: outgoing,
        }
    }

    pub fn start(self) {
        let hook = self.hook.clone();
        let outgoing = self.outgoing_events.lock().clone();

        thread::spawn(move || unsafe {
            let hook_ref = SetWinEventHook(
                EVENT_MIN as u32,
                EVENT_MAX as u32,
                None,
                Some(windows_callbacks::win_event_hook),
                0,
                0,
                0,
            );

            hook.store(hook_ref.0, Ordering::SeqCst);

            // The code in the callback doesn't work in its own loop, needs to be within
            // the MessageLoop callback for the winevent callback to even fire
            MessageLoop::start(10, |_msg| {
                if let Ok(event) = WINEVENT_CALLBACK_CHANNEL.lock().1.try_recv() {
                    match outgoing.send(event) {
                        Ok(_) => {}
                        Err(error) => {
                            tracing::error!("{}", error);
                        }
                    }
                }

                true
            });
        });
    }
}

#[derive(Debug, Copy, Clone)]
pub struct MessageLoop;

impl MessageLoop {
    pub fn start(sleep: u64, cb: impl Fn(Option<MSG>) -> bool) {
        Self::start_with_sleep(sleep, cb);
    }

    fn start_with_sleep(sleep: u64, cb: impl Fn(Option<MSG>) -> bool) {
        let mut msg: MSG = MSG::default();
        loop {
            let mut value: Option<MSG> = None;
            unsafe {
                if !bool::from(!PeekMessageW(&mut msg, HWND(0), 0, 0, PM_REMOVE)) {
                    TranslateMessage(&msg);
                    DispatchMessageW(&msg);

                    value = Some(msg);
                }
            }

            thread::sleep(Duration::from_millis(sleep));

            if !cb(value) {
                break;
            }
        }
    }
}
