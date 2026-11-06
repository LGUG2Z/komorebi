//! The pages of `docs/cli` that are checked in are what the command line renders today.
//!
//! The identifiers are worded per platform (`ApplicationIdentifier::Class` is "Class" on Windows,
//! "Accessibility subrole or role, such as AXStandardWindow" on macOS and "Role or subrole" on Linux),
//! and the command line renders the wording of the host it is built for: only a macOS host renders the
//! pages of `docs/macos/cli` as they are checked in. So the test is built on macOS only, and the macOS
//! job of ci runs it (`_macos.yaml`); on another host there is nothing to run, and nothing that passes
//! without having checked.

#![cfg(target_os = "macos")]

use komorebic_core::cli::Meta;
use komorebic_core::cli::command;
use komorebic_core::docgen::DocFormat;
use komorebic_core::docgen::pages;
use komorebic_core::platform::macos::MacosCommands;
use std::path::Path;

const META: Meta = Meta {
    name: "komorebic",
    about: "",
    author: "",
    version: "",
};

/// The pages of komorebic for macOS have a heading, and the usage line has the name of the binary.
fn macos_page(name: &str, page: &str) -> String {
    let command = name.trim_end_matches(".md");
    let page = page.replacen(
        &format!("Usage: {command}"),
        &format!("Usage: komorebic {command}"),
        1,
    );
    format!("# {command}\n\n{page}")
}

#[test]
fn every_macos_page_is_as_checked_in() {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../docs/macos/cli");
    assert!(
        dir.is_dir(),
        "the pages of the macOS command line are not in {dir:?}"
    );
    let rendered = pages(command::<MacosCommands>(&META), &DocFormat::PLAIN);
    let mut stale = Vec::new();
    for (name, page) in &rendered {
        let checked_in = std::fs::read_to_string(dir.join(name)).unwrap_or_default();
        if checked_in != macos_page(name, page) {
            stale.push(name.as_str());
        }
    }
    assert!(stale.is_empty(), "stale pages in docs/macos/cli: {stale:?}");
    assert_eq!(rendered.len(), std::fs::read_dir(&dir).unwrap().count());
}
