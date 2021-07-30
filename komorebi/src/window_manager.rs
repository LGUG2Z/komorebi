use std::collections::VecDeque;
use std::io::ErrorKind;
use std::sync::Arc;
use std::sync::Mutex;

use color_eyre::eyre::ContextCompat;
use color_eyre::Result;
use crossbeam_channel::Receiver;
use uds_windows::UnixListener;

use komorebi_core::CycleDirection;
use komorebi_core::Layout;
use komorebi_core::LayoutFlip;
use komorebi_core::OperationDirection;
use komorebi_core::Rect;
use komorebi_core::Sizing;

use crate::container::Container;
use crate::monitor::Monitor;
use crate::ring::Ring;
use crate::window::Window;
use crate::window_manager_event::WindowManagerEvent;
use crate::windows_api::WindowsApi;
use crate::workspace::Workspace;

#[derive(Debug)]
pub struct WindowManager {
    pub monitors: Ring<Monitor>,
    pub incoming_events: Arc<Mutex<Receiver<WindowManagerEvent>>>,
    pub command_listener: UnixListener,
    pub is_paused: bool,
}

pub fn new(incoming: Arc<Mutex<Receiver<WindowManagerEvent>>>) -> Result<WindowManager> {
    let home = dirs::home_dir().context("there is no home directory")?;
    let mut socket = home;
    socket.push("komorebi.sock");
    let socket = socket.as_path();

    match std::fs::remove_file(&socket) {
        Ok(_) => {}
        Err(error) => match error.kind() {
            // Doing this because ::exists() doesn't work reliably on Windows via IntelliJ
            ErrorKind::NotFound => {}
            _ => {
                return Err(error.into());
            }
        },
    };

    let listener = UnixListener::bind(&socket)?;

    Ok(WindowManager {
        monitors: Ring::default(),
        incoming_events: incoming,
        command_listener: listener,
        is_paused: false,
    })
}

impl WindowManager {
    pub fn init(&mut self) -> Result<()> {
        tracing::info!("initialising");
        WindowsApi::load_monitor_information(&mut self.monitors)?;
        WindowsApi::load_workspace_information(&mut self.monitors)?;
        self.update_focused_workspace(false)
    }

    pub fn update_focused_workspace(&mut self, mouse_follows_focus: bool) -> Result<()> {
        tracing::info!("updating monitor: {}", self.focused_monitor_idx());

        self.focused_monitor_mut()
            .context("there is no monitor")?
            .update_focused_workspace()?;

        if mouse_follows_focus {
            self.focused_window_mut()?.focus()?;
        }

        Ok(())
    }

    pub fn restore_all_windows(&mut self) {
        for monitor in self.monitors_mut() {
            for workspace in monitor.workspaces_mut() {
                for containers in workspace.containers_mut() {
                    for window in containers.windows_mut() {
                        window.restore();
                    }
                }
            }
        }
    }

    pub fn move_container_to_monitor(&mut self, idx: usize, follow: bool) -> Result<()> {
        let monitor = self.focused_monitor_mut().context("there is no monitor")?;
        let container = monitor
            .focused_workspace_mut()
            .context("there is no workspace")?
            .remove_focused_container()
            .context("there is no container")?;

        let target_monitor = self
            .monitors_mut()
            .get_mut(idx)
            .context("there is no monitor")?;

        target_monitor.add_container(container)?;
        target_monitor.load_focused_workspace()?;

        if follow {
            self.focus_monitor(idx)?;
        }

        self.update_focused_workspace(true)
    }

    pub fn move_container_to_workspace(&mut self, idx: usize, follow: bool) -> Result<()> {
        let monitor = self.focused_monitor_mut().context("there is no monitor")?;
        monitor.move_container_to_workspace(idx, follow)?;
        monitor.load_focused_workspace()?;
        self.update_focused_workspace(true)
    }

    pub fn focus_container_in_direction(&mut self, direction: OperationDirection) -> Result<()> {
        let workspace = self.focused_workspace_mut()?;

        let new_idx = workspace
            .new_idx_for_direction(direction)
            .context("this is not a valid direction from the current position")?;

        workspace.focus_container(new_idx);
        self.focused_window_mut()?.focus()?;

        Ok(())
    }

