//! Talking to a running komorebi, on either OS, without depending on either komorebi's crates.
//!
//! komorebi for Mac and komorebi for Windows speak the same wire protocol over a unix domain
//! socket. Commands go to `komorebi.sock` in komorebi's data directory (`<local data dir>/komorebi`);
//! each message is one compact JSON object, `{"type": "<Name>", "content": <payload>}`, and a query
//! is answered by writing the reply back and closing the connection. To receive notifications a
//! subscriber binds a socket of its own in that directory and asks komorebi to connect to it by
//! name: komorebi then connects once per notification, writes one JSON blob and disconnects.
//!
//! Only the handful of messages the recorder sends are defined here. Sockets are `std`'s on unix
//! and those of the [`uds_windows`] crate on Windows, which has the same interface.
//!
//! # Where this fits
//!
//! This is the lowest layer that touches komorebi, and it is the same on both hosts. Which komorebi
//! is answering (the *flavor*, see [`crate::analysis::flavor`]) is not something this module
//! knows or asks. komorebi's `SocketMessage` is what [`Message`] has to agree with on the wire,
//! and [`Message`] says how that is checked.
//!
//! | Function | Used for |
//! |---|---|
//! | [`data_dir`], [`socket_path`] | finding komorebi's sockets; also the default parent of recordings |
//! | [`query_state`] | asking for komorebi's `State` (the recorder, and `doctor`) |
//! | [`send_message`] | subscribing and unsubscribing (see [`crate::sources::komorebi_events`]) |
//! | [`send_query`] | any other query, such as `doctor --probe window` |
//! | [`bind_subscriber`] | listening for komorebi's notifications |
//!
//! # Timeouts and retries
//!
//! Every read and every write on a connection to komorebi times out after one second, so a komorebi
//! that stops answering fails the exchange. Connecting has no timeout. Nothing here
//! retries, except [`query_state`] when komorebi answers with nothing.
//!
//! # Example
//!
//! The wire format is pinned by what [`Message`] serializes to:
//!
//! ```rust
//! use komorebi_recorder::komorebi_link::{Message, SubscribeOptions};
//!
//! assert_eq!(serde_json::to_string(&Message::State)?, r#"{"type":"State"}"#);
//!
//! let subscribe = Message::AddSubscriberSocketWithOptions(
//!     "komorebi-recorder-7".to_string(),
//!     SubscribeOptions { filter_state_changes: false },
//! );
//! assert_eq!(
//!     serde_json::to_string(&subscribe)?,
//!     r#"{"type":"AddSubscriberSocketWithOptions","content":["komorebi-recorder-7",{"filter_state_changes":false}]}"#
//! );
//! # Ok::<(), serde_json::Error>(())
//! ```
//!
//! The functions that need a komorebi, or a socket, have `no_run` examples and name the tests that
//! cover them: see [`query_state`] and [`bind_subscriber`].
//!
//! [`uds_windows`]: https://docs.rs/uds_windows

use color_eyre::eyre;
use color_eyre::eyre::WrapErr;
use serde::Serialize;
use std::io::Read;
use std::io::Write;
use std::net::Shutdown;
use std::path::Path;
use std::path::PathBuf;
use std::sync::OnceLock;
use std::time::Duration;

/// The unix domain socket listener of this host: `std`'s on unix.
#[cfg(unix)]
pub use std::os::unix::net::UnixListener;
/// The unix domain socket stream of this host: `std`'s on unix.
#[cfg(unix)]
pub use std::os::unix::net::UnixStream;
/// The unix domain socket listener of this host: the `uds_windows` crate's on Windows, which has
/// the same interface as `std`'s.
#[cfg(windows)]
pub use uds_windows::UnixListener;
/// The unix domain socket stream of this host: the `uds_windows` crate's on Windows, which has the
/// same interface as `std`'s.
#[cfg(windows)]
pub use uds_windows::UnixStream;

/// The name of the socket komorebi listens for commands on, inside the data directory.
///
/// Its path is [`socket_path`]`(KOMOREBI_SOCKET)`. It is `komorebi.sock` on both flavors of
/// komorebi.
pub const KOMOREBI_SOCKET: &str = "komorebi.sock";

/// How long to wait on komorebi before giving up on one message.
const IO_TIMEOUT: Duration = Duration::from_secs(1);

