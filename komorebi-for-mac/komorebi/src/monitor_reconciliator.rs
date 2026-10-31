use crate::DISPLAY_INDEX_PREFERENCES;
use crate::Notification;
use crate::NotificationEvent;
use crate::WORKSPACE_MATCHING_RULES;
use crate::border_manager;
use crate::core::config_generation::WorkspaceMatchingRule;
use crate::macos_api::MacosApi;
use crate::monitor::Monitor;
use crate::notify_subscribers;
use crate::state::State;
use crate::window_manager::WindowManager;
use crossbeam_channel::Receiver;
use crossbeam_channel::Sender;
use dispatch2::DispatchQueue;
use objc2_core_graphics::CGDirectDisplayID;
use parking_lot::Mutex;
use serde::Deserialize;
use serde::Serialize;
use std::collections::HashMap;
use std::sync::Arc;
use std::sync::OnceLock;
use strum::Display;

#[derive(Debug, Copy, Clone, Serialize, Deserialize, Display)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
pub enum MonitorNotification {
    Resize(CGDirectDisplayID),
    DisplayConnectionChange(CGDirectDisplayID),
}

static CHANNEL: OnceLock<(Sender<MonitorNotification>, Receiver<MonitorNotification>)> =
    OnceLock::new();

static MONITOR_CACHE: OnceLock<Mutex<HashMap<String, Monitor>>> = OnceLock::new();

pub fn channel() -> &'static (Sender<MonitorNotification>, Receiver<MonitorNotification>) {
    CHANNEL.get_or_init(|| crossbeam_channel::bounded(20))
}

fn event_tx() -> Sender<MonitorNotification> {
    channel().0.clone()
}

fn event_rx() -> Receiver<MonitorNotification> {
    channel().1.clone()
}

pub fn insert_in_monitor_cache(serial_number_id: &str, monitor: Monitor) {
    let display_index_preferences = DISPLAY_INDEX_PREFERENCES.read();
    let mut dip_ids = display_index_preferences.values();
    let preferred_id = if dip_ids.any(|id| id == serial_number_id) {
        monitor.serial_number_id.clone()
    } else {
        serial_number_id.to_string()
    };

    let mut monitor_cache = MONITOR_CACHE
        .get_or_init(|| Mutex::new(HashMap::new()))
        .lock();

    monitor_cache.insert(preferred_id, monitor);
}

pub fn send_notification(notification: MonitorNotification) {
    if event_tx().try_send(notification).is_err() {
        tracing::warn!("channel is full; dropping notification")
    }
}

pub fn listen_for_notifications(wm: Arc<Mutex<WindowManager>>) -> color_eyre::Result<()> {
    std::thread::spawn(move || {
        loop {
            match handle_notifications(wm.clone()) {
                Ok(()) => {
                    tracing::warn!("restarting finished thread");
                }
                Err(error) => {
                    if cfg!(debug_assertions) {
                        tracing::error!("restarting failed thread: {:?}", error)
                    } else {
                        tracing::error!("restarting failed thread: {}", error)
                    }
                }
            }
        }
    });

    Ok(())
}

