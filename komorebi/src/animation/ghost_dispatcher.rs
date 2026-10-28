//! The animation of one window by a ghost: a picture of the window is moved
//! and scaled in its place while the window itself, cloaked, is laid out at
//! its new size, and then shown again under the ghost as it dissolves.
//!
//! Windows of a workspace that is switched are slid with ghosts too, out of
//! and into the monitor. Their ghosts are cut off at the edge of the monitor
//! so that none of them is drawn on a monitor next to it.

use crate::AnimationStyle;
use crate::animation::ANIMATION_MANAGER;
use crate::animation::GHOST_MOVEMENT_ENABLED;
use crate::animation::RenderDispatcher;
use crate::animation::ghost;
use crate::animation::ghost::GhostWindow;
use crate::animation::lerp::Lerp;
use crate::animation::prefix::AnimationPrefix;
use crate::animation::prefix::new_animation_key;
use crate::animation::snapshot;
use crate::animation::snapshot::Snapshot;
use crate::animation::transition;
use crate::border_manager;
use crate::com::SetCloak;
use crate::core::Rect;
use crate::focus_manager;
use crate::stackbar_manager;
use crate::transparency_manager;
use crate::window::Window;
use crate::windows_api::WindowsApi;
use color_eyre::eyre;
use parking_lot::Mutex;
use std::sync::atomic::AtomicBool;
use std::sync::atomic::Ordering;
use std::thread;

/// What the window is made to do by its ghost
#[derive(Clone, Copy, Debug, PartialEq)]
enum Mode {
    /// Move to another place, and be shown there again when it has got ready
    Move,
    /// Slide out of the monitor and be hidden: the window is hidden as soon as
    /// its ghost is up, and not shown again. None of the ghost is drawn
    /// outside `clip`.
    SlideOut { clip: Rect },
    /// Slide into the monitor from where it is hidden, and be shown when it
    /// has got most of the way. None of the ghost is drawn outside `clip`.
    SlideIn { clip: Rect },
}

/// How far through a slide into a monitor the window is shown, and the ghost
/// starts to fade. It is later than for a move because the ghost has all of
/// the way in to go, and the window would be where it is seen to be only at
/// the end.
const SLIDE_IN_REVEAL_AT: f64 = 0.85;

pub(crate) struct GhostDispatcher {
    mode: Mode,
    hwnd: isize,
    start_rect: Rect,
    target_rect: Rect,
    top: bool,
    style: AnimationStyle,
    /// Some between successful pre_render and post_render/cleanup_on_cancel when
    /// ghost movement is active. None for the legacy code path.
    ghost: Mutex<Option<GhostWindow>>,
    /// Tracks whether the source has been cloaked so cleanup can uncloak idempotently.
    cloaked: AtomicBool,
    /// Last lerped logical rect actually applied; used by cleanup_on_cancel to
    /// snap the real window to the position the user was last seeing.
    last_animated_rect: Mutex<Rect>,
    /// Resizing the cloaked source to its target, which is done while the
    /// animation runs because it can take a while.
    resize: Mutex<Option<thread::JoinHandle<eyre::Result<()>>>>,
    /// Whether the source has been shown again, under the fading ghost
    revealed: AtomicBool,
    /// How far through the animation the source was shown again, which is
    /// when the ghost starts to fade
    revealed_at: Mutex<f64>,
    /// Whether the source is laid out once it has been shown, and not before: a
    /// window that renders through a swap chain is not laid out while it is hidden
    resize_after_reveal: AtomicBool,
}

impl GhostDispatcher {
    pub(crate) const PREFIX: AnimationPrefix = AnimationPrefix::Movement;

    pub(crate) fn new(
        hwnd: isize,
        start_rect: Rect,
        target_rect: Rect,
        top: bool,
        style: AnimationStyle,
    ) -> Self {
        Self::with_mode(Mode::Move, hwnd, start_rect, target_rect, top, style)
    }

    /// A window that slides from where it is to `target_rect`, which is out of
    /// the monitor, and is hidden
    pub(crate) fn slide_out(
        hwnd: isize,
        start_rect: Rect,
        target_rect: Rect,
        clip: Rect,
        style: AnimationStyle,
    ) -> Self {
        Self::with_mode(
            Mode::SlideOut { clip },
            hwnd,
            start_rect,
            target_rect,
            false,
            style,
        )
    }

