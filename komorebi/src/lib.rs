//! The komorebi daemon's library: the vocabulary the window manager is written in and the data
//! structures it is built from.
//!
//! The desktop is modeled as monitors ([`monitor::Monitor`]), each with a ring of workspaces
//! ([`workspace::Workspace`]), each with a ring of containers ([`container::Container`]), each with
//! a ring of windows ([`window::Window`]). A workspace tiles its containers by its layout, and a
//! window is moved by asking the [`backend`].

use std::sync::atomic::AtomicI32;

#[macro_use]
pub mod ring;

pub mod backend;
pub mod container;
pub mod core;
pub mod lockable_sequence;
pub mod monitor;
pub mod window;
pub mod workspace;

/// The padding, in pixels, between the edge of a monitor's work area and the tiles of its
/// workspaces, for a monitor that sets none of its own. The default for this is 5.
pub static DEFAULT_WORKSPACE_PADDING: AtomicI32 = AtomicI32::new(5);
/// The padding, in pixels, around each tile, for a monitor that sets none of its own. The default
/// for this is 5.
pub static DEFAULT_CONTAINER_PADDING: AtomicI32 = AtomicI32::new(5);
