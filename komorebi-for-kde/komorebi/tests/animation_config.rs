//! The `animation` key of `komorebi.json` is the animation settings: the file sets them when the
//! daemon starts and when it is read again, for every kind of animation or for one, a file without
//! the key leaves them as they are, and the compositor is asked for the animation they make.
//!
//! This file has one test because it changes the process-wide animation settings, which every
//! other test that asks for the look would see.

use color_eyre::eyre;
use komorebi::animation::ANIMATION_FPS;
use komorebi::animation::movement_animation;
use komorebi::animation::movement_style;
use komorebi::backend::fake::FakeBackend;
use komorebi::backend::with_backend;
use komorebi::core::animation::AnimationStyle;
use komorebi::static_config::StaticConfig;
use komorebi::static_config::StaticConfigExt;
use komorebi::window_manager::WindowManager;
use parking_lot::Mutex;
use std::os::unix::net::UnixListener;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::Ordering;

fn write_config(name: &str, json: &str) -> PathBuf {
    let path = PathBuf::from(format!("/tmp/kmb-anim-{name}-{}.json", std::process::id()));
    std::fs::write(&path, json).unwrap();
    path
}

#[test]
fn the_animation_key_of_the_file_is_the_animation_settings() {
    let fake = Arc::new(FakeBackend::with_one_monitor());

    let path = write_config(
        "all",
        r#"{ "animation": { "enabled": true, "duration": 120, "style": "EaseOutExpo", "fps": 30 } }"#,
    );
    let (_sender, receiver) = crossbeam_channel::bounded(1);
    let socket = PathBuf::from(format!("/tmp/kmb-anim-config-{}.sock", std::process::id()));
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

    // one value for every kind of animation
    assert_eq!(movement_animation(), Some(120));
    assert_eq!(movement_style(), AnimationStyle::EaseOutExpo);
    assert_eq!(ANIMATION_FPS.load(Ordering::SeqCst), 30);

    // a value for the kinds a file names wins over the global ones, which a global value takes away
    let by_kind = write_config(
        "kind",
        r#"{ "animation": { "enabled": { "movement": false }, "duration": { "movement": 500 },
                            "style": { "movement": "EaseInOutSine" } } }"#,
    );
    with_backend(fake.clone(), || {
        StaticConfig::reload(&by_kind, &mut wm.lock()).unwrap();
    });
    assert_eq!(movement_animation(), None, "movement is off");
    assert_eq!(movement_style(), AnimationStyle::EaseInOutSine);
    assert_eq!(
        ANIMATION_FPS.load(Ordering::SeqCst),
        60,
        "a file that leaves fps out has the default"
    );

    let on = write_config(
        "on",
        r#"{ "animation": { "enabled": { "movement": true }, "duration": 90, "style": "Linear" } }"#,
    );
    with_backend(fake.clone(), || {
        StaticConfig::reload(&on, &mut wm.lock()).unwrap();
    });
    assert_eq!(movement_animation(), Some(90), "a global duration wins");
    assert_eq!(
        movement_style(),
        AnimationStyle::Linear,
        "and so does a global style"
    );

    // a file that leaves the key out, or the duration and the style, leaves the settings as they are
    let empty = write_config("empty", "{}");
    with_backend(fake.clone(), || {
        StaticConfig::reload(&empty, &mut wm.lock()).unwrap();
    });
    assert_eq!(movement_animation(), Some(90));
    let no_duration = write_config("no-duration", r#"{ "animation": { "enabled": true } }"#);
    with_backend(fake.clone(), || {
        StaticConfig::reload(&no_duration, &mut wm.lock()).unwrap();
    });
    assert_eq!(movement_animation(), Some(90));
    assert_eq!(movement_style(), AnimationStyle::Linear);

    // `enabled` is the one key the file has to give
    assert!(StaticConfig::read_raw(r#"{ "animation": { "duration": 100 } }"#).is_err());

    for file in [path, by_kind, on, empty, no_duration] {
        let _ = std::fs::remove_file(file);
    }
}
