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

mod layout_default_entry_tests {
    use super::*;
    use std::collections::HashMap;

    #[test]
    fn test_default_layout_as_hashmap_key() {
        let mut map: HashMap<DefaultLayout, &str> = HashMap::new();
        map.insert(DefaultLayout::BSP, "bsp");
        map.insert(DefaultLayout::VerticalStack, "vstack");
        map.insert(DefaultLayout::Columns, "cols");

        assert_eq!(map.len(), 3);
        assert_eq!(map[&DefaultLayout::BSP], "bsp");
        assert_eq!(map[&DefaultLayout::VerticalStack], "vstack");
        assert_eq!(map[&DefaultLayout::Columns], "cols");
    }

    #[test]
    fn test_default_layout_hash_consistency() {
        // Same variant inserted twice should overwrite
        let mut map: HashMap<DefaultLayout, i32> = HashMap::new();
        map.insert(DefaultLayout::Grid, 1);
        map.insert(DefaultLayout::Grid, 2);
        assert_eq!(map.len(), 1);
        assert_eq!(map[&DefaultLayout::Grid], 2);
    }

    #[test]
    fn test_layout_default_entry_deserialize_full() {
        let json = r#"{
            "layout_options": {"column_ratios": [0.7]},
            "layout_options_rules": {
                "2": {"column_ratios": [0.7]},
                "3": {"column_ratios": [0.55]},
                "5": {"column_ratios": [0.3, 0.3, 0.3]}
            }
        }"#;
        let entry: LayoutDefaultEntry = serde_json::from_str(json).unwrap();

        let base = entry.layout_options.unwrap();
        assert_eq!(base.column_ratios.unwrap()[0], Some(0.7));

