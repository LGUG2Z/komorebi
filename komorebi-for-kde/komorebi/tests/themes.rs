//! Named themes, against an in-memory desktop: a theme gives every kind of border its color.
//!
//! Window ids are in the 70_000s. The colors are process-wide, so the tests of this file take
//! turns.

use komorebi::backend::fake::FakeBackend;
use komorebi::backend::with_backend;
use komorebi::border_manager;
use komorebi::core::SocketMessage;
use komorebi::window::Window;
use komorebi::window_manager::WindowManager;
use komorebi::window_manager_event::KWinNotification;
use komorebi::window_manager_event::SystemNotification;
use komorebi::window_manager_event::WindowManagerEvent;
use komorebi_themes::KomorebiTheme;
use komorebi_themes::KomorebiThemeCatppuccin;
use parking_lot::Mutex;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::Ordering;

static TURN: Mutex<()> = Mutex::new(());

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

fn focused_border() -> u32 {
    border_manager::FOCUSED.load(Ordering::SeqCst)
}

#[test]
fn a_named_theme_colours_every_kind_of_border() {
    let _turn = TURN.lock();
    let (fake, mut wm) = desktop();

    fake.open_window(70_001, "foot", "foot", "terminal");
    with_backend(fake.clone(), || {
        wm.process_event(WindowManagerEvent::Show(
            SystemNotification::KWin(KWinNotification::WindowAdded),
            Window::from(70_001),
        ))
    })
    .unwrap();
    border_manager::BORDER_ENABLED.store(true, Ordering::SeqCst);

    command(
        &fake,
        &mut wm,
        SocketMessage::Theme(Box::new(KomorebiTheme::Catppuccin(
            KomorebiThemeCatppuccin {
                name: komorebi_themes::Catppuccin::Mocha,
                bar_accent: None,
                single_border: None,
                stack_border: None,
                monocle_border: None,
                floating_border: None,
                unfocused_border: None,
                unfocused_locked_border: None,
            },
        ))),
    );

    // Mocha's blue, which is the default for the focused window: kept as Windows keeps colours
    // (0x00BBGGRR), and given to the compositor as 0xRRGGBB
    assert_eq!(focused_border(), 0xFAB489);
    assert_eq!(
        fake.border_colour_of(70_001),
        Some(0x89B4FA),
        "{:?}",
        fake.commands()
    );

    border_manager::BORDER_ENABLED.store(false, Ordering::SeqCst);
}
