#![deny(clippy::unwrap_used, clippy::expect_used)]

use crossbeam_channel::Receiver;
use crossbeam_channel::Sender;
use crossbeam_utils::atomic::AtomicConsume;
use parking_lot::Mutex;
use std::sync::Arc;
use std::sync::OnceLock;
use std::sync::atomic::AtomicBool;
use std::sync::atomic::AtomicU8;

use crate::REGEX_IDENTIFIERS;
use crate::TRANSPARENCY_BLACKLIST;
use crate::Window;
use crate::WindowManager;
use crate::WindowsApi;
use crate::should_act;

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
    CHANNEL.get_or_init(|| crossbeam_channel::bounded(20))
}

fn event_tx() -> Sender<Notification> {
    channel().0.clone()
}

fn event_rx() -> Receiver<Notification> {
    channel().1.clone()
}

pub fn send_notification() {
    if event_tx().try_send(Notification).is_err() {
        tracing::warn!("channel is full; dropping notification")
    }
}

pub fn listen_for_notifications(wm: Arc<Mutex<WindowManager>>) {
    std::thread::spawn(move || {
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

    let receiver = event_rx();
    event_tx().send(Notification)?;

    'receiver: for _ in receiver {
        let known_hwnds = KNOWN_HWNDS.get_or_init(|| Mutex::new(Vec::new()));
        if !TRANSPARENCY_ENABLED.load_consume() {
            for hwnd in known_hwnds.lock().iter() {
                if let Err(error) = Window::from(*hwnd).opaque() {
                    tracing::error!("failed to make window {hwnd} opaque: {error}")
                }
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
                if !ws.tile || workspace_idx != focused_workspace_idx {
                    for window in ws.visible_windows().iter().flatten() {
                        if let Err(error) = window.opaque() {
                            let hwnd = window.hwnd;
                            tracing::error!("failed to make window {hwnd} opaque: {error}")
                        }
                    }

                    continue 'workspaces;
                }

                // Monocle container is never transparent
                if let Some(monocle) = &ws.monocle_container {
                    if let Some(window) = monocle.focused_window() {
                        if monitor_idx == focused_monitor_idx {
                            if let Err(error) = window.opaque() {
                                let hwnd = window.hwnd;
                                tracing::error!(
                                    "failed to make monocle window {hwnd} opaque: {error}"
                                )
                            }
                        } else if let Err(error) = window.transparent() {
                            let hwnd = window.hwnd;
                            tracing::error!(
                                "failed to make monocle window {hwnd} transparent: {error}"
                            )
                        }
                    }

                    continue 'monitors;
                }

                let foreground_hwnd = WindowsApi::foreground_window().unwrap_or_default();
                let is_maximized = WindowsApi::is_zoomed(foreground_hwnd);

                if is_maximized {
                    if let Err(error) = Window::from(foreground_hwnd).opaque() {
                        let hwnd = foreground_hwnd;
                        tracing::error!("failed to make maximized window {hwnd} opaque: {error}")
                    }

                    continue 'monitors;
                }

                let transparency_blacklist = TRANSPARENCY_BLACKLIST.lock();
                let regex_identifiers = REGEX_IDENTIFIERS.lock();

                for (idx, c) in ws.containers().iter().enumerate() {
                    // Update the transparency for all containers on this workspace

                    // If the window is not focused on the current workspace, or isn't on the focused monitor
                    // make it transparent
                    #[allow(clippy::collapsible_else_if)]
                    if idx != ws.focused_container_idx() || monitor_idx != focused_monitor_idx {
                        let focused_window_idx = c.focused_window_idx();
                        for (window_idx, window) in c.windows().iter().enumerate() {
                            if window_idx == focused_window_idx {
                                let mut should_make_transparent = true;
                                if !transparency_blacklist.is_empty()
                                    && let (Ok(title), Ok(exe_name), Ok(class), Ok(path)) = (
                                        window.title(),
                                        window.exe(),
                                        window.class(),
                                        window.path(),
                                    )
                                {
                                    let is_blacklisted = should_act(
                                        &title,
                                        &exe_name,
                                        &class,
                                        &path,
                                        &transparency_blacklist,
                                        &regex_identifiers,
                                    )
                                    .is_some();

                                    should_make_transparent = !is_blacklisted;
                                }

                                if should_make_transparent {
                                    match window.transparent() {
                                        Err(error) => {
                                            let hwnd = foreground_hwnd;
                                            tracing::error!(
                                                "failed to make unfocused window {hwnd} transparent: {error}"
                                            )
                                        }
                                        Ok(..) => {
                                            known_hwnds.lock().push(window.hwnd);
                                        }
                                    }
                                }
                            } else {
                                // just in case, this is useful when people are clicking around
                                // on unfocused stackbar tabs
                                known_hwnds.lock().push(window.hwnd);
                            }
                        }
                    // Otherwise, make it opaque
                    } else {
                        let focused_window_idx = c.focused_window_idx();
                        for (window_idx, window) in c.windows().iter().enumerate() {
                            if window_idx != focused_window_idx {
                                known_hwnds.lock().push(window.hwnd);
                            } else {
                                if let Err(error) =
                                    c.focused_window().copied().unwrap_or_default().opaque()
                                {
                                    let hwnd = foreground_hwnd;
                                    tracing::error!(
                                        "failed to make focused window {hwnd} opaque: {error}"
                                    )
                                }
                            }
                        }
                    };
                }
            }
        }
    }

    Ok(())
}
