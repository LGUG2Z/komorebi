//! Focus follows mouse, against an in-memory desktop.
//!
//! KWin has its own focus follows mouse, and komorebi asks for it: both implementations the
//! configuration can name are the compositor's.
//!
//! The `focus_follows_mouse` key of `komorebi.json` is a setting of the window manager, not a
//! process-wide one, so its test is here.

use komorebi::backend::fake::Command;
use komorebi::backend::fake::FakeBackend;
use komorebi::backend::with_backend;
use komorebi::core::FocusFollowsMouseImplementation;
use komorebi::core::SocketMessage;
use komorebi::state::State;
use komorebi::static_config::StaticConfig;
use komorebi::window_manager::WindowManager;
use parking_lot::Mutex;
use std::os::unix::net::UnixListener;
use std::path::PathBuf;
use std::sync::Arc;

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

fn command(fake: &Arc<FakeBackend>, wm: &mut WindowManager, message: SocketMessage) {
    with_backend(fake.clone(), || wm.process_command(message, Vec::new())).unwrap();
}

#[test]
fn focus_follows_mouse_is_asked_of_the_compositor_and_toggles() {
    let (fake, mut wm) = desktop();
    fake.clear_commands();

    command(
        &fake,
        &mut wm,
        SocketMessage::FocusFollowsMouse(FocusFollowsMouseImplementation::Windows, true),
    );
    assert_eq!(
        wm.focus_follows_mouse,
        Some(FocusFollowsMouseImplementation::Windows)
    );
    assert!(fake.commands().contains(&Command::FocusFollowsMouse(true)));

    command(
        &fake,
        &mut wm,
        SocketMessage::ToggleFocusFollowsMouse(FocusFollowsMouseImplementation::Windows),
    );
    assert_eq!(wm.focus_follows_mouse, None);
    assert!(fake.commands().contains(&Command::FocusFollowsMouse(false)));

    command(
        &fake,
        &mut wm,
        SocketMessage::ToggleFocusFollowsMouse(FocusFollowsMouseImplementation::Komorebi),
    );
    assert_eq!(
        wm.focus_follows_mouse,
        Some(FocusFollowsMouseImplementation::Komorebi),
        "the other implementation is KWin's too, and is remembered as asked for"
    );
}

#[test]
fn the_state_says_whether_focus_follows_mouse_is_on_and_a_change_is_a_modification() {
    let (fake, mut wm) = desktop();
    let before = State::from(&wm);
    assert_eq!(before.focus_follows_mouse, None);

    command(
        &fake,
        &mut wm,
        SocketMessage::FocusFollowsMouse(FocusFollowsMouseImplementation::Komorebi, true),
    );

    assert_eq!(
        State::from(&wm).focus_follows_mouse,
        Some(FocusFollowsMouseImplementation::Komorebi)
    );
    assert!(before.has_been_modified(&wm));
}

#[test]
fn the_file_turns_focus_follows_mouse_on_and_a_file_that_leaves_it_out_turns_it_off() {
    let fake = Arc::new(FakeBackend::with_one_monitor());
    let write = |name: &str, json: &str| {
        let path = PathBuf::from(format!("/tmp/kmf-ffm-{name}-{}.json", std::process::id()));
        std::fs::write(&path, json).unwrap();
        path
    };
    let path = write("on", r#"{ "focus_follows_mouse": "Komorebi" }"#);
    let (_sender, receiver) = crossbeam_channel::bounded(1);
    let socket = PathBuf::from(format!("/tmp/kmf-ffm-{}.sock", std::process::id()));
    let listener = UnixListener::bind(&socket).unwrap();

    let wm = with_backend(fake.clone(), || {
        let mut wm = StaticConfig::preload(&path, receiver, Some(listener)).unwrap();
        wm.init().unwrap();
        wm
    });
    let wm = Arc::new(Mutex::new(wm));
    // the window manager has it from the file, and KWin is asked once the monitors are loaded
    assert_eq!(
        wm.lock().focus_follows_mouse,
        Some(FocusFollowsMouseImplementation::Komorebi)
    );
    with_backend(fake.clone(), || StaticConfig::postload(&path, &wm).unwrap());
    let _ = std::fs::remove_file(&socket);
    assert!(fake.commands().contains(&Command::FocusFollowsMouse(true)));
    assert_eq!(
        StaticConfig::from(&*wm.lock()).focus_follows_mouse,
        Some(FocusFollowsMouseImplementation::Komorebi),
        "and it comes out again in the generated configuration"
    );

    // a file that is read again and leaves the key out gives the user's focus policy back
    let empty = write("empty", "{}");
    fake.clear_commands();
    with_backend(fake.clone(), || {
        StaticConfig::reload(&empty, &mut wm.lock()).unwrap();
    });
    assert_eq!(wm.lock().focus_follows_mouse, None);
    assert!(fake.commands().contains(&Command::FocusFollowsMouse(false)));

    for file in [path, empty] {
        let _ = std::fs::remove_file(file);
    }
}
