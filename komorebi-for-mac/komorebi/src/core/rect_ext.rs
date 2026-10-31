use crate::core::Rect;
use crate::core_graphics::CoreGraphicsApi;
use crate::window::WindowBounds;
use objc2_core_foundation::CGRect;

/// Mac-specific extensions for the Rect type from komorebi-layouts
pub trait RectExt {
    /// Check if this rectangle contains another rectangle
    fn contains(&self, other: &Rect) -> bool;
}

impl RectExt for Rect {
    fn contains(&self, other: &Rect) -> bool {
        let self_cg: CGRect = self.into();
        let other_cg: CGRect = other.into();
        CoreGraphicsApi::contains_rect(other_cg, self_cg)
    }
}

impl From<WindowBounds> for Rect {
    fn from(value: WindowBounds) -> Self {
        Self {
            left: value.x as i32,
            top: value.y as i32,
            right: value.width as i32,
            bottom: value.height as i32,
        }
    }
}
