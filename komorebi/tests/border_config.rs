//! The border keys of `komorebi.json` are the border settings: the file sets them when the daemon
//! starts and when it is read again, a key it leaves out leaves the setting as it is, and the
//! settings come out again in the configuration the daemon generates.
//!
//! This file has one test because it changes the process-wide border settings, which every other
//! test that tiles windows would see.

use color_eyre::eyre;
use komorebi::backend::fake::FakeBackend;
use komorebi::backend::with_backend;
use komorebi::border_manager;
use komorebi::static_config::StaticConfig;
use komorebi::window_manager::WindowManager;
use parking_lot::Mutex;
use std::os::unix::net::UnixListener;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::Ordering;

fn write_config(name: &str, json: &str) -> PathBuf {
    let path = PathBuf::from(format!("/tmp/kmb-{name}-{}.json", std::process::id()));
    std::fs::write(&path, json).unwrap();
    path
}

#[test]
fn the_border_keys_of_the_file_are_the_border_settings() {
    let fake = Arc::new(FakeBackend::with_one_monitor());
    fake.set_borders_drawn(true);

    let path = write_config(
        "set",
        r#"{ "border": true, "border_width": 6, "border_offset": 2, "border_radius": 12 }"#,
    );
    let (_sender, receiver) = crossbeam_channel::bounded(1);
    let socket = PathBuf::from(format!("/tmp/kmb-config-{}.sock", std::process::id()));
    let listener = UnixListener::bind(&socket).unwrap();

    let wm = with_backend(fake.clone(), || -> eyre::Result<WindowManager> {
        let mut wm = StaticConfig::preload(&path, receiver, Some(listener))?;
        wm.init()?;
        Ok(wm)
    })
    .unwrap();
    let wm = Arc::new(Mutex::new(wm));
    StaticConfig::postload(&path, &wm).unwrap();
    let _ = std::fs::remove_file(&socket);

    // the file made the settings, and the settings make the look
    assert_eq!(border_manager::BORDER_WIDTH.load(Ordering::SeqCst), 6);
    assert_eq!(border_manager::BORDER_OFFSET.load(Ordering::SeqCst), 2);
    assert_eq!(border_manager::BORDER_RADIUS.load(Ordering::SeqCst), 12);
    let border = border_manager::current_look()
        .border
        .expect("borders are on");
    assert_eq!((border.width, border.radius), (6, 12));

    // and they come out again in the generated configuration
    let generated = StaticConfig::from(&*wm.lock());
    assert_eq!(generated.border, Some(true));
    assert_eq!(generated.border_width, Some(6));
    assert_eq!(generated.border_offset, Some(2));
    assert_eq!(generated.border_radius, Some(12));

    // the old names of the keys are the same keys
    let aliased = StaticConfig::read_raw(
        r#"{ "active_window_border": false, "active_window_border_width": 4,
             "active_window_border_offset": 1 }"#,
    )
    .unwrap();
    assert_eq!(aliased.border, Some(false));
    assert_eq!(aliased.border_width, Some(4));
    assert_eq!(aliased.border_offset, Some(1));

    // a file that is read again changes what it sets, with borders off asking for no border
    let other = write_config("other", r#"{ "border": false, "border_width": 3 }"#);
    with_backend(fake.clone(), || {
        StaticConfig::reload(&other, &mut wm.lock()).unwrap();
    });
    assert_eq!(border_manager::BORDER_WIDTH.load(Ordering::SeqCst), 3);
    assert!(!border_manager::BORDER_ENABLED.load(Ordering::SeqCst));
    assert_eq!(border_manager::current_look().border, None);

    // and one that leaves the keys out leaves the settings as they were
    let empty = write_config("empty", "{}");
    with_backend(fake.clone(), || {
        StaticConfig::reload(&empty, &mut wm.lock()).unwrap();
    });
    assert_eq!(border_manager::BORDER_WIDTH.load(Ordering::SeqCst), 3);
    assert_eq!(border_manager::BORDER_OFFSET.load(Ordering::SeqCst), 2);
    assert_eq!(border_manager::BORDER_RADIUS.load(Ordering::SeqCst), 12);
    assert!(!border_manager::BORDER_ENABLED.load(Ordering::SeqCst));

    for file in [path, other, empty] {
        let _ = std::fs::remove_file(file);
    }
}
