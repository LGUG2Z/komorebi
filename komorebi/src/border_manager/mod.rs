//! The borders around the windows, which the compositor draws.
//!
//! komorebi draws no border itself. KWin's KDE Rounded Corners effect draws an outline outside
//! each window, in the gap komorebi leaves between tiles, and this module decides what it looks
//! like:
//!
//! - [`send_notification`] asks the backend for the [`Look`] of the settings in force
//!   ([`current_look`]): the border's width, radius and two colors, one for the focused window and
//!   one for the others. The backend answers whether the borders are drawn, which is kept in
//!   [`BORDER_DRAWN`]: a drawn border takes room around each window, which the layout leaves.
//!
//! The settings are the statics of this module, which the `komorebic border*` commands set and
//! the `border*` keys of `komorebi.json` ([`crate::static_config::StaticConfig`]) give their
//! first values.

use crate::backend::BorderLook;
use crate::backend::Look;
use crate::backend::backend;
use color_eyre::eyre;
use lazy_static::lazy_static;
use std::sync::atomic::AtomicBool;
use std::sync::atomic::AtomicI32;
use std::sync::atomic::AtomicU32;
use std::sync::atomic::Ordering;

/// The thickness of the border, in pixels.
///
/// It is also the room the layout leaves for the border on each side of a window while the borders
/// are drawn ([`BORDER_DRAWN`]). The effect draws at most 20 pixels; the layout leaves the whole
/// width even when it is more.
pub static BORDER_WIDTH: AtomicI32 = AtomicI32::new(10);
/// More room the layout leaves on each side of a window while the borders are drawn, on top of
/// [`BORDER_WIDTH`]. The effect draws nothing in it.
pub static BORDER_OFFSET: AtomicI32 = AtomicI32::new(0);
/// The radius of the window's corners, in pixels.
pub static BORDER_RADIUS: AtomicI32 = AtomicI32::new(10);
/// Whether borders are asked for at all.
pub static BORDER_ENABLED: AtomicBool = AtomicBool::new(true);

lazy_static! {
    /// The color of the focused window's border, as [`pack`] stores it.
    pub static ref FOCUSED: AtomicU32 = AtomicU32::new(pack(66, 165, 245));
    /// The color of the border of every other window, as [`pack`] stores it.
    pub static ref UNFOCUSED: AtomicU32 = AtomicU32::new(pack(128, 128, 128));
}

/// Returns a color as the statics store it: red in the low byte, then green, then blue.
///
/// ```rust
/// use komorebi::border_manager::pack;
/// use komorebi::border_manager::unpack;
///
/// assert_eq!(pack(1, 2, 3), 0x030201);
/// assert_eq!(unpack(pack(66, 165, 245)), (66, 165, 245));
/// ```
pub const fn pack(r: u8, g: u8, b: u8) -> u32 {
    (r as u32) | ((g as u32) << 8) | ((b as u32) << 16)
}

/// Returns the red, green and blue of a color as the statics store it. The bits above the third
/// byte are dropped.
pub const fn unpack(colour: u32) -> (u8, u8, u8) {
    (
        (colour & 0xff) as u8,
        ((colour >> 8) & 0xff) as u8,
        ((colour >> 16) & 0xff) as u8,
    )
}

/// Whether the compositor is drawing the borders, as the backend last answered
/// [`send_notification`].
///
/// When it is, they take room around each window ([`BORDER_WIDTH`] and [`BORDER_OFFSET`]), which
/// the layout leaves; when it is not (no effect installed, borders off) they take none.
pub static BORDER_DRAWN: AtomicBool = AtomicBool::new(false);

/// Returns the [`Look`] of these settings.
///
/// The border is there when `enabled`, with its width and radius at least 0; the colors are as the
/// statics store them, red, green and blue in one number.
///
/// ```rust
/// use komorebi::border_manager::look;
/// use komorebi::border_manager::pack;
///
/// let blue = pack(66, 165, 245);
/// let grey = pack(128, 128, 128);
///
/// let on = look(true, 8, -2, blue, grey);
/// let border = on.border.expect("the borders are on");
/// assert_eq!((border.width, border.radius), (8, 0));
/// assert_eq!(border.focused, (66, 165, 245));
/// assert_eq!(border.unfocused, (128, 128, 128));
///
/// assert_eq!(look(false, 8, 10, blue, grey).border, None);
/// ```
pub fn look(enabled: bool, width: i32, radius: i32, focused: u32, unfocused: u32) -> Look {
    Look {
        border: enabled.then(|| BorderLook {
            width: width.max(0),
            radius: radius.max(0),
            focused: unpack(focused),
            unfocused: unpack(unfocused),
        }),
    }
}

/// Returns the [`Look`] of the settings in force: the border statics of this module.
pub fn current_look() -> Look {
    look(
        BORDER_ENABLED.load(Ordering::SeqCst),
        BORDER_WIDTH.load(Ordering::SeqCst),
        BORDER_RADIUS.load(Ordering::SeqCst),
        FOCUSED.load(Ordering::SeqCst),
        UNFOCUSED.load(Ordering::SeqCst),
    )
}

/// Asks the backend for the [`Look`] of the settings in force, and keeps its answer, whether the
/// borders are drawn, in [`BORDER_DRAWN`].
///
/// Called whenever the window manager's state changes in a way that could change the look, and
/// whenever a setting does. The backend does nothing when the look is the one it applied last, and
/// a failure is logged.
pub fn send_notification() {
    match backend().apply_look(&current_look()) {
        Ok(drawn) => BORDER_DRAWN.store(drawn, Ordering::SeqCst),
        Err(error) => tracing::warn!("could not set how windows look: {error}"),
    }
}

/// Asks the backend for the look again, as [`send_notification`] does. Called when a border
/// setting changed.
///
/// There are no border windows to destroy: the compositor draws the borders. The name is the one
/// its callers share with komorebi on the other platforms.
///
/// # Errors
///
/// Never returns an error: a failure is logged.
pub fn destroy_all_borders() -> eyre::Result<()> {
    send_notification();
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_stored_colours_become_red_green_and_blue() {
        let blue = pack(66, 165, 245);
        let grey = pack(128, 128, 128);

        let look = look(true, 10, 12, blue, grey);
        let border = look.border.unwrap();

        assert_eq!(border.focused, (66, 165, 245));
        assert_eq!(border.unfocused, (128, 128, 128));
        assert_eq!((border.width, border.radius), (10, 12));
    }

    #[test]
    fn a_negative_width_or_radius_is_none() {
        let look = look(true, -4, -1, 0, 0);
        let border = look.border.unwrap();
        assert_eq!((border.width, border.radius), (0, 0));
    }
}
