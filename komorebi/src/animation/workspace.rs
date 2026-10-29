use std::cell::RefCell;
use std::collections::HashMap;
use std::collections::HashSet;
use std::sync::atomic::AtomicBool;
use std::sync::atomic::AtomicUsize;
use std::sync::atomic::Ordering;
use std::time::Duration;
use std::time::Instant;

use windows::Win32::Graphics::Dwm::DwmFlush;

use crate::HIDING_BEHAVIOUR;
use crate::animation::ANIMATION_DURATION_GLOBAL;
use crate::animation::ANIMATION_DURATION_PER_ANIMATION;
use crate::animation::ANIMATION_ENABLED_GLOBAL;
use crate::animation::ANIMATION_ENABLED_PER_ANIMATION;
use crate::animation::ANIMATION_MANAGER;
use crate::animation::ANIMATION_STYLE_GLOBAL;
use crate::animation::ANIMATION_STYLE_PER_ANIMATION;
use crate::animation::AnimationEngine;
use crate::animation::RenderDispatcher;
use crate::animation::ghost::GhostWindow;
use crate::animation::ghost::RevealOverlay;
use crate::animation::ghost::present_clips;
use crate::animation::ghost::show_clips;
use crate::animation::lerp::Lerp;
use crate::animation::prefix::AnimationPrefix;
use crate::animation::prefix::new_animation_key;
use crate::border_manager;
use crate::com::SetCloak;
use crate::core::AnimationStyle;
use crate::core::HidingBehaviour;
use crate::core::Rect;
use crate::stackbar_manager;
use crate::transparency_manager;
use crate::window::Window;
use crate::windows_api::WindowsApi;
use lazy_static::lazy_static;
use parking_lot::Mutex;
use parking_lot::MutexGuard;

/// Horizontal shift for a workspace change, in pixels.
///
/// A higher target index enters from the right (`+width`). A lower target index
/// enters from the left (`-width`). The distance is always one monitor width.
pub fn slide_offset(from_idx: usize, to_idx: usize, width: i32) -> i32 {
    match to_idx.cmp(&from_idx) {
        std::cmp::Ordering::Greater => width,
        std::cmp::Ordering::Less => -width,
        std::cmp::Ordering::Equal => 0,
    }
}

fn shift_x(rect: Rect, dx: i32) -> Rect {
    Rect {
        left: rect.left + dx,
        ..rect
    }
}

struct ClippedGhost {
    host: Rect,
    source_x: i32,
    source_y: i32,
    source_width: i32,
    source_height: i32,
}

/// Portion of `window` that lies inside `monitor`. Both rects are left/top plus size.
/// `None` when the window is fully outside the monitor.
fn clip_window(window: Rect, monitor: Rect) -> Option<ClippedGhost> {
    let window_right = window.left.saturating_add(window.right);
    let window_bottom = window.top.saturating_add(window.bottom);
    let monitor_right = monitor.left.saturating_add(monitor.right);
    let monitor_bottom = monitor.top.saturating_add(monitor.bottom);

    let left = window.left.max(monitor.left);
    let top = window.top.max(monitor.top);
    let right = window_right.min(monitor_right);
    let bottom = window_bottom.min(monitor_bottom);

    if right <= left || bottom <= top {
        return None;
    }

    Some(ClippedGhost {
        host: Rect {
            left,
            top,
            right: right - left,
            bottom: bottom - top,
        },
        source_x: left - window.left,
        source_y: top - window.top,
        source_width: right - left,
        source_height: bottom - top,
    })
}

