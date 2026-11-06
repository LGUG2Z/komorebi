//! The platform-free half of komorebi's Scrolling layout, shared by every komorebi.
//!
//! The strip of columns itself is the `komorebi-scrolling` crate, which has no idea what a window
//! is, and this module is what is left of the glue between it and a window manager once everything
//! that touches an operating system is taken out: what the configuration and the layout options
//! come to for the strip, where the strip says that every container goes, and how much of a
//! resize becomes how much of a column.
//!
//! A host owns a [`ScrollingSpace`] per workspace, keyed by the id of each container, and asks
//! this module the questions that do not depend on the platform. How it carries out the answers
//! (animating, hiding, parking at an edge) is its own business.

use crate::LayoutOptions;
use crate::OperationDirection;
use crate::Rect;
use crate::Sizing;
use komorebi_scrolling::CenterFocusedColumn;
use komorebi_scrolling::ColumnRecord;
use komorebi_scrolling::ColumnWidth;
use komorebi_scrolling::Options;
use komorebi_scrolling::ScrollingSpace;
use komorebi_scrolling::Viewport;
use komorebi_scrolling::Visibility;
use serde::Deserialize;
use serde::Serialize;
use std::sync::LazyLock;
use std::sync::Mutex;

/// The model of a strip of columns, for hosts that depend on this crate and not on the model's.
pub use komorebi_scrolling as model;

/// How many columns fit in the viewport when nothing says otherwise.
pub const DEFAULT_COLUMNS: usize = 3;

/// The narrowest that a column can be made to be by resizing it.
pub const MINIMUM_COLUMN_WIDTH: i32 = 100;

/// How short a window in a column of several can be made.
pub const MINIMUM_WINDOW_HEIGHT: i32 = 100;

/// When the Scrolling layout centres the focused column
///
/// A boolean is also accepted when reading, for the layout option that used to be one: `true` is
/// `Always` and `false` is `Never`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
pub enum ScrollingCentering {
    /// Scroll just far enough to bring the focused column into view, and no further
    #[default]
    Never,
    /// Keep the focused column in the middle of the screen
    Always,
    /// Centre the focused column when it does not fit on the screen together with the column that
    /// focus came from
    OnOverflow,
}

impl<'de> Deserialize<'de> for ScrollingCentering {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        #[derive(Deserialize)]
        #[serde(untagged)]
        enum Repr {
            Flag(bool),
            Name(String),
        }

        match Repr::deserialize(deserializer)? {
            Repr::Flag(true) => Ok(Self::Always),
            Repr::Flag(false) => Ok(Self::Never),
            Repr::Name(name) => match name.as_str() {
                "Never" => Ok(Self::Never),
                "Always" => Ok(Self::Always),
                "OnOverflow" => Ok(Self::OnOverflow),
                _ => Err(serde::de::Error::unknown_variant(
                    &name,
                    &["Never", "Always", "OnOverflow"],
                )),
            },
        }
    }
}

impl From<ScrollingCentering> for CenterFocusedColumn {
    fn from(centering: ScrollingCentering) -> Self {
        match centering {
            ScrollingCentering::Never => Self::Never,
            ScrollingCentering::Always => Self::Always,
            ScrollingCentering::OnOverflow => Self::OnOverflow,
        }
    }
}

/// What the layout options of a workspace come to for the model.
///
/// `columns` says how many columns fit in the viewport, which is to say that a new column is a
/// `columns`th of its width. It does not say how many columns there are, and it does not stretch
/// a lone column to fill the viewport.
#[must_use]
pub fn scrolling_options(layout_options: Option<LayoutOptions>) -> Options {
    let scrolling = layout_options.and_then(|options| options.scrolling);
    let columns = scrolling
        .map_or(DEFAULT_COLUMNS, |scrolling| scrolling.columns)
        .max(1);
    let centering = scrolling
        .and_then(|scrolling| scrolling.center_focused_column)
        .unwrap_or_default();

    Options {
        default_width: ColumnWidth::Proportion(1.0 / columns as f64),
        center_focused_column: centering.into(),
        minimum_tile_height: MINIMUM_WINDOW_HEIGHT,
        ..Options::default()
    }
}

