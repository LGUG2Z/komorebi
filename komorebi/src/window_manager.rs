//! [`WindowManager`], everything komorebi tiles, and the operations on it.
//!
//! The window manager keeps the monitors it loaded from the backend
//! ([`WindowManager::init`]), the windows the user minimized, and an index of which monitor and
//! workspace each window it manages is on. It changes when an event comes in
//! ([`WindowManager::process_event`]), and every change ends with the focused workspace tiled
//! again ([`WindowManager::update_focused_workspace`]).
//!
//! It also owns the daemon's command socket, which [`listen_for_commands`] reads
//! ([`WindowManager::process_command`]), carries out the orders that come in on it, such as
//! moving the focus ([`WindowManager::focus_container_in_direction`],
//! [`WindowManager::focus_container_in_cycle_direction`]) and moving the focused container
//! ([`WindowManager::move_container_in_direction`],
//! [`WindowManager::move_container_in_cycle_direction`]), and stops the daemon when it is asked
//! to ([`WindowManager::stop`]).
//!
//! A monitor's workspaces are the compositor's virtual desktops. The window manager shows one
//! ([`WindowManager::focus_workspace`]), moves a container to one
//! ([`WindowManager::move_container_to_workspace`]), makes sure the compositor has a desktop for
//! each ([`WindowManager::sync_workspace_desktops`]), and follows a workspace switch that
//! something else made ([`WindowManager::adopt_workspace_change`]). A workspace can be named
//! ([`WindowManager::set_workspace_name`]) and found by its name on any monitor
//! ([`WindowManager::monitor_workspace_index_by_name`]).
//!
//! A container with more than one window is a stack. The window in front is the container's focused
//! window, and the others are hidden ([`Window::hide`]), on KDE on a virtual desktop nobody looks
//! at. Windows are stacked and unstacked one at a time
//! ([`WindowManager::add_window_to_container`], [`WindowManager::remove_window_from_container`])
//! or all of a workspace together ([`WindowManager::stack_all`], [`WindowManager::unstack_all`]),
//! and a window that opens joins the focused container instead of getting one of its own when the
//! window container behavior says so ([`WindowManager::window_management_behaviour`]). The window
//! in front is changed by cycling the stack ([`WindowManager::cycle_container_window_in_direction`])
//! or by the index of a window in it ([`WindowManager::focus_container_window`]), and a window
//! moves along the stack with [`WindowManager::cycle_container_window_index_in_direction`].
//! [`WindowManager::stop`] brings back every window the window manager hid
//! ([`WindowManager::restore_all_windows`]).
//!
//! With more than one monitor, the focus and a move at the edge of a monitor cross to the monitor
//! in that direction ([`WindowManager::monitor_idx_in_direction`]), and a container or a workspace
//! can be taken to another monitor ([`WindowManager::move_container_to_monitor`],
//! [`WindowManager::move_workspace_to_monitor`]).
//!
//! A `komorebi.json` ([`StaticConfig`]) can make the window manager
//! ([`StaticConfig::preload`]), and [`WindowManager::reload_static_configuration`] applies one to a
//! window manager that is running. A workspace's layout rules are added and cleared with
//! [`WindowManager::add_workspace_layout_default_rule`] and
//! [`WindowManager::clear_workspace_layout_rules`].
//!
//! [`listen_for_commands`]: crate::process_command::listen_for_commands

use crate::DATA_DIR;
use crate::HIDDEN_WINDOWS;
use crate::LibraryError;
use crate::REGEX_IDENTIFIERS;
use crate::SUBSCRIPTION_SOCKETS;
use crate::UNMANAGED_WINDOW_IDS;
use crate::WORKSPACE_MATCHING_RULES;
use crate::backend::backend;
use crate::border_manager;
use crate::container::Container;
use crate::core::Arrangement;
use crate::core::Axis;
use crate::core::CrossBoundaryBehaviour;
use crate::core::CycleDirection;
use crate::core::DefaultLayout;
use crate::core::FocusFollowsMouseImplementation;
use crate::core::Layout;
use crate::core::MonocleFocusBehaviour;
use crate::core::MoveBehaviour;
use crate::core::OperationBehaviour;
use crate::core::OperationDirection;
use crate::core::Rect;
use crate::core::Sizing;
use crate::core::WindowContainerBehaviour;
use crate::core::WindowManagementBehaviour;
use crate::core::config_generation::MatchingRule;
use crate::desktop;
use crate::lockable_sequence::Lockable;
use crate::monitor::Monitor;
use crate::ring::Ring;
use crate::static_config::StaticConfig;
use crate::window::Window;
use crate::window::should_act_individual;
use crate::window_manager_event::WindowManagerEvent;
use crate::window_manager_event_listener;
use crate::workspace::Workspace;
use crate::workspace::WorkspaceLayer;
use color_eyre::eyre;
use color_eyre::eyre::OptionExt;
use color_eyre::eyre::bail;
use crossbeam_channel::Receiver;
use hotwatch::Hotwatch;
use parking_lot::Mutex;
use std::collections::HashMap;
use std::collections::HashSet;
use std::collections::VecDeque;
use std::io::ErrorKind;
use std::net::Shutdown;
use std::num::NonZeroUsize;
use std::os::unix::net::UnixListener;
use std::os::unix::net::UnixStream;
use std::path::PathBuf;
use std::sync::Arc;

/// A tile cannot be resized to less than this fraction of the work area (or less than it already is)
const MINIMUM_TILE_FRACTION: i32 = 10;

/// What the focused workspace's layout was when a drag of one of its edges began: the sizes the
/// tiles had been given by hand, and where the layout put them. Every step of the drag is worked
/// out from that, not from the step before (the drag so far is the sum of its steps), and the end
/// of the drag, or a drag that is taken back, starts from it too.
#[derive(Debug, Clone)]
pub struct LiveResize {
    /// The window whose edge is being dragged
    pub window: u32,
    /// how many tiles there were: when a window opens or closes part way through a drag, the
    /// layout it was taken from is not the layout there is
    pub containers: usize,
    /// The workspace's [`Workspace::resize_dimensions`] when the drag began
    pub resize_dimensions: Vec<Option<Rect>>,
    /// The workspace's [`Workspace::latest_layout`] when the drag began
    pub latest_layout: Vec<Rect>,
    /// whether the neighbours were moved while the drag was going on
    pub stepped: bool,
}

#[derive(Debug)]
pub struct WindowManager {
    pub monitors: Ring<Monitor>,
    /// The monitor index a user's index stands for, when the indices the user gives monitors
    /// (`DisplayIndexPreference`) are not the order the compositor reports them in
    pub monitor_usr_idx_map: HashMap<usize, usize>,
    /// The socket the daemon takes messages on, `komorebi.sock` in [`struct@DATA_DIR`] unless
    /// [`WindowManager::new`] was given another path
    pub command_listener: UnixListener,
    /// Whether `TogglePause` has paused the window manager: while it is paused, it handles no
    /// event, and of the messages only `TogglePause`, `State`, `GlobalState` and `Stop`
    pub is_paused: bool,
    /// How many pixels `ResizeWindowEdge` and `ResizeWindowAxis` resize by
    pub resize_delta: i32,
    /// Watches `komorebi.json`, when the window manager was made from it
    /// ([`StaticConfig::preload`]): a change to the file makes the daemon reload it
    pub hotwatch: Hotwatch,
    /// Whether a message is carried out while the window that has the focus is not one komorebi
    /// tiles ([`WindowManager::handle_unmanaged_window_behaviour`])
    pub unmanaged_window_operation_behaviour: OperationBehaviour,
    /// How new and floated windows are placed: `ToggleFloat` reads its `toggle_float_placement`
    pub window_management_behaviour: WindowManagementBehaviour,
    /// What a move towards a side with no container does: swap the container with the one at the
    /// edge of the monitor on that side (`Swap`, the default), put it in among the containers there
    /// (`Insert`), or nothing (`NoOp`)
    pub cross_monitor_move_behaviour: MoveBehaviour,
    /// Where a focus or a move towards a side with no container goes: the monitor on that side
    /// (`Monitor`, the default), or the next workspace of the same monitor (`Workspace`)
    pub cross_boundary_behaviour: CrossBoundaryBehaviour,
    /// What focusing in a direction does while the workspace has a monocle container: cycle the
    /// monocle to the next or the previous container, or nothing (the default)
    pub monocle_focus_behaviour: MonocleFocusBehaviour,
    /// Whether the pointer is moved to the window that is given the focus, which is the default
    pub mouse_follows_focus: bool,
    /// Whether the compositor gives the focus to the window under the pointer, and the
    /// implementation that was asked for: on KDE both are the compositor's own focus policy
    pub focus_follows_mouse: Option<FocusFollowsMouseImplementation>,
    /// The opacity each window was last given by [`crate::transparency_manager`], for the ones that
    /// are not opaque
    pub opacity_applied: HashMap<u32, u8>,
    pub work_area_offset: Option<Rect>,
    /// The events the backend has reported and the window manager has yet to handle, from
    /// [`crate::window_manager_event_listener`]
    pub incoming_events: Receiver<WindowManagerEvent>,
    pub minimized_windows: HashMap<u32, Window>,
    /// The monitor, the workspace and the window of a move the user began with the pointer and
    /// has not let go of yet
    pub pending_move_op: Arc<Option<(usize, usize, u32)>>,
    /// The window of a resize the user began with the pointer and has not let go of yet, and the
    /// frame it had when they took hold of it
    pub pending_resize_op: Arc<Option<(u32, Option<Rect>)>>,
    /// While the user drags an edge: what the layout was when they took hold of it
    pub live_resize: Option<LiveResize>,
    /// The windows an `initial_only` workspace rule has moved already, so that it moves each of
    /// them once, until it closes
    pub already_moved_window_handles: Arc<Mutex<HashSet<u32>>>,
    /// Maps each known window id to the (monitor, workspace) index pair managing it
    pub known_window_ids: HashMap<u32, (usize, usize)>,
}

impl_ring_elements!(WindowManager, Monitor);

/// A window that a workspace rule sends from the workspace it is on to the one the rule names
#[derive(Debug, Clone, Copy)]
struct EnforceWorkspaceRuleOp {
    window_id: u32,
    origin_monitor_idx: usize,
    origin_workspace_idx: usize,
    target_monitor_idx: usize,
    target_workspace_idx: usize,
    floating: bool,
}

impl EnforceWorkspaceRuleOp {
    const fn is_origin(&self, monitor_idx: usize, workspace_idx: usize) -> bool {
        self.origin_monitor_idx == monitor_idx && self.origin_workspace_idx == workspace_idx
    }

    const fn is_target(&self, monitor_idx: usize, workspace_idx: usize) -> bool {
        self.target_monitor_idx == monitor_idx && self.target_workspace_idx == workspace_idx
    }

    const fn is_enforced(&self) -> bool {
        (self.origin_monitor_idx == self.target_monitor_idx)
            && (self.origin_workspace_idx == self.target_workspace_idx)
    }
}

impl AsRef<Self> for WindowManager {
    fn as_ref(&self) -> &Self {
        self
    }
}

impl WindowManager {
    /// Returns a window manager with no monitors, which handles the events that arrive on
    /// `incoming` and listens on `custom_socket_path`, or on `komorebi.sock` in
    /// [`struct@DATA_DIR`] when that is `None`: [`WindowManager::init`] loads the monitors from
    /// the backend
    ///
    /// A socket file that is left over at that path is removed first.
    ///
    /// # Errors
    ///
    /// - The leftover socket file is there but cannot be removed.
    /// - The socket cannot be bound, for example because its directory does not exist.
    /// - The file watcher cannot be made.
    pub fn new(
        incoming: Receiver<WindowManagerEvent>,
        custom_socket_path: Option<PathBuf>,
    ) -> eyre::Result<Self> {
        let socket = custom_socket_path.unwrap_or_else(|| DATA_DIR.join("komorebi.sock"));

        match std::fs::remove_file(&socket) {
            Ok(()) => {}
            Err(error) => match error.kind() {
                ErrorKind::NotFound => {}
                _ => {
                    return Err(error.into());
                }
            },
        };

        let listener = UnixListener::bind(&socket)?;

        Ok(Self {
            monitors: Ring::default(),
            monitor_usr_idx_map: HashMap::new(),
            command_listener: listener,
            is_paused: false,
            resize_delta: 50,
            hotwatch: Hotwatch::new()?,
            unmanaged_window_operation_behaviour: Default::default(),
            window_management_behaviour: WindowManagementBehaviour::default(),
            cross_monitor_move_behaviour: Default::default(),
            cross_boundary_behaviour: Default::default(),
            monocle_focus_behaviour: MonocleFocusBehaviour::default(),
            focus_follows_mouse: None,
            opacity_applied: HashMap::new(),
            mouse_follows_focus: true,
            work_area_offset: None,
            incoming_events: incoming,
            minimized_windows: HashMap::new(),
            pending_move_op: Arc::new(None),
            pending_resize_op: Arc::new(None),
            live_resize: None,
            already_moved_window_handles: Default::default(),
            known_window_ids: Default::default(),
        })
    }

    /// Loads the monitors and the windows that are already open from the backend
    /// ([`desktop`]), and makes sure the compositor has a desktop for every workspace
    /// ([`WindowManager::sync_workspace_desktops`])
    #[tracing::instrument(skip(self))]
    pub fn init(&mut self) -> Result<(), LibraryError> {
        tracing::info!("initializing");
        desktop::load_monitor_information(self)?;
        self.sync_workspace_desktops();
        desktop::load_workspace_information(self)?;
        // the windows that were open already are in containers now; an event about one of them
        // (the first thing to happen may be that it closes) has to find it
        self.update_known_window_ids();
        Ok(())
    }

    /// Gives the focus to the floating window whose frame is on the `direction` side of the focused
    /// one, the nearest of them, and the pointer follows it if [`WindowManager::mouse_follows_focus`]
    /// is set
    ///
    /// The floating windows are compared by where the compositor has them, not by the order they
    /// are kept in, and only when there is more than one. Where no floating window is on that
    /// side, the focus crosses as [`WindowManager::focus_container_in_direction`] does: to the
    /// next workspace with `CrossBoundaryBehaviour::Workspace`, else to the monitor on that side.
    ///
    /// # Errors
    ///
    /// - There is no focused workspace, or the compositor reports no focused window.
    /// - No floating window is on that side of the focused one, and no monitor either.
    /// - The backend refuses to focus the window.
    #[tracing::instrument(skip(self))]
    pub fn focus_floating_window_in_direction(
        &mut self,
        direction: OperationDirection,
    ) -> eyre::Result<()> {
        let mouse_follows_focus = self.mouse_follows_focus;
        let focused_workspace = self.focused_workspace_mut()?;

        let mut target_idx = None;
        let len = focused_workspace.floating_windows().len();

        if len > 1 {
            let focused_window_id = backend()
                .foreground_window()
                .ok_or_eyre("no foreground window")?;
            let focused_rect = Window::from(focused_window_id).rect()?;

            match direction {
                OperationDirection::Left => {
                    let mut windows_in_direction = focused_workspace
                        .floating_windows()
                        .iter()
                        .enumerate()
                        .flat_map(|(idx, w)| {
                            (w.id != focused_window_id).then_some(w.rect().ok().map(|r| (idx, r)))
                        })
                        .flatten()
                        .flat_map(|(idx, r)| {
                            (r.left < focused_rect.left)
                                .then_some((idx, i32::abs(r.left - focused_rect.left)))
                        })
                        .collect::<Vec<_>>();

                    // Sort by distance to focused
                    windows_in_direction.sort_by_key(|(_, d)| (*d as f32 * 1000.0).trunc() as i32);

                    if let Some((idx, _)) = windows_in_direction.first() {
                        target_idx = Some(*idx);
                    }
                }
                OperationDirection::Right => {
                    let mut windows_in_direction = focused_workspace
                        .floating_windows()
                        .iter()
                        .enumerate()
                        .flat_map(|(idx, w)| {
                            (w.id != focused_window_id).then_some(w.rect().ok().map(|r| (idx, r)))
                        })
                        .flatten()
                        .flat_map(|(idx, r)| {
                            (r.left > focused_rect.left)
                                .then_some((idx, i32::abs(r.left - focused_rect.left)))
                        })
                        .collect::<Vec<_>>();

                    // Sort by distance to focused
                    windows_in_direction.sort_by_key(|(_, d)| (*d as f32 * 1000.0).trunc() as i32);

                    if let Some((idx, _)) = windows_in_direction.first() {
                        target_idx = Some(*idx);
                    }
                }
                OperationDirection::Up => {
                    let mut windows_in_direction = focused_workspace
                        .floating_windows()
                        .iter()
                        .enumerate()
                        .flat_map(|(idx, w)| {
                            (w.id != focused_window_id).then_some(w.rect().ok().map(|r| (idx, r)))
                        })
                        .flatten()
                        .flat_map(|(idx, r)| {
                            (r.top < focused_rect.top)
                                .then_some((idx, i32::abs(r.top - focused_rect.top)))
                        })
                        .collect::<Vec<_>>();

                    // Sort by distance to focused
                    windows_in_direction.sort_by_key(|(_, d)| (*d as f32 * 1000.0).trunc() as i32);

                    if let Some((idx, _)) = windows_in_direction.first() {
                        target_idx = Some(*idx);
                    }
                }
                OperationDirection::Down => {
                    let mut windows_in_direction = focused_workspace
                        .floating_windows()
                        .iter()
                        .enumerate()
                        .flat_map(|(idx, w)| {
                            (w.id != focused_window_id).then_some(w.rect().ok().map(|r| (idx, r)))
                        })
                        .flatten()
                        .flat_map(|(idx, r)| {
                            (r.top > focused_rect.top)
                                .then_some((idx, i32::abs(r.top - focused_rect.top)))
                        })
                        .collect::<Vec<_>>();

                    // Sort by distance to focused
                    windows_in_direction.sort_by_key(|(_, d)| (*d as f32 * 1000.0).trunc() as i32);

                    if let Some((idx, _)) = windows_in_direction.first() {
                        target_idx = Some(*idx);
                    }
                }
            };
        }

        if let Some(idx) = target_idx {
            focused_workspace.floating_windows.focus(idx);
            if let Some(window) = focused_workspace.floating_windows().get(idx) {
                window.focus(mouse_follows_focus)?;
            }
            return Ok(());
        }

        let mut cross_monitor_monocle_or_max = false;

        let workspace_idx = self.focused_workspace_idx()?;

        // this is for when we are scrolling across workspaces like PaperWM
        if matches!(
            self.cross_boundary_behaviour,
            CrossBoundaryBehaviour::Workspace
        ) && matches!(
            direction,
            OperationDirection::Left | OperationDirection::Right
        ) {
            let workspace_count = if let Some(monitor) = self.focused_monitor() {
                monitor.workspaces().len()
            } else {
                1
            };

            let next_idx = match direction {
                OperationDirection::Left => match workspace_idx {
                    0 => workspace_count - 1,
                    n => n - 1,
                },
                OperationDirection::Right => match workspace_idx {
                    n if n == workspace_count - 1 => 0,
                    n => n + 1,
                },
                _ => workspace_idx,
            };

            self.focus_workspace(next_idx)?;

            if let Ok(focused_workspace) = self.focused_workspace_mut()
                && focused_workspace.monocle_container.is_none()
            {
                match direction {
                    OperationDirection::Left => match focused_workspace.layout {
                        Layout::Default(layout) => {
                            let target_index =
                                layout.rightmost_index(focused_workspace.containers().len());
                            focused_workspace.focus_container(target_index);
                        }
                    },
                    OperationDirection::Right => match focused_workspace.layout {
                        Layout::Default(layout) => {
                            let target_index =
                                layout.leftmost_index(focused_workspace.containers().len());
                            focused_workspace.focus_container(target_index);
                        }
                    },
                    _ => {}
                };
            }

            return Ok(());
        }

        // if there is no floating_window in that direction for this workspace
        let monitor_idx = self
            .monitor_idx_in_direction(direction)
            .ok_or_eyre("there is no container or monitor in this direction")?;

        self.focus_monitor(monitor_idx)?;
        let mouse_follows_focus = self.mouse_follows_focus;

        if let Ok(focused_workspace) = self.focused_workspace_mut() {
            if let Some(monocle) = &focused_workspace.monocle_container {
                if let Some(window) = monocle.focused_window() {
                    window.focus(mouse_follows_focus)?;
                    cross_monitor_monocle_or_max = true;
                }
            } else if focused_workspace.layer == WorkspaceLayer::Tiling {
                match direction {
                    OperationDirection::Left => match focused_workspace.layout {
                        Layout::Default(layout) => {
                            let target_index =
                                layout.rightmost_index(focused_workspace.containers().len());
                            focused_workspace.focus_container(target_index);
                        }
                    },
                    OperationDirection::Right => match focused_workspace.layout {
                        Layout::Default(layout) => {
                            let target_index =
                                layout.leftmost_index(focused_workspace.containers().len());
                            focused_workspace.focus_container(target_index);
                        }
                    },
                    _ => {}
                };
            }
        }

        if !cross_monitor_monocle_or_max {
            let ws = self.focused_workspace_mut()?;
            if !ws.is_empty() {
                if ws.layer == WorkspaceLayer::Floating && !ws.floating_windows().is_empty() {
                    if let Some(window) = ws.focused_floating_window() {
                        window.focus(mouse_follows_focus)?;
                    }
                } else {
                    ws.layer = WorkspaceLayer::Tiling;
                    if let Ok(focused_window) = self.focused_window() {
                        focused_window.focus(mouse_follows_focus)?;
                    }
                }
            }
        }

        Ok(())
    }

    /// Reserves the place on the `direction` side of the focused container for the next window
    /// that opens on the focused workspace ([`Workspace::preselect_container_idx`])
    ///
    /// The grid layout has no preselection: there, and where there is no container on that side,
    /// or while the workspace has a monocle container, nothing is reserved.
    ///
    /// # Errors
    ///
    /// There is no focused workspace.
    #[tracing::instrument(skip(self))]
    pub fn preselect_container_in_direction(
        &mut self,
        direction: OperationDirection,
    ) -> eyre::Result<()> {
        let workspace = self.focused_workspace_mut()?;
        let focused_idx = workspace.focused_container_idx();

        if matches!(workspace.layout, Layout::Default(DefaultLayout::Grid)) {
            tracing::warn!("preselection is not supported on the grid layout");
            return Ok(());
        }

        tracing::info!("preselecting container");

        let new_idx =
            if workspace.maximized_window.is_some() || workspace.monocle_container.is_some() {
                None
            } else {
                workspace.new_idx_for_direction(direction)
            };

        match new_idx {
            Some(new_idx) => {
                let adjusted_idx = match direction {
                    OperationDirection::Left | OperationDirection::Up => {
                        if focused_idx.abs_diff(new_idx) == 1 {
                            new_idx + 1
                        } else {
                            new_idx
                        }
                    }

                    _ => new_idx,
                };

                workspace.preselect_container_idx(adjusted_idx);
            }

            None => {
                tracing::debug!(
                    "this is not a valid preselection direction from the current position"
                )
            }
        }

        Ok(())
    }

