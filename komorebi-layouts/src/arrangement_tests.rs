use super::*;
use std::num::NonZeroUsize;

// Helper to create a test area
fn test_area() -> Rect {
    Rect {
        left: 0,
        top: 0,
        right: 1000,
        bottom: 800,
    }
}

// Helper to create LayoutOptions with column ratios
fn layout_options_with_column_ratios(ratios: &[f32]) -> LayoutOptions {
    let mut arr = [None; MAX_RATIOS];
    for (i, &r) in ratios.iter().take(MAX_RATIOS).enumerate() {
        arr[i] = Some(r);
    }
    LayoutOptions {
        scrolling: None,
        grid: None,
        column_ratios: Some(arr),
        row_ratios: None,
    }
}

// Helper to create LayoutOptions with row ratios
fn layout_options_with_row_ratios(ratios: &[f32]) -> LayoutOptions {
    let mut arr = [None; MAX_RATIOS];
    for (i, &r) in ratios.iter().take(MAX_RATIOS).enumerate() {
        arr[i] = Some(r);
    }
    LayoutOptions {
        scrolling: None,
        grid: None,
        column_ratios: None,
        row_ratios: Some(arr),
    }
}

// Helper to create LayoutOptions with both column and row ratios
fn layout_options_with_ratios(column_ratios: &[f32], row_ratios: &[f32]) -> LayoutOptions {
    let mut col_arr = [None; MAX_RATIOS];
    for (i, &r) in column_ratios.iter().take(MAX_RATIOS).enumerate() {
        col_arr[i] = Some(r);
    }
    let mut row_arr = [None; MAX_RATIOS];
    for (i, &r) in row_ratios.iter().take(MAX_RATIOS).enumerate() {
        row_arr[i] = Some(r);
    }
    LayoutOptions {
        scrolling: None,
        grid: None,
        column_ratios: Some(col_arr),
        row_ratios: Some(row_arr),
    }
}

fn assert_containers_adjacent_horizontally(layouts: &[Rect], area: &Rect) {
    let mut sorted: Vec<&Rect> = layouts.iter().collect();
    sorted.sort_by_key(|r| r.left);

    for pair in sorted.windows(2) {
        assert_eq!(
            pair[0].left + pair[0].right,
            pair[1].left,
            "gap between containers at left={} (width {}) and left={}",
            pair[0].left,
            pair[0].right,
            pair[1].left,
        );
    }

    let rightmost = sorted.last().unwrap();
    assert_eq!(
        rightmost.left + rightmost.right,
        area.left + area.right,
        "rightmost container does not reach the area edge",
    );
}

fn assert_containers_adjacent_vertically(layouts: &[Rect], area: &Rect) {
    let mut sorted: Vec<&Rect> = layouts.iter().collect();
    sorted.sort_by_key(|r| r.top);

    for pair in sorted.windows(2) {
        assert_eq!(
            pair[0].top + pair[0].bottom,
            pair[1].top,
            "gap between containers at top={} (height {}) and top={}",
            pair[0].top,
            pair[0].bottom,
            pair[1].top,
        );
    }

    let bottommost = sorted.last().unwrap();
    assert_eq!(
        bottommost.top + bottommost.bottom,
        area.top + area.bottom,
        "bottommost container does not reach the area edge",
    );
}

mod columns_with_ratios_tests {
    use super::*;

    #[test]
    fn test_columns_equal_width_no_ratios() {
        let area = test_area();
        let layouts = columns_with_ratios(&area, 4, None);

        assert_eq!(layouts.len(), 4);
        // Each column should be 250 pixels wide (1000 / 4)
        for layout in &layouts {
            assert_eq!(layout.right, 250);
            assert_eq!(layout.bottom, 800);
        }
    }

    #[test]
    fn test_columns_with_single_ratio() {
        let area = test_area();
        let opts = layout_options_with_column_ratios(&[0.3]);
        let layouts = columns_with_ratios(&area, 3, opts.column_ratios);

        assert_eq!(layouts.len(), 3);
        // First column: 30% of 1000 = 300
        assert_eq!(layouts[0].right, 300);
        // Remaining 700 split between 2 columns = 350 each
        assert_eq!(layouts[1].right, 350);
        assert_eq!(layouts[2].right, 350);
    }

    #[test]
    fn test_columns_with_multiple_ratios() {
        let area = test_area();
        let opts = layout_options_with_column_ratios(&[0.2, 0.3, 0.5]);
        let layouts = columns_with_ratios(&area, 4, opts.column_ratios);

        assert_eq!(layouts.len(), 4);
        // First column: 20% of 1000 = 200
        assert_eq!(layouts[0].right, 200);
        // Second column: 30% of 1000 = 300
        assert_eq!(layouts[1].right, 300);
        // Third column: 50% of 1000 = 500
        // But wait - cumulative is 1.0, so third might be truncated
        // Let's check what actually happens
        // Actually, the sum 0.2 + 0.3 = 0.5 < 1.0, and 0.5 + 0.5 = 1.0
        // So 0.5 won't be included because cumulative would reach 1.0
    }

    #[test]
    fn test_columns_positions_are_correct() {
        let area = test_area();
        let opts = layout_options_with_column_ratios(&[0.3, 0.4]);
        let layouts = columns_with_ratios(&area, 3, opts.column_ratios);

        // First column starts at 0
        assert_eq!(layouts[0].left, 0);
        // Second column starts where first ends
        assert_eq!(layouts[1].left, layouts[0].right);
        // Third column starts where second ends
        assert_eq!(layouts[2].left, layouts[1].left + layouts[1].right);
    }

    #[test]
    fn test_columns_last_column_gets_remaining_space() {
        let area = test_area();
        let opts = layout_options_with_column_ratios(&[0.3]);
        let layouts = columns_with_ratios(&area, 2, opts.column_ratios);

        assert_eq!(layouts.len(), 2);
        // First column: 30% = 300
        assert_eq!(layouts[0].right, 300);
        // Last column gets remaining space: 700
        assert_eq!(layouts[1].right, 700);
    }

    #[test]
    fn test_columns_single_column() {
        let area = test_area();
        let opts = layout_options_with_column_ratios(&[0.5]);
        let layouts = columns_with_ratios(&area, 1, opts.column_ratios);

        assert_eq!(layouts.len(), 1);
        // Single column takes full width regardless of ratio
        assert_eq!(layouts[0].right, 1000);
    }

    #[test]
    fn test_columns_more_columns_than_ratios() {
        let area = test_area();
        let opts = layout_options_with_column_ratios(&[0.2]);
        let layouts = columns_with_ratios(&area, 5, opts.column_ratios);

        assert_eq!(layouts.len(), 5);
        // First column: 20% = 200
        assert_eq!(layouts[0].right, 200);
        // Remaining 800 split among 4 columns = 200 each
        for i in 1..5 {
            assert_eq!(layouts[i].right, 200);
        }
    }

