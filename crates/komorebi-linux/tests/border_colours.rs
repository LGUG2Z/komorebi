//! The color of each window's border, for what the window is to the layout, against an in-memory
//! desktop.
//!
//! Window ids are in the 30_000s.

use komorebi::backend::fake::FakeBackend;
use komorebi::backend::with_backend;
use komorebi::core::OperationDirection;
use komorebi::core::SocketMessage;
use komorebi::window::Window;
use komorebi::window_manager::WindowManager;
use komorebi::window_manager_event::KWinNotification;
use komorebi::window_manager_event::SystemNotification;
use komorebi::window_manager_event::WindowManagerEvent;
use std::sync::Arc;

fn added(id: u32) -> WindowManagerEvent {
    WindowManagerEvent::Show(
        SystemNotification::KWin(KWinNotification::WindowAdded),
        Window::from(id),
    )
}

fn focused(id: u32) -> WindowManagerEvent {
    WindowManagerEvent::FocusChange(
        SystemNotification::KWin(KWinNotification::WindowActivated),
        Window::from(id),
    )
}

fn desktop() -> (Arc<FakeBackend>, WindowManager) {
    let fake = Arc::new(FakeBackend::with_one_monitor());
    let (_sender, receiver) = crossbeam_channel::bounded(1);
    static NEXT: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);
    let socket = std::path::PathBuf::from(format!(
        "/tmp/kmf-{}-{}.sock",
        std::process::id(),
        NEXT.fetch_add(1, std::sync::atomic::Ordering::SeqCst)
    ));

    let wm = with_backend(fake.clone(), || {
        let mut wm = WindowManager::new(receiver, Some(socket)).unwrap();
        wm.init().unwrap();
        wm
    });

    (fake, wm)
}

fn tiled(first: u32, count: u32) -> (Arc<FakeBackend>, WindowManager) {
    let (fake, mut wm) = desktop();
    with_backend(fake.clone(), || {
        for id in first..first + count {
            fake.open_window(id, "foot", "foot", &format!("terminal {id}"));
            wm.process_event(added(id)).unwrap();
        }
    });
    (fake, wm)
}

fn command(fake: &Arc<FakeBackend>, wm: &mut WindowManager, message: SocketMessage) {
    with_backend(fake.clone(), || wm.process_command(message, Vec::new())).unwrap();
}

// The colours are the defaults: focused (66, 165, 245), unfocused (128, 128, 128), locked and
// unfocused (158, 8, 8), monocle (255, 51, 153), stack (0, 165, 66), floating (245, 245, 165).

const SINGLE: u32 = 0x42A5F5;
const UNFOCUSED: u32 = 0x808080;
const UNFOCUSED_LOCKED: u32 = 0x9E0808;
const MONOCLE: u32 = 0xFF3399;
const STACK: u32 = 0x00A542;
const FLOATING: u32 = 0xF5F5A5;

#[test]
fn the_focused_window_has_the_focus_colour_and_the_others_do_not() {
    let (fake, _wm) = tiled(30_201, 3);

    assert_eq!(fake.border_colour_of(30_203), Some(SINGLE));
    assert_eq!(fake.border_colour_of(30_201), Some(UNFOCUSED));
    assert_eq!(fake.border_colour_of(30_202), Some(UNFOCUSED));
}

#[test]
fn a_locked_container_that_is_not_focused_is_marked() {
    let (fake, mut wm) = tiled(30_211, 3);

    with_backend(fake.clone(), || wm.process_event(focused(30_211))).unwrap();
    command(&fake, &mut wm, SocketMessage::ToggleLock);
    assert_eq!(
        fake.border_colour_of(30_211),
        Some(SINGLE),
        "a locked container with the focus is the focused one"
    );

    with_backend(fake.clone(), || wm.process_event(focused(30_213))).unwrap();
    assert_eq!(fake.border_colour_of(30_211), Some(UNFOCUSED_LOCKED));
    assert_eq!(fake.border_colour_of(30_212), Some(UNFOCUSED));
    assert_eq!(fake.border_colour_of(30_213), Some(SINGLE));
}

#[test]
fn a_stack_has_its_own_colour_when_it_has_the_focus() {
    let (fake, mut wm) = tiled(30_221, 3);

    command(
        &fake,
        &mut wm,
        SocketMessage::StackWindow(OperationDirection::Up),
    );

    // the focused window is now in a container with another
    let stacked = wm
        .focused_workspace()
        .unwrap()
        .focused_container()
        .unwrap()
        .windows()
        .len();
    assert_eq!(stacked, 2);
    let focused = wm
        .focused_workspace()
        .unwrap()
        .focused_container()
        .unwrap()
        .focused_window()
        .unwrap()
        .id;
    assert_eq!(fake.border_colour_of(focused), Some(STACK));
}

#[test]
fn a_monocled_window_has_the_monocle_colour() {
    let (fake, mut wm) = tiled(30_231, 3);
    command(&fake, &mut wm, SocketMessage::ToggleMonocle);

    assert_eq!(fake.border_colour_of(30_233), Some(MONOCLE));
}

#[test]
fn a_floating_window_has_the_floating_colour_while_it_has_the_focus() {
    let (fake, mut wm) = tiled(30_241, 3);
    command(&fake, &mut wm, SocketMessage::ToggleFloat);

    // the focused window floats now, and has the focus
    assert_eq!(fake.border_colour_of(30_243), Some(FLOATING));

    // focus goes to a tile: the floating window is one of the others
    fake.set_foreground(Some(30_241));
    with_backend(fake.clone(), || wm.process_event(focused(30_241))).unwrap();
    assert_eq!(fake.border_colour_of(30_243), Some(UNFOCUSED));
    assert_eq!(fake.border_colour_of(30_241), Some(SINGLE));
}
