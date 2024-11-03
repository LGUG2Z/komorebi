#![deny(clippy::unwrap_used, clippy::expect_used)]

mod border;
use crate::core::BorderImplementation;
use crate::core::BorderStyle;
use crate::core::WindowKind;
use crate::ring::Ring;
use crate::workspace_reconciliator::ALT_TAB_HWND;
use crate::Colour;
use crate::Rgb;
use crate::WindowManager;
use crate::WindowsApi;
use border::border_hwnds;
use border::Border;
use crossbeam_channel::Receiver;
use crossbeam_channel::Sender;
use crossbeam_utils::atomic::AtomicCell;
use crossbeam_utils::atomic::AtomicConsume;
use lazy_static::lazy_static;
use parking_lot::Mutex;
use schemars::JsonSchema;
use serde::Deserialize;
use serde::Serialize;
use std::collections::hash_map::Entry;
use std::collections::HashMap;
use std::sync::atomic::AtomicBool;
use std::sync::atomic::AtomicI32;
use std::sync::atomic::AtomicU32;
use std::sync::atomic::Ordering;
use std::sync::Arc;
use std::sync::OnceLock;
use windows::Win32::Graphics::Direct2D::ID2D1HwndRenderTarget;
use windows::Win32::System::Threading::GetCurrentThread;
use windows::Win32::System::Threading::SetThreadPriority;
use windows::Win32::System::Threading::THREAD_PRIORITY_TIME_CRITICAL;

pub static BORDER_WIDTH: AtomicI32 = AtomicI32::new(8);
pub static BORDER_OFFSET: AtomicI32 = AtomicI32::new(-1);

pub static BORDER_ENABLED: AtomicBool = AtomicBool::new(true);
pub static BORDER_TEMPORARILY_DISABLED: AtomicBool = AtomicBool::new(false);

lazy_static! {
    pub static ref Z_ORDER: AtomicCell<ZOrder> = AtomicCell::new(ZOrder::Bottom);
    pub static ref STYLE: AtomicCell<BorderStyle> = AtomicCell::new(BorderStyle::System);
    pub static ref IMPLEMENTATION: AtomicCell<BorderImplementation> =
        AtomicCell::new(BorderImplementation::Komorebi);
    pub static ref FOCUSED: AtomicU32 =
        AtomicU32::new(u32::from(Colour::Rgb(Rgb::new(66, 165, 245))));
    pub static ref UNFOCUSED: AtomicU32 =
        AtomicU32::new(u32::from(Colour::Rgb(Rgb::new(128, 128, 128))));
    pub static ref MONOCLE: AtomicU32 =
        AtomicU32::new(u32::from(Colour::Rgb(Rgb::new(255, 51, 153))));
    pub static ref STACK: AtomicU32 = AtomicU32::new(u32::from(Colour::Rgb(Rgb::new(0, 165, 66))));
    pub static ref FLOATING: AtomicU32 =
        AtomicU32::new(u32::from(Colour::Rgb(Rgb::new(245, 245, 165))));
}

lazy_static! {
    static ref BORDERS_MONITORS: Mutex<HashMap<String, usize>> = Mutex::new(HashMap::new());
    static ref BORDER_STATE: Mutex<HashMap<String, Border>> = Mutex::new(HashMap::new());
    static ref FOCUS_STATE: Mutex<HashMap<isize, WindowKind>> = Mutex::new(HashMap::new());
    static ref RENDER_TARGETS: Mutex<HashMap<isize, ID2D1HwndRenderTarget>> =
        Mutex::new(HashMap::new());
}

pub struct Notification(pub Option<isize>);

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

pub fn send_notification(hwnd: Option<isize>) {
    if event_tx().try_send(Notification(hwnd)).is_err() {
        tracing::warn!("channel is full; dropping notification")
    }
}

pub fn destroy_all_borders() -> color_eyre::Result<()> {
    let mut borders = BORDER_STATE.lock();
    tracing::info!(
        "purging known borders: {:?}",
        borders.iter().map(|b| b.1.hwnd).collect::<Vec<_>>()
    );

    for (_, border) in borders.iter() {
        let _ = border.destroy();
    }

    borders.clear();
    BORDERS_MONITORS.lock().clear();
    FOCUS_STATE.lock().clear();
    RENDER_TARGETS.lock().clear();

    let mut remaining_hwnds = vec![];

    WindowsApi::enum_windows(
        Some(border_hwnds),
        &mut remaining_hwnds as *mut Vec<isize> as isize,
    )?;

    if !remaining_hwnds.is_empty() {
        tracing::info!("purging unknown borders: {:?}", remaining_hwnds);

        for hwnd in remaining_hwnds {
            let _ = Border::from(hwnd).destroy();
        }
    }

    Ok(())
}