    #[test]
    fn test_columns_cover_full_width_no_ratios() {
        // 1000 / 3 = 333, 333*3 = 999 => 1px remainder
        let area = test_area();
        let layouts = columns_with_ratios(&area, 3, None);

        let total_width: i32 = layouts.iter().map(|r| r.right).sum();
        assert_eq!(
            total_width, area.right,
            "columns should cover full width, got {total_width} expected {}",
            area.right,
        );

        let last = layouts.last().unwrap();
        let right_edge = last.left + last.right;
        assert_eq!(right_edge, area.left + area.right);
    }

    #[test]
    fn test_columns_cover_full_width_with_ratios() {
        // ratio=0.3 with 4 columns: col0=300, remaining 700/3=233, 233*3=699 => 1px remainder
        let area = test_area();
        let opts = layout_options_with_column_ratios(&[0.3]);
        let layouts = columns_with_ratios(&area, 4, opts.column_ratios);

        let total_width: i32 = layouts.iter().map(|r| r.right).sum();
        assert_eq!(
            total_width, area.right,
            "columns should cover full width, got {total_width} expected {}",
            area.right,
        );

        let last = layouts.last().unwrap();
        let right_edge = last.left + last.right;
        assert_eq!(right_edge, area.left + area.right);
    }
}

mod rows_with_ratios_tests {
    use super::*;

    #[test]
    fn test_rows_equal_height_no_ratios() {
        let area = test_area();
        let layouts = rows_with_ratios(&area, 4, None);

        assert_eq!(layouts.len(), 4);
        // Each row should be 200 pixels tall (800 / 4)
        for layout in &layouts {
            assert_eq!(layout.bottom, 200);
            assert_eq!(layout.right, 1000);
        }
    }

    #[test]
    fn test_rows_with_single_ratio() {
        let area = test_area();
        let opts = layout_options_with_row_ratios(&[0.5]);
        let layouts = rows_with_ratios(&area, 3, opts.row_ratios);

        assert_eq!(layouts.len(), 3);
        // First row: 50% of 800 = 400
        assert_eq!(layouts[0].bottom, 400);
        // Remaining 400 split between 2 rows = 200 each
        assert_eq!(layouts[1].bottom, 200);
        assert_eq!(layouts[2].bottom, 200);
    }

    #[test]
    fn test_rows_positions_are_correct() {
        let area = test_area();
        let opts = layout_options_with_row_ratios(&[0.25, 0.25]);
        let layouts = rows_with_ratios(&area, 3, opts.row_ratios);

        // First row starts at top
        assert_eq!(layouts[0].top, 0);
        // Second row starts where first ends
        assert_eq!(layouts[1].top, layouts[0].bottom);
        // Third row starts where second ends
        assert_eq!(layouts[2].top, layouts[1].top + layouts[1].bottom);
    }

    #[test]
    fn test_rows_last_row_gets_remaining_space() {
        let area = test_area();
        let opts = layout_options_with_row_ratios(&[0.25]);
        let layouts = rows_with_ratios(&area, 2, opts.row_ratios);

        assert_eq!(layouts.len(), 2);
        // First row: 25% of 800 = 200
        assert_eq!(layouts[0].bottom, 200);
        // Last row gets remaining: 600
        assert_eq!(layouts[1].bottom, 600);
    }

    #[test]
    fn test_rows_cover_full_height_no_ratios() {
        // 800 / 3 = 266, 266*3 = 798 => 2px remainder
        let area = test_area();
        let layouts = rows_with_ratios(&area, 3, None);

        let total_height: i32 = layouts.iter().map(|r| r.bottom).sum();
        assert_eq!(
            total_height, area.bottom,
            "rows should cover full height, got {total_height} expected {}",
            area.bottom,
        );

        let last = layouts.last().unwrap();
        let bottom_edge = last.top + last.bottom;
        assert_eq!(bottom_edge, area.top + area.bottom);
    }

    #[test]
    fn test_rows_cover_full_height_with_ratios() {
        // ratio=0.3 with 4 rows: row0=240, remaining 560/3=186, 186*3=558 => 2px remainder
        let area = test_area();
        let opts = layout_options_with_row_ratios(&[0.3]);
        let layouts = rows_with_ratios(&area, 4, opts.row_ratios);

        let total_height: i32 = layouts.iter().map(|r| r.bottom).sum();
        assert_eq!(
            total_height, area.bottom,
            "rows should cover full height, got {total_height} expected {}",
            area.bottom,
        );

        let last = layouts.last().unwrap();
        let bottom_edge = last.top + last.bottom;
        assert_eq!(bottom_edge, area.top + area.bottom);
    }
}

mod vertical_stack_layout_tests {
    use super::*;

    #[test]
    fn test_vertical_stack_default_ratio() {
        let area = test_area();
        let len = NonZeroUsize::new(3).unwrap();
        let layouts =
            DefaultLayout::VerticalStack.calculate(&area, len, None, None, &[], 0, None, &[]);

        assert_eq!(layouts.len(), 3);
        // Primary column should be 50% (default ratio)
        assert_eq!(layouts[0].right, 500);
    }

    #[test]
    fn test_vertical_stack_custom_ratio() {
        let area = test_area();
        let len = NonZeroUsize::new(3).unwrap();
        let opts = layout_options_with_column_ratios(&[0.7]);
        let layouts =
            DefaultLayout::VerticalStack.calculate(&area, len, None, None, &[], 0, Some(opts), &[]);

        assert_eq!(layouts.len(), 3);
        // Primary column should be 70%
        assert_eq!(layouts[0].right, 700);
        // Stack columns should share remaining 30%
        assert_eq!(layouts[1].right, 300);
        assert_eq!(layouts[2].right, 300);
    }

    #[test]
    fn test_vertical_stack_with_row_ratios() {
        let area = test_area();
        let len = NonZeroUsize::new(4).unwrap();
        let opts = layout_options_with_ratios(&[0.6], &[0.5, 0.3]);
        let layouts =
            DefaultLayout::VerticalStack.calculate(&area, len, None, None, &[], 0, Some(opts), &[]);

        assert_eq!(layouts.len(), 4);
        // Primary column: 60%
        assert_eq!(layouts[0].right, 600);
        // Stack rows should use row_ratios
        // First stack row: 50% of 800 = 400
        assert_eq!(layouts[1].bottom, 400);
        // Second stack row: 30% of 800 = 240
        assert_eq!(layouts[2].bottom, 240);
    }

    #[test]
    fn test_vertical_stack_single_window() {
        let area = test_area();
        let len = NonZeroUsize::new(1).unwrap();
        let opts = layout_options_with_column_ratios(&[0.6]);
        let layouts =
            DefaultLayout::VerticalStack.calculate(&area, len, None, None, &[], 0, Some(opts), &[]);

        assert_eq!(layouts.len(), 1);
        // Single window should take full width
        assert_eq!(layouts[0].right, 1000);
    }
}

