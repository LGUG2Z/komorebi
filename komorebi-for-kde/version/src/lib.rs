//! The version string every komorebi binary reports.
//!
//! [`LONG_VERSION`] is the package version followed by the commit the binary
//! was built from and the Rust toolchain that built it. The binaries pass it to
//! their command line parsers, so `komorebi --version` and
//! `komorebic --version` report the same thing.
//!
//! # Where the commit comes from
//!
//! A build from a git checkout whose `HEAD` is on a branch asks git for the
//! branch and the commit. Any other build has nothing to ask: a detached `HEAD`
//! (which is what a jj workspace leaves), a source tree with no `.git`
//! directory, or a sandboxed nix build. Such a build reads the commit from the
//! `COMMIT_HASH` environment variable, and does not compile without it.
//! [`.cargo/config.toml`](https://github.com/LGUG2Z/komorebi-for-kde/blob/master/.cargo/config.toml)
//! defaults the variable to `unknown`, and nix sets it to the real commit.

shadow_rs::shadow!(build);

/// The package version, the commit and the toolchain of this build, one per
/// line.
///
/// The first line is the package version, which every crate in the workspace
/// shares.
///
/// # Example
///
/// ```rust
/// let first_line = version::LONG_VERSION.lines().next();
///
/// assert_eq!(first_line, Some(env!("CARGO_PKG_VERSION")));
/// ```
pub use build::LONG_VERSION;
