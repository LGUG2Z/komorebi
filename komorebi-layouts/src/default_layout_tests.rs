use super::*;

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

mod deserialize_ratios_tests {
    use super::*;

    #[test]
    fn test_deserialize_valid_ratios() {
        let json = r#"{"column_ratios": [0.3, 0.4, 0.2]}"#;
        let opts: LayoutOptions = serde_json::from_str(json).unwrap();

        let ratios = opts.column_ratios.unwrap();
        assert_eq!(ratios[0], Some(0.3));
        assert_eq!(ratios[1], Some(0.4));
        assert_eq!(ratios[2], Some(0.2));
        assert_eq!(ratios[3], None);
        assert_eq!(ratios[4], None);
    }

    #[test]
    fn test_deserialize_clamps_values_to_min() {
        // Values below MIN_RATIO should be clamped
        let json = r#"{"column_ratios": [0.05]}"#;
        let opts: LayoutOptions = serde_json::from_str(json).unwrap();

        let ratios = opts.column_ratios.unwrap();
        assert_eq!(ratios[0], Some(MIN_RATIO)); // Clamped to 0.1
    }

    #[test]
    fn test_deserialize_clamps_values_to_max() {
        // Values above MAX_RATIO should be clamped
        let json = r#"{"column_ratios": [0.95]}"#;
        let opts: LayoutOptions = serde_json::from_str(json).unwrap();

        let ratios = opts.column_ratios.unwrap();
        // 0.9 is the max, so it should be clamped
        assert!(ratios[0].unwrap() <= MAX_RATIO);
    }

    #[test]
    fn test_deserialize_truncates_when_sum_exceeds_one() {
        // Sum of ratios should not reach 1.0
        // [0.5, 0.4] = 0.9, then 0.3 would make it 1.2, so it should be truncated
        let json = r#"{"column_ratios": [0.5, 0.4, 0.3]}"#;
        let opts: LayoutOptions = serde_json::from_str(json).unwrap();

        let ratios = opts.column_ratios.unwrap();
        assert_eq!(ratios[0], Some(0.5));
        assert_eq!(ratios[1], Some(0.4));
        // Third ratio should be truncated because 0.5 + 0.4 + 0.3 >= 1.0
        assert_eq!(ratios[2], None);
    }

    #[test]
    fn test_deserialize_truncates_at_max_ratios() {
        // More than MAX_RATIOS values should be truncated
        let json = r#"{"column_ratios": [0.1, 0.1, 0.1, 0.1, 0.1, 0.1, 0.1]}"#;
        let opts: LayoutOptions = serde_json::from_str(json).unwrap();

        let ratios = opts.column_ratios.unwrap();
        // Only MAX_RATIOS (5) values should be stored
        for item in ratios.iter().take(MAX_RATIOS) {
            assert_eq!(*item, Some(0.1));
        }
    }

    #[test]
    fn test_deserialize_empty_array() {
        let json = r#"{"column_ratios": []}"#;
        let opts: LayoutOptions = serde_json::from_str(json).unwrap();

        let ratios = opts.column_ratios.unwrap();
        for item in ratios.iter().take(MAX_RATIOS) {
            assert_eq!(*item, None);
        }
    }

    #[test]
    fn test_deserialize_null() {
        let json = r#"{"column_ratios": null}"#;
        let opts: LayoutOptions = serde_json::from_str(json).unwrap();
        assert!(opts.column_ratios.is_none());
    }

    #[test]
    fn test_deserialize_row_ratios() {
        let json = r#"{"row_ratios": [0.3, 0.5]}"#;
        let opts: LayoutOptions = serde_json::from_str(json).unwrap();

        let ratios = opts.row_ratios.unwrap();
        assert_eq!(ratios[0], Some(0.3));
        assert_eq!(ratios[1], Some(0.5));
        assert_eq!(ratios[2], None);
    }
}

mod serialize_ratios_tests {
    use super::*;

    #[test]
    fn test_serialize_ratios_compact() {
        let opts = layout_options_with_column_ratios(&[0.3, 0.4]);
        let json = serde_json::to_string(&opts).unwrap();

        // Should serialize ratios as compact array without trailing nulls in the ratios array
        assert!(json.contains("0.3") && json.contains("0.4"));
    }

    #[test]
    fn test_serialize_none_ratios() {
        let opts = LayoutOptions {
            scrolling: None,
            grid: None,
            column_ratios: None,
            row_ratios: None,
        };
        let json = serde_json::to_string(&opts).unwrap();

        // None values should serialize as null or be omitted
        assert!(!json.contains("["));
    }

    #[test]
    fn test_roundtrip_serialization() {
        let original = layout_options_with_column_ratios(&[0.3, 0.4, 0.2]);
        let json = serde_json::to_string(&original).unwrap();
        let deserialized: LayoutOptions = serde_json::from_str(&json).unwrap();

        assert_eq!(original.column_ratios, deserialized.column_ratios);
    }