mod horizontal_stack_layout_tests {
    use super::*;

    #[test]
    fn test_horizontal_stack_default_ratio() {
        let area = test_area();
        let len = NonZeroUsize::new(3).unwrap();
        let layouts =
            DefaultLayout::HorizontalStack.calculate(&area, len, None, None, &[], 0, None, &[]);

        assert_eq!(layouts.len(), 3);
        // Primary row should be 50% height (default ratio)
        assert_eq!(layouts[0].bottom, 400);
    }

    #[test]
    fn test_horizontal_stack_custom_ratio() {
        let area = test_area();
        let len = NonZeroUsize::new(3).unwrap();
        let opts = layout_options_with_row_ratios(&[0.7]);
        let layouts = DefaultLayout::HorizontalStack.calculate(
            &area,
            len,
            None,
            None,
            &[],
            0,
            Some(opts),
            &[],
        );

        assert_eq!(layouts.len(), 3);
        // Primary row should be 70% height
        assert_eq!(layouts[0].bottom, 560);
    }

    #[test]
    fn test_horizontal_stack_columns_cover_full_width() {
        // 4 windows: primary row + 3 stack columns
        // stack width = 1000, 1000/3 = 333, 333*3 = 999 => 1px gap
        let area = test_area();
        let len = NonZeroUsize::new(4).unwrap();
        let layouts =
            DefaultLayout::HorizontalStack.calculate(&area, len, None, None, &[], 0, None, &[]);

        // Stack windows (indices 1..4) share the bottom row
        let stack = &layouts[1..];
        let last = stack.last().unwrap();
        let right_edge = last.left + last.right;
        assert_eq!(
            right_edge,
            area.left + area.right,
            "stack columns should cover full width, right edge is {right_edge} expected {}",
            area.left + area.right,
        );
    }
}

mod vertical_stack_rows_cover_full_height_tests {
    use super::*;

    #[test]
    fn test_vertical_stack_rows_cover_full_height() {
        // 4 windows: primary column + 3 stack rows
        // stack height = 800, 800/3 = 266, 266*3 = 798 => 2px gap
        let area = test_area();
        let len = NonZeroUsize::new(4).unwrap();
        let layouts =
            DefaultLayout::VerticalStack.calculate(&area, len, None, None, &[], 0, None, &[]);

        // Stack windows (indices 1..4) share the right column
        let stack = &layouts[1..];
        let last = stack.last().unwrap();
        let bottom_edge = last.top + last.bottom;
        assert_eq!(
            bottom_edge,
            area.top + area.bottom,
            "stack rows should cover full height, bottom edge is {bottom_edge} expected {}",
            area.top + area.bottom,
        );
    }
}

mod scrolling_layout_tests {
    use super::*;

    #[test]
    fn test_scrolling_visible_columns_cover_full_width() {
        // 1921 / 3 = 640, 640*3 = 1920 => 1px gap
        let area = Rect {
            left: 0,
            top: 0,
            right: 1921,
            bottom: 800,
        };
        let len = NonZeroUsize::new(5).unwrap();
        let opts = LayoutOptions {
            scrolling: Some(crate::ScrollingLayoutOptions {
                columns: 3,
                center_focused_column: None,
            }),
            grid: None,
            column_ratios: None,
            row_ratios: None,
        };
        let layouts =
            DefaultLayout::Scrolling.calculate(&area, len, None, None, &[], 0, Some(opts), &[]);

        // First 3 windows should be visible (focused_idx=0)
        let visible = &layouts[0..3];
        let last_visible = visible.last().unwrap();
        let right_edge = last_visible.left + last_visible.right;
        assert_eq!(
            right_edge,
            area.left + area.right,
            "visible columns should cover full width, right edge is {right_edge} expected {}",
            area.left + area.right,
        );
    }
}

mod ultrawide_layout_tests {
    use super::*;

    #[test]
    fn test_ultrawide_default_ratios() {
        let area = test_area();
        let len = NonZeroUsize::new(3).unwrap();
        let layouts = DefaultLayout::UltrawideVerticalStack.calculate(
            &area,
            len,
            None,
            None,
            &[],
            0,
            None,
            &[],
        );

        assert_eq!(layouts.len(), 3);
        // Primary (center): 50% = 500
        assert_eq!(layouts[0].right, 500);
        // Secondary (left): 25% = 250
        assert_eq!(layouts[1].right, 250);
        // Tertiary gets remaining: 250
        assert_eq!(layouts[2].right, 250);
    }

    #[test]
    fn test_ultrawide_custom_ratios() {
        let area = test_area();
        let len = NonZeroUsize::new(4).unwrap();
        let opts = layout_options_with_column_ratios(&[0.5, 0.2]);
        let layouts = DefaultLayout::UltrawideVerticalStack.calculate(
            &area,
            len,
            None,
            None,
            &[],
            0,
            Some(opts),
            &[],
        );

        assert_eq!(layouts.len(), 4);
        // Primary (center): 50% = 500
        assert_eq!(layouts[0].right, 500);
        // Secondary (left): 20% = 200
        assert_eq!(layouts[1].right, 200);
        // Tertiary column gets remaining: 300
        assert_eq!(layouts[2].right, 300);
        assert_eq!(layouts[3].right, 300);
    }

    #[test]
    fn test_ultrawide_two_windows() {
        let area = test_area();
        let len = NonZeroUsize::new(2).unwrap();
        let opts = layout_options_with_column_ratios(&[0.6]);
        let layouts = DefaultLayout::UltrawideVerticalStack.calculate(
            &area,
            len,
            None,
            None,
            &[],
            0,
            Some(opts),
            &[],
        );

        assert_eq!(layouts.len(), 2);
        // Primary: 60% = 600
        assert_eq!(layouts[0].right, 600);
        // Secondary gets remaining: 400
        assert_eq!(layouts[1].right, 400);
    }
}

mod bsp_layout_tests {
    use super::*;

    #[test]
    fn test_bsp_default_ratio() {
        let area = test_area();
        let len = NonZeroUsize::new(2).unwrap();
        let layouts = DefaultLayout::BSP.calculate(&area, len, None, None, &[], 0, None, &[]);

        assert_eq!(layouts.len(), 2);
        // First window should be 50% width
        assert_eq!(layouts[0].right, 500);
    }

    #[test]
    fn test_bsp_custom_column_ratio() {
        let area = test_area();
        let len = NonZeroUsize::new(2).unwrap();
        let opts = layout_options_with_column_ratios(&[0.7]);
        let layouts = DefaultLayout::BSP.calculate(&area, len, None, None, &[], 0, Some(opts), &[]);

        assert_eq!(layouts.len(), 2);
        // First window should be 70% width
        assert_eq!(layouts[0].right, 700);
    }

