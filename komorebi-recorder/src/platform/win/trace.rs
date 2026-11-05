//! The two taps of the Windows OS trace: a `WinEvent` hook, and Event Log subscriptions. See
//! [`crate::sources::wintrace`] for what is recorded and why.
//!
//! [`start`] starts both and returns them as one [`Taps`], which listens until it is dropped:
//!
//! - **The `WinEvent` hook** runs on a thread of its own (`komorebi-recorder-winevents`): it
//!   installs `SetWinEventHook` for every event of every process except the recorder's own, and
//!   pumps messages because the hook is delivered through them. The hook callback only stamps the
//!   event on the master clock and the wall clock and queues it. A second thread
//!   (`komorebi-recorder-trace-sink`) folds bursts ([`Coalescer`]), looks up the window's class and
//!   program, and writes the record to the `system` stream.
//! - **The Event Log subscriptions** are two `EvtSubscribe` queries on the `System` channel for
//!   future events, delivered on threads of the system's. Each event is rendered to XML,
//!   summarized ([`summarize_event_xml`]) and written to the `event_log` stream.
//!
//! macOS has no taps: [`crate::platform::trace`] there starts none and its OS trace is `log
//! stream`. Nothing tests this module automatically: the record format, the folding and the XML
//! reading it uses are tested in [`crate::sources::wintrace`], and the taps are checked on a real
//! machine (see `docs/dev/windows-checkpoints.md`).

/// What the manifest calls the stream of `WinEvent`s.
pub const SYSTEM_NAME: &str = "winevents";
/// What the manifest calls the stream of the Event Log.
pub const EVENT_LOG_NAME: &str = "eventlog";
/// What the stream of `WinEvent`s is, for the manifest.
pub const SYSTEM_ABOUT: &str = "WinEvent hook on every top-level window";
/// What the stream of the Event Log is, for the manifest.
pub const EVENT_LOG_ABOUT: &str = "System Event Log: warnings, errors, display and power providers";

use crate::clock;
use crate::sources::wintrace::Coalescer;
use crate::sources::wintrace::Offer;
use crate::sources::wintrace::TraceRecord;
use crate::sources::wintrace::TraceStream;
use crate::sources::wintrace::is_noisy;
use crate::sources::wintrace::summarize_event_xml;
use crate::sources::wintrace::win_event_name;
use crossbeam_channel::Receiver;
use crossbeam_channel::Sender;
use parking_lot::Mutex;
use std::collections::HashMap;
use std::ffi::c_void;
use std::sync::Arc;
use std::sync::OnceLock;
use std::thread::JoinHandle;
use windows::Win32::Foundation::HWND;
use windows::Win32::Foundation::LPARAM;
use windows::Win32::Foundation::WPARAM;
use windows::Win32::System::EventLog::EVT_HANDLE;
use windows::Win32::System::EventLog::EVT_SUBSCRIBE_NOTIFY_ACTION;
use windows::Win32::System::EventLog::EvtClose;
use windows::Win32::System::EventLog::EvtRender;
use windows::Win32::System::EventLog::EvtRenderEventXml;
use windows::Win32::System::EventLog::EvtSubscribe;
use windows::Win32::System::EventLog::EvtSubscribeActionDeliver;
use windows::Win32::System::EventLog::EvtSubscribeToFutureEvents;
use windows::Win32::System::Threading::GetCurrentThreadId;
use windows::Win32::UI::Accessibility::HWINEVENTHOOK;
use windows::Win32::UI::Accessibility::SetWinEventHook;
use windows::Win32::UI::Accessibility::UnhookWinEvent;
use windows::Win32::UI::WindowsAndMessaging::CHILDID_SELF;
use windows::Win32::UI::WindowsAndMessaging::DispatchMessageW;
use windows::Win32::UI::WindowsAndMessaging::EVENT_MAX;
use windows::Win32::UI::WindowsAndMessaging::EVENT_MIN;
use windows::Win32::UI::WindowsAndMessaging::GA_ROOT;
use windows::Win32::UI::WindowsAndMessaging::GetAncestor;
use windows::Win32::UI::WindowsAndMessaging::GetClassNameW;
use windows::Win32::UI::WindowsAndMessaging::GetMessageW;
use windows::Win32::UI::WindowsAndMessaging::GetWindowThreadProcessId;
use windows::Win32::UI::WindowsAndMessaging::MSG;
use windows::Win32::UI::WindowsAndMessaging::OBJID_WINDOW;
use windows::Win32::UI::WindowsAndMessaging::PostThreadMessageW;
use windows::Win32::UI::WindowsAndMessaging::TranslateMessage;
use windows::Win32::UI::WindowsAndMessaging::WINEVENT_OUTOFCONTEXT;
use windows::Win32::UI::WindowsAndMessaging::WINEVENT_SKIPOWNPROCESS;
use windows::Win32::UI::WindowsAndMessaging::WM_QUIT;
use windows::core::HSTRING;

