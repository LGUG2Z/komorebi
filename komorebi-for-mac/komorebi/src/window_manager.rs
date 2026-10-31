use crate::AccessibilityUiElement;
use crate::CoreFoundationRunLoop;
use crate::DATA_DIR;
use crate::LibraryError;
use crate::REGEX_IDENTIFIERS;
use crate::SUBSCRIPTION_SOCKETS;
use crate::UNMANAGED_WINDOW_IDS;
use crate::WINDOW_RESTORE_POSITIONS;
use crate::WORKSPACE_MATCHING_RULES;
use crate::accessibility::AccessibilityApi;
use crate::animation;
use crate::animation::AnimationPrefix;
use crate::animation::transition;
use crate::animation::transition::SlideDirection;
use crate::animation::workspace_transition::WorkspaceTransition;
use crate::application::Application;
use crate::border_manager;
use crate::border_manager::BorderLook;
use crate::container::Container;
use crate::core::Arrangement;
use crate::core::Axis;
use crate::core::CrossBoundaryBehaviour;
use crate::core::CycleDirection;
use crate::core::DefaultLayout;
use crate::core::Layout;
use crate::core::MonocleFocusBehaviour;
use crate::core::MoveBehaviour;
use crate::core::OperationBehaviour;
use crate::core::OperationDirection;
use crate::core::Placement;
use crate::core::Rect;
use crate::core::Sizing;
use crate::core::WindowContainerBehaviour;
use crate::core::WindowHidingPosition;
use crate::core::WindowKind;
use crate::core::WindowManagementBehaviour;
use crate::core::config_generation::MatchingRule;
use crate::current_space_id;
use crate::lockable_sequence::Lockable;
use crate::macos_api::MacosApi;
use crate::monitor::Monitor;
use crate::pan::Pan;
use crate::ring::Ring;
use crate::settle;
use crate::skylight::BatchedUpdates;
use crate::static_config::StaticConfig;
use crate::window::AdhocWindow;
use crate::window::Snapping;
use crate::window::Window;
use crate::window::dragged_window;
use crate::window::live_resize_begins;
use crate::window::live_resize_ends;
use crate::window::should_act_individual;
use crate::window_manager_event::ManualNotification;
use crate::window_manager_event::SystemNotification;
use crate::window_manager_event::WindowManagerEvent;
use crate::window_manager_event_listener;
use crate::workspace::Workspace;
use crate::workspace::WorkspaceLayer;
use color_eyre::eyre;
use color_eyre::eyre::OptionExt;
use color_eyre::eyre::bail;
use crossbeam_channel::Receiver;
use hotwatch::Hotwatch;
use komorebi_layouts::scrolling::model::ScrollingSpace;
use objc2_core_foundation::CFRetained;
use objc2_core_foundation::CFRunLoop;
use parking_lot::Mutex;
use std::collections::HashMap;
use std::collections::HashSet;
use std::collections::VecDeque;
use std::collections::hash_map::Entry;
use std::io::ErrorKind;
use std::net::Shutdown;
use std::num::NonZeroUsize;
use std::os::unix::net::UnixListener;
use std::os::unix::net::UnixStream;
use std::path::PathBuf;
use std::sync::Arc;

/// What the workspace was when somebody took hold of an edge of a window, and what has been done
/// with it since, so that the windows next to the edge can follow it as it is dragged
///
/// Every step of the drag, and the end of it, is worked out from this and not from the step
/// before: a drag ends the same with its steps as without them, and one that is taken back to
/// where it began puts the windows next to it back.
#[derive(Debug)]
pub struct LiveResize {
    /// The window that is being dragged
    pub window: u32,
    /// How many containers there were, since a window that opens or closes in the middle of a drag
    /// changes what the layout is made of, and what is kept is no longer about it
    pub containers: usize,
    pub resize_dimensions: Vec<Option<Rect>>,
    pub latest_layout: Vec<Rect>,
    pub scrolling: Option<ScrollingSpace<String>>,
    /// Whether the windows next to the edge have been laid out for a step, which is to be put
    /// right when the drag ends, whatever it comes to
    pub stepped: bool,
    /// The windows next to the dragged one and their apps, which are not waited for for as long as
    /// they are by default while the drag is on
    timeouts: Vec<AccessibilityUiElement>,
}

/// How long, in seconds, the apps of the windows next to a dragged edge are waited for, which is
/// a good deal less than the time that it takes for an app that does not answer to be given up on:
/// a step is no use to anybody by then
const LIVE_RESIZE_TIMEOUT: f32 = 0.25;

/// Say how long the apps in `elements` are waited for. Nought is what they are waited for by
/// default.
fn set_messaging_timeouts(elements: &[AccessibilityUiElement], seconds: f32) {
    for element in elements {
        let _ = AccessibilityApi::set_messaging_timeout(element, seconds);
    }
}

#[derive(Debug)]
pub struct WindowManager {
    pub monitors: Ring<Monitor>,
    pub monitor_usr_idx_map: HashMap<usize, usize>,
    pub applications: HashMap<i32, Application>,
    pub run_loop: CoreFoundationRunLoop,
    pub command_listener: UnixListener,
    pub space_id: Option<u64>,
    pub is_paused: bool,
    pub resize_delta: i32,
    pub hotwatch: Hotwatch,
    pub unmanaged_window_operation_behaviour: OperationBehaviour,
    pub window_management_behaviour: WindowManagementBehaviour,
    pub cross_monitor_move_behaviour: MoveBehaviour,
    pub cross_boundary_behaviour: CrossBoundaryBehaviour,
    pub monocle_focus_behaviour: MonocleFocusBehaviour,
    pub mouse_follows_focus: bool,
    pub work_area_offset: Option<Rect>,
    pub incoming_events: Receiver<WindowManagerEvent>,
    pub minimized_windows: HashMap<u32, Window>,
    pub pending_move_op: Arc<Option<(usize, usize, u32)>>,
    pub pending_resize_op: Arc<Option<(u32, Option<Rect>)>>,
    /// The workspace as it was when an edge that the windows next to it follow was taken hold of
    pub live_resize: Option<LiveResize>,
    /// The fingers of a gesture that pans the strip of the Scrolling layout, while they are down
    pub(crate) pan: Option<Pan>,
    pub already_moved_window_handles: Arc<Mutex<HashSet<u32>>>,
    /// Maps each known window id to the (monitor, workspace) index pair managing it
    pub known_window_ids: HashMap<u32, (usize, usize)>,
}

