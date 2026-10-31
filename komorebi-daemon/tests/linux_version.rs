#![cfg(target_os = "linux")]
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
    // the version the KDE daemon reports is that of the KDE `version` crate (the workspace version of
    // komorebi for KDE), not of this package, which is shared with the other platforms
    let manifest = include_str!("../../komorebi-for-kde/version/Cargo.toml");
    let version = manifest
        .lines()
        .find_map(|line| line.strip_prefix("version = \""))
        .and_then(|rest| rest.strip_suffix('"'))
        .expect("the version crate has a version");
    let expected = format!("komorebi {version}");

    assert_eq!(stdout.lines().next(), Some(expected.as_str()));
}