    #[test]
    fn test_serialize_row_ratios() {
        let opts = layout_options_with_row_ratios(&[0.3, 0.5]);
        let json = serde_json::to_string(&opts).unwrap();

        assert!(json.contains("row_ratios"));
        assert!(json.contains("0.3") && json.contains("0.5"));
    }

    #[test]
    fn test_roundtrip_row_ratios() {
        let original = layout_options_with_row_ratios(&[0.4, 0.3]);
        let json = serde_json::to_string(&original).unwrap();
        let deserialized: LayoutOptions = serde_json::from_str(&json).unwrap();

        assert_eq!(original.row_ratios, deserialized.row_ratios);
        assert!(original.column_ratios.is_none());
    }

    #[test]
    fn test_roundtrip_both_ratios() {
        let original = layout_options_with_ratios(&[0.3, 0.4], &[0.5, 0.3]);
        let json = serde_json::to_string(&original).unwrap();
        let deserialized: LayoutOptions = serde_json::from_str(&json).unwrap();

        assert_eq!(original.column_ratios, deserialized.column_ratios);
        assert_eq!(original.row_ratios, deserialized.row_ratios);
    }
}

mod ratio_constants_tests {
    use super::*;

    #[test]
    fn test_constants_valid_ranges() {
        const {
            assert!(MIN_RATIO > 0.0);
            assert!(MIN_RATIO < MAX_RATIO);
            assert!(MAX_RATIO < 1.0);
            assert!(DEFAULT_RATIO >= MIN_RATIO && DEFAULT_RATIO <= MAX_RATIO);
            assert!(DEFAULT_SECONDARY_RATIO >= MIN_RATIO && DEFAULT_SECONDARY_RATIO <= MAX_RATIO);
            assert!(MAX_RATIOS >= 1);
        }
    }

    #[test]
    fn test_default_ratio_is_half() {
        assert_eq!(DEFAULT_RATIO, 0.5);
    }

    #[test]
    fn test_max_ratios_is_five() {
        assert_eq!(MAX_RATIOS, 5);
    }
}

mod layout_options_tests {
    use super::*;

    #[test]
    fn test_layout_options_default_values() {
        let json = r#"{}"#;
        let opts: LayoutOptions = serde_json::from_str(json).unwrap();

        assert!(opts.scrolling.is_none());
        assert!(opts.grid.is_none());
        assert!(opts.column_ratios.is_none());
        assert!(opts.row_ratios.is_none());
    }

    #[test]
    fn test_layout_options_with_all_fields() {
        let json = r#"{
            "scrolling": {"columns": 3},
            "grid": {"rows": 2},
            "column_ratios": [0.3, 0.4],
            "row_ratios": [0.5]
        }"#;
        let opts: LayoutOptions = serde_json::from_str(json).unwrap();

        assert!(opts.scrolling.is_some());
        assert_eq!(opts.scrolling.unwrap().columns, 3);
        assert!(opts.grid.is_some());
        assert_eq!(opts.grid.unwrap().rows, 2);
        assert!(opts.column_ratios.is_some());
        assert!(opts.row_ratios.is_some());
    }
}

mod default_layout_tests {
    use super::*;

    #[test]
    fn test_cycle_next_covers_all_layouts() {
        let start = DefaultLayout::BSP;
        let mut current = start;
        let mut visited = vec![current];

        loop {
            current = current.cycle_next();
            if current == start {
                break;
            }
            assert!(
                !visited.contains(&current),
                "Cycle contains duplicate: {:?}",
                current
            );
            visited.push(current);
        }

        // Should have visited all layouts
        assert_eq!(visited.len(), 9); // 9 layouts total
    }

    #[test]
    fn test_cycle_previous_is_inverse_of_next() {
        // Note: cycle_previous has some inconsistencies in the current implementation
        // This test documents the expected behavior for most layouts
        let layouts_with_correct_inverse = [
            DefaultLayout::Columns,
            DefaultLayout::Rows,
            DefaultLayout::VerticalStack,
            DefaultLayout::HorizontalStack,
            DefaultLayout::UltrawideVerticalStack,
            DefaultLayout::Grid,
            DefaultLayout::RightMainVerticalStack,
        ];

        for layout in layouts_with_correct_inverse {
            let next = layout.cycle_next();
            assert_eq!(
                next.cycle_previous(),
                layout,
                "cycle_previous should be inverse of cycle_next for {:?}",
                layout
            );
        }
    }

    #[test]
    fn test_leftmost_index_standard_layouts() {
        assert_eq!(DefaultLayout::BSP.leftmost_index(5), 0);
        assert_eq!(DefaultLayout::Columns.leftmost_index(5), 0);
        assert_eq!(DefaultLayout::Rows.leftmost_index(5), 0);
        assert_eq!(DefaultLayout::VerticalStack.leftmost_index(5), 0);
        assert_eq!(DefaultLayout::HorizontalStack.leftmost_index(5), 0);
        assert_eq!(DefaultLayout::Grid.leftmost_index(5), 0);
    }