    /// Gives the focus to the window of the container whose tile is on the `direction` side of
    /// the focused one, and the pointer follows it if [`WindowManager::mouse_follows_focus`] is
    /// set. Where there is no such container, the focus crosses to the monitor on that side.
    ///
    /// Nothing is retiled: the focus moves, the windows stay where they are.
    ///
    /// While the workspace has a monocle container, no container is on any side of the window in
    /// front. With the monocle focus behaviour `Cycle`, focusing left or down cycles the monocle to
    /// the previous container and focusing right or up to the next one
    /// ([`WindowManager::cycle_monocle`]); with `NoOp`, the default, the focus falls through to
    /// the monitor on that side, and gives the window in front of its monocle container the focus
    /// if it has one.
    ///
    /// # Errors
    ///
    /// - There is no focused workspace, or neither a container nor a monitor on that side of the
    ///   focused one.
    /// - [`WindowManager::handle_unmanaged_window_behaviour`] refuses the message.
    /// - The backend refuses to focus the window.
    #[tracing::instrument(skip(self))]
    pub fn focus_container_in_direction(
        &mut self,
        direction: OperationDirection,
    ) -> eyre::Result<()> {
        self.handle_unmanaged_window_behaviour()?;
        let mouse_follows_focus = self.mouse_follows_focus;

        let workspace = self.focused_workspace()?;
        let workspace_idx = self.focused_workspace_idx()?;

        tracing::info!("focusing container");

        if workspace.monocle_container.is_some()
            && matches!(self.monocle_focus_behaviour, MonocleFocusBehaviour::Cycle)
        {
            let cycle_direction = match direction {
                OperationDirection::Left | OperationDirection::Down => CycleDirection::Previous,
                OperationDirection::Right | OperationDirection::Up => CycleDirection::Next,
            };
            return self.cycle_monocle(cycle_direction);
        }

        let new_idx =
            if workspace.maximized_window.is_some() || workspace.monocle_container.is_some() {
                None
            } else {
                workspace.new_idx_for_direction(direction)
            };

        let mut cross_monitor_monocle_or_max = false;

        // this is for when we are scrolling across workspaces like PaperWM
        if new_idx.is_none()
            && matches!(
                self.cross_boundary_behaviour,
                CrossBoundaryBehaviour::Workspace
            )
            && matches!(
                direction,
                OperationDirection::Left | OperationDirection::Right
            )
        {
            let workspace_count = if let Some(monitor) = self.focused_monitor() {
                monitor.workspaces().len()
            } else {
                1
            };

            let next_idx = match direction {
                OperationDirection::Left => match workspace_idx {
                    0 => workspace_count - 1,
                    n => n - 1,
                },
                OperationDirection::Right => match workspace_idx {
                    n if n == workspace_count - 1 => 0,
                    n => n + 1,
                },
                _ => workspace_idx,
            };

            self.focus_workspace(next_idx)?;

            if let Ok(focused_workspace) = self.focused_workspace_mut()
                && focused_workspace.monocle_container.is_none()
            {
                match direction {
                    OperationDirection::Left => match focused_workspace.layout {
                        Layout::Default(layout) => {
                            let target_index =
                                layout.rightmost_index(focused_workspace.containers().len());
                            focused_workspace.focus_container(target_index);
                        }
                    },
                    OperationDirection::Right => match focused_workspace.layout {
                        Layout::Default(layout) => {
                            let target_index =
                                layout.leftmost_index(focused_workspace.containers().len());
                            focused_workspace.focus_container(target_index);
                        }
                    },
                    _ => {}
                };
            }

            return Ok(());
        }

        // if there is no container in that direction for this workspace
        match new_idx {
            None => {
                let monitor_idx = self
                    .monitor_idx_in_direction(direction)
                    .ok_or_eyre("there is no container or monitor in this direction")?;

                self.focus_monitor(monitor_idx)?;

                if let Ok(focused_workspace) = self.focused_workspace_mut() {
                    if let Some(window) = &focused_workspace.maximized_window {
                        window.focus(mouse_follows_focus)?;
                        cross_monitor_monocle_or_max = true;
                    } else if let Some(monocle) = &focused_workspace.monocle_container {
                        if let Some(window) = monocle.focused_window() {
                            window.focus(mouse_follows_focus)?;
                            cross_monitor_monocle_or_max = true;
                        }
                    } else if focused_workspace.layer == WorkspaceLayer::Tiling {
                        match direction {
                            OperationDirection::Left => match focused_workspace.layout {
                                Layout::Default(layout) => {
                                    let target_index = layout
                                        .rightmost_index(focused_workspace.containers().len());
                                    focused_workspace.focus_container(target_index);
                                }
                            },
                            OperationDirection::Right => match focused_workspace.layout {
                                Layout::Default(layout) => {
                                    let target_index =
                                        layout.leftmost_index(focused_workspace.containers().len());
                                    focused_workspace.focus_container(target_index);
                                }
                            },
                            _ => {}
                        };
                    }
                }
            }
            Some(idx) => {
                let workspace = self.focused_workspace_mut()?;
                workspace.focus_container(idx);
            }
        }

        if !cross_monitor_monocle_or_max {
            let ws = self.focused_workspace_mut()?;
            if !ws.is_empty() {
                if ws.layer == WorkspaceLayer::Floating && !ws.floating_windows().is_empty() {
                    if let Some(window) = ws.focused_floating_window() {
                        window.focus(mouse_follows_focus)?;
                    }
                } else {
                    ws.layer = WorkspaceLayer::Tiling;
                    if let Ok(focused_window) = self.focused_window() {
                        focused_window.focus(mouse_follows_focus)?;
                    }
                }
            }
        }

        Ok(())
    }

    /// Moves the focused floating window by [`WindowManager::resize_delta`] pixels towards the
    /// `direction` side, but not out of the work area of the focused monitor, and the pointer
    /// follows it if [`WindowManager::mouse_follows_focus`] is set
    ///
    /// # Errors
    ///
    /// - There is no focused monitor or workspace, or the compositor reports no focused window.
    /// - The backend refuses to move the window.
    #[tracing::instrument(skip(self))]
    pub fn move_floating_window_in_direction(
        &mut self,
        direction: OperationDirection,
    ) -> eyre::Result<()> {
        let mouse_follows_focus = self.mouse_follows_focus;

        let mut focused_monitor_work_area = self.focused_monitor_work_area()?;
        let border_offset = 0;
        let border_width = 0;
        focused_monitor_work_area.left += border_offset;
        focused_monitor_work_area.left += border_width;
        focused_monitor_work_area.top += border_offset;
        focused_monitor_work_area.top += border_width;
        focused_monitor_work_area.right -= border_offset * 2;
        focused_monitor_work_area.right -= border_width * 2;
        focused_monitor_work_area.bottom -= border_offset * 2;
        focused_monitor_work_area.bottom -= border_width * 2;

        let focused_workspace = self.focused_workspace()?;
        let delta = self.resize_delta;

        let focused_window_id = backend()
            .foreground_window()
            .ok_or_eyre("no foreground window")?;
        for window in focused_workspace.floating_windows().iter() {
            if window.id == focused_window_id {
                let mut rect = window.rect()?;
                match direction {
                    OperationDirection::Left => {
                        if rect.left - delta < focused_monitor_work_area.left {
                            rect.left = focused_monitor_work_area.left;
                        } else {
                            rect.left -= delta;
                        }
                    }
                    OperationDirection::Right => {
                        if rect.left + delta + rect.right
                            > focused_monitor_work_area.left + focused_monitor_work_area.right
                        {
                            rect.left = focused_monitor_work_area.left
                                + focused_monitor_work_area.right
                                - rect.right;
                        } else {
                            rect.left += delta;
                        }
                    }
                    OperationDirection::Up => {
                        if rect.top - delta < focused_monitor_work_area.top {
                            rect.top = focused_monitor_work_area.top;
                        } else {
                            rect.top -= delta;
                        }
                    }
                    OperationDirection::Down => {
                        if rect.top + delta + rect.bottom
                            > focused_monitor_work_area.top + focused_monitor_work_area.bottom
                        {
                            rect.top = focused_monitor_work_area.top
                                + focused_monitor_work_area.bottom
                                - rect.bottom;
                        } else {
                            rect.top += delta;
                        }
                    }
                }

                window.set_position(&rect)?;

                if mouse_follows_focus {
                    backend().center_cursor_in(&rect)?;
                }

                break;
            }
        }

        Ok(())
    }

    /// Swaps the focused container with the one whose tile is on the `direction` side of it, so
    /// that the focused window takes that tile, and tiles the workspace again
    ///
    /// The container keeps the focus, and the pointer follows it if
    /// [`WindowManager::mouse_follows_focus`] is set. A lock stays with its index: a container
    /// that is swapped onto the index of a locked one is locked in its place
    /// ([`Workspace::swap_containers`]). Where there is no container on that side, the container
    /// goes to the monitor there as [`WindowManager::cross_monitor_move_behaviour`] says: swapped
    /// with a container of that monitor, put in among them, or left where it is.
    ///
    /// # Errors
    ///
    /// - There is no focused workspace, or neither a container nor a monitor on that side of the
    ///   focused one.
    /// - [`WindowManager::handle_unmanaged_window_behaviour`] refuses the message.
    /// - The window in front is in monocle mode, which has no neighbours to swap with.
    /// - The workspace cannot be tiled, or the backend refuses to focus the window.
    #[tracing::instrument(skip(self))]
    pub fn move_container_in_direction(
        &mut self,
        direction: OperationDirection,
    ) -> eyre::Result<()> {
        self.handle_unmanaged_window_behaviour()?;

        let workspace = self.focused_workspace()?;
        let workspace_idx = self.focused_workspace_idx()?;

        // the monocle container is not one of the containers, so the index of the focused
        // container is not the window in front: moving it would move one that is out of sight
        if workspace.is_focused_window_monocle_or_maximized()? {
            bail!("ignoring command while active window is in monocle mode or maximized");
        }

        tracing::info!("moving container");

        let origin_container_idx = workspace.focused_container_idx();
        let origin_monitor_idx = self.focused_monitor_idx();
        let target_container_idx = workspace.new_idx_for_direction(direction);

        // this is for when we are scrolling across workspaces like PaperWM
        if target_container_idx.is_none()
            && matches!(
                self.cross_boundary_behaviour,
                CrossBoundaryBehaviour::Workspace
            )
            && matches!(
                direction,
                OperationDirection::Left | OperationDirection::Right
            )
        {
            let workspace_count = if let Some(monitor) = self.focused_monitor() {
                monitor.workspaces().len()
            } else {
                1
            };

            let next_idx = match direction {
                OperationDirection::Left => match workspace_idx {
                    0 => workspace_count - 1,
                    n => n - 1,
                },
                OperationDirection::Right => match workspace_idx {
                    n if n == workspace_count - 1 => 0,
                    n => n + 1,
                },
                _ => workspace_idx,
            };

            // passing the direction here is how we handle whether to insert at the front
            // or the back of the container vecdeque in the target workspace
            self.move_container_to_workspace(next_idx, true, Some(direction))?;
            self.update_focused_workspace(self.mouse_follows_focus, true)?;

            return Ok(());
        }

        match target_container_idx {
            // If there is nowhere to move on the current workspace, try to move it onto the monitor
            // in that direction if there is one
            None => {
                // Don't do anything if the user has set the MoveBehaviour to NoOp
                if matches!(self.cross_monitor_move_behaviour, MoveBehaviour::NoOp) {
                    return Ok(());
                }

                let target_monitor_idx = self
                    .monitor_idx_in_direction(direction)
                    .ok_or_eyre("there is no container or monitor in this direction")?;

                {
                    // actually move the container to target monitor using the direction
                    self.move_container_to_monitor(
                        target_monitor_idx,
                        None,
                        true,
                        Some(direction),
                    )?;

                    // focus the target monitor
                    self.focus_monitor(target_monitor_idx)?;

                    // unset monocle container on target workspace if there is one
                    let mut target_workspace_has_monocle = false;
                    if let Ok(target_workspace) = self.focused_workspace()
                        && target_workspace.monocle_container.is_some()
                    {
                        target_workspace_has_monocle = true;
                    }

                    if target_workspace_has_monocle {
                        self.toggle_monocle()?;
                    }

                    // get a mutable ref to the focused workspace on the target monitor
                    let target_workspace = self.focused_workspace_mut()?;

                    // if there is only one container on the target workspace after the insertion
                    // it means that there won't be one swapped back, so we have to decrement the
                    // focused position
                    if target_workspace.containers().len() == 1 {
                        let origin_workspace =
                            self.focused_workspace_for_monitor_idx_mut(origin_monitor_idx)?;

                        origin_workspace.focus_container(
                            origin_workspace.focused_container_idx().saturating_sub(1),
                        );
                    }
                }

                // if our MoveBehaviour is Swap, let's try to send back the window container
                // whose position which just took over
                if matches!(self.cross_monitor_move_behaviour, MoveBehaviour::Swap) {
                    {
                        let target_workspace = self.focused_workspace_mut()?;

                        // if the target workspace doesn't have more than one container, this means it
                        // was previously empty, by only doing the second part of the swap when there is
                        // more than one container, we can fall back to a "move" if there is nothing to
                        // swap with on the target monitor
                        if target_workspace.containers().len() > 1 {
                            // remove the container from the target monitor workspace
                            let target_container = target_workspace
                                // this is now focused_container_idx + 1 because we have inserted our origin container
                                .remove_container_by_idx(
                                    target_workspace.focused_container_idx() + 1,
                                )
                                .ok_or_eyre("could not remove container at given target index")?;

                            let origin_workspace =
                                self.focused_workspace_for_monitor_idx_mut(origin_monitor_idx)?;

                            // insert the container from the target monitor workspace into the origin monitor workspace
                            // at the same position from which our origin container was removed
                            origin_workspace
                                .insert_container_at_idx(origin_container_idx, target_container);
                        }
                    }
                }

                // make sure to update the origin monitor workspace layout because it is no
                // longer focused so it won't get updated at the end of this fn
                let offset = self.work_area_offset;

                self.monitors_mut()
                    .get_mut(origin_monitor_idx)
                    .ok_or_eyre("there is no monitor at this index")?
                    .update_focused_workspace(offset)?;
            }
            Some(new_idx) => {
                let workspace = self.focused_workspace_mut()?;
                workspace.swap_containers(origin_container_idx, new_idx);
                workspace.focus_container(new_idx);
            }
        }

        self.update_focused_workspace(self.mouse_follows_focus, true)?;

        Ok(())
    }

    #[tracing::instrument(skip(self))]
    pub fn update_focused_workspace(
        &mut self,
        follow_focus: bool,
        trigger_focus: bool,
    ) -> eyre::Result<()> {
        let offset = self.work_area_offset;
        let mouse_follows_focus = self.mouse_follows_focus;

        self.focused_monitor_mut()
            .ok_or_eyre("there is no monitor")?
            .update_focused_workspace(offset)?;

        if follow_focus && trigger_focus {
            // When a monocle container is active, workspace.update() already positioned it at
            // the work area. Focus that window directly: the focused container is one of the
            // tiles behind it, and the pointer is to follow the window that is in front.
            let monocle_window = self
                .focused_workspace_mut()?
                .monocle_container
                .as_mut()
                .and_then(|c| c.focused_window_mut())
                .cloned();

            if let Some(window) = monocle_window {
                window.focus(mouse_follows_focus)?;
            } else if let Ok(window) = self.focused_window_mut() {
                window.focus(mouse_follows_focus)?;
            }
        }

        Ok(())
    }

    /// Gives the focused workspace the layout `layout` and tiles it again
    ///
    /// # Errors
    ///
    /// There is no focused workspace, or it cannot be tiled.
    #[tracing::instrument(skip(self))]
    pub fn change_workspace_layout_default(&mut self, layout: DefaultLayout) -> eyre::Result<()> {
        tracing::info!("changing layout");

        let workspace = self.focused_workspace_mut()?;

        workspace.layout = Layout::Default(layout);
        self.update_focused_workspace(self.mouse_follows_focus, false)
    }

    #[tracing::instrument(skip(self))]
    pub fn set_workspace_layout_default(
        &mut self,
        monitor_idx: usize,
        workspace_idx: usize,
        layout: DefaultLayout,
    ) -> eyre::Result<()> {
        tracing::info!("setting workspace layout");

        let focused_monitor_idx = self.focused_monitor_idx();

        let monitor = self
            .monitors_mut()
            .get_mut(monitor_idx)
            .ok_or_eyre("there is no monitor")?;

        let focused_workspace_idx = monitor.focused_workspace_idx();

        let workspace = monitor
            .workspaces_mut()
            .get_mut(workspace_idx)
            .ok_or_eyre("there is no monitor")?;

        workspace.layout = Layout::Default(layout);

        // If this is the focused workspace on a non-focused screen, let's update it
        if focused_monitor_idx != monitor_idx && focused_workspace_idx == workspace_idx {
            workspace.update()?;
            Ok(())
        } else {
            Ok(self.update_focused_workspace(false, false)?)
        }
    }

    pub fn focused_workspace_idx(&self) -> eyre::Result<usize> {
        Ok(self
            .focused_monitor()
            .ok_or_eyre("there is no monitor")?
            .focused_workspace_idx())
    }

    pub fn focused_workspace(&self) -> eyre::Result<&Workspace> {
        self.focused_monitor()
            .ok_or_eyre("there is no monitor")?
            .focused_workspace()
            .ok_or_eyre("there is no workspace")
    }

    pub fn focused_workspace_mut(&mut self) -> eyre::Result<&mut Workspace> {
        self.focused_monitor_mut()
            .ok_or_eyre("there is no monitor")?
            .focused_workspace_mut()
            .ok_or_eyre("there is no workspace")
    }

    /// Returns the index of the workspace the monitor at `idx` shows
    ///
    /// # Errors
    ///
    /// There is no monitor at the index.
    pub fn focused_workspace_idx_for_monitor_idx(&self, idx: usize) -> eyre::Result<usize> {
        Ok(self
            .monitors()
            .get(idx)
            .ok_or_eyre("there is no monitor at this index")?
            .focused_workspace_idx())
    }

    pub fn focused_workspace_for_monitor_idx(&self, idx: usize) -> eyre::Result<&Workspace> {
        self.monitors()
            .get(idx)
            .ok_or_eyre("there is no monitor at this index")?
            .focused_workspace()
            .ok_or_eyre("there is no workspace")
    }

    pub fn focused_workspace_for_monitor_idx_mut(
        &mut self,
        idx: usize,
    ) -> eyre::Result<&mut Workspace> {
        self.monitors_mut()
            .get_mut(idx)
            .ok_or_eyre("there is no monitor at this index")?
            .focused_workspace_mut()
            .ok_or_eyre("there is no workspace")
    }

    #[tracing::instrument(skip(self))]
    pub fn new_workspace(&mut self) -> eyre::Result<()> {
        tracing::info!("adding new workspace");

        let mouse_follows_focus = self.mouse_follows_focus;
        let monitor = self
            .focused_monitor_mut()
            .ok_or_eyre("there is no workspace")?;

        monitor.focus_workspace(monitor.new_workspace_idx())?;
        monitor.load_focused_workspace(mouse_follows_focus)?;

        self.update_focused_workspace(self.mouse_follows_focus, false)
    }

    pub fn focused_container(&self) -> eyre::Result<&Container> {
        self.focused_workspace()?
            .focused_container()
            .ok_or_eyre("there is no container")
    }

    pub fn focused_container_idx(&self) -> eyre::Result<usize> {
        Ok(self.focused_workspace()?.focused_container_idx())
    }

    pub fn focused_container_mut(&mut self) -> eyre::Result<&mut Container> {
        self.focused_workspace_mut()?
            .focused_container_mut()
            .ok_or_eyre("there is no container")
    }

    pub fn focused_window(&self) -> eyre::Result<&Window> {
        self.focused_container()?
            .focused_window()
            .ok_or_eyre("there is no window")
    }

    fn focused_window_mut(&mut self) -> eyre::Result<&mut Window> {
        self.focused_container_mut()?
            .focused_window_mut()
            .ok_or_eyre("there is no window")
    }

    pub fn focused_monitor_size(&self) -> eyre::Result<Rect> {
        Ok(self
            .focused_monitor()
            .ok_or_eyre("there is no monitor")?
            .size)
    }

    pub fn focused_monitor_work_area(&self) -> eyre::Result<Rect> {
        Ok(self
            .focused_monitor()
            .ok_or_eyre("there is no monitor")?
            .work_area_size)
    }

    /// Takes a window the user minimized out of the focused workspace, and keeps it until it is
    /// restored
    pub fn extract_minimized_window(&mut self, window_id: u32) -> eyre::Result<()> {
        let workspace = self.focused_workspace_mut()?;
        let window = workspace.remove_window(window_id)?;
        self.minimized_windows.insert(window_id, window);

        Ok(())
    }

    /// Drop every window that no longer exists from the focused workspace and retile
    pub fn reap_invalid_windows(&mut self) -> eyre::Result<()> {
        let reaped_count = self.focused_workspace_mut()?.reap_invalid_windows()?;

        if reaped_count > 0 {
            tracing::debug!("reaped {reaped_count} invalid window(s)");
        }

        self.update_focused_workspace(false, false)
    }

    /// Stacks the focused window with the window in the container next to it in `direction`: the
    /// two end up in one container, which shows the window that moved and hides the other.
    /// Nothing happens when the layout has no container in that direction. A container that is
    /// left with no window is removed.
    ///
    /// # Errors
    ///
    /// - There is no focused workspace, or it has no container.
    /// - The windows cannot be moved into one container, or the one in front cannot be focused.
    /// - The workspace cannot be tiled again.
    #[tracing::instrument(skip(self))]
    pub fn add_window_to_container(&mut self, direction: OperationDirection) -> eyre::Result<()> {
        tracing::info!("adding window to container");
        let mouse_follows_focus = self.mouse_follows_focus;

        let workspace = self.focused_workspace_mut()?;
        let len = NonZeroUsize::new(workspace.containers_mut().len())
            .ok_or_eyre("there must be at least one container")?;
        let current_container_idx = workspace.focused_container_idx();

        let is_valid = direction
            .destination(
                workspace.layout.as_boxed_direction().as_ref(),
                workspace.layout_flip,
                workspace.focused_container_idx(),
                len,
                workspace.layout_options,
            )
            .is_some();

        if is_valid {
            let new_idx = workspace
                .new_idx_for_direction(direction)
                .ok_or_eyre("this is not a valid direction from the current position")?;

            let mut changed_focus = false;

            let adjusted_new_index = if new_idx > current_container_idx
                && !matches!(
                    workspace.layout,
                    Layout::Default(DefaultLayout::Grid)
                        | Layout::Default(DefaultLayout::UltrawideVerticalStack)
                ) {
                workspace.focus_container(new_idx);
                changed_focus = true;
                new_idx.saturating_sub(1)
            } else {
                new_idx
            };

            let mut target_container_is_stack = false;

            if let Some(container) = workspace.containers().get(adjusted_new_index)
                && container.windows().len() > 1
            {
                target_container_is_stack = true;
            }

            if let Some(current) = workspace.focused_container() {
                if current.windows().len() > 1 && !target_container_is_stack {
                    workspace.focus_container(adjusted_new_index);
                    changed_focus = true;
                    workspace.move_window_to_container(current_container_idx)?;
                } else {
                    workspace.move_window_to_container(adjusted_new_index)?;
                }
            }
            if changed_focus && let Some(container) = workspace.focused_container_mut() {
                container.load_focused_window()?;
                if let Some(window) = container.focused_window() {
                    window.focus(mouse_follows_focus)?;
                }
            }

            self.update_focused_workspace(mouse_follows_focus, false)?;
        }

        Ok(())
    }

    /// Takes the focused window out of its stack into a container of its own, next to the stack,
    /// and shows the stack's next window in its place
    ///
    /// # Errors
    ///
    /// - The focused container has only one window.
    /// - There is no focused workspace, container or window.
    /// - The workspace cannot be tiled again.
    #[tracing::instrument(skip(self))]
    pub fn remove_window_from_container(&mut self) -> eyre::Result<()> {
        tracing::info!("removing window");

        if self.focused_container()?.windows().len() == 1 {
            bail!("a container must have at least one window");
        }

        let workspace = self.focused_workspace_mut()?;

        workspace.new_container_for_focused_window()?;
        self.update_focused_workspace(self.mouse_follows_focus, false)
    }

