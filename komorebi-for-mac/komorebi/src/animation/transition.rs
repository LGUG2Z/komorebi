//! The geometry of a workspace sliding in and out of a monitor.
//!
//! The windows of the workspace that is left slide out of the monitor, and the windows of the
//! workspace that is entered slide in from the other side, as if the workspaces were next to each
//! other on a strip and the monitor looked at one place on it. Everything here is arithmetic on
//! rects, so that it can be tested without a screen.

use crate::animation::style::apply_ease_func;
use crate::core::OperationDirection;
use crate::core::Rect;
use crate::core::animation::AnimationStyle;

/// Which side of the monitor the workspace that is entered comes in from. The workspace that is
/// left goes out on the other side.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SlideDirection {
    FromLeft,
    FromRight,
}

impl SlideDirection {
    /// The direction of going from the workspace at `from` to the one at `to`: to a workspace with
    /// a higher index is to the right, and to one with a lower index to the left. There is none for
    /// the same workspace.
    pub fn between(from: usize, to: usize) -> Option<Self> {
        match to.cmp(&from) {
            std::cmp::Ordering::Greater => Some(Self::FromRight),
            std::cmp::Ordering::Less => Some(Self::FromLeft),
            std::cmp::Ordering::Equal => None,
        }
    }

    /// The direction of going to the workspace in a direction, as when moving across the edge of a
    /// workspace to the next one, which is the same whichever of the two indices is the higher, so
    /// that going right from the last workspace to the first comes in from the right too.
    pub fn of_operation(direction: OperationDirection) -> Option<Self> {
        match direction {
            OperationDirection::Left => Some(Self::FromLeft),
            OperationDirection::Right => Some(Self::FromRight),
            OperationDirection::Up | OperationDirection::Down => None,
        }
    }

    /// How far, in points to the right, the windows of the workspace that is entered start from
    /// where they belong on a monitor `monitor_width` wide
    pub fn incoming_offset(self, monitor_width: i32) -> i32 {
        match self {
            Self::FromRight => monitor_width,
            Self::FromLeft => -monitor_width,
        }
    }

    /// How far, in points to the right, the windows of the workspace that is left end up from
    /// where they are
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

/// Whether any of `rect` is inside `clip`
pub fn intersects(rect: Rect, clip: Rect) -> bool {
    rect.left < clip.left + clip.right
        && rect.left + rect.right > clip.left
        && rect.top < clip.top + clip.bottom
        && rect.top + rect.bottom > clip.top
}

/// A window of the workspace that is left or entered, and where it slides from and to
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Slide {
    pub id: u32,
    pub from: Rect,
    pub to: Rect,
}

/// What switching to another workspace on a monitor is to slide
#[derive(Debug, Clone, PartialEq)]
pub struct Plan {
    pub direction: SlideDirection,
    /// The monitor, outside of which nothing of it is to be seen
    pub clip: Rect,
    /// The windows of the workspace that is left, which slide out of the monitor
    pub exits: Vec<Slide>,
    /// The windows of the workspace that is entered, which slide in to where the layout puts them
    pub enters: Vec<Slide>,
}

/// What to slide on the monitor `clip`, given the windows of the workspace that is left and where
/// they are, and the windows of the one that is entered and where they are to be.
///
/// A window that is not inside the monitor to begin with, or to end up with, is not slid: it is not
/// seen, and no ghost of it would be either. There is nothing to slide when there are no windows
/// at all.
pub fn plan(
    direction: SlideDirection,
    clip: Rect,
    outgoing: &[(u32, Rect)],
    incoming: &[(u32, Rect)],
) -> Option<Plan> {
    let out = direction.outgoing_offset(clip.right);
    let into = direction.incoming_offset(clip.right);

    let exits: Vec<Slide> = outgoing
        .iter()
        .filter(|(_, rect)| intersects(*rect, clip))
        .map(|(id, rect)| Slide {
            id: *id,
            from: *rect,
            to: shifted(*rect, out),
        })
        .collect();

    let enters: Vec<Slide> = incoming
        .iter()
        .filter(|(_, rect)| intersects(*rect, clip))
        .map(|(id, rect)| Slide {
            id: *id,
            from: shifted(*rect, into),
            to: *rect,
        })
        .collect();

    if exits.is_empty() && enters.is_empty() {
        return None;
    }

    Some(Plan {
        direction,
        clip,
        exits,
        enters,
    })
}

/// How far along its way something is for each of `steps + 1` evenly spaced moments of an
/// animation in `style`: from 0 at the start to 1 at the end
///
/// The compositor interpolates linearly between these, so enough of them make any style smooth.
pub fn progress_samples(style: AnimationStyle, steps: usize) -> Vec<f64> {
    let steps = steps.max(1);

    (0..=steps)
        .map(|step| match step {
            0 => 0.0,
            last if last == steps => 1.0,
            step => apply_ease_func(step as f64 / steps as f64, style),
        })
        .collect()
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
            SlideDirection::between(3, 1),
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
    }

