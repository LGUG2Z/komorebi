//! The komorebi daemon's library: the vocabulary the window manager is written in, the data
//! structures it is built from, and the window manager.
//!
//! The desktop is modeled as monitors ([`monitor::Monitor`]), each with a ring of workspaces
//! ([`workspace::Workspace`]), each with a ring of containers ([`container::Container`]), each with
//! a ring of windows ([`window::Window`]). A workspace tiles its containers by its layout, and a
//! window is moved by asking the [`backend`].
//!
//! The [`window_manager::WindowManager`] keeps that model. It is loaded from what the backend
//! reports ([`desktop`]) and changed by what the compositor reports about windows
//! ([`window_manager_event`], [`process_event`]).

use color_eyre::eyre;
use std::sync::atomic::AtomicI32;

#[macro_use]
pub mod ring;

pub mod backend;
pub mod container;
pub mod core;
pub mod desktop;
pub mod lockable_sequence;
pub mod monitor;
pub mod process_event;
pub mod window;
pub mod window_manager;
pub mod window_manager_event;
pub mod workspace;

/// The error the window manager returns when it talks to the desktop: a report, since every
/// failure comes from the one backend.
pub type LibraryError = eyre::Report;

/// The padding, in pixels, between the edge of a monitor's work area and the tiles of its
/// workspaces, for a monitor that sets none of its own. The default for this is 5.
pub static DEFAULT_WORKSPACE_PADDING: AtomicI32 = AtomicI32::new(5);
/// The padding, in pixels, around each tile, for a monitor that sets none of its own. The default
/// for this is 5.
pub static DEFAULT_CONTAINER_PADDING: AtomicI32 = AtomicI32::new(5);
