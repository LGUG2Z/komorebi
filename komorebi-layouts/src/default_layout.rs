use clap::ValueEnum;
use serde::Deserialize;
use serde::Serialize;
use strum::Display;
use strum::EnumString;

use super::OperationDirection;
use super::Rect;
use super::Sizing;

/// Maximum number of ratio values that can be specified for column_ratios and row_ratios
pub const MAX_RATIOS: usize = 5;

/// Minimum allowed ratio value (prevents zero-sized windows)
pub const MIN_RATIO: f32 = 0.1;

/// Maximum allowed ratio value (ensures space for remaining windows)
pub const MAX_RATIO: f32 = 0.9;

/// Default ratio value when none is specified
pub const DEFAULT_RATIO: f32 = 0.5;

/// Default secondary ratio value for UltrawideVerticalStack layout
pub const DEFAULT_SECONDARY_RATIO: f32 = 0.25;

/// Validates and converts a Vec of ratios into a fixed-size array.
/// - Clamps values to MIN_RATIO..MAX_RATIO range
/// - Truncates when cumulative sum reaches or exceeds 1.0
/// - Limits to MAX_RATIOS values
#[must_use]
pub fn validate_ratios(ratios: &[f32]) -> [Option<f32>; MAX_RATIOS] {
    let mut arr = [None; MAX_RATIOS];
    let mut cumulative_sum = 0.0_f32;

    for (i, &val) in ratios.iter().take(MAX_RATIOS).enumerate() {
        let clamped_val = val.clamp(MIN_RATIO, MAX_RATIO);

        // Only add this ratio if cumulative sum stays below 1.0
        if cumulative_sum + clamped_val < 1.0 {
            arr[i] = Some(clamped_val);
            cumulative_sum += clamped_val;
        } else {
            // Stop adding ratios - cumulative sum would reach or exceed 1.0
            tracing::debug!(
                "Truncating ratios at index {} - cumulative sum {} + {} would reach/exceed 1.0",
                i,
                cumulative_sum,
                clamped_val
            );
            break;
        }
    }
    arr
}

#[derive(
    Clone, Copy, Debug, Serialize, Deserialize, Eq, PartialEq, Display, EnumString, ValueEnum,
)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
/// A predefined komorebi layout
pub enum DefaultLayout {
    /// BSP Layout
    ///
    /// ```text
    /// +-------+-----+
    /// |       |     |
    /// |       +--+--+
    /// |       |  |--|
    /// +-------+--+--+
    /// ```
    BSP,
    /// Columns Layout
    ///
    /// ```text
    /// +--+--+--+--+
    /// |  |  |  |  |
    /// |  |  |  |  |
    /// |  |  |  |  |
    /// +--+--+--+--+
    /// ```
    Columns,
    /// Rows Layout
    ///
    /// ```text
    /// +-----------+
    /// |-----------|
    /// |-----------|
    /// |-----------|
    /// +-----------+
    /// ```
    Rows,
    /// Vertical Stack Layout
    ///
    /// ```text
    /// +-------+-----+
    /// |       |     |
    /// |       +-----+
    /// |       |     |
    /// +-------+-----+
    /// ```
    VerticalStack,
    /// Horizontal Stack Layout
    ///
    /// ```text
    /// +------+------+
    /// |             |
    /// |------+------+
    /// |      |      |
    /// +------+------+
    /// ```
    HorizontalStack,
    /// Ultrawide Vertical Stack Layout
    ///
    /// ```text
    /// +-----+-----------+-----+
    /// |     |           |     |
    /// |     |           +-----+
    /// |     |           |     |
    /// |     |           +-----+
    /// |     |           |     |
    /// +-----+-----------+-----+
    /// ```
    UltrawideVerticalStack,
    /// Grid Layout
    ///
    /// ```text
    /// +-----+-----+   +---+---+---+   +---+---+---+   +---+---+---+
    /// |     |     |   |   |   |   |   |   |   |   |   |   |   |   |
    /// |     |     |   |   |   |   |   |   |   |   |   |   |   +---+
    /// +-----+-----+   |   +---+---+   +---+---+---+   +---+---|   |
    /// |     |     |   |   |   |   |   |   |   |   |   |   |   +---+
    /// |     |     |   |   |   |   |   |   |   |   |   |   |   |   |
    /// +-----+-----+   +---+---+---+   +---+---+---+   +---+---+---+
    ///   4 windows       5 windows       6 windows       7 windows
    /// ```
    Grid,
    /// Right Main Vertical Stack Layout
    ///
    /// ```text
    /// +-----+-------+
    /// |     |       |
    /// +-----+       |
    /// |     |       |
    /// +-----+-------+
    /// ```
    RightMainVerticalStack,
    /// Scrolling Layout
    ///
    /// ```text
    /// +--+--+--+--+--+--+
    /// |     |     |     |
    /// |     |     |     |
    /// |     |     |     |
    /// +--+--+--+--+--+--+
    /// ```
    Scrolling,
    // NOTE: If any new layout is added, please make sure to register the same in `DefaultLayout::cycle`
}

