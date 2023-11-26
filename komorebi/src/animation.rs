use color_eyre::Result;
use komorebi_core::EaseEnum;
use komorebi_core::Rect;

use schemars::JsonSchema;

use std::f64::consts::PI;
use std::time::Duration;
use std::time::Instant;

use crate::ANIMATE_EASE;

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
    let ease = *ANIMATE_EASE.lock();

    match ease {
        EaseEnum::Linear => Linear::evaluate(t),
        EaseEnum::EaseInSine => EaseInSine::evaluate(t),
        EaseEnum::EaseOutSine => EaseOutSine::evaluate(t),
        EaseEnum::EaseInOutSine => EaseInOutSine::evaluate(t),
        EaseEnum::EaseInQuad => EaseInQuad::evaluate(t),
        EaseEnum::EaseOutQuad => EaseOutQuad::evaluate(t),
        EaseEnum::EaseInOutQuad => EaseInOutQuad::evaluate(t),
        EaseEnum::EaseInCubic => EaseInCubic::evaluate(t),
        EaseEnum::EaseInOutCubic => EaseInOutCubic::evaluate(t),
        EaseEnum::EaseInQuart => EaseInQuart::evaluate(t),
        EaseEnum::EaseOutQuart => EaseOutQuart::evaluate(t),
        EaseEnum::EaseInOutQuart => EaseInOutQuart::evaluate(t),
        EaseEnum::EaseInQuint => EaseInQuint::evaluate(t),
        EaseEnum::EaseOutQuint => EaseOutQuint::evaluate(t),
        EaseEnum::EaseInOutQuint => EaseInOutQuint::evaluate(t),
        EaseEnum::EaseInExpo => EaseInExpo::evaluate(t),
        EaseEnum::EaseOutExpo => EaseOutExpo::evaluate(t),
        EaseEnum::EaseInOutExpo => EaseInOutExpo::evaluate(t),
        EaseEnum::EaseInCirc => EaseInCirc::evaluate(t),
        EaseEnum::EaseOutCirc => EaseOutCirc::evaluate(t),
        EaseEnum::EaseInOutCirc => EaseInOutCirc::evaluate(t),
        EaseEnum::EaseInBack => EaseInBack::evaluate(t),
        EaseEnum::EaseOutBack => EaseOutBack::evaluate(t),
        EaseEnum::EaseInOutBack => EaseInOutBack::evaluate(t),
        EaseEnum::EaseInElastic => EaseInElastic::evaluate(t),
        EaseEnum::EaseOutElastic => EaseOutElastic::evaluate(t),
        EaseEnum::EaseInOutElastic => EaseInOutElastic::evaluate(t),
        EaseEnum::EaseInBounce => EaseInBounce::evaluate(t),
        EaseEnum::EaseOutBounce => EaseOutBounce::evaluate(t),
        EaseEnum::EaseInOutBounce => EaseInOutBounce::evaluate(t),
    }
}

#[derive(Debug, Default, Clone, Copy, JsonSchema)]
pub struct Animation {
    // is_cancel: AtomicBool,
    // pub in_progress: AtomicBool,
    is_cancel: bool,
    pub in_progress: bool,
}

// impl Default for Animation {
//     fn default() -> Self {
//         Animation {
//             // I'm not sure if this is the right way to do it
//             // I've tried to use Arc<Mutex<bool>> but it dooes not implement Copy trait
//             // and I dont want to rewrite everything cause I'm not experienced with rust
//             // Down here you can see the idea I've tried to achive like in any other OOP language
//             // My thought is that in order to prevent Google Chrome breaking render window
//             // I need to cancel animation if user starting new window movement. So window stops
//             // moving at one point and then fires new animation.
//             // But my approach does not work because of rust borrowing rules and wired pointers
//             // lifetime annotation that I dont know how to use.
//             is_cancel: false,
//             in_progress: false,
//             // is_cancel: AtomicBool::new(false),
//             // in_progress: AtomicBool::new(false),
//         }
//     }
// }

impl Animation {
    pub fn cancel(&mut self) {
        if !self.in_progress {
            return;
        }

        self.is_cancel = true;
        let max_duration = Duration::from_secs(1);
        let spent_duration = Instant::now();

        while self.in_progress {
            if spent_duration.elapsed() >= max_duration {
                self.in_progress = false;
            }

            std::thread::sleep(Duration::from_millis(16));
        }
    }

    #[allow(clippy::cast_possible_truncation)]
    pub fn lerp(x: i32, new_x: i32, t: f64) -> i32 {
        let time = apply_ease_func(t);
        f64::from(new_x - x).mul_add(time, f64::from(x)) as i32
    }

    pub fn lerp_rect(original_rect: &Rect, new_rect: &Rect, t: f64) -> Rect {
        Rect {
            left: Self::lerp(original_rect.left, new_rect.left, t),
            top: Self::lerp(original_rect.top, new_rect.top, t),
            right: Self::lerp(original_rect.right, new_rect.right, t),
            bottom: Self::lerp(original_rect.bottom, new_rect.bottom, t),
        }
    }

    #[allow(clippy::cast_precision_loss)]
    pub fn animate(
        &mut self,
        duration: Duration,
        mut f: impl FnMut(f64) -> Result<()>,
    ) -> Result<()> {
        self.in_progress = true;
        // set target frame time to match 240 fps (my max refresh rate of monitor)
        // probably not the best way to do it is take actual monitor refresh rate
        // or make it configurable
        let target_frame_time = Duration::from_millis(1000 / 240);
        let mut progress = 0.0;
        let animation_start = Instant::now();

        // start animation
        while progress < 1.0 {
            // check if animation is cancelled
            if self.is_cancel {
                // cancel animation
                // set all flags
                self.is_cancel = !self.is_cancel;
                self.in_progress = false;
                return Ok(());
            }

            let tick_start = Instant::now();
            // calculate progress
            progress = animation_start.elapsed().as_millis() as f64 / duration.as_millis() as f64;
            f(progress).ok();

            // sleep until next frame
            while tick_start.elapsed() < target_frame_time {
                std::thread::sleep(target_frame_time - tick_start.elapsed());
            }
        }

        self.in_progress = false;

        // limit progress to 1.0 if animation took longer
        if progress > 1.0 {
            progress = 1.0;
        }

        // process animation for 1.0 to set target position
        f(progress)
    }
}
