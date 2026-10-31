//! `komorebi --version` reports the workspace version.

use std::process::Command;

#[test]
fn reports_the_package_version() {
    let output = Command::new(env!("CARGO_BIN_EXE_komorebi"))
        .arg("--version")
        .output()
        .expect("komorebi runs");

    assert!(output.status.success());

    let stdout = String::from_utf8(output.stdout).expect("utf-8 output");
    let expected = format!("komorebi {}", env!("CARGO_PKG_VERSION"));

    assert_eq!(stdout.lines().next(), Some(expected.as_str()));
}