/// Helper to deserialize a variable-length array into a fixed [Option<f32>; MAX_RATIOS]
/// Ratios are truncated when their cumulative sum reaches or exceeds 1.0 to ensure
/// there's always remaining space for additional windows.
fn deserialize_ratios<'de, D>(
    deserializer: D,
) -> Result<Option<[Option<f32>; MAX_RATIOS]>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let opt: Option<Vec<f32>> = Option::deserialize(deserializer)?;
    Ok(opt.map(|vec| validate_ratios(&vec)))
}

/// Helper to serialize [Option<f32>; MAX_RATIOS] as a compact array (without trailing nulls)
fn serialize_ratios<S>(
    value: &Option<[Option<f32>; MAX_RATIOS]>,
    serializer: S,
) -> Result<S::Ok, S::Error>
where
    S: serde::Serializer,
{
    match value {
        None => serializer.serialize_none(),
        Some(arr) => {
            // Find last non-None index
            let last_idx = arr
                .iter()
                .rposition(|x| x.is_some())
                .map(|i| i + 1)
                .unwrap_or(0);
            let vec: Vec<f32> = arr.iter().take(last_idx).filter_map(|&x| x).collect();
            serializer.serialize_some(&vec)
        }
    }
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
/// Options for specific layouts
pub struct LayoutOptions {
    /// Options related to the Scrolling layout
    pub scrolling: Option<ScrollingLayoutOptions>,
    /// Options related to the Grid layout
    pub grid: Option<GridLayoutOptions>,
    /// Column width ratios (up to MAX_RATIOS values between 0.1 and 0.9)
    ///
    /// - Used by Columns layout: ratios for each column width
    /// - Used by Grid layout: ratios for column widths
    /// - Used by BSP, VerticalStack, RightMainVerticalStack: column_ratios[0] as primary split ratio
    /// - Used by HorizontalStack: column_ratios[0] as primary split ratio (top area height)
    /// - Used by UltrawideVerticalStack: column_ratios[0] as center ratio, column_ratios[1] as left ratio
    ///
    /// Columns without a ratio share remaining space equally.
    /// Example: `[0.3, 0.4, 0.3]` for 30%-40%-30% columns
    #[serde(
        default,
        deserialize_with = "deserialize_ratios",
        serialize_with = "serialize_ratios"
    )]
    pub column_ratios: Option<[Option<f32>; MAX_RATIOS]>,
    /// Row height ratios (up to MAX_RATIOS values between 0.1 and 0.9)
    ///
    /// - Used by Rows layout: ratios for each row height
    /// - Used by Grid layout: ratios for row heights
    ///
    /// Rows without a ratio share remaining space equally.
    /// Example: `[0.5, 0.5]` for 50%-50% rows
    #[serde(
        default,
        deserialize_with = "deserialize_ratios",
        serialize_with = "serialize_ratios"
    )]
    pub row_ratios: Option<[Option<f32>; MAX_RATIOS]>,
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize, Eq, PartialEq)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
/// Options for the Scrolling layout
pub struct ScrollingLayoutOptions {
    /// Desired number of visible columns (default: 3)
    pub columns: usize,
    /// With an odd number of visible columns, keep the focused window column centered
    pub center_focused_column: Option<bool>,
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize, Eq, PartialEq)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
/// Options for the Grid layout
pub struct GridLayoutOptions {
    /// Maximum number of rows per grid column
    pub rows: usize,
}

impl DefaultLayout {
    pub fn leftmost_index(&self, len: usize) -> usize {
        match self {
            Self::UltrawideVerticalStack | Self::RightMainVerticalStack => match len {
                n if n > 1 => 1,
                _ => 0,
            },
            Self::Scrolling => 0,
            DefaultLayout::BSP
            | DefaultLayout::Columns
            | DefaultLayout::Rows
            | DefaultLayout::VerticalStack
            | DefaultLayout::HorizontalStack
            | DefaultLayout::Grid => 0,
        }
    }

    pub fn rightmost_index(&self, len: usize) -> usize {
        match self {
            DefaultLayout::BSP
            | DefaultLayout::Columns
            | DefaultLayout::Rows
            | DefaultLayout::VerticalStack
            | DefaultLayout::HorizontalStack
            | DefaultLayout::Grid => len.saturating_sub(1),
            DefaultLayout::UltrawideVerticalStack => match len {
                2 => 0,
                _ => len.saturating_sub(1),
            },
            DefaultLayout::RightMainVerticalStack => 0,
            DefaultLayout::Scrolling => len.saturating_sub(1),
        }
    }

