//! The transparency keys of `komorebi.json` are the transparency settings: the file sets them when
//! the daemon starts and when it is read again (a message that tiles and applies the opacity), a key
//! it leaves out has the default, the ignore rules keep windows opaque, and the settings come out
//! again in the configuration the daemon generates.
//!
//! This file has one test because it changes the process-wide transparency settings, which every
//! other test that tiles windows would see.

use color_eyre::eyre;
use komorebi::backend::fake::FakeBackend;
use komorebi::backend::with_backend;
use komorebi::core::SocketMessage;
use komorebi::static_config::StaticConfig;
use komorebi::static_config::StaticConfigExt;
use komorebi::window::Window;
use komorebi::window_manager::WindowManager;
use komorebi::window_manager_event::KWinNotification;
use komorebi::window_manager_event::SystemNotification;
use komorebi::window_manager_event::WindowManagerEvent;
use parking_lot::Mutex;
use std::os::unix::net::UnixListener;
use std::path::PathBuf;
use std::sync::Arc;

fn write_config(name: &str, json: &str) -> PathBuf {
    let path = PathBuf::from(format!(
        "/tmp/kmb-transp-{name}-{}.json",
        std::process::id()
    ));
    std::fs::write(&path, json).unwrap();
    path
}

#[test]
fn the_transparency_keys_of_the_file_are_the_transparency_settings() {
    let fake = Arc::new(FakeBackend::with_one_monitor());

    let path = write_config(
        "on",
        r#"{
            "transparency": true,
            "transparency_alpha": 120,
            "transparency_ignore_rules": [
                { "kind": "Exe", "id": "mpv", "matching_strategy": "Equals" }
            ]
        }"#,
    );
    let (_sender, receiver) = crossbeam_channel::bounded(1);
    let socket = PathBuf::from(format!("/tmp/kmb-transp-{}.sock", std::process::id()));
    let listener = UnixListener::bind(&socket).unwrap();

    let wm = with_backend(fake.clone(), || -> eyre::Result<WindowManager> {
        let mut wm = StaticConfig::preload(&path, receiver, Some(listener))?;
        wm.init()?;
        Ok(wm)
    })
    .unwrap();
    let wm = Arc::new(Mutex::new(wm));
    with_backend(fake.clone(), || StaticConfig::postload(&path, &wm)).unwrap();
    let _ = std::fs::remove_file(&socket);

    assert!(komorebi::settings::transparency_enabled());
    assert_eq!(komorebi::settings::transparency_alpha(), 120);

    // two windows, one of which the rules keep opaque, and the one without the focus is see-through
    with_backend(fake.clone(), || {
        for (id, exe) in [(46_001, "mpv"), (46_002, "foot"), (46_003, "foot")] {
            fake.open_window(id, exe, exe, &format!("window {id}"));
            wm.lock()
                .process_event(WindowManagerEvent::Show(
                    SystemNotification::KWin(KWinNotification::WindowAdded),
                    Window::from(id),
                ))
                .unwrap();
        }
    });
    assert_eq!(fake.opacity_of(46_002), 120, "a window without the focus");
    assert_eq!(fake.opacity_of(46_003), 255, "the one with the focus");
    assert_eq!(fake.opacity_of(46_001), 255, "a window the rules ignore");

    // and they come out again in the generated configuration
    let generated = StaticConfig::from(&*wm.lock());
    assert_eq!(generated.transparency, Some(true));
    assert_eq!(generated.transparency_alpha, Some(120));

    // a file that is read again and leaves the keys out has the defaults
    let empty = write_config("empty", "{}");
    with_backend(fake.clone(), || {
        wm.lock()
            .process_command(
                SocketMessage::ReloadStaticConfiguration(empty.clone()),
                vec![],
            )
            .unwrap();
    });
    assert!(!komorebi::settings::transparency_enabled());
    assert_eq!(komorebi::settings::transparency_alpha(), 200);
    assert_eq!(fake.opacity_of(46_002), 255, "opaque again");

    // the ignore rules add up, and are not asked again
    let more = write_config(
        "more",
        r#"{ "transparency_ignore_rules": [
            { "kind": "Exe", "id": "mpv", "matching_strategy": "Equals" },
            { "kind": "Class", "id": "vlc", "matching_strategy": "Equals" }
        ] }"#,
    );
    with_backend(fake.clone(), || {
        wm.lock()
            .process_command(
                SocketMessage::ReloadStaticConfiguration(more.clone()),
                vec![],
            )
            .unwrap();
    });
    assert_eq!(komorebi::TRANSPARENCY_BLACKLIST.lock().len(), 2);

    for file in [path, empty, more] {
        let _ = std::fs::remove_file(file);
    }
}
