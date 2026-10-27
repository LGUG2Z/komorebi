//! [`State`], the window manager as the daemon shows it: every monitor, with its workspaces, their
//! containers and the windows in them, and the settings that apply to all of them.
//!
//! The daemon writes it as JSON in answer to [`SocketMessage::State`].
//!
//! [`SocketMessage::State`]: crate::core::SocketMessage::State

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
                            layout: workspace.layout.clone(),
                            workspace_padding: workspace.workspace_padding,
                            container_padding: workspace.container_padding,
                            globals: workspace.globals,
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
            mouse_follows_focus: wm.mouse_follows_focus,
        }
    }
}
