//! The glue between a [`Workspace`] and the strip of columns of the Scrolling layout.
//!
//! Everything that does not touch Windows lives in [`komorebi_layouts::scrolling`] and the
//! `komorebi-scrolling` crate, so that every komorebi shares it. What is here is what needs a
//! [`Workspace`]: a container is a tile in the strip, and is known to it by its id.

use crate::WINDOW_HANDLING_BEHAVIOUR;
use crate::animation::ANIMATION_ENABLED_GLOBAL;
use crate::animation::ANIMATION_ENABLED_PER_ANIMATION;
use crate::animation::ANIMATION_MANAGER;
use crate::animation::prefix::AnimationPrefix;
use crate::container::Container;
use crate::core::DefaultLayout;
use crate::core::Layout;
use crate::core::LayoutOptions;
use crate::core::OperationDirection;
use crate::core::Rect;
use crate::core::ScrollingLayoutOptions;
use crate::core::Sizing;
use crate::core::WindowHandlingBehaviour;
use crate::windows_api::WindowsApi;
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

/// How much wider than its column a window can be before it is taken to refuse to be narrower.
const WIDTH_TOLERANCE: i32 = 16;

/// Whether what windows say about their size can be trusted to be what they will stay at: their
/// moves are done by the time they are asked, and none is being animated.
fn window_sizes_are_settled() -> bool {
    matches!(
        WINDOW_HANDLING_BEHAVIOUR.load(),
        WindowHandlingBehaviour::Sync
    ) && ANIMATION_MANAGER.lock().count() == 0
}

/// How much of a window that has scrolled out of view is left on the screen, so that it has an
/// edge to slide back in from.
const PARKED_VISIBLE_PIXELS: i32 = 5;

/// Where a window with this rectangle rests when the strip says how much of it is in view.
///
/// A window that scrolled out of view comes to rest at the edge of the screen that it left by,
/// with a few pixels of it still on screen, and keeps its size, so that nothing has to be resized
/// to bring it back and an animation back into view is short. One that is partly or wholly in
/// view is where the strip puts it.
pub(crate) fn parked(rect: Rect, visibility: Visibility, work_area: &Rect) -> Rect {
    match visibility {
        Visibility::OffLeft => Rect {
            left: work_area.left - rect.right + PARKED_VISIBLE_PIXELS,
            ..rect
        },
        Visibility::OffRight => Rect {
            left: work_area.left + work_area.right - PARKED_VISIBLE_PIXELS,
            ..rect
        },
        _ => rect,
    }
}

/// Whether two rectangles have any area in common.
fn overlaps(a: &Rect, b: &Rect) -> bool {
    a.left < b.left + b.right
        && b.left < a.left + a.right
        && a.top < b.top + b.bottom
        && b.top < a.top + a.bottom
}

/// The monitors other than the one that has the work area, which is the one that contains it.
pub(crate) fn other_monitors(work_area: &Rect, monitors: &[Rect]) -> Vec<Rect> {
    monitors
        .iter()
        .filter(|monitor| {
            !(monitor.left <= work_area.left
                && monitor.top <= work_area.top
                && monitor.left + monitor.right >= work_area.left + work_area.right
                && monitor.top + monitor.bottom >= work_area.top + work_area.bottom)
        })
        .copied()
        .collect()
}

/// Cut a rectangle down to what is inside the work area.
pub(crate) fn clamp_to_work_area(rect: &mut Rect, work_area: &Rect) {
    if rect.left < work_area.left {
        rect.right -= work_area.left - rect.left;
        rect.left = work_area.left;
    }

    let right_edge = work_area.left + work_area.right;
    if rect.left + rect.right > right_edge {
        rect.right = right_edge - rect.left;
    }

    if rect.top < work_area.top {
        rect.bottom -= work_area.top - rect.top;
        rect.top = work_area.top;
    }

    let bottom_edge = work_area.top + work_area.bottom;
    if rect.top + rect.bottom > bottom_edge {
        rect.bottom = bottom_edge - rect.top;
    }
}

