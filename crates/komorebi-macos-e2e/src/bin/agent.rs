//! The guest half of the e2e suite on macOS.
//!
//! It has to run inside the logged-in GUI session, and be started by a program that holds the
//! Accessibility and Screen Recording grants (the golden image starts it from a Terminal window),
//! because the daemon it starts, and the windows it reads, are attributed to that program. The
//! host talks to it over TCP, on the guest's own address. The protocol is the one of the Windows
//! agent (`komorebi-e2e-agent`): one request is one line of JSON, `{"id": 1, "verb": "...", ...}`,
//! answered by one line, `{"id": 1, "ok": ...}` or `{"id": 1, "error": "..."}`, and `put_file` and
//! `get_file` move `size` raw bytes after the line.
//!
//! The agent serves one connection at a time on the main thread, because AppKit (`NSScreen`) is
//! for the main thread. It injects no input.
//!
//! The windows it describes are the ones the accessibility API gives, as the daemon sees them;
//! their frames are in points, with the origin at the top left of the main display, as the
//! daemon's are. A frame is `{left, top, right, bottom}` where `right` and `bottom` are the width
//! and the height, the shape komorebi itself prints.

#![cfg(target_os = "macos")]

use std::collections::HashMap;
use std::io::BufRead;
use std::io::BufReader;
use std::io::Read;
use std::io::Write;
use std::net::TcpListener;
use std::net::TcpStream;
use std::process::Child;
use std::process::Command;
use std::process::Stdio;
use std::ptr::NonNull;

use objc2::MainThreadMarker;
use objc2_app_kit::NSApplication;
use objc2_app_kit::NSApplicationActivationOptions;
use objc2_app_kit::NSApplicationActivationPolicy;
use objc2_app_kit::NSScreen;
use objc2_app_kit::NSWorkspace;
use objc2_application_services::AXError;
use objc2_application_services::AXUIElement;
use objc2_application_services::AXValue;
use objc2_application_services::AXValueType;
use objc2_core_foundation::CFArray;
use objc2_core_foundation::CFBoolean;
use objc2_core_foundation::CFRetained;
use objc2_core_foundation::CFString;
use objc2_core_foundation::CGPoint;
use objc2_core_foundation::CGRect;
use objc2_core_foundation::Type;
use serde::Deserialize;
use serde_json::Value;
use serde_json::json;

const DEFAULT_PORT: u16 = 7879;

/// How long a call to an application that does not answer holds the agent.
const AX_TIMEOUT: f32 = 2.0;

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
    title: String,
    #[serde(default)]
    pid: u32,
    #[serde(default)]
    x: f64,
    #[serde(default)]
    y: f64,
    #[serde(default)]
    env: HashMap<String, String>,
    /// a file that gets what a spawned program prints, standard output and error
    #[serde(default)]
    out: Option<String>,
}

fn copy_attribute<T: Type>(element: &AXUIElement, attribute: &str) -> Option<CFRetained<T>> {
    let mut receiver = std::ptr::null();
    // SAFETY: the receiver is a pointer the call fills, as the daemon's own calls do.
    unsafe {
        let error = element
            .copy_attribute_value(&CFString::from_str(attribute), NonNull::from(&mut receiver));
        if error != AXError::Success {
            return None;
        }
        NonNull::new(receiver.cast::<T>().cast_mut()).map(|pointer| CFRetained::from_raw(pointer))
    }
}

fn bool_attribute(element: &AXUIElement, attribute: &str) -> bool {
    copy_attribute::<CFBoolean>(element, attribute).is_some_and(|value| value.as_bool())
}

fn set_bool(element: &AXUIElement, attribute: &str, on: bool) -> Result<(), String> {
    let value = CFBoolean::new(on);
    // SAFETY: plain attribute write on an element.
    let error = unsafe { element.set_attribute_value(&CFString::from_str(attribute), value) };
    if error == AXError::Success {
        Ok(())
    } else {
        Err(format!("setting {attribute}: {error:?}"))
    }
}

fn frame_of(element: &AXUIElement) -> Option<CGRect> {
    let position = copy_attribute::<AXValue>(element, "AXPosition")?;
    let size = copy_attribute::<AXValue>(element, "AXSize")?;
    let mut rect = CGRect::default();
    // SAFETY: each value is read into a field of the type its tag names.
    unsafe {
        position.value(
            AXValueType::CGPoint,
            NonNull::from_mut(&mut rect.origin).cast(),
        );
        size.value(
            AXValueType::CGSize,
            NonNull::from_mut(&mut rect.size).cast(),
        );
    }
    Some(rect)
}

