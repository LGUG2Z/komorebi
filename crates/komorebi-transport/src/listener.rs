use crate::LOG_TARGET;
use crate::allowed_while_paused;
use color_eyre::eyre;
use komorebi_protocol::message::SocketMessage;
use std::io::BufRead;
use std::io::BufReader;
use std::io::Read;
use std::io::Write;
use std::str::FromStr;
use std::time::Duration;

/// A connection a client sends commands on and reads the replies from
pub trait CommandStream: Read + Write + Send + Sized + 'static {
    /// A second handle on the same connection
    ///
    /// # Errors
    ///
    /// The connection cannot be duplicated.
    fn try_clone(&self) -> std::io::Result<Self>;
}

/// What accepts the connections of the clients
pub trait CommandListener: Send + Sized + 'static {
    type Stream: CommandStream;

    /// A second handle on the same listener
    ///
    /// # Errors
    ///
    /// The listener cannot be duplicated.
    fn try_clone(&self) -> std::io::Result<Self>;

    /// The connections, as they are accepted, until accepting fails
    fn incoming(&self) -> impl Iterator<Item = std::io::Result<Self::Stream>>;
}

/// The stream type a [`CommandTarget`] listens on
type StreamOf<T> = <<T as CommandTarget>::Listener as CommandListener>::Stream;

/// The window manager as the listener sees it
///
/// The window manager is shared between threads behind a lock whose type is the daemon's own
/// ([`Shared`](CommandTarget::Shared), an `Arc<Mutex<WindowManager>>`): the daemons do not all
/// use the same version of `parking_lot`, so the crate asks them to take the lock.
pub trait CommandTarget: Send + 'static {
    type Listener: CommandListener;

    /// The shared window manager
    type Shared: Clone + Send + Sync + 'static;

    /// A handle on the listener the window manager owns, taken with the window manager locked
    /// (waiting for the lock as long as it takes)
    ///
    /// # Errors
    ///
    /// The listener cannot be duplicated.
    fn command_listener(shared: &Self::Shared) -> std::io::Result<Self::Listener>;

    /// Runs `f` with the window manager locked, unless the lock is not got within `timeout`
    fn try_lock_for<R>(
        shared: &Self::Shared,
        timeout: Duration,
        f: impl FnOnce(&mut Self) -> R,
    ) -> Option<R>;

    /// Whether the window manager is paused
    fn is_paused(&self) -> bool;

    /// Handles one message, and writes what it has to answer on `reply`
    ///
    /// # Errors
    ///
    /// The message cannot be handled.
    fn process_command(
        &mut self,
        message: SocketMessage,
        reply: &mut StreamOf<Self>,
    ) -> eyre::Result<()>;
}

/// What a platform does at the two points of a connection it needs to: when it is accepted, and
/// when a message has been read from it (before the window manager is locked for it)
///
/// Both default to doing nothing.
pub trait ConnectionPolicy<S>: Clone + Send + Sync + 'static {
    fn on_accept(&self, _stream: &S) {}

    fn on_message(&self, _message: &SocketMessage, _stream: &S) {}
}

/// The policy that does nothing at either point
#[derive(Clone, Copy, Debug, Default)]
pub struct NoPolicy;

impl<S> ConnectionPolicy<S> for NoPolicy {}

/// Starts the thread that accepts connections on the listener of the window manager and reads
/// each connection on a thread of its own ([`read_commands`])
///
/// When accepting fails, the error is logged and the listening thread is started again.
pub fn listen_for_commands<T, P>(wm: T::Shared, policy: P)
where
    T: CommandTarget,
    P: ConnectionPolicy<StreamOf<T>>,
{
    std::thread::spawn(move || {
        loop {
            let wm = wm.clone();
            let policy = policy.clone();

            let _ = std::thread::spawn(move || {
                let listener = T::command_listener(&wm)
                    .expect("could not clone unix listener");

                tracing::info!(target: LOG_TARGET, "listening on komorebi.sock");
                for client in listener.incoming() {
                    match client {
                        Ok(stream) => {
                            let wm_clone = wm.clone();
                            let policy = policy.clone();
                            std::thread::spawn(move || {
                                policy.on_accept(&stream);
                                match read_commands::<T, P>(&wm_clone, stream, &policy) {
                                    Ok(()) => {}
                                    Err(error) => {
                                        tracing::error!(target: LOG_TARGET, "{error}")
                                    }
                                }
                            });
                        }
                        Err(error) => {
                            tracing::error!(target: LOG_TARGET, "failed to get unix stream {}", error);
                            break;
                        }
                    }
                }
            })
            .join();

            tracing::error!(target: LOG_TARGET, "restarting failed thread");
        }
    });
}

