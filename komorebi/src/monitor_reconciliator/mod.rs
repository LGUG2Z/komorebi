#![deny(clippy::unwrap_used, clippy::expect_used)]

use crate::border_manager;
use crate::monitor;
use crate::monitor::Monitor;
use crate::monitor_reconciliator::hidden::Hidden;
use crate::MonitorConfig;
use crate::WindowManager;
use crate::WindowsApi;
use crossbeam_channel::Receiver;
use crossbeam_channel::Sender;
use crossbeam_utils::atomic::AtomicConsume;
use komorebi_core::Rect;
use parking_lot::Mutex;
use std::collections::HashMap;
use std::sync::atomic::AtomicBool;
use std::sync::atomic::Ordering;
use std::sync::Arc;
use std::sync::OnceLock;

pub mod hidden;

pub enum Notification {
    ResolutionScalingChanged,
    WorkAreaChanged,
    DisplayConnectionChange,
    EnteringSuspendedState,
    ResumingFromSuspendedState,
    SessionLocked,
    SessionUnlocked,
}

static ACTIVE: AtomicBool = AtomicBool::new(true);

static CHANNEL: OnceLock<(Sender<Notification>, Receiver<Notification>)> = OnceLock::new();

static MONITOR_CACHE: OnceLock<Mutex<HashMap<String, MonitorConfig>>> = OnceLock::new();

pub fn channel() -> &'static (Sender<Notification>, Receiver<Notification>) {
    CHANNEL.get_or_init(|| crossbeam_channel::bounded(1))
}

fn event_tx() -> Sender<Notification> {
    channel().0.clone()
}

fn event_rx() -> Receiver<Notification> {
    channel().1.clone()
}

pub fn send_notification(notification: Notification) {
    if event_tx().try_send(notification).is_err() {
        tracing::warn!("channel is full; dropping notification")
    }
}

pub fn insert_in_monitor_cache(device_id: &str, config: MonitorConfig) {
    let mut monitor_cache = MONITOR_CACHE
        .get_or_init(|| Mutex::new(HashMap::new()))
        .lock();

    monitor_cache.insert(device_id.to_string(), config);
}

