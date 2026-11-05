//! One window, however many times the program is started: the window's local socket.
//!
//! The first `komorebi-recorder` to start a window (with no command, `gui` or `view`) listens on a
//! unix domain socket in the per-user data directory ([`default_socket`]); every later one connects
//! to it, asks it to open the sessions as tabs or to show the Record tab, and exits, instead of
//! making a window of its own. The socket types are the ones
//! [`crate::komorebi_link`] uses (std's on unix, the `uds_windows` crate's on Windows), so there is
//! one place where the two hosts differ.
//!
//! # Protocol
//!
//! A request is one line of JSON, [`Request`], for example
//! `{"open":["/recordings/20261028T154619Z"],"focus":true}`. A request may also say which tab to
//! show, `"tab":"record"` (what `gui` sends: see [`TabName`]); a request without `"tab"`, which is
//! what older programs send, is still accepted. The server answers with one line,
//! [`Reply`] (`{"ok":true}`, or `{"ok":false,"error":"..."}`), and closes the connection. An empty
//! `open` asks only for the window to be raised and a start-screen tab to be there. Paths must be
//! absolute: the viewer runs somewhere else, so a relative path would mean nothing to it
//! ([`absolute`] makes them so before they are sent).
//!
//! The server reads at most [`MAX_LINE`] bytes of a request and waits at most [`TIMEOUT`] for
//! it, ignores what is not a request (answering with an error) and never panics on input.
//!
//! # Who becomes the server
//!
//! [`claim`] does it all: it tries to hand the request to a running viewer; if none answers it
//! binds the socket, removing a stale socket file (left by a viewer that did not exit normally)
//! when nothing is listening on it, and the caller becomes the viewer. Two viewers started at the
//! same moment resolve as one binding and the other finding the bind taken and forwarding. A viewer
//! that dies between accepting and answering looks like no viewer, so the request is served by the
//! caller instead of being lost. The one race left is two viewers both finding a *stale* file at
//! the same moment, which can leave two windows; it is harmless and cannot lose a request.
//!
//! The server's socket file is removed when the [`Server`] is dropped, which is when the viewer
//! exits normally.
//!
//! Every function takes the socket path as a parameter, so tests use a temporary one and never the
//! user's real socket.

use crate::komorebi_link::UnixListener;
use crate::komorebi_link::UnixStream;
use serde::Deserialize;
use serde::Serialize;
use std::io::BufRead;
use std::io::Read;
use std::io::Write;
use std::path::Path;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::AtomicBool;
use std::sync::atomic::Ordering;
use std::time::Duration;

/// The name of the viewer's socket, inside komorebi's data directory.
pub const SOCKET_NAME: &str = "komorebi-recorder-viewer.sock";

/// The environment variable that replaces the socket path ([`default_socket`]). It exists so
/// that a viewer can be tried out, or tested end to end, without touching the user's real one.
pub const SOCKET_VARIABLE: &str = "KOMOREBI_RECORDER_VIEWER_SOCKET";

/// The longest request line the server reads, in bytes. A longer one is refused.
pub const MAX_LINE: u64 = 1 << 20;

/// How long either side waits on the other for one read or write.
pub const TIMEOUT: Duration = Duration::from_secs(2);

/// What to ask the viewer to do.
///
/// # Example
///
/// ```rust
/// use komorebi_recorder::viewer::ipc::{Request, TabName};
///
/// let request = Request { open: vec!["/recordings/a".into()], ..Request::default() };
/// assert_eq!(
///     serde_json::to_string(&request)?,
///     r#"{"open":["/recordings/a"],"focus":true}"#
/// );
///
/// // both fields may be left out
/// let empty: Request = serde_json::from_str("{}")?;
/// assert!(empty.open.is_empty() && empty.focus && empty.tab.is_none());
///
/// // `gui` asks for the Record tab
/// let record = Request { tab: Some(TabName::Record), ..Request::default() };
/// assert_eq!(serde_json::to_string(&record)?, r#"{"open":[],"focus":true,"tab":"record"}"#);
/// # Ok::<(), serde_json::Error>(())
/// ```
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Request {
    /// The sessions to open, as absolute paths. Empty means: open a start-screen tab (unless the
    /// current one is already one) and raise the window.
    #[serde(default)]
    pub open: Vec<PathBuf>,
    /// Whether to raise the window and un-minimise it. The default is `true`.
    #[serde(default = "yes")]
    pub focus: bool,
    /// Which tab to show once the sessions are open. `None`, the default and what `view` sends,
    /// shows the last session opened (or a start screen, when there is none).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tab: Option<TabName>,
}

