//! The verbs of the macOS agent: the windows and displays of the logged-in GUI session.
//!
//! It has to run inside that session, and be started by a program that holds the Accessibility and
//! Screen Recording grants (the golden image starts it from a Terminal window), because the daemon
//! it starts, and the windows it reads, are attributed to that program.
//!
//! The windows it describes are the ones the accessibility API gives, as the daemon sees them;
//! their frames are in points, with the origin at the top left of the main display, as the daemon's
//! are. AppKit (`NSScreen`) is for the main thread, so one connection is served at a time, on it.
//! The agent injects no input.

use std::net::TcpListener;
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
use objc2_foundation::NSDate;
use objc2_foundation::NSDefaultRunLoopMode;
use objc2_foundation::NSRunLoop;
use serde_json::Value;
use serde_json::json;

use crate::protocol::Mode;
use crate::protocol::Request;
use crate::protocol::Verbs;
use crate::protocol::frame_json;

/// How long a call to an application that does not answer holds the agent.
const AX_TIMEOUT: f32 = 2.0;

/// A frame as the protocol has it, from points.
fn frame_json_f(x: f64, y: f64, width: f64, height: f64) -> Value {
    frame_json(
        x.round() as i64,
        y.round() as i64,
        width.round() as i64,
        height.round() as i64,
    )
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

/// A window of a regular application, with what the agent needs to act on it.
struct Window {
    pid: i32,
    app: String,
    active: bool,
    element: CFRetained<AXUIElement>,
    title: String,
}

/// Lets the main run loop turn once. What the workspace says of the running applications, and of
/// which one is active, is kept up to date by that loop, and the agent otherwise waits in
/// `accept`: it would list the applications of the time it started, and not see a window that
/// opened since.
fn turn_run_loop() {
    NSRunLoop::mainRunLoop().runMode_beforeDate(
        // SAFETY: the default mode, which is what the main loop runs in.
        unsafe { NSDefaultRunLoopMode },
        &NSDate::dateWithTimeIntervalSinceNow(0.05),
    );
}

fn windows() -> Vec<Window> {
    turn_run_loop();
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
        "frame": frame_json_f(frame.origin.x, frame.origin.y, frame.size.width, frame.size.height),
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
        frame_json_f(
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

/// The displays as a process that has just started sees them.
///
/// `NSScreen` caches the visible frame in the process, and turning the run loop does not refresh
/// it: an agent that has been up for a while keeps answering with the work area of the time it
/// started. The Dock of a guest is that kind of change: with the room it has on a display of 1280
/// points it shrinks its icons, and the work area gains 9 points, once the first test windows have
/// opened. The daemon reads the screens as it starts, so the work area it is held to is the one
/// that the system has now, and only a process of its own can say what that is.
fn fresh_displays(mtm: MainThreadMarker) -> Vec<Value> {
    let asked = std::env::current_exe()
        .and_then(|exe| {
            Command::new(exe)
                .arg(DISPLAYS_ARG)
                .stdin(Stdio::null())
                .output()
        })
        .ok()
        .filter(|output| output.status.success())
        .and_then(|output| serde_json::from_slice::<Vec<Value>>(&output.stdout).ok());
    asked.unwrap_or_else(|| displays(mtm))
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

struct MacosVerbs {
    mtm: MainThreadMarker,
}

impl Verbs for MacosVerbs {
    fn prepare(&self, command: &mut Command, mode: Mode) {
        match mode {
            Mode::Exec => {
                command.stdin(Stdio::null());
            }
            // the standard input stays open, because the test window leaves when it closes, and
            // `send` writes to it; what the program prints goes nowhere unless `out` names a file
            Mode::Spawn => {
                command
                    .stdin(Stdio::piped())
                    .stdout(Stdio::null())
                    .stderr(Stdio::null());
            }
        }
    }

    fn handle(&mut self, request: &Request) -> Option<Result<Value, String>> {
        Some(match request.verb.as_str() {
            "os_windows" => Ok(Value::Array(windows().iter().map(describe).collect())),
            "displays" => Ok(Value::Array(fresh_displays(self.mtm))),
            "activate" => activate(&request.title),
            "minimize" => find(&request.title)
                .and_then(|w| set_bool(&w.element, "AXMinimized", true))
                .map(|()| json!(true)),
            "close" => close(&request.title),
            "move" => move_to(&request.title, request.x, request.y),
            _ => return None,
        })
    }
}

/// The argument that makes the agent print its displays and exit, as `fresh_displays` asks.
const DISPLAYS_ARG: &str = "--print-displays";

pub fn run() -> std::io::Result<()> {
    let mtm = MainThreadMarker::new().expect("main runs on the main thread");
    // the window server connection that NSScreen needs, with no Dock icon and no window
    let application = NSApplication::sharedApplication(mtm);
    application.setActivationPolicy(NSApplicationActivationPolicy::Accessory);

    if std::env::args().nth(1).as_deref() == Some(DISPLAYS_ARG) {
        println!("{}", Value::Array(displays(mtm)));
        return Ok(());
    }

    let listener = TcpListener::bind(("0.0.0.0", crate::protocol::port()))?;
    let mut verbs = MacosVerbs { mtm };
    for stream in listener.incoming() {
        if let Err(e) = crate::protocol::serve(stream?, &mut verbs) {
            eprintln!("connection ended: {e}");
        }
    }
    Ok(())
}
