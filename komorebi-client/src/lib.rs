#![warn(clippy::all, clippy::nursery, clippy::pedantic)]
#![allow(clippy::missing_errors_doc)]

pub use komorebi::container::Container;
pub use komorebi::monitor::Monitor;
pub use komorebi::ring::Ring;
pub use komorebi::window::Window;
pub use komorebi::window_manager_event::WindowManagerEvent;
pub use komorebi::workspace::Workspace;
pub use komorebi::Notification;
pub use komorebi::NotificationEvent;
pub use komorebi::State;
pub use komorebi_core::Arrangement;
pub use komorebi_core::Axis;
pub use komorebi_core::CustomLayout;
pub use komorebi_core::CycleDirection;
pub use komorebi_core::DefaultLayout;
pub use komorebi_core::Direction;
pub use komorebi_core::Layout;
pub use komorebi_core::OperationDirection;
pub use komorebi_core::Rect;
pub use komorebi_core::SocketMessage;

use komorebi::DATA_DIR;

use std::io::BufReader;
use std::io::Read;
use std::io::Write;
use std::net::Shutdown;
pub use uds_windows::UnixListener;
use uds_windows::UnixStream;

const KOMOREBI: &str = "komorebi.sock";

pub fn send_message(message: &SocketMessage) -> std::io::Result<()> {
    let socket = DATA_DIR.join(KOMOREBI);
    let mut connected = false;
    while !connected {
        if let Ok(mut stream) = UnixStream::connect(&socket) {
            connected = true;
            stream.write_all(serde_json::to_string(message)?.as_bytes())?;
        }
    }

    Ok(())
}
pub fn send_query(message: &SocketMessage) -> std::io::Result<String> {
    let socket = DATA_DIR.join(KOMOREBI);

    let mut stream = UnixStream::connect(socket)?;
    stream.write_all(serde_json::to_string(message)?.as_bytes())?;
    stream.shutdown(Shutdown::Write)?;

    let mut reader = BufReader::new(stream);
    let mut response = String::new();
    reader.read_to_string(&mut response)?;

    Ok(response)
}

pub fn subscribe(name: &str) -> std::io::Result<UnixListener> {
    let socket = DATA_DIR.join(name);

    match std::fs::remove_file(&socket) {
        Ok(()) => {}
        Err(error) => match error.kind() {
            std::io::ErrorKind::NotFound => {}
            _ => {
                return Err(error);
            }
        },
    };

    let listener = UnixListener::bind(&socket)?;

    send_message(&SocketMessage::AddSubscriberSocket(name.to_string()))?;

    Ok(listener)
}
