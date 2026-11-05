//! The `border_colours` and `theme` keys of `komorebi.json` are the border colors: the file sets
//! the colors it names when the daemon starts and when it is read again, a color it leaves out is
//! left as it is, a theme wins over the colors, and the colors come out again in the configuration
//! the daemon generates.
//!
//! This file has one test because it changes the process-wide border colors, which every other test
//! that asks for the look would see.

use color_eyre::eyre;
use komorebi::backend::fake::FakeBackend;
use komorebi::backend::with_backend;
use komorebi::border_manager;
use komorebi::static_config::StaticConfig;
use komorebi::static_config::StaticConfigExt;
use komorebi::window_manager::WindowManager;
use komorebi_themes::colour::Colour;
use komorebi_themes::colour::Rgb;
use parking_lot::Mutex;
use std::os::unix::net::UnixListener;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::Ordering;

fn write_config(name: &str, json: &str) -> PathBuf {
    let path = PathBuf::from(format!("/tmp/kmb-theme-{name}-{}.json", std::process::id()));
    std::fs::write(&path, json).unwrap();
    path
}

#[test]
fn the_colour_and_theme_keys_of_the_file_are_the_border_colours() {
    let fake = Arc::new(FakeBackend::with_one_monitor());
    let colour = |slot: &std::sync::atomic::AtomicU32| slot.load(Ordering::SeqCst);

    let path = write_config(
        "colours",
        r##"{
            "border_colours": {
                "single": { "r": 1, "g": 2, "b": 3 },
                "stack": "#040506"
            }
        }"##,
    );
    let (_sender, receiver) = crossbeam_channel::bounded(1);
    let socket = PathBuf::from(format!("/tmp/kmb-theme-config-{}.sock", std::process::id()));
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

    // the colors the file names are the border's, in red, green and blue; the others are as they were
    assert_eq!(colour(&border_manager::FOCUSED), 0x03_02_01);
    assert_eq!(colour(&border_manager::STACK), 0x06_05_04);
    assert_eq!(colour(&border_manager::UNFOCUSED), 0x80_80_80);

    // and they come out again in the generated configuration
    let generated = StaticConfig::from(&*wm.lock()).border_colours.unwrap();
    assert_eq!(generated.single, Some(Colour::Rgb(Rgb::new(1, 2, 3))));
    assert_eq!(generated.stack, Some(Colour::Rgb(Rgb::new(4, 5, 6))));

    // the old name of the key is the same key
    let aliased = StaticConfig::read_raw(
        r#"{ "active_window_border_colours": { "floating": { "r": 7, "g": 8, "b": 9 } } }"#,
    )
    .unwrap();
    assert_eq!(
        aliased.border_colours.unwrap().floating,
        Some(Colour::Rgb(Rgb::new(7, 8, 9)))
    );

    // a theme wins over the colors that come with it
    let themed = write_config(
        "themed",
        r#"{
            "border_colours": { "single": { "r": 9, "g": 9, "b": 9 } },
            "theme": { "palette": "Catppuccin", "name": "Mocha" }
        }"#,
    );
    with_backend(fake.clone(), || {
        StaticConfig::reload(&themed, &mut wm.lock()).unwrap();
    });
    assert_eq!(
        colour(&border_manager::FOCUSED),
        0xFA_B4_89,
        "Mocha's blue, and not the 9 9 9 of the file"
    );

    // and a file with neither leaves the colors as they are
    let empty = write_config("empty", "{}");
    with_backend(fake.clone(), || {
        StaticConfig::reload(&empty, &mut wm.lock()).unwrap();
    });
    assert_eq!(colour(&border_manager::FOCUSED), 0xFA_B4_89);

    for file in [path, themed, empty] {
        let _ = std::fs::remove_file(file);
    }
}
