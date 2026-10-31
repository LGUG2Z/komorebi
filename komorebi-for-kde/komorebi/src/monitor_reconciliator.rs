//! Keeping komorebi's monitors in step with the compositor's outputs, as they change size, are
//! unplugged and plugged back, and come back from a sleep or a lock.
//!
//! The KWin backend sends a [`MonitorNotification`] when KWin's list of outputs changes, and
//! [`kwin::power`](crate::kwin::power) sends one when the machine wakes or the screen is unlocked.
//! The thread [`listen_for_notifications`] starts takes them one at a time:
//!
//! - [`MonitorNotification::Resize`]: the size and work area of every monitor are read again
//!   ([`desktop::update_monitor_work_areas`]), and the focused workspace of the focused monitor
//!   is tiled in the area it has now.
//! - [`MonitorNotification::DisplayConnectionChange`]: [`reconcile_display_change`] removes the
//!   monitors that went, once what was on them is on the monitors that are left, and adds the ones
//!   that came.
//! - [`MonitorNotification::Wake`]: [`wake_up`] does both, and puts every window in its tile.
//!
//! A monitor that is unplugged is kept, with its workspaces and settings, and a monitor plugged in
//! with the same serial number id gets them back. The workspace rules of a monitor that goes are
//! dropped, and the rules of the workspaces of one that comes back apply again. A moment after each
//! notification, the windows are followed to the monitors the compositor moved them to
//! ([`WindowManager::follow_windows_to_their_monitors`]).
//!
//! Every subscriber is sent a notification after each one is handled
//! ([`NotificationEvent::Monitor`]), and the backend is asked for the look again
//! ([`border_manager::send_notification`]), which it does nothing about unless a setting changed.
//!
//! Covered by `tests/monitor_resize.rs`, `tests/hotplug.rs`, `tests/follow_output.rs` and
//! `tests/wake.rs` against the fake backend, and by the `vm-hotplug` and `vm-lock-sleep` VM suites.

use crate::DISPLAY_INDEX_PREFERENCES;
use crate::Notification;
use crate::NotificationEvent;
use crate::WORKSPACE_MATCHING_RULES;
use crate::border_manager;
use crate::core::config_generation::WorkspaceMatchingRule;
use crate::desktop;
use crate::monitor::Monitor;
use crate::notify_subscribers;
use crate::state::State;
use crate::window_manager::WindowManager;
use crossbeam_channel::Receiver;
use crossbeam_channel::Sender;
use parking_lot::Mutex;
use serde::Deserialize;
use serde::Serialize;
use std::collections::HashMap;
use std::sync::Arc;
use std::sync::OnceLock;
use std::sync::atomic::AtomicBool;
use std::sync::atomic::Ordering;
use std::time::Duration;
use strum::Display;

/// A change of the monitors, sent to the monitor reconciliator with [`send_notification`].
///
/// The number in `Resize` and `DisplayConnectionChange` is a KWin monitor id, which the
/// reconciliator does not read: it reads every monitor again.
#[derive(Debug, Copy, Clone, Serialize, Deserialize, Display)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
pub enum MonitorNotification {
    /// A monitor's size or work area changed (a panel moved, a resolution or scale changed): the
    /// id of the monitor that changed, or 0 when the KWin backend cannot tell which
    Resize(u32),
    /// A monitor was connected or disconnected: the KWin backend sends 0
    DisplayConnectionChange(u32),
    /// The machine woke up, or the screen was unlocked: look at the monitors and at where the
    /// windows are, and put each where it belongs
    Wake,
}

static CHANNEL: OnceLock<(Sender<MonitorNotification>, Receiver<MonitorNotification>)> =
    OnceLock::new();

static MONITOR_CACHE: OnceLock<Mutex<HashMap<String, Monitor>>> = OnceLock::new();

/// The channel [`send_notification`] sends on and the thread of [`listen_for_notifications`]
/// receives from. It holds 20 notifications.
pub fn channel() -> &'static (Sender<MonitorNotification>, Receiver<MonitorNotification>) {
    CHANNEL.get_or_init(|| crossbeam_channel::bounded(20))
}

fn event_tx() -> Sender<MonitorNotification> {
    channel().0.clone()
}

fn event_rx() -> Receiver<MonitorNotification> {
    channel().1.clone()
}

/// Keeps `monitor`, with its workspaces and settings, for when a monitor with the same serial
/// number id is plugged in again.
///
/// It is kept under `serial_number_id`, except when a display index preference has that id as its
/// value: then it is kept under the monitor's own serial number id.
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

