use crate::core_graphics::error::CoreGraphicsError;
use objc2_core_foundation::CFArray;
use objc2_core_foundation::CFRetained;
use objc2_core_foundation::CGFloat;
use objc2_core_foundation::CGPoint;
use objc2_core_foundation::CGRect;
use objc2_core_graphics::CGDisplayBounds;
use objc2_core_graphics::CGDisplaySerialNumber;
use objc2_core_graphics::CGGetDisplaysWithPoint;
use objc2_core_graphics::CGGetOnlineDisplayList;
use objc2_core_graphics::CGMainDisplayID;
use objc2_core_graphics::CGRectIntersectsRect;
use objc2_core_graphics::CGWarpMouseCursorPosition;
use objc2_core_graphics::CGWindowListCopyWindowInfo;
use objc2_core_graphics::CGWindowListOption;
use objc2_core_graphics::kCGNullWindowID;

pub mod error;

pub struct CoreGraphicsApi;

impl CoreGraphicsApi {
    pub fn warp_mouse_cursor_position(x: i32, y: i32) -> Result<(), CoreGraphicsError> {
        match CoreGraphicsError::from(CGWarpMouseCursorPosition(CGPoint::new(
            x as CGFloat,
            y as CGFloat,
        ))) {
            CoreGraphicsError::Success => Ok(()),
            error => Err(error),
        }
    }

    pub fn contains_rect(smaller: CGRect, bigger: CGRect) -> bool {
        CGRectIntersectsRect(smaller, bigger)
    }

    pub fn connected_display_ids() -> Result<Vec<u32>, CoreGraphicsError> {
        let mut displays: Vec<u32> = Vec::with_capacity(16);
        let mut display_count = 0;

        unsafe {
            match CoreGraphicsError::from(CGGetOnlineDisplayList(
                displays.capacity() as u32,
                displays.as_mut_ptr(),
                &mut display_count,
            )) {
                CoreGraphicsError::Success => {
                    displays.set_len(display_count as usize);
                    Ok(displays)
                }
                error => Err(error),
            }
        }
    }

    pub fn display_bounds(display_id: u32) -> CGRect {
        CGDisplayBounds(display_id)
    }

    pub fn display_serial_number(display_id: u32) -> u32 {
        CGDisplaySerialNumber(display_id)
    }

    /// The bounds of the display that a window with this frame is on
    ///
    /// The display under the top-left corner of the window answers whenever there is one. A window
    /// whose top-left corner is past the edge of every display, such as one that overhangs the left
    /// edge of the screen or is parked beyond it, is on the display that it overlaps the most, or
    /// failing that on the nearest one. Only a system that reports no displays at all gets the main
    /// display, which is logged.
    pub fn display_bounds_for_window_rect(window_rect: CGRect) -> Option<CGRect> {
        if let Some(display) = CoreGraphicsApi::display_with_point(window_rect.origin) {
            return Some(CoreGraphicsApi::display_bounds(display));
        }

        let displays = CoreGraphicsApi::connected_display_ids()
            .unwrap_or_default()
            .into_iter()
            .map(CoreGraphicsApi::display_bounds)
            .collect::<Vec<_>>();

        if let Some(idx) = display_for_frame(window_rect, &displays) {
            tracing::trace!(
                "no display under {},{}, using the one that is nearest to the window",
                window_rect.origin.x,
                window_rect.origin.y,
            );

            return displays.get(idx).copied();
        }

        tracing::warn!(
            "no display is known for the window at {},{}, using the main display",
            window_rect.origin.x,
            window_rect.origin.y,
        );

        let bounds = CoreGraphicsApi::display_bounds(CGMainDisplayID());
        (bounds.size.width > 0.0 && bounds.size.height > 0.0).then_some(bounds)
    }

    pub fn display_with_point(point: CGPoint) -> Option<u32> {
        let mut displays: Vec<u32> = Vec::with_capacity(1);
        let mut display_count = 0;

        unsafe {
            match CoreGraphicsError::from(CGGetDisplaysWithPoint(
                point,
                displays.capacity() as u32,
                displays.as_mut_ptr(),
                &mut display_count,
            )) {
                CoreGraphicsError::Success => {
                    displays.set_len(display_count as usize);
                }
                error => {
                    tracing::error!("failed to find display for point: {error}");
                    return None;
                }
            }
        }

        displays.first().copied()
    }

    pub fn window_list_info() -> Option<CFRetained<CFArray>> {
        CGWindowListCopyWindowInfo(
            // this is still way too many bogus windows
            CGWindowListOption::OptionOnScreenOnly | CGWindowListOption::ExcludeDesktopElements,
            // required when using OnScreenOnly
            kCGNullWindowID,
        )
    }
}

