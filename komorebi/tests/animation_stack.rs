//! A command that changes a stack is carried out with the animation off, and the animation is on
//! again when it is done: the compositor is asked for it as it was before. The two commands that
//! ask for the look while they run, the ones that take a window out of a stack, show it.
//!
//! This file has one test because it changes the process-wide animation settings, which every
//! other test that asks for the look would see.

use komorebi::animation::ANIMATION_ENABLED_GLOBAL;
use komorebi::backend::AnimationLook;
use komorebi::backend::fake::Command;
use komorebi::backend::fake::FakeBackend;
use komorebi::backend::with_backend;
use komorebi::core::CycleDirection;
use komorebi::core::OperationDirection;
use komorebi::core::SocketMessage;
use komorebi::window::Window;
use komorebi::window_manager::WindowManager;
use komorebi::window_manager_event::KWinNotification;
use komorebi::window_manager_event::SystemNotification;
use komorebi::window_manager_event::WindowManagerEvent;
use std::sync::Arc;
use std::sync::atomic::Ordering;

#[test]
fn the_animation_is_on_again_after_a_command_that_changes_a_stack() {
    let fake = Arc::new(FakeBackend::with_one_monitor());

    let (_sender, receiver) = crossbeam_channel::bounded(1);
    let socket =
        std::path::PathBuf::from(format!("/tmp/kmb-anim-stack-{}.sock", std::process::id()));
    let mut wm = with_backend(fake.clone(), || {
        let mut wm = WindowManager::new(receiver, Some(socket)).unwrap();
        wm.init().unwrap();
        wm
    });
    with_backend(fake.clone(), || {
        for id in 45_001..45_004 {
            fake.open_window(id, "foot", "foot", &format!("terminal {id}"));
            wm.process_event(WindowManagerEvent::Show(
                SystemNotification::KWin(KWinNotification::WindowAdded),
                Window::from(id),
            ))
            .unwrap();
        }
    });
    let send = |wm: &mut WindowManager, message| {
        with_backend(fake.clone(), || wm.process_command(message, Vec::new())).unwrap();
    };
    // what the compositor was last asked to animate
    let animation = || {
        fake.commands()
            .into_iter()
            .rev()
            .find_map(|command| match command {
                Command::ApplyLook(look) => Some(look.animation),
                _ => None,
            })
            .expect("the compositor was asked for a look")
    };

    send(&mut wm, SocketMessage::Animation(true, None));
    assert_eq!(animation(), Some(AnimationLook { duration_ms: 250 }));

    // every look the compositor is asked for while the command runs, as what it animates
    let looks = || {
        fake.commands()
            .into_iter()
            .filter_map(|command| match command {
                Command::ApplyLook(look) => Some(look.animation),
                _ => None,
            })
            .collect::<Vec<_>>()
    };
    let on = Some(AnimationLook { duration_ms: 250 });

    for (message, asked) in [
        (SocketMessage::StackWindow(OperationDirection::Left), vec![]),
        (SocketMessage::CycleStack(CycleDirection::Next), vec![]),
        (SocketMessage::CycleStackIndex(CycleDirection::Next), vec![]),
        (SocketMessage::FocusStackWindow(0), vec![]),
        (SocketMessage::UnstackWindow, vec![None, on.clone()]),
        (SocketMessage::StackAll, vec![]),
        (SocketMessage::UnstackAll, vec![None, on.clone()]),
    ] {
        fake.clear_commands();
        send(&mut wm, message.clone());
        assert!(
            ANIMATION_ENABLED_GLOBAL.load(Ordering::SeqCst),
            "the animation is on again after {message:?}"
        );
        assert_eq!(
            looks(),
            asked,
            "the compositor is asked for the look with the animation off while {message:?} runs"
        );
    }
}
