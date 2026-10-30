use color_eyre::eyre;

use serde::Deserialize;
use serde::Serialize;
use std::cell::RefCell;
use std::sync::atomic::Ordering;
use std::time::Duration;
use std::time::Instant;

use super::ANIMATION_DURATION_GLOBAL;
use super::ANIMATION_FPS;
use super::ANIMATION_MANAGER;
use super::RenderDispatcher;

/// How often to check whether an animation that was asked to stop has done so.
/// An animation notices that it was cancelled once per frame, so waiting much
/// longer than that only delays the animation that replaces it.
const CANCEL_POLL_INTERVAL_MS: u64 = 4;

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

            std::thread::sleep(Duration::from_millis(CANCEL_POLL_INTERVAL_MS));
        }

        let latest_cancel_idx = ANIMATION_MANAGER.lock().latest_cancel_idx(animation_key);

        ANIMATION_MANAGER.lock().end_cancel(animation_key);

        latest_cancel_idx == cancel_idx
    }

    /// Animate with `render_dispatcher` for `duration`.
    ///
    /// Inside an [`AnimationBatch`] the animation is held back and started
    /// together with the other animations of the batch when it ends; outside
    /// one it starts straight away.
    pub fn animate(
        render_dispatcher: impl RenderDispatcher + Send + 'static,
        duration: Duration,
    ) -> eyre::Result<()> {
        let animation = Animation {
            dispatcher: Box::new(render_dispatcher),
            duration,
        };

        let batched = BATCH.with(|batch| match batch.borrow_mut().as_mut() {
            Some(animations) => {
                animations.push(animation);
                None
            }
            None => Some(animation),
        });

        if let Some(animation) = batched {
            Self::run(vec![animation]);
        }

        Ok(())
    }

    /// Run animations together on one thread, one frame at a time.
    ///
    /// Every animation is prepared before the first frame is drawn, so that
    /// they all start from the same moment, and every frame of all of them is
    /// drawn before the compositor is asked to present it, so that they are
    /// seen to move together. Animating the windows of a layout one thread at
    /// a time lets each of them be a little ahead of or behind the others.
    #[allow(clippy::cast_precision_loss)]
    fn run(animations: Vec<Animation>) {
        std::thread::spawn(move || {
            // Preparing an animation can take a while (a ghost takes a picture
            // of its window), and they are all prepared at once so that the
            // time it takes is that of the slowest rather than of all of them.
            let pending = std::thread::scope(|scope| {
                animations
                    .into_iter()
                    .map(|animation| {
                        scope.spawn(move || {
                            let key = animation.dispatcher.get_animation_key();
                            if ANIMATION_MANAGER.lock().in_progress(key.as_str()) {
                                let should_animate = Self::cancel(key.as_str());

                                if !should_animate {
                                    return None;
                                }
                            }

                            if let Err(error) = animation.dispatcher.pre_render() {
                                tracing::warn!("animation {key} could not be prepared: {error}");
                                return None;
                            }

                            Some((key, animation))
                        })
                    })
                    .collect::<Vec<_>>()
                    .into_iter()
                    .filter_map(|prepared| prepared.join().ok().flatten())
                    .collect::<Vec<_>>()
            });
            let mut pending = pending;

            for (key, _) in &pending {
                ANIMATION_MANAGER.lock().start(key.as_str());
            }

            let target_frame_time =
                Duration::from_millis(1000 / ANIMATION_FPS.load(Ordering::Relaxed));
            let animation_start = Instant::now();

            let mut stats = FrameStats::default();
            let mut previous_frame_start: Option<Instant> = None;

            while !pending.is_empty() {
                // check if any animation is cancelled
                pending.retain(|(key, animation)| {
                    if ANIMATION_MANAGER.lock().is_cancelled(key.as_str()) {
                        ANIMATION_MANAGER.lock().cancel(key.as_str());
                        animation.dispatcher.cleanup_on_cancel();
                        false
                    } else {
                        true
                    }
                });

                let frame_start = Instant::now();
                let elapsed = animation_start.elapsed().as_millis() as f64;
                let interval = previous_frame_start.map(|previous| frame_start - previous);
                previous_frame_start = Some(frame_start);

                let mut finished = Vec::new();
                let mut remaining = Vec::with_capacity(pending.len());
                for (key, animation) in pending {
                    let progress = elapsed / animation.duration.as_millis() as f64;
                    if progress.is_nan() || progress >= 1.0 {
                        finished.push((key, animation));
                    } else {
                        animation.dispatcher.render(progress).ok();
                        remaining.push((key, animation));
                    }
                }
                pending = remaining;

                stats.add(frame_start.elapsed(), interval, target_frame_time);

                Self::finish(finished);

                // wait for the frame to be presented
                if pending
                    .iter()
                    .any(|(_, animation)| animation.dispatcher.paces_with_compositor())
                {
                    unsafe {
                        let _ = windows::Win32::Graphics::Dwm::DwmFlush();
                    }
                }

                // then for the rest of the time the frame is meant to take
                let frame_time_elapsed = frame_start.elapsed();
                if !pending.is_empty() && frame_time_elapsed < target_frame_time {
                    std::thread::sleep(target_frame_time - frame_time_elapsed);
                }
            }

            tracing::debug!("animations: {stats}");
        });
    }

    /// Land animations that have reached their end.
    ///
    /// Each lands on a thread of its own, drawing its last frame there too: landing one means
    /// calling into its window, which can be slow or never come back, and that is not to keep
    /// the others from landing. A batch that is not landed by `LANDING_DEADLINE` is left to the
    /// threads that have not come back, so that what comes after it is not kept waiting.
    fn finish(animations: Vec<(String, Animation)>) {
        for (key, _) in &animations {
            ANIMATION_MANAGER.lock().end(key.as_str());
        }

        let (landed_tx, landed_rx) = std::sync::mpsc::channel();
        let count = animations.len();
        let keys: Vec<String> = animations.iter().map(|(key, _)| key.clone()).collect();

        for (key, animation) in animations {
            let landed_tx = landed_tx.clone();

            std::thread::spawn(move || {
                tracing::trace!("landing {key}: drawing the last frame");
                // limit progress to 1.0 if animation took longer, and process
                // the animation for 1.0 to set the target position
                animation.dispatcher.render(1.0).ok();

                tracing::trace!("landing {key}: finishing");
                if let Err(error) = animation.dispatcher.post_render() {
                    tracing::warn!("animation {key} could not be completed: {error}");
                }

                tracing::trace!("landing {key}: landed");
                let _ = landed_tx.send(key);
            });
        }

        drop(landed_tx);

        let deadline = Instant::now() + LANDING_DEADLINE;
        let mut landed = 0;
        let mut not_landed = keys;

        while landed < count {
            match landed_rx.recv_timeout(deadline.saturating_duration_since(Instant::now())) {
                Ok(key) => {
                    landed += 1;
                    not_landed.retain(|not_landed| *not_landed != key);
                }
                Err(_) => break,
            }
        }

        if landed < count {
            tracing::warn!(
                "{} of {count} animations did not land in {} ms; they are left to land on their own: {}",
                count - landed,
                LANDING_DEADLINE.as_millis(),
                not_landed.join(", ")
            );
        }
    }
}

