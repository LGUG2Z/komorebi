use crate::CenterFocusedColumn;
use crate::ColumnWidth;
use crate::Options;
use crate::Rect;
use crate::TileHeight;
use crate::Viewport;
use crate::view;

/// How far a float may be off when deciding whether a column is inside the viewport. Widths come
/// from proportions, so the edges of columns that fit exactly are rarely exact.
const EPSILON: f64 = 1e-6;

/// How far a column may stick out of the viewport, or into the gap at its edge, and still count
/// as fully in view. Widths are whole pixels and edges are not, so a column that was made to fill
/// the room that is left can end up a fraction of a pixel too wide, and scrolling the strip by
/// that fraction must not make its neighbour count as out of view.
const TOLERANCE: f64 = 1.0;

#[derive(Clone, Debug, PartialEq)]
struct Column<K> {
    /// The key of the first tile, from the top, which is also the key of the column.
    id: K,
    /// The other tiles, from the top, after the first.
    rest: Vec<K>,
    /// Which tile had focus last, counting from the top: the one that gets focus again when the
    /// column does.
    last_tile: usize,
    /// The width that this column was given, or `None` if it has never been given one, which
    /// makes it as wide as [`Options::default_width`] says, whatever that is at the moment.
    width: Option<ColumnWidth>,
    /// The preset that `width` was last taken from, so that cycling carries on from there.
    preset: Option<usize>,
    /// The column takes the whole viewport width. `width` is kept, to go back to.
    full_width: bool,
    fullscreen: bool,
    /// Limits that the host found out by trying: a window that would not get any narrower than
    /// `min_width`, or any wider than `max_width`.
    min_width: Option<i32>,
    max_width: Option<i32>,
}

impl<K: Clone + Eq> Column<K> {
    fn new(id: K, width: Option<ColumnWidth>) -> Self {
        Self {
            id,
            rest: Vec::new(),
            last_tile: 0,
            width,
            preset: None,
            full_width: false,
            fullscreen: false,
            min_width: None,
            max_width: None,
        }
    }

    fn tile_count(&self) -> usize {
        1 + self.rest.len()
    }

    /// The keys of the tiles, from the top.
    fn tiles(&self) -> impl Iterator<Item = &K> {
        std::iter::once(&self.id).chain(self.rest.iter())
    }

    fn tile_index(&self, id: &K) -> Option<usize> {
        self.tiles().position(|tile| tile == id)
    }

    /// The tile that has focus when the column does.
    fn last(&self) -> &K {
        self.tiles().nth(self.last_tile).unwrap_or(&self.id)
    }

    /// Give the column these tiles, from the top, keeping focus on the tile that had it if it is
    /// still there.
    fn set_tiles(&mut self, tiles: Vec<K>) {
        let last = self.last().clone();

        let mut tiles = tiles.into_iter();
        if let Some(first) = tiles.next() {
            self.id = first;
        }
        self.rest = tiles.collect();

        self.last_tile = self.tile_index(&last).unwrap_or(0);
    }
}

/// Where every column is in the strip, which is a function of nothing but their widths.
struct Geometry {
    /// The width of each column.
    widths: Vec<f64>,
    /// The distance from the start of the strip to the left edge of each column.
    xs: Vec<f64>,
}

/// The columns that are fully in view, and what they take up.
struct Group {
    leftmost: usize,
    /// Their widths, with a gap after each.
    taken: f64,
    /// Whether the group has any column in it besides the active one.
    has_others: bool,
}

/// How much of a column is in the viewport.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Visibility {
    /// All of it is in the viewport.
    Visible,
    /// Some of it is, and some of it is past an edge.
    Clipped,
    /// None of it is: it is past the left edge.
    OffLeft,
    /// None of it is: it is past the right edge.
    OffRight,
}

/// Where a column goes.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Placement<K> {
    /// The id that the column was added with.
    pub id: K,
    /// Where the column is, in viewport coordinates. This is not clamped to the viewport: a
    /// column that is partly or wholly out of view has the rectangle that it really has, so a
    /// host that cannot clip has all the information it needs to decide what to do instead.
    pub rect: Rect,
    /// How much of the rectangle is in the viewport.
    pub visibility: Visibility,
    /// Whether this is the column that has focus.
    pub active: bool,
}

/// What a column was given: its width, the preset that it came from, whether it takes the whole
/// viewport, and the limits that were found out about its window.
///
/// A tile that goes to another strip takes this with it, so that it is not as wide there as the
/// default again, and [`ScrollingSpace::arrive`] is how the other strip is told.
#[derive(Clone, Debug, PartialEq)]
pub struct ColumnRecord {
    width: Option<ColumnWidth>,
    preset: Option<usize>,
    full_width: bool,
    min_width: Option<i32>,
    max_width: Option<i32>,
}

/// How many tiles that are on their way to this strip it waits for before it forgets the oldest.
const MAXIMUM_ARRIVALS: usize = 32;

/// Where a pan came to rest.
#[derive(Clone, Debug, PartialEq)]
pub struct Snap<K> {
    /// Where the left edge of the viewport was in the strip when the pan was let go.
    pub from: f64,
    /// Where it is now, which is what the strip is laid out for. Something that animates the
    /// release goes from `from` to this.
    pub to: f64,
    /// The tile that has focus now. It is the one that had it before the pan if that is still in
    /// view where the pan came to rest, and otherwise a tile that is.
    pub active: K,
}

/// An ordered strip of columns, a viewport onto it, and the rules for how the viewport moves.
///
/// # What is stored
///
/// Each column has a width of its own, so the strip is as wide as its columns make it, and
/// opening or closing a column does not change any other.
///
/// The viewport is stored as one number: where the left edge of the **active** column is on
/// screen. Every other position follows from that and the widths. This has a useful consequence:
/// anything that changes the columns to the left of the active one, such as opening one, closing
/// one or making one wider, leaves that number alone, and so the active column does not move.
///
/// # Changing which column is active
///
/// Moving focus keeps the viewport where it is and then scrolls by the least amount that brings
/// the newly active column fully into view. Moving focus to a column that is already in view
/// therefore does not scroll at all.
///
/// Two things are remembered so that temporary changes can be undone, and both are forgotten as
/// soon as focus moves to some other column:
///
/// * When a column is opened next to the active one and made active, where the old active column
///   was on screen is remembered. If the new column is closed before focus goes anywhere else,
///   focus returns to the old column and the viewport is put back as it was.
/// * When the active column goes fullscreen, where it was on screen is remembered, so that
///   leaving fullscreen puts the viewport back.
///
/// # Keys
///
/// Columns are identified by a key `K` of the host's choosing, usually the id of whatever the
/// host keeps its windows in. All of a column's state is looked up by that key, so it stays with
/// the column when columns are reordered.
#[derive(Clone, Debug, PartialEq)]
pub struct ScrollingSpace<K> {
    options: Options,
    viewport: Viewport,
    columns: Vec<Column<K>>,
    /// The index of the active column. Meaningless when there are no columns.
    active: usize,
    /// Where the left edge of the active column is, measured from the left edge of the viewport.
    active_x: f64,
    /// Set when a column opened next to the active one was made active: where the old active
    /// column's left edge was.
    return_to: Option<f64>,
    /// Set when the active column goes fullscreen: where its left edge was.
    restore_x: Option<f64>,
    /// The tiles that have been given a height, and what it is. A tile that is not here is
    /// [`TileHeight::Auto`], and so does not need to be remembered.
    heights: Vec<(K, TileHeight)>,
    /// What tiles that are expected to be added brought with them from another strip.
    arrivals: Vec<(K, ColumnRecord)>,
}