/// Returns komorebi's data directory, where its sockets and every subscriber's socket live.
///
/// This is `<local data dir>/komorebi` on both OSes (`~/Library/Application Support/komorebi`,
/// `%LOCALAPPDATA%\komorebi`). If the OS has no local data directory, the directory is `komorebi`
/// inside the temporary directory. The path is worked out on the first call, and is the same for
/// the life of the process. The directory is not created and need not exist.
///
/// A session's recordings go under it, in `recordings/`, unless told otherwise
/// (`session::default_recordings_dir`).
///
/// # Example
///
/// ```rust
/// use komorebi_recorder::komorebi_link::{data_dir, socket_dir, socket_path, KOMOREBI_SOCKET};
///
/// assert!(data_dir().ends_with("komorebi"));
/// assert_eq!(socket_path(KOMOREBI_SOCKET), socket_dir().join("komorebi.sock"));
/// ```
#[must_use]
pub fn data_dir() -> &'static Path {
    static DATA_DIR: OnceLock<PathBuf> = OnceLock::new();
    DATA_DIR.get_or_init(|| {
        dirs::data_local_dir()
            .unwrap_or_else(std::env::temp_dir)
            .join("komorebi")
    })
}

/// Returns the directory komorebi's sockets are in.
///
/// That is [`data_dir`] for komorebi for Mac and for Windows. komorebi for KDE keeps them in
/// `$XDG_RUNTIME_DIR/komorebi`, and in [`data_dir`] only where the session has no runtime
/// directory, so on Linux this is the same.
///
/// # Example
///
/// ```rust
/// use komorebi_recorder::komorebi_link::{data_dir, socket_dir};
///
/// assert!(socket_dir().ends_with("komorebi"));
/// # let _ = data_dir();
/// ```
#[must_use]
pub fn socket_dir() -> &'static Path {
    static SOCKET_DIR: OnceLock<PathBuf> = OnceLock::new();
    SOCKET_DIR.get_or_init(|| {
        if cfg!(target_os = "linux")
            && let Some(runtime) = std::env::var_os("XDG_RUNTIME_DIR").filter(|dir| !dir.is_empty())
        {
            return PathBuf::from(runtime).join("komorebi");
        }
        data_dir().to_path_buf()
    })
}

/// Returns the path of the socket called `name` in komorebi's socket directory
/// ([`socket_dir`]).
///
/// The name is joined as given, without checking that it is a plain file name, and the socket
/// need not exist. A unix socket path is limited to about 104 bytes on macOS and 108 on Windows
/// and Linux, so a long data directory leaves room for only a short `name`: `doctor` checks that
/// the recorder's subscriber socket fits.
#[must_use]
pub fn socket_path(name: &str) -> PathBuf {
    socket_dir().join(name)
}

/// What a subscriber asks komorebi for.
///
/// The default is to ask for every notification. The recorder does too: it wants every event, not
/// only the ones that changed state.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize)]
pub struct SubscribeOptions {
    /// Only send notifications that changed the window manager's state.
    pub filter_state_changes: bool,
}

/// A message to komorebi, in the form komorebi reads from its socket.
///
/// Only the messages the recorder sends are defined. The type serializes, as `{"type": "<Variant>"}`
/// or `{"type": "<Variant>", "content": <payload>}`, to what komorebi's `SocketMessage` reads.
/// It is never read back, because the recorder never receives one.
///
/// komorebi is not a dependency, so nothing in the build checks that the two agree: the variant
/// names and payload shapes are kept as komorebi spells them, and the test
/// `messages_are_spelled_as_komorebi_reads_them` pins the JSON each variant produces.
///
/// # Example
///
/// ```rust
/// use komorebi_recorder::komorebi_link::Message;
///
/// let json = |message: &Message| serde_json::to_string(message);
///
/// assert_eq!(json(&Message::State)?, r#"{"type":"State"}"#);
/// assert_eq!(
///     json(&Message::RemoveSubscriberSocket("komorebi-recorder-7".to_string()))?,
///     r#"{"type":"RemoveSubscriberSocket","content":"komorebi-recorder-7"}"#
/// );
/// assert_eq!(
///     json(&Message::DebugWindow(131_072))?,
///     r#"{"type":"DebugWindow","content":131072}"#
/// );
/// # Ok::<(), serde_json::Error>(())
/// ```
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "type", content = "content")]
pub enum Message {
    /// Reply with the full window manager state as JSON.
    State,
    /// Connect to the socket named `.0` in the data directory for every notification, with the
    /// options `.1`.
    AddSubscriberSocketWithOptions(String, SubscribeOptions),
    /// Stop connecting to the socket named `.0` in the data directory.
    RemoveSubscriberSocket(String),
    /// Reply with komorebi's verdict on whether it would manage the window with this handle, and
    /// why (Windows).
    DebugWindow(u32),
}

