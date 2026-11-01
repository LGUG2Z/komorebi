//! Wallpapers, and the themes made from them.
//!
//! A monitor or a workspace can have a wallpaper; it is set when the workspace comes into view,
//! however it came (komorebi's own switch, or a pager's), and a palette made from it gives the
//! borders their colors. The colors are process-wide, so this file is one binary and its tests
//! take turns.

use color_eyre::eyre;
use komorebi::backend::fake::Command;
use komorebi::backend::fake::FakeBackend;
use komorebi::backend::with_backend;
use komorebi::border_manager;
use komorebi::core::SocketMessage;
use komorebi::static_config::StaticConfig;
use komorebi::static_config::StaticConfigExt;
use komorebi::static_config::ThemeOptions;
use komorebi::static_config::Wallpaper;
use komorebi::theme_manager;
use komorebi::window_manager::WindowManager;
use komorebi::window_manager_event::KWinNotification;
use komorebi::window_manager_event::SystemNotification;
use komorebi::window_manager_event::WindowManagerEvent;
use komorebi_themes::ThemeVariant;
use parking_lot::Mutex;
use std::os::unix::net::UnixListener;
use std::path::Path;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::Ordering;

const MONITOR: u32 = 1;

static TURN: Mutex<()> = Mutex::new(());

/// Waits for the turn of a test, and makes the palettes it keeps go to a directory of this run's.
///
/// A palette is kept in the user's cache directory, which a build in a sandbox has no right to
/// make, and the tests that keep one would otherwise leave a file in the cache of whoever runs
/// them. The environment is only touched by the test that has the turn, and no other thread of
/// this binary reads it.
fn take_turn() -> parking_lot::MutexGuard<'static, ()> {
    let turn = TURN.lock();
    let cache = std::env::temp_dir().join(format!("kmf-wallpaper-cache-{}", std::process::id()));
    // SAFETY: see above
    unsafe { std::env::set_var("XDG_CACHE_HOME", cache) };
    turn
}

// ---- pictures ---------------------------------------------------------------------------------

fn crc32(bytes: &[u8]) -> u32 {
    let mut crc = 0xFFFF_FFFFu32;
    for &b in bytes {
        crc ^= u32::from(b);
        for _ in 0..8 {
            crc = if crc & 1 == 1 {
                (crc >> 1) ^ 0xEDB8_8320
            } else {
                crc >> 1
            };
        }
    }
    !crc
}

fn adler32(bytes: &[u8]) -> u32 {
    let (mut a, mut b) = (1u32, 0u32);
    for &x in bytes {
        a = (a + u32::from(x)) % 65521;
        b = (b + a) % 65521;
    }
    (b << 16) | a
}

/// A PNG of `width` by `height`, each pixel from `pixel(x, y)`, with the data stored, not packed:
/// the tests need a picture, not a good one
fn write_png(path: &Path, width: u32, height: u32, pixel: impl Fn(u32, u32) -> [u8; 3]) {
    let mut raw = Vec::new();
    for y in 0..height {
        raw.push(0);
        for x in 0..width {
            raw.extend_from_slice(&pixel(x, y));
        }
    }

    let mut zlib = vec![0x78, 0x01];
    let mut chunks = raw.chunks(65_535).peekable();
    while let Some(chunk) = chunks.next() {
        zlib.push(u8::from(chunks.peek().is_none()));
        zlib.extend_from_slice(&(chunk.len() as u16).to_le_bytes());
        zlib.extend_from_slice(&(!(chunk.len() as u16)).to_le_bytes());
        zlib.extend_from_slice(chunk);
    }
    zlib.extend_from_slice(&adler32(&raw).to_be_bytes());

    let chunk = |kind: &[u8; 4], data: &[u8]| {
        let mut out = (data.len() as u32).to_be_bytes().to_vec();
        out.extend_from_slice(kind);
        out.extend_from_slice(data);
        out.extend_from_slice(&crc32(&out[4..]).to_be_bytes());
        out
    };

    let mut header = Vec::new();
    header.extend_from_slice(&width.to_be_bytes());
    header.extend_from_slice(&height.to_be_bytes());
    header.extend_from_slice(&[8, 2, 0, 0, 0]);

    let mut png = b"\x89PNG\r\n\x1a\n".to_vec();
    png.extend(chunk(b"IHDR", &header));
    png.extend(chunk(b"IDAT", &zlib));
    png.extend(chunk(b"IEND", &[]));
    std::fs::write(path, png).unwrap();
}