/// An event as the hook heard it: the least that can be done on the hook's thread.
struct Heard {
    host_ns: u64,
    wall_us: i64,
    event: u32,
    window: isize,
}

/// The hook has no user data, so what it feeds is here.
static HEARD: OnceLock<Mutex<Option<Sender<Heard>>>> = OnceLock::new();

unsafe extern "system" fn on_win_event(
    _hook: HWINEVENTHOOK,
    event: u32,
    window: HWND,
    id_object: i32,
    id_child: i32,
    _thread: u32,
    _time: u32,
) {
    // only windows themselves, not the objects inside them
    if id_object != OBJID_WINDOW.0 || id_child != CHILDID_SELF as i32 || window.0.is_null() {
        return;
    }

    if let Some(sender) = HEARD.get().and_then(|heard| heard.lock().clone()) {
        let _ = sender.send(Heard {
            host_ns: clock::mono_now_ns(),
            wall_us: clock::wall_now_us(),
            event,
            window: window.0 as isize,
        });
    }
}

/// Everything about a window that is cheap to ask and may already be gone: its class and owner.
fn describe(window: isize) -> Option<(i64, String, String)> {
    let hwnd = HWND(window as *mut c_void);

    // a child window is a part of one; the events worth keeping are the top-level windows'. A
    // window already destroyed has no ancestor to ask about, and a destroy is worth keeping.
    // SAFETY: queries about a handle that may be stale, which they report as a failure
    let root = unsafe { GetAncestor(hwnd, GA_ROOT) };
    if !root.0.is_null() && root != hwnd {
        return None;
    }

    let mut pid = 0_u32;
    // SAFETY: as above, with a valid out pointer
    unsafe { GetWindowThreadProcessId(hwnd, Some(&raw mut pid)) };

    let mut class = [0_u16; 128];
    // SAFETY: the buffer is valid for its length
    let length = unsafe { GetClassNameW(hwnd, &mut class) };
    let class = String::from_utf16_lossy(&class[..usize::try_from(length).unwrap_or(0)]);

    let process = if pid == 0 {
        String::new()
    } else {
        super::apps::process_name(pid).map_or_else(String::new, |name| format!("{name}.exe"))
    };

    Some((i64::from(pid), process, class))
}

/// Turns what the hook heard into records.
fn sink(heard: &Receiver<Heard>, stream: &TraceStream) {
    let mut coalescer = Coalescer::default();
    // a process's name is asked for once per program: pids come and go but the cache is cleared
    // before it can grow without bound
    let mut names: HashMap<i64, String> = HashMap::new();

    while let Ok(event) = heard.recv() {
        let folded = match coalescer.offer(event.host_ns, event.window, event.event) {
            Offer::Fold => continue,
            Offer::Emit { folded } => folded,
        };

        let Some((pid, process, class)) = describe(event.window) else {
            continue;
        };
        if names.len() > 1024 {
            names.clear();
        }
        let process = if process.is_empty() {
            names.get(&pid).cloned().unwrap_or_default()
        } else {
            names.insert(pid, process.clone());
            process
        };

        let name = win_event_name(event.event);
        stream.write(&TraceRecord {
            host_ns: event.host_ns,
            wall_us: event.wall_us,
            pid,
            process,
            subsystem: "winevent".to_string(),
            level: if is_noisy(event.event) {
                "Debug"
            } else {
                "Info"
            },
            message: format!(
                "{name} hwnd=0x{:x} class={class}{}",
                event.window,
                if folded > 0 {
                    format!(" (+{folded} like it folded in)")
                } else {
                    String::new()
                }
            ),
            category: name,
        });
    }
}