/// `clip_window` is in visual-rect space. The DWM thumbnail is the real window,
/// which for a tiled incoming window is already the post-layout size while the
/// ghost is still interpolating. Scale the crop into that thumbnail.
fn clip_thumbnail(
    visual: Rect,
    monitor: Rect,
    source_width: i32,
    source_height: i32,
) -> Option<ClippedGhost> {
    let clipped = clip_window(visual, monitor)?;
    let visual_width = visual.right.max(1);
    let visual_height = visual.bottom.max(1);
    let source_width = source_width.max(1);
    let source_height = source_height.max(1);

    Some(ClippedGhost {
        host: clipped.host,
        source_x: clipped.source_x.saturating_mul(source_width) / visual_width,
        source_y: clipped.source_y.saturating_mul(source_height) / visual_height,
        source_width: (clipped.source_width.saturating_mul(source_width) / visual_width).max(1),
        source_height: (clipped.source_height.saturating_mul(source_height) / visual_height).max(1),
    })
}

/// Visible rect for a slide snapshot. A minimized window is restored and cloaked
/// again so the read does not leave it on screen, and so the later slide can
/// interpolate from this size instead of the post-layout one.
pub fn snapshot_visible(hwnd: isize) -> Option<Rect> {
    let rect = WindowsApi::window_rect(hwnd).ok()?;
    if rect.left > -30_000 && rect.top > -30_000 {
        return Some(rect);
    }
    Window::from(hwnd).restore_with_border(false);
    cloak_window(hwnd);
    let restored = WindowsApi::window_rect(hwnd).ok()?;
    if restored.left <= -30_000 || restored.top <= -30_000 {
        None
    } else {
        Some(restored)
    }
}

pub fn cloak_windows(hwnds: &[isize]) {
    for hwnd in hwnds {
        cloak_window(*hwnd);
    }
}

pub fn uncloak_windows(hwnds: &[isize]) {
    for hwnd in hwnds {
        uncloak_window(*hwnd);
    }
}

fn cloak_window(hwnd: isize) {
    SetCloak(Window::from(hwnd).hwnd(), 1, 2);
}

fn uncloak_window(hwnd: isize) {
    SetCloak(Window::from(hwnd).hwnd(), 1, 0);
}

// Show/Uncloak/Focus events from the slide itself must not be treated as the user
// alt-tabbing back to the previous workspace. The deadline outlives the animation
// so events still queued when it finishes are ignored too.
lazy_static! {
    static ref SLIDE_IGNORED_EVENTS: Mutex<HashMap<isize, Instant>> = Mutex::new(HashMap::new());
    /// Window -> id of the slide that owns it. A newer slide takes over the windows
    /// of an older one that has not settled yet.
    static ref SLIDE_DRIVEN_HWNDS: Mutex<HashMap<isize, usize>> = Mutex::new(HashMap::new());
}

static INSTANT_POSITION: AtomicBool = AtomicBool::new(false);
static SLIDE_ACTIVE: AtomicUsize = AtomicUsize::new(0);
static NEXT_SLIDE_ID: AtomicUsize = AtomicUsize::new(1);
/// Non-zero while a new slide waits for the previous one; cuts its reveal fade short.
static SETTLE_REQUESTS: AtomicUsize = AtomicUsize::new(0);

pub fn begin_instant_position() {
    INSTANT_POSITION.store(true, Ordering::SeqCst);
}

pub fn end_instant_position() {
    INSTANT_POSITION.store(false, Ordering::SeqCst);
}

pub fn position_instantly() -> bool {
    INSTANT_POSITION.load(Ordering::SeqCst)
}

pub fn slide_in_progress() -> bool {
    SLIDE_ACTIVE.load(Ordering::SeqCst) > 0
}

pub fn slide_drives_window(hwnd: isize) -> bool {
    SLIDE_DRIVEN_HWNDS.lock().contains_key(&hwnd)
}

fn drive_slide_windows(hwnds: &[isize], slide_id: usize) {
    let mut driven = SLIDE_DRIVEN_HWNDS.lock();
    for hwnd in hwnds {
        driven.insert(*hwnd, slide_id);
    }
}

fn release_slide_windows(hwnds: &[isize], slide_id: usize) {
    let mut driven = SLIDE_DRIVEN_HWNDS.lock();
    for hwnd in hwnds {
        if driven.get(hwnd) == Some(&slide_id) {
            driven.remove(hwnd);
        }
    }
}

