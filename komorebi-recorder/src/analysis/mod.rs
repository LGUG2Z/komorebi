//! Understanding a recorded session without a window.
//!
//! Everything here reads a session directory and produces plain serializable values, so the same
//! answers serve the command line and anything else that wants them.
//! This module holds the parts every command shares: the JSON envelope and exit codes
//! ([`envelope`]), how a point in time is named ([`timespec`], [`ids`]), the guard that keeps file
//! access inside the session ([`guard`]) and the budget that keeps an answer from growing without
//! bound ([`budget`]).

pub mod answers;
pub mod blocks;
pub mod brief;
pub mod budget;
pub mod catalog;
pub mod changes;
pub mod classify;
pub mod code;
pub mod diff;
pub mod envelope;
pub mod events;
pub mod facade;
pub mod flavor;
pub mod guard;
pub mod history;
pub mod ids;
pub mod invariants;
pub mod logparse;
pub mod lookup;
pub mod os_trace;
pub mod overview;
pub mod probed;
pub mod schemas;
pub mod search;
pub mod sheet;
pub mod state;
pub mod timespec;
pub mod trace;
pub mod video;
pub mod views;

pub use facade::Analysis;