/// How long a batch of animations is waited for to land
const LANDING_DEADLINE: Duration = Duration::from_secs(2);

/// What the frames of a group of animations cost, to see whether they keep up
#[derive(Default)]
struct FrameStats {
    frames: u32,
    work: Duration,
    longest_work: Duration,
    intervals: u32,
    interval: Duration,
    longest_interval: Duration,
    late: u32,
    target: Duration,
}

impl FrameStats {
    /// `work` is what drawing the frame took, `interval` the time from the
    /// start of the frame before it
    fn add(&mut self, work: Duration, interval: Option<Duration>, target: Duration) {
        self.frames += 1;
        self.work += work;
        self.longest_work = self.longest_work.max(work);
        self.target = target;

        if let Some(interval) = interval {
            self.intervals += 1;
            self.interval += interval;
            self.longest_interval = self.longest_interval.max(interval);

            if interval > target + target / 2 {
                self.late += 1;
            }
        }
    }
}

impl std::fmt::Display for FrameStats {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let ms = |duration: Duration| duration.as_secs_f64() * 1000.0;
        let average = |total: Duration, count: u32| ms(total) / f64::from(count.max(1));

        write!(
            f,
            "{} frames, drawing took {:.2} ms on average and {:.2} ms at most, frames came \
             every {:.2} ms on average and {:.2} ms at most, {} of them more than half as \
             late again as the {:.0} ms they are meant to take",
            self.frames,
            average(self.work, self.frames),
            ms(self.longest_work),
            average(self.interval, self.intervals),
            ms(self.longest_interval),
            self.late,
            ms(self.target),
        )
    }
}