/// Finish a slide on this monitor before a new layout moves the same windows.
pub fn settle_slide(monitor_id: isize) {
    SETTLE_REQUESTS.fetch_add(1, Ordering::SeqCst);
    let key = new_animation_key(AnimationPrefix::Workspace, monitor_id.to_string());
    if ANIMATION_MANAGER.lock().in_progress(key.as_str()) {
        let _ = AnimationEngine::cancel(key.as_str());
    }
    // A slide leaves the animation manager before it reveals, hides and fades its
    // windows. Wait for that, or it uncloaks windows the next slide just hid.
    drop(SLIDE_LOCK.try_lock_for(Duration::from_secs(1)));
    SETTLE_REQUESTS.fetch_sub(1, Ordering::SeqCst);
}

fn suppress_slide_events(hwnds: &[isize], for_duration: Duration) {
    let deadline = Instant::now() + for_duration;
    let mut ignored = SLIDE_IGNORED_EVENTS.lock();
    let now = Instant::now();
    ignored.retain(|_, until| *until > now);
    for hwnd in hwnds {
        ignored.insert(*hwnd, deadline);
    }
}

pub fn slide_event_suppressed(hwnd: isize) -> bool {
    let mut ignored = SLIDE_IGNORED_EVENTS.lock();
    let now = Instant::now();
    ignored.retain(|_, until| *until > now);
    ignored.get(&hwnd).is_some_and(|until| *until > now)
}

/// `Some` when workspace slides should run. Duration and style follow the
/// `workspace` prefix, then the global animation settings.
pub fn animation_settings() -> Option<(Duration, AnimationStyle)> {
    let prefix_enabled = ANIMATION_ENABLED_PER_ANIMATION
        .lock()
        .get(&AnimationPrefix::Workspace)
        .is_some_and(|enabled| *enabled);
    if !prefix_enabled && !ANIMATION_ENABLED_GLOBAL.load(Ordering::SeqCst) {
        return None;
    }

    let duration_ms = ANIMATION_DURATION_PER_ANIMATION
        .lock()
        .get(&AnimationPrefix::Workspace)
        .copied()
        .unwrap_or_else(|| ANIMATION_DURATION_GLOBAL.load(Ordering::SeqCst));
    let duration = Duration::from_millis(duration_ms);
    let style = ANIMATION_STYLE_PER_ANIMATION
        .lock()
        .get(&AnimationPrefix::Workspace)
        .copied()
        .unwrap_or_else(|| *ANIMATION_STYLE_GLOBAL.lock());

    Some((duration, style))
}

struct TrackedWindow {
    hwnd: isize,
    incoming: bool,
    /// Floating windows keep the outer rect captured before the slide.
    floating: bool,
    outer: Option<Rect>,
    start: Rect,
    target: Rect,
    /// Invisible resize border between `GetWindowRect` and the visible frame.
    /// Thumbnail source coordinates start at the outer rect.
    source_offset: (i32, i32),
    active: bool,
    ghost: Option<GhostWindow>,
    cloaked: bool,
}

pub struct SlideRequest {
    pub monitor_id: isize,
    pub outgoing: Vec<isize>,
    pub incoming: Vec<isize>,
    pub focus_hwnd: Option<isize>,
    pub mouse_follows_focus: bool,
    pub offset: i32,
    pub monitor_bounds: Rect,
    pub duration: Duration,
    /// Visible rect of each incoming window before the destination was laid out.
    /// Missing entries fall back to the post-layout rect, a pure horizontal slide.
    pub incoming_from: HashMap<isize, Rect>,
    pub floating: HashSet<isize>,
    /// `GetWindowRect` of floating windows, captured before anything moves them.
    pub floating_outer: HashMap<isize, Rect>,
}

static SLIDE_LOCK: Mutex<()> = Mutex::new(());

thread_local! {
    static SLIDE_GUARD: RefCell<Option<MutexGuard<'static, ()>>> = const { RefCell::new(None) };
}

fn hold_slide_lock() {
    SLIDE_GUARD.with(|slot| {
        let mut guard = slot.borrow_mut();
        if guard.is_none() {
            *guard = Some(SLIDE_LOCK.lock());
        }
    });
}

