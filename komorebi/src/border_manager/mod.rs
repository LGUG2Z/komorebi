mod border;

use crossbeam_channel::Receiver;
use crossbeam_channel::Sender;
use crossbeam_utils::atomic::AtomicConsume;
use komorebi_core::ActiveWindowBorderStyle;
use lazy_static::lazy_static;
use parking_lot::Mutex;
use schemars::JsonSchema;
use serde::Deserialize;
use serde::Serialize;
use std::collections::HashMap;
use std::sync::atomic::AtomicBool;
use std::sync::atomic::AtomicI32;
use std::sync::atomic::AtomicU32;
use std::sync::Arc;
use std::sync::OnceLock;
use windows::Win32::Foundation::HWND;

use crate::Colour;
use crate::Rect;
use crate::Rgb;
use crate::WindowManager;
use crate::WindowsApi;
use border::Border;
use komorebi_core::WindowKind;

pub static BORDER_WIDTH: AtomicI32 = AtomicI32::new(8);
pub static BORDER_OFFSET: AtomicI32 = AtomicI32::new(-1);

pub static BORDER_ENABLED: AtomicBool = AtomicBool::new(true);

lazy_static! {
    pub static ref Z_ORDER: Arc<Mutex<ZOrder>> = Arc::new(Mutex::new(ZOrder::Bottom));
    pub static ref STYLE: Arc<Mutex<ActiveWindowBorderStyle>> =
        Arc::new(Mutex::new(ActiveWindowBorderStyle::System));
    pub static ref FOCUSED: AtomicU32 =
        AtomicU32::new(u32::from(Colour::Rgb(Rgb::new(66, 165, 245))));
    pub static ref UNFOCUSED: AtomicU32 =
        AtomicU32::new(u32::from(Colour::Rgb(Rgb::new(128, 128, 128))));
    pub static ref MONOCLE: AtomicU32 =
        AtomicU32::new(u32::from(Colour::Rgb(Rgb::new(255, 51, 153))));
    pub static ref STACK: AtomicU32 = AtomicU32::new(u32::from(Colour::Rgb(Rgb::new(0, 165, 66))));
}

lazy_static! {
    static ref BORDERS_MONITORS: Mutex<HashMap<String, usize>> = Mutex::new(HashMap::new());
    static ref BORDER_STATE: Mutex<HashMap<String, Border>> = Mutex::new(HashMap::new());
    static ref RECT_STATE: Mutex<HashMap<isize, Rect>> = Mutex::new(HashMap::new());
    static ref FOCUS_STATE: Mutex<HashMap<isize, WindowKind>> = Mutex::new(HashMap::new());
}

pub struct Notification;

static CHANNEL: OnceLock<(Sender<Notification>, Receiver<Notification>)> = OnceLock::new();

pub fn channel() -> &'static (Sender<Notification>, Receiver<Notification>) {
    CHANNEL.get_or_init(crossbeam_channel::unbounded)
}

pub fn event_tx() -> Sender<Notification> {
    channel().0.clone()
}

pub fn event_rx() -> Receiver<Notification> {
    channel().1.clone()
}

pub fn destroy_all_borders() -> color_eyre::Result<()> {
    let mut borders = BORDER_STATE.lock();
    for (_, border) in borders.iter() {
        border.destroy()?;
    }

    borders.clear();
    RECT_STATE.lock().clear();
    BORDERS_MONITORS.lock().clear();
    FOCUS_STATE.lock().clear();

    Ok(())
}