    /// A window that is hidden, and slides from `start_rect`, which is out of
    /// the monitor, to `target_rect`, where it is shown
    pub(crate) fn slide_in(
        hwnd: isize,
        start_rect: Rect,
        target_rect: Rect,
        top: bool,
        clip: Rect,
        style: AnimationStyle,
    ) -> Self {
        Self::with_mode(
            Mode::SlideIn { clip },
            hwnd,
            start_rect,
            target_rect,
            top,
            style,
        )
    }

    fn with_mode(
        mode: Mode,
        hwnd: isize,
        start_rect: Rect,
        target_rect: Rect,
        top: bool,
        style: AnimationStyle,
    ) -> Self {
        let revealed_at = match mode {
            Mode::SlideIn { .. } => SLIDE_IN_REVEAL_AT,
            Mode::Move | Mode::SlideOut { .. } => ghost::REVEAL_AT,
        };

        Self {
            mode,
            hwnd,
            start_rect,
            target_rect,
            top,
            style,
            ghost: Mutex::new(None),
            cloaked: AtomicBool::new(false),
            last_animated_rect: Mutex::new(start_rect),
            resize: Mutex::new(None),
            revealed: AtomicBool::new(false),
            revealed_at: Mutex::new(revealed_at),
            resize_after_reveal: AtomicBool::new(false),
        }
    }

    /// What nothing of the ghost is to be drawn outside of
    fn clip(&self) -> Option<Rect> {
        match self.mode {
            Mode::Move => None,
            Mode::SlideOut { clip } | Mode::SlideIn { clip } => Some(clip),
        }
    }

    /// How far through the animation the window can be shown again
    fn reveal_at(&self) -> f64 {
        match self.mode {
            Mode::SlideIn { .. } => SLIDE_IN_REVEAL_AT,
            Mode::Move | Mode::SlideOut { .. } => ghost::REVEAL_AT,
        }
    }

    /// Whether the window is shown again before the animation is over, which
    /// the one that is hidden by it never is
    fn reveals(&self) -> bool {
        !matches!(self.mode, Mode::SlideOut { .. })
    }

    fn pre_render_move(&self) {
        if self.use_ghost() {
            // The ghost is a snapshot of the window as it is now, which is
            // taken before anything is done to the window, and shown at the
            // LOGICAL rect (visible content area) the window is in. It does
            // not follow the window, so it shows the old frame for as long as
            // the window takes to lay itself out again, which is what the
            // animation is for.
            match GhostWindow::create(self.hwnd, self.start_rect, None) {
                Ok(ghost) => {
                    *self.ghost.lock() = Some(ghost);
                    border_manager::set_ghosted(self.hwnd, true);

                    // DwmSetWindowAttribute(DWMWA_CLOAK) is rejected with
                    // E_ACCESSDENIED for foreign HWNDs; the undocumented
                    // IApplicationView::SetCloak path used elsewhere does not
                    // have that restriction.
                    SetCloak(Window { hwnd: self.hwnd }.hwnd(), 1, 2);
                    self.cloaked.store(true, Ordering::SeqCst);

                    // The window is resized now rather than when the animation
                    // is over, so that it has the animation to paint at its new
                    // size in, while it cannot be seen.
                    self.start_resize();
                }
                Err(error) => {
                    tracing::warn!(
                        "ghost movement: failed to create ghost for hwnd {}: {error}; \
                         falling back to legacy path",
                        self.hwnd
                    );
                }
            }
        }
    }

    fn pre_render_slide_out(&self, clip: Rect) {
        if self.use_ghost() {
            // the picture is taken of the window as it is, and its ghost is up
            // before the window is hidden so that nothing flickers in between
            let start = std::time::Instant::now();
            let ghost = Snapshot::capture(self.hwnd).and_then(|picture| {
                // kept for when the window is back, which is not to be asked for a
                // picture of itself then, as it is hidden
                snapshot::remember(self.hwnd, &picture);

                GhostWindow::create_from(picture, self.hwnd, self.start_rect, Some(clip), start)
            });

            match ghost {
                Ok(ghost) => *self.ghost.lock() = Some(ghost),
                Err(error) => tracing::warn!(
                    "workspace transition: failed to create ghost for hwnd {}: {error}; \
                     hiding the window without it",
                    self.hwnd
                ),
            }
        }

        self.hide_exited();
    }

