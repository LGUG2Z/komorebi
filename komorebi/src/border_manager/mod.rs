#![deny(clippy::unwrap_used, clippy::expect_used)]

mod border;
use crate::core::BorderImplementation;
use crate::core::BorderStyle;
use crate::core::WindowKind;
use crate::ring::Ring;
use crate::windows_api;
use crate::workspace::Workspace;
use crate::workspace::WorkspaceLayer;
use crate::WindowManager;
use crate::WindowsApi;
use border::border_hwnds;
pub use border::Border;
use crossbeam_channel::Receiver;
use crossbeam_channel::Sender;
use crossbeam_utils::atomic::AtomicCell;
use crossbeam_utils::atomic::AtomicConsume;
use komorebi_themes::colour::Colour;
use komorebi_themes::colour::Rgb;
use lazy_static::lazy_static;
use parking_lot::Mutex;
use serde::Deserialize;
use serde::Serialize;
use std::collections::hash_map::Entry;
use std::collections::HashMap;
use std::ops::Deref;
use std::sync::atomic::AtomicBool;
use std::sync::atomic::AtomicI32;
use std::sync::atomic::AtomicU32;
use std::sync::atomic::Ordering;
use std::sync::Arc;
use std::sync::OnceLock;
use strum::Display;
use windows::Win32::Foundation::HWND;
use windows::Win32::Graphics::Direct2D::ID2D1HwndRenderTarget;

pub static BORDER_WIDTH: AtomicI32 = AtomicI32::new(8);
pub static BORDER_OFFSET: AtomicI32 = AtomicI32::new(-1);

pub static BORDER_ENABLED: AtomicBool = AtomicBool::new(true);

lazy_static! {
    pub static ref STYLE: AtomicCell<BorderStyle> = AtomicCell::new(BorderStyle::System);
    pub static ref IMPLEMENTATION: AtomicCell<BorderImplementation> =
        AtomicCell::new(BorderImplementation::Komorebi);
    pub static ref FOCUSED: AtomicU32 =
        AtomicU32::new(u32::from(Colour::Rgb(Rgb::new(66, 165, 245))));
    pub static ref UNFOCUSED: AtomicU32 =
        AtomicU32::new(u32::from(Colour::Rgb(Rgb::new(128, 128, 128))));
    pub static ref UNFOCUSED_LOCKED: AtomicU32 =
        AtomicU32::new(u32::from(Colour::Rgb(Rgb::new(158, 8, 8))));
    pub static ref MONOCLE: AtomicU32 =
        AtomicU32::new(u32::from(Colour::Rgb(Rgb::new(255, 51, 153))));
    pub static ref STACK: AtomicU32 = AtomicU32::new(u32::from(Colour::Rgb(Rgb::new(0, 165, 66))));
    pub static ref FLOATING: AtomicU32 =
        AtomicU32::new(u32::from(Colour::Rgb(Rgb::new(245, 245, 165))));
}

lazy_static! {
    static ref BORDER_STATE: Mutex<HashMap<String, Box<Border>>> = Mutex::new(HashMap::new());
    static ref WINDOWS_BORDERS: Mutex<HashMap<isize, String>> = Mutex::new(HashMap::new());
}

#[derive(Debug, Clone)]
pub struct RenderTarget(pub ID2D1HwndRenderTarget);
unsafe impl Send for RenderTarget {}

impl Deref for RenderTarget {
    type Target = ID2D1HwndRenderTarget;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

pub enum Notification {
    Update(Option<isize>),
    ForceUpdate,
}

#[derive(Debug, Default, Clone, Copy, PartialEq)]
pub struct BorderInfo {
    pub border_hwnd: isize,
    pub window_kind: WindowKind,
}

impl BorderInfo {
    pub fn hwnd(&self) -> HWND {
        HWND(windows_api::as_ptr!(self.border_hwnd))
    }
}

static CHANNEL: OnceLock<(Sender<Notification>, Receiver<Notification>)> = OnceLock::new();

pub fn channel() -> &'static (Sender<Notification>, Receiver<Notification>) {
    CHANNEL.get_or_init(|| crossbeam_channel::bounded(50))
}