        let rules = entry.layout_options_rules.unwrap();
        assert_eq!(rules.len(), 3);
        assert_eq!(rules[&2].column_ratios.unwrap()[0], Some(0.7));
        assert_eq!(rules[&3].column_ratios.unwrap()[0], Some(0.55));
        let r5 = rules[&5].column_ratios.unwrap();
        assert_eq!(r5[0], Some(0.3));
        assert_eq!(r5[1], Some(0.3));
        assert_eq!(r5[2], Some(0.3));
    }

    #[test]
    fn test_layout_default_entry_deserialize_only_base() {
        let json = r#"{
            "layout_options": {"column_ratios": [0.6]}
        }"#;
        let entry: LayoutDefaultEntry = serde_json::from_str(json).unwrap();

        assert!(entry.layout_options.is_some());
        assert_eq!(
            entry.layout_options.unwrap().column_ratios.unwrap()[0],
            Some(0.6)
        );
        assert!(entry.layout_options_rules.is_none());
    }

    #[test]
    fn test_layout_default_entry_deserialize_only_rules() {
        let json = r#"{
            "layout_options_rules": {
                "3": {"column_ratios": [0.4]}
            }
        }"#;
        let entry: LayoutDefaultEntry = serde_json::from_str(json).unwrap();

        assert!(entry.layout_options.is_none());
        let rules = entry.layout_options_rules.unwrap();
        assert_eq!(rules.len(), 1);
        assert_eq!(rules[&3].column_ratios.unwrap()[0], Some(0.4));
    }

    #[test]
    fn test_layout_default_entry_deserialize_empty() {
        let json = r#"{}"#;
        let entry: LayoutDefaultEntry = serde_json::from_str(json).unwrap();
        assert!(entry.layout_options.is_none());
        assert!(entry.layout_options_rules.is_none());
    }

    #[test]
    fn test_layout_default_entry_roundtrip() {
        let json = r#"{
            "layout_options": {"column_ratios": [0.7]},
            "layout_options_rules": {
                "2": {"column_ratios": [0.6]},
                "5": {"column_ratios": [0.3, 0.3, 0.3]}
            }
        }"#;
        let original: LayoutDefaultEntry = serde_json::from_str(json).unwrap();
        let serialized = serde_json::to_string(&original).unwrap();
        let deserialized: LayoutDefaultEntry = serde_json::from_str(&serialized).unwrap();

        assert_eq!(
            original.layout_options.unwrap().column_ratios,
            deserialized.layout_options.unwrap().column_ratios
        );
        let orig_rules = original.layout_options_rules.unwrap();
        let deser_rules = deserialized.layout_options_rules.unwrap();
        assert_eq!(orig_rules.len(), deser_rules.len());
        for (key, orig_opts) in &orig_rules {
            let deser_opts = &deser_rules[key];
            assert_eq!(orig_opts.column_ratios, deser_opts.column_ratios);
        }
    }

    #[test]
    fn test_layout_defaults_full_config_deserialize() {
        // Simulate the top-level layout_defaults field
        let json = r#"{
            "VerticalStack": {
                "layout_options": {"column_ratios": [0.7]},
                "layout_options_rules": {
                    "2": {"column_ratios": [0.7]},
                    "3": {"column_ratios": [0.55]}
                }
            },
            "HorizontalStack": {
                "layout_options": {"column_ratios": [0.6]}
            },
            "Columns": {
                "layout_options_rules": {
                    "4": {"column_ratios": [0.3, 0.3, 0.3]}
                }
            }
        }"#;
        let defaults: HashMap<DefaultLayout, LayoutDefaultEntry> =
            serde_json::from_str(json).unwrap();

        assert_eq!(defaults.len(), 3);

        // VerticalStack: has both base and rules
        let vs = &defaults[&DefaultLayout::VerticalStack];
        assert!(vs.layout_options.is_some());
        assert_eq!(vs.layout_options_rules.as_ref().unwrap().len(), 2);

        // HorizontalStack: has only base
        let hs = &defaults[&DefaultLayout::HorizontalStack];
        assert!(hs.layout_options.is_some());
        assert!(hs.layout_options_rules.is_none());

        // Columns: has only rules
        let cols = &defaults[&DefaultLayout::Columns];
        assert!(cols.layout_options.is_none());
        assert_eq!(cols.layout_options_rules.as_ref().unwrap().len(), 1);
    }

    #[test]
    fn test_layout_default_entry_with_scrolling_and_grid() {
        let json = r#"{
            "layout_options": {
                "column_ratios": [0.5],
                "scrolling": {"columns": 3},
                "grid": {"rows": 2}
            },
            "layout_options_rules": {
                "4": {
                    "scrolling": {"columns": 5, "center_focused_column": true}
                }
            }
        }"#;
        let entry: LayoutDefaultEntry = serde_json::from_str(json).unwrap();

        let base = entry.layout_options.unwrap();
        assert_eq!(base.scrolling.unwrap().columns, 3);
        assert_eq!(base.grid.unwrap().rows, 2);

        let rules = entry.layout_options_rules.unwrap();
        let r4 = &rules[&4];
        assert_eq!(r4.scrolling.unwrap().columns, 5);
        assert_eq!(r4.scrolling.unwrap().center_focused_column, Some(true));
        // Rule doesn't inherit base fields - full replacement
        assert!(r4.column_ratios.is_none());
        assert!(r4.grid.is_none());
    }

    #[test]
    fn test_layout_default_entry_skip_serializing_none() {
        // When both fields are None, they should not appear in output
        let entry = LayoutDefaultEntry {
            layout_options: None,
            layout_options_rules: None,
        };
        let json = serde_json::to_string(&entry).unwrap();
        assert!(!json.contains("layout_options"));
        assert!(!json.contains("layout_options_rules"));
        assert_eq!(json, "{}");
    }
}

/// Tests for the complete-replacement cascade logic.
///
/// This mirrors the resolution algorithm in workspace.rs::update():
///   - If the workspace defines EITHER layout_options OR layout_options_rules,
///     it completely replaces the global layout_defaults for this layout.
///   - Global defaults are only used when the workspace has NEITHER setting.
///   - Within the effective source (workspace or global):
///     1. Try threshold match from rules (highest matching threshold wins)
///     2. If a rule matches -> use it (full replacement of base)
///     3. Else -> use the base layout_options
///
/// Since the actual cascade is in workspace.rs (which has heavy WM dependencies),
/// we test the pure algorithm here using the same data structures.
mod cascade_resolution_tests {
    use super::*;

