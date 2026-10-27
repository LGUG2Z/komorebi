//! `komorebi` connects to KWin unless it is told to run against the in-memory desktop, and says
//! what went wrong when it cannot.

use std::process::Command;
use std::process::Stdio;
use std::time::Duration;

/// A bus that is not there, so that a test never reaches the KWin of the session it runs in
const NO_BUS: &str = "unix:path=/nonexistent/komorebi-test.bus";

#[test]
fn without_a_session_bus_komorebi_exits_with_an_error_naming_it() {
    let output = Command::new(env!("CARGO_BIN_EXE_komorebi"))
        .env("DBUS_SESSION_BUS_ADDRESS", NO_BUS)
        .output()
        .expect("komorebi runs");

    assert_eq!(output.status.code(), Some(1));

    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("could not connect to the session bus as org.komorebi.Bridge"),
        "{stderr}"
    );
}

#[test]
fn with_the_fake_backend_komorebi_runs_without_a_session_bus() {
    let mut daemon = Command::new(env!("CARGO_BIN_EXE_komorebi"))
        .args(["--fake-backend", "--log-level", "error"])
        .env("DBUS_SESSION_BUS_ADDRESS", NO_BUS)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .expect("komorebi runs");

    std::thread::sleep(Duration::from_millis(1500));
    let exited = daemon.try_wait().expect("komorebi can be waited for");

    daemon.kill().expect("komorebi can be stopped");
    daemon.wait().expect("komorebi can be waited for");
    assert!(exited.is_none(), "komorebi exited: {exited:?}");
}
