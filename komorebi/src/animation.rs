use color_eyre::Result;
use komorebi_core::AnimationStyle;
use komorebi_core::Rect;

use schemars::JsonSchema;

use serde::Deserialize;
use serde::Serialize;
use std::f64::consts::PI;
use std::sync::atomic::AtomicU64;
use std::sync::atomic::Ordering;
use std::time::Duration;
use std::time::Instant;

use crate::ANIMATION_DURATION;
use crate::ANIMATION_MANAGER;
use crate::ANIMATION_STYLE;

pub static ANIMATION_FPS: AtomicU64 = AtomicU64::new(60);

pub trait Ease {
    fn evaluate(t: f64) -> f64;
}

pub struct Linear;

impl Ease for Linear {
    fn evaluate(t: f64) -> f64 {
        t
    }
}

pub struct EaseInSine;

impl Ease for EaseInSine {
    fn evaluate(t: f64) -> f64 {
        1.0 - f64::cos((t * PI) / 2.0)
    }
}

pub struct EaseOutSine;

impl Ease for EaseOutSine {
    fn evaluate(t: f64) -> f64 {
        f64::sin((t * PI) / 2.0)
    }
}

pub struct EaseInOutSine;

impl Ease for EaseInOutSine {
    fn evaluate(t: f64) -> f64 {
        -(f64::cos(PI * t) - 1.0) / 2.0
    }
}

pub struct EaseInQuad;

impl Ease for EaseInQuad {
    fn evaluate(t: f64) -> f64 {
        t * t
    }
}

pub struct EaseOutQuad;

impl Ease for EaseOutQuad {
    fn evaluate(t: f64) -> f64 {
        (1.0 - t).mul_add(-1.0 - t, 1.0)
    }
}

pub struct EaseInOutQuad;

impl Ease for EaseInOutQuad {
    fn evaluate(t: f64) -> f64 {
        if t < 0.5 {
            2.0 * t * t
        } else {
            1.0 - (-2.0f64).mul_add(t, 2.0).powi(2) / 2.0
        }
    }
}

pub struct EaseInCubic;

impl Ease for EaseInCubic {
    fn evaluate(t: f64) -> f64 {
        t * t * t
    }
}

pub struct EaseOutCubic;

impl Ease for EaseOutCubic {
    fn evaluate(t: f64) -> f64 {
        1.0 - (1.0 - t).powi(3)
    }
}

pub struct EaseInOutCubic;

impl Ease for EaseInOutCubic {
    fn evaluate(t: f64) -> f64 {
        if t < 0.5 {
            4.0 * t * t * t
        } else {
            1.0 - (-2.0f64).mul_add(t, 2.0).powi(3) / 2.0
        }
    }
}

pub struct EaseInQuart;

impl Ease for EaseInQuart {
    fn evaluate(t: f64) -> f64 {
        t * t * t * t
    }
}

pub struct EaseOutQuart;

impl Ease for EaseOutQuart {
    fn evaluate(t: f64) -> f64 {
        1.0 - (1.0 - t).powi(4)
    }
}

pub struct EaseInOutQuart;

impl Ease for EaseInOutQuart {
    fn evaluate(t: f64) -> f64 {
        if t < 0.5 {
            8.0 * t * t * t * t
        } else {
            1.0 - (-2.0f64).mul_add(t, 2.0).powi(4) / 2.0
        }
    }
}

pub struct EaseInQuint;

impl Ease for EaseInQuint {
    fn evaluate(t: f64) -> f64 {
        t * t * t * t * t
    }
}

pub struct EaseOutQuint;

impl Ease for EaseOutQuint {
    fn evaluate(t: f64) -> f64 {
        1.0 - (1.0 - t).powi(5)
    }
}

pub struct EaseInOutQuint;

impl Ease for EaseInOutQuint {
    fn evaluate(t: f64) -> f64 {
        if t < 0.5 {
            16.0 * t * t * t * t
        } else {
            1.0 - (-2.0f64).mul_add(t, 2.0).powi(5) / 2.0
        }
    }
}

pub struct EaseInExpo;

impl Ease for EaseInExpo {
    fn evaluate(t: f64) -> f64 {
        if t == 0.0 {
            return t;
        }

        10.0f64.mul_add(t, -10.0).exp2()
    }
}

pub struct EaseOutExpo;

impl Ease for EaseOutExpo {
    fn evaluate(t: f64) -> f64 {
        if (t - 1.0).abs() < f64::EPSILON {
            return t;
        }

        1.0 - (-10.0 * t).exp2()
    }
}

pub struct EaseInOutExpo;

impl Ease for EaseInOutExpo {
    fn evaluate(t: f64) -> f64 {
        if t == 0.0 || (t - 1.0).abs() < f64::EPSILON {
            return t;
        }

        if t < 0.5 {
            20.0f64.mul_add(t, -10.0).exp2() / 2.0
        } else {
            (2.0 - (-20.0f64).mul_add(t, 10.0).exp2()) / 2.0
        }
    }
}