fn event_tx() -> Sender<Notification> {
    channel().0.clone()
}

fn event_rx() -> Receiver<Notification> {
    channel().1.clone()
}

pub fn window_border(hwnd: isize) -> Option<BorderInfo> {
    let id = WINDOWS_BORDERS.lock().get(&hwnd)?.clone();
    BORDER_STATE.lock().get(&id).map(|b| BorderInfo {
        border_hwnd: b.hwnd,
        window_kind: b.window_kind,
    })
}

pub fn send_notification(hwnd: Option<isize>) {
    if event_tx().try_send(Notification::Update(hwnd)).is_err() {
        tracing::warn!("channel is full; dropping notification")
    }
}

pub fn send_force_update() {
    if event_tx().try_send(Notification::ForceUpdate).is_err() {
        tracing::warn!("channel is full; dropping notification")
    }
}

pub fn destroy_all_borders() -> color_eyre::Result<()> {
    let mut borders = BORDER_STATE.lock();
    tracing::info!(
        "purging known borders: {:?}",
        borders.iter().map(|b| b.1.hwnd).collect::<Vec<_>>()
    );

    for (_, border) in borders.drain() {
        let _ = destroy_border(border);
    }

    drop(borders);

    WINDOWS_BORDERS.lock().clear();

    let mut remaining_hwnds = vec![];

    WindowsApi::enum_windows(
        Some(border_hwnds),
        &mut remaining_hwnds as *mut Vec<isize> as isize,
    )?;

    if !remaining_hwnds.is_empty() {
        tracing::info!("purging unknown borders: {:?}", remaining_hwnds);

        for hwnd in remaining_hwnds {
            let _ = destroy_border(Box::new(Border::from(hwnd)));
        }
    }

    Ok(())
}

fn window_kind_colour(focus_kind: WindowKind) -> u32 {
    match focus_kind {
        WindowKind::Unfocused => UNFOCUSED.load(Ordering::Relaxed),
        WindowKind::UnfocusedLocked => UNFOCUSED_LOCKED.load(Ordering::Relaxed),
        WindowKind::Single => FOCUSED.load(Ordering::Relaxed),
        WindowKind::Stack => STACK.load(Ordering::Relaxed),
        WindowKind::Monocle => MONOCLE.load(Ordering::Relaxed),
        WindowKind::Floating => FLOATING.load(Ordering::Relaxed),
    }
}

pub fn listen_for_notifications(wm: Arc<Mutex<WindowManager>>) {
    std::thread::spawn(move || loop {
        match handle_notifications(wm.clone()) {
            Ok(()) => {
                tracing::warn!("restarting finished thread");
            }
            Err(error) => {
                tracing::warn!("restarting failed thread: {}", error);
            }
        }
    });
}

