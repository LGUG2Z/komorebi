//! The command line of one platform: [`Messages`] and the subcommands of the platform.

use crate::messages::Messages;
use clap::Command;
use clap::CommandFactory;
use clap::FromArgMatches;
use clap::Parser;
use clap::Subcommand;

/// What the top of the help and `--version` say. It is the platform's, so each `komorebic` passes
/// its own (the package description, the authors and its long version).
#[derive(Clone, Copy, Debug)]
pub struct Meta {
    pub name: &'static str,
    pub about: &'static str,
    pub author: &'static str,
    pub version: &'static str,
}

/// A subcommand: one of [`Messages`], or one that only this platform has, or has differently.
///
/// The two are flattened, so they are all subcommands of `komorebic` and none is nested.
#[derive(Subcommand, Clone, Debug)]
pub enum Cmd<P: Subcommand> {
    #[command(flatten)]
    Messages(Messages),
    #[command(flatten)]
    Platform(P),
}

/// The whole command line, generic over the subcommands of the platform.
#[derive(Parser, Debug)]
pub struct Opts<P: Subcommand + 'static> {
    #[command(subcommand)]
    pub cmd: Cmd<P>,
}

/// The clap command of a platform, with its top-level texts.
pub fn command<P: Subcommand + 'static>(meta: &Meta) -> Command {
    Opts::<P>::command()
        .name(meta.name)
        .about(meta.about)
        .author(meta.author)
        .version(meta.version)
}

/// Parse the arguments of the process, and exit with the usage if they are wrong.
pub fn parse<P: Subcommand + 'static>(meta: &Meta) -> Opts<P> {
    let matches = command::<P>(meta).get_matches();
    Opts::<P>::from_arg_matches(&matches).unwrap_or_else(|error| error.exit())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Subcommand, Clone, Debug)]
    enum Only {
        /// A command of one platform
        Special,
    }

    const META: Meta = Meta {
        name: "komorebic",
        about: "about",
        author: "author",
        version: "1.2.3",
    };

    #[test]
    fn every_message_builds_next_to_a_platform_command() {
        // building the command asserts that the names, aliases and arguments are consistent
        command::<Only>(&META).debug_assert();
    }

    #[test]
    fn a_message_and_a_platform_command_are_both_top_level() {
        let focus = Opts::<Only>::try_parse_from(["komorebic", "focus", "left"]).unwrap();
        assert!(matches!(focus.cmd, Cmd::Messages(Messages::Focus(_))));
        let special = Opts::<Only>::try_parse_from(["komorebic", "special"]).unwrap();
        assert!(matches!(special.cmd, Cmd::Platform(Only::Special)));
    }

    #[test]
    fn the_top_level_texts_are_the_platforms() {
        let cmd = command::<Only>(&META);
        assert_eq!(cmd.get_name(), "komorebic");
        assert_eq!(cmd.get_version(), Some("1.2.3"));
        assert_eq!(
            cmd.get_about().map(ToString::to_string).as_deref(),
            Some("about")
        );
    }
}
