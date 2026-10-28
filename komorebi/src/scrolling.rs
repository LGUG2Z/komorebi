//! The glue between a [`Workspace`] and the strip of columns of the Scrolling layout.
//!
//! Everything that does not touch Windows lives in [`komorebi_layouts::scrolling`] and the
//! `komorebi-scrolling` crate, so that every komorebi shares it. What is here is what needs a
//! [`Workspace`]: a container is a tile in the strip, and is known to it by its id.

use crate::container::Container;
use crate::core::DefaultLayout;
use crate::core::Layout;
use crate::core::LayoutOptions;
use crate::core::OperationDirection;
use crate::core::Rect;
use crate::core::ScrollingLayoutOptions;
use crate::core::Sizing;
use crate::workspace::Workspace;
use color_eyre::eyre;
use komorebi_layouts::scrolling;
use komorebi_layouts::scrolling::ScrollingPlacement;
use komorebi_scrolling::Options;
use komorebi_scrolling::Visibility;
use std::sync::atomic::Ordering;

/// What the layout options of a workspace come to for the model.
pub(crate) fn scrolling_options(layout_options: Option<LayoutOptions>) -> Options {
    scrolling::scrolling_options(layout_options)
}

impl Workspace {
    /// Whether the workspace is laid out as a strip of columns.
    pub(crate) fn is_scrolling(&self) -> bool {
        matches!(self.layout, Layout::Default(DefaultLayout::Scrolling))
    }

    fn scrolling_ids(&self) -> Vec<String> {
        self.containers()
            .iter()
            .map(|container| container.id.clone())
            .collect()
    }

    /// Set how many columns the Scrolling layout shows at once, keeping every other layout option.
    pub fn set_scrolling_columns(&mut self, columns: usize) {
        let mut options = self.layout_options.unwrap_or(LayoutOptions {
            scrolling: None,
            grid: None,
            column_ratios: None,
            row_ratios: None,
        });

        match &mut options.scrolling {
            Some(scrolling) => scrolling.columns = columns,
            None => {
                options.scrolling = Some(ScrollingLayoutOptions {
                    columns,
                    center_focused_column: None,
                });
            }
        }

        self.layout_options = Some(options);
    }

    /// Make the space match the containers, and the focus, as they are now.
    fn sync_scrolling(&mut self, viewport: komorebi_scrolling::Viewport) {
        let options = scrolling_options(self.effective_layout_options());
        let ids = self.scrolling_ids();
        let focused = self
            .focused_container()
            .map(|container| container.id.clone());

        scrolling::sync(
            &mut self.scrolling,
            options,
            viewport,
            &ids,
            focused.as_ref(),
        );
    }

    /// Say what the column of the container at this index was given, as it is about to be taken
    /// out of the workspace, for the workspace that it is put in to find.
    pub(crate) fn note_scrolling_departure(&self, idx: usize) {
        if let Some(space) = &self.scrolling
            && let Some(container) = self.containers().get(idx)
            && let Some(record) = space.column_record(&container.id)
        {
            scrolling::note_departure(&container.id, record);
        }
    }

    /// Make the space match the containers and the focus as they are now, for a space that is
    /// there to be made to match.
    ///
    /// This is for a change that is about to be made to the containers, and that the space is
    /// going to be told about directly. The space has to know what there is before it is told
    /// what goes.
    pub(crate) fn sync_scrolling_containers(&mut self) {
        let ids = self.scrolling_ids();
        let focused = self
            .focused_container()
            .map(|container| container.id.clone());

        if let Some(space) = self.scrolling.as_mut() {
            scrolling::tell_of_arrivals(space, &ids);
            space.reconcile(&ids, focused.as_ref());
        }
    }