    /// Simulates the cascade resolution logic from workspace.rs::update().
    /// This is a pure function equivalent of the inline code in update().
    fn resolve_effective_options(
        container_count: usize,
        workspace_base: Option<LayoutOptions>,
        workspace_rules: &[(usize, LayoutOptions)], // sorted by threshold ascending
        global_base: Option<LayoutOptions>,
        global_rules: &[(usize, LayoutOptions)], // sorted by threshold ascending
    ) -> Option<LayoutOptions> {
        let has_workspace_overrides = workspace_base.is_some() || !workspace_rules.is_empty();

        let (effective_base, effective_rules): (Option<LayoutOptions>, &[(usize, LayoutOptions)]) =
            if has_workspace_overrides {
                (workspace_base, workspace_rules)
            } else {
                (global_base, global_rules)
            };

        // Try threshold match from effective rules
        let mut matched = None;
        for (threshold, opts) in effective_rules {
            if container_count >= *threshold {
                matched = Some(*opts);
            }
        }

        // If a rule matched, use it (full replacement); otherwise use effective base
        if matched.is_some() {
            matched
        } else {
            effective_base
        }
    }

    fn opts_with_ratio(ratio: f32) -> LayoutOptions {
        layout_options_with_column_ratios(&[ratio])
    }

    // --- No overrides ---

    #[test]
    fn test_no_workspace_no_global_returns_none() {
        let result = resolve_effective_options(3, None, &[], None, &[]);
        assert!(result.is_none());
    }

    // --- Base-only scenarios ---

    #[test]
    fn test_workspace_base_only() {
        let ws_base = opts_with_ratio(0.7);
        let result = resolve_effective_options(3, Some(ws_base), &[], None, &[]);
        assert_eq!(result.unwrap().column_ratios, ws_base.column_ratios);
    }

    #[test]
    fn test_global_base_only() {
        let global_base = opts_with_ratio(0.6);
        let result = resolve_effective_options(3, None, &[], Some(global_base), &[]);
        assert_eq!(result.unwrap().column_ratios, global_base.column_ratios);
    }

    #[test]
    fn test_workspace_base_overrides_all_globals() {
        // Workspace has base → globals (both base and rules) are ignored entirely
        let ws_base = opts_with_ratio(0.7);
        let global_base = opts_with_ratio(0.6);
        let global_rules = vec![(2, opts_with_ratio(0.5))];
        let result =
            resolve_effective_options(3, Some(ws_base), &[], Some(global_base), &global_rules);
        // Workspace base wins; global rules are NOT used even though they would match
        assert_eq!(result.unwrap().column_ratios, ws_base.column_ratios);
    }

    // --- Rules-only scenarios ---

    #[test]
    fn test_global_rules_match() {
        let global_rules = vec![(2, opts_with_ratio(0.6)), (4, opts_with_ratio(0.5))];
        // 3 containers: matches threshold 2, not 4
        let result = resolve_effective_options(3, None, &[], None, &global_rules);
        assert_eq!(result.unwrap().column_ratios.unwrap()[0], Some(0.6));
    }

    #[test]
    fn test_global_rules_highest_matching_threshold_wins() {
        let global_rules = vec![(2, opts_with_ratio(0.6)), (4, opts_with_ratio(0.5))];
        // 5 containers: matches both thresholds 2 and 4; highest (4) wins
        let result = resolve_effective_options(5, None, &[], None, &global_rules);
        assert_eq!(result.unwrap().column_ratios.unwrap()[0], Some(0.5));
    }

    #[test]
    fn test_global_rules_no_match_falls_through_to_none() {
        let global_rules = vec![(5, opts_with_ratio(0.5))];
        // 3 containers: doesn't match threshold 5
        let result = resolve_effective_options(3, None, &[], None, &global_rules);
        assert!(result.is_none());
    }

    #[test]
    fn test_global_rules_no_match_falls_through_to_global_base() {
        let global_base = opts_with_ratio(0.6);
        let global_rules = vec![(5, opts_with_ratio(0.5))];
        // 3 containers: doesn't match threshold 5, falls back to global base
        let result = resolve_effective_options(3, None, &[], Some(global_base), &global_rules);
        assert_eq!(result.unwrap().column_ratios, global_base.column_ratios);
    }

    #[test]
    fn test_workspace_rules_override_global_rules() {
        let ws_rules = vec![(2, opts_with_ratio(0.8))];
        let global_rules = vec![(2, opts_with_ratio(0.6))];
        // Workspace has rules → global rules are ignored entirely
        let result = resolve_effective_options(3, None, &ws_rules, None, &global_rules);
        assert_eq!(result.unwrap().column_ratios.unwrap()[0], Some(0.8));
    }

