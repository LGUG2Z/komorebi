#![cfg(target_os = "linux")]
//! `komorebi` connects to KWin unless it is told to run against the in-memory desktop, says
//! what went wrong when it cannot, and then leaves the socket of a komorebi that is running
//! alone.

use std::os::unix::net::UnixListener;
use std::os::unix::net::UnixStream;
use std::path::Path;
use std::process::Command;
use std::process::Stdio;
use std::time::Duration;

/// A bus that is not there, so that a test never reaches the KWin of the session it runs in
const NO_BUS: &str = "unix:path=/nonexistent/komorebi-test.bus";

/// A runtime directory of the test's own, made empty, which is also its home: komorebi makes
/// its log directory in the home and its data directory in the runtime directory, and binds its
/// socket there with `--fake-backend`, before anything else, so the user's are kept out of it,
/// and it is writable where the user's home is not (the nix sandbox). /tmp: short enough for a
/// unix socket path on macOS.
fn runtime(test: &str) -> String {
    let runtime = format!("/tmp/komorebi-no-kwin-{test}-{}", std::process::id());
    let _ = std::fs::remove_dir_all(&runtime);
    std::fs::create_dir_all(&runtime).unwrap();
    runtime
}

#[test]
fn without_a_session_bus_komorebi_exits_with_an_error_naming_it() {
    let runtime = runtime("bus");
    let output = Command::new(env!("CARGO_BIN_EXE_komorebi"))
        .env("DBUS_SESSION_BUS_ADDRESS", NO_BUS)
        .env("XDG_RUNTIME_DIR", &runtime)
        .env("HOME", &runtime)
        .env_remove("XDG_DATA_HOME")
        .output()
        .expect("komorebi runs");
    let _ = std::fs::remove_dir_all(&runtime);

    assert_eq!(output.status.code(), Some(1));

    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("could not connect to the session bus as org.komorebi.Bridge"),
        "{stderr}"
    );
}

#[test]
fn without_kwin_komorebi_leaves_the_socket_it_finds_alone() {
    let runtime = runtime("socket");
    let socket = Path::new(&runtime).join("komorebi").join("komorebi.sock");
    std::fs::create_dir_all(socket.parent().unwrap()).unwrap();
    // the socket of the komorebi that is running
    let running = UnixListener::bind(&socket).unwrap();

    let output = Command::new(env!("CARGO_BIN_EXE_komorebi"))
        .env("DBUS_SESSION_BUS_ADDRESS", NO_BUS)
        .env("XDG_RUNTIME_DIR", &runtime)
        .env("HOME", &runtime)
        .env_remove("XDG_DATA_HOME")
        .output()
        .expect("komorebi runs");

    let still_there = UnixStream::connect(&socket).is_ok();
    drop(running);
    let _ = std::fs::remove_dir_all(&runtime);

    assert_eq!(output.status.code(), Some(1));
    assert!(still_there, "the socket of the running komorebi was taken");
}

#[test]
fn with_the_fake_backend_komorebi_runs_without_a_session_bus() {
    let runtime = runtime("fake");
    let mut daemon = Command::new(env!("CARGO_BIN_EXE_komorebi"))
        .args(["--fake-backend", "--log-level", "error"])
        .env("DBUS_SESSION_BUS_ADDRESS", NO_BUS)
        .env("XDG_RUNTIME_DIR", &runtime)
        .env("HOME", &runtime)
        .env_remove("XDG_DATA_HOME")
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .expect("komorebi runs");

    std::thread::sleep(Duration::from_millis(1500));
    let exited = daemon.try_wait().expect("komorebi can be waited for");

    daemon.kill().expect("komorebi can be stopped");
    daemon.wait().expect("komorebi can be waited for");
    let _ = std::fs::remove_dir_all(&runtime);
    assert!(exited.is_none(), "komorebi exited: {exited:?}");
}
