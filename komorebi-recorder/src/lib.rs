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
