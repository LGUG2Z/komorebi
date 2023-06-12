use schemars::JsonSchema;
use serde::Deserialize;
use serde::Serialize;
use windows::Win32::Foundation::RECT;

#[derive(Debug, Default, Clone, Copy, Serialize, Deserialize, Eq, PartialEq, JsonSchema)]
pub struct Rect {
    /// The left point in a Win32 Rect
    pub left: i32,
    /// The top point in a Win32 Rect
    pub top: i32,
    /// The right point in a Win32 Rect
    pub right: i32,
    /// The bottom point in a Win32 Rect
    pub bottom: i32,
}

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

impl Rect {
    pub fn add_padding(&mut self, padding: Option<i32>) {
        if let Some(padding) = padding {
            self.left += padding;
            self.top += padding;
            self.right -= padding * 2;
            self.bottom -= padding * 2;
        }
    }

    #[must_use]
    pub const fn contains_point(&self, point: (i32, i32)) -> bool {
        point.0 >= self.left
            && point.0 <= self.left + self.right
            && point.1 >= self.top
            && point.1 <= self.top + self.bottom
    }
}
