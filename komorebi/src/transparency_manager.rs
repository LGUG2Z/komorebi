#![deny(clippy::unwrap_used, clippy::expect_used)]

use crossbeam_channel::Receiver;
use crossbeam_channel::Sender;
use crossbeam_utils::atomic::AtomicConsume;
use parking_lot::Mutex;
use schemars::JsonSchema;
use serde::Deserialize;
use serde::Serialize;
use std::sync::atomic::AtomicBool;
use std::sync::atomic::AtomicU8;
use std::sync::Arc;
use std::sync::OnceLock;
use windows::Win32::Foundation::HWND;

use crate::Window;
use crate::WindowManager;
use crate::WindowsApi;

pub static TRANSPARENCY_ENABLED: AtomicBool = AtomicBool::new(false);
pub static TRANSPARENCY_ALPHA: AtomicU8 = AtomicU8::new(200);

static KNOWN_HWNDS: OnceLock<Mutex<Vec<isize>>> = OnceLock::new();

pub struct Notification;

static CHANNEL: OnceLock<(Sender<Notification>, Receiver<Notification>)> = OnceLock::new();

pub fn known_hwnds() -> Vec<isize> {
    let known = KNOWN_HWNDS.get_or_init(|| Mutex::new(Vec::new())).lock();
    known.iter().copied().collect()
}

pub fn channel() -> &'static (Sender<Notification>, Receiver<Notification>) {
    CHANNEL.get_or_init(crossbeam_channel::unbounded)
}

pub fn event_tx() -> Sender<Notification> {
    channel().0.clone()
}

pub fn event_rx() -> Receiver<Notification> {
    channel().1.clone()
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
    event_tx().send(Notification)?;

    'receiver: for _ in receiver {
        let known_hwnds = KNOWN_HWNDS.get_or_init(|| Mutex::new(Vec::new()));
        if !TRANSPARENCY_ENABLED.load_consume() {
            for hwnd in known_hwnds.lock().iter() {
                Window::from(*hwnd).opaque()?;
            }

            continue 'receiver;
        }

        known_hwnds.lock().clear();

        // Check the wm state every time we receive a notification
        let state = wm.lock();

        let focused_monitor_idx = state.focused_monitor_idx();

        'monitors: for (monitor_idx, m) in state.monitors.elements().iter().enumerate() {
            let focused_workspace_idx = m.focused_workspace_idx();

            'workspaces: for (workspace_idx, ws) in m.workspaces().iter().enumerate() {
                // Only operate on the focused workspace of each monitor
                // Workspaces with tiling disabled don't have transparent windows
                if !ws.tile() || workspace_idx != focused_workspace_idx {
                    for window in ws.visible_windows().iter().flatten() {
                        window.opaque()?;
                    }

                    continue 'workspaces;
                }

                // Monocle container is never transparent
                if let Some(monocle) = ws.monocle_container() {
                    if let Some(window) = monocle.focused_window() {
                        window.opaque()?;
                    }

                    continue 'monitors;
                }

                let foreground_hwnd = WindowsApi::foreground_window().unwrap_or_default();
                let is_maximized = WindowsApi::is_zoomed(HWND(foreground_hwnd));

                if is_maximized {
                    Window {
                        hwnd: foreground_hwnd,
                    }
                    .opaque()?;
                    continue 'monitors;
                }

                for (idx, c) in ws.containers().iter().enumerate() {
                    // Update the transparency for all containers on this workspace

                    // If the window is not focused on the current workspace, or isn't on the focused monitor
                    // make it transparent
                    if idx != ws.focused_container_idx() || monitor_idx != focused_monitor_idx {
                        let unfocused_window = c.focused_window().copied().unwrap_or_default();
                        unfocused_window.transparent()?;

                        known_hwnds.lock().push(unfocused_window.hwnd);
                    // Otherwise, make it opaque
                    } else {
                        c.focused_window().copied().unwrap_or_default().opaque()?;
                    };
                }
            }
        }
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
