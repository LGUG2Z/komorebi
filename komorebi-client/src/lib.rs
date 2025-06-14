#![warn(clippy::all)]
#![allow(clippy::missing_errors_doc)]

pub use komorebi::animation::prefix::AnimationPrefix;
pub use komorebi::animation::PerAnimationPrefixConfig;
pub use komorebi::asc::ApplicationSpecificConfiguration;
pub use komorebi::border_manager::BorderInfo;
pub use komorebi::config_generation::ApplicationConfiguration;
pub use komorebi::config_generation::IdWithIdentifier;
pub use komorebi::config_generation::IdWithIdentifierAndComment;
pub use komorebi::config_generation::MatchingRule;
pub use komorebi::config_generation::MatchingStrategy;
pub use komorebi::container::Container;
pub use komorebi::core::config_generation::ApplicationConfigurationGenerator;
pub use komorebi::core::replace_env_in_path;
pub use komorebi::core::AnimationStyle;
pub use komorebi::core::ApplicationIdentifier;
pub use komorebi::core::Arrangement;
pub use komorebi::core::Axis;
pub use komorebi::core::BorderImplementation;
pub use komorebi::core::BorderStyle;
pub use komorebi::core::Column;
pub use komorebi::core::ColumnSplit;
pub use komorebi::core::ColumnSplitWithCapacity;
pub use komorebi::core::ColumnWidth;
pub use komorebi::core::CustomLayout;
pub use komorebi::core::CycleDirection;
pub use komorebi::core::DefaultLayout;
pub use komorebi::core::Direction;
pub use komorebi::core::FloatingLayerBehaviour;
pub use komorebi::core::FocusFollowsMouseImplementation;
pub use komorebi::core::HidingBehaviour;
pub use komorebi::core::Layout;
pub use komorebi::core::MoveBehaviour;
pub use komorebi::core::OperationBehaviour;
pub use komorebi::core::OperationDirection;
pub use komorebi::core::PathExt;
pub use komorebi::core::Rect;
pub use komorebi::core::Sizing;
pub use komorebi::core::SocketMessage;
pub use komorebi::core::StackbarLabel;
pub use komorebi::core::StackbarMode;
pub use komorebi::core::StateQuery;
pub use komorebi::core::WindowKind;
pub use komorebi::monitor::Monitor;
pub use komorebi::monitor_reconciliator::MonitorNotification;
pub use komorebi::ring::Ring;
pub use komorebi::win32_display_data;
pub use komorebi::window::Window;
pub use komorebi::window_manager_event::WindowManagerEvent;
pub use komorebi::workspace::Workspace;
pub use komorebi::workspace::WorkspaceGlobals;
pub use komorebi::workspace::WorkspaceLayer;
pub use komorebi::AnimationsConfig;
pub use komorebi::AppSpecificConfigurationPath;
pub use komorebi::AspectRatio;
pub use komorebi::BorderColours;
pub use komorebi::Colour;
pub use komorebi::CrossBoundaryBehaviour;
pub use komorebi::GlobalState;
pub use komorebi::KomorebiTheme;
pub use komorebi::MonitorConfig;
pub use komorebi::Notification;
pub use komorebi::NotificationEvent;
pub use komorebi::PredefinedAspectRatio;
pub use komorebi::Rgb;
pub use komorebi::RuleDebug;
pub use komorebi::StackbarConfig;
pub use komorebi::State;
pub use komorebi::StaticConfig;
pub use komorebi::SubscribeOptions;
pub use komorebi::TabsConfig;
pub use komorebi::VirtualDesktopNotification;
pub use komorebi::WindowContainerBehaviour;
pub use komorebi::WindowsApi;
pub use komorebi::WorkspaceConfig;

use komorebi::DATA_DIR;

use std::borrow::Borrow;
use std::io::BufReader;
use std::io::Read;
use std::io::Write;
use std::net::Shutdown;
use std::time::Duration;
pub use uds_windows::UnixListener;
use uds_windows::UnixStream;

const KOMOREBI: &str = "komorebi.sock";

pub fn send_message(message: &SocketMessage) -> std::io::Result<()> {
    let socket = DATA_DIR.join(KOMOREBI);
    let mut stream = UnixStream::connect(socket)?;
    stream.set_write_timeout(Some(Duration::from_secs(1)))?;
    stream.write_all(serde_json::to_string(message)?.as_bytes())
}

pub fn send_batch<Q>(messages: impl IntoIterator<Item = Q>) -> std::io::Result<()>
where
    Q: Borrow<SocketMessage>,
{
    let socket = DATA_DIR.join(KOMOREBI);
    let mut stream = UnixStream::connect(socket)?;
    stream.set_write_timeout(Some(Duration::from_secs(1)))?;
    let msgs = messages.into_iter().fold(String::new(), |mut s, m| {
        if let Ok(m_str) = serde_json::to_string(m.borrow()) {
            s.push_str(&m_str);
            s.push('\n');
        }
        s
    });
    stream.write_all(msgs.as_bytes())
}

pub fn send_query(message: &SocketMessage) -> std::io::Result<String> {
    let socket = DATA_DIR.join(KOMOREBI);

    let mut stream = UnixStream::connect(socket)?;
    stream.set_read_timeout(Some(Duration::from_secs(1)))?;
    stream.set_write_timeout(Some(Duration::from_secs(1)))?;
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

pub fn subscribe_with_options(
    name: &str,
    options: SubscribeOptions,
) -> std::io::Result<UnixListener> {
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

    send_message(&SocketMessage::AddSubscriberSocketWithOptions(
        name.to_string(),
        options,
    ))?;

    Ok(listener)
}