pub struct EaseInCirc;

impl Ease for EaseInCirc {
    fn evaluate(t: f64) -> f64 {
        1.0 - f64::sqrt(t.mul_add(-t, 1.0))
    }
}

pub struct EaseOutCirc;

impl Ease for EaseOutCirc {
    fn evaluate(t: f64) -> f64 {
        f64::sqrt((t - 1.0).mul_add(-(t - 1.0), 1.0))
    }
}

pub struct EaseInOutCirc;

impl Ease for EaseInOutCirc {
    fn evaluate(t: f64) -> f64 {
        if t < 0.5 {
            (1.0 - f64::sqrt((2.0 * t).mul_add(-(2.0 * t), 1.0))) / 2.0
        } else {
            (f64::sqrt(
                (-2.0f64)
                    .mul_add(t, 2.0)
                    .mul_add(-(-2.0f64).mul_add(t, 2.0), 1.0),
            ) + 1.0)
                / 2.0
        }
    }
}

pub struct EaseInBack;

impl Ease for EaseInBack {
    fn evaluate(t: f64) -> f64 {
        let c1 = 1.70158;
        let c3 = c1 + 1.0;

        (c3 * t * t).mul_add(t, -c1 * t * t)
    }
}

pub struct EaseOutBack;

impl Ease for EaseOutBack {
    fn evaluate(t: f64) -> f64 {
        let c1: f64 = 1.70158;
        let c3: f64 = c1 + 1.0;

        c1.mul_add((t - 1.0).powi(2), c3.mul_add((t - 1.0).powi(3), 1.0))
    }
}

pub struct EaseInOutBack;

impl Ease for EaseInOutBack {
    fn evaluate(t: f64) -> f64 {
        let c1: f64 = 1.70158;
        let c2: f64 = c1 * 1.525;

        if t < 0.5 {
            ((2.0 * t).powi(2) * ((c2 + 1.0) * 2.0).mul_add(t, -c2)) / 2.0
        } else {
            ((2.0f64.mul_add(t, -2.0))
                .powi(2)
                .mul_add((c2 + 1.0).mul_add(t.mul_add(2.0, -2.0), c2), 2.0))
                / 2.0
        }
    }
}

pub struct EaseInElastic;

impl Ease for EaseInElastic {
    fn evaluate(t: f64) -> f64 {
        if (t - 1.0).abs() < f64::EPSILON || t == 0.0 {
            return t;
        }

        let c4 = (2.0 * PI) / 3.0;

        -(10.0f64.mul_add(t, -10.0).exp2()) * f64::sin(t.mul_add(10.0, -10.75) * c4)
    }
}

pub struct EaseOutElastic;

impl Ease for EaseOutElastic {
    fn evaluate(t: f64) -> f64 {
        if (t - 1.0).abs() < f64::EPSILON || t == 0.0 {
            return t;
        }

        let c4 = (2.0 * PI) / 3.0;

        (-10.0 * t)
            .exp2()
            .mul_add(f64::sin(t.mul_add(10.0, -0.75) * c4), 1.0)
    }
}

pub struct EaseInOutElastic;

impl Ease for EaseInOutElastic {
    fn evaluate(t: f64) -> f64 {
        if (t - 1.0).abs() < f64::EPSILON || t == 0.0 {
            return t;
        }

        let c5 = (2.0 * PI) / 4.5;

        if t < 0.5 {
            -(20.0f64.mul_add(t, -10.0).exp2() * f64::sin(20.0f64.mul_add(t, -11.125) * c5)) / 2.0
        } else {
            ((-20.0f64).mul_add(t, 10.0).exp2() * f64::sin(20.0f64.mul_add(t, -11.125) * c5)) / 2.0
                + 1.0
        }
    }
}

pub struct EaseInBounce;

impl Ease for EaseInBounce {
    fn evaluate(t: f64) -> f64 {
        1.0 - EaseOutBounce::evaluate(1.0 - t)
    }
}

pub struct EaseOutBounce;

impl Ease for EaseOutBounce {
    fn evaluate(t: f64) -> f64 {
        let mut time = t;
        let n1 = 7.5625;
        let d1 = 2.75;

        if t < 1.0 / d1 {
            n1 * time * time
        } else if time < 2.0 / d1 {
            time -= 1.5 / d1;
            (n1 * time).mul_add(time, 0.75)
        } else if time < 2.5 / d1 {
            time -= 2.25 / d1;
            (n1 * time).mul_add(time, 0.9375)
        } else {
            time -= 2.625 / d1;
            (n1 * time).mul_add(time, 0.984_375)
        }
    }
}

pub struct EaseInOutBounce;

