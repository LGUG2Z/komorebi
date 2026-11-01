//! A message that only other platforms handle is answered, with what it is and that this
//! platform does not do it, and changes nothing: the one `SocketMessage` of every daemon has
//! variants of the other platforms (`komorebi_protocol::platforms`).

use komorebi::backend::fake::FakeBackend;
use komorebi::backend::with_backend;
use komorebi::core::SocketMessage;
use komorebi::window_manager::WindowManager;
use komorebi_protocol::types::StackbarMode;
use std::sync::Arc;

fn desktop() -> (Arc<FakeBackend>, WindowManager) {
    let fake = Arc::new(FakeBackend::with_one_monitor());
    let (_sender, receiver) = crossbeam_channel::bounded(1);
    static NEXT: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);
    let socket = std::path::PathBuf::from(format!(
        "/tmp/kmr-unsupported-{}-{}.sock",
        std::process::id(),
        NEXT.fetch_add(1, std::sync::atomic::Ordering::SeqCst)
    ));
    let wm = with_backend(fake.clone(), || {
        let mut wm = WindowManager::new(receiver, Some(socket)).unwrap();
        wm.init().unwrap();
        wm
    });
    (fake, wm)
}

fn ask(fake: &Arc<FakeBackend>, wm: &mut WindowManager, message: SocketMessage) -> String {
    let mut reply = Vec::new();
    with_backend(fake.clone(), || wm.process_command(message, &mut reply)).unwrap();
    String::from_utf8(reply).unwrap()
}

#[test]
fn a_message_of_another_platform_is_answered_as_not_supported() {
    let (fake, mut wm) = desktop();
    assert_eq!(
        ask(&fake, &mut wm, SocketMessage::ForceFocus),
        "ForceFocus is not supported on this platform"
    );
    assert_eq!(
        ask(
            &fake,
            &mut wm,
            SocketMessage::StackbarMode(StackbarMode::Always)
        ),
        "StackbarMode is not supported on this platform"
    );
    assert_eq!(
        ask(&fake, &mut wm, SocketMessage::ToggleTitleBars),
        "ToggleTitleBars is not supported on this platform"
    );
}

#[test]
fn a_message_this_platform_handles_is_not_answered_as_not_supported() {
    let (fake, mut wm) = desktop();
    assert_eq!(ask(&fake, &mut wm, SocketMessage::Retile), "");
}
