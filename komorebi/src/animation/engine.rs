use color_eyre::Result;

use serde::Deserialize;
use serde::Serialize;
use std::sync::atomic::Ordering;
use std::time::Duration;
use std::time::Instant;

use super::ANIMATION_DURATION_GLOBAL;
use super::ANIMATION_FPS;
use super::ANIMATION_MANAGER;
use super::RenderDispatcher;

#[derive(Debug, Default, Clone, Copy, Serialize, Deserialize, PartialEq)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
pub struct AnimationEngine;

impl AnimationEngine {
    pub fn wait_for_all_animations() {
        let max_duration = Duration::from_secs(20);
        let spent_duration = Instant::now();

        while ANIMATION_MANAGER.lock().count() > 0 {
            if spent_duration.elapsed() >= max_duration {
                break;
            }

            std::thread::sleep(Duration::from_millis(
                ANIMATION_DURATION_GLOBAL.load(Ordering::SeqCst),
            ));
        }
    }

    /// Returns true if the animation needs to continue
    pub fn cancel(animation_key: &str) -> bool {
        // should be more than 0
        let cancel_idx = ANIMATION_MANAGER.lock().init_cancel(animation_key);
        let max_duration = Duration::from_secs(5);
        let spent_duration = Instant::now();

        while ANIMATION_MANAGER.lock().in_progress(animation_key) {
            if spent_duration.elapsed() >= max_duration {
                ANIMATION_MANAGER.lock().end(animation_key);
            }

            std::thread::sleep(Duration::from_millis(250 / 2));
        }

        let latest_cancel_idx = ANIMATION_MANAGER.lock().latest_cancel_idx(animation_key);

        ANIMATION_MANAGER.lock().end_cancel(animation_key);

        latest_cancel_idx == cancel_idx
    }

    #[allow(clippy::cast_precision_loss)]
    pub fn animate(
        render_dispatcher: impl RenderDispatcher + Send + 'static,
        duration: Duration,
    ) -> Result<()> {
        std::thread::spawn(move || {
            let animation_key = render_dispatcher.get_animation_key();
            if ANIMATION_MANAGER.lock().in_progress(animation_key.as_str()) {
                let should_animate = Self::cancel(animation_key.as_str());

                if !should_animate {
                    return Ok(());
                }
            }

            render_dispatcher.pre_render()?;

            ANIMATION_MANAGER.lock().start(animation_key.as_str());

            let target_frame_time =
                Duration::from_millis(1000 / ANIMATION_FPS.load(Ordering::Relaxed));
            let mut progress = 0.0;
            let animation_start = Instant::now();

            // start animation
            while progress < 1.0 {
                // check if animation is cancelled
                if ANIMATION_MANAGER
                    .lock()
                    .is_cancelled(animation_key.as_str())
                {
                    // cancel animation
                    ANIMATION_MANAGER.lock().cancel(animation_key.as_str());
                    return Ok(());
                }

                let frame_start = Instant::now();
                // calculate progress
                progress =
                    animation_start.elapsed().as_millis() as f64 / duration.as_millis() as f64;
                render_dispatcher.render(progress).ok();

                // sleep until next frame
                let frame_time_elapsed = frame_start.elapsed();

                if frame_time_elapsed < target_frame_time {
                    std::thread::sleep(target_frame_time - frame_time_elapsed);
                }
            }

            ANIMATION_MANAGER.lock().end(animation_key.as_str());

            // limit progress to 1.0 if animation took longer
            if progress != 1.0 {
                progress = 1.0;

                // process animation for 1.0 to set target position
                render_dispatcher.render(progress).ok();
            }

            render_dispatcher.post_render()
        });

        Ok(())
    }
}
