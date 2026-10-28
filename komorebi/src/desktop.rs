//! Loads what the backend reports about the desktop (its monitors, and the windows that are
//! already open) into the window manager.
//!
//! [`WindowManager::init`] calls [`load_monitor_information`] and then
//! [`load_workspace_information`], so that starting komorebi tiles what is on screen. Both read
//! the backend installed for the calling thread ([`crate::backend::backend`]).
//!
//! # Example
//!
//! ```rust
//! use std::path::PathBuf;
//! use std::sync::Arc;
//!
//! use komorebi::backend::fake::FakeBackend;
//! use komorebi::backend::with_backend;
//! use komorebi::desktop::load_monitor_information;
//! use komorebi::desktop::load_workspace_information;
//! use komorebi::window_manager::WindowManager;
//!
//! let fake = Arc::new(FakeBackend::with_one_monitor());
//! fake.open_window(1, "foot", "foot", "a terminal");
//!
//! let (_sender, events) = crossbeam_channel::bounded(1);
//! let socket = PathBuf::from(format!("/tmp/komorebi-desktop-doc-{}.sock", std::process::id()));
//! let mut wm = WindowManager::new(events, Some(socket.clone()))?;
//! with_backend(fake, || {
//!     load_monitor_information(&mut wm)?;
//!     load_workspace_information(&mut wm)
//! })?;
//!
//! assert_eq!(wm.monitors().len(), 1);
//! assert!(wm.focused_workspace()?.contains_window(1));
//! # std::fs::remove_file(socket)?;
//! # Ok::<(), color_eyre::eyre::Report>(())
//! ```

#![deny(missing_docs)]

use crate::backend::backend;
use crate::container::Container;
use crate::core::Rect;
use crate::monitor::Monitor;
use crate::window::RuleDebug;
use crate::window::Window;
use crate::window_manager::WindowManager;
use color_eyre::eyre;

fn contains_point(area: &Rect, x: i32, y: i32) -> bool {
    x >= area.left && x < area.left + area.right && y >= area.top && y < area.top + area.bottom
}

/// Returns every connected monitor, as the window manager models it, in the backend's order.
///
/// Each [`Monitor`] has the backend's id, bounds and work area, the output's name as its device,
/// the backend's identity as its serial number id, and one empty workspace.
///
/// # Errors
///
/// When the backend cannot report its monitors
/// ([`Backend::monitors`](crate::backend::Backend::monitors)).
///
/// # Example
///
/// ```rust
/// use std::sync::Arc;
///
/// use komorebi::backend::fake::FakeBackend;
/// use komorebi::backend::with_backend;
/// use komorebi::desktop::latest_monitor_information;
///
/// let fake = Arc::new(FakeBackend::with_one_monitor());
///
/// let monitors = with_backend(fake, latest_monitor_information)?;
///
/// assert_eq!(monitors.len(), 1);
/// assert_eq!(monitors[0].device, "Virtual-0");
/// assert_eq!(monitors[0].work_area_size.bottom, 1050);
/// assert_eq!(monitors[0].workspaces().len(), 1);
/// # Ok::<(), color_eyre::eyre::Report>(())
/// ```
pub fn latest_monitor_information() -> eyre::Result<Vec<Monitor>> {
    Ok(backend()
        .monitors()?
        .into_iter()
        .map(|m| Monitor::new(m.id, m.size, m.work_area, &m.name, &m.serial_number_id))
        .collect())
}

/// Adds the monitors the window manager does not have yet, after the ones it has, in the
/// backend's order.
///
/// A monitor is known when the window manager has one with the same serial number id, so
/// loading twice adds nothing the second time, and a known monitor keeps its workspaces.
///
/// # Errors
///
/// When the backend cannot report its monitors
/// ([`Backend::monitors`](crate::backend::Backend::monitors)).
///
/// # Example
///
/// ```rust
/// use std::path::PathBuf;
/// use std::sync::Arc;
///
/// use komorebi::backend::fake::FakeBackend;
/// use komorebi::backend::with_backend;
/// use komorebi::desktop::load_monitor_information;
/// use komorebi::window_manager::WindowManager;
///
/// let fake = Arc::new(FakeBackend::with_one_monitor());
/// let (_sender, events) = crossbeam_channel::bounded(1);
/// let socket = PathBuf::from(format!("/tmp/komorebi-monitors-doc-{}.sock", std::process::id()));
/// let mut wm = WindowManager::new(events, Some(socket.clone()))?;
///
/// with_backend(fake, || {
///     load_monitor_information(&mut wm)?;
///     load_monitor_information(&mut wm)
/// })?;
///
/// assert_eq!(wm.monitors().len(), 1);
/// # std::fs::remove_file(socket)?;
/// # Ok::<(), color_eyre::eyre::Report>(())
/// ```
#[tracing::instrument(skip_all)]
pub fn load_monitor_information(wm: &mut WindowManager) -> eyre::Result<()> {
    let all_devices = latest_monitor_information()?;

    let monitors = &mut wm.monitors;

    'read: for device in all_devices {
        for monitor in monitors.elements() {
            if device.serial_number_id.eq(&monitor.serial_number_id) {
                continue 'read;
            }
        }

        monitors.elements_mut().push_back(device);
    }

    Ok(())
}

