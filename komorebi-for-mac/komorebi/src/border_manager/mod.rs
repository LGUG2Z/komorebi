mod border;
mod ns_window;

use crate::AccessibilityUiElement;
use crate::CoreFoundationRunLoop;
use crate::accessibility::AccessibilityApi;
use crate::border_manager::border::Border;
use crate::core::Rect;
use crate::core::WindowKind;
use crate::macos_api::MacosApi;
use crate::ring::Ring;
use crate::window_manager::WindowManager;
use crate::workspace::Workspace;
use crate::workspace::WorkspaceLayer;
use color_eyre::eyre;
use crossbeam_channel::Receiver;
use crossbeam_channel::Sender;
use crossbeam_utils::atomic::AtomicConsume;
use dispatch2::DispatchQueue;
use komorebi_themes::colour::Colour;
use komorebi_themes::colour::Rgb;
use lazy_static::lazy_static;
use objc2::rc::autoreleasepool;
use parking_lot::Mutex;
use std::collections::HashMap;
use std::collections::hash_map::Entry;
use std::sync::Arc;
use std::sync::OnceLock;
use std::sync::atomic::AtomicBool;
use std::sync::atomic::AtomicI32;
use std::sync::atomic::AtomicU32;
use std::sync::atomic::Ordering;

pub static BORDER_WIDTH: AtomicI32 = AtomicI32::new(10);
pub static BORDER_OFFSET: AtomicI32 = AtomicI32::new(0);
pub static BORDER_RADIUS: AtomicI32 = AtomicI32::new(10);
pub static BORDER_ENABLED: AtomicBool = AtomicBool::new(true);

// Windows has a 7px invisible border around every app which the border_x
// config options have to take into account. In order to keep the komorebi
// configuration truly portable across Windows and macOS, we need to add
// that same 7px adjustment to whatever BORDER_OFFSET is set to. I hate this,
// but config compatibility has to come first, and komorebi for Windows already
// has tens of thousands of users.
pub static BORDER_OFFSET_ADJUSTMENT: i32 = 7;

lazy_static! {
    pub static ref FOCUSED: AtomicU32 =
        AtomicU32::new(u32::from(Colour::Rgb(Rgb::new(66, 165, 245))));
    pub static ref UNFOCUSED: AtomicU32 =
        AtomicU32::new(u32::from(Colour::Rgb(Rgb::new(128, 128, 128))));
    pub static ref UNFOCUSED_LOCKED: AtomicU32 =
        AtomicU32::new(u32::from(Colour::Rgb(Rgb::new(158, 8, 8))));
    pub static ref MONOCLE: AtomicU32 =
        AtomicU32::new(u32::from(Colour::Rgb(Rgb::new(255, 51, 153))));
    pub static ref STACK: AtomicU32 = AtomicU32::new(u32::from(Colour::Rgb(Rgb::new(0, 165, 66))));
    pub static ref FLOATING: AtomicU32 =
        AtomicU32::new(u32::from(Colour::Rgb(Rgb::new(245, 245, 165))));
}

lazy_static! {
    static ref BORDER_STATE: Mutex<HashMap<String, Box<Border>>> = Mutex::new(HashMap::new());
    static ref WINDOWS_BORDERS: Mutex<HashMap<u32, String>> = Mutex::new(HashMap::new());
    // the latest frame of each window the animator has moved that has not been applied to a border
    static ref PENDING_FOLLOWS: Mutex<HashMap<u32, Rect>> = Mutex::new(HashMap::new());
}

static FOLLOW_SCHEDULED: AtomicBool = AtomicBool::new(false);

/// How many times in a row applying the frames has had to be put off because the borders were busy
static FOLLOW_DEFERRALS: AtomicU32 = AtomicU32::new(0);

/// How many times applying the frames is put off before they are given up on
const MAX_FOLLOW_DEFERRALS: u32 = 50;

pub enum Notification {
    // NOTE: bool should only ever be set to true by the reaper
    // TODO: eventually the bool should be a source enum so we can match on emitters
    Update(Option<AccessibilityUiElement>, Option<u32>, bool),
    ForceUpdate,
}

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

#[derive(Debug, Default, Clone, PartialEq)]
pub struct BorderInfo {
    pub border_id: String,
    pub window_kind: WindowKind,
}