    pub fn move_container_in_direction(&mut self, direction: OperationDirection) -> Result<()> {
        let workspace = self.focused_workspace_mut()?;

        let current_idx = workspace.focused_container_idx();
        let new_idx = workspace
            .new_idx_for_direction(direction)
            .context("this is not a valid direction from the current position")?;

        workspace.swap_containers(current_idx, new_idx);
        workspace.focus_container(new_idx);
        self.update_focused_workspace(true)
    }

    pub fn cycle_container_window_in_direction(&mut self, direction: CycleDirection) -> Result<()> {
        let container = self.focused_container_mut()?;

        if container.windows().len() == 1 {
            return Err(eyre::anyhow!("there is only one window in this container"));
        }

        let current_idx = container.focused_window_idx();
        let next_idx = direction.next_idx(current_idx, container.windows().len());

        container.focus_window(next_idx);
        container.load_focused_window();

        self.update_focused_workspace(true)
    }

    pub fn add_window_to_container(&mut self, direction: OperationDirection) -> Result<()> {
        let workspace = self.focused_workspace_mut()?;
        let current_container_idx = workspace.focused_container_idx();

        let is_valid = direction.is_valid(
            workspace.layout(),
            workspace.focused_container_idx(),
            workspace.containers_mut().len(),
        );

        if is_valid {
            let new_idx = workspace
                .new_idx_for_direction(direction)
                .context("this is not a valid direction from the current position")?;

            let adjusted_new_index = if new_idx > current_container_idx {
                new_idx - 1
            } else {
                new_idx
            };

            workspace.move_window_to_container(adjusted_new_index)?;
            self.update_focused_workspace(true)?;
        }

        Ok(())
    }

    pub fn promote_container_to_front(&mut self) -> Result<()> {
        let workspace = self.focused_workspace_mut()?;
        workspace.promote_container()?;
        self.update_focused_workspace(true)
    }

    pub fn remove_window_from_container(&mut self) -> Result<()> {
        if self.focused_container()?.windows().len() == 1 {
            return Err(eyre::anyhow!("a container must have at least one window"));
        }

        let workspace = self.focused_workspace_mut()?;

        workspace.new_container_for_focused_window()?;
        self.update_focused_workspace(true)
    }

    pub fn toggle_float(&mut self) -> Result<()> {
        let hwnd = WindowsApi::top_visible_window()?;
        let workspace = self.focused_workspace_mut()?;

        let mut is_floating_window = false;

        for window in workspace.floating_windows() {
            if window.hwnd == hwnd {
                is_floating_window = true;
            }
        }

        if is_floating_window {
            tracing::info!("unfloating window");
            self.unfloat_window()?;
            self.update_focused_workspace(true)
        } else {
            tracing::info!("floating window");
            self.float_window()?;
            self.update_focused_workspace(false)
        }
    }

    pub fn float_window(&mut self) -> Result<()> {
        let work_area = self.focused_monitor_work_area()?;

        let workspace = self.focused_workspace_mut()?;
        workspace.new_floating_window()?;

        let window = workspace
            .floating_windows_mut()
            .last_mut()
            .context("there is no floating window")?;

        let half_width = work_area.right / 2;
        let half_weight = work_area.bottom / 2;

        let center = Rect {
            left: work_area.left + ((work_area.right - half_width) / 2),
            top: work_area.top + ((work_area.bottom - half_weight) / 2),
            right: half_width,
            bottom: half_weight,
        };

        window.set_position(&center, true)?;
        window.focus()?;

        Ok(())
    }

    pub fn unfloat_window(&mut self) -> Result<()> {
        let workspace = self.focused_workspace_mut()?;
        workspace.new_container_for_floating_window()
    }

    pub fn toggle_monocle(&mut self) -> Result<()> {
        let workspace = self.focused_workspace_mut()?;

        match workspace.monocle_container() {
            None => self.monocle_on()?,
            Some(_) => self.monocle_off()?,
        }

        self.update_focused_workspace(false)
    }

