//! The geometry of a workspace sliding in and out of a monitor.
//!
//! The windows of the workspace that is left slide out of the monitor, and the
//! windows of the workspace that is entered slide in from the other side, as
//! if the workspaces were next to each other on a strip and the monitor looked
//! at one place on it. Everything here is arithmetic on rects in virtual
//! screen coordinates, so that it can be tested without a screen.

use crate::core::OperationDirection;
use crate::core::Rect;

/// Which side of the monitor the workspace that is entered comes in from. The
/// workspace that is left goes out on the other side.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SlideDirection {
    FromLeft,
    FromRight,
}

impl SlideDirection {
    /// The direction in which going from the workspace at `from` to the one at
    /// `to` is: to a workspace with a higher index is to the right, and to one
    /// with a lower index to the left. There is none for the same workspace.
    pub fn between(from: usize, to: usize) -> Option<Self> {
        match to.cmp(&from) {
            std::cmp::Ordering::Greater => Some(Self::FromRight),
            std::cmp::Ordering::Less => Some(Self::FromLeft),
            std::cmp::Ordering::Equal => None,
        }
    }

    /// The direction of going to the workspace in a direction, as when moving
    /// across the edge of a workspace to the next one, which is the same
    /// whichever of the two indices is the higher, so that going right from
    /// the last workspace to the first comes in from the right too.
    pub fn of_operation(direction: OperationDirection) -> Option<Self> {
        match direction {
            OperationDirection::Left => Some(Self::FromLeft),
            OperationDirection::Right => Some(Self::FromRight),
            OperationDirection::Up | OperationDirection::Down => None,
        }
    }

    /// How far, in pixels to the right, the windows of the workspace that is
    /// entered start from where they belong on a monitor `monitor_width` wide
    pub fn incoming_offset(self, monitor_width: i32) -> i32 {
        match self {
            Self::FromRight => monitor_width,
            Self::FromLeft => -monitor_width,
        }
    }

    /// How far, in pixels to the right, the windows of the workspace that is
    /// left end up from where they are
    pub fn outgoing_offset(self, monitor_width: i32) -> i32 {
        -self.incoming_offset(monitor_width)
    }
}

/// A rect moved `dx` to the right
pub fn shifted(rect: Rect, dx: i32) -> Rect {
    Rect {
        left: rect.left + dx,
        ..rect
    }
}

