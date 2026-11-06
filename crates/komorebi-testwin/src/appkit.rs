//! The test window on macOS: an ordinary titled window of a regular application, which the daemon
//! sees as it sees any program's window.
//!
//! AppKit is for the main thread, so the main thread runs the event loop in short turns, and the
//! thread that reads standard input leaves its lines in a queue that each turn takes from. The
//! program leaves when its window is closed, as a program of one window does. Its name is the name
//! of the file, so that a copy under another name is another program for a rule that matches by it.

use std::collections::VecDeque;
use std::io::BufRead;
use std::sync::Mutex;
use std::time::Duration;
use std::time::Instant;

use objc2::MainThreadMarker;
use objc2::MainThreadOnly;
use objc2::rc::Retained;
use objc2_app_kit::NSAccessibility;
use objc2_app_kit::NSApplication;
use objc2_app_kit::NSApplicationActivationPolicy;
use objc2_app_kit::NSBackingStoreType;
use objc2_app_kit::NSColor;
use objc2_app_kit::NSEventMask;
use objc2_app_kit::NSScreen;
use objc2_app_kit::NSWindow;
use objc2_app_kit::NSWindowStyleMask;
use objc2_foundation::NSDate;
use objc2_foundation::NSDefaultRunLoopMode;
use objc2_foundation::NSPoint;
use objc2_foundation::NSRect;
use objc2_foundation::NSSize;
use objc2_foundation::NSString;

use crate::cli;
use crate::cli::Command;
use crate::cli::say;

/// The lines of standard input, in the order they came, until the main thread takes them.
static COMMANDS: Mutex<VecDeque<String>> = Mutex::new(VecDeque::new());

/// How long a turn of the event loop waits for an event, which is also how late a command or a
/// rename is acted on.
const TURN: f64 = 0.05;

/// The style and the frame of the window before it went fullscreen.
struct BeforeFullscreen {
    style: NSWindowStyleMask,
    frame: NSRect,
}

fn color(rgb: u32) -> Retained<NSColor> {
    let channel = |shift: u32| f64::from((rgb >> shift) & 0xff) / 255.0;
    NSColor::colorWithRed_green_blue_alpha(channel(16), channel(8), channel(0), 1.0)
}

/// The frame as the daemon and the host see it: points, with the origin at the top left of the
/// main display, where AppKit has it at the bottom left.
fn top_left_frame(mtm: MainThreadMarker, frame: NSRect) -> serde_json::Value {
    let main_height = NSScreen::screens(mtm)
        .iter()
        .next()
        .map_or(0.0, |screen| screen.frame().size.height);
    serde_json::json!({
        "left": frame.origin.x.round() as i64,
        "top": (main_height - (frame.origin.y + frame.size.height)).round() as i64,
        "right": frame.size.width.round() as i64,
        "bottom": frame.size.height.round() as i64,
    })
}

fn run_command(
    mtm: MainThreadMarker,
    window: &NSWindow,
    before: &mut Option<BeforeFullscreen>,
    command: Command,
) -> bool {
    match command {
        Command::Title(title) => {
            window.setTitle(&NSString::from_str(&title));
            say(serde_json::json!({"ok": true}));
        }
        Command::Color(rgb) => {
            window.setBackgroundColor(Some(&color(rgb)));
            say(serde_json::json!({"ok": true}));
        }
        Command::Resize(w, h) => {
            let mut frame = window.frame();
            frame.size = NSSize::new(f64::from(w), f64::from(h));
            window.setFrame_display(frame, true);
            say(serde_json::json!({"ok": true}));
        }
        Command::Fullscreen(on) => {
            fullscreen(mtm, window, before, on);
            say(serde_json::json!({"ok": true}));
        }
        Command::Report => say(serde_json::json!({
            "window": window.windowNumber(),
            "title": window.title().to_string(),
            "frame": top_left_frame(mtm, window.frame()),
        })),
        Command::Close => window.performClose(None),
        Command::Exit => return false,
        Command::Invalid(why) => say(serde_json::json!({"error": why})),
        Command::Empty => {}
    }
    true
}

