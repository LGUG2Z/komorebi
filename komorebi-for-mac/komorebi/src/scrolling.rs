//! The Scrolling layout of a workspace, which is laid out by [`komorebi_scrolling`] and not by the
//! layouts in `komorebi_layouts`.
//!
//! The other layouts are stateless: where a container goes is a function of the list of
//! containers. A strip of columns that a viewport scrolls over is not, because where the viewport
//! is and how wide each column has been made are facts that cannot be worked out from the list. So
//! a workspace in the Scrolling layout keeps a [`ScrollingSpace`] next to its containers, which is
//! told after every change what the containers are and which one has focus, and which answers with
//! where they all go.
//!
//! The list of containers stays what is true. Every layout change starts from it, and the space is
//! made to match it, so that nothing that changes the list has to know the space exists.

use crate::arrival;
use crate::container::Container;
use crate::core::DefaultLayout;
use crate::core::Layout;
use crate::core::LayoutOptions;
use crate::core::OperationDirection;
use crate::core::Rect;
use crate::core::Sizing;
use crate::core::WindowHidingPosition;
use crate::core_graphics::CoreGraphicsApi;
use crate::is_visible_on_another_display;
use crate::window::is_stepping;
use crate::workspace::Workspace;
use crate::workspace::clamp_to_work_area;
use color_eyre::eyre;
use komorebi_layouts::scrolling;
use komorebi_layouts::scrolling::ScrollingPlacement;
use komorebi_layouts::scrolling::model::ColumnWidth;
use komorebi_layouts::scrolling::model::Options;
use komorebi_layouts::scrolling::model::ScrollingSpace;
use komorebi_layouts::scrolling::model::Viewport;
use komorebi_layouts::scrolling::model::Visibility;
use komorebi_layouts::scrolling::note_departure;
use komorebi_layouts::scrolling::tell_of_arrivals;
use objc2_core_foundation::CGRect;

/// What the layout options of a workspace come to for the model.
pub(crate) fn scrolling_options(layout_options: Option<LayoutOptions>) -> Options {
    scrolling::scrolling_options(layout_options)
}

#[cfg(test)]
thread_local! {
    /// The windows that were put where they are shown, which a test cannot see on a screen.
    static SHOWN: std::cell::RefCell<Vec<u32>> = const { std::cell::RefCell::new(Vec::new()) };
}

/// Put the window that is in front of a container where it is shown, which is where it was before
/// it was hidden as a tab at the back of a stack.
///
/// A test has no screen to put a window on, so it is told which windows this was done for.
fn show(container: &mut Container) -> eyre::Result<()> {
    #[cfg(test)]
    SHOWN.with(|shown| {
        shown
            .borrow_mut()
            .extend(container.focused_window().map(|window| window.id));
    });

    #[cfg(not(test))]
    container.restore()?;

    Ok(())
}

impl Workspace {
    /// Whether the workspace is laid out as a strip of columns.
    pub(crate) fn is_scrolling(&self) -> bool {
        matches!(self.layout, Layout::Default(DefaultLayout::Scrolling))
    }