/// Whether a [`settle_later`] is waiting, so that a burst of notifications makes one.
static SETTLING: AtomicBool = AtomicBool::new(false);

/// The compositor moves windows about for a moment after the monitors change: an output that went
/// takes its windows to another, one that came back may take them again, a scale or a resolution
/// changes where the middle of a window is. A moment later (1.5 seconds), see where they all are,
/// with [`WindowManager::follow_windows_to_their_monitors`]. A call made while one is waiting
/// does nothing.
fn settle_later(wm: Arc<Mutex<WindowManager>>) {
    if SETTLING.swap(true, Ordering::SeqCst) {
        return;
    }

    std::thread::spawn(move || {
        std::thread::sleep(Duration::from_millis(1500));
        SETTLING.store(false, Ordering::SeqCst);

        let mut wm = wm.lock();
        match wm.follow_windows_to_their_monitors() {
            Ok(true) => tracing::info!("windows followed the monitors they were moved to"),
            Ok(false) => {}
            Err(error) => {
                tracing::error!("could not follow the windows to their monitors: {error}")
            }
        }
    });
}

/// Sends `notification` to the monitor reconciliator, without waiting. A notification is dropped,
/// with a warning in the log, when the channel is full.
pub fn send_notification(notification: MonitorNotification) {
    if event_tx().try_send(notification).is_err() {
        tracing::warn!("channel is full; dropping notification")
    }
}

/// Starts the thread that takes the notifications, for as long as komorebi runs, and returns at
/// once.
///
/// [`handle_notifications`] is called again when it returns, and the reason is logged.
///
/// # Errors
///
/// None: the result is always `Ok`.
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

