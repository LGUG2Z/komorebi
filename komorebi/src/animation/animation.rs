use color_eyre::Result;

use schemars::JsonSchema;

use serde::Deserialize;
use serde::Serialize;
use std::sync::atomic::Ordering;
use std::time::Duration;
use std::time::Instant;

use super::ANIMATION_DURATION;
use super::ANIMATION_FPS;
use super::ANIMATION_MANAGER;

#[derive(Debug, Default, Clone, Copy, Serialize, Deserialize, JsonSchema, PartialEq)]
pub struct Animation {
    pub hwnd: isize,
}

impl Animation {
    pub fn new(hwnd: isize) -> Self {
        Self { hwnd }
    }

    /// Returns true if the animation needs to continue
    pub fn cancel(&mut self) -> bool {
        if !ANIMATION_MANAGER.lock().in_progress(self.hwnd) {
            return true;
        }

        // should be more than 0
        let cancel_idx = ANIMATION_MANAGER.lock().init_cancel(self.hwnd);
        let max_duration = Duration::from_secs(1);
        let spent_duration = Instant::now();

        while ANIMATION_MANAGER.lock().in_progress(self.hwnd) {
            if spent_duration.elapsed() >= max_duration {
                ANIMATION_MANAGER.lock().end(self.hwnd);
            }

            std::thread::sleep(Duration::from_millis(
                ANIMATION_DURATION.load(Ordering::SeqCst) / 2,
            ));
        }

        let latest_cancel_idx = ANIMATION_MANAGER.lock().latest_cancel_idx(self.hwnd);

        ANIMATION_MANAGER.lock().end_cancel(self.hwnd);

        latest_cancel_idx == cancel_idx
    }

    #[allow(clippy::cast_precision_loss)]
    pub fn animate(
        &mut self,
        duration: Duration,
        mut render_callback: impl FnMut(f64) -> Result<()>,
    ) -> Result<()> {
        if ANIMATION_MANAGER.lock().in_progress(self.hwnd) {
            let should_animate = self.cancel();

            if !should_animate {
                return Ok(());
            }
        }

        ANIMATION_MANAGER.lock().start(self.hwnd);

        let target_frame_time = Duration::from_millis(1000 / ANIMATION_FPS.load(Ordering::Relaxed));
        let mut progress = 0.0;
        let animation_start = Instant::now();

        // start animation
        while progress < 1.0 {
            // check if animation is cancelled
            if ANIMATION_MANAGER.lock().is_cancelled(self.hwnd) {
                // cancel animation
                ANIMATION_MANAGER.lock().cancel(self.hwnd);
                return Ok(());
            }

            let frame_start = Instant::now();
            // calculate progress
            progress = animation_start.elapsed().as_millis() as f64 / duration.as_millis() as f64;
            render_callback(progress).ok();

            // sleep until next frame
            let frame_time_elapsed = frame_start.elapsed();

            if frame_time_elapsed < target_frame_time {
                std::thread::sleep(target_frame_time - frame_time_elapsed);
            }
        }

        ANIMATION_MANAGER.lock().end(self.hwnd);

        // limit progress to 1.0 if animation took longer
        if progress > 1.0 {
            progress = 1.0;
        }

        // process animation for 1.0 to set target position
        render_callback(progress)
    }
}
