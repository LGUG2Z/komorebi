//! The animation of one window by a ghost: a picture of the window is moved
//! and scaled in its place while the window itself, cloaked, is laid out at
//! its new size, and then shown again under the ghost as it dissolves.

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

pub(crate) struct GhostDispatcher {
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
        Self {
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
            revealed_at: Mutex::new(ghost::REVEAL_AT),
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

        *self.revealed_at.lock() = progress.clamp(ghost::REVEAL_AT, 1.0);

        if let Err(error) = self.finish_resize() {
            // the window cannot be left hidden, and the next layout pass
            // puts it where it belongs
            tracing::warn!(
                "ghost movement: failed to resize hwnd {} to its target: {error}",
                self.hwnd
            );
        }

        self.uncloak();
    }
}

impl RenderDispatcher for GhostDispatcher {
    fn get_animation_key(&self) -> String {
        new_animation_key(GhostDispatcher::PREFIX, self.hwnd.to_string())
    }

    fn pre_render(&self) -> eyre::Result<()> {
        stackbar_manager::STACKBAR_TEMPORARILY_DISABLED.store(true, Ordering::SeqCst);
        stackbar_manager::send_notification();

        if self.use_ghost() {
            // The ghost is a snapshot of the window as it is now, which is
            // taken before anything is done to the window, and shown at the
            // LOGICAL rect (visible content area) the window is in. It does
            // not follow the window, so it shows the old frame for as long as
            // the window takes to lay itself out again, which is what the
            // animation is for.
            match GhostWindow::create(self.hwnd, self.start_rect) {
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
            if progress >= ghost::REVEAL_AT
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
                && let Err(error) = ghost.update(logical, opacity)
            {
                tracing::trace!("ghost update failed: {error}");
            }
            border_manager::animate_to(self.hwnd, logical);
        } else {
            // Legacy path: animations always run on a separate thread, so we don't
            // gate on WINDOW_HANDLING_BEHAVIOUR here.
            WindowsApi::move_window(self.hwnd, &logical, false)?;
            WindowsApi::invalidate_rect(self.hwnd, None, false);
        }

        Ok(())
    }

    fn post_render(&self) -> eyre::Result<()> {
        let Some(ghost) = self.ghost.lock().take() else {
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
        if !self.revealed.load(Ordering::SeqCst) {
            self.reveal(1.0);
            ghost.fade_out(self.target_rect);
        }

        border_manager::set_ghosted(self.hwnd, false);
        let _ = ghost.dispose();

        self.finalise_managers();

        Ok(())
    }

    fn cleanup_on_cancel(&self) {
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
        if let Some(ghost) = self.ghost.lock().take() {
            let _ = ghost.dispose();
        }

        self.finalise_managers();
    }
}
