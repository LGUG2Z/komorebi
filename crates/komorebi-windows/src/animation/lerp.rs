use crate::AnimationStyle;
use crate::core::Rect;

use super::style::apply_ease_func;

pub trait Lerp<T = Self> {
    fn lerp(self, end: T, time: f64, style: AnimationStyle) -> T;
}

impl Lerp for i32 {
    #[allow(clippy::cast_possible_truncation)]
    fn lerp(self, end: i32, time: f64, style: AnimationStyle) -> i32 {
        let time = apply_ease_func(time, style);

        f64::from(end - self).mul_add(time, f64::from(self)).round() as i32
    }
}

impl Lerp for f64 {
    fn lerp(self, end: f64, time: f64, style: AnimationStyle) -> f64 {
        let time = apply_ease_func(time, style);

        (end - self).mul_add(time, self)
    }
}

impl Lerp for u8 {
    fn lerp(self, end: u8, time: f64, style: AnimationStyle) -> u8 {
        (self as f64).lerp(end as f64, time, style) as u8
    }
}

impl Lerp for Rect {
    fn lerp(self, end: Rect, time: f64, style: AnimationStyle) -> Rect {
        Rect {
            left: self.left.lerp(end.left, time, style),
            top: self.top.lerp(end.top, time, style),
            right: self.right.lerp(end.right, time, style),
            bottom: self.bottom.lerp(end.bottom, time, style),
        }
    }
}