    #[test]
    fn test_bsp_custom_row_ratio() {
        let area = test_area();
        let len = NonZeroUsize::new(3).unwrap();
        let opts = layout_options_with_ratios(&[0.5], &[0.7]);
        let layouts = DefaultLayout::BSP.calculate(&area, len, None, None, &[], 0, Some(opts), &[]);

        assert_eq!(layouts.len(), 3);
        // Second window should be 70% of remaining height
        assert_eq!(layouts[1].bottom, 560);
    }

    #[test]
    fn test_bsp_horizontal_flip_no_gap_with_resize() {
        let area = test_area();
        let len = NonZeroUsize::new(2).unwrap();
        let opts = layout_options_with_column_ratios(&[0.7]);

        // Container 0 resized right by 50
        let resize = [
            Some(Rect {
                left: 0,
                top: 0,
                right: 50,
                bottom: 0,
            }),
            None,
        ];

        let layouts = DefaultLayout::BSP.calculate(
            &area,
            len,
            None,
            Some(Axis::Horizontal),
            &resize,
            0,
            Some(opts),
            &[],
        );

        assert_eq!(layouts.len(), 2);
        assert_containers_adjacent_horizontally(&layouts, &area);
    }

    #[test]
    fn test_bsp_vertical_flip_no_gap_with_resize() {
        let area = test_area();
        let len = NonZeroUsize::new(3).unwrap();
        let opts = layout_options_with_ratios(&[0.5], &[0.7]);

        // Container 1 resized bottom by 50
        let resize = [
            None,
            Some(Rect {
                left: 0,
                top: 0,
                right: 0,
                bottom: 50,
            }),
            None,
        ];

        let layouts = DefaultLayout::BSP.calculate(
            &area,
            len,
            None,
            Some(Axis::Vertical),
            &resize,
            0,
            Some(opts),
            &[],
        );

        assert_eq!(layouts.len(), 3);
        // Containers 1 and 2 share the right column vertically
        assert_containers_adjacent_vertically(
            &layouts[1..],
            &Rect {
                left: layouts[1].left,
                top: area.top,
                right: layouts[1].right,
                bottom: area.bottom,
            },
        );
    }

    #[test]
    fn test_bsp_horizontal_and_vertical_flip_no_gap_with_resize() {
        let area = test_area();
        let len = NonZeroUsize::new(3).unwrap();
        let opts = layout_options_with_ratios(&[0.7], &[0.7]);

        // Both containers resized
        let resize = [
            Some(Rect {
                left: 0,
                top: 0,
                right: 50,
                bottom: 0,
            }),
            Some(Rect {
                left: 0,
                top: 0,
                right: 0,
                bottom: 40,
            }),
            None,
        ];

        let layouts = DefaultLayout::BSP.calculate(
            &area,
            len,
            None,
            Some(Axis::HorizontalAndVertical),
            &resize,
            0,
            Some(opts),
            &[],
        );

        assert_eq!(layouts.len(), 3);
        assert_containers_adjacent_horizontally(&[layouts[0], layouts[1]], &area);
        assert_containers_adjacent_vertically(
            &layouts[1..],
            &Rect {
                left: layouts[1].left,
                top: area.top,
                right: layouts[1].right,
                bottom: area.bottom,
            },
        );
    }

    #[test]
    fn test_bsp_flip_no_gap_across_multiple_ratios() {
        let area = test_area();

        for &ratio in &[0.3, 0.4, 0.6, 0.7, 0.8] {
            let opts = layout_options_with_column_ratios(&[ratio]);
            let len = NonZeroUsize::new(2).unwrap();

            for &delta in &[25, 50, 100] {
                let resize = [
                    Some(Rect {
                        left: 0,
                        top: 0,
                        right: delta,
                        bottom: 0,
                    }),
                    None,
                ];

                let layouts = DefaultLayout::BSP.calculate(
                    &area,
                    len,
                    None,
                    Some(Axis::Horizontal),
                    &resize,
                    0,
                    Some(opts),
                    &[],
                );

                assert_containers_adjacent_horizontally(&layouts, &area);
            }
        }
    }
}

mod right_main_vertical_stack_tests {
    use super::*;

    #[test]
    fn test_right_main_default_ratio() {
        let area = test_area();
        let len = NonZeroUsize::new(3).unwrap();
        let layouts = DefaultLayout::RightMainVerticalStack.calculate(
            &area,
            len,
            None,
            None,
            &[],
            0,
            None,
            &[],
        );

        assert_eq!(layouts.len(), 3);
        // Primary should be on the right, 50% width
        assert_eq!(layouts[0].right, 500);
        assert_eq!(layouts[0].left, 500); // Right side
    }

    #[test]
    fn test_right_main_custom_ratio() {
        let area = test_area();
        let len = NonZeroUsize::new(3).unwrap();
        let opts = layout_options_with_column_ratios(&[0.6]);
        let layouts = DefaultLayout::RightMainVerticalStack.calculate(
            &area,
            len,
            None,
            None,
            &[],
            0,
            Some(opts),
            &[],
        );

        assert_eq!(layouts.len(), 3);
        // Primary: 60% = 600
        assert_eq!(layouts[0].right, 600);
        // Should be positioned on the right
        assert_eq!(layouts[0].left, 400);
    }
}

mod columns_layout_tests {
    use super::*;

    #[test]
    fn test_columns_layout_with_ratios() {
        let area = test_area();
        let len = NonZeroUsize::new(3).unwrap();
        let opts = layout_options_with_column_ratios(&[0.2, 0.5]);
        let layouts =
            DefaultLayout::Columns.calculate(&area, len, None, None, &[], 0, Some(opts), &[]);

        assert_eq!(layouts.len(), 3);
        assert_eq!(layouts[0].right, 200); // 20%
        assert_eq!(layouts[1].right, 500); // 50%
        assert_eq!(layouts[2].right, 300); // remaining
    }
}

mod rows_layout_tests {
    use super::*;

    #[test]
    fn test_rows_layout_with_ratios() {
        let area = test_area();
        let len = NonZeroUsize::new(3).unwrap();
        let opts = layout_options_with_row_ratios(&[0.25, 0.5]);
        let layouts =
            DefaultLayout::Rows.calculate(&area, len, None, None, &[], 0, Some(opts), &[]);

        assert_eq!(layouts.len(), 3);
        assert_eq!(layouts[0].bottom, 200); // 25%
        assert_eq!(layouts[1].bottom, 400); // 50%
        assert_eq!(layouts[2].bottom, 200); // remaining
    }
}

mod grid_layout_tests {
    use super::*;