pub fn handle_notifications(wm: Arc<Mutex<WindowManager>>) -> color_eyre::Result<()> {
    tracing::info!("listening");
    let arc = wm.clone();

    let receiver = event_rx();

    'receiver: for notification in receiver {
        let initial_state = {
            let wm = wm.lock();
            State::from(wm.as_ref())
        };

        match notification {
            MonitorNotification::Resize(_display_id) => {
                tracing::debug!("handling resize notification");
                DispatchQueue::main().exec_sync(|| {
                    let mut wm = wm.lock();
                    if let Err(error) = MacosApi::update_monitor_work_areas(&mut wm) {
                        tracing::error!("failed to update monitor work areas: {error}");
                    }
                });
            }
            MonitorNotification::DisplayConnectionChange(_) => {
                tracing::debug!("handling display connection change notification");

                let mut attached_devices = vec![];
                DispatchQueue::main().exec_sync(|| {
                    if let Ok(latest) = MacosApi::latest_monitor_information() {
                        attached_devices = latest.clone();
                    }
                });

                let mut wm = wm.lock();
                let mut monitor_cache = MONITOR_CACHE
                    .get_or_init(|| Mutex::new(HashMap::new()))
                    .lock();

                let initial_monitor_count = wm.monitors().len();

                // Make sure that in our state any attached displays have the latest macOS API data
                for monitor in wm.monitors_mut() {
                    for attached in &attached_devices {
                        let serial_number_ids_match =
                            attached.serial_number_id == monitor.serial_number_id;

                        if serial_number_ids_match {
                            monitor.id = attached.id;
                            monitor.device = attached.device.clone();
                            monitor.serial_number_id = attached.serial_number_id.clone();
                            monitor.size = attached.size;
                            monitor.work_area_size = attached.work_area_size;
                        }
                    }
                }

                if initial_monitor_count == attached_devices.len() {
                    // The count can match while the displays do not: closing a laptop lid with
                    // an external display connected swaps one display for another. Hand the
                    // unplugged monitor's workspaces to the display that took its place.
                    let unplugged = wm
                        .monitors()
                        .iter()
                        .enumerate()
                        .filter(|(_, m)| {
                            !attached_devices
                                .iter()
                                .any(|a| a.serial_number_id.eq(&m.serial_number_id))
                        })
                        .map(|(idx, _)| idx)
                        .collect::<Vec<_>>();

                    let plugged = attached_devices
                        .iter()
                        .filter(|a| {
                            !wm.monitors()
                                .iter()
                                .any(|m| m.serial_number_id.eq(&a.serial_number_id))
                        })
                        .collect::<Vec<_>>();

                    if !unplugged.is_empty() && unplugged.len() == plugged.len() {
                        tracing::info!(
                            "{} display(s) swapped for others, moving their monitors over",
                            unplugged.len()
                        );

                        for (idx, attached) in unplugged.iter().zip(plugged) {
                            if let Some(monitor) = wm.monitors_mut().get_mut(*idx) {
                                monitor.id = attached.id;
                                monitor.device = attached.device.clone();
                                monitor.serial_number_id = attached.serial_number_id.clone();
                                monitor.size = attached.size;
                                monitor.work_area_size = attached.work_area_size;
                            }
                        }

                        let offset = wm.work_area_offset;
                        for monitor in wm.monitors_mut() {
                            monitor.update_all_workspace_globals(offset);
                        }

                        if let Err(error) = wm.retile_all(true) {
                            tracing::error!("failed to retile after swapping displays: {error}");
                        }

                        let mouse_follows_focus = wm.mouse_follows_focus;
                        for monitor in wm.monitors_mut() {
                            if let Err(error) = monitor.load_focused_workspace(mouse_follows_focus)
                            {
                                tracing::error!(
                                    "failed to load the focused workspace after swapping displays: {error}"
                                );
                            }
                        }

                        drop(wm);
                        border_manager::send_notification(None, None, false);
                        continue 'receiver;
                    }

                    tracing::debug!("monitor counts match, reconciliation not required");
                    drop(wm);
                    continue 'receiver;
                }

                if attached_devices.is_empty() {
                    tracing::debug!(
                        "no devices found, skipping reconciliation to avoid breaking state"
                    );
                    drop(wm);
                    continue 'receiver;
                }

                if initial_monitor_count > attached_devices.len() {
                    tracing::info!(
                        "monitor count mismatch ({initial_monitor_count} vs {}), removing disconnected monitors",
                        attached_devices.len()
                    );

                    // Windows to remove from `known_hwnds`
                    let mut windows_to_remove = Vec::new();

                    // Collect the ids in our state which aren't in the current attached display ids
                    // These are monitors that have been removed
                    let mut newly_removed_displays = vec![];

                    for (m_idx, m) in wm.monitors().iter().enumerate() {
                        if !attached_devices
                            .iter()
                            .any(|attached| attached.serial_number_id.eq(&m.serial_number_id))
                        {
                            let id = m.serial_number_id.clone();

                            newly_removed_displays.push(id.clone());

                            let focused_workspace_idx = m.focused_workspace_idx();

                            for (idx, workspace) in m.workspaces().iter().enumerate() {
                                let is_focused_workspace = idx == focused_workspace_idx;
                                let focused_container_idx = workspace.focused_container_idx();
                                for (c_idx, container) in workspace.containers().iter().enumerate()
                                {
                                    let focused_window_idx = container.focused_window_idx();
                                    for (w_idx, window) in container.windows().iter().enumerate() {
                                        windows_to_remove.push(window.id);
                                        if is_focused_workspace
                                            && c_idx == focused_container_idx
                                            && w_idx == focused_window_idx
                                        {
                                            // TODO: Do we need this on macOS?
                                            // Minimize the focused window since Windows might try
                                            // to move it to another monitor if it was focused.
                                            // if window.is_focused() {
                                            //     window.minimize()?;
                                            // }
                                        }
                                    }
                                }

                                // TODO: might not support maximization on macOS
                                // if let Some(maximized) = &workspace.maximized_window {
                                //     windows_to_remove.push(maximized.id);
                                //     // Minimize the focused window since Windows might try
                                //     // to move it to another monitor if it was focused.
                                //     if maximized.is_focused() {
                                //         maximized.minimize();
                                //     }
                                // }

                                if let Some(container) = &workspace.monocle_container {
                                    for window in container.windows() {
                                        windows_to_remove.push(window.id);
                                    }
                                    if let Some(_window) = container.focused_window() {
                                        // TODO: Do we need this on macOS?
                                        // Minimize the focused window since Windows might try
                                        // to move it to another monitor if it was focused.
                                        // if window.is_focused() {
                                        //     window.minimize();
                                        // }
                                    }
                                }

                                for window in workspace.floating_windows() {
                                    windows_to_remove.push(window.id);
                                    // TODO: Do we need this on macOS?
                                    // Minimize the focused window since Windows might try
                                    // to move it to another monitor if it was focused.
                                    // if window.is_focused() {
                                    //     window.minimize();
                                    // }
                                }
                            }

                            // Remove any workspace_rules for this specific monitor
                            let mut workspace_rules = WORKSPACE_MATCHING_RULES.lock();
                            let mut rules_to_remove = Vec::new();
                            for (i, rule) in workspace_rules.iter().enumerate().rev() {
                                if rule.monitor_index == m_idx {
                                    rules_to_remove.push(i);
                                }
                            }

                            for i in rules_to_remove {
                                workspace_rules.remove(i);
                            }

                            // Let's add their state to the cache for later, make sure to use what
                            // the user set as preference as the id.
                            let display_index_preferences = DISPLAY_INDEX_PREFERENCES.read();
                            let mut dip_ids = display_index_preferences.values();

                            let preferred_id = if dip_ids.any(|id| m.serial_number_id.eq(id)) {
                                m.serial_number_id.clone()
                            } else {
                                id.to_string()
                            };

                            monitor_cache.insert(preferred_id, m.clone());
                        }
                    }

                    // Update known_window_ids
                    wm.known_window_ids
                        .retain(|i, _| !windows_to_remove.contains(i));

                    if !newly_removed_displays.is_empty() {
                        // After we have cached them, remove them from our state
                        wm.monitors_mut().retain(|m| {
                            !newly_removed_displays
                                .iter()
                                .any(|id| m.serial_number_id.eq(id))
                        });
                    }

                    let post_removal_monitor_count = wm.monitors().len();
                    let focused_monitor_idx = wm.focused_monitor_idx();
                    if focused_monitor_idx >= post_removal_monitor_count {
                        wm.focus_monitor(0)?;
                    }

                    let offset = wm.work_area_offset;

                    for monitor in wm.monitors_mut() {
                        // If we have lost a monitor, update everything to filter out any jank
                        if initial_monitor_count != post_removal_monitor_count {
                            monitor.update_focused_workspace(offset)?;
                        }
                    }
                }

                let post_removal_monitor_count = wm.monitors().len();

                // This is the list of device ids after we have removed detached displays. We can
                // keep this with just the device_ids without the serial numbers since this is used
                // only to check which one is the newly added monitor below if there is a new
                // monitor. Everything done after with said new monitor will again consider both
                // serial number and device ids.
                let post_removal_serial_ids = wm
                    .monitors()
                    .iter()
                    .map(|m| &m.serial_number_id)
                    .cloned()
                    .collect::<Vec<_>>();

                // Check for and add any new monitors that may have been plugged in
                // Monitor and display index preferences get applied in this function
                {
                    // need to drop here because the main thread needs the lock to load the latest
                    // monitor info into the wm state
                    drop(wm);

                    DispatchQueue::main().exec_sync(|| {
                        let mut wm = arc.lock();
                        if let Err(error) = MacosApi::load_monitor_information(&mut wm) {
                            tracing::error!("failed to update load monitor information: {error}");
                        }
                    });
                }

                // regain the lock here once the monitor information has been updated
                let mut wm = arc.lock();

                let post_addition_monitor_count = wm.monitors().len();

                if post_addition_monitor_count > post_removal_monitor_count {
                    tracing::info!(
                        "monitor count mismatch ({post_removal_monitor_count} vs {post_addition_monitor_count}), adding connected monitors",
                    );

                    let known_window_ids = wm.known_window_ids.clone();
                    let offset = wm.work_area_offset;
                    let mouse_follows_focus = wm.mouse_follows_focus;
                    let focused_monitor_idx = wm.focused_monitor_idx();
                    let focused_workspace_idx = wm.focused_workspace_idx()?;

                    // Look in the updated state for new monitors
                    for (i, m) in wm.monitors_mut().iter_mut().enumerate() {
                        // let device_id = &m.device_id;
                        let serial_id = m.serial_number_id.clone();
                        // We identify a new monitor when we encounter a new device id
                        if !post_removal_serial_ids.contains(&serial_id) {
                            let mut cache_hit = false;
                            let mut cached_id = String::new();
                            // Check if that device id exists in the cache for this session
                            if let Some((id, cached)) = monitor_cache.get_key_value(&serial_id) {
                                cache_hit = true;
                                cached_id = id.clone();

                                tracing::info!(
                                    "found monitor and workspace configuration for {id} in the monitor cache, applying"
                                );

                                // If it does, update the cached monitor info with the new one and
                                // load the cached monitor removing any window that has since been
                                // closed or moved to another workspace
                                *m = Monitor {
                                    // Data that should be the one just read from `win32-display-data`
                                    id: m.id,
                                    // name: m.name.clone(),
                                    device: m.device.clone(),
                                    // device_id: m.device_id.clone(),
                                    serial_number_id: m.serial_number_id.clone(),
                                    size: m.size,
                                    work_area_size: m.work_area_size,

                                    // The rest should come from the cached monitor
                                    work_area_offset: cached.work_area_offset,
                                    window_based_work_area_offset: cached
                                        .window_based_work_area_offset,
                                    window_based_work_area_offset_limit: cached
                                        .window_based_work_area_offset_limit,
                                    workspaces: cached.workspaces.clone(),
                                    last_focused_workspace: cached.last_focused_workspace,
                                    // workspace_names: cached.workspace_names.clone(),
                                    container_padding: cached.container_padding,
                                    workspace_padding: cached.workspace_padding,
                                    wallpaper: cached.wallpaper.clone(),
                                    floating_layer_behaviour: cached.floating_layer_behaviour,
                                    window_hiding_position: cached.window_hiding_position,
                                };

                                let focused_workspace_idx = m.focused_workspace_idx();

                                for (j, workspace) in m.workspaces_mut().iter_mut().enumerate() {
                                    // If this is the focused workspace we need to show (restore) all
                                    // windows that were visible since they were probably minimized by
                                    // Windows.
                                    let is_focused_workspace = j == focused_workspace_idx;
                                    let focused_container_idx = workspace.focused_container_idx();

                                    let mut empty_containers = Vec::new();
                                    for (idx, container) in
                                        workspace.containers_mut().iter_mut().enumerate()
                                    {
                                        container.windows_mut().retain(|window| {
                                            window.exe().is_some()
                                                && !known_window_ids.contains_key(&window.id)
                                        });

                                        if container.windows().is_empty() {
                                            empty_containers.push(idx);
                                        }

                                        if is_focused_workspace {
                                            if let Some(_window) = container.focused_window() {
                                                // TODO: do we need to do this on macOS?
                                                // tracing::debug!(
                                                //     "restoring window: {}",
                                                //     window.id
                                                // );
                                                // WindowsApi::restore_window(window.hwnd);
                                            } else {
                                                // If the focused window was moved or removed by
                                                // the user after the disconnect then focus the
                                                // first window and show that one
                                                container.focus_window(0);
                                                //
                                                // if let Some(window) = container.focused_window() {
                                                //     WindowsApi::restore_window(window.hwnd);
                                                // }
                                            }
                                        }
                                    }

                                    // Remove empty containers
                                    for empty_idx in empty_containers {
                                        if empty_idx == focused_container_idx {
                                            workspace.remove_container(empty_idx);
                                        } else {
                                            workspace.remove_container_by_idx(empty_idx);
                                        }
                                    }

                                    // if let Some(window) = &workspace.maximized_window {
                                    //     if window.exe().is_none()
                                    //         || known_window_ids.contains_key(&window.id)
                                    //     {
                                    //         workspace.maximized_window = None;
                                    //     } else if is_focused_workspace {
                                    //         WindowsApi::restore_window(window.hwnd);
                                    //     }
                                    // }

                                    if let Some(container) = &mut workspace.monocle_container {
                                        container.windows_mut().retain(|window| {
                                            window.exe().is_some()
                                                && !known_window_ids.contains_key(&window.id)
                                        });

                                        if container.windows().is_empty() {
                                            workspace.monocle_container = None;
                                        } else if is_focused_workspace {
                                            if let Some(_window) = container.focused_window() {
                                                // WindowsApi::restore_window(window.hwnd);
                                            } else {
                                                // If the focused window was moved or removed by
                                                // the user after the disconnect then focus the
                                                // first window and show that one
                                                container.focus_window(0);

                                                if let Some(_window) = container.focused_window() {
                                                    // WindowsApi::restore_window(window.hwnd);
                                                }
                                            }
                                        }
                                    }

                                    workspace.floating_windows_mut().retain(|window| {
                                        window.exe().is_some()
                                            && !known_window_ids.contains_key(&window.id)
                                    });

                                    if is_focused_workspace {
                                        for _window in workspace.floating_windows() {
                                            // WindowsApi::restore_window(window.hwnd);
                                        }
                                    }

                                    // Apply workspace rules
                                    let mut workspace_matching_rules =
                                        WORKSPACE_MATCHING_RULES.lock();
                                    if let Some(rules) = workspace
                                        .workspace_config
                                        .as_ref()
                                        .and_then(|c| c.workspace_rules.as_ref())
                                    {
                                        for r in rules {
                                            workspace_matching_rules.push(WorkspaceMatchingRule {
                                                monitor_index: i,
                                                workspace_index: j,
                                                matching_rule: r.clone(),
                                                initial_only: false,
                                            });
                                        }
                                    }

                                    if let Some(rules) = workspace
                                        .workspace_config
                                        .as_ref()
                                        .and_then(|c| c.initial_workspace_rules.as_ref())
                                    {
                                        for r in rules {
                                            workspace_matching_rules.push(WorkspaceMatchingRule {
                                                monitor_index: i,
                                                workspace_index: j,
                                                matching_rule: r.clone(),
                                                initial_only: true,
                                            });
                                        }
                                    }
                                }

                                // Restore windows from new monitor and update the focused
                                // workspace
                                m.load_focused_workspace(mouse_follows_focus)?;
                                m.update_focused_workspace(offset)?;
                            }

                            // Entries in the cache should only be used once; remove the entry there was a cache hit
                            if cache_hit && !cached_id.is_empty() {
                                monitor_cache.remove(&cached_id);
                            }
                        }
                    }

                    // Refocus the previously focused monitor since the code above might
                    // steal the focus away.
                    wm.focus_monitor(focused_monitor_idx)?;
                    wm.focus_workspace(focused_workspace_idx)?;
                }

                let final_count = wm.monitors().len();

                if post_removal_monitor_count != final_count {
                    wm.retile_all(true)?;
                    // Second retile to fix DPI/resolution related jank
                    // wm.retile_all(true)?;
                    // Border updates to fix DPI/resolution related jank
                    // border_manager::send_notification(None);
                }

                let mouse_follows_focus = wm.mouse_follows_focus;
                // when a monitor is added/removed some of our "hidden" windows
                // get brought back onto the screen, so we wanna make sure unfocused
                // workspaces get hidden again
                if initial_monitor_count != final_count {
                    let offset = wm.work_area_offset;
                    for monitor in wm.monitors_mut() {
                        monitor.update_all_workspace_globals(offset);
                        monitor.load_focused_workspace(mouse_follows_focus)?;
                    }
                }
            }
        }

        {
            let wm = wm.lock();
            notify_subscribers(
                Notification {
                    event: NotificationEvent::Monitor(notification),
                    state: wm.as_ref().into(),
                },
                initial_state.has_been_modified(&wm),
            )?;
        }

        border_manager::send_notification(None, None, false);
    }

    Ok(())
}
