//! A client for the komorebi daemon: it writes [`SocketMessage`]s on the daemon's socket and reads
//! the daemon's answers.
//!
//! The daemon listens on `komorebi.sock` in [`struct@DATA_DIR`], which is
//! `$XDG_RUNTIME_DIR/komorebi` in a desktop session. Every function here opens a new connection
//! and writes each message as one line of JSON. [`send_message`] and [`send_batch`] give
//! orders and read nothing back; [`send_query`] asks a question and returns the answer, which the
//! daemon ends by closing the connection. A function fails with the error of the connection, so a
//! daemon that is not running is an [`std::io::ErrorKind::NotFound`] or a
//! [`std::io::ErrorKind::ConnectionRefused`].
//!
//! [`subscribe`] and [`subscribe_with_options`] make the program a subscriber: they listen on a
//! socket of its own in [`struct@DATA_DIR`], and ask the daemon to send a [`Notification`] to it
//! after each event and each message the window manager handles.
//!
//! The crate also re-exports the types the messages and the answers are made of, so that a client
//! needs nothing else, and [`check`], the checks `komorebic check` runs.
//!
//! # Example
//!
//! The examples on this page answer for the daemon: each one listens on the socket itself, in a
//! runtime directory made for it, before it sends anything.
//!
//! ```rust
//! # use std::io::{BufRead, BufReader, Write};
//! # let runtime = format!("/tmp/komorebi-client-doc-crate-{}", std::process::id());
//! # std::fs::create_dir_all(format!("{runtime}/komorebi"))?;
//! # unsafe { std::env::set_var("XDG_RUNTIME_DIR", &runtime) }; // the socket is in it
//! # let socket = komorebi_client::DATA_DIR.join("komorebi.sock");
//! # let listener = std::os::unix::net::UnixListener::bind(&socket)?;
//! # let daemon = std::thread::spawn(move || -> std::io::Result<String> {
//! #     let (mut stream, _) = listener.accept()?;
//! #     let mut line = String::new();
//! #     BufReader::new(stream.try_clone()?).read_line(&mut line)?;
//! #     stream.write_all(b"2")?;
//! #     Ok(line)
//! # });
//! use komorebi_client::SocketMessage;
//! use komorebi_client::StateQuery;
//! use komorebi_client::send_query;
//!
//! let workspace = send_query(&SocketMessage::Query(StateQuery::FocusedWorkspaceIndex))?;
//! assert_eq!(workspace, "2");
//!
//! # assert_eq!(
//! #     daemon.join().unwrap()?,
//! #     r#"{"type":"Query","content":"FocusedWorkspaceIndex"}"#
//! # );
//! # std::fs::remove_dir_all(&runtime)?;
//! # Ok::<(), Box<dyn std::error::Error>>(())
//! ```
//!
//! `komorebi-client/tests/daemon.rs` sends the same messages to a window manager that listens the
//! way the daemon does, over an in-memory desktop.

pub use komorebi::DATA_DIR;
pub use komorebi::LOG_DIR;
pub use komorebi::Notification;
pub use komorebi::NotificationEvent;
pub use komorebi::animation::AnimationPrefix;
pub use komorebi::check;
pub use komorebi::container::Container;
pub use komorebi::core::ApplicationIdentifier;
pub use komorebi::core::Axis;
pub use komorebi::core::CycleDirection;
pub use komorebi::core::DefaultLayout;
pub use komorebi::core::FocusFollowsMouseImplementation;
pub use komorebi::core::Layout;
pub use komorebi::core::MonocleFocusBehaviour;
pub use komorebi::core::MoveBehaviour;
pub use komorebi::core::OperationBehaviour;
pub use komorebi::core::OperationDirection;
pub use komorebi::core::PanDirection;
pub use komorebi::core::Rect;
pub use komorebi::core::ScrollingCentering;
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

/// Sends one message to the daemon, and does not wait for it to be handled
///
/// # Errors
///
/// - The daemon's socket cannot be connected to, for example because the daemon is not running.
/// - The message cannot be written within a second.
///
/// # Example
///
/// ```rust
/// # use std::io::{BufRead, BufReader};
/// # let runtime = format!("/tmp/komorebi-client-doc-message-{}", std::process::id());
/// # std::fs::create_dir_all(format!("{runtime}/komorebi"))?;
/// # unsafe { std::env::set_var("XDG_RUNTIME_DIR", &runtime) }; // the socket is in it
/// # let socket = komorebi_client::DATA_DIR.join("komorebi.sock");
/// # let listener = std::os::unix::net::UnixListener::bind(&socket)?;
/// # let daemon = std::thread::spawn(move || -> std::io::Result<String> {
/// #     let (stream, _) = listener.accept()?;
/// #     let mut line = String::new();
/// #     BufReader::new(stream).read_line(&mut line)?;
/// #     Ok(line)
/// # });
/// use komorebi_client::SocketMessage;
/// use komorebi_client::send_message;
///
/// send_message(&SocketMessage::Retile)?;
///
/// # assert_eq!(daemon.join().unwrap()?, r#"{"type":"Retile"}"#);
/// # std::fs::remove_dir_all(&runtime)?;
/// # Ok::<(), Box<dyn std::error::Error>>(())
/// ```
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
    /// Opens a connection to the daemon's socket.
    ///
    /// # Errors
    ///
    /// The daemon's socket cannot be connected to, or written to within a second.
    pub fn open() -> std::io::Result<Self> {
        let socket = DATA_DIR.join(KOMOREBI);

        Self::from_stream(UnixStream::connect(socket)?)
    }

    fn from_stream(stream: UnixStream) -> std::io::Result<Self> {
        stream.set_write_timeout(Some(Duration::from_secs(1)))?;

        Ok(Self { stream })
    }

    /// Writes one message as a line.
    ///
    /// # Errors
    ///
    /// The message cannot be written, for example because the daemon let go of the connection.
    pub fn send(&mut self, message: &SocketMessage) -> std::io::Result<()> {
        let mut line = serde_json::to_string(message)?;
        line.push('\n');

        self.stream.write_all(line.as_bytes())
    }
}

