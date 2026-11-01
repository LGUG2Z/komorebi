use crate::core::Rect;
use crate::core::animation::AnimationStyle;

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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_i32_lerp_linear() {
        assert_eq!(0i32.lerp(100, 0.0, AnimationStyle::Linear), 0);
        assert_eq!(0i32.lerp(100, 0.5, AnimationStyle::Linear), 50);
        assert_eq!(0i32.lerp(100, 1.0, AnimationStyle::Linear), 100);
    }

    #[test]
    fn test_f64_lerp_linear() {
        assert!((0.0f64.lerp(100.0, 0.0, AnimationStyle::Linear) - 0.0).abs() < f64::EPSILON);
        assert!((0.0f64.lerp(100.0, 0.5, AnimationStyle::Linear) - 50.0).abs() < f64::EPSILON);
        assert!((0.0f64.lerp(100.0, 1.0, AnimationStyle::Linear) - 100.0).abs() < f64::EPSILON);
    }

    #[test]
    fn test_rect_lerp_linear() {
        let start = Rect {
            left: 0,
            top: 0,
            right: 100,
            bottom: 100,
        };
        let end = Rect {
            left: 100,
            top: 100,
            right: 200,
            bottom: 200,
        };

        let mid = start.lerp(end, 0.5, AnimationStyle::Linear);
        assert_eq!(mid.left, 50);
        assert_eq!(mid.top, 50);
        assert_eq!(mid.right, 150);
        assert_eq!(mid.bottom, 150);
    }
}