fn rect_json(x: f64, y: f64, width: f64, height: f64) -> Value {
    // the shape komorebi itself prints: right and bottom are width and height
    json!({
        "left": x.round() as i64, "top": y.round() as i64,
        "right": width.round() as i64, "bottom": height.round() as i64,
    })
}

/// A window of a regular application, with what the agent needs to act on it.
struct Window {
    pid: i32,
    app: String,
    active: bool,
    element: CFRetained<AXUIElement>,
    title: String,
}

fn windows() -> Vec<Window> {
    let mut out = Vec::new();
    let applications = NSWorkspace::sharedWorkspace().runningApplications();
    for application in applications.iter() {
        if application.activationPolicy() != NSApplicationActivationPolicy::Regular {
            continue;
        }
        let pid = application.processIdentifier();
        // SAFETY: an application element for a process id.
        let element = unsafe { AXUIElement::new_application(pid) };
        // SAFETY: a timeout on the element just made.
        let _ = unsafe { element.set_messaging_timeout(AX_TIMEOUT) };
        let Some(elements) = copy_attribute::<CFArray<AXUIElement>>(&element, "AXWindows") else {
            continue;
        };
        let app = application
            .localizedName()
            .map(|name| name.to_string())
            .unwrap_or_default();
        for window in elements {
            // SAFETY: as above.
            let _ = unsafe { window.set_messaging_timeout(AX_TIMEOUT) };
            let title = copy_attribute::<CFString>(&window, "AXTitle")
                .map(|title| title.to_string())
                .unwrap_or_default();
            out.push(Window {
                pid,
                app: app.clone(),
                active: application.isActive(),
                element: window,
                title,
            });
        }
    }
    out
}

fn describe(window: &Window) -> Value {
    let frame = frame_of(&window.element).unwrap_or_default();
    let minimized = bool_attribute(&window.element, "AXMinimized");
    let focused = window.active
        && (bool_attribute(&window.element, "AXFocused")
            || bool_attribute(&window.element, "AXMain"));
    json!({
        "title": window.title,
        "app": window.app,
        "pid": window.pid,
        "role": copy_attribute::<CFString>(&window.element, "AXRole").map(|r| r.to_string()),
        "subrole": copy_attribute::<CFString>(&window.element, "AXSubrole").map(|r| r.to_string()),
        "frame": rect_json(frame.origin.x, frame.origin.y, frame.size.width, frame.size.height),
        "minimized": minimized,
        "focused": focused,
    })
}

fn find(title: &str) -> Result<Window, String> {
    windows()
        .into_iter()
        .find(|window| window.title == title)
        .ok_or_else(|| format!("no window titled {title:?}"))
}

fn displays(mtm: MainThreadMarker) -> Vec<Value> {
    let screens = NSScreen::screens(mtm);
    // AppKit's origin is the bottom left of the main display; the daemon's, and ours, the top left
    let Some(main) = screens.iter().next() else {
        return Vec::new();
    };
    let main_height = main.frame().size.height;
    let flip = |rect: CGRect| {
        rect_json(
            rect.origin.x,
            main_height - (rect.origin.y + rect.size.height),
            rect.size.width,
            rect.size.height,
        )
    };
    screens
        .iter()
        .enumerate()
        .map(|(i, screen)| {
            json!({
                "monitor": flip(screen.frame()),
                "work_area": flip(screen.visibleFrame()),
                "primary": i == 0,
            })
        })
        .collect()
}

fn activate(title: &str) -> Result<Value, String> {
    let window = find(title)?;
    if bool_attribute(&window.element, "AXMinimized") {
        set_bool(&window.element, "AXMinimized", false)?;
    }
    let application =
        objc2_app_kit::NSRunningApplication::runningApplicationWithProcessIdentifier(window.pid);
    if let Some(application) = application {
        #[allow(deprecated)]
        application.activateWithOptions(NSApplicationActivationOptions::ActivateAllWindows);
    }
    set_bool(&window.element, "AXMain", true)?;
    set_bool(&window.element, "AXFocused", true)?;
    Ok(json!(true))
}

fn close(title: &str) -> Result<Value, String> {
    let window = find(title)?;
    let button = copy_attribute::<AXUIElement>(&window.element, "AXCloseButton")
        .ok_or_else(|| format!("{title:?} has no close button"))?;
    // SAFETY: an action on an element.
    let error = unsafe { button.perform_action(&CFString::from_str("AXPress")) };
    if error == AXError::Success {
        Ok(json!(true))
    } else {
        Err(format!("pressing the close button: {error:?}"))
    }
}