    #[test]
    fn test_grid_with_column_ratios() {
        let area = test_area();
        let len = NonZeroUsize::new(4).unwrap();
        let opts = layout_options_with_column_ratios(&[0.3]);
        let layouts =
            DefaultLayout::Grid.calculate(&area, len, None, None, &[], 0, Some(opts), &[]);

        assert_eq!(layouts.len(), 4);
        // Grid with 4 windows should be 2x2
        // First column: 30% = 300
        assert_eq!(layouts[0].right, 300);
        assert_eq!(layouts[1].right, 300);
    }

    #[test]
    fn test_grid_without_ratios() {
        let area = test_area();
        let len = NonZeroUsize::new(4).unwrap();
        let layouts = DefaultLayout::Grid.calculate(&area, len, None, None, &[], 0, None, &[]);

        assert_eq!(layouts.len(), 4);
        // 2x2 grid, equal columns = 500 each
        assert_eq!(layouts[0].right, 500);
        assert_eq!(layouts[2].right, 500);
    }

    #[test]
    fn test_grid_flip_horizontal_with_ratios_no_overlap() {
        // 4 windows => 2x2 grid, column_ratios=[0.3]
        // col 0: width=300 (30%), col 1: width=700 (remaining)
        // With horizontal flip: col 1 (width 700) at left=0, col 0 (width 300) at left=700
        let area = test_area(); // 1000x800
        let opts = layout_options_with_column_ratios(&[0.3]);
        let layouts = DefaultLayout::Grid.calculate(
            &area,
            NonZeroUsize::new(4).unwrap(),
            None,
            Some(Axis::Horizontal),
            &[],
            0,
            Some(opts),
            &[],
        );

        assert_eq!(layouts.len(), 4);

        // Group by left position
        let mut columns: std::collections::BTreeMap<i32, Vec<&Rect>> =
            std::collections::BTreeMap::new();
        for layout in &layouts {
            columns.entry(layout.left).or_default().push(layout);
        }

        assert_eq!(
            columns.len(),
            2,
            "expected 2 columns, got {:?}",
            columns.keys().collect::<Vec<_>>()
        );

        // No container should overlap with any other
        for (i, a) in layouts.iter().enumerate() {
            for (j, b) in layouts.iter().enumerate() {
                if i >= j {
                    continue;
                }
                let h_overlap = a.left < b.left + b.right && b.left < a.left + a.right;
                let v_overlap = a.top < b.top + b.bottom && b.top < a.top + a.bottom;
                assert!(
                    !(h_overlap && v_overlap),
                    "containers {i} and {j} overlap: {a:?} vs {b:?}"
                );
            }
        }

        // Columns should tile the full width with no gaps
        let col_entries: Vec<_> = columns.iter().collect();
        let first_left = *col_entries[0].0;
        let last = col_entries.last().unwrap();
        let last_right_edge = last.0 + last.1[0].right;
        assert_eq!(
            first_left, area.left,
            "first column should start at area.left"
        );
        assert_eq!(
            last_right_edge,
            area.left + area.right,
            "last column should reach area right edge"
        );
    }

    #[test]
    fn test_grid_flip_all_axes_with_ratios_no_overlap() {
        let area = test_area();
        let opts = layout_options_with_column_ratios(&[0.3]);

        for flip in [
            Axis::Horizontal,
            Axis::Vertical,
            Axis::HorizontalAndVertical,
        ] {
            let layouts = DefaultLayout::Grid.calculate(
                &area,
                NonZeroUsize::new(4).unwrap(),
                None,
                Some(flip),
                &[],
                0,
                Some(opts),
                &[],
            );

            for (i, a) in layouts.iter().enumerate() {
                for (j, b) in layouts.iter().enumerate() {
                    if i >= j {
                        continue;
                    }
                    let h_overlap = a.left < b.left + b.right && b.left < a.left + a.right;
                    let v_overlap = a.top < b.top + b.bottom && b.top < a.top + a.bottom;
                    assert!(
                        !(h_overlap && v_overlap),
                        "{flip:?}: containers {i} and {j} overlap: {a:?} vs {b:?}"
                    );
                }
            }

            // All containers should cover the full area
            let total_area: i64 = layouts
                .iter()
                .map(|r| r.right as i64 * r.bottom as i64)
                .sum();
            assert_eq!(
                total_area,
                area.right as i64 * area.bottom as i64,
                "{flip:?}: total container area doesn't match grid area"
            );
        }
    }

    #[test]
    fn test_grid_uneven_rows_cover_full_height() {
        // 7 windows => ceil(sqrt(7)) = 3 columns
        // Distribution: col0=2 rows, col1=2 rows, col2=3 rows
        // With area.bottom=800:
        //   2-row columns: 800/2=400 each, total=800 (ok)
        //   3-row column:  800/3=266 each, total=798 (2px gap!)
        let area = Rect {
            left: 0,
            top: 0,
            right: 1200,
            bottom: 800,
        };
        let layouts = DefaultLayout::Grid.calculate(
            &area,
            NonZeroUsize::new(7).unwrap(),
            None,
            None,
            &[],
            0,
            None,
            &[],
        );

        assert_eq!(layouts.len(), 7);

        // Group windows by column (by their left position)
        let mut columns: std::collections::BTreeMap<i32, Vec<&Rect>> =
            std::collections::BTreeMap::new();
        for layout in &layouts {
            columns.entry(layout.left).or_default().push(layout);
        }

        // Every column's windows should cover the full area height
        for (&col_left, windows) in &columns {
            // Sort by top position
            let mut sorted: Vec<&&Rect> = windows.iter().collect();
            sorted.sort_by_key(|w| w.top);

            // First window should start at area.top
            assert_eq!(
                sorted[0].top, area.top,
                "column at left={col_left}: first window should start at area.top"
            );

            // Last window's bottom edge should reach area.bottom
            let last = sorted.last().unwrap();
            let bottom_edge = last.top + last.bottom;
            assert_eq!(
                bottom_edge,
                area.bottom,
                "column at left={col_left} ({} rows): bottom edge is {bottom_edge}, \
                 expected {}. Gap of {} pixels",
                windows.len(),
                area.bottom,
                area.bottom - bottom_edge,
            );
        }
    }