    fn pre_render_slide_in(&self, clip: Rect) {
        // its border has been held since it was put on this animation
        if self.use_ghost()
            && let Some(picture) = self.picture_of_entering_window()
        {
            let start = std::time::Instant::now();

            match GhostWindow::create_from(picture, self.hwnd, self.start_rect, Some(clip), start) {
                Ok(ghost) => {
                    *self.ghost.lock() = Some(ghost);
                    self.lay_out_entering_window();
                    return;
                }
                Err(error) => tracing::warn!(
                    "workspace transition: failed to create ghost for hwnd {}: {error}; \
                     showing the window at once",
                    self.hwnd
                ),
            }
        }

        self.show_entered_at_once();
    }

    /// What the ghost of a window that is coming in shows. The window is hidden, so
    /// it is not asked to render itself if there is a picture of it from when
    /// it left: asking a hidden Chromium or Electron window to has been seen to
    /// leave it with a stale render target. The windows of other apps that have no
    /// such picture can be asked, and a Chromium window that has none is just shown.
    fn picture_of_entering_window(&self) -> Option<Snapshot> {
        let size = (self.target_rect.right, self.target_rect.bottom);

        if let Some(picture) = snapshot::recall(self.hwnd, size) {
            return Some(picture);
        }

        if ghost::is_swap_chain_window(self.hwnd) {
            tracing::debug!(
                "workspace transition: no picture of hwnd {} from when it left, so it is shown at once",
                self.hwnd
            );
            return None;
        }

        Snapshot::capture(self.hwnd).ok()
    }

    /// Have the window that is coming in at the size it is to have, if it is not
    /// already: a window that is where it is to be is left as it is, as laying it
    /// out again is how Chromium and Electron windows are left with a stale
    /// render target. Those that do have to be laid out are not until they are
    /// shown.
    fn lay_out_entering_window(&self) {
        let is_there =
            WindowsApi::window_rect(self.hwnd).is_ok_and(|rect| rect == self.target_rect);

        if is_there {
            return;
        }

        if ghost::is_swap_chain_window(self.hwnd) {
            self.resize_after_reveal.store(true, Ordering::SeqCst);
        } else {
            self.start_resize();
        }
    }

    fn use_ghost(&self) -> bool {
        // DWM has no thumbnail to show of a window that is minimized or gone
        GHOST_MOVEMENT_ENABLED.load(Ordering::Relaxed)
            && WindowsApi::is_window(self.hwnd)
            && !WindowsApi::is_iconic(self.hwnd)
    }

    fn finalise_managers(&self) {
        if ANIMATION_MANAGER
            .lock()
            .count_in_progress(GhostDispatcher::PREFIX)
            == 0
        {
            if WindowsApi::foreground_window().unwrap_or_default() == self.hwnd {
                focus_manager::send_notification(self.hwnd)
            }

            stackbar_manager::STACKBAR_TEMPORARILY_DISABLED.store(false, Ordering::SeqCst);

            stackbar_manager::send_notification();
            transparency_manager::send_notification();
        }
    }

    fn uncloak(&self) {
        if self.cloaked.swap(false, Ordering::SeqCst) {
            SetCloak(Window { hwnd: self.hwnd }.hwnd(), 1, 0);
        }
    }

    /// Start resizing the cloaked source to its target rect, on a thread of
    /// its own: a window that is slow to take a new size would otherwise hold
    /// up the animation, and the ghost is on screen in its place meanwhile.
    fn start_resize(&self) {
        let hwnd = self.hwnd;
        let target = self.target_rect;
        let top = self.top;

        *self.resize.lock() = Some(thread::spawn(move || {
            let resized = WindowsApi::position_window(hwnd, &target, top, false);

            if resized.is_ok() {
                // A window that is hidden has no reason to lay itself out again
                // before it is shown, and Explorer takes its time over it when
                // it is: have it do it now, while the ghost is on screen.
                snapshot::paint_now(hwnd);

                // Windows that render through a swap chain are given a real
                // size change on the way (see `ghost::nudge_swap_chain_window`)
                // so that their compositor rebuilds its render target at the
                // final size. It takes as long as the window takes to answer
                // twice, which is why it is done here, while the window is
                // hidden, and not in the middle of the animation. It has to be
                // the last thing done to the size of the window before it is
                // shown: rendering the window leaves its compositor as stale as
                // it was, and the window then takes over a hundred
                // milliseconds to show anything.
                if ghost::is_swap_chain_window(hwnd) {
                    ghost::nudge_swap_chain_window(hwnd, &target, top);
                }
            }

            resized
        }));
    }