pub fn handle_notifications(wm: Arc<Mutex<WindowManager>>) -> color_eyre::Result<()> {
    tracing::info!("listening");

    let receiver = event_rx();
    event_tx().send(Notification::Update(None))?;

    let mut previous_snapshot = Ring::default();
    let mut previous_pending_move_op = None;
    let mut previous_is_paused = false;
    let mut previous_notification: Option<Notification> = None;
    let mut previous_layer = WorkspaceLayer::default();

    'receiver: for notification in receiver {
        // Check the wm state every time we receive a notification
        let state = wm.lock();
        let is_paused = state.is_paused;
        let focused_monitor_idx = state.focused_monitor_idx();
        let focused_workspace_idx =
            state.monitors.elements()[focused_monitor_idx].focused_workspace_idx();
        let monitors = state.monitors.clone();
        let pending_move_op = *state.pending_move_op;
        let floating_window_hwnds = state.monitors.elements()[focused_monitor_idx].workspaces()
            [focused_workspace_idx]
            .floating_windows()
            .iter()
            .map(|w| w.hwnd)
            .collect::<Vec<_>>();
        let workspace_layer = *state.monitors.elements()[focused_monitor_idx].workspaces()
            [focused_workspace_idx]
            .layer();
        let foreground_window = WindowsApi::foreground_window().unwrap_or_default();
        let layer_changed = previous_layer != workspace_layer;
        let forced_update = matches!(notification, Notification::ForceUpdate);

        drop(state);

        match IMPLEMENTATION.load() {
            BorderImplementation::Windows => {
                'monitors: for (monitor_idx, m) in monitors.elements().iter().enumerate() {
                    // Only operate on the focused workspace of each monitor
                    if let Some(ws) = m.focused_workspace() {
                        // Handle the monocle container separately
                        if let Some(monocle) = ws.monocle_container() {
                            let window_kind = if monitor_idx != focused_monitor_idx {
                                WindowKind::Unfocused
                            } else {
                                WindowKind::Monocle
                            };

                            monocle
                                .focused_window()
                                .copied()
                                .unwrap_or_default()
                                .set_accent(window_kind_colour(window_kind))?;

                            if ws.layer() == &WorkspaceLayer::Floating {
                                for window in ws.floating_windows() {
                                    let mut window_kind = WindowKind::Unfocused;

                                    if foreground_window == window.hwnd {
                                        window_kind = WindowKind::Floating;
                                    }

                                    window.set_accent(window_kind_colour(window_kind))?;
                                }
                            }
                            continue 'monitors;
                        }

                        for (idx, c) in ws.containers().iter().enumerate() {
                            let window_kind = if idx != ws.focused_container_idx()
                                || monitor_idx != focused_monitor_idx
                            {
                                if c.locked() {
                                    WindowKind::UnfocusedLocked
                                } else {
                                    WindowKind::Unfocused
                                }
                            } else if c.windows().len() > 1 {
                                WindowKind::Stack
                            } else {
                                WindowKind::Single
                            };

                            c.focused_window()
                                .copied()
                                .unwrap_or_default()
                                .set_accent(window_kind_colour(window_kind))?;
                        }

                        for window in ws.floating_windows() {
                            let mut window_kind = WindowKind::Unfocused;

                            if foreground_window == window.hwnd {
                                window_kind = WindowKind::Floating;
                            }

                            window.set_accent(window_kind_colour(window_kind))?;
                        }
                    }
                }
            }
            BorderImplementation::Komorebi => {
                let should_process_notification = match notification {
                    Notification::Update(notification_hwnd) => {
                        let mut should_process_notification = true;

                        if monitors == previous_snapshot
                        // handle the window dragging edge case
                        && pending_move_op == previous_pending_move_op
                        {
                            should_process_notification = false;
                        }

                        // handle the pause edge case
                        if is_paused && !previous_is_paused {
                            should_process_notification = true;
                        }

                        // handle the unpause edge case
                        if previous_is_paused && !is_paused {
                            should_process_notification = true;
                        }

                        // handle the retile edge case
                        if !should_process_notification && BORDER_STATE.lock().is_empty() {
                            should_process_notification = true;
                        }

                        // when we switch focus to/from a floating window
                        let switch_focus_to_from_floating_window =
                            floating_window_hwnds.iter().any(|fw| {
                                // if we switch focus to a floating window
                                fw == &notification_hwnd.unwrap_or_default() ||
                        // if there is any floating window with a `WindowKind::Floating` border
                        // that no longer is the foreground window then we need to update that
                        // border.
                        (fw != &foreground_window
                            && window_border(*fw)
                            .is_some_and(|b| b.window_kind == WindowKind::Floating))
                            });

                        // when the focused window has an `Unfocused` border kind, usually this happens if
                        // we focus an admin window and then refocus the previously focused window. For
                        // komorebi it will have the same state has before, however the previously focused
                        // window changed its border to unfocused so now we need to update it again.
                        if !should_process_notification
                            && window_border(notification_hwnd.unwrap_or_default())
                                .is_some_and(|b| b.window_kind == WindowKind::Unfocused)
                        {
                            should_process_notification = true;
                        }

                        if !should_process_notification && switch_focus_to_from_floating_window {
                            should_process_notification = true;
                        }

                        if !should_process_notification {
                            if let Some(Notification::Update(ref previous)) = previous_notification
                            {
                                if previous.unwrap_or_default()
                                    != notification_hwnd.unwrap_or_default()
                                {
                                    should_process_notification = true;
                                }
                            }
                        }

                        should_process_notification
                    }
                    Notification::ForceUpdate => true,
                };

                if !should_process_notification {
                    tracing::debug!("monitor state matches latest snapshot, skipping notification");
                    continue 'receiver;
                }

                let mut borders = BORDER_STATE.lock();
                let mut windows_borders = WINDOWS_BORDERS.lock();

                // If borders are disabled
                if !BORDER_ENABLED.load_consume()
                    // Or if the wm is paused
                    || is_paused
                {
                    // Destroy the borders we know about
                    for (_, border) in borders.drain() {
                        destroy_border(border)?;
                    }

                    windows_borders.clear();

                    previous_is_paused = is_paused;
                    continue 'receiver;
                }

                'monitors: for (monitor_idx, m) in monitors.elements().iter().enumerate() {
                    // Only operate on the focused workspace of each monitor
                    if let Some(ws) = m.focused_workspace() {
                        // Workspaces with tiling disabled don't have borders
                        if !ws.tile() {
                            // Remove all borders on this monitor
                            remove_borders(
                                &mut borders,
                                &mut windows_borders,
                                monitor_idx,
                                |_, _| true,
                            )?;

                            continue 'monitors;
                        }

                        // Handle the monocle container separately
                        if let Some(monocle) = ws.monocle_container() {
                            let mut new_border = false;
                            let focused_window_hwnd =
                                monocle.focused_window().map(|w| w.hwnd).unwrap_or_default();
                            let id = monocle.id().clone();
                            let border = match borders.entry(id.clone()) {
                                Entry::Occupied(entry) => entry.into_mut(),
                                Entry::Vacant(entry) => {
                                    if let Ok(border) = Border::create(
                                        monocle.id(),
                                        focused_window_hwnd,
                                        monitor_idx,
                                    ) {
                                        new_border = true;
                                        entry.insert(border)
                                    } else {
                                        continue 'monitors;
                                    }
                                }
                            };

                            let new_focus_state = if monitor_idx != focused_monitor_idx {
                                WindowKind::Unfocused
                            } else {
                                WindowKind::Monocle
                            };
                            border.window_kind = new_focus_state;

                            // Update the borders tracking_hwnd in case it changed and remove the
                            // old `tracking_hwnd` from `WINDOWS_BORDERS` if needed.
                            if border.tracking_hwnd != focused_window_hwnd {
                                if let Some(previous) = windows_borders.get(&border.tracking_hwnd) {
                                    // Only remove the border from `windows_borders` if it
                                    // still corresponds to the same border, if doesn't then
                                    // it means it was already updated by another border for
                                    // that window and in that case we don't want to remove it.
                                    if previous == &id {
                                        windows_borders.remove(&border.tracking_hwnd);
                                    }
                                }
                                border.tracking_hwnd = focused_window_hwnd;
                                if !WindowsApi::is_window_visible(border.hwnd) {
                                    WindowsApi::restore_window(border.hwnd);
                                }
                            }

                            // Update the border's monitor idx in case it changed
                            border.monitor_idx = Some(monitor_idx);

                            let rect = WindowsApi::window_rect(focused_window_hwnd)?;
                            border.window_rect = rect;

                            if new_border {
                                border.set_position(&rect, focused_window_hwnd)?;
                            } else if matches!(notification, Notification::ForceUpdate) {
                                // Update the border brushes if there was a forced update
                                // notification and this is not a new border (new border's
                                // already have their brushes updated on creation)
                                border.update_brushes()?;
                            }

                            border.invalidate();

                            windows_borders.insert(focused_window_hwnd, id);

                            let border_hwnd = border.hwnd;

                            if ws.layer() == &WorkspaceLayer::Floating {
                                handle_floating_borders(
                                    &mut borders,
                                    &mut windows_borders,
                                    ws,
                                    monitor_idx,
                                    foreground_window,
                                    layer_changed,
                                    forced_update,
                                )?;

                                // Remove all borders on this monitor except monocle and floating borders
                                remove_borders(
                                    &mut borders,
                                    &mut windows_borders,
                                    monitor_idx,
                                    |_, b| {
                                        border_hwnd != b.hwnd
                                            && !ws
                                                .floating_windows()
                                                .iter()
                                                .any(|w| w.hwnd == b.tracking_hwnd)
                                    },
                                )?;
                            } else {
                                // Remove all borders on this monitor except monocle
                                remove_borders(
                                    &mut borders,
                                    &mut windows_borders,
                                    monitor_idx,
                                    |_, b| border_hwnd != b.hwnd,
                                )?;
                            }
                            continue 'monitors;
                        }

                        let foreground_hwnd = WindowsApi::foreground_window().unwrap_or_default();
                        let foreground_monitor_id =
                            WindowsApi::monitor_from_window(foreground_hwnd);
                        let is_maximized = foreground_monitor_id == m.id()
                            && WindowsApi::is_zoomed(foreground_hwnd);

                        if is_maximized {
                            // Remove all borders on this monitor
                            remove_borders(
                                &mut borders,
                                &mut windows_borders,
                                monitor_idx,
                                |_, _| true,
                            )?;

                            continue 'monitors;
                        }

                        // Collect focused workspace container and floating windows ID's
                        let mut container_and_floating_window_ids = ws
                            .containers()
                            .iter()
                            .map(|c| c.id().clone())
                            .collect::<Vec<_>>();

                        for w in ws.floating_windows() {
                            container_and_floating_window_ids.push(w.hwnd.to_string());
                        }

                        // Remove any borders not associated with the focused workspace
                        remove_borders(
                            &mut borders,
                            &mut windows_borders,
                            monitor_idx,
                            |id, _| !container_and_floating_window_ids.contains(id),
                        )?;

                        'containers: for (idx, c) in ws.containers().iter().enumerate() {
                            let focused_window_hwnd =
                                c.focused_window().map(|w| w.hwnd).unwrap_or_default();
                            let id = c.id().clone();

                            // Get the border entry for this container from the map or create one
                            let mut new_border = false;
                            let border = match borders.entry(id.clone()) {
                                Entry::Occupied(entry) => entry.into_mut(),
                                Entry::Vacant(entry) => {
                                    if let Ok(border) =
                                        Border::create(c.id(), focused_window_hwnd, monitor_idx)
                                    {
                                        new_border = true;
                                        entry.insert(border)
                                    } else {
                                        continue 'monitors;
                                    }
                                }
                            };

                            let last_focus_state = border.window_kind;

                            let new_focus_state = if idx != ws.focused_container_idx()
                                || monitor_idx != focused_monitor_idx
                                || focused_window_hwnd != foreground_window
                            {
                                if c.locked() {
                                    WindowKind::UnfocusedLocked
                                } else {
                                    WindowKind::Unfocused
                                }
                            } else if c.windows().len() > 1 {
                                WindowKind::Stack
                            } else {
                                WindowKind::Single
                            };

                            border.window_kind = new_focus_state;

                            // Update the borders `tracking_hwnd` in case it changed and remove the
                            // old `tracking_hwnd` from `WINDOWS_BORDERS` if needed.
                            if border.tracking_hwnd != focused_window_hwnd {
                                if let Some(previous) = windows_borders.get(&border.tracking_hwnd) {
                                    // Only remove the border from `windows_borders` if it
                                    // still corresponds to the same border, if doesn't then
                                    // it means it was already updated by another border for
                                    // that window and in that case we don't want to remove it.
                                    if previous == &id {
                                        windows_borders.remove(&border.tracking_hwnd);
                                    }
                                }
                                border.tracking_hwnd = focused_window_hwnd;
                                if !WindowsApi::is_window_visible(border.hwnd) {
                                    WindowsApi::restore_window(border.hwnd);
                                }
                            }

                            // Update the border's monitor idx in case it changed
                            border.monitor_idx = Some(monitor_idx);

                            // avoid getting into a thread restart loop if we try to look up
                            // rect info for a window that has been destroyed by the time
                            // we get here
                            let rect = match WindowsApi::window_rect(focused_window_hwnd) {
                                Ok(rect) => rect,
                                Err(_) => {
                                    remove_border(c.id(), &mut borders, &mut windows_borders)?;
                                    continue 'containers;
                                }
                            };
                            border.window_rect = rect;

                            let should_invalidate = new_border
                                || (last_focus_state != new_focus_state)
                                || layer_changed
                                || forced_update;

                            if should_invalidate {
                                if forced_update && !new_border {
                                    // Update the border brushes if there was a forced update
                                    // notification and this is not a new border (new border's
                                    // already have their brushes updated on creation)
                                    border.update_brushes()?;
                                }
                                border.set_position(&rect, focused_window_hwnd)?;
                                border.invalidate();
                            }

                            windows_borders.insert(focused_window_hwnd, id);
                        }

                        handle_floating_borders(
                            &mut borders,
                            &mut windows_borders,
                            ws,
                            monitor_idx,
                            foreground_window,
                            layer_changed,
                            forced_update,
                        )?;
                    }
                }
            }
        }

        previous_snapshot = monitors;
        previous_pending_move_op = pending_move_op;
        previous_is_paused = is_paused;
        previous_notification = Some(notification);
        previous_layer = workspace_layer;
    }

    Ok(())
}