/// The hook's thread: install it, then pump messages (it is delivered through them) until told to
/// stop.
fn hook_thread(ready: &std::sync::mpsc::Sender<Result<u32, String>>) {
    // SAFETY: installing a hook that calls `on_win_event`, which does nothing unsafe; the hook is
    // removed below before the thread ends
    let hook = unsafe {
        SetWinEventHook(
            EVENT_MIN,
            EVENT_MAX,
            None,
            Some(on_win_event),
            0,
            0,
            WINEVENT_OUTOFCONTEXT | WINEVENT_SKIPOWNPROCESS,
        )
    };
    if hook.0.is_null() {
        let _ = ready.send(Err("SetWinEventHook failed".to_string()));
        return;
    }

    // SAFETY: no preconditions
    let _ = ready.send(Ok(unsafe { GetCurrentThreadId() }));

    let mut message = MSG::default();
    // SAFETY: the standard message loop; `WM_QUIT` ends it
    unsafe {
        while GetMessageW(&raw mut message, None, 0, 0).as_bool() {
            let _ = TranslateMessage(&raw const message);
            DispatchMessageW(&raw const message);
        }
        let _ = UnhookWinEvent(hook);
    }
}

/// An Event Log subscription, delivered on a thread of the system's.
struct Subscription {
    handle: EVT_HANDLE,
    /// The `Arc<TraceStream>` the callback was given, to be released after it is closed.
    context: *const TraceStream,
}

// SAFETY: the handle is an opaque token and the context an `Arc` raw pointer used only to release it
unsafe impl Send for Subscription {}

/// Render an event to its XML.
fn render(event: EVT_HANDLE) -> Option<String> {
    let (mut used, mut count) = (0_u32, 0_u32);
    // SAFETY: the first call asks how much room is needed; it fails with that, which is the point
    let _ = unsafe {
        EvtRender(
            None,
            event,
            EvtRenderEventXml.0,
            0,
            None,
            &raw mut used,
            &raw mut count,
        )
    };
    if used == 0 {
        return None;
    }

    let mut buffer = vec![0_u16; (used as usize).div_ceil(2)];
    // SAFETY: the buffer is `used` bytes, as asked
    unsafe {
        EvtRender(
            None,
            event,
            EvtRenderEventXml.0,
            used,
            Some(buffer.as_mut_ptr().cast()),
            &raw mut used,
            &raw mut count,
        )
    }
    .ok()?;

    let length = buffer
        .iter()
        .position(|unit| *unit == 0)
        .unwrap_or(buffer.len());
    Some(String::from_utf16_lossy(&buffer[..length]))
}

unsafe extern "system" fn on_event_log(
    action: EVT_SUBSCRIBE_NOTIFY_ACTION,
    context: *const c_void,
    event: EVT_HANDLE,
) -> u32 {
    if action != EvtSubscribeActionDeliver {
        return 0;
    }

    // SAFETY: `context` is the stream pointer `subscribe` gave, which outlives the subscription
    let Some(stream) = (unsafe { context.cast::<TraceStream>().as_ref() }) else {
        return 0;
    };
    let host_ns = clock::mono_now_ns();
    let wall_us = clock::wall_now_us();

    if let Some(summary) = render(event).as_deref().and_then(summarize_event_xml) {
        let pid = summary.pid.unwrap_or(0);
        stream.write(&TraceRecord {
            host_ns,
            wall_us,
            pid,
            process: u32::try_from(pid)
                .ok()
                .and_then(super::apps::process_name)
                .map_or_else(String::new, |name| format!("{name}.exe")),
            message: format!(
                "{} event {}{}{}",
                summary.provider,
                summary.event_id,
                if summary.data.is_empty() { "" } else { ": " },
                summary.data.join("; ")
            ),
            category: summary.event_id,
            subsystem: summary.provider,
            level: summary.level,
        });
    }

    0
}

/// What the System log is asked for: warnings and errors from anyone, and the providers that talk
/// about displays, power and the GPU at any level.
const QUERIES: &[&str] = &[
    "*[System[Level=1 or Level=2 or Level=3]]",
    "*[System[(Provider[@Name='Microsoft-Windows-Kernel-Power' or \
     @Name='Microsoft-Windows-Kernel-PnP' or @Name='Microsoft-Windows-DxgKrnl' or \
     @Name='Display' or @Name='Microsoft-Windows-Dwm-Core']) and (Level=0 or Level=4 or Level=5)]]",
];

