//! The pure maths of a window animation.
//!
//! Nothing in here touches macOS or reads a clock: every function is handed `now`, so the behaviour
//! can be tested deterministically and the animator is free to decide when a frame happens.

use super::style::apply_ease_func;
use crate::core::Rect;
use crate::core::animation::AnimationStyle;
use std::time::Duration;
use std::time::Instant;

/// A window frame with sub-pixel precision
///
/// Rounding only happens when a frame is committed to the window, so what is committed is always
/// the nearest whole pixel to where the animation has got to.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct FrameF {
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
}

impl From<Rect> for FrameF {
    fn from(rect: Rect) -> Self {
        Self {
            x: f64::from(rect.left),
            y: f64::from(rect.top),
            width: f64::from(rect.right),
            height: f64::from(rect.bottom),
        }
    }
}

impl FrameF {
    #[allow(clippy::cast_possible_truncation)]
    pub fn round(self) -> Rect {
        Rect {
            left: self.x.round() as i32,
            top: self.y.round() as i32,
            right: self.width.round() as i32,
            bottom: self.height.round() as i32,
        }
    }

    pub fn lerp(self, end: Self, t: f64) -> Self {
        Self {
            x: (end.x - self.x).mul_add(t, self.x),
            y: (end.y - self.y).mul_add(t, self.y),
            width: (end.width - self.width).mul_add(t, self.width),
            height: (end.height - self.height).mul_add(t, self.height),
        }
    }
}

/// How long a window has to get where it is going, and how that time is spent
///
/// This is the configured animation duration and style, before it is anchored to a frame and a
/// moment in time.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct MotionSpec {
    pub duration: Duration,
    pub style: AnimationStyle,
}

impl MotionSpec {
    /// Anchor this spec at `current` and `now`
    pub fn begin(self, current: FrameF, now: Instant) -> Motion {
        Motion {
            start: current,
            started: now,
            duration: self.duration,
            style: self.style,
        }
    }

    /// Whether `motion` is what this spec would produce, ignoring where and when it started
    pub fn matches(self, motion: &Motion) -> bool {
        self.duration == motion.duration && self.style == motion.style
    }
}

/// A window on its way from where it started to where it is going
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Motion {
    pub start: FrameF,
    pub started: Instant,
    pub duration: Duration,
    pub style: AnimationStyle,
}

/// The outcome of advancing an animation to a moment in time
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Step {
    pub frame: FrameF,
    pub finished: bool,
}

