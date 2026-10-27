//! The client against a window manager that listens the way the daemon does, with
//! [`listen_for_commands`], over an in-memory desktop with one monitor and two windows.
//!
//! The tests share the window manager, and its socket in a runtime directory of this process.

use komorebi::backend::fake::FakeBackend;
use komorebi::backend::set_backend;
use komorebi::core::Rect;
use komorebi::process_command::listen_for_commands;
use komorebi::window_manager::WindowManager;
use komorebi::window_manager_event_listener;
use komorebi_client::DATA_DIR;
use komorebi_client::SocketMessage;
use komorebi_client::State;
use komorebi_client::StateQuery;
use komorebi_client::send_batch;
use komorebi_client::send_message;
use komorebi_client::send_query;
use parking_lot::Mutex;
use std::sync::Arc;
use std::sync::OnceLock;
use std::time::Duration;
use std::time::Instant;

/// Starts the window manager once, with the windows 801 and 802 open, and returns the desktop
fn daemon() -> Arc<FakeBackend> {
    static FAKE: OnceLock<Arc<FakeBackend>> = OnceLock::new();

    FAKE.get_or_init(|| {
        // /tmp: short enough for a unix socket path on macOS
        let runtime = format!("/tmp/komorebi-client-test-{}", std::process::id());
        // SAFETY: the first test to get here sets it, before any thread reads the environment
        unsafe { std::env::set_var("XDG_RUNTIME_DIR", &runtime) };
        std::fs::create_dir_all(&*DATA_DIR).unwrap();

        let fake = Arc::new(FakeBackend::with_one_monitor());
        fake.open_window(801, "foot", "foot", "first");
        fake.open_window(802, "foot", "foot", "second");
        set_backend(fake.clone());

        let mut wm = WindowManager::new(window_manager_event_listener::event_rx(), None).unwrap();
        wm.init().unwrap();
        wm.retile_all().unwrap();
        listen_for_commands(Arc::new(Mutex::new(wm)));

        fake
    })
    .clone()
}

/// Waits up to five seconds for `done`, since a message that is sent is handled on another thread
fn eventually(done: impl Fn() -> bool) -> bool {
    let started = Instant::now();
    while started.elapsed() < Duration::from_secs(5) {
        if done() {
            return true;
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    false
}

#[test]
fn the_state_reads_back_as_the_window_managers_state() -> Result<(), Box<dyn std::error::Error>> {
    daemon();

    let state: State = serde_json::from_str(&send_query(&SocketMessage::State)?)?;

    let workspace = &state.monitors.elements()[0].workspaces()[0];
    let windows = workspace
        .containers()
        .iter()
        .flat_map(|container| container.windows().iter().map(|window| window.id))
        .collect::<Vec<_>>();
    assert_eq!(windows, [801, 802]);
    Ok(())
}

#[test]
fn a_query_is_answered_with_one_value() -> Result<(), Box<dyn std::error::Error>> {
    daemon();

    let kind = send_query(&SocketMessage::Query(StateQuery::FocusedContainerKind))?;
    let layout = send_query(&SocketMessage::Query(StateQuery::FocusedWorkspaceLayout))?;

    assert_eq!(kind, "Single");
    assert_eq!(layout, "BSP");
    Ok(())
}

#[test]
fn a_retile_puts_a_window_moved_by_hand_back_on_its_tile() -> Result<(), Box<dyn std::error::Error>>
{
    let fake = daemon();
    let tile = fake.rect_of(802);
    let elsewhere = Rect {
        left: 300,
        top: 300,
        right: 400,
        bottom: 400,
    };

    fake.move_window_by_user(802, elsewhere);
    send_message(&SocketMessage::Retile)?;
    assert!(eventually(|| fake.rect_of(802) == tile));

    fake.move_window_by_user(802, elsewhere);
    send_batch(vec![SocketMessage::Retile; 2])?;
    assert!(eventually(|| fake.rect_of(802) == tile));
    Ok(())
}
