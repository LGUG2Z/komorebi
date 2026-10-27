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
//! [`listen_for_commands`]: crate::process_command::listen_for_commands

use crate::DATA_DIR;
use crate::LibraryError;
use crate::SUBSCRIPTION_SOCKETS;
use crate::backend::backend;
use crate::container::Container;
use crate::core::Arrangement;
use crate::core::Axis;
use crate::core::CycleDirection;
use crate::core::DefaultLayout;
use crate::core::Layout;
use crate::core::OperationDirection;
use crate::core::Rect;
use crate::core::Sizing;
use crate::desktop;
use crate::monitor::Monitor;
use crate::ring::Ring;
use crate::window::Window;
use crate::window_manager_event::WindowManagerEvent;
use crate::workspace::Workspace;
use color_eyre::eyre;
use color_eyre::eyre::OptionExt;
use color_eyre::eyre::bail;
use crossbeam_channel::Receiver;
use std::collections::HashMap;
use std::io::ErrorKind;
use std::net::Shutdown;
use std::num::NonZeroUsize;
use std::os::unix::net::UnixListener;
use std::os::unix::net::UnixStream;
use std::path::PathBuf;

/// A tile cannot be resized to less than this fraction of the work area (or less than it already is)
const MINIMUM_TILE_FRACTION: i32 = 10;

#[derive(Debug)]
pub struct WindowManager {
    pub monitors: Ring<Monitor>,
    /// The socket the daemon takes messages on, `komorebi.sock` in [`struct@DATA_DIR`] unless
    /// [`WindowManager::new`] was given another path
    pub command_listener: UnixListener,
    /// How many pixels `ResizeWindowEdge` and `ResizeWindowAxis` resize by
    pub resize_delta: i32,
    /// Whether the pointer is moved to the window that is given the focus, which is the default
    pub mouse_follows_focus: bool,
    pub work_area_offset: Option<Rect>,
    /// The events the backend has reported and the window manager has yet to handle, from
    /// [`crate::window_manager_event_listener`]
    pub incoming_events: Receiver<WindowManagerEvent>,
    pub minimized_windows: HashMap<u32, Window>,
    /// Maps each known window id to the (monitor, workspace) index pair managing it
    pub known_window_ids: HashMap<u32, (usize, usize)>,
}

