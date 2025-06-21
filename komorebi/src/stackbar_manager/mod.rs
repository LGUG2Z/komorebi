mod stackbar;

use crate::container::Container;
use crate::core::StackbarLabel;
use crate::core::StackbarMode;
use crate::stackbar_manager::stackbar::Stackbar;
use crate::WindowManager;
use crate::WindowsApi;
use crate::DEFAULT_CONTAINER_PADDING;
use crossbeam_channel::Receiver;
use crossbeam_channel::Sender;
use crossbeam_utils::atomic::AtomicCell;
use crossbeam_utils::atomic::AtomicConsume;
use lazy_static::lazy_static;
use parking_lot::Mutex;
use std::collections::hash_map::Entry;
use std::collections::HashMap;
use std::sync::atomic::AtomicBool;
use std::sync::atomic::AtomicI32;
use std::sync::atomic::AtomicU32;
use std::sync::atomic::Ordering;
use std::sync::Arc;
use std::sync::OnceLock;

pub static STACKBAR_FONT_SIZE: AtomicI32 = AtomicI32::new(0); // 0 will produce the system default
pub static STACKBAR_FOCUSED_TEXT_COLOUR: AtomicU32 = AtomicU32::new(16777215); // white
pub static STACKBAR_UNFOCUSED_TEXT_COLOUR: AtomicU32 = AtomicU32::new(11776947); // gray text
pub static STACKBAR_TAB_BACKGROUND_COLOUR: AtomicU32 = AtomicU32::new(3355443); // gray
pub static STACKBAR_TAB_HEIGHT: AtomicI32 = AtomicI32::new(40);
pub static STACKBAR_TAB_WIDTH: AtomicI32 = AtomicI32::new(200);
pub static STACKBAR_LABEL: AtomicCell<StackbarLabel> = AtomicCell::new(StackbarLabel::Title);
pub static STACKBAR_MODE: AtomicCell<StackbarMode> = AtomicCell::new(StackbarMode::Never);

pub static STACKBAR_TEMPORARILY_DISABLED: AtomicBool = AtomicBool::new(false);

lazy_static! {
    pub static ref STACKBAR_STATE: Mutex<HashMap<Arc<str>, Stackbar>> = Mutex::new(HashMap::new());
    pub static ref STACKBAR_FONT_FAMILY: Mutex<Option<String>> = Mutex::new(None);
    static ref STACKBARS_MONITORS: Mutex<HashMap<Arc<str>, usize>> = Mutex::new(HashMap::new());
    static ref STACKBARS_CONTAINERS: Mutex<HashMap<isize, Container>> = Mutex::new(HashMap::new());
}

pub struct Notification;

static CHANNEL: OnceLock<(Sender<Notification>, Receiver<Notification>)> = OnceLock::new();

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

pub fn should_have_stackbar(window_count: usize) -> bool {
    match STACKBAR_MODE.load() {
        StackbarMode::Always => true,
        StackbarMode::OnStack => window_count > 1,
        StackbarMode::Never => false,
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

    'receiver: for _ in receiver {
        let mut stackbars = STACKBAR_STATE.lock();
        let mut stackbars_monitors = STACKBARS_MONITORS.lock();

        // Check the wm state every time we receive a notification
        let mut state = wm.lock();

        // If stackbars are disabled
        if matches!(STACKBAR_MODE.load(), StackbarMode::Never)
            || STACKBAR_TEMPORARILY_DISABLED.load(Ordering::SeqCst)
        {
            for (_, stackbar) in stackbars.iter() {
                stackbar.destroy()?;
            }

            stackbars.clear();
            continue 'receiver;
        }

        for (monitor_idx, m) in state.monitors_mut().iter_mut().enumerate() {
            // Only operate on the focused workspace of each monitor
            if let Some(ws) = m.focused_workspace_mut() {
                // Workspaces with tiling disabled don't have stackbars
                if !ws.tile() {
                    let mut to_remove = vec![];
                    for (id, border) in stackbars.iter() {
                        if stackbars_monitors.get(id).copied().unwrap_or_default() == monitor_idx {
                            border.destroy()?;
                            to_remove.push(id.clone());
                        }
                    }

                    for id in &to_remove {
                        stackbars.remove(id);
                    }

                    continue 'receiver;
                }

                let is_maximized =
                    WindowsApi::is_zoomed(WindowsApi::foreground_window().unwrap_or_default());

                // Handle the monocle container separately
                if ws.monocle_container().is_some() || is_maximized {
                    // Destroy any stackbars associated with the focused workspace
                    let mut to_remove = vec![];
                    for (id, stackbar) in stackbars.iter() {
                        if stackbars_monitors.get(id).copied().unwrap_or_default() == monitor_idx {
                            stackbar.destroy()?;
                            to_remove.push(id.clone());
                        }
                    }

                    for id in &to_remove {
                        stackbars.remove(id);
                    }

                    continue 'receiver;
                }

                // Destroy any stackbars not associated with the focused workspace
                let container_ids = ws
                    .containers()
                    .iter()
                    .map(|c| c.id().clone())
                    .collect::<Vec<_>>();

                let mut to_remove = vec![];
                for (id, stackbar) in stackbars.iter() {
                    if stackbars_monitors.get(id).copied().unwrap_or_default() == monitor_idx
                        && !container_ids.contains(id)
                    {
                        stackbar.destroy()?;
                        to_remove.push(id.clone());
                    }
                }

                for id in &to_remove {
                    stackbars.remove(id);
                }

                let container_padding = ws
                    .container_padding()
                    .unwrap_or_else(|| DEFAULT_CONTAINER_PADDING.load_consume());

                'containers: for container in ws.containers_mut() {
                    let should_add_stackbar = match STACKBAR_MODE.load() {
                        StackbarMode::Always => true,
                        StackbarMode::OnStack => container.windows().len() > 1,
                        StackbarMode::Never => false,
                    };

                    if !should_add_stackbar {
                        if let Some(stackbar) = stackbars.get(container.id()) {
                            stackbar.destroy()?
                        }

                        stackbars.remove(container.id());
                        stackbars_monitors.remove(container.id());
                        continue 'containers;
                    }

                    // Get the stackbar entry for this container from the map or create one
                    let stackbar = match stackbars.entry(container.id().clone()) {
                        Entry::Occupied(entry) => entry.into_mut(),
                        Entry::Vacant(entry) => {
                            if let Ok(stackbar) = Stackbar::create(container.id()) {
                                entry.insert(stackbar)
                            } else {
                                continue 'receiver;
                            }
                        }
                    };

                    stackbars_monitors.insert(container.id().clone(), monitor_idx);

                    let rect = WindowsApi::window_rect(
                        container.focused_window().copied().unwrap_or_default().hwnd,
                    )?;

                    stackbar.update(container_padding, container, &rect)?;
                }
            }
        }
    }

    Ok(())
}