fn handle_floating_borders(
    borders: &mut HashMap<String, Box<Border>>,
    windows_borders: &mut HashMap<isize, String>,
    ws: &Workspace,
    monitor_idx: usize,
    foreground_window: isize,
    layer_changed: bool,
    forced_update: bool,
) -> color_eyre::Result<()> {
    for window in ws.floating_windows() {
        let mut new_border = false;
        let id = window.hwnd.to_string();
        let border = match borders.entry(id.clone()) {
            Entry::Occupied(entry) => entry.into_mut(),
            Entry::Vacant(entry) => {
                if let Ok(border) =
                    Border::create(&window.hwnd.to_string(), window.hwnd, monitor_idx)
                {
                    new_border = true;
                    entry.insert(border)
                } else {
                    return Ok(());
                }
            }
        };

        let last_focus_state = border.window_kind;

        let new_focus_state = if foreground_window == window.hwnd {
            WindowKind::Floating
        } else {
            WindowKind::Unfocused
        };

        border.window_kind = new_focus_state;

        // Update the border's monitor idx in case it changed
        border.monitor_idx = Some(monitor_idx);

        let rect = WindowsApi::window_rect(window.hwnd)?;
        border.window_rect = rect;

        let should_invalidate =
            new_border || (last_focus_state != new_focus_state) || layer_changed || forced_update;

        if should_invalidate {
            if forced_update && !new_border {
                // Update the border brushes if there was a forced update
                // notification and this is not a new border (new border's
                // already have their brushes updated on creation)
                border.update_brushes()?;
            }
            border.set_position(&rect, window.hwnd)?;
            border.invalidate();
        }

        windows_borders.insert(window.hwnd, id);
    }

    Ok(())
}