/// A tab a [`Request`] can ask for by name.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum TabName {
    /// The pinned Record tab, first in the window.
    Record,
}

fn yes() -> bool {
    true
}

impl Default for Request {
    fn default() -> Self {
        Self {
            open: Vec::new(),
            focus: true,
            tab: None,
        }
    }
}

/// The server's one-line answer to a [`Request`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Reply {
    /// Whether the request was understood and handed to the window.
    pub ok: bool,
    /// Why not, when `ok` is false.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

impl Reply {
    fn accepted() -> Self {
        Self {
            ok: true,
            error: None,
        }
    }

    fn refused(error: impl Into<String>) -> Self {
        Self {
            ok: false,
            error: Some(error.into()),
        }
    }
}

/// Returns the socket path to use: `$KOMOREBI_RECORDER_VIEWER_SOCKET` if it is set and not
/// empty, else [`SOCKET_NAME`] in komorebi's data directory
/// ([`data_dir`](crate::komorebi_link::data_dir)).
///
/// Only the edges of the program call this (the `gui` and `view` commands); everything else takes
/// the path as a parameter.
#[must_use]
pub fn default_socket() -> PathBuf {
    match std::env::var_os(SOCKET_VARIABLE) {
        Some(path) if !path.is_empty() => PathBuf::from(path),
        _ => crate::komorebi_link::socket_path(SOCKET_NAME),
    }
}

/// Makes `path` absolute against the current directory, without looking at the disk, so a request
/// means the same thing in the viewer's process as in this one.
#[must_use]
pub fn absolute(path: &Path) -> PathBuf {
    std::path::absolute(path).unwrap_or_else(|_| path.to_path_buf())
}

/// What came of handing a request to a viewer.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Delivery {
    /// A viewer took it and said so.
    Delivered,
    /// Nothing is listening on the socket (no file, or nobody behind it).
    NoViewer,
    /// Something was listening but went away before answering: treated as no viewer.
    Died,
}

/// Hands `request` to the viewer listening on `socket`, if there is one.
///
/// # Errors
///
/// Returns an error if a viewer answered that it refused the request (kind `InvalidInput`, with its
/// reason), or if writing the request failed for a reason other than the viewer going away.
pub fn send(socket: &Path, request: &Request) -> std::io::Result<Delivery> {
    use std::io::ErrorKind;

    let mut stream = match UnixStream::connect(socket) {
        Ok(stream) => stream,
        // no file, a refusal, or a file that is not a socket at all: nobody is there to answer
        Err(_) => return Ok(Delivery::NoViewer),
    };
    stream.set_read_timeout(Some(TIMEOUT))?;
    stream.set_write_timeout(Some(TIMEOUT))?;

    let mut line = serde_json::to_string(request).map_err(std::io::Error::other)?;
    line.push('\n');
    if let Err(error) = stream
        .write_all(line.as_bytes())
        .and_then(|()| stream.flush())
    {
        return match error.kind() {
            ErrorKind::BrokenPipe
            | ErrorKind::ConnectionReset
            | ErrorKind::ConnectionAborted
            | ErrorKind::TimedOut
            | ErrorKind::WouldBlock => Ok(Delivery::Died),
            _ => Err(error),
        };
    }

    let mut answer = String::new();
    let read = std::io::BufReader::new(&stream)
        .take(MAX_LINE)
        .read_line(&mut answer);
    match read {
        Ok(0) => Ok(Delivery::Died),
        Ok(_) => match serde_json::from_str::<Reply>(answer.trim()) {
            Ok(Reply { ok: true, .. }) => Ok(Delivery::Delivered),
            Ok(Reply { error, .. }) => Err(std::io::Error::new(
                ErrorKind::InvalidInput,
                error.unwrap_or_else(|| "the viewer refused the request".to_string()),
            )),
            // something that is not our viewer is on the socket
            Err(_) => Ok(Delivery::Died),
        },
        Err(error)
            if matches!(
                error.kind(),
                ErrorKind::ConnectionReset
                    | ErrorKind::ConnectionAborted
                    | ErrorKind::TimedOut
                    | ErrorKind::WouldBlock
                    | ErrorKind::UnexpectedEof
            ) =>
        {
            Ok(Delivery::Died)
        }
        Err(error) => Err(error),
    }
}

/// A bound socket that is not being served yet: the result of [`claim`] when this process is
/// the viewer.
///
/// It is made before the window is, and started with [`Bound::serve`] once the window's context
/// exists.
pub struct Bound {
    listener: UnixListener,
    path: PathBuf,
}