impl_ring_elements!(WindowManager, Monitor);

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
    #[allow(clippy::field_reassign_with_default)]
    pub fn new(
        run_loop: &CFRetained<CFRunLoop>,
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

        // TODO: undo this when we get config
        let mut behaviour = WindowManagementBehaviour::default();
        behaviour.toggle_float_placement = Placement::CenterAndResize;

        Ok(Self {
            monitors: Ring::default(),
            monitor_usr_idx_map: HashMap::new(),
            applications: Default::default(),
            run_loop: CoreFoundationRunLoop(run_loop.clone()),
            command_listener: listener,
            space_id: current_space_id(),
            is_paused: false,
            resize_delta: 50,
            hotwatch: Hotwatch::new()?,
            unmanaged_window_operation_behaviour: Default::default(),
            window_management_behaviour: behaviour,
            cross_monitor_move_behaviour: Default::default(),
            cross_boundary_behaviour: Default::default(),
            monocle_focus_behaviour: MonocleFocusBehaviour::default(),
            mouse_follows_focus: true,
            work_area_offset: None,
            incoming_events: incoming,
            minimized_windows: HashMap::new(),
            pending_move_op: Arc::new(None),
            pending_resize_op: Arc::new(None),
            live_resize: None,
            pan: None,
            already_moved_window_handles: Default::default(),
            known_window_ids: Default::default(),
        })
    }

    #[tracing::instrument(skip(self))]
    pub fn init(&mut self) -> Result<(), LibraryError> {
        tracing::info!("initializing");
        MacosApi::load_monitor_information(self)?;
        MacosApi::load_workspace_information(self)
    }

    pub fn application(&mut self, process_id: i32) -> Result<&mut Application, LibraryError> {
        match self.applications.entry(process_id) {
            Entry::Occupied(entry) => Ok(entry.into_mut()),
            Entry::Vacant(vacant) => {
                let mut application = Application::new(process_id)?;
                application.observe(&self.run_loop, None);
                Ok(vacant.insert(application))
            }
        }
    }

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
            let focused_window_id =
                MacosApi::foreground_window_id().ok_or_eyre("no foreground window")?;
            let focused_rect = Rect::from(MacosApi::window_rect(
                MacosApi::foreground_window()
                    .ok_or_eyre("no foreground window")?
                    .as_ref(),
            )?);

            match direction {
                OperationDirection::Left => {
                    let mut windows_in_direction = focused_workspace
                        .floating_windows()
                        .iter()
                        .enumerate()
                        .flat_map(|(idx, w)| {
                            (w.id != focused_window_id).then_some(
                                MacosApi::window_rect(&w.element)
                                    .ok()
                                    .map(|r| (idx, Rect::from(r))),
                            )
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
                            (w.id != focused_window_id).then_some(
                                MacosApi::window_rect(&w.element)
                                    .ok()
                                    .map(|r| (idx, Rect::from(r))),
                            )
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
                            (w.id != focused_window_id).then_some(
                                MacosApi::window_rect(&w.element)
                                    .ok()
                                    .map(|r| (idx, Rect::from(r))),
                            )
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
                            (w.id != focused_window_id).then_some(
                                MacosApi::window_rect(&w.element)
                                    .ok()
                                    .map(|r| (idx, Rect::from(r))),
                            )
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
            // if let Some(window) = focused_workspace.maximized_window() {
            //     window.focus(mouse_follows_focus)?;
            //     cross_monitor_monocle_or_max = true;
            // } else
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
            if ws.is_empty() {
                // TODO: figure out if we need to do this on macos
                // This is to remove focus from the previous monitor
                // let desktop_window = Window::from(MacosApi::desktop_window()?);
                //
                // match MacosApi::raise_and_focus_window(desktop_window.id) {
                //     Ok(()) => {}
                //     Err(error) => {
                //         tracing::warn!("{} {}:{}", error, file!(), line!());
                //     }
                // }
            } else if ws.layer == WorkspaceLayer::Floating && !ws.floating_windows().is_empty() {
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

        Ok(())
    }

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
            if ws.is_empty() {
                // TODO: figure out if we need to do this on macOS
                // This is to remove focus from the previous monitor
                // let desktop_window = Window::from(WindowsApi::desktop_window()?);
                //
                // match WindowsApi::raise_and_focus_window(desktop_window.hwnd) {
                //     Ok(()) => {}
                //     Err(error) => {
                //         tracing::warn!("{} {}:{}", error, file!(), line!());
                //     }
                // }
            } else if ws.layer == WorkspaceLayer::Floating && !ws.floating_windows().is_empty() {
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

        Ok(())
    }

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

        let focused_window_id =
            MacosApi::foreground_window_id().ok_or_eyre("no foreground window")?;
        for window in focused_workspace.floating_windows().iter() {
            if window.id == focused_window_id {
                let mut rect = Rect::from(MacosApi::window_rect(&window.element)?);
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
                    MacosApi::center_cursor_in_rect(&rect)?;
                }

                break;
            }
        }

        Ok(())
    }

    #[tracing::instrument(skip(self))]
    pub fn move_container_in_direction(
        &mut self,
        direction: OperationDirection,
    ) -> eyre::Result<()> {
        self.handle_unmanaged_window_behaviour()?;

        let workspace = self.focused_workspace()?;
        let workspace_idx = self.focused_workspace_idx()?;

        // removing this messes up the monitor / container / window index somewhere
        // and results in the wrong window getting moved across the monitor boundary
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

                // TODO: figure out monitor DPI differences on macOS
                // let a = self
                //     .focused_monitor()
                //     .ok_or_eyre("there is no monitor focused monitor")?
                //     .id;
                // let b = self
                //     .monitors_mut()
                //     .get_mut(origin_monitor_idx)
                //     .ok_or_eyre("there is no monitor at this index")?
                //     .id;

                // if !WindowsApi::monitors_have_same_dpi(a, b)? {
                //     self.update_focused_workspace(self.mouse_follows_focus, true)?;
                // }
            }
            Some(new_idx) => {
                let workspace = self.focused_workspace_mut()?;

                // a column of the Scrolling layout moves whole, and its tiles move within it,
                // which is not a swap of two containers
                if !(workspace.is_scrolling() && workspace.move_scrolling(direction)) {
                    workspace.swap_containers(origin_container_idx, new_idx);
                    workspace.focus_container(new_idx);
                }
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
            // the work area. Focus that window directly so the cursor follows the monocle
            // window's actual on-screen position, not a hidden tiling container at the hiding
            // corner.
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

    pub fn extract_minimized_window(&mut self, window_id: u32) -> eyre::Result<()> {
        let workspace = self.focused_workspace_mut()?;
        let window = workspace.remove_window(window_id)?;
        self.minimized_windows.insert(window_id, window);

        Ok(())
    }

    pub fn reap_invalid_windows_for_application(&mut self, process_id: i32) -> eyre::Result<()> {
        let application = self.application(process_id)?;
        let mut valid_window_ids = vec![];
        if let Some(elements) = application.window_elements() {
            for element in elements {
                if let Ok(window_id) = AccessibilityApi::window_id(&element) {
                    valid_window_ids.push(window_id);
                }
            }
        }

        let mut reaped_count = 0;

        let focused_workspace = self.focused_workspace_mut()?;
        reaped_count += focused_workspace
            .reap_invalid_windows_for_application(process_id, &valid_window_ids)?;

        if reaped_count > 0 {
            tracing::debug!("reaped {reaped_count} invalid window(s)");
        }

        self.update_focused_workspace(false, false)
    }

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

            let window_hiding_position = workspace.globals.window_hiding_position;
            if changed_focus && let Some(container) = workspace.focused_container_mut() {
                container.load_focused_window(window_hiding_position)?;
                if let Some(window) = container.focused_window() {
                    window.focus(mouse_follows_focus)?;
                }
            }

            self.update_focused_workspace(mouse_follows_focus, false)?;
        }

        Ok(())
    }

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

    #[tracing::instrument(skip(self))]
    pub fn stop(&mut self, ignore_restore: bool) -> eyre::Result<()> {
        tracing::info!(
            "received stop command, restoring all hidden windows and terminating process"
        );

        // TODO: figure out if we wanna do state restores in macOS
        // let state = &State::from(&*self);
        // std::fs::write(
        //     DATA_DIR.join("komorebi.state.json"),
        //     serde_json::to_string_pretty(&state)?,
        // )?;

        self.restore_all_windows(ignore_restore)?;

        let sockets = SUBSCRIPTION_SOCKETS.lock();
        for path in (*sockets).values() {
            if let Ok(stream) = UnixStream::connect(path) {
                stream.shutdown(Shutdown::Both)?;
            }
        }

        let socket = DATA_DIR.join("komorebi.sock");
        let _ = std::fs::remove_file(socket);

        std::process::exit(0)
    }

    #[tracing::instrument(skip(self))]
    pub fn restore_all_windows(&mut self, ignore_restore: bool) -> eyre::Result<()> {
        tracing::info!("restoring all hidden windows");

        for monitor in self.monitors_mut() {
            for workspace in monitor.workspaces_mut() {
                for containers in workspace.containers_mut() {
                    for window in containers.windows_mut() {
                        if !ignore_restore && let Err(error) = window.restore() {
                            tracing::error!("failed to restore window: {}", error);
                        }
                    }
                }
            }
        }

        Ok(())
    }

    #[tracing::instrument(skip(self))]
    pub fn cycle_container_window_in_direction(
        &mut self,
        direction: CycleDirection,
    ) -> eyre::Result<()> {
        tracing::info!("cycling container windows");

        let mouse_follows_focus = self.mouse_follows_focus;
        let window_hiding_position = self.focused_workspace()?.globals.window_hiding_position;

        let container = self.focused_container_mut()?;

        let len = NonZeroUsize::new(container.windows().len())
            .ok_or_eyre("there must be at least one window in a container")?;

        if len.get() == 1 {
            eyre::bail!("there is only one window in this container");
        }

        let current_idx = container.focused_window_idx();
        let next_idx = direction.next_idx(current_idx, len);

        container.focus_window(next_idx);
        container.load_focused_window(window_hiding_position)?;

        if let Some(window) = container.focused_window() {
            window.focus(mouse_follows_focus)?;
        }

        self.update_focused_workspace(mouse_follows_focus, true)
    }
    #[tracing::instrument(skip(self))]
    pub fn focus_workspace(&mut self, idx: usize) -> eyre::Result<()> {
        tracing::info!("focusing workspace");

        let mouse_follows_focus = self.mouse_follows_focus;

        let monitor_idx = self.focused_monitor_idx();

        if let Some(transition) = self.workspace_transition(idx) {
            let monitor = self
                .focused_monitor_mut()
                .ok_or_eyre("there is no workspace")?;

            transition.run(
                monitor,
                |monitor| {
                    // behind the ghosts, the workspace that is left goes away at once, and the one
                    // that is entered waits for them to slide in
                    let _batched = BatchedUpdates::begin();
                    // the pictures have borders of their own
                    border_manager::hide_borders_on(monitor_idx);
                    monitor.focus_workspace(idx)?;
                    monitor.hide_unfocused_workspaces()
                },
                |monitor| monitor.restore_focused_workspace(mouse_follows_focus),
            )?;

            return self.update_focused_workspace(false, true);
        }

        let monitor = self
            .focused_monitor_mut()
            .ok_or_eyre("there is no workspace")?;

        monitor.focus_workspace(idx)?;
        monitor.load_focused_workspace(mouse_follows_focus)?;

        self.update_focused_workspace(false, true)
    }

    /// The borders that the windows of a workspace that is entered on the focused monitor are going
    /// to have, as the border manager is going to give them
    fn entering_borders(workspace: &Workspace) -> Vec<(u32, BorderLook)> {
        let mut borders = vec![];

        if !workspace.tile {
            return borders;
        }

        if let Some(window) = workspace
            .monocle_container
            .as_ref()
            .and_then(|monocle| monocle.focused_window())
        {
            borders
                .extend(border_manager::look_of(WindowKind::Monocle).map(|look| (window.id, look)));
        } else if workspace.layer != WorkspaceLayer::Floating {
            for (idx, container) in workspace.containers().iter().enumerate() {
                let Some(window) = container.focused_window() else {
                    continue;
                };

                let kind = if idx != workspace.focused_container_idx() {
                    if container.locked {
                        WindowKind::UnfocusedLocked
                    } else {
                        WindowKind::Unfocused
                    }
                } else if container.windows().len() > 1 {
                    WindowKind::Stack
                } else {
                    WindowKind::Single
                };

                borders.extend(border_manager::look_of(kind).map(|look| (window.id, look)));
            }
        }

        for window in workspace.floating_windows() {
            borders.extend(
                border_manager::look_of(WindowKind::Unfocused).map(|look| (window.id, look)),
            );
        }

        borders
    }

    /// What it takes to slide the workspace that is focused out of the focused monitor and the one
    /// at `idx` in, or `None` for when the switch is to be done at once: animations are not asked
    /// for, there is nothing to slide, or a window cannot be pictured
    fn workspace_transition(&self, idx: usize) -> Option<WorkspaceTransition> {
        let motion = animation::motion_for(AnimationPrefix::Workspace)?;

        let monitor = self.focused_monitor()?;
        let from_idx = monitor.focused_workspace_idx();
        let direction = SlideDirection::between(from_idx, idx)?;

        let leaving = monitor.workspaces().get(from_idx)?;
        let entering = monitor.workspaces().get(idx);

        // a window that is maximized is not hidden and shown like the rest
        if leaving.maximized_window.is_some()
            || entering.is_some_and(|workspace| workspace.maximized_window.is_some())
        {
            return None;
        }

        let outgoing: Vec<(u32, Rect)> = leaving
            .visible_windows()
            .into_iter()
            .flatten()
            .filter_map(|window| {
                MacosApi::window_rect(&window.element)
                    .ok()
                    .map(|rect| (window.id, Rect::from(rect)))
            })
            .collect();

        // where the windows of the workspace that is entered were when it was left, which is
        // where they are put back
        let restore_positions = WINDOW_RESTORE_POSITIONS.lock();
        let incoming: Vec<(u32, Rect)> = entering
            .map(|workspace| workspace.visible_windows())
            .unwrap_or_default()
            .into_iter()
            .flatten()
            .filter_map(|window| {
                restore_positions
                    .get(&window.id)
                    .map(|rect| (window.id, Rect::from(*rect)))
            })
            .collect();
        drop(restore_positions);

        let plan = transition::plan(direction, monitor.size, &outgoing, &incoming)?;

        // the borders that the windows have now, and the ones that the windows that are entered are
        // going to have, which the border manager decides on once it gets to it
        let mut borders: HashMap<u32, BorderLook> = outgoing
            .iter()
            .filter_map(|(id, _)| border_manager::window_look(*id).map(|look| (*id, look)))
            .collect();

        if let Some(workspace) = entering {
            borders.extend(Self::entering_borders(workspace));
        }

        WorkspaceTransition::capture(plan, motion, &borders)
    }

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

    #[tracing::instrument(skip(self))]
    pub fn monocle_on(&mut self) -> eyre::Result<()> {
        tracing::info!("enabling monocle");

        let hiding_position = self
            .focused_monitor()
            .ok_or_eyre("there is no monitor")?
            .window_hiding_position;

        let workspace = self.focused_workspace_mut()?;
        workspace.new_monocle_container()?;

        for container in workspace.containers_mut() {
            container.hide(hiding_position, None, None)?;
        }

        Ok(())
    }

    #[tracing::instrument(skip(self))]
    pub fn monocle_off(&mut self) -> eyre::Result<()> {
        tracing::info!("disabling monocle");

        let workspace = self.focused_workspace_mut()?;

        for container in workspace.containers_mut() {
            container.restore()?;
        }

        workspace.reintegrate_monocle_container()
    }

    #[tracing::instrument(skip(self))]
    pub fn cycle_monocle(&mut self, direction: CycleDirection) -> eyre::Result<()> {
        tracing::info!("cycling monocle container");

        if self.focused_workspace()?.containers().is_empty() {
            return Ok(());
        }

        let hiding_position = self
            .focused_monitor()
            .ok_or_eyre("there is no monitor")?
            .window_hiding_position;

        self.focused_workspace_mut()?
            .cycle_monocle_container(direction)?;

        for container in self.focused_workspace_mut()?.containers_mut() {
            container.hide(hiding_position, None, None)?;
        }

        self.update_focused_workspace(true, true)
    }

    #[tracing::instrument(skip(self))]
    pub fn toggle_float(&mut self, force_float: bool) -> eyre::Result<()> {
        let window_id = MacosApi::foreground_window_id().ok_or_eyre("no foreground window")?;

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

    #[tracing::instrument(skip(self))]
    pub fn unfloat_window(&mut self) -> eyre::Result<()> {
        tracing::info!("unfloating window");

        let workspace = self.focused_workspace_mut()?;
        workspace.new_container_for_floating_window()
    }

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
                let focused_window_id =
                    MacosApi::foreground_window_id().ok_or_eyre("no foreground window")?;

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
                        let mut rect = Rect::from(MacosApi::window_rect(&window.element)?);
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
                            MacosApi::center_cursor_in_rect(&rect)?;
                        }

                        break;
                    }
                }
            }
            WorkspaceLayer::Tiling => {
                match workspace.layout {
                    Layout::Default(DefaultLayout::Scrolling) => {
                        tracing::info!("resizing column");
                        workspace.resize_scrolling_column(direction, sizing, delta);

                        return if update {
                            self.update_focused_workspace(false, false)
                        } else {
                            Ok(())
                        };
                    }
                    Layout::Default(layout) => {
                        tracing::info!("resizing window");
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

                            let min_width = focused_monitor_work_area.right / 10;
                            let min_height = focused_monitor_work_area.bottom / 10;
                            let unaltered_rect = *unaltered
                                .get(focused_idx)
                                .ok_or_eyre("there is no last layout")?;

                            let candidate = |amount: i32| {
                                layout.resize(
                                    &unaltered_rect,
                                    focused_idx_resize,
                                    direction,
                                    sizing,
                                    amount,
                                )
                            };

                            let calculate = |dimensions: &[Option<Rect>]| {
                                layout.calculate(
                                    &focused_monitor_work_area,
                                    len,
                                    workspace.container_padding,
                                    workspace.layout_flip,
                                    dimensions,
                                    workspace.focused_container_idx(),
                                    workspace.layout_options,
                                    &workspace.latest_layout,
                                )
                            };

                            // No tile may end up smaller than a tenth of the work area, or
                            // smaller than it already is
                            let now = calculate(&workspace.resize_dimensions);
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

                            // Grant as much of the request as fits; tiles only get smaller as
                            // the amount grows, so bisect
                            let granted = if fits(delta) {
                                delta
                            } else {
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

                            let resize = candidate(granted);

                            workspace.resize_dimensions[focused_idx] = resize;

                            return if update {
                                self.update_focused_workspace(false, false)
                            } else {
                                Ok(())
                            };
                        }

                        tracing::warn!("cannot resize container in this direction");
                    }
                }
            }
        }

        Ok(())
    }

    /// How far an edge can be from its tile without having been dragged there: an app that snaps
    /// its size to a grid of cells, or keeps to a minimum size, is never quite on its tile
    const DRAG_SNAP_TOLERANCE: i32 = 32;

    /// How far an edge can have got between somebody taking hold of it and the start of the drag
    /// being handled, which a fast drag makes a good many pixels
    const DRAG_LAG_ALLOWANCE: i32 = 64;

    /// Where the edges of a drag began, given the first frame of the window that was seen once the
    /// drag was going on, the frame that it is in now and the tile that it was put in. All three
    /// are `[left, top, right, bottom]`.
    ///
    /// The first AXWindowResized is reported after the edge has already moved, and handlers run
    /// one at a time, so the frame a drag is first seen in is not the one it began from, and can
    /// even be the one it has ended in.
    ///
    /// * An edge that has moved since the drag was first seen was dragged, and began on its tile
    ///   if the frame that it was first seen in was no further from it than a fast drag gets
    ///   before it is handled. Any further and it is measured from where it was seen.
    /// * An edge that has not moved was not dragged, and the offset of an app from its tile is not
    ///   part of the drag, nor is a window that cannot be as small as its tile.
    /// * Where nothing has moved at all the drag was over before it was seen, and an edge that is
    ///   further from its tile than an app strays from its tile on its own was dragged.
    fn drag_origin(started: [i32; 4], ended: [i32; 4], tile: Option<[i32; 4]>) -> [i32; 4] {
        let Some(tile) = tile else {
            return started;
        };

        let seen_moving = started != ended;

        let mut origin = started;
        for i in 0..4 {
            let began_on_tile = if seen_moving {
                ended[i] != started[i] && (started[i] - tile[i]).abs() <= Self::DRAG_LAG_ALLOWANCE
            } else {
                (ended[i] - tile[i]).abs() > Self::DRAG_SNAP_TOLERANCE
            };

            if began_on_tile {
                origin[i] = tile[i];
            }
        }

        origin
    }

    /// The edges that were moved from `origin` to `ended`, and by how much, which is negative
    /// for an edge that went left or up.
    fn edges_moved(origin: [i32; 4], ended: [i32; 4]) -> Vec<(OperationDirection, i32)> {
        [
            OperationDirection::Left,
            OperationDirection::Up,
            OperationDirection::Right,
            OperationDirection::Down,
        ]
        .into_iter()
        .enumerate()
        .filter_map(|(i, edge)| (ended[i] != origin[i]).then_some((edge, ended[i] - origin[i])))
        .collect()
    }

    /// Applies a mouse drag of the edge(s) of `window_id`'s tile, given the frame the window was
    /// first seen in once the drag was going on and the frame it ended in. Returns whether the
    /// workspace needs to be re-tiled, which also puts back a window dragged by an edge with no
    /// neighbour.
    #[tracing::instrument(skip(self))]
    pub fn resize_by_drag(
        &mut self,
        window_id: u32,
        started_from: Rect,
        ended_at: Rect,
    ) -> eyre::Result<bool> {
        let workspace = self.focused_workspace()?;
        let Some(idx) = workspace.container_idx_for_window(window_id) else {
            tracing::warn!("the dragged window is not in a container on the focused workspace");
            return Ok(false);
        };

        // A Rect is (left, top, width, height); work in edges instead
        let edges = |r: &Rect| [r.left, r.top, r.left + r.right, r.top + r.bottom];
        let ended = edges(&ended_at);
        let tile = workspace.latest_layout.get(idx).map(edges);

        let origin = Self::drag_origin(edges(&started_from), ended, tile);
        let moved = Self::edges_moved(origin, ended);

        if moved.is_empty() {
            return Ok(false);
        }

        self.apply_mouse_resize(idx, &moved)?;

        Ok(true)
    }

    /// Resize the container at `idx` by the edges that were dragged, which is not necessarily
    /// the one that has focus.
    fn apply_mouse_resize(
        &mut self,
        idx: usize,
        moved: &[(OperationDirection, i32)],
    ) -> eyre::Result<()> {
        self.focused_workspace_mut()?.focus_container(idx);

        for &(edge, by) in moved {
            let outward = match edge {
                OperationDirection::Left | OperationDirection::Up => by < 0,
                OperationDirection::Right | OperationDirection::Down => by > 0,
            };

            let sizing = if outward {
                Sizing::Increase
            } else {
                Sizing::Decrease
            };

            // A shared edge is moved by half of what the layout is given
            self.resize_window(edge, sizing, by.abs() * 2, false)?;
        }

        Ok(())
    }

    /// What a drag by an edge of `window_id` is followed from, if it is one that the windows next
    /// to the edge follow: that is a window in a container of a workspace that is tiled, and
    /// where nothing is maximised or in monocle.
    fn live_resize_snapshot(&self, window_id: u32) -> Option<LiveResize> {
        let workspace = self.focused_workspace().ok()?;

        let follows = workspace.tile
            && matches!(workspace.layer, WorkspaceLayer::Tiling)
            && workspace.monocle_container.is_none()
            && workspace.maximized_window.is_none()
            && workspace.container_idx_for_window(window_id).is_some();

        follows.then(|| LiveResize {
            window: window_id,
            containers: workspace.containers().len(),
            resize_dimensions: workspace.resize_dimensions.clone(),
            latest_layout: workspace.latest_layout.clone(),
            scrolling: workspace.scrolling.clone(),
            stepped: false,
            timeouts: vec![],
        })
    }

    /// Somebody has taken hold of an edge of `window_id`: keep what the workspace is, for the
    /// windows next to the edge to follow it from here, if they are to.
    pub fn live_resize_begin(&mut self, window_id: u32) {
        // the windows are not looked at for the drag before any more
        settle::begin();

        // one that was never ended, by a window manager that did not hear that the button went up
        self.live_resize_forget();

        let Some(mut live) = self.live_resize_snapshot(window_id) else {
            return;
        };

        if let Ok(workspace) = self.focused_workspace() {
            let dragged_process = workspace
                .container_for_window(window_id)
                .and_then(|container| container.windows().iter().find(|w| w.id == window_id))
                .map(|window| window.application.process_id);

            for container in workspace.containers() {
                for window in container.windows().iter().filter(|w| w.id != window_id) {
                    live.timeouts.push(window.element.clone());

                    // the app of the window that is dragged is asked where that window is for
                    // every step, and is not to be given up on any sooner for a neighbour that
                    // is one of its windows
                    if Some(window.application.process_id) != dragged_process {
                        live.timeouts.push(window.application.element().clone());
                    }
                }
            }
        }

        set_messaging_timeouts(&live.timeouts, LIVE_RESIZE_TIMEOUT);

        live_resize_begins(window_id);
        self.live_resize = Some(live);
    }

    /// Stop following a drag, wherever it got to, and give back what was kept.
    pub(crate) fn live_resize_forget(&mut self) -> Option<LiveResize> {
        live_resize_ends();

        let live = self.live_resize.take()?;
        set_messaging_timeouts(&live.timeouts, 0.0);

        Some(live)
    }

    /// Whether the windows next to an edge are following the drag of a window other than
    /// `window_id`.
    ///
    /// A window that is moved or resized while somebody is dragging another one is a window that
    /// the layout has moved, and not somebody dragging it. With the button up there is no drag,
    /// whatever was kept.
    pub fn another_window_is_being_dragged(&self, window_id: u32) -> bool {
        self.live_resize
            .as_ref()
            .is_some_and(|live| live.window != window_id && dragged_window() == Some(live.window))
    }

    /// Work out what the workspace is for where the dragged window is now, from what it was when
    /// the drag began, and say whether that is to be laid out.
    ///
    /// `now` is where the window is, which is not asked for here.
    pub(crate) fn live_resize_apply(&mut self, now: Rect) -> eyre::Result<bool> {
        let Some(mut live) = self.live_resize.take() else {
            return Ok(false);
        };

        let Some((_, Some(started_from))) =
            (*self.pending_resize_op).filter(|(window, _)| *window == live.window)
        else {
            self.live_resize = Some(live);
            return Ok(false);
        };

        let workspace = self.focused_workspace()?;

        // a window that opens or closes in the middle of a drag changes what the layout is made
        // of, and there is nothing in what was kept to follow it by: the end of the drag is
        // worked out from what the workspace is by then
        if workspace.containers().len() != live.containers
            || workspace.container_idx_for_window(live.window).is_none()
        {
            tracing::debug!("not following the drag any more: the containers have changed");

            // the drag is still going on, and what the layout moves is still not somebody dragging
            // it, so that is left as it is until the button goes up
            set_messaging_timeouts(&live.timeouts, 0.0);

            return Ok(false);
        }

        // a window that was only moved changes no tile until it is let go, and one that has been
        // taken back to where it began is a step like any other, which puts the windows back
        let resized = now.right != started_from.right || now.bottom != started_from.bottom;
        if !resized && !live.stepped {
            self.live_resize = Some(live);
            return Ok(false);
        }

        let workspace = self.focused_workspace_mut()?;
        workspace.resize_dimensions = live.resize_dimensions.clone();
        workspace.latest_layout = live.latest_layout.clone();
        workspace.scrolling = live.scrolling.clone();

        let moved = self.resize_by_drag(live.window, started_from, now);

        // what was kept is kept whatever came of this
        live.stepped |= moved.as_ref().is_ok_and(|moved| *moved);
        let lay_out = live.stepped;
        self.live_resize = Some(live);

        moved?;

        Ok(lay_out)
    }

    /// Lay out the workspace for where the edge of the dragged window is now.
    ///
    /// This is for a step of the drag of `window_id`, which says only that the window has moved.
    pub fn live_resize_step(&mut self, window_id: u32) -> eyre::Result<()> {
        if self.is_paused
            || !self
                .live_resize
                .as_ref()
                .is_some_and(|live| live.window == window_id)
        {
            return Ok(());
        }

        // the button went up, and the end of the drag is on its way
        if dragged_window() != Some(window_id) {
            return Ok(());
        }

        let element = self
            .focused_workspace()?
            .container_for_window(window_id)
            .and_then(|container| container.windows().iter().find(|w| w.id == window_id))
            .map(|window| window.element.clone())
            .ok_or_eyre("the dragged window is not in a container on the focused workspace")?;

        let now = Rect::from(MacosApi::window_rect(&element)?);

        if !self.live_resize_apply(now)? {
            return Ok(());
        }

        let offset = self.work_area_offset;
        let _snapping = Snapping::step(window_id);

        self.focused_monitor_mut()
            .ok_or_eyre("there is no monitor")?
            .update_focused_workspace(offset)
    }

    /// The drag is over: put the workspace back to what it was when the drag began, for the end
    /// of the drag to work out what it did from there, as it does when there were no steps.
    ///
    /// Says whether the windows next to the edge were laid out for any step of the drag, in
    /// which case they are not where the layout for the end of the drag puts them, and are to be
    /// laid out again whatever the drag comes to.
    pub(crate) fn live_resize_end(&mut self) -> bool {
        let Some(live) = self.live_resize_forget() else {
            return false;
        };

        if !live.stepped {
            return false;
        }

        if let Ok(workspace) = self.focused_workspace_mut()
            && workspace.containers().len() == live.containers
            && workspace.container_idx_for_window(live.window).is_some()
        {
            workspace.resize_dimensions = live.resize_dimensions;
            workspace.latest_layout = live.latest_layout;
            workspace.scrolling = live.scrolling;
        }

        // a step may have left a window short of where the end of the drag sends it, which is not
        // known until the app has had time to do what it was going to
        if let Ok(workspace_idx) = self.focused_workspace_idx() {
            settle::request(self.focused_monitor_idx(), workspace_idx);
        }

        true
    }

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
                for resize in &mut workspace.resize_dimensions {
                    *resize = None;
                }

                // and the widths and heights of the Scrolling layout, which are resize
                // adjustments too
                workspace.reset_scrolling_sizes();
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

    #[tracing::instrument(skip(self))]
    pub fn toggle_lock(&mut self) -> eyre::Result<()> {
        let workspace = self.focused_workspace_mut()?;
        if let Some(container) = workspace.focused_container_mut() {
            // Toggle the locked flag
            container.set_locked(!container.locked());
        }
        Ok(())
    }

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

    pub fn monitor_idx_from_current_pos(&mut self) -> Option<usize> {
        let monitor_id = MacosApi::monitor_from_point(MacosApi::cursor_pos())?;

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

        let foreground_window_id =
            MacosApi::foreground_window_id().ok_or_eyre("no foreground window")?;
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
                window.restore()?;
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
            let focused_window_id =
                MacosApi::foreground_window_id().ok_or_eyre("no foreground window")?;
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
                                target_idx = Some(idx - 1)
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

    #[tracing::instrument(skip(self))]
    pub fn cycle_container_window_index_in_direction(
        &mut self,
        direction: CycleDirection,
    ) -> eyre::Result<()> {
        tracing::info!("cycling container window index");

        let mouse_follows_focus = self.mouse_follows_focus;
        let window_hiding_position = self.focused_workspace()?.globals.window_hiding_position;

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
        container.load_focused_window(window_hiding_position)?;

        if let Some(window) = container.focused_window() {
            window.focus(mouse_follows_focus)?;
        }

        self.update_focused_workspace(mouse_follows_focus, true)
    }

    #[tracing::instrument(skip(self))]
    pub fn focus_container_window(&mut self, idx: usize) -> eyre::Result<()> {
        tracing::info!("focusing container window at index {idx}");

        let mouse_follows_focus = self.mouse_follows_focus;
        let window_hiding_position = self.focused_workspace()?.globals.window_hiding_position;

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
        container.load_focused_window(window_hiding_position)?;

        if let Some(window) = container.focused_window() {
            window.focus(mouse_follows_focus)?;
        }

        self.update_focused_workspace(mouse_follows_focus, true)
    }

    #[tracing::instrument(skip(self))]
    pub fn stack_all(&mut self) -> eyre::Result<()> {
        tracing::info!("stacking all windows on workspace");

        let workspace = self.focused_workspace_mut()?;
        let window_hiding_position = workspace.globals.window_hiding_position;

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
            c.load_focused_window(window_hiding_position)?;
        }
        self.update_focused_workspace(self.mouse_follows_focus, true)
    }

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
                        if let (
                            Some(exe_name),
                            Some(title),
                            Some(role),
                            Some(subrole),
                            Some(path),
                        ) = (
                            window.exe(),
                            window.title(),
                            window.role(),
                            window.subrole(),
                            window.path(),
                        ) {
                            for rule in &*workspace_matching_rules {
                                let matched = match &rule.matching_rule {
                                    MatchingRule::Simple(r) => should_act_individual(
                                        &title,
                                        &exe_name,
                                        &[&role, &subrole],
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
                                                &[&role, &subrole],
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

            let mut window = origin_workspace.remove_window(op.window_id)?;

            // If it is a floating window move it to the target area
            if op.floating {
                window.move_to_area(&origin_area, &target_area)?;
            }

            // Hide the window we are about to remove if it is on the currently focused workspace
            if op.is_origin(focused_monitor_idx, focused_workspace_idx) {
                window.hide(WindowHidingPosition::default())?;
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
                    .push_back(window.clone());
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

        // Only re-tile the focused workspace if we need to
        if should_update_focused_workspace {
            self.update_focused_workspace(false, false)?;
        }

        Ok(())
    }

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

    #[tracing::instrument(skip(self))]
    pub fn reload_static_configuration(&mut self, pathbuf: &PathBuf) -> eyre::Result<()> {
        tracing::info!("reloading static configuration");
        StaticConfig::reload(pathbuf, self)
    }

    #[tracing::instrument(skip(self))]
    pub fn handle_unmanaged_window_behaviour(&self) -> eyre::Result<()> {
        if matches!(
            self.unmanaged_window_operation_behaviour,
            OperationBehaviour::NoOp
        ) {
            let workspace = self.focused_workspace()?;
            let focused_hwnd =
                MacosApi::foreground_window_id().ok_or_eyre("there is no foreground window")?;
            if !workspace.contains_managed_window(focused_hwnd) {
                bail!("ignoring commands while active window is not managed by komorebi");
            }
        }

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
        rules.sort_by(|a, b| a.0.cmp(&b.0));

        // If this is the focused workspace on a non-focused screen, let's update it
        if focused_monitor_idx != monitor_idx && focused_workspace_idx == workspace_idx {
            workspace.update()?;
            Ok(())
        } else {
            Ok(self.update_focused_workspace(false, false)?)
        }
    }

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

    #[tracing::instrument(skip(self))]
    pub fn manage_focused_window(&mut self) -> eyre::Result<()> {
        let element = MacosApi::foreground_window().ok_or_eyre("there is no foreground window")?;
        let window_id = MacosApi::foreground_window_id();
        if let Some(process_id) = AdhocWindow::process_id(&element)
            && let Some(event) = WindowManagerEvent::from_system_notification(
                SystemNotification::Manual(ManualNotification::Manage),
                process_id,
                window_id,
            )
        {
            UNMANAGED_WINDOW_IDS
                .lock()
                .retain(|id| *id != window_id.unwrap_or_default());

            window_manager_event_listener::send_notification(event);
        }

        Ok(())
    }

    #[tracing::instrument(skip(self))]
    pub fn unmanage_focused_window(&mut self) -> eyre::Result<()> {
        let element = MacosApi::foreground_window().ok_or_eyre("there is no foreground window")?;
        let window_id = MacosApi::foreground_window_id();
        if let Some(process_id) = AdhocWindow::process_id(&element)
            && let Some(event) = WindowManagerEvent::from_system_notification(
                SystemNotification::Manual(ManualNotification::Unmanage),
                process_id,
                window_id,
            )
        {
            window_manager_event_listener::send_notification(event);
        }

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
                // TODO: don't think this is needed on macOS
                // After we restore the origin workspace, some windows that were cloacked
                // by the monocle might now be uncloacked which would trigger a workspace
                // reconciliation since the focused monitor would be different from origin.
                // That workspace reconciliation would focus the window on the origin monitor.
                // So we need to ignore the uncloak events produced by the origin workspace
                // restore to avoid that issue.
                // self.uncloack_to_ignore = uncloack_amount;
            }
            // TODO: this probably won't be used on macOS
        } else if origin_workspace
            .maximized_window
            .as_ref()
            .map(|max| max.id == window_id)
            .unwrap_or_default()
        {
            // Moving maximized_window
            // TODO: not sure if we'll support maximized windows on macOS
            // if let Some(maximized_idx) = origin_workspace.maximized_window_restore_idx {
            //     self.focus_monitor(origin_monitor_idx)?;
            //     let origin_monitor = self
            //         .focused_monitor_mut()
            //         .ok_or_eyre("there is no origin monitor")?;
            //     origin_monitor.focus_workspace(origin_workspace_idx)?;
            //     self.unmaximize_window()?;
            //     self.focus_monitor(target_monitor_idx)?;
            //     let target_monitor = self
            //         .focused_monitor_mut()
            //         .ok_or_eyre("there is no target monitor")?;
            //     target_monitor.focus_workspace(target_workspace_idx)?;
            //
            //     self.transfer_container(
            //         (origin_monitor_idx, origin_workspace_idx, maximized_idx),
            //         (
            //             target_monitor_idx,
            //             target_workspace_idx,
            //             target_container_idx,
            //         ),
            //     )?;
            // }
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
    use crate::SocketMessage;
    use crate::animation::animator;
    use crate::core::animation::AnimationStyle;
    use crate::monitor;
    use crate::reaper;
    use crate::reaper::ReaperNotification;
    use crossbeam_channel::Sender;
    use crossbeam_channel::bounded;
    use std::path::PathBuf;
    use std::time::Duration;
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
        let socket_name = format!("komorebi-test-{}.sock", Uuid::new_v4());
        let socket_path = PathBuf::from(socket_name);

        // Create a new WindowManager instance
        let wm = WindowManager::new(
            &CFRunLoop::main().unwrap(),
            receiver,
            Some(socket_path.clone()),
        );

        // Window Manager should be created successfully
        assert!(wm.is_ok());

        (
            wm.unwrap(),
            TestContext {
                socket_path: Some(socket_path),
            },
        )
    }

    /// Windows move by animation, for as long as this is alive
    struct AnimationsOn;

    impl AnimationsOn {
        fn new() -> Self {
            animation::ANIMATION_ENABLED_PER_ANIMATION
                .lock()
                .insert(AnimationPrefix::Movement, true);
            Self
        }
    }

    impl Drop for AnimationsOn {
        fn drop(&mut self) {
            animation::ANIMATION_ENABLED_PER_ANIMATION
                .lock()
                .remove(&AnimationPrefix::Movement);
        }
    }

    /// Start a journey for a window that is to last a minute, and so is in flight until
    /// something takes it over
    fn start_journey(window_id: u32, target: Rect) {
        animator::retarget(
            &animator::Journey {
                window_id,
                pid: 1,
                handle: animator::WindowHandle {
                    element: AccessibilityUiElement::default(),
                    app: AccessibilityUiElement::default(),
                },
                target,
                motion: animation::step::MotionSpec {
                    duration: Duration::from_secs(60),
                    style: AnimationStyle::Linear,
                },
                after: None,
            },
            || Ok(Rect::default()),
        )
        .unwrap();

        assert!(animator::is_animating(window_id));
    }

    #[test]
    fn resize_commands_lay_the_windows_out_in_place_with_animations_on() {
        // ids that nothing else in the tests uses, as what is animating is shared by all of them
        const FIRST: u32 = 3_700_000_001;
        let area = Rect {
            left: 0,
            top: 0,
            right: 1200,
            bottom: 800,
        };

        for message in [
            SocketMessage::ResizeWindowEdge(OperationDirection::Right, Sizing::Increase),
            SocketMessage::ResizeWindowAxis(Axis::Horizontal, Sizing::Decrease),
        ] {
            let (mut wm, _context) = setup_window_manager();
            wm.monitors_mut().push_back(monitor::new(
                0,
                area,
                area,
                "TestMonitor".to_string(),
                "TestDeviceID".to_string(),
            ));

            let workspace = wm.focused_workspace_mut().unwrap();
            workspace.layout = Layout::Default(DefaultLayout::Scrolling);
            for id in FIRST..FIRST + 3 {
                workspace
                    .new_container_for_window(&Window::from(id))
                    .unwrap();
            }
            workspace.focus_container(0);
            workspace.layout_scrolling(&area, 0);

            let _animations = AnimationsOn::new();
            start_journey(FIRST, area);

            wm.process_command(message.clone(), std::io::sink())
                .unwrap();

            assert!(
                !animator::is_animating(FIRST),
                "{message}: the window that was resized is still animating"
            );
        }
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

    // Resizing a tile by dragging an edge. A Rect is `left, top, width, height` (`right` and
    // `bottom` are the size), and the work area of the monitor is 1920x1050, which makes the
    // right-hand stack of an UltrawideVerticalStack layout narrow enough to expose the old guard.

    const WORK_AREA: Rect = Rect {
        left: 0,
        top: 0,
        right: 1920,
        bottom: 1050,
    };

    /// A monitor with one workspace in `layout`, with a container per window id, the last of
    /// them focused, and the tiles komorebi would have given them recorded as the latest layout
    fn tiled(layout: DefaultLayout, ids: &[u32]) -> (WindowManager, TestContext) {
        let (mut wm, context) = setup_window_manager();

        let m = monitor::new(
            0,
            WORK_AREA,
            WORK_AREA,
            "TestMonitor".to_string(),
            "TestDeviceID".to_string(),
        );
        wm.monitors_mut().push_back(m);

        let workspace = wm.focused_workspace_mut().unwrap();
        workspace.layout = Layout::Default(layout);
        for id in ids {
            workspace
                .new_container_for_window(&Window::from(*id))
                .unwrap();
        }

        let latest_layout = tiles(&wm);
        wm.focused_workspace_mut().unwrap().latest_layout = latest_layout;

        (wm, context)
    }

    /// The tile of each container, as the layout calculates them with the current resizes
    fn tiles(wm: &WindowManager) -> Vec<Rect> {
        let workspace = wm.focused_workspace().unwrap();
        let Layout::Default(layout) = workspace.layout;

        layout.calculate(
            &WORK_AREA,
            NonZeroUsize::new(workspace.containers().len()).unwrap(),
            workspace.container_padding,
            workspace.layout_flip,
            &workspace.resize_dimensions,
            workspace.focused_container_idx(),
            workspace.layout_options,
            &workspace.latest_layout,
        )
    }

    fn grown_left(r: Rect, by: i32) -> Rect {
        Rect {
            left: r.left - by,
            right: r.right + by,
            ..r
        }
    }

    fn overlap(a: &Rect, b: &Rect) -> bool {
        a.left < b.left + b.right
            && b.left < a.left + a.right
            && a.top < b.top + b.bottom
            && b.top < a.top + a.bottom
    }

    fn assert_tiling(tiles: &[Rect]) {
        for (i, a) in tiles.iter().enumerate() {
            assert!(
                a.left >= 0
                    && a.top >= 0
                    && a.left + a.right <= WORK_AREA.right
                    && a.top + a.bottom <= WORK_AREA.bottom,
                "{a:?} is outside the work area"
            );
            for b in &tiles[i + 1..] {
                assert!(!overlap(a, b), "{a:?} and {b:?} overlap");
            }
        }
    }

    fn no_resizes(wm: &WindowManager) -> bool {
        wm.focused_workspace()
            .unwrap()
            .resize_dimensions
            .iter()
            .all(|r| r.is_none())
    }

    #[test]
    fn test_a_long_drag_of_a_narrow_tile_is_not_dropped() {
        // the right-hand stack of an ultrawide layout is the narrowest thing there is, and the
        // layout used to refuse any drag of more than half of a tile's own width, without a word
        let (mut wm, _context) = tiled(DefaultLayout::UltrawideVerticalStack, &[1, 2, 3, 4, 5]);
        // 1 is the centre, 2 the left column, 3..5 the stack on the right
        let before = tiles(&wm);
        assert!(before[2].left > WORK_AREA.right / 2, "{:?}", before[2]);

        // 5 has focus, not the window being dragged
        let redraw = wm
            .resize_by_drag(3, before[2], grown_left(before[2], 300))
            .unwrap();
        assert!(redraw);

        let after = tiles(&wm);
        assert!(
            after[2].right >= before[2].right + 290,
            "dragged by 300 and got {:?} -> {:?}",
            before[2],
            after[2]
        );
        assert!(after[0].right > 0 && after[0].left + after[0].right <= after[2].left);
        assert_tiling(&after);

        // and the whole stack moved with it: they share the edge
        for idx in [3, 4] {
            assert_eq!(after[idx].left, after[2].left, "tile {idx}");
        }
    }

    #[test]
    fn test_a_drag_further_than_the_neighbour_can_give_goes_as_far_as_it_can() {
        let (mut wm, _context) = tiled(DefaultLayout::UltrawideVerticalStack, &[1, 2, 3, 4, 5]);
        let before = tiles(&wm);

        // far past the centre tile's whole width
        wm.resize_by_drag(3, before[2], grown_left(before[2], 1000))
            .unwrap();

        let after = tiles(&wm);
        assert!(
            after[2].right > before[2].right + 400,
            "it grew: {:?} -> {:?}",
            before[2],
            after[2]
        );

        // no tile is smaller than a tenth of the work area, or than it was
        let tenth = WORK_AREA.right / 10;
        for (idx, (tile, was)) in after.iter().zip(before.iter()).enumerate() {
            assert!(tile.right >= tenth.min(was.right), "tile {idx}: {tile:?}");
        }

        // and the centre tile was squeezed to the floor, not left where it was
        assert!(
            after[0].right < tenth + 10,
            "the neighbour was not given all it could give: {:?}",
            after[0]
        );
        assert_tiling(&after);
    }

    #[test]
    fn test_shrinking_a_tile_by_more_than_it_can_is_granted_as_far_as_it_goes() {
        let (mut wm, _context) = tiled(DefaultLayout::UltrawideVerticalStack, &[1, 2, 3, 4, 5]);
        let before = tiles(&wm);
        let tile = before[2];

        // the user pulls the left edge inward by more than the tile is wide
        wm.resize_by_drag(
            3,
            tile,
            Rect {
                left: tile.left + tile.right - 20,
                right: 20,
                ..tile
            },
        )
        .unwrap();

        let after = tiles(&wm);
        let tenth = WORK_AREA.right / 10;
        assert!(
            after[2].right < tile.right,
            "it shrank: {tile:?} -> {:?}",
            after[2]
        );
        assert!(after[2].right >= tenth, "but not too far: {:?}", after[2]);
        assert!(
            after[2].right < tenth + 10,
            "as far as it goes: {:?}",
            after[2]
        );
        assert_tiling(&after);
    }

    #[test]
    fn test_a_drag_is_measured_from_where_the_window_was_not_from_its_tile() {
        // a terminal snaps its size to its cells, so it never quite fills its tile; the horizontal
        // split between the two windows on the right must stay where it is when the *left* edge
        // is dragged
        let (mut wm, _context) = tiled(DefaultLayout::BSP, &[1, 2, 3]);
        let before = tiles(&wm);
        let (top_right, split_before) = (before[1], before[2].top);

        let snapped = Rect {
            right: top_right.right - 1,
            bottom: top_right.bottom - 8,
            ..top_right
        };

        wm.resize_by_drag(2, snapped, grown_left(snapped, 100))
            .unwrap();

        let after = tiles(&wm);
        assert!(
            after[1].left <= top_right.left - 90,
            "{top_right:?} -> {:?}",
            after[1]
        );
        assert_eq!(
            after[2].top, split_before,
            "the split between the two on the right moved, though only a left edge was dragged"
        );
        assert_eq!(after[1].bottom, top_right.bottom);
        assert_tiling(&after);
    }

    #[test]
    fn test_the_window_that_was_dragged_is_the_one_resized_not_the_focused_one() {
        let (mut wm, _context) = tiled(DefaultLayout::BSP, &[1, 2, 3]);
        // 3 (the bottom right tile) has focus; 2 (top right) is dragged by its bottom edge
        assert_eq!(wm.focused_workspace().unwrap().focused_container_idx(), 2);
        let before = tiles(&wm);
        let (top_right, bottom_right) = (before[1], before[2]);

        wm.resize_by_drag(
            2,
            top_right,
            Rect {
                bottom: top_right.bottom + 100,
                ..top_right
            },
        )
        .unwrap();

        let after = tiles(&wm);
        assert!(
            after[1].bottom >= top_right.bottom + 90,
            "{top_right:?} -> {:?}",
            after[1]
        );
        assert!(
            after[2].bottom <= bottom_right.bottom - 90,
            "{bottom_right:?} -> {:?}",
            after[2]
        );
        assert_tiling(&after);

        // the dragged one is the focused one now
        assert_eq!(wm.focused_workspace().unwrap().focused_container_idx(), 1);
    }

    #[test]
    fn test_dragging_an_edge_that_has_no_neighbour_changes_nothing_but_puts_the_window_back() {
        let (mut wm, _context) = tiled(DefaultLayout::BSP, &[1, 2]);
        let before = tiles(&wm);

        // the outer left edge of the left tile has nothing to take room from
        let redraw = wm
            .resize_by_drag(1, before[0], grown_left(before[0], 80))
            .unwrap();

        assert!(
            redraw,
            "the window is not where its tile is, so it needs to be put back"
        );
        assert!(no_resizes(&wm));
        assert_eq!(tiles(&wm), before);
    }

    #[test]
    fn test_a_drag_that_changed_nothing_changes_nothing() {
        let (mut wm, _context) = tiled(DefaultLayout::BSP, &[1, 2, 3]);
        let before = tiles(&wm);

        // a click on an edge: started, finished, nowhere
        let redraw = wm.resize_by_drag(2, before[1], before[1]).unwrap();

        assert!(!redraw);
        assert!(no_resizes(&wm));
        assert_eq!(tiles(&wm), before);
        assert_eq!(wm.focused_workspace().unwrap().focused_container_idx(), 2);
    }

    #[test]
    fn test_a_corner_resizes_both_of_its_edges() {
        let (mut wm, _context) = tiled(DefaultLayout::BSP, &[1, 2, 3]);
        // the bottom right tile, by its top left corner: up into the tile above, left into the
        // one beside
        let before = tiles(&wm);
        let (beside, above, tile) = (before[0], before[1], before[2]);

        wm.resize_by_drag(
            3,
            tile,
            Rect {
                left: tile.left - 100,
                top: tile.top - 100,
                right: tile.right + 100,
                bottom: tile.bottom + 100,
            },
        )
        .unwrap();

        let after = tiles(&wm);
        assert!(
            after[2].left <= tile.left - 90 && after[2].top <= tile.top - 90,
            "{tile:?} -> {:?}",
            after[2]
        );
        assert!(after[1].bottom < above.bottom);
        assert!(after[0].right < beside.right);
        assert_tiling(&after);
    }

    #[test]
    fn test_a_window_that_is_in_no_container_is_ignored() {
        let (mut wm, _context) = tiled(DefaultLayout::BSP, &[1, 2, 3]);
        let before = tiles(&wm);

        let redraw = wm
            .resize_by_drag(99, before[1], grown_left(before[1], 100))
            .unwrap();

        assert!(!redraw);
        assert!(no_resizes(&wm));
        assert_eq!(wm.focused_workspace().unwrap().focused_container_idx(), 2);
    }

    // The two recordings of a left-hand drag of the right-hand tile of an UltrawideVerticalStack
    // that "doesn't resize properly" (docs/drag-resize.md). Both end with the tile narrower
    // instead of wider, and in both the log shows the same thing: a workspace update on the
    // reaper's thread, between the button going up and the handler for the end of the drag
    // reading where the window is.

    #[test]
    fn test_a_mouse_up_does_not_re_tile_the_workspace() {
        // The reaper looks for windows that went away on every mouse-up and key-up. It did that
        // by re-tiling the workspace, which put the window being dragged back on its tile
        // before ResizeEnd was handled, so the drag was measured from the dragged frame to the
        // tile: backwards. A re-tile replaces the latest layout, so a marker in it shows one.
        let (mut wm, _context) = tiled(DefaultLayout::UltrawideVerticalStack, &[1, 2, 3]);
        let marker = vec![Rect::default(); 3];
        wm.focused_workspace_mut().unwrap().latest_layout = marker.clone();

        reaper::handle_notification(&mut wm, ReaperNotification::MouseUpKeyUp, |_| true).unwrap();

        assert_eq!(
            wm.focused_workspace().unwrap().latest_layout,
            marker,
            "the mouse-up re-tiled the workspace"
        );
    }

    #[test]
    fn test_a_mouse_up_still_reaps_the_windows_that_went_away() {
        // what the re-tile on mouse-up was for: a window closed with a click or a key press
        let (mut wm, _context) = tiled(DefaultLayout::UltrawideVerticalStack, &[1, 2, 3]);

        reaper::handle_notification(&mut wm, ReaperNotification::MouseUpKeyUp, |w| w.id != 2)
            .unwrap();

        let ids: Vec<u32> = wm
            .focused_workspace()
            .unwrap()
            .containers()
            .iter()
            .flat_map(|c| c.windows().iter().map(|w| w.id))
            .collect();
        assert_eq!(ids, vec![1, 3]);
    }

    #[test]
    fn test_the_first_step_of_a_drag_is_part_of_the_drag() {
        // AXWindowResized is reported after the edge has already moved, so the frame recorded as
        // the start of the drag is not the one it started from: in the second recording the edge
        // was 22 points out when first seen and 107 when the button went up, and the divider
        // ended 22 points short of the pointer. The edge began on its tile.
        let (mut wm, _context) = tiled(DefaultLayout::UltrawideVerticalStack, &[1, 2, 3]);
        let tile = tiles(&wm)[2];

        let redraw = wm
            .resize_by_drag(3, grown_left(tile, 22), grown_left(tile, 107))
            .unwrap();

        assert!(redraw);
        let after = tiles(&wm);
        assert_eq!(after[2].left, tile.left - 107, "{tile:?} -> {:?}", after[2]);
        assert_tiling(&after);
    }

    #[test]
    fn test_a_drag_that_was_over_before_it_was_first_seen_is_still_applied() {
        // Handlers run one at a time and can be slow (432 ms in the first recording, before the
        // handler for the first AXWindowResized even logged its header), so the frame recorded as
        // the start of the drag can already be the frame the drag ended in
        let (mut wm, _context) = tiled(DefaultLayout::UltrawideVerticalStack, &[1, 2, 3]);
        let tile = tiles(&wm)[2];
        let dragged = grown_left(tile, 211);

        let redraw = wm.resize_by_drag(3, dragged, dragged).unwrap();

        assert!(redraw);
        let after = tiles(&wm);
        assert_eq!(after[2].left, tile.left - 211, "{tile:?} -> {:?}", after[2]);
        assert_tiling(&after);
    }

    // Following an edge that is dragged, with the windows next to it laid out for every step.
    // Nothing here asks a window where it is, so the frame that a step finds the dragged window
    // in is given.

    /// The edge of `window_id` has been taken hold of, and the first frame of it that was seen is
    /// `started_from`
    fn followed(wm: &mut WindowManager, window_id: u32, started_from: Rect) {
        *Arc::make_mut(&mut wm.pending_resize_op) = Some((window_id, Some(started_from)));
        wm.live_resize = Some(wm.live_resize_snapshot(window_id).unwrap());
    }

    /// A step of the drag, and the layout that comes of it recorded the way that updating the
    /// workspace records it. Says whether the workspace was laid out.
    fn step(wm: &mut WindowManager, now: Rect) -> bool {
        let laid_out = wm.live_resize_apply(now).unwrap();

        if laid_out {
            let latest_layout = tiles(wm);
            wm.focused_workspace_mut().unwrap().latest_layout = latest_layout;
        }

        laid_out
    }

    #[test]
    fn test_the_steps_of_a_drag_and_its_end_come_to_what_the_drag_comes_to_without_them() {
        let (mut stepped, _context) = tiled(DefaultLayout::UltrawideVerticalStack, &[1, 2, 3]);
        let tile = tiles(&stepped)[2];
        let started_from = grown_left(tile, 22);
        let ended_at = grown_left(tile, 107);

        let (mut at_once, _other) = tiled(DefaultLayout::UltrawideVerticalStack, &[1, 2, 3]);
        at_once.resize_by_drag(3, started_from, ended_at).unwrap();

        let before = tiles(&stepped);
        followed(&mut stepped, 3, started_from);
        for by in [40, 75, 107] {
            assert!(step(&mut stepped, grown_left(tile, by)), "step to {by}");

            // the neighbours follow, a step at a time
            assert_ne!(tiles(&stepped), before, "step to {by}");
        }

        assert!(stepped.live_resize_end());
        stepped.resize_by_drag(3, started_from, ended_at).unwrap();

        assert_eq!(tiles(&stepped), tiles(&at_once));
        assert_eq!(
            stepped.focused_workspace().unwrap().latest_layout,
            at_once.focused_workspace().unwrap().latest_layout
        );
        assert!(stepped.live_resize.is_none());
    }

    #[test]
    fn test_a_step_moves_the_neighbours_to_where_the_edge_is() {
        let (mut wm, _context) = tiled(DefaultLayout::BSP, &[1, 2, 3]);
        let before = tiles(&wm);
        let tile = before[1];

        followed(&mut wm, 2, tile);
        assert!(step(&mut wm, grown_left(tile, 60)));

        let after = tiles(&wm);
        assert_eq!(after[1].left, tile.left - 60);
        assert_eq!(after[0].right, before[0].right - 60);
        assert_tiling(&after);

        // the window that is dragged is the one with focus
        assert_eq!(wm.focused_workspace().unwrap().focused_container_idx(), 1);
    }

    #[test]
    fn test_a_drag_that_is_taken_back_to_where_it_began_puts_the_neighbours_back() {
        let (mut wm, _context) = tiled(DefaultLayout::BSP, &[1, 2, 3]);
        let before = tiles(&wm);
        let tile = before[1];

        followed(&mut wm, 2, grown_left(tile, 22));
        assert!(step(&mut wm, grown_left(tile, 120)));
        assert_ne!(tiles(&wm), before);

        // back to the frame that it was first seen in
        assert!(step(&mut wm, grown_left(tile, 22)));

        assert_eq!(tiles(&wm), before);
        assert!(no_resizes(&wm));
    }

    #[test]
    fn test_two_steps_to_the_same_frame_do_not_add_up() {
        let (mut wm, _context) = tiled(DefaultLayout::BSP, &[1, 2, 3]);
        let tile = tiles(&wm)[1];

        followed(&mut wm, 2, tile);
        step(&mut wm, grown_left(tile, 60));
        let once = tiles(&wm);

        step(&mut wm, grown_left(tile, 60));
        step(&mut wm, grown_left(tile, 60));

        assert_eq!(tiles(&wm), once);
    }

    #[test]
    fn test_a_drag_that_only_moved_the_window_lays_nothing_out() {
        let (mut wm, _context) = tiled(DefaultLayout::BSP, &[1, 2, 3]);
        let tile = tiles(&wm)[1];
        followed(&mut wm, 2, tile);

        // the same size, somewhere else: that is a window being moved by its title bar
        let moved = Rect {
            left: tile.left + 80,
            top: tile.top + 40,
            ..tile
        };

        assert!(!step(&mut wm, moved));
        assert!(no_resizes(&wm));

        // and the end of it has nothing to put back
        assert!(!wm.live_resize_end());
    }

    #[test]
    fn test_the_end_of_a_drag_that_was_not_laid_out_for_has_nothing_to_put_back() {
        let (mut wm, _context) = tiled(DefaultLayout::BSP, &[1, 2, 3]);
        let tile = tiles(&wm)[1];
        followed(&mut wm, 2, tile);

        assert!(!wm.live_resize_end());
        assert!(wm.live_resize.is_none());
    }

    #[test]
    fn test_the_end_of_a_drag_puts_the_workspace_back_for_the_drag_to_be_applied_to() {
        let (mut wm, _context) = tiled(DefaultLayout::BSP, &[1, 2, 3]);
        let before = tiles(&wm);
        let tile = before[1];

        followed(&mut wm, 2, tile);
        assert!(step(&mut wm, grown_left(tile, 60)));
        assert_ne!(tiles(&wm), before);

        assert!(wm.live_resize_end());

        assert_eq!(tiles(&wm), before);
        assert_eq!(wm.focused_workspace().unwrap().latest_layout, before);
        assert!(no_resizes(&wm));
    }

    #[test]
    fn test_a_container_that_opens_in_the_middle_of_a_drag_stops_it_being_followed() {
        let (mut wm, _context) = tiled(DefaultLayout::BSP, &[1, 2, 3]);
        let tile = tiles(&wm)[1];

        followed(&mut wm, 2, tile);
        assert!(step(&mut wm, grown_left(tile, 40)));

        wm.focused_workspace_mut()
            .unwrap()
            .new_container_for_window(&Window::from(4))
            .unwrap();
        let resizes = wm.focused_workspace().unwrap().resize_dimensions.clone();

        assert!(!step(&mut wm, grown_left(tile, 80)));
        assert!(wm.live_resize.is_none());

        // what the workspace is now is left as it is, not put back to what it was before
        assert_eq!(wm.focused_workspace().unwrap().resize_dimensions, resizes);
        assert!(!wm.live_resize_end());
    }

    #[test]
    fn test_a_container_that_closes_in_the_middle_of_a_drag_stops_it_being_followed() {
        let (mut wm, _context) = tiled(DefaultLayout::BSP, &[1, 2, 3]);
        let tile = tiles(&wm)[1];

        followed(&mut wm, 2, tile);
        wm.focused_workspace_mut()
            .unwrap()
            .remove_window(3)
            .unwrap();

        assert!(!step(&mut wm, grown_left(tile, 80)));
        assert!(wm.live_resize.is_none());
    }

    #[test]
    fn test_a_step_for_a_window_that_is_not_the_one_that_was_taken_hold_of_is_ignored() {
        let (mut wm, _context) = tiled(DefaultLayout::BSP, &[1, 2, 3]);
        let before = tiles(&wm);
        let tile = before[1];

        // the pending resize is of another window than the one that is followed
        followed(&mut wm, 2, tile);
        *Arc::make_mut(&mut wm.pending_resize_op) = Some((3, Some(before[2])));

        assert!(!step(&mut wm, grown_left(tile, 60)));
        assert_eq!(tiles(&wm), before);

        wm.live_resize_step(3).unwrap();
        assert_eq!(tiles(&wm), before);
    }

    #[test]
    fn test_a_drag_is_only_followed_in_a_workspace_that_is_tiled() {
        let (mut wm, _context) = tiled(DefaultLayout::BSP, &[1, 2, 3]);
        assert!(wm.live_resize_snapshot(2).is_some());

        // a window that is not in a container
        assert!(wm.live_resize_snapshot(99).is_none());

        wm.focused_workspace_mut().unwrap().tile = false;
        assert!(wm.live_resize_snapshot(2).is_none());
        wm.focused_workspace_mut().unwrap().tile = true;

        wm.focused_workspace_mut().unwrap().layer = WorkspaceLayer::Floating;
        assert!(wm.live_resize_snapshot(2).is_none());
        wm.focused_workspace_mut().unwrap().layer = WorkspaceLayer::Tiling;

        wm.focused_workspace_mut().unwrap().monocle_container = Some(Container::default());
        assert!(wm.live_resize_snapshot(2).is_none());
        wm.focused_workspace_mut().unwrap().monocle_container = None;

        wm.focused_workspace_mut().unwrap().maximized_window = Some(Window::from(2));
        assert!(wm.live_resize_snapshot(2).is_none());
    }

    #[test]
    fn test_nothing_is_followed_where_there_is_nothing_that_was_kept() {
        let (mut wm, _context) = tiled(DefaultLayout::BSP, &[1, 2, 3]);
        let before = tiles(&wm);

        assert!(!wm.live_resize_apply(grown_left(before[1], 60)).unwrap());
        assert!(!wm.live_resize_end());
        wm.live_resize_step(2).unwrap();

        assert_eq!(tiles(&wm), before);
    }

    // A drag is measured from the tile that the window was put in, which is where the edge began
    // when it was taken hold of, and not from where the window had got to when the start of the
    // drag was first seen.

    #[test]
    fn test_an_edge_dragged_before_the_start_was_handled_is_measured_from_the_tile() {
        let tile = [27, 27, 623, 725];
        // read after the right edge had gone 7px
        let started = [27, 27, 630, 725];
        let ended = [27, 27, 683, 725];

        assert_eq!(
            WindowManager::drag_origin(started, ended, Some(tile)),
            [27, 27, 623, 725]
        );
    }

    #[test]
    fn test_a_drag_of_under_the_snap_tolerance_is_measured_from_the_tile_too() {
        // dragged by 10, and seen after 3 of them: it used to be 7 that was asked for, since an
        // edge that is not 32 from its tile is taken to be where it began
        let tile = [27, 27, 623, 725];
        let started = [27, 27, 626, 725];
        let ended = [27, 27, 633, 725];

        let origin = WindowManager::drag_origin(started, ended, Some(tile));

        let moved = WindowManager::edges_moved(origin, ended);
        assert!(
            matches!(moved[..], [(OperationDirection::Right, 10)]),
            "{moved:?}"
        );
    }

    #[test]
    fn test_an_edge_that_did_not_move_is_left_as_it_was_read() {
        // a window that cannot be as narrow as its tile, dragged by the other edge
        let tile = [27, 27, 623, 725];
        let started = [27, 27, 650, 725];
        let ended = [27, 27, 650, 760];

        assert_eq!(
            WindowManager::drag_origin(started, ended, Some(tile)),
            [27, 27, 650, 725]
        );
    }

    #[test]
    fn test_an_edge_far_from_its_tile_is_left_as_it_was_read() {
        let tile = [27, 27, 623, 725];
        let started = [27, 27, 800, 725];
        let ended = [27, 27, 830, 725];

        assert_eq!(
            WindowManager::drag_origin(started, ended, Some(tile)),
            started
        );
    }

    #[test]
    fn test_there_is_nothing_to_go_by_without_a_tile() {
        let started = [27, 27, 630, 725];
        let ended = [27, 27, 683, 725];

        assert_eq!(WindowManager::drag_origin(started, ended, None), started);
    }

    #[test]
    fn test_a_drag_that_was_over_when_it_was_first_seen_is_measured_from_the_tile() {
        let tile = [27, 27, 623, 725];
        let seen = [27, 27, 834, 725];

        assert_eq!(
            WindowManager::drag_origin(seen, seen, Some(tile)),
            [27, 27, 623, 725]
        );
    }

    #[test]
    fn test_a_window_that_is_a_little_off_its_tile_has_not_been_dragged() {
        // nothing moved, and the offset is what an app does on its own
        let tile = [27, 27, 623, 725];
        let seen = [27, 27, 640, 725];

        let origin = WindowManager::drag_origin(seen, seen, Some(tile));

        assert_eq!(origin, seen);
        assert!(WindowManager::edges_moved(origin, seen).is_empty());
    }

    // The same, for the Scrolling layout, whose tiles are not worked out from the resizes of the
    // workspace but from the strip that it keeps.

    /// A monitor with a workspace in the Scrolling layout, with a container per window id, the
    /// last of them focused, laid out as a window update would
    fn scrolled(ids: &[u32]) -> (WindowManager, TestContext) {
        let (mut wm, context) = setup_window_manager();

        let m = monitor::new(
            0,
            WORK_AREA,
            WORK_AREA,
            "TestMonitor".to_string(),
            "TestDeviceID".to_string(),
        );
        wm.monitors_mut().push_back(m);

        let workspace = wm.focused_workspace_mut().unwrap();
        workspace.layout = Layout::Default(DefaultLayout::Scrolling);
        for id in ids {
            workspace
                .new_container_for_window(&Window::from(*id))
                .unwrap();
        }

        scroll_layout(&mut wm);

        (wm, context)
    }

    /// Lay the strip out and record the tiles, as updating the workspace does
    fn scroll_layout(wm: &mut WindowManager) -> Vec<Rect> {
        let workspace = wm.focused_workspace_mut().unwrap();
        let rects: Vec<Rect> = workspace
            .layout_scrolling(&WORK_AREA, 0)
            .iter()
            .map(|placement| placement.rect)
            .collect();
        workspace.latest_layout = rects.clone();

        rects
    }

    fn scroll_step(wm: &mut WindowManager, now: Rect) -> Vec<Rect> {
        assert!(wm.live_resize_apply(now).unwrap());

        scroll_layout(wm)
    }

    fn grown_right(r: Rect, by: i32) -> Rect {
        Rect {
            right: r.right + by,
            ..r
        }
    }

    #[test]
    fn test_the_steps_of_a_drag_of_a_column_do_not_add_up_and_a_drag_back_restores_the_strip() {
        let (mut wm, _context) = scrolled(&[1, 2, 3, 4, 5]);
        let before = scroll_layout(&mut wm);
        let tile = before[3];

        followed(&mut wm, 4, tile);

        // a column is as much wider as the edge was dragged, and not by the sum of the steps
        for by in [40, 100, 100] {
            let rects = scroll_step(&mut wm, grown_right(tile, by));

            assert_eq!(rects[3].right, tile.right + by, "dragged by {by}");
        }

        // and taken back
        let rects = scroll_step(&mut wm, tile);
        assert_eq!(rects, before);

        assert!(wm.live_resize_end());
        assert_eq!(scroll_layout(&mut wm), before);
    }

    #[test]
    fn test_the_left_edge_of_a_column_that_is_dragged_stays_where_it_is_dragged_to() {
        let (mut wm, _context) = scrolled(&[1, 2, 3, 4, 5]);

        // the strip is at its start, with the first column the one that the strip says is active,
        // and the second is the one that is dragged: dragging an edge only moves the focus of the
        // workspace to it, and not that of the strip
        wm.focused_workspace_mut().unwrap().focus_container(0);
        let before = scroll_layout(&mut wm);
        let tile = before[1];
        assert_eq!(tile.left, 640);
        followed(&mut wm, 2, tile);

        let rects = scroll_step(&mut wm, grown_left(tile, 60));

        // the edge goes as far as it was dragged, and the other is where it was
        assert_eq!(rects[1].left, tile.left - 60);
        assert_eq!(rects[1].left + rects[1].right, tile.left + tile.right);
        assert_eq!(rects[1].right, tile.right + 60);
    }

    #[test]
    fn test_the_left_edge_of_the_last_column_that_is_dragged_stays_where_it_is_dragged_to() {
        let (mut wm, _context) = scrolled(&[1, 2, 3, 4, 5]);
        let before = scroll_layout(&mut wm);
        let tile = before[3];
        followed(&mut wm, 4, tile);

        let rects = scroll_step(&mut wm, grown_left(tile, 60));

        assert_eq!(rects[3].left, tile.left - 60);
        assert_eq!(rects[3].left + rects[3].right, tile.left + tile.right);
    }
}