    /// Make the space match the containers, and the focus, as they are now.
    fn sync_scrolling(&mut self, viewport: Viewport) {
        let options = scrolling_options(self.effective_layout_options());
        let ids: Vec<String> = self
            .containers()
            .iter()
            .map(|container| container.id.clone())
            .collect();
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
            note_departure(&container.id, record);
        }
    }

    /// Make the space match the containers and the focus as they are now, for a space that is
    /// there to be made to match.
    ///
    /// This is for a change that is about to be made to the containers, and that the space is
    /// going to be told about directly. The space has to know what there is before it is told
    /// what goes.
    pub(crate) fn sync_scrolling_containers(&mut self) {
        let ids: Vec<String> = self
            .containers()
            .iter()
            .map(|container| container.id.clone())
            .collect();
        let focused = self
            .focused_container()
            .map(|container| container.id.clone());

        if let Some(space) = self.scrolling.as_mut() {
            tell_of_arrivals(space, &ids);
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

        let Some(space) = &self.scrolling else {
            return vec![];
        };

        scrolling::place(space, work_area, container_padding)
    }

    /// Lay the workspace out as a strip of columns and put every window where it goes.
    ///
    /// * `work_area` is the work area of the monitor, and `adjusted_work_area` is what is left of it
    ///   once offsets and padding are taken out.
    pub(crate) fn arrange_scrolling(
        &mut self,
        work_area: &Rect,
        adjusted_work_area: &Rect,
        container_padding: i32,
        border_offset: i32,
        border_width: i32,
    ) -> eyre::Result<()> {
        // what a column takes up around its window on either side
        let around_window = container_padding + border_offset + border_width;

        // windows next to an edge that is dragged are laid out for every step of the drag, and
        // what they have turned out to be is not asked: a window that has not settled is not what
        // it will be, and a column is never made narrower again once it was made wider for it
        let stepping = is_stepping();

        // windows that were not what they were told to be since the last layout, which is where
        // the animator says so
        if !stepping {
            self.adopt_actual_sizes(around_window, adjusted_work_area.right);
        }

        self.place_scrolling(
            work_area,
            adjusted_work_area,
            container_padding,
            border_offset,
            border_width,
        )?;

        // with animations off every window that changed width was just asked what it is, so a
        // window that refused is known by now, and everything after it is placed again with room
        // for it. Once is enough: the sizes that are read back this time round are picked up by
        // the next layout, and a layout is never made to chase them.
        if !stepping && self.adopt_actual_sizes(around_window, adjusted_work_area.right) {
            self.place_scrolling(
                work_area,
                adjusted_work_area,
                container_padding,
                border_offset,
                border_width,
            )?;
        }

        Ok(())
    }

    /// Learn from what windows turned out to be when they were sent to where the layout put them,
    /// which is that some of them are wider than the layout said and will not be any narrower.
    ///
    /// A column is never made narrower than the window in it needs, and everything after it is
    /// placed where it ends. Only ever wider: what an app will not do is a fact about the app, and
    /// a limit that came and went would make the strip bounce between two layouts.
    ///
    /// `around_window` is what a column takes up around its window on either side, and the strip
    /// is `viewport_width` wide. Returns whether any column changed.
    pub(crate) fn adopt_actual_sizes(&mut self, around_window: i32, viewport_width: i32) -> bool {
        let Some(space) = self.scrolling.as_mut() else {
            return false;
        };

        let mut adopted = false;

        for container in self.containers.elements() {
            // a stack is one tile and as wide as the widest of its windows needs
            let needed = container
                .windows()
                .iter()
                .filter_map(|window| arrival::take(window.id))
                .filter_map(|arrival| {
                    arrival::column_width_for(&arrival, around_window, viewport_width)
                })
                .max();

            let Some(needed) = needed else {
                continue;
            };

            let Some((min, max)) = space.width_limits(&container.id) else {
                continue;
            };

            if min.is_some_and(|min| min >= needed) {
                continue;
            }

            tracing::debug!(
                "a window in column {} will not be narrower than a column of {needed}px",
                container.id
            );

            space.set_width_limits(&container.id, Some(needed), max);
            adopted = true;
        }

        adopted
    }

    /// Put every window where the layout says that it goes.
    fn place_scrolling(
        &mut self,
        work_area: &Rect,
        adjusted_work_area: &Rect,
        container_padding: i32,
        border_offset: i32,
        border_width: i32,
    ) -> eyre::Result<()> {
        let placements = self.layout_scrolling(adjusted_work_area, container_padding);
        let screen = CoreGraphicsApi::display_bounds_for_window_rect(CGRect::from(*work_area));
        let previous = std::mem::take(&mut self.latest_layout);

        let mut latest_layout = Vec::with_capacity(placements.len());

        for (idx, (container, placement)) in self
            .containers_mut()
            .iter_mut()
            .zip(&placements)
            .enumerate()
        {
            let mut rect = placement.rect;
            rect.add_padding(border_offset);
            rect.add_padding(border_width);
            latest_layout.push(rect);

            // a window that is asked for the width that it already has is not asked what it is
            let resized = previous
                .get(idx)
                .is_none_or(|before| before.right != rect.right);

            for window in container.windows_mut() {
                let result = match placement.visibility {
                    Visibility::Visible => window
                        .set_position(&rect)
                        .map(|()| window.note_arrival(&rect, resized)),
                    Visibility::Clipped => {
                        // part of it is in view and the rest is past the edge of the screen,
                        // which is all right unless there is another display past that edge,
                        // where the rest would be seen
                        let mut target = rect;
                        if overhangs_another_display(&rect, screen) {
                            clamp_to_work_area(&mut target, adjusted_work_area);
                        }

                        window
                            .set_position(&target)
                            .map(|()| window.note_arrival(&target, resized))
                    }
                    Visibility::OffLeft => {
                        window.hide_scrolling(WindowHidingPosition::BottomLeft, screen)
                    }
                    Visibility::OffRight => {
                        window.hide_scrolling(WindowHidingPosition::BottomRight, screen)
                    }
                };

                if let Err(error) = result {
                    tracing::warn!("failed to place window for scrolling layout: {error}");
                }
            }
        }

        self.latest_layout = latest_layout;

        Ok(())
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
        let ids: Vec<String> = self
            .containers()
            .iter()
            .map(|container| container.id.clone())
            .collect();
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
            self.stack_scrolling_column(&column)?;

            return Ok(true);
        }

        self.unstack_scrolling_column()
    }

    /// Put the windows of all of the containers with these ids in the first of them, in order.
    fn stack_scrolling_column(&mut self, column: &[String]) -> eyre::Result<()> {
        let hiding_position = self.globals.window_hiding_position;

        let focused_window = self
            .focused_container()
            .and_then(Container::focused_window)
            .map(|window| window.id);

        let mut windows = vec![];
        for id in &column[1..] {
            if let Some(idx) = self.containers().iter().position(|c| c.id == *id)
                && let Some(container) = self.remove_container_by_idx(idx)
            {
                windows.extend(container.windows().iter().cloned());
            }
        }

        let Some(idx) = self.containers().iter().position(|c| c.id == column[0]) else {
            return Ok(());
        };

        if let Some(container) = self.containers_mut().get_mut(idx) {
            for window in &windows {
                container.add_window(window, hiding_position)?;
            }

            // the window that had focus is the one that is showing
            if let Some(focused) = focused_window
                && let Some(at) = container.idx_for_window(focused)
            {
                container.focus_window(at);
            }

            if !cfg!(test) {
                container.load_focused_window(hiding_position)?;
            }
        }

        self.focus_container(idx);

        Ok(())
    }

    /// Put each of the windows of the focused container in a container of its own, in the same
    /// column, and tell whether it had more than one.
    fn unstack_scrolling_column(&mut self) -> eyre::Result<bool> {
        let hiding_position = self.globals.window_hiding_position;
        let idx = self.focused_container_idx();

        let Some(container) = self.containers_mut().get_mut(idx) else {
            return Ok(false);
        };

        if container.windows().len() < 2 {
            return Ok(false);
        }

        let focused = container.focused_window().map(|window| window.id);
        let first_id = container.id.clone();

        let mut windows: Vec<_> = container.windows_mut().drain(..).collect();
        let rest = windows.split_off(1);
        container.windows_mut().extend(windows);
        container.focus_window(0);

        // the window that stays was one of the tabs, and may be the one that was out of sight, so
        // it is put where it is shown as well, or its column is left empty until focus reaches it
        show(container)?;

        // the strip is about to be told that there are more containers than it has columns for,
        // and so each of them is a column, until they are put in this one
        let mut ids = vec![];
        for (offset, window) in rest.iter().enumerate() {
            let mut container = Container::default();
            container.add_window(window, hiding_position)?;
            ids.push(container.id.clone());

            // it was one of the tabs that were out of sight, and has to be put where it is shown
            show(&mut container)?;

            self.insert_container_at_idx(idx + 1 + offset, container);
        }

        // inserting a container gives it focus, and the strip scrolls to bring the focused column
        // into view, which it would do for a column that is only there until it is put in this
        // one, and then not scroll back. Focus stays on the column that they are put in.
        self.focus_container(idx);
        self.sync_scrolling_containers();

        if let Some(space) = self.scrolling.as_mut() {
            space.focus(&first_id);
            for _ in &ids {
                space.consume_right();
            }
        }

        // focus is on the tile that had the window that had it
        let focused_idx = focused
            .and_then(|focused| self.container_idx_for_window(focused))
            .unwrap_or(idx);
        self.focus_container(focused_idx);

        Ok(true)
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

    /// Start moving the strip across the screen by hand, and give back the strip as it is, for
    /// putting it back if the pan is given up. Only the Scrolling layout has a strip to move, and
    /// not before a layout has made it.
    pub(crate) fn begin_scrolling_pan(&mut self) -> Option<ScrollingSpace<String>> {
        if !self.is_scrolling() || self.scrolling.is_none() {
            return None;
        }

        self.sync_scrolling_containers();

        let space = self.scrolling.as_mut()?;
        let saved = space.clone();
        space.begin_pan();

        Some(saved)
    }

    /// Move the strip by `dx` pixels on the screen, to the right if it is positive. Nothing else
    /// changes: not which container has focus, nor the width of any.
    pub(crate) fn pan_scrolling(&mut self, dx: f64) {
        if let Some(space) = self.scrolling.as_mut() {
            space.pan_by(dx);
        }
    }

    /// Let go of the strip, going at `velocity` pixels a second on the screen, to the right if it
    /// is positive, and let it come to rest, which is with a column against an edge of the screen.
    /// Focus goes to the nearest column that is fully in view if the focused one is not, and
    /// this tells whether it moved.
    pub(crate) fn end_scrolling_pan(&mut self, velocity: f64) -> bool {
        let Some(space) = self.scrolling.as_mut() else {
            return false;
        };

        if space.pan_end(velocity).is_none() {
            return false;
        }

        let focused = space.active_tile_index();
        let before = self.focused_container_idx();

        match focused {
            Some(idx) if idx != before => {
                self.focus_container(idx);
                true
            }
            _ => false,
        }
    }

    /// Give up a pan: the strip is put back to what it was when it began.
    pub(crate) fn cancel_scrolling_pan(&mut self, saved: ScrollingSpace<String>) {
        if self.scrolling.is_some() {
            self.scrolling = Some(saved);
        }
    }

    /// How wide the screen is that the strip is moved across, in pixels.
    pub(crate) fn scrolling_view_width(&self) -> Option<i32> {
        self.scrolling.as_ref().map(|space| space.viewport().width)
    }

    /// How far one step of a pan goes, if nothing says: a column of the width that a new one is
    /// given.
    pub(crate) fn scrolling_pan_step(&self) -> Option<i32> {
        let space = self.scrolling.as_ref()?;
        let viewport = f64::from(space.viewport().width);

        let step = match space.options().default_width {
            ColumnWidth::Proportion(share) => viewport * share,
            ColumnWidth::Fixed(width) => f64::from(width),
        };

        Some(step.round() as i32)
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
    /// taller or shorter in its column with the vertical ones. A window that is alone in its
    /// column is as tall as the screen, so resizing it vertically does nothing.
    ///
    /// The layouts that are made of splits move one edge of a container by half of `delta`, and
    /// callers that want to move both, or to follow a mouse, rely on that, so a column changes
    /// width, or a window height, by half of `delta` per edge too.
    pub(crate) fn resize_scrolling_column(
        &mut self,
        direction: OperationDirection,
        sizing: Sizing,
        delta: i32,
    ) {
        // the left edge of a column is moved by panning the strip, which is only done for the
        // active column of the space, so the space has to agree about which one that is
        let Some(id) = self.focused_scrolling_column() else {
            return;
        };

        let Some(space) = self.scrolling.as_mut() else {
            return;
        };

        scrolling::resize(space, &id, direction, sizing, delta);
    }
}

/// Whether a window with this rectangle would be seen on a display other than the one that
/// `screen` is the bounds of.
///
/// If the display is not known, it is safer to say that it would.
fn overhangs_another_display(rect: &Rect, screen: Option<CGRect>) -> bool {
    match screen {
        Some(screen) => is_visible_on_another_display(CGRect::from(*rect), screen),
        None => true,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::container::Container;
    use crate::core::ScrollingLayoutOptions;
    use crate::window::Window;
    use komorebi_layouts::scrolling::MINIMUM_COLUMN_WIDTH;
    use komorebi_layouts::scrolling::MINIMUM_WINDOW_HEIGHT;
    use komorebi_layouts::scrolling::ScrollingCentering;
    use komorebi_layouts::scrolling::model::CenterFocusedColumn;
    use komorebi_layouts::scrolling::model::ColumnWidth;

    const AREA: Rect = Rect {
        left: 100,
        top: 50,
        right: 1200,
        bottom: 800,
    };

    fn options(
        columns: usize,
        center_focused_column: Option<ScrollingCentering>,
    ) -> Option<LayoutOptions> {
        Some(LayoutOptions {
            scrolling: Some(ScrollingLayoutOptions {
                columns,
                center_focused_column,
            }),
            grid: None,
            column_ratios: None,
            row_ratios: None,
        })
    }

    fn workspace(columns: usize, containers: usize) -> Workspace {
        let mut workspace = Workspace {
            layout: Layout::Default(DefaultLayout::Scrolling),
            layout_options: options(columns, None),
            ..Default::default()
        };

        for _ in 0..containers {
            workspace.add_container_to_back(Container::default());
        }

        workspace
    }

    /// Open a container the way a new window does: to the right of the focused one, and in focus.
    fn open(workspace: &mut Workspace) {
        let at = if workspace.containers().is_empty() {
            0
        } else {
            workspace.focused_container_idx() + 1
        };
        workspace.insert_container_at_idx(at, Container::default());
    }

    fn widths(placements: &[ScrollingPlacement]) -> Vec<i32> {
        placements
            .iter()
            .map(|placement| placement.rect.right)
            .collect()
    }

    #[test]
    fn a_column_is_a_share_of_the_viewport_as_wide_as_the_columns_option_says() {
        let options = scrolling_options(options(4, None));
        assert_eq!(options.default_width, ColumnWidth::Proportion(0.25));

        // and three if it says nothing at all
        let options = scrolling_options(None);
        assert_eq!(options.default_width, ColumnWidth::Proportion(1.0 / 3.0));

        // a count of nought cannot be a share of anything
        let options = scrolling_options(options_for(0));
        assert_eq!(options.default_width, ColumnWidth::Proportion(1.0));
    }

    fn options_for(columns: usize) -> Option<LayoutOptions> {
        options(columns, None)
    }

    #[test]
    fn centring_the_focused_column_is_what_the_layout_option_says() {
        for (given, expected) in [
            (None, CenterFocusedColumn::Never),
            (Some(ScrollingCentering::Never), CenterFocusedColumn::Never),
            (
                Some(ScrollingCentering::Always),
                CenterFocusedColumn::Always,
            ),
            (
                Some(ScrollingCentering::OnOverflow),
                CenterFocusedColumn::OnOverflow,
            ),
        ] {
            assert_eq!(
                scrolling_options(options(3, given)).center_focused_column,
                expected
            );
        }
    }

    #[test]
    fn containers_are_laid_out_inside_the_work_area_with_their_padding() {
        let mut workspace = workspace(4, 3);

        let placements = workspace.layout_scrolling(&AREA, 10);

        // 1200 / 4 = 300 a column, less 10 off every side
        assert_eq!(widths(&placements), [280, 280, 280]);
        assert_eq!(placements[0].rect.left, 100 + 10);
        assert_eq!(placements[1].rect.left, 100 + 300 + 10);
        assert_eq!(placements[0].rect.top, 50 + 10);
        assert_eq!(placements[0].rect.bottom, 800 - 20);
    }

    #[test]
    fn a_lone_container_is_not_stretched_to_fill_the_work_area() {
        let mut workspace = workspace(3, 1);

        let placements = workspace.layout_scrolling(&AREA, 0);

        assert_eq!(widths(&placements), [400]);
    }

    #[test]
    fn opening_a_window_does_not_resize_the_others() {
        let mut workspace = workspace(3, 0);

        for count in 1..=7 {
            open(&mut workspace);
            let placements = workspace.layout_scrolling(&AREA, 0);

            assert_eq!(placements.len(), count);
            assert_eq!(widths(&placements), vec![400; count]);
        }
    }

    #[test]
    fn the_view_stays_put_while_focus_moves_within_it() {
        let mut workspace = workspace(3, 6);
        workspace.focus_container(5);
        let initial = workspace.layout_scrolling(&AREA, 0);
        assert_eq!(visible(&initial), [3, 4, 5]);

        // moving focus left across columns that are already in view must not scroll the strip
        for idx in [4, 3] {
            workspace.focus_container(idx);
            assert_eq!(
                workspace.layout_scrolling(&AREA, 0),
                initial,
                "column {idx}"
            );
        }

        // leaving the view on the left scrolls just far enough to bring the column in
        workspace.focus_container(2);
        let scrolled = workspace.layout_scrolling(&AREA, 0);
        assert_eq!(visible(&scrolled), [2, 3, 4]);

        // and moving right within the new view leaves it where it is
        for idx in [3, 4] {
            workspace.focus_container(idx);
            assert_eq!(
                workspace.layout_scrolling(&AREA, 0),
                scrolled,
                "column {idx}"
            );
        }
    }

    fn visible(placements: &[ScrollingPlacement]) -> Vec<usize> {
        placements
            .iter()
            .enumerate()
            .filter(|(_, placement)| placement.visibility == Visibility::Visible)
            .map(|(idx, _)| idx)
            .collect()
    }

    #[test]
    fn a_container_that_scrolls_out_of_view_says_which_side_it_went_by() {
        let mut workspace = workspace(3, 6);
        // the strip scrolls to the end, and then focus moves back to where the last three begin
        workspace.focus_container(5);
        workspace.layout_scrolling(&AREA, 0);
        workspace.focus_container(3);

        let placements = workspace.layout_scrolling(&AREA, 0);

        let visibility: Vec<_> = placements
            .iter()
            .map(|placement| placement.visibility)
            .collect();
        assert_eq!(
            visibility,
            [
                Visibility::OffLeft,
                Visibility::OffLeft,
                Visibility::OffLeft,
                Visibility::Visible,
                Visibility::Visible,
                Visibility::Visible,
            ]
        );

        // a column that is out of view keeps the rectangle it has in the strip
        assert_eq!(placements[2].rect.left, 100 - 400);
    }

    #[test]
    fn a_column_that_was_made_wider_stays_wider_when_others_come_and_go() {
        let mut workspace = workspace(3, 3);
        workspace.layout_scrolling(&AREA, 0);

        workspace.focus_container(1);
        workspace.resize_scrolling_column(OperationDirection::Right, Sizing::Increase, 200);
        let placements = workspace.layout_scrolling(&AREA, 0);
        assert_eq!(widths(&placements), [400, 500, 400]);

        // a window opens to the right of the focused column, and moving focus does not change a width
        open(&mut workspace);
        workspace.focus_container(0);
        let placements = workspace.layout_scrolling(&AREA, 0);
        assert_eq!(widths(&placements), [400, 500, 400, 400]);
    }

    #[test]
    fn resizing_changes_the_width_by_half_of_the_delta_and_never_below_the_minimum() {
        let mut workspace = workspace(3, 2);
        workspace.layout_scrolling(&AREA, 0);

        workspace.resize_scrolling_column(OperationDirection::Left, Sizing::Increase, 100);
        assert_eq!(widths(&workspace.layout_scrolling(&AREA, 0))[1], 450);

        workspace.resize_scrolling_column(OperationDirection::Right, Sizing::Decrease, 100);
        assert_eq!(widths(&workspace.layout_scrolling(&AREA, 0))[1], 400);

        workspace.resize_scrolling_column(OperationDirection::Right, Sizing::Decrease, 10_000);
        assert_eq!(
            widths(&workspace.layout_scrolling(&AREA, 0))[1],
            MINIMUM_COLUMN_WIDTH
        );
    }

    /// Where the edges of the column at `idx` are, from the left and the right
    fn edges_of(placements: &[ScrollingPlacement], idx: usize) -> (i32, i32) {
        let rect = placements[idx].rect;

        (rect.left, rect.left + rect.right)
    }

    #[test]
    fn resizing_by_the_left_edge_moves_that_edge_and_keeps_the_right_one() {
        let mut workspace = workspace(3, 3);
        let before = workspace.layout_scrolling(&AREA, 0);
        let (left, right) = edges_of(&before, 2);

        workspace.resize_scrolling_column(OperationDirection::Left, Sizing::Increase, 100);
        let after = workspace.layout_scrolling(&AREA, 0);

        assert_eq!(edges_of(&after, 2), (left - 50, right));

        // and moving it back puts it back
        workspace.resize_scrolling_column(OperationDirection::Left, Sizing::Decrease, 100);
        assert_eq!(
            edges_of(&workspace.layout_scrolling(&AREA, 0), 2),
            (left, right)
        );
    }

    #[test]
    fn resizing_a_column_that_has_focus_but_is_not_the_active_one_moves_its_left_edge_too() {
        // dragging the edge of a window moves the focus of the workspace to its container, and the
        // strip is only told about that when something asks it to lay out
        let mut workspace = workspace(3, 5);
        workspace.focus_container(0);
        let before = workspace.layout_scrolling(&AREA, 0);
        let (left, right) = edges_of(&before, 1);

        workspace.focus_container(1);
        workspace.resize_scrolling_column(OperationDirection::Left, Sizing::Increase, 100);
        let after = workspace.layout_scrolling(&AREA, 0);

        assert_eq!(edges_of(&after, 1), (left - 50, right));
    }

    #[test]
    fn a_column_is_as_tall_as_the_work_area_so_resizing_its_height_does_nothing() {
        let mut workspace = workspace(3, 2);
        let before = workspace.layout_scrolling(&AREA, 0);

        workspace.resize_scrolling_column(OperationDirection::Up, Sizing::Increase, 100);
        workspace.resize_scrolling_column(OperationDirection::Down, Sizing::Decrease, 100);

        assert_eq!(workspace.layout_scrolling(&AREA, 0), before);
    }

    #[test]
    fn resizing_before_there_is_a_layout_does_nothing() {
        // the space is made by the first layout, and there has not been one
        let mut workspace = workspace(3, 2);

        workspace.resize_scrolling_column(OperationDirection::Right, Sizing::Increase, 100);

        assert!(workspace.scrolling.is_none());
    }

    #[test]
    fn resetting_the_widths_puts_every_column_back_to_what_the_columns_option_gives() {
        let mut workspace = workspace(3, 4);
        workspace.layout_scrolling(&AREA, 0);

        workspace.resize_scrolling_column(OperationDirection::Left, Sizing::Increase, 200);
        workspace.focus_container(1);
        workspace.resize_scrolling_column(OperationDirection::Right, Sizing::Decrease, 100);
        let resized = widths(&workspace.layout_scrolling(&AREA, 0));
        assert_ne!(resized, [400; 4]);

        workspace.reset_scrolling_sizes();

        assert_eq!(widths(&workspace.layout_scrolling(&AREA, 0)), [400; 4]);
    }

    #[test]
    fn resetting_the_widths_keeps_the_focused_column_where_it_is() {
        let mut workspace = workspace(3, 6);
        workspace.focus_container(3);
        workspace.layout_scrolling(&AREA, 0);
        workspace.focus_container(1);
        workspace.resize_scrolling_column(OperationDirection::Left, Sizing::Increase, 200);
        workspace.focus_container(3);
        let before = workspace.layout_scrolling(&AREA, 0)[3].rect.left;

        workspace.reset_scrolling_sizes();

        assert_eq!(workspace.layout_scrolling(&AREA, 0)[3].rect.left, before);
        assert_eq!(workspace.focused_container_idx(), 3);
    }

    #[test]
    fn resetting_the_widths_before_there_is_a_layout_does_nothing() {
        let mut workspace = workspace(3, 2);

        workspace.reset_scrolling_sizes();

        assert!(workspace.scrolling.is_none());
    }

    /// A workspace with a layout, which is what makes the strip that the width commands act on.
    fn laid_out(columns: usize, containers: usize, focus: usize) -> Workspace {
        let mut workspace = workspace(columns, containers);
        workspace.focus_container(focus);
        workspace.layout_scrolling(&AREA, 0);
        workspace
    }

    #[test]
    fn the_full_width_is_toggled_on_and_off() {
        let mut workspace = laid_out(3, 3, 1);

        workspace.toggle_scrolling_full_width();
        assert_eq!(widths(&workspace.layout_scrolling(&AREA, 0))[1], 1200);

        workspace.toggle_scrolling_full_width();
        assert_eq!(widths(&workspace.layout_scrolling(&AREA, 0))[1], 400);
    }

    #[test]
    fn a_column_is_centred_once() {
        let mut workspace = laid_out(3, 2, 0);
        assert_eq!(workspace.layout_scrolling(&AREA, 0)[0].rect.left, AREA.left);

        workspace.center_scrolling_column();

        // the work area is 1200 wide and starts at 100, and the column is 400 wide
        assert_eq!(
            workspace.layout_scrolling(&AREA, 0)[0].rect.left,
            AREA.left + 400
        );

        // and it is centred once, so moving on to the next column does not keep it centred
        workspace.focus_container(1);
        let lefts: Vec<_> = workspace
            .layout_scrolling(&AREA, 0)
            .iter()
            .map(|placement| placement.rect.left)
            .collect();
        assert_eq!(lefts, [500, 900]);
    }

    #[test]
    fn the_commands_are_about_the_container_that_has_focus_now() {
        let mut workspace = laid_out(3, 3, 0);

        // focus moved since the layout, and the next layout has not been made yet
        workspace.focus_container(2);
        workspace.toggle_scrolling_full_width();

        assert_eq!(
            widths(&workspace.layout_scrolling(&AREA, 0)),
            [400, 400, 1200]
        );
    }

    #[test]
    fn the_commands_do_nothing_before_there_is_a_layout() {
        let mut workspace = workspace(3, 3);

        workspace.toggle_scrolling_full_width();
        workspace.center_scrolling_column();

        assert!(workspace.scrolling.is_none());
    }

    #[test]
    fn the_commands_do_nothing_in_a_layout_that_is_not_scrolling() {
        let mut workspace = laid_out(3, 3, 1);
        let before = workspace.scrolling.clone();
        workspace.layout = Layout::Default(DefaultLayout::BSP);

        workspace.toggle_scrolling_full_width();
        workspace.center_scrolling_column();

        assert_eq!(workspace.scrolling, before);
    }

    /// A workspace with a window in every container, which is how closing a window finds a container.
    fn workspace_of_windows(columns: usize, count: u32) -> Workspace {
        workspace_of_windows_from(columns, 0, count)
    }

    /// A workspace with a window in every container, whose ids start at `first`.
    fn workspace_of_windows_from(columns: usize, first: u32, count: u32) -> Workspace {
        let mut workspace = Workspace {
            layout: Layout::Default(DefaultLayout::Scrolling),
            layout_options: options(columns, None),
            ..Default::default()
        };

        for id in first..first + count {
            workspace
                .new_container_for_window(&Window::from(id))
                .unwrap();
        }

        workspace
    }

    fn lefts(placements: &[ScrollingPlacement]) -> Vec<i32> {
        placements
            .iter()
            .map(|placement| placement.rect.left)
            .collect()
    }

    fn sent_to(width: i32) -> Rect {
        Rect {
            left: 0,
            top: 0,
            right: width,
            bottom: 800,
        }
    }

    // The arrivals that these tests report are shared by every test that runs at the same time,
    // so each of them has windows with ids of its own.

    #[test]
    fn a_window_that_will_not_get_narrower_pushes_the_columns_after_it_along() {
        let mut workspace = workspace_of_windows_from(3, 6_000_000, 3);
        let before = workspace.layout_scrolling(&AREA, 0);
        assert_eq!(widths(&before), [400, 400, 400]);

        // the second window was sent 400 and is 700
        arrival::report(6_000_001, sent_to(400), sent_to(700));
        assert!(workspace.adopt_actual_sizes(0, 1200));

        let after = workspace.layout_scrolling(&AREA, 0);
        assert_eq!(widths(&after), [400, 700, 400]);

        // and the next column starts where that one ends, where it would have overlapped it
        assert_eq!(lefts(&after)[2], lefts(&after)[1] + 700);
    }

    #[test]
    fn the_room_around_a_window_is_taken_into_account() {
        let mut workspace = workspace_of_windows_from(3, 6_000_010, 2);
        workspace.layout_scrolling(&AREA, 0);

        // padding of 10, and a border of 2 and 0, on either side of a window of 700
        arrival::report(6_000_010, sent_to(376), sent_to(700));
        assert!(workspace.adopt_actual_sizes(12, 1200));

        assert_eq!(widths(&workspace.layout_scrolling(&AREA, 0))[0], 724);
    }

    #[test]
    fn what_a_window_will_not_do_is_only_ever_learned_upwards() {
        let mut workspace = workspace_of_windows_from(3, 6_000_020, 2);
        workspace.layout_scrolling(&AREA, 0);

        arrival::report(6_000_020, sent_to(400), sent_to(700));
        assert!(workspace.adopt_actual_sizes(0, 1200));

        // a window that is smaller than that now is not a reason to forget it
        arrival::report(6_000_020, sent_to(700), sent_to(750));
        assert!(workspace.adopt_actual_sizes(0, 1200));
        arrival::report(6_000_020, sent_to(750), sent_to(720));
        assert!(!workspace.adopt_actual_sizes(0, 1200));
        arrival::report(6_000_020, sent_to(750), sent_to(750));
        assert!(!workspace.adopt_actual_sizes(0, 1200));

        assert_eq!(widths(&workspace.layout_scrolling(&AREA, 0))[0], 750);
    }

    #[test]
    fn a_column_is_never_made_wider_than_the_screen() {
        let mut workspace = workspace_of_windows_from(3, 6_000_030, 2);
        workspace.layout_scrolling(&AREA, 0);

        arrival::report(6_000_030, sent_to(400), sent_to(1500));
        assert!(workspace.adopt_actual_sizes(12, 1200));

        assert_eq!(widths(&workspace.layout_scrolling(&AREA, 0))[0], 1200);
    }

    #[test]
    fn a_window_that_took_its_size_changes_nothing() {
        let mut workspace = workspace_of_windows_from(3, 6_000_040, 3);
        let before = workspace.layout_scrolling(&AREA, 0);

        // exactly, and within the rounding that apps do
        arrival::report(6_000_040, sent_to(400), sent_to(400));
        arrival::report(6_000_041, sent_to(400), sent_to(402));

        assert!(!workspace.adopt_actual_sizes(0, 1200));
        assert_eq!(workspace.layout_scrolling(&AREA, 0), before);
    }

    #[test]
    fn a_window_that_refused_and_then_took_its_size_is_forgotten() {
        let mut workspace = workspace_of_windows_from(3, 6_000_050, 2);
        workspace.layout_scrolling(&AREA, 0);

        arrival::report(6_000_050, sent_to(400), sent_to(700));
        arrival::report(6_000_050, sent_to(400), sent_to(400));

        assert!(!workspace.adopt_actual_sizes(0, 1200));
    }

    #[test]
    fn a_stack_is_as_wide_as_the_widest_of_its_windows_needs() {
        let mut workspace = workspace_of_windows_from(3, 6_000_060, 2);
        workspace
            .containers_mut()
            .front_mut()
            .unwrap()
            .add_window(&Window::from(6_000_062), WindowHidingPosition::default())
            .unwrap();
        workspace.layout_scrolling(&AREA, 0);

        arrival::report(6_000_060, sent_to(400), sent_to(500));
        arrival::report(6_000_062, sent_to(400), sent_to(650));

        assert!(workspace.adopt_actual_sizes(0, 1200));
        assert_eq!(widths(&workspace.layout_scrolling(&AREA, 0)), [650, 400]);
    }

    #[test]
    fn nothing_is_adopted_before_there_is_a_strip() {
        let mut workspace = workspace_of_windows_from(3, 6_000_070, 2);
        arrival::report(6_000_070, sent_to(400), sent_to(700));

        assert!(!workspace.adopt_actual_sizes(0, 1200));

        // and it is still there for when there is
        workspace.layout_scrolling(&AREA, 0);
        assert!(workspace.adopt_actual_sizes(0, 1200));
    }

    fn focused_window(workspace: &Workspace) -> u32 {
        workspace
            .focused_container()
            .and_then(|container| container.focused_window())
            .map(|window| window.id)
            .unwrap()
    }

    /// The id of the window in every container, in the order of the containers.
    fn window_ids(workspace: &Workspace) -> Vec<u32> {
        workspace
            .containers()
            .iter()
            .filter_map(|container| container.focused_window())
            .map(|window| window.id)
            .collect()
    }

    /// A workspace of four windows, 0 and 1 of which are in one column, with 0 in focus.
    fn workspace_with_a_column(first: u32) -> Workspace {
        let mut workspace = workspace_of_windows_from(3, first, 4);
        workspace.focus_container(0);
        workspace.layout_scrolling(&AREA, 0);
        workspace.consume_scrolling(OperationDirection::Right);
        workspace.layout_scrolling(&AREA, 0);

        workspace
    }

    #[test]
    fn consuming_puts_two_windows_in_one_column_and_leaves_the_order_of_the_containers() {
        let mut workspace = workspace_with_a_column(0);

        assert_eq!(window_ids(&workspace), [0, 1, 2, 3]);

        let placements = workspace.layout_scrolling(&AREA, 0);
        assert_eq!(placements[0].rect.left, placements[1].rect.left);
        assert_eq!(placements[0].rect.right, placements[1].rect.right);
        assert_eq!(
            placements[0].rect.bottom + placements[1].rect.bottom,
            AREA.bottom
        );
        assert_eq!(
            placements[1].rect.top,
            placements[0].rect.top + placements[0].rect.bottom
        );

        // the column that was next to the one that gave up its window moved up to it
        assert_eq!(placements[2].rect.left, placements[0].rect.left + 400);
        assert_eq!(focused_window(&workspace), 0);
    }

    #[test]
    fn expelling_puts_the_focused_window_in_a_column_of_its_own() {
        let mut workspace = workspace_with_a_column(0);
        workspace.focus_container(1);

        workspace.expel_scrolling();

        let placements = workspace.layout_scrolling(&AREA, 0);
        assert_eq!(widths(&placements), [400; 4]);
        assert!(
            placements
                .iter()
                .all(|placement| placement.rect.bottom == AREA.bottom)
        );
        assert_eq!(focused_window(&workspace), 1);
    }

    #[test]
    fn expelling_a_window_that_has_others_below_it_moves_its_container_in_the_list() {
        let mut workspace = workspace_of_windows_from(3, 10, 3);
        workspace.focus_container(0);
        workspace.layout_scrolling(&AREA, 0);
        workspace.consume_scrolling(OperationDirection::Right);
        workspace.layout_scrolling(&AREA, 0);
        workspace.consume_scrolling(OperationDirection::Right);
        assert_eq!(window_ids(&workspace), [10, 11, 12]);

        workspace.expel_scrolling();

        // 10 left the column, so it is after the windows that were below it
        assert_eq!(window_ids(&workspace), [11, 12, 10]);
        assert_eq!(focused_window(&workspace), 10);
        assert_eq!(workspace.focused_container_idx(), 2);
    }

    #[test]
    fn up_and_down_are_the_windows_of_a_column_and_left_and_right_are_the_columns() {
        let mut workspace = workspace_with_a_column(0);
        workspace.focus_container(1);

        assert_eq!(
            workspace.scrolling_idx_for_direction(OperationDirection::Up),
            Some(0)
        );
        assert_eq!(
            workspace.scrolling_idx_for_direction(OperationDirection::Down),
            None
        );
        assert_eq!(
            workspace.scrolling_idx_for_direction(OperationDirection::Right),
            Some(2)
        );
        assert_eq!(
            workspace.scrolling_idx_for_direction(OperationDirection::Left),
            None
        );

        // the strip hears about focus at the next layout, which is made after every change of it
        workspace.layout_scrolling(&AREA, 0);
        workspace.focus_container(2);
        // and left goes to the window that had focus in the column that it arrives at
        assert_eq!(
            workspace.scrolling_idx_for_direction(OperationDirection::Left),
            Some(1)
        );
    }

    #[test]
    fn asking_where_focus_would_go_changes_nothing() {
        let mut workspace = workspace_with_a_column(0);
        workspace.focus_container(1);
        let before = workspace.layout_scrolling(&AREA, 0);
        let strip = workspace.scrolling.clone();

        workspace.scrolling_idx_for_direction(OperationDirection::Right);
        workspace.scrolling_idx_for_direction(OperationDirection::Up);

        assert_eq!(workspace.scrolling, strip);
        assert_eq!(workspace.layout_scrolling(&AREA, 0), before);
    }

    #[test]
    fn moving_down_swaps_the_windows_of_a_column_and_focus_stays_with_the_window() {
        let mut workspace = workspace_with_a_column(20);
        assert_eq!(window_ids(&workspace), [20, 21, 22, 23]);

        assert!(workspace.move_scrolling(OperationDirection::Down));

        assert_eq!(window_ids(&workspace), [21, 20, 22, 23]);
        assert_eq!(focused_window(&workspace), 20);
        assert_eq!(workspace.focused_container_idx(), 1);

        assert!(
            !workspace.move_scrolling(OperationDirection::Down),
            "it is the last"
        );
    }

    #[test]
    fn moving_right_moves_the_whole_column() {
        let mut workspace = workspace_with_a_column(30);

        assert!(workspace.move_scrolling(OperationDirection::Right));

        assert_eq!(window_ids(&workspace), [32, 30, 31, 33]);
        assert_eq!(focused_window(&workspace), 30);

        assert!(workspace.move_scrolling(OperationDirection::Right));
        assert_eq!(window_ids(&workspace), [32, 33, 30, 31]);
        assert!(
            !workspace.move_scrolling(OperationDirection::Right),
            "it is the last column"
        );

        assert!(workspace.move_scrolling(OperationDirection::Left));
        assert_eq!(window_ids(&workspace), [32, 30, 31, 33]);
    }

    #[test]
    fn a_new_window_goes_after_the_whole_column_that_has_focus() {
        let mut workspace = workspace_with_a_column(40);
        // the top window of the column has focus, and there is another below it

        workspace
            .new_container_for_window(&Window::from(99))
            .unwrap();

        assert_eq!(window_ids(&workspace), [40, 41, 99, 42, 43]);

        // and it is a column of its own
        let placements = workspace.layout_scrolling(&AREA, 0);
        assert_eq!(placements[2].rect.bottom, AREA.bottom);
        assert_eq!(placements[2].rect.left, placements[0].rect.left + 400);
    }

    fn heights(placements: &[ScrollingPlacement]) -> Vec<i32> {
        placements
            .iter()
            .map(|placement| placement.rect.bottom)
            .collect()
    }

    #[test]
    fn resizing_a_vertical_edge_changes_the_height_of_the_window_by_half_of_the_delta() {
        let mut workspace = workspace_with_a_column(70);
        assert_eq!(
            heights(&workspace.layout_scrolling(&AREA, 0))[..2],
            [400, 400]
        );

        workspace.resize_scrolling_column(OperationDirection::Down, Sizing::Increase, 200);
        assert_eq!(
            heights(&workspace.layout_scrolling(&AREA, 0))[..2],
            [500, 300]
        );

        // either edge is the same, and a window can be made shorter
        workspace.resize_scrolling_column(OperationDirection::Up, Sizing::Decrease, 100);
        assert_eq!(
            heights(&workspace.layout_scrolling(&AREA, 0))[..2],
            [450, 350]
        );

        // and none of it touched the width
        assert_eq!(widths(&workspace.layout_scrolling(&AREA, 0)), [400; 4]);
    }

    #[test]
    fn a_window_is_not_made_shorter_than_the_least_that_is_configured() {
        let mut workspace = workspace_with_a_column(80);

        workspace.resize_scrolling_column(OperationDirection::Down, Sizing::Decrease, 10_000);

        assert_eq!(
            heights(&workspace.layout_scrolling(&AREA, 0))[..2],
            [MINIMUM_WINDOW_HEIGHT, 800 - MINIMUM_WINDOW_HEIGHT]
        );
    }

    #[test]
    fn retile_puts_heights_back_as_well_as_widths() {
        let mut workspace = workspace_with_a_column(110);
        workspace.resize_scrolling_column(OperationDirection::Down, Sizing::Increase, 200);
        workspace.resize_scrolling_column(OperationDirection::Right, Sizing::Increase, 200);
        let placements = workspace.layout_scrolling(&AREA, 0);
        assert_ne!(heights(&placements)[..2], [400, 400]);
        assert_ne!(widths(&placements), [400; 4]);

        workspace.reset_scrolling_sizes();

        let placements = workspace.layout_scrolling(&AREA, 0);
        assert_eq!(heights(&placements)[..2], [400, 400]);
        assert_eq!(widths(&placements), [400; 4]);
    }

    #[test]
    fn resizing_the_height_does_nothing_for_a_window_that_is_alone_in_its_column() {
        let mut workspace = laid_out(3, 3, 1);
        let before = workspace.layout_scrolling(&AREA, 0);

        workspace.resize_scrolling_column(OperationDirection::Down, Sizing::Increase, 200);
        workspace.resize_scrolling_column(OperationDirection::Up, Sizing::Decrease, 200);

        assert_eq!(workspace.layout_scrolling(&AREA, 0), before);
    }

    #[test]
    fn a_window_that_is_moved_to_another_workspace_is_as_wide_as_it_was() {
        let mut from = workspace_of_windows_from(3, 120, 3);
        from.focus_container(1);
        from.layout_scrolling(&AREA, 0);
        from.resize_scrolling_column(OperationDirection::Right, Sizing::Increase, 200);
        assert_eq!(widths(&from.layout_scrolling(&AREA, 0))[1], 500);

        let mut to = workspace_of_windows_from(3, 130, 2);
        to.layout_scrolling(&AREA, 0);

        // the way that the window manager does it: out of one, and into the other
        let idx = from.focused_container_idx();
        let container = from.remove_container_by_idx(idx).unwrap();
        to.add_container_to_back(container);

        assert_eq!(widths(&to.layout_scrolling(&AREA, 0)), [400, 400, 500]);
    }

    #[test]
    fn what_a_window_was_given_is_used_by_the_workspace_that_gets_it_and_by_no_other() {
        let mut from = workspace_of_windows_from(3, 140, 2);
        from.focus_container(0);
        from.layout_scrolling(&AREA, 0);
        from.resize_scrolling_column(OperationDirection::Right, Sizing::Increase, 200);

        let container = from.remove_container_by_idx(0).unwrap();
        let mut to = workspace_of_windows_from(3, 150, 1);
        to.add_container_to_back(container.clone());
        assert_eq!(widths(&to.layout_scrolling(&AREA, 0)), [400, 500]);

        // a workspace that is given the same container afterwards is not told what the first one
        // was, which it took
        let mut other = workspace_of_windows_from(3, 155, 1);
        other.add_container_to_back(container);
        assert_eq!(widths(&other.layout_scrolling(&AREA, 0)), [400, 400]);
    }

    #[test]
    fn a_window_that_is_moved_on_takes_what_it_has_where_it_is_now() {
        let mut from = workspace_of_windows_from(3, 180, 2);
        from.focus_container(0);
        from.layout_scrolling(&AREA, 0);
        from.resize_scrolling_column(OperationDirection::Right, Sizing::Increase, 200);

        let container = from.remove_container_by_idx(0).unwrap();
        let mut to = workspace_of_windows_from(3, 190, 1);
        to.add_container_to_back(container);
        to.layout_scrolling(&AREA, 0);
        to.resize_scrolling_column(OperationDirection::Right, Sizing::Increase, 200);
        assert_eq!(widths(&to.layout_scrolling(&AREA, 0)), [400, 600]);

        let container = to.remove_container_by_idx(1).unwrap();
        let mut third = workspace_of_windows_from(3, 195, 1);
        third.add_container_to_back(container);

        assert_eq!(widths(&third.layout_scrolling(&AREA, 0)), [400, 600]);
    }

    #[test]
    fn a_window_that_was_not_laid_out_where_it_was_has_nothing_to_take() {
        let mut from = workspace_of_windows_from(3, 160, 2);
        let mut to = workspace_of_windows_from(3, 170, 1);
        to.layout_scrolling(&AREA, 0);

        // there was never a strip to ask
        let container = from.remove_container_by_idx(0).unwrap();
        to.add_container_to_back(container);

        assert_eq!(widths(&to.layout_scrolling(&AREA, 0)), [400, 400]);
    }

    /// The ids of the windows in every container, in order.
    fn windows_of(workspace: &Workspace) -> Vec<Vec<u32>> {
        workspace
            .containers()
            .iter()
            .map(|container| container.windows().iter().map(|w| w.id).collect())
            .collect()
    }

    #[test]
    fn a_column_of_several_windows_becomes_one_stack_when_toggled() {
        let mut workspace = workspace_with_a_column(200);
        workspace.layout_scrolling(&AREA, 0);
        assert_eq!(focused_window(&workspace), 200);

        assert!(workspace.toggle_scrolling_column_stack().unwrap());

        assert_eq!(
            windows_of(&workspace),
            [vec![200, 201], vec![202], vec![203]]
        );
        // the window that had focus is the one that shows, and not the one that was added last
        assert_eq!(focused_window(&workspace), 200);

        let placements = workspace.layout_scrolling(&AREA, 0);
        assert_eq!(placements.len(), 3);
        assert_eq!(widths(&placements), [400; 3]);
        assert_eq!(placements[0].rect.bottom, AREA.bottom);
    }

    #[test]
    fn a_stack_becomes_a_column_of_its_windows_when_toggled() {
        let mut workspace = workspace_of_windows_from(3, 210, 2);
        workspace.focus_container(0);
        workspace
            .containers_mut()
            .front_mut()
            .unwrap()
            .add_window(&Window::from(212), WindowHidingPosition::default())
            .unwrap();
        workspace.layout_scrolling(&AREA, 0);
        assert_eq!(windows_of(&workspace), [vec![210, 212], vec![211]]);

        assert!(workspace.toggle_scrolling_column_stack().unwrap());

        // the tabs are in the order of the tabs, one above the other, and the window that was
        // showing is the one with focus
        assert_eq!(windows_of(&workspace), [vec![210], vec![212], vec![211]]);
        assert_eq!(focused_window(&workspace), 212);

        let placements = workspace.layout_scrolling(&AREA, 0);
        assert_eq!(placements[0].rect.left, placements[1].rect.left);
        assert_eq!(
            placements[0].rect.bottom + placements[1].rect.bottom,
            AREA.bottom
        );
        assert_eq!(placements[2].rect.left, placements[0].rect.left + 400);
        assert_eq!(placements[2].rect.bottom, AREA.bottom);
    }

    #[test]
    fn unstacking_pops_the_window_that_has_focus_into_a_column_of_its_own_on_the_right() {
        let mut workspace = workspace_with_a_column(250);
        workspace.layout_scrolling(&AREA, 0);
        workspace.toggle_scrolling_column_stack().unwrap();
        workspace.layout_scrolling(&AREA, 0);
        assert_eq!(windows_of(&workspace)[0], [250, 251]);

        // what unstack does, which is the same in every layout, finds its place in the strip
        workspace.new_container_for_focused_window().unwrap();

        assert_eq!(
            windows_of(&workspace),
            [vec![251], vec![250], vec![252], vec![253]]
        );
        assert_eq!(focused_window(&workspace), 250);

        // four columns of their own, the window that was popped off in the one after the stack
        let placements = workspace.layout_scrolling(&AREA, 0);
        assert_eq!(widths(&placements), [400; 4]);
        assert!(placements.iter().all(|p| p.rect.bottom == AREA.bottom));
        assert_eq!(placements[1].rect.left, placements[0].rect.left + 400);
    }

    #[test]
    fn toggling_twice_gives_the_column_back_with_its_width() {
        let mut workspace = workspace_with_a_column(220);
        workspace.resize_scrolling_column(OperationDirection::Right, Sizing::Increase, 200);
        let before = workspace.layout_scrolling(&AREA, 0);

        workspace.toggle_scrolling_column_stack().unwrap();
        workspace.layout_scrolling(&AREA, 0);
        workspace.toggle_scrolling_column_stack().unwrap();

        assert_eq!(
            windows_of(&workspace),
            [vec![220], vec![221], vec![222], vec![223]]
        );
        assert_eq!(workspace.layout_scrolling(&AREA, 0), before);
        assert_eq!(focused_window(&workspace), 220);
    }

    #[test]
    fn a_window_that_is_alone_in_its_tile_and_its_column_has_nothing_to_toggle() {
        let mut workspace = laid_out(3, 3, 1);
        let before = workspace.layout_scrolling(&AREA, 0);

        assert!(!workspace.toggle_scrolling_column_stack().unwrap());

        assert_eq!(workspace.layout_scrolling(&AREA, 0), before);
    }

    #[test]
    fn toggling_does_nothing_before_there_is_a_strip_or_in_another_layout() {
        let mut workspace = workspace_of_windows_from(3, 230, 2);
        assert!(!workspace.toggle_scrolling_column_stack().unwrap());

        let mut workspace = workspace_with_a_column(240);
        workspace.layout = Layout::Default(DefaultLayout::BSP);
        assert!(!workspace.toggle_scrolling_column_stack().unwrap());
        assert_eq!(windows_of(&workspace)[0], [240]);
    }

    #[test]
    fn consuming_to_the_left_is_the_other_way_round() {
        let mut workspace = workspace_of_windows_from(3, 50, 3);
        workspace.focus_container(1);
        workspace.layout_scrolling(&AREA, 0);

        workspace.consume_scrolling(OperationDirection::Left);

        let placements = workspace.layout_scrolling(&AREA, 0);
        assert_eq!(placements[0].rect.left, placements[1].rect.left);
        assert_eq!(window_ids(&workspace), [50, 51, 52]);
        assert_eq!(
            focused_window(&workspace),
            51,
            "focus stays with the window"
        );

        // up and down mean nothing to it
        workspace.consume_scrolling(OperationDirection::Up);
        assert_eq!(workspace.layout_scrolling(&AREA, 0), placements);
    }

    #[test]
    fn nothing_here_does_anything_before_there_is_a_strip() {
        let mut workspace = workspace_of_windows_from(3, 60, 3);

        workspace.consume_scrolling(OperationDirection::Right);
        workspace.expel_scrolling();
        assert!(!workspace.move_scrolling(OperationDirection::Down));

        assert_eq!(window_ids(&workspace), [60, 61, 62]);
        assert!(workspace.scrolling.is_none());
    }

    #[test]
    fn closing_a_window_that_was_just_opened_goes_back_to_the_one_before_it() {
        let mut workspace = workspace_of_windows(3, 8);
        workspace.focus_container(3);
        workspace.layout_scrolling(&AREA, 0);
        // window 5 is the last of the three that are in view, and 6 and 7 are beyond it
        workspace.focus_container(5);
        let before = workspace.layout_scrolling(&AREA, 0);

        workspace
            .new_container_for_window(&Window::from(100))
            .unwrap();
        let opened = workspace.layout_scrolling(&AREA, 0);
        assert_ne!(opened, before, "opening it scrolled the strip");

        workspace.remove_window(100).unwrap();

        assert_eq!(focused_window(&workspace), 5);
        assert_eq!(workspace.layout_scrolling(&AREA, 0), before);
    }

    #[test]
    fn once_focus_has_moved_closing_a_window_goes_to_the_one_that_takes_its_place() {
        let mut workspace = workspace_of_windows(3, 8);
        workspace.focus_container(3);
        workspace.layout_scrolling(&AREA, 0);
        workspace.focus_container(5);
        workspace.layout_scrolling(&AREA, 0);

        workspace
            .new_container_for_window(&Window::from(100))
            .unwrap();
        workspace.layout_scrolling(&AREA, 0);
        workspace.focus_container(5);
        workspace.layout_scrolling(&AREA, 0);
        workspace.focus_container(6);
        workspace.layout_scrolling(&AREA, 0);

        workspace.remove_window(100).unwrap();

        // window 6 was to the right of the closed window, and the other layouts would have gone left
        assert_eq!(focused_window(&workspace), 6);
    }

    #[test]
    fn closing_the_last_window_goes_to_the_one_before_it() {
        let mut workspace = workspace_of_windows(3, 5);
        workspace.layout_scrolling(&AREA, 0);
        assert_eq!(focused_window(&workspace), 4);

        workspace.remove_window(4).unwrap();

        assert_eq!(focused_window(&workspace), 3);
    }

    #[test]
    fn closing_a_window_that_does_not_have_focus_leaves_focus_where_it_is() {
        let mut workspace = workspace_of_windows(3, 6);
        workspace.focus_container(2);
        workspace.layout_scrolling(&AREA, 0);

        // to the right of the focused one, and then to the left of it
        workspace.remove_window(5).unwrap();
        assert_eq!(focused_window(&workspace), 2);

        workspace.remove_window(0).unwrap();
        assert_eq!(focused_window(&workspace), 2);
        assert_eq!(workspace.focused_container_idx(), 1);
    }

    #[test]
    fn before_there_is_a_strip_closing_a_window_moves_focus_left_like_the_other_layouts() {
        let mut workspace = workspace_of_windows(3, 4);
        workspace.focus_container(2);
        assert!(workspace.scrolling.is_none());

        workspace.remove_window(2).unwrap();

        assert_eq!(focused_window(&workspace), 1);
    }

    #[test]
    fn changing_the_columns_option_changes_the_width_of_columns_that_have_not_been_resized() {
        let mut workspace = workspace(3, 3);
        assert_eq!(
            widths(&workspace.layout_scrolling(&AREA, 0)),
            [400, 400, 400]
        );

        workspace.set_scrolling_columns(4);

        assert_eq!(
            widths(&workspace.layout_scrolling(&AREA, 0)),
            [300, 300, 300]
        );
    }

    /// The windows that were put where they are shown since this was last asked.
    fn take_shown() -> Vec<u32> {
        SHOWN.with(|shown| std::mem::take(&mut *shown.borrow_mut()))
    }

    #[test]
    fn every_window_of_a_stack_that_is_spread_out_is_shown() {
        // one of the two was a tab at the back, which stacking hid, and whichever it was, it has
        // to be put where it is shown, the one that stays in its container as much as the one
        // that goes to a container of its own
        for in_front in [0, 1] {
            let mut workspace = workspace_of_windows_from(3, 300, 3);
            workspace.layout_scrolling(&AREA, 0);
            workspace.focus_container(1);
            workspace.consume_scrolling(OperationDirection::Right);
            workspace.layout_scrolling(&AREA, 0);

            assert!(workspace.toggle_scrolling_column_stack().unwrap());

            let idx = workspace.focused_container_idx();
            workspace
                .containers_mut()
                .get_mut(idx)
                .unwrap()
                .focus_window(in_front);

            take_shown();
            assert!(workspace.toggle_scrolling_column_stack().unwrap());

            let mut shown = take_shown();
            shown.sort_unstable();
            assert_eq!(shown, [301, 302], "with window {in_front} in front");
        }
    }

    #[test]
    fn stacking_a_column_and_spreading_it_out_again_leaves_everything_where_it_was() {
        // three columns fit across, and the strip is exactly as wide as the screen, as in a
        // column of two windows that is the last of three
        let mut workspace = workspace_of_windows(3, 4);
        workspace.layout_scrolling(&AREA, 0);
        workspace.focus_container(0);
        workspace.layout_scrolling(&AREA, 0);
        workspace.focus_container(2);
        workspace.layout_scrolling(&AREA, 0);
        workspace.consume_scrolling(OperationDirection::Right);

        let before = lefts(&workspace.layout_scrolling(&AREA, 0));
        let focused = workspace.focused_container_idx();

        assert!(workspace.toggle_scrolling_column_stack().unwrap());
        assert_eq!(workspace.containers().len(), 3);
        workspace.layout_scrolling(&AREA, 0);

        assert!(workspace.toggle_scrolling_column_stack().unwrap());
        assert_eq!(workspace.containers().len(), 4);

        assert_eq!(lefts(&workspace.layout_scrolling(&AREA, 0)), before);
        assert_eq!(workspace.focused_container_idx(), focused);
    }

    #[test]
    fn expanding_fills_the_room_that_the_columns_in_view_leave() {
        let mut workspace = workspace(3, 2);
        workspace.layout_scrolling(&AREA, 0);
        workspace.focus_container(1);
        let before = workspace.layout_scrolling(&AREA, 0);

        workspace.expand_scrolling_column();
        let after = workspace.layout_scrolling(&AREA, 0);

        // the focused column takes the third of the screen that was empty, and no more
        assert_eq!(after[1].rect.right, before[1].rect.right + (AREA.right / 3));
        assert_eq!(after[0], before[0]);
    }

    #[test]
    fn expanding_does_nothing_where_there_are_no_columns() {
        let mut workspace = workspace(3, 2);
        workspace.layout = Layout::Default(DefaultLayout::BSP);

        workspace.expand_scrolling_column();

        assert!(workspace.scrolling.is_none());
    }

    #[test]
    fn panning_moves_the_strip_and_leaves_focus_where_it_is_until_it_is_let_go() {
        let mut workspace = workspace(3, 6);
        workspace.layout_scrolling(&AREA, 0);
        workspace.focus_container(0);
        let before = workspace.layout_scrolling(&AREA, 0);

        assert!(workspace.begin_scrolling_pan().is_some());
        workspace.pan_scrolling(-300.0);
        let panned = workspace.layout_scrolling(&AREA, 0);

        assert_eq!(
            panned[1].rect.left,
            before[1].rect.left - 300,
            "the strip moved"
        );
        assert_eq!(workspace.focused_container_idx(), 0, "and focus did not");
    }

    #[test]
    fn a_strip_that_is_let_go_comes_to_rest_and_focus_goes_to_a_column_in_view() {
        let mut workspace = workspace(3, 6);
        workspace.layout_scrolling(&AREA, 0);
        workspace.focus_container(0);

        assert!(workspace.begin_scrolling_pan().is_some());
        // three columns to the left of the screen, which is where the first three are
        workspace.pan_scrolling(-1200.0);
        assert!(workspace.end_scrolling_pan(0.0), "focus moved");

        let rested = workspace.layout_scrolling(&AREA, 0);
        let focused = workspace.focused_container_idx();

        assert!(focused >= 3, "to a column that is in view, not {focused}");
        assert_eq!(rested[focused].visibility, Visibility::Visible);
    }

    #[test]
    fn there_is_no_strip_to_pan_in_another_layout_or_before_there_is_one() {
        let mut workspace = workspace(3, 6);
        assert!(workspace.begin_scrolling_pan().is_none(), "no layout yet");
        assert!(!workspace.end_scrolling_pan(0.0));

        workspace.layout_scrolling(&AREA, 0);
        workspace.layout = Layout::Default(DefaultLayout::BSP);
        assert!(workspace.begin_scrolling_pan().is_none());
    }

    #[test]
    fn a_step_of_a_pan_is_a_column_of_the_width_that_a_new_one_has() {
        let mut workspace = workspace(3, 6);
        assert_eq!(workspace.scrolling_pan_step(), None, "no layout yet");

        workspace.layout_scrolling(&AREA, 0);
        assert_eq!(workspace.scrolling_pan_step(), Some(400));

        workspace.set_scrolling_columns(4);
        workspace.layout_scrolling(&AREA, 0);
        assert_eq!(workspace.scrolling_pan_step(), Some(300));
    }

    #[test]
    fn a_pan_that_is_given_up_puts_the_strip_back() {
        let mut workspace = workspace(3, 6);
        workspace.layout_scrolling(&AREA, 0);
        workspace.focus_container(1);
        let before = workspace.layout_scrolling(&AREA, 0);

        let saved = workspace.begin_scrolling_pan().expect("a strip to pan");
        workspace.pan_scrolling(-500.0);
        workspace.cancel_scrolling_pan(saved);

        assert_eq!(workspace.layout_scrolling(&AREA, 0), before);
        assert_eq!(workspace.focused_container_idx(), 1);
    }

    #[test]
    fn the_strip_is_moved_across_a_screen_as_wide_as_the_work_area() {
        let mut workspace = workspace(3, 6);
        assert_eq!(workspace.scrolling_view_width(), None, "no layout yet");

        workspace.layout_scrolling(&AREA, 0);
        assert_eq!(workspace.scrolling_view_width(), Some(1200));
    }
}