fn release_slide_lock() {
    SLIDE_GUARD.with(|slot| {
        slot.borrow_mut().take();
    });
}

struct WorkspaceSlideDispatcher {
    slide_id: usize,
    monitor_id: isize,
    windows: Mutex<Vec<TrackedWindow>>,
    style: AnimationStyle,
    focus_hwnd: Option<isize>,
    mouse_follows_focus: bool,
    offset: i32,
    monitor_bounds: Rect,
    duration: Duration,
    incoming_from: HashMap<isize, Rect>,
    prepared: AtomicBool,
    settled: AtomicBool,
    borders_suppressed: AtomicBool,
}

impl WorkspaceSlideDispatcher {
    fn new(request: SlideRequest, style: AnimationStyle, slide_id: usize) -> Self {
        let mut windows = Vec::with_capacity(request.outgoing.len() + request.incoming.len());
        for hwnd in request.outgoing {
            windows.push(tracked_window(
                hwnd,
                false,
                request.floating.contains(&hwnd),
                request.floating_outer.get(&hwnd).copied(),
            ));
        }
        for hwnd in request.incoming {
            if let Some(existing) = windows.iter_mut().find(|window| window.hwnd == hwnd) {
                existing.incoming = true;
                existing.cloaked = true;
                existing.floating = request.floating.contains(&hwnd);
                existing.outer = request.floating_outer.get(&hwnd).copied();
                continue;
            }
            windows.push(tracked_window(
                hwnd,
                true,
                request.floating.contains(&hwnd),
                request.floating_outer.get(&hwnd).copied(),
            ));
        }

        Self {
            slide_id,
            monitor_id: request.monitor_id,
            windows: Mutex::new(windows),
            style,
            focus_hwnd: request.focus_hwnd,
            mouse_follows_focus: request.mouse_follows_focus,
            offset: request.offset,
            monitor_bounds: request.monitor_bounds,
            duration: request.duration,
            incoming_from: request.incoming_from,
            prepared: AtomicBool::new(false),
            settled: AtomicBool::new(false),
            borders_suppressed: AtomicBool::new(false),
        }
    }

    fn prepare(&self) {
        if self.prepared.swap(true, Ordering::SeqCst) {
            return;
        }

        stackbar_manager::STACKBAR_TEMPORARILY_DISABLED.store(true, Ordering::SeqCst);
        stackbar_manager::send_notification();
        border_manager::suppress_borders();
        self.borders_suppressed.store(true, Ordering::SeqCst);

        let mut windows = self.windows.lock();
        let hwnds = windows.iter().map(|window| window.hwnd).collect::<Vec<_>>();
        // Cover the slide plus the Show/Uncloak events that arrive after it ends.
        suppress_slide_events(&hwnds, self.duration + Duration::from_millis(1500));

        let mut opening = Vec::new();
        for window in windows.iter_mut() {
            // Post-layout visible rect. Incoming windows are already cloaked, so
            // reading this does not flash the new tile on screen.
            let Some(rect) = visible_rect(window.hwnd) else {
                tracing::warn!(
                    "workspace slide: no rect for hwnd {}; showing or hiding it immediately",
                    window.hwnd
                );
                settle_window_now(window);
                continue;
            };

            if let Ok(outer) = WindowsApi::outer_window_rect(window.hwnd) {
                window.source_offset = (
                    (rect.left - outer.left).max(0),
                    (rect.top - outer.top).max(0),
                );
            }

            if window.incoming {
                if window.floating {
                    // Floating windows are not retiled. Slide the rect they already
                    // had so the ghost cannot animate them into a smaller size.
                    let origin = self
                        .incoming_from
                        .get(&window.hwnd)
                        .copied()
                        .unwrap_or(rect);
                    window.target = origin;
                    window.start = shift_x(origin, self.offset);
                } else {
                    window.target = rect;
                    let origin = self
                        .incoming_from
                        .get(&window.hwnd)
                        .copied()
                        .unwrap_or(rect);
                    window.start = shift_x(origin, self.offset);
                }
            } else {
                window.start = rect;
                window.target = shift_x(rect, -self.offset);
            }

            // Outgoing windows are still on screen; they are cloaked once their
            // ghosts are up so the desktop never shows through between the two.
            if window.incoming {
                cloak_window(window.hwnd);
                window.cloaked = true;
            }

            match GhostWindow::create_with_visibility(
                window.hwnd,
                window.start,
                Some(window.hwnd),
                false,
            ) {
                Ok(ghost) => {
                    opening.push(clip_for(&ghost, window, window.start, self.monitor_bounds));
                    window.ghost = Some(ghost);
                    window.active = true;
                }
                Err(error) => {
                    tracing::warn!(
                        "workspace slide: ghost for hwnd {} failed: {error}",
                        window.hwnd
                    );
                    settle_window_now(window);
                }
            }
        }

        if !opening.is_empty()
            && let Err(error) = show_clips(opening)
        {
            tracing::trace!("workspace slide: opening frame failed: {error}");
        }
        for window in windows.iter_mut() {
            if window.active && !window.cloaked {
                cloak_window(window.hwnd);
                window.cloaked = true;
            }
        }
        unsafe {
            let _ = DwmFlush();
        }

        let any_active = windows.iter().any(|window| window.active);
        drop(windows);

        if !any_active {
            self.finish(true);
        }
    }

