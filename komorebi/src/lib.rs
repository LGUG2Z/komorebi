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
//! ([`window_manager_event`], [`process_event`]), which reaches it through
//! [`window_manager_event_listener`].
//!
//! On KDE Plasma the backend is [`kwin::KWinBackend`], which talks to a script it loads into KWin.

use color_eyre::eyre;
use lazy_static::lazy_static;
use std::path::PathBuf;
use std::sync::atomic::AtomicI32;

#[macro_use]
pub mod ring;

pub mod backend;
pub mod container;
pub mod core;
pub mod desktop;
pub mod kwin;
pub mod lockable_sequence;
pub mod monitor;
pub mod process_event;
pub mod window;
pub mod window_manager;
pub mod window_manager_event;
pub mod window_manager_event_listener;
pub mod workspace;

/// The error the window manager returns when it talks to the desktop: a report, since every
/// failure comes from the one backend.
pub type LibraryError = eyre::Report;

lazy_static! {
    /// Where komorebi keeps the files it needs while it runs, such as the script it loads into
    /// KWin: `$XDG_RUNTIME_DIR/komorebi`, which is per-user, on tmpfs and removed at logout.
    /// Without a runtime directory (a bare ssh session, or the tests on a Mac) it falls back to the
    /// local data directory.
    pub static ref DATA_DIR: PathBuf = std::env::var_os("XDG_RUNTIME_DIR")
        .filter(|dir| !dir.is_empty())
        .map(|dir| PathBuf::from(dir).join("komorebi"))
        .unwrap_or_else(|| {
            dirs::data_local_dir()
                .expect("there is no local data directory")
                .join("komorebi")
        });
}

/// The padding, in pixels, between the edge of a monitor's work area and the tiles of its
/// workspaces, for a monitor that sets none of its own. The default for this is 5.
pub static DEFAULT_WORKSPACE_PADDING: AtomicI32 = AtomicI32::new(5);
/// The padding, in pixels, around each tile, for a monitor that sets none of its own. The default
/// for this is 5.
pub static DEFAULT_CONTAINER_PADDING: AtomicI32 = AtomicI32::new(5);
