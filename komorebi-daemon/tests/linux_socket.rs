#![cfg(target_os = "linux")]
//! The daemon over its socket: `komorebi --fake-backend` answers on `komorebi.sock` in its
//! runtime directory, and stops when it is sent `Stop`.

use std::io::Read;
use std::io::Write;
use std::net::Shutdown;
use std::os::unix::net::UnixStream;
use std::path::Path;
use std::process::Child;
use std::process::Command;
use std::process::Stdio;
use std::time::Duration;
use std::time::Instant;

/// The daemon, which is killed if the test ends before it stopped
struct Daemon(Child);

impl Drop for Daemon {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

/// Waits up to ten seconds for `done`
fn eventually(mut done: impl FnMut() -> bool) -> bool {
    let started = Instant::now();
    while started.elapsed() < Duration::from_secs(10) {
        if done() {
            return true;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    false
}

/// Writes `message` on a new connection to `socket` and returns the answer
fn send(socket: &Path, message: &str) -> std::io::Result<String> {
    let mut stream = UnixStream::connect(socket)?;
    stream.write_all(message.as_bytes())?;
    stream.shutdown(Shutdown::Write)?;

    let mut answer = String::new();
    stream.read_to_string(&mut answer)?;
    Ok(answer)
}

#[test]
fn the_daemon_answers_on_its_socket_and_stops_when_it_is_told_to() {
    // /tmp: short enough for a unix socket path on macOS
    let runtime = format!("/tmp/komorebi-socket-test-{}", std::process::id());
    let socket = Path::new(&runtime).join("komorebi").join("komorebi.sock");
    let _ = std::fs::remove_dir_all(&runtime);
    std::fs::create_dir_all(&runtime).unwrap();

    let mut daemon = Daemon(
        Command::new(env!("CARGO_BIN_EXE_komorebi"))
            .arg("--fake-backend")
            // the logs go to the home directory, which may not be writable (the nix sandbox)
            .env("XDG_RUNTIME_DIR", &runtime)
            .env("HOME", &runtime)
            .env_remove("XDG_DATA_HOME")
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .expect("komorebi runs"),
    );

    assert!(
        eventually(|| send(&socket, r#"{"type":"State"}"#).is_ok()),
        "nothing answers on {}",
        socket.display()
    );

    let monitors: serde_json::Value =
        serde_json::from_str(&send(&socket, r#"{"type":"MonitorInformation"}"#).unwrap()).unwrap();
    // the fake's one monitor
    assert_eq!(monitors[0]["size"]["right"], 1920);

    send(&socket, r#"{"type":"Stop"}"#).unwrap();

    let mut status = None;
    assert!(eventually(|| {
        status = daemon.0.try_wait().unwrap();
        status.is_some()
    }));
    assert_eq!(status.and_then(|status| status.code()), Some(0));
    assert!(!socket.exists(), "the socket is left behind");

    let _ = std::fs::remove_dir_all(&runtime);
}