impl_ring_elements!(WindowManager, Monitor);

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
            command_listener: listener,
            resize_delta: 50,
            mouse_follows_focus: true,
            work_area_offset: None,
            incoming_events: incoming,
            minimized_windows: HashMap::new(),
            known_window_ids: Default::default(),
        })
    }

    /// Loads the monitors and the windows that are already open from the backend
    /// ([`desktop`])
    #[tracing::instrument(skip(self))]
    pub fn init(&mut self) -> Result<(), LibraryError> {
        tracing::info!("initializing");
        desktop::load_monitor_information(self)?;
        desktop::load_workspace_information(self)?;
        // the windows that were open already are in containers now; an event about one of them
        // (the first thing to happen may be that it closes) has to find it
        self.update_known_window_ids();
        Ok(())
    }

    /// Reserves the place on the `direction` side of the focused container for the next window
    /// that opens on the focused workspace ([`Workspace::preselect_container_idx`])
    ///
    /// The grid layout has no preselection: there, and where there is no container on that side,
    /// nothing is reserved.
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

        let new_idx = workspace.new_idx_for_direction(direction);

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
    /// set
    ///
    /// Nothing is retiled: the focus moves, the windows stay where they are.
    ///
    /// # Errors
    ///
    /// - There is no focused workspace, or no container on that side of the focused one.
    /// - The backend refuses to focus the window.
    #[tracing::instrument(skip(self))]
    pub fn focus_container_in_direction(
        &mut self,
        direction: OperationDirection,
    ) -> eyre::Result<()> {
        let mouse_follows_focus = self.mouse_follows_focus;

        let workspace = self.focused_workspace()?;

        tracing::info!("focusing container");

        let new_idx = workspace.new_idx_for_direction(direction);

        // if there is no container in that direction for this workspace
        match new_idx {
            None => bail!("there is no container in this direction"),
            Some(idx) => {
                let workspace = self.focused_workspace_mut()?;
                workspace.focus_container(idx);
            }
        }

        let ws = self.focused_workspace_mut()?;
        if !ws.is_empty()
            && let Ok(focused_window) = self.focused_window()
        {
            focused_window.focus(mouse_follows_focus)?;
        }

        Ok(())
    }

    /// Swaps the focused container with the one whose tile is on the `direction` side of it, so
    /// that the focused window takes that tile, and tiles the workspace again
    ///
    /// The container keeps the focus, and the pointer follows it if
    /// [`WindowManager::mouse_follows_focus`] is set. A lock stays with its index: a container
    /// that is swapped onto the index of a locked one is locked in its place
    /// ([`Workspace::swap_containers`]).
    ///
    /// # Errors
    ///
    /// - There is no focused workspace, or no container on that side of the focused one.
    /// - The workspace cannot be tiled, or the backend refuses to focus the window.
    #[tracing::instrument(skip(self))]
    pub fn move_container_in_direction(
        &mut self,
        direction: OperationDirection,
    ) -> eyre::Result<()> {
        let workspace = self.focused_workspace()?;

        tracing::info!("moving container");

        let origin_container_idx = workspace.focused_container_idx();
        let target_container_idx = workspace.new_idx_for_direction(direction);

        match target_container_idx {
            None => bail!("there is no container in this direction"),
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

        if follow_focus
            && trigger_focus
            && let Ok(window) = self.focused_window_mut()
        {
            window.focus(mouse_follows_focus)?;
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

    /// Stops the daemon: connects to every subscriber and closes the connection without writing
    /// anything, removes the command socket, lets the backend give up what it set up in the
    /// compositor, and exits the process with status 0
    ///
    /// # Errors
    ///
    /// A connection to a subscriber cannot be shut down. The daemon does not stop.
    #[tracing::instrument(skip(self))]
    pub fn stop(&mut self) -> eyre::Result<()> {
        tracing::info!("received stop command, terminating process");

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

    /// Resizes the focused container by `delta` pixels on its `direction` side, by giving it a
    /// resize adjustment that the layout applies, and tiles the workspace again if `update` is set
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
        let focused_monitor_work_area = self.focused_monitor_work_area()?;
        let workspace = self.focused_workspace_mut()?;

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
                    let min_height = focused_monitor_work_area.bottom / MINIMUM_TILE_FRACTION;

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
                        calculate(&dimensions)
                            .iter()
                            .zip(now.iter())
                            .all(|(tile, before)| {
                                tile.right >= min_width.min(before.right)
                                    && tile.bottom >= min_height.min(before.bottom)
                            })
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

        Ok(())
    }

    /// Tiles the focused workspace of every monitor, without the resize adjustments of its
    /// containers unless `preserve_resize_dimensions` is set
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

            let workspace = monitor
                .focused_workspace_mut()
                .ok_or_eyre("there is no workspace")?;

            // Reset any resize adjustments if we want to force a retile
            if !preserve_resize_dimensions {
                workspace.resize_dimensions.fill(None);
            }

            workspace.update()?;
        }

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

    /// Gives the focus to the window of the next (or the previous) container of the focused
    /// workspace, wrapping around at the ends, and the pointer follows it if
    /// [`WindowManager::mouse_follows_focus`] is set
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

        let mouse_follows_focus = self.mouse_follows_focus;

        let workspace = self.focused_workspace_mut()?;

        let new_idx = workspace
            .new_idx_for_cycle_direction(direction)
            .ok_or_eyre("this is not a valid direction from the current position")?;

        workspace.focus_container(new_idx);

        self.focused_window_mut()?.focus(mouse_follows_focus)?;

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
    /// - The workspace cannot be tiled, or the backend refuses to focus the window.
    #[tracing::instrument(skip(self))]
    pub fn move_container_in_cycle_direction(
        &mut self,
        direction: CycleDirection,
    ) -> eyre::Result<()> {
        let workspace = self.focused_workspace_mut()?;

        tracing::info!("moving container");

        let current_idx = workspace.focused_container_idx();
        let new_idx = workspace
            .new_idx_for_cycle_direction(direction)
            .ok_or_eyre("this is not a valid direction from the current position")?;

        workspace.swap_containers(current_idx, new_idx);
        workspace.focus_container(new_idx);
        self.update_focused_workspace(self.mouse_follows_focus, true)
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
            }
        }

        if self.known_window_ids != known_window_ids {
            // Store new window_ids
            self.known_window_ids = known_window_ids;
        }
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

            let workspace = m.focused_workspace_mut().unwrap();

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
}