    #[test]
    fn test_grid_uneven_rows_cover_full_height_with_vertical_flip() {
        let area = Rect {
            left: 0,
            top: 0,
            right: 1200,
            bottom: 800,
        };

        for flip in [Axis::Vertical, Axis::HorizontalAndVertical] {
            let layouts = DefaultLayout::Grid.calculate(
                &area,
                NonZeroUsize::new(7).unwrap(),
                None,
                Some(flip),
                &[],
                0,
                None,
                &[],
            );

            let mut columns: std::collections::BTreeMap<i32, Vec<&Rect>> =
                std::collections::BTreeMap::new();
            for layout in &layouts {
                columns.entry(layout.left).or_default().push(layout);
            }

            for (&col_left, windows) in &columns {
                let mut sorted: Vec<&&Rect> = windows.iter().collect();
                sorted.sort_by_key(|w| w.top);

                assert_eq!(
                    sorted[0].top, area.top,
                    "{flip:?}: column at left={col_left}: first window should start at area.top"
                );

                let last = sorted.last().unwrap();
                let bottom_edge = last.top + last.bottom;
                assert_eq!(
                    bottom_edge,
                    area.bottom,
                    "{flip:?}: column at left={col_left} ({} rows): bottom edge is {bottom_edge}, \
                     expected {}. Gap of {} pixels",
                    windows.len(),
                    area.bottom,
                    area.bottom - bottom_edge,
                );

                // Adjacent windows within the column should have no gaps
                for pair in sorted.windows(2) {
                    let edge = pair[0].top + pair[0].bottom;
                    assert_eq!(
                        edge, pair[1].top,
                        "{flip:?}: column at left={col_left}: gap between rows at y={edge} and y={}",
                        pair[1].top,
                    );
                }
            }
        }
    }

    #[test]
    fn test_grid_uneven_columns_cover_full_width() {
        // 5 windows => ceil(sqrt(5)) = 3 columns
        // With area.right=1000: 1000/3=333 each, total=999 (1px gap!)
        let area = Rect {
            left: 0,
            top: 0,
            right: 1000,
            bottom: 800,
        };
        let layouts = DefaultLayout::Grid.calculate(
            &area,
            NonZeroUsize::new(5).unwrap(),
            None,
            None,
            &[],
            0,
            None,
            &[],
        );

        assert_eq!(layouts.len(), 5);

        // Group windows by column (by their left position)
        let mut columns: std::collections::BTreeMap<i32, Vec<&Rect>> =
            std::collections::BTreeMap::new();
        for layout in &layouts {
            columns.entry(layout.left).or_default().push(layout);
        }

        // First column should start at area.left
        let first_left = *columns.keys().next().unwrap();
        assert_eq!(
            first_left, area.left,
            "first column should start at area.left"
        );

        // Last column's right edge should reach area.right
        let (&last_left, last_windows) = columns.iter().last().unwrap();
        let last_right_edge = last_left + last_windows[0].right;
        assert_eq!(
            last_right_edge,
            area.left + area.right,
            "last column right edge is {last_right_edge}, expected {}. Gap of {} pixels",
            area.left + area.right,
            area.left + area.right - last_right_edge,
        );

        // Adjacent columns should have no gaps
        let col_entries: Vec<_> = columns.iter().collect();
        for pair in col_entries.windows(2) {
            let (&left_a, windows_a) = pair[0];
            let (&left_b, _) = pair[1];
            let right_edge_a = left_a + windows_a[0].right;
            assert_eq!(
                right_edge_a, left_b,
                "gap between columns at x={right_edge_a} and x={left_b}",
            );
        }
    }

    #[test]
    fn test_grid_uneven_columns_cover_full_width_with_horizontal_flip() {
        let area = Rect {
            left: 0,
            top: 0,
            right: 1000,
            bottom: 800,
        };

        for flip in [Axis::Horizontal, Axis::HorizontalAndVertical] {
            let layouts = DefaultLayout::Grid.calculate(
                &area,
                NonZeroUsize::new(5).unwrap(),
                None,
                Some(flip),
                &[],
                0,
                None,
                &[],
            );

            let mut columns: std::collections::BTreeMap<i32, Vec<&Rect>> =
                std::collections::BTreeMap::new();
            for layout in &layouts {
                columns.entry(layout.left).or_default().push(layout);
            }

            let first_left = *columns.keys().next().unwrap();
            assert_eq!(
                first_left, area.left,
                "{flip:?}: first column should start at area.left"
            );

            let (&last_left, last_windows) = columns.iter().last().unwrap();
            let last_right_edge = last_left + last_windows[0].right;
            assert_eq!(
                last_right_edge,
                area.left + area.right,
                "{flip:?}: last column right edge is {last_right_edge}, expected {}. Gap of {} pixels",
                area.left + area.right,
                area.left + area.right - last_right_edge,
            );

            // Adjacent columns should have no gaps
            let col_entries: Vec<_> = columns.iter().collect();
            for pair in col_entries.windows(2) {
                let (&left_a, windows_a) = pair[0];
                let (&left_b, _) = pair[1];
                let right_edge_a = left_a + windows_a[0].right;
                assert_eq!(
                    right_edge_a, left_b,
                    "{flip:?}: gap between columns at x={right_edge_a} and x={left_b}",
                );
            }
        }
    }
}

mod layout_flip_tests {
    use super::*;

    #[test]
    fn test_columns_flip_horizontal() {
        let area = test_area();
        let len = NonZeroUsize::new(3).unwrap();
        let opts = layout_options_with_column_ratios(&[0.2, 0.3]);
        let layouts = DefaultLayout::Columns.calculate(
            &area,
            len,
            None,
            Some(Axis::Horizontal),
            &[],
            0,
            Some(opts),
            &[],
        );

        assert_eq!(layouts.len(), 3);
        // Columns should be reversed
        // Last column (originally 50%) should now be first
        assert_eq!(layouts[2].left, 0);
    }

    #[test]
    fn test_rows_flip_vertical() {
        let area = test_area();
        let len = NonZeroUsize::new(3).unwrap();
        let opts = layout_options_with_row_ratios(&[0.25, 0.5]);
        let layouts = DefaultLayout::Rows.calculate(
            &area,
            len,
            None,
            Some(Axis::Vertical),
            &[],
            0,
            Some(opts),
            &[],
        );

        assert_eq!(layouts.len(), 3);
        // Rows should be reversed
        // Last row should now be at top
        assert_eq!(layouts[2].top, 0);
    }
}

mod flip_remainder_coverage_tests {
    use super::*;

    /// Verify that layouts tile the full area with no gaps after flipping.
    /// Checks that the leftmost edge == area.left, rightmost edge == area.left + area.right,
    /// topmost edge == area.top, bottommost edge == area.top + area.bottom,
    /// and no two windows overlap.
    fn assert_full_coverage(layouts: &[Rect], area: &Rect, label: &str) {
        assert!(!layouts.is_empty(), "{label}: no layouts produced");

        let left_edge = layouts.iter().map(|r| r.left).min().unwrap();
        let top_edge = layouts.iter().map(|r| r.top).min().unwrap();
        let right_edge = layouts.iter().map(|r| r.left + r.right).max().unwrap();
        let bottom_edge = layouts.iter().map(|r| r.top + r.bottom).max().unwrap();

        assert_eq!(left_edge, area.left, "{label}: left edge gap");
        assert_eq!(top_edge, area.top, "{label}: top edge gap");
        assert_eq!(
            right_edge,
            area.left + area.right,
            "{label}: right edge gap of {} pixels",
            area.left + area.right - right_edge,
        );
        assert_eq!(
            bottom_edge,
            area.top + area.bottom,
            "{label}: bottom edge gap of {} pixels",
            area.top + area.bottom - bottom_edge,
        );

        // No overlaps
        for (i, a) in layouts.iter().enumerate() {
            for (j, b) in layouts.iter().enumerate() {
                if i >= j {
                    continue;
                }
                let h = a.left < b.left + b.right && b.left < a.left + a.right;
                let v = a.top < b.top + b.bottom && b.top < a.top + a.bottom;
                assert!(
                    !(h && v),
                    "{label}: windows {i} and {j} overlap: {a:?} vs {b:?}"
                );
            }
        }
    }

