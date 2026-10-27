//! Hearing about sleep, wake, the lid, the displays and the session from Windows as they happen.
//!
//! A hidden window on a thread of its own, the way komorebi for Windows listens for the same
//! things: Windows sends suspend and resume (and display changes) to every top-level window, and
//! the lid, the displays' power and session lock are registered for. Each message is stamped with a
//! clock anchor on arrival and handed to the recording loop over a channel; nothing is done on the
//! window's thread but that.
//!
//! [`start`] makes a [`Watcher`], which the recording loop drains with [`Watcher::drain`] and
//! hands to [`PowerTracker`](crate::power::PowerTracker). What Windows says becomes a
//! [`PowerKind`]:
//!
//! | Windows says | becomes |
//! |---|---|
//! | `PBT_APMSUSPEND` | `Suspend` |
//! | `PBT_APMRESUMEAUTOMATIC` or `PBT_APMRESUMESUSPEND` (woken by a timer, or by the user) | `Resume` |
//! | the lid switch changed to closed, or to opened | `LidClosed`, `LidOpened` |
//! | the console display turned off, or on (dimmed is ignored) | `DisplaysOff`, `DisplaysOn` |
//! | this session was locked, or unlocked (`WM_WTSSESSION_CHANGE`) | `SessionLocked`, `SessionUnlocked` |
//! | a display was added or removed, or changed size or scale (`WM_DISPLAYCHANGE`) | `DisplaysChanged` |
//!
//! A machine with no lid says so when the watcher registers for it, and the rest still works.
//!
//! macOS has no watcher: [`crate::platform::power`] there starts none, and the recording loop finds
//! a sleep from the clocks. Nothing tests this module automatically; the logic that reads its
//! events is tested through [`PowerTracker`](crate::power::PowerTracker), and the window itself is
//! checked on a real machine (see `docs/dev/windows-checkpoints.md`).

use crate::clock::Anchor;
use crate::power::PowerEvent;
use crate::power::PowerKind;
use color_eyre::eyre;
use color_eyre::eyre::WrapErr;
use crossbeam_channel::Receiver;
use crossbeam_channel::Sender;
use std::sync::mpsc;
use std::thread::JoinHandle;
use windows::Win32::Foundation::HANDLE;
use windows::Win32::Foundation::HWND;
use windows::Win32::Foundation::LPARAM;
use windows::Win32::Foundation::LRESULT;
use windows::Win32::Foundation::WPARAM;
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::System::Power::POWERBROADCAST_SETTING;
use windows::Win32::System::Power::RegisterPowerSettingNotification;
use windows::Win32::System::Power::UnregisterPowerSettingNotification;
use windows::Win32::System::RemoteDesktop::WTSRegisterSessionNotification;
use windows::Win32::System::RemoteDesktop::WTSUnRegisterSessionNotification;
use windows::Win32::System::SystemServices::GUID_CONSOLE_DISPLAY_STATE;
use windows::Win32::System::SystemServices::GUID_LIDSWITCH_STATE_CHANGE;
use windows::Win32::UI::WindowsAndMessaging::CW_USEDEFAULT;
use windows::Win32::UI::WindowsAndMessaging::CreateWindowExW;
use windows::Win32::UI::WindowsAndMessaging::DefWindowProcW;
use windows::Win32::UI::WindowsAndMessaging::DestroyWindow;
use windows::Win32::UI::WindowsAndMessaging::DispatchMessageW;
use windows::Win32::UI::WindowsAndMessaging::GWLP_USERDATA;
use windows::Win32::UI::WindowsAndMessaging::GetMessageW;
use windows::Win32::UI::WindowsAndMessaging::GetWindowLongPtrW;
use windows::Win32::UI::WindowsAndMessaging::MSG;
use windows::Win32::UI::WindowsAndMessaging::PBT_APMRESUMEAUTOMATIC;
use windows::Win32::UI::WindowsAndMessaging::PBT_APMRESUMESUSPEND;
use windows::Win32::UI::WindowsAndMessaging::PBT_APMSUSPEND;
use windows::Win32::UI::WindowsAndMessaging::PBT_POWERSETTINGCHANGE;
use windows::Win32::UI::WindowsAndMessaging::PostMessageW;
use windows::Win32::UI::WindowsAndMessaging::PostQuitMessage;
use windows::Win32::UI::WindowsAndMessaging::REGISTER_NOTIFICATION_FLAGS;
use windows::Win32::UI::WindowsAndMessaging::RegisterClassW;
use windows::Win32::UI::WindowsAndMessaging::SetWindowLongPtrW;
use windows::Win32::UI::WindowsAndMessaging::TranslateMessage;
use windows::Win32::UI::WindowsAndMessaging::WM_CLOSE;
use windows::Win32::UI::WindowsAndMessaging::WM_DESTROY;
use windows::Win32::UI::WindowsAndMessaging::WM_DISPLAYCHANGE;
use windows::Win32::UI::WindowsAndMessaging::WM_POWERBROADCAST;
use windows::Win32::UI::WindowsAndMessaging::WM_WTSSESSION_CHANGE;
use windows::Win32::UI::WindowsAndMessaging::WNDCLASSW;
use windows::Win32::UI::WindowsAndMessaging::WS_DISABLED;
use windows::Win32::UI::WindowsAndMessaging::WS_EX_NOACTIVATE;
use windows::Win32::UI::WindowsAndMessaging::WTS_SESSION_LOCK;
use windows::Win32::UI::WindowsAndMessaging::WTS_SESSION_UNLOCK;
use windows::core::w;

