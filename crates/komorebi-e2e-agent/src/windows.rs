//! The verbs of the Windows agent: the windows and displays of the interactive session.
//!
//! It must run inside the interactive session (a Run entry of the autologon user), because
//! session 0 has no usable desktop. The agent injects no input: keys, pointer and screenshots
//! come from the host (QMP), which also works at the lock screen.

use std::net::TcpListener;
use std::os::windows::process::CommandExt;
use std::process::Command;

use serde_json::Value;
use serde_json::json;
use windows::Win32::Foundation::HWND;
use windows::Win32::Foundation::LPARAM;
use windows::Win32::Foundation::RECT;
use windows::Win32::Foundation::WPARAM;
use windows::Win32::Graphics::Dwm::DWMWA_CLOAKED;
use windows::Win32::Graphics::Dwm::DWMWA_EXTENDED_FRAME_BOUNDS;
use windows::Win32::Graphics::Dwm::DwmGetWindowAttribute;
use windows::Win32::Graphics::Gdi::EnumDisplayMonitors;
use windows::Win32::Graphics::Gdi::GetMonitorInfoW;
use windows::Win32::Graphics::Gdi::HDC;
use windows::Win32::Graphics::Gdi::HMONITOR;
use windows::Win32::Graphics::Gdi::MONITORINFO;
use windows::Win32::UI::WindowsAndMessaging::EnumWindows;
use windows::Win32::UI::WindowsAndMessaging::GetClassNameW;
use windows::Win32::UI::WindowsAndMessaging::GetForegroundWindow;
use windows::Win32::UI::WindowsAndMessaging::GetWindowTextW;
use windows::Win32::UI::WindowsAndMessaging::IsIconic;
use windows::Win32::UI::WindowsAndMessaging::IsWindowVisible;
use windows::Win32::UI::WindowsAndMessaging::PostMessageW;
use windows::Win32::UI::WindowsAndMessaging::SW_MINIMIZE;
use windows::Win32::UI::WindowsAndMessaging::SW_RESTORE;
use windows::Win32::UI::WindowsAndMessaging::SetForegroundWindow;
use windows::Win32::UI::WindowsAndMessaging::ShowWindow;
use windows::Win32::UI::WindowsAndMessaging::WM_CLOSE;
use windows::core::BOOL;

use crate::protocol::Mode;
use crate::protocol::Request;
use crate::protocol::Verbs;
use crate::protocol::frame_json;

/// A windowless program has nowhere to print a panic, so it goes here.
const LOG: &str = "C:\\e2e\\agent.log";

fn log(message: &str) {
    use std::io::Write as _;
    if let Ok(mut file) = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(LOG)
    {
        let _ = writeln!(file, "{message}");
    }
}

/// `CREATE_NO_WINDOW`: a child must not open a console of its own either.
const NO_WINDOW: u32 = 0x0800_0000;

fn rect_json(r: &RECT) -> Value {
    frame_json(
        r.left.into(),
        r.top.into(),
        (r.right - r.left).into(),
        (r.bottom - r.top).into(),
    )
}

fn hwnd_of(raw: isize) -> HWND {
    HWND(raw as *mut _)
}

/// One top-level window as the operating system describes it.
fn describe(hwnd: HWND) -> Option<Value> {
    let mut title = [0u16; 512];
    // SAFETY: the buffer is valid for the length it is given.
    let n = unsafe { GetWindowTextW(hwnd, &mut title) };
    if n <= 0 {
        return None;
    }
    let mut class = [0u16; 256];
    // SAFETY: as above.
    let c = unsafe { GetClassNameW(hwnd, &mut class) };

    let mut frame = RECT::default();
    // SAFETY: `frame` is a RECT and its size is passed.
    let _ = unsafe {
        DwmGetWindowAttribute(
            hwnd,
            DWMWA_EXTENDED_FRAME_BOUNDS,
            (&mut frame as *mut RECT).cast(),
            std::mem::size_of::<RECT>() as u32,
        )
    };
    let mut cloaked = 0u32;
    // SAFETY: `cloaked` is a u32 and its size is passed.
    let _ = unsafe {
        DwmGetWindowAttribute(
            hwnd,
            DWMWA_CLOAKED,
            (&mut cloaked as *mut u32).cast(),
            std::mem::size_of::<u32>() as u32,
        )
    };

    // SAFETY: plain queries on a window handle.
    let (visible, minimized, focused) = unsafe {
        (
            IsWindowVisible(hwnd).as_bool(),
            IsIconic(hwnd).as_bool(),
            GetForegroundWindow() == hwnd,
        )
    };
    Some(json!({
        "hwnd": hwnd.0 as isize,
        "title": String::from_utf16_lossy(&title[..n as usize]),
        "class": String::from_utf16_lossy(&class[..c.max(0) as usize]),
        "frame": rect_json(&frame),
        "visible": visible,
        "minimized": minimized,
        "cloaked": cloaked != 0,
        "focused": focused,
    }))
}