/// A borderless window over the whole of its display, as a program's own key takes it, and back.
fn fullscreen(
    mtm: MainThreadMarker,
    window: &NSWindow,
    before: &mut Option<BeforeFullscreen>,
    on: bool,
) {
    if on && before.is_none() {
        let Some(screen) = window.screen().or_else(|| NSScreen::mainScreen(mtm)) else {
            return;
        };
        *before = Some(BeforeFullscreen {
            style: window.styleMask(),
            frame: window.frame(),
        });
        window.setStyleMask(NSWindowStyleMask::Borderless);
        window.setFrame_display(screen.frame(), true);
    } else if !on && let Some(before) = before.take() {
        window.setStyleMask(before.style);
        window.setFrame_display(before.frame, true);
    }
}

pub fn run(options: cli::Options) {
    let mtm = MainThreadMarker::new().expect("main runs on the main thread");
    let application = NSApplication::sharedApplication(mtm);
    application.setActivationPolicy(NSApplicationActivationPolicy::Regular);

    let mut style = NSWindowStyleMask::Titled
        | NSWindowStyleMask::Closable
        | NSWindowStyleMask::Miniaturizable
        | NSWindowStyleMask::Resizable;
    if options.fixed {
        style.remove(NSWindowStyleMask::Resizable);
    }
    let size = NSSize::new(f64::from(options.size.0), f64::from(options.size.1));
    // SAFETY: the designated initialiser, on the main thread.
    let window = unsafe {
        NSWindow::initWithContentRect_styleMask_backing_defer(
            NSWindow::alloc(mtm),
            NSRect::new(NSPoint::new(200.0, 200.0), size),
            style,
            NSBackingStoreType::Buffered,
            false,
        )
    };
    // SAFETY: this program owns the window, so a close does not release it a second time.
    unsafe { window.setReleasedWhenClosed(false) };
    window.setTitle(&NSString::from_str(&options.title));
    if let Some(class) = &options.class {
        // a macOS window has no class: the daemon takes the accessibility subrole of a window for
        // it, which a program is free to name as it likes
        window.setAccessibilitySubrole(Some(&NSString::from_str(class)));
    }
    if let Some(rgb) = options.color {
        window.setBackgroundColor(Some(&color(rgb)));
    }
    if let Some((w, h)) = options.min_size {
        window.setContentMinSize(NSSize::new(f64::from(w), f64::from(h)));
    }
    if options.fixed {
        window.setContentMinSize(size);
        window.setContentMaxSize(size);
    }
    window.makeKeyAndOrderFront(None);
    #[allow(deprecated)]
    application.activateIgnoringOtherApps(true);
    application.finishLaunching();

    std::thread::spawn(|| {
        for line in std::io::stdin().lock().lines() {
            let Ok(line) = line else { break };
            COMMANDS.lock().unwrap().push_back(line);
        }
        // standard input closed: the scenario is gone, so go too
        COMMANDS.lock().unwrap().push_back(String::from("exit"));
    });

    let began = Instant::now();
    let mut rename = options.rename;
    let mut before_fullscreen = None;
    loop {
        // SAFETY: the default run loop mode, on the main thread.
        let event = unsafe {
            application.nextEventMatchingMask_untilDate_inMode_dequeue(
                NSEventMask::Any,
                Some(&NSDate::dateWithTimeIntervalSinceNow(TURN)),
                NSDefaultRunLoopMode,
                true,
            )
        };
        if let Some(event) = event {
            application.sendEvent(&event);
        }
        if let Some((title, after)) = &rename
            && began.elapsed() >= Duration::from_secs_f64(*after)
        {
            window.setTitle(&NSString::from_str(title));
            rename = None;
        }
        loop {
            let Some(line) = COMMANDS.lock().unwrap().pop_front() else {
                break;
            };
            if !run_command(mtm, &window, &mut before_fullscreen, Command::parse(&line)) {
                return;
            }
        }
        // closed: not on screen, and not just minimized to the Dock
        if !window.isVisible() && !window.isMiniaturized() {
            return;
        }
    }
}
