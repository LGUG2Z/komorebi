//! The messages that set the border are carried out, and the compositor is asked for the look they
//! make.
//!
//! This file has one test because it changes the process-wide border settings, which every other
//! test that tiles windows would see.

use komorebi::backend::BorderLook;
use komorebi::backend::Look;
use komorebi::backend::fake::Command;
use komorebi::backend::fake::FakeBackend;
use komorebi::backend::with_backend;
use komorebi::border_manager;
use komorebi::core::SocketMessage;
use komorebi::core::WindowKind;
use komorebi::window_manager::WindowManager;
use std::sync::Arc;
use std::sync::atomic::Ordering;

#[test]
fn the_border_messages_set_the_border_and_the_compositor_is_asked_for_it() {
    let fake = Arc::new(FakeBackend::with_one_monitor());
    fake.set_borders_drawn(true);

    let (_sender, receiver) = crossbeam_channel::bounded(1);
    let socket = std::path::PathBuf::from(format!("/tmp/kmb-bcmd-{}.sock", std::process::id()));
    let mut wm = with_backend(fake.clone(), || {
        let mut wm = WindowManager::new(receiver, Some(socket)).unwrap();
        wm.init().unwrap();
        wm
    });
    let send = |wm: &mut WindowManager, message| {
        with_backend(fake.clone(), || wm.process_command(message, Vec::new())).unwrap();
    };
    let last_look = || {
        fake.commands()
            .into_iter()
            .rev()
            .find_map(|command| match command {
                Command::ApplyLook(look) => Some(look),
                _ => None,
            })
            .expect("the compositor was asked for a look")
    };

    send(&mut wm, SocketMessage::BorderWidth(8));
    send(&mut wm, SocketMessage::BorderOffset(3));
    assert_eq!(border_manager::BORDER_WIDTH.load(Ordering::SeqCst), 8);
    assert_eq!(border_manager::BORDER_OFFSET.load(Ordering::SeqCst), 3);
    assert!(border_manager::BORDER_DRAWN.load(Ordering::SeqCst));
    let border: BorderLook = last_look().border.expect("borders are on");
    assert_eq!(border.width, 8);

    // the colors of the focused window and of the others are in the look, the others are not
    send(
        &mut wm,
        SocketMessage::BorderColour(WindowKind::Single, 10, 20, 30),
    );
    send(
        &mut wm,
        SocketMessage::BorderColour(WindowKind::Unfocused, 40, 50, 60),
    );
    send(
        &mut wm,
        SocketMessage::BorderColour(WindowKind::Floating, 70, 80, 90),
    );
    let border = last_look().border.expect("borders are on");
    assert_eq!(
        (border.focused, border.unfocused),
        ((10, 20, 30), (40, 50, 60))
    );
    assert_eq!(
        border_manager::unpack(border_manager::FLOATING.load(Ordering::SeqCst)),
        (70, 80, 90)
    );

    send(&mut wm, SocketMessage::Border(false));
    assert_eq!(last_look(), Look::default());
    assert!(!border_manager::BORDER_DRAWN.load(Ordering::SeqCst));

    send(&mut wm, SocketMessage::Border(true));
    assert_eq!(last_look().border, Some(border));
    assert!(border_manager::BORDER_DRAWN.load(Ordering::SeqCst));
}