fn os_windows() -> Vec<Value> {
    unsafe extern "system" fn each(hwnd: HWND, out: LPARAM) -> BOOL {
        // SAFETY: `out` is the `Vec` passed below, alive for the whole call.
        let out = unsafe { &mut *(out.0 as *mut Vec<Value>) };
        out.extend(describe(hwnd));
        BOOL(1)
    }
    let mut out = Vec::new();
    // SAFETY: the callback only touches the `Vec` passed as the parameter.
    let _ = unsafe { EnumWindows(Some(each), LPARAM(&mut out as *mut _ as isize)) };
    out
}

fn displays() -> Vec<Value> {
    unsafe extern "system" fn each(monitor: HMONITOR, _: HDC, _: *mut RECT, out: LPARAM) -> BOOL {
        // SAFETY: `out` is the `Vec` passed below, alive for the whole call.
        let out = unsafe { &mut *(out.0 as *mut Vec<Value>) };
        let mut info = MONITORINFO {
            cbSize: std::mem::size_of::<MONITORINFO>() as u32,
            ..Default::default()
        };
        // SAFETY: `info` has its size set, as the call requires.
        if unsafe { GetMonitorInfoW(monitor, &mut info) }.as_bool() {
            out.push(json!({
                "monitor": rect_json(&info.rcMonitor),
                "work_area": rect_json(&info.rcWork),
                "primary": info.dwFlags & 1 != 0,
            }));
        }
        BOOL(1)
    }
    let mut out = Vec::new();
    // SAFETY: the callback only touches the `Vec` passed as the parameter.
    let _ =
        unsafe { EnumDisplayMonitors(None, None, Some(each), LPARAM(&mut out as *mut _ as isize)) };
    out
}

struct WindowsVerbs;

impl Verbs for WindowsVerbs {
    fn prepare(&self, command: &mut Command, _mode: Mode) {
        command.creation_flags(NO_WINDOW);
    }

    fn handle(&mut self, request: &Request) -> Option<Result<Value, String>> {
        let hwnd = hwnd_of(request.hwnd);
        Some(match request.verb.as_str() {
            "os_windows" => Ok(Value::Array(os_windows())),
            "displays" => Ok(Value::Array(displays())),
            "activate" => {
                // SAFETY: plain calls on a window handle given by the host.
                unsafe {
                    let _ = ShowWindow(hwnd, SW_RESTORE);
                    Ok(json!(SetForegroundWindow(hwnd).as_bool()))
                }
            }
            "minimize" => {
                // SAFETY: as above.
                Ok(json!(unsafe { ShowWindow(hwnd, SW_MINIMIZE) }.as_bool()))
            }
            "close" => {
                // SAFETY: as above.
                unsafe { PostMessageW(Some(hwnd), WM_CLOSE, WPARAM(0), LPARAM(0)) }
                    .map(|()| json!(true))
                    .map_err(|e| e.to_string())
            }
            _ => return None,
        })
    }
}

pub fn run() -> std::io::Result<()> {
    std::panic::set_hook(Box::new(|info| log(&format!("panic: {info}"))));
    // QEMU's user-mode network forwards the host port to the guest's own
    // address, so loopback would not see it. The guest is on a private
    // network of its own and has its firewall off (see `setup.ps1`).
    let listener = TcpListener::bind(("0.0.0.0", crate::protocol::port()))?;
    for stream in listener.incoming() {
        let stream = stream?;
        std::thread::spawn(move || {
            if let Err(e) = crate::protocol::serve(stream, &mut WindowsVerbs) {
                log(&format!("connection ended: {e}"));
            }
        });
    }
    Ok(())
}
