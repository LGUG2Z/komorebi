//! A window for the e2e scenarios to tile.
//!
//! It has a stable exe name and window class, so that rules can match it, and
//! a control channel on stdin so that a scenario can change it while it runs.
//! Every command is one line; every answer is one line of JSON on stdout.
//!
//! ```text
//! title TEXT        set the title
//! color RRGGBB      fill the client area with a colour
//! resize W H        resize the outer frame
//! report            print the title and the visible frame
//! close             close the window, as the close button would
//! exit              leave without closing the window first
//! ```
//!
//! Options: `--title T`, `--class C` (the window class, `KomorebiTestWin` by
//! default), `--color RRGGBB`, `--size WxH`, `--min-size WxH`, `--fixed` (not
//! resizable) and `--rename-to T` with `--after SECONDS` (the window gives
//! itself that title after that long, as a program that sets it late does).
//!
//! The message `WM_APP + 2` takes the window fullscreen (`wparam` 1: a
//! borderless window over its monitor, as a program's own key does) or brings
//! it back (`wparam` 0).

#![cfg(windows)]

use std::collections::VecDeque;
use std::io::BufRead;
use std::io::Write;
use std::mem::size_of;
use std::sync::Mutex;
use std::sync::atomic::AtomicIsize;
use std::sync::atomic::AtomicU32;
use std::sync::atomic::Ordering;

use windows::Win32::Foundation::COLORREF;
use windows::Win32::Foundation::HWND;
use windows::Win32::Foundation::LPARAM;
use windows::Win32::Foundation::LRESULT;
use windows::Win32::Foundation::RECT;
use windows::Win32::Foundation::WPARAM;
use windows::Win32::Graphics::Dwm::DWMWA_EXTENDED_FRAME_BOUNDS;
use windows::Win32::Graphics::Dwm::DwmGetWindowAttribute;
use windows::Win32::Graphics::Gdi::BeginPaint;
use windows::Win32::Graphics::Gdi::CreateSolidBrush;
use windows::Win32::Graphics::Gdi::DeleteObject;
use windows::Win32::Graphics::Gdi::EndPaint;
use windows::Win32::Graphics::Gdi::FillRect;
use windows::Win32::Graphics::Gdi::GetMonitorInfoW;
use windows::Win32::Graphics::Gdi::HGDIOBJ;
use windows::Win32::Graphics::Gdi::InvalidateRect;
use windows::Win32::Graphics::Gdi::MONITOR_DEFAULTTONEAREST;
use windows::Win32::Graphics::Gdi::MONITORINFO;
use windows::Win32::Graphics::Gdi::MonitorFromWindow;
use windows::Win32::Graphics::Gdi::PAINTSTRUCT;
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::WindowsAndMessaging::CW_USEDEFAULT;
use windows::Win32::UI::WindowsAndMessaging::CreateWindowExW;
use windows::Win32::UI::WindowsAndMessaging::DefWindowProcW;
use windows::Win32::UI::WindowsAndMessaging::DestroyWindow;
use windows::Win32::UI::WindowsAndMessaging::DispatchMessageW;
use windows::Win32::UI::WindowsAndMessaging::GWL_STYLE;
use windows::Win32::UI::WindowsAndMessaging::GetWindowLongPtrW;
use windows::Win32::UI::WindowsAndMessaging::GetWindowRect;
use windows::Win32::UI::WindowsAndMessaging::HWND_TOP;
use windows::Win32::UI::WindowsAndMessaging::SWP_FRAMECHANGED;
use windows::Win32::UI::WindowsAndMessaging::SWP_NOOWNERZORDER;
use windows::Win32::UI::WindowsAndMessaging::SetWindowLongPtrW;
use windows::Win32::UI::WindowsAndMessaging::WS_POPUP;
use windows::Win32::UI::WindowsAndMessaging::WS_VISIBLE;
use windows::Win32::UI::WindowsAndMessaging::GetMessageW;
use windows::Win32::UI::WindowsAndMessaging::HMENU;
use windows::Win32::UI::WindowsAndMessaging::IDC_ARROW;
use windows::Win32::UI::WindowsAndMessaging::LoadCursorW;
use windows::Win32::UI::WindowsAndMessaging::MINMAXINFO;
use windows::Win32::UI::WindowsAndMessaging::MSG;
use windows::Win32::UI::WindowsAndMessaging::PostMessageW;
use windows::Win32::UI::WindowsAndMessaging::PostQuitMessage;
use windows::Win32::UI::WindowsAndMessaging::RegisterClassW;
use windows::Win32::UI::WindowsAndMessaging::SW_SHOW;
use windows::Win32::UI::WindowsAndMessaging::SWP_NOMOVE;
use windows::Win32::UI::WindowsAndMessaging::SWP_NOZORDER;
use windows::Win32::UI::WindowsAndMessaging::SetWindowPos;
use windows::Win32::UI::WindowsAndMessaging::SetWindowTextW;
use windows::Win32::UI::WindowsAndMessaging::ShowWindow;
use windows::Win32::UI::WindowsAndMessaging::TranslateMessage;
use windows::Win32::UI::WindowsAndMessaging::WINDOW_EX_STYLE;
use windows::Win32::UI::WindowsAndMessaging::WM_APP;
use windows::Win32::UI::WindowsAndMessaging::WM_CLOSE;
use windows::Win32::UI::WindowsAndMessaging::WM_DESTROY;
use windows::Win32::UI::WindowsAndMessaging::WM_ERASEBKGND;
use windows::Win32::UI::WindowsAndMessaging::WM_GETMINMAXINFO;
use windows::Win32::UI::WindowsAndMessaging::WM_PAINT;
use windows::Win32::UI::WindowsAndMessaging::WNDCLASSW;
use windows::Win32::UI::WindowsAndMessaging::WS_MAXIMIZEBOX;
use windows::Win32::UI::WindowsAndMessaging::WS_OVERLAPPEDWINDOW;
use windows::Win32::UI::WindowsAndMessaging::WS_THICKFRAME;
use windows::core::PCWSTR;

