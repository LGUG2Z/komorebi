//! Understanding a recorded session without a window.
//!
//! Everything here reads a session directory and produces plain serializable values, so the same
//! answers serve the command line and anything else that wants them.
//! This module holds the parts every command shares: the JSON envelope and exit codes
//! ([`envelope`]) and how an event or a log line is named ([`ids`]).

pub mod classify;
pub mod envelope;
pub mod events;
pub mod flavor;
pub mod ids;