/// Removes all borders from monitor with index `monitor_idx` filtered by
/// `condition`. This condition is a function that will take a reference to
/// the container id and the border and returns a bool, if true that border
/// will be removed.
fn remove_borders(
    borders: &mut HashMap<String, Box<Border>>,
    windows_borders: &mut HashMap<isize, String>,
    monitor_idx: usize,
    condition: impl Fn(&String, &Border) -> bool,
) -> color_eyre::Result<()> {
    let mut to_remove = vec![];
    for (id, border) in borders.iter() {
        // if border is on this monitor
        if border.monitor_idx.is_some_and(|idx| idx == monitor_idx)
            // and the condition applies
            && condition(id, border)
            // and the border is visible (we don't remove hidden borders)
            && WindowsApi::is_window_visible(border.hwnd)
        {
            // we mark it to be removed
            to_remove.push(id.clone());
        }
    }

    for id in &to_remove {
        remove_border(id, borders, windows_borders)?;
    }

    Ok(())
}

/// Removes the border with `id` and all its related info from all maps
fn remove_border(
    id: &str,
    borders: &mut HashMap<String, Box<Border>>,
    windows_borders: &mut HashMap<isize, String>,
) -> color_eyre::Result<()> {
    if let Some(removed_border) = borders.remove(id) {
        windows_borders.remove(&removed_border.tracking_hwnd);
        destroy_border(removed_border)?;
    }

    Ok(())
}