struct Animation {
    dispatcher: Box<dyn RenderDispatcher + Send>,
    duration: Duration,
}

thread_local! {
    static BATCH: RefCell<Option<Vec<Animation>>> = const { RefCell::new(None) };
}

/// Collects the animations started on this thread while it is alive, and
/// starts them together once it is dropped.
///
/// Batches nest; the animations are started when the outermost one ends.
pub struct AnimationBatch {
    outermost: bool,
}

impl AnimationBatch {
    pub fn begin() -> Self {
        let outermost = BATCH.with(|batch| {
            let mut batch = batch.borrow_mut();
            if batch.is_none() {
                *batch = Some(Vec::new());
                true
            } else {
                false
            }
        });

        Self { outermost }
    }
}

impl Drop for AnimationBatch {
    fn drop(&mut self) {
        if !self.outermost {
            return;
        }

        let animations = BATCH.with(|batch| batch.borrow_mut().take().unwrap_or_default());
        if !animations.is_empty() {
            AnimationEngine::run(animations);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use parking_lot::Mutex;
    use std::sync::Arc;

    struct Recorder {
        name: &'static str,
        events: Arc<Mutex<Vec<String>>>,
    }

    impl RenderDispatcher for Recorder {
        fn get_animation_key(&self) -> String {
            format!("engine-test-{}", self.name)
        }

        fn pre_render(&self) -> eyre::Result<()> {
            self.events.lock().push(format!("pre {}", self.name));
            Ok(())
        }

        fn render(&self, _progress: f64) -> eyre::Result<()> {
            self.events.lock().push(format!("render {}", self.name));
            Ok(())
        }

        fn post_render(&self) -> eyre::Result<()> {
            self.events.lock().push(format!("post {}", self.name));
            Ok(())
        }

        fn cleanup_on_cancel(&self) {
            self.events.lock().push(format!("cancel {}", self.name));
        }
    }

    fn recorder(name: &'static str, events: &Arc<Mutex<Vec<String>>>) -> Recorder {
        Recorder {
            name,
            events: events.clone(),
        }
    }

    fn wait_for(events: &Arc<Mutex<Vec<String>>>, event: &str) {
        let start = Instant::now();
        while !events.lock().iter().any(|e| e == event) {
            assert!(
                start.elapsed() < Duration::from_secs(5),
                "timed out waiting for '{event}', saw {:?}",
                events.lock()
            );
            std::thread::sleep(Duration::from_millis(5));
        }
    }

    #[test]
    fn batched_animations_are_prepared_before_any_is_drawn() {
        let events = Arc::new(Mutex::new(Vec::new()));

        {
            let _batch = AnimationBatch::begin();
            AnimationEngine::animate(recorder("batch-a", &events), Duration::from_millis(80))
                .unwrap();
            AnimationEngine::animate(recorder("batch-b", &events), Duration::from_millis(80))
                .unwrap();

            // nothing starts until the batch ends
            std::thread::sleep(Duration::from_millis(50));
            assert!(events.lock().is_empty());
        }

        wait_for(&events, "post batch-a");
        wait_for(&events, "post batch-b");

        let events = events.lock();
        let first_render = events
            .iter()
            .position(|e| e.starts_with("render"))
            .expect("the animations were drawn");
        assert!(events[..first_render].contains(&"pre batch-a".to_string()));
        assert!(events[..first_render].contains(&"pre batch-b".to_string()));

        // both are drawn in every frame, not one after the other
        let first_a = events.iter().position(|e| e == "render batch-a").unwrap();
        let first_b = events.iter().position(|e| e == "render batch-b").unwrap();
        assert!(first_a.abs_diff(first_b) <= 1);

        // and both finish at their target
        assert_eq!(events.iter().filter(|e| *e == "post batch-a").count(), 1);
        assert_eq!(events.iter().filter(|e| *e == "post batch-b").count(), 1);
    }

    #[test]
    fn nested_batches_start_when_the_outermost_one_ends() {
        let events = Arc::new(Mutex::new(Vec::new()));

        {
            let _outer = AnimationBatch::begin();
            {
                let _inner = AnimationBatch::begin();
                AnimationEngine::animate(recorder("nested", &events), Duration::from_millis(20))
                    .unwrap();
            }

            std::thread::sleep(Duration::from_millis(50));
            assert!(events.lock().is_empty());
        }

        wait_for(&events, "post nested");
    }

    /// Lands never: its last frame does not come back
    struct Stuck(Recorder);

    impl RenderDispatcher for Stuck {
        fn get_animation_key(&self) -> String {
            self.0.get_animation_key()
        }

        fn pre_render(&self) -> eyre::Result<()> {
            self.0.pre_render()
        }

        fn render(&self, progress: f64) -> eyre::Result<()> {
            if progress >= 1.0 {
                std::thread::sleep(Duration::from_secs(3600));
            }
            Ok(())
        }

        fn post_render(&self) -> eyre::Result<()> {
            self.0.post_render()
        }

        fn cleanup_on_cancel(&self) {}
    }

    #[test]
    fn an_animation_that_does_not_come_back_does_not_keep_the_others_from_landing() {
        let events = Arc::new(Mutex::new(Vec::new()));

        {
            let _batch = AnimationBatch::begin();
            AnimationEngine::animate(Stuck(recorder("stuck", &events)), Duration::from_millis(20))
                .unwrap();
            AnimationEngine::animate(recorder("fine", &events), Duration::from_millis(20)).unwrap();
        }

        wait_for(&events, "post fine");
        assert!(!events.lock().iter().any(|e| e == "post stuck"));
    }

    #[test]
    fn animations_outside_a_batch_start_straight_away() {
        let events = Arc::new(Mutex::new(Vec::new()));

        AnimationEngine::animate(recorder("alone", &events), Duration::from_millis(20)).unwrap();

        wait_for(&events, "post alone");
    }

    #[test]
    fn an_animation_without_a_duration_still_ends() {
        let events = Arc::new(Mutex::new(Vec::new()));

        AnimationEngine::animate(recorder("instant", &events), Duration::ZERO).unwrap();

        wait_for(&events, "post instant");
    }

    #[test]
    fn a_newer_animation_replaces_a_running_one_without_waiting_long() {
        let events = Arc::new(Mutex::new(Vec::new()));

        AnimationEngine::animate(recorder("replaced", &events), Duration::from_secs(30)).unwrap();
        wait_for(&events, "pre replaced");

        let replaced_at = Instant::now();
        AnimationEngine::animate(recorder("replaced", &events), Duration::from_millis(20)).unwrap();

        wait_for(&events, "cancel replaced");
        wait_for(&events, "post replaced");

        assert!(replaced_at.elapsed() < Duration::from_secs(2));

        let events = events.lock();
        let cancelled = events.iter().position(|e| e == "cancel replaced").unwrap();
        let posted = events.iter().position(|e| e == "post replaced").unwrap();
        assert!(cancelled < posted);
        assert_eq!(events.iter().filter(|e| *e == "pre replaced").count(), 2);
    }
}
