use crate::LOG_TARGET;
use komorebi_protocol::types::SubscribeOptions;
use std::collections::HashMap;
use std::io::Write;
use std::path::Path;
use std::path::PathBuf;

/// Pushes `notification` (one serialised notification) to the subscriber sockets, and removes
/// the subscribers that cannot be connected to
///
/// The daemon keeps the registry (`sockets`: name to socket path; `options`: name to options)
/// and the lock on it, and decides whether a notification is pushed to a subscriber that only
/// wants the changes of the state: `push_unchanged` is true when the state has been modified, or
/// when the event is one of the events the platform always pushes. `connect` opens the socket at
/// a path (the Windows daemon has its own socket type).
///
/// A subscriber whose socket cannot be connected to is stale: it is removed from `sockets`, and
/// its socket file, which is `data_dir` joined with its name, is removed (a failure to do that is
/// logged).
///
/// # Errors
///
/// Writing to a subscriber that was connected fails: the subscribers after it are not pushed to,
/// and the stale ones found so far are not removed.
pub fn notify_sockets<S: Write>(
    notification: &str,
    sockets: &mut HashMap<String, PathBuf>,
    options: &HashMap<String, SubscribeOptions>,
    push_unchanged: bool,
    data_dir: &Path,
    connect: impl Fn(&Path) -> std::io::Result<S>,
) -> std::io::Result<()> {
    let mut stale_sockets = vec![];

    for (socket, path) in &mut *sockets {
        let apply_state_filter = options
            .get(socket)
            .copied()
            .unwrap_or_default()
            .filter_state_changes;

        if !apply_state_filter || push_unchanged {
            match connect(path) {
                Ok(mut stream) => {
                    tracing::debug!(target: LOG_TARGET, "pushed notification to subscriber: {socket}");
                    stream.write_all(notification.as_bytes())?;
                }
                Err(_) => {
                    stale_sockets.push(socket.clone());
                }
            }
        }
    }

    for socket in stale_sockets {
        tracing::warn!(target: LOG_TARGET, "removing stale subscription: {socket}");
        sockets.remove(&socket);
        let socket_path = data_dir.join(socket);
        if let Err(error) = std::fs::remove_file(&socket_path) {
            tracing::error!(
                target: LOG_TARGET,
                "could not remove stale subscriber socket file at {}: {error}",
                socket_path.display()
            )
        }
    }

    Ok(())
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use std::io::Read;
    use std::os::unix::net::UnixListener;
    use std::os::unix::net::UnixStream;

    fn dir(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("komorebi-sub-{}-{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn a_subscriber_gets_the_notification_and_a_stale_one_is_removed() {
        let dir = dir("push");
        let live = UnixListener::bind(dir.join("bar.sock")).unwrap();
        std::fs::write(dir.join("gone.sock"), "").unwrap();
        let mut sockets = HashMap::from([
            ("bar.sock".to_string(), dir.join("bar.sock")),
            ("gone.sock".to_string(), dir.join("gone.sock")),
        ]);

        notify_sockets("{}", &mut sockets, &HashMap::new(), false, &dir, |path| {
            UnixStream::connect(path)
        })
        .unwrap();

        let mut got = String::new();
        live.accept().unwrap().0.read_to_string(&mut got).unwrap();
        assert_eq!(got, "{}");
        assert!(!sockets.contains_key("gone.sock"));
        assert!(sockets.contains_key("bar.sock"));
        assert!(!dir.join("gone.sock").exists());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_subscriber_that_wants_changes_only_is_skipped_unless_pushed_anyway() {
        let dir = dir("filter");
        let live = UnixListener::bind(dir.join("bar.sock")).unwrap();
        live.set_nonblocking(true).unwrap();
        let mut sockets = HashMap::from([("bar.sock".to_string(), dir.join("bar.sock"))]);
        let options = HashMap::from([(
            "bar.sock".to_string(),
            SubscribeOptions {
                filter_state_changes: true,
            },
        )]);

        notify_sockets("{}", &mut sockets, &options, false, &dir, |path| {
            UnixStream::connect(path)
        })
        .unwrap();
        assert!(live.accept().is_err());

        notify_sockets("{}", &mut sockets, &options, true, &dir, |path| {
            UnixStream::connect(path)
        })
        .unwrap();
        assert!(live.accept().is_ok());
        let _ = std::fs::remove_dir_all(&dir);
    }
}