/// Brings komorebi's monitors in line with the compositor's: removes the ones that went and adds
/// the ones that came.
///
/// The containers and windows of a monitor that went are moved with
/// [`WindowManager::evacuate_monitor`] to the monitor the compositor took each window to, and the
/// monitor itself is kept for when it comes back. A monitor that comes back gets what was kept of
/// it, less the windows that closed or that are now somewhere else, and the workspace rules of
/// its workspaces (`WORKSPACE_MATCHING_RULES`) apply again; those of a monitor that went are
/// dropped. When the number of monitors has not changed, only the id, size and work area of each
/// are read again; when the compositor reports no monitor at all, nothing is done.
///
/// # Errors
///
/// Returns an error if focusing a monitor or workspace, or showing and laying out the workspaces
/// of the monitors, fails. A monitor list that cannot be read is taken as no change, and a
/// monitor whose windows cannot be moved is logged.
pub fn reconcile_display_change(wm: &mut WindowManager) -> color_eyre::Result<()> {
    let attached_devices = desktop::latest_monitor_information().unwrap_or_default();

    let mut monitor_cache = MONITOR_CACHE
        .get_or_init(|| Mutex::new(HashMap::new()))
        .lock();

    let initial_monitor_count = wm.monitors().len();

    // Make sure that in our state any attached displays have the latest data from the desktop
    for monitor in wm.monitors_mut() {
        for attached in &attached_devices {
            let serial_number_ids_match = attached.serial_number_id == monitor.serial_number_id;

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
        tracing::debug!("monitor counts match, reconciliation not required");
        return Ok(());
    }

    if attached_devices.is_empty() {
        tracing::debug!("no devices found, skipping reconciliation to avoid breaking state");
        return Ok(());
    }

    if initial_monitor_count > attached_devices.len() {
        tracing::info!(
            "monitor count mismatch ({initial_monitor_count} vs {}), removing disconnected monitors",
            attached_devices.len()
        );

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

        // What was on the monitors that went is now on the ones that are left, because
        // that is where the compositor took the windows: it is theirs to tile
        for id in &newly_removed_displays {
            if let Err(error) = wm.evacuate_monitor(id) {
                tracing::error!("could not move the windows of {id}: {error}");
            }
        }
        wm.update_known_window_ids();

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
    if let Err(error) = desktop::load_monitor_information(wm) {
        tracing::error!("failed to update load monitor information: {error}");
    }

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
                        // Data that should be the one just read from the compositor
                        id: m.id,
                        device: m.device.clone(),
                        serial_number_id: m.serial_number_id.clone(),
                        size: m.size,
                        work_area_size: m.work_area_size,

                        // The rest should come from the cached monitor
                        work_area_offset: cached.work_area_offset,
                        window_based_work_area_offset: cached.window_based_work_area_offset,
                        window_based_work_area_offset_limit: cached
                            .window_based_work_area_offset_limit,
                        workspaces: cached.workspaces.clone(),
                        last_focused_workspace: cached.last_focused_workspace,
                        container_padding: cached.container_padding,
                        workspace_padding: cached.workspace_padding,
                        wallpaper: cached.wallpaper.clone(),
                        floating_layer_behaviour: cached.floating_layer_behaviour,
                    };

                    let focused_workspace_idx = m.focused_workspace_idx();

                    for (j, workspace) in m.workspaces_mut().iter_mut().enumerate() {
                        let is_focused_workspace = j == focused_workspace_idx;
                        let focused_container_idx = workspace.focused_container_idx();

                        let mut empty_containers = Vec::new();
                        for (idx, container) in workspace.containers_mut().iter_mut().enumerate() {
                            container.windows_mut().retain(|window| {
                                window.exe().is_some() && !known_window_ids.contains_key(&window.id)
                            });

                            if container.windows().is_empty() {
                                empty_containers.push(idx);
                            }

                            // If the focused window was moved or removed by the user after
                            // the disconnect then focus the first window
                            if is_focused_workspace && container.focused_window().is_none() {
                                container.focus_window(0);
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

                        if let Some(container) = &mut workspace.monocle_container {
                            container.windows_mut().retain(|window| {
                                window.exe().is_some() && !known_window_ids.contains_key(&window.id)
                            });

                            if container.windows().is_empty() {
                                workspace.monocle_container = None;
                            } else if is_focused_workspace && container.focused_window().is_none() {
                                // If the focused window was moved or removed by the user after
                                // the disconnect then focus the first window
                                container.focus_window(0);
                            }
                        }

                        workspace.floating_windows_mut().retain(|window| {
                            window.exe().is_some() && !known_window_ids.contains_key(&window.id)
                        });

                        // Apply workspace rules
                        let mut workspace_matching_rules = WORKSPACE_MATCHING_RULES.lock();
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
    }

    let mouse_follows_focus = wm.mouse_follows_focus;
    // the monitors are not the ones they were: show the workspace in front of each again
    if initial_monitor_count != final_count {
        for monitor in wm.monitors_mut() {
            monitor.load_focused_workspace(mouse_follows_focus)?;
        }
    }
    Ok(())
}

/// The desktop after a sleep or a lock: the monitors as they are now, the windows on the monitors
/// they are on, and every one of them where its tile is
///
/// Nothing is done while komorebi is paused.
///
/// # Errors
///
/// Returns the errors of [`reconcile_display_change`] and of
/// [`WindowManager::follow_windows_to_their_monitors`], and an error if the monitors cannot be
/// read again or the windows cannot be tiled.
pub fn wake_up(wm: &mut WindowManager) -> color_eyre::Result<()> {
    if wm.is_paused {
        return Ok(());
    }

    reconcile_display_change(wm)?;
    desktop::update_monitor_work_areas(wm)?;
    wm.follow_windows_to_their_monitors()?;
    wm.retile_all(true)?;

    Ok(())
}

/// Takes the notifications one at a time, for as long as komorebi runs.
///
/// Each is dealt with as the [module docs](self) say, and a moment later the windows are followed
/// to their monitors. The subscribers are then notified, and the backend is asked for the look
/// again.
///
/// # Errors
///
/// Returns an error if [`reconcile_display_change`] fails for a
/// [`MonitorNotification::DisplayConnectionChange`], or if the subscribers cannot be notified;
/// [`listen_for_notifications`] then calls it again. The errors of the other notifications are
/// logged.
pub fn handle_notifications(wm: Arc<Mutex<WindowManager>>) -> color_eyre::Result<()> {
    tracing::info!("listening");
    let receiver = event_rx();

    for notification in receiver {
        let initial_state = {
            let wm = wm.lock();
            State::from(wm.as_ref())
        };

        match notification {
            MonitorNotification::Resize(_display_id) => {
                tracing::debug!("handling resize notification");
                let mut wm = wm.lock();
                if let Err(error) = desktop::update_monitor_work_areas(&mut wm) {
                    tracing::error!("failed to update monitor work areas: {error}");
                }
            }
            MonitorNotification::Wake => {
                tracing::debug!("handling wake notification");
                let mut wm = wm.lock();
                if let Err(error) = wake_up(&mut wm) {
                    tracing::error!("could not put the desktop in order after waking: {error}");
                }
            }
            MonitorNotification::DisplayConnectionChange(_) => {
                tracing::debug!("handling display connection change notification");
                let mut wm = wm.lock();
                reconcile_display_change(&mut wm)?;
            }
        }

        settle_later(wm.clone());

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

        border_manager::send_notification();
    }

    Ok(())
}