    fn finish(&self, move_mouse: bool) {
        if self.settled.swap(true, Ordering::SeqCst) {
            return;
        }
        if !self.prepared.load(Ordering::SeqCst) {
            return;
        }

        let mut overlays = Vec::new();
        let owned = {
            let mut windows = self.windows.lock();
            let hwnds = windows.iter().map(|window| window.hwnd).collect::<Vec<_>>();
            let owned = {
                let driven = SLIDE_DRIVEN_HWNDS.lock();
                hwnds
                    .iter()
                    .filter(|hwnd| driven.get(*hwnd) == Some(&self.slide_id))
                    .copied()
                    .collect::<HashSet<_>>()
            };
            // Refresh the ignore window so uncloak/show from settling cannot switch back.
            suppress_slide_events(&hwnds, Duration::from_millis(1500));

            // Thumbnails lack the backdrop (acrylic, mica) DWM draws behind a window,
            // and that backdrop only exists while the window is fully opaque, so
            // neither the window nor its ghost can fade. Cover each window with an
            // opaque copy of what its ghost showed and fade that instead.
            if move_mouse {
                for window in windows.iter() {
                    if window.incoming
                        && owned.contains(&window.hwnd)
                        && let Some(ghost) = window.ghost.as_ref()
                        && let Ok(overlay) = RevealOverlay::create(
                            window.hwnd,
                            clip_for(ghost, window, window.target, self.monitor_bounds),
                        )
                    {
                        overlays.push(overlay);
                    }
                }
                if !overlays.is_empty() {
                    unsafe {
                        let _ = DwmFlush();
                    }
                }
            }

            for window in windows.iter_mut() {
                let ghost = window.ghost.take();
                window.active = false;
                // A newer slide already took this window over and decides where it ends up.
                if !owned.contains(&window.hwnd) {
                    if let Some(ghost) = ghost {
                        let _ = ghost.dispose();
                    }
                    continue;
                }

                if window.incoming {
                    place_incoming(window);
                    reveal_incoming(window, ghost.as_ref());
                } else {
                    conceal_outgoing(window);
                }
                if let Some(ghost) = ghost {
                    let _ = ghost.dispose();
                }
            }
            release_slide_windows(&hwnds, self.slide_id);
            owned
        };

        if SLIDE_ACTIVE.load(Ordering::SeqCst) > 0 {
            SLIDE_ACTIVE.fetch_sub(1, Ordering::SeqCst);
        }

        if self.borders_suppressed.swap(false, Ordering::SeqCst) {
            border_manager::restore_borders();
        }

        if move_mouse
            && let Some(hwnd) = self.focus_hwnd
            && owned.contains(&hwnd)
        {
            if let Err(error) = Window::from(hwnd).focus(false) {
                tracing::warn!("workspace slide: failed to focus hwnd {hwnd}: {error}");
            }
            if self.mouse_follows_focus
                && let Ok(rect) = WindowsApi::window_rect(hwnd)
                && let Err(error) = WindowsApi::center_cursor_in_rect(&rect)
            {
                tracing::warn!("workspace slide: failed to center cursor: {error}");
            }
        }

        fade_out_overlays(overlays, (self.duration / 2).min(REVEAL_FADE));

        self.finalise_managers();
    }