    /// Whether the resize started by `start_resize` is done, or never was
    fn resize_is_done(&self) -> bool {
        self.resize
            .lock()
            .as_ref()
            .is_none_or(thread::JoinHandle::is_finished)
    }

    /// Wait for the resize started by `start_resize` to be done
    fn finish_resize(&self) -> eyre::Result<()> {
        match self.resize.lock().take() {
            Some(resize) => resize
                .join()
                .unwrap_or_else(|_| Err(eyre::eyre!("the resize of hwnd {} panicked", self.hwnd))),
            None => Ok(()),
        }
    }

    /// Show the source again, at its target, under the ghost, which fades out
    /// over it from here on. By now it has had part of the animation to lay
    /// itself out at its new size, which is why this is not done at the end.
    fn reveal(&self, progress: f64) {
        if self.revealed.swap(true, Ordering::SeqCst) {
            return;
        }

        *self.revealed_at.lock() = progress.clamp(self.reveal_at(), 1.0);

        if let Err(error) = self.finish_resize() {
            // the window cannot be left hidden, and the next layout pass
            // puts it where it belongs
            tracing::warn!(
                "ghost movement: failed to resize hwnd {} to its target: {error}",
                self.hwnd
            );
        }

        self.show();
    }

    fn dispose_ghost(&self) {
        if let Some(ghost) = self.ghost.lock().take() {
            let _ = ghost.dispose();
        }
    }

    /// Show the window, which is to be seen from here on
    fn show(&self) {
        match self.mode {
            Mode::SlideIn { .. } => {
                // it was hidden by the workspace it is on, which is shown
                // again as it is, along with its border, which has been held
                // back because it cannot be cut off at the edge of the monitor
                // as the ghost is
                border_manager::set_held(self.hwnd, false);
                transition::finish_enter(self.hwnd);
                Window { hwnd: self.hwnd }.restore();
                border_manager::send_notification(Some(self.hwnd));

                // which is when it takes the focus it was asked to, if it was
                transition::focus_if_asked(self.hwnd);

                // and laid out, if it is not where it is to be, now that it is
                // shown and there is nothing to be left stale
                if self.resize_after_reveal.swap(false, Ordering::SeqCst) {
                    self.start_resize();
                }
            }
            Mode::Move | Mode::SlideOut { .. } => self.uncloak(),
        }
    }

    /// Hide the window that is slid out, as its ghost is in its place. It is
    /// left to the animation when the workspace is switched, so it is
    /// taken from there first, or hiding it would be left to the animation.
    fn hide_exited(&self) {
        // unless it is on its way back in already, which is not for this to undo
        if transition::is_entering(self.hwnd) {
            return;
        }

        transition::take_exit(self.hwnd);
        Window { hwnd: self.hwnd }.hide();
    }

    /// Show a window that is slid into the monitor at once, where it belongs.
    ///
    /// A window that is there already is only shown. One that renders through a
    /// swap chain is shown before it is laid out, and not after: laying one out
    /// while it is hidden is what leaves it blank or drawn off its place.
    fn show_entered_at_once(&self) {
        let is_there =
            WindowsApi::window_rect(self.hwnd).is_ok_and(|rect| rect == self.target_rect);

        if is_there {
            self.show();
            return;
        }

        if ghost::is_swap_chain_window(self.hwnd) {
            self.show();
            self.place_entered_window();
        } else {
            self.place_entered_window();
            self.show();
        }
    }

    fn place_entered_window(&self) {
        if let Err(error) =
            WindowsApi::position_window(self.hwnd, &self.target_rect, self.top, false)
        {
            tracing::warn!(
                "workspace transition: failed to place hwnd {}: {error}",
                self.hwnd
            );
        }
    }
}

impl RenderDispatcher for GhostDispatcher {
    fn get_animation_key(&self) -> String {
        new_animation_key(GhostDispatcher::PREFIX, self.hwnd.to_string())
    }

    fn pre_render(&self) -> eyre::Result<()> {
        stackbar_manager::STACKBAR_TEMPORARILY_DISABLED.store(true, Ordering::SeqCst);
        stackbar_manager::send_notification();

        match self.mode {
            Mode::Move => self.pre_render_move(),
            Mode::SlideOut { clip } => self.pre_render_slide_out(clip),
            Mode::SlideIn { clip } => self.pre_render_slide_in(clip),
        }

        Ok(())
    }

    fn paces_with_compositor(&self) -> bool {
        self.ghost.lock().is_some()
    }