/// Sends one message and does not wait for an answer.
///
/// This connects to komorebi's command socket ([`KOMOREBI_SOCKET`]), writes the message as one JSON
/// object, and closes the connection. The write times out after one second. Use [`send_query`] for
/// a message komorebi answers.
///
/// # Errors
///
/// Returns the I/O error if
///
/// * komorebi's socket cannot be connected to: komorebi is not running, or its socket cannot be
///   used by this user;
/// * the write fails or takes longer than one second (`WouldBlock` or `TimedOut`, depending on the
///   OS).
///
/// # Example
///
/// ```rust,no_run
/// use komorebi_recorder::komorebi_link::{send_message, Message, SubscribeOptions};
///
/// // ask komorebi to connect to `komorebi-recorder-7` in its data directory, per notification
/// send_message(&Message::AddSubscriberSocketWithOptions(
///     "komorebi-recorder-7".to_string(),
///     SubscribeOptions::default(),
/// ))?;
/// # Ok::<(), std::io::Error>(())
/// ```
///
/// This needs komorebi running. What is sent is covered by the test
/// `messages_are_spelled_as_komorebi_reads_them`. No test covers the exchange over the socket.
pub fn send_message(message: &Message) -> std::io::Result<()> {
    let mut stream = UnixStream::connect(socket_path(KOMOREBI_SOCKET))?;
    stream.set_write_timeout(Some(IO_TIMEOUT))?;
    stream.write_all(serde_json::to_string(message)?.as_bytes())
}

/// Sends one message and reads komorebi's reply to the end of the connection.
///
/// This connects to komorebi's command socket, writes the message, shuts down the writing half so
/// that komorebi sees the end of it, and reads until komorebi closes the connection. Reads and
/// writes each time out after one second: the limit applies to each, and not to the exchange as a
/// whole. The reply is returned as komorebi wrote it, which may be empty: komorebi for Windows
/// declines a state query by answering with nothing (see [`query_state`]).
///
/// # Errors
///
/// Returns the I/O error if
///
/// * komorebi's socket cannot be connected to: komorebi is not running, or this user may not use
///   its socket;
/// * a write or a read fails or takes longer than one second;
/// * the reply is not valid UTF-8 (`InvalidData`).
///
/// # Example
///
/// ```rust,no_run
/// use komorebi_recorder::komorebi_link::{send_query, Message};
///
/// let verdict = send_query(&Message::DebugWindow(131_072))?;
/// println!("komorebi says: {verdict}");
/// # Ok::<(), std::io::Error>(())
/// ```
///
/// This needs komorebi running, and no test covers the exchange over the socket.
pub fn send_query(message: &Message) -> std::io::Result<String> {
    let mut stream = UnixStream::connect(socket_path(KOMOREBI_SOCKET))?;
    stream.set_read_timeout(Some(IO_TIMEOUT))?;
    stream.set_write_timeout(Some(IO_TIMEOUT))?;
    stream.write_all(serde_json::to_string(message)?.as_bytes())?;
    stream.shutdown(Shutdown::Write)?;

    let mut response = String::new();
    stream.read_to_string(&mut response)?;
    Ok(response)
}

