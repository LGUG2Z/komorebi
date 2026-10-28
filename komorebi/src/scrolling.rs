//! The glue between a [`Workspace`] and the platform-free model of a strip of columns in
//! `komorebi-scrolling`.
//!
//! A container is a tile in the strip, and is known to it by the id of the container.

use crate::core::DefaultLayout;
use crate::core::Layout;
use crate::core::LayoutOptions;
use crate::core::Rect;
use crate::workspace::Workspace;
use komorebi_scrolling::CenterFocusedColumn;
use komorebi_scrolling::ColumnWidth;
use komorebi_scrolling::Options;
use komorebi_scrolling::ScrollingSpace;
use komorebi_scrolling::Viewport;
use komorebi_scrolling::Visibility;

/// How many columns fit in the viewport when nothing says otherwise.
const DEFAULT_COLUMNS: usize = 3;

/// Where a container goes, and how much of it is in view.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct ScrollingPlacement {
    /// Where the container is in the strip, in the coordinates of the screen. A container that is
    /// partly or wholly out of view has the rectangle that it really has, which reaches past the
    /// work area.
    pub rect: Rect,
    #[allow(dead_code)]
    pub visibility: Visibility,
}

/// What the layout options of a workspace come to for the model.
///
/// `columns` says how many columns fit in the viewport, which is to say that a new column is a
/// `columns`th of its width. It does not say how many columns there are, and it does not stretch
/// a lone column to fill the viewport.
pub(crate) fn scrolling_options(layout_options: Option<LayoutOptions>) -> Options {
    let scrolling = layout_options.and_then(|options| options.scrolling);
    let columns = scrolling
        .map_or(DEFAULT_COLUMNS, |scrolling| scrolling.columns)
        .max(1);
    let centering = match scrolling.and_then(|scrolling| scrolling.center_focused_column) {
        Some(true) => CenterFocusedColumn::Always,
        _ => CenterFocusedColumn::Never,
    };

    Options {
        default_width: ColumnWidth::Proportion(1.0 / columns as f64),
        center_focused_column: centering,
        ..Options::default()
    }
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

        let space = self
            .scrolling
            .get_or_insert_with(|| ScrollingSpace::new(options.clone(), viewport));

        if *space.options() != options {
            space.set_options(options);
        }

        if space.viewport() != viewport {
            space.set_viewport(viewport);
        }

        space.reconcile(&ids, focused.as_ref());
    }

    /// Where every container goes in `work_area`, from left to right, with `container_padding`
    /// taken off every side of each.
    pub(crate) fn layout_scrolling(
        &mut self,
        work_area: &Rect,
        container_padding: i32,
    ) -> Vec<ScrollingPlacement> {
        self.sync_scrolling(Viewport {
            width: work_area.right,
            height: work_area.bottom,
        });

        let Some(space) = &self.scrolling else {
            return vec![];
        };

        space
            .layout()
            .into_iter()
            .map(|placement| {
                let mut rect = Rect {
                    left: work_area.left + placement.rect.x,
                    top: work_area.top + placement.rect.y,
                    right: placement.rect.width,
                    bottom: placement.rect.height,
                };
                rect.add_padding(container_padding);

                ScrollingPlacement {
                    rect,
                    visibility: placement.visibility,
                }
            })
            .collect()
    }
}