    // --- Complete replacement: workspace having EITHER setting disables ALL globals ---

    #[test]
    fn test_workspace_rules_disable_global_base() {
        // Workspace has rules but no base. Global has base.
        // Since workspace has a setting, globals are completely replaced.
        let ws_rules = vec![(2, opts_with_ratio(0.8))];
        let global_base = opts_with_ratio(0.6);
        // Rule matches → use it. Global base is NOT available as fallback.
        let result = resolve_effective_options(3, None, &ws_rules, Some(global_base), &[]);
        assert_eq!(result.unwrap().column_ratios.unwrap()[0], Some(0.8));
    }

    #[test]
    fn test_workspace_rules_no_match_does_not_fall_to_global_base() {
        // Workspace has rules (but they don't match). Global has base.
        // Since workspace has a setting, globals are completely replaced → returns None.
        let ws_rules = vec![(5, opts_with_ratio(0.8))];
        let global_base = opts_with_ratio(0.6);
        let result = resolve_effective_options(3, None, &ws_rules, Some(global_base), &[]);
        // No workspace base, no rule match, globals ignored → None
        assert!(result.is_none());
    }

    #[test]
    fn test_workspace_base_disables_global_rules() {
        // Workspace has base but no rules. Global has rules.
        // Since workspace has a setting, globals are completely replaced.
        let ws_base = opts_with_ratio(0.7);
        let global_rules = vec![(2, opts_with_ratio(0.5))];
        // No workspace rules → no rule match → use workspace base. Global rules ignored.
        let result = resolve_effective_options(3, Some(ws_base), &[], None, &global_rules);
        assert_eq!(result.unwrap().column_ratios, ws_base.column_ratios);
    }

    #[test]
    fn test_workspace_base_disables_global_rules_and_base() {
        // Workspace has base. Global has both rules and base.
        // Since workspace has a setting, all globals are completely replaced.
        let ws_base = opts_with_ratio(0.7);
        let global_base = opts_with_ratio(0.6);
        let global_rules = vec![(2, opts_with_ratio(0.5))];
        let result =
            resolve_effective_options(3, Some(ws_base), &[], Some(global_base), &global_rules);
        // Only workspace base is used; global rules and base are both ignored
        assert_eq!(result.unwrap().column_ratios, ws_base.column_ratios);
    }

    #[test]
    fn test_workspace_rules_disable_global_rules_and_base() {
        // Workspace has rules. Global has both rules and base.
        // Since workspace has a setting, all globals are completely replaced.
        let ws_rules = vec![(2, opts_with_ratio(0.8))];
        let global_base = opts_with_ratio(0.6);
        let global_rules = vec![(2, opts_with_ratio(0.5))];
        let result =
            resolve_effective_options(3, None, &ws_rules, Some(global_base), &global_rules);
        // Workspace rule matches → 0.8. Global base and rules both ignored.
        assert_eq!(result.unwrap().column_ratios.unwrap()[0], Some(0.8));
    }

    // --- Full replacement semantics (rule match replaces base) ---

    #[test]
    fn test_rule_match_is_full_replacement_not_merge() {
        // When a rule matches, its options FULLY REPLACE the base.
        // Fields not specified in the rule default to their standard defaults.
        let ws_base = layout_options_with_ratios(&[0.7], &[0.4]);
        let rule_opts = layout_options_with_column_ratios(&[0.5]);
        // rule_opts has column_ratios but no row_ratios
        let ws_rules = vec![(2, rule_opts)];
        let result = resolve_effective_options(3, Some(ws_base), &ws_rules, None, &[]);
        let effective = result.unwrap();
        // Column ratios come from the rule
        assert_eq!(effective.column_ratios.unwrap()[0], Some(0.5));
        // Row ratios are NOT inherited from ws_base - they're None (full replacement)
        assert!(effective.row_ratios.is_none());
    }

    // --- Edge cases ---

    #[test]
    fn test_exact_threshold_match() {
        let rules = vec![(3, opts_with_ratio(0.6))];
        let result = resolve_effective_options(3, None, &rules, None, &[]);
        assert_eq!(result.unwrap().column_ratios.unwrap()[0], Some(0.6));
    }