    fn finalise_managers(&self) {
        let busy = {
            let manager = ANIMATION_MANAGER.lock();
            manager.count_in_progress(AnimationPrefix::Workspace) > 0
                || manager.count_in_progress(AnimationPrefix::Movement) > 0
        };
        if busy {
            return;
        }

        if let Some(hwnd) = self.focus_hwnd
            && WindowsApi::foreground_window().unwrap_or_default() == hwnd
        {
            crate::focus_manager::send_notification(hwnd);
        }

        stackbar_manager::STACKBAR_TEMPORARILY_DISABLED.store(false, Ordering::SeqCst);
        stackbar_manager::send_notification();
        transparency_manager::send_notification();
    }
}

fn tracked_window(
    hwnd: isize,
    incoming: bool,
    floating: bool,
    outer: Option<Rect>,
) -> TrackedWindow {
    TrackedWindow {
        hwnd,
        incoming,
        floating,
        outer,
        start: Rect::default(),
        target: Rect::default(),
        source_offset: (0, 0),
        active: false,
        ghost: None,
        // The caller cloaks incoming windows before the layout snap. Outgoing ones
        // stay visible until their ghosts are up.
        cloaked: incoming,
    }
}

fn visible_rect(hwnd: isize) -> Option<Rect> {
    let rect = WindowsApi::window_rect(hwnd).ok()?;
    // Minimized windows report a sentinel rect around -32000.
    if rect.left <= -30_000 || rect.top <= -30_000 {
        Window::from(hwnd).restore_with_border(false);
        cloak_window(hwnd);
        let restored = WindowsApi::window_rect(hwnd).ok()?;
        if restored.left <= -30_000 || restored.top <= -30_000 {
            None
        } else {
            Some(restored)
        }
    } else {
        Some(rect)
    }
}

fn thumbnail_source(window: &TrackedWindow) -> (i32, i32) {
    // Incoming tiled windows were already moved to the post-layout size while
    // cloaked, so the thumbnail is that size. Outgoing and floating windows
    // were not resized; their pixels still match `start`.
    if window.incoming && !window.floating {
        (window.target.right, window.target.bottom)
    } else {
        (window.start.right, window.start.bottom)
    }
}

fn clip_for(
    ghost: &GhostWindow,
    window: &TrackedWindow,
    visual: Rect,
    monitor: Rect,
) -> crate::animation::ghost::GhostClip {
    let (source_width, source_height) = thumbnail_source(window);
    let (offset_x, offset_y) = window.source_offset;
    match clip_thumbnail(visual, monitor, source_width, source_height) {
        Some(clipped) => ghost.clip(
            Some(clipped.host),
            clipped.source_x + offset_x,
            clipped.source_y + offset_y,
            clipped.source_width,
            clipped.source_height,
        ),
        None => ghost.clip(None, 0, 0, 0, 0),
    }
}

/// Tiled windows get one shadow-aware placement. Floating windows are put back
/// on the outer rect captured before the slide, so they cannot shrink.
/// Skips windows already in place: the move is synchronous and forces a frame
/// recalculation, which stalls on apps with a busy UI thread.
fn place_incoming(window: &TrackedWindow) {
    if window.floating {
        if let Some(outer) = window.outer
            && WindowsApi::outer_window_rect(window.hwnd).ok() != Some(outer)
        {
            let _ = WindowsApi::move_window_exact(window.hwnd, &outer);
        }
        return;
    }
    if Window::from(window.hwnd).is_maximized() {
        return;
    }
    if window.target.right <= 0 || window.target.bottom <= 0 {
        return;
    }
    if WindowsApi::window_rect(window.hwnd).ok() == Some(window.target) {
        return;
    }
    let _ = WindowsApi::position_window(window.hwnd, &window.target, false, false);
}

