//! `komorebi` refuses to start without a backend, and says how to choose one.

use std::process::Command;

#[test]
fn without_a_backend_komorebi_exits_with_an_error_naming_the_flag() {
    let output = Command::new(env!("CARGO_BIN_EXE_komorebi"))
        .output()
        .expect("komorebi runs");

    // the exit code of a usage error
    assert_eq!(output.status.code(), Some(2));

    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("a backend has to be chosen: pass --fake-backend"),
        "{stderr}"
    );
}
