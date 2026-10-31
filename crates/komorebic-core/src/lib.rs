//! The command line of `komorebic`, in one place for the three platforms.
//!
//! Every definition is plain clap code that is always compiled, with no `cfg`: what differs per
//! platform is a type parameter, so one binary can render the help of all of them.
//!
//! - [`args`] has the argument blocks the subcommands are built from.
//! - [`cli`] has [`cli::Opts`], the command line of a platform, generic over its own subcommands.
//! - [`launcher`] has the part of `start` and `stop` that is the same on every platform.
//! - [`messages`] has the subcommands that are one message to the daemon.
//! - [`pan`] reads the stream of touchpad gestures that `komorebic pan --stream` is given.
//! - [`platform`] has the subcommands of each platform that are not in [`messages`].
//! - [`docgen`] renders the pages of `docs/cli`.

pub mod args;
pub mod cli;
pub mod docgen;
pub mod launcher;
pub mod messages;
pub mod pan;
pub mod platform;