fn move_to(title: &str, x: f64, y: f64) -> Result<Value, String> {
    let window = find(title)?;
    let mut point = CGPoint::new(x, y);
    // SAFETY: the value is made from a point and tagged as one.
    let value = unsafe { AXValue::new(AXValueType::CGPoint, NonNull::from_mut(&mut point).cast()) }
        .ok_or("could not make a position")?;
    // SAFETY: plain attribute write on an element.
    let error = unsafe {
        window
            .element
            .set_attribute_value(&CFString::from_str("AXPosition"), &value)
    };
    if error == AXError::Success {
        Ok(json!(true))
    } else {
        Err(format!("moving {title:?}: {error:?}"))
    }
}

fn answer(stream: &mut TcpStream, id: &Value, result: Result<Value, String>) {
    let line = match result {
        Ok(ok) => json!({"id": id, "ok": ok}),
        Err(error) => json!({"id": id, "error": error}),
    };
    let _ = writeln!(stream, "{line}");
}

fn exec(args: &[String], env: &HashMap<String, String>) -> Result<Value, String> {
    let (program, rest) = args.split_first().ok_or("exec wants a program")?;
    let out = Command::new(program)
        .args(rest)
        .envs(env)
        .stdin(Stdio::null())
        .output()
        .map_err(|e| format!("{program}: {e}"))?;
    Ok(json!({
        "status": out.status.code(),
        "stdout": String::from_utf8_lossy(&out.stdout),
        "stderr": String::from_utf8_lossy(&out.stderr),
    }))
}

/// Starts a program that keeps running and returns at once. The agent keeps the child, so that
/// `status` can say how it ended and it is not left a zombie.
fn spawn(
    args: &[String],
    env: &HashMap<String, String>,
    out: &Option<String>,
    children: &mut HashMap<u32, Child>,
) -> Result<Value, String> {
    let (program, rest) = args.split_first().ok_or("spawn wants a program")?;
    let mut command = Command::new(program);
    command.args(rest).envs(env).stdin(Stdio::null());
    match out {
        Some(path) => {
            let file = std::fs::File::create(path).map_err(|e| format!("{path}: {e}"))?;
            let again = file.try_clone().map_err(|e| format!("{path}: {e}"))?;
            command.stdout(file).stderr(again);
        }
        None => {
            command.stdout(Stdio::null()).stderr(Stdio::null());
        }
    }
    let child = command.spawn().map_err(|e| format!("{program}: {e}"))?;
    let pid = child.id();
    children.insert(pid, child);
    Ok(json!({"pid": pid}))
}

fn status(pid: u32, children: &mut HashMap<u32, Child>) -> Result<Value, String> {
    let child = children
        .get_mut(&pid)
        .ok_or_else(|| format!("{pid} was not started by this agent"))?;
    match child.try_wait() {
        Ok(Some(status)) => Ok(json!({"running": false, "code": status.code()})),
        Ok(None) => Ok(json!({"running": true})),
        Err(e) => Err(e.to_string()),
    }
}

fn serve(
    stream: TcpStream,
    mtm: MainThreadMarker,
    children: &mut HashMap<u32, Child>,
) -> std::io::Result<()> {
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
        let result = match req.verb.as_str() {
            "ping" => Ok(json!("pong")),
            "exec" => exec(&req.args, &req.env),
            "spawn" => spawn(&req.args, &req.env, &req.out, children),
            "status" => status(req.pid, children),
            "os_windows" => Ok(Value::Array(windows().iter().map(describe).collect())),
            "displays" => Ok(Value::Array(displays(mtm))),
            "activate" => activate(&req.title),
            "minimize" => find(&req.title)
                .and_then(|w| set_bool(&w.element, "AXMinimized", true))
                .map(|()| json!(true)),
            "close" => close(&req.title),
            "move" => move_to(&req.title, req.x, req.y),
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
    let mtm = MainThreadMarker::new().expect("main runs on the main thread");
    // the window server connection that NSScreen needs, with no Dock icon and no window
    let application = NSApplication::sharedApplication(mtm);
    application.setActivationPolicy(NSApplicationActivationPolicy::Accessory);

    let port = std::env::var("KOMOREBI_E2E_AGENT_PORT")
        .ok()
        .and_then(|p| p.parse().ok())
        .unwrap_or(DEFAULT_PORT);
    let listener = TcpListener::bind(("0.0.0.0", port))?;
    let mut children = HashMap::new();
    for stream in listener.incoming() {
        if let Err(e) = serve(stream?, mtm, &mut children) {
            eprintln!("connection ended: {e}");
        }
    }
    Ok(())
}