fn subscribe(stream: &Arc<TraceStream>, query: &str) -> Result<Subscription, String> {
    let context = Arc::into_raw(stream.clone());
    // SAFETY: the callback only reads `context`, which is released after the subscription closes
    let handle = unsafe {
        EvtSubscribe(
            None,
            None,
            &HSTRING::from("System"),
            &HSTRING::from(query),
            None,
            Some(context.cast()),
            Some(on_event_log),
            EvtSubscribeToFutureEvents.0,
        )
    };

    match handle {
        Ok(handle) => Ok(Subscription { handle, context }),
        Err(error) => {
            // SAFETY: not subscribed, so the pointer was never given away
            drop(unsafe { Arc::from_raw(context) });
            Err(format!("{error}"))
        }
    }
}

/// The running taps, listening until dropped.
///
/// Dropping them stops hearing first (the Event Log subscriptions are closed, then the hook is
/// removed), then lets what was already heard be written, so nothing arrives after the drop
/// returns.
pub struct Taps {
    hook_thread: Option<(u32, JoinHandle<()>)>,
    sink: Option<JoinHandle<()>>,
    subscriptions: Vec<Subscription>,
}

impl Drop for Taps {
    fn drop(&mut self) {
        // stop hearing first, then let what was heard be written
        for subscription in self.subscriptions.drain(..) {
            // SAFETY: the subscription was made by `subscribe`; once closed no callback is running
            // or will run, so the context can be released
            unsafe {
                let _ = EvtClose(subscription.handle);
                drop(Arc::from_raw(subscription.context));
            }
        }

        if let Some((thread_id, thread)) = self.hook_thread.take() {
            // SAFETY: posting a quit to the hook thread's queue
            let _ = unsafe { PostThreadMessageW(thread_id, WM_QUIT, WPARAM(0), LPARAM(0)) };
            let _ = thread.join();
        }
        // the hook is gone, so no more is sent; closing the channel ends the sink
        if let Some(heard) = HEARD.get() {
            *heard.lock() = None;
        }
        if let Some(sink) = self.sink.take() {
            let _ = sink.join();
        }
    }
}

/// Starts both taps, writing `WinEvents` into `system` and Event Log records into `event_log`.
///
/// Returns the running [`Taps`], always `Some` on Windows, and the problems met: one entry in words
/// for each tap that could not start, beginning `winevents:` for the hook (or its thread) and
/// `eventlog:` for a subscription that could not be made. One that cannot start is named in the
/// problems and the other carries on. Two Event Log queries are made (warnings and errors from
/// any provider, and the display, power and GPU providers at the other levels), so `eventlog:` can
/// appear twice. On macOS this starts nothing and returns `None`.
#[must_use]
pub fn start(system: Arc<TraceStream>, event_log: Arc<TraceStream>) -> (Option<Taps>, Vec<String>) {
    let mut problems = Vec::new();

    let (heard_tx, heard_rx) = crossbeam_channel::unbounded();
    *HEARD.get_or_init(|| Mutex::new(None)).lock() = Some(heard_tx);

    let sink_stream = system.clone();
    let sink_thread = std::thread::Builder::new()
        .name("komorebi-recorder-trace-sink".to_string())
        .spawn(move || sink(&heard_rx, &sink_stream))
        .ok();

    let (ready_tx, ready_rx) = std::sync::mpsc::channel();
    let hook = std::thread::Builder::new()
        .name("komorebi-recorder-winevents".to_string())
        .spawn(move || hook_thread(&ready_tx))
        .ok()
        .and_then(|thread| match ready_rx.recv() {
            Ok(Ok(thread_id)) => Some((thread_id, thread)),
            Ok(Err(reason)) => {
                problems.push(format!("winevents: {reason}"));
                let _ = thread.join();
                None
            }
            Err(_) => {
                problems.push("winevents: the hook thread ended before it was ready".to_string());
                None
            }
        });

    let mut subscriptions = Vec::new();
    for query in QUERIES {
        match subscribe(&event_log, query) {
            Ok(subscription) => subscriptions.push(subscription),
            Err(reason) => problems.push(format!("eventlog: could not subscribe ({reason})")),
        }
    }

    let taps = Taps {
        hook_thread: hook,
        sink: sink_thread,
        subscriptions,
    };
    (Some(taps), problems)
}