/// `WTSRegisterSessionNotification`: only this session's lock and unlock.
const NOTIFY_FOR_THIS_SESSION: u32 = 0;

/// What `GUID_CONSOLE_DISPLAY_STATE` reports in its one byte of data.
const DISPLAY_OFF: u8 = 0;
const DISPLAY_ON: u8 = 1;
/// And `GUID_LIDSWITCH_STATE_CHANGE`.
const LID_CLOSED: u8 = 0;
const LID_OPENED: u8 = 1;

/// Listens for power events until dropped, and holds what it heard until it is drained.
///
/// Dropping a watcher asks its window to close and waits for the thread to finish.
pub struct Watcher {
    events: Receiver<PowerEvent>,
    window: isize,
    thread: Option<JoinHandle<()>>,
}

impl Watcher {
    /// Returns what has been heard since the last call, oldest first. Each [`PowerEvent`] carries
    /// the clock [`Anchor`] sampled when the message arrived.
    #[must_use]
    pub fn drain(&self) -> Vec<PowerEvent> {
        self.events.try_iter().collect()
    }
}

impl Drop for Watcher {
    fn drop(&mut self) {
        // ask the window to close itself on its own thread, which ends the message loop
        // SAFETY: posting to a window handle that is at worst already gone
        let _ = unsafe {
            PostMessageW(
                Some(HWND(self.window as *mut std::ffi::c_void)),
                WM_CLOSE,
                WPARAM(0),
                LPARAM(0),
            )
        };
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

fn emit(window: HWND, kind: PowerKind) {
    // SAFETY: the user data is either null or the `Sender` boxed by `run` for this window, which
    // outlives every message the window is sent
    let sender = unsafe { GetWindowLongPtrW(window, GWLP_USERDATA) } as *const Sender<PowerEvent>;
    // SAFETY: see above
    if let Some(sender) = unsafe { sender.as_ref() } {
        let _ = sender.send(PowerEvent {
            kind,
            anchor: Anchor::sample(),
        });
    }
}

unsafe extern "system" fn callback(
    window: HWND,
    message: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    match message {
        WM_POWERBROADCAST => {
            match wparam.0 as u32 {
                PBT_APMSUSPEND => emit(window, PowerKind::Suspend),
                // woken by itself (a timer) or by the user: either way, awake
                PBT_APMRESUMEAUTOMATIC | PBT_APMRESUMESUSPEND => emit(window, PowerKind::Resume),
                PBT_POWERSETTINGCHANGE => {
                    // SAFETY: for this message `lparam` points at a POWERBROADCAST_SETTING whose
                    // `DataLength` bytes of data follow it; only the first byte is read
                    let setting = unsafe { (lparam.0 as *const POWERBROADCAST_SETTING).as_ref() };
                    if let Some(setting) = setting
                        && setting.DataLength >= 1
                    {
                        let value = setting.Data[0];
                        if setting.PowerSetting == GUID_LIDSWITCH_STATE_CHANGE {
                            match value {
                                LID_CLOSED => emit(window, PowerKind::LidClosed),
                                LID_OPENED => emit(window, PowerKind::LidOpened),
                                _ => {}
                            }
                        } else if setting.PowerSetting == GUID_CONSOLE_DISPLAY_STATE {
                            match value {
                                DISPLAY_OFF => emit(window, PowerKind::DisplaysOff),
                                DISPLAY_ON => emit(window, PowerKind::DisplaysOn),
                                // dimmed: still showing
                                _ => {}
                            }
                        }
                    }
                }
                _ => {}
            }
            LRESULT(1)
        }
        WM_WTSSESSION_CHANGE => {
            match wparam.0 as u32 {
                WTS_SESSION_LOCK => emit(window, PowerKind::SessionLocked),
                WTS_SESSION_UNLOCK => emit(window, PowerKind::SessionUnlocked),
                _ => {}
            }
            LRESULT(0)
        }
        // a display was added or removed, or changed size or scale
        WM_DISPLAYCHANGE => {
            emit(window, PowerKind::DisplaysChanged);
            LRESULT(0)
        }
        WM_CLOSE => {
            // SAFETY: closing our own window on its own thread
            let _ = unsafe { DestroyWindow(window) };
            LRESULT(0)
        }
        WM_DESTROY => {
            // SAFETY: ends this thread's message loop
            unsafe { PostQuitMessage(0) };
            LRESULT(0)
        }
        // SAFETY: the defaults for everything else
        _ => unsafe { DefWindowProcW(window, message, wparam, lparam) },
    }
}

/// The window's thread: make the window, register for what it should hear, pump messages.
fn run(ready: &mpsc::Sender<eyre::Result<isize>>, events: Sender<PowerEvent>) {
    let setup = || -> eyre::Result<(HWND, Box<Sender<PowerEvent>>)> {
        // SAFETY: plain Win32 calls with the arguments they document; the boxed sender is stored
        // in the window's user data and freed after the loop ends, when no message can arrive
        unsafe {
            let instance = GetModuleHandleW(None).wrap_err("GetModuleHandleW")?;
            let class = WNDCLASSW {
                hInstance: instance.into(),
                lpszClassName: w!("komorebi-recorder-power"),
                lpfnWndProc: Some(callback),
                ..Default::default()
            };
            // a second watcher in the same process finds the class already registered
            RegisterClassW(&raw const class);

            // never shown, and disabled and unactivatable besides: a window only to be sent to
            let window = CreateWindowExW(
                WS_EX_NOACTIVATE,
                w!("komorebi-recorder-power"),
                w!("komorebi-recorder-power"),
                WS_DISABLED,
                CW_USEDEFAULT,
                CW_USEDEFAULT,
                CW_USEDEFAULT,
                CW_USEDEFAULT,
                None,
                None,
                Some(instance.into()),
                None,
            )
            .wrap_err("could not create the power window")?;

            let sender = Box::new(events);
            SetWindowLongPtrW(window, GWLP_USERDATA, &raw const *sender as isize);
            Ok((window, sender))
        }
    };

    let (window, sender) = match setup() {
        Ok(made) => made,
        Err(error) => {
            let _ = ready.send(Err(error));
            return;
        }
    };

    // SAFETY: registrations against the window just made, undone below
    let (lid, display, session) = unsafe {
        (
            RegisterPowerSettingNotification(
                HANDLE(window.0),
                &GUID_LIDSWITCH_STATE_CHANGE,
                REGISTER_NOTIFICATION_FLAGS(0),
            ),
            RegisterPowerSettingNotification(
                HANDLE(window.0),
                &GUID_CONSOLE_DISPLAY_STATE,
                REGISTER_NOTIFICATION_FLAGS(0),
            ),
            WTSRegisterSessionNotification(window, NOTIFY_FOR_THIS_SESSION),
        )
    };
    // a machine with no lid says so here; the rest still works
    let _ = ready.send(Ok(window.0 as isize));

    let mut message = MSG::default();
    // SAFETY: the standard message loop for this thread's window
    unsafe {
        while GetMessageW(&raw mut message, None, 0, 0).as_bool() {
            let _ = TranslateMessage(&raw const message);
            DispatchMessageW(&raw const message);
        }
    }

    // SAFETY: undoing the registrations above
    unsafe {
        if let Ok(lid) = lid {
            let _ = UnregisterPowerSettingNotification(lid);
        }
        if let Ok(display) = display {
            let _ = UnregisterPowerSettingNotification(display);
        }
        if session.is_ok() {
            let _ = WTSUnRegisterSessionNotification(window);
        }
    }
    drop(sender);
}

/// Starts listening, and returns the watcher. It is always `Some` on Windows: the `Option` is the
/// facade's, shared with hosts that have no watcher.
///
/// # Errors
///
/// Returns an error if the module handle cannot be read or the hidden window cannot be made, or if
/// the thread cannot be started or ends before it was ready. Failing to register for the lid, the
/// displays or the session lock is not an error: those messages are then not heard.
pub fn start() -> eyre::Result<Option<Watcher>> {
    let (events_tx, events_rx) = crossbeam_channel::unbounded();
    let (ready_tx, ready_rx) = mpsc::channel();
    let thread = std::thread::Builder::new()
        .name("komorebi-recorder-power".to_string())
        .spawn(move || run(&ready_tx, events_tx))?;

    let window = ready_rx
        .recv()
        .wrap_err("the power watcher thread ended before it was ready")??;

    Ok(Some(Watcher {
        events: events_rx,
        window,
        thread: Some(thread),
    }))
}