    /// Give focus to what the strip says should have it after the container `removed` has gone,
    /// and tell whether the strip had anything to say.
    ///
    /// Focus goes back to the column that the closed one was opened next to, if nothing has
    /// moved focus since, and otherwise to the column that takes its place on the right. Focus that
    /// is not on the closed column stays where it is. This is not what the other layouts do, which
    /// always move focus to the left.
    pub(crate) fn focus_after_scrolling_removal(&mut self, removed: &str) -> bool {
        let Some(space) = self.scrolling.as_mut() else {
            return false;
        };

        if !space.remove_column(&removed.to_string()) {
            return false;
        }

        // a container is a tile, and the strip says where in reading order the one that has focus
        // is, which is the index of the container
        if let Some(idx) = space.active_tile_index() {
            self.focus_container(idx);
        }

        true
    }

    /// Where every container goes in `work_area`, from left to right, with `container_padding`
    /// taken off every side of each.
    pub(crate) fn layout_scrolling(
        &mut self,
        work_area: &Rect,
        container_padding: i32,
    ) -> Vec<ScrollingPlacement> {
        self.sync_scrolling(scrolling::viewport_of(work_area));

        self.scrolling
            .as_ref()
            .map(|space| scrolling::place(space, work_area, container_padding))
            .unwrap_or_default()
    }

    /// Where the container that a new window gets goes, if the strip has a say: after the last
    /// tile of the column that has focus.
    ///
    /// A new window is a column of its own beside the focused one, and not a tile in the middle of
    /// it, which is what the container after the focused one would be if the column has other
    /// tiles below.
    pub(crate) fn scrolling_insertion_idx(&self) -> Option<usize> {
        if !self.is_scrolling() {
            return None;
        }

        let space = self.scrolling.as_ref()?;
        let focused = self.focused_container()?.id.clone();
        let last = space.column_of(&focused)?.pop()?;

        self.containers()
            .iter()
            .position(|container| container.id == last)
            .map(|idx| idx + 1)
    }

    /// Where focus would go from the focused container in this direction, in the strip as it is
    /// now, or `None` if it would not go anywhere.
    ///
    /// Left and right are the neighbouring columns, and focus goes to the tile that had it last
    /// in the one it arrives at. Up and down are the tiles of the focused column, and stop at
    /// the ends.
    pub(crate) fn scrolling_idx_for_direction(
        &self,
        direction: OperationDirection,
    ) -> Option<usize> {
        // the strip may not have heard about everything that happened since the last layout, so
        // this goes by a copy of it that has
        let mut probe = self.scrolling.clone()?;
        let ids = self.scrolling_ids();
        let focused = self
            .focused_container()
            .map(|container| container.id.clone());
        probe.reconcile(&ids, focused.as_ref());

        let moved = match direction {
            OperationDirection::Left => probe.focus_left(),
            OperationDirection::Right => probe.focus_right(),
            OperationDirection::Up => probe.focus_up(),
            OperationDirection::Down => probe.focus_down(),
        };

        if moved {
            probe.active_tile_index()
        } else {
            None
        }
    }

    /// Make the order of the containers the order of the tiles of the strip, and focus the tile
    /// that the strip says has it.
    ///
    /// Most of what the strip does leaves the order alone. What moves a tile past another does
    /// not, and the list of containers is what is true, so it is made to match.
    fn apply_scrolling_order(&mut self) {
        let Some(space) = self.scrolling.as_ref() else {
            return;
        };

        let order = space.ids();
        let focused = space.active_tile_index();

        let containers = self.containers_mut();
        let mut containers_in_order: Vec<_> = containers.drain(..).collect();
        containers_in_order.sort_by_key(|container| {
            order
                .iter()
                .position(|id| *id == container.id)
                .unwrap_or(usize::MAX)
        });
        containers.extend(containers_in_order);

        if let Some(idx) = focused {
            self.focus_container(idx);
        }

        // resize adjustments are not used in this layout, and must not follow a container
        // to where it no longer is
        self.resize_dimensions = vec![None; self.containers().len()];
    }