/// A ghost left under its revealed window doubles any translucent layer for a
/// frame, so its thumbnail is hidden right after the uncloak, before anything
/// that waits on the window's thread. Destroying the host on the owner thread
/// would land a frame late.
fn reveal_incoming(window: &mut TrackedWindow, ghost: Option<&GhostWindow>) {
    if window.cloaked {
        uncloak_window(window.hwnd);
        window.cloaked = false;
    }
    if let Some(ghost) = ghost {
        let _ = ghost.set_opacity(0);
    }
    Window::from(window.hwnd).restore_with_border(false);
}

fn conceal_outgoing(window: &mut TrackedWindow) {
    Window::from(window.hwnd).hide();
    // `hide` for Minimize/Hide does not clear a cloak we applied. Drop it only
    // after the window is minimized or SW_HIDE'd, so it does not flash on screen.
    // Cloak hiding leaves our cloak in place on purpose.
    #[allow(deprecated)]
    let cloak_hides = matches!(*HIDING_BEHAVIOUR.lock(), HidingBehaviour::Cloak);
    if window.cloaked && !cloak_hides {
        uncloak_window(window.hwnd);
        window.cloaked = false;
    }
}

const REVEAL_FADE: Duration = Duration::from_millis(200);

/// Fade the overlays out over their revealed windows, one step per composed frame.
fn fade_out_overlays(overlays: Vec<RevealOverlay>, duration: Duration) {
    if !overlays.is_empty() {
        let started = Instant::now();
        loop {
            let progress = if duration.is_zero() {
                1.0
            } else {
                (started.elapsed().as_secs_f64() / duration.as_secs_f64()).min(1.0)
            };
            if progress >= 1.0 || SETTLE_REQUESTS.load(Ordering::SeqCst) > 0 {
                break;
            }

            let eased = progress * progress * (3.0 - 2.0 * progress);
            #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
            let alpha = (255.0 * (1.0 - eased)).round() as u8;
            for overlay in &overlays {
                let _ = overlay.set_alpha(alpha);
            }
            unsafe {
                let _ = DwmFlush();
            }
        }
    }

    for overlay in overlays {
        overlay.dispose();
    }
}

fn settle_window_now(window: &mut TrackedWindow) {
    if window.incoming {
        let ghost = window.ghost.take();
        place_incoming(window);
        reveal_incoming(window, ghost.as_ref());
        window.ghost = ghost;
    } else {
        conceal_outgoing(window);
    }
    if let Some(ghost) = window.ghost.take() {
        let _ = ghost.dispose();
    }
    window.active = false;
}

impl RenderDispatcher for WorkspaceSlideDispatcher {
    fn get_animation_key(&self) -> String {
        new_animation_key(AnimationPrefix::Workspace, self.monitor_id.to_string())
    }

    fn pre_render(&self) -> color_eyre::eyre::Result<()> {
        hold_slide_lock();
        self.prepare();
        Ok(())
    }

    fn vblank_paced(&self) -> bool {
        true
    }

    fn completed(&self) -> bool {
        self.settled.load(Ordering::SeqCst)
    }

    fn render(&self, progress: f64) -> color_eyre::eyre::Result<()> {
        if self.settled.load(Ordering::SeqCst) {
            return Ok(());
        }

        let clips = {
            let windows = self.windows.lock();
            let mut clips = Vec::new();
            for window in windows.iter() {
                if !window.active {
                    continue;
                }
                let Some(ghost) = window.ghost.as_ref() else {
                    continue;
                };
                let visual = window.start.lerp(window.target, progress, self.style);
                clips.push(clip_for(ghost, window, visual, self.monitor_bounds));
            }
            clips
        };

        if !clips.is_empty()
            && let Err(error) = present_clips(clips)
        {
            tracing::trace!("workspace slide present failed: {error}");
        }

        Ok(())
    }