    // Area whose dimensions are not evenly divisible by 3
    fn uneven_area() -> Rect {
        Rect {
            left: 0,
            top: 0,
            right: 1000, // 1000/3 = 333 rem 1
            bottom: 800, // 800/3 = 266 rem 2
        }
    }

    #[test]
    fn test_columns_flipped_cover_full_area() {
        let area = uneven_area();
        let len = NonZeroUsize::new(3).unwrap();
        for flip in [Axis::Horizontal, Axis::HorizontalAndVertical] {
            let layouts =
                DefaultLayout::Columns.calculate(&area, len, None, Some(flip), &[], 0, None, &[]);
            assert_full_coverage(&layouts, &area, &format!("Columns {flip:?}"));
        }
    }

    #[test]
    fn test_rows_flipped_cover_full_area() {
        let area = uneven_area();
        let len = NonZeroUsize::new(3).unwrap();
        for flip in [Axis::Vertical, Axis::HorizontalAndVertical] {
            let layouts =
                DefaultLayout::Rows.calculate(&area, len, None, Some(flip), &[], 0, None, &[]);
            assert_full_coverage(&layouts, &area, &format!("Rows {flip:?}"));
        }
    }

    #[test]
    fn test_vertical_stack_flipped_cover_full_area() {
        let area = uneven_area();
        // 4 windows: 1 primary + 3 stack rows (triggers remainder in rows_with_ratios)
        let len = NonZeroUsize::new(4).unwrap();
        for flip in [
            Axis::Horizontal,
            Axis::Vertical,
            Axis::HorizontalAndVertical,
        ] {
            let layouts = DefaultLayout::VerticalStack.calculate(
                &area,
                len,
                None,
                Some(flip),
                &[],
                0,
                None,
                &[],
            );
            assert_full_coverage(&layouts, &area, &format!("VerticalStack {flip:?}"));
        }
    }

    #[test]
    fn test_horizontal_stack_flipped_cover_full_area() {
        let area = uneven_area();
        // 4 windows: 1 primary + 3 stack columns (triggers remainder in columns_with_ratios)
        let len = NonZeroUsize::new(4).unwrap();
        for flip in [
            Axis::Horizontal,
            Axis::Vertical,
            Axis::HorizontalAndVertical,
        ] {
            let layouts = DefaultLayout::HorizontalStack.calculate(
                &area,
                len,
                None,
                Some(flip),
                &[],
                0,
                None,
                &[],
            );
            assert_full_coverage(&layouts, &area, &format!("HorizontalStack {flip:?}"));
        }
    }

    #[test]
    fn test_right_main_vertical_stack_flipped_cover_full_area() {
        let area = uneven_area();
        let len = NonZeroUsize::new(4).unwrap();
        for flip in [
            Axis::Horizontal,
            Axis::Vertical,
            Axis::HorizontalAndVertical,
        ] {
            let layouts = DefaultLayout::RightMainVerticalStack.calculate(
                &area,
                len,
                None,
                Some(flip),
                &[],
                0,
                None,
                &[],
            );
            assert_full_coverage(&layouts, &area, &format!("RightMainVerticalStack {flip:?}"));
        }
    }

    #[test]
    fn test_ultrawide_vertical_stack_flipped_cover_full_area() {
        let area = uneven_area();
        // 5 windows: primary + secondary + 3 tertiary rows (triggers remainder)
        let len = NonZeroUsize::new(5).unwrap();
        for flip in [
            Axis::Horizontal,
            Axis::Vertical,
            Axis::HorizontalAndVertical,
        ] {
            let layouts = DefaultLayout::UltrawideVerticalStack.calculate(
                &area,
                len,
                None,
                Some(flip),
                &[],
                0,
                None,
                &[],
            );
            assert_full_coverage(&layouts, &area, &format!("UltrawideVerticalStack {flip:?}"));
        }
    }
}

mod container_padding_tests {
    use super::*;

    #[test]
    fn test_padding_applied_to_all_layouts() {
        let area = test_area();
        let len = NonZeroUsize::new(2).unwrap();
        let padding = 10;
        let layouts =
            DefaultLayout::Columns.calculate(&area, len, Some(padding), None, &[], 0, None, &[]);

        assert_eq!(layouts.len(), 2);
        // Each layout should have padding applied
        // left increases, right decreases, top increases, bottom decreases
        assert_eq!(layouts[0].left, padding);
        assert_eq!(layouts[0].top, padding);
        assert_eq!(layouts[0].right, 500 - padding * 2);
        assert_eq!(layouts[0].bottom, 800 - padding * 2);
    }
}

mod flip_resize_adjacency_tests {
    use super::*;

    fn resize_3() -> Vec<Option<Rect>> {
        vec![
            Some(Rect {
                left: 0,
                top: 0,
                right: 50,
                bottom: 0,
            }),
            Some(Rect {
                left: 0,
                top: 0,
                right: 0,
                bottom: 40,
            }),
            None,
        ]
    }

    fn resize_4() -> Vec<Option<Rect>> {
        vec![
            Some(Rect {
                left: 0,
                top: 0,
                right: 50,
                bottom: 0,
            }),
            Some(Rect {
                left: 0,
                top: 0,
                right: 0,
                bottom: 40,
            }),
            None,
            None,
        ]
    }

    #[test]
    fn test_columns_flip_resize_no_gap() {
        let area = test_area();
        let opts = layout_options_with_column_ratios(&[0.3, 0.5]);

        for flip in [Axis::Horizontal, Axis::HorizontalAndVertical] {
            let layouts = DefaultLayout::Columns.calculate(
                &area,
                NonZeroUsize::new(3).unwrap(),
                None,
                Some(flip),
                &resize_3(),
                0,
                Some(opts),
                &[],
            );
            assert_containers_adjacent_horizontally(&layouts, &area);
        }
    }

    #[test]
    fn test_rows_flip_resize_no_gap() {
        let area = test_area();
        let opts = layout_options_with_row_ratios(&[0.3, 0.5]);

        for flip in [Axis::Vertical, Axis::HorizontalAndVertical] {
            let layouts = DefaultLayout::Rows.calculate(
                &area,
                NonZeroUsize::new(3).unwrap(),
                None,
                Some(flip),
                &resize_3(),
                0,
                Some(opts),
                &[],
            );
            assert_containers_adjacent_vertically(&layouts, &area);
        }
    }