    pub fn monocle_on(&mut self) -> Result<()> {
        let workspace = self.focused_workspace_mut()?;
        workspace.new_monocle_container()
    }

    pub fn monocle_off(&mut self) -> Result<()> {
        let workspace = self.focused_workspace_mut()?;
        workspace.reintegrate_monocle_container()
    }

    pub fn flip_layout(&mut self, layout_flip: LayoutFlip) -> Result<()> {
        let workspace = self.focused_workspace_mut()?;

        #[allow(clippy::match_same_arms)]
        match workspace.layout_flip() {
            None => workspace.set_layout_flip(Option::from(layout_flip)),
            Some(current_layout_flip) => {
                match current_layout_flip {
                    LayoutFlip::Horizontal => match layout_flip {
                        LayoutFlip::Horizontal => workspace.set_layout_flip(None),
                        LayoutFlip::Vertical => workspace
                            .set_layout_flip(Option::from(LayoutFlip::HorizontalAndVertical)),
                        LayoutFlip::HorizontalAndVertical => workspace
                            .set_layout_flip(Option::from(LayoutFlip::HorizontalAndVertical)),
                    },
                    LayoutFlip::Vertical => match layout_flip {
                        LayoutFlip::Horizontal => workspace
                            .set_layout_flip(Option::from(LayoutFlip::HorizontalAndVertical)),
                        LayoutFlip::Vertical => workspace.set_layout_flip(None),
                        LayoutFlip::HorizontalAndVertical => workspace
                            .set_layout_flip(Option::from(LayoutFlip::HorizontalAndVertical)),
                    },
                    LayoutFlip::HorizontalAndVertical => {
                        match layout_flip {
                            LayoutFlip::Horizontal => {
                                workspace.set_layout_flip(Option::from(LayoutFlip::Vertical));
                            }
                            LayoutFlip::Vertical => {
                                workspace.set_layout_flip(Option::from(LayoutFlip::Horizontal));
                            }
                            LayoutFlip::HorizontalAndVertical => workspace.set_layout_flip(None),
                        };
                    }
                }
            }
        }

        self.update_focused_workspace(false)
    }

    pub fn change_workspace_layout(&mut self, layout: Layout) -> Result<()> {
        let workspace = self.focused_workspace_mut()?;
        workspace.set_layout(layout);
        self.update_focused_workspace(false)
    }

    pub fn adjust_workspace_padding(&mut self, sizing: Sizing, adjustment: i32) -> Result<()> {
        let workspace = self.focused_workspace_mut()?;

        let padding = workspace
            .workspace_padding()
            .context("there is no workspace padding")?;

        workspace.set_workspace_padding(Option::from(sizing.adjust_by(padding, adjustment)));

        self.update_focused_workspace(false)
    }

    pub fn adjust_container_padding(&mut self, sizing: Sizing, adjustment: i32) -> Result<()> {
        let workspace = self.focused_workspace_mut()?;

        let padding = workspace
            .container_padding()
            .context("there is no container padding")?;

        workspace.set_container_padding(Option::from(sizing.adjust_by(padding, adjustment)));

        self.update_focused_workspace(false)
    }

    pub fn set_workspace_layout(
        &mut self,
        monitor_idx: usize,
        workspace_idx: usize,
        layout: Layout,
    ) -> Result<()> {
        let focused_monitor_idx = self.focused_monitor_idx();

        let monitor = self
            .monitors_mut()
            .get_mut(monitor_idx)
            .context("there is no monitor")?;

        let work_area = *monitor.work_area_size();
        let focused_workspace_idx = monitor.focused_workspace_idx();

        let workspace = monitor
            .workspaces_mut()
            .get_mut(workspace_idx)
            .context("there is no monitor")?;

        workspace.set_layout(layout);

        // If this is the focused workspace on a non-focused screen, let's update it
        if focused_monitor_idx != monitor_idx && focused_workspace_idx == workspace_idx {
            workspace.update(&work_area)?;
            Ok(())
        } else {
            Ok(self.update_focused_workspace(false)?)
        }
    }

