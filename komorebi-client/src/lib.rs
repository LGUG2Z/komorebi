#![warn(clippy::all)]
#![allow(clippy::missing_errors_doc)]

pub use komorebi::colour::Colour;
pub use komorebi::colour::Rgb;
pub use komorebi::container::Container;
pub use komorebi::core::config_generation::ApplicationConfigurationGenerator;
pub use komorebi::core::resolve_home_path;
pub use komorebi::core::AnimationStyle;
pub use komorebi::core::ApplicationIdentifier;
pub use komorebi::core::Arrangement;
pub use komorebi::core::Axis;
pub use komorebi::core::BorderImplementation;
pub use komorebi::core::BorderStyle;
pub use komorebi::core::CustomLayout;
pub use komorebi::core::CycleDirection;
pub use komorebi::core::DefaultLayout;
pub use komorebi::core::Direction;
pub use komorebi::core::FocusFollowsMouseImplementation;
pub use komorebi::core::HidingBehaviour;
pub use komorebi::core::Layout;
pub use komorebi::core::MoveBehaviour;
pub use komorebi::core::OperationBehaviour;
pub use komorebi::core::OperationDirection;
pub use komorebi::core::Rect;
pub use komorebi::core::Sizing;
pub use komorebi::core::SocketMessage;
pub use komorebi::core::StackbarLabel;
pub use komorebi::core::StackbarMode;
pub use komorebi::core::StateQuery;
pub use komorebi::core::WindowKind;
pub use komorebi::monitor::Monitor;
pub use komorebi::ring::Ring;
pub use komorebi::window::Window;
pub use komorebi::window_manager_event::WindowManagerEvent;
pub use komorebi::workspace::Workspace;
pub use komorebi::BorderColours;
pub use komorebi::GlobalState;
pub use komorebi::Notification;
pub use komorebi::NotificationEvent;
pub use komorebi::RuleDebug;
pub use komorebi::StackbarConfig;
pub use komorebi::State;
pub use komorebi::StaticConfig;
pub use komorebi::TabsConfig;

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
    let mut stream = UnixStream::connect(socket)?;
    stream.write_all(serde_json::to_string(message)?.as_bytes())
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