/// Pictures that are warm, and cool. The palette is made of at least eight colors of the picture,
/// so each has a dozen of them, in bands.
struct Pictures {
    dir: PathBuf,
    warm: PathBuf,
    cool: PathBuf,
}

impl Pictures {
    fn new(tag: &str) -> Self {
        let dir = std::env::temp_dir().join(format!("kmf-wallpaper-{tag}-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let (warm, cool) = (dir.join("warm.png"), dir.join("cool.png"));

        // red to yellow, and blue to cyan, going dark and light
        let band = |hue: [u8; 3], i: u32| {
            let shade = 40 + (i % 4) * 55;
            hue.map(|c| ((u32::from(c) * shade) / 255).min(255) as u8)
        };
        write_png(&warm, 96, 96, |x, y| {
            let i = (x / 8 + y / 8) % 12;
            band([255, (i * 20).min(255) as u8, 20], i)
        });
        write_png(&cool, 96, 96, |x, y| {
            let i = (x / 8 + y / 8) % 12;
            band([20, (80 + i * 14).min(255) as u8, 255], i)
        });

        Self { dir, warm, cool }
    }
}

impl Drop for Pictures {
    fn drop(&mut self) {
        for path in [&self.warm, &self.cool] {
            let _ = std::fs::remove_file(theme_manager::palette_file(path, ThemeVariant::Dark));
            let _ = std::fs::remove_file(theme_manager::palette_file(path, ThemeVariant::Light));
        }
        let _ = std::fs::remove_dir_all(&self.dir);
        if let Some(cache) = theme_manager::palette_file(&self.warm, ThemeVariant::Dark).parent() {
            let _ = std::fs::remove_dir(cache);
        }
    }
}

fn wallpaper(path: &Path, generate_theme: Option<bool>) -> Wallpaper {
    Wallpaper {
        path: path.to_path_buf(),
        generate_theme,
        theme_options: None,
    }
}

// ---- the desktop ------------------------------------------------------------------------------

fn desktop() -> (Arc<FakeBackend>, WindowManager) {
    let fake = Arc::new(FakeBackend::with_one_monitor());
    let (_sender, receiver) = crossbeam_channel::bounded(1);
    let socket = PathBuf::from(format!("/tmp/kmf-wallpaper-{}.sock", std::process::id()));

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

fn workspace_changed(idx: usize) -> WindowManagerEvent {
    WindowManagerEvent::WorkspaceChange(
        SystemNotification::KWin(KWinNotification::WorkspaceChanged),
        MONITOR,
        idx,
    )
}

fn focused_border() -> u32 {
    border_manager::FOCUSED.load(Ordering::SeqCst)
}

// ---- tests ------------------------------------------------------------------------------------

#[test]
fn a_wallpaper_is_set_when_its_workspace_comes_into_view_by_whoever_brings_it() {
    let _turn = take_turn();
    let pictures = Pictures::new("set");
    let (fake, mut wm) = desktop();

    // the monitor has the warm one, and workspace 1 has the cool one of its own
    let monitor = wm.monitors_mut().get_mut(0).unwrap();
    assert_eq!(monitor.id, MONITOR);
    monitor.wallpaper = Some(wallpaper(&pictures.warm, Some(false)));
    monitor.ensure_workspace_count(3);
    monitor.workspaces_mut()[1].wallpaper = Some(wallpaper(&pictures.cool, Some(false)));

    // komorebi's own switch
    command(&fake, &mut wm, SocketMessage::FocusWorkspaceNumber(1));
    assert_eq!(fake.wallpaper_of(MONITOR), Some(pictures.cool.clone()));

    // back to a workspace that has none of its own: the monitor's
    command(&fake, &mut wm, SocketMessage::FocusWorkspaceNumber(0));
    assert_eq!(fake.wallpaper_of(MONITOR), Some(pictures.warm.clone()));

    // and a switch somebody else made (a pager, a shortcut) comes as an event
    with_backend(fake.clone(), || wm.process_event(workspace_changed(1))).unwrap();
    assert_eq!(fake.wallpaper_of(MONITOR), Some(pictures.cool.clone()));

    with_backend(fake.clone(), || wm.process_event(workspace_changed(2))).unwrap();
    assert_eq!(
        fake.wallpaper_of(MONITOR),
        Some(pictures.warm.clone()),
        "a workspace with none of its own has the monitor's"
    );
}

#[test]
fn no_wallpaper_is_touched_that_was_not_asked_for() {
    let _turn = take_turn();
    let (fake, mut wm) = desktop();
    wm.monitors_mut()[0].ensure_workspace_count(2);

    command(&fake, &mut wm, SocketMessage::FocusWorkspaceNumber(1));
    command(&fake, &mut wm, SocketMessage::FocusWorkspaceNumber(0));

    assert!(
        !fake
            .commands()
            .iter()
            .any(|c| matches!(c, Command::SetWallpaper(..))),
        "{:?}",
        fake.commands()
    );
}

#[test]
fn the_borders_take_their_colours_from_the_wallpaper_and_follow_it() {
    let _turn = take_turn();
    let pictures = Pictures::new("theme");
    let (fake, mut wm) = desktop();

    let monitor = wm.monitors_mut().get_mut(0).unwrap();
    monitor.ensure_workspace_count(2);
    monitor.workspaces_mut()[0].wallpaper = Some(wallpaper(&pictures.warm, None));
    monitor.workspaces_mut()[1].wallpaper = Some(wallpaper(&pictures.cool, None));

    // what an earlier test left behind is not what this one starts from
    border_manager::FOCUSED.store(0x0001_0203, Ordering::SeqCst);
    let before = focused_border();

    // what starting up does: the wallpaper of the workspace that is in front is set
    with_backend(fake.clone(), || wm.retile_all(false)).unwrap();
    let warm = focused_border();
    assert_eq!(fake.wallpaper_of(MONITOR), Some(pictures.warm.clone()));
    assert_ne!(
        warm, before,
        "the warm wallpaper did not change the focused border"
    );

    command(&fake, &mut wm, SocketMessage::FocusWorkspaceNumber(1));
    let cool = focused_border();
    assert_ne!(cool, warm, "two different wallpapers gave the same border");

    // a pager's switch brings the first one's colors back
    with_backend(fake.clone(), || wm.process_event(workspace_changed(0))).unwrap();
    assert_eq!(focused_border(), warm);

    // the palette was kept, and is the same when read again
    let palette = theme_manager::palette_file(&pictures.warm, ThemeVariant::Dark);
    assert!(palette.is_file(), "{} was not kept", palette.display());
    command(&fake, &mut wm, SocketMessage::FocusWorkspaceNumber(1));
    command(&fake, &mut wm, SocketMessage::FocusWorkspaceNumber(0));
    assert_eq!(
        focused_border(),
        warm,
        "the cached palette gave other colors"
    );
}

#[test]
fn a_wallpaper_that_asks_for_no_theme_leaves_the_borders_alone() {
    let _turn = take_turn();
    let pictures = Pictures::new("notheme");
    let (fake, mut wm) = desktop();

    wm.monitors_mut()[0].workspaces_mut()[0].wallpaper =
        Some(wallpaper(&pictures.cool, Some(false)));
    let before = focused_border();

    with_backend(fake.clone(), || wm.retile_all(false)).unwrap();

    assert_eq!(fake.wallpaper_of(MONITOR), Some(pictures.cool.clone()));
    assert_eq!(focused_border(), before);
}

#[test]
fn a_wallpaper_that_is_not_a_picture_still_sets_and_does_not_break_the_window_manager() {
    let _turn = take_turn();
    let pictures = Pictures::new("broken");
    let broken = pictures.dir.join("broken.png");
    std::fs::write(&broken, b"this is not a picture").unwrap();
    let (fake, mut wm) = desktop();

    wm.monitors_mut()[0].workspaces_mut()[0].wallpaper = Some(wallpaper(&broken, None));
    let before = focused_border();

    with_backend(fake.clone(), || wm.retile_all(false)).unwrap();

    assert_eq!(fake.wallpaper_of(MONITOR), Some(broken.clone()));
    assert_eq!(focused_border(), before, "no palette, no new colors");
    // and the window manager still answers
    command(&fake, &mut wm, SocketMessage::FocusWorkspaceNumber(0));
}

#[test]
fn theme_options_choose_the_variant_and_the_colour_each_border_takes() {
    let _turn = take_turn();
    let pictures = Pictures::new("options");

    // what the focused border is, with these options, once the wallpaper is in front
    let focused_with = |options: Option<ThemeOptions>| {
        let (fake, mut wm) = desktop();
        wm.monitors_mut()[0].workspaces_mut()[0].wallpaper = Some(Wallpaper {
            path: pictures.warm.clone(),
            generate_theme: None,
            theme_options: options,
        });
        with_backend(fake.clone(), || wm.retile_all(false)).unwrap();
        focused_border()
    };
    let options = |variant, single| ThemeOptions {
        theme_variant: variant,
        single_border: single,
        stack_border: None,
        monocle_border: None,
        floating_border: None,
        unfocused_border: None,
        unfocused_locked_border: None,
        bar_accent: None,
        stackbar_focused_text: None,
        stackbar_unfocused_text: None,
        stackbar_background: None,
    };

    let by_default = focused_with(None);
    let another_colour = focused_with(Some(options(
        None,
        Some(komorebi_themes::Base16Value::Base08),
    )));
    let light = focused_with(Some(options(Some(ThemeVariant::Light), None)));

    assert_ne!(
        another_colour, by_default,
        "asking for another color of the palette changed nothing"
    );
    assert_ne!(light, by_default, "the light variant is the dark one");
    assert_eq!(
        by_default,
        focused_with(Some(options(Some(ThemeVariant::Dark), None)))
    );
}

#[test]
fn the_wallpaper_example_in_the_docs_loads() {
    let docs = Path::new(env!("CARGO_MANIFEST_DIR")).join("../docs/linux/usage/wallpapers.md");
    let docs = std::fs::read_to_string(docs).unwrap();
    let json = docs
        .split("```json\n")
        .nth(1)
        .and_then(|rest| rest.split("```").next())
        .expect("docs/usage/wallpapers.md has no json example");

    let config: StaticConfig = serde_json::from_str(json).unwrap();
    let monitor = &config.monitors.unwrap()[0];

    let on_monitor = monitor.wallpaper.as_ref().expect("the monitor's wallpaper");
    assert!(on_monitor.path.ends_with("Pictures/dunes.png"));
    assert!(
        !on_monitor.path.starts_with("~"),
        "the home directory was not resolved: {:?}",
        on_monitor.path
    );

    let chat = monitor.workspaces[1]
        .wallpaper
        .as_ref()
        .expect("a workspace's wallpaper");
    let options = chat.theme_options.as_ref().unwrap();
    assert_eq!(options.theme_variant, Some(ThemeVariant::Light));
    assert_eq!(
        options.single_border,
        Some(komorebi_themes::Base16Value::Base09)
    );
}

#[test]
fn the_wallpapers_of_the_file_are_on_the_monitor_and_its_workspaces_and_a_reload_shows_them() {
    let _turn = take_turn();
    let pictures = Pictures::new("file");
    let fake = Arc::new(FakeBackend::with_one_monitor());

    let file = |first: &Path, second: &Path| {
        format!(
            r#"{{ "monitors": [ {{
                 "wallpaper": {{ "path": {first:?}, "generate_theme": false }},
                 "workspaces": [
                   {{ "name": "code" }},
                   {{ "name": "chat", "wallpaper": {{ "path": {second:?}, "generate_theme": false }} }}
                 ] }} ] }}"#
        )
    };
    let path = PathBuf::from(format!(
        "/tmp/kmf-wallpaper-file-{}.json",
        std::process::id()
    ));
    std::fs::write(&path, file(&pictures.warm, &pictures.cool)).unwrap();

    let (_sender, receiver) = crossbeam_channel::bounded(1);
    let socket = PathBuf::from(format!(
        "/tmp/kmf-wallpaper-file-{}.sock",
        std::process::id()
    ));
    let listener = UnixListener::bind(&socket).unwrap();
    let wm = with_backend(fake.clone(), || -> eyre::Result<WindowManager> {
        let mut wm = StaticConfig::preload(&path, receiver, Some(listener))?;
        wm.init()?;
        Ok(wm)
    })
    .unwrap();
    let wm = Arc::new(parking_lot::Mutex::new(wm));
    with_backend(fake.clone(), || StaticConfig::postload(&path, &wm)).unwrap();
    let _ = std::fs::remove_file(&socket);

    {
        let wm = wm.lock();
        let monitor = &wm.monitors()[0];
        assert_eq!(monitor.workspaces().len(), 2);
        assert_eq!(
            monitor.wallpaper.as_ref().map(|w| w.path.clone()),
            Some(pictures.warm.clone())
        );
        assert!(monitor.workspaces()[0].wallpaper.is_none());
        assert_eq!(
            monitor.workspaces()[1]
                .wallpaper
                .as_ref()
                .map(|w| w.path.clone()),
            Some(pictures.cool.clone())
        );
    }

    // the first layout shows the monitor's wallpaper, and the workspace's own shows with it
    with_backend(fake.clone(), || wm.lock().retile_all(false)).unwrap();
    assert_eq!(fake.wallpaper_of(MONITOR), Some(pictures.warm.clone()));
    command(
        &fake,
        &mut wm.lock(),
        SocketMessage::FocusWorkspaceNumber(1),
    );
    assert_eq!(fake.wallpaper_of(MONITOR), Some(pictures.cool.clone()));

    // the file is read again with the pictures the other way round: the workspace in front shows
    // its new one at once
    std::fs::write(&path, file(&pictures.cool, &pictures.warm)).unwrap();
    with_backend(fake.clone(), || {
        StaticConfig::reload(&path, &mut wm.lock()).unwrap();
    });
    assert_eq!(fake.wallpaper_of(MONITOR), Some(pictures.warm.clone()));
    let _ = std::fs::remove_file(&path);

    // the configuration the daemon generates has the monitor's wallpaper
    let generated = StaticConfig::from(&*wm.lock());
    let monitor = &generated.monitors.unwrap()[0];
    assert_eq!(
        monitor.wallpaper.as_ref().map(|w| w.path.clone()),
        Some(pictures.cool.clone())
    );
}

#[test]
fn the_wallpapers_are_in_the_state() {
    let _turn = take_turn();
    let (_fake, mut wm) = desktop();

    let monitor = wm.monitors_mut().get_mut(0).unwrap();
    monitor.wallpaper = Some(wallpaper(Path::new("/pictures/dunes.png"), None));
    monitor.ensure_workspace_count(2);
    monitor.workspaces_mut()[1].wallpaper = Some(wallpaper(Path::new("/pictures/sea.png"), None));

    let state = serde_json::to_value(komorebi::state::State::from(&wm)).unwrap();
    let monitor = &state["monitors"]["elements"][0];

    assert_eq!(monitor["wallpaper"]["path"], "/pictures/dunes.png");
    assert_eq!(
        monitor["workspaces"]["elements"][0]["wallpaper"],
        serde_json::Value::Null
    );
    assert_eq!(
        monitor["workspaces"]["elements"][1]["wallpaper"]["path"],
        "/pictures/sea.png"
    );
}
