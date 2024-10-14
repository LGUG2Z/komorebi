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
pub struct Animation {}

impl Animation {
    /// Returns true if the animation needs to continue
    pub fn cancel(animation_key: &str) -> bool {
        if !ANIMATION_MANAGER.lock().in_progress(animation_key) {
            return true;
        }

        // should be more than 0
        let cancel_idx = ANIMATION_MANAGER.lock().init_cancel(animation_key);
        let max_duration = Duration::from_secs(1);
        let spent_duration = Instant::now();

        while ANIMATION_MANAGER.lock().in_progress(animation_key) {
            if spent_duration.elapsed() >= max_duration {
                ANIMATION_MANAGER.lock().end(animation_key);
            }

            std::thread::sleep(Duration::from_millis(
                ANIMATION_DURATION.load(Ordering::SeqCst) / 2,
            ));
        }

        let latest_cancel_idx = ANIMATION_MANAGER.lock().latest_cancel_idx(animation_key);

        ANIMATION_MANAGER.lock().end_cancel(animation_key);

        latest_cancel_idx == cancel_idx
    }

    #[allow(clippy::cast_precision_loss)]
    pub fn animate(
        animation_key: &str,
        duration: Duration,
        mut render_callback: impl FnMut(f64) -> Result<()>,
    ) -> Result<()> {
        if ANIMATION_MANAGER.lock().in_progress(animation_key) {
            let should_animate = Self::cancel(animation_key);

            if !should_animate {
                return Ok(());
            }
        }

        ANIMATION_MANAGER.lock().start(animation_key);

        let target_frame_time = Duration::from_millis(1000 / ANIMATION_FPS.load(Ordering::Relaxed));
        let mut progress = 0.0;
        let animation_start = Instant::now();

        // start animation
        while progress < 1.0 {
            // check if animation is cancelled
            if ANIMATION_MANAGER.lock().is_cancelled(animation_key) {
                // cancel animation
                ANIMATION_MANAGER.lock().cancel(animation_key);
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

        ANIMATION_MANAGER.lock().end(animation_key);

        // limit progress to 1.0 if animation took longer
        if progress > 1.0 {
            progress = 1.0;
        }

        // process animation for 1.0 to set target position
        render_callback(progress)
    }
}
