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
    let dip = DISPLAY_INDEX_PREFERENCES.read();
    let mut dip_ids = dip.values();
    let preferred_id = if dip_ids.any(|id| id.eq(&monitor.device_id)) {
        monitor.device_id.clone()
    } else if dip_ids.any(|id| Some(id) == monitor.serial_number_id.as_ref()) {
        monitor.serial_number_id.clone().unwrap_or_default()
    } else {
        serial_or_device_id.to_string()
    };
    let mut monitor_cache = MONITOR_CACHE
        .get_or_init(|| Mutex::new(HashMap::new()))
        .lock();

    monitor_cache.insert(preferred_id, monitor);
}

fn monitor_is_disconnected(monitor: &Monitor, attached_devices: &[Monitor]) -> bool {
    !attached_devices.iter().any(|attached| {
        let serial_matches = match (&attached.serial_number_id, &monitor.serial_number_id) {
            (Some(attached), Some(tracked)) => attached == tracked,
            _ => false,
        };
        serial_matches || attached.device_id == monitor.device_id
    })
}

fn monitor_topology_matches(
    monitors: &std::collections::VecDeque<Monitor>,
    attached_devices: &[Monitor],
) -> bool {
    monitors.len() == attached_devices.len()
        && !monitors
            .iter()
            .any(|monitor| monitor_is_disconnected(monitor, attached_devices))
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
                let mut monitor_cache = MONITOR_CACHE
                    .get_or_init(|| Mutex::new(HashMap::new()))
                    .lock();

                let initial_monitor_count = wm.monitors().len();

                // Get the currently attached display devices
                let attached_devices = attached_display_devices(display_provider)?;

                // Make sure that in our state any attached displays have the latest Win32 data
                for monitor in wm.monitors_mut() {
                    for attached in &attached_devices {
                        let serial_number_ids_match = if let (Some(attached_snid), Some(m_snid)) =
                            (&attached.serial_number_id, &monitor.serial_number_id)
                        {
                            attached_snid.eq(m_snid)
                        } else {
                            false
                        };

                        if serial_number_ids_match || attached.device_id.eq(&monitor.device_id) {
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
                let attached_devices = if wm
                    .monitors()
                    .iter()
                    .any(|monitor| monitor_is_disconnected(monitor, &attached_devices))
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
                                let serial_number_ids_match =
                                    if let (Some(attached_snid), Some(m_snid)) =
                                        (&attached.serial_number_id, &monitor.serial_number_id)
                                    {
                                        attached_snid.eq(m_snid)
                                    } else {
                                        false
                                    };

                                if serial_number_ids_match
                                    || attached.device_id.eq(&monitor.device_id)
                                {
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
                    monitor_cache = MONITOR_CACHE
                        .get_or_init(|| Mutex::new(HashMap::new()))
                        .lock();

                    // Make sure that in our state any attached displays have the latest Win32 data
                    // We must do this again because we dropped the lock and are working with new data
                    for monitor in wm.monitors_mut() {
                        for attached in &re_queried_devices {
                            let serial_number_ids_match =
                                if let (Some(attached_snid), Some(m_snid)) =
                                    (&attached.serial_number_id, &monitor.serial_number_id)
                                {
                                    attached_snid.eq(m_snid)
                                } else {
                                    false
                                };

                            if serial_number_ids_match || attached.device_id.eq(&monitor.device_id)
                            {
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

                let removed_monitor_indices: Vec<_> = wm
                    .monitors()
                    .iter()
                    .enumerate()
                    .filter_map(|(index, monitor)| {
                        monitor_is_disconnected(monitor, &attached_devices).then_some(index)
                    })
                    .collect();
                if !removed_monitor_indices.is_empty() {
                    tracing::info!("removing disconnected monitors");

                    for &m_idx in &removed_monitor_indices {
                        if let Some(m) = wm.monitors().get(m_idx) {
                            let id = m
                                .serial_number_id
                                .as_ref()
                                .map_or(m.device_id.clone(), |sn| sn.clone());

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

                            // Let's add their state to the cache for later, make sure to use what
                            // the user set as preference as the id.
                            let dip = DISPLAY_INDEX_PREFERENCES.read();
                            let mut dip_ids = dip.values();
                            let preferred_id = if dip_ids.any(|id| id.eq(&m.device_id)) {
                                m.device_id.clone()
                            } else if dip_ids.any(|id| Some(id) == m.serial_number_id.as_ref()) {
                                m.serial_number_id.clone().unwrap_or_default()
                            } else {
                                id
                            };
                            monitor_cache.insert(preferred_id, m.clone());
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

                // This is the list of device ids after we have removed detached displays. We can
                // keep this with just the device_ids without the serial numbers since this is used
                // only to check which one is the newly added monitor below if there is a new
                // monitor. Everything done after with said new monitor will again consider both
                // serial number and device ids.
                let post_removal_device_ids = wm
                    .monitors()
                    .iter()
                    .map(|m| &m.device_id)
                    .cloned()
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
                        let device_id = &m.device_id;
                        // We identify a new monitor when we encounter a new device id
                        if !post_removal_device_ids.contains(device_id) {
                            let mut cache_hit = false;
                            let mut cached_id = String::new();
                            // Check if that device id exists in the cache for this session
                            if let Some((id, cached)) = monitor_cache.get_key_value(device_id).or(m
                                .serial_number_id
                                .as_ref()
                                .and_then(|sn| monitor_cache.get_key_value(sn)))
                            {
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
                                    name: m.name.clone(),
                                    device: m.device.clone(),
                                    device_id: m.device_id.clone(),
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
                                    for container in workspace.containers_mut() {
                                        container.windows_mut().retain(|window| {
                                            window.exe().is_ok()
                                                && !known_hwnds.contains_key(&window.hwnd)
                                        });

                                        if is_focused_workspace {
                                            if let Some(window) = container.focused_window()
                                                && WindowsApi::is_window(window.hwnd)
                                            {
                                                tracing::debug!(
                                                    "restoring window: {}",
                                                    window.hwnd
                                                );
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

                                    workspace.remove_empty_containers();

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
                                        window.exe().is_ok()
                                            && !known_hwnds.contains_key(&window.hwnd)
                                    });

                                    if is_focused_workspace {
                                        for window in workspace.floating_windows() {
                                            if WindowsApi::is_window(window.hwnd) {
                                                WindowsApi::restore_window(window.hwnd);
                                            }
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
                                m.load_focused_workspace_without_focus()?;
                                m.update_focused_workspace(offset)?;
                            }

                            // Entries in the cache should only be used once; remove the entry there was a cache hit
                            if cache_hit && !cached_id.is_empty() {
                                monitor_cache.remove(&cached_id);
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
                .get_or_init(|| Mutex::new(HashMap::new()))
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
                .get_or_init(|| Mutex::new(HashMap::new()))
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
        monitor_cache: HashMap<String, Monitor>,
    }

    impl TestContext {
        fn new() -> Self {
            let shared = crate::test_support::TestContext::new();
            let monitor_cache = std::mem::take(
                &mut *MONITOR_CACHE
                    .get_or_init(|| Mutex::new(HashMap::new()))
                    .lock(),
            );
            Self {
                _shared: shared,
                monitor_cache,
            }
        }
    }

    impl Drop for TestContext {
        fn drop(&mut self) {
            *MONITOR_CACHE
                .get_or_init(|| Mutex::new(HashMap::new()))
                .lock() = std::mem::take(&mut self.monitor_cache);
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
    fn test_disconnected_monitors_without_serial_numbers() {
        let attached = vec![
            identity_monitor("DELA0DE-5&42b49c9&0&UID264", None),
            identity_monitor("GSM76E4-5&42b49c9&0&UID256", None),
        ];
        let mut tracked = attached.clone();
        tracked.push(identity_monitor(
            "Default_Monitor-1&1f0c3c2f&1&UID256",
            None,
        ));
        tracked.push(identity_monitor("UNKNOWN", None));

        assert_eq!(tracked.len(), 4);
        assert_eq!(attached.len(), 2);
        let disconnected: Vec<_> = tracked
            .iter()
            .filter(|monitor| monitor_is_disconnected(monitor, &attached))
            .map(|monitor| monitor.device_id.as_str())
            .collect();
        assert_eq!(
            disconnected,
            ["Default_Monitor-1&1f0c3c2f&1&UID256", "UNKNOWN"]
        );
    }

    #[test]
    fn test_same_count_monitor_replacement() {
        let tracked: std::collections::VecDeque<_> = [identity_monitor("console", None)].into();
        let attached = [identity_monitor("RDP", None)];
        assert_eq!(tracked.len(), attached.len());
        assert!(!monitor_topology_matches(&tracked, &attached));
        assert!(monitor_topology_matches(
            &tracked,
            &[identity_monitor("console", None)]
        ));
        assert!(!monitor_topology_matches(&tracked, &[]));

        let tracked = [identity_monitor("old-device", Some("serial"))].into();
        assert!(monitor_topology_matches(
            &tracked,
            &[identity_monitor("new-device", Some("serial"))]
        ));
    }

    #[test]
    fn test_attached_monitor_identity() {
        let cases = [
            ("same-device", None, "same-device", None, true),
            (
                "old-device",
                Some("serial"),
                "new-device",
                Some("serial"),
                true,
            ),
            (
                "old-device",
                Some("old-serial"),
                "new-device",
                Some("new-serial"),
                false,
            ),
            ("old-device", None, "new-device", None, false),
            ("old-device", Some("serial"), "new-device", None, false),
            ("old-device", None, "new-device", Some("serial"), false),
            ("same-device", Some("serial"), "same-device", None, true),
            ("same-device", None, "same-device", Some("serial"), true),
            (
                "same-device",
                Some("old-serial"),
                "same-device",
                Some("new-serial"),
                true,
            ),
        ];

        for (device, serial, attached_device, attached_serial, expected) in cases {
            let monitor = identity_monitor(device, serial);
            let attached = identity_monitor(attached_device, attached_serial);
            assert_eq!(
                !monitor_is_disconnected(&monitor, &[attached]),
                expected,
                "tracked: ({device}, {serial:?}), attached: ({attached_device}, {attached_serial:?})"
            );
        }

        assert!(monitor_is_disconnected(
            &identity_monitor("device", Some("serial")),
            &[]
        ));
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
        let cache = MONITOR_CACHE
            .get_or_init(|| Mutex::new(HashMap::new()))
            .lock();
        let retrieved_monitor = cache.get("TestMonitorID");

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
        let cache = MONITOR_CACHE
            .get_or_init(|| Mutex::new(HashMap::new()))
            .lock();

        // Check if Monitor 1 was found in the cache
        assert_eq!(
            cache.get("TestMonitorID"),
            Some(&m1),
            "Monitor cache should contain monitor 1"
        );

        // Check if Monitor 2 was found in the cache
        assert_eq!(
            cache.get("TestMonitorID2"),
            Some(&m2),
            "Monitor cache should contain monitor 2"
        );
    }

    #[test]
    fn test_handle_display_connection_notification() -> color_eyre::Result<()> {
        let (_context, wm) = setup_window_manager();
        assert!(wm.monitors().is_empty());
        let wm = Arc::new(Mutex::new(wm));
        let device = MockDevice {
            hmonitor: 1,
            device_path: r"\\?\DISPLAY#TEST#instance#{test}".to_string(),
            device_name: r"\\.\DISPLAY1".to_string(),
            device_description: String::new(),
            serial_number_id: None,
            size: RECT::default(),
            work_area_size: RECT::default(),
            device_key: String::new(),
            output_technology: None,
        };
        let provider = || std::iter::once(Ok(device.clone().into()));
        let (sender, receiver) = bounded(1);
        sender.send(MonitorNotification::DisplayConnectionChange)?;
        drop(sender);

        handle_notifications_with_receiver(Arc::clone(&wm), provider, receiver)?;

        let wm = wm.lock();
        assert_eq!(wm.monitors().len(), 1);
        assert_eq!(wm.monitors()[0].device_id, "TEST-instance");
        Ok(())
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
}
