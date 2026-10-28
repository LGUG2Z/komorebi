//! A model of a strip of columns that a viewport scrolls across, with no idea what a window is.
//!
//! The model is for a window manager in which windows sit in a row that is as long as it needs to
//! be, and the screen shows a part of it. It owns the questions that make that feel right: how
//! wide each column is, which part of the strip is in view, and how the view moves when a column is
//! opened, closed, resized or focused. It does not own anything that touches the operating system,
//! so the same model can sit under a window manager for any of them.
//!
//! # What the host does
//!
//! The host gives the model a [`Viewport`] and a key for each column, and tells it what happens:
//! a window opened, a window closed, focus moved. After each, the host asks for the
//! [`layout`](ScrollingSpace::layout) and carries it out. How it carries it out is the host's
//! business, and so are the things that differ between platforms: whether windows that are out of
//! view are hidden or parked at an edge, how a window that refuses to be a width is dealt with
//! (see [`set_width_limits`](ScrollingSpace::set_width_limits)), and animation. The model says
//! where everything goes. It never says how to get there.
//!
//! A host that already keeps its own ordered list of windows can hand that list over after every
//! change with [`reconcile`](ScrollingSpace::reconcile), instead of reporting each change.
//!
//! # Columns of tiles
//!
//! A column holds one tile, or several one above the other, which share the height of the
//! viewport. The key that the host gives the model is the key of a tile. Everything that takes a
//! key takes the key of any tile in a column, and means the column that it is in. A column has the
//! width, the limits and the preset that it had whatever tiles come and go.
//!
//! The tiles are kept in **reading order**: left to right by column, and top to bottom within one.
//! That is the order of [`ids`](ScrollingSpace::ids), of [`layout`](ScrollingSpace::layout), and of
//! the list that a host hands to [`reconcile`](ScrollingSpace::reconcile). A column is a run of that
//! list, so the host's list is always a valid strip, and the model never disagrees with it about
//! the order. Of the things that change a column's tiles, [`consume_right`], [`consume_left`] and
//! tiles that are added or removed leave reading order alone or change it as the list would. The
//! ones that move a tile past another ([`expel`], [`move_tile_up`], [`move_tile_down`] and
//! [`move_column`]) change it, and a host that keeps its own list reads the new order from
//! [`ids`](ScrollingSpace::ids) and makes its list match.
//!
//! [`consume_right`]: ScrollingSpace::consume_right
//! [`consume_left`]: ScrollingSpace::consume_left
//! [`expel`]: ScrollingSpace::expel
//! [`move_tile_up`]: ScrollingSpace::move_tile_up
//! [`move_tile_down`]: ScrollingSpace::move_tile_down
//! [`move_column`]: ScrollingSpace::move_column
//!
//! A column remembers which of its tiles had focus last, and that is the one that has it again
//! when focus returns to the column.
//!
//! # Moving the strip by hand
//!
//! A pan is the user dragging the strip across the viewport with a finger or a wheel, and it is
//! the one thing that moves the viewport without moving focus. [`begin_pan`], [`pan_by`] and
//! [`pan_end`] bracket it: nothing but the position of the strip changes between the first two,
//! so that the host can lay out and draw every step, and letting go comes to rest with a column
//! aligned to an edge of the viewport, nearest to where the strip would carry on to at the speed
//! that it was going. A [`SwipeTracker`] says how fast that was from the movements that the host
//! saw, and has no clock of its own, so the same movements always give the same answer.
//!
//! [`begin_pan`]: ScrollingSpace::begin_pan
//! [`pan_by`]: ScrollingSpace::pan_by
//! [`pan_end`]: ScrollingSpace::pan_end
//!
//! # Coordinates
//!
//! Everything is measured from the top left corner of the [`Viewport`], in whole pixels in the
//! results and in floating point inside, so that proportions do not accumulate rounding errors.
//! A column that is out of view has coordinates outside of `0..width`, which are the true ones.
//!
//! # The rules that make it feel like a strip
//!
//! These are the properties that the rest of the crate is built to keep, and each is tested.
//!
//! 1. **A column's width is its own.** Opening a column, closing one or focusing one never changes
//!    another's width. A strip with more columns than fit is simply wider than the viewport.
//! 2. **The focused column does not move by itself.** Opening, closing or resizing a column
//!    to its left does not move it on screen. Only a change to the focused column itself, or a
//!    change of focus, can scroll the viewport.
//! 3. **Scrolling is as little as it can be.** A column that is already in view when it gets focus
//!    does not scroll the strip, and one that is not is brought in until it is just in view.
//! 4. **Temporary changes are undone.** Closing a column that was just opened next to the previous
//!    one returns focus to it, and the viewport to where it was. Leaving fullscreen puts the
//!    viewport back too.
//! 5. **A column wider than the viewport starts at its left edge.**
//! 6. **Proportions include the gaps.** `n` columns of `1 / n` fill the viewport exactly, however
//!    big the gaps are.
//! 7. **Fullscreen is a column like any other**, which the others can be scrolled to either side of.
//!
//! # Example
//!
//! ```
//! use komorebi_scrolling::{ColumnWidth, Options, ScrollingSpace, Viewport, Visibility};
//!
//! let options = Options {
//!     default_width: ColumnWidth::Proportion(0.5),
//!     ..Options::default()
//! };
//! let mut space = ScrollingSpace::new(options, Viewport { width: 1200, height: 800 });
//!
//! // Open four windows, each next to the one before, and in focus.
//! for id in 0..4_u32 {
//!     space.add_column(id, None, None, true);
//! }
//!
//! // Opening windows did not change how wide any of them is. The strip has scrolled just far
//! // enough to keep the newest in view, which leaves the first two out of it.
//! let layout = space.layout();
//! assert!(layout.iter().all(|placement| placement.rect.width == 600));
//! assert_eq!(layout[3].rect.x, 600);
//! assert_eq!(layout[0].visibility, Visibility::OffLeft);
//! assert_eq!(layout[1].visibility, Visibility::OffLeft);
//!
//! // Moving focus to a window that is already in view does not scroll.
//! space.focus_left();
//! assert_eq!(space.layout()[3].rect.x, 600);
//!
//! // Moving it to one that is not scrolls just far enough to bring it in.
//! space.focus_index(0);
//! assert_eq!(space.layout()[0].rect.x, 0);
//! ```
//!
//! # Prior art
//!
//! The behaviour follows what the [niri] documentation and design notes say about its scrollable
//! tiling, and [Paneru] showed that the approach works on macOS. The code here was written from
//! those descriptions and shares none with either.
//!
//! [niri]: https://github.com/YaLTeR/niri
//! [Paneru]: https://github.com/karinushka/paneru

#![forbid(unsafe_code)]
#![warn(missing_docs)]

mod geometry;
mod options;
mod space;
mod swipe;
mod view;

pub use geometry::Rect;
pub use geometry::Viewport;
pub use options::CenterFocusedColumn;
pub use options::ColumnWidth;
pub use options::Options;
pub use options::TileHeight;
pub use space::ColumnRecord;
pub use space::Placement;
pub use space::ScrollingSpace;
pub use space::Snap;
pub use space::Visibility;
pub use swipe::SwipeTracker;
