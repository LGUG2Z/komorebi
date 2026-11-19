#![deny(clippy::unwrap_used, clippy::expect_used)]

use crate::DISPLAY_INDEX_PREFERENCES;
use crate::DUPLICATE_MONITOR_SERIAL_IDS;
use crate::Notification;
use crate::NotificationEvent;
use crate::WORKSPACE_MATCHING_RULES;
use crate::WindowManager;
use crate::WindowsApi;
use crate::border_manager;
use crate::config_generation::WorkspaceMatchingRule;
use crate::core::Rect;
use crate::monitor;
use crate::monitor::Monitor;
use crate::monitor_reconciliator::hidden::Hidden;
use crate::notify_subscribers;
use crate::state::State;
use crossbeam_channel::Receiver;
use crossbeam_channel::Sender;
use crossbeam_utils::atomic::AtomicConsume;
use parking_lot::Mutex;
use serde::Deserialize;
use serde::Serialize;
use std::collections::HashMap;
use std::sync::Arc;
use std::sync::OnceLock;
use std::sync::atomic::AtomicBool;
use std::sync::atomic::AtomicI64;
use std::sync::atomic::Ordering;

pub mod hidden;

#[derive(Debug, Copy, Clone, Serialize, Deserialize, PartialEq)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[serde(tag = "type", content = "content")]
pub enum MonitorNotification {
    ResolutionScalingChanged,
    WorkAreaChanged,
    DisplayConnectionChange,
    EnteringSuspendedState,
    ResumingFromSuspendedState,
    SessionLocked,
    SessionUnlocked,
}

static ACTIVE: AtomicBool = AtomicBool::new(true);

/// Timestamp (epoch millis) of the last DisplayConnectionChange notification.
/// Used to suppress OS-initiated window minimizes during transient display events.
static LAST_DISPLAY_CHANGE_TIMESTAMP: AtomicI64 = AtomicI64::new(0);

static CHANNEL: OnceLock<(Sender<MonitorNotification>, Receiver<MonitorNotification>)> =
    OnceLock::new();

static MONITOR_CACHE: OnceLock<Mutex<Vec<CachedMonitor>>> = OnceLock::new();

/// Keeps session display identity separate from an explicit configuration binding.
/// Empty serials and shared UNKNOWN device IDs cannot key the cache. Session
/// matching accepts either a serial or device match and is nontransitive, so it
/// cannot implement Hash/Eq. Configured bindings also cover future displays whose
/// physical identity is not yet known.
struct CachedMonitor {
    // Configured bindings must not fall back to the cached display's other identities.
    configured_id: Option<String>,
    monitor: Monitor,
}

fn configured_cache_id(id: &str, monitor: &Monitor) -> Option<String> {
    let preferences = DISPLAY_INDEX_PREFERENCES.read();
    if preferences.values().any(|id| id == &monitor.device_id) {
        Some(monitor.device_id.clone())
    } else if let Some(serial) = monitor.serial_number_id.as_ref().filter(|s| !s.is_empty())
        && preferences.values().any(|id| id == serial)
    {
        Some(serial.clone())
    } else if monitor.device_id.is_empty() && monitor.name.is_empty() && !id.is_empty() {
        // Static config can cache a future monitor before any physical identity is known.
        Some(id.to_string())
    } else {
        None
    }
}

fn cached_session_monitor_index(cache: &[CachedMonitor], monitor: &Monitor) -> Option<usize> {
    // A physical serial takes precedence when another cached display occupied the same port.
    monitor
        .serial_number_id
        .as_ref()
        .filter(|serial| !serial.is_empty())
        .and_then(|serial| {
            cache.iter().position(|cached| {
                cached.configured_id.is_none()
                    && cached.monitor.serial_number_id.as_ref() == Some(serial)
            })
        })
        .or_else(|| {
            cache.iter().position(|cached| {
                cached.configured_id.is_none() && same_display(&cached.monitor, monitor)
            })
        })
}

fn cache_monitor(cache: &mut Vec<CachedMonitor>, configured_id: Option<String>, monitor: Monitor) {
    let existing = match &configured_id {
        Some(id) => cache
            .iter()
            .position(|cached| cached.configured_id.as_ref() == Some(id)),
        None => cached_session_monitor_index(cache, &monitor),
    };
    let entry = CachedMonitor {
        configured_id,
        monitor,
    };
    if let Some(index) = existing {
        cache[index] = entry;
    } else {
        cache.push(entry);
    }
}

fn cached_monitor_index(cache: &[CachedMonitor], monitor: &Monitor) -> Option<usize> {
    // Preserve configured device-id priority over serial, then try session identity.
    cache
        .iter()
        .position(|cached| cached.configured_id.as_ref() == Some(&monitor.device_id))
        .or_else(|| {
            monitor
                .serial_number_id
                .as_ref()
                .filter(|s| !s.is_empty())
                .and_then(|serial| {
                    cache
                        .iter()
                        .position(|cached| cached.configured_id.as_ref() == Some(serial))
                })
        })
        .or_else(|| cached_session_monitor_index(cache, monitor))
}

pub fn channel() -> &'static (Sender<MonitorNotification>, Receiver<MonitorNotification>) {
    CHANNEL.get_or_init(|| crossbeam_channel::bounded(20))
}

fn event_tx() -> Sender<MonitorNotification> {
    channel().0.clone()
}

fn event_rx() -> Receiver<MonitorNotification> {
    channel().1.clone()
}

pub fn send_notification(notification: MonitorNotification) {
    if matches!(
        notification,
        MonitorNotification::DisplayConnectionChange
            | MonitorNotification::ResumingFromSuspendedState
            | MonitorNotification::SessionUnlocked
    ) {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as i64;
        LAST_DISPLAY_CHANGE_TIMESTAMP.store(now, Ordering::SeqCst);
    }

    if event_tx().try_send(notification).is_err() {
        tracing::warn!("channel is full; dropping notification")
    }
}

/// Returns true if a display connection change event was received within the
/// last `grace_period` duration. This is used by the event processor to avoid
/// treating OS-initiated minimizes (caused by transient monitor disconnects)
/// as user-initiated minimizes.
pub fn display_change_in_progress(grace_period: std::time::Duration) -> bool {
    let last = LAST_DISPLAY_CHANGE_TIMESTAMP.load(Ordering::SeqCst);
    if last == 0 {
        return false;
    }
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as i64;
    (now - last) < grace_period.as_millis() as i64
}

pub fn insert_in_monitor_cache(serial_or_device_id: &str, monitor: Monitor) {
    let configured_id = configured_cache_id(serial_or_device_id, &monitor);
    let mut monitor_cache = MONITOR_CACHE.get_or_init(|| Mutex::new(Vec::new())).lock();

    cache_monitor(&mut monitor_cache, configured_id, monitor);
}

fn monitor_topology_matches(
    monitors: &std::collections::VecDeque<Monitor>,
    attached_devices: &[Monitor],
) -> bool {
    monitors.len() == attached_devices.len()
        && disconnected_monitor_indices(monitors, attached_devices).is_empty()
}

pub fn attached_display_devices<F, I>(display_provider: F) -> color_eyre::Result<Vec<Monitor>>
where
    F: Fn() -> I + Copy,
    I: Iterator<Item = Result<win32_display_data::Device, win32_display_data::Error>>,
{
    let mut attempts = 0;

    let (displays, errors) = loop {
        let (displays, errors): (Vec<_>, Vec<_>) = display_provider().partition(Result::is_ok);

        if errors.is_empty() {
            break (displays, errors);
        }

        for err in &errors {
            if let Err(e) = err {
                tracing::warn!(
                    "enumerating display in reconciliator (attempt {}): {:?}",
                    attempts + 1,
                    e
                );
            }
        }

        if attempts < 5 {
            attempts += 1;
            std::thread::sleep(std::time::Duration::from_millis(150));
            continue;
        }

        break (displays, errors);
    };

    if !errors.is_empty() {
        return Err(color_eyre::eyre::eyre!(
            "could not successfully enumerate all displays"
        ));
    }

    let all_displays = displays.into_iter().map(Result::unwrap).collect::<Vec<_>>();

    let mut serial_id_map = HashMap::new();

    for d in &all_displays {
        if let Some(id) = &d.serial_number_id {
            *serial_id_map.entry(id.clone()).or_insert(0) += 1;
        }
    }

    for d in &all_displays {
        if let Some(id) = &d.serial_number_id
            && serial_id_map.get(id).copied().unwrap_or_default() > 1
        {
            let mut dupes = DUPLICATE_MONITOR_SERIAL_IDS.write();
            if !dupes.contains(id) {
                (*dupes).push(id.clone());
            }
        }
    }

    Ok(all_displays
        .into_iter()
        .map(|mut display| {
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

            if let Some(id) = &display.serial_number_id {
                let dupes = DUPLICATE_MONITOR_SERIAL_IDS.read();
                if dupes.contains(id) {
                    display.serial_number_id = None;
                }
            }

            monitor::new(
                display.hmonitor,
                display.size.into(),
                display.work_area_size.into(),
                name,
                device,
                device_id,
                display.serial_number_id,
            )
        })
        .collect::<Vec<_>>())
}