/// Posted to the window when a command is waiting in [`COMMANDS`].
const WM_COMMAND_READY: u32 = WM_APP + 1;

/// Posted to the window to take it fullscreen (`wparam` 1) or back (0).
const WM_FULLSCREEN: u32 = WM_APP + 2;

/// The style and the rectangle of the window before it went fullscreen.
static BEFORE_FULLSCREEN: Mutex<Option<(isize, RECT)>> = Mutex::new(None);

static COMMANDS: Mutex<VecDeque<String>> = Mutex::new(VecDeque::new());
static HWND_RAW: AtomicIsize = AtomicIsize::new(0);
static COLOR: AtomicU32 = AtomicU32::new(0x0070_7070);
static MIN_SIZE: Mutex<Option<(i32, i32)>> = Mutex::new(None);
static FIXED: Mutex<Option<(i32, i32)>> = Mutex::new(None);

fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(Some(0)).collect()
}

/// `RRGGBB` as a GDI colour, which is `0x00BBGGRR`.
fn parse_color(s: &str) -> Option<u32> {
    let rgb = u32::from_str_radix(s.trim_start_matches('#'), 16).ok()?;
    let (r, g, b) = ((rgb >> 16) & 0xff, (rgb >> 8) & 0xff, rgb & 0xff);
    Some(r | (g << 8) | (b << 16))
}

fn parse_size(s: &str) -> Option<(i32, i32)> {
    let (w, h) = s.split_once('x')?;
    Some((w.parse().ok()?, h.parse().ok()?))
}

fn say(value: serde_json::Value) {
    let mut out = std::io::stdout().lock();
    let _ = writeln!(out, "{value}");
    let _ = out.flush();
}

fn frame(hwnd: HWND) -> Option<RECT> {
    let mut rect = RECT::default();
    // SAFETY: `rect` is a RECT and the size passed is the size of a RECT.
    unsafe {
        DwmGetWindowAttribute(
            hwnd,
            DWMWA_EXTENDED_FRAME_BOUNDS,
            (&mut rect as *mut RECT).cast(),
            size_of::<RECT>() as u32,
        )
    }
    .ok()?;
    Some(rect)
}

fn title_of(hwnd: HWND) -> String {
    use windows::Win32::UI::WindowsAndMessaging::GetWindowTextW;
    let mut buf = [0u16; 512];
    // SAFETY: the buffer is valid for the length it is given.
    let n = unsafe { GetWindowTextW(hwnd, &mut buf) };
    String::from_utf16_lossy(&buf[..n.max(0) as usize])
}