/// Reads the messages on one connection, one per line, and hands each to
/// [`CommandTarget::process_command`] with the connection to answer on
///
/// A message that arrives while the window manager has been locked for a second is logged and
/// dropped. While the window manager is paused, the first message that is not allowed
/// ([`allowed_while_paused`]) is dropped, and the first that is allowed is handled; either way,
/// the lines after it are not read.
///
/// # Errors
///
/// - The connection cannot be read, or cloned to be read and written at once.
/// - A line is not a [`SocketMessage`]: the lines after it are not read.
/// - [`CommandTarget::process_command`] fails: the lines after it are not read.
pub fn read_commands<T, P>(wm: &T::Shared, mut stream: StreamOf<T>, policy: &P) -> eyre::Result<()>
where
    T: CommandTarget,
    P: ConnectionPolicy<StreamOf<T>>,
{
    let reader = BufReader::new(stream.try_clone()?);
    // TODO(raggi): while this processes more than one command, if there are
    // replies there is no clearly defined protocol for framing yet - it's
    // perhaps whole-json objects for now, but termination is signalled by
    // socket shutdown.
    for line in reader.lines() {
        let message = SocketMessage::from_str(&line?)?;

        policy.on_message(&message, &stream);

        // the lock is held while the message is handled, and released before the next line is read
        let ended = T::try_lock_for(wm, Duration::from_secs(1), |wm| -> eyre::Result<bool> {
            if wm.is_paused() {
                if allowed_while_paused(&message) {
                    wm.process_command(message.clone(), &mut stream)?;
                } else {
                    tracing::trace!(target: LOG_TARGET, "ignoring while paused");
                }

                return Ok(true);
            }

            wm.process_command(message.clone(), &mut stream)?;
            Ok(false)
        });

        match ended {
            None => {
                tracing::warn!(
                    target: LOG_TARGET,
                    "could not acquire window manager lock, not processing message: {message}"
                );
            }
            Some(ended) => {
                if ended? {
                    return Ok(());
                }
            }
        }
    }

    Ok(())
}

#[cfg(unix)]
impl CommandStream for std::os::unix::net::UnixStream {
    fn try_clone(&self) -> std::io::Result<Self> {
        Self::try_clone(self)
    }
}

#[cfg(unix)]
impl CommandListener for std::os::unix::net::UnixListener {
    type Stream = std::os::unix::net::UnixStream;

    fn try_clone(&self) -> std::io::Result<Self> {
        Self::try_clone(self)
    }

    fn incoming(&self) -> impl Iterator<Item = std::io::Result<Self::Stream>> {
        Self::incoming(self)
    }
}

#[cfg(windows)]
impl CommandStream for uds_windows::UnixStream {
    fn try_clone(&self) -> std::io::Result<Self> {
        Self::try_clone(self)
    }
}

#[cfg(windows)]
impl CommandListener for uds_windows::UnixListener {
    type Stream = uds_windows::UnixStream;

    fn try_clone(&self) -> std::io::Result<Self> {
        Self::try_clone(self)
    }

    fn incoming(&self) -> impl Iterator<Item = std::io::Result<Self::Stream>> {
        Self::incoming(self)
    }
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use parking_lot::Mutex;
    use std::os::unix::net::UnixListener;
    use std::os::unix::net::UnixStream;
    use std::sync::Arc;

    /// A window manager that records what it is asked, and answers each message with its name
    struct Recorder {
        paused: bool,
        handled: Vec<String>,
        listener: UnixListener,
        path: std::path::PathBuf,
    }

    impl Drop for Recorder {
        fn drop(&mut self) {
            let _ = std::fs::remove_file(&self.path);
        }
    }

    impl Recorder {
        fn new(paused: bool) -> Self {
            // the listener is bound to a socket for this test alone
            static NEXT: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
            let path = std::env::temp_dir().join(format!(
                "komorebi-transport-{}-{}.sock",
                std::process::id(),
                NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
            ));
            let _ = std::fs::remove_file(&path);
            let listener = UnixListener::bind(&path).unwrap();
            Self {
                paused,
                handled: vec![],
                listener,
                path,
            }
        }
    }

