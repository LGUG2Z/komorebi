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

use crate::DISPLAY_INDEX_PREFERENCES;
use crate::backend::backend;
use crate::container::Container;
use crate::core::Rect;
use crate::monitor::Monitor;
use crate::window::RuleDebug;
use crate::window::Window;
use crate::window_manager::WindowManager;
use color_eyre::eyre;
use std::collections::HashMap;

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

/// Adds the monitors the window manager does not have yet, and rebuilds the map from the indices
/// the user gives monitors to the window manager's ([`WindowManager::monitor_usr_idx_map`]).
///
/// A monitor the user gave an index to (`DisplayIndexPreference`) is put at that index, with
/// placeholders before it while the monitors in between are not there; any other new monitor goes
/// after the ones the window manager has, in the backend's order. A monitor is known when the
/// window manager has one with the same serial number id, so loading twice adds nothing the
/// second time, and a known monitor keeps its workspaces.
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
    let monitor_usr_idx_map = &mut wm.monitor_usr_idx_map;

    'read: for device in all_devices {
        for monitor in monitors.elements() {
            if device.serial_number_id.eq(&monitor.serial_number_id) {
                continue 'read;
            }
        }

        let m = device.clone();

        let mut index_preference = None;
        let display_index_preferences = DISPLAY_INDEX_PREFERENCES.read();
        for (index, id) in &*display_index_preferences {
            if m.serial_number_id.eq(id) {
                index_preference = Option::from(index);
            }
        }

        if let Some(preference) = index_preference {
            while *preference >= monitors.elements().len() {
                monitors.elements_mut().push_back(Monitor::placeholder());
            }

            let current_serial_id = monitors
                .elements_mut()
                .get(*preference)
                .map_or("", |m| &m.serial_number_id);
            if current_serial_id == "PLACEHOLDER" {
                let _ = monitors.elements_mut().remove(*preference);
                monitors.elements_mut().insert(*preference, m);
            } else {
                monitors.elements_mut().insert(*preference, m);
            }
        } else {
            monitors.elements_mut().push_back(m);
        }
    }

    monitors
        .elements_mut()
        .retain(|m| m.serial_number_id.ne("PLACEHOLDER"));

    // Rebuild monitor index map
    *monitor_usr_idx_map = HashMap::new();
    let mut added_monitor_idxs = Vec::new();
    for (index, id) in &*DISPLAY_INDEX_PREFERENCES.read() {
        if let Some(m_idx) = monitors
            .elements()
            .iter()
            .position(|m| m.serial_number_id.eq(id))
        {
            monitor_usr_idx_map.insert(*index, m_idx);
            added_monitor_idxs.push(m_idx);
        }
    }

    let max_usr_idx = monitors
        .elements()
        .len()
        .max(monitor_usr_idx_map.keys().max().map_or(0, |v| *v));

    let mut available_usr_idxs = (0..max_usr_idx)
        .filter(|i| !monitor_usr_idx_map.contains_key(i))
        .collect::<Vec<_>>();

    let not_added_monitor_idxs = (0..monitors.elements().len())
        .filter(|i| !added_monitor_idxs.contains(i))
        .collect::<Vec<_>>();

    for i in not_added_monitor_idxs {
        if let Some(next_usr_idx) = available_usr_idxs.first() {
            monitor_usr_idx_map.insert(*next_usr_idx, i);
            available_usr_idxs.remove(0);
        } else if let Some(idx) = monitor_usr_idx_map.keys().max() {
            monitor_usr_idx_map.insert(*idx, i);
        }
    }

    Ok(())
}

/// Reads the size and work area of every monitor again, and tiles the focused workspace of the
/// focused monitor in the work area it has now.
///
/// A monitor is matched with the backend's by its id: a monitor the backend does not report is left
/// as it is, and so is one the window manager does not have yet ([`load_monitor_information`] adds
/// it). The window that has the focus is given the focus again.
///
/// # Errors
///
/// - The backend cannot report its monitors
///   ([`Backend::monitors`](crate::backend::Backend::monitors)).
/// - There is no focused monitor or workspace, or the windows of the focused workspace cannot be
///   put in their tiles.
///
/// # Example
///
/// ```rust
/// use std::path::PathBuf;
/// use std::sync::Arc;
///
/// use komorebi::backend::fake::FakeBackend;
/// use komorebi::backend::with_backend;
/// use komorebi::core::Rect;
/// use komorebi::desktop::update_monitor_work_areas;
/// use komorebi::window_manager::WindowManager;
///
/// let fake = Arc::new(FakeBackend::with_one_monitor());
/// let (_sender, events) = crossbeam_channel::bounded(1);
/// let socket = PathBuf::from(format!("/tmp/komorebi-areas-doc-{}.sock", std::process::id()));
/// let mut wm = WindowManager::new(events, Some(socket.clone()))?;
/// with_backend(fake.clone(), || wm.init())?;
/// assert_eq!(wm.monitors()[0].work_area_size.bottom, 1050);
///
/// // a panel takes more of the monitor
/// let size = Rect { left: 0, top: 0, right: 1920, bottom: 1080 };
/// fake.resize_monitor(1, size, Rect { bottom: 1000, ..size });
/// with_backend(fake, || update_monitor_work_areas(&mut wm))?;
///
/// assert_eq!(wm.monitors()[0].work_area_size.bottom, 1000);
/// # std::fs::remove_file(socket)?;
/// # Ok::<(), color_eyre::eyre::Report>(())
/// ```
pub fn update_monitor_work_areas(wm: &mut WindowManager) -> eyre::Result<()> {
    let all_devices = latest_monitor_information()?;
    for device in all_devices {
        for monitor in wm.monitors_mut() {
            if monitor.id == device.id {
                monitor.size = device.size;
                monitor.work_area_size = device.work_area_size;

                tracing::info!(
                    "updated monitor size and work area for monitor {}",
                    monitor.serial_number_id
                );
            }
        }
    }

    let mouse_follows_focus = wm.mouse_follows_focus;
    wm.update_focused_workspace(mouse_follows_focus, true)?;

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
/// monitor is left alone. A window that is already maximized is the workspace's maximized window
/// ([`Workspace::adopt_maximized_window`](crate::workspace::Workspace::adopt_maximized_window)), not
/// a container. Nothing is moved: tiling is [`WindowManager::retile_all`]'s job.
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
/// fake.open_window(3, "foot", "foot", "a maximized terminal");
/// fake.user_maximize(3, true);
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
/// assert_eq!(workspace.maximized_window.map(|window| window.id), Some(3));
/// assert_eq!(workspace.containers().len(), 1);
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

            // already maximized when komorebi started: it is the maximized window, not a tile that
            // would be asked to leave the maximized state
            if info.maximized
                && let Err(error) = workspace.adopt_maximized_window(info.id)
            {
                tracing::warn!("could not take the maximized window {}: {error}", info.id);
            }
        }
    }

    tracing::info!("{valid_window_count} valid windows identified");

    Ok(())
}