/// Sends several messages to the daemon on one connection, which handles them in order, and does
/// not wait for them to be handled
///
/// # Errors
///
/// The same as [`send_message`].
///
/// # Example
///
/// ```rust
/// # use std::io::{BufRead, BufReader};
/// # let runtime = format!("/tmp/komorebi-client-doc-batch-{}", std::process::id());
/// # std::fs::create_dir_all(format!("{runtime}/komorebi"))?;
/// # unsafe { std::env::set_var("XDG_RUNTIME_DIR", &runtime) }; // the socket is in it
/// # let socket = komorebi_client::DATA_DIR.join("komorebi.sock");
/// # let listener = std::os::unix::net::UnixListener::bind(&socket)?;
/// # let daemon = std::thread::spawn(move || -> std::io::Result<Vec<String>> {
/// #     let (stream, _) = listener.accept()?;
/// #     BufReader::new(stream).lines().collect()
/// # });
/// use komorebi_client::SocketMessage;
/// use komorebi_client::send_batch;
///
/// send_batch([SocketMessage::Retile, SocketMessage::Stop])?;
///
/// # assert_eq!(daemon.join().unwrap()?, [r#"{"type":"Retile"}"#, r#"{"type":"Stop"}"#]);
/// # std::fs::remove_dir_all(&runtime)?;
/// # Ok::<(), Box<dyn std::error::Error>>(())
/// ```
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

/// Sends one message to the daemon and returns its answer: JSON for
/// [`SocketMessage::State`] and [`SocketMessage::MonitorInformation`], one value as text for
/// [`SocketMessage::Query`], and nothing for a message that is not a question
///
/// # Errors
///
/// - The daemon's socket cannot be connected to, for example because the daemon is not running.
/// - The message cannot be written within a second.
/// - The daemon sends nothing for a second before its answer is complete.
///
/// # Example
///
/// The answer to [`SocketMessage::State`] reads back as a [`State`]:
///
/// ```rust
/// # use std::io::{BufRead, BufReader, Write};
/// # let runtime = format!("/tmp/komorebi-client-doc-query-{}", std::process::id());
/// # std::fs::create_dir_all(format!("{runtime}/komorebi"))?;
/// # unsafe { std::env::set_var("XDG_RUNTIME_DIR", &runtime) }; // the socket is in it
/// # let socket = komorebi_client::DATA_DIR.join("komorebi.sock");
/// # let listener = std::os::unix::net::UnixListener::bind(&socket)?;
/// # let daemon = std::thread::spawn(move || -> std::io::Result<()> {
/// #     let (mut stream, _) = listener.accept()?;
/// #     BufReader::new(stream.try_clone()?).read_line(&mut String::new())?;
/// #     let state = r#"{"monitors":{"elements":[],"focused":0},"monitor_usr_idx_map":{},"is_paused":false,"resize_delta":50,"new_window_behaviour":"Create","float_override":false,"cross_monitor_move_behaviour":"Swap","monocle_focus_behaviour":"NoOp","unmanaged_window_operation_behaviour":"Op","work_area_offset":null,"mouse_follows_focus":true}"#;
/// #     stream.write_all(state.as_bytes())
/// # });
/// use komorebi_client::SocketMessage;
/// use komorebi_client::State;
/// use komorebi_client::send_query;
///
/// let state: State = serde_json::from_str(&send_query(&SocketMessage::State)?)?;
/// assert!(state.mouse_follows_focus);
///
/// # daemon.join().unwrap()?;
/// # std::fs::remove_dir_all(&runtime)?;
/// # Ok::<(), Box<dyn std::error::Error>>(())
/// ```
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