/// Where a window rests, and whether it has to be hidden to rest there, when `others` are the
/// other monitors.
///
/// A window is seen on whichever monitor it overlaps. One that rests or overhangs past an edge that
/// another monitor is next to would be seen there, so a window that scrolled out of view is hidden
/// instead of parked, and a window that is partly in view is made narrower to fit the work area.
/// Without a monitor next to it, the window is where [`parked`] puts it.
pub(crate) fn rest(
    rect: Rect,
    visibility: Visibility,
    work_area: &Rect,
    others: &[Rect],
) -> (Rect, bool) {
    let at = parked(rect, visibility, work_area);
    let seen_elsewhere = others.iter().any(|monitor| overlaps(&at, monitor));

    match visibility {
        Visibility::Visible => (at, false),
        Visibility::Clipped if seen_elsewhere => {
            let mut clamped = at;
            clamp_to_work_area(&mut clamped, work_area);
            (clamped, false)
        }
        Visibility::OffLeft | Visibility::OffRight if seen_elsewhere => (at, true),
        _ => (at, false),
    }
}

/// Whether a window that is about to be put where the strip says should be shown first, because it
/// was hidden when it went out of view past an edge with another monitor next to it.
///
/// Only the window that is showing in its container is: the others are the tabs at the back of a
/// stack, which are hidden because they are at the back, and showing them would put them in front
/// of the one that is in front, and the system would take the one that was shown for the one that
/// was chosen.
pub(crate) fn should_restore_when_placed(
    window: isize,
    focused_in_container: Option<isize>,
    hidden: &[isize],
) -> bool {
    focused_in_container == Some(window) && hidden.contains(&window)
}

/// Whether windows that are moved are animated to where they go.
pub(crate) fn animations_enabled() -> bool {
    ANIMATION_ENABLED_GLOBAL.load(Ordering::SeqCst)
        || ANIMATION_ENABLED_PER_ANIMATION
            .lock()
            .get(&AnimationPrefix::Movement)
            .is_some_and(|enabled| *enabled)
}

impl Workspace {
    /// Learn from what windows turned out to be when they were sent to where the layout put them,
    /// which is that some of them are wider than the layout said and will not be any narrower.
    ///
    /// A column is never made narrower than the window in it needs, and everything after it is
    /// placed where it ends. Only ever wider: what an app will not do is a fact about the app, and
    /// a limit that came and went would make the strip bounce between two layouts.
    ///
    /// `around_window` is what a column takes up around its window on either side, and the strip
    /// is `viewport_width` wide. Returns whether any column changed.
    pub(crate) fn adopt_actual_widths(&mut self, around_window: i32, viewport_width: i32) -> bool {
        // a window that is still on its way, or whose move was only asked for, is not what it
        // is going to be, and taking its size for a minimum would make the column too wide for good
        if !window_sizes_are_settled() {
            return false;
        }

        let Some(space) = self.scrolling.as_mut() else {
            return false;
        };

        let mut adopted = false;

        for (idx, container) in self.containers.elements().iter().enumerate() {
            let Some(placed) = self.latest_layout.get(idx) else {
                continue;
            };

            // a stack is one tile and as wide as the widest of its windows needs
            let excess = container
                .windows()
                .iter()
                // a window that is hidden has not been where it was told to be
                .filter(|window| !crate::HIDDEN_HWNDS.lock().contains(&window.hwnd))
                .filter_map(|window| WindowsApi::window_rect(window.hwnd).ok())
                .map(|actual| actual.right - placed.right)
                .max()
                .unwrap_or_default();

            // a window that is not wider than it was sent to be, give or take what rounding, the
            // borders of the window and a terminal's grid of cells do to it, has nothing to say
            if excess <= WIDTH_TOLERANCE {
                continue;
            }

            let Some(width) = space.column_width(&container.id) else {
                continue;
            };
            let Some((min, max)) = space.width_limits(&container.id) else {
                continue;
            };

            let needed = ((width.round() as i32) + excess)
                .min(viewport_width)
                .max(around_window);

            if min.is_some_and(|min| min >= needed) {
                continue;
            }

            tracing::info!(
                "a window in column {} is {excess}px wider than the {}px that it was placed at, so the column will not be narrower than {needed}px",
                container.id,
                placed.right
            );

            space.set_width_limits(&container.id, Some(needed), max);
            adopted = true;
        }

        adopted
    }

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

