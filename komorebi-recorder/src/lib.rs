//! Synchronized screen, log and trace capture for komorebi bug reproduction. `komorebi-recorder` records a bug while it
//! happens, on macOS and on Windows: the screen, komorebi's log, every event komorebi sends its subscribers and the
//! operating system's own trace, all on one clock, into a single _session directory_. It then answers, from that
//! directory alone, what komorebi and the OS were doing at any second of the video and where in the video any log line or
//! event is: for a person, with a window that plays the recording with everything that happened laid out on a timeline,
//! and for an agent, with commands that print JSON.
//!
//! This crate is the library behind the `komorebi-recorder` command. A reader who wants to run it should start with the
//! [README](https://github.com/LGUG2Z/komorebi-recorder#readme); this documentation is for someone who is changing it,
//! or reading a session directory from a program of their own.
//!
//! # The model
//!
//! Two things vary independently. The _host_ is the OS the recorder runs on, and everything that differs by it is behind
//! [`platform`]. The _flavor_ is the komorebi being recorded, komorebi for Mac or komorebi for Windows, and everything
//! that differs by it is behind a trait in [`analysis::flavor`]. A session records both, so a session recorded on one
//! host can be read on the other.
//!
//! Every record in a session is stamped on one monotonic clock, the _master clock_ ([`clock`]). Video frames and the OS
//! trace are already on it; komorebi's log lines carry wall-clock time and are mapped onto it with paired readings of
//! both clocks. For example, a log line stamped five seconds after the first of two anchors that are ten seconds apart
//! is five seconds after it on the master clock too:
//!
//! ```rust
//! use komorebi_recorder::clock::Anchor;
//! use komorebi_recorder::clock::ClockMap;
//!
//! let map = ClockMap::new(vec![
//!     Anchor { wall_us: 1_000_000, mach_ns: 5_000_000_000 },
//!     Anchor { wall_us: 11_000_000, mach_ns: 15_000_000_000 },
//! ]);
//!
//! assert_eq!(map.wall_to_mach(6_000_000), Some(10_000_000_000));
//! ```
//!
//! # Where to start
//!
//! The modules, from the ones everything else uses to the ones that use everything:
//!
//! | module | what it is |
//! |---|---|
//! | [`timecode`] | parsing and printing video timecodes |
//! | [`clock`] | the master clock, anchors and the map between the wall clock and the master clock |
//! | [`platform`] | everything that differs by host, behind one facade |
//! | [`analysis`] | understanding a session without a window, starting with the komorebi flavors |
//! | [`komorebi_link`] | talking to a running komorebi over its socket |
//! | [`incidents`] | what happened to the recording itself, as a log of its own |
//! | [`lock`] | telling a running recorder from one that died |
//! | [`sources`] | the sources that write raw files into the session |
//! | [`session`] | the session directory and its manifest |
//! | [`capture`] | capturing the screen into segments of video |
//!
//! # What is not supported
//!
//! The recorder does not change komorebi's log level or lifecycle, it does not need `sudo` and changes no SIP setting on
//! a Mac, and it does not trace system calls. A session records what komorebi, the OS trace and the screen said, and
//! nothing from before the recording began.
//!
//! # Platform support
//!
//! The Windows host is type-checked and cross-built from a Mac and built on Windows in CI, and has never been run. See
//! [`platform`] for what each host provides.

pub mod analysis;
pub mod capture;
pub mod clock;
pub mod incidents;
pub mod komorebi_link;
pub mod lock;
pub mod platform;
pub mod session;
pub mod sources;
pub mod timecode;
