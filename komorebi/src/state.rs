//! [`State`], the window manager as the daemon shows it: every monitor, with its workspaces, their
//! containers and the windows in them, and the settings that apply to all of them.
//!
//! The daemon writes it as JSON in answer to [`SocketMessage::State`], and sends it to its
//! subscribers in every [`Notification`]. [`State::has_been_modified`] decides whether an event or
//! a message changed it, for the subscribers that want a notification only then.
//!
//! [`SocketMessage::State`]: crate::core::SocketMessage::State
//! [`Notification`]: crate::Notification

use crate::core::Rect;
use crate::monitor::Monitor;
use crate::ring::Ring;
use crate::window_manager::WindowManager;
use crate::workspace::Workspace;
use serde::Deserialize;
use serde::Serialize;
use std::collections::VecDeque;

/// A copy of the window manager's monitors, with their workspaces, containers and windows, and
/// of its settings, which serializes to JSON
///
/// # Example
///
/// ```rust
/// use std::path::PathBuf;
/// use std::sync::Arc;
///
/// use komorebi::backend::fake::FakeBackend;
/// use komorebi::backend::with_backend;
/// use komorebi::state::State;
/// use komorebi::window_manager::WindowManager;
///
/// let fake = Arc::new(FakeBackend::with_one_monitor());
/// fake.open_window(9, "foot", "foot", "a terminal");
/// let (_sender, events) = crossbeam_channel::bounded(1);
/// let socket = PathBuf::from(format!("/tmp/komorebi-state-doc-{}.sock", std::process::id()));
///
/// let state = with_backend(fake, || -> color_eyre::Result<State> {
///     let mut wm = WindowManager::new(events, Some(socket.clone()))?;
///     wm.init()?;
///     Ok(State::from(&wm))
/// })?;
/// # std::fs::remove_file(&socket)?;
///
/// let workspace = &state.monitors.elements()[0].workspaces()[0];
/// assert_eq!(workspace.containers()[0].windows()[0].id, 9);
///
/// # Ok::<(), Box<dyn std::error::Error>>(())
/// ```
#[allow(clippy::struct_excessive_bools)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct State {
    pub monitors: Ring<Monitor>,
    pub resize_delta: i32,
    pub work_area_offset: Option<Rect>,
    pub mouse_follows_focus: bool,
}

impl From<&WindowManager> for State {
    fn from(wm: &WindowManager) -> Self {
        // Every field of a monitor and of a workspace is copied by name here, so that what does
        // not need to be shown in the state can be left out of the copy.
        let mut stripped_monitors = Ring::default();
        *stripped_monitors.elements_mut() = wm
            .monitors()
            .iter()
            .map(|monitor| Monitor {
                id: monitor.id,
                device: monitor.device.clone(),
                serial_number_id: monitor.serial_number_id.clone(),
                size: monitor.size,
                work_area_size: monitor.work_area_size,
                work_area_offset: monitor.work_area_offset,
                workspaces: {
                    let mut ws = Ring::default();
                    *ws.elements_mut() = monitor
                        .workspaces()
                        .iter()
                        .map(|workspace| Workspace {
                            containers: workspace.containers.clone(),
                            floating_windows: workspace.floating_windows.clone(),
                            layout: workspace.layout.clone(),
                            layout_options: workspace.layout_options,
                            layout_flip: workspace.layout_flip,
                            workspace_padding: workspace.workspace_padding,
                            container_padding: workspace.container_padding,
                            latest_layout: workspace.latest_layout.clone(),
                            resize_dimensions: workspace.resize_dimensions.clone(),
                            tile: workspace.tile,
                            globals: workspace.globals,
                            preselected_container_idx: None,
                            promotion_swap_container_idx: None,
                        })
                        .collect::<VecDeque<_>>();
                    ws.focus(monitor.workspaces.focused_idx());
                    ws
                },
                container_padding: monitor.container_padding,
                workspace_padding: monitor.workspace_padding,
            })
            .collect::<VecDeque<_>>();
        stripped_monitors.focus(wm.monitors.focused_idx());

        Self {
            monitors: stripped_monitors,
            work_area_offset: wm.work_area_offset,
            resize_delta: wm.resize_delta,
            mouse_follows_focus: wm.mouse_follows_focus,
        }
    }
}

impl State {
    /// Returns true if and only if the window manager's monitors (with their workspaces,
    /// containers and windows), its work area offset or its mouse follows focus setting differ
    /// from this state's
    ///
    /// A window is compared by its id only, so a window that moved or was renamed is not a
    /// change.
    pub fn has_been_modified(&self, wm: &WindowManager) -> bool {
        let new = Self::from(wm);

        if self.monitors != new.monitors {
            return true;
        }

        if self.work_area_offset != new.work_area_offset {
            return true;
        }

        if self.mouse_follows_focus != new.mouse_follows_focus {
            return true;
        }

        false
    }
}