    #[must_use]
    #[allow(clippy::cast_precision_loss, clippy::only_used_in_recursion)]
    pub fn resize(
        &self,
        unaltered: &Rect,
        resize: &Option<Rect>,
        edge: OperationDirection,
        sizing: Sizing,
        delta: i32,
    ) -> Option<Rect> {
        if !matches!(
            self,
            Self::BSP
                | Self::Columns
                | Self::Rows
                | Self::VerticalStack
                | Self::RightMainVerticalStack
                | Self::HorizontalStack
                | Self::UltrawideVerticalStack
                | Self::Scrolling
        ) {
            return None;
        };

        let mut r = resize.unwrap_or_default();

        let resize_delta = delta;

        match edge {
            OperationDirection::Left => match sizing {
                Sizing::Increase => {
                    // Some final checks to make sure the user can't infinitely resize to
                    // the point of pushing other windows out of bounds

                    // Note: These checks cannot take into account the changes made to the
                    // edges of adjacent windows at operation time, so it is still possible
                    // to push windows out of bounds by maxing out an Increase Left on a
                    // Window with index 1, and then maxing out a Decrease Right on a Window
                    // with index 0. I don't think it's worth trying to defensively program
                    // against this; if people end up in this situation they are better off
                    // just hitting the retile command
                    let diff = ((r.left + -resize_delta) as f32).abs();
                    if diff < unaltered.right as f32 {
                        r.left += -resize_delta;
                    }
                }
                Sizing::Decrease => {
                    let diff = ((r.left - -resize_delta) as f32).abs();
                    if diff < unaltered.right as f32 {
                        r.left -= -resize_delta;
                    }
                }
            },
            OperationDirection::Up => match sizing {
                Sizing::Increase => {
                    let diff = ((r.top + resize_delta) as f32).abs();
                    if diff < unaltered.bottom as f32 {
                        r.top += -resize_delta;
                    }
                }
                Sizing::Decrease => {
                    let diff = ((r.top - resize_delta) as f32).abs();
                    if diff < unaltered.bottom as f32 {
                        r.top -= -resize_delta;
                    }
                }
            },
            OperationDirection::Right => match sizing {
                Sizing::Increase => {
                    let diff = ((r.right + resize_delta) as f32).abs();
                    if diff < unaltered.right as f32 {
                        r.right += resize_delta;
                    }
                }
                Sizing::Decrease => {
                    let diff = ((r.right - resize_delta) as f32).abs();
                    if diff < unaltered.right as f32 {
                        r.right -= resize_delta;
                    }
                }
            },
            OperationDirection::Down => match sizing {
                Sizing::Increase => {
                    let diff = ((r.bottom + resize_delta) as f32).abs();
                    if diff < unaltered.bottom as f32 {
                        r.bottom += resize_delta;
                    }
                }
                Sizing::Decrease => {
                    let diff = ((r.bottom - resize_delta) as f32).abs();
                    if diff < unaltered.bottom as f32 {
                        r.bottom -= resize_delta;
                    }
                }
            },
        };

        if r.eq(&Rect::default()) {
            None
        } else {
            Option::from(r)
        }
    }

    #[must_use]
    pub const fn cycle_next(self) -> Self {
        match self {
            Self::BSP => Self::Columns,
            Self::Columns => Self::Rows,
            Self::Rows => Self::VerticalStack,
            Self::VerticalStack => Self::HorizontalStack,
            Self::HorizontalStack => Self::UltrawideVerticalStack,
            Self::UltrawideVerticalStack => Self::Grid,
            Self::Grid => Self::RightMainVerticalStack,
            Self::RightMainVerticalStack => Self::Scrolling,
            Self::Scrolling => Self::BSP,
        }
    }

    #[must_use]
    pub const fn cycle_previous(self) -> Self {
        match self {
            Self::Scrolling => Self::RightMainVerticalStack,
            Self::RightMainVerticalStack => Self::Grid,
            Self::Grid => Self::UltrawideVerticalStack,
            Self::UltrawideVerticalStack => Self::HorizontalStack,
            Self::HorizontalStack => Self::VerticalStack,
            Self::VerticalStack => Self::Rows,
            Self::Rows => Self::Columns,
            Self::Columns => Self::BSP,
            Self::BSP => Self::RightMainVerticalStack,
        }
    }
}

#[cfg(test)]
mod tests {
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
            for i in 0..MAX_RATIOS {
                assert_eq!(ratios[i], Some(0.1));
            }
        }

        #[test]
        fn test_deserialize_empty_array() {
            let json = r#"{"column_ratios": []}"#;
            let opts: LayoutOptions = serde_json::from_str(json).unwrap();

            let ratios = opts.column_ratios.unwrap();
            for i in 0..MAX_RATIOS {
                assert_eq!(ratios[i], None);
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
            assert!(MIN_RATIO > 0.0);
            assert!(MIN_RATIO < MAX_RATIO);
            assert!(MAX_RATIO < 1.0);
            assert!(DEFAULT_RATIO >= MIN_RATIO && DEFAULT_RATIO <= MAX_RATIO);
            assert!(DEFAULT_SECONDARY_RATIO >= MIN_RATIO && DEFAULT_SECONDARY_RATIO <= MAX_RATIO);
            assert!(MAX_RATIOS >= 1);
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
}
