use std::sync::Arc;

use parking_lot::Mutex;
use winput::message_loop;
use winput::message_loop::Event;
use winput::Action;

use crate::window_manager::WindowManager;

#[tracing::instrument]
pub fn listen_for_movements(wm: Arc<Mutex<WindowManager>>) {
    std::thread::spawn(move || {
        let mut ignore_movement = false;

        let receiver = message_loop::start().expect("could not start winput message loop");

        loop {
            match receiver.next_event() {
                // Don't want to send any raise events while we are dragging or resizing
                Event::MouseButton { action, .. } => match action {
                    Action::Press => ignore_movement = true,
                    Action::Release => ignore_movement = false,
                },
                Event::MouseMoveRelative { .. } => {
                    if !ignore_movement {
                        match wm.lock().raise_window_at_cursor_pos() {
                            Ok(_) => {}
                            Err(error) => tracing::error!("{}", error),
                        }
                    }
                }
                _ => {}
            }
        }
    });
}
