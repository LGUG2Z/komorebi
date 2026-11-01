//! The commands the `commands` end-to-end scenario sweeps (`SWEEP` and `PRINTED` in
//! `e2e/lib/komorebi_vm/core/scenarios/commands.py`) are commands of the platforms' `komorebic`.
//!
//! The scenario only learns that a command is not one when it runs it in a guest (`error:
//! unrecognized subcommand`, skipped with its name). Here the command lines are parsed with the
//! platform's clap command, so a command that a platform lost, or a typo in the scenario, fails a
//! native test and not a 7 minute guest run.
//!
//! mirrors: commands / <command> (the part "the command is understood: exit status 0", for the
//! command line only; what the daemon does with it is the daemon's tests, see
//! `komorebi-for-kde/komorebi/tests/command_sweep.rs`)

use komorebic_core::cli::Meta;
use komorebic_core::cli::command;
use komorebic_core::platform::macos::MacosCommands;
use komorebic_core::platform::windows::WindowsCommands;
use std::path::Path;

const META: Meta = Meta {
    name: "komorebic",
    about: "",
    author: "",
    version: "",
};

/// The strings of the tuple named `name` in the scenario, or `None` without the e2e library (a
/// source tree that is only the crates, as the nix build of one daemon has).
fn tuple(name: &str) -> Option<Vec<String>> {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../e2e/lib/komorebi_vm/core/scenarios/commands.py");
    let source = std::fs::read_to_string(path).ok()?;
    let start = source.find(&format!("{name}: tuple[str, ...] = ("))?;
    let body = &source[start..];
    let end = body.find("\n)")?;
    Some(
        body[..end]
            .lines()
            .filter_map(|line| {
                let line = line.trim();
                line.strip_prefix('"')?
                    .strip_suffix("\",")
                    .map(str::to_string)
            })
            .collect(),
    )
}

fn parses(cli: &clap::Command, line: &str) -> Result<(), String> {
    let mut args = vec!["komorebic".to_string()];
    args.extend(
        line.replace("{config}", "/tmp/komorebi.json")
            .split_whitespace()
            .map(str::to_string),
    );
    cli.clone()
        .try_get_matches_from(args)
        .map(|_| ())
        .map_err(|e| e.to_string().lines().next().unwrap_or_default().to_string())
}

fn lines() -> Option<Vec<String>> {
    let mut lines = tuple("SWEEP")?;
    lines.extend(tuple("PRINTED")?);
    Some(lines)
}

#[test]
fn the_sweep_was_read() {
    if let Some(lines) = lines() {
        assert!(lines.len() > 200, "{} lines", lines.len());
        assert!(lines.iter().any(|l| l == "focus left"));
        assert!(lines.iter().any(|l| l == "state"));
    }
}

#[test]
fn every_command_of_the_sweep_is_a_command_of_the_windows_komorebic() {
    let Some(lines) = lines() else { return };
    let cli = command::<WindowsCommands>(&META);
    let missing: Vec<_> = lines
        .iter()
        .filter_map(|l| parses(&cli, l).err().map(|e| format!("{l}: {e}")))
        .collect();
    assert!(missing.is_empty(), "{missing:#?}");
}

#[test]
fn the_sweep_commands_the_macos_komorebic_lacks_are_the_known_eight() {
    let Some(lines) = lines() else { return };
    let cli = command::<MacosCommands>(&META);
    let mut missing: Vec<_> = lines
        .iter()
        .filter_map(|l| parses(&cli, l).err().map(|_| l.clone()))
        .collect();
    missing.sort();
    missing.dedup();
    // what the macOS command line lacks of the sweep, as of this writing: the scenario skips
    // these there (focus follows mouse by the Windows implementation, transparency, maximize)
    assert_eq!(
        missing,
        [
            "focus-follows-mouse disable",
            "focus-follows-mouse enable",
            "toggle-focus-follows-mouse --implementation windows",
            "toggle-maximize",
            "toggle-transparency",
            "transparency disable",
            "transparency enable",
            "transparency-alpha 150",
        ]
    );
    // a command that every platform must have (the sweep's first lines) is one of macOS too
    for line in [
        "focus left",
        "move right",
        "retile",
        "toggle-float",
        "state",
    ] {
        assert!(!missing.iter().any(|m| m == line), "{line}");
    }
}