/// Which of `displays` a window with this frame belongs to
///
/// The display under the top-left corner of the window comes first, since that is the one that
/// the system says the window is on. A frame whose corner is on no display, which is the case for a
/// window that overhangs the left or top edge of the screen, belongs to the display that it
/// overlaps the most, and failing that to the nearest display. This is a total function: there is
/// an answer whenever there is a display.
pub fn display_for_frame(frame: CGRect, displays: &[CGRect]) -> Option<usize> {
    let contains = |display: &CGRect, x: f64, y: f64| {
        x >= display.origin.x
            && x < display.origin.x + display.size.width
            && y >= display.origin.y
            && y < display.origin.y + display.size.height
    };

    if let Some(idx) = displays
        .iter()
        .position(|display| contains(display, frame.origin.x, frame.origin.y))
    {
        return Some(idx);
    }

    // how far apart two intervals are, which is zero when they touch or overlap
    let gap = |start: f64, length: f64, other_start: f64, other_length: f64| {
        (other_start - (start + length))
            .max(start - (other_start + other_length))
            .max(0.0)
    };

    let overlap = |display: &CGRect| {
        let width = (frame.origin.x + frame.size.width).min(display.origin.x + display.size.width)
            - frame.origin.x.max(display.origin.x);
        let height = (frame.origin.y + frame.size.height)
            .min(display.origin.y + display.size.height)
            - frame.origin.y.max(display.origin.y);

        width.max(0.0) * height.max(0.0)
    };

    let distance = |display: &CGRect| {
        let dx = gap(
            frame.origin.x,
            frame.size.width,
            display.origin.x,
            display.size.width,
        );
        let dy = gap(
            frame.origin.y,
            frame.size.height,
            display.origin.y,
            display.size.height,
        );

        dx * dx + dy * dy
    };

    // the first of equals wins, so that the answer does not depend on the order of anything but
    // the list
    let mut best: Option<(usize, f64, f64)> = None;
    for (idx, display) in displays.iter().enumerate() {
        let (area, away) = (overlap(display), distance(display));
        let better = match best {
            None => true,
            Some((_, best_area, best_away)) => {
                area > best_area || (area == best_area && area == 0.0 && away < best_away)
            }
        };

        if better {
            best = Some((idx, area, away));
        }
    }

    best.map(|(idx, _, _)| idx)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rect(x: f64, y: f64, width: f64, height: f64) -> CGRect {
        CGRect::new(
            CGPoint::new(x, y),
            objc2_core_foundation::CGSize::new(width, height),
        )
    }

    /// A display of 3840 by 2160 on the right, and one of 1920 by 1080 to the left of it
    fn displays() -> Vec<CGRect> {
        vec![
            rect(0.0, 0.0, 3840.0, 2160.0),
            rect(-1920.0, 0.0, 1920.0, 1080.0),
        ]
    }

    #[test]
    fn the_display_under_the_corner_wins_over_the_one_with_more_of_the_window() {
        // the corner is on the left display, and nearly all of the window is on the right one
        let side_by_side = vec![
            rect(0.0, 0.0, 1920.0, 1080.0),
            rect(1920.0, 0.0, 1920.0, 1080.0),
        ];

        let frame = rect(1900.0, 100.0, 1800.0, 600.0);
        assert_eq!(display_for_frame(frame, &side_by_side), Some(0));

        let frame = rect(1930.0, 100.0, 1800.0, 600.0);
        assert_eq!(display_for_frame(frame, &side_by_side), Some(1));
    }

    #[test]
    fn a_window_past_the_left_edge_is_on_the_display_it_overlaps() {
        let main = vec![rect(0.0, 0.0, 3840.0, 2160.0)];

        // Slack, hanging off the left edge: the corner is on no display
        assert_eq!(
            display_for_frame(rect(-570.0, 60.0, 1840.0, 2000.0), &main),
            Some(0)
        );

        // parked at the edge with a sliver of 5px showing
        assert_eq!(
            display_for_frame(rect(-1670.0, 60.0, 1675.0, 2000.0), &main),
            Some(0)
        );
    }

    #[test]
    fn a_window_past_the_edge_with_a_neighbour_beyond_it_is_ambiguous() {
        // a window parked at the left edge of the main display has its corner on the neighbour,
        // so geometry alone says that it is on the neighbour. This is why the display that a
        // workspace hides its windows on comes from the monitor that owns the workspace, and not
        // from where the windows are
        let frame = rect(-1670.0, 60.0, 1675.0, 900.0);
        assert_eq!(display_for_frame(frame, &displays()), Some(1));
    }

    #[test]
    fn a_corner_on_no_display_goes_by_overlap() {
        // above everything, with most of the window over the neighbour and a sliver over the main
        // display
        let frame = rect(-1670.0, -100.0, 1675.0, 900.0);
        assert_eq!(display_for_frame(frame, &displays()), Some(1));

        let frame = rect(-100.0, -100.0, 1000.0, 900.0);
        assert_eq!(display_for_frame(frame, &displays()), Some(0));
    }

    #[test]
    fn a_window_past_the_right_and_top_edges_is_found_too() {
        let main = vec![rect(0.0, 0.0, 3840.0, 2160.0)];

        assert_eq!(
            display_for_frame(rect(3835.0, 60.0, 1275.0, 2000.0), &main),
            Some(0)
        );
        assert_eq!(
            display_for_frame(rect(100.0, -300.0, 1000.0, 900.0), &main),
            Some(0)
        );
    }

    #[test]
    fn a_window_that_overlaps_nothing_is_on_the_nearest_display() {
        // wholly past the left edge of the main display, closer to the display on its left
        assert_eq!(
            display_for_frame(rect(-2100.0, 3000.0, 100.0, 100.0), &displays()),
            Some(1)
        );

        // wholly below everything, below the main display
        assert_eq!(
            display_for_frame(rect(1000.0, 5000.0, 100.0, 100.0), &displays()),
            Some(0)
        );

        // wholly to the right of everything
        assert_eq!(
            display_for_frame(rect(9000.0, 100.0, 100.0, 100.0), &displays()),
            Some(0)
        );
    }

    #[test]
    fn there_is_no_answer_without_displays() {
        assert_eq!(display_for_frame(rect(0.0, 0.0, 10.0, 10.0), &[]), None);
    }

    #[test]
    fn equals_are_resolved_by_the_order_of_the_list() {
        // exactly between two displays and touching neither
        let displays = vec![rect(0.0, 0.0, 100.0, 100.0), rect(300.0, 0.0, 100.0, 100.0)];
        let frame = rect(200.0, 0.0, 0.0, 100.0);

        assert_eq!(display_for_frame(frame, &displays), Some(0));
    }
}