    impl CommandTarget for Recorder {
        type Listener = UnixListener;

        type Shared = Arc<Mutex<Recorder>>;

        fn command_listener(shared: &Self::Shared) -> std::io::Result<UnixListener> {
            shared.lock().listener.try_clone()
        }

        fn try_lock_for<R>(
            shared: &Self::Shared,
            timeout: Duration,
            f: impl FnOnce(&mut Self) -> R,
        ) -> Option<R> {
            shared.try_lock_for(timeout).map(|mut wm| f(&mut wm))
        }

        fn is_paused(&self) -> bool {
            self.paused
        }

        fn process_command(
            &mut self,
            message: SocketMessage,
            reply: &mut UnixStream,
        ) -> eyre::Result<()> {
            self.handled.push(message.to_string());
            reply.write_all(message.to_string().as_bytes())?;
            Ok(())
        }
    }

    /// What `read_commands` answers to `lines`, and what the target was asked
    fn run(paused: bool, lines: &str) -> (eyre::Result<()>, String, Vec<String>) {
        let wm = Arc::new(Mutex::new(Recorder::new(paused)));
        let (server, mut client) = UnixStream::pair().unwrap();
        client.write_all(lines.as_bytes()).unwrap();
        client.shutdown(std::net::Shutdown::Write).unwrap();

        let result = read_commands::<Recorder, _>(&wm, server, &NoPolicy);

        let mut reply = String::new();
        client.read_to_string(&mut reply).unwrap();
        let handled = wm.lock().handled.clone();
        (result, reply, handled)
    }

    #[test]
    fn the_messages_on_one_connection_are_handled_in_order() {
        let (result, reply, handled) = run(false, "{\"type\":\"Retile\"}\n{\"type\":\"State\"}\n");
        assert!(result.is_ok());
        assert_eq!(handled, ["Retile", "State"]);
        assert_eq!(reply, "RetileState");
    }

    #[test]
    fn while_paused_the_first_message_ends_the_connection() {
        // dropped: the line after it is not read
        let (result, reply, handled) = run(true, "{\"type\":\"Retile\"}\n{\"type\":\"State\"}\n");
        assert!(result.is_ok());
        assert!(handled.is_empty());
        assert_eq!(reply, "");

        // handled: the line after it is not read either
        let (result, reply, handled) = run(true, "{\"type\":\"State\"}\n{\"type\":\"Retile\"}\n");
        assert!(result.is_ok());
        assert_eq!(handled, ["State"]);
        assert_eq!(reply, "State");
    }

    #[test]
    fn a_line_that_is_not_a_message_is_an_error_and_ends_the_connection() {
        let (result, _, handled) = run(false, "nonsense\n{\"type\":\"Retile\"}\n");
        assert!(result.is_err());
        assert!(handled.is_empty());
    }

    #[test]
    fn the_policy_sees_each_message_before_it_is_handled() {
        #[derive(Clone, Default)]
        struct Seen(Arc<Mutex<Vec<String>>>);

        impl ConnectionPolicy<UnixStream> for Seen {
            fn on_message(&self, message: &SocketMessage, _stream: &UnixStream) {
                self.0.lock().push(message.to_string());
            }
        }

        let wm = Arc::new(Mutex::new(Recorder::new(false)));
        let (server, mut client) = UnixStream::pair().unwrap();
        client
            .write_all(b"{\"type\":\"Retile\"}\n{\"type\":\"State\"}\n")
            .unwrap();
        client.shutdown(std::net::Shutdown::Write).unwrap();

        let seen = Seen::default();
        read_commands::<Recorder, _>(&wm, server, &seen).unwrap();
        assert_eq!(*seen.0.lock(), ["Retile", "State"]);
    }

    #[test]
    fn the_listener_hands_each_connection_to_the_window_manager() {
        let wm = Arc::new(Mutex::new(Recorder::new(false)));
        let path = wm.lock().path.clone();

        listen_for_commands::<Recorder, _>(wm.clone(), NoPolicy);

        let mut client = UnixStream::connect(&path).unwrap();
        client.write_all(b"{\"type\":\"Retile\"}\n").unwrap();
        client.shutdown(std::net::Shutdown::Write).unwrap();
        let mut reply = String::new();
        client.read_to_string(&mut reply).unwrap();

        assert_eq!(reply, "Retile");
        assert_eq!(wm.lock().handled, ["Retile"]);
    }
}
