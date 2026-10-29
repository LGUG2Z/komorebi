//! The guest half of the e2e suite on Windows.
//!
//! It must run inside the interactive session (a Run entry of the autologon
//! user), because session 0 has no usable desktop. The host talks to it over
//! a TCP port that QEMU forwards from the host's loopback. One request is one
//! line of JSON, `{"id": 1, "verb": "...", ...}`, answered by one line,
//! `{"id": 1, "ok": ...}` or `{"id": 1, "error": "..."}`.
//!
//! `put_file` and `get_file` move bytes after the JSON line, as the `size`
//! raw bytes that the request or the answer announces, so that a daemon of
//! tens of megabytes is not wrapped in JSON.
//!
//! The agent injects no input: keys, pointer and screenshots come from the
//! host (QMP), which also works at the lock screen.

#![cfg(windows)]

use std::io::BufRead;
use std::io::BufReader;
use std::io::Read;
use std::io::Write;
use std::net::TcpListener;
use std::net::TcpStream;
use std::process::Command;

use serde::Deserialize;
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

const DEFAULT_PORT: u16 = 7878;

#[derive(Deserialize)]
struct Request {
    id: Value,
    verb: String,
    #[serde(default)]
    args: Vec<String>,
    #[serde(default)]
    path: String,
    #[serde(default)]
    size: u64,
    #[serde(default)]
    hwnd: isize,
}

fn rect_json(r: &RECT) -> Value {
    // the shape komorebi itself prints: right and bottom are width and height
    json!({
        "left": r.left, "top": r.top,
        "right": r.right - r.left, "bottom": r.bottom - r.top,
    })
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

fn answer(stream: &mut TcpStream, id: &Value, result: Result<Value, String>) {
    let line = match result {
        Ok(ok) => json!({"id": id, "ok": ok}),
        Err(error) => json!({"id": id, "error": error}),
    };
    let _ = writeln!(stream, "{line}");
}

fn exec(args: &[String]) -> Result<Value, String> {
    let (program, rest) = args.split_first().ok_or("exec wants a program")?;
    let out = Command::new(program)
        .args(rest)
        .output()
        .map_err(|e| format!("{program}: {e}"))?;
    Ok(json!({
        "status": out.status.code(),
        "stdout": String::from_utf8_lossy(&out.stdout),
        "stderr": String::from_utf8_lossy(&out.stderr),
    }))
}

/// Starts a program that keeps running, in this session, and returns at once.
fn spawn(args: &[String]) -> Result<Value, String> {
    let (program, rest) = args.split_first().ok_or("spawn wants a program")?;
    let child = Command::new(program)
        .args(rest)
        .spawn()
        .map_err(|e| format!("{program}: {e}"))?;
    Ok(json!({"pid": child.id()}))
}

fn serve(stream: TcpStream) -> std::io::Result<()> {
    let mut writer = stream.try_clone()?;
    let mut reader = BufReader::new(stream);
    let mut line = String::new();
    loop {
        line.clear();
        if reader.read_line(&mut line)? == 0 {
            return Ok(());
        }
        let req: Request = match serde_json::from_str(&line) {
            Ok(req) => req,
            Err(e) => {
                answer(&mut writer, &Value::Null, Err(format!("bad request: {e}")));
                continue;
            }
        };
        let hwnd = hwnd_of(req.hwnd);
        let result = match req.verb.as_str() {
            "ping" => Ok(json!("pong")),
            "exec" => exec(&req.args),
            "spawn" => spawn(&req.args),
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
            "put_file" => {
                let mut bytes = vec![0u8; req.size as usize];
                reader.read_exact(&mut bytes)?;
                std::fs::write(&req.path, bytes)
                    .map(|()| json!(req.size))
                    .map_err(|e| format!("{}: {e}", req.path))
            }
            "get_file" => match std::fs::read(&req.path) {
                Ok(bytes) => {
                    answer(&mut writer, &req.id, Ok(json!({"size": bytes.len()})));
                    writer.write_all(&bytes)?;
                    continue;
                }
                Err(e) => Err(format!("{}: {e}", req.path)),
            },
            other => Err(format!("unknown verb {other}")),
        };
        answer(&mut writer, &req.id, result);
    }
}

fn main() -> std::io::Result<()> {
    let port = std::env::var("KOMOREBI_E2E_AGENT_PORT")
        .ok()
        .and_then(|p| p.parse().ok())
        .unwrap_or(DEFAULT_PORT);
    // QEMU's user-mode network forwards the host port to the guest's own
    // address, so loopback would not see it. The guest is on a private
    // network of its own and has its firewall off (see `setup.ps1`).
    let listener = TcpListener::bind(("0.0.0.0", port))?;
    for stream in listener.incoming() {
        let stream = stream?;
        std::thread::spawn(move || {
            let _ = serve(stream);
        });
    }
    Ok(())
}
