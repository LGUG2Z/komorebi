use crate::core::AnimationStyle;

use std::f64::consts::PI;

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

pub struct CubicBezier {
    pub x1: f64,
    pub y1: f64,
    pub x2: f64,
    pub y2: f64,
}

impl CubicBezier {
    fn x(&self, s: f64) -> f64 {
        3.0 * self.x1 * s * (1.0 - s).powi(2) + 3.0 * self.x2 * s.powi(2) * (1.0 - s) + s.powi(3)
    }

    fn y(&self, s: f64) -> f64 {
        3.0 * self.y1 * s * (1.0 - s).powi(2) + 3.0 * self.y2 * s.powi(2) * (1.0 - s) + s.powi(3)
    }

    fn dx_ds(&self, s: f64) -> f64 {
        3.0 * self.x1 * (1.0 - s) * (1.0 - 3.0 * s)
            + 3.0 * self.x2 * (2.0 * s - 3.0 * s.powi(2))
            + 3.0 * s.powi(2)
    }

    fn find_s(&self, t: f64) -> f64 {
        if t <= 0.0 {
            return 0.0;
        }

        if t >= 1.0 {
            return 1.0;
        }

        let mut s = t;

        for _ in 0..8 {
            let x_val = self.x(s);
            let dx_val = self.dx_ds(s);
            if dx_val.abs() < 1e-6 {
                break;
            }
            let delta = (x_val - t) / dx_val;
            s = (s - delta).clamp(0.0, 1.0);
            if delta.abs() < 1e-6 {
                break;
            }
        }

        s
    }

    fn evaluate(&self, t: f64) -> f64 {
        let s = self.find_s(t.clamp(0.0, 1.0));
        self.y(s)
    }
}

pub fn apply_ease_func(t: f64, style: AnimationStyle) -> f64 {
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
        AnimationStyle::CubicBezier(x1, y1, x2, y2) => CubicBezier { x1, y1, x2, y2 }.evaluate(t),
    }
}
