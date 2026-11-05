use crate::Arrangement;
use crate::Axis;
use crate::DefaultLayout;
use crate::OperationDirection;
use crate::Rect;
use std::num::NonZeroUsize;

/// A horizontal flip renders BSP container 0 on the right even though it is the
/// structural "leftmost", so a flip-blind edge pick focuses the wrong container
/// when crossing a boundary. This checks `cross_boundary_edge_index` against the
/// rectangles `calculate()` produces: focus lands on the container at
/// the edge the user crossed toward. See the function's doc for why the flip
/// forces this.
#[test]
fn cross_boundary_edge_index_honors_horizontal_flip() {
    let layout = DefaultLayout::BSP;
    let len = 4usize;
    let flip = Some(Axis::Horizontal);
    let area = Rect {
        left: 0,
        top: 0,
        right: 3440,
        bottom: 1440,
    };

    // The rectangles this flipped layout renders; rects[i] belongs to
    // container index i.
    let rects = layout.calculate(
        &area,
        NonZeroUsize::new(len).unwrap(),
        None,
        flip,
        &[],
        0,
        None,
        &[],
    );
    let max_left = rects.iter().map(|r| r.left).max().unwrap(); // right / seam edge
    let min_left = rects.iter().map(|r| r.left).min().unwrap(); // left edge
    assert_ne!(
        max_left, min_left,
        "precondition: the flip should spread containers across both horizontal edges"
    );

    // Crossing in by moving Left = entering from the workspace's right edge, so
    // focus must land on a container flush with the right edge (max left coord).
    let left_arrival = OperationDirection::Left.cross_boundary_edge_index(layout, len, flip);
    assert_eq!(
        rects[left_arrival].left, max_left,
        "focus-left arrival into a horizontally-flipped BSP workspace must land on the \
         right-edge (near-seam) container, not the far edge"
    );

    // Crossing in by moving Right = entering from the left edge.
    let right_arrival = OperationDirection::Right.cross_boundary_edge_index(layout, len, flip);
    assert_eq!(
        rects[right_arrival].left, min_left,
        "focus-right arrival into a horizontally-flipped BSP workspace must land on the \
         left-edge container"
    );

    // Without a flip, the pick is unchanged from the structural edge indices.
    assert_eq!(
        OperationDirection::Left.cross_boundary_edge_index(layout, len, None),
        layout.rightmost_index(len)
    );
    assert_eq!(
        OperationDirection::Right.cross_boundary_edge_index(layout, len, None),
        layout.leftmost_index(len)
    );
}