pub fn attached_display_devices() -> color_eyre::Result<Vec<Monitor>> {
    Ok(win32_display_data::connected_displays_all()
        .flatten()
        .map(|display| {
            let path = display.device_path;

            let (device, device_id) = if path.is_empty() {
                (String::from("UNKNOWN"), String::from("UNKNOWN"))
            } else {
                let mut split: Vec<_> = path.split('#').collect();
                split.remove(0);
                split.remove(split.len() - 1);
                let device = split[0].to_string();
                let device_id = split.join("-");
                (device, device_id)
            };

            let name = display.device_name.trim_start_matches(r"\\.\").to_string();
            let name = name.split('\\').collect::<Vec<_>>()[0].to_string();

            monitor::new(
                display.hmonitor,
                display.size.into(),
                display.work_area_size.into(),
                name,
                device,
                device_id,
            )
        })
        .collect::<Vec<_>>())
}
pub fn listen_for_notifications(wm: Arc<Mutex<WindowManager>>) -> color_eyre::Result<()> {
    #[allow(clippy::expect_used)]
    Hidden::create("komorebi-hidden")?;

    tracing::info!("created hidden window to listen for monitor-related events");

    std::thread::spawn(move || loop {
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
    });

    Ok(())
}
pub fn handle_notifications(wm: Arc<Mutex<WindowManager>>) -> color_eyre::Result<()> {
    tracing::info!("listening");

    let receiver = event_rx();

    'receiver: for notification in receiver {
        if !ACTIVE.load_consume() {
            if matches!(
                notification,
                Notification::ResumingFromSuspendedState | Notification::SessionUnlocked
            ) {
                tracing::debug!(
                    "reactivating reconciliator - system has resumed from suspended state or session has been unlocked"
                );

                ACTIVE.store(true, Ordering::SeqCst);
            }

            continue 'receiver;
        }

        let mut wm = wm.lock();

        match notification {
            Notification::EnteringSuspendedState | Notification::SessionLocked => {
                tracing::debug!(
                    "deactivating reconciliator until system resumes from suspended state or session is unlocked"
                );
                ACTIVE.store(false, Ordering::SeqCst);
            }
            Notification::ResumingFromSuspendedState | Notification::SessionUnlocked => {
                // this is only handled above if the reconciliator is paused
            }
            Notification::WorkAreaChanged => {
                tracing::debug!("handling work area changed notification");
                let offset = wm.work_area_offset;
                for monitor in wm.monitors_mut() {
                    let mut should_update = false;

                    // Update work areas as necessary
                    if let Ok(reference) = WindowsApi::monitor(monitor.id()) {
                        if reference.work_area_size() != monitor.work_area_size() {
                            monitor.set_work_area_size(Rect {
                                left: reference.work_area_size().left,
                                top: reference.work_area_size().top,
                                right: reference.work_area_size().right,
                                bottom: reference.work_area_size().bottom,
                            });

                            should_update = true;
                        }
                    }

                    if should_update {
                        tracing::info!("updated work area for {}", monitor.device_id());
                        monitor.update_focused_workspace(offset)?;
                        border_manager::send_notification();
                    } else {
                        tracing::debug!(
                            "work areas match, reconciliation not required for {}",
                            monitor.device_id()
                        );
                    }
                }
            }
            Notification::ResolutionScalingChanged => {
                tracing::debug!("handling resolution/scaling changed notification");
                let offset = wm.work_area_offset;
                for monitor in wm.monitors_mut() {
                    let mut should_update = false;

                    // Update sizes and work areas as necessary
                    if let Ok(reference) = WindowsApi::monitor(monitor.id()) {
                        if reference.work_area_size() != monitor.work_area_size() {
                            monitor.set_work_area_size(Rect {
                                left: reference.work_area_size().left,
                                top: reference.work_area_size().top,
                                right: reference.work_area_size().right,
                                bottom: reference.work_area_size().bottom,
                            });

                            should_update = true;
                        }

                        if reference.size() != monitor.size() {
                            monitor.set_size(Rect {
                                left: reference.size().left,
                                top: reference.size().top,
                                right: reference.size().right,
                                bottom: reference.size().bottom,
                            });

                            should_update = true;
                        }
                    }

                    if should_update {
                        tracing::info!(
                            "updated monitor resolution/scaling for {}",
                            monitor.device_id()
                        );

                        monitor.update_focused_workspace(offset)?;
                        border_manager::send_notification();
                    } else {
                        tracing::debug!(
                            "resolutions match, reconciliation not required for {}",
                            monitor.device_id()
                        );
                    }
                }
            }
            Notification::DisplayConnectionChange => {
                tracing::debug!("handling display connection change notification");
                let mut monitor_cache = MONITOR_CACHE
                    .get_or_init(|| Mutex::new(HashMap::new()))
                    .lock();

                let initial_monitor_count = wm.monitors().len();

                // Get the currently attached display devices
                let attached_devices = attached_display_devices()?;

                // Make sure that in our state any attached displays have the latest Win32 data
                for monitor in wm.monitors_mut() {
                    for attached in &attached_devices {
                        if attached.device_id().eq(monitor.device_id()) {
                            monitor.set_id(attached.id());
                            monitor.set_name(attached.name().clone());
                            monitor.set_size(*attached.size());
                            monitor.set_work_area_size(*attached.work_area_size());
                        }
                    }
                }

                if initial_monitor_count == attached_devices.len() {
                    tracing::debug!("monitor counts match, reconciliation not required");
                    continue 'receiver;
                }

                if attached_devices.is_empty() {
                    tracing::debug!(
                        "no devices found, skipping reconciliation to avoid breaking state"
                    );
                    continue 'receiver;
                }

                if initial_monitor_count > attached_devices.len() {
                    tracing::info!(
                        "monitor count mismatch ({initial_monitor_count} vs {}), removing disconnected monitors",
                        attached_devices.len()
                    );

                    // Gather all the containers that will be orphaned from disconnected and invalid displays
                    let mut orphaned_containers = vec![];

                    // Collect the ids in our state which aren't in the current attached display ids
                    // These are monitors that have been removed
                    let mut newly_removed_displays = vec![];

                    for m in wm.monitors().iter() {
                        if !attached_devices
                            .iter()
                            .any(|attached| attached.device_id().eq(m.device_id()))
                        {
                            newly_removed_displays.push(m.device_id().clone());
                            for workspace in m.workspaces() {
                                for container in workspace.containers() {
                                    // Save the orphaned containers from the removed monitor
                                    orphaned_containers.push(container.clone());
                                }
                            }

                            // Let's add their state to the cache for later
                            monitor_cache.insert(m.device_id().clone(), m.into());
                        }
                    }

                    if !orphaned_containers.is_empty() {
                        tracing::info!(
                            "removed orphaned containers from: {newly_removed_displays:?}"
                        );
                    }

                    if !newly_removed_displays.is_empty() {
                        // After we have cached them, remove them from our state
                        wm.monitors_mut()
                            .retain(|m| !newly_removed_displays.contains(m.device_id()));
                    }

                    let post_removal_monitor_count = wm.monitors().len();
                    let focused_monitor_idx = wm.focused_monitor_idx();
                    if focused_monitor_idx >= post_removal_monitor_count {
                        wm.focus_monitor(0)?;
                    }

                    if !orphaned_containers.is_empty() {
                        if let Some(primary) = wm.monitors_mut().front_mut() {
                            if let Some(focused_ws) = primary.focused_workspace_mut() {
                                let focused_container_idx = focused_ws.focused_container_idx();

                                // Put the orphaned containers somewhere visible
                                for container in orphaned_containers {
                                    focused_ws.add_container(container);
                                }

                                // Gotta reset the focus or the movement will feel "off"
                                if initial_monitor_count != post_removal_monitor_count {
                                    focused_ws.focus_container(focused_container_idx);
                                }
                            }
                        }
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

                // This is the list of device ids after we have removed detached displays
                let post_removal_device_ids = wm
                    .monitors()
                    .iter()
                    .map(Monitor::device_id)
                    .cloned()
                    .collect::<Vec<_>>();

                // Check for and add any new monitors that may have been plugged in
                // Monitor and display index preferences get applied in this function
                WindowsApi::load_monitor_information(&mut wm.monitors)?;

                let post_addition_monitor_count = wm.monitors().len();

                if post_addition_monitor_count > post_removal_monitor_count {
                    tracing::info!(
                        "monitor count mismatch ({post_removal_monitor_count} vs {post_addition_monitor_count}), adding connected monitors",
                    );

                    // Look in the updated state for new monitors
                    for m in wm.monitors_mut() {
                        let device_id = m.device_id().clone();
                        // We identify a new monitor when we encounter a new device id
                        if !post_removal_device_ids.contains(&device_id) {
                            let mut cache_hit = false;
                            // Check if that device id exists in the cache for this session
                            if let Some(cached) = monitor_cache.get(&device_id) {
                                cache_hit = true;

                                tracing::info!("found monitor and workspace configuration for {device_id} in the monitor cache, applying");

                                // If it does, load all the monitor settings from the cache entry
                                m.ensure_workspace_count(cached.workspaces.len());
                                m.set_work_area_offset(cached.work_area_offset);
                                m.set_window_based_work_area_offset(
                                    cached.window_based_work_area_offset,
                                );
                                m.set_window_based_work_area_offset_limit(
                                    cached.window_based_work_area_offset_limit.unwrap_or(1),
                                );

                                for (w_idx, workspace) in m.workspaces_mut().iter_mut().enumerate()
                                {
                                    if let Some(cached_workspace) = cached.workspaces.get(w_idx) {
                                        workspace.load_static_config(cached_workspace)?;
                                    }
                                }
                            }

                            // Entries in the cache should only be used once; remove the entry there was a cache hit
                            if cache_hit {
                                monitor_cache.remove(&device_id);
                            }
                        }
                    }
                }

                let final_count = wm.monitors().len();

                if post_removal_monitor_count != final_count {
                    wm.retile_all(true)?;
                    // Second retile to fix DPI/resolution related jank
                    wm.retile_all(true)?;
                    // Border updates to fix DPI/resolution related jank
                    border_manager::send_notification();
                }
            }
        }
    }

    Ok(())
}