/// IMPORTANT: BEWARE when changing this function. We need to make sure that we don't let the
/// `Box<Border>` be dropped normally. We need to turn the `Box` into the raw pointer and use that
/// pointer to call the `.destroy()` funtion of the border so it closes the window. This way the
/// `Box` is consumed and the pointer is dropped like a normal `Copy` number instead of trying to
/// drop the struct it points to. The actual border is owned by the thread that created the window
/// and once the window closes that thread gets out of its loop, finishes and properly disposes of
/// the border.
fn destroy_border(border: Box<Border>) -> color_eyre::Result<()> {
    let raw_pointer = Box::into_raw(border);
    unsafe {
        (*raw_pointer).destroy()?;
    }
    Ok(())
}

/// Removes the border around window with `tracking_hwnd` if it exists
pub fn delete_border(tracking_hwnd: isize) {
    std::thread::spawn(move || {
        let id = {
            WINDOWS_BORDERS
                .lock()
                .get(&tracking_hwnd)
                .cloned()
                .unwrap_or_default()
        };

        let mut borders = BORDER_STATE.lock();
        let mut windows_borders = WINDOWS_BORDERS.lock();

        if let Err(error) = remove_border(&id, &mut borders, &mut windows_borders) {
            tracing::error!("Failed to delete border: {}", error);
        }
    });
}

/// Shows the border around window with `tracking_hwnd` if it exists
pub fn show_border(tracking_hwnd: isize) {
    std::thread::spawn(move || {
        if let Some(border_info) = window_border(tracking_hwnd) {
            WindowsApi::restore_window(border_info.border_hwnd);
        }
    });
}

/// Hides the border around window with `tracking_hwnd` if it exists, unless the border kind is a
/// `Stack` border.
pub fn hide_border(tracking_hwnd: isize) {
    std::thread::spawn(move || {
        if let Some(border_info) = window_border(tracking_hwnd) {
            WindowsApi::hide_window(border_info.border_hwnd);
        }
    });
}

#[derive(Debug, Copy, Clone, Display, Serialize, Deserialize, PartialEq)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
pub enum ZOrder {
    Top,
    NoTopMost,
    Bottom,
    TopMost,
}

impl From<ZOrder> for isize {
    fn from(val: ZOrder) -> Self {
        match val {
            ZOrder::Top => 0,
            ZOrder::NoTopMost => -2,
            ZOrder::Bottom => 1,
            ZOrder::TopMost => -1,
        }
    }
}