    /// Stops the daemon: brings back the windows it has hidden, unless `ignore_restore` is set
    /// ([`WindowManager::restore_all_windows`]), makes every window komorebi made see-through
    /// opaque again ([`crate::transparency_manager::restore`]), connects to every subscriber and
    /// closes the connection without writing anything, removes the command socket, lets the
    /// backend give up what it set up in the compositor (the desktop hidden windows are parked
    /// on), and exits the process with status 0
    ///
    /// # Errors
    ///
    /// A connection to a subscriber cannot be shut down. The daemon does not stop.
    #[tracing::instrument(skip(self))]
    pub fn stop(&mut self, ignore_restore: bool) -> eyre::Result<()> {
        tracing::info!(
            "received stop command, restoring all hidden windows and terminating process"
        );

        self.restore_all_windows(ignore_restore)?;

        crate::transparency_manager::restore(self);

        let sockets = SUBSCRIPTION_SOCKETS.lock();
        for path in (*sockets).values() {
            if let Ok(stream) = UnixStream::connect(path) {
                stream.shutdown(Shutdown::Both)?;
            }
        }

        let socket = DATA_DIR.join("komorebi.sock");
        let _ = std::fs::remove_file(socket);

        backend().shutdown();

        std::process::exit(0)
    }

    /// Brings back every window komorebi has hidden, wherever it is (in a stack, or on a workspace
    /// that is not in front): they are exactly the ones it recorded when it hid them
    /// ([`struct@HIDDEN_WINDOWS`]). With `ignore_restore` it brings back none.
    ///
    /// It never fails; the result is there so that callers can use `?`.
    #[tracing::instrument(skip(self))]
    pub fn restore_all_windows(&mut self, ignore_restore: bool) -> eyre::Result<()> {
        tracing::info!("restoring all hidden windows");

        if !ignore_restore {
            let hidden: Vec<u32> = HIDDEN_WINDOWS.lock().iter().copied().collect();
            for id in hidden {
                Window::from(id).restore();
            }
        }

        Ok(())
    }

    /// Brings the next or the previous window of the focused stack to the front, wrapping around,
    /// and gives it the focus; the window that was in front is hidden
    ///
    /// # Errors
    ///
    /// - The focused container has no window, or only one.
    /// - There is no focused workspace or container, or the window cannot be focused.
    /// - The workspace cannot be tiled again.
    #[tracing::instrument(skip(self))]
    pub fn cycle_container_window_in_direction(
        &mut self,
        direction: CycleDirection,
    ) -> eyre::Result<()> {
        tracing::info!("cycling container windows");

        let mouse_follows_focus = self.mouse_follows_focus;

        let container = self.focused_container_mut()?;

        let len = NonZeroUsize::new(container.windows().len())
            .ok_or_eyre("there must be at least one window in a container")?;

        if len.get() == 1 {
            eyre::bail!("there is only one window in this container");
        }

        let current_idx = container.focused_window_idx();
        let next_idx = direction.next_idx(current_idx, len);

        container.focus_window(next_idx);
        container.load_focused_window()?;

        if let Some(window) = container.focused_window() {
            window.focus(mouse_follows_focus)?;
        }

        self.update_focused_workspace(mouse_follows_focus, true)
    }

    /// Shows workspace `idx` on the focused monitor, creating it (and the ones before it) if it
    /// does not exist yet.
    ///
    /// The compositor is asked to show the workspace's desktop on that monitor
    /// ([`Monitor::load_focused_workspace`]): it shows the windows on it and hides the others, and
    /// komorebi focuses the workspace's focused window and tiles it.
    #[tracing::instrument(skip(self))]
    pub fn focus_workspace(&mut self, idx: usize) -> eyre::Result<()> {
        tracing::info!("focusing workspace");

        let mouse_follows_focus = self.mouse_follows_focus;
        let monitor = self
            .focused_monitor_mut()
            .ok_or_eyre("there is no workspace")?;

        monitor.focus_workspace(idx)?;
        monitor.load_focused_workspace(mouse_follows_focus)?;

        self.update_focused_workspace(false, true)
    }

    /// Moves the focused container of the focused monitor to workspace `idx`, creating it if it
    /// does not exist yet, and shows that workspace when `follow` is set.
    ///
    /// The container's windows are put on the workspace's desktop before the desktop is shown or
    /// one of them is focused ([`Monitor::load_focused_workspace`]): a window that is focused while
    /// it is still on the desktop it came from takes the monitor back to that desktop.
    #[tracing::instrument(skip(self))]
    pub fn move_container_to_workspace(
        &mut self,
        idx: usize,
        follow: bool,
        direction: Option<OperationDirection>,
    ) -> eyre::Result<()> {
        tracing::info!("moving container");

        let mouse_follows_focus = self.mouse_follows_focus;
        let monitor = self
            .focused_monitor_mut()
            .ok_or_eyre("there is no monitor")?;

        monitor.move_container_to_workspace(idx, follow, direction)?;
        monitor.load_focused_workspace(mouse_follows_focus)?;

        self.update_focused_workspace(mouse_follows_focus, true)?;

        Ok(())
    }

    /// Turns the monocle of the focused workspace on, for the focused container, or off, and tiles
    /// the workspace again
    ///
    /// # Errors
    ///
    /// - There is no focused workspace, or it has no container to turn into a monocle.
    /// - The workspace cannot be tiled, or the backend refuses to focus the window.
    #[tracing::instrument(skip(self))]
    pub fn toggle_monocle(&mut self) -> eyre::Result<()> {
        let workspace = self.focused_workspace()?;
        match workspace.monocle_container {
            None => self.monocle_on()?,
            Some(_) => self.monocle_off()?,
        }

        self.update_focused_workspace(true, true)?;

        Ok(())
    }

    /// Toggles the compositor's own maximize for the focused window: it leaves the layout, fills
    /// the work area, and goes back to its tile, where it was, when toggled again
    ///
    /// # Errors
    ///
    /// - There is no focused workspace, or nothing to maximize on it.
    /// - The compositor refuses to maximize or to restore the window.
    #[tracing::instrument(skip(self))]
    pub fn toggle_maximize(&mut self) -> eyre::Result<()> {
        self.handle_unmanaged_window_behaviour()?;

        let workspace = self.focused_workspace_mut()?;

        match workspace.maximized_window {
            None => self.maximize_window()?,
            Some(_) => self.unmaximize_window()?,
        }

        self.update_focused_workspace(true, false)
    }

    /// Takes the focused window out of the layout of the focused workspace and has the compositor
    /// maximize it ([`Workspace::new_maximized_window`])
    ///
    /// # Errors
    ///
    /// See [`Workspace::new_maximized_window`].
    #[tracing::instrument(skip(self))]
    pub fn maximize_window(&mut self) -> eyre::Result<()> {
        tracing::info!("maximizing window");

        let workspace = self.focused_workspace_mut()?;
        workspace.new_maximized_window()
    }

    /// Puts the maximized window of the focused workspace back in the layout, where it was
    /// ([`Workspace::reintegrate_maximized_window`])
    ///
    /// # Errors
    ///
    /// See [`Workspace::reintegrate_maximized_window`].
    #[tracing::instrument(skip(self))]
    pub fn unmaximize_window(&mut self) -> eyre::Result<()> {
        tracing::info!("unmaximizing window");

        let workspace = self.focused_workspace_mut()?;
        workspace.reintegrate_maximized_window()
    }

    /// Takes the focused container out of the layout of the focused workspace and makes it the
    /// monocle container, which the next tiling gives the whole work area, and hides the windows
    /// of the other containers
    ///
    /// # Errors
    ///
    /// There is no focused workspace, or it has no container.
    #[tracing::instrument(skip(self))]
    pub fn monocle_on(&mut self) -> eyre::Result<()> {
        tracing::info!("enabling monocle");

        let workspace = self.focused_workspace_mut()?;
        workspace.new_monocle_container()?;

        for container in workspace.containers_mut() {
            container.hide(None)?;
        }

        Ok(())
    }

    /// Shows the windows of the other containers again, and puts the monocle container of the
    /// focused workspace back in the layout, where it was
    ///
    /// # Errors
    ///
    /// There is no focused workspace, or it has no monocle container.
    #[tracing::instrument(skip(self))]
    pub fn monocle_off(&mut self) -> eyre::Result<()> {
        tracing::info!("disabling monocle");

        let workspace = self.focused_workspace_mut()?;

        for container in workspace.containers_mut() {
            container.restore()?;
        }

        workspace.reintegrate_monocle_container()
    }

    /// Puts the monocle container back in the layout and makes the next (or the previous)
    /// container the monocle, wrapping around at the ends, hides the windows of the containers
    /// that are left, and tiles the workspace again
    ///
    /// A workspace where the monocle is the only container is left as it is.
    ///
    /// # Errors
    ///
    /// - There is no focused workspace, or it has no monocle container.
    /// - The workspace cannot be tiled, or the backend refuses to focus the window.
    #[tracing::instrument(skip(self))]
    pub fn cycle_monocle(&mut self, direction: CycleDirection) -> eyre::Result<()> {
        tracing::info!("cycling monocle container");

        if self.focused_workspace()?.containers().is_empty() {
            return Ok(());
        }

        self.focused_workspace_mut()?
            .cycle_monocle_container(direction)?;

        for container in self.focused_workspace_mut()?.containers_mut() {
            container.hide(None)?;
        }

        self.update_focused_workspace(true, true)
    }

    /// Takes the window that has the focus in the compositor out of the layout of the focused
    /// workspace, or puts it back when it is one of its floating windows and `force_float` is not
    /// set, and tiles the workspace again
    ///
    /// A window that is floated puts the workspace on its floating layer, and one that is put back
    /// puts it on its tiling layer ([`Workspace::layer`]). While the workspace has a monocle
    /// container nothing is done: the message is logged and ignored.
    ///
    /// # Errors
    ///
    /// - The compositor reports no focused window.
    /// - There is no focused workspace, or the window cannot be floated or put back, for one of
    ///   the reasons [`WindowManager::float_window`] and [`WindowManager::unfloat_window`] give.
    /// - The workspace cannot be tiled again.
    #[tracing::instrument(skip(self))]
    pub fn toggle_float(&mut self, force_float: bool) -> eyre::Result<()> {
        let window_id = backend()
            .foreground_window()
            .ok_or_eyre("no foreground window")?;

        let workspace = self.focused_workspace_mut()?;
        if workspace.monocle_container.is_some() {
            tracing::warn!("ignoring toggle-float command while workspace has a monocle container");
            return Ok(());
        }

        let mut is_floating_window = false;

        for window in workspace.floating_windows() {
            if window.id == window_id {
                is_floating_window = true;
            }
        }

        if is_floating_window && !force_float {
            workspace.layer = WorkspaceLayer::Tiling;
            self.unfloat_window()?;
        } else {
            workspace.layer = WorkspaceLayer::Floating;
            self.float_window()?;
        }

        self.update_focused_workspace(is_floating_window, true)
    }

    /// Takes the focused window out of its container into the floating windows of the focused
    /// workspace ([`Workspace::new_floating_window`]), places it as
    /// [`WindowManagementBehaviour::toggle_float_placement`] says, and gives it the focus
    ///
    /// # Errors
    ///
    /// There is no focused monitor, workspace, container or window, or the backend refuses to
    /// move or focus the window.
    #[tracing::instrument(skip(self))]
    pub fn float_window(&mut self) -> eyre::Result<()> {
        tracing::info!("floating window");

        let mouse_follows_focus = self.mouse_follows_focus;
        let work_area = self.focused_monitor_work_area()?;

        let toggle_float_placement = self.window_management_behaviour.toggle_float_placement;

        let workspace = self.focused_workspace_mut()?;
        workspace.new_floating_window()?;

        let window = workspace
            .floating_windows_mut()
            .back_mut()
            .ok_or_eyre("there is no floating window")?;

        if toggle_float_placement.should_center() {
            window.center(&work_area, toggle_float_placement.should_resize())?;
        }
        window.focus(mouse_follows_focus)?;

        Ok(())
    }

    /// Puts the floating window that has the focus in the compositor back in the layout of the
    /// focused workspace ([`Workspace::new_container_for_floating_window`])
    ///
    /// # Errors
    ///
    /// There is no focused workspace, or the focused window is not one of its floating windows.
    #[tracing::instrument(skip(self))]
    pub fn unfloat_window(&mut self) -> eyre::Result<()> {
        tracing::info!("unfloating window");

        let workspace = self.focused_workspace_mut()?;
        workspace.new_container_for_floating_window()
    }

    /// Resizes the focused container by `delta` pixels on its `direction` side, by giving it a
    /// resize adjustment that the layout applies, and tiles the workspace again if `update` is set
    ///
    /// On the floating layer it is the frame of the focused floating window that changes, and no
    /// adjustment is made and nothing is tiled. With `Sizing::Increase` on the left or the upper
    /// side the frame moves by `delta` pixels that way, and on the right or the lower side the
    /// window gets wider or taller by twice `delta`, never out of the work area of the monitor;
    /// `Sizing::Decrease` does the opposite.
    ///
    /// The neighbors on that side give up the room, and none of the tiles may end up smaller than
    /// a tenth of the monitor's work area, measured before the paddings are taken off, or than it
    /// already is: a resize that would make one smaller is granted as far as it can be. Where
    /// there is no container on that side, nothing changes.
    ///
    /// # Errors
    ///
    /// - There is no focused monitor, workspace or container.
    /// - The workspace cannot be tiled again.
    #[tracing::instrument(skip(self))]
    pub fn resize_window(
        &mut self,
        direction: OperationDirection,
        sizing: Sizing,
        delta: i32,
        update: bool,
    ) -> eyre::Result<()> {
        let mouse_follows_focus = self.mouse_follows_focus;
        let mut focused_monitor_work_area = self.focused_monitor_work_area()?;
        let workspace = self.focused_workspace_mut()?;

        match workspace.layer {
            WorkspaceLayer::Floating => {
                let workspace = self.focused_workspace()?;
                let focused_window_id = backend()
                    .foreground_window()
                    .ok_or_eyre("no foreground window")?;

                let border_offset = 0;
                let border_width = 0;
                focused_monitor_work_area.left += border_offset;
                focused_monitor_work_area.left += border_width;
                focused_monitor_work_area.top += border_offset;
                focused_monitor_work_area.top += border_width;
                focused_monitor_work_area.right -= border_offset * 2;
                focused_monitor_work_area.right -= border_width * 2;
                focused_monitor_work_area.bottom -= border_offset * 2;
                focused_monitor_work_area.bottom -= border_width * 2;

                for window in workspace.floating_windows().iter() {
                    if window.id == focused_window_id {
                        let mut rect = window.rect()?;
                        match (direction, sizing) {
                            (OperationDirection::Left, Sizing::Increase) => {
                                if rect.left - delta < focused_monitor_work_area.left {
                                    rect.left = focused_monitor_work_area.left;
                                } else {
                                    rect.left -= delta;
                                }
                            }
                            (OperationDirection::Left, Sizing::Decrease) => {
                                rect.left += delta;
                            }
                            (OperationDirection::Right, Sizing::Increase) => {
                                if rect.left + rect.right + delta * 2
                                    > focused_monitor_work_area.left
                                        + focused_monitor_work_area.right
                                {
                                    rect.right = focused_monitor_work_area.left
                                        + focused_monitor_work_area.right
                                        - rect.left;
                                } else {
                                    rect.right += delta * 2;
                                }
                            }
                            (OperationDirection::Right, Sizing::Decrease) => {
                                rect.right -= delta * 2;
                            }
                            (OperationDirection::Up, Sizing::Increase) => {
                                if rect.top - delta < focused_monitor_work_area.top {
                                    rect.top = focused_monitor_work_area.top;
                                } else {
                                    rect.top -= delta;
                                }
                            }
                            (OperationDirection::Up, Sizing::Decrease) => {
                                rect.top += delta;
                            }
                            (OperationDirection::Down, Sizing::Increase) => {
                                if rect.top + rect.bottom + delta * 2
                                    > focused_monitor_work_area.top
                                        + focused_monitor_work_area.bottom
                                {
                                    rect.bottom = focused_monitor_work_area.top
                                        + focused_monitor_work_area.bottom
                                        - rect.top;
                                } else {
                                    rect.bottom += delta * 2;
                                }
                            }
                            (OperationDirection::Down, Sizing::Decrease) => {
                                rect.bottom -= delta * 2;
                            }
                        }

                        window.set_position(&rect)?;

                        if mouse_follows_focus {
                            backend().center_cursor_in(&rect)?;
                        }

                        break;
                    }
                }
            }
            WorkspaceLayer::Tiling => {
                match workspace.layout {
                    Layout::Default(layout) => {
                        tracing::debug!("resizing window");
                        let len = NonZeroUsize::new(workspace.containers().len())
                            .ok_or_eyre("there must be at least one container")?;
                        let focused_idx = workspace.focused_container_idx();
                        let focused_idx_resize = workspace
                            .resize_dimensions
                            .get(focused_idx)
                            .ok_or_eyre("there is no resize adjustment for this container")?;

                        if direction
                            .destination(
                                workspace.layout.as_boxed_direction().as_ref(),
                                workspace.layout_flip,
                                focused_idx,
                                len,
                                workspace.layout_options,
                            )
                            .is_some()
                        {
                            let unaltered = layout.calculate(
                                &focused_monitor_work_area,
                                len,
                                workspace.container_padding,
                                workspace.layout_flip,
                                &[],
                                workspace.focused_container_idx(),
                                workspace.layout_options,
                                &workspace.latest_layout,
                            );

                            let mut direction = direction;

                            // We only ever want to operate on the unflipped Rect positions when resizing, then we
                            // can flip them however they need to be flipped once the resizing has been done
                            if let Some(flip) = workspace.layout_flip {
                                match flip {
                                    Axis::Horizontal => {
                                        if matches!(direction, OperationDirection::Left)
                                            || matches!(direction, OperationDirection::Right)
                                        {
                                            direction = direction.opposite();
                                        }
                                    }
                                    Axis::Vertical => {
                                        if matches!(direction, OperationDirection::Up)
                                            || matches!(direction, OperationDirection::Down)
                                        {
                                            direction = direction.opposite();
                                        }
                                    }
                                    Axis::HorizontalAndVertical => direction = direction.opposite(),
                                }
                            }

                            let unaltered_tile = *unaltered
                                .get(focused_idx)
                                .ok_or_eyre("there is no last layout")?;

                            // How much of what was asked for the layout can take. The layout crate no
                            // longer turns a resize down by the size of the container being resized
                            // (the wrong measure: the room is in the neighbours, and a narrow tile
                            // could not be dragged by more than half its own width, however much its
                            // neighbours had to give); the right one is what every tile looks like
                            // afterwards. None may end up smaller than a tenth of the work area, or
                            // than it already is, and a request that would is granted as far as it
                            // can be rather than dropped.
                            let min_width = focused_monitor_work_area.right / MINIMUM_TILE_FRACTION;
                            let min_height =
                                focused_monitor_work_area.bottom / MINIMUM_TILE_FRACTION;

                            let calculate = |dimensions: &[Option<Rect>]| {
                                layout.calculate(
                                    &focused_monitor_work_area,
                                    len,
                                    workspace.container_padding,
                                    workspace.layout_flip,
                                    dimensions,
                                    focused_idx,
                                    workspace.layout_options,
                                    &workspace.latest_layout,
                                )
                            };
                            let now = calculate(&workspace.resize_dimensions);
                            let candidate = |amount: i32| {
                                layout.resize(
                                    &unaltered_tile,
                                    focused_idx_resize,
                                    direction,
                                    sizing,
                                    amount,
                                )
                            };
                            let fits = |amount: i32| {
                                let mut dimensions = workspace.resize_dimensions.clone();
                                dimensions[focused_idx] = candidate(amount);
                                calculate(&dimensions).iter().zip(now.iter()).all(
                                    |(tile, before)| {
                                        tile.right >= min_width.min(before.right)
                                            && tile.bottom >= min_height.min(before.bottom)
                                    },
                                )
                            };

                            let granted = if fits(delta) {
                                delta
                            } else {
                                // the tiles only ever get smaller as the amount grows
                                let (mut fits_at, mut fails_at) = (0, delta);
                                while fails_at - fits_at > 1 {
                                    let middle = fits_at + (fails_at - fits_at) / 2;
                                    if fits(middle) {
                                        fits_at = middle;
                                    } else {
                                        fails_at = middle;
                                    }
                                }
                                fits_at
                            };

                            if granted != delta {
                                tracing::debug!(
                                    "granting {granted} of the {delta} asked for: a tile would get smaller than {min_width}x{min_height}"
                                );
                            }

                            if granted > 0 {
                                let resize = candidate(granted);
                                workspace.resize_dimensions[focused_idx] = resize;
                            }

                            return if update {
                                self.update_focused_workspace(false, false)
                            } else {
                                Ok(())
                            };
                        }

                        tracing::debug!("cannot resize container in this direction");
                    }
                }
            }
        }

        Ok(())
    }

    /// Tiles the focused workspace of every monitor, without the resize adjustments of its
    /// containers unless `preserve_resize_dimensions` is set, sets the wallpaper each of them is
    /// to show ([`Workspace::apply_wallpaper`]), and asks the backend for the look again
    /// ([`border_manager::destroy_all_borders`])
    #[tracing::instrument(skip(self))]
    pub fn retile_all(&mut self, preserve_resize_dimensions: bool) -> eyre::Result<()> {
        let offset = self.work_area_offset;

        for monitor in self.monitors_mut() {
            let offset = if monitor.work_area_offset.is_some() {
                monitor.work_area_offset
            } else {
                offset
            };

            let focused_workspace_idx = monitor.focused_workspace_idx();
            monitor.update_workspace_globals(focused_workspace_idx, offset);

            let monitor_id = monitor.id;
            let monitor_wp = monitor.wallpaper.clone();
            let workspace = monitor
                .focused_workspace_mut()
                .ok_or_eyre("there is no workspace")?;

            // Reset any resize adjustments if we want to force a retile
            if !preserve_resize_dimensions {
                workspace.resize_dimensions.fill(None);
            }

            if (workspace.wallpaper.is_some() || monitor_wp.is_some())
                && let Err(error) = workspace.apply_wallpaper(monitor_id, &monitor_wp)
            {
                tracing::error!("failed to apply wallpaper: {}", error);
            }

            workspace.update()?;
        }

        border_manager::destroy_all_borders()?;

        Ok(())
    }

    /// Moves the focused container to the first index of the focused workspace, which is the
    /// largest tile, and tiles the workspace again: the containers before it each move one index
    /// on
    ///
    /// The grid layout has no largest tile, and nothing changes there.
    ///
    /// # Errors
    ///
    /// There is no focused workspace, it has no containers, or it cannot be tiled again.
    #[tracing::instrument(skip(self))]
    pub fn promote_container_to_front(&mut self) -> eyre::Result<()> {
        let workspace = self.focused_workspace_mut()?;

        if matches!(workspace.layout, Layout::Default(DefaultLayout::Grid)) {
            tracing::debug!("ignoring promote command for grid layout");
            return Ok(());
        }

        tracing::info!("promoting container");

        workspace.promote_container()?;
        self.update_focused_workspace(self.mouse_follows_focus, true)
    }

    /// Swaps the focused container with the one at the first index, the largest tile, and tiles
    /// the workspace again; when the focused container is the first already, swaps it back with
    /// the container it was swapped with last
    ///
    /// The grid layout has no largest tile, and nothing changes there.
    ///
    /// # Errors
    ///
    /// There is no focused workspace, or it cannot be tiled again.
    #[tracing::instrument(skip(self))]
    pub fn promote_container_swap(&mut self) -> eyre::Result<()> {
        let workspace = self.focused_workspace_mut()?;
        let focused_container_idx = workspace.focused_container_idx();

        let primary_idx = match workspace.layout {
            Layout::Default(_) => 0,
        };

        if matches!(workspace.layout, Layout::Default(DefaultLayout::Grid)) {
            tracing::debug!("ignoring promote-swap command for grid layout");
            return Ok(());
        }

        let primary_tile_is_focused = focused_container_idx == primary_idx;

        if primary_tile_is_focused && let Some(swap_idx) = workspace.promotion_swap_container_idx {
            workspace.swap_containers(focused_container_idx, swap_idx);
        } else {
            workspace.promotion_swap_container_idx = Some(focused_container_idx);
            workspace.swap_containers(focused_container_idx, primary_idx);
        }

        self.update_focused_workspace(self.mouse_follows_focus, true)
    }