    /// Make the space agree with which container has focus, and give back the id of that
    /// container, for a command that is about the focused column.
    ///
    /// There is nothing to do until a layout has made the space, and in a layout that is not
    /// Scrolling.
    fn focused_scrolling_column(&mut self) -> Option<String> {
        if !self.is_scrolling() || self.scrolling.is_none() {
            return None;
        }

        self.sync_scrolling_containers();

        self.focused_container()
            .map(|container| container.id.clone())
    }

    /// Move the focused column one place to the left or right, or the focused tile one place up
    /// or down in its column, and tell whether anything moved.
    ///
    /// A column moves with all of its tiles.
    pub(crate) fn move_scrolling(&mut self, direction: OperationDirection) -> bool {
        let Some(id) = self.focused_scrolling_column() else {
            return false;
        };

        let Some(space) = self.scrolling.as_mut() else {
            return false;
        };

        let moved = match direction {
            OperationDirection::Up => space.move_tile_up(),
            OperationDirection::Down => space.move_tile_down(),
            OperationDirection::Left | OperationDirection::Right => match space.position(&id) {
                Some(at) => {
                    let to = match direction {
                        OperationDirection::Left => at.checked_sub(1),
                        _ => Some(at + 1).filter(|to| *to < space.len()),
                    };

                    match to {
                        Some(to) => {
                            space.move_column(&id, to);
                            true
                        }
                        None => false,
                    }
                }
                None => false,
            },
        };

        if moved {
            self.apply_scrolling_order();
        }

        moved
    }

    /// Take the neighbouring tile on one side into the focused column: the first tile of the
    /// column on the right to the bottom of it, or the last tile of the column on the left to the
    /// top of it. Up and down mean nothing here.
    pub(crate) fn consume_scrolling(&mut self, direction: OperationDirection) {
        if self.focused_scrolling_column().is_none() {
            return;
        }

        let Some(space) = self.scrolling.as_mut() else {
            return;
        };

        let changed = match direction {
            OperationDirection::Left => space.consume_left(),
            OperationDirection::Right => space.consume_right(),
            OperationDirection::Up | OperationDirection::Down => false,
        };

        if changed {
            self.apply_scrolling_order();
        }
    }

    /// Take the focused tile out of its column and put it in a column of its own on the right.
    pub(crate) fn expel_scrolling(&mut self) {
        if self.focused_scrolling_column().is_none() {
            return;
        }

        let Some(space) = self.scrolling.as_mut() else {
            return;
        };

        if space.expel() {
            self.apply_scrolling_order();
        }
    }

    /// Stack the windows of the focused column in one tile, or spread the stack in the focused tile
    /// out into the tiles of a column, whichever it is, and tell whether there was anything to do.
    ///
    /// A column of several tiles becomes one tile that is a stack of all of their windows, in the
    /// order that they were in, with the window that had focus still showing. A tile that is a stack
    /// becomes a column of a tile for each of its windows, in the order of its tabs, with focus on
    /// the window that had it. Either way the column keeps its width, but the heights that its tiles
    /// were given are not remembered for when it is toggled back.
    pub(crate) fn toggle_scrolling_column_stack(&mut self) -> eyre::Result<bool> {
        let Some(id) = self.focused_scrolling_column() else {
            return Ok(false);
        };

        let Some(column) = self
            .scrolling
            .as_ref()
            .and_then(|space| space.column_of(&id))
        else {
            return Ok(false);
        };

        if column.len() > 1 {
            self.stack_scrolling_column(&column);

            return Ok(true);
        }

        Ok(self.unstack_scrolling_column())
    }

    /// Put the windows of all of the containers with these ids in the first of them, in order.
    fn stack_scrolling_column(&mut self, column: &[String]) {
        let focused_window = self
            .focused_container()
            .and_then(Container::focused_window)
            .map(|window| window.hwnd);

        let mut windows = vec![];
        for id in &column[1..] {
            if let Some(idx) = self.containers().iter().position(|c| c.id == *id)
                && let Some(container) = self.remove_container_by_idx(idx)
            {
                windows.extend(container.windows().iter().copied());
            }
        }

        let Some(idx) = self.containers().iter().position(|c| c.id == column[0]) else {
            return;
        };

        if let Some(container) = self.containers_mut().get_mut(idx) {
            for window in windows {
                container.add_window(window);
            }

            // the window that had focus is the one that is showing
            if let Some(focused) = focused_window
                && let Some(at) = container.idx_for_window(focused)
            {
                container.focus_window(at);
            }

            if !cfg!(test) {
                container.load_focused_window();
            }
        }

        self.focus_container(idx);
    }

