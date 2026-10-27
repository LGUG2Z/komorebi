//! The messages that set the animation are carried out, and the compositor is asked for the
//! animation they make.
//!
//! This file has one test because it changes the process-wide animation settings, which every
//! other test that asks for the look would see.

use komorebi::animation::ANIMATION_FPS;
use komorebi::animation::AnimationPrefix;
use komorebi::backend::AnimationLook;
use komorebi::backend::fake::Command;
use komorebi::backend::fake::FakeBackend;
use komorebi::backend::with_backend;
use komorebi::core::SocketMessage;
use komorebi::core::animation::AnimationStyle;
use komorebi::window_manager::WindowManager;
use std::sync::Arc;
use std::sync::atomic::Ordering;

#[test]
fn the_animation_messages_set_the_animation_and_the_compositor_is_asked_for_it() {
    let fake = Arc::new(FakeBackend::with_one_monitor());

    let (_sender, receiver) = crossbeam_channel::bounded(1);
    let socket = std::path::PathBuf::from(format!("/tmp/kmb-anim-{}.sock", std::process::id()));
    let mut wm = with_backend(fake.clone(), || {
        let mut wm = WindowManager::new(receiver, Some(socket)).unwrap();
        wm.init().unwrap();
        wm
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
    let animated = |duration_ms, curve| Some(AnimationLook { duration_ms, curve });
    let movement = Some(AnimationPrefix::Movement);

    // nothing is animated until it is asked for, and then for the default time
    send(&mut wm, SocketMessage::BorderWidth(10));
    assert_eq!(animation(), None);
    send(&mut wm, SocketMessage::Animation(true, None));
    assert_eq!(animation(), animated(250, "Linear"));

    send(&mut wm, SocketMessage::AnimationDuration(120, None));
    assert_eq!(animation(), animated(120, "Linear"));

    // the easing is a Qt curve: the style without its "Ease"
    send(
        &mut wm,
        SocketMessage::AnimationStyle(AnimationStyle::EaseOutExpo, None),
    );
    assert_eq!(animation(), animated(120, "OutExpo"));

    // the settings of one kind of animation win over the global ones
    send(&mut wm, SocketMessage::Animation(false, movement));
    assert_eq!(animation(), None);
    send(&mut wm, SocketMessage::AnimationDuration(500, movement));
    assert_eq!(animation(), None);
    send(
        &mut wm,
        SocketMessage::AnimationStyle(AnimationStyle::EaseInSine, movement),
    );
    send(&mut wm, SocketMessage::Animation(true, movement));
    assert_eq!(animation(), animated(500, "InSine"));

    // and a global setting takes the ones of the kinds away
    send(&mut wm, SocketMessage::AnimationDuration(90, None));
    assert_eq!(animation(), animated(90, "InSine"));
    send(
        &mut wm,
        SocketMessage::AnimationStyle(AnimationStyle::Linear, None),
    );
    assert_eq!(animation(), animated(90, "Linear"));
    send(&mut wm, SocketMessage::Animation(false, None));
    assert_eq!(animation(), None);

    // the frame rate is kept, and asks nothing of the compositor
    let asked = fake.commands().len();
    send(&mut wm, SocketMessage::AnimationFps(30));
    assert_eq!(ANIMATION_FPS.load(Ordering::SeqCst), 30);
    assert_eq!(fake.commands().len(), asked);
}