/// Where a container goes, and how much of it is in view.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ScrollingPlacement {
    /// Where the container is in the strip, in the coordinates of the screen. A container that is
    /// partly or wholly out of view has the rectangle that it really has, which reaches past the
    /// work area.
    pub rect: Rect,
    /// How much of the container is in view.
    pub visibility: Visibility,
}

/// Make a space match the containers and the focus as they are now, creating it if there is none,
/// and with the options and the viewport that it should have.
pub fn sync(
    space: &mut Option<ScrollingSpace<String>>,
    options: Options,
    viewport: Viewport,
    ids: &[String],
    focused: Option<&String>,
) {
    let space = space.get_or_insert_with(|| ScrollingSpace::new(options.clone(), viewport));

    if *space.options() != options {
        space.set_options(options);
    }

    if space.viewport() != viewport {
        space.set_viewport(viewport);
    }

    tell_of_arrivals(space, ids);
    space.reconcile(ids, focused);
}

/// Where every container goes in `work_area`, from left to right, with `container_padding`
/// taken off every side of each.
#[must_use]
pub fn place(
    space: &ScrollingSpace<String>,
    work_area: &Rect,
    container_padding: i32,
) -> Vec<ScrollingPlacement> {
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

/// The viewport that is the size of a work area.
#[must_use]
pub const fn viewport_of(work_area: &Rect) -> Viewport {
    Viewport {
        width: work_area.right,
        height: work_area.bottom,
    }
}

/// Make the focused column wider or narrower with the horizontal edges, and the focused window
/// taller or shorter in its column with the vertical ones. A window that is alone in its column
/// is as tall as the screen, so resizing it vertically does nothing.
///
/// The layouts that are made of splits move one edge of a container by half of `delta`, and
/// callers that want to move both, or to follow a mouse, rely on that, so a column changes
/// width, or a window height, by half of `delta` per edge too.
pub fn resize(
    space: &mut ScrollingSpace<String>,
    id: &String,
    direction: OperationDirection,
    sizing: Sizing,
    delta: i32,
) {
    let change = f64::from(delta) / 2.0;
    let change = match sizing {
        Sizing::Increase => change,
        Sizing::Decrease => -change,
    };

    if matches!(direction, OperationDirection::Up | OperationDirection::Down) {
        space.resize_tile_height(id, change);
        return;
    }

    let Some(width) = space.column_width(id) else {
        return;
    };

    // a column grows to the right, from its left edge, unless it is its left edge that is
    // moved: that one is to be where it is moved to, and its right edge where it was
    let left_edge = matches!(direction, OperationDirection::Left) && space.active_id() == Some(id);
    let right_edge = |space: &ScrollingSpace<String>| {
        space
            .layout()
            .into_iter()
            .find(|placement| &placement.id == id)
            .map(|placement| placement.rect.x + placement.rect.width)
    };
    let before = left_edge.then(|| right_edge(space)).flatten();

    let resized = ((width + change).round() as i32).max(MINIMUM_COLUMN_WIDTH);
    space.set_width(id, ColumnWidth::Fixed(resized));

    // a column that is made wider than the room it has is brought into view, which moves the
    // strip already: what is left to put back is where its right edge has got to, and not
    // what the column grew by
    if let Some(before) = before
        && let Some(after) = right_edge(space)
    {
        space.pan_by(f64::from(before - after));
    }
}

/// How many containers that were taken out of a workspace are waited for at another one.
const MAXIMUM_DEPARTED: usize = 32;

/// What the columns of the containers that were taken out of a workspace were given, until they
/// are put into another, whose strip has never heard of them.
///
/// A container goes from one workspace to another by being taken out of one and put into the
/// other, with nothing in between that both could be asked, so this is where it leaves what it
/// was given for the other to find.
static DEPARTED: LazyLock<Mutex<Vec<(String, ColumnRecord)>>> =
    LazyLock::new(|| Mutex::new(Vec::new()));

/// Say what a container that is leaving a workspace was given.
pub fn note_departure(id: &str, record: ColumnRecord) {
    let Ok(mut departed) = DEPARTED.lock() else {
        return;
    };

    departed.retain(|(container, _)| container != id);
    departed.push((id.to_string(), record));

    if departed.len() > MAXIMUM_DEPARTED {
        departed.remove(0);
    }
}

/// What a container that arrives was given where it was, if it came from a strip.
#[must_use]
pub fn take_departed(id: &str) -> Option<ColumnRecord> {
    let mut departed = DEPARTED.lock().ok()?;
    let at = departed.iter().position(|(container, _)| container == id)?;

    Some(departed.remove(at).1)
}

/// Tell a strip what the containers that it has not seen before were given where they came from,
/// before it is made to match the list of containers.
pub fn tell_of_arrivals(space: &mut ScrollingSpace<String>, ids: &[String]) {
    for id in ids {
        if space.position(id).is_none()
            && let Some(record) = take_departed(id)
        {
            space.arrive(id.clone(), record);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ScrollingLayoutOptions;

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

    #[test]
    fn a_column_is_a_share_of_the_viewport_as_wide_as_the_columns_option_says() {
        for columns in [1, 2, 3, 5] {
            assert_eq!(
                scrolling_options(options(columns, None)).default_width,
                ColumnWidth::Proportion(1.0 / columns as f64)
            );
        }

        assert_eq!(
            scrolling_options(None).default_width,
            ColumnWidth::Proportion(1.0 / DEFAULT_COLUMNS as f64)
        );
    }

    #[test]
    fn centring_is_what_the_layout_option_says_and_never_by_default() {
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
    fn centring_reads_the_booleans_that_it_used_to_be() {
        let read = |json: &str| serde_json::from_str::<ScrollingCentering>(json).unwrap();

        assert_eq!(read("true"), ScrollingCentering::Always);
        assert_eq!(read("false"), ScrollingCentering::Never);
        assert_eq!(read("\"Never\""), ScrollingCentering::Never);
        assert_eq!(read("\"Always\""), ScrollingCentering::Always);
        assert_eq!(read("\"OnOverflow\""), ScrollingCentering::OnOverflow);
        assert!(serde_json::from_str::<ScrollingCentering>("\"Sometimes\"").is_err());
        assert!(serde_json::from_str::<ScrollingCentering>("3").is_err());
    }

    #[test]
    fn centring_is_written_as_its_name() {
        assert_eq!(
            serde_json::to_string(&ScrollingCentering::OnOverflow).unwrap(),
            "\"OnOverflow\""
        );
    }

    #[test]
    fn containers_are_placed_inside_the_work_area_with_their_padding() {
        let work_area = Rect {
            left: 100,
            top: 50,
            right: 1200,
            bottom: 800,
        };
        let ids: Vec<String> = ["a", "b"].iter().map(ToString::to_string).collect();
        let mut space = None;

        sync(
            &mut space,
            scrolling_options(options(3, None)),
            viewport_of(&work_area),
            &ids,
            ids.first(),
        );

        let placements = place(space.as_ref().unwrap(), &work_area, 10);

        assert_eq!(placements.len(), 2);
        assert_eq!(placements[0].rect.left, 110);
        assert_eq!(placements[0].rect.top, 60);
        assert_eq!(placements[0].rect.right, 400 - 20);
        assert_eq!(placements[0].rect.bottom, 800 - 20);
    }

    #[test]
    fn resizing_changes_the_width_by_half_of_the_delta_and_never_below_the_minimum() {
        let work_area = Rect {
            left: 0,
            top: 0,
            right: 1200,
            bottom: 800,
        };
        let ids = vec!["a".to_string()];
        let mut space = None;

        sync(
            &mut space,
            scrolling_options(options(3, None)),
            viewport_of(&work_area),
            &ids,
            ids.first(),
        );

        let space = space.as_mut().unwrap();
        let id = ids[0].clone();

        resize(space, &id, OperationDirection::Right, Sizing::Increase, 100);
        assert_eq!(space.column_width(&id), Some(450.0));

        resize(
            space,
            &id,
            OperationDirection::Left,
            Sizing::Decrease,
            10_000,
        );
        assert_eq!(
            space.column_width(&id),
            Some(f64::from(MINIMUM_COLUMN_WIDTH))
        );
    }

    #[test]
    fn the_left_edge_of_a_column_that_is_resized_by_it_moves_and_the_right_edge_stays() {
        let work_area = Rect {
            left: 0,
            top: 0,
            right: 1200,
            bottom: 800,
        };
        let ids = vec!["a".to_string(), "b".to_string()];
        let mut space = None;

        sync(
            &mut space,
            scrolling_options(options(3, None)),
            viewport_of(&work_area),
            &ids,
            ids.get(1),
        );

        let space = space.as_mut().unwrap();
        let edges = |space: &ScrollingSpace<String>| {
            place(space, &work_area, 0)
                .iter()
                .map(|placement| {
                    (
                        placement.rect.left,
                        placement.rect.left + placement.rect.right,
                    )
                })
                .collect::<Vec<_>>()
        };

        let before = edges(space);

        // by half of the delta, which is 100 here, out
        resize(
            space,
            &ids[1],
            OperationDirection::Left,
            Sizing::Increase,
            200,
        );
        let grown = edges(space);
        assert_eq!(grown[1].0, before[1].0 - 100);
        assert_eq!(grown[1].1, before[1].1);

        // and in, which gives it back
        resize(
            space,
            &ids[1],
            OperationDirection::Left,
            Sizing::Decrease,
            200,
        );
        assert_eq!(edges(space), before);

        // the right edge of the last column is the one that grows when it is the right edge
        resize(
            space,
            &ids[1],
            OperationDirection::Right,
            Sizing::Increase,
            200,
        );
        let grown = edges(space);
        assert_eq!(grown[1].0, before[1].0);
        assert_eq!(grown[1].1, before[1].1 + 100);
    }

    #[test]
    fn the_left_edge_of_a_column_is_where_it_is_moved_to_however_far_it_is_moved() {
        let work_area = Rect {
            left: 0,
            top: 0,
            right: 1200,
            bottom: 800,
        };
        let ids = vec!["a".to_string(), "b".to_string()];
        let mut space = None;

        sync(
            &mut space,
            scrolling_options(options(3, None)),
            viewport_of(&work_area),
            &ids,
            ids.get(1),
        );

        let space = space.as_mut().unwrap();
        let edges = |space: &ScrollingSpace<String>| {
            place(space, &work_area, 0)
                .iter()
                .map(|placement| {
                    (
                        placement.rect.left,
                        placement.rect.left + placement.rect.right,
                    )
                })
                .collect::<Vec<_>>()
        };

        // the last column ends at 800 of a viewport of 1200: there is room for 400 more, and
        // past it the column is brought into view, which moves the strip as well
        let before = edges(space);
        assert_eq!(before[1], (400, 800));

        for growth in [300, 400, 500, 600, 900] {
            let mut grown = space.clone();
            resize(
                &mut grown,
                &ids[1],
                OperationDirection::Left,
                Sizing::Increase,
                growth * 2,
            );

            let after = edges(&grown);
            assert_eq!(
                after[1],
                (before[1].0 - growth, before[1].1),
                "grown by {growth}"
            );

            // and the column before it follows its left edge
            assert_eq!(after[0].1, after[1].0, "grown by {growth}");
        }
    }
}
