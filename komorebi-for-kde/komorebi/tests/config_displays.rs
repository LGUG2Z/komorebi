//! A `komorebi.json` with more than one monitor in it, against an in-memory desktop with three:
//! which monitor takes which configuration.
//!
//! A monitor takes the configuration at the index `display_index_preferences` gives its serial
//! number id, and a monitor without one takes the first configuration that no preference and no
//! monitor has taken. The preferences are process-wide, so this file is one test.

use komorebi::backend::fake::FakeBackend;
use komorebi::backend::with_backend;
use komorebi::core::Rect;
use komorebi::monitor::Monitor;
use komorebi::static_config::StaticConfig;
use parking_lot::Mutex;
use std::os::unix::net::UnixListener;
use std::path::PathBuf;
use std::sync::Arc;

fn screen(width: i32) -> Rect {
    Rect {
        left: 0,
        top: 0,
        right: width,
        bottom: 1080,
    }
}

fn workspace_names(monitor: &Monitor) -> Vec<Option<String>> {
    monitor
        .workspaces()
        .iter()
        .map(|workspace| workspace.name.clone())
        .collect()
}

#[test]
fn a_monitor_takes_the_configuration_its_display_index_preference_gives_it() {
    let fake = Arc::new(FakeBackend::new());
    fake.add_monitor(1, "Left", screen(1920), screen(1920));
    fake.add_monitor(2, "Right", screen(1920), screen(1920));
    fake.add_monitor(3, "Third", screen(1920), screen(1920));

    let path = PathBuf::from(format!("/tmp/kmd-{}.json", std::process::id()));
    std::fs::write(
        &path,
        r#"{
            "display_index_preferences": { "1": "Left" },
            "monitors": [
                { "workspaces": [ { "name": "first" } ] },
                { "workspaces": [ { "name": "second" }, { "name": "second too" } ] }
            ]
        }"#,
    )
    .unwrap();
    let socket = PathBuf::from(format!("/tmp/kmd-{}.sock", std::process::id()));
    let (_sender, receiver) = crossbeam_channel::bounded(1);

    let wm = with_backend(fake.clone(), || {
        let mut wm =
            StaticConfig::preload(&path, receiver, Some(UnixListener::bind(&socket).unwrap()))
                .unwrap();
        wm.init().unwrap();
        wm
    });
    let wm = Arc::new(Mutex::new(wm));
    with_backend(fake.clone(), || StaticConfig::postload(&path, &wm)).unwrap();
    let _ = std::fs::remove_file(&socket);
    let _ = std::fs::remove_file(&path);

    let wm = wm.lock();
    let monitor = |serial: &str| -> &Monitor {
        wm.monitors()
            .iter()
            .find(|monitor| monitor.serial_number_id == serial)
            .unwrap()
    };

    // the preference makes "Left" the monitor of the second configuration
    assert_eq!(
        workspace_names(monitor("Left")),
        [
            Some(String::from("second")),
            Some(String::from("second too"))
        ]
    );
    // "Right" has no preference: the first configuration that is no one's preference
    assert_eq!(
        workspace_names(monitor("Right")),
        [Some(String::from("first"))]
    );
    // "Third" has none left, and keeps what it began with
    assert_eq!(workspace_names(monitor("Third")), [None]);

    // and the state the daemon would generate keeps the preference
    let generated = StaticConfig::from(&*wm);
    assert_eq!(
        generated.display_index_preferences.unwrap().get(&1),
        Some(&String::from("Left"))
    );
}