impl<K: Clone + Eq> ScrollingSpace<K> {
    /// An empty strip.
    #[must_use]
    pub fn new(options: Options, viewport: Viewport) -> Self {
        Self {
            options: Options {
                gaps: options.gaps.max(0),
                ..options
            },
            viewport,
            columns: Vec::new(),
            active: 0,
            active_x: 0.0,
            return_to: None,
            restore_x: None,
            heights: Vec::new(),
            arrivals: Vec::new(),
        }
    }

    /// The number of columns.
    #[must_use]
    pub fn len(&self) -> usize {
        self.columns.len()
    }

    /// The number of tiles, which is more than the number of columns when a column has more than
    /// one.
    #[must_use]
    pub fn tile_count(&self) -> usize {
        self.columns.iter().map(Column::tile_count).sum()
    }

    /// Whether there are no columns.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.columns.is_empty()
    }

    /// The keys of the tiles in reading order: from left to right by column, and from top to
    /// bottom within one.
    ///
    /// A host that keeps its tiles in a list keeps that list in this order, which is what
    /// [`reconcile`](Self::reconcile) works out the columns from.
    #[must_use]
    pub fn ids(&self) -> Vec<K> {
        self.columns
            .iter()
            .flat_map(|column| column.tiles().cloned())
            .collect()
    }

    /// Where the column that has the tile with this key is in the strip, counting from the left.
    #[must_use]
    pub fn position(&self, id: &K) -> Option<usize> {
        self.columns
            .iter()
            .position(|column| column.tile_index(id).is_some())
    }

    /// The keys of the tiles in the column that has the tile with this key, from the top.
    #[must_use]
    pub fn column_of(&self, id: &K) -> Option<Vec<K>> {
        let column = &self.columns[self.position(id)?];

        Some(column.tiles().cloned().collect())
    }

    /// The index of the column that has focus.
    #[must_use]
    pub fn active_index(&self) -> Option<usize> {
        (!self.columns.is_empty()).then_some(self.active)
    }

    /// The key of the tile that has focus, which is the one that last had it in the active column.
    #[must_use]
    pub fn active_id(&self) -> Option<&K> {
        self.columns.get(self.active).map(Column::last)
    }

    /// Where the tile that has focus is in reading order, which is its place in
    /// [`ids`](Self::ids).
    #[must_use]
    pub fn active_tile_index(&self) -> Option<usize> {
        let column = self.columns.get(self.active)?;
        let before: usize = self.columns[..self.active]
            .iter()
            .map(Column::tile_count)
            .sum();

        Some(before + column.last_tile)
    }

    /// Where the left edge of the active column is on screen, measured from the left edge of the
    /// viewport.
    #[must_use]
    pub fn active_x(&self) -> Option<f64> {
        (!self.columns.is_empty()).then_some(self.active_x)
    }

    /// How wide the column with this key is, in pixels, not rounded.
    ///
    /// This is what the column is, limits and fullscreen included, which is not always what it
    /// was asked to be. It is what to start from to make a column a little wider or narrower.
    #[must_use]
    pub fn column_width(&self, id: &K) -> Option<f64> {
        self.columns
            .iter()
            .find(|column| column.tile_index(id).is_some())
            .map(|column| self.width_of(column))
    }

    /// How far into the strip the left edge of the viewport is.
    ///
    /// This is the number that something animating the scrolling would interpolate.
    #[must_use]
    pub fn view_offset(&self) -> f64 {
        if self.columns.is_empty() {
            return 0.0;
        }

        self.geometry().xs[self.active] - self.active_x
    }

    /// The options in force.
    #[must_use]
    pub fn options(&self) -> &Options {
        &self.options
    }

    /// The viewport in force.
    #[must_use]
    pub fn viewport(&self) -> Viewport {
        self.viewport
    }

    /// Change the options. The active column stays where it is, unless it has to move to be in
    /// view or to be centred.
    pub fn set_options(&mut self, options: Options) {
        self.options = Options {
            gaps: options.gaps.max(0),
            ..options
        };

        let presets = self.options.preset_widths.len();
        for column in &mut self.columns {
            if column.preset.is_some_and(|preset| preset >= presets) {
                column.preset = None;
            }
        }

        self.reveal(None);
    }

    /// Change the size of the viewport. Columns that are proportions of it change width with it.
    /// The active column stays where it is, unless it has to move to be in view or to be centred.
    pub fn set_viewport(&mut self, viewport: Viewport) {
        self.viewport = viewport;
        self.reveal(None);
    }

    /// Add a column.
    ///
    /// * `at` is where in the strip it goes, and it defaults to directly to the right of the
    ///   active column.
    /// * `width` is how wide it is. Without one the column has never been given a width, so it is
    ///   as wide as [`Options::default_width`] says, and goes on being as that changes, until it is
    ///   resized.
    /// * With `activate`, the new column gets focus, and the viewport scrolls just far enough to
    ///   show it. Without, the active column does not move, whether the new column goes to its
    ///   left or its right. The first column of an empty strip always gets focus.
    ///
    /// No other column changes width.
    pub fn add_column(
        &mut self,
        id: K,
        at: Option<usize>,
        width: Option<ColumnWidth>,
        activate: bool,
    ) {
        debug_assert!(
            self.position(&id).is_none(),
            "the key is already in the strip"
        );

        let mut column = Column::new(id, width);

        // a tile that came from another strip is as wide as it was there, unless it is told
        if width.is_none()
            && let Some(at) = self
                .arrivals
                .iter()
                .position(|(tile, _)| *tile == column.id)
        {
            let (_, record) = self.arrivals.remove(at);

            column.width = record.width;
            column.preset = record
                .preset
                .filter(|preset| *preset < self.options.preset_widths.len());
            column.full_width = record.full_width;
            column.min_width = record.min_width;
            column.max_width = record.max_width;
        }

        if self.columns.is_empty() {
            self.columns.push(column);
            self.reset_view();
            self.reveal(None);
            return;
        }

        let previous_active = self.active;
        let previous_x = self.active_x;
        let index = at.map_or(previous_active + 1, |at| at.min(self.columns.len()));

        self.columns.insert(index, column);
        if index <= previous_active {
            // It is the same column, in the same place on screen, with one more column before it.
            self.active += 1;
            // And the column that was to be returned to is no longer the one next to this one.
            self.return_to = None;
        }

        if activate {
            self.activate(index);

            // Only a column directly to the right of the old active one can be returned from: that
            // is the one that is left of it when it is closed.
            if index == previous_active + 1 {
                self.return_to = Some(previous_x);
            }
        }
    }

    /// Remove the tile with this key, and with it its column if it was the only one in it, and
    /// tell whether there was one.
    ///
    /// A column that has other tiles goes on being a column, with the same width, and the tiles
    /// that were below the one that went move up. If the tile had focus, focus goes to the one
    /// that took its place, or to the one above if there is none. Nothing moves sideways.
    ///
    /// When it is the only tile in its column, the column goes:
    ///
    /// * A column to the left of the active one goes without the active column moving.
    /// * A column to the right of it goes without anything else changing.
    /// * If the active column goes, focus returns to the column it was opened next to if nothing
    ///   has changed focus since, with the viewport as it was. Otherwise the column to its right
    ///   becomes active, or the last column if there is none, with the viewport where it was.
    pub fn remove_column(&mut self, id: &K) -> bool {
        let Some(index) = self.position(id) else {
            return false;
        };

        // a tile that is gone takes the height it was given with it
        self.heights.retain(|(tile, _)| tile != id);

        if self.columns[index].tile_count() > 1 {
            let column = &mut self.columns[index];
            let removed = column.tile_index(id).unwrap_or(0);
            let last = column.last_tile;

            let mut tiles: Vec<K> = column.tiles().cloned().collect();
            tiles.remove(removed);

            // the tile that was focused is gone, so the one that takes its place is
            let next = match removed.cmp(&last) {
                std::cmp::Ordering::Less => last - 1,
                std::cmp::Ordering::Equal => last.min(tiles.len() - 1),
                std::cmp::Ordering::Greater => last,
            };

            column.id = tiles.remove(0);
            column.rest = tiles;
            column.last_tile = next;

            return true;
        }

        let was_active = index == self.active;
        let view_pos = self.geometry().xs[self.active] - self.active_x;

        self.columns.remove(index);

        if self.columns.is_empty() {
            self.reset_view();
            return true;
        }

        if index < self.active {
            self.active -= 1;
            self.return_to = None;
        } else if was_active {
            self.restore_x = None;

            match self.return_to.take() {
                Some(x) if index > 0 => {
                    self.active = index - 1;
                    self.active_x = x;
                }
                _ => {
                    self.active = index.min(self.columns.len() - 1);
                    self.active_x = self.geometry().xs[self.active] - view_pos;
                }
            }

            self.reveal(None);
        }

        true
    }

    /// Give focus to the tile with this key, and tell whether there was one.
    pub fn focus(&mut self, id: &K) -> bool {
        match self.position(id) {
            Some(index) => {
                let column = &mut self.columns[index];
                column.last_tile = column.tile_index(id).unwrap_or(0);

                self.activate(index);
                true
            }
            None => false,
        }
    }

    /// Give focus to the tile above the one that has it in its column, and tell whether there was
    /// one. Nothing is above the first tile, and nothing goes round.
    pub fn focus_up(&mut self) -> bool {
        match self.columns.get_mut(self.active) {
            Some(column) if column.last_tile > 0 => {
                column.last_tile -= 1;
                true
            }
            _ => false,
        }
    }

    /// Give focus to the tile below the one that has it in its column, and tell whether there was
    /// one. Nothing is below the last tile, and nothing goes round.
    pub fn focus_down(&mut self) -> bool {
        match self.columns.get_mut(self.active) {
            Some(column) if column.last_tile + 1 < column.tile_count() => {
                column.last_tile += 1;
                true
            }
            _ => false,
        }
    }

    /// Give focus to the column at this position, and tell whether there was one.
    pub fn focus_index(&mut self, index: usize) -> bool {
        if index < self.columns.len() {
            self.activate(index);
            true
        } else {
            false
        }
    }

    /// Give focus to the column on the left, and tell whether there was one.
    pub fn focus_left(&mut self) -> bool {
        match self.active.checked_sub(1) {
            Some(index) if !self.columns.is_empty() => self.focus_index(index),
            _ => false,
        }
    }

    /// Give focus to the column on the right, and tell whether there was one.
    pub fn focus_right(&mut self) -> bool {
        !self.columns.is_empty() && self.focus_index(self.active + 1)
    }

    /// Move the column with this key to a new position in the strip.
    ///
    /// If it is the active column, it goes on being the active column and the viewport follows it.
    /// Otherwise the active column does not move.
    pub fn move_column(&mut self, id: &K, to: usize) {
        let Some(from) = self.position(id) else {
            return;
        };

        let to = to.min(self.columns.len() - 1);
        if from == to {
            return;
        }

        let view_pos = self.geometry().xs[self.active] - self.active_x;

        let column = self.columns.remove(from);
        self.columns.insert(to, column);
        self.return_to = None;

        if from == self.active {
            self.active = to;
            self.active_x = self.geometry().xs[to] - view_pos;
            self.reveal(None);
        } else if from < self.active && to >= self.active {
            self.active -= 1;
        } else if from > self.active && to <= self.active {
            self.active += 1;
        }
    }

    /// Take the first tile of the column on the right and put it at the bottom of the active
    /// column, and tell whether there was one.
    ///
    /// The column on the right goes if that was its only tile. Nothing else changes: the tile
    /// that is taken is the one that comes next in reading order, so the order of the tiles is
    /// what it was.
    pub fn consume_right(&mut self) -> bool {
        if self.columns.is_empty() || self.active + 1 >= self.columns.len() {
            return false;
        }

        let next = self.active + 1;
        let taken = if self.columns[next].rest.is_empty() {
            self.columns.remove(next).id
        } else {
            let column = &mut self.columns[next];
            let new_first = column.rest.remove(0);
            column.last_tile = column.last_tile.saturating_sub(1);

            std::mem::replace(&mut column.id, new_first)
        };

        // a tile that joins another column is as tall as an equal share of it
        self.heights.retain(|(tile, _)| *tile != taken);
        self.columns[self.active].rest.push(taken);

        true
    }

    /// Take the last tile of the column on the left and put it at the top of the active column,
    /// and tell whether there was one.
    ///
    /// The column on the left goes if that was its only tile, and the active column does not move
    /// on screen when it does. The tile that has focus goes on having it. The order of the tiles
    /// is what it was.
    pub fn consume_left(&mut self) -> bool {
        if self.columns.is_empty() || self.active == 0 {
            return false;
        }

        let previous = self.active - 1;
        let taken = if self.columns[previous].rest.is_empty() {
            let column = self.columns.remove(previous);
            self.active -= 1;
            column.id
        } else {
            let column = &mut self.columns[previous];
            let taken = column.rest.pop().expect("there is more than one tile");
            column.last_tile = column.last_tile.min(column.tile_count() - 1);

            taken
        };

        // the strip is different on the left, and so there is nothing to return to there
        self.return_to = None;

        self.heights.retain(|(tile, _)| *tile != taken);

        let column = &mut self.columns[self.active];
        let old_first = std::mem::replace(&mut column.id, taken);
        column.rest.insert(0, old_first);
        column.last_tile += 1;

        true
    }

    /// Take the tile that has focus out of its column and put it in a column of its own on the
    /// right, with the default width, and tell whether it was in a column with others.
    ///
    /// The tile goes on having focus, and the viewport scrolls only if it has to. When it was
    /// not the last tile in its column it moves past the tiles that were below it, so the order of
    /// the tiles is no longer what it was, and the host has to change its list to match.
    pub fn expel(&mut self) -> bool {
        if self.columns.is_empty() || self.columns[self.active].tile_count() == 1 {
            return false;
        }

        let column = &mut self.columns[self.active];
        let index = column.last_tile;

        let mut tiles: Vec<K> = column.tiles().cloned().collect();
        let tile = tiles.remove(index);

        // a column of its own is as tall as it is
        self.heights.retain(|(other, _)| *other != tile);

        let column = &mut self.columns[self.active];
        column.id = tiles.remove(0);
        column.rest = tiles;
        // the tile that took its place is next to it, as are all the others
        column.last_tile = index.min(column.tile_count() - 1);

        let expelled = self.active + 1;
        self.columns.insert(expelled, Column::new(tile, None));
        self.activate(expelled);

        true
    }

    /// Swap the tile that has focus with the one above it, and tell whether there was one. The
    /// tile goes on having focus. The order of the tiles changes, and the host has to change its
    /// list to match.
    pub fn move_tile_up(&mut self) -> bool {
        self.swap_tile(-1)
    }

    /// Swap the tile that has focus with the one below it, and tell whether there was one. The
    /// tile goes on having focus. The order of the tiles changes, and the host has to change its
    /// list to match.
    pub fn move_tile_down(&mut self) -> bool {
        self.swap_tile(1)
    }

    fn swap_tile(&mut self, by: isize) -> bool {
        let Some(column) = self.columns.get_mut(self.active) else {
            return false;
        };

        let Some(to) = column
            .last_tile
            .checked_add_signed(by)
            .filter(|to| *to < column.tile_count())
        else {
            return false;
        };

        let mut tiles: Vec<K> = column.tiles().cloned().collect();
        tiles.swap(column.last_tile, to);

        column.id = tiles.remove(0);
        column.rest = tiles;
        column.last_tile = to;

        true
    }

    /// Give a column an exact width.
    ///
    /// If it is the active column, its left edge stays where it is and the viewport scrolls only
    /// if it has to. A column to the left of the active one can change width without the active
    /// column moving.
    pub fn set_width(&mut self, id: &K, width: ColumnWidth) {
        let Some(index) = self.position(id) else {
            return;
        };

        let column = &mut self.columns[index];
        column.width = Some(width);
        column.preset = None;
        column.full_width = false;

        self.after_resize(index);
    }

    /// Give a column the next preset width, or the previous one.
    ///
    /// A column that has not been given a preset yet gets the first one that is wider than it
    /// (or the first one, if none are), or the last one that is narrower (or the last one) when
    /// going backwards. After that, the presets are stepped through in order, wrapping.
    pub fn cycle_preset_width(&mut self, id: &K, forward: bool) {
        let Some(index) = self.position(id) else {
            return;
        };

        let count = self.options.preset_widths.len();
        if count == 0 {
            return;
        }

        let next = match self.columns[index].preset {
            Some(current) if current < count => {
                if forward {
                    (current + 1) % count
                } else {
                    (current + count - 1) % count
                }
            }
            _ => {
                let width = self.width_of(&self.columns[index]);
                let presets: Vec<f64> = self
                    .options
                    .preset_widths
                    .iter()
                    .map(|preset| self.resolve(*preset))
                    .collect();

                if forward {
                    presets
                        .iter()
                        .position(|preset| *preset > width + 0.5)
                        .unwrap_or(0)
                } else {
                    presets
                        .iter()
                        .rposition(|preset| *preset < width - 0.5)
                        .unwrap_or(count - 1)
                }
            }
        };

        let preset = self.options.preset_widths[next];
        let column = &mut self.columns[index];
        column.width = Some(preset);
        column.preset = Some(next);
        column.full_width = false;

        self.after_resize(index);
    }

    /// Forget every width that any column was given, so that all of them are as wide as
    /// [`Options::default_width`] says again.
    ///
    /// Presets and full width are forgotten with the widths, and fullscreen is not: that is a
    /// state that something else put a column in, and it is the host's to take it out of. What the
    /// host found out that a window will not accept stays too, since that is a fact about the
    /// window and not a choice that was made about the strip.
    ///
    /// The active column keeps its left edge on screen and the viewport scrolls only if it has to.
    pub fn reset_widths(&mut self) {
        for column in &mut self.columns {
            column.width = None;
            column.preset = None;
            column.full_width = false;
        }

        if !self.columns.is_empty() {
            self.after_resize(self.active);
        }
    }

    /// Make a column as wide as the viewport, or give it back the width that it had.
    pub fn toggle_full_width(&mut self, id: &K) {
        let Some(index) = self.position(id) else {
            return;
        };

        let column = &mut self.columns[index];
        column.full_width = !column.full_width;

        self.after_resize(index);
    }

    /// Make a column fill the viewport, or put it back.
    ///
    /// A fullscreen column is still a column in the strip: the others stay where they are on
    /// either side of it, and focus can move away from it and back. When the active column goes
    /// fullscreen, the viewport is remembered, and put back when it stops being fullscreen.
    pub fn set_fullscreen(&mut self, id: &K, fullscreen: bool) {
        let Some(index) = self.position(id) else {
            return;
        };

        if self.columns[index].fullscreen == fullscreen {
            return;
        }

        self.columns[index].fullscreen = fullscreen;

        if index != self.active {
            return;
        }

        if fullscreen {
            self.restore_x = Some(self.active_x);
        } else if let Some(x) = self.restore_x.take() {
            self.active_x = x;
        }

        self.reveal(None);
    }

    /// Tell the model what a column's window turned out to accept.
    ///
    /// A window may refuse to be narrower than some width, or wider than some. If the host finds
    /// out, say with a `min`, the column is never narrower than that again, and the columns
    /// after it are placed where it really ends instead of overlapping it.
    pub fn set_width_limits(&mut self, id: &K, min: Option<i32>, max: Option<i32>) {
        let Some(index) = self.position(id) else {
            return;
        };

        let column = &mut self.columns[index];
        column.min_width = min;
        column.max_width = max;

        self.after_resize(index);
    }

    /// The limits that the host has told the model about for a column: the narrowest and the widest
    /// that it can be, or `None` if there is no such column.
    ///
    /// Learning that a window needs more room than it was given means raising a limit that may
    /// already be there, which takes knowing what is there.
    pub fn width_limits(&self, id: &K) -> Option<(Option<i32>, Option<i32>)> {
        let column = &self.columns[self.position(id)?];

        Some((column.min_width, column.max_width))
    }

    /// What the column that has this tile was given, to take to another strip, if there is such a
    /// tile.
    #[must_use]
    pub fn column_record(&self, id: &K) -> Option<ColumnRecord> {
        let column = &self.columns[self.position(id)?];

        Some(ColumnRecord {
            width: column.width,
            preset: column.preset,
            full_width: column.full_width,
            min_width: column.min_width,
            max_width: column.max_width,
        })
    }

    /// Say that the tile with this key is expected to be added, from another strip, and that
    /// the column that it is in when it is added is to be what its column was there.
    ///
    /// Nothing happens if the tile is already in the strip. A tile that is added with a width
    /// of its own has that width. Only so many arrivals are waited for, and the oldest are
    /// forgotten first, so a tile that never comes does not stay for ever.
    pub fn arrive(&mut self, id: K, record: ColumnRecord) {
        if self.position(&id).is_some() {
            return;
        }

        self.arrivals.retain(|(tile, _)| *tile != id);
        self.arrivals.push((id, record));

        if self.arrivals.len() > MAXIMUM_ARRIVALS {
            self.arrivals.remove(0);
        }
    }

    /// Give a tile a height, and make the others in its column [`TileHeight::Auto`] so that they
    /// take up what it leaves. A tile that is alone in its column fills it, so there is nothing
    /// to give it, and nothing happens. [`TileHeight::Auto`] gives the tile back an equal share
    /// and leaves the others as they are.
    ///
    /// Nothing moves sideways, so the strip does not scroll.
    pub fn set_tile_height(&mut self, id: &K, height: TileHeight) {
        let Some(index) = self.position(id) else {
            return;
        };

        let tiles: Vec<K> = self.columns[index].tiles().cloned().collect();
        if tiles.len() == 1 {
            return;
        }

        match height {
            TileHeight::Auto => self.heights.retain(|(tile, _)| tile != id),
            height => {
                self.heights.retain(|(tile, _)| !tiles.contains(tile));
                self.heights.push((id.clone(), height));
            }
        }
    }

    /// Make a tile taller or shorter by `delta` pixels from the height that it is now, and give it
    /// that height as [`set_tile_height`](Self::set_tile_height) does. It is never made shorter
    /// than [`Options::minimum_tile_height`], nor so tall that the others are.
    pub fn resize_tile_height(&mut self, id: &K, delta: f64) {
        let Some(current) = self.tile_height(id) else {
            return;
        };
        let Some(index) = self.position(id) else {
            return;
        };

        let column = &self.columns[index];
        let count = column.tile_count();
        if count == 1 || !delta.is_finite() {
            return;
        }

        let (_, column_height, between) = self.column_extent(column);
        let room = column_height - (count - 1) as f64 * between;
        let minimum = f64::from(self.options.minimum_tile_height.max(1));
        let most = (room - minimum * (count - 1) as f64).max(minimum);

        let height = (current + delta).clamp(minimum, most);
        self.set_tile_height(id, TileHeight::Fixed(height.round() as i32));
    }

    /// Give a tile the next preset height, or the previous one, as a share of the column.
    ///
    /// The first preset that is taller than the tile is now is the next one, or the first if there
    /// is none, and the last that is shorter is the previous, or the last if there is none. After
    /// that the presets are stepped through in order, and wrap.
    pub fn cycle_preset_height(&mut self, id: &K, forward: bool) {
        let Some(current) = self.tile_height(id) else {
            return;
        };
        let Some(index) = self.position(id) else {
            return;
        };

        let column = &self.columns[index];
        let count = column.tile_count();
        let presets = &self.options.preset_heights;
        if count == 1 || presets.is_empty() {
            return;
        }

        let (_, column_height, between) = self.column_extent(column);
        let room = column_height - (count - 1) as f64 * between;

        let next = if forward {
            presets
                .iter()
                .position(|preset| preset * room > current + 0.5)
                .unwrap_or(0)
        } else {
            presets
                .iter()
                .rposition(|preset| preset * room < current - 0.5)
                .unwrap_or(presets.len() - 1)
        };

        let preset = presets[next];
        self.set_tile_height(id, TileHeight::Proportion(preset));
    }

    /// Give every tile in the column that has this tile an equal share of its height again.
    pub fn reset_tile_heights(&mut self, id: &K) {
        let Some(index) = self.position(id) else {
            return;
        };

        let tiles: Vec<K> = self.columns[index].tiles().cloned().collect();
        self.heights.retain(|(tile, _)| !tiles.contains(tile));
    }

    /// Forget every height that any tile was given.
    pub fn reset_heights(&mut self) {
        self.heights.clear();
    }

    /// Start a pan, which is the user moving the strip across the viewport by hand, and forget
    /// the viewport that was to be put back: they chose where it is now.
    pub fn begin_pan(&mut self) {
        self.return_to = None;
        self.restore_x = None;
    }

    /// Move the strip by `dx` on screen, in pixels, to the right if it is positive. Between
    /// [`begin_pan`](Self::begin_pan) and [`pan_end`](Self::pan_end) nothing else changes: no
    /// width, and not which column is active, so that the active column is only ever moved by the
    /// user. The layout is what it should be at every step, and a column may be partly out of
    /// view, or the whole strip may be beyond its ends by up to the width of the viewport.
    pub fn pan_by(&mut self, dx: f64) {
        if self.columns.is_empty() || !dx.is_finite() {
            return;
        }

        let geometry = self.geometry();
        let (min, max) = self.view_range(&geometry);
        let overscroll = f64::from(self.viewport.width);

        let position = geometry.xs[self.active] - self.active_x - dx;
        let position = position.clamp(min - overscroll, max + overscroll);

        self.active_x = geometry.xs[self.active] - position;
    }

    /// Let go of the strip, moving at `velocity` pixels a second on screen, in the same direction
    /// as [`pan_by`](Self::pan_by), and let it come to rest. Gives back where, or `None` if there
    /// is nothing in the strip.
    ///
    /// It comes to rest with a column aligned to an edge of the viewport (or all of them centred,
    /// where that is what the options ask for), at the alignment nearest to where it would get to
    /// in about a third of a second at the speed that it was going. It never rests beyond the first
    /// or the last column, whatever the speed.
    ///
    /// The tile that has focus keeps it if it is fully in view where the strip comes to rest.
    /// Otherwise focus goes to a tile that is, which is the nearest column to it that is, and the
    /// result says which.
    pub fn pan_end(&mut self, velocity: f64) -> Option<Snap<K>> {
        if self.columns.is_empty() {
            return None;
        }

        let geometry = self.geometry();
        let view_width = f64::from(self.viewport.width);
        let gaps = f64::from(self.options.gaps);
        let (min, max) = self.view_range(&geometry);

        let from = geometry.xs[self.active] - self.active_x;
        let velocity = if velocity.is_finite() { -velocity } else { 0.0 };

        // where the viewport's left edge is when each column is aligned in each of the ways that
        // a column can be aligned
        let mut alignments = Vec::with_capacity(self.columns.len() * 2);
        for (index, width) in geometry.widths.iter().enumerate() {
            let x = geometry.xs[index];

            if self.centers_focused_column() {
                alignments.push(x - view::center(*width, view_width));
            } else {
                alignments.push(x - view::fit(f64::NEG_INFINITY, *width, view_width, gaps));
                alignments.push(x - view::fit(f64::INFINITY, *width, view_width, gaps));
            }
        }

        let mut to = view::snap(from, velocity, &alignments, min, max);

        // the columns that are fully in view where it comes to rest
        let fully_visible = |position: f64, index: usize| {
            let x = geometry.xs[index] - position;
            let width = geometry.widths[index];

            if width >= view_width {
                return x.abs() < EPSILON;
            }

            let margin = ((view_width - width) / 2.0).clamp(0.0, gaps);
            x >= margin - EPSILON && x + width <= view_width - margin + EPSILON
        };

        let mut active = if fully_visible(to, self.active) {
            Some(self.active)
        } else {
            (0..self.columns.len())
                .filter(|index| fully_visible(to, *index))
                .min_by_key(|index| index.abs_diff(self.active))
        };

        if active.is_none() {
            // not a single column fits where it came to rest, which happens when the columns are
            // wider than the viewport, so the column nearest the middle is aligned instead
            let middle = to + view_width / 2.0;
            let nearest = (0..self.columns.len())
                .min_by(|a, b| {
                    let from_middle = |index: usize| {
                        (geometry.xs[index] + geometry.widths[index] / 2.0 - middle).abs()
                    };

                    from_middle(*a).total_cmp(&from_middle(*b))
                })
                .unwrap_or(self.active);

            to = geometry.xs[nearest]
                - if self.centers_focused_column() {
                    view::center(geometry.widths[nearest], view_width)
                } else {
                    view::fit(
                        f64::NEG_INFINITY,
                        geometry.widths[nearest],
                        view_width,
                        gaps,
                    )
                };
            active = Some(nearest);
        }

        let active = active.unwrap_or(self.active);

        if active != self.active {
            self.active = active;
        }
        self.active_x = geometry.xs[self.active] - to;
        self.return_to = None;
        self.restore_x = None;

        Some(Snap {
            from,
            to,
            active: self.columns[self.active].last().clone(),
        })
    }

    /// The furthest left and furthest right that the left edge of the viewport can be in the
    /// strip: with the first column aligned to the left, and with the last aligned to the right,
    /// or the same if the strip fits in the viewport.
    fn view_range(&self, geometry: &Geometry) -> (f64, f64) {
        let view_width = f64::from(self.viewport.width);
        let gaps = f64::from(self.options.gaps);
        let last = self.columns.len() - 1;

        let (first_aligned, last_aligned) = if self.centers_focused_column() {
            (
                -view::center(geometry.widths[0], view_width),
                geometry.xs[last] - view::center(geometry.widths[last], view_width),
            )
        } else {
            (
                -view::fit(f64::NEG_INFINITY, geometry.widths[0], view_width, gaps),
                geometry.xs[last]
                    - view::fit(f64::INFINITY, geometry.widths[last], view_width, gaps),
            )
        };

        (first_aligned, last_aligned.max(first_aligned))
    }

    /// Centre the active column, this once.
    pub fn center_column(&mut self) {
        if self.columns.is_empty() || self.columns[self.active].fullscreen {
            return;
        }

        let width = self.geometry().widths[self.active];
        self.active_x = view::center(width, f64::from(self.viewport.width));
    }

    /// Centre all of the columns that are fully in view, as a group, this once.
    ///
    /// Nothing happens if the active column is not one of them.
    pub fn center_visible_columns(&mut self) {
        if self.columns.is_empty() || self.centers_focused_column() {
            return;
        }

        let geometry = self.geometry();
        let Some(group) = self.fully_visible(&geometry) else {
            return;
        };

        let gaps = f64::from(self.options.gaps);
        let view_width = f64::from(self.viewport.width);
        let view_pos = geometry.xs[self.active] - self.active_x;

        // What is left over once the columns and the gaps between them are taken out is split
        // between the two sides.
        let margin = (view_width - group.taken + gaps) / 2.0;
        let leftmost_x = geometry.xs[group.leftmost] - view_pos;

        self.active_x += margin - leftmost_x;
        self.reveal(None);
    }

    /// Make the active column as wide as it can be without hiding any of the columns that are
    /// fully in view with it, and bring the leftmost of those to the left edge.
    ///
    /// If the active column is the only one in view, it takes the whole viewport width instead,
    /// which [`toggle_full_width`](Self::toggle_full_width) can undo.
    pub fn expand_to_available_width(&mut self) {
        if self.columns.is_empty() {
            return;
        }

        let active = &self.columns[self.active];
        if active.fullscreen || active.full_width {
            return;
        }

        let id = active.id.clone();

        if self.centers_focused_column() {
            self.toggle_full_width(&id);
            return;
        }

        let geometry = self.geometry();
        let Some(group) = self.fully_visible(&geometry) else {
            return;
        };

        let gaps = f64::from(self.options.gaps);
        let view_width = f64::from(self.viewport.width);

        // The group has a gap after each column, and there is one more before the first.
        let available = view_width - gaps - group.taken;
        if available <= 0.0 {
            return;
        }

        if !group.has_others {
            self.toggle_full_width(&id);
            return;
        }

        let view_pos = geometry.xs[self.active] - self.active_x;
        let leftmost_x = geometry.xs[group.leftmost] - view_pos;
        let width = geometry.widths[self.active] + available;

        let column = &mut self.columns[self.active];
        column.width = Some(ColumnWidth::Fixed(width.floor() as i32));
        column.preset = None;
        column.full_width = false;

        self.active_x += gaps - leftmost_x;
        self.reveal(None);
    }

    /// Make the strip hold exactly these keys in exactly this order, with `focused` active, by
    /// changing as little as possible.
    ///
    /// This is for a host that keeps its own list of tiles, which it changes by its own rules and
    /// not through this model. Given the list after each change, the strip behaves as if every
    /// change had been made through it: tiles that are no longer in the list are removed, new ones
    /// are added (and made active, if one is `focused`), the others are moved into order, and
    /// focus follows. Columns that are in both keep their width.
    ///
    /// The list is in reading order, and the columns are worked out from it: a tile that was in
    /// a column with the tile before it is in a column with it still, and every other tile
    /// starts a column of its own. So a tile that is new starts a column, and a list that the
    /// host changed in a way that the model would not have is made sense of, with the list the
    /// one that is right.
    pub fn reconcile(&mut self, ids: &[K], focused: Option<&K>) {
        let mut wanted: Vec<&K> = Vec::with_capacity(ids.len());
        for id in ids {
            if !wanted.contains(&id) {
                wanted.push(id);
            }
        }

        let gone: Vec<K> = self
            .ids()
            .into_iter()
            .filter(|id| !wanted.contains(&id))
            .collect();
        for id in &gone {
            self.remove_column(id);
        }

        // the tiles that are with the tile before them, which every column but the first tile of
        // it is
        let joined: Vec<K> = self
            .columns
            .iter()
            .flat_map(|column| column.rest.iter().cloned())
            .collect();

        let mut runs: Vec<Vec<K>> = Vec::new();
        for id in &wanted {
            match runs.last_mut() {
                Some(run) if joined.contains(id) => run.push((*id).clone()),
                _ => runs.push(vec![(*id).clone()]),
            }
        }

        // every column that is there takes the tiles of its run. A tile that is not in the run of
        // the column that had it has gone to another, which takes it.
        for column in &mut self.columns {
            if let Some(run) = runs.iter().find(|run| run[0] == column.id)
                && column.tiles().ne(run.iter())
            {
                column.set_tiles(run.clone());
            }
        }

        for (index, run) in runs.iter().enumerate() {
            if self.columns.iter().all(|column| column.id != run[0]) {
                let activate = focused.is_some_and(|focused| run.contains(focused));
                self.add_column(run[0].clone(), Some(index), None, activate);

                if let Some(at) = self.position(&run[0]) {
                    self.columns[at].set_tiles(run.clone());
                }
            }
        }

        for (index, run) in runs.iter().enumerate() {
            if self.columns[index].id != run[0] {
                self.move_column(&run[0], index);
            }
        }

        if let Some(focused) = focused {
            self.focus(focused);
        }
    }

    /// Where every column goes, from left to right.
    #[must_use]
    pub fn layout(&self) -> Vec<Placement<K>> {
        let geometry = self.geometry();
        let view_pos = if self.columns.is_empty() {
            0.0
        } else {
            geometry.xs[self.active] - self.active_x
        };

        let gaps = self.options.gaps;
        let Viewport { width, height } = self.viewport;

        let mut placements = Vec::with_capacity(self.tile_count());

        for (index, column) in self.columns.iter().enumerate() {
            let left = geometry.xs[index] - view_pos;
            let right = left + geometry.widths[index];

            // Edges are rounded, not widths, so that columns that touch in the strip still do
            // once they are in pixels.
            let x = left.round() as i32;
            let end = right.round() as i32;

            let (top, column_height, between) = if column.fullscreen {
                (0, height, 0)
            } else {
                (gaps, (height - 2 * gaps).max(1), gaps)
            };

            let visibility = if end <= 0 {
                Visibility::OffLeft
            } else if x >= width {
                Visibility::OffRight
            } else if x >= 0 && end <= width {
                Visibility::Visible
            } else {
                Visibility::Clipped
            };

            // The tiles share the height of the column, with a gap between each, and edges are
            // rounded here too so that the tiles meet.
            let heights = self.tile_heights(column, f64::from(column_height), f64::from(between));
            let mut tile_top = f64::from(top);

            for ((tile, id), tile_height) in column.tiles().enumerate().zip(heights) {
                let y = tile_top.round() as i32;
                let bottom = (tile_top + tile_height).round() as i32;
                tile_top += tile_height + f64::from(between);

                placements.push(Placement {
                    id: id.clone(),
                    rect: Rect {
                        x,
                        y,
                        width: end - x,
                        height: (bottom - y).max(1),
                    },
                    visibility,
                    active: index == self.active && tile == column.last_tile,
                });
            }
        }

        placements
    }

    /// The height that a tile was given, if it was given one.
    fn height_given(&self, id: &K) -> Option<TileHeight> {
        self.heights
            .iter()
            .find(|(tile, _)| tile == id)
            .map(|(_, height)| *height)
    }

    /// How tall each tile of a column is, from the top, in pixels and not rounded, in a column of
    /// `column_height` with `between` between its tiles.
    ///
    /// Tiles with a height of their own have it, and the others share what is left equally, but
    /// no tile is made shorter than [`Options::minimum_tile_height`] unless the column has not got
    /// the room for that many tiles that tall, when they are all the same.
    fn tile_heights(&self, column: &Column<K>, column_height: f64, between: f64) -> Vec<f64> {
        let count = column.tile_count();
        let room = (column_height - (count - 1) as f64 * between).max(count as f64);
        let equal = room / count as f64;
        let minimum = f64::from(self.options.minimum_tile_height.max(0));

        if count == 1 || room < minimum * count as f64 {
            return vec![equal; count];
        }

        let given: Vec<Option<f64>> = column
            .tiles()
            .map(|id| match self.height_given(id)? {
                TileHeight::Auto => None,
                TileHeight::Proportion(proportion) => Some((room * proportion).max(minimum)),
                TileHeight::Fixed(pixels) => Some(f64::from(pixels).max(minimum)),
            })
            .collect();

        let others = given.iter().filter(|height| height.is_none()).count();
        let taken: f64 = given.iter().flatten().sum();

        // with nothing to take up the rest, what was asked for is what each gets, in proportion
        if others == 0 {
            return given.iter().flatten().map(|h| h * room / taken).collect();
        }

        // what is asked for cannot leave the others with less than the least
        let most = room - minimum * others as f64;
        let scale = if taken > most { most / taken } else { 1.0 };
        let left = room - taken * scale;

        given
            .into_iter()
            .map(|height| height.map_or(left / others as f64, |height| height * scale))
            .collect()
    }

    /// How tall the tile with this key is, in pixels and not rounded, or `None` if there is no
    /// such tile.
    #[must_use]
    pub fn tile_height(&self, id: &K) -> Option<f64> {
        let column = &self.columns[self.position(id)?];
        let (_, column_height, between) = self.column_extent(column);

        let heights = self.tile_heights(column, column_height, between);
        Some(heights[column.tile_index(id)?])
    }

    /// Where a column starts from the top, how tall it is, and the gap between its tiles.
    fn column_extent(&self, column: &Column<K>) -> (f64, f64, f64) {
        let gaps = f64::from(self.options.gaps);
        let height = f64::from(self.viewport.height);

        if column.fullscreen {
            (0.0, height, 0.0)
        } else {
            (gaps, (height - 2.0 * gaps).max(1.0), gaps)
        }
    }

    /// What a width comes to in pixels in the current viewport, before any limits.
    fn resolve(&self, width: ColumnWidth) -> f64 {
        match width {
            ColumnWidth::Proportion(proportion) => {
                let gaps = f64::from(self.options.gaps);
                (f64::from(self.viewport.width) - gaps) * proportion - gaps
            }
            ColumnWidth::Fixed(pixels) => f64::from(pixels),
        }
    }

    /// How wide a column is, in pixels.
    fn width_of(&self, column: &Column<K>) -> f64 {
        if column.fullscreen {
            return f64::from(self.viewport.width).max(1.0);
        }

        let mut width = if column.full_width {
            self.resolve(ColumnWidth::Proportion(1.0))
        } else {
            self.resolve(column.width.unwrap_or(self.options.default_width))
        };

        if let Some(max) = column.max_width {
            width = width.min(f64::from(max));
        }

        if let Some(min) = column.min_width {
            width = width.max(f64::from(min));
        }

        width.max(1.0)
    }

    fn geometry(&self) -> Geometry {
        let gaps = f64::from(self.options.gaps);
        let widths: Vec<f64> = self
            .columns
            .iter()
            .map(|column| self.width_of(column))
            .collect();

        let mut xs = Vec::with_capacity(widths.len());
        let mut x = 0.0;
        for width in &widths {
            xs.push(x);
            x += width + gaps;
        }

        Geometry { widths, xs }
    }

    fn reset_view(&mut self) {
        self.active = 0;
        self.active_x = 0.0;
        self.return_to = None;
        self.restore_x = None;
    }

    /// Whether the active column is centred whatever else is the case.
    fn centers_focused_column(&self) -> bool {
        self.options.center_focused_column == CenterFocusedColumn::Always
            || (self.options.always_center_single_column && self.columns.len() <= 1)
    }

    /// Whether the active column should be centred, having just become active, coming from the
    /// column at `from` if there was one.
    fn should_center(&self, geometry: &Geometry, from: Option<usize>) -> bool {
        if self.centers_focused_column() {
            return true;
        }

        let (CenterFocusedColumn::OnOverflow, Some(from)) =
            (self.options.center_focused_column, from)
        else {
            return false;
        };

        if from == self.active {
            return false;
        }

        // Not the column that focus came from, but the one next to the new column on that side:
        // jumping a long way does not make the new column any more likely to need centring.
        let neighbour = if from > self.active {
            self.active + 1
        } else {
            self.active - 1
        };

        let left = geometry.xs[neighbour].min(geometry.xs[self.active]);
        let right = (geometry.xs[neighbour] + geometry.widths[neighbour])
            .max(geometry.xs[self.active] + geometry.widths[self.active]);

        let gaps = f64::from(self.options.gaps);
        right - left + 2.0 * gaps > f64::from(self.viewport.width)
    }

    /// Put the active column where it belongs in view, starting from where it is now.
    fn reveal(&mut self, from: Option<usize>) {
        if self.columns.is_empty() {
            return;
        }

        let geometry = self.geometry();
        let width = geometry.widths[self.active];
        let view_width = f64::from(self.viewport.width);
        let gaps = f64::from(self.options.gaps);

        self.active_x = if self.columns[self.active].fullscreen {
            0.0
        } else if self.should_center(&geometry, from) {
            view::center(width, view_width)
        } else {
            view::fit(self.active_x, width, view_width, gaps)
        };
    }

    /// Make another column active, keeping the viewport and scrolling only as far as is needed.
    fn activate(&mut self, index: usize) {
        if index == self.active {
            return;
        }

        let geometry = self.geometry();
        let view_pos = geometry.xs[self.active] - self.active_x;
        let previous = self.active;

        self.active = index;
        self.active_x = geometry.xs[index] - view_pos;
        self.return_to = None;
        self.restore_x = None;

        self.reveal(Some(previous));
    }

    /// A column changed width. Only the active column needs anything done about it: the columns
    /// to its right are placed from wherever it ends, and the ones to its left are placed from
    /// where it starts, which has not changed.
    fn after_resize(&mut self, index: usize) {
        if index == self.active {
            self.reveal(None);
        }
    }

    /// The columns that are fully in view, with at least a gap on either side, if the active one
    /// is among them.
    fn fully_visible(&self, geometry: &Geometry) -> Option<Group> {
        let gaps = f64::from(self.options.gaps);
        let view_width = f64::from(self.viewport.width);
        let view_pos = geometry.xs[self.active] - self.active_x;

        let mut leftmost = None;
        let mut taken = 0.0;
        let mut has_active = false;
        let mut has_others = false;

        for (index, (x, width)) in geometry.xs.iter().zip(&geometry.widths).enumerate() {
            let x = x - view_pos;

            if x < gaps - TOLERANCE {
                continue;
            }

            if x + width + gaps > view_width + TOLERANCE {
                break;
            }

            leftmost.get_or_insert(index);
            if index == self.active {
                has_active = true;
            } else {
                has_others = true;
            }

            taken += width + gaps;
        }

        has_active.then_some(Group {
            leftmost: leftmost?,
            taken,
            has_others,
        })
    }
}