fn window_kind_colour(focus_kind: WindowKind) -> u32 {
    match focus_kind {
        WindowKind::Unfocused => UNFOCUSED.load(Ordering::SeqCst),
        WindowKind::Single => FOCUSED.load(Ordering::SeqCst),
        WindowKind::Stack => STACK.load(Ordering::SeqCst),
        WindowKind::Monocle => MONOCLE.load(Ordering::SeqCst),
        WindowKind::Floating => FLOATING.load(Ordering::SeqCst),
    }
}

pub fn listen_for_notifications(wm: Arc<Mutex<WindowManager>>) {
    std::thread::spawn(move || {
        unsafe {
            if let Err(error) = SetThreadPriority(GetCurrentThread(), THREAD_PRIORITY_TIME_CRITICAL)
            {
                tracing::error!("{error}");
            }
        }

        loop {
            match handle_notifications(wm.clone()) {
                Ok(()) => {
                    tracing::warn!("restarting finished thread");
                }
                Err(error) => {
                    tracing::warn!("restarting failed thread: {}", error);
                }
            }
        }
    });
}

pub fn handle_notifications(wm: Arc<Mutex<WindowManager>>) -> color_eyre::Result<()> {
    tracing::info!("listening");

    BORDER_TEMPORARILY_DISABLED.store(false, Ordering::SeqCst);
    let receiver = event_rx();
    event_tx().send(Notification(None))?;

    let mut previous_snapshot = Ring::default();
    let mut previous_pending_move_op = None;
    let mut previous_is_paused = false;
    let mut previous_notification: Option<Notification> = None;

    'receiver: for notification in receiver {
        // Check the wm state every time we receive a notification
        let state = wm.lock();
        let is_paused = state.is_paused;
        let focused_monitor_idx = state.focused_monitor_idx();
        let focused_workspace_idx =
            state.monitors.elements()[focused_monitor_idx].focused_workspace_idx();
        let monitors = state.monitors.clone();
        let weak_pending_move_op = Arc::downgrade(&state.pending_move_op);
        let pending_move_op = *state.pending_move_op;
        let floating_window_hwnds = state.monitors.elements()[focused_monitor_idx].workspaces()
            [focused_workspace_idx]
            .floating_windows()
            .iter()
            .map(|w| w.hwnd)
            .collect::<Vec<_>>();

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

                            continue 'monitors;
                        }

                        for (idx, c) in ws.containers().iter().enumerate() {
                            let window_kind = if idx != ws.focused_container_idx()
                                || monitor_idx != focused_monitor_idx
                            {
                                WindowKind::Unfocused
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
                    }
                }
            }
            BorderImplementation::Komorebi => {
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

                // when we switch focus to a floating window
                if !should_process_notification
                    && floating_window_hwnds.contains(&notification.0.unwrap_or_default())
                {
                    should_process_notification = true;
                }

                if !should_process_notification {
                    if let Some(ref previous) = previous_notification {
                        if previous.0.unwrap_or_default() != notification.0.unwrap_or_default() {
                            should_process_notification = true;
                        }
                    }
                }

                if !should_process_notification {
                    tracing::trace!("monitor state matches latest snapshot, skipping notification");
                    continue 'receiver;
                }

                let mut borders = BORDER_STATE.lock();
                let mut borders_monitors = BORDERS_MONITORS.lock();

                // If borders are disabled
                if !BORDER_ENABLED.load_consume()
                    // Or if they are temporarily disabled
                    || BORDER_TEMPORARILY_DISABLED.load(Ordering::SeqCst)
                    // Or if the wm is paused
                    || is_paused
                    // Or if we are handling an alt-tab across workspaces
                    || ALT_TAB_HWND.load().is_some()
                {
                    // Destroy the borders we know about
                    for (_, border) in borders.iter() {
                        border.destroy()?;
                    }

                    borders.clear();

                    previous_is_paused = is_paused;
                    continue 'receiver;
                }

                'monitors: for (monitor_idx, m) in monitors.elements().iter().enumerate() {
                    // Only operate on the focused workspace of each monitor
                    if let Some(ws) = m.focused_workspace() {
                        // Workspaces with tiling disabled don't have borders
                        if !ws.tile() {
                            let mut to_remove = vec![];
                            for (id, border) in borders.iter() {
                                if borders_monitors.get(id).copied().unwrap_or_default()
                                    == monitor_idx
                                {
                                    border.destroy()?;
                                    to_remove.push(id.clone());
                                }
                            }

                            for id in &to_remove {
                                borders.remove(id);
                            }

                            continue 'monitors;
                        }

                        // Handle the monocle container separately
                        if let Some(monocle) = ws.monocle_container() {
                            let border = match borders.entry(monocle.id().clone()) {
                                Entry::Occupied(entry) => entry.into_mut(),
                                Entry::Vacant(entry) => {
                                    if let Ok(border) = Border::create(monocle.id()) {
                                        entry.insert(border)
                                    } else {
                                        continue 'monitors;
                                    }
                                }
                            };

                            borders_monitors.insert(monocle.id().clone(), monitor_idx);

                            {
                                let mut focus_state = FOCUS_STATE.lock();
                                focus_state.insert(
                                    border.hwnd,
                                    if monitor_idx != focused_monitor_idx {
                                        WindowKind::Unfocused
                                    } else {
                                        WindowKind::Monocle
                                    },
                                );
                            }

                            let rect = WindowsApi::window_rect(
                                monocle.focused_window().copied().unwrap_or_default().hwnd,
                            )?;

                            border.update(&rect, true)?;

                            let border_hwnd = border.hwnd;
                            let mut to_remove = vec![];
                            for (id, b) in borders.iter() {
                                if borders_monitors.get(id).copied().unwrap_or_default()
                                    == monitor_idx
                                    && border_hwnd != b.hwnd
                                {
                                    b.destroy()?;
                                    to_remove.push(id.clone());
                                }
                            }

                            for id in &to_remove {
                                borders.remove(id);
                            }

                            continue 'monitors;
                        }

                        let is_maximized = WindowsApi::is_zoomed(
                            WindowsApi::foreground_window().unwrap_or_default(),
                        );

                        if is_maximized {
                            let mut to_remove = vec![];
                            for (id, border) in borders.iter() {
                                if borders_monitors.get(id).copied().unwrap_or_default()
                                    == monitor_idx
                                {
                                    border.destroy()?;
                                    to_remove.push(id.clone());
                                }
                            }

                            for id in &to_remove {
                                borders.remove(id);
                            }

                            continue 'monitors;
                        }

                        // Destroy any borders not associated with the focused workspace
                        let mut container_and_floating_window_ids = ws
                            .containers()
                            .iter()
                            .map(|c| c.id().clone())
                            .collect::<Vec<_>>();

                        for w in ws.floating_windows() {
                            container_and_floating_window_ids.push(w.hwnd.to_string());
                        }

                        let mut to_remove = vec![];
                        for (id, border) in borders.iter() {
                            if borders_monitors.get(id).copied().unwrap_or_default() == monitor_idx
                                && !container_and_floating_window_ids.contains(id)
                            {
                                border.destroy()?;
                                to_remove.push(id.clone());
                            }
                        }

                        for id in &to_remove {
                            borders.remove(id);
                        }

                        for (idx, c) in ws.containers().iter().enumerate() {
                            let hwnd = c.focused_window().copied().unwrap_or_default().hwnd;
                            let notification_hwnd = notification.0.unwrap_or_default();

                            // Update border when moving or resizing with mouse
                            if pending_move_op.is_some()
                                && idx == ws.focused_container_idx()
                                && hwnd == notification_hwnd
                            {
                                let restore_z_order = Z_ORDER.load();
                                Z_ORDER.store(ZOrder::TopMost);

                                let mut rect = WindowsApi::window_rect(
                                    c.focused_window().copied().unwrap_or_default().hwnd,
                                )?;

                                // We create a new variable to track the actual pending move op so
                                // that the other variable `pending_move_op` still holds the
                                // pending move info so that when the move ends we know on the next
                                // notification that the previous pending move and pending move are
                                // different (because a move just finished) and still handle the
                                // notification. If otherwise we updated the pending_move_op here
                                // directly then the next pending move after finish would be the
                                // same because we had already updated it here.
                                let mut sync_pending_move_op =
                                    weak_pending_move_op.upgrade().and_then(|p| *p);
                                while sync_pending_move_op.is_some() {
                                    sync_pending_move_op =
                                        weak_pending_move_op.upgrade().and_then(|p| *p);
                                    let border = match borders.entry(c.id().clone()) {
                                        Entry::Occupied(entry) => entry.into_mut(),
                                        Entry::Vacant(entry) => {
                                            if let Ok(border) = Border::create(c.id()) {
                                                entry.insert(border)
                                            } else {
                                                continue 'monitors;
                                            }
                                        }
                                    };

                                    let new_rect = WindowsApi::window_rect(
                                        c.focused_window().copied().unwrap_or_default().hwnd,
                                    )?;

                                    if rect != new_rect {
                                        rect = new_rect;
                                        border.update(&rect, false)?;
                                    }
                                }

                                Z_ORDER.store(restore_z_order);

                                continue 'monitors;
                            }

                            // Get the border entry for this container from the map or create one
                            let border = match borders.entry(c.id().clone()) {
                                Entry::Occupied(entry) => entry.into_mut(),
                                Entry::Vacant(entry) => {
                                    if let Ok(border) = Border::create(c.id()) {
                                        entry.insert(border)
                                    } else {
                                        continue 'monitors;
                                    }
                                }
                            };

                            borders_monitors.insert(c.id().clone(), monitor_idx);

                            #[allow(unused_assignments)]
                            let mut last_focus_state = None;

                            let new_focus_state = if idx != ws.focused_container_idx()
                                || monitor_idx != focused_monitor_idx
                            {
                                WindowKind::Unfocused
                            } else if c.windows().len() > 1 {
                                WindowKind::Stack
                            } else {
                                WindowKind::Single
                            };

                            // Update the focused state for all containers on this workspace
                            {
                                let mut focus_state = FOCUS_STATE.lock();
                                last_focus_state = focus_state.insert(border.hwnd, new_focus_state);
                            }

                            let rect = WindowsApi::window_rect(
                                c.focused_window().copied().unwrap_or_default().hwnd,
                            )?;

                            let should_invalidate = match last_focus_state {
                                None => true,
                                Some(last_focus_state) => last_focus_state != new_focus_state,
                            };

                            border.update(&rect, should_invalidate)?;
                        }

                        {
                            let restore_z_order = Z_ORDER.load();
                            Z_ORDER.store(ZOrder::TopMost);

                            'windows: for window in ws.floating_windows() {
                                let hwnd = window.hwnd;
                                let notification_hwnd = notification.0.unwrap_or_default();

                                if pending_move_op.is_some() && hwnd == notification_hwnd {
                                    let mut rect = WindowsApi::window_rect(hwnd)?;

                                    // Check comment above for containers move
                                    let mut sync_pending_move_op =
                                        weak_pending_move_op.upgrade().and_then(|p| *p);
                                    while sync_pending_move_op.is_some() {
                                        sync_pending_move_op =
                                            weak_pending_move_op.upgrade().and_then(|p| *p);
                                        let border = match borders.entry(hwnd.to_string()) {
                                            Entry::Occupied(entry) => entry.into_mut(),
                                            Entry::Vacant(entry) => {
                                                if let Ok(border) =
                                                    Border::create(&hwnd.to_string())
                                                {
                                                    entry.insert(border)
                                                } else {
                                                    continue 'monitors;
                                                }
                                            }
                                        };

                                        let new_rect = WindowsApi::window_rect(hwnd)?;

                                        if rect != new_rect {
                                            rect = new_rect;
                                            border.update(&rect, true)?;
                                        }
                                    }

                                    Z_ORDER.store(restore_z_order);

                                    continue 'monitors;
                                }

                                let border = match borders.entry(window.hwnd.to_string()) {
                                    Entry::Occupied(entry) => entry.into_mut(),
                                    Entry::Vacant(entry) => {
                                        if let Ok(border) = Border::create(&window.hwnd.to_string())
                                        {
                                            entry.insert(border)
                                        } else {
                                            continue 'monitors;
                                        }
                                    }
                                };

                                borders_monitors.insert(window.hwnd.to_string(), monitor_idx);

                                let mut should_destroy = false;

                                if let Some(notification_hwnd) = notification.0 {
                                    if notification_hwnd != window.hwnd {
                                        should_destroy = true;
                                    }
                                }

                                if WindowsApi::foreground_window().unwrap_or_default()
                                    != window.hwnd
                                {
                                    should_destroy = true;
                                }

                                if should_destroy {
                                    border.destroy()?;
                                    borders.remove(&window.hwnd.to_string());
                                    borders_monitors.remove(&window.hwnd.to_string());
                                    continue 'windows;
                                }

                                #[allow(unused_assignments)]
                                let mut last_focus_state = None;
                                let new_focus_state = WindowKind::Floating;
                                {
                                    let mut focus_state = FOCUS_STATE.lock();
                                    last_focus_state =
                                        focus_state.insert(border.hwnd, new_focus_state);
                                }

                                let rect = WindowsApi::window_rect(window.hwnd)?;

                                let should_invalidate = match last_focus_state {
                                    None => true,
                                    Some(last_focus_state) => last_focus_state != new_focus_state,
                                };

                                border.update(&rect, should_invalidate)?;
                            }

                            Z_ORDER.store(restore_z_order);
                        }
                    }
                }
            }
        }

        previous_snapshot = monitors;
        previous_pending_move_op = pending_move_op;
        previous_is_paused = is_paused;
        previous_notification = Some(notification);
    }

    Ok(())
}

#[derive(Debug, Copy, Clone, Serialize, Deserialize, JsonSchema)]
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
