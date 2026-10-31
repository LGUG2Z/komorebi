//! The command line of `komorebic`, in one place for the three platforms.
//!
//! Every definition is plain clap code that is always compiled, with no `cfg`: what differs per
//! platform is a type parameter, so one binary can render the help of all of them.
//!
//! - [`docgen`] renders the pages of `docs/cli`.

pub mod docgen;
