//! Understanding a recorded session without a window.
//!
//! Everything here reads a session directory and produces plain serializable values, so the same
//! answers serve the command line and anything else that wants them.
//! This module holds the parts every command shares: the JSON envelope and exit codes
//! ([`envelope`]), how a point in time is named ([`timespec`], [`ids`]) and the guard that keeps
//! file access inside the session ([`guard`]).

pub mod blocks;
pub mod classify;
pub mod diff;
pub mod envelope;
pub mod events;
pub mod flavor;
pub mod guard;
pub mod ids;
pub mod logparse;
pub mod lookup;
pub mod state;
pub mod timespec;