/// Why komorebi did not hand over its state.
///
/// The `Display` text is a sentence fit for a warning, with no trailing period. Neither case says
/// anything about the state komorebi holds: it may be perfectly healthy.
///
/// # Example
///
/// ```rust
/// use komorebi_recorder::komorebi_link::StateError;
///
/// let error = StateError::Empty { attempts: 5 };
/// assert_eq!(
///     error.to_string(),
///     "komorebi answered the state query with nothing 5 times in a row \
///      (busy, paused, or on another virtual desktop)"
/// );
/// ```
#[derive(Debug)]
pub enum StateError {
    /// komorebi could not be reached at all (not running, or its socket is unreadable). The
    /// error is whatever [`send_query`] returned, including a timeout.
    Unreachable(std::io::Error),
    /// komorebi accepted the query and answered with nothing, every time we asked. On Windows that
    /// is how it declines while it is busy, paused, or showing another virtual desktop.
    Empty {
        /// How many times the query was made, and answered with nothing.
        attempts: u32,
    },
}

impl std::fmt::Display for StateError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            StateError::Unreachable(error) => write!(f, "could not query komorebi: {error}"),
            StateError::Empty { attempts } => write!(
                f,
                "komorebi answered the state query with nothing {attempts} times in a row \
                 (busy, paused, or on another virtual desktop)"
            ),
        }
    }
}

impl std::error::Error for StateError {}

/// How many times to ask for the state when komorebi answers with nothing, and how long to wait
/// between asks.
const STATE_ATTEMPTS: u32 = 5;
const STATE_RETRY_DELAY: Duration = Duration::from_millis(200);

/// Asks komorebi for its state, asking again when it answers with nothing.
///
/// Returns komorebi's `State` as the JSON text komorebi sent, unparsed. A reply that is empty, or
/// only whitespace, is asked for again, up to 5 times in all, 200 ms apart. A failure to reach
/// komorebi is not asked again.
///
/// # Errors
///
/// Returns [`StateError::Unreachable`] if [`send_query`] fails, on any attempt, and
/// [`StateError::Empty`] if every attempt was answered with nothing.
///
/// # Example
///
/// ```rust,no_run
/// use komorebi_recorder::komorebi_link::{query_state, StateError};
///
/// match query_state() {
///     Ok(state) => println!("komorebi's state is {} bytes", state.len()),
///     Err(StateError::Unreachable(error)) => println!("not running? {error}"),
///     Err(error @ StateError::Empty { .. }) => println!("{error}"),
/// }
/// ```
///
/// This needs komorebi running. The retry rule is covered by the tests
/// `an_empty_state_reply_is_asked_again`,
/// `a_komorebi_that_only_ever_answers_with_nothing_is_an_error_that_says_so` and
/// `an_unreachable_komorebi_is_not_asked_again`, which supply the answers themselves. No test covers
/// the exchange over the socket.
pub fn query_state() -> Result<String, StateError> {
    query_state_with(
        || send_query(&Message::State),
        STATE_ATTEMPTS,
        STATE_RETRY_DELAY,
    )
}

fn query_state_with(
    mut ask: impl FnMut() -> std::io::Result<String>,
    attempts: u32,
    delay: Duration,
) -> Result<String, StateError> {
    for attempt in 1..=attempts {
        let response = ask().map_err(StateError::Unreachable)?;
        if !response.trim().is_empty() {
            return Ok(response);
        }

        if attempt < attempts {
            std::thread::sleep(delay);
        }
    }

    Err(StateError::Empty { attempts })
}