fn run_command(hwnd: HWND, line: &str) -> bool {
    let mut words = line.split_whitespace();
    match words.next() {
        Some("title") => {
            let title = line.trim_start().strip_prefix("title").unwrap().trim();
            let title = wide(title);
            // SAFETY: `title` is NUL-terminated and outlives the call.
            let _ = unsafe { SetWindowTextW(hwnd, PCWSTR(title.as_ptr())) };
            say(serde_json::json!({"ok": true}));
        }
        Some("color") => match words.next().and_then(parse_color) {
            Some(c) => {
                COLOR.store(c, Ordering::SeqCst);
                // SAFETY: `hwnd` is this process's own window.
                let _ = unsafe { InvalidateRect(Some(hwnd), None, true) };
                say(serde_json::json!({"ok": true}));
            }
            None => say(serde_json::json!({"error": "color wants RRGGBB"})),
        },
        Some("resize") => {
            let w = words.next().and_then(|s| s.parse::<i32>().ok());
            let h = words.next().and_then(|s| s.parse::<i32>().ok());
            if let (Some(w), Some(h)) = (w, h) {
                // SAFETY: `hwnd` is this process's own window.
                let _ = unsafe { SetWindowPos(hwnd, None, 0, 0, w, h, SWP_NOMOVE | SWP_NOZORDER) };
                say(serde_json::json!({"ok": true}));
            } else {
                say(serde_json::json!({"error": "resize wants W H"}));
            }
        }
        Some("report") => match frame(hwnd) {
            Some(r) => say(serde_json::json!({
                "hwnd": hwnd.0 as isize,
                "title": title_of(hwnd),
                "frame": {
                    "left": r.left, "top": r.top,
                    "right": r.right - r.left, "bottom": r.bottom - r.top,
                },
            })),
            None => say(serde_json::json!({"error": "no frame"})),
        },
        Some("close") => {
            // SAFETY: `hwnd` is this process's own window.
            let _ = unsafe { PostMessageW(Some(hwnd), WM_CLOSE, WPARAM(0), LPARAM(0)) };
        }
        Some("exit") => return false,
        Some(other) => say(serde_json::json!({"error": format!("unknown {other}")})),
        None => {}
    }
    true
}

fn set_fullscreen(hwnd: HWND, on: bool) {
    let mut before = BEFORE_FULLSCREEN.lock().unwrap();
    // SAFETY: plain Win32 calls on this process's own window, on the thread
    // that owns it.
    unsafe {
        if on && before.is_none() {
            let style = GetWindowLongPtrW(hwnd, GWL_STYLE);
            let mut rect = RECT::default();
            let _ = GetWindowRect(hwnd, &mut rect);
            let monitor = MonitorFromWindow(hwnd, MONITOR_DEFAULTTONEAREST);
            let mut info = MONITORINFO {
                cbSize: size_of::<MONITORINFO>() as u32,
                ..Default::default()
            };
            if GetMonitorInfoW(monitor, &mut info).as_bool() {
                *before = Some((style, rect));
                let area = info.rcMonitor;
                SetWindowLongPtrW(hwnd, GWL_STYLE, (WS_POPUP | WS_VISIBLE).0 as isize);
                let _ = SetWindowPos(
                    hwnd,
                    Some(HWND_TOP),
                    area.left,
                    area.top,
                    area.right - area.left,
                    area.bottom - area.top,
                    SWP_FRAMECHANGED | SWP_NOOWNERZORDER,
                );
            }
        } else if !on && let Some((style, rect)) = before.take() {
            SetWindowLongPtrW(hwnd, GWL_STYLE, style);
            let _ = SetWindowPos(
                hwnd,
                None,
                rect.left,
                rect.top,
                rect.right - rect.left,
                rect.bottom - rect.top,
                SWP_FRAMECHANGED | SWP_NOOWNERZORDER | SWP_NOZORDER,
            );
        }
    }
}

extern "system" fn wndproc(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    match msg {
        WM_COMMAND_READY => {
            loop {
                let Some(line) = COMMANDS.lock().unwrap().pop_front() else {
                    break;
                };
                if !run_command(hwnd, &line) {
                    // SAFETY: plain Win32 call on the thread that owns the loop.
                    unsafe { PostQuitMessage(0) };
                    break;
                }
            }
            LRESULT(0)
        }
        WM_FULLSCREEN => {
            set_fullscreen(hwnd, wparam.0 != 0);
            LRESULT(0)
        }
        WM_ERASEBKGND => LRESULT(1),
        WM_PAINT => {
            let mut ps = PAINTSTRUCT::default();
            // SAFETY: the usual paint sequence, on the window's own thread.
            unsafe {
                let hdc = BeginPaint(hwnd, &mut ps);
                let brush = CreateSolidBrush(COLORREF(COLOR.load(Ordering::SeqCst)));
                FillRect(hdc, &ps.rcPaint, brush);
                let _ = DeleteObject(HGDIOBJ(brush.0));
                let _ = EndPaint(hwnd, &ps);
            }
            LRESULT(0)
        }
        WM_GETMINMAXINFO => {
            // SAFETY: for this message `lparam` points at a MINMAXINFO.
            let info = unsafe { &mut *(lparam.0 as *mut MINMAXINFO) };
            if let Some((w, h)) = *MIN_SIZE.lock().unwrap() {
                info.ptMinTrackSize.x = w;
                info.ptMinTrackSize.y = h;
            }
            if let Some((w, h)) = *FIXED.lock().unwrap() {
                info.ptMinTrackSize.x = w;
                info.ptMinTrackSize.y = h;
                info.ptMaxTrackSize.x = w;
                info.ptMaxTrackSize.y = h;
            }
            LRESULT(0)
        }
        WM_CLOSE => {
            // SAFETY: `hwnd` is this process's own window.
            let _ = unsafe { DestroyWindow(hwnd) };
            LRESULT(0)
        }
        WM_DESTROY => {
            // SAFETY: plain Win32 call on the thread that owns the loop.
            unsafe { PostQuitMessage(0) };
            LRESULT(0)
        }
        // SAFETY: forwards the arguments the system gave us, unchanged.
        _ => unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) },
    }
}