    pub fn set_workspace_padding(
        &mut self,
        monitor_idx: usize,
        workspace_idx: usize,
        size: i32,
    ) -> Result<()> {
        let monitor = self
            .monitors_mut()
            .get_mut(monitor_idx)
            .context("there is no monitor")?;

        let workspace = monitor
            .workspaces_mut()
            .get_mut(workspace_idx)
            .context("there is no monitor")?;

        workspace.set_workspace_padding(Option::from(size));

        self.update_focused_workspace(false)
    }

    pub fn set_workspace_name(
        &mut self,
        monitor_idx: usize,
        workspace_idx: usize,
        name: String,
    ) -> Result<()> {
        let monitor = self
            .monitors_mut()
            .get_mut(monitor_idx)
            .context("there is no monitor")?;

        let workspace = monitor
            .workspaces_mut()
            .get_mut(workspace_idx)
            .context("there is no monitor")?;

        workspace.set_name(Option::from(name.clone()));
        monitor.workspace_names_mut().insert(workspace_idx, name);

        Ok(())
    }

    pub fn set_container_padding(
        &mut self,
        monitor_idx: usize,
        workspace_idx: usize,
        size: i32,
    ) -> Result<()> {
        let monitor = self
            .monitors_mut()
            .get_mut(monitor_idx)
            .context("there is no monitor")?;

        let workspace = monitor
            .workspaces_mut()
            .get_mut(workspace_idx)
            .context("there is no monitor")?;

        workspace.set_container_padding(Option::from(size));

        self.update_focused_workspace(false)
    }
}

impl WindowManager {
    pub const fn monitors(&self) -> &VecDeque<Monitor> {
        self.monitors.elements()
    }

    pub fn monitors_mut(&mut self) -> &mut VecDeque<Monitor> {
        self.monitors.elements_mut()
    }

    pub fn focused_monitor(&self) -> Option<&Monitor> {
        self.monitors.focused()
    }

    pub const fn focused_monitor_idx(&self) -> usize {
        self.monitors.focused_idx()
    }

    pub fn focused_monitor_mut(&mut self) -> Option<&mut Monitor> {
        self.monitors.focused_mut()
    }

    pub fn focused_monitor_work_area(&self) -> Result<Rect> {
        Ok(*self
            .focused_monitor()
            .context("there is no monitor")?
            .work_area_size())
    }

    pub fn focus_monitor(&mut self, idx: usize) -> Result<()> {
        if self.monitors().get(idx).is_some() {
            self.monitors.focus(idx);
        } else {
            return Err(eyre::anyhow!("this is not a valid monitor index"));
        }

        Ok(())
    }

    pub fn monitor_idx_from_window(&mut self, window: &Window) -> Option<usize> {
        let hmonitor = WindowsApi::monitor_from_window(window.hwnd());

        for (i, monitor) in self.monitors().iter().enumerate() {
            if monitor.id() == hmonitor {
                return Option::from(i);
            }
        }

        None
    }

    pub fn focused_workspace(&self) -> Result<&Workspace> {
        self.focused_monitor()
            .context("there is no monitor")?
            .focused_workspace()
            .context("there is no workspace")
    }

    pub fn focused_workspace_mut(&mut self) -> Result<&mut Workspace> {
        self.focused_monitor_mut()
            .context("there is no monitor")?
            .focused_workspace_mut()
            .context("there is no workspace")
    }

    pub fn focus_workspace(&mut self, idx: usize) -> Result<()> {
        let monitor = self
            .focused_monitor_mut()
            .context("there is no workspace")?;

        monitor.focus_workspace(idx)?;
        monitor.load_focused_workspace()?;

        self.update_focused_workspace(true)
    }

    pub fn focused_container(&self) -> Result<&Container> {
        self.focused_workspace()?
            .focused_container()
            .context("there is no container")
    }

    pub fn focused_container_mut(&mut self) -> Result<&mut Container> {
        self.focused_workspace_mut()?
            .focused_container_mut()
            .context("there is no container")
    }

    fn focused_window_mut(&mut self) -> Result<&mut Window> {
        self.focused_container_mut()?
            .focused_window_mut()
            .context("there is no window")
    }
}
