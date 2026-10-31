//! Every platform's command line builds, and has what the platform's `komorebic` had.

use komorebic_core::cli::Meta;
use komorebic_core::cli::command;
use komorebic_core::platform::macos::MacosCommands;
use komorebic_core::platform::windows::WindowsCommands;

const META: Meta = Meta {
    name: "komorebic",
    about: "",
    author: "",
    version: "",
};

fn names(cli: &clap::Command) -> Vec<String> {
    cli.get_subcommands()
        .map(|c| c.get_name().to_string())
        .collect()
}

#[test]
fn the_windows_command_line_builds_and_has_199_subcommands() {
    // building asserts that no two arguments of a command clash (what flattening a generic block
    // twice would trip over)
    let cli = command::<WindowsCommands>(&META);
    cli.clone().debug_assert();
    assert_eq!(names(&cli).len(), 199);
}

#[test]
fn the_macos_command_line_builds_and_has_160_subcommands() {
    let cli = command::<MacosCommands>(&META);
    cli.clone().debug_assert();
    assert_eq!(names(&cli).len(), 160);
}

#[test]
fn a_subcommand_name_is_in_one_place_only() {
    for cli in [
        command::<WindowsCommands>(&META),
        command::<MacosCommands>(&META),
    ] {
        let mut names = names(&cli);
        names.sort();
        let before = names.len();
        names.dedup();
        assert_eq!(before, names.len());
    }
}