pub fn listen_for_notifications(wm: Arc<Mutex<WindowManager>>) {
    tracing::info!("listening");
    let receiver = event_rx();

    std::thread::spawn(move || -> color_eyre::Result<()> {
        'receiver: for _ in receiver {
            let mut borders = BORDER_STATE.lock();
            let mut borders_monitors = BORDERS_MONITORS.lock();

            // Check the wm state every time we receive a notification
            let state = wm.lock();

            if !BORDER_ENABLED.load_consume() || state.is_paused {
                if !borders.is_empty() {
                    for (_, border) in borders.iter() {
                        border.destroy()?;
                    }

                    borders.clear();
                }

                continue 'receiver;
            }

            let focused_monitor_idx = state.focused_monitor_idx();

            for (monitor_idx, m) in state.monitors.elements().iter().enumerate() {
                // Only operate on the focused workspace of each monitor
                if let Some(ws) = m.focused_workspace() {
                    // Workspaces with tiling disabled don't have borders
                    if !ws.tile() {
                        let mut to_remove = vec![];
                        for (id, border) in borders.iter() {
                            if borders_monitors.get(id).copied().unwrap_or_default() == monitor_idx
                            {
                                border.destroy()?;
                                to_remove.push(id.clone());
                            }
                        }

                        for id in &to_remove {
                            borders.remove(id);
                        }

                        continue 'receiver;
                    }

                    // Handle the monocle container separately
                    if let Some(monocle) = ws.monocle_container() {
                        let mut to_remove = vec![];
                        for (id, border) in borders.iter() {
                            if borders_monitors.get(id).copied().unwrap_or_default() == monitor_idx
                            {
                                border.destroy()?;
                                to_remove.push(id.clone());
                            }
                        }

                        for id in &to_remove {
                            borders.remove(id);
                        }

                        let border = borders.entry(monocle.id().clone()).or_insert_with(|| {
                            Border::create(monocle.id()).expect("border creation failed")
                        });

                        borders_monitors.insert(monocle.id().clone(), monitor_idx);

                        {
                            let mut focus_state = FOCUS_STATE.lock();
                            focus_state.insert(border.hwnd, WindowKind::Monocle);
                        }

                        let rect = WindowsApi::window_rect(
                            monocle
                                .focused_window()
                                .expect("monocle container has no focused window")
                                .hwnd(),
                        )?;

                        border.update(&rect)?;
                        continue 'receiver;
                    }

                    let is_maximized = WindowsApi::is_zoomed(HWND(
                        WindowsApi::foreground_window().unwrap_or_default(),
                    ));

                    if is_maximized {
                        let mut to_remove = vec![];
                        for (id, border) in borders.iter() {
                            if borders_monitors.get(id).copied().unwrap_or_default() == monitor_idx
                            {
                                border.destroy()?;
                                to_remove.push(id.clone());
                            }
                        }

                        for id in &to_remove {
                            borders.remove(id);
                        }

                        continue 'receiver;
                    }

                    // Destroy any borders not associated with the focused workspace
                    let container_ids = ws
                        .containers()
                        .iter()
                        .map(|c| c.id().clone())
                        .collect::<Vec<_>>();

                    let mut to_remove = vec![];
                    for (id, border) in borders.iter() {
                        if borders_monitors.get(id).copied().unwrap_or_default() == monitor_idx {
                            if !container_ids.contains(id) {
                                border.destroy()?;
                                to_remove.push(id.clone());
                            }
                        }
                    }

                    for id in &to_remove {
                        borders.remove(id);
                    }

                    for (idx, c) in ws.containers().iter().enumerate() {
                        // Update border when moving or resizing with mouse
                        if state.pending_move_op.is_some() && idx == ws.focused_container_idx() {
                            let restore_z_order = *Z_ORDER.lock();
                            *Z_ORDER.lock() = ZOrder::TopMost;

                            let mut rect = WindowsApi::window_rect(
                                c.focused_window()
                                    .expect("container has no focused window")
                                    .hwnd(),
                            )?;

                            while WindowsApi::lbutton_is_pressed() {
                                let border = borders.entry(c.id().clone()).or_insert_with(|| {
                                    Border::create(c.id()).expect("border creation failed")
                                });

                                let new_rect = WindowsApi::window_rect(
                                    c.focused_window()
                                        .expect("container has no focused window")
                                        .hwnd(),
                                )?;

                                if rect != new_rect {
                                    rect = new_rect;
                                    border.update(&rect)?;
                                }
                            }

                            *Z_ORDER.lock() = restore_z_order;

                            continue 'receiver;
                        }

                        // Get the border entry for this container from the map or create one
                        let border = borders.entry(c.id().clone()).or_insert_with(|| {
                            Border::create(c.id()).expect("border creation failed")
                        });

                        borders_monitors.insert(c.id().clone(), monitor_idx);

                        // Update the focused state for all containers on this workspace
                        {
                            let mut focus_state = FOCUS_STATE.lock();
                            focus_state.insert(
                                border.hwnd,
                                if idx != ws.focused_container_idx()
                                    || monitor_idx != focused_monitor_idx
                                {
                                    WindowKind::Unfocused
                                } else {
                                    if c.windows().len() > 1 {
                                        WindowKind::Stack
                                    } else {
                                        WindowKind::Single
                                    }
                                },
                            );
                        }

                        let rect = WindowsApi::window_rect(
                            c.focused_window()
                                .expect("container has no focused window")
                                .hwnd(),
                        )?;

                        border.update(&rect)?;
                    }
                }
            }
        }

        Ok(())
    });
}

#[derive(Debug, Copy, Clone, Serialize, Deserialize, JsonSchema)]
pub enum ZOrder {
    Top,
    NoTopMost,
    Bottom,
    TopMost,
}

impl Into<isize> for ZOrder {
    fn into(self) -> isize {
        match self {
            ZOrder::Top => 0,
            ZOrder::NoTopMost => -2,
            ZOrder::Bottom => 1,
            ZOrder::TopMost => -1,
        }
    }
}