    fn post_render(&self) -> color_eyre::eyre::Result<()> {
        self.finish(true);
        self.finalise_managers();
        release_slide_lock();
        Ok(())
    }

    fn cleanup_on_cancel(&self) {
        self.finish(false);
        release_slide_lock();
    }
}

impl Drop for WorkspaceSlideDispatcher {
    fn drop(&mut self) {
        self.finish(false);
        release_slide_lock();
    }
}

pub fn start_slide(
    request: SlideRequest,
    duration: Duration,
    style: AnimationStyle,
) -> color_eyre::eyre::Result<()> {
    // Set this before the animation thread runs. Show/Uncloak from restore are
    // queued while the command still holds the window-manager lock, and they
    // must already be ignored when that lock is released.
    let hwnds = request
        .outgoing
        .iter()
        .chain(request.incoming.iter())
        .copied()
        .collect::<Vec<_>>();
    suppress_slide_events(&hwnds, duration + Duration::from_millis(1500));
    let slide_id = NEXT_SLIDE_ID.fetch_add(1, Ordering::SeqCst);
    drive_slide_windows(&hwnds, slide_id);
    SLIDE_ACTIVE.fetch_add(1, Ordering::SeqCst);

    let dispatcher = WorkspaceSlideDispatcher::new(request, style, slide_id);
    // Place windows before returning. The caller's layout update would otherwise
    // start a movement animation and pull them back to the destination.
    dispatcher.prepare();
    AnimationEngine::animate(dispatcher, duration)
}

#[cfg(test)]
mod tests {
    use super::slide_offset;

    #[test]
    fn slide_offset_follows_index_direction() {
        assert_eq!(slide_offset(0, 2, 1920), 1920);
        assert_eq!(slide_offset(3, 1, 1920), -1920);
        assert_eq!(slide_offset(1, 1, 1920), 0);
    }

    #[test]
    fn clip_window_stays_inside_the_monitor() {
        use super::clip_window;
        use crate::core::Rect;

        let monitor = Rect {
            left: 0,
            top: 0,
            right: 1920,
            bottom: 1080,
        };
        let inside = Rect {
            left: 100,
            top: 10,
            right: 400,
            bottom: 300,
        };
        let clipped = clip_window(inside, monitor).expect("window is on the monitor");
        assert_eq!(clipped.host, inside);
        assert_eq!(clipped.source_x, 0);
        assert_eq!(clipped.source_width, 400);

        let hanging_left = Rect {
            left: -200,
            top: 10,
            right: 800,
            bottom: 600,
        };
        let clipped = clip_window(hanging_left, monitor).expect("part of the window is visible");
        assert_eq!(clipped.host.left, 0);
        assert_eq!(clipped.host.right, 600);
        assert_eq!(clipped.source_x, 200);

        let other_monitor = Rect {
            left: 2000,
            top: 10,
            right: 800,
            bottom: 600,
        };
        assert!(clip_window(other_monitor, monitor).is_none());
    }

    #[test]
    fn clip_thumbnail_scales_into_the_real_window() {
        use super::clip_thumbnail;
        use crate::core::Rect;

        let monitor = Rect {
            left: 0,
            top: 0,
            right: 1920,
            bottom: 1080,
        };
        // Visual is half the post-layout window and hangs 100px off the left.
        let visual = Rect {
            left: -100,
            top: 0,
            right: 400,
            bottom: 200,
        };
        let clipped = clip_thumbnail(visual, monitor, 800, 400).expect("part is visible");
        assert_eq!(clipped.host.left, 0);
        assert_eq!(clipped.host.right, 300);
        assert_eq!(clipped.source_x, 200);
        assert_eq!(clipped.source_width, 600);
        assert_eq!(clipped.source_height, 400);
    }
}