/// The outcome of [`claim`].
pub enum Claim {
    /// A running viewer took the request: there is nothing more to do.
    Forwarded,
    /// Nobody answered and the socket is now ours: be the viewer.
    Serving(Bound),
}

/// Hands `request` to a running viewer, or becomes the viewer: see the [module
/// documentation](self#who-becomes-the-server).
///
/// # Errors
///
/// Returns an error if a viewer refused the request (see [`send`]), or if the socket can neither
/// be answered nor bound (for example, its directory cannot be written to).
pub fn claim(socket: &Path, request: &Request) -> std::io::Result<Claim> {
    use std::io::ErrorKind;

    if let Some(parent) = socket.parent() {
        let _ = std::fs::create_dir_all(parent);
    }

    let mut last = None;
    for _ in 0..4 {
        if send(socket, request)? == Delivery::Delivered {
            return Ok(Claim::Forwarded);
        }

        match UnixListener::bind(socket) {
            Ok(listener) => {
                return Ok(Claim::Serving(Bound {
                    listener,
                    path: socket.to_path_buf(),
                }));
            }
            Err(error)
                if matches!(
                    error.kind(),
                    ErrorKind::AddrInUse | ErrorKind::AlreadyExists | ErrorKind::PermissionDenied
                ) && socket.exists() =>
            {
                // either another viewer bound it a moment ago, or the file is left over from one
                // that died: ask again, and only if nobody answers, clear it and go round
                if send(socket, request)? == Delivery::Delivered {
                    return Ok(Claim::Forwarded);
                }
                let _ = std::fs::remove_file(socket);
                last = Some(error);
            }
            Err(error) => return Err(error),
        }
    }
    Err(last.unwrap_or_else(|| std::io::Error::other("could not claim the viewer's socket")))
}

impl Bound {
    /// Starts answering requests on a thread of its own and returns the [`Server`] to drain them
    /// from.
    ///
    /// `notify` is called, from that thread, after each request is queued: the window uses it to ask
    /// for a repaint, so a request is taken up even when the window is idle.
    #[must_use]
    pub fn serve(self, notify: impl Fn() + Send + Sync + 'static) -> Server {
        let (sender, receiver) = crossbeam_channel::unbounded();
        let stopping = Arc::new(AtomicBool::new(false));
        let notify = Arc::new(notify);
        let flag = stopping.clone();
        let Bound { listener, path } = self;
        let thread = std::thread::Builder::new()
            .name("viewer-ipc".to_string())
            .spawn(move || {
                for stream in listener.incoming() {
                    if flag.load(Ordering::Acquire) {
                        break;
                    }
                    let Ok(stream) = stream else {
                        continue;
                    };
                    let sender = sender.clone();
                    let notify = notify.clone();
                    // a client that stalls must not hold up the next one
                    let _ = std::thread::Builder::new()
                        .name("viewer-ipc-client".to_string())
                        .spawn(move || handle(stream, &sender, &*notify));
                }
            })
            .ok();
        Server {
            path,
            receiver,
            stopping,
            thread,
        }
    }
}

/// Reads one request from `stream`, queues it and answers. Never panics on what it reads.
fn handle(
    mut stream: UnixStream,
    queue: &crossbeam_channel::Sender<Request>,
    notify: &(dyn Fn() + Send + Sync),
) {
    let _ = stream.set_read_timeout(Some(TIMEOUT));
    let _ = stream.set_write_timeout(Some(TIMEOUT));

    let mut line = Vec::new();
    let read = std::io::BufReader::new(&stream)
        .take(MAX_LINE + 1)
        .read_until(b'\n', &mut line);
    let reply = match read {
        Err(_) | Ok(0) => return,
        Ok(_) if line.len() as u64 > MAX_LINE => Reply::refused("the request is too long"),
        Ok(_) => match parse(&line) {
            Err(why) => Reply::refused(why),
            Ok(request) => {
                if queue.send(request).is_ok() {
                    notify();
                    Reply::accepted()
                } else {
                    Reply::refused("the viewer is closing")
                }
            }
        },
    };

    if let Ok(mut text) = serde_json::to_string(&reply) {
        text.push('\n');
        let _ = stream.write_all(text.as_bytes());
        let _ = stream.flush();
    }
}

/// Reads a request line, refusing what is not one and any path that is not absolute.
fn parse(line: &[u8]) -> Result<Request, String> {
    let text = std::str::from_utf8(line).map_err(|_| "the request is not text".to_string())?;
    let request: Request =
        serde_json::from_str(text.trim()).map_err(|error| format!("not a request: {error}"))?;
    if let Some(path) = request.open.iter().find(|path| !path.is_absolute()) {
        return Err(format!("{} is not an absolute path", path.display()));
    }
    Ok(request)
}

