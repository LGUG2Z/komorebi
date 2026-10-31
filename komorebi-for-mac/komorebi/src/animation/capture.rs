//! Still pictures of windows, for the ghosts that slide in their place
//!
//! This is the only place that asks the window server for the pixels of a window, so that it can
//! be moved to ScreenCaptureKit when `CGWindowListCreateImage` goes away.

use objc2_core_foundation::CFRetained;
use objc2_core_foundation::CGPoint;
use objc2_core_foundation::CGRect;
use objc2_core_foundation::CGSize;
use objc2_core_graphics::CGImage;
use objc2_core_graphics::CGWindowImageOption;
#[allow(deprecated)]
use objc2_core_graphics::CGWindowListCreateImage;
use objc2_core_graphics::CGWindowListOption;

/// What a window looked like at a moment
#[derive(Debug, Clone)]
pub struct Still {
    pub image: CFRetained<CGImage>,
    /// In pixels, which is more than the size of the window in points on a retina display
    pub width: usize,
    pub height: usize,
}

// a `CGImage` is immutable once it is made
unsafe impl Send for Still {}
unsafe impl Sync for Still {}

impl Still {
    /// How much memory the picture takes, for a cache to keep to a budget
    pub fn bytes(&self) -> usize {
        CGImage::bytes_per_row(Some(&self.image)) * self.height
    }
}

/// `CGRectNull`, which stands for the bounds of the window itself when capturing a single window
const NULL_RECT: CGRect = CGRect {
    origin: CGPoint {
        x: f64::INFINITY,
        y: f64::INFINITY,
    },
    size: CGSize {
        width: 0.0,
        height: 0.0,
    },
};

/// Take a picture of the window, whether or not it is where it can be seen
///
/// This is `None` when the window server has no picture of it, which is what it says for a window
/// that it does not know, and when the app has not been allowed to record the screen.
#[allow(deprecated)]
pub fn capture(window_id: u32) -> Option<Still> {
    let image = CGWindowListCreateImage(
        NULL_RECT,
        CGWindowListOption::OptionIncludingWindow,
        window_id,
        // the frame is what the window is, without the shadow that it casts, which would otherwise
        // be as large as the picture
        CGWindowImageOption::BoundsIgnoreFraming | CGWindowImageOption::BestResolution,
    )?;

    let width = CGImage::width(Some(&image));
    let height = CGImage::height(Some(&image));

    if width == 0 || height == 0 {
        return None;
    }

    Some(Still {
        image,
        width,
        height,
    })
}