        // the window that stays was one of the tabs, and may be the one that was out of sight, so
        // it is put where it is shown as well, or its column is left empty until focus reaches it
        container.restore();

        // the strip is about to be told that there are more containers than it has columns for,
        // and so each of them is a column, until they are put in this one
        let mut count = 0;
        for (offset, window) in rest.into_iter().enumerate() {
            let mut container = Container::default();
            container.add_window(window);

            // it was one of the tabs that were out of sight, and has to be put where it is shown
            container.restore();

            self.insert_container_at_idx(idx + 1 + offset, container);
            count += 1;
        }

        // inserting a container gives it focus, and the strip scrolls to bring the focused column
        // into view, which it would do for a column that is only there until it is put in this
        // one, and then not scroll back. Focus stays on the column that they are put in.
        self.focus_container(idx);
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
    /// back to an equal share of the height of its column. The narrowest that windows were found
    /// to accept is forgotten as well.
    ///
    /// The focused column stays where it is on screen unless it has to scroll to stay in view.
    pub(crate) fn reset_scrolling_sizes(&mut self) {
        if let Some(space) = self.scrolling.as_mut() {
            space.reset_widths();
            space.reset_heights();

            // what windows were found to refuse is forgotten too, to be found out again if it is
            // still so: a limit that was learned wrongly would otherwise survive the one command
            // that is there to put everything back
            for id in space.ids() {
                space.set_width_limits(&id, None, None);
            }
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::HIDDEN_HWNDS;
    use crate::window::Window;

    const AREA: Rect = Rect {
        left: 100,
        top: 50,
        right: 1200,
        bottom: 800,
    };

    fn workspace(columns: usize, count: isize) -> Workspace {
        workspace_from(columns, 1, count)
    }

    /// A workspace of containers of one window each, with window handles from `first` on.
    fn workspace_from(columns: usize, first: isize, count: isize) -> Workspace {
        let mut workspace = Workspace {
            layout: Layout::Default(DefaultLayout::Scrolling),
            ..Workspace::default()
        };
        workspace.set_scrolling_columns(columns);

        for hwnd in first..first + count {
            let mut container = Container::default();
            container.add_window(Window::from(hwnd));
            let idx = workspace.containers().len();
            workspace.insert_container_at_idx(idx, container);
        }

        workspace
    }

    fn hwnds(workspace: &Workspace) -> Vec<isize> {
        workspace
            .containers()
            .iter()
            .filter_map(|container| container.focused_window().map(|window| window.hwnd))
            .collect()
    }

    #[test]
    fn opening_a_window_does_not_resize_the_others() {
        let mut workspace = workspace(3, 1);
        let before = workspace.layout_scrolling(&AREA, 0)[0].rect.right;

        for hwnd in 2..=5 {
            let mut container = Container::default();
            container.add_window(Window::from(hwnd));
            let idx = workspace
                .scrolling_insertion_idx()
                .unwrap_or_else(|| workspace.containers().len());
            workspace.insert_container_at_idx(idx, container);
        }

        let placements = workspace.layout_scrolling(&AREA, 0);

        assert_eq!(placements.len(), 5);
        assert!(placements.iter().all(|p| p.rect.right == before));
        assert_eq!(before, AREA.right / 3);
    }

    #[test]
    fn consuming_puts_two_windows_in_one_column_and_expelling_takes_one_out() {
        let mut workspace = workspace(3, 3);
        workspace.layout_scrolling(&AREA, 0);
        workspace.focus_container(0);

        workspace.consume_scrolling(OperationDirection::Right);
        let placements = workspace.layout_scrolling(&AREA, 0);

        // the first two windows are one above the other, in the same column
        assert_eq!(placements[0].rect.left, placements[1].rect.left);
        assert!(placements[1].rect.top > placements[0].rect.top);
        assert_ne!(placements[1].rect.left, placements[2].rect.left);
        assert_eq!(hwnds(&workspace), vec![1, 2, 3]);

        workspace.expel_scrolling();
        let placements = workspace.layout_scrolling(&AREA, 0);
        assert_ne!(placements[0].rect.left, placements[1].rect.left);
    }

    #[test]
    fn a_column_moves_whole_and_the_containers_follow_its_order() {
        let mut workspace = workspace(3, 3);
        workspace.layout_scrolling(&AREA, 0);
        workspace.focus_container(0);

        assert!(workspace.move_scrolling(OperationDirection::Right));
        assert_eq!(hwnds(&workspace), vec![2, 1, 3]);
        assert_eq!(workspace.focused_container_idx(), 1);
    }

    #[test]
    fn the_commands_do_nothing_in_a_layout_that_is_not_scrolling() {
        let mut workspace = workspace(3, 3);
        workspace.layout_scrolling(&AREA, 0);
        workspace.layout = Layout::Default(DefaultLayout::BSP);

        assert!(!workspace.move_scrolling(OperationDirection::Right));
        workspace.expel_scrolling();
        assert_eq!(hwnds(&workspace), vec![1, 2, 3]);
    }

    #[test]
    fn a_window_out_of_view_rests_at_the_edge_that_it_left_by() {
        let rect = Rect {
            left: -500,
            top: 50,
            right: 400,
            bottom: 800,
        };

        let left = parked(rect, Visibility::OffLeft, &AREA);
        assert_eq!(left.left + left.right, AREA.left + PARKED_VISIBLE_PIXELS);
        assert_eq!(left.right, 400);

        let right = parked(rect, Visibility::OffRight, &AREA);
        assert_eq!(right.left, AREA.left + AREA.right - PARKED_VISIBLE_PIXELS);

        assert_eq!(parked(rect, Visibility::Visible, &AREA), rect);
    }

    #[test]
    fn retiling_forgets_what_windows_were_found_to_refuse() {
        let mut workspace = workspace(3, 3);
        workspace.layout_scrolling(&AREA, 0);

        let id = workspace.containers().iter().next().unwrap().id.clone();
        let space = workspace.scrolling.as_mut().unwrap();
        space.set_width_limits(&id, Some(900), None);
        assert_eq!(workspace.layout_scrolling(&AREA, 0)[0].rect.right, 900);

        workspace.reset_scrolling_sizes();

        assert_eq!(
            workspace.layout_scrolling(&AREA, 0)[0].rect.right,
            AREA.right / 3
        );
    }

    fn monitor(left: i32) -> Rect {
        Rect {
            left,
            top: 0,
            right: 1400,
            bottom: 900,
        }
    }

    #[test]
    fn the_monitor_with_the_work_area_is_not_one_of_the_others() {
        let monitors = [monitor(-1400), monitor(0), monitor(1400)];
        let others = other_monitors(&AREA, &monitors);

        assert_eq!(others, vec![monitor(-1400), monitor(1400)]);
    }

    #[test]
    fn a_window_that_would_be_seen_on_a_neighbouring_monitor_is_hidden_or_made_narrower() {
        let work_area = Rect {
            left: 0,
            top: 0,
            right: 1400,
            bottom: 900,
        };
        let others = [monitor(1400)];
        let wide = Rect {
            left: -1000,
            top: 0,
            right: 400,
            bottom: 900,
        };

        // out to the left, with nothing there, it rests at the edge
        let (rect, hidden) = rest(wide, Visibility::OffLeft, &work_area, &others);
        assert!(!hidden);
        assert_eq!(rect.left + rect.right, PARKED_VISIBLE_PIXELS);

        // out to the right, where there is a monitor, it is hidden
        let off_right = Rect { left: 2000, ..wide };
        let (_, hidden) = rest(off_right, Visibility::OffRight, &work_area, &others);
        assert!(hidden);

        // partly in view at the right edge it is cut down to the work area
        let clipped = Rect { left: 1200, ..wide };
        let (rect, hidden) = rest(clipped, Visibility::Clipped, &work_area, &others);
        assert!(!hidden);
        assert_eq!(rect.left + rect.right, 1400);

        // and without a monitor there it is left as it is
        let (rect, _) = rest(clipped, Visibility::Clipped, &work_area, &[]);
        assert_eq!(rect, clipped);
    }

    #[test]
    fn only_the_window_at_the_front_of_a_stack_is_shown_when_it_is_placed() {
        let hidden = [10, 11];

        // the one at the front, if it was hidden, is shown
        assert!(should_restore_when_placed(10, Some(10), &hidden));
        // the one at the back stays hidden, whatever is hidden
        assert!(!should_restore_when_placed(11, Some(10), &hidden));
        // one that is not hidden has nothing to be shown from
        assert!(!should_restore_when_placed(12, Some(12), &hidden));
        // and a container with no window in front shows none
        assert!(!should_restore_when_placed(10, None, &hidden));
    }

    fn lefts(placements: &[ScrollingPlacement]) -> Vec<i32> {
        placements.iter().map(|p| p.rect.left).collect()
    }

    #[test]
    fn stacking_a_column_and_spreading_it_out_again_leaves_everything_where_it_was() {
        // three columns fit across, and the strip is exactly as wide as the screen, as in a
        // column of two windows that is the last of three
        let mut workspace = workspace(3, 4);
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
    fn every_window_of_a_stack_that_is_spread_out_is_shown() {
        // handles of their own, since the list of what is hidden is shared with every test
        let mut workspace = workspace_from(3, 9001, 3);
        workspace.layout_scrolling(&AREA, 0);
        workspace.focus_container(1);
        workspace.consume_scrolling(OperationDirection::Right);
        workspace.layout_scrolling(&AREA, 0);

        assert!(workspace.toggle_scrolling_column_stack().unwrap());

        // the tab at the back is the one that is hidden, whichever of the two it is
        for (focused, hidden) in [(1, 9002), (0, 9003)] {
            let idx = workspace.focused_container_idx();
            let container = workspace.containers_mut().get_mut(idx).unwrap();
            container.focus_window(focused);
            container.load_focused_window();
            assert!(HIDDEN_HWNDS.lock().contains(&hidden));

            assert!(workspace.toggle_scrolling_column_stack().unwrap());
            assert_eq!(workspace.containers().len(), 3);
            assert!(
                !HIDDEN_HWNDS.lock().contains(&hidden),
                "window {hidden} was left hidden by spreading the stack out"
            );

            assert!(workspace.toggle_scrolling_column_stack().unwrap());
        }
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
        let width = |placements: &[ScrollingPlacement], idx: usize| {
            placements[idx].rect.right - placements[idx].rect.left
        };
        assert!(width(&after, 1) > width(&before, 1) + AREA.right / 4);
        assert_eq!(after[1].rect.right, before[1].rect.right + (AREA.right / 3));
        assert_eq!(after[0], before[0]);
    }

    #[test]
    fn expanding_does_nothing_where_there_are_no_columns() {
        let mut workspace = Workspace {
            layout: Layout::Default(DefaultLayout::BSP),
            ..Workspace::default()
        };
        let mut container = Container::default();
        container.add_window(Window::from(9101));
        workspace.insert_container_at_idx(0, container);

        workspace.expand_scrolling_column();

        assert!(workspace.scrolling.is_none());
    }
}
