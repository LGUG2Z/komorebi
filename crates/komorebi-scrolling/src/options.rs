/// How wide a column is.
///
/// The width is part of the column, not of the strip: opening or closing other columns never
/// changes it.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum ColumnWidth {
    /// A share of the viewport width, where gaps are taken out first, so that `n` columns of
    /// `1 / n` always tile the viewport exactly, whatever the gaps are.
    Proportion(f64),
    /// An exact width in pixels, which the gaps do not affect.
    Fixed(i32),
}

/// How tall a tile in a column of several is.
///
/// A tile that has been given no height is [`Auto`](Self::Auto): it takes an equal share of what the
/// tiles with a height of their own leave. Giving one tile a height makes the others in its column
/// `Auto`, so that there is always something to take up the rest.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub enum TileHeight {
    /// An equal share of what is left over.
    #[default]
    Auto,
    /// A share of the height of the column, once the gaps between its tiles are taken out, so that
    /// tiles of `1 / n` always tile the column exactly.
    Proportion(f64),
    /// An exact height in pixels.
    Fixed(i32),
}

/// When focusing a column should also centre it in the viewport.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum CenterFocusedColumn {
    /// Never: a column that is out of view is scrolled in just far enough to be fully visible, and
    /// a column that is already in view does not move the strip at all.
    #[default]
    Never,
    /// Always centre the focused column.
    Always,
    /// Centre the focused column only if it does not fit in the viewport together with its
    /// neighbour on the side that focus came from.
    OnOverflow,
}

/// How a [`ScrollingSpace`](crate::ScrollingSpace) behaves.
#[derive(Clone, Debug, PartialEq)]
pub struct Options {
    /// The space between columns and between the columns and the edges of the viewport, in pixels.
    ///
    /// A host that applies its own padding to every window will normally leave this at zero.
    pub gaps: i32,
    /// The width of a column that has never been given one.
    ///
    /// A column that has never been resized, or given a width when it was added, is as wide as
    /// this, and changes width when this does. Opening a column never changes it, so opening
    /// columns never resizes any.
    pub default_width: ColumnWidth,
    /// The widths that cycling through presets steps between, in order.
    pub preset_widths: Vec<ColumnWidth>,
    /// When to centre the focused column.
    pub center_focused_column: CenterFocusedColumn,
    /// Centre a lone column, whatever `center_focused_column` says.
    pub always_center_single_column: bool,
    /// The heights that cycling through preset heights steps between, in order, each a share of
    /// the height of the column once the gaps between its tiles are taken out.
    pub preset_heights: Vec<f64>,
    /// The least that a tile is made to be tall, in pixels, as long as the column has room for
    /// that many tiles of that height. Zero means that there is no least.
    pub minimum_tile_height: i32,
}

impl Default for Options {
    fn default() -> Self {
        Self {
            gaps: 0,
            default_width: ColumnWidth::Proportion(0.5),
            preset_widths: vec![
                ColumnWidth::Proportion(1.0 / 3.0),
                ColumnWidth::Proportion(0.5),
                ColumnWidth::Proportion(2.0 / 3.0),
            ],
            center_focused_column: CenterFocusedColumn::Never,
            always_center_single_column: false,
            preset_heights: vec![1.0 / 3.0, 0.5, 2.0 / 3.0],
            minimum_tile_height: 0,
        }
    }
}
