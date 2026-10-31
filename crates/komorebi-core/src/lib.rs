//! The model every komorebi daemon shares: the windows, containers, workspaces and monitors that
//! the layout works on, generic over what a platform's window is.
//!
//! The model never talks to a compositor or to Win32. It holds the platform's window as a value of
//! a type `W` and asks it, through [`ContainerWindow`], only what it cannot know itself. See
//! `docs/dev/core-model.md` for the layers and the order they move in.

#![deny(missing_docs)]

mod container;
mod globals;

pub use container::Container;
pub use container::ContainerWindow;
pub use globals::WorkspaceGlobals;
pub use komorebi_collections::Lockable;
pub use komorebi_collections::ring;