pub fn listen_for_notifications(wm: Arc<Mutex<WindowManager>>) -> color_eyre::Result<()> {
    #[allow(clippy::expect_used)]
    Hidden::create("komorebi-hidden")?;

    tracing::info!("created hidden window to listen for monitor-related events");

    std::thread::spawn(move || {
        loop {
            match handle_notifications(wm.clone(), win32_display_data::connected_displays_all) {
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

pub fn handle_notifications<F, I>(
    wm: Arc<Mutex<WindowManager>>,
    display_provider: F,
) -> color_eyre::Result<()>
where
    F: Fn() -> I + Copy,
    I: Iterator<Item = Result<win32_display_data::Device, win32_display_data::Error>>,
{
    handle_notifications_with_receiver(wm, display_provider, event_rx())
}

fn handle_notifications_with_receiver<F, I>(
    wm: Arc<Mutex<WindowManager>>,
    display_provider: F,
    receiver: Receiver<MonitorNotification>,
) -> color_eyre::Result<()>
where
    F: Fn() -> I + Copy,
    I: Iterator<Item = Result<win32_display_data::Device, win32_display_data::Error>>,
{
    tracing::info!("listening");

    'receiver: for notification in receiver {
        if !ACTIVE.load_consume()
            && matches!(
                notification,
                MonitorNotification::ResumingFromSuspendedState
                    | MonitorNotification::SessionUnlocked
            )
        {
            tracing::debug!(
                "reactivating reconciliator - system has resumed from suspended state or session has been unlocked"
            );

            ACTIVE.store(true, Ordering::SeqCst);
            border_manager::send_notification(None);
        }

        // Keep reference to Arc for potential re-locking
        let wm_arc = Arc::clone(&wm);
        let mut wm = wm.lock();

        let initial_state = State::from(wm.as_ref());

        match notification {
            MonitorNotification::EnteringSuspendedState | MonitorNotification::SessionLocked => {
                tracing::debug!(
                    "deactivating reconciliator until system resumes from suspended state or session is unlocked"
                );
                ACTIVE.store(false, Ordering::SeqCst);
            }
            MonitorNotification::WorkAreaChanged => {
                tracing::debug!("handling work area changed notification");
                let offset = wm.work_area_offset;
                for monitor in wm.monitors_mut() {
                    let mut should_update = false;

                    // Update work areas as necessary
                    if let Ok(reference) = WindowsApi::monitor(monitor.id)
                        && reference.work_area_size != monitor.work_area_size
                    {
                        monitor.work_area_size = Rect {
                            left: reference.work_area_size.left,
                            top: reference.work_area_size.top,
                            right: reference.work_area_size.right,
                            bottom: reference.work_area_size.bottom,
                        };

                        should_update = true;
                    }

                    if should_update {
                        tracing::info!("updated work area for {}", monitor.device_id);
                        monitor.update_focused_workspace(offset)?;
                        border_manager::send_notification(None);
                    } else {
                        tracing::debug!(
                            "work areas match, reconciliation not required for {}",
                            monitor.device_id
                        );
                    }
                }
            }
            MonitorNotification::ResolutionScalingChanged => {
                tracing::debug!("handling resolution/scaling changed notification");
                let offset = wm.work_area_offset;
                for monitor in wm.monitors_mut() {
                    let mut should_update = false;

                    // Update sizes and work areas as necessary
                    if let Ok(reference) = WindowsApi::monitor(monitor.id) {
                        if reference.work_area_size != monitor.work_area_size {
                            monitor.work_area_size = Rect {
                                left: reference.work_area_size.left,
                                top: reference.work_area_size.top,
                                right: reference.work_area_size.right,
                                bottom: reference.work_area_size.bottom,
                            };

                            should_update = true;
                        }

                        if reference.size != monitor.size {
                            monitor.size = Rect {
                                left: reference.size.left,
                                top: reference.size.top,
                                right: reference.size.right,
                                bottom: reference.size.bottom,
                            };

                            should_update = true;
                        }
                    }

                    if should_update {
                        tracing::info!(
                            "updated monitor resolution/scaling for {}",
                            monitor.device_id
                        );

                        monitor.update_focused_workspace(offset)?;
                        border_manager::send_notification(None);
                    } else {
                        tracing::debug!(
                            "resolutions match, reconciliation not required for {}",
                            monitor.device_id
                        );
                    }
                }
            }
            // this is handled above if the reconciliator is paused but we should still check if
            // there were any changes to the connected monitors while the system was
            // suspended/locked.
            MonitorNotification::ResumingFromSuspendedState
            | MonitorNotification::SessionUnlocked
            | MonitorNotification::DisplayConnectionChange => {
                tracing::debug!("handling display connection change notification");
                let mut monitor_cache = MONITOR_CACHE.get_or_init(|| Mutex::new(Vec::new())).lock();

                let initial_monitor_count = wm.monitors().len();

                // Get the currently attached display devices
                let attached_devices = attached_display_devices(display_provider)?;

                // Make sure that in our state any attached displays have the latest Win32 data
                for monitor in wm.monitors_mut() {
                    for attached in &attached_devices {
                        if same_display(attached, monitor) {
                            monitor.id = attached.id;
                            monitor.device = attached.device.clone();
                            monitor.device_id = attached.device_id.clone();
                            monitor.serial_number_id = attached.serial_number_id.clone();
                            monitor.name = attached.name.clone();
                            monitor.size = attached.size;
                            monitor.work_area_size = attached.work_area_size;
                        }
                    }
                }

                if monitor_topology_matches(wm.monitors(), &attached_devices) {
                    tracing::debug!("monitor identities match, reconciliation not required");
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

                // Handle potential monitor removal with verification
                let attached_devices = if !disconnected_monitor_indices(
                    wm.monitors(),
                    &attached_devices,
                )
                .is_empty()
                {
                    tracing::info!(
                        "potential monitor removal detected ({initial_monitor_count} vs {}), verifying in 3s",
                        attached_devices.len()
                    );

                    // Release locks before waiting
                    drop(wm);
                    drop(monitor_cache);

                    // Wait 3 seconds for display state to stabilize
                    std::thread::sleep(std::time::Duration::from_secs(3));

                    // Re-query the Win32 display APIs
                    let re_queried_devices = match attached_display_devices(display_provider) {
                        Ok(devices) => devices,
                        Err(e) => {
                            tracing::error!("failed to re-query display devices: {}", e);
                            continue 'receiver;
                        }
                    };

                    tracing::debug!(
                        "after verification: wm had {} monitors, initial query found {}, re-query found {}",
                        initial_monitor_count,
                        attached_devices.len(),
                        re_queried_devices.len()
                    );

                    wm = wm_arc.lock();
                    if re_queried_devices.is_empty() {
                        tracing::debug!("no devices found during verification, preserving state");
                        continue 'receiver;
                    }

                    // If the same monitors are back, the removal was transient.
                    // Still try to restore state since windows might have been minimized.
                    if monitor_topology_matches(wm.monitors(), &re_queried_devices) {
                        tracing::info!(
                            "monitor removal was transient (spurious event), attempting state restoration. Initial: {}, Re-queried: {}",
                            initial_monitor_count,
                            re_queried_devices.len()
                        );

                        // Update Win32 data for all monitors
                        for monitor in wm.monitors_mut() {
                            for attached in &re_queried_devices {
                                if same_display(attached, monitor) {
                                    monitor.id = attached.id;
                                    monitor.device = attached.device.clone();
                                    monitor.device_id = attached.device_id.clone();
                                    monitor.serial_number_id = attached.serial_number_id.clone();
                                    monitor.name = attached.name.clone();
                                    monitor.size = attached.size;
                                    monitor.work_area_size = attached.work_area_size;
                                }
                            }
                        }

                        // Try to restore windows that might have been minimized
                        let offset = wm.work_area_offset;
                        for monitor in wm.monitors_mut() {
                            let focused_workspace_idx = monitor.focused_workspace_idx();

                            for (idx, workspace) in monitor.workspaces_mut().iter_mut().enumerate()
                            {
                                let is_focused_workspace = idx == focused_workspace_idx;

                                if is_focused_workspace {
                                    // Restore containers
                                    for container in workspace.containers_mut() {
                                        if let Some(window) = container.focused_window()
                                            && WindowsApi::is_window(window.hwnd)
                                        {
                                            tracing::debug!(
                                                "restoring window after transient removal: {}",
                                                window.hwnd
                                            );
                                            WindowsApi::restore_window(window.hwnd);
                                        } else if let Some(window) = container.focused_window() {
                                            tracing::debug!(
                                                "skipping restore of invalid window: {}",
                                                window.hwnd
                                            );
                                        }
                                    }

                                    // Restore maximized window
                                    if let Some(window) = &workspace.maximized_window
                                        && WindowsApi::is_window(window.hwnd)
                                    {
                                        WindowsApi::restore_window(window.hwnd);
                                    }

                                    // Restore monocle container
                                    if let Some(container) = &workspace.monocle_container
                                        && let Some(window) = container.focused_window()
                                        && WindowsApi::is_window(window.hwnd)
                                    {
                                        WindowsApi::restore_window(window.hwnd);
                                    }

                                    // Restore floating windows
                                    for window in workspace.floating_windows() {
                                        if WindowsApi::is_window(window.hwnd) {
                                            WindowsApi::restore_window(window.hwnd);
                                        }
                                    }
                                }
                            }

                            monitor.update_focused_workspace(offset)?;
                        }

                        border_manager::send_notification(None);
                        continue 'receiver;
                    }

                    // If monitors are still missing, proceed with actual removal logic
                    tracing::info!(
                        "verified monitor removal ({initial_monitor_count} vs {}), removing disconnected monitors",
                        re_queried_devices.len()
                    );

                    // Re-acquire the cache lock for removal processing.
                    monitor_cache = MONITOR_CACHE.get_or_init(|| Mutex::new(Vec::new())).lock();

                    // Make sure that in our state any attached displays have the latest Win32 data
                    // We must do this again because we dropped the lock and are working with new data
                    for monitor in wm.monitors_mut() {
                        for attached in &re_queried_devices {
                            if same_display(attached, monitor) {
                                monitor.id = attached.id;
                                monitor.device = attached.device.clone();
                                monitor.device_id = attached.device_id.clone();
                                monitor.serial_number_id = attached.serial_number_id.clone();
                                monitor.name = attached.name.clone();
                                monitor.size = attached.size;
                                monitor.work_area_size = attached.work_area_size;
                            }
                        }
                    }

                    // Use re-queried devices for remaining logic
                    re_queried_devices
                } else {
                    attached_devices
                };

                let removed_monitor_indices =
                    disconnected_monitor_indices(wm.monitors(), &attached_devices);
                if !removed_monitor_indices.is_empty() {
                    tracing::info!("removing disconnected monitors");

                    for &m_idx in &removed_monitor_indices {
                        if let Some(m) = wm.monitors().get(m_idx) {
                            let focused_workspace_idx = m.focused_workspace_idx();

                            for (idx, workspace) in m.workspaces().iter().enumerate() {
                                let is_focused_workspace = idx == focused_workspace_idx;
                                let focused_container_idx = workspace.focused_container_idx();
                                for (c_idx, container) in workspace.containers().iter().enumerate()
                                {
                                    let focused_window_idx = container.focused_window_idx();
                                    for (w_idx, window) in container.windows().iter().enumerate() {
                                        if is_focused_workspace
                                            && c_idx == focused_container_idx
                                            && w_idx == focused_window_idx
                                        {
                                            // Minimize the focused window since Windows might try
                                            // to move it to another monitor if it was focused.
                                            if window.is_focused() {
                                                window.minimize();
                                            }
                                        }
                                    }
                                }

                                if let Some(maximized) = &workspace.maximized_window {
                                    // Minimize the focused window since Windows might try
                                    // to move it to another monitor if it was focused.
                                    if maximized.is_focused() {
                                        maximized.minimize();
                                    }
                                }

                                if let Some(container) = &workspace.monocle_container
                                    && let Some(window) = container.focused_window()
                                {
                                    // Minimize the focused window since Windows might try
                                    // to move it to another monitor if it was focused.
                                    if window.is_focused() {
                                        window.minimize();
                                    }
                                }

                                for window in workspace.floating_windows() {
                                    // Minimize the focused window since Windows might try
                                    // to move it to another monitor if it was focused.
                                    if window.is_focused() {
                                        window.minimize();
                                    }
                                }
                            }

                            let configured_id = configured_cache_id(&m.device_id, m);
                            cache_monitor(&mut monitor_cache, configured_id, m.clone());
                        }
                    }

                    wm.remove_monitors(&removed_monitor_indices);

                    let post_removal_monitor_count = wm.monitors().len();

                    let offset = wm.work_area_offset;

                    for monitor in wm.monitors_mut() {
                        // If we have lost a monitor, update everything to filter out any jank
                        if initial_monitor_count != post_removal_monitor_count {
                            monitor.update_focused_workspace(offset)?;
                        }
                    }
                }

                let post_removal_monitor_count = wm.monitors().len();

                let new_monitors = attached_devices
                    .iter()
                    .filter(|attached| !wm.monitors().iter().any(|m| same_display(attached, m)))
                    .collect::<Vec<_>>();

                // Check for and add any new monitors that may have been plugged in
                // Monitor and display index preferences get applied in this function
                WindowsApi::apply_monitor_information(&mut wm, &attached_devices);

                let post_addition_monitor_count = wm.monitors().len();

                if post_addition_monitor_count > post_removal_monitor_count {
                    tracing::info!(
                        "monitor count mismatch ({post_removal_monitor_count} vs {post_addition_monitor_count}), adding connected monitors",
                    );

                    let known_hwnds = wm.known_hwnds.clone();
                    let offset = wm.work_area_offset;

                    // Look in the updated state for new monitors
                    for (i, m) in wm.monitors_mut().iter_mut().enumerate() {
                        if new_monitors
                            .iter()
                            .any(|attached| same_display(attached, m))
                            && let Some(cache_index) = cached_monitor_index(&monitor_cache, m)
                        {
                            let cached = &monitor_cache[cache_index].monitor;
                            tracing::info!(
                                "found monitor and workspace configuration for {} in the monitor cache, applying",
                                m.device_id
                            );

                            // If it does, update the cached monitor info with the new one and
                            // load the cached monitor removing any window that has since been
                            // closed or moved to another workspace
                            *m = Monitor {
                                // Data that should be the one just read from `win32-display-data`
                                id: m.id,
                                name: m.name.clone(),
                                device: m.device.clone(),
                                device_id: m.device_id.clone(),
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
                                workspace_names: cached.workspace_names.clone(),
                                container_padding: cached.container_padding,
                                workspace_padding: cached.workspace_padding,
                                wallpaper: cached.wallpaper.clone(),
                                floating_layer_behaviour: cached.floating_layer_behaviour,
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
                                        window.exe().is_ok()
                                            && !known_hwnds.contains_key(&window.hwnd)
                                    });

                                    if container.windows().is_empty() {
                                        empty_containers.push(idx);
                                    }

                                    if is_focused_workspace {
                                        if let Some(window) = container.focused_window()
                                            && WindowsApi::is_window(window.hwnd)
                                        {
                                            tracing::debug!("restoring window: {}", window.hwnd);
                                            WindowsApi::restore_window(window.hwnd);
                                        } else {
                                            // If the focused window was moved or removed by
                                            // the user after the disconnect then focus the
                                            // first window and show that one
                                            container.focus_window(0);

                                            if let Some(window) = container.focused_window()
                                                && WindowsApi::is_window(window.hwnd)
                                            {
                                                WindowsApi::restore_window(window.hwnd);
                                            }
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

                                if let Some(window) = &workspace.maximized_window {
                                    if window.exe().is_err()
                                        || known_hwnds.contains_key(&window.hwnd)
                                    {
                                        workspace.maximized_window = None;
                                    } else if is_focused_workspace
                                        && WindowsApi::is_window(window.hwnd)
                                    {
                                        WindowsApi::restore_window(window.hwnd);
                                    }
                                }

                                if let Some(container) = &mut workspace.monocle_container {
                                    container.windows_mut().retain(|window| {
                                        window.exe().is_ok()
                                            && !known_hwnds.contains_key(&window.hwnd)
                                    });

                                    if container.windows().is_empty() {
                                        workspace.monocle_container = None;
                                    } else if is_focused_workspace {
                                        if let Some(window) = container.focused_window()
                                            && WindowsApi::is_window(window.hwnd)
                                        {
                                            WindowsApi::restore_window(window.hwnd);
                                        } else {
                                            // If the focused window was moved or removed by
                                            // the user after the disconnect then focus the
                                            // first window and show that one
                                            container.focus_window(0);

                                            if let Some(window) = container.focused_window()
                                                && WindowsApi::is_window(window.hwnd)
                                            {
                                                WindowsApi::restore_window(window.hwnd);
                                            }
                                        }
                                    }
                                }

                                workspace.floating_windows_mut().retain(|window| {
                                    window.exe().is_ok() && !known_hwnds.contains_key(&window.hwnd)
                                });

                                if is_focused_workspace {
                                    for window in workspace.floating_windows() {
                                        if WindowsApi::is_window(window.hwnd) {
                                            WindowsApi::restore_window(window.hwnd);
                                        }
                                    }
                                }

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
                            m.load_focused_workspace_without_focus()?;
                            m.update_focused_workspace(offset)?;

                            // Consume exactly the selected entry, only after restoration succeeds.
                            monitor_cache.remove(cache_index);
                        }
                    }
                }

                let final_count = wm.monitors().len();

                if post_removal_monitor_count != final_count {
                    wm.retile_all(true)?;
                    // Second retile to fix DPI/resolution related jank
                    wm.retile_all(true)?;
                    // Border updates to fix DPI/resolution related jank
                    border_manager::send_notification(None);
                }

                wm.update_known_hwnds();
            }
        }

        notify_subscribers(
            Notification {
                event: NotificationEvent::Monitor(notification),
                state: wm.as_ref().into(),
            },
            initial_state.has_been_modified(&wm),
        )?;

        // Foreground activation is best effort, after restoration and bookkeeping.
        if matches!(
            notification,
            MonitorNotification::DisplayConnectionChange
                | MonitorNotification::SessionUnlocked
                | MonitorNotification::ResumingFromSuspendedState
        ) && let Some(window) = wm
            .focused_monitor()
            .and_then(|monitor| monitor.focused_workspace())
            .and_then(|workspace| workspace.focus_target())
            && let Err(error) = window.focus(wm.mouse_follows_focus)
        {
            tracing::warn!("could not focus restored window {}: {error}", window.hwnd);
        }
    }

    Ok(())
}

/// The device id of a display the OS reports no device path for
const UNKNOWN_DEVICE_ID: &str = "UNKNOWN";

/// Whether `attached`, a display the OS reports, is the same display as `monitor` in our state
///
/// A missing or empty serial number says nothing about a display, since many have none (virtual
/// displays among them), so two of them are not the same display for lacking one. The same goes for
/// the device id of a display without a device path, which is shared by every such display; only
/// the name of those tells them apart.
fn same_display(attached: &Monitor, monitor: &Monitor) -> bool {
    if let (Some(attached_snid), Some(monitor_snid)) =
        (&attached.serial_number_id, &monitor.serial_number_id)
        && !attached_snid.is_empty()
        && attached_snid.eq(monitor_snid)
    {
        return true;
    }

    same_device(
        (&attached.device_id, &attached.name),
        (&monitor.device_id, &monitor.name),
    )
}

/// Whether two displays, each given as its device id and name, are the same device
pub fn same_device((device_id, name): (&str, &str), (other_id, other_name): (&str, &str)) -> bool {
    device_id.eq(other_id) && (device_id != UNKNOWN_DEVICE_ID || name.eq(other_name))
}

/// The indices of the monitors in our state which none of the attached displays is
fn disconnected_monitor_indices<'a>(
    monitors: impl IntoIterator<Item = &'a Monitor>,
    attached: &[Monitor],
) -> Vec<usize> {
    monitors
        .into_iter()
        .enumerate()
        .filter(|(_, m)| !attached.iter().any(|a| same_display(a, m)))
        .map(|(idx, _)| idx)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::window_manager_event::WindowManagerEvent;
    use crossbeam_channel::Sender;
    use crossbeam_channel::bounded;
    use uuid::Uuid;
    use windows::Win32::Devices::Display::DISPLAYCONFIG_VIDEO_OUTPUT_TECHNOLOGY;
    // NOTE: Using RECT instead of RECT since I get a mismatched type error. Can be updated if
    // needed.
    use windows::Win32::Foundation::RECT;

    #[test]
    fn test_focus_refusal_finishes_cached_restoration() -> color_eyre::Result<()> {
        match std::thread::spawn(check_focus_refusal_finishes_cached_restoration).join() {
            Ok(result) => result,
            Err(panic) => std::panic::resume_unwind(panic),
        }
    }

    fn check_focus_refusal_finishes_cached_restoration() -> color_eyre::Result<()> {
        use crate::Window;
        use windows::Win32::Foundation::HWND;
        use windows::Win32::System::StationsAndDesktops::CloseDesktop;
        use windows::Win32::System::StationsAndDesktops::CreateDesktopW;
        use windows::Win32::System::StationsAndDesktops::DESKTOP_CREATEWINDOW;
        use windows::Win32::System::StationsAndDesktops::DESKTOP_READOBJECTS;
        use windows::Win32::System::StationsAndDesktops::DESKTOP_WRITEOBJECTS;
        use windows::Win32::System::StationsAndDesktops::GetThreadDesktop;
        use windows::Win32::System::StationsAndDesktops::HDESK;
        use windows::Win32::System::StationsAndDesktops::SetThreadDesktop;
        use windows::Win32::System::Threading::GetCurrentThreadId;
        use windows::Win32::UI::WindowsAndMessaging::DestroyWindow;
        use windows::core::HSTRING;
        use windows::core::w;

        let _context = TestContext::new();

        struct TestDesktop {
            original: HDESK,
            desktop: HDESK,
            windows: Vec<Window>,
        }

        impl Drop for TestDesktop {
            fn drop(&mut self) {
                for window in &self.windows {
                    crate::HIDDEN_HWNDS
                        .lock()
                        .retain(|hwnd| *hwnd != window.hwnd);
                    if let Err(error) = unsafe { DestroyWindow(HWND(window.hwnd as _)) } {
                        tracing::error!("failed to destroy test window: {error}");
                    }
                }
                unsafe {
                    if let Err(error) = SetThreadDesktop(self.original) {
                        tracing::error!("failed to restore test thread desktop: {error}");
                    }
                    if let Err(error) = CloseDesktop(self.desktop) {
                        tracing::error!("failed to close test desktop: {error}");
                    }
                }
            }
        }

        // Windows on a non-input desktop cannot take the foreground. Never switch
        // the input desktop; only this isolated test thread uses the new desktop.
        let original_desktop = unsafe { GetThreadDesktop(GetCurrentThreadId())? };
        let desktop_name = HSTRING::from(format!("komorebi-test-{}", Uuid::new_v4()));
        let desktop = unsafe {
            CreateDesktopW(
                &desktop_name,
                None,
                None,
                Default::default(),
                DESKTOP_CREATEWINDOW.0 | DESKTOP_READOBJECTS.0 | DESKTOP_WRITEOBJECTS.0,
                None,
            )?
        };
        let mut desktop = TestDesktop {
            original: original_desktop,
            desktop,
            windows: Vec::new(),
        };
        unsafe { SetThreadDesktop(desktop.desktop)? };

        let mut wm = create_window_manager(&_context);
        let devices: Vec<_> = (1..=2)
            .map(|id| MockDevice {
                hmonitor: id,
                device_path: format!(r"\\?\DISPLAY#TEST{id}#instance#{{test}}"),
                device_name: format!(r"\\.\DISPLAY{id}"),
                device_description: String::new(),
                serial_number_id: None,
                size: RECT::default(),
                work_area_size: RECT::default(),
                device_key: String::new(),
                output_technology: None,
            })
            .collect();
        let provider = || devices.iter().cloned().map(|d| Ok(d.into()));
        let monitors = attached_display_devices(provider)?;
        for mut monitor in monitors {
            // Use a real HWND; foreground refusal is not an invalid-handle error.
            let window = Window::from(WindowsApi::create_hidden_window(w!("STATIC"), 0)?);
            desktop.windows.push(window);
            assert!(window.is_window());
            assert!(window.focus(false).is_err());
            WindowsApi::hide_window(window.hwnd);
            assert!(!window.is_visible());
            monitor.workspaces_mut()[0]
                .floating_windows_mut()
                .push_back(window);
            insert_in_monitor_cache(&monitor.device_id, monitor.clone());
        }
        assert_eq!(
            MONITOR_CACHE
                .get_or_init(|| Mutex::new(Vec::new()))
                .lock()
                .len(),
            2
        );

        wm.monitors_mut().clear();
        assert!(wm.known_hwnds.is_empty());
        let wm = Arc::new(Mutex::new(wm));
        let (sender, receiver) = bounded(1);
        sender.send(MonitorNotification::DisplayConnectionChange)?;
        drop(sender);
        handle_notifications_with_receiver(Arc::clone(&wm), provider, receiver)?;

        assert!(
            MONITOR_CACHE
                .get_or_init(|| Mutex::new(Vec::new()))
                .lock()
                .is_empty()
        );
        let wm = wm.lock();
        assert_eq!(wm.monitors().len(), 2);
        assert_eq!(wm.known_hwnds.len(), 2);
        for window in &desktop.windows {
            assert!(window.is_visible());
            assert!(wm.known_hwnds.contains_key(&window.hwnd));
        }
        Ok(())
    }

    // Creating a Mock Display Provider
    #[derive(Clone)]
    struct MockDevice {
        hmonitor: isize,
        device_path: String,
        device_name: String,
        device_description: String,
        serial_number_id: Option<String>,
        size: RECT,
        work_area_size: RECT,
        device_key: String,
        output_technology: Option<DISPLAYCONFIG_VIDEO_OUTPUT_TECHNOLOGY>,
    }

    impl From<MockDevice> for win32_display_data::Device {
        fn from(mock: MockDevice) -> Self {
            win32_display_data::Device {
                hmonitor: mock.hmonitor,
                device_path: mock.device_path,
                device_name: mock.device_name,
                device_description: mock.device_description,
                serial_number_id: mock.serial_number_id,
                size: mock.size,
                work_area_size: mock.work_area_size,
                device_key: mock.device_key,
                output_technology: mock.output_technology,
            }
        }
    }

    struct TestContext {
        _shared: crate::test_support::TestContext,
        monitor_cache: Vec<CachedMonitor>,
    }

    impl TestContext {
        fn new() -> Self {
            let shared = crate::test_support::TestContext::new();
            let monitor_cache =
                std::mem::take(&mut *MONITOR_CACHE.get_or_init(|| Mutex::new(Vec::new())).lock());
            Self {
                _shared: shared,
                monitor_cache,
            }
        }
    }

    impl Drop for TestContext {
        fn drop(&mut self) {
            *MONITOR_CACHE.get_or_init(|| Mutex::new(Vec::new())).lock() =
                std::mem::take(&mut self.monitor_cache);
        }
    }

    fn setup_window_manager() -> (TestContext, WindowManager) {
        let context = TestContext::new();
        let wm = create_window_manager(&context);
        (context, wm)
    }

    fn create_window_manager(context: &TestContext) -> WindowManager {
        let (_sender, receiver): (Sender<WindowManagerEvent>, Receiver<WindowManagerEvent>) =
            bounded(1);
        match WindowManager::new(receiver, Some(context._shared.socket_path())) {
            Ok(wm) => wm,
            Err(error) => panic!("failed to create test manager: {error}"),
        }
    }

    fn identity_monitor(device_id: &str, serial_number_id: Option<&str>) -> Monitor {
        monitor::new(
            0,
            Rect::default(),
            Rect::default(),
            String::new(),
            String::new(),
            device_id.to_string(),
            serial_number_id.map(str::to_string),
        )
    }

    #[test]
    fn test_same_count_reconciliation_preserves_display_identity() -> color_eyre::Result<()> {
        for (case, old, attached, same) in [
            (
                "empty serial replacement",
                mock_display(Some("old"), "DISPLAY1", Some("")),
                mock_display(Some("new"), "DISPLAY2", Some("")),
                false,
            ),
            (
                "distinct UNKNOWN replacement",
                mock_display(None, "DISPLAY1", None),
                mock_display(None, "DISPLAY2", None),
                false,
            ),
            (
                "unchanged display",
                mock_display(Some("console"), "DISPLAY1", None),
                mock_display(Some("console"), "DISPLAY1", None),
                true,
            ),
            (
                "serial moves to another port",
                mock_display(Some("old-port"), "DISPLAY1", Some("serial")),
                mock_display(Some("new-port"), "DISPLAY2", Some("serial")),
                true,
            ),
        ] {
            let (_context, mut wm) = setup_window_manager();
            let mut old = mock_monitor(&old)?;
            let expected = mock_monitor(&attached)?;
            assert_eq!(same_display(&old, &expected), same, "{case}");
            old.workspaces_mut()[0].name = Some("original workspace".to_string());
            assert!(wm.monitors().is_empty());
            wm.monitors_mut().push_back(old);
            assert_eq!(wm.monitors().len(), 1);
            assert_eq!(
                wm.monitors()[0].workspaces()[0].name.as_deref(),
                Some("original workspace")
            );
            let wm = Arc::new(Mutex::new(wm));

            reconcile_displays(&wm, &[attached])?;

            let wm = wm.lock();
            assert_eq!(wm.monitors().len(), 1, "{case}");
            assert!(same_display(&expected, &wm.monitors()[0]), "{case}");
            assert_eq!(
                wm.monitors()[0].workspaces()[0].name.as_deref(),
                same.then_some("original workspace"),
                "{case}"
            );
        }
        Ok(())
    }

    fn mock_display(device: Option<&str>, name: &str, serial: Option<&str>) -> MockDevice {
        MockDevice {
            hmonitor: 0,
            device_path: device
                .map(|id| format!(r"\\?\DISPLAY#{id}#instance#{{test}}"))
                .unwrap_or_default(),
            device_name: name.to_string(),
            device_description: String::new(),
            serial_number_id: serial.map(str::to_string),
            size: RECT::default(),
            work_area_size: RECT::default(),
            device_key: String::new(),
            output_technology: None,
        }
    }

    fn mock_monitor(display: &MockDevice) -> color_eyre::Result<Monitor> {
        Ok(attached_display_devices(|| std::iter::once(Ok(display.clone().into())))?.remove(0))
    }

    fn reconcile_displays(
        wm: &Arc<Mutex<WindowManager>>,
        devices: &[MockDevice],
    ) -> color_eyre::Result<()> {
        let (sender, receiver) = bounded(1);
        sender.send(MonitorNotification::DisplayConnectionChange)?;
        drop(sender);
        handle_notifications_with_receiver(
            Arc::clone(wm),
            || devices.iter().cloned().map(|d| Ok(d.into())),
            receiver,
        )
    }

    #[test]
    fn test_cached_unknown_displays_restore_independently() -> color_eyre::Result<()> {
        let (_context, mut wm) = setup_window_manager();
        assert!(wm.monitors().is_empty());
        assert!(WORKSPACE_MATCHING_RULES.lock().is_empty());
        assert!(DISPLAY_INDEX_PREFERENCES.read().is_empty());
        let anchor = mock_display(Some("anchor"), "anchor", None);
        let first = mock_display(None, "UNKNOWN-first", None);
        let second = mock_display(None, "UNKNOWN-second", None);
        let first_monitor = mock_monitor(&first)?;
        let second_monitor = mock_monitor(&second)?;
        assert_eq!(first_monitor.device_id, UNKNOWN_DEVICE_ID);
        assert_eq!(second_monitor.device_id, UNKNOWN_DEVICE_ID);
        assert!(!same_display(&first_monitor, &second_monitor));
        wm.monitors_mut().push_back(mock_monitor(&anchor)?);
        for (mut monitor, marker) in [(first_monitor, "first"), (second_monitor, "second")] {
            monitor.workspaces_mut()[0].name = Some(marker.to_string());
            wm.monitors_mut().push_back(monitor);
        }
        let cache = MONITOR_CACHE.get_or_init(|| Mutex::new(Vec::new()));
        assert!(cache.lock().is_empty());
        assert_eq!(wm.monitors().len(), 3);
        let wm = Arc::new(Mutex::new(wm));

        // Exercise removal insertion, not just public config insertion.
        reconcile_displays(&wm, std::slice::from_ref(&anchor))?;
        assert_eq!(wm.lock().monitors().len(), 1);
        assert_eq!(cache.lock().len(), 2);

        reconcile_displays(&wm, &[anchor.clone(), second.clone()])?;
        assert_eq!(wm.lock().monitors().len(), 2);
        assert_eq!(
            wm.lock().monitors()[1].workspaces()[0].name.as_deref(),
            Some("second")
        );
        {
            let cache = cache.lock();
            assert_eq!(cache.len(), 1);
            assert_eq!(
                cache[0].monitor.workspaces()[0].name.as_deref(),
                Some("first")
            );
        }

        // A new UNKNOWN must be recognized alongside an already connected UNKNOWN.
        let devices = [anchor, second, first];
        reconcile_displays(&wm, &devices)?;
        {
            let wm = wm.lock();
            assert_eq!(wm.monitors().len(), 3);
            assert_eq!(
                wm.monitors()[1].workspaces()[0].name.as_deref(),
                Some("second")
            );
            assert_eq!(
                wm.monitors()[2].workspaces()[0].name.as_deref(),
                Some("first")
            );
        }
        assert!(cache.lock().is_empty());

        // Re-enumerating from empty manager state cannot restore consumed entries again.
        wm.lock().monitors_mut().clear();
        reconcile_displays(&wm, &devices)?;
        assert!(
            wm.lock()
                .monitors()
                .iter()
                .all(|m| m.workspaces()[0].name.is_none())
        );
        assert!(cache.lock().is_empty());
        Ok(())
    }

    #[test]
    fn test_session_cache_serial_priority_on_restore() -> color_eyre::Result<()> {
        for reverse in [false, true] {
            for returning_first in [false, true] {
                let (_context, wm) = setup_window_manager();
                assert!(wm.monitors().is_empty());
                assert!(DISPLAY_INDEX_PREFERENCES.read().is_empty());
                assert!(WORKSPACE_MATCHING_RULES.lock().is_empty());
                let original = [
                    mock_display(Some("priority-port-a"), "A", Some("priority-serial-a")),
                    mock_display(Some("priority-port-b"), "B", Some("priority-serial-b")),
                ];
                let moved = [
                    mock_display(Some("priority-port-b"), "A", Some("priority-serial-a")),
                    mock_display(Some("priority-port-a"), "B", Some("priority-serial-b")),
                ];
                let monitors = [mock_monitor(&original[0])?, mock_monitor(&original[1])?];
                assert!(!same_display(&monitors[0], &monitors[1]));
                let target = usize::from(!returning_first);
                let competitor = 1 - target;
                let returning = mock_monitor(&moved[target])?;
                assert!(
                    !returning
                        .serial_number_id
                        .as_deref()
                        .unwrap_or_default()
                        .is_empty()
                );
                assert_eq!(
                    returning.serial_number_id,
                    monitors[target].serial_number_id
                );
                assert_ne!(
                    returning.serial_number_id,
                    monitors[competitor].serial_number_id
                );
                assert!(same_device(
                    (&returning.device_id, &returning.name),
                    (&monitors[competitor].device_id, &monitors[competitor].name),
                ));
                assert!(same_display(&returning, &monitors[target]));
                assert!(same_display(&returning, &monitors[competitor]));
                let cache = MONITOR_CACHE.get_or_init(|| Mutex::new(Vec::new()));
                assert!(cache.lock().is_empty());
                for index in if reverse { [1, 0] } else { [0, 1] } {
                    let mut monitor = monitors[index].clone();
                    monitor.workspaces_mut()[0].name = Some(original[index].device_name.clone());
                    insert_in_monitor_cache(
                        monitor.serial_number_id.as_deref().unwrap_or_default(),
                        monitor.clone(),
                    );
                }
                assert_eq!(cache.lock().len(), 2);
                assert!(
                    cache
                        .lock()
                        .iter()
                        .all(|entry| entry.configured_id.is_none())
                );
                let wm = Arc::new(Mutex::new(wm));

                reconcile_displays(&wm, std::slice::from_ref(&moved[target]))?;

                assert_eq!(wm.lock().monitors().len(), 1);
                assert_eq!(
                    wm.lock().monitors()[0].workspaces()[0].name.as_deref(),
                    Some(original[target].device_name.as_str()),
                    "reverse={reverse}, returning_first={returning_first}"
                );
                {
                    let cache = cache.lock();
                    assert_eq!(cache.len(), 1);
                    assert_eq!(
                        cache[0].monitor.serial_number_id,
                        monitors[competitor].serial_number_id
                    );
                    assert_eq!(
                        cache[0].monitor.workspaces()[0].name.as_deref(),
                        Some(original[competitor].device_name.as_str())
                    );
                }

                reconcile_displays(&wm, &[moved[target].clone(), moved[competitor].clone()])?;
                assert_eq!(wm.lock().monitors().len(), 2);
                assert_eq!(
                    wm.lock().monitors()[1].workspaces()[0].name.as_deref(),
                    Some(original[competitor].device_name.as_str())
                );
                assert!(cache.lock().is_empty());
            }
        }
        Ok(())
    }

    #[test]
    fn test_session_cache_serial_priority_on_replacement() {
        for reverse in [false, true] {
            for returning_first in [false, true] {
                let _context = TestContext::new();
                assert!(DISPLAY_INDEX_PREFERENCES.read().is_empty());
                assert!(WORKSPACE_MATCHING_RULES.lock().is_empty());
                let mut monitors = [
                    identity_monitor("priority-port-a", Some("priority-serial-a")),
                    identity_monitor("priority-port-b", Some("priority-serial-b")),
                ];
                for (index, monitor) in monitors.iter_mut().enumerate() {
                    monitor.workspaces_mut()[0].name = Some(format!("original-{index}"));
                }
                assert!(!same_display(&monitors[0], &monitors[1]));
                let target = usize::from(!returning_first);
                let competitor = 1 - target;
                let mut returning = monitors[target].clone();
                returning.device_id = monitors[competitor].device_id.clone();
                returning.workspaces_mut()[0].name = Some("refreshed".to_string());
                assert!(
                    !returning
                        .serial_number_id
                        .as_deref()
                        .unwrap_or_default()
                        .is_empty()
                );
                assert_eq!(
                    returning.serial_number_id,
                    monitors[target].serial_number_id
                );
                assert_ne!(
                    returning.serial_number_id,
                    monitors[competitor].serial_number_id
                );
                assert!(same_display(&returning, &monitors[target]));
                assert!(same_display(&returning, &monitors[competitor]));
                let cache = MONITOR_CACHE.get_or_init(|| Mutex::new(Vec::new()));
                assert!(cache.lock().is_empty());
                for index in if reverse { [1, 0] } else { [0, 1] } {
                    insert_in_monitor_cache(
                        monitors[index]
                            .serial_number_id
                            .as_deref()
                            .unwrap_or_default(),
                        monitors[index].clone(),
                    );
                }
                assert_eq!(cache.lock().len(), 2);
                assert!(
                    cache
                        .lock()
                        .iter()
                        .all(|entry| entry.configured_id.is_none())
                );

                insert_in_monitor_cache(
                    returning.serial_number_id.as_deref().unwrap_or_default(),
                    returning.clone(),
                );

                let cache = cache.lock();
                assert_eq!(cache.len(), 2);
                for (expected, marker) in [
                    (&returning, "refreshed"),
                    (
                        &monitors[competitor],
                        monitors[competitor].workspaces()[0]
                            .name
                            .as_deref()
                            .unwrap_or_default(),
                    ),
                ] {
                    let entry = cache
                        .iter()
                        .find(|entry| entry.monitor.serial_number_id == expected.serial_number_id);
                    let Some(entry) = entry else {
                        panic!("lost serial: reverse={reverse}, returning_first={returning_first}");
                    };
                    assert_eq!(entry.monitor.device_id, expected.device_id);
                    assert_eq!(entry.monitor.workspaces()[0].name.as_deref(), Some(marker));
                }
            }
        }
    }

    #[test]
    fn test_session_cache_uses_display_identity() -> color_eyre::Result<()> {
        let original = mock_display(Some("session-port"), "original", Some("session-serial"));
        let empty_serial = mock_display(Some("empty-port"), "empty", Some(""));
        let unknown = mock_display(None, "unknown-original", None);
        for (case, old, connected, matches) in [
            ("own reconnect", original.clone(), original.clone(), true),
            (
                "changed serial",
                original.clone(),
                mock_display(Some("session-port"), "original", Some("new-session-serial")),
                true,
            ),
            (
                "changed name without serial",
                original.clone(),
                mock_display(Some("session-port"), "renamed", None),
                true,
            ),
            (
                "serial moved ports",
                original,
                mock_display(Some("other-port"), "moved", Some("session-serial")),
                true,
            ),
            (
                "own empty serial reconnect",
                empty_serial.clone(),
                empty_serial.clone(),
                true,
            ),
            (
                "empty serial is not identity",
                empty_serial,
                mock_display(Some("different-port"), "empty", Some("")),
                false,
            ),
            (
                "own UNKNOWN reconnect",
                unknown.clone(),
                unknown.clone(),
                true,
            ),
            (
                "distinct UNKNOWN name",
                unknown,
                mock_display(None, "unknown-other", None),
                false,
            ),
        ] {
            let (_context, wm) = setup_window_manager();
            assert!(wm.monitors().is_empty());
            assert!(WORKSPACE_MATCHING_RULES.lock().is_empty());
            assert!(DISPLAY_INDEX_PREFERENCES.read().is_empty());
            let mut cached = mock_monitor(&old)?;
            let connected_monitor = mock_monitor(&connected)?;
            assert_eq!(same_display(&cached, &connected_monitor), matches, "{case}");
            cached.workspaces_mut()[0].name = Some(case.to_string());
            let cache = MONITOR_CACHE.get_or_init(|| Mutex::new(Vec::new()));
            assert!(cache.lock().is_empty());
            insert_in_monitor_cache(&cached.device_id, cached.clone());
            assert_eq!(cache.lock().len(), 1);
            assert!(cache.lock()[0].configured_id.is_none());
            let wm = Arc::new(Mutex::new(wm));

            reconcile_displays(&wm, &[connected])?;

            let wm = wm.lock();
            assert_eq!(wm.monitors().len(), 1, "{case}");
            assert_eq!(
                wm.monitors()[0].workspaces()[0].name.as_deref(),
                matches.then_some(case),
                "{case}"
            );
            assert_eq!(cache.lock().len(), usize::from(!matches), "{case}");
        }
        Ok(())
    }

    #[test]
    fn test_configured_cache_preserves_identity_binding() -> color_eyre::Result<()> {
        for (case, preference, connected, matches) in [
            (
                "device follows port despite new serial",
                "configured-port-instance",
                mock_display(
                    Some("configured-port"),
                    "renamed",
                    Some("replacement-serial"),
                ),
                true,
            ),
            (
                "device does not follow serial to another port",
                "configured-port-instance",
                mock_display(Some("another-port"), "original", Some("configured-serial")),
                false,
            ),
            (
                "serial follows physical display",
                "configured-serial",
                mock_display(Some("another-port"), "renamed", Some("configured-serial")),
                true,
            ),
            (
                "serial does not follow port to another serial",
                "configured-serial",
                mock_display(
                    Some("configured-port"),
                    "original",
                    Some("replacement-serial"),
                ),
                false,
            ),
            (
                "serial does not match absent serial",
                "configured-serial",
                mock_display(Some("configured-port"), "original", None),
                false,
            ),
            (
                "explicit UNKNOWN retains legacy mapping",
                UNKNOWN_DEVICE_ID,
                mock_display(None, "unknown-other", None),
                true,
            ),
        ] {
            let (_context, wm) = setup_window_manager();
            assert!(wm.monitors().is_empty());
            assert!(WORKSPACE_MATCHING_RULES.lock().is_empty());
            assert!(DISPLAY_INDEX_PREFERENCES.read().is_empty());
            DISPLAY_INDEX_PREFERENCES
                .write()
                .insert(0, preference.to_string());
            let original = if preference == UNKNOWN_DEVICE_ID {
                mock_display(None, "unknown-original", None)
            } else {
                mock_display(
                    Some("configured-port"),
                    "original",
                    Some("configured-serial"),
                )
            };
            let mut cached = mock_monitor(&original)?;
            cached.workspaces_mut()[0].name = Some(case.to_string());
            let cache = MONITOR_CACHE.get_or_init(|| Mutex::new(Vec::new()));
            assert!(cache.lock().is_empty());
            // Static config supplies serial even when the explicit preference binds the device.
            insert_in_monitor_cache(
                cached
                    .serial_number_id
                    .as_deref()
                    .unwrap_or(&cached.device_id),
                cached.clone(),
            );
            assert_eq!(cache.lock().len(), 1);
            assert_eq!(cache.lock()[0].configured_id.as_deref(), Some(preference));
            let wm = Arc::new(Mutex::new(wm));

            reconcile_displays(&wm, &[connected])?;

            let wm = wm.lock();
            assert_eq!(wm.monitors().len(), 1, "{case}");
            assert_eq!(
                wm.monitors()[0].workspaces()[0].name.as_deref(),
                matches.then_some(case),
                "{case}"
            );
            assert_eq!(cache.lock().len(), usize::from(!matches), "{case}");
        }
        Ok(())
    }

    #[test]
    fn test_configured_future_monitor_and_device_lookup_priority() -> color_eyre::Result<()> {
        let (_context, wm) = setup_window_manager();
        assert!(wm.monitors().is_empty());
        assert!(WORKSPACE_MATCHING_RULES.lock().is_empty());
        assert!(DISPLAY_INDEX_PREFERENCES.read().is_empty());
        let device = mock_display(Some("future-port"), "future", Some("future-serial"));
        let connected = mock_monitor(&device)?;
        DISPLAY_INDEX_PREFERENCES.write().extend([
            (0, connected.device_id.clone()),
            (1, "future-serial".to_string()),
        ]);
        let cache = MONITOR_CACHE.get_or_init(|| Mutex::new(Vec::new()));
        assert!(cache.lock().is_empty());
        for (id, marker) in [
            ("future-serial", "serial-bound"),
            (connected.device_id.as_str(), "device-bound"),
        ] {
            let mut placeholder = identity_monitor("", None);
            assert!(placeholder.device_id.is_empty());
            assert!(placeholder.name.is_empty());
            assert!(placeholder.serial_number_id.is_none());
            placeholder.workspaces_mut()[0].name = Some(marker.to_string());
            insert_in_monitor_cache(id, placeholder);
        }
        assert_eq!(cache.lock().len(), 2);
        let wm = Arc::new(Mutex::new(wm));

        reconcile_displays(&wm, &[device])?;

        assert_eq!(wm.lock().monitors().len(), 1);
        assert_eq!(
            wm.lock().monitors()[0].workspaces()[0].name.as_deref(),
            Some("device-bound")
        );
        {
            let cache = cache.lock();
            assert_eq!(cache.len(), 1);
            assert_eq!(cache[0].configured_id.as_deref(), Some("future-serial"));
        }

        // The unconsumed serial placeholder must still work when its display moves ports.
        wm.lock().monitors_mut().clear();
        let moved = mock_display(Some("future-moved"), "moved", Some("future-serial"));
        reconcile_displays(&wm, &[moved])?;
        assert_eq!(wm.lock().monitors().len(), 1);
        assert_eq!(
            wm.lock().monitors()[0].workspaces()[0].name.as_deref(),
            Some("serial-bound")
        );
        assert!(cache.lock().is_empty());
        Ok(())
    }

    #[test]
    fn test_send_notification() {
        // Create a monitor notification
        let notification = MonitorNotification::ResolutionScalingChanged;

        // Use the send_notification function to send the notification
        send_notification(notification);

        // Receive the notification from the channel
        let received = event_rx().try_recv();

        // Check if we received the notification and if it matches what we sent
        match received {
            Ok(notification) => {
                assert_eq!(notification, MonitorNotification::ResolutionScalingChanged);
            }
            Err(e) => panic!("Failed to receive MonitorNotification: {e}"),
        }
    }

    #[test]
    fn test_channel_bounded_capacity() {
        let (_, receiver) = channel();

        // Fill the channel to its capacity (20 messages)
        for _ in 0..20 {
            send_notification(MonitorNotification::WorkAreaChanged);
        }

        // Attempt to send another message (should be dropped)
        send_notification(MonitorNotification::ResolutionScalingChanged);

        // Verify the channel contains only the first 20 messages
        for _ in 0..20 {
            let notification = match receiver.try_recv() {
                Ok(notification) => notification,
                Err(e) => panic!("Failed to receive MonitorNotification: {e}"),
            };
            assert_eq!(
                notification,
                MonitorNotification::WorkAreaChanged,
                "Unexpected notification in the channel"
            );
        }

        // Verify that no additional messages are in the channel
        assert!(
            receiver.try_recv().is_err(),
            "Channel should be empty after consuming all messages"
        );
    }

    #[test]
    fn test_insert_in_monitor_cache() {
        let _context = TestContext::new();
        let m = monitor::new(
            0,
            Rect::default(),
            Rect::default(),
            "Test Monitor".to_string(),
            "Test Device".to_string(),
            "Test Device ID".to_string(),
            Some("TestMonitorID".to_string()),
        );

        // Insert the monitor into the cache
        insert_in_monitor_cache("TestMonitorID", m.clone());

        // Retrieve the monitor from the cache
        let cache = MONITOR_CACHE.get_or_init(|| Mutex::new(Vec::new())).lock();
        assert_eq!(cache.len(), 1);
        let retrieved_monitor = cached_monitor_index(&cache, &m).map(|i| &cache[i].monitor);

        // Check that the monitor was inserted correctly and matches the expected value
        assert_eq!(retrieved_monitor, Some(&m));
    }

    #[test]
    fn test_insert_two_monitors_cache() {
        let _context = TestContext::new();
        let m1 = monitor::new(
            0,
            Rect::default(),
            Rect::default(),
            "Test Monitor".to_string(),
            "Test Device".to_string(),
            "Test Device ID".to_string(),
            Some("TestMonitorID".to_string()),
        );

        let m2 = monitor::new(
            0,
            Rect::default(),
            Rect::default(),
            "Test Monitor 2".to_string(),
            "Test Device 2".to_string(),
            "Test Device ID 2".to_string(),
            Some("TestMonitorID2".to_string()),
        );

        // Insert the first monitor into the cache
        insert_in_monitor_cache("TestMonitorID", m1.clone());

        // Insert the second monitor into the cache
        insert_in_monitor_cache("TestMonitorID2", m2.clone());

        // Retrieve the cache to check if the first and second monitors are present
        let cache = MONITOR_CACHE.get_or_init(|| Mutex::new(Vec::new())).lock();

        // Check if Monitor 1 was found in the cache
        assert_eq!(
            cached_monitor_index(&cache, &m1).map(|i| &cache[i].monitor),
            Some(&m1),
            "Monitor cache should contain monitor 1"
        );

        // Check if Monitor 2 was found in the cache
        assert_eq!(
            cached_monitor_index(&cache, &m2).map(|i| &cache[i].monitor),
            Some(&m2),
            "Monitor cache should contain monitor 2"
        );
    }

    #[test]
    fn test_attached_display_devices() {
        // Define mock display data
        let mock_monitor = MockDevice {
            hmonitor: 1,
            device_path: String::from(
                "\\\\?\\DISPLAY#ABC123#4&123456&0&UID0#{saucepackets-4321-5678-2468-abc123456789}",
            ),
            device_name: String::from("\\\\.\\DISPLAY1"),
            device_description: String::from("Display description"),
            serial_number_id: Some(String::from("SaucePackets123")),
            device_key: String::from("Mock Key"),
            size: RECT {
                left: 0,
                top: 0,
                right: 1920,
                bottom: 1080,
            },
            work_area_size: RECT {
                left: 0,
                top: 0,
                right: 1920,
                bottom: 1080,
            },
            output_technology: Some(DISPLAYCONFIG_VIDEO_OUTPUT_TECHNOLOGY(0)),
        };

        // Create a closure to simulate the display provider
        let display_provider = || {
            vec![Ok::<win32_display_data::Device, win32_display_data::Error>(
                win32_display_data::Device::from(mock_monitor.clone()),
            )]
            .into_iter()
        };

        // Should contain the mock monitor
        let result = attached_display_devices(display_provider).ok();
        if let Some(monitors) = result {
            // Check Number of monitors
            assert_eq!(monitors.len(), 1, "Expected one monitor");

            // hmonitor
            assert_eq!(monitors[0].id, 1);

            // device name
            assert_eq!(monitors[0].name, String::from("DISPLAY1"));

            // Device
            assert_eq!(monitors[0].device, String::from("ABC123"));

            // Device ID
            assert_eq!(
                monitors[0].device_id,
                String::from("ABC123-4&123456&0&UID0")
            );

            // Check monitor serial number id
            assert_eq!(
                monitors[0].serial_number_id,
                Some(String::from("SaucePackets123")),
            );
        } else {
            panic!("No monitors found");
        }
    }

    fn display(id: isize, name: &str, device_id: &str, serial: Option<&str>) -> Monitor {
        monitor::new(
            id,
            Rect::default(),
            Rect::default(),
            name.to_string(),
            device_id.split('-').next().unwrap_or_default().to_string(),
            device_id.to_string(),
            serial.map(String::from),
        )
    }

    const PRIMARY: &str = "Default_Monitor-4&183790ca&0&UID0";

    // the displays of the virtual machine in which the unplug was recorded: neither has a serial
    // number, and the second one has no device path either
    fn vm_primary() -> Monitor {
        display(65537, "DISPLAY1", PRIMARY, None)
    }

    fn vm_secondary() -> Monitor {
        display(131073, "DISPLAY2", "UNKNOWN", None)
    }

    #[test]
    fn a_display_without_a_serial_number_is_not_the_one_that_is_left() {
        // the OS reports the primary only, and under a new handle as it does after a hotplug
        let attached = [display(393783, "DISPLAY1", PRIMARY, None)];
        let state = [vm_primary(), vm_secondary()];

        assert_eq!(disconnected_monitor_indices(&state, &attached), vec![1]);
    }

    #[test]
    fn a_display_without_a_device_path_is_kept_while_it_is_attached() {
        // the primary is the one unplugged
        let attached = [display(131073, "DISPLAY2", "UNKNOWN", None)];
        let state = [vm_primary(), vm_secondary()];

        assert_eq!(disconnected_monitor_indices(&state, &attached), vec![0]);
    }

    #[test]
    fn nothing_is_disconnected_while_every_display_is_attached() {
        let attached = [vm_primary(), vm_secondary()];
        let state = [vm_primary(), vm_secondary()];

        assert!(disconnected_monitor_indices(&state, &attached).is_empty());
    }

    #[test]
    fn displays_without_a_device_path_are_told_apart_by_name() {
        let third = display(131074, "DISPLAY3", "UNKNOWN", None);
        let state = [vm_primary(), vm_secondary(), third];

        // the second is attached, the third is not
        let attached = [vm_primary(), vm_secondary()];
        assert_eq!(disconnected_monitor_indices(&state, &attached), vec![2]);

        // the third is attached, the second is not
        let attached = [vm_primary(), display(131074, "DISPLAY3", "UNKNOWN", None)];
        assert_eq!(disconnected_monitor_indices(&state, &attached), vec![1]);
    }

    #[test]
    fn empty_serial_numbers_do_not_identify_a_display() {
        let state = [
            display(1, "DISPLAY1", "AAA-1", Some("")),
            display(2, "DISPLAY2", "BBB-2", Some("")),
        ];
        let attached = [display(1, "DISPLAY1", "AAA-1", Some(""))];

        assert_eq!(disconnected_monitor_indices(&state, &attached), vec![1]);
    }

    #[test]
    fn a_serial_number_still_identifies_a_display_that_moved_to_another_port() {
        let state = [
            display(1, "DISPLAY1", "AAA-1", Some("S1")),
            display(2, "DISPLAY2", "BBB-2", Some("S2")),
        ];
        let attached = [
            display(1, "DISPLAY1", "AAA-1", Some("S1")),
            display(3, "DISPLAY3", "CCC-3", Some("S2")),
        ];

        assert!(disconnected_monitor_indices(&state, &attached).is_empty());
    }

    #[test]
    fn a_new_serial_number_does_not_make_a_display_a_new_one() {
        let state = [display(2, "DISPLAY2", "BBB-2", Some("S2"))];
        let attached = [display(2, "DISPLAY2", "BBB-2", Some("S9"))];

        // the device id is the same, so it is the same display with a new serial number
        assert!(disconnected_monitor_indices(&state, &attached).is_empty());
    }

    #[test]
    fn displays_without_a_device_path_are_the_same_device_by_name_only() {
        assert!(same_device(
            ("UNKNOWN", "DISPLAY2"),
            ("UNKNOWN", "DISPLAY2")
        ));
        assert!(!same_device(
            ("UNKNOWN", "DISPLAY2"),
            ("UNKNOWN", "DISPLAY3")
        ));
        assert!(same_device(("AAA-1", "DISPLAY1"), ("AAA-1", "DISPLAY7")));
        assert!(!same_device(("AAA-1", "DISPLAY1"), ("BBB-2", "DISPLAY1")));
    }
}
