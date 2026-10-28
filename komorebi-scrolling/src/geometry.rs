/// The size of the area that the strip scrolls through.
///
/// This is the working area of a monitor: whatever is left once the host has taken out the menu
/// bar, the dock and any padding it wants. The model never learns where that area sits on a
/// screen. Every coordinate it returns is relative to the area's top left corner, and the host
/// adds its own offset.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Viewport {
    /// How wide the area is, in pixels.
    pub width: i32,
    /// How tall the area is, in pixels.
    pub height: i32,
}

/// A rectangle in viewport coordinates: the origin is the top left corner of the [`Viewport`] and
/// `y` grows downwards.
///
/// Unlike komorebi's own `Rect`, whose `right` and `bottom` hold a width and a height, a
/// `width` here is a width and `x + width` is the right edge.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Rect {
    /// The distance of the left edge from the left edge of the viewport.
    pub x: i32,
    /// The distance of the top edge from the top edge of the viewport.
    pub y: i32,
    /// How wide the rectangle is.
    pub width: i32,
    /// How tall the rectangle is.
    pub height: i32,
}

impl Rect {
    /// The x coordinate of the right edge.
    #[must_use]
    pub const fn right(&self) -> i32 {
        self.x + self.width
    }

    /// The y coordinate of the bottom edge.
    #[must_use]
    pub const fn bottom(&self) -> i32 {
        self.y + self.height
    }
}