fn main() -> windows::core::Result<()> {
    let mut title = String::from("komorebi-testwin");
    let mut size = (400, 300);
    let mut fixed = false;
    let mut class_name = wide("KomorebiTestWin");
    let mut rename: Option<(String, f64)> = None;
    let mut rename_to = None;
    let mut after = 0.0;

    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--title" => title = args.next().unwrap_or(title),
            "--class" => {
                if let Some(c) = args.next() {
                    class_name = wide(&c);
                }
            }
            "--color" => {
                if let Some(c) = args.next().as_deref().and_then(parse_color) {
                    COLOR.store(c, Ordering::SeqCst);
                }
            }
            "--size" => size = args.next().as_deref().and_then(parse_size).unwrap_or(size),
            "--min-size" => {
                *MIN_SIZE.lock().unwrap() = args.next().as_deref().and_then(parse_size);
            }
            "--fixed" => fixed = true,
            "--rename-to" => rename_to = args.next(),
            "--after" => after = args.next().and_then(|s| s.parse().ok()).unwrap_or(0.0),
            other => eprintln!("komorebi-testwin: ignoring {other}"),
        }
    }
    if fixed {
        *FIXED.lock().unwrap() = Some(size);
    }
    if let Some(new) = rename_to {
        rename = Some((new, after));
    }

    // SAFETY: plain Win32 setup on the main thread, which then runs the loop.
    unsafe {
        let instance = GetModuleHandleW(None)?;
        let class = WNDCLASSW {
            lpfnWndProc: Some(wndproc),
            hInstance: instance.into(),
            lpszClassName: PCWSTR(class_name.as_ptr()),
            hCursor: LoadCursorW(None, IDC_ARROW)?,
            ..Default::default()
        };
        RegisterClassW(&class);

        let mut style = WS_OVERLAPPEDWINDOW;
        if fixed {
            style &= !(WS_THICKFRAME | WS_MAXIMIZEBOX);
        }
        let title = wide(&title);
        let hwnd = CreateWindowExW(
            WINDOW_EX_STYLE(0),
            PCWSTR(class_name.as_ptr()),
            PCWSTR(title.as_ptr()),
            style,
            CW_USEDEFAULT,
            CW_USEDEFAULT,
            size.0,
            size.1,
            None::<HWND>,
            None::<HMENU>,
            Some(instance.into()),
            None,
        )?;
        HWND_RAW.store(hwnd.0 as isize, Ordering::SeqCst);
        let _ = ShowWindow(hwnd, SW_SHOW);

        if let Some((new, after)) = rename {
            std::thread::spawn(move || {
                std::thread::sleep(std::time::Duration::from_secs_f64(after));
                let hwnd = HWND(HWND_RAW.load(Ordering::SeqCst) as *mut _);
                let title = wide(&new);
                // SAFETY: `title` is NUL-terminated and outlives the call.
                let _ = unsafe { SetWindowTextW(hwnd, PCWSTR(title.as_ptr())) };
            });
        }

        std::thread::spawn(move || {
            let hwnd = HWND(HWND_RAW.load(Ordering::SeqCst) as *mut _);
            for line in std::io::stdin().lock().lines() {
                let Ok(line) = line else { break };
                COMMANDS.lock().unwrap().push_back(line);
                let _ = PostMessageW(Some(hwnd), WM_COMMAND_READY, WPARAM(0), LPARAM(0));
            }
            // stdin closed: the scenario is gone, so go too
            COMMANDS.lock().unwrap().push_back(String::from("exit"));
            let _ = PostMessageW(Some(hwnd), WM_COMMAND_READY, WPARAM(0), LPARAM(0));
        });

        let mut msg = MSG::default();
        while GetMessageW(&mut msg, None, 0, 0).as_bool() {
            let _ = TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }
    }
    Ok(())
}