/// Listens on the socket `name` in [`struct@DATA_DIR`], and asks the daemon to send a
/// [`Notification`] to it after each event and each message the window manager handles
///
/// A socket file that is left over at that path is removed first. Each notification comes on a
/// new connection, as JSON, and ends when the daemon closes the connection; the first is the
/// notification of the subscription itself. When the daemon stops, it connects and closes the
/// connection without writing anything.
///
/// A subscriber that the daemon cannot connect to, because its program has exited or its socket
/// was removed, is removed at the next notification, with its socket file. It has to subscribe
/// again: the daemon does not try it again, and does not keep its subscribers when it restarts.
///
/// # Errors
///
/// - The socket file left over at that path cannot be removed.
/// - The socket cannot be bound, for example because [`struct@DATA_DIR`] does not exist.
/// - The subscription cannot be sent, for one of the reasons [`send_message`] gives.
///
/// # Example
///
/// ```rust
/// # use std::io::{BufRead, BufReader, Write};
/// # let runtime = format!("/tmp/komorebi-client-doc-subscribe-{}", std::process::id());
/// # std::fs::create_dir_all(format!("{runtime}/komorebi"))?;
/// # unsafe { std::env::set_var("XDG_RUNTIME_DIR", &runtime) }; // the sockets are in it
/// # let socket = komorebi_client::DATA_DIR.join("komorebi.sock");
/// # let listener = std::os::unix::net::UnixListener::bind(&socket)?;
/// # let daemon = std::thread::spawn(move || -> std::io::Result<String> {
/// #     let (stream, _) = listener.accept()?;
/// #     let mut message = String::new();
/// #     BufReader::new(stream).read_line(&mut message)?;
/// #     let subscriber = komorebi_client::DATA_DIR.join("bar.sock");
/// #     let state = r#"{"monitors":{"elements":[],"focused":0},"monitor_usr_idx_map":{},"is_paused":false,"resize_delta":50,"new_window_behaviour":"Create","float_override":false,"cross_monitor_move_behaviour":"Swap","monocle_focus_behaviour":"NoOp","unmanaged_window_operation_behaviour":"Op","work_area_offset":null,"mouse_follows_focus":true}"#;
/// #     let notification = format!(r#"{{"event":{message},"state":{state}}}"#);
/// #     std::os::unix::net::UnixStream::connect(subscriber)?.write_all(notification.as_bytes())?;
/// #     Ok(message)
/// # });
/// use std::io::Read;
///
/// use komorebi_client::Notification;
/// use komorebi_client::NotificationEvent;
/// use komorebi_client::SocketMessage;
/// use komorebi_client::subscribe;
///
/// let listener = subscribe("bar.sock")?;
///
/// // the first notification is the one of the subscription
/// let (mut connection, _) = listener.accept()?;
/// let mut json = String::new();
/// connection.read_to_string(&mut json)?;
/// let notification: Notification = serde_json::from_str(&json)?;
/// assert!(matches!(
///     notification.event,
///     NotificationEvent::Socket(SocketMessage::AddSubscriberSocket(name)) if name == "bar.sock"
/// ));
///
/// # assert_eq!(
/// #     daemon.join().unwrap()?,
/// #     r#"{"type":"AddSubscriberSocket","content":"bar.sock"}"#
/// # );
/// # std::fs::remove_dir_all(&runtime)?;
/// # Ok::<(), Box<dyn std::error::Error>>(())
/// ```
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

/// The same as [`subscribe`], with the [`SubscribeOptions`] the daemon applies to this subscriber
///
/// With `filter_state_changes`, the daemon sends a notification only when the event or the message
/// changed the [`State`], and always when a window is shown, when a subscriber is added, when a
/// theme is set and when the configuration is reloaded.
///
/// # Errors
///
/// The same as [`subscribe`].
///
/// # Example
///
/// ```rust
/// # use std::io::{BufRead, BufReader};
/// # let runtime = format!("/tmp/komorebi-client-doc-options-{}", std::process::id());
/// # std::fs::create_dir_all(format!("{runtime}/komorebi"))?;
/// # unsafe { std::env::set_var("XDG_RUNTIME_DIR", &runtime) }; // the sockets are in it
/// # let socket = komorebi_client::DATA_DIR.join("komorebi.sock");
/// # let listener = std::os::unix::net::UnixListener::bind(&socket)?;
/// # let daemon = std::thread::spawn(move || -> std::io::Result<String> {
/// #     let (stream, _) = listener.accept()?;
/// #     let mut message = String::new();
/// #     BufReader::new(stream).read_line(&mut message)?;
/// #     Ok(message)
/// # });
/// use komorebi_client::DATA_DIR;
/// use komorebi_client::SubscribeOptions;
/// use komorebi_client::subscribe_with_options;
///
/// let options = SubscribeOptions {
///     filter_state_changes: true,
/// };
/// let listener = subscribe_with_options("bar.sock", options)?;
///
/// let socket = DATA_DIR.join("bar.sock");
/// assert_eq!(listener.local_addr()?.as_pathname(), Some(socket.as_path()));
///
/// # assert_eq!(
/// #     daemon.join().unwrap()?,
/// #     r#"{"type":"AddSubscriberSocketWithOptions","content":["bar.sock",{"filter_state_changes":true}]}"#
/// # );
/// # std::fs::remove_dir_all(&runtime)?;
/// # Ok::<(), Box<dyn std::error::Error>>(())
/// ```
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