/// The viewer's end of the socket: requests arrive on [`Server::requests`].
///
/// Dropping it stops the listener thread and removes the socket file.
pub struct Server {
    path: PathBuf,
    receiver: crossbeam_channel::Receiver<Request>,
    stopping: Arc<AtomicBool>,
    thread: Option<std::thread::JoinHandle<()>>,
}

impl Server {
    /// Returns the requests that have arrived and not yet been taken, oldest first.
    #[must_use]
    pub fn requests(&self) -> Vec<Request> {
        self.receiver.try_iter().collect()
    }

    /// Returns the path of the socket being served.
    #[must_use]
    pub fn path(&self) -> &Path {
        &self.path
    }
}

impl Drop for Server {
    fn drop(&mut self) {
        self.stopping.store(true, Ordering::Release);
        // a connection wakes the listener, which is waiting in accept
        let _ = UnixStream::connect(&self.path);
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
        let _ = std::fs::remove_file(&self.path);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::AtomicUsize;
    use std::time::Instant;

    fn socket(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("kr-ipc-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("dir");
        dir.join("v.sock")
    }

    fn serving(path: &Path) -> Server {
        match claim(path, &Request::default()).expect("claim") {
            Claim::Serving(bound) => bound.serve(|| {}),
            Claim::Forwarded => panic!("a viewer was already there"),
        }
    }

    fn wait_for(server: &Server, count: usize) -> Vec<Request> {
        let mut got = Vec::new();
        let deadline = Instant::now() + Duration::from_secs(5);
        while got.len() < count && Instant::now() < deadline {
            got.extend(server.requests());
            std::thread::sleep(Duration::from_millis(5));
        }
        got
    }

    fn abs(name: &str) -> PathBuf {
        absolute(Path::new(name))
    }

    #[test]
    fn a_request_goes_to_the_server_and_is_acknowledged() {
        let path = socket("round");
        let woken = Arc::new(AtomicUsize::new(0));
        let counter = woken.clone();
        let Claim::Serving(bound) = claim(&path, &Request::default()).expect("claim") else {
            panic!("nobody is serving yet");
        };
        let server = bound.serve(move || {
            counter.fetch_add(1, Ordering::SeqCst);
        });

        let request = Request {
            open: vec![abs("a"), abs("b")],
            focus: true,
            ..Request::default()
        };
        assert_eq!(send(&path, &request).expect("sent"), Delivery::Delivered);
        assert_eq!(wait_for(&server, 1), [request]);
        assert_eq!(woken.load(Ordering::SeqCst), 1);

        // a second claim forwards instead of binding
        let more = Request {
            open: vec![abs("c")],
            focus: false,
            ..Request::default()
        };
        assert!(matches!(
            claim(&path, &more).expect("claim"),
            Claim::Forwarded
        ));
        assert_eq!(wait_for(&server, 1), [more]);
    }

    #[test]
    fn the_tab_is_additive_and_old_requests_still_read() {
        let old: Request = serde_json::from_str(r#"{"open":["/a"],"focus":false}"#).expect("old");
        assert_eq!(old.tab, None);
        let new: Request = serde_json::from_str(r#"{"tab":"record"}"#).expect("new");
        assert_eq!(new.tab, Some(TabName::Record));
        assert!(serde_json::from_str::<Request>(r#"{"tab":"nowhere"}"#).is_err());

        let path = socket("tab");
        let server = serving(&path);
        let request = Request {
            tab: Some(TabName::Record),
            ..Request::default()
        };
        assert_eq!(send(&path, &request).expect("sent"), Delivery::Delivered);
        assert_eq!(wait_for(&server, 1), [request]);
    }

    #[test]
    fn nothing_is_delivered_when_nobody_listens() {
        let path = socket("nobody");
        assert_eq!(
            send(&path, &Request::default()).expect("sent"),
            Delivery::NoViewer
        );
    }

    #[test]
    fn the_socket_file_goes_when_the_server_does() {
        let path = socket("cleanup");
        let server = serving(&path);
        assert!(path.exists());
        drop(server);
        assert!(!path.exists());
        assert_eq!(
            send(&path, &Request::default()).expect("sent"),
            Delivery::NoViewer
        );
    }

    #[test]
    fn a_stale_socket_file_is_cleared_and_claimed() {
        let path = socket("stale");
        // a listener that is dropped without removing its file leaves one behind
        drop(UnixListener::bind(&path).expect("bind"));
        assert!(path.exists(), "the file is left behind");

        let server = serving(&path);
        let request = Request {
            open: vec![abs("x")],
            focus: true,
            ..Request::default()
        };
        assert_eq!(send(&path, &request).expect("sent"), Delivery::Delivered);
        assert_eq!(wait_for(&server, 1), [request]);
    }

    #[test]
    fn a_file_that_is_not_a_socket_is_replaced() {
        let path = socket("regular");
        std::fs::write(&path, "left by something else").expect("write");
        let server = serving(&path);
        drop(server);
    }

    #[test]
    fn two_viewers_starting_together_make_one_server() {
        let path = socket("race");
        let mut threads = Vec::new();
        for index in 0..6 {
            let path = path.clone();
            threads.push(std::thread::spawn(move || {
                let request = Request {
                    open: vec![abs(&format!("s{index}"))],
                    focus: true,
                    ..Request::default()
                };
                match claim(&path, &request).expect("claim") {
                    Claim::Forwarded => None,
                    Claim::Serving(bound) => Some((bound.serve(|| {}), request)),
                }
            }));
        }
        let results: Vec<_> = threads
            .into_iter()
            .map(|thread| thread.join().expect("thread"))
            .collect();
        let servers: Vec<_> = results.into_iter().flatten().collect();
        assert_eq!(servers.len(), 1, "exactly one of them is the viewer");

        // every other request reached it; the server's own was not sent to itself
        let (server, own) = &servers[0];
        let got = wait_for(server, 5);
        assert_eq!(got.len(), 5, "{got:?}");
        assert!(!got.contains(own));
    }

    #[test]
    fn what_is_not_a_request_is_refused_and_the_server_carries_on() {
        let path = socket("garbage");
        let server = serving(&path);

        let ask = |bytes: &[u8]| -> Option<Reply> {
            let mut stream = UnixStream::connect(&path).expect("connect");
            stream.set_read_timeout(Some(TIMEOUT)).expect("timeout");
            // the server may answer and close before everything is written
            let _ = stream.write_all(bytes);
            let mut answer = String::new();
            std::io::BufReader::new(&stream)
                .read_line(&mut answer)
                .ok()?;
            serde_json::from_str(answer.trim()).ok()
        };

        for garbage in [
            &b"not json at all\n"[..],
            b"{\"open\": 7}\n",
            b"\xff\xfe\n",
            b"42\n",
            b"\"open\"\n",
            b"{\"open\":[\"relative/path\"]}\n",
        ] {
            let reply = ask(garbage).expect("an answer");
            assert!(!reply.ok, "{}", String::from_utf8_lossy(garbage));
            assert!(reply.error.is_some());
        }

        // far too long, with no end of line
        let long = vec![b'a'; usize::try_from(MAX_LINE).expect("fits") + 10];
        if let Some(reply) = ask(&long) {
            assert!(!reply.ok);
        }

        // a client that connects and says nothing, or hangs up, changes nothing
        drop(UnixStream::connect(&path).expect("connect"));
        assert!(server.requests().is_empty());

        // and a good request still works afterwards
        let request = Request {
            open: vec![abs("fine")],
            focus: true,
            ..Request::default()
        };
        assert_eq!(send(&path, &request).expect("sent"), Delivery::Delivered);
        assert_eq!(wait_for(&server, 1), [request]);
    }

    #[test]
    fn a_refused_request_is_an_error_to_the_sender() {
        let path = socket("refused");
        let _server = serving(&path);
        let relative = Request {
            open: vec![PathBuf::from("relative")],
            focus: true,
            ..Request::default()
        };
        let error = send(&path, &relative).expect_err("refused");
        assert_eq!(error.kind(), std::io::ErrorKind::InvalidInput);
        assert!(error.to_string().contains("absolute"), "{error}");
    }

    #[test]
    fn a_server_that_dies_mid_request_looks_like_no_viewer() {
        let path = socket("died");
        let listener = UnixListener::bind(&path).expect("bind");
        let thread = std::thread::spawn(move || {
            // accepts, reads the request, and hangs up without answering
            let (stream, _) = listener.accept().expect("accept");
            let mut line = String::new();
            let _ = std::io::BufReader::new(&stream).read_line(&mut line);
            drop(stream);
        });

        let outcome = send(&path, &Request::default()).expect("no error");
        assert_eq!(outcome, Delivery::Died);
        thread.join().expect("thread");
    }

    #[test]
    fn the_default_socket_is_in_the_data_dir_unless_overridden() {
        // only the pure parts: the environment is shared by every test
        assert!(
            default_socket().ends_with(SOCKET_NAME) || std::env::var_os(SOCKET_VARIABLE).is_some()
        );
        assert!(abs("a").is_absolute());
    }
}
