use serde::Deserialize;
use serde::Serialize;

#[cfg(feature = "win32")]
use windows::Win32::Foundation::RECT;

#[cfg(feature = "darwin")]
use objc2_core_foundation::CGFloat;
#[cfg(feature = "darwin")]
use objc2_core_foundation::CGPoint;
#[cfg(feature = "darwin")]
use objc2_core_foundation::CGRect;
#[cfg(feature = "darwin")]
use objc2_core_foundation::CGSize;

#[derive(Debug, Default, Clone, Copy, Serialize, Deserialize, Eq, PartialEq)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
/// Rectangle dimensions
pub struct Rect {
    /// Left point of the rectangle
    pub left: i32,
    /// Top point of the rectangle
    pub top: i32,
    /// Width of the recentangle (from the left point)
    pub right: i32,
    /// Height of the rectangle (from the top point)
    pub bottom: i32,
}

#[cfg(feature = "win32")]
impl From<RECT> for Rect {
    fn from(rect: RECT) -> Self {
        Self {
            left: rect.left,
            top: rect.top,
            right: rect.right - rect.left,
            bottom: rect.bottom - rect.top,
        }
    }
}

#[cfg(feature = "win32")]
impl From<Rect> for RECT {
    fn from(rect: Rect) -> Self {
        Self {
            left: rect.left,
            top: rect.top,
            right: rect.right,
            bottom: rect.bottom,
        }
    }
}

#[cfg(feature = "darwin")]
impl From<CGSize> for Rect {
    fn from(value: CGSize) -> Self {
        Self {
            left: 0,
            top: 0,
            right: value.width as i32,
            bottom: value.height as i32,
        }
    }
}

#[cfg(feature = "darwin")]
impl From<CGRect> for Rect {
    fn from(value: CGRect) -> Self {
        Self {
            left: value.origin.x as i32,
            top: value.origin.y as i32,
            right: value.size.width as i32,
            bottom: value.size.height as i32,
        }
    }
}

#[cfg(feature = "darwin")]
impl From<&Rect> for CGRect {
    fn from(value: &Rect) -> Self {
        Self {
            origin: CGPoint {
                x: value.left as CGFloat,
                y: value.top as CGFloat,
            },
            size: CGSize {
                width: value.right as CGFloat,
                height: value.bottom as CGFloat,
            },
        }
    }
}

#[cfg(feature = "darwin")]
impl From<Rect> for CGRect {
    fn from(value: Rect) -> Self {
        CGRect::from(&value)
    }
}

impl Rect {
    pub fn is_same_size_as(&self, rhs: &Self) -> bool {
        self.right == rhs.right && self.bottom == rhs.bottom
    }

    pub fn has_same_position_as(&self, rhs: &Self) -> bool {
        self.left == rhs.left && self.top == rhs.top
    }
}

impl Rect {
    /// decrease the size of self by the padding amount.
    pub fn add_padding<T>(&mut self, padding: T)
    where
        T: Into<Option<i32>>,
    {
        if let Some(padding) = padding.into() {
            self.left += padding;
            self.top += padding;
            self.right -= padding * 2;
            self.bottom -= padding * 2;
        }
    }

    /// increase the size of self by the margin amount.
    pub fn add_margin(&mut self, margin: i32) {
        self.left -= margin;
        self.top -= margin;
        self.right += margin * 2;
        self.bottom += margin * 2;
    }

    pub fn left_padding(&mut self, padding: i32) {
        self.left += padding;
    }

    pub fn right_padding(&mut self, padding: i32) {
        self.right -= padding;
    }

    #[must_use]
    pub const fn contains_point(&self, point: (i32, i32)) -> bool {
        point.0 >= self.left
            && point.0 <= self.left + self.right
            && point.1 >= self.top
            && point.1 <= self.top + self.bottom
    }

    #[must_use]
    pub const fn scale(&self, system_dpi: i32, rect_dpi: i32) -> Rect {
        Rect {
            left: (self.left * rect_dpi) / system_dpi,
            top: (self.top * rect_dpi) / system_dpi,
            right: (self.right * rect_dpi) / system_dpi,
            bottom: (self.bottom * rect_dpi) / system_dpi,
        }
    }

    #[cfg(feature = "win32")]
    #[must_use]
    pub const fn rect(&self) -> RECT {
        RECT {
            left: self.left,
            top: self.top,
            right: self.left + self.right,
            bottom: self.top + self.bottom,
        }
    }

    #[cfg(feature = "darwin")]
    #[must_use]
    pub fn percentage_within_horizontal_bounds(&self, other: &Rect) -> f64 {
        let overlap_left = self.left.max(other.left);
        let overlap_right = (self.left + self.right).min(other.left + other.right);

        let overlap_width = overlap_right - overlap_left;

        if overlap_width <= 0 {
            0.0
        } else {
            (overlap_width as f64) / (other.right as f64) * 100.0
        }
    }
}