pub fn window_border(window_id: u32) -> Option<BorderInfo> {
    let id = WINDOWS_BORDERS.lock().get(&window_id)?.clone();
    BORDER_STATE.lock().get(&id).map(|b| BorderInfo {
        border_id: b.id.clone(),
        window_kind: b.window_kind,
    })
}

/// What a border looks like, for a picture of a window that is to be drawn with one
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct BorderLook {
    pub colour: Rgb,
    pub width: f64,
    pub radius: f64,
    /// How far outside of the window the border is
    pub outset: f64,
}

/// What the border of a window of this kind looks like, or `None` when there are no borders
pub fn look_of(kind: WindowKind) -> Option<BorderLook> {
    if !BORDER_ENABLED.load(Ordering::Relaxed) {
        return None;
    }

    Some(BorderLook {
        colour: Rgb::from(window_kind_colour(kind)),
        width: f64::from(BORDER_WIDTH.load(Ordering::Relaxed)),
        radius: f64::from(BORDER_RADIUS.load(Ordering::Relaxed)),
        outset: f64::from(BORDER_OFFSET.load(Ordering::Relaxed) + BORDER_OFFSET_ADJUSTMENT),
    })
}

/// What the border that a window has now looks like, or `None` if it has none
pub fn window_look(window_id: u32) -> Option<BorderLook> {
    window_border(window_id).and_then(|info| look_of(info.window_kind))
}

/// Take down the borders of the windows on a monitor, which is to be done while pictures of them
/// are on screen instead
///
/// The borders are not shown again: they belong to windows that are being put away, and the ones
/// of the windows that take their place are made when the borders are next looked at.
pub fn hide_borders_on(monitor_idx: usize) {
    let windows: Vec<_> = BORDER_STATE
        .lock()
        .values()
        .filter(|border| border.monitor_idx == Some(monitor_idx))
        .map(|border| border.hide_handle())
        .collect();

    if windows.is_empty() {
        return;
    }

    DispatchQueue::main().exec_sync(move || {
        autoreleasepool(|_| {
            for window in windows {
                window.hide();
            }
        });
    });
}

pub fn send_notification(
    element: Option<AccessibilityUiElement>,
    window_id: Option<u32>,
    reaper: bool,
) {
    if event_tx()
        .try_send(Notification::Update(element, window_id, reaper))
        .is_err()
    {
        tracing::warn!("channel is full; dropping notification")
    }
}

/// Move the border around a window to follow the window while the animator moves it
///
/// This is called from the animator's threads once for every frame it writes. The frames are handed
/// to the main thread in batches, where only the latest frame of each window is applied.
pub fn follow(window_id: u32, frame: Rect) {
    if !BORDER_ENABLED.load_consume() {
        return;
    }

    PENDING_FOLLOWS.lock().insert(window_id, frame);

    if !FOLLOW_SCHEDULED.swap(true, Ordering::AcqRel) {
        DispatchQueue::main().exec_async(apply_follows);
    }
}

fn apply_follows() {
    FOLLOW_SCHEDULED.store(false, Ordering::Release);

    let frames = std::mem::take(&mut *PENDING_FOLLOWS.lock());

    // the border thread holds these while it creates borders, which it does by waiting for the main
    // thread, which is where this runs: waiting for them here would deadlock. Instead the frames
    // are put back, behind any newer ones, and this is tried again on the next turn of the main
    // thread, by which time the border thread has been let go
    let (Some(borders), Some(windows_borders)) =
        (BORDER_STATE.try_lock(), WINDOWS_BORDERS.try_lock())
    else {
        if FOLLOW_DEFERRALS.fetch_add(1, Ordering::Relaxed) < MAX_FOLLOW_DEFERRALS {
            let mut pending = PENDING_FOLLOWS.lock();

            for (window_id, frame) in frames {
                pending.entry(window_id).or_insert(frame);
            }

            drop(pending);

            if !FOLLOW_SCHEDULED.swap(true, Ordering::AcqRel) {
                DispatchQueue::main().exec_async(apply_follows);
            }
        } else {
            FOLLOW_DEFERRALS.store(0, Ordering::Relaxed);
        }

        return;
    };

    FOLLOW_DEFERRALS.store(0, Ordering::Relaxed);

    autoreleasepool(|_| {
        for (window_id, frame) in frames {
            if let Some(border_id) = windows_borders.get(&window_id)
                && let Some(border) = borders.get(border_id)
            {
                border.follow(frame);
            }
        }
    });
}