    fn render(&self, progress: f64) -> eyre::Result<()> {
        let logical = self.start_rect.lerp(self.target_rect, progress, self.style);
        *self.last_animated_rect.lock() = logical;

        let ghost_active = self.ghost.lock().is_some();
        if ghost_active {
            // A window that is slow to take its new size is not waited for: the
            // animation goes on with the ghost as it is, and the window is
            // shown when it is ready, or once the animation is over.
            if self.reveals()
                && progress >= self.reveal_at()
                && !self.revealed.load(Ordering::SeqCst)
                && self.resize_is_done()
            {
                self.reveal(progress);
            }

            let opacity = if self.revealed.load(Ordering::SeqCst) {
                ghost::ghost_opacity(progress, *self.revealed_at.lock())
            } else {
                255
            };

            if let Some(ghost) = self.ghost.lock().as_ref()
                && let Err(error) = ghost.update_clipped(logical, opacity, self.clip())
            {
                tracing::trace!("ghost update failed: {error}");
            }

            // the border of a window that slides in or out of the monitor is not
            // moved with it, as it cannot be cut off at the edge of the monitor
            if matches!(self.mode, Mode::Move) {
                border_manager::animate_to(self.hwnd, logical);
            }
        } else if matches!(self.mode, Mode::Move) {
            // Legacy path: animations always run on a separate thread, so we don't
            // gate on WINDOW_HANDLING_BEHAVIOUR here.
            WindowsApi::move_window(self.hwnd, &logical, false)?;
            WindowsApi::invalidate_rect(self.hwnd, None, false);
        }

        Ok(())
    }

    fn post_render(&self) -> eyre::Result<()> {
        let Some(ghost) = self.ghost.lock().take() else {
            if !matches!(self.mode, Mode::Move) {
                // the window was hidden or shown at once, without a ghost
                self.finalise_managers();
                return Ok(());
            }

            // Legacy path: the final reposition, then one DWM frame's wait so
            // the app's first post-move paint lands.
            let landed = WindowsApi::position_window(self.hwnd, &self.target_rect, self.top, false);
            self.uncloak();
            unsafe {
                let _ = windows::Win32::Graphics::Dwm::DwmFlush();
            }
            self.finalise_managers();
            return landed;
        };

        // Normally done a good while ago, but not for a window that was slow to
        // take its new size, which is waited for now, with the ghost on screen
        // over it, and then faded out of the way a little more quickly than
        // it otherwise would be.
        if self.reveals() && !self.revealed.load(Ordering::SeqCst) {
            self.reveal(1.0);
            ghost.fade_out(self.target_rect, self.clip());
        }

        // a window that was laid out after it was shown is done being by now, or is
        // waited for, so that nothing is left running when the animation is over
        if matches!(self.mode, Mode::SlideIn { .. }) {
            let _ = self.finish_resize();
        }

        border_manager::set_ghosted(self.hwnd, false);
        let _ = ghost.dispose();

        self.finalise_managers();

        Ok(())
    }

    fn cleanup_on_cancel(&self) {
        match self.mode {
            Mode::Move => {}
            Mode::SlideOut { .. } => {
                // all that was left to do is to hide the window
                self.hide_exited();
                self.dispose_ghost();
                self.finalise_managers();
                return;
            }
            Mode::SlideIn { .. } => {
                let _ = self.finish_resize();

                if self.revealed.swap(true, Ordering::SeqCst) {
                    // it was shown, and is where it is
                } else if transition::is_exiting(self.hwnd) {
                    // on its way out again, which is the next to hide it, and
                    // showing it before that would be seen
                    border_manager::set_held(self.hwnd, false);
                } else {
                    self.show_entered_at_once();
                }

                self.dispose_ghost();
                self.finalise_managers();
                return;
            }
        }

        // Snap the real window to wherever the ghost was last drawn so the next
        // dispatcher can capture an accurate start_rect. Then uncloak and tear
        // down the ghost.
        let _ = self.finish_resize();

        if WindowsApi::is_window(self.hwnd) {
            let target = *self.last_animated_rect.lock();

            if let Err(error) = WindowsApi::position_window(self.hwnd, &target, false, false) {
                tracing::warn!(
                    "ghost movement cancel: failed to snap hwnd {} to last rect: {error}",
                    self.hwnd
                );
            }

            self.revealed.store(true, Ordering::SeqCst);
            self.uncloak();
        }

        border_manager::set_ghosted(self.hwnd, false);
        self.dispose_ghost();

        self.finalise_managers();
    }
}