    #[test]
    fn the_two_workspaces_slide_opposite_ways_by_a_monitor_width() {
        let direction = SlideDirection::FromRight;

        assert_eq!(direction.incoming_offset(1920), 1920);
        assert_eq!(direction.outgoing_offset(1920), -1920);
        assert_eq!(SlideDirection::FromLeft.incoming_offset(1920), -1920);
        assert_eq!(SlideDirection::FromLeft.outgoing_offset(1920), 1920);
    }

    #[test]
    fn windows_slide_out_to_the_side_opposite_to_the_one_the_others_come_in_from() {
        let clip = rect(0, 0, 1000, 800);
        let plan = plan(
            SlideDirection::FromRight,
            clip,
            &[(1, rect(10, 10, 400, 780))],
            &[(2, rect(10, 10, 400, 780))],
        )
        .unwrap();

        assert_eq!(plan.exits[0].from, rect(10, 10, 400, 780));
        assert_eq!(plan.exits[0].to, rect(-990, 10, 400, 780));
        assert_eq!(plan.enters[0].from, rect(1010, 10, 400, 780));
        assert_eq!(plan.enters[0].to, rect(10, 10, 400, 780));
    }

    #[test]
    fn what_slides_out_of_a_monitor_is_measured_from_where_the_monitor_is() {
        // a monitor to the left of the primary one
        let clip = rect(-1000, 0, 1000, 800);
        let plan = plan(
            SlideDirection::FromLeft,
            clip,
            &[(1, rect(-900, 0, 300, 800))],
            &[],
        )
        .unwrap();

        assert_eq!(plan.exits[0].to, rect(100, 0, 300, 800));
    }

    #[test]
    fn a_window_that_is_not_in_the_monitor_is_not_slid() {
        let clip = rect(0, 0, 1000, 800);
        let plan = plan(
            SlideDirection::FromRight,
            clip,
            &[(1, rect(1000, 0, 300, 800)), (2, rect(0, 0, 300, 800))],
            &[(3, rect(-300, 0, 300, 800))],
        )
        .unwrap();

        assert_eq!(plan.exits.len(), 1);
        assert_eq!(plan.exits[0].id, 2);
        assert!(plan.enters.is_empty());
    }

    #[test]
    fn there_is_nothing_to_slide_without_windows() {
        let clip = rect(0, 0, 1000, 800);

        assert_eq!(plan(SlideDirection::FromLeft, clip, &[], &[]), None);
    }

    #[test]
    fn progress_starts_at_nothing_and_ends_at_everything() {
        for style in [
            AnimationStyle::Linear,
            AnimationStyle::EaseOutCubic,
            AnimationStyle::EaseInOutQuint,
        ] {
            let samples = progress_samples(style, 30);

            assert_eq!(samples.len(), 31);
            assert_eq!(samples[0], 0.0);
            assert_eq!(samples[30], 1.0);
        }
    }

    #[test]
    fn a_linear_style_is_evenly_spaced() {
        let samples = progress_samples(AnimationStyle::Linear, 4);

        assert_eq!(samples, vec![0.0, 0.25, 0.5, 0.75, 1.0]);
    }
}