pub fn destroy_all_borders() -> eyre::Result<()> {
    let mut borders = BORDER_STATE.lock();
    tracing::info!(
        "purging known borders: {:?}",
        borders.iter().map(|b| b.1.id.clone()).collect::<Vec<_>>()
    );

    for (_, border) in borders.drain() {
        let _ = destroy_border(border);
    }

    drop(borders);

    WINDOWS_BORDERS.lock().clear();

    Ok(())
}

fn window_kind_colour(focus_kind: WindowKind) -> u32 {
    match focus_kind {
        WindowKind::Unfocused => UNFOCUSED.load(Ordering::Relaxed),
        WindowKind::UnfocusedLocked => UNFOCUSED_LOCKED.load(Ordering::Relaxed),
        WindowKind::Single => FOCUSED.load(Ordering::Relaxed),
        WindowKind::Stack => STACK.load(Ordering::Relaxed),
        WindowKind::Monocle => MONOCLE.load(Ordering::Relaxed),
        WindowKind::Floating => FLOATING.load(Ordering::Relaxed),
    }
}

fn remove_borders(
    borders: &mut HashMap<String, Box<Border>>,
    windows_borders: &mut HashMap<u32, String>,
    monitor_idx: usize,
    condition: impl Fn(&String, &Border) -> bool,
) -> color_eyre::Result<()> {
    let mut to_remove = vec![];
    for (id, border) in borders.iter() {
        // if border is on this monitor
        if border.monitor_idx.is_some_and(|idx| idx == monitor_idx)
            // and the condition applies
            && condition(id, border)
        {
            // we mark it to be removed
            to_remove.push(id.clone());
        }
    }

    for id in &to_remove {
        remove_border(id, borders, windows_borders)?;
    }

    Ok(())
}

fn remove_border(
    id: &str,
    borders: &mut HashMap<String, Box<Border>>,
    windows_borders: &mut HashMap<u32, String>,
) -> color_eyre::Result<()> {
    if let Some(removed_border) = borders.remove(id) {
        windows_borders.remove(&removed_border.tracking_window_id);
        destroy_border(removed_border)?;
    }

    Ok(())
}

fn destroy_border(border: Box<Border>) -> color_eyre::Result<()> {
    DispatchQueue::main().exec_sync(|| {
        tracing::info!("invalidating border observer");
        AccessibilityApi::invalidate_observer(&border.observer);
    });

    std::thread::sleep(std::time::Duration::from_millis(10));

    let raw_pointer = Box::into_raw(border);
    unsafe {
        (*raw_pointer).destroy();
    }

    Ok(())
}