    /// Put each of the windows of the focused container in a container of its own, in the same
    /// column, and tell whether it had more than one.
    fn unstack_scrolling_column(&mut self) -> bool {
        let idx = self.focused_container_idx();

        let Some(container) = self.containers_mut().get_mut(idx) else {
            return false;
        };

        if container.windows().len() < 2 {
            return false;
        }

        let focused = container.focused_window().map(|window| window.hwnd);
        let first_id = container.id.clone();

        let mut windows: Vec<_> = container.windows_mut().drain(..).collect();
        let rest = windows.split_off(1);
        container.windows_mut().extend(windows);
        container.focus_window(0);

        // the strip is about to be told that there are more containers than it has columns for,
        // and so each of them is a column, until they are put in this one
        let mut count = 0;
        for (offset, window) in rest.into_iter().enumerate() {
            let mut container = Container::default();
            container.add_window(window);

            // it was one of the tabs that were out of sight, and has to be put where it is shown
            if !cfg!(test) {
                container.restore();
            }

            self.insert_container_at_idx(idx + 1 + offset, container);
            count += 1;
        }

        self.sync_scrolling_containers();

        if let Some(space) = self.scrolling.as_mut() {
            space.focus(&first_id);
            for _ in 0..count {
                space.consume_right();
            }
        }

        // focus is on the tile that had the window that had it
        let focused_idx = focused
            .and_then(|focused| self.container_idx_for_window(focused))
            .unwrap_or(idx);
        self.focus_container(focused_idx);

        true
    }

    /// Put every column back to the width that the `columns` option gives, which forgets every
    /// width that was set by resizing, by a preset, or by taking the full width, and every window
    /// back to an equal share of the height of its column.
    ///
    /// The focused column stays where it is on screen unless it has to scroll to stay in view.
    pub(crate) fn reset_scrolling_sizes(&mut self) {
        if let Some(space) = self.scrolling.as_mut() {
            space.reset_widths();
            space.reset_heights();
        }
    }

    /// Make the focused column as wide as the screen, or give it back the width that it had.
    pub(crate) fn toggle_scrolling_full_width(&mut self) {
        if let Some(id) = self.focused_scrolling_column()
            && let Some(space) = self.scrolling.as_mut()
        {
            space.toggle_full_width(&id);
        }
    }

    /// Make the focused column as wide as it can be without scrolling anything else out of view,
    /// which is to fill the room that the columns in view leave. Only the Scrolling layout has
    /// columns to expand.
    pub(crate) fn expand_scrolling_column(&mut self) {
        if self.is_scrolling()
            && self.focused_scrolling_column().is_some()
            && let Some(space) = self.scrolling.as_mut()
        {
            space.expand_to_available_width();
        }
    }

    /// Centre the focused column on the screen, this once.
    pub(crate) fn center_scrolling_column(&mut self) {
        if self.focused_scrolling_column().is_some()
            && let Some(space) = self.scrolling.as_mut()
        {
            space.center_column();
        }
    }

    /// Make the focused column wider or narrower with the horizontal edges, and the focused window
    /// taller or shorter in its column with the vertical ones.
    pub(crate) fn resize_scrolling_column(
        &mut self,
        direction: OperationDirection,
        sizing: Sizing,
        delta: i32,
    ) {
        let Some(id) = self
            .focused_container()
            .map(|container| container.id.clone())
        else {
            return;
        };

        if let Some(space) = self.scrolling.as_mut() {
            scrolling::resize(space, &id, direction, sizing, delta);
        }
    }
}