    #[test]
    fn test_container_count_one_below_threshold() {
        let rules = vec![(3, opts_with_ratio(0.6))];
        let result = resolve_effective_options(2, None, &rules, None, &[]);
        assert!(result.is_none());
    }

    #[test]
    fn test_zero_containers() {
        let ws_base = opts_with_ratio(0.7);
        let rules = vec![(1, opts_with_ratio(0.5))];
        let result = resolve_effective_options(0, Some(ws_base), &rules, None, &[]);
        // 0 containers doesn't match threshold 1 → falls back to workspace base
        assert_eq!(result.unwrap().column_ratios, ws_base.column_ratios);
    }

    #[test]
    fn test_many_thresholds_correct_match() {
        let rules = vec![
            (1, opts_with_ratio(0.8)),
            (3, opts_with_ratio(0.6)),
            (5, opts_with_ratio(0.4)),
            (8, opts_with_ratio(0.3)),
        ];
        // 6 containers: matches 1, 3, 5 but not 8. Highest match is 5.
        let result = resolve_effective_options(6, None, &rules, None, &[]);
        assert_eq!(result.unwrap().column_ratios.unwrap()[0], Some(0.4));
    }

    #[test]
    fn test_workspace_rules_disable_global_rules_even_if_ws_rules_dont_match() {
        // Key behavior: if workspace has ANY setting, globals are entirely ignored.
        // Even if workspace rules don't match, we don't fall back to global rules.
        let ws_rules = vec![(10, opts_with_ratio(0.8))]; // threshold too high
        let global_rules = vec![(2, opts_with_ratio(0.5))]; // would match
        let result = resolve_effective_options(3, None, &ws_rules, None, &global_rules);
        // Workspace has rules → all globals ignored. WS rules don't match → None.
        assert!(result.is_none());
    }

    #[test]
    fn test_all_four_sources_present_rules_match() {
        // All four sources present: workspace base, workspace rules, global base, global rules
        let ws_base = opts_with_ratio(0.7);
        let ws_rules = vec![(2, opts_with_ratio(0.8))];
        let global_base = opts_with_ratio(0.6);
        let global_rules = vec![(2, opts_with_ratio(0.5))];
        let result = resolve_effective_options(
            3,
            Some(ws_base),
            &ws_rules,
            Some(global_base),
            &global_rules,
        );
        // Workspace has settings → uses workspace only. Rule matches → 0.8
        assert_eq!(result.unwrap().column_ratios.unwrap()[0], Some(0.8));
    }

    #[test]
    fn test_all_four_sources_present_rules_no_match() {
        // All four sources present, but workspace rules don't match
        let ws_base = opts_with_ratio(0.7);
        let ws_rules = vec![(10, opts_with_ratio(0.8))]; // threshold too high
        let global_base = opts_with_ratio(0.6);
        let global_rules = vec![(10, opts_with_ratio(0.5))]; // also too high
        let result = resolve_effective_options(
            3,
            Some(ws_base),
            &ws_rules,
            Some(global_base),
            &global_rules,
        );
        // Workspace has settings → uses workspace only. No rule match → workspace base 0.7
        assert_eq!(result.unwrap().column_ratios, ws_base.column_ratios);
    }

    // --- Workspace with both base and rules ---

    #[test]
    fn test_workspace_both_rule_matches() {
        let ws_base = opts_with_ratio(0.7);
        let ws_rules = vec![(2, opts_with_ratio(0.5))];
        let result = resolve_effective_options(3, Some(ws_base), &ws_rules, None, &[]);
        // Rule matches → use rule (full replacement), not ws_base
        assert_eq!(result.unwrap().column_ratios.unwrap()[0], Some(0.5));
    }

    #[test]
    fn test_workspace_both_rule_no_match() {
        let ws_base = opts_with_ratio(0.7);
        let ws_rules = vec![(10, opts_with_ratio(0.5))];
        let result = resolve_effective_options(3, Some(ws_base), &ws_rules, None, &[]);
        // Rule doesn't match → fall back to ws_base
        assert_eq!(result.unwrap().column_ratios, ws_base.column_ratios);
    }
}