impl Ease for EaseInOutBounce {
    fn evaluate(t: f64) -> f64 {
        if t < 0.5 {
            (1.0 - EaseOutBounce::evaluate(2.0f64.mul_add(-t, 1.0))) / 2.0
        } else {
            (1.0 + EaseOutBounce::evaluate(2.0f64.mul_add(t, -1.0))) / 2.0
        }
    }
}
fn apply_ease_func(t: f64) -> f64 {
    let style = *ANIMATION_STYLE.lock();

    match style {
        AnimationStyle::Linear => Linear::evaluate(t),
        AnimationStyle::EaseInSine => EaseInSine::evaluate(t),
        AnimationStyle::EaseOutSine => EaseOutSine::evaluate(t),
        AnimationStyle::EaseInOutSine => EaseInOutSine::evaluate(t),
        AnimationStyle::EaseInQuad => EaseInQuad::evaluate(t),
        AnimationStyle::EaseOutQuad => EaseOutQuad::evaluate(t),
        AnimationStyle::EaseInOutQuad => EaseInOutQuad::evaluate(t),
        AnimationStyle::EaseInCubic => EaseInCubic::evaluate(t),
        AnimationStyle::EaseInOutCubic => EaseInOutCubic::evaluate(t),
        AnimationStyle::EaseInQuart => EaseInQuart::evaluate(t),
        AnimationStyle::EaseOutQuart => EaseOutQuart::evaluate(t),
        AnimationStyle::EaseInOutQuart => EaseInOutQuart::evaluate(t),
        AnimationStyle::EaseInQuint => EaseInQuint::evaluate(t),
        AnimationStyle::EaseOutQuint => EaseOutQuint::evaluate(t),
        AnimationStyle::EaseInOutQuint => EaseInOutQuint::evaluate(t),
        AnimationStyle::EaseInExpo => EaseInExpo::evaluate(t),
        AnimationStyle::EaseOutExpo => EaseOutExpo::evaluate(t),
        AnimationStyle::EaseInOutExpo => EaseInOutExpo::evaluate(t),
        AnimationStyle::EaseInCirc => EaseInCirc::evaluate(t),
        AnimationStyle::EaseOutCirc => EaseOutCirc::evaluate(t),
        AnimationStyle::EaseInOutCirc => EaseInOutCirc::evaluate(t),
        AnimationStyle::EaseInBack => EaseInBack::evaluate(t),
        AnimationStyle::EaseOutBack => EaseOutBack::evaluate(t),
        AnimationStyle::EaseInOutBack => EaseInOutBack::evaluate(t),
        AnimationStyle::EaseInElastic => EaseInElastic::evaluate(t),
        AnimationStyle::EaseOutElastic => EaseOutElastic::evaluate(t),
        AnimationStyle::EaseInOutElastic => EaseInOutElastic::evaluate(t),
        AnimationStyle::EaseInBounce => EaseInBounce::evaluate(t),
        AnimationStyle::EaseOutBounce => EaseOutBounce::evaluate(t),
        AnimationStyle::EaseInOutBounce => EaseInOutBounce::evaluate(t),
    }
}

#[derive(Debug, Default, Clone, Copy, Serialize, Deserialize, JsonSchema, PartialEq)]
pub struct Animation {
    pub hwnd: isize,
}

impl Animation {
    pub fn new(hwnd: isize) -> Self {
        Self { hwnd }
    }
    pub fn cancel(&mut self) {
        if !ANIMATION_MANAGER.lock().in_progress(self.hwnd) {
            return;
        }

        ANIMATION_MANAGER.lock().cancel(self.hwnd);
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
    }

    #[allow(clippy::cast_possible_truncation)]
    pub fn lerp(start: i32, end: i32, t: f64) -> i32 {
        let time = apply_ease_func(t);
        f64::from(end - start)
            .mul_add(time, f64::from(start))
            .round() as i32
    }

    pub fn lerp_rect(start_rect: &Rect, end_rect: &Rect, t: f64) -> Rect {
        Rect {
            left: Self::lerp(start_rect.left, end_rect.left, t),
            top: Self::lerp(start_rect.top, end_rect.top, t),
            right: Self::lerp(start_rect.right, end_rect.right, t),
            bottom: Self::lerp(start_rect.bottom, end_rect.bottom, t),
        }
    }

    #[allow(clippy::cast_precision_loss)]
    pub fn animate(
        &mut self,
        duration: Duration,
        mut render_callback: impl FnMut(f64) -> Result<()>,
    ) -> Result<()> {
        if ANIMATION_MANAGER.lock().in_progress(self.hwnd) {
            self.cancel();
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
                // set all flags
                ANIMATION_MANAGER.lock().end(self.hwnd);
                return Ok(());
            }

            let frame_start = Instant::now();
            // calculate progress
            progress = animation_start.elapsed().as_millis() as f64 / duration.as_millis() as f64;
            render_callback(progress).ok();

            // sleep until next frame
            if frame_start.elapsed() < target_frame_time {
                std::thread::sleep(target_frame_time - frame_start.elapsed());
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