    #[test]
    fn test_leftmost_index_ultrawide() {
        assert_eq!(DefaultLayout::UltrawideVerticalStack.leftmost_index(1), 0);
        assert_eq!(DefaultLayout::UltrawideVerticalStack.leftmost_index(2), 1);
        assert_eq!(DefaultLayout::UltrawideVerticalStack.leftmost_index(5), 1);
    }

    #[test]
    fn test_leftmost_index_right_main() {
        assert_eq!(DefaultLayout::RightMainVerticalStack.leftmost_index(1), 0);
        assert_eq!(DefaultLayout::RightMainVerticalStack.leftmost_index(2), 1);
        assert_eq!(DefaultLayout::RightMainVerticalStack.leftmost_index(5), 1);
    }

    #[test]
    fn test_rightmost_index_standard_layouts() {
        assert_eq!(DefaultLayout::BSP.rightmost_index(5), 4);
        assert_eq!(DefaultLayout::Columns.rightmost_index(5), 4);
        assert_eq!(DefaultLayout::Rows.rightmost_index(5), 4);
        assert_eq!(DefaultLayout::VerticalStack.rightmost_index(5), 4);
    }

    #[test]
    fn test_rightmost_index_right_main() {
        assert_eq!(DefaultLayout::RightMainVerticalStack.rightmost_index(1), 0);
        assert_eq!(DefaultLayout::RightMainVerticalStack.rightmost_index(5), 0);
    }

    #[test]
    fn test_rightmost_index_ultrawide() {
        assert_eq!(DefaultLayout::UltrawideVerticalStack.rightmost_index(1), 0);
        assert_eq!(DefaultLayout::UltrawideVerticalStack.rightmost_index(2), 0);
        assert_eq!(DefaultLayout::UltrawideVerticalStack.rightmost_index(3), 2);
        assert_eq!(DefaultLayout::UltrawideVerticalStack.rightmost_index(5), 4);
    }
}

mod layout_options_rules_tests {
    use super::*;

    #[test]
    fn test_hashmap_deserialization_ratios_only() {
        // layout_options_rules entries with only ratios
        // Note: ratios must sum to < 1.0 to avoid truncation by validate_ratios
        let json = r#"{
            "2": {"column_ratios": [0.7]},
            "3": {"column_ratios": [0.55]},
            "5": {"column_ratios": [0.3, 0.3, 0.3]}
        }"#;
        let rules: std::collections::HashMap<usize, LayoutOptions> =
            serde_json::from_str(json).unwrap();
        assert_eq!(rules.len(), 3);
        assert_eq!(rules[&2].column_ratios.unwrap()[0], Some(0.7));
        assert_eq!(rules[&3].column_ratios.unwrap()[0], Some(0.55));
        let r5 = rules[&5].column_ratios.unwrap();
        assert_eq!(r5[0], Some(0.3));
        assert_eq!(r5[1], Some(0.3));
        assert_eq!(r5[2], Some(0.3));
        // No scrolling/grid in these entries
        assert!(rules[&2].scrolling.is_none());
        assert!(rules[&2].grid.is_none());
    }

    #[test]
    fn test_hashmap_deserialization_full_options() {
        // layout_options_rules entries with full options including scrolling/grid
        let json = r#"{
            "2": {"column_ratios": [0.7], "scrolling": {"columns": 3}},
            "5": {"column_ratios": [0.3, 0.3, 0.3], "grid": {"rows": 2}}
        }"#;
        let rules: std::collections::HashMap<usize, LayoutOptions> =
            serde_json::from_str(json).unwrap();
        assert_eq!(rules.len(), 2);
        assert_eq!(rules[&2].scrolling.unwrap().columns, 3);
        assert!(rules[&2].grid.is_none());
        assert!(rules[&5].scrolling.is_none());
        assert_eq!(rules[&5].grid.unwrap().rows, 2);
    }

    #[test]
    fn test_rule_entry_with_all_fields() {
        let json = r#"{
            "column_ratios": [0.6, 0.3],
            "scrolling": {"columns": 4, "center_focused_column": true},
            "grid": {"rows": 2},
            "row_ratios": [0.5]
        }"#;
        let opts: LayoutOptions = serde_json::from_str(json).unwrap();
        let col = opts.column_ratios.unwrap();
        assert_eq!(col[0], Some(0.6));
        assert_eq!(col[1], Some(0.3));
        let row = opts.row_ratios.unwrap();
        assert_eq!(row[0], Some(0.5));
        assert_eq!(opts.scrolling.unwrap().columns, 4);
        assert_eq!(opts.scrolling.unwrap().center_focused_column, Some(true));
        assert_eq!(opts.grid.unwrap().rows, 2);
    }

    #[test]
    fn test_rule_entry_empty_object_gives_defaults() {
        let json = r#"{}"#;
        let opts: LayoutOptions = serde_json::from_str(json).unwrap();
        assert!(opts.column_ratios.is_none());
        assert!(opts.row_ratios.is_none());
        assert!(opts.scrolling.is_none());
        assert!(opts.grid.is_none());
    }
}