/// Binds a listener for komorebi's notifications at `path`, replacing a stale socket file left by
/// a subscriber that died.
///
/// Whatever is at `path` is removed first. A socket another live process is listening on is taken
/// over, not refused: that process keeps its listener, which nothing can reach by name. This
/// function does not remove the socket file afterwards; the caller does. A subscriber's `path` is
/// [`socket_path`] of the name it then gives komorebi in [`Message::AddSubscriberSocketWithOptions`].
///
/// # Errors
///
/// Returns an error if
///
/// * something at `path` cannot be removed, for any reason other than there being nothing there;
/// * the socket cannot be bound: the directory does not exist, this user may not write to it, or
///   `path` is longer than a unix socket path may be (about 104 bytes on macOS, 108 elsewhere).
///
/// # Example
///
/// ```rust,no_run
/// use komorebi_recorder::komorebi_link::{bind_subscriber, socket_path};
///
/// let listener = bind_subscriber(&socket_path("komorebi-recorder-7"))?;
/// // komorebi connects here once per notification, writes one JSON blob and disconnects
/// let (_stream, _) = listener.accept()?;
/// # Ok::<(), Box<dyn std::error::Error>>(())
/// ```
///
/// This waits for komorebi, so it is not run. It is covered by the test
/// `a_subscriber_socket_receives_what_is_written_to_it`, which binds over a stale file, connects a
/// writer to the socket, and reads what was written.
pub fn bind_subscriber(path: &Path) -> eyre::Result<UnixListener> {
    match std::fs::remove_file(path) {
        Ok(()) => {}
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => return Err(error).wrap_err("could not clear a stale subscriber socket"),
    }

    UnixListener::bind(path).wrap_err_with(|| format!("could not listen on {}", path.display()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn messages_are_spelled_as_komorebi_reads_them() {
        let json = |message: &Message| serde_json::to_string(message).expect("serialize");

        assert_eq!(json(&Message::State), r#"{"type":"State"}"#);
        assert_eq!(
            json(&Message::AddSubscriberSocketWithOptions(
                "komorebi-recorder-7".to_string(),
                SubscribeOptions {
                    filter_state_changes: false
                }
            )),
            r#"{"type":"AddSubscriberSocketWithOptions","content":["komorebi-recorder-7",{"filter_state_changes":false}]}"#
        );
        assert_eq!(
            json(&Message::RemoveSubscriberSocket(
                "komorebi-recorder-7".into()
            )),
            r#"{"type":"RemoveSubscriberSocket","content":"komorebi-recorder-7"}"#
        );
        assert_eq!(
            json(&Message::DebugWindow(131_072)),
            r#"{"type":"DebugWindow","content":131072}"#
        );
    }

    #[test]
    fn the_data_dir_is_komorebis() {
        assert!(data_dir().ends_with("komorebi"));
        assert_eq!(socket_path("x.sock"), socket_dir().join("x.sock"));
    }

    #[test]
    fn an_empty_state_reply_is_asked_again() {
        let mut replies = vec![
            Ok(String::new()),
            Ok("  \n".to_string()),
            Ok(r#"{"monitors":{}}"#.to_string()),
        ]
        .into_iter();

        let state = query_state_with(
            || replies.next().expect("asked too often"),
            5,
            Duration::ZERO,
        )
        .expect("state");
        assert_eq!(state, r#"{"monitors":{}}"#);
    }

    #[test]
    fn a_komorebi_that_only_ever_answers_with_nothing_is_an_error_that_says_so() {
        let mut asked = 0;
        let error = query_state_with(
            || {
                asked += 1;
                Ok(String::new())
            },
            3,
            Duration::ZERO,
        )
        .expect_err("nothing to return");

        assert_eq!(asked, 3);
        assert!(
            matches!(error, StateError::Empty { attempts: 3 }),
            "{error}"
        );
        assert!(error.to_string().contains("another virtual desktop"));
    }

    #[test]
    fn an_unreachable_komorebi_is_not_asked_again() {
        let mut asked = 0;
        let error = query_state_with(
            || {
                asked += 1;
                Err(std::io::Error::from(std::io::ErrorKind::NotFound))
            },
            5,
            Duration::ZERO,
        )
        .expect_err("unreachable");

        assert_eq!(asked, 1);
        assert!(matches!(error, StateError::Unreachable(_)));
    }

    #[test]
    fn a_subscriber_socket_receives_what_is_written_to_it() {
        // a unix socket path is limited to ~104 bytes, which a nix shell's $TMPDIR already exceeds
        let base = if cfg!(unix) {
            PathBuf::from("/tmp")
        } else {
            std::env::temp_dir()
        };
        let dir = base.join(format!("kl-{}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("dir");
        let path = dir.join("sub");

        // binding over a stale file from a dead subscriber works
        std::fs::write(&path, b"stale").expect("stale");
        let listener = bind_subscriber(&path).expect("bind");

        let writer = std::thread::spawn({
            let path = path.clone();
            move || {
                let mut stream = UnixStream::connect(path).expect("connect");
                stream.write_all(b"{\"a\":1}").expect("write");
            }
        });

        let (mut stream, _) = listener.accept().expect("accept");
        let mut bytes = Vec::new();
        stream.read_to_end(&mut bytes).expect("read");
        writer.join().expect("writer");
        let _ = std::fs::remove_dir_all(&dir);
        assert_eq!(bytes, br#"{"a":1}"#);
    }
}