    /// Gives the focus to the container at the first index, the largest tile, without moving
    /// anything
    ///
    /// The grid layout has no largest tile, and nothing changes there.
    ///
    /// # Errors
    ///
    /// There is no focused workspace, or the backend refuses to focus the window.
    #[tracing::instrument(skip(self))]
    pub fn promote_focus_to_front(&mut self) -> eyre::Result<()> {
        let workspace = self.focused_workspace_mut()?;

        if matches!(workspace.layout, Layout::Default(DefaultLayout::Grid)) {
            tracing::info!("ignoring promote focus command for grid layout");
            return Ok(());
        }

        tracing::info!("promoting focus");

        let target_idx = match workspace.layout {
            Layout::Default(_) => 0,
        };

        workspace.focus_container(target_idx);
        self.update_focused_workspace(self.mouse_follows_focus, true)
    }

    /// Mirrors the layout of the focused workspace along `layout_flip`, on top of the way it is
    /// mirrored already, and tiles the workspace again
    ///
    /// Flipping along an axis the layout is already flipped along undoes that flip, so flipping
    /// twice along the same axis puts the layout back.
    ///
    /// # Errors
    ///
    /// There is no focused workspace, or it cannot be tiled.
    #[tracing::instrument(skip(self))]
    pub fn flip_layout(&mut self, layout_flip: Axis) -> eyre::Result<()> {
        let workspace = self.focused_workspace_mut()?;

        tracing::info!("flipping layout");

        #[allow(clippy::match_same_arms)]
        match workspace.layout_flip {
            None => {
                workspace.layout_flip = Option::from(layout_flip);
            }
            Some(current_layout_flip) => {
                match current_layout_flip {
                    Axis::Horizontal => match layout_flip {
                        Axis::Horizontal => workspace.layout_flip = None,
                        Axis::Vertical => {
                            workspace.layout_flip = Option::from(Axis::HorizontalAndVertical)
                        }
                        Axis::HorizontalAndVertical => {
                            workspace.layout_flip = Option::from(Axis::HorizontalAndVertical)
                        }
                    },
                    Axis::Vertical => match layout_flip {
                        Axis::Horizontal => {
                            workspace.layout_flip = Option::from(Axis::HorizontalAndVertical)
                        }
                        Axis::Vertical => workspace.layout_flip = None,
                        Axis::HorizontalAndVertical => {
                            workspace.layout_flip = Option::from(Axis::HorizontalAndVertical)
                        }
                    },
                    Axis::HorizontalAndVertical => match layout_flip {
                        Axis::Horizontal => workspace.layout_flip = Option::from(Axis::Vertical),
                        Axis::Vertical => workspace.layout_flip = Option::from(Axis::Horizontal),
                        Axis::HorizontalAndVertical => workspace.layout_flip = None,
                    },
                };
            }
        }

        self.update_focused_workspace(false, false)
    }

    /// Gives the focused workspace the next (or the previous) layout, in the order of
    /// [`DefaultLayout`], and tiles it again
    ///
    /// # Errors
    ///
    /// There is no focused workspace, or it cannot be tiled.
    #[tracing::instrument(skip(self))]
    pub fn cycle_layout(&mut self, direction: CycleDirection) -> eyre::Result<()> {
        tracing::info!("cycling layout");

        let workspace = self.focused_workspace_mut()?;

        match workspace.layout {
            Layout::Default(current) => {
                let new_layout = match direction {
                    CycleDirection::Previous => current.cycle_previous(),
                    CycleDirection::Next => current.cycle_next(),
                };

                tracing::info!("next layout: {new_layout}");
                workspace.layout = Layout::Default(new_layout);
            }
        }

        self.update_focused_workspace(self.mouse_follows_focus, false)
    }

    /// Locks the focused container of the focused workspace at its index, or unlocks it
    ///
    /// A locked container stays at its index while containers are inserted, removed and swapped
    /// around it. Nothing is retiled.
    ///
    /// # Errors
    ///
    /// There is no focused workspace.
    #[tracing::instrument(skip(self))]
    pub fn toggle_lock(&mut self) -> eyre::Result<()> {
        let workspace = self.focused_workspace_mut()?;
        if let Some(container) = workspace.focused_container_mut() {
            // Toggle the locked flag
            container.set_locked(!container.locked());
        }
        Ok(())
    }

    /// Turns the tiling of the focused workspace off, or back on and tiles it again
    ///
    /// While its tiling is off, the workspace keeps its containers and moves no window.
    ///
    /// # Errors
    ///
    /// There is no focused workspace, or it cannot be tiled.
    #[tracing::instrument(skip(self))]
    pub fn toggle_tiling(&mut self) -> eyre::Result<()> {
        let workspace = self.focused_workspace_mut()?;
        workspace.tile = !workspace.tile;
        self.update_focused_workspace(false, false)
    }

    #[tracing::instrument(skip(self))]
    pub fn focus_monitor(&mut self, idx: usize) -> eyre::Result<()> {
        tracing::info!("focusing monitor");

        if self.monitors().get(idx).is_some() {
            self.monitors.focus(idx);
        } else {
            eyre::bail!("this is not a valid monitor index");
        }

        Ok(())
    }

    // TODO: see if this logic is the same in macos when I have more than one monitor
    pub fn monitor_idx_in_direction(&self, direction: OperationDirection) -> Option<usize> {
        let current_monitor_size = self.focused_monitor_size().ok()?;

        for (idx, monitor) in self.monitors.elements().iter().enumerate() {
            match direction {
                OperationDirection::Left => {
                    if monitor.size.left + monitor.size.right == current_monitor_size.left {
                        return Option::from(idx);
                    }
                }
                OperationDirection::Right => {
                    if current_monitor_size.right + current_monitor_size.left == monitor.size.left {
                        return Option::from(idx);
                    }
                }
                OperationDirection::Up => {
                    if monitor.size.top + monitor.size.bottom == current_monitor_size.top {
                        return Option::from(idx);
                    }
                }
                OperationDirection::Down => {
                    if current_monitor_size.top + current_monitor_size.bottom == monitor.size.top {
                        return Option::from(idx);
                    }
                }
            }
        }

        None
    }

    /// The monitor a window is on: the one whose workspace holds it if it is managed, otherwise
    /// the one the middle of the window is on.
    ///
    /// A managed window is not necessarily where its workspace is: the scrolling layout runs a
    /// strip of windows past the edge of the monitor, and KWin lets them sit on the next monitor
    /// over, where asking the geometry would hand focus to a monitor that does not own them.
    pub fn monitor_idx_from_window(&self, window: Window) -> Option<usize> {
        if let Some((monitor_idx, _)) = self.known_window_ids.get(&window.id) {
            return Some(*monitor_idx);
        }

        let rect = window.rect().ok()?;
        let middle = (rect.left + rect.right / 2, rect.top + rect.bottom / 2);

        self.monitors()
            .iter()
            .position(|monitor| monitor.size.contains_point(middle))
    }

    pub fn monitor_idx_from_current_pos(&mut self) -> Option<usize> {
        let (x, y) = backend().cursor_pos()?;
        let monitor_id = backend().monitor_from_point(x, y)?;

        for (i, monitor) in self.monitors().iter().enumerate() {
            if monitor.id == monitor_id {
                return Option::from(i);
            }
        }

        // TODO: figure out if we need this on macOS
        // // our hmonitor might be stale, so if we didn't return above, try querying via the latest
        // // info taken from win32_display_data and update our hmonitor while we're at it
        // if let Ok(latest) = MacosApi::monitor(monitor_id) {
        //     for (i, monitor) in self.monitors_mut().iter_mut().enumerate() {
        //         if monitor.device_id() == latest.device_id() {
        //             monitor.set_id(latest.id());
        //             return Option::from(i);
        //         }
        //     }
        // }

        None
    }

    #[tracing::instrument(skip(self))]
    pub fn monitor_workspace_index_by_name(&mut self, name: &str) -> Option<(usize, usize)> {
        tracing::info!("looking up workspace by name");

        for (monitor_idx, monitor) in self.monitors().iter().enumerate() {
            for (workspace_idx, workspace) in monitor.workspaces().iter().enumerate() {
                if let Some(workspace_name) = &workspace.name
                    && workspace_name == name
                {
                    return Option::from((monitor_idx, workspace_idx));
                }
            }
        }

        None
    }

    /// Calculates the direction of a move across monitors given a specific monitor index
    pub fn direction_from_monitor_idx(
        &self,
        target_monitor_idx: usize,
    ) -> Option<OperationDirection> {
        let current_monitor_idx = self.focused_monitor_idx();
        if current_monitor_idx == target_monitor_idx {
            return None;
        }

        let current_monitor_size = self.focused_monitor_size().ok()?;
        let target_monitor_size = self.monitors().get(target_monitor_idx)?.size;

        if target_monitor_size.left + target_monitor_size.right == current_monitor_size.left {
            return Some(OperationDirection::Left);
        }
        if current_monitor_size.right + current_monitor_size.left == target_monitor_size.left {
            return Some(OperationDirection::Right);
        }
        if target_monitor_size.top + target_monitor_size.bottom == current_monitor_size.top {
            return Some(OperationDirection::Up);
        }
        if current_monitor_size.top + current_monitor_size.bottom == target_monitor_size.top {
            return Some(OperationDirection::Down);
        }

        None
    }

    #[tracing::instrument(skip(self))]
    pub fn move_container_to_monitor(
        &mut self,
        monitor_idx: usize,
        workspace_idx: Option<usize>,
        follow: bool,
        move_direction: Option<OperationDirection>,
    ) -> eyre::Result<()> {
        tracing::info!("moving container");

        let focused_monitor_idx = self.focused_monitor_idx();

        if focused_monitor_idx == monitor_idx
            && let Some(workspace_idx) = workspace_idx
        {
            return self.move_container_to_workspace(workspace_idx, follow, None);
        }

        let offset = self.work_area_offset;
        let mouse_follows_focus = self.mouse_follows_focus;

        let monitor = self
            .focused_monitor_mut()
            .ok_or_eyre("there is no monitor")?;

        let current_area = monitor.work_area_size;

        let workspace = monitor
            .focused_workspace_mut()
            .ok_or_eyre("there is no workspace")?;

        if workspace.maximized_window.is_some() {
            eyre::bail!("cannot move native maximized window to another monitor or workspace");
        }

        let foreground_window_id = backend()
            .foreground_window()
            .ok_or_eyre("no foreground window")?;
        let floating_window_index = workspace
            .floating_windows()
            .iter()
            .position(|w| w.id == foreground_window_id);

        let floating_window =
            floating_window_index.and_then(|idx| workspace.floating_windows_mut().remove(idx));
        let container = if floating_window_index.is_none() {
            Some(
                workspace
                    .remove_focused_container()
                    .ok_or_eyre("there is no container")?,
            )
        } else {
            None
        };
        monitor.update_focused_workspace(offset)?;

        let target_monitor = self
            .monitors_mut()
            .get_mut(monitor_idx)
            .ok_or_eyre("there is no monitor")?;

        let target_monitor_work_area_size = target_monitor.work_area_size;

        let mut should_load_workspace = false;
        if let Some(workspace_idx) = workspace_idx
            && workspace_idx != target_monitor.focused_workspace_idx()
        {
            target_monitor.focus_workspace(workspace_idx)?;
            should_load_workspace = true;
        }
        let target_workspace = target_monitor
            .focused_workspace_mut()
            .ok_or_eyre("there is no focused workspace on target monitor")?;

        if target_workspace.monocle_container.is_some() {
            for container in target_workspace.containers_mut() {
                container.restore()?;
            }

            for window in target_workspace.floating_windows_mut() {
                window.restore();
            }

            target_workspace.reintegrate_monocle_container()?;
        }

        if let Some(window) = floating_window {
            window.move_to_area(&current_area, &target_monitor_work_area_size)?;
            target_workspace.floating_windows_mut().push_back(window);
            target_workspace.layer = WorkspaceLayer::Floating;
        } else if let Some(container) = container {
            let _container_window_ids =
                container.windows().iter().map(|w| w.id).collect::<Vec<_>>();

            target_workspace.layer = WorkspaceLayer::Tiling;

            if let Some(direction) = move_direction {
                target_monitor.add_container_with_direction(container, workspace_idx, direction)?;
            } else {
                target_monitor.add_container(container, workspace_idx)?;
            }

            if let Some(workspace) = target_monitor.focused_workspace()
                && !workspace.tile
            {
                // TODO: figure out how to construct a Window from just an id here
                // for window_id in container_window_ids {
                //     Window::from(window_id)
                //         .move_to_area(&current_area, &target_monitor.work_area_size)?;
                // }
            }
        } else {
            eyre::bail!("failed to find a window to move");
        }

        // the moved window has to be on the target workspace's desktop before it is shown or focused
        target_monitor.assign_windows_to_workspaces();

        if should_load_workspace {
            target_monitor.load_focused_workspace(mouse_follows_focus)?;
        }
        target_monitor.update_focused_workspace(offset)?;

        // this second one is for DPI changes when the target is another monitor
        // if we don't do this the layout on the other monitor could look funny
        // until it is interacted with again
        target_monitor.update_focused_workspace(offset)?;

        if follow {
            self.focus_monitor(monitor_idx)?;
        }

        self.update_focused_workspace(self.mouse_follows_focus, true)?;

        Ok(())
    }

    #[tracing::instrument(skip(self))]
    pub fn swap_focused_monitor(&mut self, idx: usize) -> eyre::Result<()> {
        tracing::info!("swapping focused monitor");

        let focused_monitor_idx = self.focused_monitor_idx();
        let mouse_follows_focus = self.mouse_follows_focus;

        self.swap_monitor_workspaces(focused_monitor_idx, idx)?;

        self.update_focused_workspace(mouse_follows_focus, true)
    }

    #[tracing::instrument(skip(self))]
    pub fn swap_monitor_workspaces(
        &mut self,
        first_idx: usize,
        second_idx: usize,
    ) -> eyre::Result<()> {
        tracing::info!("swaping monitors");
        if first_idx == second_idx {
            return Ok(());
        }
        let mouse_follows_focus = self.mouse_follows_focus;
        let offset = self.work_area_offset;
        let first_focused_workspace = {
            let first_monitor = self
                .monitors()
                .get(first_idx)
                .ok_or_eyre("There is no monitor")?;
            first_monitor.focused_workspace_idx()
        };

        let second_focused_workspace = {
            let second_monitor = self
                .monitors()
                .get(second_idx)
                .ok_or_eyre("There is no monitor")?;
            second_monitor.focused_workspace_idx()
        };

        // Swap workspaces between the first and second monitors

        let first_workspaces = self
            .monitors_mut()
            .get_mut(first_idx)
            .ok_or_eyre("There is no monitor")?
            .remove_workspaces();

        let second_workspaces = self
            .monitors_mut()
            .get_mut(second_idx)
            .ok_or_eyre("There is no monitor")?
            .remove_workspaces();

        self.monitors_mut()
            .get_mut(first_idx)
            .ok_or_eyre("There is no monitor")?
            .workspaces_mut()
            .extend(second_workspaces);

        self.monitors_mut()
            .get_mut(second_idx)
            .ok_or_eyre("There is no monitor")?
            .workspaces_mut()
            .extend(first_workspaces);

        // Set the focused workspaces for the first and second monitors
        if let Some(first_monitor) = self.monitors_mut().get_mut(first_idx) {
            first_monitor.update_workspaces_globals(offset);
            first_monitor.focus_workspace(second_focused_workspace)?;
            first_monitor.load_focused_workspace(mouse_follows_focus)?;
        }

        if let Some(second_monitor) = self.monitors_mut().get_mut(second_idx) {
            second_monitor.update_workspaces_globals(offset);
            second_monitor.focus_workspace(first_focused_workspace)?;
            second_monitor.load_focused_workspace(mouse_follows_focus)?;
        }

        self.update_focused_workspace_by_monitor_idx(second_idx)?;
        self.update_focused_workspace_by_monitor_idx(first_idx)
    }

    pub fn update_focused_workspace_by_monitor_idx(&mut self, idx: usize) -> eyre::Result<()> {
        let offset = self.work_area_offset;

        self.monitors_mut()
            .get_mut(idx)
            .ok_or_eyre("there is no monitor")?
            .update_focused_workspace(offset)
    }

    /// Gives the focus to the window of the next (or the previous) container of the focused
    /// workspace, wrapping around at the ends, and the pointer follows it if
    /// [`WindowManager::mouse_follows_focus`] is set
    ///
    /// While the workspace has a monocle container, the monocle goes with the focus: it is turned
    /// off, the next container is focused, and it is turned on for that one.
    ///
    /// # Errors
    ///
    /// - There is no focused workspace, or it has no containers.
    /// - The backend refuses to focus the window.
    #[tracing::instrument(skip(self))]
    pub fn focus_container_in_cycle_direction(
        &mut self,
        direction: CycleDirection,
    ) -> eyre::Result<()> {
        tracing::info!("focusing container");
        // let mut maximize_next = false;
        let mut monocle_next = false;

        let mouse_follows_focus = self.mouse_follows_focus;

        if self.focused_workspace_mut()?.maximized_window.is_some() {
            // TODO: maximized window stuff
            // maximize_next = true;
            // self.unmaximize_window()?;
        }

        if self.focused_workspace_mut()?.monocle_container.is_some() {
            monocle_next = true;
            self.monocle_off()?;
        }

        let workspace = self.focused_workspace_mut()?;

        let new_idx = workspace
            .new_idx_for_cycle_direction(direction)
            .ok_or_eyre("this is not a valid direction from the current position")?;

        workspace.focus_container(new_idx);

        // if maximize_next {
        //     self.toggle_maximize()?;
        // } else
        if monocle_next {
            self.toggle_monocle()?;
        } else {
            self.focused_window_mut()?.focus(mouse_follows_focus)?;
        }

        Ok(())
    }

    /// Swaps the focused container with the next (or the previous) one of the focused
    /// workspace, wrapping around at the ends, and tiles the workspace again
    ///
    /// The container keeps the focus, and the pointer follows it if
    /// [`WindowManager::mouse_follows_focus`] is set.
    ///
    /// # Errors
    ///
    /// - There is no focused workspace, or it has no containers.
    /// - The window in front is in monocle mode, which has no neighbours to swap with.
    /// - The workspace cannot be tiled, or the backend refuses to focus the window.
    #[tracing::instrument(skip(self))]
    pub fn move_container_in_cycle_direction(
        &mut self,
        direction: CycleDirection,
    ) -> eyre::Result<()> {
        let workspace = self.focused_workspace_mut()?;
        if workspace.is_focused_window_monocle_or_maximized()? {
            eyre::bail!("ignoring command while active window is in monocle mode or maximized");
        }

        tracing::info!("moving container");

        let current_idx = workspace.focused_container_idx();
        let new_idx = workspace
            .new_idx_for_cycle_direction(direction)
            .ok_or_eyre("this is not a valid direction from the current position")?;

        workspace.swap_containers(current_idx, new_idx);
        workspace.focus_container(new_idx);
        self.update_focused_workspace(self.mouse_follows_focus, true)
    }

    /// Gives the focus to the next (or the previous) floating window of the focused workspace,
    /// wrapping around at the ends, and the pointer follows it if
    /// [`WindowManager::mouse_follows_focus`] is set
    ///
    /// When the focused window is not one of the floating windows, the first is given the focus.
    /// With no more than one floating window nothing changes.
    ///
    /// # Errors
    ///
    /// - There is no focused workspace, or the compositor reports no focused window.
    /// - The backend refuses to focus the window.
    #[tracing::instrument(skip(self))]
    pub fn focus_floating_window_in_cycle_direction(
        &mut self,
        direction: CycleDirection,
    ) -> eyre::Result<()> {
        let mouse_follows_focus = self.mouse_follows_focus;
        let focused_workspace = self.focused_workspace()?;

        let mut target_idx = None;
        let len = focused_workspace.floating_windows().len();

        if len > 1 {
            let focused_window_id = backend()
                .foreground_window()
                .ok_or_eyre("no foreground window")?;
            for (idx, window) in focused_workspace.floating_windows().iter().enumerate() {
                if window.id == focused_window_id {
                    match direction {
                        CycleDirection::Previous => {
                            if idx == 0 {
                                target_idx = Some(len - 1)
                            } else {
                                target_idx = Some(idx - 1)
                            }
                        }
                        CycleDirection::Next => {
                            if idx == len - 1 {
                                target_idx = Some(0)
                            } else {
                                target_idx = Some(idx + 1)
                            }
                        }
                    }
                }
            }

            if target_idx.is_none() {
                target_idx = Some(0);
            }
        }

        if let Some(idx) = target_idx
            && let Some(window) = focused_workspace.floating_windows().get(idx)
        {
            window.focus(mouse_follows_focus)?;
        }

        Ok(())
    }

    /// Moves the front window of the focused stack one place along the stack, which wraps around
    /// at both ends, by swapping it with the window there. It stays in front and keeps the focus:
    /// only the order of the stack changes.
    ///
    /// # Errors
    ///
    /// - The focused container has no window, or only one.
    /// - There is no focused workspace or container, or the window cannot be focused.
    /// - The workspace cannot be tiled again.
    #[tracing::instrument(skip(self))]
    pub fn cycle_container_window_index_in_direction(
        &mut self,
        direction: CycleDirection,
    ) -> eyre::Result<()> {
        tracing::info!("cycling container window index");

        let mouse_follows_focus = self.mouse_follows_focus;

        let container =
            if let Some(container) = &mut self.focused_workspace_mut()?.monocle_container {
                container
            } else {
                self.focused_container_mut()?
            };

        let len = NonZeroUsize::new(container.windows().len())
            .ok_or_eyre("there must be at least one window in a container")?;

        if len.get() == 1 {
            eyre::bail!("there is only one window in this container");
        }

        let current_idx = container.focused_window_idx();
        let next_idx = direction.next_idx(current_idx, len);
        container.windows_mut().swap(current_idx, next_idx);

        container.focus_window(next_idx);
        container.load_focused_window()?;

        if let Some(window) = container.focused_window() {
            window.focus(mouse_follows_focus)?;
        }

        self.update_focused_workspace(mouse_follows_focus, true)
    }

    /// Gives the focus to the window at `idx` in the focused container, and brings it to the front
    /// when the container is a stack
    ///
    /// # Errors
    ///
    /// - The focused container has only one window and `idx` is not 0.
    /// - The focused container has no window at `idx`.
    /// - There is no focused container, or the workspace cannot be tiled again.
    #[tracing::instrument(skip(self))]
    pub fn focus_container_window(&mut self, idx: usize) -> eyre::Result<()> {
        tracing::info!("focusing container window at index {idx}");

        let mouse_follows_focus = self.mouse_follows_focus;

        let container =
            if let Some(container) = &mut self.focused_workspace_mut()?.monocle_container {
                container
            } else {
                self.focused_container_mut()?
            };

        let len = NonZeroUsize::new(container.windows().len())
            .ok_or_eyre("there must be at least one window in a container")?;

        if len.get() == 1 && idx != 0 {
            eyre::bail!("there is only one window in this container");
        }

        if container.windows().get(idx).is_none() {
            eyre::bail!("there is no window in this container at index {idx}");
        }

        container.focus_window(idx);
        container.load_focused_window()?;

        if let Some(window) = container.focused_window() {
            window.focus(mouse_follows_focus)?;
        }

        self.update_focused_workspace(mouse_follows_focus, true)
    }

