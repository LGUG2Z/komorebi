pub use komorebi::DATA_DIR;
pub use komorebi::Notification;
pub use komorebi::NotificationEvent;
pub use komorebi::animation::AnimationPrefix;
pub use komorebi::container::Container;
pub use komorebi::core::ApplicationIdentifier;
pub use komorebi::core::Axis;
pub use komorebi::core::CycleDirection;
pub use komorebi::core::DefaultLayout;
pub use komorebi::core::Layout;
pub use komorebi::core::MonocleFocusBehaviour;
pub use komorebi::core::MoveBehaviour;
pub use komorebi::core::OperationBehaviour;
pub use komorebi::core::OperationDirection;
pub use komorebi::core::PanDirection;
pub use komorebi::core::PanGesture;
pub use komorebi::core::Rect;
pub use komorebi::core::Sizing;
pub use komorebi::core::SocketMessage;
pub use komorebi::core::StateQuery;
pub use komorebi::core::SubscribeOptions;
pub use komorebi::core::WindowKind;
pub use komorebi::core::animation::AnimationStyle;
pub use komorebi::core::asc::ApplicationSpecificConfiguration;
pub use komorebi::core::pathext::PathExt;
pub use komorebi::core::pathext::replace_env_in_path;
pub use komorebi::monitor_reconciliator::MonitorNotification;
pub use komorebi::splash;
pub use komorebi::state::State;
pub use komorebi::static_config::StaticConfig;
pub use komorebi::window::Window;
pub use komorebi::workspace::Workspace;
pub use komorebi::workspace::WorkspaceLayer;
pub use komorebi_themes::KomorebiTheme;
pub use komorebi_themes::colour::Colour;
pub use komorebi_themes::colour::Rgb;
use std::borrow::Borrow;
use std::io::BufReader;
use std::io::Read;
use std::io::Write;
use std::net::Shutdown;
use std::os::unix::net::UnixListener;
use std::os::unix::net::UnixStream;
use std::time::Duration;

const KOMOREBI: &str = "komorebi.sock";

pub fn send_message(message: &SocketMessage) -> std::io::Result<()> {
    let socket = DATA_DIR.join(KOMOREBI);
    let mut stream = UnixStream::connect(socket)?;
    stream.set_write_timeout(Some(Duration::from_secs(1)))?;
    stream.write_all(serde_json::to_string(message)?.as_bytes())
}

/// A connection to komorebi that messages are written to one after the other, which komorebi
/// handles in the order that they were written, as it does the lines of a batch. Messages sent
/// with [`send_message`] are each on a connection of their own, which komorebi handles on a
/// thread of its own, so two that are sent together can be handled in either order.
pub struct Connection {
    stream: UnixStream,
}

impl Connection {
    pub fn open() -> std::io::Result<Self> {
        let socket = DATA_DIR.join(KOMOREBI);

        Self::from_stream(UnixStream::connect(socket)?)
    }

    fn from_stream(stream: UnixStream) -> std::io::Result<Self> {
        stream.set_write_timeout(Some(Duration::from_secs(1)))?;

        Ok(Self { stream })
    }

    pub fn send(&mut self, message: &SocketMessage) -> std::io::Result<()> {
        let mut line = serde_json::to_string(message)?;
        line.push('\n');

        self.stream.write_all(line.as_bytes())
    }
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::BufRead;
    use std::str::FromStr;

    #[test]
    fn messages_are_written_to_a_connection_a_line_each_in_the_order_that_they_were_sent() {
        let (ours, theirs) = UnixStream::pair().unwrap();
        let mut connection = Connection::from_stream(ours).unwrap();

        connection
            .send(&SocketMessage::PanGesture(PanGesture::Begin))
            .unwrap();
        connection
            .send(&SocketMessage::Pan(PanDirection::Right))
            .unwrap();
        connection
            .send(&SocketMessage::PanGesture(PanGesture::Move {
                dx: 0.5,
                dy: 0.0,
                t: 12.0,
            }))
            .unwrap();
        drop(connection);

        let lines: Vec<String> = BufReader::new(theirs)
            .lines()
            .collect::<Result<_, _>>()
            .unwrap();
        assert_eq!(lines.len(), 3, "one line to a message: {lines:?}");

        let messages: Vec<SocketMessage> = lines
            .iter()
            .map(|line| SocketMessage::from_str(line).unwrap())
            .collect();
        assert!(matches!(
            messages[0],
            SocketMessage::PanGesture(PanGesture::Begin)
        ));
        assert!(matches!(
            messages[1],
            SocketMessage::Pan(PanDirection::Right)
        ));
        assert!(matches!(
            messages[2],
            SocketMessage::PanGesture(PanGesture::Move { dx, t, .. }) if dx == 0.5 && t == 12.0
        ));
    }

    #[test]
    fn writing_to_a_connection_that_was_let_go_of_is_an_error() {
        let (ours, theirs) = UnixStream::pair().unwrap();
        let mut connection = Connection::from_stream(ours).unwrap();
        drop(theirs);

        assert!(
            connection
                .send(&SocketMessage::PanGesture(PanGesture::Cancel))
                .is_err()
        );
    }
}