/// Where a ghost of `source` size, shown in `dest`, is to be drawn so that none
/// of it is outside `clip`: the part of `dest` that is inside, and the part of
/// the source that goes there, in pixels of the source. Nothing if no part of
/// it is inside.
///
/// A ghost is drawn on a host that covers every monitor, and a ghost that slides
/// out of a monitor would otherwise be drawn on the one next to it.
pub fn clip_thumbnail(dest: Rect, source: (i32, i32), clip: Rect) -> Option<(Rect, Rect)> {
    let (source_width, source_height) = source;

    if dest.right <= 0 || dest.bottom <= 0 || source_width <= 0 || source_height <= 0 {
        return None;
    }

    let left = dest.left.max(clip.left);
    let top = dest.top.max(clip.top);
    let right = (dest.left + dest.right).min(clip.left + clip.right);
    let bottom = (dest.top + dest.bottom).min(clip.top + clip.bottom);

    if right <= left || bottom <= top {
        return None;
    }

    // the part of the source that is under that part of the destination, which
    // is as much of it as the destination is stretched by
    let scale = |offset: i32, dest_size: i32, source_size: i32| -> i32 {
        let scaled = i64::from(offset) * i64::from(source_size);
        let divisor = i64::from(dest_size);
        // rounded to the nearest pixel
        ((scaled * 2 + divisor) / (divisor * 2)) as i32
    };

    let source_left = scale(left - dest.left, dest.right, source_width).clamp(0, source_width);
    let source_right = scale(right - dest.left, dest.right, source_width).clamp(0, source_width);
    let source_top = scale(top - dest.top, dest.bottom, source_height).clamp(0, source_height);
    let source_bottom =
        scale(bottom - dest.top, dest.bottom, source_height).clamp(0, source_height);

    if source_right <= source_left || source_bottom <= source_top {
        return None;
    }

    Some((
        Rect {
            left,
            top,
            right: right - left,
            bottom: bottom - top,
        },
        Rect {
            left: source_left,
            top: source_top,
            right: source_right - source_left,
            bottom: source_bottom - source_top,
        },
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rect(left: i32, top: i32, width: i32, height: i32) -> Rect {
        Rect {
            left,
            top,
            right: width,
            bottom: height,
        }
    }

    #[test]
    fn going_to_a_higher_workspace_is_to_the_right() {
        assert_eq!(
            SlideDirection::between(0, 1),
            Some(SlideDirection::FromRight)
        );
        assert_eq!(
            SlideDirection::between(1, 4),
            Some(SlideDirection::FromRight)
        );
        assert_eq!(
            SlideDirection::between(3, 2),
            Some(SlideDirection::FromLeft)
        );
        assert_eq!(
            SlideDirection::between(2, 0),
            Some(SlideDirection::FromLeft)
        );
        assert_eq!(SlideDirection::between(2, 2), None);
    }

    #[test]
    fn moving_across_an_edge_slides_the_way_it_moves() {
        assert_eq!(
            SlideDirection::of_operation(OperationDirection::Right),
            Some(SlideDirection::FromRight)
        );
        assert_eq!(
            SlideDirection::of_operation(OperationDirection::Left),
            Some(SlideDirection::FromLeft)
        );
        assert_eq!(SlideDirection::of_operation(OperationDirection::Up), None);
        assert_eq!(SlideDirection::of_operation(OperationDirection::Down), None);
    }

    #[test]
    fn the_two_workspaces_slide_opposite_ways_by_a_monitor_width() {
        let width = 5120;

        assert_eq!(SlideDirection::FromRight.incoming_offset(width), width);
        assert_eq!(SlideDirection::FromRight.outgoing_offset(width), -width);
        assert_eq!(SlideDirection::FromLeft.incoming_offset(width), -width);
        assert_eq!(SlideDirection::FromLeft.outgoing_offset(width), width);

        // the window at the left edge of a monitor that slides out to the left
        // ends up entirely outside it, and so does the one at the right edge
        let monitor = rect(0, 0, width, 2160);
        for window in [rect(0, 0, 1000, 2160), rect(width - 1000, 0, 1000, 2160)] {
            let gone = shifted(window, SlideDirection::FromRight.outgoing_offset(width));
            assert!(gone.left + gone.right <= monitor.left);
        }
    }

    #[test]
    fn a_ghost_inside_the_clip_is_not_cropped() {
        let (dest, source) =
            clip_thumbnail(rect(100, 50, 400, 300), (800, 600), rect(0, 0, 1920, 1080)).unwrap();

        assert_eq!(dest, rect(100, 50, 400, 300));
        assert_eq!(source, rect(0, 0, 800, 600));
    }

    #[test]
    fn a_ghost_that_leaves_on_the_left_is_cropped_on_the_left() {
        // half of it (200 of 400) is left of the monitor, at the same scale
        let (dest, source) =
            clip_thumbnail(rect(-200, 0, 400, 300), (400, 300), rect(0, 0, 1920, 1080)).unwrap();

        assert_eq!(dest, rect(0, 0, 200, 300));
        assert_eq!(source, rect(200, 0, 200, 300));
    }

    #[test]
    fn a_ghost_that_leaves_on_the_right_is_cropped_on_the_right() {
        let (dest, source) =
            clip_thumbnail(rect(1720, 0, 400, 300), (400, 300), rect(0, 0, 1920, 1080)).unwrap();

        assert_eq!(dest, rect(1720, 0, 200, 300));
        assert_eq!(source, rect(0, 0, 200, 300));
    }

    #[test]
    fn a_ghost_wider_than_the_clip_is_cropped_on_both_sides() {
        let (dest, source) = clip_thumbnail(
            rect(-100, 0, 2120, 100),
            (2120, 100),
            rect(0, 0, 1920, 1080),
        )
        .unwrap();

        assert_eq!(dest, rect(0, 0, 1920, 100));
        assert_eq!(source, rect(100, 0, 1920, 100));
    }

    #[test]
    fn the_part_of_the_source_follows_the_stretch_of_the_ghost() {
        // a source of 1000 pixels shown 500 wide, so each pixel is 2 of the source
        let (dest, source) =
            clip_thumbnail(rect(-100, 0, 500, 100), (1000, 200), rect(0, 0, 1920, 1080)).unwrap();

        assert_eq!(dest, rect(0, 0, 400, 100));
        assert_eq!(source, rect(200, 0, 800, 200));

        // and a source shown larger than it is
        let (dest, source) =
            clip_thumbnail(rect(-50, 0, 200, 100), (100, 50), rect(0, 0, 1920, 1080)).unwrap();

        assert_eq!(dest, rect(0, 0, 150, 100));
        assert_eq!(source, rect(25, 0, 75, 50));
    }

    #[test]
    fn a_ghost_cropped_vertically_keeps_the_matching_part_of_the_source() {
        let (dest, source) =
            clip_thumbnail(rect(0, -100, 400, 400), (400, 400), rect(0, 0, 1920, 1080)).unwrap();

        assert_eq!(dest, rect(0, 0, 400, 300));
        assert_eq!(source, rect(0, 100, 400, 300));
    }

    #[test]
    fn a_ghost_outside_the_clip_is_not_drawn() {
        let clip = rect(0, 0, 1920, 1080);

        // all of it to the left, all of it to the right, touching the edge
        assert!(clip_thumbnail(rect(-500, 0, 400, 300), (400, 300), clip).is_none());
        assert!(clip_thumbnail(rect(2000, 0, 400, 300), (400, 300), clip).is_none());
        assert!(clip_thumbnail(rect(-400, 0, 400, 300), (400, 300), clip).is_none());
        assert!(clip_thumbnail(rect(1920, 0, 400, 300), (400, 300), clip).is_none());

        // above and below
        assert!(clip_thumbnail(rect(0, -400, 400, 300), (400, 300), clip).is_none());
        assert!(clip_thumbnail(rect(0, 1080, 400, 300), (400, 300), clip).is_none());
    }

    #[test]
    fn nothing_without_a_size_is_drawn() {
        let clip = rect(0, 0, 1920, 1080);

        assert!(clip_thumbnail(rect(0, 0, 0, 300), (400, 300), clip).is_none());
        assert!(clip_thumbnail(rect(0, 0, 400, 0), (400, 300), clip).is_none());
        assert!(clip_thumbnail(rect(0, 0, 400, 300), (0, 300), clip).is_none());
        assert!(clip_thumbnail(rect(0, 0, 400, 300), (400, 0), clip).is_none());
    }

    #[test]
    fn a_monitor_to_the_left_of_the_primary_one_clips_in_its_own_coordinates() {
        // the monitor is at -1920, and the ghost slides out of it to the right,
        // towards the primary monitor, which is not where it is to be drawn
        let monitor = rect(-1920, 0, 1920, 1080);

        let (dest, source) = clip_thumbnail(rect(-300, 0, 600, 300), (600, 300), monitor).unwrap();

        assert_eq!(dest, rect(-300, 0, 300, 300));
        assert_eq!(source, rect(0, 0, 300, 300));

        // and the one that has slid right out of it
        assert!(clip_thumbnail(rect(0, 0, 600, 300), (600, 300), monitor).is_none());
    }

    #[test]
    fn what_is_drawn_never_goes_outside_the_clip_or_the_source() {
        let clip = rect(-1920, 100, 3840, 900);

        for left in (-6000..6000).step_by(137) {
            for (width, source_width) in [(100, 400), (700, 300), (1920, 1920), (4000, 123)] {
                let dest = rect(left, 300, width, 400);

                if let Some((drawn, part)) = clip_thumbnail(dest, (source_width, 90), clip) {
                    assert!(drawn.left >= clip.left);
                    assert!(drawn.left + drawn.right <= clip.left + clip.right);
                    assert!(drawn.top >= clip.top);
                    assert!(drawn.top + drawn.bottom <= clip.top + clip.bottom);
                    assert!(part.left >= 0 && part.top >= 0);
                    assert!(part.left + part.right <= source_width);
                    assert!(part.top + part.bottom <= 90);
                    assert!(drawn.right > 0 && drawn.bottom > 0);
                    assert!(part.right > 0 && part.bottom > 0);
                }
            }
        }
    }
}
