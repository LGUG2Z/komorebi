//! `ReplaceConfiguration` against an in-memory desktop: the window manager that is running is
//! made again from another `komorebi.json`, with the windows the desktop has.
//!
//! The settings a file sets are process-wide, so this file is one test. Window ids are in the
//! 41_000s.

use komorebi::backend::fake::FakeBackend;
use komorebi::backend::with_backend;
use komorebi::core::OperationDirection;
use komorebi::core::Rect;
use komorebi::core::SocketMessage;
use komorebi::static_config::StaticConfig;
use komorebi::static_config::StaticConfigExt;
use komorebi::window::Window;
use komorebi::window_manager::WindowManager;
use komorebi::window_manager_event::KWinNotification;
use komorebi::window_manager_event::SystemNotification;
use komorebi::window_manager_event::WindowManagerEvent;
use std::os::unix::net::UnixListener;
use std::path::PathBuf;
use std::sync::Arc;

fn rect(left: i32, top: i32, right: i32, bottom: i32) -> Rect {
    Rect {
        left,
        top,
        right,
        bottom,
    }
}

fn write(name: &str, json: &str) -> PathBuf {
    let path = PathBuf::from(format!("/tmp/kmr-{}-{name}.json", std::process::id()));
    std::fs::write(&path, json).unwrap();
    path
}

fn workspace_names(wm: &WindowManager) -> Vec<Option<String>> {
    wm.monitors()[0]
        .workspaces()
        .iter()
        .map(|workspace| workspace.name.clone())
        .collect()
}

#[test]
fn replacing_the_configuration_makes_the_window_manager_again_from_the_new_file() {
    let fake = Arc::new(FakeBackend::with_one_monitor());
    let first = write(
        "first",
        r#"{ "resize_delta": 100, "monitors": [ { "workspaces": [ { "name": "one", "layout": "BSP" } ] } ] }"#,
    );
    let socket = PathBuf::from(format!("/tmp/kmr-{}.sock", std::process::id()));
    let (_sender, receiver) = crossbeam_channel::bounded(1);

    let mut wm = with_backend(fake.clone(), || {
        let mut wm =
            StaticConfig::preload(&first, receiver, Some(UnixListener::bind(&socket).unwrap()))
                .unwrap();
        wm.init().unwrap();
        wm
    });
    fake.open_window(41_001, "foot", "foot", "terminal");
    with_backend(fake.clone(), || {
        wm.process_event(WindowManagerEvent::Show(
            SystemNotification::KWin(KWinNotification::WindowAdded),
            Window::from(41_001),
        ))
    })
    .unwrap();
    assert_eq!(wm.resize_delta, 100);
    assert_eq!(fake.rect_of(41_001), Some(rect(10, 10, 1900, 1030)));

    let second = write(
        "second",
        r#"{
            "resize_delta": 30,
            "default_workspace_padding": 0,
            "default_container_padding": 0,
            "monitors": [ { "workspaces": [ { "name": "uno", "layout": "Columns" }, { "name": "dos" } ] } ]
        }"#,
    );
    with_backend(fake.clone(), || {
        wm.process_command(
            SocketMessage::ReplaceConfiguration(second.clone()),
            Vec::new(),
        )
    })
    .unwrap();

    // a window manager made from the new file, which found the window the desktop has
    assert_eq!(wm.resize_delta, 30);
    assert_eq!(
        workspace_names(&wm),
        [Some(String::from("uno")), Some(String::from("dos"))]
    );
    assert_eq!(fake.rect_of(41_001), Some(rect(0, 0, 1920, 1050)));
    assert!(wm.monitors()[0].workspaces()[0].contains_window(41_001));

    // a file that is not a configuration changes nothing, and does not stop the daemon
    let broken = write("broken", r#"{ "resize_delta": "wide" "#);
    with_backend(fake.clone(), || {
        wm.process_command(
            SocketMessage::ReplaceConfiguration(broken.clone()),
            Vec::new(),
        )
    })
    .unwrap();
    assert_eq!(wm.resize_delta, 30);
    assert_eq!(workspace_names(&wm).len(), 2);

    // the back window of a stack is out of sight, and the new window manager would not find it
    // there: the old one brings it back before the new one looks at the desktop
    fake.open_window(41_002, "foot", "foot", "terminal");
    for event in [
        WindowManagerEvent::Show(
            SystemNotification::KWin(KWinNotification::WindowAdded),
            Window::from(41_002),
        ),
        WindowManagerEvent::FocusChange(
            SystemNotification::KWin(KWinNotification::WindowActivated),
            Window::from(41_002),
        ),
    ] {
        fake.set_foreground(Some(41_002));
        with_backend(fake.clone(), || wm.process_event(event)).unwrap();
    }
    with_backend(fake.clone(), || {
        wm.process_command(
            SocketMessage::StackWindow(OperationDirection::Left),
            Vec::new(),
        )
    })
    .unwrap();
    assert_eq!(
        [41_001, 41_002]
            .iter()
            .filter(|id| fake.is_hidden(**id))
            .count(),
        1
    );

    with_backend(fake.clone(), || {
        wm.process_command(
            SocketMessage::ReplaceConfiguration(second.clone()),
            Vec::new(),
        )
    })
    .unwrap();
    assert!(!fake.is_hidden(41_001) && !fake.is_hidden(41_002));
    assert_eq!(
        wm.focused_workspace().unwrap().containers().len(),
        2,
        "the new window manager found both windows, each with a tile"
    );

    let _ = std::fs::remove_file(&socket);
    for path in [first, second, broken] {
        let _ = std::fs::remove_file(path);
    }
}
