//! The arithmetic of where a column sits in the viewport.
//!
//! Everything here is a pure function of numbers, so that the rules for scrolling can be tested
//! without building a strip first. `x` is always the distance from the left edge of the viewport
//! to the left edge of a column.

/// Where the left edge of a column of `width` should sit so that the column is fully in view,
/// moving it no further than necessary from where it is now (`x`).
///
/// A column that is already in view is left exactly where it is. A column that is not is brought
/// in until it sits `gaps` away from the nearest edge of the viewport, or as close to that as
/// there is room for, since a column that nearly fills the viewport leaves less than a gap on
/// each side. A column that is as wide as the viewport or wider cannot be in view, so it is
/// aligned with the left edge, where the start of it can be seen.
pub(crate) fn fit(x: f64, width: f64, view_width: f64, gaps: f64) -> f64 {
    if width >= view_width {
        return 0.0;
    }

    // The margin cannot be more than what is left over, split between the two sides. This also
    // keeps the range below from being empty.
    let margin = ((view_width - width) / 2.0).clamp(0.0, gaps);
    x.clamp(margin, view_width - margin - width)
}

/// Where the left edge of a column of `width` sits when it is centred, or at the left edge if it
/// is too wide to be centred.
pub(crate) fn center(width: f64, view_width: f64) -> f64 {
    if width >= view_width {
        0.0
    } else {
        (view_width - width) / 2.0
    }
}

/// How far ahead of where it is released a pan is taken to be going, in seconds. A pan that is
/// let go while it is still moving carries on for about this long, which is what a flick is.
pub(crate) const PAN_PROJECTION_SECONDS: f64 = 0.333;

/// Which of the `alignments` a pan comes to rest at, as a position of the left edge of the viewport
/// in the strip.
///
/// The pan was let go at `position` moving at `velocity`, in the same units per second and in the
/// direction that the position grows, and is taken to carry on for [`PAN_PROJECTION_SECONDS`]. It
/// ends at the alignment that is nearest to where that gets to, with ties going to the first. The
/// strip has `min` and `max` for its ends, and nothing outside of them is where it can come to
/// rest, however hard it was flicked. If there is no alignment in range it rests at the end that
/// the projection is nearest to.
pub(crate) fn snap(position: f64, velocity: f64, alignments: &[f64], min: f64, max: f64) -> f64 {
    let max = max.max(min);
    let projected = (position + velocity * PAN_PROJECTION_SECONDS).clamp(min, max);

    let mut best: Option<f64> = None;
    for alignment in alignments {
        let alignment = alignment.clamp(min, max);

        if best.is_none_or(|best| (alignment - projected).abs() < (best - projected).abs()) {
            best = Some(alignment);
        }
    }

    best.unwrap_or(projected)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_pan_that_is_let_go_standing_still_rests_at_the_nearest_alignment() {
        let alignments = [0.0, 300.0, 600.0, 900.0];

        assert_eq!(snap(140.0, 0.0, &alignments, 0.0, 900.0), 0.0);
        assert_eq!(snap(160.0, 0.0, &alignments, 0.0, 900.0), 300.0);
        assert_eq!(snap(300.0, 0.0, &alignments, 0.0, 900.0), 300.0);
    }

    #[test]
    fn a_flick_carries_on_in_the_direction_that_it_was_going() {
        let alignments = [0.0, 300.0, 600.0, 900.0];

        // 1500 a second for a third of a second is 500 further on
        assert_eq!(snap(100.0, 1500.0, &alignments, 0.0, 900.0), 600.0);
        assert_eq!(snap(700.0, -1500.0, &alignments, 0.0, 900.0), 300.0);
    }

    #[test]
    fn a_pan_cannot_end_beyond_the_ends_of_the_strip() {
        let alignments = [0.0, 300.0, 600.0, 900.0];

        assert_eq!(snap(500.0, 90_000.0, &alignments, 0.0, 900.0), 900.0);
        assert_eq!(snap(500.0, -90_000.0, &alignments, 0.0, 900.0), 0.0);
        // and one that is let go beyond them comes back
        assert_eq!(snap(-200.0, 0.0, &alignments, 0.0, 900.0), 0.0);
        assert_eq!(snap(1200.0, 0.0, &alignments, 0.0, 900.0), 900.0);
    }

    #[test]
    fn with_nothing_to_align_to_the_pan_rests_where_the_projection_says() {
        assert_eq!(snap(100.0, 0.0, &[], 0.0, 900.0), 100.0);
        assert_eq!(snap(100.0, 0.0, &[], 300.0, 300.0), 300.0);
    }

    #[test]
    fn a_column_in_view_stays_where_it_is() {
        assert_eq!(fit(300.0, 400.0, 1200.0, 16.0), 300.0);
        // right up against either margin is still in view
        assert_eq!(fit(16.0, 400.0, 1200.0, 16.0), 16.0);
        assert_eq!(fit(784.0, 400.0, 1200.0, 16.0), 784.0);
    }

    #[test]
    fn a_column_that_is_out_of_view_is_brought_in_by_the_least_amount() {
        // off the right edge: its right edge ends up one gap from the edge of the viewport
        assert_eq!(fit(1000.0, 400.0, 1200.0, 16.0), 784.0);
        // off the left edge: its left edge ends up one gap from the edge of the viewport
        assert_eq!(fit(-300.0, 400.0, 1200.0, 16.0), 16.0);
        // partly in view counts as out of view
        assert_eq!(fit(900.0, 400.0, 1200.0, 0.0), 800.0);
        assert_eq!(fit(-1.0, 400.0, 1200.0, 0.0), 0.0);
    }

    #[test]
    fn a_column_that_nearly_fills_the_viewport_gets_what_room_there_is() {
        // 1180 leaves 20 pixels, so the margin is 10 and not the 16 that was asked for
        assert_eq!(fit(500.0, 1180.0, 1200.0, 16.0), 10.0);
        assert_eq!(fit(-500.0, 1180.0, 1200.0, 16.0), 10.0);
    }

    #[test]
    fn a_column_that_cannot_be_in_view_is_aligned_with_the_left_edge() {
        assert_eq!(fit(500.0, 1200.0, 1200.0, 16.0), 0.0);
        assert_eq!(fit(500.0, 3000.0, 1200.0, 16.0), 0.0);
        assert_eq!(fit(-500.0, 3000.0, 1200.0, 16.0), 0.0);
    }

    #[test]
    fn centring() {
        assert_eq!(center(400.0, 1200.0), 400.0);
        assert_eq!(center(1200.0, 1200.0), 0.0);
        assert_eq!(center(2000.0, 1200.0), 0.0);
    }
}