/// Where a window is at `now`, on its way to `target`
///
/// The animation ends exactly on the target, whatever the easing does on the way there.
pub fn step(motion: &Motion, target: FrameF, now: Instant) -> Step {
    let progress = if motion.duration.is_zero() {
        1.0
    } else {
        (now.saturating_duration_since(motion.started).as_secs_f64()
            / motion.duration.as_secs_f64())
        .clamp(0.0, 1.0)
    };

    if progress >= 1.0 {
        return Step {
            frame: target,
            finished: true,
        };
    }

    Step {
        frame: motion
            .start
            .lerp(target, apply_ease_func(progress, motion.style)),
        finished: false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn frame(x: f64, y: f64, width: f64, height: f64) -> FrameF {
        FrameF {
            x,
            y,
            width,
            height,
        }
    }

    fn spec(millis: u64, style: AnimationStyle) -> MotionSpec {
        MotionSpec {
            duration: Duration::from_millis(millis),
            style,
        }
    }

    #[test]
    fn it_follows_the_easing_and_ends_exactly() {
        let start = frame(0.0, 0.0, 800.0, 600.0);
        let target = frame(100.0, 50.0, 400.0, 300.0);
        let begun = Instant::now();
        let motion = spec(200, AnimationStyle::Linear).begin(start, begun);

        let halfway = step(&motion, target, begun + Duration::from_millis(100));
        assert!(!halfway.finished);
        assert!((halfway.frame.x - 50.0).abs() < 1e-9);
        assert!((halfway.frame.width - 600.0).abs() < 1e-9);

        let done = step(&motion, target, begun + Duration::from_millis(200));
        assert!(done.finished);
        assert_eq!(done.frame, target);

        let late = step(&motion, target, begun + Duration::from_secs(5));
        assert!(late.finished);
        assert_eq!(late.frame, target);
    }

    #[test]
    fn it_starts_where_the_window_is() {
        let start = frame(300.0, -20.0, 800.0, 600.0);
        let begun = Instant::now();
        let motion = spec(200, AnimationStyle::EaseOutCubic).begin(start, begun);

        let first = step(&motion, frame(0.0, 0.0, 100.0, 100.0), begun);
        assert!(!first.finished);
        assert_eq!(first.frame, start);
    }

    #[test]
    fn it_never_overshoots_after_the_end() {
        let start = frame(0.0, 0.0, 800.0, 600.0);
        let target = frame(100.0, 0.0, 800.0, 600.0);
        let begun = Instant::now();
        let motion = spec(100, AnimationStyle::EaseOutBack).begin(start, begun);

        let late = step(&motion, target, begun + Duration::from_millis(250));
        assert_eq!(late.frame, target);
    }

    #[test]
    fn a_zero_duration_finishes_immediately() {
        let start = frame(0.0, 0.0, 800.0, 600.0);
        let target = frame(100.0, 0.0, 800.0, 600.0);
        let begun = Instant::now();
        let motion = spec(0, AnimationStyle::Linear).begin(start, begun);

        assert!(step(&motion, target, begun).finished);
    }

    #[test]
    fn a_clock_that_has_gone_backwards_does_not_panic() {
        let start = frame(0.0, 0.0, 800.0, 600.0);
        let target = frame(100.0, 0.0, 800.0, 600.0);
        let begun = Instant::now() + Duration::from_secs(1);
        let motion = spec(100, AnimationStyle::Linear).begin(start, begun);

        let before = step(&motion, target, begun - Duration::from_millis(500));
        assert_eq!(before.frame, start);
    }

    #[test]
    fn it_agrees_with_the_rect_lerp() {
        use crate::animation::lerp::Lerp;

        let start = Rect {
            left: 0,
            top: 0,
            right: 800,
            bottom: 600,
        };
        let target = Rect {
            left: 300,
            top: 40,
            right: 400,
            bottom: 300,
        };
        let begun = Instant::now();

        for style in [
            AnimationStyle::Linear,
            AnimationStyle::EaseOutCubic,
            AnimationStyle::EaseInOutQuad,
        ] {
            let motion = spec(250, style).begin(start.into(), begun);

            for millis in [0u64, 50, 125, 200, 249] {
                let progress = millis as f64 / 250.0;
                let from_step = step(
                    &motion,
                    target.into(),
                    begun + Duration::from_millis(millis),
                )
                .frame
                .round();

                assert_eq!(from_step, start.lerp(target, progress, style));
            }
        }
    }

    #[test]
    fn specs_match_the_motion_they_began() {
        let now = Instant::now();
        let linear = spec(250, AnimationStyle::Linear);

        assert!(linear.matches(&linear.begin(frame(0.0, 0.0, 1.0, 1.0), now)));
        assert!(
            !spec(300, AnimationStyle::Linear)
                .matches(&linear.begin(frame(0.0, 0.0, 1.0, 1.0), now))
        );
        assert!(
            !spec(250, AnimationStyle::EaseOutSine)
                .matches(&linear.begin(frame(0.0, 0.0, 1.0, 1.0), now))
        );
    }

    #[test]
    fn rounding_happens_only_at_the_edge() {
        assert_eq!(
            frame(0.4, 0.5, 99.6, 10.49).round(),
            Rect {
                left: 0,
                top: 1,
                right: 100,
                bottom: 10,
            }
        );
        assert_eq!(
            frame(-1900.4, -0.6, 640.0, 480.0).round(),
            Rect {
                left: -1900,
                top: -1,
                right: 640,
                bottom: 480,
            }
        );
    }
}