pub fn listen_for_notifications(wm: Arc<Mutex<WindowManager>>, run_loop: CoreFoundationRunLoop) {
    std::thread::spawn(move || {
        loop {
            match handle_notifications(wm.clone(), run_loop.clone()) {
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

fn handle_notifications(
    wm: Arc<Mutex<WindowManager>>,
    run_loop: CoreFoundationRunLoop,
) -> color_eyre::Result<()> {
    tracing::info!("listening");

    let receiver = event_rx();
    event_tx().send(Notification::Update(None, None, false))?;

    let mut previous_snapshot = Ring::default();
    let mut previous_pending_move_op = None;
    let mut previous_is_paused = false;
    let mut previous_notification: Option<Notification> = None;
    let mut previous_layer = WorkspaceLayer::default();

    'receiver: for notification in receiver {
        let state = wm.lock();
        let is_paused = state.is_paused;
        let focused_monitor_idx = state.focused_monitor_idx();
        let focused_workspace_idx =
            state.monitors.elements()[focused_monitor_idx].focused_workspace_idx();
        let monitors = state.monitors.clone();
        let pending_move_op = *state.pending_move_op;
        let floating_window_hwnds = state.monitors.elements()[focused_monitor_idx].workspaces()
            [focused_workspace_idx]
            .floating_windows()
            .iter()
            .map(|w| w.id)
            .collect::<Vec<_>>();
        let workspace_layer = state.monitors.elements()[focused_monitor_idx].workspaces()
            [focused_workspace_idx]
            .layer;
        let foreground_window = MacosApi::foreground_window_id().unwrap_or_default();
        let managed_window_processes = monitors
            .elements()
            .iter()
            .flat_map(|m| m.workspaces())
            .flat_map(|w| w.containers())
            .flat_map(|c| c.windows())
            .map(|w| (w.id, w.application.process_id))
            .collect::<HashMap<_, _>>();
        let _layer_changed = previous_layer != workspace_layer;
        let _forced_update = matches!(notification, Notification::ForceUpdate);

        drop(state);

        let should_process_notification = match notification {
            Notification::Update(_, notification_window_id, reaper) => {
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

                // when we switch focus to/from a floating window
                let switch_focus_to_from_floating_window = floating_window_hwnds.iter().any(|fw| {
                    // if we switch focus to a floating window
                    fw == &notification_window_id.unwrap_or_default() ||
                            // if there is any floating window with a `WindowKind::Floating` border
                            // that no longer is the foreground window then we need to update that
                            // border.
                            (fw != &foreground_window
                                && window_border(*fw)
                                .is_some_and(|b| b.window_kind == WindowKind::Floating))
                });

                // when the focused window has an `Unfocused` border kind, usually this happens if
                // we focus an admin window and then refocus the previously focused window. For
                // komorebi it will have the same state has before, however the previously focused
                // window changed its border to unfocused so now we need to update it again.
                if !should_process_notification
                    && window_border(notification_window_id.unwrap_or_default())
                        .is_some_and(|b| b.window_kind == WindowKind::Unfocused)
                {
                    should_process_notification = true;
                }

                if !should_process_notification && switch_focus_to_from_floating_window {
                    should_process_notification = true;
                }

                if !should_process_notification
                    && let Some(Notification::Update(_, ref previous_window_id, false)) =
                        previous_notification
                    && previous_window_id.unwrap_or_default()
                        != notification_window_id.unwrap_or_default()
                {
                    should_process_notification = true;
                }

                if reaper {
                    should_process_notification = true;
                }

                should_process_notification
            }
            Notification::ForceUpdate => true,
        };

        if !should_process_notification {
            tracing::debug!(
                foreground_window,
                "monitor state matches latest snapshot, skipping notification"
            );
            continue 'receiver;
        }

        let mut borders = BORDER_STATE.lock();
        let mut windows_borders = WINDOWS_BORDERS.lock();

        // If borders are disabled
        if !BORDER_ENABLED.load_consume()
            // Or if the wm is paused
            || is_paused
        {
            // Destroy the borders we know about
            for (_, border) in borders.drain() {
                destroy_border(border)?;
            }

            windows_borders.clear();

            previous_is_paused = is_paused;
            continue 'receiver;
        }

        'monitors: for (monitor_idx, m) in monitors.elements().iter().enumerate() {
            if let Some(ws) = m.focused_workspace() {
                if !ws.tile {
                    remove_borders(&mut borders, &mut windows_borders, monitor_idx, |_, _| true)?;
                    continue 'monitors;
                }

                if let Some(monocle) = &ws.monocle_container
                    && let Some(window) = monocle.focused_window()
                {
                    let focused_window_id =
                        monocle.focused_window().map(|w| w.id).unwrap_or_default();
                    let id = monocle.id.clone();
                    let border = match borders.entry(id.clone()) {
                        Entry::Occupied(entry) => entry.into_mut(),
                        Entry::Vacant(entry) => {
                            if let Ok(border) = Border::create(
                                &monocle.id,
                                window.id,
                                window.application.process_id,
                                window.element.clone(),
                                Some(monitor_idx),
                                run_loop.clone(),
                            ) {
                                entry.insert(border)
                            } else {
                                continue 'monitors;
                            }
                        }
                    };

                    let new_focus_state = if monitor_idx != focused_monitor_idx {
                        WindowKind::Unfocused
                    } else {
                        WindowKind::Monocle
                    };

                    border.window_kind = new_focus_state;

                    // Update the borders tracking_hwnd in case it changed and remove the
                    // old `tracking_hwnd` from `WINDOWS_BORDERS` if needed.
                    if border.tracking_window_id != focused_window_id {
                        if let Some(previous) = windows_borders.get(&border.tracking_window_id) {
                            // Only remove the border from `windows_borders` if it
                            // still corresponds to the same border, if doesn't then
                            // it means it was already updated by another border for
                            // that window and in that case we don't want to remove it.
                            if previous == &id {
                                windows_borders.remove(&border.tracking_window_id);
                            }
                        }
                        border.tracking_window_id = focused_window_id;
                        // Only update tracking element if same process (safe for tabbed apps)
                        if window.application.process_id == border.process_id {
                            let border_ptr =
                                std::ptr::addr_of_mut!(**border).cast::<std::ffi::c_void>();
                            border.update_tracking_element(window.element.clone(), border_ptr);
                        }
                    }

                    // Update the border's monitor idx in case it changed
                    border.monitor_idx = Some(monitor_idx);
                    border.update();

                    windows_borders.insert(focused_window_id, id);

                    let border_id = border.id.clone();

                    if ws.layer == WorkspaceLayer::Floating {
                        handle_floating_borders(
                            &mut borders,
                            &mut windows_borders,
                            ws,
                            monitor_idx,
                            foreground_window,
                            run_loop.clone(),
                        )?;

                        // Remove all borders on this monitor except monocle and floating borders
                        remove_borders(&mut borders, &mut windows_borders, monitor_idx, |_, b| {
                            border_id != b.id
                                && !ws
                                    .floating_windows()
                                    .iter()
                                    .any(|w| w.id == b.tracking_window_id)
                        })?;
                    } else {
                        // Remove all borders on this monitor except monocle
                        remove_borders(&mut borders, &mut windows_borders, monitor_idx, |_, b| {
                            border_id != b.id
                        })?;
                    }
                    continue 'monitors;
                }

                // Collect focused workspace container and floating windows ID's
                let container_and_floating_window_ids = ws
                    .containers()
                    .iter()
                    .map(|c| c.id.clone())
                    .collect::<Vec<_>>();

                remove_borders(&mut borders, &mut windows_borders, monitor_idx, |id, _| {
                    !container_and_floating_window_ids.contains(id)
                })?;

                for (idx, c) in ws.containers().iter().enumerate() {
                    if let Some(window) = c.focused_window() {
                        let id = c.id.clone();

                        let border = match borders.entry(id.clone()) {
                            Entry::Occupied(entry) => entry.into_mut(),
                            Entry::Vacant(entry) => {
                                if let Ok(border) = Border::create(
                                    &c.id,
                                    window.id,
                                    window.application.process_id,
                                    window.element.clone(),
                                    Some(monitor_idx),
                                    run_loop.clone(),
                                ) {
                                    entry.insert(border)
                                } else {
                                    continue 'monitors;
                                }
                            }
                        };

                        // this happens if we cmd+w a window, but the app is still active with no windows
                        let ignore_foreground_window = foreground_window == 0;

                        let focus_state_condition = !container_has_focused_look(
                            idx == ws.focused_container_idx() && monitor_idx == focused_monitor_idx,
                            ignore_foreground_window,
                            window.id == foreground_window,
                            managed_window_processes.get(&foreground_window).copied()
                                == Some(window.application.process_id),
                        );

                        let new_focus_state = if focus_state_condition {
                            if c.locked {
                                WindowKind::UnfocusedLocked
                            } else {
                                WindowKind::Unfocused
                            }
                        } else if c.windows().len() > 1 {
                            WindowKind::Stack
                        } else {
                            WindowKind::Single
                        };

                        tracing::trace!(
                            container_idx = idx,
                            window_id = window.id,
                            process_id = window.application.process_id,
                            foreground_window,
                            focused_container = idx == ws.focused_container_idx(),
                            focused_monitor = monitor_idx == focused_monitor_idx,
                            kind = ?new_focus_state,
                            "deciding the border of a container"
                        );

                        // if we are in the cmd+w situation, we wanna make sure the window we're gonna
                        // set a focused border on actually has keyboard focus
                        if ignore_foreground_window
                            && matches!(new_focus_state, WindowKind::Single | WindowKind::Stack)
                        {
                            window.focus(false)?;
                        }

                        border.window_kind = new_focus_state;

                        if border.tracking_window_id != window.id {
                            if let Some(previous) = windows_borders.get(&border.tracking_window_id)
                            {
                                // Only remove the border from `windows_borders` if it
                                // still corresponds to the same border, if doesn't then
                                // it means it was already updated by another border for
                                // that window and in that case we don't want to remove it.
                                if previous == &id {
                                    windows_borders.remove(&border.tracking_window_id);
                                }
                            }
                            border.tracking_window_id = window.id;
                            // Only update tracking element if same process (safe for tabbed apps)
                            if window.application.process_id == border.process_id {
                                let border_ptr =
                                    std::ptr::addr_of_mut!(**border).cast::<std::ffi::c_void>();
                                border.update_tracking_element(window.element.clone(), border_ptr);
                            }
                        }

                        border.monitor_idx = Some(monitor_idx);
                        border.update();

                        windows_borders.insert(window.id, id);
                    }

                    handle_floating_borders(
                        &mut borders,
                        &mut windows_borders,
                        ws,
                        monitor_idx,
                        foreground_window,
                        run_loop.clone(),
                    )?;
                }
            }
        }

        previous_snapshot = monitors;
        previous_pending_move_op = pending_move_op;
        previous_is_paused = is_paused;
        previous_notification = Some(notification);
        previous_layer = workspace_layer;
    }

    Ok(())
}

fn handle_floating_borders(
    borders: &mut HashMap<String, Box<Border>>,
    windows_borders: &mut HashMap<u32, String>,
    ws: &Workspace,
    monitor_idx: usize,
    foreground_window: u32,
    run_loop: CoreFoundationRunLoop,
) -> color_eyre::Result<()> {
    for window in ws.floating_windows() {
        let id = window.id.to_string();
        let border = match borders.entry(id.clone()) {
            Entry::Occupied(entry) => entry.into_mut(),
            Entry::Vacant(entry) => {
                if let Ok(border) = Border::create(
                    &window.id.to_string(),
                    window.id,
                    window.application.process_id,
                    window.element.clone(),
                    Some(monitor_idx),
                    run_loop.clone(),
                ) {
                    entry.insert(border)
                } else {
                    return Ok(());
                }
            }
        };

        let new_focus_state = if foreground_window == window.id {
            WindowKind::Floating
        } else {
            WindowKind::Unfocused
        };

        border.window_kind = new_focus_state;
        // Update the border's monitor idx in case it changed
        border.monitor_idx = Some(monitor_idx);

        border.update();

        windows_borders.insert(window.id, id);
    }

    Ok(())
}

/// Whether the border of a container is the one for the window that has the focus.
///
/// komorebi focuses a window and asks the window server which window has the focus soon after. An
/// application with another window beside the one komorebi focused can be slow to move its own
/// focus, and until it does the other window is the one it reports. Both windows are komorebi's,
/// and the one in its focused container is the one it chose.
fn container_has_focused_look(
    is_focused_container: bool,
    ignore_foreground_window: bool,
    is_foreground_window: bool,
    foreground_window_is_another_of_the_same_application: bool,
) -> bool {
    is_focused_container
        && (ignore_foreground_window
            || is_foreground_window
            || foreground_window_is_another_of_the_same_application)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_focused_window_that_has_the_focus_has_the_focused_look() {
        assert!(container_has_focused_look(true, false, true, false));
    }

    #[test]
    fn a_window_that_is_not_in_the_focused_container_does_not() {
        assert!(!container_has_focused_look(false, false, true, false));
    }

    #[test]
    fn the_focused_window_while_another_application_has_the_focus_does_not() {
        // a window of an application that komorebi does not manage, such as an admin prompt
        assert!(!container_has_focused_look(true, false, false, false));
    }

    #[test]
    fn the_focused_window_of_an_application_with_no_window_focused_has_it() {
        assert!(container_has_focused_look(true, true, false, false));
    }

    #[test]
    fn the_focused_window_while_its_application_still_reports_its_other_window_has_it() {
        // WezTerm 51 is focused here and WezTerm still says that 939 is its focused window
        assert!(container_has_focused_look(true, false, false, true));
    }
}