    #[test]
    fn test_vertical_stack_flip_resize_no_gap() {
        let area = test_area();
        let opts = layout_options_with_ratios(&[0.7], &[0.4]);

        for flip in [
            Axis::Horizontal,
            Axis::Vertical,
            Axis::HorizontalAndVertical,
        ] {
            let layouts = DefaultLayout::VerticalStack.calculate(
                &area,
                NonZeroUsize::new(4).unwrap(),
                None,
                Some(flip),
                &resize_4(),
                0,
                Some(opts),
                &[],
            );

            // Primary and stack share the horizontal axis
            let primary = &layouts[0];
            let stack = &layouts[1..];

            // All stack elements should be in the same column
            let stack_left = stack[0].left;
            let stack_width = stack[0].right;
            for s in stack {
                assert_eq!(s.left, stack_left);
                assert_eq!(s.right, stack_width);
            }

            // Primary and stack column should be adjacent and fill the area
            if primary.left < stack_left {
                assert_eq!(primary.left + primary.right, stack_left);
                assert_eq!(stack_left + stack_width, area.left + area.right);
            } else {
                assert_eq!(stack_left + stack_width, primary.left);
                assert_eq!(primary.left + primary.right, area.left + area.right);
            }

            // Stack elements should tile vertically
            assert_containers_adjacent_vertically(
                stack,
                &Rect {
                    left: stack_left,
                    top: area.top,
                    right: stack_width,
                    bottom: area.bottom,
                },
            );
        }
    }

    #[test]
    fn test_right_main_vertical_stack_flip_resize_no_gap() {
        let area = test_area();
        let opts = layout_options_with_ratios(&[0.7], &[0.4]);

        for flip in [
            Axis::Horizontal,
            Axis::Vertical,
            Axis::HorizontalAndVertical,
        ] {
            let layouts = DefaultLayout::RightMainVerticalStack.calculate(
                &area,
                NonZeroUsize::new(4).unwrap(),
                None,
                Some(flip),
                &resize_4(),
                0,
                Some(opts),
                &[],
            );

            let primary = &layouts[0];
            let stack = &layouts[1..];

            let stack_left = stack[0].left;
            let stack_width = stack[0].right;
            for s in stack {
                assert_eq!(s.left, stack_left);
                assert_eq!(s.right, stack_width);
            }

            if primary.left < stack_left {
                assert_eq!(primary.left + primary.right, stack_left);
                assert_eq!(stack_left + stack_width, area.left + area.right);
            } else {
                assert_eq!(stack_left + stack_width, primary.left);
                assert_eq!(primary.left + primary.right, area.left + area.right);
            }

            assert_containers_adjacent_vertically(
                stack,
                &Rect {
                    left: stack_left,
                    top: area.top,
                    right: stack_width,
                    bottom: area.bottom,
                },
            );
        }
    }

    #[test]
    fn test_horizontal_stack_flip_resize_no_gap() {
        let area = test_area();
        let opts = layout_options_with_ratios(&[0.3, 0.5], &[0.7]);

        for flip in [
            Axis::Horizontal,
            Axis::Vertical,
            Axis::HorizontalAndVertical,
        ] {
            let layouts = DefaultLayout::HorizontalStack.calculate(
                &area,
                NonZeroUsize::new(4).unwrap(),
                None,
                Some(flip),
                &resize_4(),
                0,
                Some(opts),
                &[],
            );

            let primary = &layouts[0];
            let stack = &layouts[1..];

            // All stack elements should be in the same row
            let stack_top = stack[0].top;
            let stack_height = stack[0].bottom;
            for s in stack {
                assert_eq!(s.top, stack_top);
                assert_eq!(s.bottom, stack_height);
            }

            // Primary and stack row should be adjacent and fill the area
            if primary.top < stack_top {
                assert_eq!(primary.top + primary.bottom, stack_top);
                assert_eq!(stack_top + stack_height, area.top + area.bottom);
            } else {
                assert_eq!(stack_top + stack_height, primary.top);
                assert_eq!(primary.top + primary.bottom, area.top + area.bottom);
            }

            // Stack elements should tile horizontally
            assert_containers_adjacent_horizontally(
                stack,
                &Rect {
                    left: area.left,
                    top: stack_top,
                    right: area.right,
                    bottom: stack_height,
                },
            );
        }
    }

    #[test]
    fn test_ultrawide_vertical_stack_flip_resize_no_gap() {
        let area = test_area();
        let opts = layout_options_with_ratios(&[0.5, 0.2], &[0.6]);

        for flip in [
            Axis::Horizontal,
            Axis::Vertical,
            Axis::HorizontalAndVertical,
        ] {
            let layouts = DefaultLayout::UltrawideVerticalStack.calculate(
                &area,
                NonZeroUsize::new(4).unwrap(),
                None,
                Some(flip),
                &resize_4(),
                0,
                Some(opts),
                &[],
            );

            let primary = &layouts[0];
            let secondary = &layouts[1];
            let tertiary = &layouts[2..];

            // All tertiary elements share the same column
            let tert_left = tertiary[0].left;
            let tert_width = tertiary[0].right;
            for t in tertiary {
                assert_eq!(t.left, tert_left);
                assert_eq!(t.right, tert_width);
            }

            // The three columns (primary, secondary, tertiary) should tile horizontally
            let columns = [
                Rect {
                    left: primary.left,
                    top: 0,
                    right: primary.right,
                    bottom: 0,
                },
                Rect {
                    left: secondary.left,
                    top: 0,
                    right: secondary.right,
                    bottom: 0,
                },
                Rect {
                    left: tert_left,
                    top: 0,
                    right: tert_width,
                    bottom: 0,
                },
            ];
            assert_containers_adjacent_horizontally(&columns, &area);

            // Tertiary elements should tile vertically
            if tertiary.len() > 1 {
                assert_containers_adjacent_vertically(
                    tertiary,
                    &Rect {
                        left: tert_left,
                        top: area.top,
                        right: tert_width,
                        bottom: area.bottom,
                    },
                );
            }
        }
    }

    #[test]
    fn test_scrolling_resize_no_gap() {
        let area = test_area();

        // Scrolling doesn't support flip, but verify resize adjacency
        let layouts = DefaultLayout::Scrolling.calculate(
            &area,
            NonZeroUsize::new(3).unwrap(),
            None,
            None,
            &resize_3(),
            0,
            None,
            &[],
        );

        // Adjacent visible columns should not have gaps
        for pair in layouts.windows(2) {
            assert_eq!(
                pair[0].left + pair[0].right,
                pair[1].left,
                "scrolling gap at left={} (width {}) and left={}",
                pair[0].left,
                pair[0].right,
                pair[1].left,
            );
        }
    }
}