    /// Puts every window of the focused workspace in one container, in the order they had, with the
    /// window that had the focus in front and the others hidden
    ///
    /// # Errors
    ///
    /// There is no focused workspace, or it cannot be tiled again.
    #[tracing::instrument(skip(self))]
    pub fn stack_all(&mut self) -> eyre::Result<()> {
        tracing::info!("stacking all windows on workspace");

        let workspace = self.focused_workspace_mut()?;

        let mut focused_hwnd = None;
        if let Some(container) = workspace.focused_container()
            && let Some(window) = container.focused_window()
        {
            focused_hwnd = Some(window.id);
        }

        let workspace_hwnds =
            workspace
                .containers()
                .iter()
                .fold(VecDeque::new(), |mut hwnds, c| {
                    hwnds.extend(c.windows().clone());
                    hwnds
                });

        let mut container = Container::default();
        *container.windows_mut() = workspace_hwnds;
        *workspace.containers_mut() = VecDeque::from([container]);
        workspace.focus_container(0);

        if let Some(hwnd) = focused_hwnd
            && let Some(c) = workspace.focused_container_mut()
            && let Some(w_idx_to_focus) = c.idx_for_window(hwnd)
        {
            c.focus_window(w_idx_to_focus);
            c.load_focused_window()?;
        }
        self.update_focused_workspace(self.mouse_follows_focus, true)
    }

    /// Takes every window out of the focused container, each into a container of its own, and
    /// gives the focus back to the window that had it. With `update_workspace` the workspace is
    /// tiled again.
    ///
    /// # Errors
    ///
    /// There is no focused workspace, or, with `update_workspace`, it cannot be tiled again.
    #[tracing::instrument(skip(self))]
    pub fn unstack_all(&mut self, update_workspace: bool) -> eyre::Result<()> {
        tracing::info!("unstacking all windows in container");

        let workspace = self.focused_workspace_mut()?;

        let mut focused_window_id = None;
        if let Some(container) = workspace.focused_container()
            && let Some(window) = container.focused_window()
        {
            focused_window_id = Some(window.id);
        }

        let initial_focused_container_index = workspace.focused_container_idx();
        let mut focused_container = workspace.focused_container().cloned();

        while let Some(focused) = &focused_container {
            if focused.windows().len() > 1 {
                workspace.new_container_for_focused_window()?;
                workspace.focus_container(initial_focused_container_index);
                focused_container = workspace.focused_container().cloned();
            } else {
                focused_container = None;
            }
        }

        if let Some(window_id) = focused_window_id {
            workspace.focus_container_by_window(window_id)?;
        }

        if update_workspace {
            self.update_focused_workspace(self.mouse_follows_focus, true)?;
        }

        Ok(())
    }