/// Puts the windows that are already open into containers on the workspace of the monitor they
/// are on, so that starting komorebi tiles what is on screen.
///
/// Each monitor is first focused on the workspace the compositor shows on it
/// ([`Backend::current_workspaces`]). A window is on the monitor whose bounds contain its middle,
/// and on the workspace whose desktop it is on ([`Backend::window_workspace`]), which need not be
/// the one in front; a window on every desktop goes to the one in front. The monitor gets as many
/// workspaces as that takes. A window that is minimized, that komorebi does not manage
/// ([`Window::should_manage`]: not a normal window, or one the rules ignore), or that is on no
/// monitor is left alone. Nothing is moved: tiling is [`WindowManager::retile_all`]'s job.
///
/// [`Backend::current_workspaces`]: crate::backend::Backend::current_workspaces
/// [`Backend::window_workspace`]: crate::backend::Backend::window_workspace
///
/// # Errors
///
/// This never returns an error: a window that cannot be placed is skipped.
///
/// # Example
///
/// ```rust
/// use std::path::PathBuf;
/// use std::sync::Arc;
///
/// use komorebi::backend::fake::FakeBackend;
/// use komorebi::backend::with_backend;
/// use komorebi::desktop::load_monitor_information;
/// use komorebi::desktop::load_workspace_information;
/// use komorebi::window_manager::WindowManager;
///
/// let fake = Arc::new(FakeBackend::with_one_monitor());
/// fake.open_window(1, "foot", "foot", "a terminal");
/// fake.open_window(2, "foot", "foot", "a dialog");
/// fake.set_normal(2, false);
///
/// let (_sender, events) = crossbeam_channel::bounded(1);
/// let socket = PathBuf::from(format!("/tmp/komorebi-windows-doc-{}.sock", std::process::id()));
/// let mut wm = WindowManager::new(events, Some(socket.clone()))?;
/// with_backend(fake, || {
///     load_monitor_information(&mut wm)?;
///     load_workspace_information(&mut wm)
/// })?;
///
/// let workspace = wm.focused_workspace()?;
/// assert!(workspace.contains_window(1));
/// assert!(!workspace.contains_window(2));
/// assert_eq!(wm.focused_workspace_idx()?, 0);
/// # std::fs::remove_file(socket)?;
/// # Ok::<(), color_eyre::eyre::Report>(())
/// ```
#[tracing::instrument(skip_all)]
pub fn load_workspace_information(wm: &mut WindowManager) -> eyre::Result<()> {
    let mut valid_window_count = 0;

    // each monitor starts out showing the workspace the compositor is showing on it
    for (monitor_id, idx) in backend().current_workspaces() {
        if let Some(monitor) = wm.monitors_mut().iter_mut().find(|m| m.id == monitor_id) {
            monitor.focus_workspace(idx)?;
        }
    }

    for info in backend().windows() {
        if info.minimized {
            continue;
        }

        let window = Window::from(info.id);
        let mut rule_debug = RuleDebug::default();
        if !window.should_manage(None, &mut rule_debug)? {
            continue;
        }

        let (x, y) = (
            info.rect.left + info.rect.right / 2,
            info.rect.top + info.rect.bottom / 2,
        );

        let Some(monitor_idx) = wm
            .monitors()
            .iter()
            .position(|m| contains_point(&m.size, x, y))
        else {
            continue;
        };

        // a window is on the workspace whose desktop it is on, not necessarily the one in front
        let Some(monitor) = wm.monitors_mut().get_mut(monitor_idx) else {
            continue;
        };
        let workspace_idx = backend()
            .window_workspace(info.id)
            .unwrap_or_else(|| monitor.focused_workspace_idx());
        monitor.ensure_workspace_count(workspace_idx + 1);

        if let Some(workspace) = monitor.workspaces_mut().get_mut(workspace_idx) {
            let mut container = Container::default();
            container.windows_mut().push_back(window);
            workspace.containers_mut().push_back(container);
            valid_window_count += 1;
        }
    }

    tracing::info!("{valid_window_count} valid windows identified");

    Ok(())
}