    /// Sends each window that a workspace rule names to the workspace the rule gives, and tiles the
    /// focused workspace again if a window left it
    ///
    /// The rules are in `WORKSPACE_MATCHING_RULES`; only the window at the top of each container
    /// and the floating windows are looked at, and a window that is on its workspace already is
    /// left alone. A rule that is `initial_only` moves a window once: the window is remembered in
    /// [`WindowManager::already_moved_window_handles`] until it closes. A floating window is
    /// taken to the monitor of its workspace. A workspace that does not exist yet is made. A window
    /// that leaves the focused workspace is hidden, so it stays out of sight until its new
    /// workspace is shown. The windows that moved are put on the desktops of their workspaces,
    /// which is also how a window that leaves a workspace that is out of view comes to the desktop
    /// of its new one.
    ///
    /// # Errors
    ///
    /// There is no focused monitor, a rule names a monitor that is not there, a window cannot be
    /// read, or the focused workspace cannot be tiled again.
    #[tracing::instrument(skip(self), level = "debug")]
    pub fn enforce_workspace_rules(&mut self) -> eyre::Result<()> {
        let mut to_move = vec![];

        let focused_monitor_idx = self.focused_monitor_idx();
        let focused_workspace_idx = self
            .monitors()
            .get(focused_monitor_idx)
            .ok_or_eyre("there is no monitor with that index")?
            .focused_workspace_idx();

        // scope mutex locks to avoid deadlock if should_update_focused_workspace evaluates to true
        // at the end of this function
        {
            let workspace_matching_rules = WORKSPACE_MATCHING_RULES.lock();
            let regex_identifiers = REGEX_IDENTIFIERS.lock();
            // Go through all the monitors and workspaces
            for (i, monitor) in self.monitors().iter().enumerate() {
                for (j, workspace) in monitor.workspaces().iter().enumerate() {
                    // And all the visible windows (at the top of a container)
                    for window in workspace.visible_windows().into_iter().flatten() {
                        if let (Some(exe_name), Some(title), Some(class), Some(path)) =
                            (window.exe(), window.title(), window.class(), window.path())
                        {
                            for rule in &*workspace_matching_rules {
                                let matched = match &rule.matching_rule {
                                    MatchingRule::Simple(r) => should_act_individual(
                                        &title,
                                        &exe_name,
                                        &[&class],
                                        &path.to_string_lossy(),
                                        r,
                                        &regex_identifiers,
                                    ),
                                    MatchingRule::Composite(r) => {
                                        let mut composite_results = vec![];
                                        for identifier in r {
                                            composite_results.push(should_act_individual(
                                                &title,
                                                &exe_name,
                                                &[&class],
                                                &path.to_string_lossy(),
                                                identifier,
                                                &regex_identifiers,
                                            ));
                                        }

                                        composite_results.iter().all(|&x| x)
                                    }
                                };

                                if matched {
                                    let floating = workspace.floating_windows().contains(window);

                                    let mut already_moved_window_handles =
                                        self.already_moved_window_handles.lock();

                                    if rule.initial_only {
                                        if !already_moved_window_handles.contains(&window.id) {
                                            already_moved_window_handles.insert(window.id);

                                            self.add_window_handle_to_move_based_on_workspace_rule(
                                                &window
                                                    .title()
                                                    .ok_or_eyre("could not read window title")?,
                                                window.id,
                                                i,
                                                j,
                                                rule.monitor_index,
                                                rule.workspace_index,
                                                floating,
                                                &mut to_move,
                                            );
                                        }
                                    } else {
                                        self.add_window_handle_to_move_based_on_workspace_rule(
                                            &window
                                                .title()
                                                .ok_or_eyre("could not read window title")?,
                                            window.id,
                                            i,
                                            j,
                                            rule.monitor_index,
                                            rule.workspace_index,
                                            floating,
                                            &mut to_move,
                                        );
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }

        // Only retain operations where the target is not the current workspace
        to_move.retain(|op| !op.is_target(focused_monitor_idx, focused_workspace_idx));
        // Only retain operations where the rule has not already been enforced
        to_move.retain(|op| !op.is_enforced());

        let mut should_update_focused_workspace = false;
        let mut removed_windows = HashMap::new();

        // Parse the operation and remove any windows that are not placed according to their rules
        for op in &to_move {
            let target_area = self
                .monitors_mut()
                .get_mut(op.target_monitor_idx)
                .ok_or_eyre("there is no monitor with that index")?
                .work_area_size;

            let origin_monitor = self
                .monitors_mut()
                .get_mut(op.origin_monitor_idx)
                .ok_or_eyre("there is no monitor with that index")?;

            let origin_area = origin_monitor.work_area_size;

            let origin_workspace = origin_monitor
                .workspaces_mut()
                .get_mut(op.origin_workspace_idx)
                .ok_or_eyre("there is no workspace with that index")?;

            let window = origin_workspace.remove_window(op.window_id)?;

            // If it is a floating window move it to the target area
            if op.floating {
                window.move_to_area(&origin_area, &target_area)?;
            }

            // Hide the window we are about to remove if it is on the currently focused workspace
            if op.is_origin(focused_monitor_idx, focused_workspace_idx) {
                window.hide();
                should_update_focused_workspace = true;
            }

            removed_windows.insert(window.id, window);
        }

        // Parse the operation again and associate those removed windows with the workspace that
        // their rules have defined for them
        for op in &to_move {
            let window = removed_windows
                .get_mut(&op.window_id)
                .ok_or_eyre("there is no window")?;

            let target_monitor = self
                .monitors_mut()
                .get_mut(op.target_monitor_idx)
                .ok_or_eyre("there is no monitor with that index")?;

            // The very first time this fn is called, the workspace might not even exist yet
            if target_monitor
                .workspaces()
                .get(op.target_workspace_idx)
                .is_none()
            {
                // If it doesn't, let's make sure it does for the next step
                target_monitor.ensure_workspace_count(op.target_workspace_idx + 1);
            }

            let target_workspace = target_monitor
                .workspaces_mut()
                .get_mut(op.target_workspace_idx)
                .ok_or_eyre("there is no workspace with that index")?;

            if op.floating {
                target_workspace
                    .floating_windows_mut()
                    // TODO: not sure about this clone
                    .push_back(*window);
            } else {
                //TODO(alex-ds13): should this take into account the target workspace
                //`window_container_behaviour`?
                //In the case above a floating window should always be moved as floating,
                //because it was set as so either manually by the user or by a
                //`floating_applications` rule so it should stay that way. But a tiled window
                //when moving to another workspace by a `workspace_rule` should honor that
                //workspace `window_container_behaviour` in my opinion! Maybe this should be done
                //on the `new_container_for_window` function instead.
                // TODO: not sure about this clone
                target_workspace.new_container_for_window(window)?;
            }
        }

        // The compositor learns where the windows that moved belong: each is put on the desktop of
        // the workspace it is on now (a hidden window only has the desktop it comes back to
        // changed), which also takes a window that was not hidden out of view when that workspace
        // is not the one in front
        let mut assigned = vec![];
        for op in &to_move {
            if !assigned.contains(&op.target_monitor_idx)
                && let Some(monitor) = self.monitors().get(op.target_monitor_idx)
            {
                monitor.assign_windows_to_workspaces();
                assigned.push(op.target_monitor_idx);
            }
        }

        // Only re-tile the focused workspace if we need to
        if should_update_focused_workspace {
            self.update_focused_workspace(false, false)?;
        }

        Ok(())
    }

    /// Adds to `to_move` the operation that takes the window from where it is to the workspace a
    /// rule gives it
    #[allow(clippy::too_many_arguments)]
    #[tracing::instrument(skip(self), level = "debug")]
    fn add_window_handle_to_move_based_on_workspace_rule(
        &self,
        window_title: &String,
        window_id: u32,
        origin_monitor_idx: usize,
        origin_workspace_idx: usize,
        target_monitor_idx: usize,
        target_workspace_idx: usize,
        floating: bool,
        to_move: &mut Vec<EnforceWorkspaceRuleOp>,
    ) {
        tracing::trace!(
            "{} should be on monitor {}, workspace {}",
            window_title,
            target_monitor_idx,
            target_workspace_idx
        );

        // Create an operation outline and save it for later in the fn
        to_move.push(EnforceWorkspaceRuleOp {
            window_id,
            origin_monitor_idx,
            origin_workspace_idx,
            target_monitor_idx,
            target_workspace_idx,
            floating,
        });
    }

    /// Applies the `komorebi.json` at `pathbuf` to the window manager and tiles it again
    /// ([`StaticConfig::reload`])
    ///
    /// # Errors
    ///
    /// The file cannot be read, or is not a configuration, or has a rule with a regular expression
    /// that is not valid, or the workspaces cannot be tiled again.
    #[tracing::instrument(skip(self))]
    pub fn reload_static_configuration(&mut self, pathbuf: &PathBuf) -> eyre::Result<()> {
        tracing::info!("reloading static configuration");
        StaticConfig::reload(pathbuf, self)
    }

    /// With [`OperationBehaviour::NoOp`], refuses a message while the window that has the focus
    /// in the compositor is not in a container of the focused workspace: a window komorebi does
    /// not manage, one the user unmanaged, or a floating window
    ///
    /// With [`OperationBehaviour::Op`], the default, every message is carried out.
    ///
    /// # Errors
    ///
    /// With [`OperationBehaviour::NoOp`]:
    ///
    /// - There is no focused workspace, or the compositor reports no focused window.
    /// - The focused window is not in a container of the focused workspace.
    #[tracing::instrument(skip(self))]
    pub fn handle_unmanaged_window_behaviour(&self) -> eyre::Result<()> {
        if matches!(
            self.unmanaged_window_operation_behaviour,
            OperationBehaviour::NoOp
        ) {
            let workspace = self.focused_workspace()?;
            let focused_hwnd = backend()
                .foreground_window()
                .ok_or_eyre("there is no foreground window")?;
            if !workspace.contains_managed_window(focused_hwnd) {
                bail!("ignoring commands while active window is not managed by komorebi");
            }
        }

        Ok(())
    }

    /// Make sure the compositor has a virtual desktop for every workspace any monitor has, with
    /// the workspaces' names where it has to create them (and "Workspace 2" and so on for a
    /// workspace without one)
    pub fn sync_workspace_desktops(&self) {
        let count = self
            .monitors()
            .iter()
            .map(|m| m.workspaces().len())
            .max()
            .unwrap_or(1)
            .max(1);

        let names: Vec<String> = (0..count)
            .map(|i| {
                self.monitors()
                    .iter()
                    .filter_map(|m| m.workspaces().get(i).and_then(|w| w.name.clone()))
                    .find(|name| !name.is_empty())
                    .unwrap_or_else(|| format!("Workspace {}", i + 1))
            })
            .collect();

        if let Err(error) = backend().ensure_workspaces(&names) {
            tracing::warn!("could not make sure there are desktops for the workspaces: {error}");
        }
    }

    /// A monitor now shows workspace `idx`, which komorebi did not ask for: a pager, a shortcut, an
    /// app raising a window of another desktop. The compositor has already shown the windows and
    /// chosen what to focus; komorebi follows, and tiles what came into view.
    pub fn adopt_workspace_change(&mut self, monitor_id: u32, idx: usize) -> eyre::Result<()> {
        let offset = self.work_area_offset;

        let Some(monitor) = self.monitors_mut().iter_mut().find(|m| m.id == monitor_id) else {
            return Ok(());
        };

        if monitor.focused_workspace_idx() == idx {
            // komorebi's own switch, coming back
            return Ok(());
        }

        tracing::info!("the compositor now shows workspace {idx} on monitor {monitor_id}");
        monitor.focus_workspace(idx)?;
        monitor.update_focused_workspace(offset)?;

        // komorebi's own switches set the wallpaper as the workspace is restored; somebody else's
        // (a pager, a shortcut) comes here instead
        let monitor_wp = monitor.wallpaper.clone();
        if let Some(workspace) = monitor.workspaces().get(idx)
            && (workspace.wallpaper.is_some() || monitor_wp.is_some())
            && let Err(error) = workspace.apply_wallpaper(monitor_id, &monitor_wp)
        {
            tracing::error!("failed to apply wallpaper: {error}");
        }

        self.update_known_window_ids();

        Ok(())
    }

    pub fn update_known_window_ids(&mut self) {
        tracing::trace!("updating list of known window_ids");
        let mut known_window_ids = HashMap::new();
        for (m_idx, monitor) in self.monitors().iter().enumerate() {
            for (w_idx, workspace) in monitor.workspaces().iter().enumerate() {
                for container in workspace.containers() {
                    for window in container.windows() {
                        known_window_ids.insert(window.id, (m_idx, w_idx));
                    }
                }

                for window in workspace.floating_windows() {
                    known_window_ids.insert(window.id, (m_idx, w_idx));
                }

                if let Some(window) = &workspace.maximized_window {
                    known_window_ids.insert(window.id, (m_idx, w_idx));
                }

                if let Some(container) = &workspace.monocle_container {
                    for window in container.windows() {
                        known_window_ids.insert(window.id, (m_idx, w_idx));
                    }
                }
            }
        }

        if self.known_window_ids != known_window_ids {
            // Store new window_ids
            self.known_window_ids = known_window_ids;
        }
    }

    /// Returns the [`WindowManagementBehaviour`] that applies to a window that opens on the
    /// workspace at `workspace_idx` of the monitor at `monitor_idx`: the workspace's own
    /// `window_container_behaviour` and `float_override` where it sets them, and the window
    /// manager's where it does not
    ///
    /// A workspace on its floating layer ([`WorkspaceLayer::Floating`]) makes new windows float
    /// (`floating_layer_override`) when its floating layer behaviour says to float: the
    /// workspace's own, else its monitor's, else the window manager's.
    ///
    /// An empty workspace has no container to append to, so `Append` is `Create` for it. A
    /// workspace or a monitor that does not exist gets `Create`.
    pub fn window_management_behaviour(
        &self,
        monitor_idx: usize,
        workspace_idx: usize,
    ) -> WindowManagementBehaviour {
        if let Some(monitor) = self.monitors().get(monitor_idx)
            && let Some(workspace) = monitor.workspaces().get(workspace_idx)
        {
            let current_behaviour = if let Some(behaviour) = workspace.window_container_behaviour {
                if workspace.containers().is_empty()
                    && matches!(behaviour, WindowContainerBehaviour::Append)
                {
                    // You can't append to an empty workspace
                    WindowContainerBehaviour::Create
                } else {
                    behaviour
                }
            } else if workspace.containers().is_empty()
                && matches!(
                    self.window_management_behaviour.current_behaviour,
                    WindowContainerBehaviour::Append
                )
            {
                // You can't append to an empty workspace
                WindowContainerBehaviour::Create
            } else {
                self.window_management_behaviour.current_behaviour
            };

            let float_override = if let Some(float_override) = workspace.float_override {
                float_override
            } else {
                self.window_management_behaviour.float_override
            };

            let floating_layer_behaviour =
                if let Some(behaviour) = workspace.floating_layer_behaviour {
                    behaviour
                } else {
                    monitor
                        .floating_layer_behaviour
                        .unwrap_or(self.window_management_behaviour.floating_layer_behaviour)
                };

            // If the workspace layer is `Floating` and the floating layer behaviour should
            // float then change floating_layer_override to true so that new windows spawn
            // as floating
            let floating_layer_override = matches!(workspace.layer, WorkspaceLayer::Floating)
                && floating_layer_behaviour.should_float();

            return WindowManagementBehaviour {
                current_behaviour,
                float_override,
                floating_layer_override,
                floating_layer_behaviour,
                toggle_float_placement: self.window_management_behaviour.toggle_float_placement,
                floating_layer_placement: self.window_management_behaviour.floating_layer_placement,
                float_override_placement: self.window_management_behaviour.float_override_placement,
                float_rule_placement: self.window_management_behaviour.float_rule_placement,
            };
        }

        WindowManagementBehaviour {
            current_behaviour: WindowContainerBehaviour::Create,
            float_override: self.window_management_behaviour.float_override,
            floating_layer_override: self.window_management_behaviour.floating_layer_override,
            floating_layer_behaviour: self.window_management_behaviour.floating_layer_behaviour,
            toggle_float_placement: self.window_management_behaviour.toggle_float_placement,
            floating_layer_placement: self.window_management_behaviour.floating_layer_placement,
            float_override_placement: self.window_management_behaviour.float_override_placement,
            float_rule_placement: self.window_management_behaviour.float_rule_placement,
        }
    }

    /// Gives the workspace at `workspace_idx` on the monitor at `monitor_idx` a workspace padding
    /// of `size` pixels, around its tiles, and tiles the focused workspace again
    ///
    /// # Errors
    ///
    /// There is no monitor or no workspace at the index, or the focused workspace cannot be tiled
    /// again.
    #[tracing::instrument(skip(self))]
    pub fn set_workspace_padding(
        &mut self,
        monitor_idx: usize,
        workspace_idx: usize,
        size: i32,
    ) -> eyre::Result<()> {
        tracing::info!("setting workspace padding");

        let monitor = self
            .monitors_mut()
            .get_mut(monitor_idx)
            .ok_or_eyre("there is no monitor")?;

        let workspace = monitor
            .workspaces_mut()
            .get_mut(workspace_idx)
            .ok_or_eyre("there is no monitor")?;

        workspace.workspace_padding = Option::from(size);

        self.update_focused_workspace(false, false)
    }

    #[tracing::instrument(skip(self))]
    pub fn set_workspace_name(
        &mut self,
        monitor_idx: usize,
        workspace_idx: usize,
        name: String,
    ) -> eyre::Result<()> {
        tracing::info!("setting workspace name");

        let monitor = self
            .monitors_mut()
            .get_mut(monitor_idx)
            .ok_or_eyre("there is no monitor")?;

        let workspace = monitor
            .workspaces_mut()
            .get_mut(workspace_idx)
            .ok_or_eyre("there is no monitor")?;

        workspace.name = Option::from(name.clone());
        // monitor.workspace_names.insert(workspace_idx, name);

        Ok(())
    }

    /// Gives the workspace at `workspace_idx` on the monitor at `monitor_idx` a container padding
    /// of `size` pixels, around each tile, and tiles the focused workspace again
    ///
    /// # Errors
    ///
    /// There is no monitor or no workspace at the index, or the focused workspace cannot be tiled
    /// again.
    #[tracing::instrument(skip(self))]
    pub fn set_container_padding(
        &mut self,
        monitor_idx: usize,
        workspace_idx: usize,
        size: i32,
    ) -> eyre::Result<()> {
        tracing::info!("setting container padding");

        let monitor = self
            .monitors_mut()
            .get_mut(monitor_idx)
            .ok_or_eyre("there is no monitor")?;

        let workspace = monitor
            .workspaces_mut()
            .get_mut(workspace_idx)
            .ok_or_eyre("there is no monitor")?;

        workspace.container_padding = Option::from(size);

        self.update_focused_workspace(false, false)
    }

    #[tracing::instrument(skip(self))]
    pub fn set_workspace_tiling(
        &mut self,
        monitor_idx: usize,
        workspace_idx: usize,
        tile: bool,
    ) -> eyre::Result<()> {
        let monitor = self
            .monitors_mut()
            .get_mut(monitor_idx)
            .ok_or_eyre("there is no monitor")?;

        let workspace = monitor
            .workspaces_mut()
            .get_mut(workspace_idx)
            .ok_or_eyre("there is no monitor")?;

        workspace.tile = tile;

        self.update_focused_workspace(false, false)
    }

    /// Makes the workspace change to `layout` once it has `at_container_count` containers or more
    /// ([`Workspace::layout_rules`]), replacing the rule it had for that count, and tiles it again
    ///
    /// # Errors
    ///
    /// There is no monitor or no workspace with those indices, or the workspaces cannot be tiled
    /// again.
    #[tracing::instrument(skip(self))]
    pub fn add_workspace_layout_default_rule(
        &mut self,
        monitor_idx: usize,
        workspace_idx: usize,
        at_container_count: usize,
        layout: DefaultLayout,
    ) -> eyre::Result<()> {
        tracing::info!("setting workspace layout");

        let focused_monitor_idx = self.focused_monitor_idx();

        let monitor = self
            .monitors_mut()
            .get_mut(monitor_idx)
            .ok_or_eyre("there is no monitor")?;

        let focused_workspace_idx = monitor.focused_workspace_idx();

        let workspace = monitor
            .workspaces_mut()
            .get_mut(workspace_idx)
            .ok_or_eyre("there is no monitor")?;

        let rules: &mut Vec<(usize, Layout)> = &mut workspace.layout_rules;
        rules.retain(|pair| pair.0 != at_container_count);
        rules.push((at_container_count, Layout::Default(layout)));
        rules.sort_by_key(|a| a.0);

        // If this is the focused workspace on a non-focused screen, let's update it
        if focused_monitor_idx != monitor_idx && focused_workspace_idx == workspace_idx {
            workspace.update()?;
            Ok(())
        } else {
            Ok(self.update_focused_workspace(false, false)?)
        }
    }

    /// Takes the layout rules of the workspace away, which leaves it with the layout it has now,
    /// and tiles it again
    ///
    /// # Errors
    ///
    /// There is no monitor or no workspace with those indices, or the workspaces cannot be tiled
    /// again.
    #[tracing::instrument(skip(self))]
    pub fn clear_workspace_layout_rules(
        &mut self,
        monitor_idx: usize,
        workspace_idx: usize,
    ) -> eyre::Result<()> {
        tracing::info!("clearing workspace layout rules");

        let focused_monitor_idx = self.focused_monitor_idx();

        let monitor = self
            .monitors_mut()
            .get_mut(monitor_idx)
            .ok_or_eyre("there is no monitor")?;

        let focused_workspace_idx = monitor.focused_workspace_idx();

        let workspace = monitor
            .workspaces_mut()
            .get_mut(workspace_idx)
            .ok_or_eyre("there is no monitor")?;

        let rules: &mut Vec<(usize, Layout)> = &mut workspace.layout_rules;
        rules.clear();

        // If this is the focused workspace on a non-focused screen, let's update it
        if focused_monitor_idx != monitor_idx && focused_workspace_idx == workspace_idx {
            workspace.update()?;
            Ok(())
        } else {
            Ok(self.update_focused_workspace(false, false)?)
        }
    }

    /// Makes the workspace padding of the focused workspace `adjustment` pixels larger or smaller,
    /// and tiles it again
    ///
    /// # Errors
    ///
    /// There is no focused workspace, it has no workspace padding of its own (set with
    /// [`WindowManager::set_workspace_padding`]), or it cannot be tiled again.
    #[tracing::instrument(skip(self))]
    pub fn adjust_workspace_padding(
        &mut self,
        sizing: Sizing,
        adjustment: i32,
    ) -> eyre::Result<()> {
        tracing::info!("adjusting workspace padding");

        let workspace = self.focused_workspace_mut()?;

        let padding = workspace
            .workspace_padding
            .ok_or_eyre("there is no workspace padding")?;

        workspace.workspace_padding = Option::from(sizing.adjust_by(padding, adjustment));

        self.update_focused_workspace(false, false)
    }

    /// Makes the container padding of the focused workspace `adjustment` pixels larger or smaller,
    /// and tiles it again
    ///
    /// # Errors
    ///
    /// There is no focused workspace, it has no container padding of its own (set with
    /// [`WindowManager::set_container_padding`]), or it cannot be tiled again.
    #[tracing::instrument(skip(self))]
    pub fn adjust_container_padding(
        &mut self,
        sizing: Sizing,
        adjustment: i32,
    ) -> eyre::Result<()> {
        tracing::info!("adjusting container padding");

        let workspace = self.focused_workspace_mut()?;

        let padding = workspace
            .container_padding
            .ok_or_eyre("there is no container padding")?;

        workspace.container_padding = Option::from(sizing.adjust_by(padding, adjustment));

        self.update_focused_workspace(false, false)
    }

    #[tracing::instrument(skip(self))]
    pub fn ensure_workspaces_for_monitor(
        &mut self,
        monitor_idx: usize,
        workspace_count: usize,
    ) -> eyre::Result<()> {
        tracing::info!("ensuring workspace count");

        let monitor = self
            .monitors_mut()
            .get_mut(monitor_idx)
            .ok_or_eyre("there is no monitor")?;

        monitor.ensure_workspace_count(workspace_count);

        Ok(())
    }

    #[tracing::instrument(skip(self))]
    pub fn ensure_named_workspaces_for_monitor(
        &mut self,
        monitor_idx: usize,
        names: &Vec<String>,
    ) -> eyre::Result<()> {
        tracing::info!("ensuring workspace count");

        let monitor = self
            .monitors_mut()
            .get_mut(monitor_idx)
            .ok_or_eyre("there is no monitor")?;

        monitor.ensure_workspace_count(names.len());

        for (workspace_idx, name) in names.iter().enumerate() {
            if let Some(workspace) = monitor.workspaces_mut().get_mut(workspace_idx) {
                workspace.name = Option::from(name.clone());
            }
        }

        Ok(())
    }

    #[tracing::instrument(skip(self))]
    pub fn move_workspace_to_monitor(&mut self, idx: usize) -> eyre::Result<()> {
        tracing::info!("moving workspace");
        let mouse_follows_focus = self.mouse_follows_focus;
        let offset = self.work_area_offset;
        let workspace = self
            .remove_focused_workspace()
            .ok_or_eyre("there is no workspace")?;

        {
            let target_monitor: &mut Monitor = self
                .monitors_mut()
                .get_mut(idx)
                .ok_or_eyre("there is no monitor")?;

            target_monitor.workspaces_mut().push_back(workspace);
            target_monitor.update_workspaces_globals(offset);
            target_monitor.focus_workspace(target_monitor.workspaces().len().saturating_sub(1))?;
            target_monitor.load_focused_workspace(mouse_follows_focus)?;
        }

        self.focus_monitor(idx)?;
        self.update_focused_workspace(mouse_follows_focus, true)
    }

    pub fn remove_focused_workspace(&mut self) -> Option<Workspace> {
        let focused_monitor: &mut Monitor = self.focused_monitor_mut()?;
        let focused_workspace_idx = focused_monitor.focused_workspace_idx();
        let workspace = focused_monitor.remove_workspace_by_idx(focused_workspace_idx);
        if let Err(error) = focused_monitor.focus_workspace(focused_workspace_idx.saturating_sub(1))
        {
            tracing::error!(
                "Error focusing previous workspace while removing the focused workspace: {}",
                error
            );
        }
        workspace
    }

    /// Asks the window manager to manage the window that has the focus in the compositor again
    /// after [`WindowManager::unmanage_focused_window`], by raising a
    /// [`WindowManagerEvent::Manage`] for it, which is handled like a window that opens
    ///
    /// # Errors
    ///
    /// The compositor reports no focused window.
    #[tracing::instrument(skip(self))]
    pub fn manage_focused_window(&mut self) -> eyre::Result<()> {
        let window_id = backend()
            .foreground_window()
            .ok_or_eyre("there is no foreground window")?;

        UNMANAGED_WINDOW_IDS.lock().retain(|id| *id != window_id);

        window_manager_event_listener::send_notification(WindowManagerEvent::Manage(Window::from(
            window_id,
        )));

        Ok(())
    }

    /// Asks the window manager to stop managing the window that has the focus in the compositor,
    /// by raising a [`WindowManagerEvent::Unmanage`] for it: the window leaves the layout, and
    /// komorebi leaves it alone until [`WindowManager::manage_focused_window`] or until it closes
    ///
    /// # Errors
    ///
    /// The compositor reports no focused window.
    #[tracing::instrument(skip(self))]
    pub fn unmanage_focused_window(&mut self) -> eyre::Result<()> {
        let window_id = backend()
            .foreground_window()
            .ok_or_eyre("there is no foreground window")?;

        window_manager_event_listener::send_notification(WindowManagerEvent::Unmanage(
            Window::from(window_id),
        ));

        Ok(())
    }

    #[tracing::instrument(skip(self))]
    pub fn transfer_window(
        &mut self,
        origin: (usize, usize, u32),
        target: (usize, usize, usize),
    ) -> eyre::Result<()> {
        let (origin_monitor_idx, origin_workspace_idx, window_id) = origin;
        let (target_monitor_idx, target_workspace_idx, target_container_idx) = target;

        let origin_workspace = self
            .monitors_mut()
            .get_mut(origin_monitor_idx)
            .ok_or_eyre("cannot get monitor idx")?
            .workspaces_mut()
            .get_mut(origin_workspace_idx)
            .ok_or_eyre("cannot get workspace idx")?;

        let origin_container_idx = origin_workspace
            .container_for_window(window_id)
            .and_then(|c| origin_workspace.containers().iter().position(|cc| cc == c));

        if let Some(origin_container_idx) = origin_container_idx {
            // Moving normal container window
            self.transfer_container(
                (
                    origin_monitor_idx,
                    origin_workspace_idx,
                    origin_container_idx,
                ),
                (
                    target_monitor_idx,
                    target_workspace_idx,
                    target_container_idx,
                ),
            )?;
        } else if let Some(idx) = origin_workspace
            .floating_windows()
            .iter()
            .position(|w| w.id == window_id)
        {
            // Moving floating window
            // There is no need to physically move the floating window between areas with
            // `move_to_area` because the user already did that, so we only need to transfer the
            // window to the target `floating_windows`
            if let Some(floating_window) = origin_workspace.floating_windows_mut().remove(idx) {
                let target_workspace = self
                    .monitors_mut()
                    .get_mut(target_monitor_idx)
                    .ok_or_eyre("there is no monitor at this idx")?
                    .focused_workspace_mut()
                    .ok_or_eyre("there is no focused workspace for this monitor")?;

                target_workspace
                    .floating_windows_mut()
                    .push_back(floating_window);
            }
        } else if origin_workspace
            .monocle_container
            .as_ref()
            .and_then(|monocle| monocle.focused_window().map(|w| w.id == window_id))
            .unwrap_or_default()
        {
            // Moving monocle container
            if let Some(monocle_idx) = origin_workspace.monocle_container_restore_idx {
                let origin_workspace = self
                    .monitors_mut()
                    .get_mut(origin_monitor_idx)
                    .ok_or_eyre("there is no monitor at this idx")?
                    .workspaces_mut()
                    .get_mut(origin_workspace_idx)
                    .ok_or_eyre("there is no workspace for this monitor")?;

                for container in origin_workspace.containers_mut() {
                    container.restore()?;
                }

                origin_workspace.reintegrate_monocle_container()?;

                self.transfer_container(
                    (origin_monitor_idx, origin_workspace_idx, monocle_idx),
                    (
                        target_monitor_idx,
                        target_workspace_idx,
                        target_container_idx,
                    ),
                )?;
            }
        } else if origin_workspace
            .maximized_window
            .as_ref()
            .map(|max| max.id == window_id)
            .unwrap_or_default()
        {
            // A maximized window is in no container, so there is none to transfer: it stays where
            // it is
        }
        Ok(())
    }

    #[tracing::instrument(skip(self))]
    pub fn swap_containers(
        &mut self,
        origin: (usize, usize, usize),
        target: (usize, usize, usize),
    ) -> eyre::Result<()> {
        let (origin_monitor_idx, origin_workspace_idx, origin_container_idx) = origin;
        let (target_monitor_idx, target_workspace_idx, target_container_idx) = target;

        let origin_container_is_valid = self
            .monitors_mut()
            .get_mut(origin_monitor_idx)
            .ok_or_eyre("there is no monitor at this index")?
            .workspaces_mut()
            .get_mut(origin_workspace_idx)
            .ok_or_eyre("there is no workspace at this index")?
            .containers()
            .get(origin_container_idx)
            .is_some();

        let target_container_is_valid = self
            .monitors_mut()
            .get_mut(target_monitor_idx)
            .ok_or_eyre("there is no monitor at this index")?
            .workspaces_mut()
            .get_mut(target_workspace_idx)
            .ok_or_eyre("there is no workspace at this index")?
            .containers()
            .get(origin_container_idx)
            .is_some();

        if origin_container_is_valid && target_container_is_valid {
            let origin_container = self
                .monitors_mut()
                .get_mut(origin_monitor_idx)
                .ok_or_eyre("there is no monitor at this index")?
                .workspaces_mut()
                .get_mut(origin_workspace_idx)
                .ok_or_eyre("there is no workspace at this index")?
                .remove_container(origin_container_idx)
                .ok_or_eyre("there is no container at this index")?;

            let target_container = self
                .monitors_mut()
                .get_mut(target_monitor_idx)
                .ok_or_eyre("there is no monitor at this index")?
                .workspaces_mut()
                .get_mut(target_workspace_idx)
                .ok_or_eyre("there is no workspace at this index")?
                .remove_container(target_container_idx);

            self.monitors_mut()
                .get_mut(target_monitor_idx)
                .ok_or_eyre("there is no monitor at this index")?
                .workspaces_mut()
                .get_mut(target_workspace_idx)
                .ok_or_eyre("there is no workspace at this index")?
                .containers_mut()
                .insert(target_container_idx, origin_container);

            if let Some(target_container) = target_container {
                self.monitors_mut()
                    .get_mut(origin_monitor_idx)
                    .ok_or_eyre("there is no monitor at this index")?
                    .workspaces_mut()
                    .get_mut(origin_workspace_idx)
                    .ok_or_eyre("there is no workspace at this index")?
                    .containers_mut()
                    .insert(origin_container_idx, target_container);
            }
        }

        Ok(())
    }

    #[tracing::instrument(skip(self))]
    pub fn transfer_container(
        &mut self,
        origin: (usize, usize, usize),
        target: (usize, usize, usize),
    ) -> eyre::Result<()> {
        let (origin_monitor_idx, origin_workspace_idx, origin_container_idx) = origin;
        let (target_monitor_idx, target_workspace_idx, target_container_idx) = target;

        let origin_container = self
            .monitors_mut()
            .get_mut(origin_monitor_idx)
            .ok_or_eyre("there is no monitor at this index")?
            .workspaces_mut()
            .get_mut(origin_workspace_idx)
            .ok_or_eyre("there is no workspace at this index")?
            .remove_container(origin_container_idx)
            .ok_or_eyre("there is no container at this index")?;

        let target_workspace = self
            .monitors_mut()
            .get_mut(target_monitor_idx)
            .ok_or_eyre("there is no monitor at this index")?
            .workspaces_mut()
            .get_mut(target_workspace_idx)
            .ok_or_eyre("there is no workspace at this index")?;

        target_workspace
            .containers_mut()
            .insert(target_container_idx, origin_container);

        target_workspace.focus_container(target_container_idx);

        Ok(())
    }

    /// Sets the wallpaper the monitor at `monitor_idx` is to show for its workspace at
    /// `workspace_idx`, and gives the borders the colors of its theme ([`Workspace::apply_wallpaper`]).
    /// A reloaded configuration calls it for the workspace each monitor shows, so that the
    /// wallpapers it gives are shown at once.
    ///
    /// # Errors
    ///
    /// There is no monitor at `monitor_idx` or no workspace at `workspace_idx`.
    #[tracing::instrument(skip(self))]
    pub fn apply_wallpaper_for_monitor_workspace(
        &mut self,
        monitor_idx: usize,
        workspace_idx: usize,
    ) -> eyre::Result<()> {
        let monitor = self
            .monitors_mut()
            .get_mut(monitor_idx)
            .ok_or_eyre("there is no monitor")?;

        let monitor_id = monitor.id;
        let monitor_wp = monitor.wallpaper.clone();

        let workspace = monitor
            .workspaces()
            .get(workspace_idx)
            .ok_or_eyre("there is no workspace")?;

        workspace.apply_wallpaper(monitor_id, &monitor_wp)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::backend::fake::FakeBackend;
    use crate::backend::with_backend;
    use crate::monitor;
    use crossbeam_channel::Sender;
    use crossbeam_channel::bounded;
    use std::path::PathBuf;
    use std::sync::Arc;
    use uuid::Uuid;

    struct TestContext {
        socket_path: Option<PathBuf>,
    }

    impl Drop for TestContext {
        fn drop(&mut self) {
            if let Some(socket_path) = &self.socket_path {
                // Clean up the socket file
                std::fs::remove_file(socket_path).unwrap();
            }
        }
    }

    fn setup_window_manager() -> (WindowManager, TestContext) {
        let (_sender, receiver): (Sender<WindowManagerEvent>, Receiver<WindowManagerEvent>) =
            bounded(1);

        // Temporary socket path for testing
        // /tmp: short enough for a unix socket path on macOS, and writable wherever the tests run
        let socket_name = format!("/tmp/komorebi-test-{}.sock", Uuid::new_v4());
        let socket_path = PathBuf::from(socket_name);

        // Create a new WindowManager instance
        let wm = WindowManager::new(receiver, Some(socket_path.clone()));

        // Window Manager should be created successfully
        assert!(wm.is_ok());

        (
            wm.unwrap(),
            TestContext {
                socket_path: Some(socket_path),
            },
        )
    }

    #[test]
    fn test_create_window_manager() {
        let (_wm, _test_context) = setup_window_manager();
    }

    #[test]
    fn test_focus_workspace() {
        let (mut wm, _test_context) = setup_window_manager();

        let m = monitor::new(
            0,
            Rect::default(),
            Rect::default(),
            "TestMonitor".to_string(),
            "TestDeviceID".to_string(),
        );

        // a new monitor should have a single workspace
        assert_eq!(m.workspaces().len(), 1);

        // the next index on the monitor should be the not-yet-created second workspace
        let new_workspace_index = m.new_workspace_idx();
        assert_eq!(new_workspace_index, 1);

        // add the monitor to the window manager
        wm.monitors_mut().push_back(m);

        {
            // focusing a workspace which doesn't yet exist should create it
            let monitor = wm.focused_monitor_mut().unwrap();
            monitor.focus_workspace(new_workspace_index).unwrap();
            assert_eq!(monitor.workspaces().len(), 2);
        }
        assert_eq!(wm.focused_workspace_idx().unwrap(), 1);

        {
            // focusing a workspace many indices ahead should create all workspaces
            // required along the way
            let monitor = wm.focused_monitor_mut().unwrap();
            monitor.focus_workspace(new_workspace_index + 2).unwrap();
            assert_eq!(monitor.workspaces().len(), 4);
        }
        assert_eq!(wm.focused_workspace_idx().unwrap(), 3);

        // we should be able to successfully focus an existing workspace too
        wm.focus_workspace(0).unwrap();
        assert_eq!(wm.focused_workspace_idx().unwrap(), 0);
    }

    #[test]
    fn test_remove_focused_workspace() {
        let (mut wm, _context) = setup_window_manager();

        let m = monitor::new(
            0,
            Rect::default(),
            Rect::default(),
            "TestMonitor".to_string(),
            "TestDeviceID".to_string(),
        );

        // a new monitor should have a single workspace
        assert_eq!(m.workspaces().len(), 1);

        // the next index on the monitor should be the not-yet-created second workspace
        let new_workspace_index = m.new_workspace_idx();
        assert_eq!(new_workspace_index, 1);

        // add the monitor to the window manager
        wm.monitors_mut().push_back(m);

        {
            // focus a workspace which doesn't yet exist should create it
            let monitor = wm.focused_monitor_mut().unwrap();
            monitor.focus_workspace(new_workspace_index + 1).unwrap();

            // Monitor focused workspace should be 2
            assert_eq!(monitor.focused_workspace_idx(), 2);

            // Should have 3 Workspaces
            assert_eq!(monitor.workspaces().len(), 3);
        }

        // Remove the focused workspace
        wm.remove_focused_workspace().unwrap();

        {
            let monitor = wm.focused_monitor_mut().unwrap();
            monitor.focus_workspace(new_workspace_index).unwrap();

            // Should be focused on workspace 1
            assert_eq!(monitor.focused_workspace_idx(), 1);

            // Should have 2 Workspaces
            assert_eq!(monitor.workspaces().len(), 2);
        }
    }

    #[test]
    fn test_set_workspace_name() {
        let (mut wm, _test_context) = setup_window_manager();

        let m = monitor::new(
            0,
            Rect::default(),
            Rect::default(),
            "TestMonitor".to_string(),
            "TestDeviceID".to_string(),
        );

        // a new monitor should have a single workspace
        assert_eq!(m.workspaces().len(), 1);

        // create a new workspace
        let new_workspace_index = m.new_workspace_idx();
        assert_eq!(new_workspace_index, 1);

        // add the monitor to the window manager
        wm.monitors_mut().push_back(m);

        {
            // focusing a workspace which doesn't yet exist should create it
            let monitor = wm.focused_monitor_mut().unwrap();
            monitor.focus_workspace(new_workspace_index).unwrap();
            assert_eq!(monitor.workspaces().len(), 2);
        }
        assert_eq!(wm.focused_workspace_idx().unwrap(), 1);

        // set the name of the first workspace
        wm.set_workspace_name(0, 0, "workspace1".to_string())
            .unwrap();

        // monitor_workspace_index_by_name should return the index of the workspace with the name "workspace1"
        let workspace_index = wm.monitor_workspace_index_by_name("workspace1").unwrap();

        // workspace index 0 should now have the name "workspace1"
        assert_eq!(workspace_index.1, 0);
    }

    #[test]
    fn test_switch_focus_monitors() {
        let (mut wm, _test_context) = setup_window_manager();

        {
            // Create a first monitor
            let m = monitor::new(
                0,
                Rect::default(),
                Rect::default(),
                "TestMonitor".to_string(),
                "TestDeviceID".to_string(),
            );

            // monitor should have a single workspace
            assert_eq!(m.workspaces().len(), 1);

            // add the monitor to the window manager
            wm.monitors_mut().push_back(m);
        }
        assert_eq!(wm.monitors().len(), 1);

        {
            // Create a second monitor
            let m = monitor::new(
                1,
                Rect::default(),
                Rect::default(),
                "TestMonitor2".to_string(),
                "TestDeviceID2".to_string(),
            );

            // monitor should have a single workspace
            assert_eq!(m.workspaces().len(), 1);

            // add the monitor to the window manager
            wm.monitors_mut().push_back(m);
        }
        assert_eq!(wm.monitors().len(), 2);

        {
            // Create a third monitor
            let m = monitor::new(
                2,
                Rect::default(),
                Rect::default(),
                "TestMonitor3".to_string(),
                "TestDeviceID3".to_string(),
            );

            // monitor should have a single workspace
            assert_eq!(m.workspaces().len(), 1);

            // add the monitor to the window manager
            wm.monitors_mut().push_back(m);
        }
        assert_eq!(wm.monitors().len(), 3);

        {
            // Set the first monitor as focused and check if it is focused
            wm.focus_monitor(0).unwrap();
            let current_monitor_idx = wm.monitors.focused_idx();
            assert_eq!(current_monitor_idx, 0);
        }

        {
            // Set the second monitor as focused and check if it is focused
            wm.focus_monitor(1).unwrap();
            let current_monitor_idx = wm.monitors.focused_idx();
            assert_eq!(current_monitor_idx, 1);
        }

        {
            // Set the third monitor as focused and check if it is focused
            wm.focus_monitor(2).unwrap();
            let current_monitor_idx = wm.monitors.focused_idx();
            assert_eq!(current_monitor_idx, 2);
        }

        // Switch back to the first monitor
        wm.focus_monitor(0).unwrap();
        let current_monitor_idx = wm.monitors.focused_idx();
        assert_eq!(current_monitor_idx, 0);
    }

    #[test]
    fn test_switch_focus_to_nonexistent_monitor() {
        let (mut wm, _test_context) = setup_window_manager();

        {
            // Create a first monitor
            let m = monitor::new(
                0,
                Rect::default(),
                Rect::default(),
                "TestMonitor".to_string(),
                "TestDeviceID".to_string(),
            );

            // monitor should have a single workspace
            assert_eq!(m.workspaces().len(), 1);

            // add the monitor to the window manager
            wm.monitors_mut().push_back(m);
        }

        // Should have 1 monitor and the monitor index should be 0
        assert_eq!(wm.monitors().len(), 1);
        assert_eq!(wm.focused_monitor_idx(), 0);

        // Should receive an error when trying to focus a non-existent monitor
        let result = wm.focus_monitor(1);
        assert!(
            result.is_err(),
            "Expected an error when focusing a non-existent monitor"
        );

        // Should still be focused on the first monitor
        assert_eq!(wm.focused_monitor_idx(), 0);
    }

    #[test]
    fn test_focused_monitor_size() {
        let (mut wm, _test_context) = setup_window_manager();

        {
            // Create a first monitor
            let m = monitor::new(
                0,
                Rect::default(),
                Rect::default(),
                "TestMonitor".to_string(),
                "TestDeviceID".to_string(),
            );

            // monitor should have a single workspace
            assert_eq!(m.workspaces().len(), 1);

            // add the monitor to the window manager
            wm.monitors_mut().push_back(m);
        }
        assert_eq!(wm.monitors().len(), 1);

        {
            // Set the first monitor as focused and check if it is focused
            wm.focus_monitor(0).unwrap();
            let current_monitor_size = wm.focused_monitor_size().unwrap();
            assert_eq!(current_monitor_size, Rect::default());
        }
    }

    #[test]
    fn test_focus_container_in_cycle_direction() {
        let (mut wm, _test_context) = setup_window_manager();

        // Create a monitor
        let mut m = monitor::new(
            0,
            Rect::default(),
            Rect::default(),
            "TestMonitor".to_string(),
            "TestDeviceID".to_string(),
        );

        let workspace = m.focused_workspace_mut().unwrap();
        workspace.layer = WorkspaceLayer::Tiling;

        for i in 0..4 {
            let mut container = Container::default();
            container.windows_mut().push_back(Window::from(i));
            workspace.add_container_to_back(container);
        }
        assert_eq!(workspace.containers().len(), 4);

        workspace.focus_container(0);

        // add the monitor to the window manager
        wm.monitors_mut().push_back(m);

        // container focus should be on the second container
        wm.focus_container_in_cycle_direction(CycleDirection::Next)
            .ok();
        assert_eq!(wm.focused_container_idx().unwrap(), 1);

        // container focus should be on the third container
        wm.focus_container_in_cycle_direction(CycleDirection::Next)
            .ok();
        assert_eq!(wm.focused_container_idx().unwrap(), 2);

        // container focus should be on the second container
        wm.focus_container_in_cycle_direction(CycleDirection::Previous)
            .ok();
        assert_eq!(wm.focused_container_idx().unwrap(), 1);

        // container focus should be on the first container
        wm.focus_container_in_cycle_direction(CycleDirection::Previous)
            .ok();
        assert_eq!(wm.focused_container_idx().unwrap(), 0);
    }

    #[test]
    fn test_transfer_window() {
        let (mut wm, _context) = setup_window_manager();

        {
            // Create a first monitor
            let mut m = monitor::new(
                0,
                Rect::default(),
                Rect::default(),
                "TestMonitor".to_string(),
                "TestDeviceID".to_string(),
            );

            // Create a container
            let workspace = m.focused_workspace_mut().unwrap();
            let mut container = Container::default();

            // Add a window to the container
            container.windows_mut().push_back(Window::from(0));
            workspace.add_container_to_back(container);

            // Should contain 1 container
            assert_eq!(workspace.containers().len(), 1);

            wm.monitors_mut().push_back(m);
        }

        {
            // Create a second monitor
            let mut m = monitor::new(
                1,
                Rect::default(),
                Rect::default(),
                "TestMonitor2".to_string(),
                "TestDeviceID2".to_string(),
            );

            // Create a container
            let workspace = m.focused_workspace_mut().unwrap();
            let mut container = Container::default();

            // Add a window to the container
            container.windows_mut().push_back(Window::from(1));
            workspace.add_container_to_back(container);

            // Should contain 1 container
            assert_eq!(workspace.containers().len(), 1);

            wm.monitors_mut().push_back(m);
        }

        // Should contain 2 monitors
        assert_eq!(wm.monitors().len(), 2);

        {
            // Monitor 0, Workspace 0, Window 0
            let origin = (0, 0, 0);

            // Monitor 1, Workspace 0, Window 0
            let target = (1, 0, 0);

            // Transfer the window from monitor 0 to monitor 1
            wm.transfer_window(origin, target).unwrap();

            // Monitor 1 should contain 0 containers
            let workspace = wm.focused_workspace_mut().unwrap();
            assert_eq!(workspace.containers().len(), 0);

            // Monitor 2 should contain 2 containers
            wm.focus_monitor(1).unwrap();
            let workspace = wm.focused_workspace_mut().unwrap();
            assert_eq!(workspace.containers().len(), 2);
        }

        {
            // Monitor 1, Workspace 0, Window 0
            let origin = (1, 0, 0);

            // Monitor 0, Workspace 0, Window 0
            let target = (0, 0, 0);

            // Transfer the window from monitor 1 back to monitor 0
            wm.transfer_window(origin, target).unwrap();

            // Monitor 2 should contain 1 containers
            let workspace = wm.focused_workspace_mut().unwrap();
            assert_eq!(workspace.containers().len(), 1);

            // Monitor 1 should contain 1 containers
            wm.focus_monitor(0).unwrap();
            let workspace = wm.focused_workspace_mut().unwrap();
            assert_eq!(workspace.containers().len(), 1);
        }
    }

    #[test]
    fn test_transfer_window_to_nonexistent_monitor() {
        // NOTE: transfer_window is primarily used when a window is being dragged by a mouse. The
        // transfer_window function does return an error when the target monitor doesn't exist but
        // there is a bug where the window isn't in the container after the window fails to
        // transfer. The test will test for the result of the transfer_window function but not if
        // the window is in the container after the transfer fails.

        let (mut wm, _context) = setup_window_manager();

        {
            // Create a first monitor
            let mut m = monitor::new(
                0,
                Rect::default(),
                Rect::default(),
                "TestMonitor".to_string(),
                "TestDeviceID".to_string(),
            );

            // Create a container
            let workspace = m.focused_workspace_mut().unwrap();
            let mut container = Container::default();

            // Add a window to the container
            container.windows_mut().push_back(Window::from(0));
            workspace.add_container_to_back(container);

            // Should contain 1 container
            assert_eq!(workspace.containers().len(), 1);

            wm.monitors_mut().push_back(m);
        }

        {
            // Monitor 0, Workspace 0, Window 0
            let origin = (0, 0, 0);

            // Monitor 1, Workspace 0, Window 0
            //
            let target = (1, 0, 0);

            // Attempt to transfer the window from monitor 0 to a non-existent monitor
            let result = wm.transfer_window(origin, target);

            // Result should be an error since the monitor doesn't exist
            assert!(
                result.is_err(),
                "Expected an error when transferring to a non-existent monitor"
            );

            assert_eq!(wm.focused_container_idx().unwrap(), 0);
            assert_eq!(wm.focused_workspace_idx().unwrap(), 0);
        }
    }

    #[test]
    fn test_transfer_container() {
        let (mut wm, _context) = setup_window_manager();

        {
            // Create a first monitor
            let mut m = monitor::new(
                0,
                Rect::default(),
                Rect::default(),
                "TestMonitor".to_string(),
                "TestDeviceID".to_string(),
            );

            // Create a container
            let workspace = m.focused_workspace_mut().unwrap();
            let mut container = Container::default();

            // Add a window to the container
            container.windows_mut().push_back(Window::from(0));
            workspace.add_container_to_back(container);

            // Should contain 1 container
            assert_eq!(workspace.containers().len(), 1);

            wm.monitors_mut().push_back(m);
        }

        {
            // Create a second monitor
            let mut m = monitor::new(
                1,
                Rect::default(),
                Rect::default(),
                "TestMonitor2".to_string(),
                "TestDeviceID2".to_string(),
            );

            // Create a container
            let workspace = m.focused_workspace_mut().unwrap();
            let mut container = Container::default();

            // Add a window to the container
            container.windows_mut().push_back(Window::from(1));
            workspace.add_container_to_back(container);

            // Should contain 1 container
            assert_eq!(workspace.containers().len(), 1);

            wm.monitors_mut().push_back(m);
        }

        // Should contain 2 monitors
        assert_eq!(wm.monitors().len(), 2);

        {
            // Monitor 0, Workspace 0, Window 0
            let origin = (0, 0, 0);

            // Monitor 1, Workspace 0, Window 0
            let target = (1, 0, 0);

            // Transfer the window from monitor 0 to monitor 1
            wm.transfer_container(origin, target).unwrap();

            // Monitor 1 should contain 0 containers
            let workspace = wm.focused_workspace_mut().unwrap();
            assert_eq!(workspace.containers().len(), 0);

            // Monitor 2 should contain 2 containers
            wm.focus_monitor(1).unwrap();
            let workspace = wm.focused_workspace_mut().unwrap();
            assert_eq!(workspace.containers().len(), 2);
        }

        {
            // Monitor 1, Workspace 0, Window 0
            let origin = (1, 0, 0);

            // Monitor 0, Workspace 0, Window 0
            let target = (0, 0, 0);

            // Transfer the window from monitor 1 back to monitor 0
            wm.transfer_container(origin, target).unwrap();

            // Monitor 2 should contain 1 containers
            let workspace = wm.focused_workspace_mut().unwrap();
            assert_eq!(workspace.containers().len(), 1);

            // Monitor 1 should contain 1 containers
            wm.focus_monitor(0).unwrap();
            let workspace = wm.focused_workspace_mut().unwrap();
            assert_eq!(workspace.containers().len(), 1);
        }
    }

    #[test]
    fn test_remove_window_from_container() {
        let (mut wm, _context) = setup_window_manager();

        {
            // Create a first monitor
            let mut m = monitor::new(
                0,
                Rect::default(),
                Rect::default(),
                "TestMonitor1".to_string(),
                "TestDeviceID1".to_string(),
            );

            // Create a container
            let mut container = Container::default();

            // Add three windows to the container
            for i in 0..3 {
                container.windows_mut().push_back(Window::from(i));
            }
            // Should have 3 windows in the container
            assert_eq!(container.windows().len(), 3);

            // Focus last window
            container.focus_window(2);

            // Should be focused on the 2nd window
            assert_eq!(container.focused_window_idx(), 2);

            // Add the container to a workspace
            let workspace = m.focused_workspace_mut().unwrap();
            workspace.add_container_to_back(container);

            // Add monitor to the window manager
            wm.monitors_mut().push_back(m);
        }

        // Remove the focused window from the container
        wm.remove_window_from_container().ok();

        {
            // Should have 2 containers in the workspace
            let workspace = wm.focused_workspace_mut().unwrap();
            assert_eq!(workspace.containers().len(), 2);

            // Should contain 1 window in the new container
            let container = workspace.focused_container_mut().unwrap();
            assert_eq!(container.windows().len(), 1);
        }

        {
            // Switch to the old container
            let workspace = wm.focused_workspace_mut().unwrap();
            workspace.focus_container(0);

            // Should contain 2 windows in the old container
            let container = workspace.focused_container_mut().unwrap();
            assert_eq!(container.windows().len(), 2);
        }
    }

    #[test]
    fn test_remove_nonexistent_window_from_container() {
        let (mut wm, _context) = setup_window_manager();

        {
            // Create a first monitor
            let mut m = monitor::new(
                0,
                Rect::default(),
                Rect::default(),
                "TestMonitor1".to_string(),
                "TestDeviceID1".to_string(),
            );

            // Create a container
            let container = Container::default();

            // Should have 3 windows in the container
            assert_eq!(container.windows().len(), 0);

            // Add the container to a workspace
            let workspace = m.focused_workspace_mut().unwrap();
            workspace.add_container_to_back(container);

            // Add monitor to the window manager
            wm.monitors_mut().push_back(m);
        }

        // Should receive an error when trying to remove a window from an empty container
        let result = wm.remove_window_from_container();
        assert!(
            result.is_err(),
            "Expected an error when trying to remove a window from an empty container"
        );
    }

    #[test]
    fn cycle_container_window_in_direction() {
        let (mut wm, _context) = setup_window_manager();

        {
            let mut m = monitor::new(
                0,
                Rect::default(),
                Rect::default(),
                "TestMonitor".to_string(),
                "TestDeviceID".to_string(),
            );

            let workspace = m.focused_workspace_mut().unwrap();

            {
                let mut container = Container::default();

                for i in 0..3 {
                    container.windows_mut().push_back(Window::from(i));
                }

                // Should have 3 windows in the container
                assert_eq!(container.windows().len(), 3);

                // Add container to workspace
                workspace.add_container_to_back(container);
            }

            // Add monitor to the window manager
            wm.monitors_mut().push_back(m);
        }

        // Cycle to the next window
        wm.cycle_container_window_in_direction(CycleDirection::Next)
            .ok();

        {
            // Should be on Window 1
            let workspace = wm.focused_workspace_mut().unwrap();
            let container = workspace.focused_container_mut().unwrap();
            assert_eq!(container.focused_window_idx(), 1);
        }

        // Cycle to the next window
        wm.cycle_container_window_in_direction(CycleDirection::Next)
            .ok();

        {
            // Should be on Window 2
            let workspace = wm.focused_workspace_mut().unwrap();
            let container = workspace.focused_container_mut().unwrap();
            assert_eq!(container.focused_window_idx(), 2);
        }

        // Cycle to the previous window
        wm.cycle_container_window_in_direction(CycleDirection::Previous)
            .ok();

        {
            // Should be on Window 1
            let workspace = wm.focused_workspace_mut().unwrap();
            let container = workspace.focused_container_mut().unwrap();
            assert_eq!(container.focused_window_idx(), 1);
        }
    }

    #[test]
    fn test_cycle_nonexistent_windows() {
        let (mut wm, _context) = setup_window_manager();

        {
            // Create a first monitor
            let mut m = monitor::new(
                0,
                Rect::default(),
                Rect::default(),
                "TestMonitor1".to_string(),
                "TestDeviceID1".to_string(),
            );

            // Create a container
            let container = Container::default();

            // Should have 3 windows in the container
            assert_eq!(container.windows().len(), 0);

            // Add the container to a workspace
            let workspace = m.focused_workspace_mut().unwrap();
            workspace.add_container_to_back(container);

            // Add monitor to the window manager
            wm.monitors_mut().push_back(m);
        }

        // Should return an error when trying to cycle through windows in an empty container
        let result = wm.cycle_container_window_in_direction(CycleDirection::Next);
        assert!(
            result.is_err(),
            "Expected an error when cycling through windows in an empty container"
        );
    }

    #[test]
    fn test_cycle_container_window_index_in_direction() {
        let (mut wm, _context) = setup_window_manager();

        {
            let mut m = monitor::new(
                0,
                Rect::default(),
                Rect::default(),
                "TestMonitor".to_string(),
                "TestDeviceID".to_string(),
            );

            let workspace = m.focused_workspace_mut().unwrap();

            {
                let mut container = Container::default();

                for i in 0..3 {
                    container.windows_mut().push_back(Window::from(i));
                }

                // Should have 3 windows in the container
                assert_eq!(container.windows().len(), 3);

                // Add container to workspace
                workspace.add_container_to_back(container);
            }

            // Add monitor to the window manager
            wm.monitors_mut().push_back(m);
        }

        // Cycle to the next window
        wm.cycle_container_window_index_in_direction(CycleDirection::Next)
            .ok();

        {
            // Should be on Window 1
            let workspace = wm.focused_workspace_mut().unwrap();
            let container = workspace.focused_container_mut().unwrap();
            assert_eq!(container.focused_window_idx(), 1);
        }

        // Cycle to the next window
        wm.cycle_container_window_index_in_direction(CycleDirection::Next)
            .ok();

        {
            // Should be on Window 2
            let workspace = wm.focused_workspace_mut().unwrap();
            let container = workspace.focused_container_mut().unwrap();
            assert_eq!(container.focused_window_idx(), 2);
        }

        // Cycle to the Previous window
        wm.cycle_container_window_index_in_direction(CycleDirection::Previous)
            .ok();

        {
            // Should be on Window 1
            let workspace = wm.focused_workspace_mut().unwrap();
            let container = workspace.focused_container_mut().unwrap();
            assert_eq!(container.focused_window_idx(), 1);
        }
    }

    #[test]
    fn test_swap_containers() {
        let (mut wm, _context) = setup_window_manager();

        {
            // Create a first monitor
            let mut m = monitor::new(
                0,
                Rect::default(),
                Rect::default(),
                "TestMonitor".to_string(),
                "TestDeviceID".to_string(),
            );

            // Create a container
            let mut container = Container::default();

            // Add three windows to the container
            for i in 0..3 {
                container.windows_mut().push_back(Window::from(i));
            }

            // Should have 3 windows in the container
            assert_eq!(container.windows().len(), 3);

            // Add the container to the workspace
            let workspace = m.focused_workspace_mut().unwrap();
            workspace.add_container_to_back(container);

            // Add monitor to the window manager
            wm.monitors_mut().push_back(m);
        }

        {
            // Create a second monitor
            let mut m = monitor::new(
                1,
                Rect::default(),
                Rect::default(),
                "TestMonitor2".to_string(),
                "TestDeviceID2".to_string(),
            );

            // Create a container
            let workspace = m.focused_workspace_mut().unwrap();
            let mut container = Container::default();

            // Add a window to the container
            container.windows_mut().push_back(Window::from(1));
            workspace.add_container_to_back(container);

            // Should contain 1 container
            assert_eq!(workspace.containers().len(), 1);

            wm.monitors_mut().push_back(m);
        }

        // Should contain 2 monitors
        assert_eq!(wm.monitors().len(), 2);

        // Monitor 0, Workspace 0, Window 0
        let origin = (0, 0, 0);

        // Monitor 1, Workspace 0, Window 0
        let target = (1, 0, 0);

        wm.swap_containers(origin, target).unwrap();

        {
            // Monitor 0 Workspace 0 container 0 should contain 1 container
            let workspace = wm.focused_workspace_mut().unwrap();
            let container = workspace.focused_container_mut().unwrap();
            assert_eq!(container.windows().len(), 1);
        }

        wm.focus_monitor(1).unwrap();

        {
            // Monitor 1 Workspace 0 container 0 should contain 3 containers
            let workspace = wm.focused_workspace_mut().unwrap();
            let container = workspace.focused_container_mut().unwrap();
            assert_eq!(container.windows().len(), 3);
        }
    }

    // #[test]
    // fn test_swap_container_with_nonexistent_container() {
    //     let (mut wm, _context) = setup_window_manager();
    //
    //     {
    //         // Create a first monitor
    //         let mut m = monitor::new(
    //             0,
    //             Rect::default(),
    //             Rect::default(),
    //             "TestMonitor".to_string(),
    //             "TestDeviceID".to_string(),
    //         );
    //
    //         // Create a container
    //         let mut container = Container::default();
    //
    //         // Add three windows to the container
    //         for i in 0..3 {
    //             container.windows_mut().push_back(Window::from(i));
    //         }
    //
    //         // Should have 3 windows in the container
    //         assert_eq!(container.windows().len(), 3);
    //
    //         // Add the container to the workspace
    //         let workspace = m.focused_workspace_mut().unwrap();
    //         workspace.add_container_to_back(container);
    //
    //         // Add monitor to the window manager
    //         wm.monitors_mut().push_back(m);
    //     }
    //
    //     // Monitor 0, Workspace 0, Window 0
    //     let origin = (0, 0, 0);
    //
    //     // Monitor 1, Workspace 0, Window 0
    //     let target = (0, 3, 0);
    //
    //     // Should be focused on the first container
    //     assert_eq!(wm.focused_container_idx().unwrap(), 0);
    //
    //     // Should return an error since there is only one container in the workspace
    //     let result = wm.swap_containers(origin, target);
    //     assert!(
    //         result.is_err(),
    //         "Expected an error when swapping with a non-existent container"
    //     );
    //
    //     // Should still be focused on the first container
    //     assert_eq!(wm.focused_container_idx().unwrap(), 0);
    //
    //     {
    //         // Should still have 1 container in the workspace
    //         let workspace = wm.focused_workspace_mut().unwrap();
    //         assert_eq!(workspace.containers().len(), 1);
    //
    //         // Container should still have 3 windows
    //         let container = workspace.focused_container_mut().unwrap();
    //         assert_eq!(container.windows().len(), 3);
    //     }
    // }

    #[test]
    fn test_swap_monitor_workspaces() {
        let (mut wm, _context) = setup_window_manager();

        {
            // Create a first monitor
            let mut m = monitor::new(
                0,
                Rect::default(),
                Rect::default(),
                "TestMonitor".to_string(),
                "TestDeviceID".to_string(),
            );

            // Create a container
            let mut container = Container::default();

            // Add three windows to the container
            for i in 0..3 {
                container.windows_mut().push_back(Window::from(i));
            }

            // Should have 3 windows in the container
            assert_eq!(container.windows().len(), 3);

            // Add the container to the workspace
            let workspace = m.focused_workspace_mut().unwrap();
            workspace.add_container_to_back(container);

            // Add monitor to the window manager
            wm.monitors_mut().push_back(m);
        }

        {
            // Create a second monitor
            let mut m = monitor::new(
                1,
                Rect::default(),
                Rect::default(),
                "TestMonitor2".to_string(),
                "TestDeviceID2".to_string(),
            );

            // Create a container
            let workspace = m.focused_workspace_mut().unwrap();
            let mut container = Container::default();

            // Add a window to the container
            container.windows_mut().push_back(Window::from(1));
            workspace.add_container_to_back(container);

            // Should contain 1 container
            assert_eq!(workspace.containers().len(), 1);

            // Add the monitor to the window manager
            wm.monitors_mut().push_back(m);
        }

        // Swap the workspaces between Monitor 0 and Monitor 1
        wm.swap_monitor_workspaces(0, 1).ok();

        {
            // The focused workspace container in Monitor 0 should contain 3 containers
            let workspace = wm.focused_workspace_mut().unwrap();
            let container = workspace.focused_container_mut().unwrap();
            assert_eq!(container.windows().len(), 1);
        }

        // Switch to Monitor 1
        wm.focus_monitor(1).unwrap();
        assert_eq!(wm.focused_monitor_idx(), 1);

        {
            // The focused workspace container in Monitor 1 should contain 3 containers
            let workspace = wm.focused_workspace_mut().unwrap();
            let container = workspace.focused_container_mut().unwrap();
            assert_eq!(container.windows().len(), 3);
        }
    }

    #[test]
    fn test_swap_workspace_with_nonexistent_monitor() {
        let (mut wm, _context) = setup_window_manager();

        {
            let mut m = monitor::new(
                0,
                Rect::default(),
                Rect::default(),
                "TestMonitor".to_string(),
                "TestDeviceID".to_string(),
            );

            // Add another workspace
            let new_workspace_index = m.new_workspace_idx();
            m.focus_workspace(new_workspace_index).unwrap();

            // Should have 2 workspaces
            assert_eq!(m.workspaces().len(), 2);

            // Add monitor to window manager
            wm.monitors_mut().push_back(m);
        }

        // Should be an error since Monitor 1 does not exist
        let result = wm.swap_monitor_workspaces(1, 0);
        assert!(
            result.is_err(),
            "Expected an error when swapping with a non-existent monitor"
        );

        {
            // Should still have 2 workspaces in Monitor 0
            let monitor = wm.monitors().front().unwrap();
            let workspaces = monitor.workspaces();
            assert_eq!(
                workspaces.len(),
                2,
                "Expected 2 workspaces after swap attempt"
            );
            assert_eq!(wm.focused_monitor_idx(), 0);
        }
    }

    #[test]
    fn test_move_workspace_to_monitor() {
        let (mut wm, _context) = setup_window_manager();

        {
            let mut m = monitor::new(
                0,
                Rect::default(),
                Rect::default(),
                "TestMonitor".to_string(),
                "TestDeviceID".to_string(),
            );

            // Add another workspace
            let new_workspace_index = m.new_workspace_idx();
            m.focus_workspace(new_workspace_index).unwrap();

            // Should have 2 workspaces
            assert_eq!(m.workspaces().len(), 2);

            // Add monitor to window manager
            wm.monitors_mut().push_back(m);
        }

        {
            let m = monitor::new(
                1,
                Rect::default(),
                Rect::default(),
                "TestMonitor2".to_string(),
                "TestDeviceID2".to_string(),
            );

            // Should contain 1 workspace
            assert_eq!(m.workspaces().len(), 1);

            // Add monitor to workspace
            wm.monitors_mut().push_back(m);
        }

        // Should contain 2 monitors
        assert_eq!(wm.monitors().len(), 2);

        // Move a workspace from Monitor 0 to Monitor 1
        wm.move_workspace_to_monitor(1).ok();

        {
            // Should be focused on Monitor 1
            assert_eq!(wm.focused_monitor_idx(), 1);

            // Should contain 2 workspaces
            let monitor = wm.focused_monitor_mut().unwrap();
            assert_eq!(monitor.workspaces().len(), 2);
        }

        {
            // Switch to Monitor 0
            wm.focus_monitor(0).unwrap();

            // Should contain 1 workspace
            let monitor = wm.focused_monitor_mut().unwrap();
            assert_eq!(monitor.workspaces().len(), 1);
        }
    }

    #[test]
    fn test_move_workspace_to_nonexistent_monitor() {
        let (mut wm, _context) = setup_window_manager();

        {
            let mut m = monitor::new(
                0,
                Rect::default(),
                Rect::default(),
                "TestMonitor".to_string(),
                "TestDeviceID".to_string(),
            );

            // Add another workspace
            let new_workspace_index = m.new_workspace_idx();
            m.focus_workspace(new_workspace_index).unwrap();

            // Should have 2 workspaces
            assert_eq!(m.workspaces().len(), 2);

            // Add monitor to window manager
            wm.monitors_mut().push_back(m);
        }

        // Attempt to move a workspace to a non-existent monitor
        let result = wm.move_workspace_to_monitor(1);

        // Should be an error since Monitor 1 does not exist
        assert!(
            result.is_err(),
            "Expected an error when moving to a non-existent monitor"
        );
    }

    #[test]
    fn test_toggle_tiling() {
        let (mut wm, _context) = setup_window_manager();

        {
            let mut m = monitor::new(
                0,
                Rect::default(),
                Rect::default(),
                "TestMonitor".to_string(),
                "TestDeviceID".to_string(),
            );

            // Set Workspace Layer to Tiling
            let workspace = m.focused_workspace_mut().unwrap();
            workspace.layer = WorkspaceLayer::Tiling;

            // Tiling state should be true
            assert!(workspace.tile);

            // Add monitor to workspace
            wm.monitors_mut().push_back(m);
        }

        {
            // Tiling state should be false
            wm.toggle_tiling().unwrap();
            let workspace = wm.focused_workspace_mut().unwrap();
            assert!(!workspace.tile);
        }

        {
            // Tiling state should be true
            wm.toggle_tiling().unwrap();
            let workspace = wm.focused_workspace_mut().unwrap();
            assert!(workspace.tile);
        }
    }

    #[test]
    fn a_socket_file_left_behind_is_replaced() -> eyre::Result<()> {
        let socket_path = PathBuf::from(format!("/tmp/komorebi-test-{}.sock", Uuid::new_v4()));
        std::fs::write(&socket_path, "left behind by a daemon that crashed")?;
        let _test_context = TestContext {
            socket_path: Some(socket_path.clone()),
        };

        let (_sender, receiver): (Sender<WindowManagerEvent>, Receiver<WindowManagerEvent>) =
            bounded(1);
        let wm = WindowManager::new(receiver, Some(socket_path.clone()))?;

        assert_eq!(
            wm.command_listener
                .local_addr()?
                .as_pathname()
                .map(PathBuf::from),
            Some(socket_path)
        );
        Ok(())
    }

    fn rect(left: i32, top: i32, right: i32, bottom: i32) -> Rect {
        Rect {
            left,
            top,
            right,
            bottom,
        }
    }

    /// The fake's 1920x1080 monitor and a second one to its right, with the windows `ids` open on
    /// the first and `id + 1` on the second for each of them
    fn two_monitors(ids: &[u32]) -> Arc<FakeBackend> {
        let fake = Arc::new(FakeBackend::with_one_monitor());
        fake.add_monitor(
            2,
            "Virtual-1",
            rect(1920, 0, 1920, 1080),
            rect(1920, 0, 1920, 1080),
        );

        for id in ids {
            fake.open_window(*id, "foot", "foot", "left");
            fake.open_window(id + 1, "foot", "foot", "right");
            fake.move_window_by_user(id + 1, rect(2000, 100, 800, 600));
        }

        fake
    }

    #[test]
    fn init_puts_each_open_window_on_the_monitor_it_is_on() -> eyre::Result<()> {
        let fake = two_monitors(&[601]);
        let (mut wm, _test_context) = setup_window_manager();

        with_backend(fake, || wm.init())?;

        assert_eq!(wm.monitors().len(), 2);
        assert_eq!(wm.known_window_ids.get(&601), Some(&(0, 0)));
        assert_eq!(wm.known_window_ids.get(&602), Some(&(1, 0)));
        assert_eq!(wm.focused_workspace_idx()?, 0);
        assert_eq!(wm.focused_container_idx()?, 0);
        assert_eq!(wm.focused_container()?.windows().len(), 1);
        assert_eq!(wm.focused_window()?.id, 601);
        Ok(())
    }

    #[test]
    fn retile_all_tiles_the_focused_workspace_of_every_monitor() -> eyre::Result<()> {
        let fake = two_monitors(&[611]);
        let (mut wm, _test_context) = setup_window_manager();

        with_backend(fake.clone(), || {
            wm.init()?;
            wm.retile_all(false)
        })?;

        // the default workspace and container padding are 5 each, and the second monitor has no
        // panel
        assert_eq!(fake.rect_of(611), Some(rect(10, 10, 1900, 1030)));
        assert_eq!(fake.rect_of(612), Some(rect(1930, 10, 1900, 1060)));
        Ok(())
    }

    #[test]
    fn reap_invalid_windows_drops_a_closed_window_and_retiles() -> eyre::Result<()> {
        let fake = Arc::new(FakeBackend::with_one_monitor());
        fake.open_window(621, "foot", "foot", "stays");
        fake.open_window(622, "foot", "foot", "closes");
        let (mut wm, _test_context) = setup_window_manager();

        with_backend(fake.clone(), || {
            wm.init()?;
            fake.close_window_now(622);
            wm.reap_invalid_windows()
        })?;

        assert!(!wm.focused_workspace()?.contains_window(622));
        assert_eq!(fake.rect_of(621), Some(rect(10, 10, 1900, 1030)));
        Ok(())
    }

    #[test]
    fn a_minimized_window_is_kept_out_of_the_workspace() -> eyre::Result<()> {
        let fake = Arc::new(FakeBackend::with_one_monitor());
        fake.open_window(631, "foot", "foot", "minimized");
        let (mut wm, _test_context) = setup_window_manager();

        with_backend(fake, || {
            wm.init()?;
            wm.extract_minimized_window(631)
        })?;

        assert!(wm.focused_workspace()?.is_empty());
        assert_eq!(wm.minimized_windows.get(&631), Some(&Window::from(631)));
        Ok(())
    }

    #[test]
    fn test_toggle_lock() {
        let (mut wm, _context) = setup_window_manager();

        {
            // Add monitor with default workspace to
            let mut m = monitor::new(
                0,
                Rect::default(),
                Rect::default(),
                "TestMonitor".to_string(),
                "TestDeviceID".to_string(),
            );

            let workspace = m.focused_workspace_mut().unwrap();

            // Create containers to add to the workspace
            for _ in 0..3 {
                let container = Container::default();
                workspace.add_container_to_back(container);
            }

            wm.monitors_mut().push_back(m);
        }

        {
            // Ensure container 2 is not locked
            let workspace = wm.focused_workspace_mut().unwrap();
            assert_eq!(workspace.focused_container_idx(), 2);
            assert!(!workspace.focused_container().unwrap().locked);
        }

        // Toggle lock on focused container
        wm.toggle_lock().unwrap();

        {
            // Ensure container 2 is locked
            let workspace = wm.focused_workspace_mut().unwrap();
            assert!(workspace.focused_container().unwrap().locked);
        }

        // Toggle lock on focused container
        wm.toggle_lock().unwrap();

        {
            // Ensure container 2 is not locked
            let workspace = wm.focused_workspace_mut().unwrap();
            assert!(!workspace.focused_container().unwrap().locked);
        }
    }

    #[test]
    fn test_float_window() {
        let (mut wm, _context) = setup_window_manager();

        {
            // Create a monitor
            let mut m = monitor::new(
                0,
                Rect::default(),
                Rect::default(),
                "TestMonitor".to_string(),
                "TestDeviceID".to_string(),
            );

            // Create a container
            let mut container = Container::default();

            // Add three windows to the container
            for i in 0..3 {
                container.windows_mut().push_back(Window::from(i));
            }

            // Should have 3 windows in the container
            assert_eq!(container.windows().len(), 3);

            // Add the container to the workspace
            let workspace = m.focused_workspace_mut().unwrap();
            workspace.add_container_to_back(container);

            // Add monitor to the window manager
            wm.monitors_mut().push_back(m);
        }

        // Add focused window to floating window list
        wm.float_window().ok();

        {
            let workspace = wm.focused_workspace().unwrap();
            let floating_windows = workspace.floating_windows();
            let container = workspace.focused_container().unwrap();

            // Hwnd 0 should be added to floating_windows
            assert_eq!(floating_windows[0].id, 0);

            // Should have a length of 1
            assert_eq!(floating_windows.len(), 1);

            // Should have 2 windows in the container
            assert_eq!(container.windows().len(), 2);

            // Should be focused on window 1
            assert_eq!(container.focused_window().unwrap().id, 1);
        }

        // Add focused window to floating window list
        wm.float_window().ok();

        {
            let workspace = wm.focused_workspace().unwrap();
            let floating_windows = workspace.floating_windows();
            let container = workspace.focused_container().unwrap();

            // Hwnd 1 should be added to floating_windows
            assert_eq!(floating_windows[1].id, 1);

            // Should have a length of 2
            assert_eq!(floating_windows.len(), 2);

            // Should have 1 window in the container
            assert_eq!(container.windows().len(), 1);

            // Should be focused on window 2
            assert_eq!(container.focused_window().unwrap().id, 2);
        }
    }

    #[test]
    fn test_float_nonexistent_window() {
        let (mut wm, _context) = setup_window_manager();

        {
            let mut m = monitor::new(
                0,
                Rect::default(),
                Rect::default(),
                "TestMonitor".to_string(),
                "TestDeviceID".to_string(),
            );

            // Add another workspace
            let new_workspace_index = m.new_workspace_idx();
            m.focus_workspace(new_workspace_index).unwrap();

            // Should have 2 workspaces
            assert_eq!(m.workspaces().len(), 2);

            // Add monitor to window manager
            wm.monitors_mut().push_back(m);
        }

        // Should return an error when trying to float a non-existent window
        let result = wm.float_window();
        assert!(
            result.is_err(),
            "Expected an error when trying to float a non-existent window"
        );
    }

    // #[test]
    // fn test_maximize_and_unmaximize_window() {
    //     let (mut wm, _context) = setup_window_manager();
    //
    //     {
    //         // Create a monitor
    //         let mut m = monitor::new(
    //             0,
    //             Rect::default(),
    //             Rect::default(),
    //             "TestMonitor".to_string(),
    //             "TestDeviceID".to_string(),
    //         );
    //
    //         // Create a container
    //         let mut container = Container::default();
    //
    //         // Add three windows to the container
    //         for i in 0..3 {
    //             container.windows_mut().push_back(Window::from(i));
    //         }
    //
    //         // Should have 3 windows in the container
    //         assert_eq!(container.windows().len(), 3);
    //
    //         // Add the container to the workspace
    //         let workspace = m.focused_workspace_mut().unwrap();
    //         workspace.add_container_to_back(container);
    //
    //         // Add monitor to the window manager
    //         wm.monitors_mut().push_back(m);
    //     }
    //
    //     {
    //         // No windows should be maximized
    //         let workspace = wm.focused_workspace().unwrap();
    //         let maximized_window = workspace.maximized_window;
    //         assert_eq!(maximized_window, None);
    //     }
    //
    //     // Maximize the focused window
    //     wm.maximize_window().ok();
    //
    //     {
    //         // Window 0 should be maximized
    //         let workspace = wm.focused_workspace().unwrap();
    //         let maximized_window = workspace.maximized_window;
    //         assert_eq!(maximized_window, Some(Window::from(0)));
    //     }
    //
    //     wm.unmaximize_window().ok();
    //
    //     {
    //         // No windows should be maximized
    //         let workspace = wm.focused_workspace().unwrap();
    //         let maximized_window = workspace.maximized_window;
    //         assert_eq!(maximized_window, None);
    //     }
    //
    //     // Focus container at index 1
    //     wm.focused_workspace_mut().unwrap().focus_container(1);
    //
    //     {
    //         // Focus the window at index 1
    //         let container = wm.focused_container_mut().unwrap();
    //         container.focus_window(1);
    //     }
    //
    //     // Maximize the focused window
    //     wm.maximize_window().ok();
    //
    //     {
    //         // Window 2 should be maximized
    //         let workspace = wm.focused_workspace().unwrap();
    //         let maximized_window = workspace.maximized_window;
    //         assert_eq!(maximized_window, Some(Window::from(2)));
    //     }
    //
    //     wm.unmaximize_window().ok();
    //
    //     {
    //         // No windows should be maximized
    //         let workspace = wm.focused_workspace().unwrap();
    //         let maximized_window = workspace.maximized_window;
    //         assert_eq!(maximized_window, None);
    //     }
    // }

    // #[test]
    // fn test_toggle_maximize() {
    //     let (mut wm, _context) = setup_window_manager();
    //
    //     {
    //         // Create a monitor
    //         let mut m = monitor::new(
    //             0,
    //             Rect::default(),
    //             Rect::default(),
    //             "TestMonitor".to_string(),
    //             "TestDevice".to_string(),
    //             "TestDeviceID".to_string(),
    //             Some("TestMonitorID".to_string()),
    //         );
    //
    //         // Create a container
    //         let mut container = Container::default();
    //
    //         // Add three windows to the container
    //         for i in 0..3 {
    //             container.windows_mut().push_back(Window::from(i));
    //         }
    //
    //         // Should have 3 windows in the container
    //         assert_eq!(container.windows().len(), 3);
    //
    //         // Add the container to the workspace
    //         let workspace = m.focused_workspace_mut().unwrap();
    //         workspace.add_container_to_back(container);
    //
    //         // Add monitor to the window manager
    //         wm.monitors_mut().push_back(m);
    //     }
    //
    //     // Toggle maximize on
    //     wm.toggle_maximize().ok();
    //
    //     {
    //         // Window 0 should be maximized
    //         let workspace = wm.focused_workspace().unwrap();
    //         let maximized_window = workspace.maximized_window;
    //         assert_eq!(maximized_window, Some(Window::from(0)));
    //     }
    //
    //     // Toggle maximize off
    //     wm.toggle_maximize().ok();
    //
    //     {
    //         // No windows should be maximized
    //         let workspace = wm.focused_workspace().unwrap();
    //         let maximized_window = workspace.maximized_window;
    //         assert_eq!(maximized_window, None);
    //     }
    // }

    // #[test]
    // fn test_toggle_maximize_nonexistent_window() {
    //     let (mut wm, _context) = setup_window_manager();
    //
    //     {
    //         // Create a monitor
    //         let mut m = monitor::new(
    //             0,
    //             Rect::default(),
    //             Rect::default(),
    //             "TestMonitor".to_string(),
    //             "TestDevice".to_string(),
    //             "TestDeviceID".to_string(),
    //             Some("TestMonitorID".to_string()),
    //         );
    //
    //         // Create a container
    //         let container = Container::default();
    //
    //         // Add the container to the workspace
    //         let workspace = m.focused_workspace_mut().unwrap();
    //         workspace.add_container_to_back(container);
    //
    //         // Add monitor to the window manager
    //         wm.monitors_mut().push_back(m);
    //     }
    //
    //     // Should return an error when trying to toggle maximize on a non-existent window
    //     let result = wm.toggle_maximize();
    //     assert!(
    //         result.is_err(),
    //         "Expected an error when trying to toggle maximize on a non-existent window"
    //     );
    // }

    #[test]
    fn test_monocle_on_and_monocle_off() {
        let (mut wm, _context) = setup_window_manager();

        {
            // Create a monitor
            let mut m = monitor::new(
                0,
                Rect::default(),
                Rect::default(),
                "TestMonitor".to_string(),
                "TestDeviceID".to_string(),
            );

            // Create a container
            let mut container = Container::default();

            // Add a window to the container
            container.windows_mut().push_back(Window::from(1));

            // Should have 1 window in the container
            assert_eq!(container.windows().len(), 1);

            // Add the container to the workspace
            let workspace = m.focused_workspace_mut().unwrap();
            workspace.add_container_to_back(container);

            // Add monitor to the window manager
            wm.monitors_mut().push_back(m);
        }

        // Move container to monocle container
        wm.monocle_on().ok();

        {
            // Container should be a monocle container
            let monocle_container = wm
                .focused_workspace()
                .unwrap()
                .monocle_container
                .as_ref()
                .unwrap();
            assert_eq!(monocle_container.windows().len(), 1);
            assert_eq!(monocle_container.windows()[0].id, 1);
        }

        {
            // Should not have any containers
            let container = wm.focused_workspace().unwrap();
            assert_eq!(container.containers().len(), 0);
        }

        // Move monocle container to regular container
        wm.monocle_off().ok();

        {
            // Should have 1 container in the workspace
            let container = wm.focused_workspace().unwrap();
            assert_eq!(container.containers().len(), 1);
            assert_eq!(container.containers()[0].windows()[0].id, 1);
        }

        {
            // No windows should be in the monocle container
            let monocle_container = &wm.focused_workspace().unwrap().monocle_container;
            assert_eq!(*monocle_container, None);
        }
    }

    #[test]
    fn test_monocle_on_and_off_nonexistent_container() {
        let (mut wm, _context) = setup_window_manager();

        {
            // Create a monitor
            let m = monitor::new(
                0,
                Rect::default(),
                Rect::default(),
                "TestMonitor".to_string(),
                "TestDeviceID".to_string(),
            );

            // Add monitor to the window manager
            wm.monitors_mut().push_back(m);
        }

        // Should return an error when trying to move a non-existent container to monocle
        let result = wm.monocle_on();
        assert!(
            result.is_err(),
            "Expected an error when trying to move a non-existent container to monocle"
        );

        // Should return an error when trying to restore a non-existent container from monocle
        let result = wm.monocle_off();
        assert!(
            result.is_err(),
            "Expected an error when trying to restore a non-existent container from monocle"
        );
    }

    #[test]
    fn test_toggle_monocle() {
        let (mut wm, _context) = setup_window_manager();

        {
            // Create a monitor
            let mut m = monitor::new(
                0,
                Rect::default(),
                Rect::default(),
                "TestMonitor".to_string(),
                "TestDeviceID".to_string(),
            );

            // Create a container
            let mut container = Container::default();

            // Add a window to the container
            container.windows_mut().push_back(Window::from(1));

            // Should have 1 window in the container
            assert_eq!(container.windows().len(), 1);

            // Add the container to the workspace
            let workspace = m.focused_workspace_mut().unwrap();
            workspace.add_container_to_back(container);

            // Add monitor to the window manager
            wm.monitors_mut().push_back(m);
        }

        // Toggle monocle on
        wm.toggle_monocle().ok();

        {
            // Container should be a monocle container
            let monocle_container = wm
                .focused_workspace()
                .unwrap()
                .monocle_container
                .as_ref()
                .unwrap();
            assert_eq!(monocle_container.windows().len(), 1);
            assert_eq!(monocle_container.windows()[0].id, 1);
        }

        {
            // Should not have any containers
            let container = wm.focused_workspace().unwrap();
            assert_eq!(container.containers().len(), 0);
        }

        // Toggle monocle off
        wm.toggle_monocle().ok();

        {
            // Should have 1 container in the workspace
            let container = wm.focused_workspace().unwrap();
            assert_eq!(container.containers().len(), 1);
            assert_eq!(container.containers()[0].windows()[0].id, 1);
        }

        {
            // No windows should be in the monocle container
            let monocle_container = &wm.focused_workspace().unwrap().monocle_container;
            assert_eq!(*monocle_container, None);
        }
    }

    #[test]
    fn test_toggle_monocle_nonexistent_container() {
        let (mut wm, _context) = setup_window_manager();

        {
            // Create a monitor
            let m = monitor::new(
                0,
                Rect::default(),
                Rect::default(),
                "TestMonitor".to_string(),
                "TestDeviceID".to_string(),
            );

            // Add monitor to the window manager
            wm.monitors_mut().push_back(m);
        }

        // Should return an error when trying to toggle monocle on a non-existent container
        let result = wm.toggle_monocle();
        assert!(
            result.is_err(),
            "Expected an error when trying to toggle monocle on a non-existent container"
        );
    }

    #[test]
    fn test_ensure_named_workspace_for_monitor() {
        let (mut wm, _context) = setup_window_manager();

        {
            // Create a monitor
            let m = monitor::new(
                0,
                Rect::default(),
                Rect::default(),
                "TestMonitor".to_string(),
                "TestDeviceID".to_string(),
            );

            // Add the monitor to the window manager
            wm.monitors_mut().push_back(m);
        }

        {
            // Create a monitor
            let m = monitor::new(
                1,
                Rect::default(),
                Rect::default(),
                "TestMonitor1".to_string(),
                "TestDeviceID1".to_string(),
            );

            // Add the monitor to the window manager
            wm.monitors_mut().push_back(m);
        }

        // Workspace names list
        let mut workspace_names = vec!["Workspace".to_string(), "Workspace1".to_string()];

        // Ensure workspaces for monitor 1
        wm.ensure_named_workspaces_for_monitor(1, &workspace_names)
            .ok();

        {
            // Monitor 1 should have 2 workspaces with names "Workspace" and "Workspace1"
            let monitor = wm.monitors().get(1).unwrap();
            let workspaces = monitor.workspaces();
            assert_eq!(workspaces.len(), workspace_names.len());
            for (i, workspace) in workspaces.iter().enumerate() {
                assert_eq!(workspace.name, Some(workspace_names[i].clone()));
            }
        }

        // Add more workspaces to list
        workspace_names.push("Workspace2".to_string());
        workspace_names.push("Workspace3".to_string());

        // Ensure workspaces for monitor 0
        wm.ensure_named_workspaces_for_monitor(0, &workspace_names)
            .ok();

        {
            // Monitor 0 should have 4 workspaces with names "Workspace", "Workspace1",
            // "Workspace2" and "Workspace3"
            let monitor = wm.monitors().front().unwrap();
            let workspaces = monitor.workspaces();
            assert_eq!(workspaces.len(), workspace_names.len());
            for (i, workspace) in workspaces.iter().enumerate() {
                assert_eq!(workspace.name, Some(workspace_names[i].clone()));
            }
        }
    }

    #[test]
    fn test_add_window_handle_to_move_based_on_workspace_rule() {
        let (wm, _context) = setup_window_manager();

        // Mock Data representing a window and its workspace/movement details
        let window_title = String::from("TestWindow");
        let window_id = 12345;
        let origin_monitor_idx = 0;
        let origin_workspace_idx = 0;
        let target_monitor_idx = 2;
        let target_workspace_idx = 3;
        let floating = false;

        // Empty vector to hold workspace rule enforcement operations
        let mut to_move: Vec<EnforceWorkspaceRuleOp> = Vec::new();

        // Call the function to add a window movement operation based on workspace rules
        wm.add_window_handle_to_move_based_on_workspace_rule(
            &window_title,
            window_id,
            origin_monitor_idx,
            origin_workspace_idx,
            target_monitor_idx,
            target_workspace_idx,
            floating,
            &mut to_move,
        );

        // Verify that the vector contains the expected operation with the correct values
        assert_eq!(to_move.len(), 1);
        let op = &to_move[0];
        assert_eq!(op.window_id, window_id); // 12345
        assert_eq!(op.origin_monitor_idx, origin_monitor_idx); // 0
        assert_eq!(op.origin_workspace_idx, origin_workspace_idx); // 0
        assert_eq!(op.target_monitor_idx, target_monitor_idx); // 2
        assert_eq!(op.target_workspace_idx, target_workspace_idx); // 3
        assert_eq!(op.floating, floating); // false
    }
}
